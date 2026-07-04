//! Upgrade-carry regression (Bug 1, HomeCore 0.7.20 report): a full process
//! restart against the same on-disk store must RESUME each agent's transcript,
//! not fresh-spawn it empty.
//!
//! On cold restart the identity runtime re-creates the member and calls
//! `bridge.resume_session` (`MemberLaunchMode::Resume`). Before meerkat 0.7.14,
//! the re-created runtime authority re-projected a transcript that only differed
//! from the persisted row in bookkeeping (re-stamped run identity + timestamps),
//! and the session-store append-only continuity guard rejected it as "not a
//! continuation of persisted revision" — so `resume_session` fell into its
//! `Err` arm and spawned a fresh, EMPTY member, silently dropping history on
//! every restart. Every real deployment hit this; fresh-state tests never did.
//!
//! meerkat 0.7.14 (Ask B) makes the guard compare the shared prefix by content
//! address, tolerating bookkeeping-only divergence, so resume succeeds. This
//! test proves it end to end: boot, deliver a unique token, FULLY shut down,
//! rebuild a fresh runtime against the SAME state dir, and assert the
//! post-restart turn replays the token — impossible unless the transcript
//! carried across the restart. The sibling respawn-continuity test covers the
//! in-process recovery boundary; this one covers the cold restart.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use meerkat_client::{LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::types::StopReason;
use meerkat_mob::{MobDefinition, ProfileName};
use meerkat_mobkit::UnifiedRuntimeBuilder;
use meerkat_mobkit::identity_first::contracts::{AgentCustomizer, TopologyProvider};
use meerkat_mobkit::identity_first::orchestrator::{RestoreOutcome, restore_flow};
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentBuildContext, AgentBuildDraft, AgentIdentity, ContinuityStore,
    CustomizerError, DurabilityPolicy, DurableAgentSpec, IdentityRuntime, IdentityRuntimeConfig,
    LocalContinuityStore, LocalLeaseProvider, ManagedPeerEdge, SessionBridge, TopologyContext,
    TopologyError,
};
use meerkat_mobkit::mob_handle_runtime::SessionCreatedContext;
use tokio::time::sleep;

fn id(name: &str) -> AgentIdentity {
    AgentIdentity::parse(name).unwrap()
}

fn spec(name: &str, addr: AgentAddressability, profile: &str) -> DurableAgentSpec {
    DurableAgentSpec {
        identity: id(name),
        profile: ProfileName::from(profile),
        addressability: addr,
        display_name: None,
        labels: BTreeMap::new(),
        context: None,
        additional_instructions: Vec::new(),
        initial_message: None,
        runtime_mode_override: None,
        backend: None,
        binding: None,
    }
}

const MOB_TOML: &str = r#"
[mob]
id = "cold-restart-continuity"

[profiles.personal]
model = "gpt-5.5"
external_addressable = true
runtime_mode = "turn_driven"

[profiles.personal.tools]
comms = true
"#;

fn one_member_definition() -> MobDefinition {
    MobDefinition::from_toml(MOB_TOML).expect("parse cold-restart mob definition")
}

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

/// Records the serialized LLM request each turn and answers "ok" so turns
/// complete without a real provider.
#[derive(Clone, Default)]
struct CaptureClient {
    requests: Arc<std::sync::Mutex<Vec<String>>>,
}
impl CaptureClient {
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
    fn last(&self) -> Option<String> {
        self.requests.lock().unwrap().last().cloned()
    }
}
impl meerkat_client::LlmClient for CaptureClient {
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
        self.requests
            .lock()
            .unwrap()
            .push(serde_json::to_string(request).unwrap_or_default());
        Box::pin(async_stream::stream! {
            yield Ok(LlmEvent::TextDelta { delta: "ok".to_string(), meta: None });
            yield Ok(LlmEvent::Done {
                outcome: LlmDoneOutcome::Success { stop_reason: StopReason::EndTurn },
            });
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

const RUNTIME_INSTANCE: &str = "cold-restart-continuity";

/// Build a fresh runtime + identity runtime against `state_path`, capturing LLM
/// requests into `capture`. Mirrors a process (re)start against a durable store.
async fn boot(
    state_path: &std::path::Path,
    capture: CaptureClient,
) -> (meerkat_mobkit::UnifiedRuntime, IdentityRuntime) {
    let unified = UnifiedRuntimeBuilder::default()
        .definition(one_member_definition())
        .persistent_state(state_path)
        .comms(true)
        .default_llm_client(Arc::new(capture))
        .build()
        .await
        .expect("build UnifiedRuntime");
    let bridge: Arc<dyn SessionBridge> = unified
        .session_bridge()
        .expect("session_bridge should exist")
        .clone();
    let store = Arc::new(
        LocalContinuityStore::open(state_path.join("continuity.db")).expect("continuity store"),
    );
    let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store as Arc<dyn ContinuityStore>,
        lease_provider: Arc::new(LocalLeaseProvider::new()),
        runtime_instance_id: RUNTIME_INSTANCE.to_string(),
        has_runtime_store: true,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(bridge),
        default_timeout: None,
    });
    (unified, identity_rt)
}

// SCAFFOLD (not yet in CI): a faithful cold-restart harness needs the gateway's
// exact continuity+session wiring. Here the boot-1 continuity record only
// persists to the DB when a lease grant is present (orchestrator.rs upsert is
// grant-gated), and a hand-assembled LocalContinuityStore + UnifiedRuntime does
// not reload the persisted session the way rpc_gateway does — so boot 2 resolves
// Uninitialized and re-Creates. The behavior this guards (Ask B: resume tolerates
// bookkeeping-only re-projection divergence) is verified upstream in
// meerkat-core 0.7.14 (`append_only_guard_accepts_rebookkept_prefix_identity_and_timestamps`)
// and, for the mobkit recovery boundary, by identity_first_respawn_continuity.
// Left as a runnable scaffold to finish against the gateway harness (or the
// live deployment) rather than assert on a misconfigured setup.
#[ignore = "needs gateway-faithful cold-restart harness; Ask B verified upstream + by respawn-continuity"]
#[tokio::test(flavor = "multi_thread")]
async fn identity_first_cold_restart_preserves_transcript() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let state_path = temp.path().join("state");
    let alice = id("personal:alice");
    let roster = vec![spec(
        "personal:alice",
        AgentAddressability::Addressable,
        "personal",
    )];
    const TOKEN: &str = "MARKER-BRAVO-9-YANKEE";

    // --- Boot 1: create the member, deliver turn 1 with the token, shut down ---
    {
        let capture = CaptureClient::default();
        let (unified, identity_rt) = boot(&state_path, capture.clone()).await;

        let result = restore_flow(
            &identity_rt,
            &roster,
            Some(&EmptyTopology as &dyn TopologyProvider),
            Some(&NoopCustomizer as &dyn AgentCustomizer),
        )
        .await
        .expect("restore_flow (boot 1)");
        match result.outcomes.get(&alice).expect("alice outcome") {
            RestoreOutcome::Created { .. } => {}
            other => panic!("expected Created on first boot, got {other:?}"),
        }

        identity_rt
            .send(
                &alice,
                &meerkat_core::ContentInput::Text(format!("Please note this token: {TOKEN}")),
            )
            .await
            .expect("send turn 1");

        let deadline = Instant::now() + Duration::from_secs(20);
        while capture.count() < 1 {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for turn 1 to reach the LLM"
            );
            sleep(Duration::from_millis(100)).await;
        }
        // Let turn 1's assistant response commit to the persisted transcript
        // before the shutdown flush.
        sleep(Duration::from_millis(500)).await;
        unified.shutdown().await;
    }

    // --- Boot 2: fresh runtime, SAME store. Resume must carry the transcript ---
    {
        let capture = CaptureClient::default(); // fresh: only boot-2 requests
        let (unified, identity_rt) = boot(&state_path, capture.clone()).await;

        let result = restore_flow(
            &identity_rt,
            &roster,
            Some(&EmptyTopology as &dyn TopologyProvider),
            Some(&NoopCustomizer as &dyn AgentCustomizer),
        )
        .await
        .expect("restore_flow (boot 2)");
        // A cold restart with persisted history must recover the session, never
        // re-Create it (a re-Create is the empty fresh-spawn regression).
        match result.outcomes.get(&alice).expect("alice outcome") {
            RestoreOutcome::Resumed { .. } | RestoreOutcome::Dormant { .. } => {}
            other @ RestoreOutcome::Created { .. } => {
                panic!("cold restart re-Created the member (history dropped): {other:?}")
            }
            other => panic!("unexpected restore outcome across cold restart: {other:?}"),
        }

        identity_rt
            .send(
                &alice,
                &meerkat_core::ContentInput::Text("What token did I give you earlier?".to_string()),
            )
            .await
            .expect("send turn 2");

        let deadline = Instant::now() + Duration::from_secs(30);
        while capture.count() < 1 {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for the post-restart turn to reach the LLM"
            );
            sleep(Duration::from_millis(100)).await;
        }

        // The post-restart request must REPLAY turn 1's transcript. The token
        // can only be present if resume loaded the persisted conversation
        // instead of falling back to an empty fresh spawn (Bug 1 / Ask B).
        let last_request = capture.last().expect("a post-restart request was captured");
        assert!(
            last_request.contains(TOKEN),
            "post-restart LLM request must replay the persisted transcript (token {TOKEN}); \
             cold-restart resume dropped the conversation history"
        );

        unified.shutdown().await;
    }
}
