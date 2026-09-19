//! One live voice path per gateway, arbitrated "latest engaged wins".
//!
//! A gateway may register console voice (`runtime_options.console_voice`)
//! and the external live channel (`runtime_options.live`) together. Both
//! doors open member channels through the same [`super::GatewayLiveContext`]
//! (one adapter host, one provider registration, one audio owner), so only
//! one of them may hold an active voice path at a time. Meerkat couples
//! nothing here: channels are per session, and a console call on member B
//! and an external channel on member A are independent upstream. The policy
//! that the newest engagement wins, and the loser learns why, is MobKit's.
//!
//! The arbiter never closes a channel itself. Each engagement carries the
//! closer its own door would use (the console slot's cancel path, the
//! external host's generated close), so a preempted owner is closed through
//! exactly the sequence it would have used to close on its own. A close that
//! fails leaves the previous owner in place and fails the newcomer closed;
//! there is never a silent double owner.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

/// Typed reason a preempted owner's channel was closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveSupersededReason {
    /// A console voice call took the live path.
    SupersededByConsoleVoice,
    /// An external `mobkit/live/open` took the live path.
    SupersededByExternalLive,
}

impl LiveSupersededReason {
    /// Wire spelling shared by the console error, the external status and
    /// close results, and the `mobkit/live/superseded` notification.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SupersededByConsoleVoice => "superseded_by_console_voice",
            Self::SupersededByExternalLive => "superseded_by_external_live",
        }
    }
}

/// Which door holds the live path, and for which member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveOwner {
    ConsoleVoice {
        principal: String,
        identity: String,
        channel_id: Option<String>,
    },
    ExternalLive {
        identity: String,
        channel_id: Option<String>,
    },
}

impl LiveOwner {
    #[must_use]
    pub fn identity(&self) -> &str {
        match self {
            Self::ConsoleVoice { identity, .. } | Self::ExternalLive { identity, .. } => identity,
        }
    }

    #[must_use]
    pub fn channel_id(&self) -> Option<&str> {
        match self {
            Self::ConsoleVoice { channel_id, .. } | Self::ExternalLive { channel_id, .. } => {
                channel_id.as_deref()
            }
        }
    }

    /// The reason the OTHER owner sees when this owner wins.
    #[must_use]
    pub const fn supersedes_with(&self) -> LiveSupersededReason {
        match self {
            Self::ConsoleVoice { .. } => LiveSupersededReason::SupersededByConsoleVoice,
            Self::ExternalLive { .. } => LiveSupersededReason::SupersededByExternalLive,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::ConsoleVoice { .. } => "console_voice",
            Self::ExternalLive { .. } => "external_live",
        }
    }

    /// Two engagements by the same door for the same member are one owner
    /// re-engaging (a reopen), never a preemption.
    fn same_door(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::ConsoleVoice {
                    principal: a,
                    identity: b,
                    ..
                },
                Self::ConsoleVoice {
                    principal: c,
                    identity: d,
                    ..
                },
            ) => a == c && b == d,
            (Self::ExternalLive { identity: a, .. }, Self::ExternalLive { identity: b, .. }) => {
                a == b
            }
            _ => false,
        }
    }

    /// `{owner, identity, channel_id}` as reported in `mobkit/live/open`
    /// results and the readiness holder.
    #[must_use]
    pub fn to_wire(&self) -> Value {
        let mut wire = json!({
            "owner": self.kind(),
            "identity": self.identity(),
        });
        if let Some(channel_id) = self.channel_id() {
            wire["channel_id"] = Value::String(channel_id.to_string());
        }
        wire
    }

    fn set_channel(&mut self, channel: &str) {
        match self {
            Self::ConsoleVoice { channel_id, .. } | Self::ExternalLive { channel_id, .. } => {
                *channel_id = Some(channel.to_string());
            }
        }
    }
}

/// The close an owner would run on itself. The arbiter calls it with the
/// reason the owner is being superseded and the channel it had bound, if any.
pub type LiveOwnerCloser = Arc<
    dyn Fn(
            LiveSupersededReason,
            Option<String>,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;

/// Fire-and-forget notification sink (`method`, `params`); the stdio gateway
/// installs its stdout writer here, HTTP-only hosts leave it empty.
pub type LiveOwnerNotifier = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// Handle to one engagement. Releasing or binding a stale lease is a no-op,
/// so a slow loser can never disturb the owner that replaced it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveOwnerLease(u64);

#[derive(Debug)]
pub enum LiveOwnerArbiterError {
    /// The previous owner's own close sequence failed. It remains the owner.
    CloseFailed {
        kind: &'static str,
        identity: String,
        error: String,
    },
}

impl std::fmt::Display for LiveOwnerArbiterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CloseFailed {
                kind,
                identity,
                error,
            } => write!(
                f,
                "could not close the active {kind} voice path for {identity}: {error}"
            ),
        }
    }
}

impl std::error::Error for LiveOwnerArbiterError {}

struct Engagement {
    lease: LiveOwnerLease,
    owner: LiveOwner,
    closer: LiveOwnerCloser,
}

#[derive(Default)]
struct Ledger {
    current: Option<Engagement>,
    /// Leases preempted before they bound a channel. The owner discovers
    /// this on bind and closes what it just opened.
    superseded_leases: VecDeque<(LiveOwnerLease, LiveSupersededReason)>,
    /// Typed close reasons for channels the arbiter closed, retained so a
    /// late `mobkit/live/status` or `mobkit/live/close` can report them.
    close_reasons: VecDeque<(String, LiveSupersededReason, Instant)>,
}

const CLOSE_REASON_RETENTION: Duration = Duration::from_mins(10);
const CLOSE_REASON_CAPACITY: usize = 64;
const SUPERSEDED_LEASE_CAPACITY: usize = 64;

/// See the module docs.
pub struct LiveOwnerArbiter {
    /// Serializes engagements so two newcomers cannot both preempt the same
    /// owner or each other concurrently. Never held while the ledger lock is.
    engage: tokio::sync::Mutex<()>,
    ledger: StdMutex<Ledger>,
    next_lease: AtomicU64,
    notifier: StdMutex<Option<LiveOwnerNotifier>>,
}

impl Default for LiveOwnerArbiter {
    fn default() -> Self {
        Self {
            engage: tokio::sync::Mutex::new(()),
            ledger: StdMutex::new(Ledger::default()),
            next_lease: AtomicU64::new(1),
            notifier: StdMutex::new(None),
        }
    }
}

impl std::fmt::Debug for LiveOwnerArbiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveOwnerArbiter")
            .field("holder", &self.holder())
            .finish_non_exhaustive()
    }
}

impl LiveOwnerArbiter {
    fn ledger(&self) -> std::sync::MutexGuard<'_, Ledger> {
        self.ledger
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Install the notification sink for `mobkit/live/superseded`.
    pub fn set_notifier(&self, notifier: LiveOwnerNotifier) {
        *self
            .notifier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(notifier);
    }

    /// The owner currently holding the live path, if any.
    #[must_use]
    pub fn holder(&self) -> Option<LiveOwner> {
        self.ledger()
            .current
            .as_ref()
            .map(|engagement| engagement.owner.clone())
    }

    /// Take the live path for `owner`, closing a different active owner first.
    ///
    /// Returns the lease and the owner that was preempted, if any. The same
    /// door re-engaging for the same member replaces its own engagement
    /// without closing anything. Runs the loser's closer with no lock held
    /// other than the engagement serializer.
    pub async fn engage(
        &self,
        owner: LiveOwner,
        closer: LiveOwnerCloser,
    ) -> Result<(LiveOwnerLease, Option<LiveOwner>), LiveOwnerArbiterError> {
        let _serialized = self.engage.lock().await;
        let previous = self.ledger().current.as_ref().map(|engagement| {
            (
                engagement.lease,
                engagement.owner.clone(),
                Arc::clone(&engagement.closer),
            )
        });
        let reason = owner.supersedes_with();
        let mut preempted = None;
        if let Some((lease, previous_owner, previous_closer)) = previous
            && !previous_owner.same_door(&owner)
        {
            if let Err(error) =
                previous_closer(reason, previous_owner.channel_id().map(ToString::to_string)).await
            {
                return Err(LiveOwnerArbiterError::CloseFailed {
                    kind: previous_owner.kind(),
                    identity: previous_owner.identity().to_string(),
                    error,
                });
            }
            {
                let mut ledger = self.ledger();
                // A different engagement may have replaced the loser while
                // its close ran (it released and someone re-engaged). Only
                // retire the exact engagement we closed.
                if ledger
                    .current
                    .as_ref()
                    .is_some_and(|engagement| engagement.lease == lease)
                {
                    ledger.current = None;
                }
                if let Some(channel) = previous_owner.channel_id() {
                    remember_close_reason(&mut ledger, channel, reason);
                } else {
                    ledger.superseded_leases.push_back((lease, reason));
                    while ledger.superseded_leases.len() > SUPERSEDED_LEASE_CAPACITY {
                        ledger.superseded_leases.pop_front();
                    }
                }
            }
            self.notify_superseded(&previous_owner, reason, &owner);
            preempted = Some(previous_owner);
        }
        let lease = LiveOwnerLease(self.next_lease.fetch_add(1, Ordering::Relaxed));
        self.ledger().current = Some(Engagement {
            lease,
            owner,
            closer,
        });
        Ok((lease, preempted))
    }

    /// Record the channel the leaseholder opened. Stale leases are ignored.
    pub fn bind_channel(&self, lease: LiveOwnerLease, channel_id: &str) {
        let mut ledger = self.ledger();
        if let Some(engagement) = ledger.current.as_mut()
            && engagement.lease == lease
        {
            engagement.owner.set_channel(channel_id);
        }
    }

    /// Whether `lease` was preempted before it bound a channel. Consumes the
    /// record; the caller closes what it opened and reports the reason.
    pub fn take_superseded(&self, lease: LiveOwnerLease) -> Option<LiveSupersededReason> {
        let mut ledger = self.ledger();
        let index = ledger
            .superseded_leases
            .iter()
            .position(|(candidate, _)| *candidate == lease)?;
        ledger
            .superseded_leases
            .remove(index)
            .map(|(_, reason)| reason)
    }

    /// Give the live path back if `lease` still holds it.
    pub fn release(&self, lease: LiveOwnerLease) {
        let mut ledger = self.ledger();
        if ledger
            .current
            .as_ref()
            .is_some_and(|engagement| engagement.lease == lease)
        {
            ledger.current = None;
        }
    }

    /// Give the live path back if the holder's bound channel is `channel_id`.
    pub fn release_channel(&self, channel_id: &str) {
        let mut ledger = self.ledger();
        if ledger
            .current
            .as_ref()
            .is_some_and(|engagement| engagement.owner.channel_id() == Some(channel_id))
        {
            ledger.current = None;
        }
    }

    /// Typed reason if the arbiter closed `channel_id` recently.
    #[must_use]
    pub fn close_reason(&self, channel_id: &str) -> Option<LiveSupersededReason> {
        let mut ledger = self.ledger();
        let now = Instant::now();
        ledger.close_reasons.retain(|(_, _, recorded)| {
            now.saturating_duration_since(*recorded) < CLOSE_REASON_RETENTION
        });
        ledger
            .close_reasons
            .iter()
            .rev()
            .find(|(candidate, _, _)| candidate == channel_id)
            .map(|(_, reason, _)| *reason)
    }

    /// Record a close reason for a channel closed outside `engage` (a loser
    /// that lost the race before binding and closed itself on bind).
    pub fn record_close_reason(&self, channel_id: &str, reason: LiveSupersededReason) {
        remember_close_reason(&mut self.ledger(), channel_id, reason);
    }

    fn notify_superseded(
        &self,
        loser: &LiveOwner,
        reason: LiveSupersededReason,
        winner: &LiveOwner,
    ) {
        let notifier = self
            .notifier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(notifier) = notifier else {
            return;
        };
        let mut params = json!({
            "owner": loser.kind(),
            "identity": loser.identity(),
            "reason": reason.as_str(),
            "superseded_by": winner.to_wire(),
        });
        if let Some(channel_id) = loser.channel_id() {
            params["channel_id"] = Value::String(channel_id.to_string());
        }
        notifier("mobkit/live/superseded", params);
    }
}

fn remember_close_reason(ledger: &mut Ledger, channel_id: &str, reason: LiveSupersededReason) {
    ledger
        .close_reasons
        .push_back((channel_id.to_string(), reason, Instant::now()));
    while ledger.close_reasons.len() > CLOSE_REASON_CAPACITY {
        ledger.close_reasons.pop_front();
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn console(identity: &str) -> LiveOwner {
        LiveOwner::ConsoleVoice {
            principal: "voice@example.com".to_string(),
            identity: identity.to_string(),
            channel_id: None,
        }
    }

    fn external(identity: &str) -> LiveOwner {
        LiveOwner::ExternalLive {
            identity: identity.to_string(),
            channel_id: None,
        }
    }

    type CloseCalls = Arc<StdMutex<Vec<(LiveSupersededReason, Option<String>)>>>;

    fn recording_closer(calls: CloseCalls, result: Result<(), String>) -> LiveOwnerCloser {
        Arc::new(move |reason, channel| {
            let calls = Arc::clone(&calls);
            let result = result.clone();
            Box::pin(async move {
                calls.lock().expect("calls").push((reason, channel));
                result
            })
        })
    }

    fn noop_closer() -> LiveOwnerCloser {
        Arc::new(|_, _| Box::pin(async { Ok(()) }))
    }

    #[tokio::test]
    async fn newest_engagement_closes_the_previous_owner_with_a_typed_reason() {
        let arbiter = LiveOwnerArbiter::default();
        let notified = Arc::new(StdMutex::new(Vec::<(String, Value)>::new()));
        let sink = Arc::clone(&notified);
        arbiter.set_notifier(Arc::new(move |method, params| {
            sink.lock()
                .expect("notified")
                .push((method.to_string(), params));
        }));
        let closes = Arc::new(StdMutex::new(Vec::new()));
        let (console_lease, preempted) = arbiter
            .engage(
                console("agent-a"),
                recording_closer(Arc::clone(&closes), Ok(())),
            )
            .await
            .expect("first engagement");
        assert!(preempted.is_none());
        arbiter.bind_channel(console_lease, "console-channel");
        assert_eq!(
            arbiter
                .holder()
                .and_then(|owner| owner.channel_id().map(ToString::to_string)),
            Some("console-channel".to_string())
        );

        let (external_lease, preempted) = arbiter
            .engage(external("agent-b"), noop_closer())
            .await
            .expect("external preempts console");
        let preempted = preempted.expect("console was preempted");
        assert_eq!(preempted.kind(), "console_voice");
        assert_eq!(preempted.channel_id(), Some("console-channel"));
        assert_eq!(
            closes.lock().expect("closes").as_slice(),
            &[(
                LiveSupersededReason::SupersededByExternalLive,
                Some("console-channel".to_string())
            )]
        );
        assert_eq!(
            arbiter.close_reason("console-channel"),
            Some(LiveSupersededReason::SupersededByExternalLive)
        );
        assert_eq!(
            arbiter.holder().map(|owner| owner.kind()),
            Some("external_live")
        );
        let notified = notified.lock().expect("notified");
        assert_eq!(notified.len(), 1);
        assert_eq!(notified[0].0, "mobkit/live/superseded");
        assert_eq!(notified[0].1["reason"], "superseded_by_external_live");
        assert_eq!(notified[0].1["channel_id"], "console-channel");
        assert_eq!(notified[0].1["superseded_by"]["owner"], "external_live");
        drop(notified);

        // A stale release from the loser must not evict the winner.
        arbiter.release(console_lease);
        assert_eq!(
            arbiter.holder().map(|owner| owner.kind()),
            Some("external_live")
        );
        arbiter.release(external_lease);
        assert!(arbiter.holder().is_none());
    }

    #[tokio::test]
    async fn same_door_reengaging_for_the_same_member_never_closes_itself() {
        let arbiter = LiveOwnerArbiter::default();
        let closes = Arc::new(StdMutex::new(Vec::new()));
        let (first, _) = arbiter
            .engage(
                console("agent-a"),
                recording_closer(Arc::clone(&closes), Ok(())),
            )
            .await
            .expect("first");
        arbiter.bind_channel(first, "channel-1");
        let (second, preempted) = arbiter
            .engage(console("agent-a"), noop_closer())
            .await
            .expect("reopen");
        assert!(preempted.is_none());
        assert!(closes.lock().expect("closes").is_empty());
        assert_ne!(first, second);
        // The console switching to ANOTHER agent is a different owner and
        // closes the previous call, matching the browser's close-before-switch.
        let (_, preempted) = arbiter
            .engage(console("agent-b"), noop_closer())
            .await
            .expect("switch");
        assert_eq!(
            preempted.map(|owner| owner.identity().to_string()),
            Some("agent-a".to_string())
        );
    }

    #[tokio::test]
    async fn a_failed_close_keeps_the_previous_owner_and_fails_the_newcomer() {
        let arbiter = LiveOwnerArbiter::default();
        let closes = Arc::new(StdMutex::new(Vec::new()));
        let (lease, _) = arbiter
            .engage(
                external("agent-a"),
                recording_closer(Arc::clone(&closes), Err("provider hung".to_string())),
            )
            .await
            .expect("first");
        arbiter.bind_channel(lease, "external-channel");
        let error = arbiter
            .engage(console("agent-b"), noop_closer())
            .await
            .expect_err("close failure fails the newcomer");
        assert!(matches!(
            error,
            LiveOwnerArbiterError::CloseFailed {
                kind: "external_live",
                ..
            }
        ));
        assert_eq!(
            arbiter.holder().map(|owner| owner.kind()),
            Some("external_live")
        );
        assert!(arbiter.close_reason("external-channel").is_none());
    }

    #[tokio::test]
    async fn preempting_an_unbound_lease_is_discovered_on_bind() {
        let arbiter = LiveOwnerArbiter::default();
        let (opening, _) = arbiter
            .engage(external("agent-a"), noop_closer())
            .await
            .expect("opening");
        let (_, preempted) = arbiter
            .engage(console("agent-b"), noop_closer())
            .await
            .expect("console wins the race");
        assert_eq!(preempted.map(|owner| owner.kind()), Some("external_live"));
        assert_eq!(
            arbiter.take_superseded(opening),
            Some(LiveSupersededReason::SupersededByConsoleVoice)
        );
        assert!(arbiter.take_superseded(opening).is_none());
        // Binding after the loss must not overwrite the winner.
        arbiter.bind_channel(opening, "late-channel");
        assert_eq!(
            arbiter.holder().map(|owner| owner.kind()),
            Some("console_voice")
        );
    }

    #[tokio::test]
    async fn release_by_channel_only_frees_the_matching_holder() {
        let arbiter = LiveOwnerArbiter::default();
        let (lease, _) = arbiter
            .engage(external("agent-a"), noop_closer())
            .await
            .expect("engage");
        arbiter.bind_channel(lease, "channel-a");
        arbiter.release_channel("channel-other");
        assert!(arbiter.holder().is_some());
        arbiter.release_channel("channel-a");
        assert!(arbiter.holder().is_none());
    }

    #[tokio::test]
    async fn engagements_are_serialized() {
        let arbiter = Arc::new(LiveOwnerArbiter::default());
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for index in 0..8 {
            let arbiter = Arc::clone(&arbiter);
            let in_flight = Arc::clone(&in_flight);
            let max_seen = Arc::clone(&max_seen);
            tasks.push(tokio::spawn(async move {
                let owner = if index % 2 == 0 {
                    external(&format!("agent-{index}"))
                } else {
                    console(&format!("agent-{index}"))
                };
                let closer: LiveOwnerCloser = Arc::new(move |_, _| {
                    let in_flight = Arc::clone(&in_flight);
                    let max_seen = Arc::clone(&max_seen);
                    Box::pin(async move {
                        let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                        max_seen.fetch_max(now, Ordering::SeqCst);
                        tokio::task::yield_now().await;
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    })
                });
                arbiter.engage(owner, closer).await.expect("engage")
            }));
        }
        for task in tasks {
            task.await.expect("task");
        }
        assert!(
            max_seen.load(Ordering::SeqCst) <= 1,
            "closers ran concurrently"
        );
        assert!(arbiter.holder().is_some());
    }
}
