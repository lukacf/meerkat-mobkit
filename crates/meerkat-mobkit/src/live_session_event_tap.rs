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
//! The policy is read from the request the session service actually
//! executes: the tap sits on the innermost MobKit session-service layer (see
//! `MobBootstrapSpec`), below every pre-build hook that could rewrite it.
//!
//! Meerkat's session broadcast keeps only its last 256 envelopes per
//! receiver, so a receiver left unread until adoption would lose exactly the
//! head it exists for. Each capture therefore starts a pump that moves the
//! receiver's envelopes into an owned queue of [`CAPTURE_QUEUE_CAPACITY`] as
//! they arrive; adoption hands the forwarder that queue, which the pump keeps
//! feeding. The bound is honest rather than silent: a full queue stops the
//! pump reading, so meerkat's own lag accounting takes over and the stream
//! carries meerkat's typed `StreamTruncated(StreamLagged { dropped })` marker
//! at the gap once it drains, exactly like any other lagging subscriber. The
//! queued prefix, including the actor's first `run_started`, is kept.
//!
//! Captures are keyed by `SessionId` and fenced by the exact actor
//! incarnation's [`LiveSessionActorWitness`]. A capture's receiver belongs to
//! that incarnation's broadcast for its whole life, and registries revoke a
//! witness before (or, on the fatal path, right after) removing or replacing
//! its actor. The witness is checked after subscribing, so a revocation that
//! raced the subscription never stores the stream, and again at adoption, so
//! a capture whose actor was revoked since is dropped, never attributed.

use std::collections::HashMap;
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
use tokio::sync::{mpsc, watch};

/// Most envelopes one capture queues for its consumer. The console forwarder
/// adopts on the tap's change signal, so a capture normally holds a handful
/// before adoption; the bound covers adoption stalled behind a restore (the
/// forwarder blocked on a full console channel, or queued behind other
/// members' subscriptions) with a long streamed run in flight. Beyond it the
/// session broadcast's own 256-envelope window applies and meerkat reports
/// the overflow as a typed `StreamTruncated` marker.
pub(crate) const CAPTURE_QUEUE_CAPACITY: usize = 4096;

type Envelope = EventEnvelope<AgentEvent>;

/// Shared tap state. Cloning shares the same captures.
///
/// Unarmed taps capture nothing: only a runtime that also runs the console
/// forwarder (which adopts captures) arms it, so MobRuntime-only embedders
/// never buffer events nobody reads.
///
/// A tap carries two independent lanes over the same create-time captures:
/// the console forwarder's (this handle) and the identity health monitor's
/// ([`Self::identity_health_lane`]). Each lane has its own captures, arm flag
/// and change signal, so each consumer adopts its own stream of every
/// actor's events from its first one, and neither consumer's adoption or
/// disarm affects the other.
#[derive(Clone)]
pub(crate) struct LiveSessionEventTap {
    state: Arc<TapState>,
    /// The identity health monitor's lane, present on the root tap only; a
    /// lane handle (from [`Self::identity_health_lane`]) has none.
    health: Option<Arc<TapState>>,
}

struct TapState {
    armed: AtomicBool,
    capacity: usize,
    captures: Mutex<HashMap<SessionId, Capture>>,
    /// Bumped on every new capture so the forwarder can reconcile at once
    /// instead of waiting for its next scheduled attempt.
    changes: watch::Sender<u64>,
}

impl TapState {
    fn new(capacity: usize) -> Self {
        Self {
            armed: AtomicBool::new(false),
            capacity,
            captures: Mutex::new(HashMap::new()),
            changes: watch::channel(0).0,
        }
    }
}

/// A capture waiting for adoption. Dropping it (superseded, swept, or
/// revoked at adoption) closes its queue, which ends the pump.
struct Capture {
    actor: LiveSessionActorWitness,
    queue: mpsc::Receiver<Envelope>,
    progress: watch::Receiver<CaptureProgress>,
}

/// What a capture's pump has done so far, observable without consuming the
/// queue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CaptureProgress {
    /// Envelopes the pump moved into the queue since the capture started.
    pub(crate) queued: u64,
    /// The queue is full and the pump stopped reading: the session broadcast
    /// is retaining (and past its window, dropping with a typed marker) what
    /// arrives until the consumer drains the queue.
    pub(crate) saturated: bool,
}

/// An adopted capture: the actor incarnation it belongs to and its stream,
/// which starts at that actor's first event.
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
            state: Arc::new(TapState::new(capacity)),
            health: Some(Arc::new(TapState::new(capacity))),
        }
    }

    /// The identity health monitor's lane: a tap handle over the same
    /// create-time captures with its own queues, arm flag and change signal.
    /// A lane handle captures only into its own lane.
    pub(crate) fn identity_health_lane(&self) -> Self {
        Self {
            state: Arc::clone(self.health.as_ref().unwrap_or(&self.state)),
            health: None,
        }
    }

    /// Start capturing on subsequent witness-bearing creates.
    pub(crate) fn arm(&self) {
        self.state.armed.store(true, Ordering::Release);
    }

    /// Stop capturing and release every held capture, ending their pumps.
    /// The console forwarder does this when it exits: nothing would adopt
    /// captures any more.
    pub(crate) fn disarm(&self) {
        self.state.armed.store(false, Ordering::Release);
        self.lock_captures().clear();
    }

    /// Subscribe to the live event stream of the actor `slot` names and
    /// start pumping it into the capture's owned queue.
    ///
    /// Called by the innermost session-service wrapper right after a
    /// witness-bearing create returned, with the initial-turn policy of the
    /// request the inner service executed. The caller still holds the
    /// session's turn-finalization boundary, which the provisioning contract
    /// requires for creating a successor actor, so a stream obtained while
    /// the witness is still live afterwards belongs to that witness's actor.
    ///
    /// Never fails the create: an unarmed tap, an eager create, an
    /// unpublished witness, a subscribe error, or a revocation that raced the
    /// subscription leaves the forwarder on its ordinary subscription path.
    /// Takes only the inner service's session read lock, never the
    /// turn-finalization boundary.
    pub(crate) async fn capture(
        &self,
        inner: &dyn MobSessionService,
        slot: &LiveSessionActorWitnessSlot,
        id: &SessionId,
        initial_turn: InitialTurnPolicy,
    ) {
        self.capture_lane(inner, slot, id, initial_turn).await;
        if let Some(health) = self.health.as_ref() {
            let lane = Self {
                state: Arc::clone(health),
                health: None,
            };
            lane.capture_lane(inner, slot, id, initial_turn).await;
        }
    }

    /// [`Self::capture`] into this handle's own lane only.
    async fn capture_lane(
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
        // Fail closed inside `install`: if the incarnation was revoked while
        // subscribing, the stream cannot be proven to be its own.
        self.install(id, actor, stream);
    }

    /// Store a create-time subscription as the capture for `id`, unless its
    /// actor incarnation was revoked by now: a revocation that raced the
    /// subscription fails closed, and the stream is dropped unread.
    fn install(&self, id: &SessionId, actor: LiveSessionActorWitness, stream: EventStream) -> bool {
        if !actor.is_live() {
            return false;
        }
        let (queue_tx, queue) = mpsc::channel(self.state.capacity.max(1));
        let (progress_tx, progress) = watch::channel(CaptureProgress::default());
        tokio::spawn(pump(id.clone(), stream, queue_tx, progress_tx));
        {
            let mut captures = self.lock_captures();
            captures.retain(|_, capture| capture.actor.is_live());
            captures.insert(
                id.clone(),
                Capture {
                    actor,
                    queue,
                    progress,
                },
            );
        }
        self.state
            .changes
            .send_modify(|version| *version = version.wrapping_add(1));
        true
    }

    /// Whether a capture of a still-live actor incarnation is waiting for
    /// `id`. Cheap; lets the forwarder skip binding revalidation for members
    /// with nothing to adopt.
    pub(crate) fn holds_live(&self, id: &SessionId) -> bool {
        self.lock_captures()
            .get(id)
            .is_some_and(|capture| capture.actor.is_live())
    }

    /// Remove the capture for `id` and adopt it. Returns its stream only
    /// while the captured actor incarnation is still live at this instant;
    /// a capture whose actor was revoked is dropped, never adopted.
    pub(crate) fn take_live(&self, id: &SessionId) -> Option<AdoptedCapture> {
        let Capture {
            actor,
            queue,
            progress,
        } = self.lock_captures().remove(id)?;
        if !actor.is_live() {
            return None;
        }
        let progress = *progress.borrow();
        tracing::debug!(
            session_id = %id,
            queued = progress.queued,
            saturated = progress.saturated,
            "live session event tap: capture adopted"
        );
        let stream = futures::stream::unfold(queue, |mut queue| async move {
            let envelope = queue.recv().await?;
            Some((envelope, queue))
        });
        Some(AdoptedCapture {
            actor,
            stream: Box::pin(stream),
        })
    }

    /// Whether any capture is waiting to be adopted. Lets the forwarder skip
    /// the per-member session resolution when there is nothing to adopt.
    pub(crate) fn holds_captures(&self) -> bool {
        !self.lock_captures().is_empty()
    }

    /// Drop the capture for `id`, if any, ending its pump: its consumer will
    /// never adopt it.
    pub(crate) fn discard(&self, id: &SessionId) {
        self.lock_captures().remove(id);
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

    #[cfg(test)]
    pub(crate) fn is_armed(&self) -> bool {
        self.state.armed.load(Ordering::Acquire)
    }

    /// The pump progress of the capture waiting for `id`.
    #[cfg(test)]
    pub(crate) fn progress(&self, id: &SessionId) -> Option<watch::Receiver<CaptureProgress>> {
        self.lock_captures()
            .get(id)
            .map(|capture| capture.progress.clone())
    }

    fn lock_captures(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, Capture>> {
        self.state
            .captures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Disarms the tap when dropped: held by the console forwarder task, so the
/// tap stops capturing however that task ends (including abort).
pub(crate) struct DisarmOnDrop(LiveSessionEventTap);

impl DisarmOnDrop {
    pub(crate) fn new(tap: LiveSessionEventTap) -> Self {
        Self(tap)
    }
}

impl Drop for DisarmOnDrop {
    fn drop(&mut self) {
        self.0.disarm();
    }
}

/// Move a captured receiver's envelopes into the capture's queue until the
/// actor's stream ends or the queue's consumer (the capture, or the stream
/// it was adopted as) is dropped.
///
/// A full queue parks the pump on the send without reading on, so envelopes
/// beyond the queue stay in the session broadcast, whose lag-aware stream
/// yields meerkat's typed `StreamTruncated` marker for whatever its window
/// could not hold. Nothing is dropped here.
async fn pump(
    session_id: SessionId,
    mut stream: EventStream,
    queue: mpsc::Sender<Envelope>,
    progress: watch::Sender<CaptureProgress>,
) {
    let mut saturations: u64 = 0;
    loop {
        let envelope = tokio::select! {
            biased;
            () = queue.closed() => return,
            next = stream.next() => match next {
                Some(envelope) => envelope,
                None => return,
            },
        };
        let envelope = match queue.try_send(envelope) {
            Ok(()) => None,
            Err(mpsc::error::TrySendError::Closed(_)) => return,
            Err(mpsc::error::TrySendError::Full(envelope)) => Some(envelope),
        };
        if let Some(envelope) = envelope {
            saturations += 1;
            if saturations == 1 {
                tracing::warn!(
                    session_id = %session_id,
                    capacity = queue.max_capacity(),
                    "live session event tap: capture queue is full; further events wait in the \
                     session broadcast, which marks any it cannot hold as StreamTruncated"
                );
            } else {
                tracing::debug!(
                    session_id = %session_id,
                    saturations,
                    "live session event tap: capture queue full again"
                );
            }
            progress.send_modify(|progress| progress.saturated = true);
            if queue.send(envelope).await.is_err() {
                return;
            }
            progress.send_modify(|progress| progress.saturated = false);
        }
        progress.send_modify(|progress| progress.queued += 1);
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

    /// Everything a test-owned subscription holds right now. The probe is
    /// read after every turn, well inside the session broadcast's window,
    /// so it sees each envelope the actor published exactly once.
    fn drain_probe(probe: &mut EventStream) -> Vec<Envelope> {
        let mut events = Vec::new();
        while let Some(Some(event)) = probe.next().now_or_never() {
            assert!(
                !matches!(event.payload, AgentEvent::StreamTruncated { .. }),
                "the test probe never lags"
            );
            events.push(event);
        }
        events
    }

    /// Read an adopted stream until it has yielded `terminals` run
    /// terminals. The pump feeds the stream from its own task, so this
    /// awaits rather than polls.
    async fn read_through_terminals(stream: &mut EventStream, terminals: usize) -> Vec<Envelope> {
        let mut events = Vec::new();
        let mut seen = 0;
        tokio::time::timeout(crate::test_wait::STRUCTURAL_BACKSTOP, async {
            while seen < terminals {
                let event = stream.next().await.expect("adopted stream open");
                if matches!(
                    event.payload,
                    AgentEvent::RunCompleted { .. } | AgentEvent::RunFailed { .. }
                ) {
                    seen += 1;
                }
                events.push(event);
            }
        })
        .await
        .expect("the adopted stream reaches every terminal");
        events
    }

    /// Read exactly `count` envelopes from an adopted stream.
    async fn read_exactly(stream: &mut EventStream, count: usize) -> Vec<Envelope> {
        let mut events = Vec::with_capacity(count);
        tokio::time::timeout(crate::test_wait::STRUCTURAL_BACKSTOP, async {
            while events.len() < count {
                events.push(stream.next().await.expect("adopted stream open"));
            }
        })
        .await
        .expect("the adopted stream yields every envelope");
        events
    }

    async fn wait_for_progress(
        tap: &LiveSessionEventTap,
        id: &SessionId,
        what: &str,
        done: impl FnMut(&CaptureProgress) -> bool,
    ) {
        let mut progress = tap.progress(id).expect("capture waiting");
        tokio::time::timeout(
            crate::test_wait::STRUCTURAL_BACKSTOP,
            progress.wait_for(done),
        )
        .await
        .unwrap_or_else(|_| panic!("capture pump never reached: {what}"))
        .expect("capture pump alive");
    }

    fn assert_contiguous(events: &[Envelope], first_seq: u64, what: &str) {
        for (offset, event) in events.iter().enumerate() {
            assert_eq!(event.seq, first_seq + offset as u64, "{what}: contiguous");
        }
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

        let mut captured = tap.take_live(&id).expect("live capture").stream;
        let events = read_through_terminals(&mut captured, 1).await;
        let first = events.first().expect("captured events");
        assert!(
            matches!(first.payload, AgentEvent::RunStarted { .. }),
            "the capture begins at the run's own start, got {:?}",
            first.payload
        );
        assert_eq!(first.seq, 1, "run_started is the actor's first event");
        assert_contiguous(&events, 1, "captured sequence through run_completed");
        assert!(
            late.next().now_or_never().is_none(),
            "a subscription opened after the run sees nothing: the stream has no replay"
        );
        assert!(
            tap.take_live(&id).is_none(),
            "adoption consumes the capture"
        );
    }

    /// Meerkat's session broadcast keeps 256 envelopes per receiver. A
    /// capture adopted only after far more than that were published still
    /// starts at `run_started` seq 1 and carries every envelope, with no
    /// `StreamTruncated` marker: the pump moved them into the capture's own
    /// queue as they arrived. The test waits for the pump after every turn,
    /// so no turn ever outruns it.
    #[tokio::test]
    async fn tap_keeps_more_than_the_session_broadcast_holds_before_adoption() {
        let fixture = fixture();
        let tap = fixture.spec.live_session_event_tap();
        tap.arm();
        let id = create_through(
            &fixture.spec.session_service,
            create_request(chatty_client(60), InitialTurnPolicy::Defer),
        )
        .await;
        let mut probe = MobSessionService::subscribe_session_events(fixture.raw.as_ref(), &id)
            .await
            .expect("probe subscription");
        const TURNS: usize = 10;
        let mut published = Vec::new();
        for turn in 0..TURNS {
            run_turn(&fixture.raw, &id, &format!("probe-{turn}")).await;
            published.extend(drain_probe(&mut probe));
            let total = published.len() as u64;
            wait_for_progress(&tap, &id, "every published envelope queued", |progress| {
                progress.queued >= total
            })
            .await;
        }
        assert!(
            published.len() > 2 * 256,
            "the test publishes well past the 256-envelope session broadcast before adoption, \
             got {}",
            published.len()
        );

        let mut captured = tap.take_live(&id).expect("live capture").stream;
        let events = read_through_terminals(&mut captured, TURNS).await;
        assert_eq!(
            events.iter().map(|event| event.seq).collect::<Vec<_>>(),
            published.iter().map(|event| event.seq).collect::<Vec<_>>(),
            "the capture holds exactly what the actor published, in order"
        );
        assert!(matches!(
            events.first().map(|event| &event.payload),
            Some(AgentEvent::RunStarted { .. })
        ));
        assert_contiguous(&events, 1, "the capture");
    }

    /// A capture whose queue fills before adoption keeps its prefix and marks
    /// the rest honestly: the pump stops reading, the session broadcast holds
    /// its last 256 envelopes, and the adopted stream yields meerkat's typed
    /// `StreamTruncated(StreamLagged)` marker counting exactly what fell out
    /// of that window. The test parks the pump at capacity before publishing
    /// the overflow, so the split is fixed, not scheduled.
    #[tokio::test]
    async fn tap_marks_a_capture_that_outgrew_its_queue_with_a_typed_gap() {
        const CAPACITY: usize = 4;
        const WINDOW: u64 = 256;
        let fixture = fixture();
        let tap = LiveSessionEventTap::with_capacity(CAPACITY);
        tap.arm();
        let slot = LiveSessionActorWitnessSlot::default();
        let raw = fixture.raw.clone() as Arc<dyn MobSessionService>;
        let id = raw
            .create_session_with_actor_witness_under_runtime_turn_boundary(
                create_request(chatty_client(40), InitialTurnPolicy::Defer),
                None,
                &slot,
            )
            .await
            .expect("witness-bearing create")
            .session_id;
        tap.capture(raw.as_ref(), &slot, &id, InitialTurnPolicy::Defer)
            .await;
        let mut probe = MobSessionService::subscribe_session_events(fixture.raw.as_ref(), &id)
            .await
            .expect("probe subscription");

        run_turn(&fixture.raw, &id, "probe-0").await;
        let mut published = drain_probe(&mut probe);
        assert!(published.len() > CAPACITY + 1);
        // Queue full and one more envelope in the pump's hand.
        wait_for_progress(&tap, &id, "saturated", |progress| {
            progress.saturated && progress.queued == CAPACITY as u64
        })
        .await;
        let held = CAPACITY as u64 + 1;
        const TURNS: usize = 8;
        for turn in 1..TURNS {
            run_turn(&fixture.raw, &id, &format!("probe-{turn}")).await;
            published.extend(drain_probe(&mut probe));
        }
        let total = published.len() as u64;
        assert!(
            total - held > WINDOW,
            "the unread remainder exceeds the session broadcast"
        );

        let mut captured = tap
            .take_live(&id)
            .expect("an overflowed capture is still adopted, with its gap marked")
            .stream;
        let events = read_exactly(&mut captured, held as usize + 1 + WINDOW as usize).await;
        let (prefix, rest) = events.split_at(held as usize);
        assert!(matches!(prefix[0].payload, AgentEvent::RunStarted { .. }));
        assert_contiguous(prefix, 1, "the queued prefix plus the held envelope");
        let dropped = total - held - WINDOW;
        assert!(
            matches!(
                &rest[0].payload,
                AgentEvent::StreamTruncated {
                    reason: meerkat_core::event::StreamTruncationReason::StreamLagged {
                        dropped: marked,
                    },
                } if *marked == dropped
            ),
            "meerkat marks the gap with its exact size ({dropped}), got {:?}",
            rest[0].payload
        );
        let tail = &rest[1..];
        assert_contiguous(tail, total - WINDOW + 1, "the retained window");
        assert!(matches!(
            tail.last().map(|event| &event.payload),
            Some(AgentEvent::RunCompleted { .. })
        ));
        assert!(
            captured.next().now_or_never().is_none(),
            "nothing beyond what the actor published"
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

        assert!(tap.take_live(&id).is_none());
        assert!(!changes.has_changed().expect("tap alive"));
    }

    /// A stock constructor with a user pre-build hook: the tap must read the
    /// initial-turn policy the hook left, not the one the request arrived
    /// with, because the hook's layer sits below the spec's base wrapper.
    fn spec_with_initial_turn_hook(
        dir: &tempfile::TempDir,
        initial_turn: InitialTurnPolicy,
    ) -> MobBootstrapSpec {
        let definition = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"event-tap-hook\"\n")
            .expect("mob definition");
        MobBootstrapSpec::ephemeral_with_hook(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            dir.path().to_path_buf(),
            16,
            None,
            move |req| {
                req.initial_turn = initial_turn;
                Box::pin(async { Ok(()) })
            },
        )
    }

    /// A hook that turns a deferred create eager: the first run happens
    /// inside the create, so the tap must decline, not present the remainder
    /// as a capture from the actor's first event.
    #[tokio::test]
    async fn tap_declines_a_create_a_pre_build_hook_made_eager() {
        let dir = tempfile::tempdir().expect("temp dir");
        let spec = spec_with_initial_turn_hook(&dir, InitialTurnPolicy::RunImmediately);
        let tap = spec.live_session_event_tap();
        tap.arm();
        let changes = tap.changes();

        let result = spec
            .session_service
            .create_session_with_actor_witness_under_runtime_turn_boundary(
                deferred_request(),
                None,
                &LiveSessionActorWitnessSlot::default(),
            )
            .await
            .expect("witness-bearing create");

        assert!(
            result.turns > 0,
            "the hook made the create eager: its first turn ran inside it"
        );
        assert!(!tap.holds_live(&result.session_id));
        assert!(!changes.has_changed().expect("tap alive"));
    }

    /// The inverse: a hook that defers an eager request is captured, from
    /// the actor's first event.
    #[tokio::test]
    async fn tap_captures_a_create_a_pre_build_hook_deferred() {
        let dir = tempfile::tempdir().expect("temp dir");
        let spec = spec_with_initial_turn_hook(&dir, InitialTurnPolicy::Defer);
        let tap = spec.live_session_event_tap();
        tap.arm();

        let id = create_through(
            &spec.session_service,
            create_request(
                meerkat_client::TestClient::default(),
                InitialTurnPolicy::RunImmediately,
            ),
        )
        .await;

        assert!(
            tap.holds_live(&id),
            "the executed request deferred, so the capture predates the first run"
        );
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
            tap.take_live(&id).is_none(),
            "a capture of a revoked actor incarnation is never adopted"
        );
    }

    /// A revocation that lands after the create-time subscription opened but
    /// before the capture is stored fails closed: the stream is dropped, not
    /// stored under a witness that no longer names a live actor.
    #[tokio::test]
    async fn tap_never_stores_a_subscription_its_actor_lost_while_subscribing() {
        let fixture = fixture();
        let tap = LiveSessionEventTap::default();
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
        let actor = slot.witness().expect("published witness");
        let stream = MobSessionService::subscribe_session_events(raw.as_ref(), &id)
            .await
            .expect("create-time subscription");

        fixture
            .raw
            .discard_live_session(&id)
            .await
            .expect("discard live session");

        assert!(!tap.install(&id, actor, stream));
        assert!(!tap.holds_captures());
        assert!(!tap.changes().has_changed().expect("tap alive"));
    }

    /// Sweeping a revoked capture drops its queue, which ends its pump.
    #[tokio::test]
    async fn sweep_releases_revoked_captures() {
        let fixture = fixture();
        let tap = fixture.spec.live_session_event_tap();
        tap.arm();

        let id = create_through_spec(&fixture).await;
        let mut progress = tap.progress(&id).expect("capture waiting");
        fixture
            .raw
            .discard_live_session(&id)
            .await
            .expect("discard live session");
        assert!(tap.holds_captures(), "nothing swept it yet");

        tap.sweep();
        assert!(!tap.holds_captures());
        tokio::time::timeout(crate::test_wait::STRUCTURAL_BACKSTOP, async {
            while progress.changed().await.is_ok() {}
        })
        .await
        .expect("the swept capture's pump ends");
    }

    #[tokio::test]
    async fn unarmed_tap_captures_nothing() {
        let fixture = fixture();
        let tap = fixture.spec.live_session_event_tap();
        let changes = tap.changes();

        let id = create_through_spec(&fixture).await;

        assert!(tap.take_live(&id).is_none());
        assert!(!changes.has_changed().expect("tap alive"));
    }

    /// Child mobs are built on the agent mob tools' session service. A stock
    /// constructor must hand those tools a service carrying the spec's tap,
    /// or child-mob members keep the lossy late subscription.
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
