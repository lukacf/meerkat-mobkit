//! Create-time capture of a member session's live event stream.
//!
//! Meerkat's per-session event stream is a broadcast with no replay: a
//! subscriber sees only events published after it subscribed. The console
//! agent-event forwarder attaches to a member on its own reconcile cadence,
//! so after a restore the first run of a revived member could start (and be
//! dropped) before the forwarder's next attempt.
//!
//! The tap closes that window at the witness-bearing session creates. Those
//! run while the provisioner holds the session's turn-finalization boundary,
//! and every later run start needs that boundary, so for a create whose
//! initial turn is [`InitialTurnPolicy::Defer`] a subscription taken right
//! after create returns predates the actor's first `run_started`. An eager
//! ([`InitialTurnPolicy::RunImmediately`]) create runs its first turn inside
//! the create itself, so the tap declines it loudly instead of adopting a
//! stream that already lost that run; stock mob member creates always defer.
//!
//! Meerkat's session broadcast keeps only its last 256 envelopes per
//! receiver, so a receiver left unread until adoption would lose exactly the
//! head it exists for. Each capture therefore starts a pump that drains the
//! receiver into an owned queue at once. Adoption stops the pump and hands
//! the forwarder the drained queue followed by the same receiver, so the
//! adopted stream is gap-free from the actor's first event. The queue is
//! bounded by [`CAPTURE_QUEUE_CAPACITY`]; a capture that outgrows it before
//! adoption is discarded with a warning and the forwarder keeps its ordinary
//! subscription path.
//!
//! Captures are keyed by `SessionId` and fenced by the exact actor
//! incarnation's [`LiveSessionActorWitness`]: registries revoke a witness
//! before removing or replacing its actor, so a capture whose witness is no
//! longer live names a dead actor and is dropped, never adopted.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use futures::StreamExt;
use meerkat_core::AgentEvent;
use meerkat_core::comms::EventStream;
use meerkat_core::event::EventEnvelope;
use meerkat_core::service::InitialTurnPolicy;
use meerkat_core::types::SessionId;
use meerkat_mob::MobSessionService;
use meerkat_session::{LiveSessionActorWitness, LiveSessionActorWitnessSlot};
use tokio::sync::{oneshot, watch};

/// Most envelopes one capture holds while it waits for adoption. The console
/// forwarder adopts on the tap's change signal, so a capture normally holds a
/// handful; the bound only covers adoption stalled behind a restore (the
/// forwarder blocked on a full console channel, or queued behind other
/// members' subscriptions) with a long streamed run in flight.
pub(crate) const CAPTURE_QUEUE_CAPACITY: usize = 4096;

type Envelope = EventEnvelope<AgentEvent>;

/// Shared tap state. Cloning shares the same captures.
///
/// Unarmed taps capture nothing: only a runtime that also runs the console
/// forwarder (which adopts captures) arms it, so MobRuntime-only embedders
/// never buffer events nobody reads.
#[derive(Clone)]
pub(crate) struct LiveSessionEventTap {
    state: Arc<TapState>,
}

struct TapState {
    armed: AtomicBool,
    capacity: usize,
    captures: Mutex<HashMap<SessionId, Capture>>,
    /// Bumped on every new capture so the forwarder can reconcile at once
    /// instead of waiting for its next scheduled attempt.
    changes: watch::Sender<u64>,
}

struct Capture {
    actor: LiveSessionActorWitness,
    /// Stops the pump for adoption. Dropping it (capture superseded, swept,
    /// or discarded) ends the pump and releases its queue.
    stop: oneshot::Sender<()>,
    pump: tokio::task::JoinHandle<PumpOutcome>,
}

enum PumpOutcome {
    /// Stopped for adoption: everything drained so far, then the receiver.
    Stopped {
        drained: VecDeque<Envelope>,
        stream: EventStream,
    },
    /// The actor's stream ended (every sender dropped) before adoption.
    Ended { drained: VecDeque<Envelope> },
    /// More envelopes arrived than the queue holds; the capture is void.
    Overflowed,
    /// The capture was dropped before adoption.
    Abandoned,
}

/// An adopted capture: the actor incarnation it belongs to and its stream,
/// gap-free from that actor's first event.
pub(crate) struct AdoptedCapture {
    pub(crate) actor: LiveSessionActorWitness,
    pub(crate) stream: EventStream,
}

impl Default for LiveSessionEventTap {
    fn default() -> Self {
        Self::with_capacity(CAPTURE_QUEUE_CAPACITY)
    }
}

impl LiveSessionEventTap {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            state: Arc::new(TapState {
                armed: AtomicBool::new(false),
                capacity,
                captures: Mutex::new(HashMap::new()),
                changes: watch::channel(0).0,
            }),
        }
    }

    /// Start capturing on subsequent witness-bearing creates.
    pub(crate) fn arm(&self) {
        self.state.armed.store(true, Ordering::Release);
    }

    /// Subscribe to the live event stream of the actor `slot` names and
    /// start draining it into the capture's owned queue.
    ///
    /// Called by the base session-service wrapper right after a
    /// witness-bearing create returned, with the create's (post-hook)
    /// initial-turn policy. Never fails the create: an unarmed tap, an eager
    /// create, an unpublished witness, or a subscribe error leaves the
    /// forwarder on its ordinary subscription path. Takes only the inner
    /// service's session read lock, never the turn-finalization boundary.
    pub(crate) async fn capture(
        &self,
        inner: &dyn MobSessionService,
        slot: &LiveSessionActorWitnessSlot,
        id: &SessionId,
        initial_turn: InitialTurnPolicy,
    ) {
        if !self.state.armed.load(Ordering::Acquire) {
            return;
        }
        match initial_turn {
            InitialTurnPolicy::Defer => {}
            InitialTurnPolicy::RunImmediately => {
                // The first run already happened inside the create; a stream
                // opened now would present its remainder as complete.
                tracing::warn!(
                    session_id = %id,
                    "live session event tap: eager create ran its first turn before capture; \
                     the console forwarder keeps its ordinary subscription path"
                );
                return;
            }
        }
        let Some(actor) = slot.witness() else {
            return;
        };
        if actor.session_id() != id {
            return;
        }
        let stream = match MobSessionService::subscribe_session_events(inner, id).await {
            Ok(stream) => stream,
            Err(error) => {
                tracing::debug!(
                    session_id = %id,
                    error = %error,
                    "live session event tap: create-time subscription unavailable; \
                     the console forwarder keeps its ordinary subscription path"
                );
                return;
            }
        };
        // The subscription is only meaningful for the actor the slot names:
        // if that incarnation was revoked while subscribing, the stream may
        // belong to a replacement actor and must not be attributed to it.
        if !actor.is_live() {
            return;
        }
        let (stop, stop_rx) = oneshot::channel();
        let pump = tokio::spawn(pump(id.clone(), stream, stop_rx, self.state.capacity));
        {
            let mut captures = self.lock_captures();
            captures.retain(|_, capture| capture.actor.is_live());
            captures.insert(id.clone(), Capture { actor, stop, pump });
        }
        self.state
            .changes
            .send_modify(|version| *version = version.wrapping_add(1));
    }

    /// Remove the capture for `id` and adopt it, returning its stream only
    /// while the captured actor incarnation is still live and its queue did
    /// not overflow.
    pub(crate) async fn take_live(&self, id: &SessionId) -> Option<AdoptedCapture> {
        let Capture { actor, stop, pump } = self.lock_captures().remove(id)?;
        if !actor.is_live() {
            return None;
        }
        // A pump that already ended dropped its receiver; its outcome says why.
        let _ = stop.send(());
        match pump.await {
            Ok(PumpOutcome::Stopped { drained, stream }) => Some(AdoptedCapture {
                actor,
                stream: Box::pin(futures::stream::iter(drained).chain(stream)),
            }),
            Ok(PumpOutcome::Ended { drained }) => Some(AdoptedCapture {
                actor,
                stream: Box::pin(futures::stream::iter(drained)),
            }),
            Ok(PumpOutcome::Overflowed | PumpOutcome::Abandoned) | Err(_) => None,
        }
    }

    /// Whether any capture is waiting to be adopted. Lets the forwarder skip
    /// the per-member session resolution when there is nothing to adopt.
    pub(crate) fn holds_captures(&self) -> bool {
        !self.lock_captures().is_empty()
    }

    /// Drop captures whose actor incarnation was revoked, ending their pumps.
    pub(crate) fn sweep(&self) {
        self.lock_captures()
            .retain(|_, capture| capture.actor.is_live());
    }

    /// Change notifications, one bump per new capture.
    pub(crate) fn changes(&self) -> watch::Receiver<u64> {
        self.state.changes.subscribe()
    }

    /// Whether a live capture for `id` is waiting to be adopted.
    #[cfg(test)]
    pub(crate) fn holds_live(&self, id: &SessionId) -> bool {
        self.lock_captures()
            .get(id)
            .is_some_and(|capture| capture.actor.is_live())
    }

    fn lock_captures(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, Capture>> {
        self.state
            .captures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Drain a captured receiver into an owned queue until adoption stops it,
/// the capture is dropped, the actor's stream ends, or the queue overflows.
async fn pump(
    session_id: SessionId,
    mut stream: EventStream,
    mut stop: oneshot::Receiver<()>,
    capacity: usize,
) -> PumpOutcome {
    let mut drained = VecDeque::new();
    loop {
        tokio::select! {
            biased;
            signal = &mut stop => {
                return match signal {
                    Ok(()) => PumpOutcome::Stopped { drained, stream },
                    Err(_) => PumpOutcome::Abandoned,
                };
            }
            next = stream.next() => match next {
                Some(envelope) => {
                    if drained.len() >= capacity {
                        tracing::warn!(
                            session_id = %session_id,
                            capacity,
                            "live session event tap: capture outgrew its queue before the \
                             console forwarder adopted it; discarding it, the forwarder keeps \
                             its ordinary subscription path"
                        );
                        return PumpOutcome::Overflowed;
                    }
                    drained.push_back(envelope);
                }
                None => return PumpOutcome::Ended { drained },
            },
        }
    }
}

/// Session fixtures shared with the console forwarder's tests.
#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) mod test_support {
    use std::sync::Arc;

    use meerkat_client::LlmEvent;
    use meerkat_core::service::{
        CreateSessionRequest, DeferredPromptPolicy, InitialTurnPolicy, SessionBuildOptions,
    };
    use meerkat_core::types::SessionId;
    use meerkat_mob::MobSessionService;
    use meerkat_session::LiveSessionActorWitnessSlot;

    use crate::MobBootstrapSpec;

    pub(crate) type RawService =
        meerkat_session::EphemeralSessionService<meerkat::FactoryAgentBuilder>;

    pub(crate) struct Fixture {
        pub(crate) raw: Arc<RawService>,
        pub(crate) spec: MobBootstrapSpec,
        _dir: tempfile::TempDir,
    }

    pub(crate) fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("temp dir");
        let raw = Arc::new(meerkat::build_ephemeral_service(
            meerkat::AgentFactory::new(dir.path()),
            meerkat::Config::default(),
            16,
        ));
        let definition = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"event-tap\"\n")
            .expect("mob definition");
        let spec = MobBootstrapSpec::new(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            raw.clone() as Arc<dyn MobSessionService>,
        );
        Fixture {
            raw,
            spec,
            _dir: dir,
        }
    }

    pub(crate) fn create_request(
        client: meerkat_client::TestClient,
        initial_turn: InitialTurnPolicy,
    ) -> CreateSessionRequest {
        CreateSessionRequest {
            model: "gpt-5.5".to_string(),
            prompt: meerkat_core::ContentInput::Text("probe".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn,
            deferred_prompt_policy: DeferredPromptPolicy::Discard,
            build: Some(SessionBuildOptions {
                llm_client_override: Some(meerkat::encode_llm_client_override_for_service(
                    Arc::new(client),
                )),
                ..Default::default()
            }),
            labels: None,
            injected_context: Vec::new(),
        }
    }

    pub(crate) fn deferred_request() -> CreateSessionRequest {
        create_request(
            meerkat_client::TestClient::default(),
            InitialTurnPolicy::Defer,
        )
    }

    /// A scripted client whose every turn streams `deltas` text deltas, with
    /// the same host-declared accounting `TestClient::default()` synthesizes.
    pub(crate) fn chatty_client(deltas: usize) -> meerkat_client::TestClient {
        let mut events: Vec<LlmEvent> = (0..deltas)
            .map(|index| LlmEvent::TextDelta {
                delta: format!("d{index} "),
                meta: None,
            })
            .collect();
        events.push(LlmEvent::UsageUpdate {
            usage: meerkat_core::TurnUsage::host_declared(
                meerkat_core::Provider::Other,
                "gpt-5.5",
                meerkat_core::Usage::default(),
            ),
        });
        events.push(LlmEvent::Done {
            outcome: meerkat_client::LlmDoneOutcome::Success {
                stop_reason: meerkat_core::StopReason::EndTurn,
            },
        });
        meerkat_client::TestClient::new(events)
    }

    /// Create one session through `service` (a spec's base wrapper, or a
    /// service holding it), the same witness-bearing create every
    /// runtime-backed member materialization lowers to.
    pub(crate) async fn create_through(
        service: &Arc<dyn MobSessionService>,
        request: CreateSessionRequest,
    ) -> SessionId {
        let slot = LiveSessionActorWitnessSlot::default();
        service
            .create_session_with_actor_witness_under_runtime_turn_boundary(request, None, &slot)
            .await
            .expect("witness-bearing create")
            .session_id
    }

    pub(crate) async fn create_through_spec(fixture: &Fixture) -> SessionId {
        create_through(&fixture.spec.session_service, deferred_request()).await
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    use futures::FutureExt;
    use meerkat_core::service::{SessionService, StartTurnRequest, StartTurnRuntimeSemantics};

    use super::test_support::*;
    use crate::MobBootstrapSpec;

    async fn run_turn(raw: &RawService, id: &SessionId, prompt: &str) {
        raw.start_turn(
            id,
            StartTurnRequest {
                prompt: meerkat_core::ContentInput::Text(prompt.to_string()),
                injected_context: Vec::new(),
                system_prompt: None,
                event_tx: None,
                runtime: StartTurnRuntimeSemantics::default(),
            },
        )
        .await
        .expect("turn completes");
    }

    fn drain_ready(stream: &mut EventStream) -> Vec<Envelope> {
        let mut events = Vec::new();
        while let Some(Some(event)) = stream.next().now_or_never() {
            events.push(event);
        }
        events
    }

    /// The capture predates the actor's first run: a turn that runs to
    /// completion before any consumer exists is still delivered in full,
    /// from `run_started` seq 1, while a subscription opened afterwards (the
    /// pre-fix forwarder's position) receives nothing because the session
    /// stream has no replay.
    #[tokio::test]
    async fn tap_captures_first_run_emitted_before_any_consumer() {
        let fixture = fixture();
        let tap = fixture.spec.live_session_event_tap();
        tap.arm();
        let mut changes = tap.changes();

        let id = create_through_spec(&fixture).await;
        assert!(
            changes.has_changed().expect("tap alive"),
            "a capture notifies the forwarder"
        );
        changes.mark_unchanged();

        run_turn(&fixture.raw, &id, "probe").await;

        let mut late = MobSessionService::subscribe_session_events(fixture.raw.as_ref(), &id)
            .await
            .expect("late subscription");

        let mut captured = tap.take_live(&id).await.expect("live capture").stream;
        let events = drain_ready(&mut captured);
        let first = events.first().expect("captured events");
        assert!(
            matches!(first.payload, AgentEvent::RunStarted { .. }),
            "the capture begins at the run's own start, got {:?}",
            first.payload
        );
        assert_eq!(first.seq, 1, "run_started is the actor's first event");
        let completed = events
            .iter()
            .position(|event| matches!(event.payload, AgentEvent::RunCompleted { .. }))
            .expect("the capture reaches the run's terminal");
        for (index, event) in events[..=completed].iter().enumerate() {
            assert_eq!(
                event.seq,
                index as u64 + 1,
                "captured sequence is contiguous through run_completed"
            );
        }
        assert!(
            late.next().now_or_never().is_none(),
            "a subscription opened after the run sees nothing: the stream has no replay"
        );
        assert!(
            tap.take_live(&id).await.is_none(),
            "adoption consumes the capture"
        );
    }

    /// Meerkat's session broadcast keeps 256 envelopes per receiver. A
    /// capture adopted only after far more than that were published still
    /// starts at `run_started` seq 1 and carries every envelope, with no
    /// `StreamTruncated` marker: the pump moved them into the capture's own
    /// queue as they arrived.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tap_keeps_more_than_the_session_broadcast_holds_before_adoption() {
        let fixture = fixture();
        let tap = fixture.spec.live_session_event_tap();
        tap.arm();
        // Each turn stays well inside one broadcast window, so only the wait
        // for adoption, not a single burst, exceeds the session channel.
        let id = create_through(
            &fixture.spec.session_service,
            create_request(chatty_client(60), InitialTurnPolicy::Defer),
        )
        .await;
        const TURNS: usize = 10;
        for turn in 0..TURNS {
            run_turn(&fixture.raw, &id, &format!("probe-{turn}")).await;
        }

        let mut captured = tap.take_live(&id).await.expect("live capture").stream;
        let events = drain_ready(&mut captured);
        assert!(
            events.len() > 2 * 256,
            "the test publishes well past the 256-envelope session broadcast before adoption, \
             got {}",
            events.len()
        );
        assert!(
            events
                .iter()
                .all(|event| !matches!(event.payload, AgentEvent::StreamTruncated { .. })),
            "nothing was dropped between capture and adoption"
        );
        assert!(matches!(
            events.first().map(|event| &event.payload),
            Some(AgentEvent::RunStarted { .. })
        ));
        for (index, event) in events.iter().enumerate() {
            assert_eq!(
                event.seq,
                index as u64 + 1,
                "contiguous from the first event"
            );
        }
        let completed = events
            .iter()
            .filter(|event| matches!(event.payload, AgentEvent::RunCompleted { .. }))
            .count();
        assert_eq!(completed, TURNS, "every pre-adoption run is complete");
    }

    /// A capture that outgrows its queue before adoption is void: adopting
    /// it would present a stream with a hole as gap-free.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tap_discards_a_capture_that_outgrew_its_queue() {
        let fixture = fixture();
        let tap = LiveSessionEventTap::with_capacity(4);
        tap.arm();
        let slot = LiveSessionActorWitnessSlot::default();
        let raw = fixture.raw.clone() as Arc<dyn MobSessionService>;
        let id = raw
            .create_session_with_actor_witness_under_runtime_turn_boundary(
                deferred_request(),
                None,
                &slot,
            )
            .await
            .expect("witness-bearing create")
            .session_id;
        tap.capture(raw.as_ref(), &slot, &id, InitialTurnPolicy::Defer)
            .await;
        assert!(tap.holds_live(&id));

        run_turn(&fixture.raw, &id, "probe").await;

        assert!(
            tap.take_live(&id).await.is_none(),
            "an overflowed capture is never adopted"
        );
    }

    /// An eager create runs its first turn inside the create, before any
    /// capture could subscribe, so the tap declines it rather than hand the
    /// forwarder a stream that already lost that run.
    #[tokio::test]
    async fn tap_declines_an_eager_create() {
        let fixture = fixture();
        let tap = fixture.spec.live_session_event_tap();
        tap.arm();
        let changes = tap.changes();

        let id = create_through(
            &fixture.spec.session_service,
            create_request(
                meerkat_client::TestClient::default(),
                InitialTurnPolicy::RunImmediately,
            ),
        )
        .await;

        assert!(tap.take_live(&id).await.is_none());
        assert!(!changes.has_changed().expect("tap alive"));
    }

    #[tokio::test]
    async fn tap_drops_revoked_actor() {
        let fixture = fixture();
        let tap = fixture.spec.live_session_event_tap();
        tap.arm();

        let id = create_through_spec(&fixture).await;
        assert!(tap.holds_live(&id));
        fixture
            .raw
            .discard_live_session(&id)
            .await
            .expect("discard live session");

        assert!(
            tap.take_live(&id).await.is_none(),
            "a capture of a revoked actor incarnation is never adopted"
        );
    }

    #[tokio::test]
    async fn sweep_releases_revoked_captures() {
        let fixture = fixture();
        let tap = fixture.spec.live_session_event_tap();
        tap.arm();

        let id = create_through_spec(&fixture).await;
        fixture
            .raw
            .discard_live_session(&id)
            .await
            .expect("discard live session");
        assert!(tap.holds_captures(), "nothing swept it yet");

        tap.sweep();
        assert!(!tap.holds_captures());
    }

    #[tokio::test]
    async fn unarmed_tap_captures_nothing() {
        let fixture = fixture();
        let tap = fixture.spec.live_session_event_tap();
        let changes = tap.changes();

        let id = create_through_spec(&fixture).await;

        assert!(tap.take_live(&id).await.is_none());
        assert!(!changes.has_changed().expect("tap alive"));
    }

    /// Child mobs are built on the agent mob tools' session service. A stock
    /// constructor must hand those tools the spec's final service, carrying
    /// the tap, or child-mob members keep the lossy late subscription.
    #[tokio::test]
    async fn agent_mob_tools_session_service_feeds_the_spec_tap() {
        let dir = tempfile::tempdir().expect("temp dir");
        let definition =
            meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"event-tap-child-mobs\"\n")
                .expect("mob definition");
        let spec = MobBootstrapSpec::ephemeral_runtime_backed_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            dir.path().to_path_buf(),
            16,
            None,
            "test session store",
            None,
            None,
            None,
            None,
            crate::mob_handle_runtime::CapabilityFlags::default(),
            None,
            None,
        );
        let tap = spec.live_session_event_tap();
        tap.arm();
        let child_mob_service = spec
            .agent_mob_mcp_state
            .as_ref()
            .expect("stock constructor installs agent mob tools")
            .session_service();

        let id = create_through(&child_mob_service, deferred_request()).await;

        assert!(
            tap.holds_live(&id),
            "a create on the child-mob session service is captured by the spec's tap"
        );
    }
}
