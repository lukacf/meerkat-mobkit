#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_clone
)]
//! Per-admission completion: a delivery tracked by a [`TurnTicket`] reports
//! THAT turn and its own output, however much other traffic reaches the
//! identity.
//!
//! The defect: `send_and_wait` / `wait_for_output(after=...)` waited until
//! the identity-wide completion cursor passed the caller's pre-delivery
//! baseline, then returned the session's latest `output_preview`. A
//! concurrent delivery (a peer message, a scheduled turn, a fork completion
//! wake) completing first satisfied the wait, and the caller got someone
//! else's output.
//!
//! The first half drives [`IdentityRuntime`] over a scripted bridge whose
//! turns complete only when a test finishes them, by the interaction id the
//! admission carried. The second half drives the gateway RPC over a real
//! runtime whose LLM replies to each message by name.

use std::collections::{BTreeMap, HashMap};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_client::{LlmError, LlmEvent, LlmRequest};
use meerkat_core::types::{HandlingMode, StopReason};
use meerkat_mobkit::identity_first::contracts::{ContinuityStore, LeaseProvider};
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentBuildDraft, AgentIdentity, AgentRuntimeId, BridgeAdmissionError,
    BridgeDelivery, BridgeError, BridgeTurnReceipt, CheckpointVersion, CompletionProgress,
    ContinuityGeneration, ContinuityRecord, CorrelationId, DispatchIdempotencyKey, DispatchInput,
    DispatchOrigin, DurabilityPolicy, DurableAgentSpec, FencingToken, IdentityLifecycleState,
    IdentityRuntime, IdentityRuntimeConfig, LeaseGrant, LocalContinuityStore, LocalLeaseProvider,
    MemberInspection, MutableRosterProvider, ResumeSessionOutcome, SessionBridge, SessionSnapshot,
    TurnOutcome, TurnOutput, TurnTicket, TurnTracking,
};
use serde_json::{Value, json};

#[path = "support/llm_usage.rs"]
mod llm_usage;

// ===========================================================================
// Scripted bridge
// ===========================================================================

fn make_identity(name: &str) -> AgentIdentity {
    AgentIdentity::parse(name).unwrap()
}

fn make_spec(name: &str) -> DurableAgentSpec {
    DurableAgentSpec {
        identity: make_identity(name),
        profile: meerkat_mob::ProfileName::from("default"),
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

fn content(text: &str) -> meerkat_core::ContentInput {
    meerkat_core::ContentInput::Text(text.to_string())
}

/// Admits deliveries on both lanes, records the interaction id each one
/// carried, and completes an output-bearing turn only when the test finishes
/// it by that interaction id. `inspect_member` reports the latest output any
/// finished turn committed, like the session's `output_preview`.
struct TicketBridge {
    tracks: bool,
    session: meerkat_core::types::SessionId,
    ingress: std::sync::Mutex<Vec<Option<String>>>,
    tracked: std::sync::Mutex<Vec<Option<String>>>,
    finishers:
        std::sync::Mutex<HashMap<String, tokio::sync::oneshot::Sender<Result<TurnOutput, String>>>>,
    latest_output: std::sync::Mutex<Option<String>>,
}

impl TicketBridge {
    fn new(tracks: bool, session: meerkat_core::types::SessionId) -> Arc<Self> {
        Arc::new(Self {
            tracks,
            session,
            ingress: std::sync::Mutex::new(Vec::new()),
            tracked: std::sync::Mutex::new(Vec::new()),
            finishers: std::sync::Mutex::new(HashMap::new()),
            latest_output: std::sync::Mutex::new(None),
        })
    }

    fn ingress_interactions(&self) -> Vec<Option<String>> {
        self.ingress.lock().unwrap().clone()
    }

    fn tracked_interactions(&self) -> Vec<Option<String>> {
        self.tracked.lock().unwrap().clone()
    }

    /// Commit `text` as the latest session output without finishing any
    /// tracked turn: an untracked (foreign) turn finishing.
    fn commit_foreign_output(&self, text: &str) {
        *self.latest_output.lock().unwrap() = Some(text.to_string());
    }

    /// Finish the tracked turn admitted under `ticket`.
    fn finish(&self, ticket: TurnTicket, outcome: Result<&str, &str>) {
        let finisher = self
            .finishers
            .lock()
            .unwrap()
            .remove(&ticket.to_string())
            .expect("no tracked turn carried this ticket");
        if let Ok(text) = outcome {
            *self.latest_output.lock().unwrap() = Some(text.to_string());
        }
        let _ = finisher.send(
            outcome
                .map(|text| TurnOutput {
                    text: Some(text.to_string()),
                    truncated: false,
                })
                .map_err(ToString::to_string),
        );
    }
}

#[async_trait]
impl SessionBridge for TicketBridge {
    async fn create_session(
        &self,
        _identity: &AgentIdentity,
        _runtime_id: &AgentRuntimeId,
        _spec: &DurableAgentSpec,
        _draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        Ok(session_id.clone())
    }

    async fn resume_session(
        &self,
        _identity: &AgentIdentity,
        _runtime_id: &AgentRuntimeId,
        _spec: &DurableAgentSpec,
        _draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
        _snapshot: &SessionSnapshot,
    ) -> Result<ResumeSessionOutcome, BridgeError> {
        Ok(ResumeSessionOutcome::Resumed {
            session_id: session_id.clone(),
        })
    }

    async fn deliver_admitted(
        &self,
        _runtime_id: &AgentRuntimeId,
        delivery: BridgeDelivery,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        self.ingress.lock().unwrap().push(delivery.interaction_id);
        Ok(self.session.clone())
    }

    fn tracks_turn_output(&self) -> bool {
        self.tracks
    }

    async fn begin_delivery_with_output(
        &self,
        _runtime_id: &AgentRuntimeId,
        delivery: BridgeDelivery,
    ) -> Result<BridgeTurnReceipt, BridgeAdmissionError> {
        let interaction = delivery.interaction_id.clone();
        self.tracked.lock().unwrap().push(interaction.clone());
        let (finish, finished) = tokio::sync::oneshot::channel();
        self.finishers.lock().unwrap().insert(
            interaction.expect("a tracked delivery carries its ticket as its interaction id"),
            finish,
        );
        Ok(BridgeTurnReceipt::with_output(
            Ok::<_, String>(self.session.clone()),
            async move {
                finished
                    .await
                    .unwrap_or_else(|_| Err("the turn was dropped".to_string()))
            },
        ))
    }

    async fn checkpoint_session(
        &self,
        _runtime_id: &AgentRuntimeId,
        _session_id: &meerkat_core::types::SessionId,
    ) -> Result<SessionSnapshot, BridgeError> {
        Ok(SessionSnapshot { data: Vec::new() })
    }

    async fn retire_member(&self, _runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
        Ok(())
    }

    async fn inspect_member(
        &self,
        _runtime_id: &AgentRuntimeId,
    ) -> Result<MemberInspection, BridgeError> {
        Ok(MemberInspection {
            output_preview: self.latest_output.lock().unwrap().clone(),
            is_final: false,
            peer_reachable_count: 0,
        })
    }
}

fn make_runtime(bridge: Option<Arc<dyn SessionBridge>>) -> Arc<IdentityRuntime> {
    let store: Arc<dyn ContinuityStore> = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease: Arc<dyn LeaseProvider> = Arc::new(LocalLeaseProvider::new());
    Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store,
        lease_provider: lease,
        runtime_instance_id: "turn-ticket-test".to_string(),
        has_runtime_store: true,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge,
        default_timeout: None,
    }))
}

/// Register an Active identity bound to `session`, the session the bridge
/// resolves every turn onto (so no delivery looks like a rotation).
async fn register_bound(
    runtime: &IdentityRuntime,
    name: &str,
    session: &meerkat_core::types::SessionId,
) -> AgentIdentity {
    runtime
        .register(
            make_spec(name),
            IdentityLifecycleState::Active,
            Some(ContinuityRecord {
                identity: make_identity(name),
                agent_runtime_id: AgentRuntimeId::parse(&format!("rt:{name}")).unwrap(),
                session_id: session.clone(),
                generation: ContinuityGeneration::new(0),
                checkpoint_version: CheckpointVersion::new(0),
            }),
            Some(LeaseGrant {
                identity: make_identity(name),
                fencing_token: FencingToken::new(1),
                ttl: Duration::from_mins(5),
            }),
        )
        .await;
    make_identity(name)
}

fn tracked(turn: &TurnTracking) -> TurnTicket {
    match turn {
        TurnTracking::Tracked(ticket) => *ticket,
        TurnTracking::Unavailable { reason } => panic!("the turn was not tracked: {reason}"),
    }
}

fn completed(text: &str) -> TurnOutcome {
    TurnOutcome::Completed {
        output: Some(TurnOutput {
            text: Some(text.to_string()),
            truncated: false,
        }),
    }
}

/// What the SDKs' old `send_and_wait` did: wait until the identity-wide
/// cursor passes `baseline`, then return the session's latest output.
async fn old_cursor_wait(
    runtime: &IdentityRuntime,
    identity: &AgentIdentity,
    baseline: meerkat_mobkit::identity_first::CompletionCursor,
) -> Option<String> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let cursor = runtime.completion_cursor(identity).await;
            let inspection = runtime.inspect(identity).await.expect("inspect");
            if cursor.progress_since(baseline) == CompletionProgress::Completed {
                return inspection.output_preview;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the cursor passes the baseline")
}

/// Two tracked sends and one untracked (foreign) delivery to ONE identity,
/// completing foreign first, then B, then A, each with distinct output. Each
/// ticket reports its own turn's output. The foreign completion moves the
/// identity-wide cursor past A's baseline, and the old cursor wait returns
/// the foreign output for A; A's ticket stays pending until A finishes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_deliveries_to_one_identity_each_get_their_own_output() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(true, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let keeper = register_bound(&runtime, "keeper", &session).await;

    let a = runtime
        .send_with_turn_ticket(&keeper, None, &content("alpha"), HandlingMode::Queue, None)
        .await
        .expect("send A");
    let b = runtime
        .send_with_turn_ticket(&keeper, None, &content("beta"), HandlingMode::Queue, None)
        .await
        .expect("send B");
    runtime
        .send_admission_tracked(
            &keeper,
            None,
            &content("foreign"),
            HandlingMode::Queue,
            None,
        )
        .await
        .expect("foreign send");
    let (ticket_a, ticket_b) = (tracked(&a.turn), tracked(&b.turn));
    assert_ne!(ticket_a, ticket_b);
    // Typed state: each ticket is exactly the interaction id its own admission
    // carried into the runtime; the untracked delivery carried none.
    assert_eq!(
        bridge.tracked_interactions(),
        vec![Some(ticket_a.to_string()), Some(ticket_b.to_string())]
    );
    assert_eq!(bridge.ingress_interactions(), vec![None]);
    assert_eq!(
        runtime.turn_outcome(&keeper, ticket_a),
        TurnOutcome::Pending
    );

    // The foreign turn completes first.
    bridge.commit_foreign_output("foreign says hi");
    runtime.record_turn_completed(&keeper).await;
    assert_eq!(
        old_cursor_wait(&runtime, &keeper, a.admission.completion_baseline)
            .await
            .as_deref(),
        Some("foreign says hi"),
        "control: the identity-wide cursor wait is satisfied by the foreign turn"
    );
    assert_eq!(
        runtime.turn_outcome(&keeper, ticket_a),
        TurnOutcome::Pending,
        "a foreign completion must never satisfy A's ticket"
    );

    bridge.finish(ticket_b, Ok("B says hi"));
    runtime.record_turn_completed(&keeper).await;
    bridge.finish(ticket_a, Ok("A says hi"));
    runtime.record_turn_completed(&keeper).await;

    assert_eq!(
        runtime
            .wait_for_turn(&keeper, ticket_a, Duration::from_secs(5))
            .await,
        completed("A says hi")
    );
    assert_eq!(
        runtime
            .wait_for_turn(&keeper, ticket_b, Duration::from_secs(5))
            .await,
        completed("B says hi")
    );
}

/// A tracked turn that fails is `Failed` with its reason; a ticket nobody
/// admitted, or one read for another identity, is `Unknown`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_turn_is_failed_and_an_unknown_ticket_is_unknown() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(true, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let keeper = register_bound(&runtime, "keeper", &session).await;
    let other = register_bound(&runtime, "other", &session).await;

    let a = runtime
        .send_with_turn_ticket(&keeper, None, &content("alpha"), HandlingMode::Queue, None)
        .await
        .expect("send A");
    let ticket = tracked(&a.turn);
    bridge.finish(ticket, Err("the model refused"));
    match runtime
        .wait_for_turn(&keeper, ticket, Duration::from_secs(5))
        .await
    {
        TurnOutcome::Failed { reason } => assert!(reason.contains("the model refused"), "{reason}"),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(
        runtime.turn_outcome(&other, ticket),
        TurnOutcome::Unknown,
        "a ticket belongs to the identity it was admitted for"
    );
    assert_eq!(
        runtime
            .wait_for_turn(&keeper, TurnTicket::mint(), Duration::from_secs(5))
            .await,
        TurnOutcome::Unknown
    );
}

/// A bridge that cannot report per-turn output: the send is delivered once,
/// on the ingress lane, and reported as untracked with the reason.
#[tokio::test]
async fn an_untrackable_turn_is_delivered_once_on_the_ingress_lane() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(false, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let keeper = register_bound(&runtime, "keeper", &session).await;

    let sent = runtime
        .send_with_turn_ticket(&keeper, None, &content("alpha"), HandlingMode::Queue, None)
        .await
        .expect("send");
    match &sent.turn {
        TurnTracking::Unavailable { reason } => {
            assert!(reason.contains("cannot report"), "{reason}");
        }
        TurnTracking::Tracked(ticket) => panic!("tracked on an untrackable bridge: {ticket}"),
    }
    assert_eq!(bridge.ingress_interactions().len(), 1);
    assert!(bridge.tracked_interactions().is_empty());

    // No bridge at all: nothing to deliver or track.
    let bare = make_runtime(None);
    let lone = register_bound(&bare, "lone", &session).await;
    let sent = bare
        .send_with_turn_ticket(&lone, None, &content("alpha"), HandlingMode::Queue, None)
        .await
        .expect("send without a bridge");
    assert!(matches!(sent.turn, TurnTracking::Unavailable { .. }));
}

/// A caller's UUID interaction id is the ticket; a non-UUID one cannot ride
/// the admission, so the turn is untracked (and delivered once on ingress).
#[tokio::test]
async fn a_caller_interaction_id_is_the_ticket() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(true, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let keeper = register_bound(&runtime, "keeper", &session).await;

    let interaction = uuid::Uuid::new_v4().to_string();
    let sent = runtime
        .send_with_turn_ticket(
            &keeper,
            None,
            &content("alpha"),
            HandlingMode::Queue,
            Some(&interaction),
        )
        .await
        .expect("send");
    assert_eq!(tracked(&sent.turn).to_string(), interaction);
    assert_eq!(bridge.tracked_interactions(), vec![Some(interaction)]);

    let sent = runtime
        .send_with_turn_ticket(
            &keeper,
            None,
            &content("beta"),
            HandlingMode::Queue,
            Some("not-a-uuid"),
        )
        .await
        .expect("send");
    assert!(matches!(sent.turn, TurnTracking::Unavailable { .. }));
    assert_eq!(bridge.ingress_interactions().len(), 1);
}

/// A tracked dispatch carries its correlation id as the ticket (so a
/// deduplicated re-dispatch names the same turn), or a fresh ticket without
/// one; either way the ticket is the interaction id the admission carried.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tracked_dispatch_carries_its_ticket_as_the_interaction_id() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(true, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let keeper = register_bound(&runtime, "keeper", &session).await;

    let correlation = uuid::Uuid::new_v4().to_string();
    let correlated = runtime
        .dispatch_with_turn_ticket(
            &keeper,
            None,
            &DispatchInput {
                content: content("scheduled"),
                origin: DispatchOrigin::System,
                correlation_id: Some(CorrelationId::new(correlation.as_str())),
                idempotency_key: Some(DispatchIdempotencyKey::new("tick-1")),
            },
        )
        .await
        .expect("correlated dispatch");
    let uncorrelated = runtime
        .dispatch_with_turn_ticket(
            &keeper,
            None,
            &DispatchInput {
                content: content("ad hoc"),
                origin: DispatchOrigin::System,
                correlation_id: None,
                idempotency_key: None,
            },
        )
        .await
        .expect("uncorrelated dispatch");
    let (first, second) = (tracked(&correlated.turn), tracked(&uncorrelated.turn));
    assert_eq!(first.to_string(), correlation);
    assert_eq!(
        bridge.tracked_interactions(),
        vec![Some(first.to_string()), Some(second.to_string())]
    );
    bridge.finish(first, Ok("scheduled done"));
    assert_eq!(
        runtime
            .wait_for_turn(&keeper, first, Duration::from_secs(5))
            .await,
        completed("scheduled done")
    );
}

// ===========================================================================
// Real runtime, through the gateway RPC
// ===========================================================================

/// Replies to each message by the name it carries ("reply to: alpha"), and
/// holds a turn whose name is held until the test releases it.
#[derive(Clone, Default)]
struct NamedReplyClient {
    holds: Arc<std::sync::Mutex<HashMap<&'static str, tokio::sync::oneshot::Receiver<()>>>>,
    calls: Arc<AtomicUsize>,
}

const NAMES: [&str; 3] = ["foreign", "alpha", "beta"];

impl NamedReplyClient {
    fn hold(&self, name: &'static str) -> tokio::sync::oneshot::Sender<()> {
        let (release, held) = tokio::sync::oneshot::channel();
        self.holds.lock().unwrap().insert(name, held);
        release
    }
}

impl meerkat_client::LlmClient for NamedReplyClient {
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
        let last_user = request
            .messages
            .iter()
            .rev()
            .find_map(|message| match message {
                meerkat_core::Message::User(user) => Some(user.text_content()),
                _ => None,
            })
            .unwrap_or_default();
        let name = NAMES
            .into_iter()
            .find(|name| last_user.contains(name))
            .unwrap_or("other");
        let held = self.holds.lock().unwrap().remove(name);
        let [usage, done] =
            llm_usage::usage_then_done(request, meerkat::Provider::OpenAI, StopReason::EndTurn);
        Box::pin(async_stream::stream! {
            if let Some(held) = held {
                let _ = held.await;
            }
            yield Ok(LlmEvent::TextDelta { delta: format!("reply to: {name}"), meta: None });
            yield Ok(usage);
            yield Ok(done);
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

async fn rpc(
    runtime: &meerkat_mobkit::UnifiedRuntime,
    ctx: &meerkat_mobkit::rpc::IdentityFirstContext,
    method: &str,
    params: Value,
) -> Value {
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let reply = meerkat_mobkit::rpc::handle_unified_rpc_json(
        runtime,
        &request.to_string(),
        Duration::from_secs(30),
        None,
        Some(ctx),
    )
    .await;
    let reply: Value = serde_json::from_str(&reply).expect("json-rpc reply");
    assert!(reply.get("error").is_none(), "{method} failed: {reply}");
    reply["result"].clone()
}

async fn turn_result(
    runtime: &meerkat_mobkit::UnifiedRuntime,
    ctx: &meerkat_mobkit::rpc::IdentityFirstContext,
    ticket: &str,
) -> Value {
    rpc(
        runtime,
        ctx,
        "mobkit/turn_result",
        json!({"identity": "keeper", "ticket": ticket}),
    )
    .await
}

async fn await_turn(
    runtime: &meerkat_mobkit::UnifiedRuntime,
    ctx: &meerkat_mobkit::rpc::IdentityFirstContext,
    ticket: &str,
) -> Value {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let result = turn_result(runtime, ctx, ticket).await;
            if result["state"] != "pending" {
                return result;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the turn settles")
}

/// End to end on a real runtime: an untracked dispatch to `keeper` is
/// admitted first and held; two tracked sends queue behind it. When the
/// dispatch finishes, the identity-wide cursor passes the first send's
/// baseline, and the old cursor wait (the SDKs' previous `send_and_wait`)
/// returns at once, without that send's reply. Each ticket still reports its
/// own turn's reply.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_gateway_sends_each_get_their_own_reply() {
    let scratch = tempfile::tempdir().expect("scratch dir");
    let identity = make_identity("keeper");
    let definition = meerkat_mob::MobDefinition::from_toml(&format!(
        r#"
[mob]
id = "turn-tickets-{}"
[profiles.worker]
model = "gpt-5.5"
external_addressable = true
[profiles.worker.tools]
comms = true
"#,
        uuid::Uuid::new_v4()
    ))
    .expect("mob definition");
    let mut spec = make_spec("keeper");
    spec.profile = "worker".into();
    spec.runtime_mode_override = Some(meerkat_mob::MobRuntimeMode::TurnDriven);
    let roster = Arc::new(MutableRosterProvider::new(vec![spec]));
    let client = NamedReplyClient::default();
    let release_foreign = client.hold("foreign");
    let release_alpha = client.hold("alpha");
    let runtime = meerkat_mobkit::UnifiedRuntime::builder()
        .definition(definition)
        .continuity_store(Arc::new(LocalContinuityStore::in_memory().unwrap()))
        .lease_provider(Arc::new(LocalLeaseProvider::new()))
        .roster_provider(roster.clone())
        .scratch_dir(scratch.path())
        .identity_runtime_instance_id("turn-tickets-live")
        .default_llm_client(Arc::new(client.clone()))
        .build()
        .await
        .expect("build runtime");
    let identity_runtime = runtime
        .identity_runtime()
        .cloned()
        .expect("identity runtime");
    let ctx = meerkat_mobkit::rpc::IdentityFirstContext {
        runtime: identity_runtime.clone(),
        roster_provider: roster,
        topology_provider: None,
        customizer: None,
        agent_memory_provider: None,
        mob_definition: Some(runtime.mob_handle().definition().clone()),
        transcript_edit_service: None,
        compaction_floors: None,
    };

    // The untracked dispatch is admitted first and held in the model.
    rpc(
        &runtime,
        &ctx,
        "mobkit/dispatch",
        json!({"identity": "keeper", "dispatch_input": {"content": "foreign", "origin": "system"}}),
    )
    .await;
    let alpha = rpc(
        &runtime,
        &ctx,
        "mobkit/send",
        json!({"identity": "keeper", "content": "alpha", "track_turn": true}),
    )
    .await;
    let beta = rpc(
        &runtime,
        &ctx,
        "mobkit/send",
        json!({"identity": "keeper", "content": "beta", "track_turn": true}),
    )
    .await;
    let alpha_ticket = alpha["turn"]["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("alpha is tracked: {alpha}"))
        .to_string();
    let beta_ticket = beta["turn"]["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("beta is tracked: {beta}"))
        .to_string();
    let alpha_baseline: meerkat_mobkit::identity_first::CompletionCursor =
        serde_json::from_value(alpha["completion_baseline"].clone()).expect("baseline");
    assert_eq!(
        turn_result(&runtime, &ctx, &alpha_ticket).await["state"],
        "pending"
    );

    // The dispatch finishes; alpha is still held in the model.
    release_foreign.send(()).expect("release the dispatch");
    let old_wait = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let inspection = rpc(
                &runtime,
                &ctx,
                "mobkit/inspect_identity",
                json!({"identity": "keeper"}),
            )
            .await;
            let cursor: meerkat_mobkit::identity_first::CompletionCursor =
                serde_json::from_value(inspection["completion_cursor"].clone())
                    .expect("completion cursor");
            if cursor.progress_since(alpha_baseline) == CompletionProgress::Completed {
                return inspection["output_preview"].as_str().map(str::to_string);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the cursor passes alpha's baseline");
    assert_ne!(
        old_wait.as_deref(),
        Some("reply to: alpha"),
        "control: the old cursor wait returns before alpha's reply exists"
    );
    assert_eq!(
        turn_result(&runtime, &ctx, &alpha_ticket).await["state"],
        "pending",
        "the dispatch's completion must not settle alpha's ticket"
    );

    release_alpha.send(()).expect("release alpha");
    let alpha_result = await_turn(&runtime, &ctx, &alpha_ticket).await;
    assert_eq!(alpha_result["state"], "completed", "{alpha_result}");
    assert_eq!(alpha_result["output"], "reply to: alpha", "{alpha_result}");
    let beta_result = await_turn(&runtime, &ctx, &beta_ticket).await;
    assert_eq!(beta_result["state"], "completed", "{beta_result}");
    assert_eq!(beta_result["output"], "reply to: beta", "{beta_result}");
    assert_eq!(
        turn_result(&runtime, &ctx, &uuid::Uuid::new_v4().to_string()).await["state"],
        "unknown"
    );
    let _ = identity;
    runtime.shutdown().await;
}
