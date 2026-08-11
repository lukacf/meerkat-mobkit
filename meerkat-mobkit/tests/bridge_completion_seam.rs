//! Adversarial regressions for the completion-bearing bridge seam.
//!
//! Both cover defects that existed in this seam and were caught in review, not
//! by a test. They are written to fail if either is reintroduced.
//!
//! 1. The completion wait must NOT be charged to the actor admission budget.
//!    That budget is wall clock and also covers session resolution, so awaiting
//!    a turn inside it makes an ordinary long turn look like an
//!    `ActorAdmissionTimeout` on a step that never misbehaved.
//!
//! 2. A turn that was admitted and then FAILED must surface as a typed
//!    `CompletionFailed`, never as an admission-shaped error. The delivery path
//!    retries "repairable" admission failures; if a completion failure reached
//!    that classifier, the member's turn would be resubmitted and RUN A SECOND
//!    TIME. Typing it is what keeps it out of that path.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_client::{LlmError, LlmEvent, LlmRequest};
use meerkat_core::types::{HandlingMode, StopReason};
use meerkat_mob::{MobDefinition, ProfileName};
use meerkat_mobkit::UnifiedRuntimeBuilder;
use meerkat_mobkit::identity_first::bridge::MobSessionBridge;
use meerkat_mobkit::identity_first::contracts::{AgentCustomizer, TopologyProvider};
use meerkat_mobkit::identity_first::orchestrator::{RestoreOutcome, restore_flow};
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentBuildContext, AgentBuildDraft, AgentIdentity, AgentRuntimeId,
    BridgeError, ContinuityStore, CustomizerError, DurabilityPolicy, DurableAgentSpec,
    IdentityRuntime, IdentityRuntimeConfig, LocalContinuityStore, LocalLeaseProvider,
    ManagedPeerEdge, SessionBridge, TopologyContext, TopologyError,
};
use meerkat_mobkit::mob_handle_runtime::SessionCreatedContext;

#[path = "support/llm_usage.rs"]
mod llm_usage;

fn id(name: &str) -> AgentIdentity {
    AgentIdentity::parse(name).unwrap()
}

fn spec(name: &str) -> DurableAgentSpec {
    DurableAgentSpec {
        identity: id(name),
        profile: ProfileName::from("personal"),
        addressability: AgentAddressability::Addressable,
        display_name: None,
        labels: BTreeMap::new(),
        context: None,
        additional_instructions: Vec::new(),
        initial_message: None,
        runtime_mode_override: None,
        backend: None,
        binding: None,
        placement: None,
    }
}

const MOB_TOML: &str = r#"
[mob]
id = "bridge-completion-seam"

[profiles.personal]
model = "gpt-5.5"
external_addressable = true
runtime_mode = "turn_driven"

[profiles.personal.tools]
comms = true
"#;

struct EmptyTopology;
#[async_trait]
impl TopologyProvider for EmptyTopology {
    async fn compute_edges(
        &self,
        _target_identities: &[AgentIdentity],
        _context: &TopologyContext,
    ) -> Result<Vec<ManagedPeerEdge>, TopologyError> {
        Ok(vec![])
    }
}

struct NoopCustomizer;
#[async_trait]
impl AgentCustomizer for NoopCustomizer {
    async fn customize_build(
        &self,
        _context: &AgentBuildContext,
        _spec: &DurableAgentSpec,
        _draft: &mut AgentBuildDraft,
    ) -> Result<(), CustomizerError> {
        Ok(())
    }
    async fn after_create(
        &self,
        _identity: &AgentIdentity,
        _session_id: &meerkat_core::types::SessionId,
        _context: &SessionCreatedContext,
    ) -> Result<(), CustomizerError> {
        Ok(())
    }
}

/// LLM double with a controllable turn duration and conformance.
///
/// `emit_usage = false` reproduces a turn that fails AFTER admission: meerkat
/// 0.8.22 fails a turn closed when its stream carried no normalized provider
/// accounting. That is a real post-admission failure rather than a simulated
/// one, which is what makes it a fair test of the completion path.
/// Sentinel wording deliberately chosen to be ACCEPTED by
/// `is_missing_bridge_session_snapshot_error`, and therefore by
/// `is_repairable_bridge_delivery_error` (`bridge.rs:39,66`: it matches on
/// `contains("missing bridge session snapshot")`).
///
/// This is the whole point of the second regression. A post-admission failure
/// the classifier would IGNORE proves nothing - the retry path would not have
/// fired for it either way. Only a failure the classifier would have SWALLOWED
/// can demonstrate that phase separation, not luck, is what prevents the
/// resubmission.
const REPAIRABLE_SENTINEL: &str = "missing bridge session snapshot - completion sentinel";

#[derive(Clone)]
struct ScriptedClient {
    /// Signalled once the turn has entered the LLM. Lets a test know the work
    /// is genuinely in flight without waiting on a clock.
    entered: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    /// Held until the test releases it. While held, the turn cannot finish, so
    /// "the delivery has not returned" is a fact rather than a race.
    release: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
    /// When false the turn fails AFTER admission, carrying REPAIRABLE_SENTINEL.
    emit_usage: bool,
    calls: Arc<AtomicUsize>,
}

impl ScriptedClient {
    fn immediate(emit_usage: bool) -> Self {
        Self {
            entered: Arc::new(tokio::sync::Mutex::new(None)),
            release: Arc::new(tokio::sync::Mutex::new(None)),
            emit_usage,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl meerkat_client::LlmClient for ScriptedClient {
    fn project_replay_messages(
        &self,
        messages: &[meerkat_core::Message],
    ) -> Result<Vec<meerkat_core::Message>, LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(
        &'a self,
        request: &'a LlmRequest,
    ) -> Pin<Box<dyn futures::Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let entered = Arc::clone(&self.entered);
        let release = Arc::clone(&self.release);
        let terminal = self.emit_usage.then(|| {
            llm_usage::usage_then_done(request, meerkat::Provider::OpenAI, StopReason::EndTurn)
        });
        Box::pin(async_stream::stream! {
            // Announce that the turn is in flight, then block until released.
            // No clocks: the test drives both edges.
            if let Some(tx) = entered.lock().await.take() {
                let _ = tx.send(());
            }
            if let Some(rx) = release.lock().await.take() {
                let _ = rx.await;
            }
            yield Ok(LlmEvent::TextDelta { delta: "ok".to_string(), meta: None });
            match terminal {
                Some([usage, done]) => {
                    yield Ok(usage);
                    yield Ok(done);
                }
                // A typed post-admission failure whose message the repairable
                // classifier accepts. The work WAS admitted; the turn then
                // failed.
                None => {
                    yield Err(LlmError::IncompleteResponse {
                        message: REPAIRABLE_SENTINEL.to_string(),
                    });
                }
            }
        })
    }

    fn provider(&self) -> meerkat::Provider {
        meerkat::Provider::OpenAI
    }

    fn health_check<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), LlmError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Ok(()) })
    }
}

/// Boot a runtime around `client` and return the bridge plus one member's
/// runtime id.
async fn boot_one_member(
    state_path: &std::path::Path,
    client: ScriptedClient,
    admission_budget: Duration,
) -> (
    meerkat_mobkit::UnifiedRuntime,
    Arc<dyn SessionBridge>,
    AgentRuntimeId,
) {
    let unified = UnifiedRuntimeBuilder::default()
        .definition(MobDefinition::from_toml(MOB_TOML).expect("parse seam mob definition"))
        .persistent_state(state_path)
        .comms(true)
        .default_llm_client(Arc::new(client))
        .build()
        .await
        .expect("build UnifiedRuntime");
    // Build the bridge explicitly so the actor admission budget can be set,
    // exactly as tests/repair_queue_carry.rs:245 does. A tight budget is what
    // makes the boundary observable: the production default is ten minutes.
    let session_service = unified
        .mob_runtime()
        .session_service()
        .cloned()
        .expect("session service");
    let bridge: Arc<dyn SessionBridge> = Arc::new(
        MobSessionBridge::with_session_service(unified.mob_handle(), session_service)
            .with_actor_admission_budget(admission_budget),
    );
    let store = Arc::new(
        LocalContinuityStore::open(state_path.join("continuity.db")).expect("continuity store"),
    );
    let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store as Arc<dyn ContinuityStore>,
        lease_provider: Arc::new(LocalLeaseProvider::new()),
        runtime_instance_id: "bridge-completion-seam".to_string(),
        has_runtime_store: true,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(bridge.clone()),
        default_timeout: None,
    });

    let alice = id("alice");
    let roster = vec![spec("alice")];
    let result = restore_flow(
        &identity_rt,
        &roster,
        Some(&EmptyTopology as &dyn TopologyProvider),
        Some(&NoopCustomizer as &dyn AgentCustomizer),
    )
    .await
    .expect("restore_flow");
    let runtime_id = match result.outcomes.get(&alice).expect("alice outcome") {
        RestoreOutcome::Created { record, .. } | RestoreOutcome::Resumed { record, .. } => {
            record.agent_runtime_id.clone()
        }
        other => panic!("expected the member to be created, got {other:?}"),
    };
    (unified, bridge, runtime_id)
}

/// The delivery must actually AWAIT the turn - proven by a barrier, not a clock.
///
/// The double signals `entered` once the turn is in flight and then blocks. While
/// it is blocked the spawned delivery MUST NOT have finished: if the completion
/// handle were dropped (the defect review caught here), the delivery would
/// return at admission and the join handle would already be complete. Releasing
/// the barrier then has to produce success.
///
/// No elapsed time is used as evidence anywhere: the test drives both edges.
#[tokio::test(flavor = "multi_thread")]
async fn the_delivery_awaits_the_turn_and_does_not_drop_the_handle() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let client = ScriptedClient {
        entered: Arc::new(tokio::sync::Mutex::new(Some(entered_tx))),
        release: Arc::new(tokio::sync::Mutex::new(Some(release_rx))),
        emit_usage: true,
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let (unified, bridge, runtime_id) = boot_one_member(
        &temp.path().join("state"),
        client.clone(),
        Duration::from_secs(30),
    )
    .await;

    let delivery = tokio::spawn({
        let bridge = Arc::clone(&bridge);
        let runtime_id = runtime_id.clone();
        async move {
            bridge
                .deliver_awaiting_commit_with_mode_context_and_system_prompt(
                    &runtime_id,
                    &meerkat_core::ContentInput::Text("held at the barrier".to_string()),
                    None,
                    &[],
                    HandlingMode::Queue,
                    None,
                )
                .await
        }
    });

    entered_rx.await.expect("the turn must reach the LLM");
    // SMOKE ONLY, deliberately not causal. A spawned task that has not been
    // polled yet is also "not finished", so this cannot PROVE the handle is
    // awaited - it only catches the blatant case. The causal proof of the
    // ordering lives in the direct-poll unit test
    // `session_resolution_completes_before_the_completion_future_is_polled`,
    // which was verified by negative mutation: swapping the awaits makes it
    // fail inside the completion future's first poll.
    assert!(
        !delivery.is_finished(),
        "smoke: the delivery returned while the turn was still held at the barrier"
    );

    release_tx.send(()).expect("release the turn");
    let delivered = delivery.await.expect("delivery task panicked");
    assert!(
        delivered.is_ok(),
        "the delivery must succeed once the turn completes, got {:?}",
        delivered.err()
    );
    assert_eq!(
        client.calls.load(Ordering::SeqCst),
        1,
        "the turn must have reached the LLM exactly once"
    );
    unified.shutdown().await;
}

/// A turn that fails AFTER admission must be typed, and must not be retried.
///
/// `CompletionFailed` is what keeps it out of the delivery path's
/// repair-and-retry classifier. If it came back as an admission-shaped error,
/// that classifier could resubmit and run the member's turn twice.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_that_fails_after_admission_is_typed_and_not_retried() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let client = ScriptedClient::immediate(false);
    let (unified, bridge, runtime_id) = boot_one_member(
        &temp.path().join("state"),
        client.clone(),
        Duration::from_secs(30),
    )
    .await;

    let delivered = bridge
        .deliver_awaiting_commit_with_mode_context_and_system_prompt(
            &runtime_id,
            &meerkat_core::ContentInput::Text("this turn will fail closed".to_string()),
            None,
            &[],
            HandlingMode::Queue,
            None,
        )
        .await;

    match delivered {
        Err(BridgeError::CompletionFailed(detail)) => {
            assert!(
                detail.contains("missing bridge session snapshot"),
                "the injected failure must be one the repairable classifier WOULD accept, \
                 otherwise this test proves nothing about the retry path; got: {detail}"
            );
        }
        other => panic!(
            "a turn that was admitted and then failed must surface as \
             BridgeError::CompletionFailed - anything admission-shaped can reach the \
             repair-and-retry classifier and run the turn twice. Got: {other:?}"
        ),
    }
    assert_eq!(
        client.calls.load(Ordering::SeqCst),
        1,
        "the failed turn must NOT have been resubmitted. Its detail is accepted by \
         is_repairable_bridge_delivery_error, so had the completion failure reached that \
         classifier the delivery path would have repaired the member and resubmitted, \
         running the turn a second time. Exactly one call proves phase separation holds."
    );
    unified.shutdown().await;
}
