//! Create-time capture of a member session's live event stream.
//!
//! Meerkat's per-session event stream is a broadcast with no replay: a
//! subscriber sees only events published after it subscribed. The console
//! agent-event forwarder attaches to a member on its own reconcile cadence,
//! so after a restore the first run of a revived member could start (and be
//! dropped) before the forwarder's next attempt.
//!
//! The tap closes that window at the one point that precedes every run of an
//! actor instance: the witness-bearing session creates. Those run while the
//! provisioner holds the session's turn-finalization boundary, and every run
//! start needs that boundary, so a subscription taken right after create
//! returns predates the actor's first `run_started`. The receiver buffers
//! meerkat's own events (bounded by the session channel; overflow surfaces as
//! meerkat's `StreamTruncated` marker) until the forwarder adopts it.
//!
//! Captures are keyed by `SessionId` and fenced by the exact actor
//! incarnation's [`LiveSessionActorWitness`]: registries revoke a witness
//! before removing or replacing its actor, so a capture whose witness is no
//! longer live names a dead actor and is dropped, never adopted.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use meerkat_core::comms::EventStream;
use meerkat_core::types::SessionId;
use meerkat_mob::MobSessionService;
use meerkat_session::{LiveSessionActorWitness, LiveSessionActorWitnessSlot};
use tokio::sync::watch;

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
    captures: Mutex<HashMap<SessionId, Capture>>,
    /// Bumped on every new capture so the forwarder can reconcile at once
    /// instead of waiting for its next scheduled attempt.
    changes: watch::Sender<u64>,
}

struct Capture {
    actor: LiveSessionActorWitness,
    stream: EventStream,
}

impl Default for LiveSessionEventTap {
    fn default() -> Self {
        Self {
            state: Arc::new(TapState {
                armed: AtomicBool::new(false),
                captures: Mutex::new(HashMap::new()),
                changes: watch::channel(0).0,
            }),
        }
    }
}

impl LiveSessionEventTap {
    /// Start capturing on subsequent witness-bearing creates.
    pub(crate) fn arm(&self) {
        self.state.armed.store(true, Ordering::Release);
    }

    /// Subscribe to the live event stream of the actor `slot` names.
    ///
    /// Called by the base session-service wrapper right after a
    /// witness-bearing create returned. Never fails the create: an unarmed
    /// tap, an unpublished witness, or a subscribe error leaves the forwarder
    /// on its ordinary subscription path. Takes only the inner service's
    /// session read lock, never the turn-finalization boundary.
    pub(crate) async fn capture(
        &self,
        inner: &dyn MobSessionService,
        slot: &LiveSessionActorWitnessSlot,
        id: &SessionId,
    ) {
        if !self.state.armed.load(Ordering::Acquire) {
            return;
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
        {
            let mut captures = self.lock_captures();
            captures.retain(|_, capture| capture.actor.is_live());
            captures.insert(id.clone(), Capture { actor, stream });
        }
        self.state
            .changes
            .send_modify(|version| *version = version.wrapping_add(1));
    }

    /// Remove the capture for `id`, returning its stream only while the
    /// captured actor incarnation is still live.
    pub(crate) fn take_live(&self, id: &SessionId) -> Option<EventStream> {
        let mut captures = self.lock_captures();
        let capture = captures.remove(id);
        captures.retain(|_, capture| capture.actor.is_live());
        capture
            .filter(|capture| capture.actor.is_live())
            .map(|capture| capture.stream)
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    use futures::{FutureExt, StreamExt};
    use meerkat_core::AgentEvent;
    use meerkat_core::service::{
        CreateSessionRequest, DeferredPromptPolicy, InitialTurnPolicy, SessionBuildOptions,
        SessionService, StartTurnRequest, StartTurnRuntimeSemantics,
    };

    use crate::MobBootstrapSpec;

    type RawService = meerkat_session::EphemeralSessionService<meerkat::FactoryAgentBuilder>;

    struct Fixture {
        raw: Arc<RawService>,
        spec: MobBootstrapSpec,
        _dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
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

    fn deferred_request() -> CreateSessionRequest {
        CreateSessionRequest {
            model: "gpt-5.5".to_string(),
            prompt: meerkat_core::ContentInput::Text(String::new()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: InitialTurnPolicy::Defer,
            deferred_prompt_policy: DeferredPromptPolicy::Discard,
            build: Some(SessionBuildOptions {
                llm_client_override: Some(meerkat::encode_llm_client_override_for_service(
                    Arc::new(meerkat_client::TestClient::default()),
                )),
                ..Default::default()
            }),
            labels: None,
            injected_context: Vec::new(),
        }
    }

    /// Create one session through the spec's base wrapper, the same
    /// witness-bearing create every runtime-backed member materialization
    /// lowers to.
    async fn create_through_spec(fixture: &Fixture) -> SessionId {
        let slot = LiveSessionActorWitnessSlot::default();
        fixture
            .spec
            .session_service
            .create_session_with_actor_witness_under_runtime_turn_boundary(
                deferred_request(),
                None,
                &slot,
            )
            .await
            .expect("witness-bearing create")
            .session_id
    }

    fn drain_ready(
        stream: &mut EventStream,
    ) -> Vec<meerkat_core::event::EventEnvelope<AgentEvent>> {
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

        fixture
            .raw
            .start_turn(
                &id,
                StartTurnRequest {
                    prompt: meerkat_core::ContentInput::Text("probe".to_string()),
                    injected_context: Vec::new(),
                    system_prompt: None,
                    event_tx: None,
                    runtime: StartTurnRuntimeSemantics::default(),
                },
            )
            .await
            .expect("turn completes");

        let mut late = MobSessionService::subscribe_session_events(fixture.raw.as_ref(), &id)
            .await
            .expect("late subscription");

        let mut captured = tap.take_live(&id).expect("live capture");
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
            tap.take_live(&id).is_none(),
            "adoption consumes the capture"
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

    #[tokio::test]
    async fn unarmed_tap_captures_nothing() {
        let fixture = fixture();
        let tap = fixture.spec.live_session_event_tap();
        let changes = tap.changes();

        let id = create_through_spec(&fixture).await;

        assert!(tap.take_live(&id).is_none());
        assert!(!changes.has_changed().expect("tap alive"));
    }
}
