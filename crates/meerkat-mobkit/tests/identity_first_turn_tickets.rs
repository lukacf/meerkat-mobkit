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
//! tracked turns complete only when a test finishes them, in admission order.
//! The second half drives the gateway RPC over a real runtime whose LLM
//! replies to each message by name, for both runtime modes.

use std::collections::{BTreeMap, HashMap, HashSet};
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
    ContinuityGeneration, ContinuityRecord, CorrelationId, DeliveryErrorClass,
    DispatchIdempotencyKey, DispatchInput, DispatchOrigin, DurabilityPolicy, DurableAgentSpec,
    FencingToken, IdentityLifecycleState, IdentityRuntime, IdentityRuntimeConfig, LeaseGrant,
    LocalContinuityStore, LocalLeaseProvider, MemberInspection, MutableRosterProvider,
    ResumeSessionOutcome, SessionBridge, SessionSnapshot, TurnOutcome, TurnOutput, TurnTicket,
    TurnTracking, TurnUntrackable,
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

/// A turn-driven member: meerkat reports per-turn completion for it.
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
        runtime_mode_override: Some(meerkat_mob::MobRuntimeMode::TurnDriven),
        backend: None,
        binding: None,
        placement: None,
    }
}

fn content(text: &str) -> meerkat_core::ContentInput {
    meerkat_core::ContentInput::Text(text.to_string())
}

/// One admission the bridge saw: the interaction id and idempotency key it
/// carried.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Admission {
    interaction_id: Option<String>,
    idempotency_key: Option<String>,
}

type Finisher = tokio::sync::oneshot::Sender<Result<TurnOutput, String>>;

/// Admits deliveries on both lanes and records what each carried. A tracked
/// turn completes only when the test finishes it, by admission order. Like
/// meerkat, a tracked admission of an idempotency key this member already
/// admitted is deduplicated onto the earlier (ended) admission and completes
/// at once without a run result of its own. `inspect_member` reports the
/// latest output any finished turn committed, like the session's
/// `output_preview`.
struct TicketBridge {
    tracks: bool,
    resolution_fails: bool,
    /// When set, every tracked admission is refused before admission, typed,
    /// as meerkat refuses one for a member that actually runs in this mode.
    refuses_tracked_in: std::sync::Mutex<Option<meerkat_mob::MobRuntimeMode>>,
    refused: AtomicUsize,
    session: meerkat_core::types::SessionId,
    ingress: std::sync::Mutex<Vec<Admission>>,
    tracked: std::sync::Mutex<Vec<Admission>>,
    finishers: std::sync::Mutex<Vec<Option<Finisher>>>,
    seen_keys: std::sync::Mutex<HashSet<(AgentRuntimeId, String)>>,
    latest_output: std::sync::Mutex<Option<String>>,
}

impl TicketBridge {
    fn new(tracks: bool, session: meerkat_core::types::SessionId) -> Arc<Self> {
        Self::build(tracks, false, session)
    }

    fn build(
        tracks: bool,
        resolution_fails: bool,
        session: meerkat_core::types::SessionId,
    ) -> Arc<Self> {
        Arc::new(Self {
            tracks,
            resolution_fails,
            refuses_tracked_in: std::sync::Mutex::new(None),
            refused: AtomicUsize::new(0),
            session,
            ingress: std::sync::Mutex::new(Vec::new()),
            tracked: std::sync::Mutex::new(Vec::new()),
            finishers: std::sync::Mutex::new(Vec::new()),
            seen_keys: std::sync::Mutex::new(HashSet::new()),
            latest_output: std::sync::Mutex::new(None),
        })
    }

    fn ingress_admissions(&self) -> Vec<Admission> {
        self.ingress.lock().unwrap().clone()
    }

    fn tracked_admissions(&self) -> Vec<Admission> {
        self.tracked.lock().unwrap().clone()
    }

    fn record(
        &self,
        runtime_id: &AgentRuntimeId,
        delivery: &BridgeDelivery,
        lane: &std::sync::Mutex<Vec<Admission>>,
    ) -> bool {
        let key = delivery
            .delivery_identity
            .as_ref()
            .map(|carrier| carrier.idempotency_key.clone());
        lane.lock().unwrap().push(Admission {
            interaction_id: delivery.interaction_id.clone(),
            idempotency_key: key.clone(),
        });
        key.is_some_and(|key| {
            !self
                .seen_keys
                .lock()
                .unwrap()
                .insert((runtime_id.clone(), key))
        })
    }

    /// Commit `text` as the latest session output without finishing any
    /// tracked turn: an untracked (foreign) turn finishing.
    fn commit_foreign_output(&self, text: &str) {
        *self.latest_output.lock().unwrap() = Some(text.to_string());
    }

    /// Finish the `index`th tracked admission.
    fn finish(&self, index: usize, outcome: Result<&str, &str>) {
        let finisher = self
            .finishers
            .lock()
            .unwrap()
            .get_mut(index)
            .and_then(Option::take)
            .expect("no pending tracked turn at this admission index");
        if let Ok(text) = outcome {
            *self.latest_output.lock().unwrap() = Some(text.to_string());
        }
        let _ = finisher.send(
            outcome
                .map(|text| TurnOutput::Text {
                    text: text.to_string(),
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
        runtime_id: &AgentRuntimeId,
        delivery: BridgeDelivery,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        self.record(runtime_id, &delivery, &self.ingress);
        Ok(self.session.clone())
    }

    fn tracks_turn_output(&self) -> bool {
        self.tracks
    }

    async fn begin_delivery_with_output(
        &self,
        runtime_id: &AgentRuntimeId,
        delivery: BridgeDelivery,
    ) -> Result<BridgeTurnReceipt, BridgeAdmissionError> {
        if let Some(mode) = *self.refuses_tracked_in.lock().unwrap() {
            self.refused.fetch_add(1, Ordering::SeqCst);
            return Err(BridgeAdmissionError::UnsupportedForMode {
                identity: meerkat_mob::ids::AgentIdentity::from(runtime_id.as_str()),
                mode,
                detail: format!(
                    "unsupported for runtime mode {mode}: tracked turn completion is not \
                     supported by autonomous inbox delivery"
                ),
            });
        }
        let deduplicated = self.record(runtime_id, &delivery, &self.tracked);
        let (finish, finished) = tokio::sync::oneshot::channel();
        if deduplicated {
            // meerkat's KeyOnly dedup onto an ended admission: a successful
            // terminal with no run result of this admission's own.
            let _ = finish.send(Ok(TurnOutput::NoOwnResult));
            self.finishers.lock().unwrap().push(None);
        } else {
            self.finishers.lock().unwrap().push(Some(finish));
        }
        let session_result = if self.resolution_fails {
            Err("the machine-state projection has no session for this member".to_string())
        } else {
            Ok(self.session.clone())
        };
        Ok(BridgeTurnReceipt::with_output(session_result, async move {
            finished
                .await
                .unwrap_or_else(|_| Err("the turn was dropped".to_string()))
        }))
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
async fn register_spec(
    runtime: &IdentityRuntime,
    spec: DurableAgentSpec,
    session: &meerkat_core::types::SessionId,
) -> AgentIdentity {
    let identity = spec.identity.clone();
    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(ContinuityRecord {
                identity: identity.clone(),
                agent_runtime_id: AgentRuntimeId::parse(&format!("rt:{identity}")).unwrap(),
                session_id: session.clone(),
                generation: ContinuityGeneration::new(0),
                checkpoint_version: CheckpointVersion::new(0),
            }),
            Some(LeaseGrant {
                identity: identity.clone(),
                fencing_token: FencingToken::new(1),
                ttl: Duration::from_mins(5),
            }),
        )
        .await;
    identity
}

async fn register_bound(
    runtime: &IdentityRuntime,
    name: &str,
    session: &meerkat_core::types::SessionId,
) -> AgentIdentity {
    register_spec(runtime, make_spec(name), session).await
}

fn tracked(turn: &TurnTracking) -> TurnTicket {
    match turn {
        TurnTracking::Tracked(ticket) => *ticket,
        TurnTracking::Unavailable(reason) => panic!("the turn was not tracked: {reason}"),
    }
}

fn completed(text: &str) -> TurnOutcome {
    TurnOutcome::Completed {
        output: TurnOutput::Text {
            text: text.to_string(),
            truncated: false,
        },
    }
}

fn unknown_ticket() -> TurnTicket {
    TurnTicket::parse(&uuid::Uuid::new_v4().to_string()).unwrap()
}

fn correlated(text: &str, key: &str, correlation: &str) -> DispatchInput {
    DispatchInput {
        content: content(text),
        origin: DispatchOrigin::Connector,
        correlation_id: Some(CorrelationId::new(correlation)),
        idempotency_key: Some(DispatchIdempotencyKey::new(key)),
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
    assert_eq!(bridge.tracked_admissions().len(), 2);
    assert_eq!(bridge.ingress_admissions().len(), 1);
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

    bridge.finish(1, Ok("B says hi"));
    runtime.record_turn_completed(&keeper).await;
    bridge.finish(0, Ok("A says hi"));
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
    bridge.finish(0, Err("the model refused"));
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
            .wait_for_turn(&keeper, unknown_ticket(), Duration::from_secs(5))
            .await,
        TurnOutcome::Unknown
    );
}

/// A bridge that cannot report per-turn output: the send is delivered once,
/// on the ingress lane, and reported as untracked with a typed reason.
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
    assert_eq!(
        sent.turn,
        TurnTracking::Unavailable(TurnUntrackable::BridgeCannotReportOutput)
    );
    assert_eq!(bridge.ingress_admissions().len(), 1);
    assert!(bridge.tracked_admissions().is_empty());

    // No bridge at all: nothing to deliver or track.
    let bare = make_runtime(None);
    let lone = register_bound(&bare, "lone", &session).await;
    let sent = bare
        .send_with_turn_ticket(&lone, None, &content("alpha"), HandlingMode::Queue, None)
        .await
        .expect("send without a bridge");
    assert_eq!(
        sent.turn,
        TurnTracking::Unavailable(TurnUntrackable::NotDelivered)
    );
}

/// An `autonomous_host` member (the default mode) refuses completion-bearing
/// admission in meerkat, so a tracked send or dispatch to it is never
/// attempted on the tracked lane: it is delivered exactly once on the ingress
/// lane and reported untracked with the typed reason.
#[tokio::test]
async fn an_autonomous_member_is_delivered_once_on_the_ingress_lane() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(true, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let mut spec = make_spec("roamer");
    spec.runtime_mode_override = None; // resolves to meerkat's default
    let roamer = register_spec(&runtime, spec, &session).await;

    let sent = runtime
        .send_with_turn_ticket(&roamer, None, &content("alpha"), HandlingMode::Queue, None)
        .await
        .expect("send");
    assert_eq!(
        sent.turn,
        TurnTracking::Unavailable(TurnUntrackable::AutonomousHost)
    );
    let dispatched = runtime
        .dispatch_with_turn_ticket(&roamer, None, &correlated("beta", "evt-1", "chat-1"))
        .await
        .expect("dispatch");
    assert_eq!(
        dispatched.turn,
        TurnTracking::Unavailable(TurnUntrackable::AutonomousHost)
    );
    assert_eq!(bridge.ingress_admissions().len(), 2);
    assert!(
        bridge.tracked_admissions().is_empty(),
        "nothing was attempted on the tracked lane"
    );
}

/// The caller's interaction id rides into the admission unchanged but never
/// names the ticket: two sends reusing one interaction id get distinct
/// tickets, and each reports its own turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reused_interaction_id_never_shares_a_ticket() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(true, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let keeper = register_bound(&runtime, "keeper", &session).await;

    let interaction = uuid::Uuid::new_v4().to_string();
    let mut tickets = Vec::new();
    for text in ["alpha", "beta"] {
        let sent = runtime
            .send_with_turn_ticket(
                &keeper,
                None,
                &content(text),
                HandlingMode::Queue,
                Some(&interaction),
            )
            .await
            .expect("send");
        tickets.push(tracked(&sent.turn));
    }
    assert_ne!(tickets[0], tickets[1]);
    assert!(
        tickets
            .iter()
            .all(|ticket| ticket.to_string() != interaction)
    );
    assert!(
        bridge
            .tracked_admissions()
            .iter()
            .all(|admission| admission.interaction_id.as_deref() == Some(interaction.as_str())),
        "the caller's interaction id rides unchanged"
    );

    bridge.finish(1, Ok("beta reply"));
    assert_eq!(
        runtime
            .wait_for_turn(&keeper, tickets[1], Duration::from_secs(5))
            .await,
        completed("beta reply")
    );
    assert_eq!(
        runtime.turn_outcome(&keeper, tickets[0]),
        TurnOutcome::Pending
    );
    bridge.finish(0, Ok("alpha reply"));
    assert_eq!(
        runtime
            .wait_for_turn(&keeper, tickets[0], Duration::from_secs(5))
            .await,
        completed("alpha reply")
    );

    // A non-UUID interaction id no longer prevents tracking.
    let sent = runtime
        .send_with_turn_ticket(
            &keeper,
            None,
            &content("gamma"),
            HandlingMode::Queue,
            Some("not-a-uuid"),
        )
        .await
        .expect("send");
    tracked(&sent.turn);
}

/// One correlation fanned out to two identities (per-target idempotency
/// keys): two distinct turns, two distinct tickets. The second identity's
/// first look is `Pending`, never `Unknown`, and neither ticket can report
/// the other identity's output, whichever finishes first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fanned_out_correlation_gets_a_ticket_per_identity() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(true, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let luka = register_bound(&runtime, "luka", &session).await;
    let louise = register_bound(&runtime, "louise", &session).await;

    let correlation = "telegram:primary/i:769";
    let to_luka = runtime
        .dispatch_with_turn_ticket(&luka, None, &correlated("hi", "luka-769", correlation))
        .await
        .expect("dispatch to luka");
    let to_louise = runtime
        .dispatch_with_turn_ticket(&louise, None, &correlated("hi", "louise-769", correlation))
        .await
        .expect("dispatch to louise");
    let (luka_ticket, louise_ticket) = (tracked(&to_luka.turn), tracked(&to_louise.turn));
    assert_ne!(luka_ticket, louise_ticket);
    // Both admissions carry the one (canonicalized) correlation as their
    // interaction id, as meerkat requires; the tickets are not derived from it.
    let carried = bridge.tracked_admissions();
    assert_eq!(carried[0].interaction_id, carried[1].interaction_id);
    assert_eq!(
        runtime.turn_outcome(&louise, louise_ticket),
        TurnOutcome::Pending
    );

    bridge.finish(1, Ok("louise's reply"));
    assert_eq!(
        runtime
            .wait_for_turn(&louise, louise_ticket, Duration::from_secs(5))
            .await,
        completed("louise's reply")
    );
    assert_eq!(
        runtime.turn_outcome(&luka, luka_ticket),
        TurnOutcome::Pending,
        "louise's completion must never settle luka's ticket"
    );
    bridge.finish(0, Ok("luka's reply"));
    assert_eq!(
        runtime
            .wait_for_turn(&luka, luka_ticket, Duration::from_secs(5))
            .await,
        completed("luka's reply")
    );
}

/// Two dispatches to ONE identity under one correlation but different
/// idempotency keys are two turns: the second's ticket never reads the
/// first's (already settled) output.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reused_correlation_on_one_identity_is_two_turns() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(true, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let keeper = register_bound(&runtime, "keeper", &session).await;

    let first = runtime
        .dispatch_with_turn_ticket(&keeper, None, &correlated("one", "msg-1", "chat-42"))
        .await
        .expect("first dispatch");
    let first = tracked(&first.turn);
    bridge.finish(0, Ok("first reply"));
    assert_eq!(
        runtime
            .wait_for_turn(&keeper, first, Duration::from_secs(5))
            .await,
        completed("first reply")
    );

    let second = runtime
        .dispatch_with_turn_ticket(&keeper, None, &correlated("two", "msg-2", "chat-42"))
        .await
        .expect("second dispatch");
    let second = tracked(&second.turn);
    assert_ne!(first, second);
    assert_eq!(
        runtime.turn_outcome(&keeper, second),
        TurnOutcome::Pending,
        "the second turn must not read the first turn's output"
    );
    bridge.finish(1, Ok("second reply"));
    assert_eq!(
        runtime
            .wait_for_turn(&keeper, second, Duration::from_secs(5))
            .await,
        completed("second reply")
    );
}

/// An idempotent re-dispatch after the original completed names the
/// ORIGINAL's ticket and reads the original's output; it is delivered on the
/// ingress lane (where the runtime deduplicates it), so no waiter can report
/// the runtime's "no result of its own" terminal over that output. A
/// deduplicated re-dispatch whose original this registry never tracked is
/// tracked on its own and completes as `NoOwnResult`, never as "no text".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_redispatch_names_the_original_ticket_and_never_overwrites_its_output() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(true, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let keeper = register_bound(&runtime, "keeper", &session).await;

    let original = runtime
        .dispatch_with_turn_ticket(&keeper, None, &correlated("work", "evt-9", "evt-9"))
        .await
        .expect("original dispatch");
    let original = tracked(&original.turn);
    bridge.finish(0, Ok("answer"));
    assert_eq!(
        runtime
            .wait_for_turn(&keeper, original, Duration::from_secs(5))
            .await,
        completed("answer")
    );

    let retry = runtime
        .dispatch_with_turn_ticket(&keeper, None, &correlated("work", "evt-9", "evt-9"))
        .await
        .expect("retried dispatch");
    assert_eq!(tracked(&retry.turn), original);
    assert_eq!(bridge.tracked_admissions().len(), 1);
    assert_eq!(bridge.ingress_admissions().len(), 1);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        runtime.turn_outcome(&keeper, original),
        completed("answer"),
        "the original's output survives the re-dispatch"
    );

    // The original was untracked, so the registry cannot name it.
    runtime
        .dispatch_admission_tracked(&keeper, None, &correlated("other", "evt-10", "evt-10"))
        .await
        .expect("untracked original");
    let retry = runtime
        .dispatch_with_turn_ticket(&keeper, None, &correlated("other", "evt-10", "evt-10"))
        .await
        .expect("tracked re-dispatch");
    assert_eq!(
        runtime
            .wait_for_turn(&keeper, tracked(&retry.turn), Duration::from_secs(5))
            .await,
        TurnOutcome::Completed {
            output: TurnOutput::NoOwnResult
        }
    );
}

/// meerkat refusing the tracked admission for the member's ACTUAL mode (typed,
/// pre-admission, nothing delivered) sends the delivery once on the ingress
/// lane instead, reported untracked with the typed reason, for a send and a
/// dispatch alike.
#[tokio::test]
async fn a_typed_runtime_refusal_falls_back_to_the_ingress_lane_once() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::new(true, session.clone());
    *bridge.refuses_tracked_in.lock().unwrap() = Some(meerkat_mob::MobRuntimeMode::AutonomousHost);
    let runtime = make_runtime(Some(bridge.clone()));
    let keeper = register_bound(&runtime, "keeper", &session).await;

    let sent = runtime
        .send_with_turn_ticket(&keeper, None, &content("alpha"), HandlingMode::Queue, None)
        .await
        .expect("the send is delivered on the ingress lane");
    let refused = TurnUntrackable::RefusedByRuntime {
        mode: meerkat_mob::MobRuntimeMode::AutonomousHost,
    };
    assert_eq!(sent.turn, TurnTracking::Unavailable(refused));
    assert_eq!(refused.code(), "autonomous_host");
    let dispatched = runtime
        .dispatch_with_turn_ticket(&keeper, None, &correlated("beta", "evt-1", "chat-1"))
        .await
        .expect("the dispatch is delivered on the ingress lane");
    assert_eq!(dispatched.turn, TurnTracking::Unavailable(refused));
    assert_eq!(bridge.refused.load(Ordering::SeqCst), 2);
    assert_eq!(
        bridge.ingress_admissions().len(),
        2,
        "each delivered exactly once"
    );
    assert!(bridge.tracked_admissions().is_empty());
    assert!(
        runtime
            .member_health(&keeper)
            .await
            .expect("health")
            .last_delivery_error
            .is_none(),
        "the fallback delivery succeeded; the refusal is not a delivery failure"
    );
}

/// Post-admission session resolution failing does not turn a successful turn
/// into a failure: the ticket reports the turn's own output, and the
/// resolution failure is recorded, typed, on the delivery-error channel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_resolution_failure_keeps_the_turns_own_output() {
    let session = meerkat_core::types::SessionId::new();
    let bridge = TicketBridge::build(true, true, session.clone());
    let runtime = make_runtime(Some(bridge.clone()));
    let keeper = register_bound(&runtime, "keeper", &session).await;

    let sent = runtime
        .send_with_turn_ticket(&keeper, None, &content("alpha"), HandlingMode::Queue, None)
        .await
        .expect("the admission succeeded");
    let ticket = tracked(&sent.turn);
    let health = runtime.member_health(&keeper).await.expect("health");
    let recorded = health
        .last_delivery_error
        .expect("the resolution failure is recorded");
    assert_eq!(recorded.class, DeliveryErrorClass::Completion);
    assert!(
        recorded.detail.contains(&ticket.to_string()),
        "{}",
        recorded.detail
    );

    bridge.finish(0, Ok("alpha reply"));
    assert_eq!(
        runtime
            .wait_for_turn(&keeper, ticket, Duration::from_secs(5))
            .await,
        completed("alpha reply")
    );
}

// ===========================================================================
// Real runtime, through the gateway RPC
// ===========================================================================

/// Replies to each message by the name it carries ("reply to: alpha"), and
/// holds a turn whose name is held until the test releases it. The name is
/// read off the newest non-assistant message, so a turn-driven member's user
/// message and an autonomous member's inbox notice both count.
#[derive(Clone, Default)]
struct NamedReplyClient {
    holds: Arc<std::sync::Mutex<HashMap<&'static str, tokio::sync::oneshot::Receiver<()>>>>,
    calls: Arc<AtomicUsize>,
    calls_by_name: Arc<std::sync::Mutex<HashMap<&'static str, usize>>>,
}

const NAMES: [&str; 3] = ["foreign", "alpha", "beta"];

impl NamedReplyClient {
    fn hold(&self, name: &'static str) -> tokio::sync::oneshot::Sender<()> {
        let (release, held) = tokio::sync::oneshot::channel();
        self.holds.lock().unwrap().insert(name, held);
        release
    }

    fn calls_for(&self, name: &str) -> usize {
        self.calls_by_name
            .lock()
            .unwrap()
            .get(name)
            .copied()
            .unwrap_or(0)
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
        let newest_input = request
            .messages
            .iter()
            .rev()
            .find(|message| !matches!(message, meerkat_core::Message::BlockAssistant(_)))
            .and_then(|message| serde_json::to_string(message).ok())
            .unwrap_or_default();
        let name = NAMES
            .into_iter()
            .find(|name| newest_input.contains(name))
            .unwrap_or("other");
        *self.calls_by_name.lock().unwrap().entry(name).or_default() += 1;
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

/// The live test mob: one `worker` profile declaring `runtime_mode`.
fn worker_definition(
    mob_id: &str,
    runtime_mode: meerkat_mob::MobRuntimeMode,
) -> meerkat_mob::MobDefinition {
    let mode = match runtime_mode {
        meerkat_mob::MobRuntimeMode::TurnDriven => "turn_driven",
        meerkat_mob::MobRuntimeMode::AutonomousHost => "autonomous_host",
    };
    meerkat_mob::MobDefinition::from_toml(&format!(
        r#"
[mob]
id = "{mob_id}"
[profiles.worker]
model = "gpt-5.5"
external_addressable = true
runtime_mode = "{mode}"
[profiles.worker.tools]
comms = true
"#
    ))
    .expect("mob definition")
}

/// Point MobKit's profile table (the reset-roster context, which is where it
/// resolves a member's runtime mode) at `definition`, WITHOUT respawning
/// anything: the member keeps running in the mode it was spawned with.
fn flip_profile_table(
    ctx: &meerkat_mobkit::rpc::IdentityFirstContext,
    definition: meerkat_mob::MobDefinition,
) {
    ctx.runtime
        .set_reset_roster_provider_context(Some(ctx.roster_provider.clone()), Some(definition));
}

/// A live runtime whose one identity, `keeper`, runs in `runtime_mode` (the
/// profile's declared mode; `turn_driven` is also pinned as the spec
/// override so MobKit and meerkat resolve it alike).
async fn live_runtime(
    runtime_mode: meerkat_mob::MobRuntimeMode,
    client: &NamedReplyClient,
) -> (
    meerkat_mobkit::UnifiedRuntime,
    meerkat_mobkit::rpc::IdentityFirstContext,
    tempfile::TempDir,
) {
    let (runtime, ctx, scratch, _mob_id) = live_runtime_with(
        runtime_mode,
        runtime_mode == meerkat_mob::MobRuntimeMode::TurnDriven,
        client,
    )
    .await;
    (runtime, ctx, scratch)
}

/// [`live_runtime`], choosing whether the mode is also pinned as the spec
/// override (unpinned, only the profile declares it), and returning the mob
/// id so a test can rebuild the definition with another profile mode.
async fn live_runtime_with(
    runtime_mode: meerkat_mob::MobRuntimeMode,
    pin_override: bool,
    client: &NamedReplyClient,
) -> (
    meerkat_mobkit::UnifiedRuntime,
    meerkat_mobkit::rpc::IdentityFirstContext,
    tempfile::TempDir,
    String,
) {
    let scratch = tempfile::tempdir().expect("scratch dir");
    let mob_id = format!("turn-tickets-{}", uuid::Uuid::new_v4());
    let definition = worker_definition(&mob_id, runtime_mode);
    let mut spec = make_spec("keeper");
    spec.profile = "worker".into();
    spec.runtime_mode_override = pin_override.then_some(runtime_mode);
    let roster = Arc::new(MutableRosterProvider::new(vec![spec]));
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
        runtime: identity_runtime,
        roster_provider: roster,
        topology_provider: None,
        customizer: None,
        agent_memory_provider: None,
        mob_definition: Some(runtime.mob_handle().definition().clone()),
        transcript_edit_service: None,
        compaction_floors: None,
    };
    // MobKit resolves an unpinned member's mode from this profile table, as
    // the gateway installs it at boot.
    flip_profile_table(&ctx, worker_definition(&mob_id, runtime_mode));
    (runtime, ctx, scratch, mob_id)
}

/// Wait until the identity-wide cursor passes `baseline`, then return the
/// session's latest output (the old SDK wait).
async fn cursor_wait(
    runtime: &meerkat_mobkit::UnifiedRuntime,
    ctx: &meerkat_mobkit::rpc::IdentityFirstContext,
    baseline: &Value,
) -> Option<String> {
    let baseline: meerkat_mobkit::identity_first::CompletionCursor =
        serde_json::from_value(baseline.clone()).expect("baseline");
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let inspection = rpc(
                runtime,
                ctx,
                "mobkit/inspect_identity",
                json!({"identity": "keeper"}),
            )
            .await;
            let cursor: meerkat_mobkit::identity_first::CompletionCursor =
                serde_json::from_value(inspection["completion_cursor"].clone())
                    .expect("completion cursor");
            if cursor.progress_since(baseline) == CompletionProgress::Completed {
                return inspection["output_preview"].as_str().map(str::to_string);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the cursor passes the baseline")
}

/// Wait until the session's latest output is `expected`: the wait for a
/// delivery the runtime does not track. The identity-wide cursor would also be
/// satisfied by an autonomous member's own kickoff turn completing after the
/// baseline was read, which is exactly the ambiguity turn tickets remove.
async fn wait_for_reply(
    runtime: &meerkat_mobkit::UnifiedRuntime,
    ctx: &meerkat_mobkit::rpc::IdentityFirstContext,
    expected: &str,
) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let inspection = rpc(
                runtime,
                ctx,
                "mobkit/inspect_identity",
                json!({"identity": "keeper"}),
            )
            .await;
            if inspection["output_preview"] == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the session never answered {expected:?}"));
}

/// End to end on a real runtime: an untracked dispatch to `keeper` is
/// admitted first and held; two tracked sends queue behind it. When the
/// dispatch finishes, the identity-wide cursor passes the first send's
/// baseline, and the old cursor wait (the SDKs' previous `send_and_wait`)
/// returns at once, without that send's reply. Each ticket still reports its
/// own turn's reply.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_gateway_sends_each_get_their_own_reply() {
    let client = NamedReplyClient::default();
    let release_foreign = client.hold("foreign");
    let release_alpha = client.hold("alpha");
    let (runtime, ctx, _scratch) =
        live_runtime(meerkat_mob::MobRuntimeMode::TurnDriven, &client).await;

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
    assert_eq!(
        turn_result(&runtime, &ctx, &alpha_ticket).await["state"],
        "pending"
    );

    // The dispatch finishes; alpha is still held in the model.
    release_foreign.send(()).expect("release the dispatch");
    let old_wait = cursor_wait(&runtime, &ctx, &alpha["completion_baseline"]).await;
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
    assert_eq!(alpha_result["output_status"], "text", "{alpha_result}");
    assert_eq!(alpha_result["output"], "reply to: alpha", "{alpha_result}");
    let beta_result = await_turn(&runtime, &ctx, &beta_ticket).await;
    assert_eq!(beta_result["state"], "completed", "{beta_result}");
    assert_eq!(beta_result["output"], "reply to: beta", "{beta_result}");
    assert_eq!(
        turn_result(&runtime, &ctx, &uuid::Uuid::new_v4().to_string()).await["state"],
        "unknown"
    );
    runtime.shutdown().await;
}

/// The same gateway calls against an `autonomous_host` member (meerkat's
/// default mode, which examples 001, 002 and 004 run): meerkat refuses
/// completion-bearing admission for autonomous inbox delivery, so a
/// `track_turn` send or dispatch is delivered exactly once on the ingress
/// lane, answered, and reported untracked with the typed
/// `turn_unavailable.code`, which is what lets the SDK helpers fall back
/// instead of failing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_autonomous_member_through_the_gateway_is_delivered_once_untracked() {
    let client = NamedReplyClient::default();
    let (runtime, ctx, _scratch) =
        live_runtime(meerkat_mob::MobRuntimeMode::AutonomousHost, &client).await;

    let sent = rpc(
        &runtime,
        &ctx,
        "mobkit/send",
        json!({"identity": "keeper", "content": "alpha", "track_turn": true}),
    )
    .await;
    assert!(sent["turn"].is_null(), "{sent}");
    assert_eq!(
        sent["turn_unavailable"]["code"], "autonomous_host",
        "{sent}"
    );
    wait_for_reply(&runtime, &ctx, "reply to: alpha").await;

    let dispatched = rpc(
        &runtime,
        &ctx,
        "mobkit/dispatch",
        json!({
            "identity": "keeper",
            "dispatch_input": {"content": "beta", "origin": "system"},
            "track_turn": true,
        }),
    )
    .await;
    assert!(dispatched["turn"].is_null(), "{dispatched}");
    assert_eq!(
        dispatched["turn_unavailable"]["code"], "autonomous_host",
        "{dispatched}"
    );
    wait_for_reply(&runtime, &ctx, "reply to: beta").await;

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        client.calls_for("alpha"),
        1,
        "alpha was delivered exactly once"
    );
    assert_eq!(
        client.calls_for("beta"),
        1,
        "beta was delivered exactly once"
    );
    runtime.shutdown().await;
}

/// meerkat's own deduplication, end to end on a turn-driven member. An
/// idempotent re-dispatch of a tracked original names the original's ticket
/// and reads its reply, without a second turn. A tracked re-dispatch of an
/// UNTRACKED original (which the registry cannot name) is deduplicated by
/// meerkat onto the ended original: it completes with `no_own_result`, the
/// typed "consumed without a run result of its own", never an empty output
/// and never the original's reply.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_live_redispatch_reads_the_original_reply_or_no_own_result() {
    let client = NamedReplyClient::default();
    let (runtime, ctx, _scratch) =
        live_runtime(meerkat_mob::MobRuntimeMode::TurnDriven, &client).await;
    let dispatch = |content: &str, key: &str, track: bool| {
        json!({
            "identity": "keeper",
            "dispatch_input": {
                "content": content,
                "origin": "connector",
                "idempotency_key": key,
                "correlation_id": format!("chat/{key}"),
            },
            "track_turn": track,
        })
    };

    let original = rpc(
        &runtime,
        &ctx,
        "mobkit/dispatch",
        dispatch("alpha", "evt-1", true),
    )
    .await;
    let ticket = original["turn"]["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("the original is tracked: {original}"))
        .to_string();
    let first = await_turn(&runtime, &ctx, &ticket).await;
    assert_eq!(first["output"], "reply to: alpha", "{first}");
    let retry = rpc(
        &runtime,
        &ctx,
        "mobkit/dispatch",
        dispatch("alpha", "evt-1", true),
    )
    .await;
    assert_eq!(retry["turn"]["ticket"], ticket.as_str(), "{retry}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let again = turn_result(&runtime, &ctx, &ticket).await;
    assert_eq!(again["output"], "reply to: alpha", "{again}");
    assert_eq!(client.calls_for("alpha"), 1, "the retry ran no second turn");

    let untracked = rpc(
        &runtime,
        &ctx,
        "mobkit/dispatch",
        dispatch("beta", "evt-2", false),
    )
    .await;
    assert!(untracked.get("turn").is_none(), "{untracked}");
    wait_for_reply(&runtime, &ctx, "reply to: beta").await;
    let retry = rpc(
        &runtime,
        &ctx,
        "mobkit/dispatch",
        dispatch("beta", "evt-2", true),
    )
    .await;
    let retry_ticket = retry["turn"]["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("the retry is tracked: {retry}"))
        .to_string();
    let deduplicated = await_turn(&runtime, &ctx, &retry_ticket).await;
    assert_eq!(deduplicated["state"], "completed", "{deduplicated}");
    assert_eq!(
        deduplicated["output_status"], "no_own_result",
        "{deduplicated}"
    );
    assert!(deduplicated["output"].is_null(), "{deduplicated}");
    assert_eq!(
        client.calls_for("beta"),
        1,
        "meerkat deduplicated the retry"
    );
    runtime.shutdown().await;
}

/// The profile's `runtime_mode` flipped WITHOUT a respawn, both ways, on a
/// real mob. Spawned `autonomous_host`, then the profile says `turn_driven`:
/// MobKit now resolves the member as trackable, meerkat still runs it
/// autonomous and refuses the tracked admission (typed, pre-admission), and
/// the send falls back to the ingress lane: delivered exactly once, answered,
/// and reported `turn_unavailable.code = autonomous_host` with the refusal's
/// reason. Spawned `turn_driven`, then the profile says `autonomous_host`:
/// MobKit resolves the member as untrackable and delivers once on the ingress
/// lane without attempting the tracked admission at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_profile_mode_flipped_without_respawn_still_delivers_once() {
    use meerkat_mob::MobRuntimeMode::{AutonomousHost, TurnDriven};

    // Spawned autonomous_host; profile flipped to turn_driven.
    let client = NamedReplyClient::default();
    let (runtime, ctx, _scratch, mob_id) = live_runtime_with(AutonomousHost, false, &client).await;
    let first = rpc(
        &runtime,
        &ctx,
        "mobkit/send",
        json!({"identity": "keeper", "content": "foreign", "track_turn": true}),
    )
    .await;
    assert_eq!(
        first["turn_unavailable"]["code"], "autonomous_host",
        "{first}"
    );
    wait_for_reply(&runtime, &ctx, "reply to: foreign").await;
    flip_profile_table(&ctx, worker_definition(&mob_id, TurnDriven));
    let sent = rpc(
        &runtime,
        &ctx,
        "mobkit/send",
        json!({"identity": "keeper", "content": "alpha", "track_turn": true}),
    )
    .await;
    assert!(sent["turn"].is_null(), "{sent}");
    assert_eq!(
        sent["turn_unavailable"]["code"], "autonomous_host",
        "{sent}"
    );
    assert!(
        sent["turn_unavailable"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("refused") && reason.contains("respawn")),
        "{sent}"
    );
    wait_for_reply(&runtime, &ctx, "reply to: alpha").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        client.calls_for("alpha"),
        1,
        "alpha was delivered exactly once"
    );
    runtime.shutdown().await;

    // Spawned turn_driven; profile flipped to autonomous_host.
    let client = NamedReplyClient::default();
    let (runtime, ctx, _scratch, mob_id) = live_runtime_with(TurnDriven, false, &client).await;
    let tracked = rpc(
        &runtime,
        &ctx,
        "mobkit/send",
        json!({"identity": "keeper", "content": "foreign", "track_turn": true}),
    )
    .await;
    let ticket = tracked["turn"]["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("a turn_driven member is tracked: {tracked}"))
        .to_string();
    assert_eq!(
        await_turn(&runtime, &ctx, &ticket).await["output"],
        "reply to: foreign"
    );
    flip_profile_table(&ctx, worker_definition(&mob_id, AutonomousHost));
    let sent = rpc(
        &runtime,
        &ctx,
        "mobkit/send",
        json!({"identity": "keeper", "content": "beta", "track_turn": true}),
    )
    .await;
    assert!(sent["turn"].is_null(), "{sent}");
    assert_eq!(
        sent["turn_unavailable"]["code"], "autonomous_host",
        "{sent}"
    );
    wait_for_reply(&runtime, &ctx, "reply to: beta").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        client.calls_for("beta"),
        1,
        "beta was delivered exactly once"
    );
    runtime.shutdown().await;
}
