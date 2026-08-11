//! Session bridge: connects the identity-first control plane to the Meerkat
//! session pipeline for real session creation, delivery, and retirement.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use meerkat_core::lifecycle::run_primitive::{
    OpenAiProviderTag, ProviderParamsOverride, ProviderTag,
};
use meerkat_core::types::HandlingMode;
use meerkat_mob::ids::AgentIdentity as MobAgentIdentity;
use meerkat_mob::launch::MemberLaunchMode;
use meerkat_mob::{
    MobHandle, MobSessionService, ResumeOverrideField, SpawnMemberSpec, SpawnSystemPromptOverride,
    WorkOrigin, WorkRef, WorkSpec,
};

use crate::mob_handle_runtime::{
    content_input_has_images, is_previous_member_cleanup_ambiguous_error,
    is_recoverable_lifecycle_cleanup_error, is_recoverable_session_owned_retire_cleanup_error,
    model_capabilities_for_member, topology_restore_failed_peer_ids,
};

use super::adapters::{ContinuitySessionStoreAdapter, SessionRuntimeState};
use super::types::{
    AgentBuildDraft, AgentIdentity, AgentRuntimeId, CheckpointVersion, ContinuityGeneration,
    DurableAgentSpec, FencingToken, SessionSnapshot,
};

fn is_missing_event_injector_error(error: &str) -> bool {
    error.contains("missing event injector capability")
        || (error.contains("missing required capability")
            && error.contains("interaction_event_injector"))
}

fn is_missing_bridge_session_snapshot_error(error: &str) -> bool {
    error.contains("missing bridge session snapshot")
}

/// TYPED respawn authorization: ONLY meerkat's typed
/// `SessionUnavailableForResume { reason: Absent }` — "no durable session
/// exists for this id" — permits falling back to a fresh respawn. Archived-
/// but-intact, recovery-held, quarantined, and every unknown failure must
/// preserve the identity/session binding and fail the delivery loudly:
/// those documents carry transcripts, and no wording change can turn them
/// into "absent". (The pre-0.8.9 form matched the prose "missing durable
/// session snapshot" by substring — a recovery or archived error whose
/// message drifted into that wording could authorize abandoning a live
/// transcript. Distinct from
/// [`is_missing_bridge_session_snapshot_error`], which is the
/// *delivery*-time revival wording and only routes INTO the repair attempt,
/// never authorizes a fresh respawn.)
fn durable_snapshot_is_typed_absent(error: &meerkat_mob::MobError) -> bool {
    matches!(
        error,
        meerkat_mob::MobError::SessionUnavailableForResume {
            reason: meerkat_mob::error::SessionResumeUnavailableReason::Absent,
            ..
        }
    )
}

fn is_repairable_bridge_delivery_error(error: &str) -> bool {
    is_missing_event_injector_error(error)
        || is_missing_bridge_session_snapshot_error(error)
        || is_previous_member_cleanup_ambiguous_error(error)
}

fn is_recoverable_bridge_respawn_cleanup_error(error: &str) -> bool {
    is_recoverable_lifecycle_cleanup_error(error)
}

fn is_member_already_exists_error(error: &meerkat_mob::MobError) -> bool {
    matches!(error, meerkat_mob::MobError::MemberAlreadyExists(_))
}

/// Delete and tombstone reset's superseded session projection before asking
/// Meerkat to retire the physical member.
///
/// The reset caller invokes this only after the replacement continuity head
/// has committed and bounded memory capture has completed. The adapter's
/// per-session lock drains every earlier save before the exact-CAS delete and
/// makes every later terminal projection a no-op. This ordering is required
/// with Meerkat 0.8: `MobHandle::retire` now returns the bare `ArchiveSession`
/// store error, so a post-retire string classifier cannot prove that the
/// archive boundary was reached. A failed abandon remains visible and the
/// caller retains the exact reset cleanup debt for retry.
async fn abandon_then_retire_reset_superseded<Abandon, AbandonFuture, Retire, RetireFuture>(
    member_id: &MobAgentIdentity,
    session_id: &meerkat_core::types::SessionId,
    abandon: Abandon,
    retire: Retire,
) -> Result<(), BridgeError>
where
    Abandon: FnOnce() -> AbandonFuture,
    AbandonFuture: Future<Output = Result<(), meerkat_store::SessionStoreError>>,
    Retire: FnOnce() -> RetireFuture,
    RetireFuture: Future<Output = Result<(), meerkat_mob::MobError>>,
{
    abandon().await.map_err(|error| {
        BridgeError::Mob(format!(
            "reset retire could not abandon superseded session {session_id}: {error}"
        ))
    })?;

    match retire().await {
        Ok(()) | Err(meerkat_mob::MobError::MemberNotFound(_)) => Ok(()),
        Err(error) => Err(BridgeError::Mob(format!(
            "reset retire cleanup failed for {member_id} after superseded session \
             {session_id} was abandoned: {error}"
        ))),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum MemberRepairRespawnFailure {
    DegradedTopologyRestore { failed_peer_ids: Vec<String> },
    RecoverableCleanup,
    Fatal(String),
}

/// Outcome of a resume-first delivery repair attempt.
#[derive(Debug)]
enum RepairResumeFailure {
    /// meerkat reports the durable session snapshot no longer exists — there
    /// is no transcript to preserve, so a fresh respawn is a legitimate
    /// fallback (recovery from actual loss, not abandonment).
    DurableSnapshotMissing { detail: String },
    /// Any other resume failure: the durable session exists but could not be
    /// resumed. Never fall back to a fresh spawn; fail the delivery loudly.
    Rejected(BridgeError),
}

fn classify_member_repair_respawn_failure(
    error: &meerkat_mob::MobRespawnError,
) -> MemberRepairRespawnFailure {
    if let Some(failed_peer_ids) = topology_restore_failed_peer_ids(error) {
        return MemberRepairRespawnFailure::DegradedTopologyRestore { failed_peer_ids };
    }
    if is_recoverable_bridge_respawn_cleanup_error(&error.to_string()) {
        return MemberRepairRespawnFailure::RecoverableCleanup;
    }
    MemberRepairRespawnFailure::Fatal(error.to_string())
}

/// Rebuild spec for a delivery-repair fresh spawn: the SAME member identity
/// with its pre-delivery role and labels, `Fresh` launch mode (a fresh
/// session is the point — this is only reached once there is no durable
/// transcript left to rebind). Shared by the two sanctioned fresh-spawn arms
/// of `repair_member_for_delivery`; each stays gated on its own evidence
/// (typed-absent durable loss / verified roster absence after recoverable
/// respawn cleanup).
fn fresh_member_spec_from_pre_delivery_entry(
    member_id: &MobAgentIdentity,
    role: meerkat_mob::ProfileName,
    labels: BTreeMap<String, String>,
) -> SpawnMemberSpec {
    let mut spec = SpawnMemberSpec::new(role, member_id.clone());
    if !labels.is_empty() {
        spec = spec.with_labels(labels);
    }
    spec
}

// ---------------------------------------------------------------------------
// BridgeError
// ---------------------------------------------------------------------------

/// Errors from session bridge operations.
#[derive(Debug)]
pub enum BridgeError {
    /// The underlying mob operation failed.
    Mob(String),
    /// A required field was missing or invalid.
    InvalidInput(String),
    /// Resume was rejected while a durable session row exists. The identity →
    /// session binding MUST stay intact: callers mark the identity degraded
    /// (Broken) with this error attached and retry on the next reconcile.
    /// Never fresh-spawn on this error — the durable transcript is the only
    /// copy of the conversation, and rebinding the identity to a fresh empty
    /// session permanently abandons it (the HomeCore restart-loss regression).
    ResumeRejected {
        kind: ResumeRejectionKind,
        detail: String,
    },
    /// A mob-actor round trip in the delivery path did not answer within the
    /// attempt's admission budget. meerkat's mob actor is ONE serialized
    /// command loop and `MobHandle::send_actor_command` has no timeout of its
    /// own, so a handler that blocks freezes every member's dispatch behind it
    /// with no upstream bound. This is containment, not a repair: the delivery
    /// did NOT happen and the actor is still blocked — but the failure is
    /// finite and names the round trip and member that hit it instead of
    /// hanging the caller indefinitely. Never treat this as a repairable
    /// stale-runtime-state error: repairing a member cannot unblock an actor.
    ActorAdmissionTimeout {
        /// Which actor round trip expired, e.g. `deliver.submit_work`.
        operation: &'static str,
        /// The mob member the round trip was addressed to.
        identity: MobAgentIdentity,
        /// Total time this delivery attempt spent waiting on the actor.
        waited: Duration,
    },
    /// The work WAS admitted and its turn reached a FAILED terminal.
    ///
    /// Distinct from [`Self::Mob`] on purpose: this names an outcome, not an
    /// admission problem. The turn ran. Never retry on it - a retry would run
    /// the member's turn a second time - and never confuse it with the delivery
    /// having failed to start.
    CompletionFailed(String),
    /// The work WAS admitted, its turn reached a SUCCESSFUL terminal, and only
    /// then did resolving the runtime's session id fail.
    ///
    /// The member did the work. Nothing about this is an admission problem, so
    /// it must never be routed into repair-and-retry: repairing the member
    /// cannot undo a turn that already ran, and resubmitting would run it
    /// twice. Non-retryable by construction.
    ///
    /// Distinct from [`Self::CompletionFailed`] because the outcomes differ for
    /// an operator: there, the turn failed; here, the turn succeeded and only
    /// the post-hoc projection of its session id did not. The resolution detail
    /// is carried verbatim and is deliberately NOT reclassified - this is a
    /// distinct variant, and the delivery path's substring classifier only ever
    /// sees admission errors, never this one.
    PostAdmissionResolutionFailed(String),
    /// This bridge does not implement completion-bearing delivery.
    ///
    /// The completion-bearing methods default to this so that every existing
    /// [`SessionBridge`] implementor keeps compiling unchanged; only the
    /// concrete mob bridge, which can reach meerkat's `WorkTurnHandle`,
    /// overrides them.
    ///
    /// Returned BEFORE any submission, so a caller that sees it knows nothing
    /// was delivered. **A caller that needs completion must fail closed here.**
    /// Do NOT fall back to the ingress-only `deliver_*` methods: those return
    /// at admission, so falling back would report success for a turn whose
    /// outcome is unknown - which is precisely the false-success this seam
    /// exists to remove.
    CompletionUnsupported(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(msg) => write!(f, "session bridge mob error: {msg}"),
            Self::CompletionFailed(msg) => write!(
                f,
                "session bridge turn was admitted and then failed: {msg}; the turn RAN - \
                 do not retry"
            ),
            Self::PostAdmissionResolutionFailed(msg) => write!(
                f,
                "session bridge turn was admitted and COMPLETED, then resolving its runtime \
                 session id failed: {msg}; the turn RAN - do not retry"
            ),
            Self::CompletionUnsupported(msg) => write!(
                f,
                "session bridge does not support completion-bearing delivery: {msg}; \
                 nothing was submitted"
            ),
            Self::InvalidInput(msg) => write!(f, "session bridge invalid input: {msg}"),
            Self::ResumeRejected { kind, detail } => write!(
                f,
                "session bridge resume rejected ({kind:?}): {detail}; durable session preserved, \
                 identity degraded pending retry"
            ),
            Self::ActorAdmissionTimeout {
                operation,
                identity,
                waited,
            } => write!(
                f,
                "session bridge actor call `{operation}` for member {identity} exceeded the \
                 admission budget after {waited:?}; the mob actor command loop is not draining"
            ),
        }
    }
}

impl std::error::Error for BridgeError {}

/// Errors that can occur before a completion-bearing delivery has produced an
/// admitted-turn receipt.
///
/// This enum deliberately cannot represent a terminal failure or a
/// post-admission projection failure. Those states exist only in
/// [`BridgeTurnError`], returned by [`BridgeTurnReceipt::wait`]. Keeping the
/// two transition errors disjoint makes it impossible to report "the turn
/// ran" from the admission edge of the state machine.
#[derive(Debug)]
pub enum BridgeAdmissionError {
    /// The bridge cannot create completion-bearing receipts. Nothing was
    /// submitted.
    CompletionUnsupported(String),
    /// The underlying mob operation failed before admission.
    Mob(String),
    /// A required field was missing or invalid before admission.
    InvalidInput(String),
    /// Resume was rejected before the delivery could be admitted.
    ResumeRejected {
        kind: ResumeRejectionKind,
        detail: String,
    },
    /// The serialized actor admission round trip exceeded its budget.
    ActorAdmissionTimeout {
        operation: &'static str,
        identity: MobAgentIdentity,
        waited: Duration,
    },
    /// A legacy bridge path produced a post-admission error before a receipt
    /// existed. This is an implementation invariant failure, not a terminal
    /// outcome, and no turn may be claimed to have run from it.
    InvariantViolation(String),
}

impl std::fmt::Display for BridgeAdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CompletionUnsupported(msg) => write!(
                f,
                "session bridge does not support completion-bearing delivery: {msg}; \
                 nothing was submitted"
            ),
            Self::Mob(msg) => write!(f, "session bridge mob error before admission: {msg}"),
            Self::InvalidInput(msg) => {
                write!(f, "session bridge invalid input before admission: {msg}")
            }
            Self::ResumeRejected { kind, detail } => write!(
                f,
                "session bridge resume rejected before admission ({kind:?}): {detail}; \
                 durable session preserved, identity degraded pending retry"
            ),
            Self::ActorAdmissionTimeout {
                operation,
                identity,
                waited,
            } => write!(
                f,
                "session bridge actor call `{operation}` for member {identity} exceeded the \
                 admission budget after {waited:?}; the mob actor command loop is not draining"
            ),
            Self::InvariantViolation(msg) => {
                write!(f, "session bridge admission invariant violated: {msg}")
            }
        }
    }
}

impl std::error::Error for BridgeAdmissionError {}

impl From<BridgeError> for BridgeAdmissionError {
    fn from(error: BridgeError) -> Self {
        match error {
            BridgeError::CompletionUnsupported(detail) => Self::CompletionUnsupported(detail),
            BridgeError::Mob(detail) => Self::Mob(detail),
            BridgeError::InvalidInput(detail) => Self::InvalidInput(detail),
            BridgeError::ResumeRejected { kind, detail } => Self::ResumeRejected { kind, detail },
            BridgeError::ActorAdmissionTimeout {
                operation,
                identity,
                waited,
            } => Self::ActorAdmissionTimeout {
                operation,
                identity,
                waited,
            },
            BridgeError::CompletionFailed(detail) => Self::InvariantViolation(format!(
                "completion failure escaped before an admitted-turn receipt existed: {detail}"
            )),
            BridgeError::PostAdmissionResolutionFailed(detail) => {
                Self::InvariantViolation(format!(
                    "post-admission projection failure escaped before an admitted-turn receipt \
                     existed: {detail}"
                ))
            }
        }
    }
}

impl From<BridgeAdmissionError> for BridgeError {
    fn from(error: BridgeAdmissionError) -> Self {
        match error {
            BridgeAdmissionError::CompletionUnsupported(detail) => {
                Self::CompletionUnsupported(detail)
            }
            BridgeAdmissionError::Mob(detail) => Self::Mob(detail),
            BridgeAdmissionError::InvalidInput(detail) => Self::InvalidInput(detail),
            BridgeAdmissionError::ResumeRejected { kind, detail } => {
                Self::ResumeRejected { kind, detail }
            }
            BridgeAdmissionError::ActorAdmissionTimeout {
                operation,
                identity,
                waited,
            } => Self::ActorAdmissionTimeout {
                operation,
                identity,
                waited,
            },
            BridgeAdmissionError::InvariantViolation(detail) => Self::Mob(detail),
        }
    }
}

/// Errors from an exact turn receipt after admission has already occurred.
///
/// Admission-only states are unrepresentable here. A caller holding this error
/// knows a turn was admitted and therefore must never repair-and-resubmit it.
#[derive(Debug)]
pub enum BridgeTurnError {
    /// The admitted turn reached a failed terminal. When session resolution
    /// also failed, its detail is retained as secondary evidence.
    CompletionFailed(String),
    /// The admitted turn succeeded, but resolving its session id failed.
    PostAdmissionResolutionFailed(String),
}

impl std::fmt::Display for BridgeTurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CompletionFailed(detail) => write!(
                f,
                "session bridge turn was admitted and then failed: {detail}; the turn RAN - \
                 do not retry"
            ),
            Self::PostAdmissionResolutionFailed(detail) => write!(
                f,
                "session bridge turn was admitted and COMPLETED, then resolving its runtime \
                 session id failed: {detail}; the turn RAN - do not retry"
            ),
        }
    }
}

impl std::error::Error for BridgeTurnError {}

impl From<BridgeTurnError> for BridgeError {
    fn from(error: BridgeTurnError) -> Self {
        match error {
            BridgeTurnError::CompletionFailed(detail) => Self::CompletionFailed(detail),
            BridgeTurnError::PostAdmissionResolutionFailed(detail) => {
                Self::PostAdmissionResolutionFailed(detail)
            }
        }
    }
}

/// Typed classification of a rejected resume, derived from meerkat's typed
/// errors where available (report ask: don't bucket every resume error into
/// one "runtime_identity_incompatible" reason).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeRejectionKind {
    /// meerkat's typed restore failure (`MobError::MemberRestoreFailed`): the
    /// member is Broken-in-roster upstream with restore diagnostics preserved.
    MemberRestoreFailed,
    /// The session store rejected the resume as a transcript-continuity
    /// violation ("incoming transcript is not a continuation of persisted
    /// revision") — the meerkat ≤0.7.14 cold-restart re-projection class.
    TranscriptContinuity,
    /// meerkat's typed `SessionUnavailableForResume { reason:
    /// ArchivedNotRevivable }`: the durable document exists and is intact,
    /// but it carries an archived terminal whose runtime lifecycle pairing
    /// refuses revival (the 0.6.x body-carried dispose shape with no runtime
    /// record). A STABLE, deterministic wall: no retry can change the
    /// verdict, so consumers park the identity typed on the FIRST encounter
    /// (OB3 rehearsal: 4 identities heal/refusal-looped because the roster
    /// heal succeeded while this materialize precondition stayed terminal).
    /// Upstream revive-by-document-authority lands in meerkat 0.8.15; until
    /// then `mobkit/reset` is the deliberate fresh start.
    ArchivedNotRevivable,
    /// Any other resume-time failure.
    Other,
}

/// The terminal park reason recorded when a resume hits the typed
/// [`ResumeRejectionKind::ArchivedNotRevivable`] wall. One producer text for
/// both doors (eager restore and on-demand materialize) so operators see one
/// stable, greppable reason with the operator path inline.
pub(crate) fn archived_not_revivable_park_reason(
    session_id: &meerkat_core::types::SessionId,
    detail: &str,
) -> String {
    format!(
        "durable session {session_id} is archived and its runtime lifecycle refuses \
         revival (typed ArchivedNotRevivable): a stable verdict retries cannot change, \
         so continuity repair parks instead of heal-looping. The transcript is intact \
         and preserved. Operator path: upstream archived-session revive lands in \
         meerkat 0.8.15; until then reset the identity via `mobkit/reset` (deliberate \
         fresh start) or restart the gateway after an upstream fix (the park is \
         process-local). Refusal: {detail}"
    )
}

/// Log and construct the typed resume rejection for one failed resume step.
/// Deliberately loud: a rejected resume degrades the identity until a
/// reconcile retry succeeds, and the operator needs the real (classified)
/// error — not a generic fallback reason.
fn resume_rejected(
    identity: &AgentIdentity,
    session_id: &meerkat_core::types::SessionId,
    error: &meerkat_mob::MobError,
    step: &str,
) -> BridgeError {
    let kind = classify_resume_error(error);
    tracing::error!(
        identity = %identity,
        session_id = %session_id,
        kind = ?kind,
        step,
        error = %error,
        "resume rejected; durable session preserved, identity degraded pending reconcile retry \
         (refusing fresh-spawn fallback)"
    );
    BridgeError::ResumeRejected {
        kind,
        detail: format!("{step}: {error}"),
    }
}

/// Classify a resume-spawn failure. Typed variants first; the string probe is
/// belt-and-braces for continuity violations that reach us stringified through
/// the provisioning path (`MobError::Internal`).
fn classify_resume_error(error: &meerkat_mob::MobError) -> ResumeRejectionKind {
    if matches!(
        error,
        meerkat_mob::MobError::SessionUnavailableForResume {
            reason: meerkat_mob::error::SessionResumeUnavailableReason::ArchivedNotRevivable,
            ..
        }
    ) {
        return ResumeRejectionKind::ArchivedNotRevivable;
    }
    if matches!(error, meerkat_mob::MobError::MemberRestoreFailed { .. }) {
        return ResumeRejectionKind::MemberRestoreFailed;
    }
    let text = error.to_string();
    if text.contains("not a continuation of persisted revision")
        || text.contains("TranscriptContinuityViolation")
        || text.contains("continuity preflight")
    {
        return ResumeRejectionKind::TranscriptContinuity;
    }
    ResumeRejectionKind::Other
}

/// Typed reason a requested resume could not reuse the persisted runtime
/// binding and had to fall back to a fresh member spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeFallbackReason {
    /// The persisted session/runtime identity is incompatible with the current
    /// mob runtime binding.
    RuntimeIdentityIncompatible { detail: String },
    /// The continuity record points at a session that meerkat reports as
    /// typed-Absent AND the durable store confirms was never persisted (a
    /// registration/rebind-minted head for a quiet member whose content-less
    /// saves were skipped). There is no transcript to preserve, so a fresh
    /// spawn is legitimate recovery, not data loss.
    NeverPersisted { detail: String },
}

/// Result of attempting to materialize a persisted identity through resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeSessionOutcome {
    /// The persisted session was resumed as-is.
    Resumed {
        session_id: meerkat_core::types::SessionId,
    },
    /// Resume was rejected for a typed compatibility reason and a fresh member
    /// was spawned instead.
    FreshSpawned {
        session_id: meerkat_core::types::SessionId,
        reason: ResumeFallbackReason,
    },
}

impl ResumeSessionOutcome {
    #[must_use]
    pub fn session_id(&self) -> &meerkat_core::types::SessionId {
        match self {
            Self::Resumed { session_id } | Self::FreshSpawned { session_id, .. } => session_id,
        }
    }

    #[must_use]
    pub fn fallback_reason(&self) -> Option<&ResumeFallbackReason> {
        match self {
            Self::Resumed { .. } => None,
            Self::FreshSpawned { reason, .. } => Some(reason),
        }
    }
}

// ---------------------------------------------------------------------------
// Bounded actor admission
// ---------------------------------------------------------------------------

/// Default budget one delivery attempt may spend waiting on the mob actor's
/// command loop, across ALL of that attempt's round trips.
///
/// The bounded calls are ADMISSION round trips (`get_member`,
/// `submit_work_with_mode` at `IngressAccepted`): they hand a command to the
/// actor and return once it is admitted at runtime ingress — they never await
/// turn completion. Each is an in-process channel send plus a machine-state
/// read on a responsive actor, so healthy latency is orders of magnitude
/// inside this budget and it cannot fire on the healthy path.
///
/// The value is therefore not sized against the healthy path but against the
/// one legitimate reason admission is slow: the mob actor is a single
/// serialized command loop, so an admission call can queue behind another
/// member's entire in-flight turn. Ten minutes is ~6.7x the 90s this crate
/// already treats as the outer bound for waiting on a turn's *output*
/// (`IdentityRuntimeConfig::default_timeout`) and ~3.3x the worst measured
/// single-turn latency in the deployment that motivated this bound (180s for
/// a one-word turn on a 94 MB document). It sits deliberately BELOW the
/// 962-second dispatch hang observed in production, so the pathology this
/// exists to name still produces a signal.
///
/// Trade-off, stated plainly: ten minutes is a terrible wait and a much
/// shorter bound would diagnose far faster. Shorter was rejected — at 30-90s
/// the timeout would fire while another member is merely running a long tool
/// chain, converting a latency problem into dropped deliveries, which is
/// worse than no bound at all. Deployments whose turns are known to be short
/// should tighten this through the env knob below.
const BRIDGE_ACTOR_ADMISSION_BUDGET: Duration = Duration::from_mins(10);

/// Effective admission budget: `MOBKIT_BRIDGE_ACTOR_ADMISSION_SECS` overrides
/// the default, clamped to [1, 3600] seconds. The floor keeps `0` from
/// meaning "reject everything"; the ceiling keeps a mistyped value from
/// silently restoring the unbounded hang this replaces.
fn bridge_actor_admission_budget() -> Duration {
    parse_bridge_actor_admission_budget(
        std::env::var("MOBKIT_BRIDGE_ACTOR_ADMISSION_SECS")
            .ok()
            .as_deref(),
    )
}

fn parse_bridge_actor_admission_budget(raw: Option<&str>) -> Duration {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .map(|secs| Duration::from_secs(secs.clamp(1, 3600)))
        .unwrap_or(BRIDGE_ACTOR_ADMISSION_BUDGET)
}

/// One delivery attempt's shared deadline for mob-actor round trips.
///
/// `MobHandle::send_actor_command` awaits both the command send and the reply
/// with no timeout, so a blocked actor hangs the dispatch silently. Every
/// round trip in a delivery attempt runs under this ONE deadline rather than
/// a per-call timeout: the delivery path makes three or four serialized round
/// trips, and three serialized bounds would cost three budgets — a worse
/// worst case than the hang being contained.
struct ActorAdmissionDeadline {
    started: tokio::time::Instant,
    deadline: tokio::time::Instant,
}

impl ActorAdmissionDeadline {
    fn new(budget: Duration) -> Self {
        let started = tokio::time::Instant::now();
        Self {
            started,
            deadline: started + budget,
        }
    }

    /// Await one actor round trip under the attempt's remaining budget. A
    /// responsive actor takes the `timeout_at` pass-through: no added
    /// latency, no allocation, no behavioural change. Expiry is the only path
    /// that logs or allocates.
    async fn bound<T, F>(
        &self,
        operation: &'static str,
        identity: &MobAgentIdentity,
        call: F,
    ) -> Result<T, BridgeError>
    where
        F: Future<Output = T>,
    {
        match tokio::time::timeout_at(self.deadline, call).await {
            Ok(value) => Ok(value),
            Err(_) => {
                let waited = self.started.elapsed();
                tracing::warn!(
                    operation,
                    identity = %identity,
                    waited_ms = waited.as_millis(),
                    "mob actor did not answer within the delivery admission budget; the actor \
                     command loop is not draining (head-of-line block) — delivery abandoned"
                );
                Err(BridgeError::ActorAdmissionTimeout {
                    operation,
                    identity: identity.clone(),
                    waited,
                })
            }
        }
    }
}

/// Build the internal-delivery [`WorkSpec`] for one member turn.
///
/// Pure by design so the threading is unit-testable: every optional carrier
/// on the deliver surface must reach the spec unchanged.
///
/// - Ask 1: `injected_context` rides as separate typed bodies rather than
///   being fused into the user's message text; WorkSpec carries it to the
///   StartTurnRequest, where meerkat stamps each entry as the
///   InjectedContext transcript role (excluded from compaction indexing).
/// - meerkat 0.8.11: `system_prompt` is one ordinary System message authored
///   for this exact turn (never member/session configuration); WorkSpec
///   carries it to the member StartTurnRequest, where meerkat appends it at
///   the turn's admitted transcript boundary.
/// - meerkat 0.7.25 ask 15 addendum: a host-supplied interaction id rides
///   WorkSpec into runtime admission, so this turn's live events AND its
///   committed transcript messages carry the SAME id the console minted at
///   send time — the exact live↔history join the console dedup needs. Only
///   UUID-form ids exist here (the identity-first console send mints v5
///   UUIDs); anything else is skipped rather than corrupted.
fn internal_bridge_work_spec(
    content: &meerkat_core::ContentInput,
    system_prompt: Option<&str>,
    injected_context: &[meerkat_core::ContentInput],
    interaction_id: Option<&str>,
) -> WorkSpec {
    let mut spec = WorkSpec::new(content.clone(), WorkOrigin::Internal);
    if let Some(system_prompt) = system_prompt {
        spec = spec.with_system_prompt(system_prompt);
    }
    if !injected_context.is_empty() {
        spec = spec.with_injected_context(injected_context.to_vec());
    }
    if let Some(raw) = interaction_id {
        match raw.parse::<uuid::Uuid>() {
            Ok(id) => {
                spec = spec.with_interaction_id(meerkat_core::interaction::InteractionId(id));
            }
            Err(_) => {
                tracing::debug!(
                    interaction_id = %raw,
                    "non-UUID interaction id not threaded into runtime admission"
                );
            }
        }
    }
    spec
}

/// Spec-shaping inputs for one internal bridge delivery, grouped so the
/// submit path carries them as a unit rather than five loose parameters.
struct InternalBridgeWork<'a> {
    content: &'a meerkat_core::ContentInput,
    system_prompt: Option<&'a str>,
    injected_context: &'a [meerkat_core::ContentInput],
    interaction_id: Option<&'a str>,
    /// meerkat 0.8.15 internal-lane dedup: when present, the submit goes
    /// through `submit_work_with_mode_and_delivery_identity` and meerkat
    /// derives the WorkRef from mob + member identity + idempotency key -
    /// stable across lease-expiry reclaim, so a crash redelivery of the
    /// same identity resolves to the SAME work instead of a duplicate turn.
    delivery_identity: Option<&'a meerkat_mob::MobDeliveryIdentity>,
}

/// Which lane a submission runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeSubmitMode {
    /// Return as soon as the work is admitted. The historical behaviour.
    AdmissionOnly,
    /// Admit through the completion-bearing verb and hand the caller the turn
    /// handle to await.
    CompletionBearing,
}

/// Admit one internal bridge delivery.
///
/// [`BridgeSubmitMode::AdmissionOnly`] is the ingress-only path and is
/// unchanged: it returns as soon as the work is admitted, yielding `None`.
///
/// [`BridgeSubmitMode::CompletionBearing`] admits through meerkat's
/// `start_work_with_mode*` and returns the resulting [`WorkTurnHandle`]
/// UNAWAITED. This function never awaits a turn: the caller decides when, and
/// does so only after the repair-and-retry decision is settled, so a turn that
/// ran and then failed can never be mistaken for a delivery that never landed.
///
/// One code path with one typed mode, deliberately - a separate copy would let
/// the completion lane drift from the ingress lane on admission-budget or
/// stale-state repair semantics, which are exactly the parts that are hard to
/// get right twice.
///
/// `deadline` bounds the ACTOR ROUND TRIP only. Completion is not awaited here
/// at all, so nothing in this function can spend the admission budget on a
/// turn.
async fn submit_internal_bridge_work(
    handle: &MobHandle,
    member_id: &MobAgentIdentity,
    work: InternalBridgeWork<'_>,
    handling_mode: HandlingMode,
    deadline: &ActorAdmissionDeadline,
    mode: BridgeSubmitMode,
) -> Result<Option<meerkat_mob::WorkTurnHandle>, BridgeError> {
    let entry = deadline
        .bound(
            "deliver.get_member",
            member_id,
            handle.get_member(member_id),
        )
        .await?
        .map_err(|err| BridgeError::Mob(err.to_string()))?
        .ok_or_else(|| BridgeError::Mob(format!("member not found: {member_id}")))?;
    let spec = internal_bridge_work_spec(
        work.content,
        work.system_prompt,
        work.injected_context,
        work.interaction_id,
    );
    if matches!(mode, BridgeSubmitMode::CompletionBearing) {
        let turn = match work.delivery_identity {
            Some(delivery_identity) => deadline
                .bound(
                    "deliver.start_work",
                    member_id,
                    handle.start_work_with_mode_and_delivery_identity(
                        entry.agent_runtime_id.clone(),
                        entry.fence_token,
                        spec,
                        handling_mode,
                        delivery_identity.clone(),
                    ),
                )
                .await?
                .map_err(|err| BridgeError::Mob(err.to_string()))?,
            None => deadline
                .bound(
                    "deliver.start_work",
                    member_id,
                    handle.start_work_with_mode(
                        entry.agent_runtime_id.clone(),
                        entry.fence_token,
                        WorkRef::new(),
                        spec,
                        handling_mode,
                    ),
                )
                .await?
                .map_err(|err| BridgeError::Mob(err.to_string()))?,
        };
        // Hand the handle back UNAWAITED. The caller awaits it only after the
        // admission-retry decision is settled, so a turn that ran and then
        // failed can never be mistaken for a delivery that never landed and
        // resubmitted - which would run the member's turn twice.
        return Ok(Some(turn));
    }
    match work.delivery_identity {
        Some(delivery_identity) => deadline
            .bound(
                "deliver.submit_work",
                member_id,
                handle.submit_work_with_mode_and_delivery_identity(
                    entry.agent_runtime_id.clone(),
                    entry.fence_token,
                    spec,
                    handling_mode,
                    delivery_identity.clone(),
                ),
            )
            .await?
            .map(|_| None)
            .map_err(|err| BridgeError::Mob(err.to_string())),
        None => deadline
            .bound(
                "deliver.submit_work",
                member_id,
                handle.submit_work_with_mode(
                    entry.agent_runtime_id.clone(),
                    entry.fence_token,
                    WorkRef::new(),
                    spec,
                    handling_mode,
                ),
            )
            .await?
            .map(|_| None)
            .map_err(|err| BridgeError::Mob(err.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Committed-boundary heal seam (2026-07-29 heal/re-Break incident)
// ---------------------------------------------------------------------------

/// Typed outcome of a committed-boundary heal attempt against the durable
/// session head — the mobkit-side mirror of meerkat's
/// `CommittedBoundaryRecovery`.
///
/// The continuity repair supervisor consults this BEFORE re-registering a
/// Broken identity as healable. Without a real recovery step, "heal" only
/// reset the runtime entry while the durable head stayed an intra-turn
/// projection, so the next materialization re-Broke the identity — measured
/// in production (2026-07-29) as an infinite heal/re-Break loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommittedBoundaryRepair {
    /// The durable head is already strict-resume-acceptable; nothing written.
    AlreadyCommitted,
    /// Machine-authorized recovery persisted a committed boundary head; a
    /// subsequent resume of the durable session is expected to succeed.
    Recovered,
    /// Terminal typed verdict: the proof inputs for a committed boundary are
    /// absent (or the machine held). Stable across calls — callers must NOT
    /// retry-loop it; surface `reason` to operators instead.
    Unprovable { reason: String },
    /// This bridge exposes no heal seam. Callers keep the legacy behavior
    /// (reconcile retries the resume directly).
    Unsupported,
}

/// Host-injectable authority that can drive the durable session head to a
/// strict-resume-acceptable committed boundary.
///
/// The production implementation wraps meerkat's
/// `PersistentSessionService::recover_committed_boundary`; it is injected
/// into [`MobSessionBridge`] at composition time because the bridge only
/// holds the session service as `dyn MobSessionService`, which does not
/// expose the concrete heal API.
#[async_trait]
pub trait CommittedBoundaryRecoverer: Send + Sync {
    /// Attempt recovery for the given durable session.
    ///
    /// # Errors
    ///
    /// `Err` is reserved for genuinely retryable failures (a live session
    /// owning the head mid-turn, store I/O, CAS races). A terminal verdict is
    /// `Ok(CommittedBoundaryRepair::Unprovable { .. })`, never an error.
    async fn recover_committed_boundary(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<CommittedBoundaryRepair, BridgeError>;
}

/// The production heal authority: meerkat's `PersistentSessionService`
/// driving the durable head to a boundary-commit-provenance checkpoint
/// through machine-authorized recovery (meerkat >= 0.8.11).
///
/// Contract mapping, per the heal API: the typed `Unprovable` VERDICT is
/// terminal and stable across calls (callers must not retry-loop it); only
/// the error tier (`Busy` mid-turn, store I/O, CAS races) is retryable and
/// maps to `Err` here.
#[async_trait]
impl<B> CommittedBoundaryRecoverer for meerkat_session::PersistentSessionService<B>
where
    B: meerkat_session::SessionAgentBuilder + 'static,
{
    async fn recover_committed_boundary(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<CommittedBoundaryRepair, BridgeError> {
        // Fully qualified: the inherent method and this trait method share a
        // name, and the inherent one is the upstream API being consumed.
        match meerkat_session::PersistentSessionService::recover_committed_boundary(
            self, session_id,
        )
        .await
        {
            Ok(meerkat_session::CommittedBoundaryRecovery::AlreadyCommitted) => {
                Ok(CommittedBoundaryRepair::AlreadyCommitted)
            }
            Ok(meerkat_session::CommittedBoundaryRecovery::Recovered { message_count }) => {
                tracing::info!(
                    %session_id,
                    message_count,
                    "machine-authorized recovery persisted a committed durable head"
                );
                Ok(CommittedBoundaryRepair::Recovered)
            }
            Ok(meerkat_session::CommittedBoundaryRecovery::Unprovable { reason }) => {
                Ok(CommittedBoundaryRepair::Unprovable { reason })
            }
            Err(error) => map_committed_boundary_recovery_error(error),
        }
    }
}

/// Disposition of the heal authority's error tier, per the heal contract:
/// `Err` is reserved for failures retrying can genuinely clear (`Busy`
/// mid-turn, store I/O, CAS races, a held tail awaiting the recovery commit
/// itself). Typed refusals that only an EXTERNAL change can clear — a
/// conflicting live runtime quiescing (`DurableTailRecoveryRefused`) or an
/// operator resolving forked evidence (`DurableEvidenceQuarantined`) — are
/// terminal `Unprovable` verdicts. Letting them escape as `Err` loops the
/// reconcile repair pass forever against a verdict no retry can change,
/// instead of parking the identity with the refusal in front of an operator.
fn map_committed_boundary_recovery_error(
    error: meerkat_core::SessionError,
) -> Result<CommittedBoundaryRepair, BridgeError> {
    match error {
        error @ (meerkat_core::SessionError::DurableTailRecoveryRefused { .. }
        | meerkat_core::SessionError::DurableEvidenceQuarantined { .. }) => {
            Ok(CommittedBoundaryRepair::Unprovable {
                reason: error.to_string(),
            })
        }
        error => Err(BridgeError::Mob(format!(
            "committed-boundary recovery: {error}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// SessionBridge trait
// ---------------------------------------------------------------------------

/// One fully-admitted member delivery: everything a bridge needs to hand a
/// turn to the mob work lane, as ONE typed request instead of a growing
/// parameter staircase. The identity runtime's shared delivery preparation
/// (defang, taint session attribution, ambient memory injection) is exactly
/// the code that POPULATES this request before
/// [`SessionBridge::deliver_admitted`].
///
/// `delivery_identity` is the meerkat 0.8.15 internal-lane dedup carrier
/// (idempotency key + occurrence-correlation UUID, validated upstream). Its
/// semantics are fail-closed end to end: the runtime refuses half-formed or
/// non-canonical pairs typed BEFORE admission, and an implementation that
/// cannot honor a present identity must refuse typed rather than deliver
/// without dedup. When the identity is present its correlation id also
/// rides as `interaction_id`.
#[derive(Debug, Clone)]
pub struct BridgeDelivery {
    pub content: meerkat_core::ContentInput,
    pub handling_mode: HandlingMode,
    /// Per-turn System message (meerkat 0.8.11 `WorkSpec::system_prompt`).
    pub system_prompt: Option<String>,
    /// Typed ambient injection bodies delivered alongside - never fused
    /// into - the user content (meerkat 0.7.12 ask 1).
    pub injected_context: Vec<meerkat_core::ContentInput>,
    /// Host-minted interaction id threaded into runtime admission
    /// (meerkat 0.7.25 ask 15 addendum).
    pub interaction_id: Option<String>,
    /// Internal-lane dedup identity (see the struct docs).
    pub delivery_identity: Option<meerkat_mob::MobDeliveryIdentity>,
}

impl BridgeDelivery {
    pub fn new(content: meerkat_core::ContentInput, handling_mode: HandlingMode) -> Self {
        Self {
            content,
            handling_mode,
            system_prompt: None,
            injected_context: Vec::new(),
            interaction_id: None,
            delivery_identity: None,
        }
    }
}

/// An ADMITTED turn, owned by the caller and awaitable OUTSIDE any lock.
///
/// This type exists for one reason: the identity lifecycle lock must cover
/// validation, admission and bounded session resolution, and NOTHING else.
/// Awaiting an LLM turn under that lock serialises same-identity sends behind a
/// model call and blocks every lifecycle operation - reset, retire, alias
/// rebind - for the turn's whole duration.
///
/// [`Self::wait`] takes `self` BY VALUE, which prevents awaiting one twice.
///
/// It does NOT make dropping one impossible: a receipt can still be dropped
/// without being awaited, abandoning a running turn. `#[must_use]` makes the
/// common case of ignoring the return value a warning, and that is the whole of
/// the protection - the rest is the caller's discipline.
#[must_use = "dropping a BridgeTurnReceipt abandons an ADMITTED, RUNNING turn without ever \
              observing its terminal"]
pub struct BridgeTurnReceipt {
    /// Session resolution ALREADY RAN, under the admission deadline, before the
    /// caller was handed this receipt.
    ///
    /// Carried as a `Result` and deliberately NOT `?`-propagated at admission
    /// time: returning early on a resolution failure would drop the completion
    /// handle of a turn that is already running. The pair is mapped in
    /// [`Self::wait`], after the turn has reached terminal.
    session_result: Result<meerkat_core::types::SessionId, String>,
    /// `'static` because the receipt is held across an UNLOCK, so it must not
    /// borrow from the bridge or from any guard. `Send` because the runtime's
    /// send future must stay `Send`.
    completion: std::pin::Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>>,
}

impl std::fmt::Debug for BridgeTurnReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeTurnReceipt")
            .field("session_result", &self.session_result)
            .finish_non_exhaustive()
    }
}

impl BridgeTurnReceipt {
    /// Build a receipt from an admitted turn.
    ///
    /// Generic over the completion future so out-of-crate [`SessionBridge`]
    /// implementors can build one from whatever handle they own, without
    /// naming a boxed type.
    ///
    /// Construct this the moment the work is admitted and BEFORE inspecting
    /// `session_result`, so no code path can inspect a failure and return
    /// without carrying the completion handle forward.
    pub fn new<F, ResolutionError, CompletionError>(
        session_result: Result<meerkat_core::types::SessionId, ResolutionError>,
        completion: F,
    ) -> Self
    where
        F: Future<Output = Result<(), CompletionError>> + Send + 'static,
        ResolutionError: std::fmt::Display,
        CompletionError: std::fmt::Display,
    {
        Self {
            session_result: session_result.map_err(|error| error.to_string()),
            completion: Box::pin(
                async move { completion.await.map_err(|error| error.to_string()) },
            ),
        }
    }

    /// The session this turn was admitted onto, if resolution succeeded.
    ///
    /// Read-only: the field is private so no caller can inspect resolution,
    /// decide it failed, and return without awaiting the running turn.
    pub fn resolved_session(&self) -> Option<&meerkat_core::types::SessionId> {
        self.session_result.as_ref().ok()
    }

    /// Await this turn's terminal, then map the (resolution, terminal) pair.
    ///
    /// | resolution | terminal | result                                        |
    /// |------------|----------|-----------------------------------------------|
    /// | `Ok`       | `Ok`     | `Ok(session_id)`                              |
    /// | `Ok`       | `Err`    | [`BridgeTurnError::CompletionFailed`]         |
    /// | `Err`      | `Err`    | `CompletionFailed`, carrying both details     |
    /// | `Err`      | `Ok`     | [`BridgeTurnError::PostAdmissionResolutionFailed`]|
    ///
    /// There is no early return: the turn is already running, so its terminal
    /// is always awaited before any error is produced.
    pub async fn wait(self) -> Result<meerkat_core::types::SessionId, BridgeTurnError> {
        let terminal_result = self.completion.await;
        match (self.session_result, terminal_result) {
            (Ok(session_id), Ok(())) => Ok(session_id),
            (Ok(_), Err(err)) => Err(BridgeTurnError::CompletionFailed(err)),
            (Err(resolve_err), Err(err)) => Err(BridgeTurnError::CompletionFailed(format!(
                "{err}; post-admission session resolution also failed: {resolve_err}"
            ))),
            (Err(resolve_err), Ok(())) => {
                Err(BridgeTurnError::PostAdmissionResolutionFailed(resolve_err))
            }
        }
    }
}

/// Bridge between the identity-first control plane and the Meerkat session
/// pipeline. Each method maps an identity-layer operation to its concrete
/// mob-level counterpart.
#[async_trait]
pub trait SessionBridge: Send + Sync {
    /// Whether the lower mob plane already contains a caller-owned raw member
    /// whose public alias collides with a durable identity. Identity restore
    /// uses this under the shared namespace reservation before publishing or
    /// materializing a durable owner.
    async fn raw_member_alias_exists(&self, _alias: &str) -> Result<bool, BridgeError> {
        Ok(false)
    }

    /// Spawn a new mob member for a freshly-created identity.
    async fn create_session(
        &self,
        identity: &AgentIdentity,
        runtime_id: &AgentRuntimeId,
        spec: &DurableAgentSpec,
        draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::types::SessionId, BridgeError>;

    /// Whether `resume_session` consumes the continuity-store snapshot payload.
    ///
    /// The default is deliberately conservative for compatibility with custom
    /// bridges: restore callers must load and pass the persisted snapshot unless
    /// an implementation explicitly declares that session-id-based resume is
    /// sufficient. Implementations returning `false` must not inspect the
    /// `snapshot` argument; bootstrap callers may pass an empty payload.
    fn requires_resume_snapshot(&self) -> bool {
        true
    }

    /// Resume a mob member from a previously checkpointed snapshot.
    ///
    /// The `session_id` comes from the ContinuityRecord — it's the session
    /// that should be loaded from the session store. When the session store
    /// has the data, this performs a true resume (conversation history intact).
    /// When the session is missing, implementations should fall back to a
    /// fresh spawn.
    async fn resume_session(
        &self,
        identity: &AgentIdentity,
        runtime_id: &AgentRuntimeId,
        spec: &DurableAgentSpec,
        draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
        snapshot: &SessionSnapshot,
    ) -> Result<ResumeSessionOutcome, BridgeError>;

    /// Deliver ONE fully-admitted request to an active mob member. The
    /// SINGLE authority-bearing delivery method: every convenience form
    /// below is a provided forwarder that BUILDS a [`BridgeDelivery`], so no
    /// implementation can receive delivery authority it did not explicitly
    /// accept. REQUIRED on purpose (the optional-default authority-drop bug
    /// class): an implementation that cannot honor
    /// `delivery.delivery_identity` must FAIL CLOSED with a typed error -
    /// a caller holding an identity gets dedup or a refusal, never a silent
    /// at-least-once downgrade.
    async fn deliver_admitted(
        &self,
        runtime_id: &AgentRuntimeId,
        delivery: BridgeDelivery,
    ) -> Result<meerkat_core::types::SessionId, BridgeError>;

    /// Deliver content to an active mob member.
    async fn deliver(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        self.deliver_admitted(
            runtime_id,
            BridgeDelivery::new(content.clone(), HandlingMode::Queue),
        )
        .await
    }

    /// Deliver content to an active mob member using a caller-selected turn
    /// handling mode.
    async fn deliver_with_mode(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        self.deliver_admitted(
            runtime_id,
            BridgeDelivery::new(content.clone(), handling_mode),
        )
        .await
    }

    /// Deliver content plus a separate `injected_context` body (meerkat
    /// 0.7.12 ask 1: typed ambient injection alongside — not fused into —
    /// the user's message) and an optional host-minted interaction id
    /// (meerkat 0.7.25 ask 15 addendum: threaded into runtime admission so
    /// transcript messages join the caller's live interaction frames).
    async fn deliver_with_mode_and_context(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        injected_context: &[meerkat_core::ContentInput],
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let mut delivery = BridgeDelivery::new(content.clone(), handling_mode);
        delivery.injected_context = injected_context.to_vec();
        delivery.interaction_id = interaction_id.map(ToString::to_string);
        self.deliver_admitted(runtime_id, delivery).await
    }

    /// [`Self::deliver_with_mode_context_and_system_prompt`], but returning
    /// only after the runtime has COMMITTED this turn's terminal boundary.
    ///
    /// The ingress-only `deliver_*` methods above are unchanged and remain the
    /// normal path: they return as soon as the work is admitted, which is what
    /// production callers want. This sibling exists for callers that need proof
    /// the turn actually finished - principally tests, which otherwise infer
    /// completion from a timer or from a session-wide event stream. Both
    /// inferences are unsound: a timer elapses whether or not the turn ran, and
    /// a session-wide stream cannot attribute a terminal to one specific
    /// delivery.
    ///
    /// Defaults to [`BridgeError::CompletionUnsupported`] so that every
    /// existing implementor compiles unchanged. The concrete mob bridge
    /// overrides it using meerkat's `MobHandle::start_work_with_mode*` and
    /// `WorkTurnHandle::wait`, which carry an exact per-work-item completion
    /// signal.
    async fn deliver_awaiting_commit_with_mode_context_and_system_prompt(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        system_prompt: Option<&str>,
        injected_context: &[meerkat_core::ContentInput],
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let receipt = self
            .begin_awaiting_commit(
                runtime_id,
                content,
                system_prompt,
                injected_context,
                handling_mode,
                interaction_id,
            )
            .await
            .map_err(BridgeError::from)?;
        receipt.wait().await.map_err(BridgeError::from)
    }

    /// Admit a turn and hand back a [`BridgeTurnReceipt`] the caller can await
    /// LATER - crucially, after releasing whatever lock it holds.
    ///
    /// This is the primitive;
    /// [`Self::deliver_awaiting_commit_with_mode_context_and_system_prompt`] is
    /// `begin` + `wait` composed, and exists only for callers with no lock to
    /// release.
    ///
    /// Returns only after BOUNDED admission plus BOUNDED session resolution -
    /// both inside the admission deadline, neither of them the LLM turn. The
    /// resolution outcome rides in `session_result` rather than being
    /// `?`-propagated, because an early return after admission drops the handle
    /// of a turn that is already running.
    ///
    /// Defaults to [`BridgeAdmissionError::CompletionUnsupported`] BEFORE any
    /// submit, so a caller that sees it knows nothing was delivered.
    async fn begin_awaiting_commit(
        &self,
        _runtime_id: &AgentRuntimeId,
        _content: &meerkat_core::ContentInput,
        _system_prompt: Option<&str>,
        _injected_context: &[meerkat_core::ContentInput],
        _handling_mode: HandlingMode,
        _interaction_id: Option<&str>,
    ) -> Result<BridgeTurnReceipt, BridgeAdmissionError> {
        Err(BridgeAdmissionError::CompletionUnsupported(
            "this bridge implements ingress-only delivery".to_string(),
        ))
    }

    /// Deliver content plus one ordinary System message authored for this
    /// exact turn (meerkat 0.8.11: `WorkSpec::system_prompt`, appended at the
    /// turn's admitted transcript boundary — per-turn content, never
    /// member/session configuration), alongside the optional injected
    /// context and interaction identity of
    /// [`Self::deliver_with_mode_and_context`].
    async fn deliver_with_mode_context_and_system_prompt(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        system_prompt: Option<&str>,
        injected_context: &[meerkat_core::ContentInput],
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let mut delivery = BridgeDelivery::new(content.clone(), handling_mode);
        delivery.system_prompt = system_prompt.map(ToString::to_string);
        delivery.injected_context = injected_context.to_vec();
        delivery.interaction_id = interaction_id.map(ToString::to_string);
        self.deliver_admitted(runtime_id, delivery).await
    }

    /// Checkpoint the current session state for a mob member.
    async fn checkpoint_session(
        &self,
        runtime_id: &AgentRuntimeId,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<SessionSnapshot, BridgeError>;

    /// Retire a mob member.
    async fn retire_member(&self, runtime_id: &AgentRuntimeId) -> Result<(), BridgeError>;

    /// Retire a member whose session was superseded by a committed destructive
    /// reset, then remove the old session's bridge persistence authority.
    ///
    /// The default keeps compatibility for custom bridges that do not expose
    /// Meerkat's retained archive-cleanup anchor. The concrete Mob bridge
    /// overrides this with the quiesce / CAS-abandon / exact-retry protocol.
    async fn retire_reset_superseded_member(
        &self,
        runtime_id: &AgentRuntimeId,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), BridgeError> {
        self.retire_member(runtime_id).await?;
        self.unregister_session_runtime_state(session_id).await
    }

    /// Wire two active same-mob members by their concrete runtime IDs.
    async fn wire_peer(&self, _a: &AgentRuntimeId, _b: &AgentRuntimeId) -> Result<(), BridgeError> {
        Err(BridgeError::Mob("peer wiring not supported".to_string()))
    }

    /// Wire many active same-mob member pairs by their concrete runtime IDs.
    async fn wire_peers_batch(
        &self,
        edges: &[(AgentRuntimeId, AgentRuntimeId)],
    ) -> Result<(), BridgeError> {
        for (a, b) in edges {
            self.wire_peer(a, b).await?;
        }
        Ok(())
    }

    /// Return currently materialized same-mob member wires by concrete runtime IDs.
    async fn current_member_wires(
        &self,
    ) -> Result<Vec<(AgentRuntimeId, AgentRuntimeId)>, BridgeError> {
        Ok(Vec::new())
    }

    /// Return edges with either directed half present. The healthy
    /// `current_member_wires` projection intentionally reports reciprocal
    /// edges only; mutation/recovery needs this stronger diagnostic surface
    /// to remove orphan halves.
    async fn current_member_wires_any_half(
        &self,
    ) -> Result<Vec<(AgentRuntimeId, AgentRuntimeId)>, BridgeError> {
        self.current_member_wires().await
    }

    /// Unwire two active same-mob members by their concrete runtime IDs.
    async fn unwire_peer(
        &self,
        _a: &AgentRuntimeId,
        _b: &AgentRuntimeId,
    ) -> Result<(), BridgeError> {
        Err(BridgeError::Mob("peer unwiring not supported".to_string()))
    }

    /// Inspect the current execution state of a mob member.
    async fn inspect_member(
        &self,
        _runtime_id: &AgentRuntimeId,
    ) -> Result<MemberInspection, BridgeError> {
        Err(BridgeError::Mob("inspect not supported".to_string()))
    }

    /// Register identity ownership for a concrete bridge session.
    ///
    /// Bridges that install a continuity-backed session store use this to
    /// ensure subsequent Meerkat session saves checkpoint under the durable
    /// identity/generation/fencing tuple.
    async fn register_session_runtime_state(
        &self,
        _session_id: &meerkat_core::types::SessionId,
        _identity: &AgentIdentity,
        _generation: ContinuityGeneration,
        checkpoint_version: CheckpointVersion,
        _fencing_token: FencingToken,
    ) -> Result<CheckpointVersion, BridgeError> {
        Ok(checkpoint_version)
    }

    /// Temporarily quiesce persistence for a concrete bridge session while
    /// external lease authority is rotated. Implementations must drain writes
    /// admitted before this call and reject later mutations until a successful
    /// [`Self::register_session_runtime_state`] publishes the replacement
    /// fencing token. The compatibility default uses permanent unregister;
    /// bridges with an in-process session-store adapter should preserve state.
    async fn suspend_session_runtime_state(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), BridgeError> {
        self.unregister_session_runtime_state(session_id).await
    }

    /// Remove identity ownership metadata for a concrete bridge session.
    ///
    /// This is used when an identity lifecycle operation aborts after session
    /// runtime state was registered but before continuity commits.
    async fn unregister_session_runtime_state(
        &self,
        _session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), BridgeError> {
        Ok(())
    }

    /// Attempt to make the durable session head strict-resume-acceptable
    /// BEFORE the continuity repair supervisor re-registers its identity as
    /// healable (2026-07-29 heal/re-Break incident).
    ///
    /// The compatibility default declares no heal seam: custom bridges keep
    /// today's behavior, where reconcile retries the resume directly.
    async fn recover_committed_boundary(
        &self,
        _session_id: &meerkat_core::types::SessionId,
    ) -> Result<CommittedBoundaryRepair, BridgeError> {
        Ok(CommittedBoundaryRepair::Unsupported)
    }
}

/// Lightweight inspection of a mob member's current execution state.
#[derive(Debug, Clone)]
pub struct MemberInspection {
    pub output_preview: Option<String>,
    pub is_final: bool,
    pub peer_reachable_count: usize,
}

/// Which restored LLM-identity fields diverge from the profile declaration
/// with no resume-override mask covering them. Pure comparison seam behind
/// [`MobSessionBridge::log_unmasked_resume_divergence`]: a field counts only
/// when it is unmasked (durable metadata will win), declared (for provider —
/// an undeclared provider states no intent), and different.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnmaskedResumeDivergence {
    model: bool,
    provider: bool,
}

fn unmasked_resume_divergence(
    mask: &meerkat_core::service::ResumeOverrideMask,
    declared_model: &str,
    declared_provider: Option<meerkat_core::Provider>,
    restored_model: &str,
    restored_provider: meerkat_core::Provider,
) -> UnmaskedResumeDivergence {
    UnmaskedResumeDivergence {
        model: !mask.model && restored_model != declared_model,
        provider: !mask.provider
            && declared_provider.is_some_and(|declared| restored_provider != declared),
    }
}

// ---------------------------------------------------------------------------
// MobSessionBridge — real implementation backed by MobHandle
// ---------------------------------------------------------------------------

/// Concrete `SessionBridge` backed by a `MobHandle`.
///
/// `AgentRuntimeId` is usually used as the `MobAgentIdentity` at the mob layer. Real
/// external bindings are the exception: Meerkat's external peer names require
/// identifier-safe `<mob>/<profile>/<member>` segments, so the bridge maps the
/// runtime ID to the durable identity for those members.
pub struct MobSessionBridge {
    handle: MobHandle,
    /// Session store used for checkpoint (loading session data to serialize).
    session_store: Option<Arc<dyn meerkat::SessionStore>>,
    /// Session service used to project the live effective model for capability checks.
    session_service: Option<Arc<dyn MobSessionService>>,
    /// Continuity-backed session store, when installed by the identity-first builder.
    continuity_session_store: Option<Arc<ContinuitySessionStoreAdapter>>,
    runtime_members: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    runtime_sessions: Arc<tokio::sync::RwLock<HashMap<String, meerkat_core::types::SessionId>>>,
    /// Lazily-minted ops-owner bridge session for external (peer-only)
    /// members, used when the mob was created without machine-bound owner
    /// bridge-session authority. Stable for the bridge lifetime so every
    /// external member shares one generated operation owner.
    generated_external_owner_session: std::sync::OnceLock<meerkat_core::types::SessionId>,
    /// Heal authority for the continuity repair supervisor. `None` means no
    /// heal seam ([`CommittedBoundaryRepair::Unsupported`]); composition
    /// injects the concrete meerkat-backed recoverer where available.
    committed_boundary_recoverer: Option<Arc<dyn CommittedBoundaryRecoverer>>,
    /// Identities whose unmasked resume divergence was already logged this
    /// boot (the bridge lives for one boot). See
    /// [`Self::log_unmasked_resume_divergence`].
    resume_divergence_logged: std::sync::Mutex<std::collections::HashSet<String>>,
    /// Budget one delivery attempt may spend waiting on the mob actor.
    /// Resolved once at construction so the success path pays nothing per
    /// call. See [`BRIDGE_ACTOR_ADMISSION_BUDGET`].
    actor_admission_budget: Duration,
    /// Machine-level ingress authority for the repair disposal paths (OB3
    /// run 33758a41: repair destroyed 15 queued inputs with the member).
    /// Lets repair capture a member's queued/steered inputs BEFORE the
    /// destructive retire and re-admit them into the healed successor
    /// session. `None` (validation-only compositions) keeps the legacy
    /// destroy-on-dispose behavior, minus the carry observability.
    runtime_ingress_authority: Option<Arc<dyn meerkat_runtime::SessionServiceRuntimeExt>>,
}

/// One queued/steered member input captured before a repair disposal, for
/// re-admission into the healed successor session (OB3 run 33758a41: the
/// disposal destroyed 15 queued review inputs with the member).
struct CarriedMemberInput {
    original_input_id: meerkat_core::lifecycle::InputId,
    admission_sequence: Option<u64>,
    input: meerkat_runtime::Input,
}

/// Pre-disposal capture of one member session's pending machine ingress.
struct PendingIngressCapture {
    /// Inputs whose payload survives re-admission (Prompt / Peer /
    /// ExternalEvent classes), in admission order.
    carryable: Vec<CarriedMemberInput>,
    /// Pending inputs repair cannot carry: `(input id, class, reason)`.
    /// Logged loudly per item before disposal proceeds.
    uncarryable: Vec<(meerkat_core::lifecycle::InputId, &'static str, String)>,
}

impl PendingIngressCapture {
    fn empty() -> Self {
        Self {
            carryable: Vec::new(),
            uncarryable: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.carryable.is_empty() && self.uncarryable.is_empty()
    }
}

/// Re-mint the runtime identity of a carried input for successor admission.
///
/// The disposal durably terminalized the original admission (abandonment),
/// so the successor must admit a NEW input: reusing the original `InputId`
/// collides with the terminal ledger row, and reusing the idempotency key
/// would dedup the carry into that terminal record and silently drop it.
/// Everything else — content, handling mode, visibility, correlation — rides
/// unchanged.
fn remint_carried_input_identity(
    mut input: meerkat_runtime::Input,
) -> Option<meerkat_runtime::Input> {
    let header = match &mut input {
        meerkat_runtime::Input::Prompt(i) => &mut i.header,
        meerkat_runtime::Input::Peer(i) => &mut i.header,
        meerkat_runtime::Input::FlowStep(i) => &mut i.header,
        meerkat_runtime::Input::ExternalEvent(i) => &mut i.header,
        meerkat_runtime::Input::Continuation(i) => &mut i.header,
        meerkat_runtime::Input::Operation(i) => &mut i.header,
        // `Input` is #[non_exhaustive]: a variant this build does not know
        // has no reachable header, so it cannot be re-identified for
        // successor admission.
        _ => return None,
    };
    header.id = meerkat_core::lifecycle::InputId::new();
    if let Some(key) = header.idempotency_key.take() {
        tracing::debug!(
            idempotency_key = %key,
            readmitted_input_id = %header.id,
            "carried input drops its idempotency key: the original admission was \
             durably terminalized by the repair disposal"
        );
    }
    Some(input)
}

impl MobSessionBridge {
    /// Create a new bridge wrapping the given mob handle.
    pub fn new(handle: MobHandle) -> Self {
        Self {
            handle,
            session_store: None,
            session_service: None,
            continuity_session_store: None,
            runtime_members: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            runtime_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            generated_external_owner_session: std::sync::OnceLock::new(),
            committed_boundary_recoverer: None,
            resume_divergence_logged: std::sync::Mutex::new(std::collections::HashSet::new()),
            actor_admission_budget: bridge_actor_admission_budget(),
            runtime_ingress_authority: None,
        }
    }

    /// Create a new bridge with session-service access for live model capability checks.
    pub fn with_session_service(
        handle: MobHandle,
        session_service: Arc<dyn MobSessionService>,
    ) -> Self {
        Self {
            handle,
            session_store: None,
            session_service: Some(session_service),
            continuity_session_store: None,
            runtime_members: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            runtime_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            generated_external_owner_session: std::sync::OnceLock::new(),
            committed_boundary_recoverer: None,
            resume_divergence_logged: std::sync::Mutex::new(std::collections::HashSet::new()),
            actor_admission_budget: bridge_actor_admission_budget(),
            runtime_ingress_authority: None,
        }
    }

    /// Create a new bridge with an explicit session store for checkpoint support.
    pub fn with_session_store(
        handle: MobHandle,
        session_store: Arc<dyn meerkat::SessionStore>,
    ) -> Self {
        Self {
            handle,
            session_store: Some(session_store),
            session_service: None,
            continuity_session_store: None,
            runtime_members: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            runtime_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            generated_external_owner_session: std::sync::OnceLock::new(),
            committed_boundary_recoverer: None,
            resume_divergence_logged: std::sync::Mutex::new(std::collections::HashSet::new()),
            actor_admission_budget: bridge_actor_admission_budget(),
            runtime_ingress_authority: None,
        }
    }

    /// Create a bridge with checkpoint and live capability support.
    pub fn with_session_store_and_service(
        handle: MobHandle,
        session_store: Arc<dyn meerkat::SessionStore>,
        session_service: Arc<dyn MobSessionService>,
    ) -> Self {
        Self {
            handle,
            session_store: Some(session_store),
            session_service: Some(session_service),
            continuity_session_store: None,
            runtime_members: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            runtime_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            generated_external_owner_session: std::sync::OnceLock::new(),
            committed_boundary_recoverer: None,
            resume_divergence_logged: std::sync::Mutex::new(std::collections::HashSet::new()),
            actor_admission_budget: bridge_actor_admission_budget(),
            runtime_ingress_authority: None,
        }
    }

    /// Create a bridge with an identity-owned continuity session store.
    pub fn with_continuity_session_store(
        handle: MobHandle,
        session_store: Arc<ContinuitySessionStoreAdapter>,
        session_service: Option<Arc<dyn MobSessionService>>,
    ) -> Self {
        Self {
            handle,
            session_store: Some(session_store.clone()),
            session_service,
            continuity_session_store: Some(session_store),
            runtime_members: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            runtime_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            generated_external_owner_session: std::sync::OnceLock::new(),
            committed_boundary_recoverer: None,
            resume_divergence_logged: std::sync::Mutex::new(std::collections::HashSet::new()),
            actor_admission_budget: bridge_actor_admission_budget(),
            runtime_ingress_authority: None,
        }
    }

    /// Override the per-delivery-attempt mob-actor admission budget.
    ///
    /// Hosts whose turns are known to be short can tighten the default; tests
    /// use this to exercise the bound without waiting out the real budget.
    #[must_use]
    pub fn with_actor_admission_budget(mut self, budget: Duration) -> Self {
        self.actor_admission_budget = budget;
        self
    }

    /// Inject the committed-boundary heal authority (2026-07-29 incident).
    ///
    /// Without it, [`SessionBridge::recover_committed_boundary`] reports
    /// [`CommittedBoundaryRepair::Unsupported`] and the continuity repair
    /// supervisor falls back to plain reconcile retries.
    #[must_use]
    pub fn with_committed_boundary_recoverer(
        mut self,
        recoverer: Arc<dyn CommittedBoundaryRecoverer>,
    ) -> Self {
        self.committed_boundary_recoverer = Some(recoverer);
        self
    }

    /// Inject the machine-level ingress authority the repair disposal paths
    /// use to carry a member's queued work across a heal (OB3 run 33758a41:
    /// disposal destroyed 15 queued inputs with the member). Compositions
    /// with a runtime machine pass it here; without it, repair proceeds but
    /// cannot observe or carry pending inputs.
    #[must_use]
    pub fn with_runtime_ingress_authority(
        mut self,
        authority: Arc<dyn meerkat_runtime::SessionServiceRuntimeExt>,
    ) -> Self {
        self.runtime_ingress_authority = Some(authority);
        self
    }

    /// The effective per-delivery-attempt mob-actor admission budget.
    #[must_use]
    pub fn actor_admission_budget(&self) -> Duration {
        self.actor_admission_budget
    }

    async fn remember_runtime_member(
        &self,
        runtime_id: &AgentRuntimeId,
        member_id: &MobAgentIdentity,
    ) {
        self.runtime_members.write().await.insert(
            runtime_id.as_str().to_string(),
            member_id.as_str().to_string(),
        );
    }

    async fn remember_runtime_session(
        &self,
        runtime_id: &AgentRuntimeId,
        session_id: &meerkat_core::types::SessionId,
    ) {
        self.runtime_sessions
            .write()
            .await
            .insert(runtime_id.as_str().to_string(), session_id.clone());
    }

    async fn forget_runtime_member(&self, runtime_id: &AgentRuntimeId) {
        self.runtime_members
            .write()
            .await
            .remove(runtime_id.as_str());
        self.runtime_sessions
            .write()
            .await
            .remove(runtime_id.as_str());
    }

    async fn member_id_for_runtime_id(&self, runtime_id: &AgentRuntimeId) -> MobAgentIdentity {
        let members = self.runtime_members.read().await;
        members
            .get(runtime_id.as_str())
            .map(|member| MobAgentIdentity::from(member.as_str()))
            // The recompute fallback must mint the same comms-safe roster id
            // as the spawn path (meerkat 0.7 MemberCommsName rejects `:`).
            .unwrap_or_else(|| crate::member_comms_id::mob_member_id(runtime_id.as_str()))
    }

    /// Retire one session-owned member through the MobMachine's retained
    /// cleanup anchor until the structural roster entry is actually gone.
    ///
    /// Meerkat can finish physical disposal and then reject the final archive
    /// projection. That first call deliberately leaves a Retiring roster
    /// anchor so the same exact incarnation can resume cleanup. Treating the
    /// error as immediate success leaks that stopped anchor into whole-mob
    /// shutdown, where it is interrupted again and prevents the mob from
    /// reaching `Stopped`. Retry once through the generated cleanup authority
    /// and verify the post-condition instead of laundering partial cleanup.
    async fn retire_session_owned_member_to_absence(
        &self,
        member_id: &MobAgentIdentity,
    ) -> Result<(), meerkat_mob::MobError> {
        let retained_cleanup_error = match self.handle.retire(member_id.clone()).await {
            Ok(()) | Err(meerkat_mob::MobError::MemberNotFound(_)) => None,
            Err(error) if is_recoverable_session_owned_retire_cleanup_error(&error.to_string()) => {
                Some(error)
            }
            Err(error) => return Err(error),
        };

        if let Some(initial_error) = retained_cleanup_error {
            tracing::warn!(
                member_id = %member_id,
                error = %initial_error,
                "session-owned retire retained a cleanup anchor; retrying exact incarnation"
            );
            match self.handle.retire(member_id.clone()).await {
                Ok(()) | Err(meerkat_mob::MobError::MemberNotFound(_)) => {}
                Err(retry_error) => {
                    return Err(meerkat_mob::MobError::Internal(format!(
                        "session-owned retire cleanup retry failed for {member_id}: initial: \
                         {initial_error}; retry: {retry_error}"
                    )));
                }
            }
        }

        if self
            .handle
            .list_all_members()
            .await
            .iter()
            .any(|entry| entry.agent_identity == *member_id)
        {
            return Err(meerkat_mob::MobError::Internal(format!(
                "session-owned retire reported success but retained roster anchor {member_id}"
            )));
        }
        Ok(())
    }

    /// Capture a member session's pending machine ingress BEFORE a repair
    /// disposal destroys it (OB3 run 33758a41: queue_len=5 steer_queue_len=10
    /// destroyed with the member). Pending = admitted but not yet run
    /// (`Accepted`/`Queued`); mid-run and terminal inputs are not queue work.
    /// Best-effort observation: an unregistered runtime or a probe fault
    /// degrades to an empty capture (there is then nothing this bridge can
    /// see, let alone carry).
    /// Resolve the machine-level ingress authority for repair carry: an
    /// explicitly injected one wins; otherwise the session service's runtime
    /// adapter (the gateway's `MeerkatMachine`) serves.
    fn resolved_runtime_ingress_authority(
        &self,
    ) -> Option<Arc<dyn meerkat_runtime::SessionServiceRuntimeExt>> {
        if let Some(explicit) = self.runtime_ingress_authority.as_ref() {
            return Some(Arc::clone(explicit));
        }
        self.session_service
            .as_ref()?
            .runtime_adapter()
            .map(|machine| machine as Arc<dyn meerkat_runtime::SessionServiceRuntimeExt>)
    }

    async fn capture_pending_member_ingress(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> PendingIngressCapture {
        let Some(authority) = self.resolved_runtime_ingress_authority() else {
            tracing::debug!(
                session_id = %session_id,
                "no runtime ingress authority; repair cannot observe or carry queued member inputs"
            );
            return PendingIngressCapture::empty();
        };
        let active = match authority.list_active_inputs(session_id).await {
            Ok(ids) => ids,
            Err(error) => {
                // NotReady / NotFound = no registered runtime = no live queue
                // to destroy. Other faults mean the queue is unobservable;
                // disposal proceeds exactly as it did before this seam.
                tracing::debug!(
                    session_id = %session_id,
                    error = %error,
                    "pending-ingress probe unavailable before repair disposal"
                );
                return PendingIngressCapture::empty();
            }
        };
        let mut capture = PendingIngressCapture::empty();
        for input_id in active {
            let stored = match authority.input_state(session_id, &input_id).await {
                Ok(Some(stored)) => stored,
                Ok(None) => continue,
                Err(error) => {
                    capture
                        .uncarryable
                        .push((input_id, "state-unreadable", error.to_string()));
                    continue;
                }
            };
            if stored.seed.terminal_outcome.is_some() {
                continue;
            }
            match stored.seed.phase {
                meerkat_runtime::InputLifecycleState::Accepted
                | meerkat_runtime::InputLifecycleState::Queued => {}
                meerkat_runtime::InputLifecycleState::Staged
                | meerkat_runtime::InputLifecycleState::Applied
                | meerkat_runtime::InputLifecycleState::AppliedPendingConsumption => {
                    capture.uncarryable.push((
                        input_id,
                        "mid-run",
                        format!(
                            "input was {:?} at repair disposal; the disposal cancel \
                             terminalizes it and the sender observes that terminal",
                            stored.seed.phase
                        ),
                    ));
                    continue;
                }
                meerkat_runtime::InputLifecycleState::Consumed
                | meerkat_runtime::InputLifecycleState::Superseded
                | meerkat_runtime::InputLifecycleState::Coalesced
                | meerkat_runtime::InputLifecycleState::Abandoned => continue,
                other => {
                    // #[non_exhaustive]: an unknown phase cannot be proven
                    // pending-and-idle, so it is not carried — but it IS
                    // named before disposal destroys it.
                    capture.uncarryable.push((
                        input_id,
                        "unrecognized-phase",
                        format!("input was in unrecognized lifecycle phase {other:?}"),
                    ));
                    continue;
                }
            }
            match stored.state.persisted_input {
                Some(
                    input @ (meerkat_runtime::Input::Prompt(_)
                    | meerkat_runtime::Input::Peer(_)
                    | meerkat_runtime::Input::ExternalEvent(_)),
                ) => {
                    capture.carryable.push(CarriedMemberInput {
                        original_input_id: input_id,
                        admission_sequence: stored.seed.admission_sequence,
                        input,
                    });
                }
                Some(meerkat_runtime::Input::FlowStep(_)) => {
                    capture.uncarryable.push((
                        input_id,
                        "flow-step",
                        "flow-step correlation is owned by the flow engine and cannot be \
                         re-admitted raw; the loss is bounded to the interrupted flow, \
                         which observes its step's terminal and owns the retry"
                            .to_string(),
                    ));
                }
                Some(
                    meerkat_runtime::Input::Continuation(_) | meerkat_runtime::Input::Operation(_),
                ) => {
                    capture.uncarryable.push((
                        input_id,
                        "runtime-internal",
                        "continuation/operation inputs are machine-internal and cannot be \
                         re-admitted raw; the successor runtime re-derives its own; the \
                         loss is bounded to the disposed runtime's in-flight bookkeeping"
                            .to_string(),
                    ));
                }
                Some(_) => {
                    capture.uncarryable.push((
                        input_id,
                        "unrecognized-class",
                        "the pending input's class is unknown to this build; no carry \
                         lane exists for it"
                            .to_string(),
                    ));
                }
                None => {
                    capture.uncarryable.push((
                        input_id,
                        "payload-unavailable",
                        "the runtime retained no payload for this pending input".to_string(),
                    ));
                }
            }
        }
        capture
            .carryable
            .sort_by_key(|entry| entry.admission_sequence.unwrap_or(u64::MAX));
        capture
    }

    /// Loud pre-disposal record of what the repair is about to do with a
    /// member's queued work: what it will carry, and — per item, with ids —
    /// what it is about to DESTROY because no carry lane exists for it.
    fn log_pending_ingress_before_repair_disposal(
        &self,
        member_id: &MobAgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        capture: &PendingIngressCapture,
    ) {
        if capture.is_empty() {
            return;
        }
        tracing::warn!(
            member_id = %member_id,
            session_id = %session_id,
            carryable = capture.carryable.len(),
            destroyed = capture.uncarryable.len(),
            "repair disposal found pending queued inputs on the member: carrying \
             the carryable set to the healed successor; anything listed below is \
             destroyed with the member"
        );
        for (input_id, class, reason) in &capture.uncarryable {
            tracing::warn!(
                member_id = %member_id,
                session_id = %session_id,
                input_id = %input_id,
                class,
                reason = %reason,
                "repair disposal DESTROYS a pending member input it cannot carry"
            );
        }
    }

    /// Re-admit captured queue work into the healed successor session through
    /// ordinary machine admission. Per-item failures are loud errors — the
    /// heal itself stands (a healed member minus one carried input is still
    /// strictly better than a Broken member), but every lost input is named.
    async fn readmit_carried_inputs(
        &self,
        member_id: &MobAgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        capture: PendingIngressCapture,
    ) {
        if capture.carryable.is_empty() {
            return;
        }
        let Some(authority) = self.resolved_runtime_ingress_authority() else {
            // Unreachable in practice: a non-empty capture required the
            // authority. Fail loud rather than silently dropping.
            tracing::error!(
                member_id = %member_id,
                session_id = %session_id,
                lost = capture.carryable.len(),
                "runtime ingress authority disappeared between capture and carry; \
                 captured queued inputs are lost"
            );
            return;
        };
        let total = capture.carryable.len();
        let mut carried = 0usize;
        for entry in capture.carryable {
            let CarriedMemberInput {
                original_input_id,
                input,
                ..
            } = entry;
            let Some(input) = remint_carried_input_identity(input) else {
                tracing::error!(
                    member_id = %member_id,
                    session_id = %session_id,
                    original_input_id = %original_input_id,
                    "carried input has no re-identifiable header in this build; the \
                     input is lost"
                );
                continue;
            };
            let readmitted_input_id = input.id().clone();
            match authority.accept_input(session_id, input).await {
                Ok(meerkat_runtime::AcceptOutcome::Accepted { .. }) => {
                    carried += 1;
                    tracing::info!(
                        member_id = %member_id,
                        session_id = %session_id,
                        original_input_id = %original_input_id,
                        readmitted_input_id = %readmitted_input_id,
                        "carried a queued member input into the healed successor session"
                    );
                }
                Ok(other) => {
                    let outcome = match &other {
                        meerkat_runtime::AcceptOutcome::Deduplicated { .. } => "deduplicated",
                        meerkat_runtime::AcceptOutcome::Rejected { .. } => "rejected",
                        _ => "unrecognized",
                    };
                    tracing::error!(
                        member_id = %member_id,
                        session_id = %session_id,
                        original_input_id = %original_input_id,
                        outcome,
                        "successor admission did not accept a carried queued input; \
                         the input is lost"
                    );
                }
                Err(error) => {
                    tracing::error!(
                        member_id = %member_id,
                        session_id = %session_id,
                        original_input_id = %original_input_id,
                        error = %error,
                        "failed to re-admit a carried queued input into the healed \
                         successor; the input is lost"
                    );
                }
            }
        }
        tracing::warn!(
            member_id = %member_id,
            session_id = %session_id,
            carried,
            total,
            "repair carried queued member inputs into the healed successor session"
        );
    }

    async fn member_wires(
        &self,
        require_reciprocal: bool,
    ) -> Result<Vec<(AgentRuntimeId, AgentRuntimeId)>, BridgeError> {
        let members = self.handle.list_members_including_retiring().await;
        let runtime_members = self.runtime_members.read().await;
        let member_runtimes = runtime_members
            .iter()
            .map(|(runtime, member)| (member.clone(), runtime.clone()))
            .collect::<HashMap<_, _>>();
        let active_ids = members
            .iter()
            .map(|member| member.agent_identity.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        let mut directed = std::collections::BTreeSet::new();
        for member in &members {
            let a = member.agent_identity.to_string();
            for peer in &member.wired_to {
                let b = peer.to_string();
                if active_ids.contains(&b) {
                    directed.insert((a.clone(), b));
                }
            }
        }
        let edges = directed
            .iter()
            .filter(|(a, b)| !require_reciprocal || directed.contains(&(b.clone(), a.clone())))
            .map(|(a, b)| {
                if a <= b {
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                }
            })
            .collect::<std::collections::BTreeSet<_>>();
        Ok(edges
            .into_iter()
            .filter_map(|(a, b)| {
                let a = member_runtimes
                    .get(&a)
                    .cloned()
                    .unwrap_or_else(|| crate::member_comms_id::runtime_alias_str(&a).into_owned());
                let b = member_runtimes
                    .get(&b)
                    .cloned()
                    .unwrap_or_else(|| crate::member_comms_id::runtime_alias_str(&b).into_owned());
                Some((
                    AgentRuntimeId::parse(&a).ok()?,
                    AgentRuntimeId::parse(&b).ok()?,
                ))
            })
            .collect())
    }

    async fn runtime_session_id(
        &self,
        runtime_id: &AgentRuntimeId,
    ) -> Option<meerkat_core::types::SessionId> {
        self.runtime_sessions
            .read()
            .await
            .get(runtime_id.as_str())
            .cloned()
    }

    /// After a rejected resume, verify the durable session actually survived.
    /// On meerkat ≤0.7.28 the spawn rollback compensated late resume-spawn
    /// failures by archiving the session it was resuming (Bug I, ask 31 —
    /// fixed in 0.7.29: resumed provisions restore to durable idle, and
    /// retired-with-intact-snapshot sessions auto-revive on resume). The
    /// probe stays as a regression tripwire: a GONE result on ≥0.7.29 is
    /// always a platform bug worth a loud, immediate signal.
    /// Belt check for the never-persisted fresh-spawn fallback: true ONLY
    /// when a durable read path positively answers "no row exists for this
    /// session id". Probes, in order of directness: the raw session store
    /// handle, the continuity session-store adapter, and finally the session
    /// service (`read` returning typed `NotFound`). No available read path,
    /// or any probe error, returns false - fail closed into the
    /// never-abandon refusal, never into a fresh spawn.
    async fn durable_session_row_is_absent(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> bool {
        if let Some(store) = self.session_store.as_ref() {
            return matches!(store.load_meta(session_id).await, Ok(None));
        }
        if let Some(store) = self.continuity_session_store.as_ref() {
            use meerkat::SessionStore as _;
            return matches!(store.load_meta(session_id).await, Ok(None));
        }
        if let Some(service) = self.session_service.as_ref() {
            return matches!(
                service.read(session_id).await,
                Err(meerkat_core::SessionError::NotFound { .. })
            );
        }
        false
    }

    /// Precondition probe for the collision-repair retire: confirmed absence
    /// of the resume target in DURABLE, NON-ACTOR authority. The raw injected
    /// `session_store` row is deliberately NOT consulted here (unlike
    /// [`Self::durable_session_row_is_absent`], whose verdict is paired with
    /// meerkat's typed Absent error): since the 0.8.11 store-owned repin,
    /// runtime-backed compositions persist through RuntimeStore authority and
    /// never project into a raw injected SessionStore, so an empty raw row is
    /// not evidence of absence there.
    ///
    /// Every probe here must be answerable WITHOUT the session's live actor:
    /// this runs while the stale member may be wedged mid-turn, and an
    /// actor-routed read (`service.read`) waits on the very member the
    /// repair is about to dispose — a self-deadlock, proven live at the
    /// meerkat 0.8.13 repin (the collision arm hung inside this probe).
    /// `session_known_to_archive_authority` is the durable disposal-routing
    /// predicate that answers from archive authority instead.
    ///
    /// Verdict: `true` (refuse the destructive retire) only when at least one
    /// durable authority was consulted and none of them knows the session.
    /// A probe fault means absence cannot be CONFIRMED, so repair proceeds
    /// exactly as it did before this guard existed.
    async fn resume_source_confirmed_absent(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> bool {
        let mut consulted_authority = false;
        if let Some(store) = self.continuity_session_store.as_ref() {
            use meerkat::SessionStore as _;
            match store.load_meta(session_id).await {
                Ok(Some(_)) => return false,
                Ok(None) => consulted_authority = true,
                Err(_) => return false,
            }
        }
        if let Some(service) = self.session_service.as_ref() {
            match service.session_known_to_archive_authority(session_id).await {
                Ok(true) => return false,
                Ok(false) => consulted_authority = true,
                Err(_) => return false,
            }
        }
        consulted_authority
    }

    async fn verify_durable_session_after_rejected_resume(
        &self,
        identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
    ) {
        let Some(store) = self.session_store.as_ref() else {
            return;
        };
        match store.load_meta(session_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                tracing::error!(
                    identity = %identity,
                    session_id = %session_id,
                    "durable session is GONE after the rejected resume (Bug I class); this \
                     should be impossible on meerkat >=0.7.29 (ask 31: non-destructive resume \
                     rollback + auto-revival) — report upstream; recovery needs a pre-damage \
                     store restore"
                );
            }
            Err(error) => {
                tracing::warn!(
                    identity = %identity,
                    session_id = %session_id,
                    %error,
                    "could not verify durable session presence after the rejected resume"
                );
            }
        }
    }

    /// Resolve the inline definition profile backing `spec.profile`, used as
    /// the base when a draft model override must be projected into the typed
    /// `override_profile` spawn owner. Realm-ref bindings resolve to `None`.
    fn base_profile_for_spec(&self, spec: &DurableAgentSpec) -> Option<meerkat_mob::Profile> {
        self.handle
            .definition()
            .resolve_inline_profile(&spec.profile)
            .cloned()
    }

    /// Resume-divergence tripwire: when the durable session restores a
    /// model/provider different from the profile's declaration and no
    /// `resume_overrides` mask covers the field, say so at INFO — once per
    /// identity per boot. Declared-field auto-mark
    /// (`crate::mob_handle_runtime::auto_mark_declared_resume_overrides`)
    /// makes the mask cover declared fields on inline profiles, so the only
    /// case that can fire today is an inline profile whose declared model
    /// resolves no coherent provider (unknown model / derived self-hosted or
    /// other without a binding). Realm-ref profiles do NOT fire:
    /// `base_profile_for_spec` resolves inline bindings only, and the
    /// `RealmProfileStore` is not threaded into this bridge, so a realm
    /// profile edit that loses to durable metadata is currently silent
    /// (tracked follow-up: thread the realm store and resolve declarations
    /// via `MobDefinition::resolve_profile`).
    ///
    /// Never fails the resume: metadata read faults are skipped at debug.
    async fn log_unmasked_resume_divergence(
        &self,
        identity: &AgentIdentity,
        spec: &DurableAgentSpec,
        draft: &AgentBuildDraft,
        base_profile: Option<&meerkat_mob::Profile>,
        session_id: &meerkat_core::types::SessionId,
    ) {
        let Some(profile) = base_profile else {
            return;
        };
        let mask = profile.resume_override_mask();
        if mask.model && mask.provider {
            return;
        }
        {
            let logged = self
                .resume_divergence_logged
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if logged.contains(identity.as_str()) {
                return;
            }
        }
        let Some(service) = self.session_service.as_ref() else {
            return;
        };
        let metadata = match service.load_persisted_session_metadata(session_id).await {
            Ok(Some(view)) => view.session_metadata,
            Ok(None) => None,
            Err(error) => {
                tracing::debug!(
                    identity = %identity,
                    session_id = %session_id,
                    %error,
                    "resume-divergence check skipped: durable metadata read failed"
                );
                None
            }
        };
        let Some(metadata) = metadata else {
            return;
        };
        // The declaration the profile (plus a draft model pin) would apply if
        // it were masked — mirrors the candidate side of meerkat-mob's
        // `effective_resumed_session_llm_identity`.
        let declared_model = draft.model.as_ref().unwrap_or(&profile.model);
        let divergence = unmasked_resume_divergence(
            &mask,
            declared_model,
            profile.provider,
            &metadata.model,
            metadata.provider,
        );
        if divergence.model || divergence.provider {
            // One line, BOTH pairs: the OB3 cutover incident's first symptom
            // was a (model, provider) pair mismatch, and a model-only line
            // hid the half that mattered.
            tracing::info!(
                identity = %identity,
                profile = %spec.profile.as_str(),
                session_id = %session_id,
                restored_model = %metadata.model,
                restored_provider = %metadata.provider.as_str(),
                profile_model = %declared_model,
                profile_provider = ?profile.provider.map(|provider| provider.as_str()),
                model_unmasked_divergent = divergence.model,
                provider_unmasked_divergent = divergence.provider,
                "resume restored an LLM identity (model, provider) that differs from the \
                 profile declaration; durable metadata wins for the unmasked fields (no \
                 resume_overrides mask covers them)"
            );
            self.resume_divergence_logged
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(identity.as_str().to_string());
        }
    }

    async fn resolve_runtime_session_id(
        &self,
        runtime_id: &AgentRuntimeId,
        member_id: &MobAgentIdentity,
        missing_message: &'static str,
        deadline: &ActorAdmissionDeadline,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        // `resolve_bridge_session_id` reads the machine-state watch
        // projection; it never enters the actor queue, so there is nothing to
        // bound here.
        if let Some(session_id) = self.handle.resolve_bridge_session_id(member_id).await {
            self.remember_runtime_session(runtime_id, &session_id).await;
            return Ok(session_id);
        }

        // `get_member` faults (machine command/transport errors) must not be
        // laundered into "member absent"; surface them to the caller.
        if member_id.as_str() != runtime_id.as_str()
            && deadline
                .bound(
                    "resolve_session.get_member",
                    member_id,
                    self.handle.get_member(member_id),
                )
                .await?
                .map_err(|err| BridgeError::Mob(err.to_string()))?
                .is_some()
            && let Some(session_id) = self.runtime_session_id(runtime_id).await
        {
            return Ok(session_id);
        }

        Err(BridgeError::Mob(missing_message.to_string()))
    }

    async fn repair_member_for_delivery(
        &self,
        runtime_id: &AgentRuntimeId,
        member_id: &MobAgentIdentity,
        member_entry_before_delivery: Option<(meerkat_mob::ProfileName, BTreeMap<String, String>)>,
    ) -> Result<(), BridgeError> {
        // Resume-repair first: the recorded durable session IS the
        // conversation. `MobHandle::respawn` retires and spawns FRESH — it
        // rotates the bridge session and abandons the transcript (the OB3
        // `identity_alias_respawn_rotation` data-loss class). When we know
        // the durable session and the member's role, rebuild the member ONTO
        // that session instead; spawn fresh under the same identity only
        // when meerkat confirms the durable snapshot itself is gone (nothing
        // left to preserve). The legacy respawn below remains only for the
        // case where we lack the material for a resume spec.
        // Fidelity note: like the legacy path, the rebuilt spec carries
        // role + labels only — deliver-time repair cannot re-run the host
        // customizer (that rebuild belongs to the runtime's materialize
        // seam).
        if let (Some(session_id), Some((role, labels))) = (
            self.runtime_session_id(runtime_id).await,
            member_entry_before_delivery.clone(),
        ) {
            // The repair below starts with a destructive retire: capture the
            // wedged member's queued inputs FIRST so the healed successor can
            // re-admit them instead of losing them with the disposal (OB3
            // run 33758a41).
            let capture = self.capture_pending_member_ingress(&session_id).await;
            self.log_pending_ingress_before_repair_disposal(member_id, &session_id, &capture);
            match self
                .resume_repair_member(
                    runtime_id,
                    member_id,
                    role.clone(),
                    labels.clone(),
                    &session_id,
                )
                .await
            {
                Ok(()) => {
                    self.readmit_carried_inputs(member_id, &session_id, capture)
                        .await;
                    return Ok(());
                }
                Err(RepairResumeFailure::DurableSnapshotMissing { detail }) => {
                    tracing::warn!(
                        runtime_id = %runtime_id,
                        member_id = %member_id,
                        session_id = %session_id,
                        detail = %detail,
                        "durable session snapshot is gone; repairing with a fresh spawn \
                         under the same identity (no transcript left to preserve)"
                    );
                    // `resume_repair_member` already retired the member to
                    // VERIFIED roster absence, so `MobHandle::respawn` —
                    // which reads the roster entry — would deterministically
                    // fail `MemberNotFound` (classified Fatal) and wedge the
                    // identity. Spawn fresh directly instead, still gated on
                    // the typed `SessionUnavailableForResume { reason:
                    // Absent }` evidence that selected this arm.
                    self.handle
                        .ensure_member(fresh_member_spec_from_pre_delivery_entry(
                            member_id, role, labels,
                        ))
                        .await
                        .map_err(|e| BridgeError::Mob(e.to_string()))?;
                    self.remember_runtime_member(runtime_id, member_id).await;
                    if let Some(fresh_session_id) =
                        self.handle.resolve_bridge_session_id(member_id).await
                    {
                        self.readmit_carried_inputs(member_id, &fresh_session_id, capture)
                            .await;
                    } else if !capture.carryable.is_empty() {
                        tracing::error!(
                            runtime_id = %runtime_id,
                            member_id = %member_id,
                            lost = capture.carryable.len(),
                            "fresh repair spawn has no resolvable session id; the \
                             captured queued inputs are lost"
                        );
                    }
                    return Ok(());
                }
                Err(RepairResumeFailure::Rejected(err)) => {
                    if !capture.carryable.is_empty() {
                        tracing::error!(
                            runtime_id = %runtime_id,
                            member_id = %member_id,
                            session_id = %session_id,
                            lost = capture.carryable.len(),
                            "delivery repair failed after its retire; the captured \
                             queued inputs are lost with the disposed member"
                        );
                    }
                    return Err(err);
                }
            }
        }
        match self.handle.respawn(member_id.clone(), None).await {
            Ok(_) => Ok(()),
            Err(respawn_err) => match classify_member_repair_respawn_failure(&respawn_err) {
                MemberRepairRespawnFailure::DegradedTopologyRestore { failed_peer_ids } => {
                    tracing::warn!(
                        runtime_id = %runtime_id,
                        member_id = %member_id,
                        failed_peer_count = failed_peer_ids.len(),
                        failed_peer_ids = ?failed_peer_ids,
                        "identity bridge respawn restored member with isolated peer edges; continuing delivery"
                    );
                    // meerkat-mob raises this only after the member/session is live; only peer edges are incomplete.
                    Ok(())
                }
                MemberRepairRespawnFailure::RecoverableCleanup => {
                    // A `get_member` fault must not be read as "member absent"
                    // (that would trigger a spurious re-spawn); fail the
                    // delivery repair instead.
                    if self
                        .handle
                        .get_member(member_id)
                        .await
                        .map_err(|err| BridgeError::Mob(err.to_string()))?
                        .is_none()
                        && let Some((role, labels)) = member_entry_before_delivery
                    {
                        self.handle
                            .ensure_member(fresh_member_spec_from_pre_delivery_entry(
                                member_id, role, labels,
                            ))
                            .await
                            .map_err(|e| BridgeError::Mob(e.to_string()))?;
                    }
                    Ok(())
                }
                MemberRepairRespawnFailure::Fatal(message) => Err(BridgeError::Mob(message)),
            },
        }
    }

    /// Rebuild a wedged member ONTO its recorded durable session
    /// (`MemberLaunchMode::Resume`): retire the stale roster entry if present
    /// (tolerating recoverable cleanup and member-absent), then spawn with the
    /// resume launch mode so the transcript survives the repair. The repaired
    /// member keeps the SAME bridge session id, so no continuity rebind is
    /// needed and the durable alias never rotates.
    async fn resume_repair_member(
        &self,
        runtime_id: &AgentRuntimeId,
        member_id: &MobAgentIdentity,
        role: meerkat_mob::ProfileName,
        labels: BTreeMap<String, String>,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), RepairResumeFailure> {
        if let Err(err) = self.retire_session_owned_member_to_absence(member_id).await {
            return Err(RepairResumeFailure::Rejected(BridgeError::Mob(format!(
                "repair retire before resume: {err}"
            ))));
        }
        self.forget_runtime_member(runtime_id).await;

        let mut spec = SpawnMemberSpec::new(role, member_id.clone());
        if !labels.is_empty() {
            spec = spec.with_labels(labels);
        }
        spec.launch_mode = MemberLaunchMode::Resume {
            bridge_session_id: session_id.clone(),
        };

        match self.spawn_member_spec(spec).await {
            Ok(()) => {
                self.remember_runtime_member(runtime_id, member_id).await;
                self.remember_runtime_session(runtime_id, session_id).await;
                tracing::info!(
                    runtime_id = %runtime_id,
                    member_id = %member_id,
                    session_id = %session_id,
                    "delivery repair resumed the member onto its durable session \
                     (transcript preserved, no session rotation)"
                );
                Ok(())
            }
            // meerkat's TYPED answer for "the durable session snapshot does
            // not exist": there is no transcript to preserve, so the caller
            // may fall back to a fresh respawn. A variant match on purpose —
            // every other resume refusal (archived-but-intact, recovery
            // hold, quarantine, unknown) falls to the arm below and
            // preserves the binding.
            Err(err) if durable_snapshot_is_typed_absent(&err) => {
                Err(RepairResumeFailure::DurableSnapshotMissing {
                    detail: err.to_string(),
                })
            }
            Err(err) => {
                let kind = classify_resume_error(&err);
                tracing::error!(
                    runtime_id = %runtime_id,
                    member_id = %member_id,
                    session_id = %session_id,
                    kind = ?kind,
                    error = %err,
                    "delivery repair resume rejected; durable session preserved, \
                     delivery fails loudly (refusing fresh-spawn fallback)"
                );
                Err(RepairResumeFailure::Rejected(BridgeError::ResumeRejected {
                    kind,
                    detail: format!("delivery repair resume: {err}"),
                }))
            }
        }
    }

    /// Ops-owner bridge session for external (peer-only) member operations.
    ///
    /// meerkat 0.7.1 fails external member provisioning closed unless the
    /// spawn carries a generated owner binding (owner bridge session + ops
    /// registry; see `MultiBackendProvisioner::provision_member`). Prefer the
    /// mob's machine-bound owner bridge-session authority when it exists;
    /// otherwise mint one stable session id for this bridge — the runtime
    /// adapter creates local session resources for it on demand, exactly like
    /// meerkat's own external smoke supervisors.
    fn external_owner_bridge_session_id(&self) -> meerkat_core::types::SessionId {
        if let Some(authority) = self.handle.owner_bridge_session_lifecycle_authority() {
            return authority.bridge_session_id;
        }
        self.generated_external_owner_session
            .get_or_init(meerkat_core::types::SessionId::new)
            .clone()
    }

    /// Spawn a mob member from a fully-built spec, attaching the generated
    /// owner context that meerkat 0.7.1 requires for external (peer-only)
    /// bindings. Session-backed specs spawn through the plain path.
    async fn spawn_member_spec(
        &self,
        spawn_spec: SpawnMemberSpec,
    ) -> Result<(), meerkat_mob::MobError> {
        if spawn_spec_requires_generated_owner_context(&spawn_spec) {
            let owner_session_id = self.external_owner_bridge_session_id();
            Box::pin(
                self.handle
                    .spawn_spec_with_generated_owner_context(spawn_spec, owner_session_id),
            )
            .await
            .map(|_| ())
        } else {
            Box::pin(self.handle.spawn_spec(spawn_spec))
                .await
                .map(|_| ())
        }
    }
}

/// Project meerkat 0.7's tri-state peer connectivity into an inspect-level
/// reachable count, when the tri-state resolves one.
///
/// Only a fully resolved probe ([`WirePeerConnectivity::Known`] with no
/// unknown peers) contributes a live count. A partially resolved Known probe,
/// the not-applicable / probe-timed-out arms, and an uncomputed projection
/// return `None` so the caller falls back to the machine-owned wiring degree
/// (`wired_to.len()`) instead of projecting 0 — a freshly wired member has
/// peers regardless of whether a live probe resolved, and the sibling console
/// alias surface computes the same wire field from `wired_to`; the two
/// surfaces must agree.
fn peer_reachable_count_from_connectivity(
    connectivity: Option<&meerkat_contracts::WirePeerConnectivity>,
) -> Option<usize> {
    match connectivity {
        Some(meerkat_contracts::WirePeerConnectivity::Known { snapshot })
            if snapshot.unknown_peer_count == 0 =>
        {
            Some(snapshot.reachable_peer_count)
        }
        Some(meerkat_contracts::WirePeerConnectivity::Known { .. }) => None,
        Some(
            meerkat_contracts::WirePeerConnectivity::NotApplicable
            | meerkat_contracts::WirePeerConnectivity::ProbeTimedOut,
        )
        | None => None,
    }
}

/// External (peer-only) member provisioning on meerkat 0.7.1 requires a
/// machine-minted owner context; plain `spawn_spec` fails closed by design.
pub(crate) fn spawn_spec_requires_generated_owner_context(spawn_spec: &SpawnMemberSpec) -> bool {
    matches!(
        spawn_spec.binding,
        Some(meerkat_mob::RuntimeBinding::External { .. })
    )
}

fn spec_uses_external_binding(spec: &DurableAgentSpec) -> bool {
    matches!(spec.backend, Some(meerkat_mob::MobBackendKind::External))
        || matches!(
            spec.binding.as_ref(),
            Some(meerkat_contracts::WireRuntimeBinding::External { .. })
        )
}

fn member_id_for_spawn_spec(
    runtime_id: &AgentRuntimeId,
    spec: &DurableAgentSpec,
) -> MobAgentIdentity {
    // meerkat 0.7's `MemberCommsName` is fail-closed: roster member ids must
    // be identifier-safe (no `:`). MobKit's public alias space — durable
    // identities like `review:singleton` and runtime ids like
    // `rt:review:singleton:0` — is unchanged; the roster id is the comms-safe
    // encoding (identity for already-safe names).
    if spec_uses_external_binding(spec) {
        crate::member_comms_id::mob_member_id(spec.identity.as_str())
    } else {
        crate::member_comms_id::mob_member_id(runtime_id.as_str())
    }
}

/// Default OpenAI prompt-cache routing key for `identity`.
///
/// `prompt_cache_key` is a routing hint only: it steers a request onto a
/// cache-warm backend and has no effect on the completion. Meerkat sends none
/// by default, so without this every identity competes in one anonymous
/// routing pool.
///
/// The key is a total function of the durable identity id and nothing else —
/// no uuid, timestamp, generation, runtime id, session id or other
/// process-local value — so it reproduces byte-for-byte across restarts,
/// respawns and revivals. Per-identity (not per-profile) is the correct
/// bucket: two identities sharing a profile still carry different
/// transcripts, and therefore different cacheable prefixes.
fn identity_prompt_cache_key(identity: &AgentIdentity) -> String {
    format!("mobkit:{}", identity.as_str())
}

/// Fill every unset knob on `target` from `defaults`.
///
/// Explicit knobs always win. The tag half delegates to meerkat's own
/// `ProviderTag::merge_missing_from`, so a provider-family conflict is a typed
/// fault rather than a silent union of unrelated provider bags. Without the
/// merge, a draft that sets one cache knob would replace the profile's
/// declaration wholesale and silently drop knobs declared in the mob
/// definition.
fn merge_provider_params_missing_from(
    target: &mut ProviderParamsOverride,
    defaults: &ProviderParamsOverride,
) -> Result<(), BridgeError> {
    fn fill<T: Clone>(target: &mut Option<T>, default: &Option<T>) {
        if target.is_none()
            && let Some(value) = default
        {
            *target = Some(value.clone());
        }
    }

    fill(&mut target.temperature, &defaults.temperature);
    fill(&mut target.top_p, &defaults.top_p);
    fill(&mut target.max_output_tokens, &defaults.max_output_tokens);
    fill(&mut target.reasoning, &defaults.reasoning);
    fill(
        &mut target.thinking_budget_tokens,
        &defaults.thinking_budget_tokens,
    );
    match (target.provider_tag.as_mut(), defaults.provider_tag.as_ref()) {
        (Some(tag), Some(default)) => tag
            .merge_missing_from(default)
            .map_err(|error| BridgeError::InvalidInput(format!("provider params: {error}")))?,
        (None, Some(default)) => target.provider_tag = Some(default.clone()),
        _ => {}
    }
    Ok(())
}

/// Whether this spawn's effective model is served by OpenAI.
///
/// `prompt_cache_key` lives on the OpenAI provider tag, and meerkat rejects a
/// tag from the wrong provider family at its merge seam
/// (`ProviderParamsMergeError::ProviderTagMismatch`), which would fail the
/// turn for an Anthropic- or Gemini-backed identity. The default is injected
/// only when the provider is provably OpenAI.
fn spawn_is_openai_backed(
    draft: &AgentBuildDraft,
    base_profile: Option<&meerkat_mob::Profile>,
) -> bool {
    // A draft model override replaces the profile's model and meerkat
    // re-infers the owner from the new id (the pinned branch below clears
    // `provider` for exactly that reason), so the new id decides.
    if let Some(model) = draft.model.as_deref() {
        return matches!(
            meerkat_models::infer_provider(model),
            Some(meerkat_core::Provider::OpenAI)
        );
    }
    match base_profile {
        Some(profile) => match profile.provider {
            Some(provider) => matches!(provider, meerkat_core::Provider::OpenAI),
            None => matches!(
                meerkat_models::infer_provider(&profile.model),
                Some(meerkat_core::Provider::OpenAI)
            ),
        },
        None => false,
    }
}

/// Give `params` the identity's default prompt-cache routing key when the
/// caller supplied none.
///
/// A caller-supplied key is never replaced, and a tag belonging to another
/// provider family is left untouched — that tag is the caller's explicit
/// provider identity, not a slot to overwrite.
fn ensure_default_prompt_cache_key(params: &mut ProviderParamsOverride, identity: &AgentIdentity) {
    match params.provider_tag.as_mut() {
        Some(ProviderTag::OpenAi(tag)) => {
            if tag.prompt_cache_key.is_none() {
                tag.prompt_cache_key = Some(identity_prompt_cache_key(identity));
            }
        }
        Some(_) => {}
        None => {
            params.provider_tag = Some(ProviderTag::OpenAi(OpenAiProviderTag {
                prompt_cache_key: Some(identity_prompt_cache_key(identity)),
                ..Default::default()
            }));
        }
    }
}

/// Land the draft's provider params, plus the per-identity prompt-cache
/// default, on `spawn_spec`.
///
/// `SpawnMemberSpec` has no field-scoped provider-params seam: the only
/// carrier meerkat reads is `override_profile.provider_params` (mob
/// `build.rs`, `config.provider_params = profile.provider_params.clone()`).
/// A declared override therefore requires a profile snapshot.
///
/// A spawn that declares nothing is deliberately left on the field-scoped
/// path: meerkat persists the snapshot as `effective_profile_override` and
/// reuses it on internal revival, so minting one for every OpenAI identity
/// just to carry a routing hint would freeze definition drift fleet-wide.
fn apply_provider_params(
    spawn_spec: &mut SpawnMemberSpec,
    spec: &DurableAgentSpec,
    draft: &AgentBuildDraft,
    base_profile: Option<&meerkat_mob::Profile>,
) -> Result<(), BridgeError> {
    let declared = draft.provider_params.clone();
    if declared.is_none() && spawn_spec.override_profile.is_none() {
        return Ok(());
    }

    let Some(mut profile) = spawn_spec
        .override_profile
        .take()
        .or_else(|| base_profile.cloned())
    else {
        // Declared params with no inline profile to carry them (realm-ref
        // binding). Fail closed: dropping them silently is the failure mode
        // that reads downstream as "caching just doesn't work".
        return Err(BridgeError::InvalidInput(format!(
            "identity {} declares provider_params but profile '{}' resolves to no inline \
             definition profile to carry them; declare provider_params on the realm profile \
             instead",
            spec.identity.as_str(),
            spec.profile.as_str(),
        )));
    };

    let mut params = match declared {
        Some(mut declared) => {
            if let Some(profile_params) = profile.provider_params.as_ref() {
                merge_provider_params_missing_from(&mut declared, profile_params)?;
            }
            // The declaration must reach RESUMED sessions too: unmasked,
            // meerkat restores provider_params from durable session metadata
            // and the draft's value is inert on every identity that already
            // has a session.
            if !profile
                .resume_overrides
                .contains(&ResumeOverrideField::ProviderParams)
            {
                profile
                    .resume_overrides
                    .push(ResumeOverrideField::ProviderParams);
            }
            declared
        }
        // No declaration, but a snapshot is already in flight for another
        // reason (pinned-provider model override): the routing-hint default
        // rides it at no extra cost. The key is deterministic, so the value
        // persisted into session metadata on create restores identically on
        // resume without a mask entry.
        None => profile.provider_params.clone().unwrap_or_default(),
    };

    if spawn_is_openai_backed(draft, base_profile) {
        ensure_default_prompt_cache_key(&mut params, &spec.identity);
    }

    profile.provider_params = (!params.is_empty()).then_some(params);
    spawn_spec.override_profile = Some(profile);
    Ok(())
}

/// Build a `SpawnMemberSpec` from identity-first types, wiring draft fields.
///
/// `base_profile` is the resolved definition profile for `spec.profile`; it is
/// needed when the draft carries a model override (meerkat 0.7 removed
/// `SpawnMemberSpec::model_override` in favor of the typed `override_profile`
/// owner, so a model override is expressed as the role profile with the model
/// swapped) and when the draft carries provider params, which meerkat reads
/// only off a profile.
pub(crate) fn build_spawn_spec(
    runtime_id: &AgentRuntimeId,
    spec: &DurableAgentSpec,
    draft: &AgentBuildDraft,
    base_profile: Option<&meerkat_mob::Profile>,
) -> Result<SpawnMemberSpec, BridgeError> {
    let mid = member_id_for_spawn_spec(runtime_id, spec);
    let mut spawn_spec = SpawnMemberSpec::new(spec.profile.clone(), mid);

    if let Some(message) = spec.initial_message.as_ref() {
        spawn_spec = spawn_spec.with_initial_message(message.clone());
    }
    if let Some(runtime_mode) = spec.runtime_mode_override {
        spawn_spec = spawn_spec.with_runtime_mode(runtime_mode);
    }
    spawn_spec.backend = spec.backend;
    if let Some(binding) = spec.binding.clone() {
        spawn_spec.binding = runtime_binding_from_wire(binding);
    }
    if let Some(ref ctx) = draft.app_context {
        spawn_spec = spawn_spec.with_context(ctx.clone());
    }
    let mut labels = draft.labels.clone();
    labels.insert(
        "agent_identity".to_string(),
        spec.identity.as_str().to_string(),
    );
    labels.insert(
        "profile_name".to_string(),
        spec.profile.as_str().to_string(),
    );
    if !labels.is_empty() {
        spawn_spec = spawn_spec.with_labels(labels);
    }
    if !draft.additional_instructions.is_empty() {
        spawn_spec = spawn_spec.with_additional_instructions(draft.additional_instructions.clone());
    }
    if let Some(model) = draft.model.as_ref() {
        match base_profile {
            // A pinned provider (or self-hosted server binding) belongs to the
            // base profile's ORIGINAL model id, and meerkat applies
            // `model_override` without re-inferring the provider. Keep the
            // whole-profile snapshot for pinned profiles only — accepting the
            // definition-drift freeze `model_override` was built to end.
            //
            // The snapshot's provider is the DRAFT model's catalog owner, not
            // None: on resume the profile's resume-override mask applies
            // model and provider as a pair, and a None provider falls back to
            // the durable one — minting exactly the invalid (model, provider)
            // pair the OB3 cutover rejected typed. Catalog-unknown ids keep
            // None (definition `[models.<id>]` / config-entry resolution
            // downstream, durable-wins on resume).
            Some(base) if base.provider.is_some() || base.self_hosted_server_id.is_some() => {
                let mut profile = base.clone();
                profile.model = model.clone();
                profile.provider = meerkat_models::canonical().infer_provider(model);
                profile.self_hosted_server_id = None;
                spawn_spec.override_profile = Some(profile);
            }
            // meerkat 0.7.29 (ask 29, Bug G′): field-scoped seam. The model
            // pin is reapplied over the CURRENT definition profile on every
            // materialization (cold restore, revival, respawn), so definition
            // drift in tools/skills/peer posture keeps reaching reprofiled
            // members. Also covers realm-ref bindings, which the old
            // whole-profile path had to skip with a warning.
            _ => {
                spawn_spec.model_override = Some(model.clone());
            }
        }
    }
    if let Some(system_prompt) = draft.system_prompt.as_ref() {
        spawn_spec.system_prompt_override =
            Some(SpawnSystemPromptOverride::Replace(system_prompt.clone()));
    }
    if let Some(dispatcher) = draft.local_external_tools.dispatcher() {
        spawn_spec.external_tools = Some(dispatcher);
    }

    // Runs after the model block: it composes with whatever snapshot that
    // block installed instead of clobbering it.
    apply_provider_params(&mut spawn_spec, spec, draft, base_profile)?;

    Ok(spawn_spec)
}

/// Build the spawn spec for a RESUME of `session_id`.
///
/// Differs from a fresh spawn in two deliberate ways:
/// - `launch_mode = Resume`, so meerkat loads the persisted session
///   (conversation history intact) instead of creating a new one.
/// - `system_prompt_override` is cleared: RESUME AUTHORS NOTHING (meerkat
///   0.8.11 prompt contract, 2026-07-29 ruling). `Message::System` is an
///   ordinary ordered authored transcript message; prompt policy
///   materializes only when creating an empty transcript, and the only way
///   to change durable instructions mid-thread is a caller EXPLICITLY
///   authoring a new System message as part of a turn
///   (`StartTurnRequest.system_messages`). Re-sending assembled
///   configuration on every activation would append a byte-duplicate System
///   per boot — the prompt-refresh depth leak reborn — so the +0-on-neutral-
///   resume invariant is achieved STRUCTURALLY by the absence of an append
///   command here, not by an unchanged-value comparison anywhere. Dynamic
///   regenerated projections belong in the transient turn-context seam (or
///   tool reads), never in boot-time prompt authoring.
pub(crate) fn build_resume_spawn_spec(
    runtime_id: &AgentRuntimeId,
    spec: &DurableAgentSpec,
    draft: &AgentBuildDraft,
    base_profile: Option<&meerkat_mob::Profile>,
    session_id: &meerkat_core::types::SessionId,
) -> Result<SpawnMemberSpec, BridgeError> {
    let mut spawn_spec = build_spawn_spec(runtime_id, spec, draft, base_profile)?;
    spawn_spec.launch_mode = MemberLaunchMode::Resume {
        bridge_session_id: session_id.clone(),
    };
    spawn_spec.system_prompt_override = None;
    Ok(spawn_spec)
}

fn runtime_binding_from_wire(
    binding: meerkat_contracts::WireRuntimeBinding,
) -> Option<meerkat_mob::RuntimeBinding> {
    match binding {
        meerkat_contracts::WireRuntimeBinding::Session => {
            Some(meerkat_mob::RuntimeBinding::Session)
        }
        meerkat_contracts::WireRuntimeBinding::External {
            address,
            bootstrap_token,
            identity,
        } => {
            let resolved = identity.resolve().ok()?;
            Some(meerkat_mob::RuntimeBinding::External {
                peer_id: resolved.peer_id.to_string(),
                address,
                bootstrap_token,
                pubkey: resolved.pubkey,
            })
        }
    }
}

#[async_trait]
impl SessionBridge for MobSessionBridge {
    async fn raw_member_alias_exists(&self, alias: &str) -> Result<bool, BridgeError> {
        let members = self.handle.list_members_including_retiring().await;
        let authoritative_members = self.runtime_members.read().await;
        Ok(members.iter().any(|member| {
            crate::member_comms_id::runtime_alias_str(member.agent_identity.as_str()) == alias
                && !authoritative_members
                    .values()
                    .any(|owned| owned == member.agent_identity.as_str())
                && crate::member_comms_id::durable_identity_label(&member.labels) != Some(alias)
        }))
    }

    fn requires_resume_snapshot(&self) -> bool {
        // MemberLaunchMode::Resume loads the durable session by id from the
        // configured Meerkat session store; this bridge never reads the
        // continuity-store payload passed to resume_session.
        false
    }

    async fn recover_committed_boundary(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<CommittedBoundaryRepair, BridgeError> {
        match self.committed_boundary_recoverer.as_ref() {
            Some(recoverer) => recoverer.recover_committed_boundary(session_id).await,
            // No heal authority composed in: keep the pre-incident contract
            // honest rather than pretending the head was checked.
            None => Ok(CommittedBoundaryRepair::Unsupported),
        }
    }

    async fn create_session(
        &self,
        _identity: &AgentIdentity,
        runtime_id: &AgentRuntimeId,
        spec: &DurableAgentSpec,
        draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let mid = member_id_for_spawn_spec(runtime_id, spec);
        let spawn_spec = build_spawn_spec(
            runtime_id,
            spec,
            draft,
            self.base_profile_for_spec(spec).as_ref(),
        )?;

        self.spawn_member_spec(spawn_spec)
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;
        self.remember_runtime_member(runtime_id, &mid).await;
        self.remember_runtime_session(runtime_id, session_id).await;

        self.resolve_runtime_session_id(
            runtime_id,
            &mid,
            "member spawned but has no session ID",
            &ActorAdmissionDeadline::new(self.actor_admission_budget),
        )
        .await
    }

    async fn resume_session(
        &self,
        identity: &AgentIdentity,
        runtime_id: &AgentRuntimeId,
        spec: &DurableAgentSpec,
        draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
        _snapshot: &SessionSnapshot,
    ) -> Result<ResumeSessionOutcome, BridgeError> {
        self.log_unmasked_resume_divergence(
            identity,
            spec,
            draft,
            self.base_profile_for_spec(spec).as_ref(),
            session_id,
        )
        .await;
        if spec_uses_external_binding(spec) {
            let spawn_spec = build_resume_spawn_spec(
                runtime_id,
                spec,
                draft,
                self.base_profile_for_spec(spec).as_ref(),
                session_id,
            )?;
            let mid = member_id_for_spawn_spec(runtime_id, spec);
            self.spawn_member_spec(spawn_spec).await.map_err(|error| {
                resume_rejected(identity, session_id, &error, "external-binding resume")
            })?;
            self.remember_runtime_member(runtime_id, &mid).await;
            self.remember_runtime_session(runtime_id, session_id).await;
            return Ok(ResumeSessionOutcome::Resumed {
                session_id: session_id.clone(),
            });
        }

        // MemberLaunchMode::Resume loads the existing session from the session
        // store (conversation history intact).
        let spawn_spec = build_resume_spawn_spec(
            runtime_id,
            spec,
            draft,
            self.base_profile_for_spec(spec).as_ref(),
            session_id,
        )?;

        let mid = member_id_for_spawn_spec(runtime_id, spec);

        match self.spawn_member_spec(spawn_spec.clone()).await {
            Ok(()) => {
                self.remember_runtime_member(runtime_id, &mid).await;
                self.remember_runtime_session(runtime_id, session_id).await;
                Ok(ResumeSessionOutcome::Resumed {
                    session_id: session_id.clone(),
                })
            }
            Err(error) if is_member_already_exists_error(&error) => {
                // Genuine roster collision: an in-process restart where the
                // previous member actor hasn't fully terminated, or a Broken
                // roster entry left by an earlier rejected resume. Retire the
                // collision and retry the RESUME — never a fresh spawn; the
                // durable session must stay bound to the identity.
                tracing::warn!(
                    identity = %identity,
                    session_id = %session_id,
                    error = %error,
                    "resume_session hit a roster collision; retiring the stale member and retrying resume"
                );
                // Preconditions FIRST (OB3 run 33758a41): the retire below is
                // destructive — on the ephemeral runtime-store shape it takes
                // the stale member's in-memory state and queued inputs with
                // it. Before destroying anything, prove the session this
                // retry will resume from actually exists; a CONFIRMED-absent
                // resume source means the stale member holds the only live
                // state and retiring it destroys the session outright.
                if self.resume_source_confirmed_absent(session_id).await {
                    return Err(resume_rejected(
                        identity,
                        session_id,
                        &meerkat_mob::MobError::Internal(format!(
                            "collision retire refused: the resume source for \
                             {session_id} is confirmed absent, so retiring the stale \
                             member would destroy the only live copy of the session"
                        )),
                        "collision retire precondition",
                    ));
                }
                let capture = self.capture_pending_member_ingress(session_id).await;
                self.log_pending_ingress_before_repair_disposal(&mid, session_id, &capture);
                if let Err(err) = self.retire_session_owned_member_to_absence(&mid).await {
                    return Err(resume_rejected(
                        identity,
                        session_id,
                        &err,
                        "collision retire before resume retry",
                    ));
                }
                // meerkat 0.7.29 (ask 32): retire disposition is machine-
                // authorized and incarnation-scoped — a retire that matches
                // no committed identity is inert for this retry, so the
                // 0.7.34 drain-poll between retire and respawn is gone.
                self.forget_runtime_member(runtime_id).await;
                if let Err(error) = self.spawn_member_spec(spawn_spec).await {
                    if !capture.carryable.is_empty() {
                        tracing::error!(
                            identity = %identity,
                            session_id = %session_id,
                            lost = capture.carryable.len(),
                            "the collision retire already destroyed the member's queued \
                             inputs and the resume retry failed; the captured inputs are \
                             lost with it"
                        );
                    }
                    self.verify_durable_session_after_rejected_resume(identity, session_id)
                        .await;
                    return Err(resume_rejected(
                        identity,
                        session_id,
                        &error,
                        "resume retry after collision",
                    ));
                }
                self.remember_runtime_member(runtime_id, &mid).await;
                self.remember_runtime_session(runtime_id, session_id).await;
                self.readmit_carried_inputs(&mid, session_id, capture).await;
                Ok(ResumeSessionOutcome::Resumed {
                    session_id: session_id.clone(),
                })
            }
            // Any other resume failure: REFUSE to fall back to a fresh spawn.
            // The durable session row exists and is the only copy of the
            // conversation; rebinding the identity to a fresh empty session
            // would permanently abandon it (the HomeCore restart-loss bug).
            // Surface a typed rejection so the caller marks the identity
            // degraded and the next reconcile retries the resume.
            //
            // ONE narrowly-typed exception (OB3 2026-07-30 fleet wedge, 30
            // identities): meerkat's typed Absent - "no durable session
            // exists for this id" - combined with the store confirming no
            // row was ever persisted, is the never-persisted continuity
            // head shape (a registration/rebind-minted record whose quiet
            // member skipped every content-less save). There is no
            // transcript to preserve; refusing forever is a permanent
            // Broken retry loop. Fall back to a FRESH spawn under a new
            // session id. Both gates are required: typed Absent alone
            // fails closed when the store probe errors or is absent, and
            // every other refusal (archived-intact, held, quarantined,
            // unknown) keeps the never-abandon contract above.
            Err(error) => {
                if durable_snapshot_is_typed_absent(&error)
                    && self.durable_session_row_is_absent(session_id).await
                {
                    tracing::warn!(
                        identity = %identity,
                        session_id = %session_id,
                        error = %error,
                        "resume target is typed-Absent and the durable store has no row for \
                         it (never-persisted continuity head): falling back to a FRESH spawn \
                         under a new session id. External row deletion produces this same \
                         shape - investigate if unexpected"
                    );
                    // The failed resume attempt may have left a stale roster
                    // entry; retire is inert when nothing matches (see the
                    // collision arm above). When it DOES match, the retire is
                    // destructive: capture the stale member's queued inputs
                    // first and carry them into the fresh successor session.
                    let capture = self.capture_pending_member_ingress(session_id).await;
                    self.log_pending_ingress_before_repair_disposal(&mid, session_id, &capture);
                    if let Err(retire_error) =
                        self.retire_session_owned_member_to_absence(&mid).await
                    {
                        return Err(resume_rejected(
                            identity,
                            session_id,
                            &retire_error,
                            "never-persisted retire before fresh fallback",
                        ));
                    }
                    self.forget_runtime_member(runtime_id).await;
                    let fresh_session_id = meerkat_core::types::SessionId::new();
                    let created_session_id = match self
                        .create_session(identity, runtime_id, spec, draft, &fresh_session_id)
                        .await
                    {
                        Ok(created) => created,
                        Err(create_error) => {
                            if !capture.carryable.is_empty() {
                                tracing::error!(
                                    identity = %identity,
                                    session_id = %session_id,
                                    lost = capture.carryable.len(),
                                    "the never-persisted retire already destroyed the \
                                     member's queued inputs and the fresh spawn failed; \
                                     the captured inputs are lost with it"
                                );
                            }
                            return Err(create_error);
                        }
                    };
                    self.readmit_carried_inputs(&mid, &created_session_id, capture)
                        .await;
                    return Ok(ResumeSessionOutcome::FreshSpawned {
                        session_id: created_session_id,
                        reason: ResumeFallbackReason::NeverPersisted {
                            detail: error.to_string(),
                        },
                    });
                }
                self.verify_durable_session_after_rejected_resume(identity, session_id)
                    .await;
                Err(resume_rejected(
                    identity,
                    session_id,
                    &error,
                    "resume spawn",
                ))
            }
        }
    }

    async fn deliver_admitted(
        &self,
        runtime_id: &AgentRuntimeId,
        delivery: BridgeDelivery,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        // Admission-only: no turn is in flight, so the receipt's completion is
        // an immediate Ok and propagating the resolution result here drops
        // nothing. Identical to the pre-receipt behaviour.
        let receipt = self
            .deliver_admitted_inner(runtime_id, delivery, BridgeSubmitMode::AdmissionOnly)
            .await
            .map_err(BridgeError::from)?;
        receipt.wait().await.map_err(BridgeError::from)
    }

    /// Completion-bearing override: identical admission, returning the turn's
    /// receipt UNAWAITED so the caller can release its locks first.
    ///
    /// The trait's `deliver_awaiting_commit_*` default composes this with
    /// `wait()`, so there is no second admission path to drift.
    async fn begin_awaiting_commit(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        system_prompt: Option<&str>,
        injected_context: &[meerkat_core::ContentInput],
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<BridgeTurnReceipt, BridgeAdmissionError> {
        let mut delivery = BridgeDelivery::new(content.clone(), handling_mode);
        delivery.system_prompt = system_prompt.map(ToString::to_string);
        delivery.injected_context = injected_context.to_vec();
        delivery.interaction_id = interaction_id.map(ToString::to_string);
        self.deliver_admitted_inner(runtime_id, delivery, BridgeSubmitMode::CompletionBearing)
            .await
    }

    async fn checkpoint_session(
        &self,
        _runtime_id: &AgentRuntimeId,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<SessionSnapshot, BridgeError> {
        let store = self.session_store.as_ref().ok_or_else(|| {
            BridgeError::InvalidInput(
                "checkpoint requires a session store but none was configured".to_string(),
            )
        })?;

        let session = store
            .load(session_id)
            .await
            .map_err(|e| BridgeError::Mob(format!("failed to load session for checkpoint: {e}")))?
            .ok_or_else(|| {
                BridgeError::Mob(format!(
                    "session {session_id} not found in store for checkpoint"
                ))
            })?;

        let data = serde_json::to_vec(&session)
            .map_err(|e| BridgeError::Mob(format!("failed to serialize session: {e}")))?;

        Ok(SessionSnapshot { data })
    }

    async fn retire_member(&self, runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
        let mid = self.member_id_for_runtime_id(runtime_id).await;
        self.retire_session_owned_member_to_absence(&mid)
            .await
            .map_err(|error| BridgeError::Mob(error.to_string()))?;
        self.forget_runtime_member(runtime_id).await;
        Ok(())
    }

    async fn retire_reset_superseded_member(
        &self,
        runtime_id: &AgentRuntimeId,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), BridgeError> {
        let mid = self.member_id_for_runtime_id(runtime_id).await;
        let adapter = self.continuity_session_store.as_ref().ok_or_else(|| {
            BridgeError::Mob(format!(
                "reset retire cannot abandon superseded member {mid}: the bridge has no continuity session-store authority"
            ))
        })?;
        abandon_then_retire_reset_superseded(
            &mid,
            session_id,
            || adapter.abandon_superseded_session(session_id),
            || self.handle.retire(mid.clone()),
        )
        .await?;

        if self
            .handle
            .list_all_members()
            .await
            .iter()
            .any(|entry| entry.agent_identity == mid)
        {
            return Err(BridgeError::Mob(format!(
                "reset retire reported success but retained roster anchor {mid}"
            )));
        }

        self.forget_runtime_member(runtime_id).await;
        self.unregister_session_runtime_state(session_id).await
    }

    async fn wire_peer(&self, a: &AgentRuntimeId, b: &AgentRuntimeId) -> Result<(), BridgeError> {
        let member_a = self.member_id_for_runtime_id(a).await;
        let member_b = self.member_id_for_runtime_id(b).await;
        self.handle
            .wire(
                meerkat_mob::AgentIdentity::from(member_a.as_str()),
                member_b,
            )
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))
    }

    async fn wire_peers_batch(
        &self,
        edges: &[(AgentRuntimeId, AgentRuntimeId)],
    ) -> Result<(), BridgeError> {
        let mut member_edges = Vec::with_capacity(edges.len());
        for (a, b) in edges {
            let member_a = self.member_id_for_runtime_id(a).await;
            let member_b = self.member_id_for_runtime_id(b).await;
            member_edges.push((
                meerkat_mob::AgentIdentity::from(member_a.as_str()),
                meerkat_mob::AgentIdentity::from(member_b.as_str()),
            ));
        }
        match self.handle.wire_members_batch(member_edges.clone()).await {
            Ok(_) => Ok(()),
            Err(error)
                if error
                    .to_string()
                    .contains("does not support legacy external (peer-only) members") =>
            {
                // Meerkat's dense batch operation is intentionally local-only.
                // Its ordinary wire command is the generated reciprocal-trust
                // authority for local↔peer-only edges, so retry the normalized
                // edge set through that existing operation instead of
                // fabricating an external-peer side channel.
                for (member_a, member_b) in member_edges {
                    self.handle
                        .wire(member_a, member_b)
                        .await
                        .map_err(|error| BridgeError::Mob(error.to_string()))?;
                }
                Ok(())
            }
            Err(error) => Err(BridgeError::Mob(error.to_string())),
        }
    }

    async fn current_member_wires(
        &self,
    ) -> Result<Vec<(AgentRuntimeId, AgentRuntimeId)>, BridgeError> {
        self.member_wires(true).await
    }

    async fn current_member_wires_any_half(
        &self,
    ) -> Result<Vec<(AgentRuntimeId, AgentRuntimeId)>, BridgeError> {
        self.member_wires(false).await
    }

    async fn unwire_peer(&self, a: &AgentRuntimeId, b: &AgentRuntimeId) -> Result<(), BridgeError> {
        let member_a = self.member_id_for_runtime_id(a).await;
        let member_b = self.member_id_for_runtime_id(b).await;
        match self
            .handle
            .unwire(
                meerkat_mob::AgentIdentity::from(member_a.as_str()),
                member_b,
            )
            .await
        {
            Ok(()) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                if message.contains("peer not found") || message.contains("not wired") {
                    Ok(())
                } else {
                    Err(BridgeError::Mob(message))
                }
            }
        }
    }

    async fn inspect_member(
        &self,
        runtime_id: &AgentRuntimeId,
    ) -> Result<MemberInspection, BridgeError> {
        let mid = self.member_id_for_runtime_id(runtime_id).await;
        let snap = self
            .handle
            .member_status(&mid)
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;
        let peer_reachable_count =
            match peer_reachable_count_from_connectivity(snap.peer_connectivity.as_ref()) {
                Some(count) => count,
                None => self
                    .handle
                    .get_member(&mid)
                    .await
                    .ok()
                    .flatten()
                    .map(|entry| entry.wired_to.len())
                    .unwrap_or(0),
            };
        Ok(MemberInspection {
            output_preview: snap.output_preview.clone(),
            is_final: snap.is_final,
            peer_reachable_count,
        })
    }

    async fn register_session_runtime_state(
        &self,
        session_id: &meerkat_core::types::SessionId,
        identity: &AgentIdentity,
        generation: ContinuityGeneration,
        checkpoint_version: CheckpointVersion,
        fencing_token: FencingToken,
    ) -> Result<CheckpointVersion, BridgeError> {
        if let Some(adapter) = self.continuity_session_store.as_ref() {
            return adapter
                .register_session(
                    session_id,
                    SessionRuntimeState {
                        identity: identity.clone(),
                        generation,
                        checkpoint_version,
                        fencing_token,
                    },
                )
                .await
                .map_err(|err| BridgeError::Mob(format!("continuity register_session: {err}")));
        }
        Ok(checkpoint_version)
    }

    async fn suspend_session_runtime_state(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), BridgeError> {
        if let Some(adapter) = self.continuity_session_store.as_ref() {
            adapter
                .suspend_session(session_id)
                .await
                .map_err(|err| BridgeError::Mob(format!("continuity suspend_session: {err}")))?;
        }
        Ok(())
    }

    async fn unregister_session_runtime_state(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), BridgeError> {
        if let Some(adapter) = self.continuity_session_store.as_ref() {
            adapter
                .unregister_session(session_id)
                .await
                .map_err(|err| BridgeError::Mob(format!("continuity unregister_session: {err}")))?;
        }
        Ok(())
    }
}

impl MobSessionBridge {
    /// Shared admission body for both the ingress-only and completion-bearing
    /// deliveries. The [`BridgeSubmitMode`] selects only which meerkat submit
    /// verb is used; every other step - budget, stale-state repair and retry,
    /// session resolution - is common by construction so the two lanes cannot
    /// drift.
    async fn deliver_admitted_inner(
        &self,
        runtime_id: &AgentRuntimeId,
        delivery: BridgeDelivery,
        mode: BridgeSubmitMode,
    ) -> Result<BridgeTurnReceipt, BridgeAdmissionError> {
        let content = &delivery.content;
        let handling_mode = delivery.handling_mode;
        let system_prompt = delivery.system_prompt.as_deref();
        let injected_context = delivery.injected_context.as_slice();
        let interaction_id = delivery.interaction_id.as_deref();
        let delivery_identity = delivery.delivery_identity.as_ref();
        let mid = self.member_id_for_runtime_id(runtime_id).await;
        // One admission budget for the whole attempt, shared by every actor
        // round trip below: the serialized hops must not each cost a budget.
        let mut deadline = ActorAdmissionDeadline::new(self.actor_admission_budget);
        // Best-effort repair material: a faulted or timed-out lookup degrades
        // to "no pre-delivery entry" (the delivery itself will surface the
        // fault; a blocked actor hits the same deadline again below).
        let member_entry_before_delivery = deadline
            .bound(
                "deliver.get_member.pre_delivery",
                &mid,
                self.handle.get_member(&mid),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
            .map(|entry| (entry.role, entry.labels));
        if content_input_has_images(content) {
            let member_entry = deadline
                .bound(
                    "deliver.get_member.image_capability",
                    &mid,
                    self.handle.get_member(&mid),
                )
                .await?
                .map_err(|err| BridgeError::Mob(err.to_string()))?
                .ok_or_else(|| {
                    BridgeError::Mob("member not found while checking image capability".to_string())
                })?;
            let caps = deadline
                .bound(
                    "deliver.model_capabilities",
                    &mid,
                    model_capabilities_for_member(
                        &self.handle,
                        self.session_service.as_ref(),
                        &member_entry.agent_identity,
                    ),
                )
                .await?;
            if !caps.image_input {
                return Err(BridgeAdmissionError::InvalidInput(
                    "target member model cannot accept image input".to_string(),
                ));
            }
        }

        // Bound BY the match, not pre-initialised: every arm either yields the
        // handle or returns, so there is no state in which a turn was admitted
        // and `pending_turn` does not hold its handle.
        let pending_turn: Option<meerkat_mob::WorkTurnHandle> = match submit_internal_bridge_work(
            &self.handle,
            &mid,
            InternalBridgeWork {
                content,
                system_prompt,
                injected_context,
                interaction_id,
                delivery_identity,
            },
            handling_mode,
            &deadline,
            mode,
        )
        .await
        {
            Ok(turn) => turn,
            // A blocked actor is not stale runtime state: keep the typed
            // timeout instead of routing it into member repair (which cannot
            // unblock an actor) or laundering it into `Mob(String)` below.
            Err(err @ BridgeError::ActorAdmissionTimeout { .. }) => return Err(err.into()),
            Err(err) if is_repairable_bridge_delivery_error(&err.to_string()) => {
                tracing::warn!(
                    runtime_id = %runtime_id,
                    error = %err,
                    "identity bridge delivery found stale runtime state; repairing member before retry"
                );
                Box::pin(self.repair_member_for_delivery(
                    runtime_id,
                    &mid,
                    member_entry_before_delivery,
                ))
                .await?;
                // Repair is a distinct multi-step recovery with its own
                // (pre-existing, unbounded) cost; the retry is a new admission
                // attempt and gets a fresh budget.
                deadline = ActorAdmissionDeadline::new(self.actor_admission_budget);
                // The retry's handle is kept exactly like the first attempt's:
                // dropping it here would abandon an admitted, running turn on
                // the repair path specifically.
                submit_internal_bridge_work(
                    &self.handle,
                    &mid,
                    InternalBridgeWork {
                        content,
                        system_prompt,
                        injected_context,
                        interaction_id,
                        delivery_identity,
                    },
                    handling_mode,
                    &deadline,
                    mode,
                )
                .await?
            }
            Err(err) => return Err(BridgeAdmissionError::Mob(err.to_string())),
        };

        // Resolution runs under the admission deadline, and its outcome is
        // CARRIED, not propagated. The handle is moved into the receipt without
        // the resolution result ever being inspected here, so there is no path
        // on which a resolution failure returns early and abandons a turn that
        // is already running. That invariant used to depend on statement order;
        // now it is structural.
        let session_result = self
            .resolve_runtime_session_id(
                runtime_id,
                &mid,
                "member has no bridge session after deliver",
                &deadline,
            )
            .await;

        Ok(BridgeTurnReceipt::new(session_result, async move {
            match pending_turn {
                // The LLM turn. Awaited by the CALLER, after it has released
                // whatever lock it holds - never here, never under a lock.
                Some(turn) => turn.wait().await.map(|_| ()).map_err(|err| err.to_string()),
                // Admission-only: nothing to wait for.
                None => Ok::<(), String>(()),
            }
        }))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use std::sync::Arc;

    /// The ordering property, proven by direct poll rather than by timing.
    ///
    /// Session resolution must COMPLETE before the completion future is polled
    /// even once. A wall-clock version of this test was written first, and a
    /// negative mutation proved it inert - it passed with the awaits swapped.
    /// This one cannot: swapping them makes the completion future observe an
    /// empty trace on its first poll and panic there.
    #[test]
    fn session_resolution_completes_before_the_completion_future_is_polled() {
        use std::sync::Mutex;
        use std::task::{Context, Poll, Waker};

        let trace: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let released = Arc::new(std::sync::atomic::AtomicBool::new(false));

        /// Records its first poll, asserts resolution already happened, then
        /// parks until released.
        struct GatedCompletion {
            trace: Arc<Mutex<Vec<&'static str>>>,
            released: Arc<std::sync::atomic::AtomicBool>,
            polled: bool,
        }
        impl std::future::Future for GatedCompletion {
            type Output = Result<(), BridgeError>;
            fn poll(
                mut self: std::pin::Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<Self::Output> {
                if !self.polled {
                    assert_eq!(
                        self.trace.lock().expect("trace mutex poisoned").as_slice(),
                        ["SessionResolved"],
                        "the completion future was polled before session resolution finished - \
                         the admission budget would be spent on the turn"
                    );
                    self.trace
                        .lock()
                        .expect("trace mutex poisoned")
                        .push("CompletionPolled");
                    self.polled = true;
                }
                if self.released.load(std::sync::atomic::Ordering::SeqCst) {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }
        }

        let session = meerkat_core::types::SessionId::new();
        let expected = session.clone();
        let resolve_trace = Arc::clone(&trace);
        let resolve = async move {
            resolve_trace
                .lock()
                .expect("trace mutex poisoned")
                .push("SessionResolved");
            Ok::<_, BridgeError>(session)
        };
        let completion = GatedCompletion {
            trace: Arc::clone(&trace),
            released: Arc::clone(&released),
            polled: false,
        };

        // Resolution happens in `begin`, BEFORE the receipt exists, so the
        // ordering is now structural: a receipt cannot be constructed without a
        // resolution outcome in hand. What still needs proving is that `wait`
        // parks on the completion rather than returning on the carried result.
        let resolved = futures::executor::block_on(resolve).expect("resolution");
        let combined =
            super::BridgeTurnReceipt::new(Ok::<_, BridgeError>(resolved), completion).wait();
        let mut combined = Box::pin(combined);
        let mut cx = Context::from_waker(Waker::noop());

        assert!(
            combined.as_mut().poll(&mut cx).is_pending(),
            "must park on the completion, not return at resolution"
        );
        assert_eq!(
            trace.lock().expect("trace mutex poisoned").as_slice(),
            ["SessionResolved", "CompletionPolled"],
            "exact ordering: resolve fully, THEN poll the completion"
        );

        released.store(true, std::sync::atomic::Ordering::SeqCst);
        match combined.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(got)) => assert_eq!(got, expected, "must return the resolved session"),
            other => panic!("expected the resolved session once released, got {other:?}"),
        }
    }

    /// P1: a resolution failure AFTER admission must not drop the turn.
    ///
    /// Proven by direct poll, not by timing. The resolution error is
    /// deliberately admission-shaped AND repairable by
    /// `is_repairable_bridge_delivery_error`, which is the dangerous case: an
    /// early return here would abandon an admitted, running turn and hand the
    /// delivery path an error it would repair and resubmit, running the
    /// member's turn a second time.
    ///
    /// Two things are asserted that an early return cannot satisfy: the
    /// completion future is polled at all, and the combined future parks on it
    /// rather than returning at resolution.
    #[test]
    fn a_post_admission_resolution_failure_still_awaits_the_turn_and_is_not_repairable() {
        use std::sync::Mutex;
        use std::task::{Context, Poll, Waker};

        let polls: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let released = Arc::new(std::sync::atomic::AtomicBool::new(false));

        struct CountedCompletion {
            polls: Arc<Mutex<usize>>,
            released: Arc<std::sync::atomic::AtomicBool>,
        }
        impl std::future::Future for CountedCompletion {
            type Output = Result<(), BridgeError>;
            fn poll(self: std::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                *self.polls.lock().expect("poll counter mutex poisoned") += 1;
                if self.released.load(std::sync::atomic::Ordering::SeqCst) {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }
        }

        // Admission-shaped AND accepted by the repair classifier.
        let resolve_detail = "missing bridge session snapshot for member rt-agent-alpha-0";
        assert!(
            is_repairable_bridge_delivery_error(resolve_detail),
            "the test is only meaningful if the resolution error is one the \
             delivery path WOULD repair and resubmit"
        );
        let completion = CountedCompletion {
            polls: Arc::clone(&polls),
            released: Arc::clone(&released),
        };

        let combined = super::BridgeTurnReceipt::new(
            Err(BridgeError::Mob(resolve_detail.to_string())),
            completion,
        )
        .wait();
        let mut combined = Box::pin(combined);
        let mut cx = Context::from_waker(Waker::noop());

        assert!(
            combined.as_mut().poll(&mut cx).is_pending(),
            "resolution failed, but the turn is still running: the combined \
             future must park on the completion, not return and drop it"
        );
        assert_eq!(
            *polls.lock().expect("poll counter mutex poisoned"),
            1,
            "the completion future was never polled - the admitted turn was dropped"
        );

        released.store(true, std::sync::atomic::Ordering::SeqCst);
        match combined.as_mut().poll(&mut cx) {
            Poll::Ready(Err(BridgeTurnError::PostAdmissionResolutionFailed(detail))) => {
                assert!(
                    detail.contains("missing bridge session snapshot"),
                    "the resolution detail must survive verbatim: {detail}"
                );
                // The protection is the VARIANT, not the wording: this error is
                // produced after admission and is never handed to the delivery
                // path's substring classifier. Assert the variant is not one of
                // the admission-shaped ones it acts on.
                let rendered = BridgeTurnError::PostAdmissionResolutionFailed(detail).to_string();
                assert!(
                    rendered.contains("do not retry"),
                    "the rendering must tell an operator this is terminal: {rendered}"
                );
            }
            other => panic!(
                "a turn that SUCCEEDED after a failed resolution must surface as \
                 PostAdmissionResolutionFailed - anything admission-shaped could be \
                 repaired and resubmitted, running the turn twice. Got: {other:?}"
            ),
        }
    }

    /// When BOTH fail, the turn's failure is the outcome and leads, and the
    /// resolution detail is carried rather than silently dropped.
    #[tokio::test]
    async fn both_failing_leads_with_the_turn_and_still_carries_the_resolution_detail() {
        let failed = super::BridgeTurnReceipt::new(
            Err(BridgeError::Mob("resolution detail marker".to_string())),
            async { Err::<(), _>(BridgeError::Mob("turn failure marker".to_string())) },
        )
        .wait()
        .await;
        match failed {
            Err(BridgeTurnError::CompletionFailed(detail)) => {
                assert!(
                    detail.contains("turn failure marker"),
                    "the turn's own failure must lead: {detail}"
                );
                assert!(
                    detail.contains("resolution detail marker"),
                    "the resolution failure must not be silently dropped: {detail}"
                );
            }
            other => panic!(
                "a turn that RAN and failed is a completion failure regardless of \
                 what resolution did. Got: {other:?}"
            ),
        }
    }

    /// A completion failure must be typed `CompletionFailed`, never anything
    /// admission-shaped that the repair-and-retry classifier could act on.
    #[tokio::test]
    async fn a_completion_failure_is_typed_and_never_admission_shaped() {
        let session = meerkat_core::types::SessionId::new();
        let failed = super::BridgeTurnReceipt::new(Ok::<_, BridgeError>(session), async {
            Err::<(), _>(BridgeError::Mob(
                "missing bridge session snapshot - completion sentinel".to_string(),
            ))
        })
        .wait()
        .await;
        match failed {
            Err(BridgeTurnError::CompletionFailed(detail)) => assert!(
                detail.contains("missing bridge session snapshot"),
                "the detail must survive so operators can see what failed: {detail}"
            ),
            other => panic!(
                "a completion failure whose text is ACCEPTED by \
                 is_repairable_bridge_delivery_error must still surface as CompletionFailed, \
                 or the delivery path could repair and resubmit a turn that already ran. \
                 Got: {other:?}"
            ),
        }
    }

    /// 8a threading pin: every optional carrier on the internal deliver
    /// surface reaches the WorkSpec unchanged - in particular the per-turn
    /// System message (meerkat 0.8.11 `WorkSpec::system_prompt`), threaded
    /// exactly like `injected_context`.
    #[test]
    fn internal_bridge_work_spec_threads_every_carrier() {
        let content = meerkat_core::ContentInput::Text("turn content".to_string());
        let injected = vec![meerkat_core::ContentInput::Text(
            "ambient recall".to_string(),
        )];
        let interaction = uuid::Uuid::new_v4();

        let spec = super::internal_bridge_work_spec(
            &content,
            Some("per-turn system message"),
            &injected,
            Some(&interaction.to_string()),
        );
        assert_eq!(
            spec.system_prompt.as_deref(),
            Some("per-turn system message")
        );
        assert_eq!(spec.injected_context, injected);
        assert_eq!(
            spec.interaction_id,
            Some(meerkat_core::interaction::InteractionId(interaction))
        );
        assert!(matches!(spec.origin, meerkat_mob::WorkOrigin::Internal));

        let bare = super::internal_bridge_work_spec(&content, None, &[], None);
        assert_eq!(bare.system_prompt, None, "absent carrier stays absent");
        assert!(bare.injected_context.is_empty());
        assert_eq!(bare.interaction_id, None);
    }

    use async_trait::async_trait;
    use meerkat_core::agent::AgentToolDispatcher;
    use meerkat_core::lifecycle::run_primitive::{OpenAiPromptCacheOptions, ReasoningEffort};
    use meerkat_core::model_profile::capabilities::{OpenAiPromptCacheMode, OpenAiPromptCacheTtl};
    use meerkat_core::types::ToolCallView;
    use meerkat_core::{ToolDef, error::ToolError, ops::ToolDispatchOutcome};
    use meerkat_mob::{MobRespawnError, MobRuntimeMode};

    use super::*;
    use crate::identity_first::{AgentAddressability, LocalExternalToolOverlay};

    struct EmptyDispatcher;

    #[async_trait]
    impl AgentToolDispatcher for EmptyDispatcher {
        fn tools(&self) -> Arc<[Arc<ToolDef>]> {
            Arc::from([])
        }

        async fn dispatch(
            &self,
            _call: ToolCallView<'_>,
        ) -> Result<ToolDispatchOutcome, ToolError> {
            Err(ToolError::ExecutionFailed {
                message: "not implemented".to_string(),
            })
        }
    }

    /// Resume-divergence tripwire semantics: unmasked + declared + different
    /// flags the field; a mask or an absent declaration silences it.
    #[test]
    fn unmasked_resume_divergence_flags_only_unmasked_declared_differences() {
        let unmasked = meerkat_core::service::ResumeOverrideMask::default();
        let divergence = unmasked_resume_divergence(
            &unmasked,
            "claude-opus-4-8",
            Some(meerkat_core::Provider::Anthropic),
            "claude-sonnet-4-5",
            meerkat_core::Provider::OpenAI,
        );
        assert!(divergence.model, "unmasked differing model must be flagged");
        assert!(
            divergence.provider,
            "unmasked differing declared provider must be flagged"
        );

        let masked = meerkat_core::service::ResumeOverrideMask {
            model: true,
            provider: true,
            ..Default::default()
        };
        let divergence = unmasked_resume_divergence(
            &masked,
            "claude-opus-4-8",
            Some(meerkat_core::Provider::Anthropic),
            "claude-sonnet-4-5",
            meerkat_core::Provider::OpenAI,
        );
        assert!(
            !divergence.model && !divergence.provider,
            "a mask covering the field silences the tripwire (the profile wins anyway)"
        );

        let divergence = unmasked_resume_divergence(
            &unmasked,
            "claude-opus-4-8",
            None,
            "claude-opus-4-8",
            meerkat_core::Provider::OpenAI,
        );
        assert!(!divergence.model, "an identical model is not a divergence");
        assert!(
            !divergence.provider,
            "an undeclared provider states no intent to diverge from"
        );
    }

    fn durable_spec() -> DurableAgentSpec {
        DurableAgentSpec {
            identity: AgentIdentity::parse("agent:alpha").expect("identity"),
            profile: meerkat_mob::ProfileName::from("worker"),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: Default::default(),
            context: None,
            additional_instructions: Vec::new(),
            initial_message: Some(meerkat_core::ContentInput::Text("hello".to_string())),
            runtime_mode_override: Some(MobRuntimeMode::TurnDriven),
            backend: None,
            binding: None,
        }
    }

    /// Regression: meerkat 0.7.1 made `member_status` peer connectivity a
    /// tri-state. Only the `Known` arm carries a live count; the
    /// not-applicable / probe-timed-out arms must defer to the machine-owned
    /// wiring degree (`wired_to.len()`) instead of projecting 0, or
    /// `peer_reachable_count` reads 0 for freshly role-wired members and
    /// topology verification breaks.
    #[test]
    fn peer_reachable_count_tri_state_defers_to_wiring_when_unresolved() {
        use meerkat_contracts::{WirePeerConnectivity, WirePeerConnectivitySnapshot};

        let known = WirePeerConnectivity::Known {
            snapshot: WirePeerConnectivitySnapshot {
                reachable_peer_count: 3,
                unknown_peer_count: 0,
                unreachable_peers: Vec::new(),
            },
        };
        assert_eq!(
            peer_reachable_count_from_connectivity(Some(&known)),
            Some(3),
            "a resolved probe owns the count"
        );
        let structurally_wired_but_unresolved = WirePeerConnectivity::Known {
            snapshot: WirePeerConnectivitySnapshot {
                reachable_peer_count: 0,
                unknown_peer_count: 3,
                unreachable_peers: Vec::new(),
            },
        };
        assert_eq!(
            peer_reachable_count_from_connectivity(Some(&structurally_wired_but_unresolved)),
            None,
            "a partially resolved probe must defer to the machine-owned wiring degree"
        );
        assert_eq!(
            peer_reachable_count_from_connectivity(Some(&WirePeerConnectivity::NotApplicable)),
            None,
            "not-applicable must defer to the wiring fallback"
        );
        assert_eq!(
            peer_reachable_count_from_connectivity(Some(&WirePeerConnectivity::ProbeTimedOut)),
            None,
            "probe timeout must defer to the wiring fallback"
        );
        assert_eq!(peer_reachable_count_from_connectivity(None), None);
    }

    #[test]
    fn build_spawn_spec_maps_identity_first_overrides() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("team".to_string(), "ops".to_string());
        let draft = AgentBuildDraft {
            compaction_curator: Default::default(),
            model: Some("gpt-test".to_string()),
            system_prompt: Some("system override".to_string()),
            additional_instructions: vec!["stay focused".to_string()],
            labels,
            app_context: Some(serde_json::json!({"ticket": 7})),
            external_tools: Vec::new(),
            local_external_tools: LocalExternalToolOverlay::new(Arc::new(EmptyDispatcher)),
            provider_params: None,
        };

        let base_profile: meerkat_mob::Profile =
            serde_json::from_value(serde_json::json!({"model": "base-model"}))
                .expect("minimal profile");
        let spawn = build_spawn_spec(&runtime_id, &durable_spec(), &draft, Some(&base_profile))
            .expect("spawn spec");

        // meerkat 0.7.29 (ask 29): an unpinned base profile takes the
        // field-scoped seam — the model pin follows definition drift instead
        // of freezing the whole profile (Bug G′).
        assert_eq!(spawn.model_override.as_deref(), Some("gpt-test"));
        assert!(
            spawn.override_profile.is_none(),
            "unpinned profiles must not freeze the whole profile for a model-only override"
        );
        assert_eq!(
            spawn.system_prompt_override,
            Some(SpawnSystemPromptOverride::Replace(
                "system override".to_string()
            ))
        );
        assert!(spawn.external_tools.is_some());
        assert_eq!(spawn.runtime_mode, Some(MobRuntimeMode::TurnDriven));
        assert_eq!(
            spawn.initial_message,
            Some(meerkat_core::ContentInput::Text("hello".to_string()))
        );
        assert_eq!(
            spawn
                .labels
                .as_ref()
                .and_then(|labels| labels.get("team"))
                .map(String::as_str),
            Some("ops")
        );
        assert_eq!(
            spawn
                .labels
                .as_ref()
                .and_then(|labels| labels.get("agent_identity"))
                .map(String::as_str),
            Some("agent:alpha")
        );
        assert_eq!(
            spawn
                .labels
                .as_ref()
                .and_then(|labels| labels.get("profile_name"))
                .map(String::as_str),
            Some("worker"),
            "identity-first spawn labels must carry the adopted profile so the \
             SDK build callback sees the roster profile, not a checkpoint default"
        );
        assert_eq!(
            spawn.role_name.as_str(),
            "worker",
            "SpawnMemberSpec role remains the authoritative mob profile"
        );
    }

    /// A base profile that pins a provider (or self-hosted binding) keeps the
    /// legacy whole-profile snapshot: upstream `model_override` does not
    /// re-infer the provider, and the pin belongs to the original model id.
    #[test]
    fn build_spawn_spec_keeps_profile_snapshot_for_pinned_provider() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = AgentBuildDraft {
            compaction_curator: Default::default(),
            model: Some("gpt-test".to_string()),
            system_prompt: None,
            additional_instructions: Vec::new(),
            labels: Default::default(),
            app_context: None,
            external_tools: Vec::new(),
            local_external_tools: LocalExternalToolOverlay::new(Arc::new(EmptyDispatcher)),
            provider_params: None,
        };
        let base_profile: meerkat_mob::Profile = serde_json::from_value(
            serde_json::json!({"model": "base-model", "provider": "openai"}),
        )
        .expect("pinned profile");

        let spawn = build_spawn_spec(&runtime_id, &durable_spec(), &draft, Some(&base_profile))
            .expect("spawn spec");

        assert!(spawn.model_override.is_none());
        let profile = spawn
            .override_profile
            .as_ref()
            .expect("pinned profile keeps the snapshot path");
        assert_eq!(profile.model.as_str(), "gpt-test");
        assert!(
            profile.provider.is_none(),
            "a catalog-unknown pin carries no provider (config-entry resolution downstream)"
        );
    }

    /// A draft model pin on a pinned-provider base must carry the DRAFT
    /// model's catalog owner in the snapshot, applied with the model as a
    /// pair. Clearing it to None (the pre-OB3 shape) let resume fall back to
    /// the durable provider under the pinned model — the exact invalid
    /// (model, provider) pair the incident rejected typed.
    #[test]
    fn build_spawn_spec_derives_pair_provider_for_catalog_model_pin() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = AgentBuildDraft {
            compaction_curator: Default::default(),
            model: Some("claude-opus-4-8".to_string()),
            system_prompt: None,
            additional_instructions: Vec::new(),
            labels: Default::default(),
            app_context: None,
            external_tools: Vec::new(),
            local_external_tools: LocalExternalToolOverlay::new(Arc::new(EmptyDispatcher)),
            provider_params: None,
        };
        // Post-auto-mark shape: the definition profile carries its model's
        // owner and the pair mask.
        let base_profile: meerkat_mob::Profile = serde_json::from_value(serde_json::json!({
            "model": "gpt-5.5",
            "provider": "openai",
            "resume_overrides": ["model", "provider"],
        }))
        .expect("pinned profile");

        let spawn = build_spawn_spec(&runtime_id, &durable_spec(), &draft, Some(&base_profile))
            .expect("spawn spec");

        let profile = spawn
            .override_profile
            .as_ref()
            .expect("pinned profile keeps the snapshot path");
        assert_eq!(profile.model.as_str(), "claude-opus-4-8");
        assert_eq!(
            profile.provider,
            Some(meerkat_core::Provider::Anthropic),
            "the pin's provider must be the DRAFT model's catalog owner, applied as a pair"
        );
        assert!(
            profile
                .resume_overrides
                .contains(&ResumeOverrideField::Model)
                && profile
                    .resume_overrides
                    .contains(&ResumeOverrideField::Provider),
            "the base profile's pair mask must ride the snapshot so both fields apply on resume"
        );
    }

    // -----------------------------------------------------------------
    // Provider params / prompt-cache plumbing
    // -----------------------------------------------------------------

    fn profile_with_provider(provider: &str) -> meerkat_mob::Profile {
        serde_json::from_value(serde_json::json!({
            "model": "base-model",
            "provider": provider,
        }))
        .expect("profile")
    }

    fn draft_with_provider_params(params: Option<ProviderParamsOverride>) -> AgentBuildDraft {
        AgentBuildDraft {
            compaction_curator: Default::default(),
            model: None,
            system_prompt: None,
            additional_instructions: Vec::new(),
            labels: Default::default(),
            app_context: None,
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
            provider_params: params,
        }
    }

    fn spawn_params(spawn: &SpawnMemberSpec) -> ProviderParamsOverride {
        spawn
            .override_profile
            .as_ref()
            .expect("provider params require a profile snapshot")
            .provider_params
            .clone()
            .expect("profile carries provider params")
    }

    fn spawn_openai_tag(spawn: &SpawnMemberSpec) -> OpenAiProviderTag {
        match spawn_params(spawn).provider_tag {
            Some(ProviderTag::OpenAi(tag)) => tag,
            other => panic!("expected an OpenAI provider tag, got {other:?}"),
        }
    }

    fn implicit_30m() -> OpenAiPromptCacheOptions {
        OpenAiPromptCacheOptions {
            mode: Some(OpenAiPromptCacheMode::Implicit),
            ttl: Some(OpenAiPromptCacheTtl::ThirtyMinutes),
        }
    }

    /// A declared prompt-cache policy reaches the profile meerkat reads
    /// (`config.provider_params = profile.provider_params`), and masks
    /// `provider_params` for resume — unmasked, durable session metadata wins
    /// and the declaration is inert on every identity that already has a
    /// session.
    #[test]
    fn build_spawn_spec_lands_declared_prompt_cache_options() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = draft_with_provider_params(Some(ProviderParamsOverride {
            provider_tag: Some(ProviderTag::OpenAi(OpenAiProviderTag {
                prompt_cache_options: Some(implicit_30m()),
                ..Default::default()
            })),
            ..Default::default()
        }));

        let spawn = build_spawn_spec(
            &runtime_id,
            &durable_spec(),
            &draft,
            Some(&profile_with_provider("openai")),
        )
        .expect("spawn spec");

        assert_eq!(
            spawn_openai_tag(&spawn).prompt_cache_options,
            Some(implicit_30m())
        );
        assert!(
            spawn
                .override_profile
                .as_ref()
                .expect("profile snapshot")
                .resume_overrides
                .contains(&ResumeOverrideField::ProviderParams),
            "an explicit declaration must win over durable metadata on resume"
        );
    }

    /// A draft that sets one knob must not wipe the knobs the mob definition
    /// declared on the same profile.
    #[test]
    fn build_spawn_spec_merges_draft_params_over_profile_declaration() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let mut base_profile = profile_with_provider("openai");
        base_profile.provider_params = Some(ProviderParamsOverride {
            thinking_budget_tokens: Some(8192),
            provider_tag: Some(ProviderTag::OpenAi(OpenAiProviderTag {
                reasoning_effort: Some(ReasoningEffort::High),
                ..Default::default()
            })),
            ..Default::default()
        });
        let draft = draft_with_provider_params(Some(ProviderParamsOverride {
            provider_tag: Some(ProviderTag::OpenAi(OpenAiProviderTag {
                prompt_cache_options: Some(implicit_30m()),
                ..Default::default()
            })),
            ..Default::default()
        }));

        let spawn = build_spawn_spec(&runtime_id, &durable_spec(), &draft, Some(&base_profile))
            .expect("spawn spec");

        assert_eq!(
            spawn_params(&spawn).thinking_budget_tokens,
            Some(8192),
            "profile-declared knobs survive a draft that sets an unrelated knob"
        );
        let tag = spawn_openai_tag(&spawn);
        assert_eq!(tag.reasoning_effort, Some(ReasoningEffort::High));
        assert_eq!(tag.prompt_cache_options, Some(implicit_30m()));
    }

    /// Meerkat sends no `prompt_cache_key`; the identity supplies the default
    /// routing bucket.
    #[test]
    fn build_spawn_spec_defaults_prompt_cache_key_from_identity() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = draft_with_provider_params(Some(ProviderParamsOverride {
            provider_tag: Some(ProviderTag::OpenAi(OpenAiProviderTag {
                prompt_cache_options: Some(implicit_30m()),
                ..Default::default()
            })),
            ..Default::default()
        }));

        let spawn = build_spawn_spec(
            &runtime_id,
            &durable_spec(),
            &draft,
            Some(&profile_with_provider("openai")),
        )
        .expect("spawn spec");

        assert_eq!(
            spawn_openai_tag(&spawn).prompt_cache_key.as_deref(),
            Some("mobkit:agent:alpha")
        );
    }

    /// The key is a routing hint the caller owns: an explicit one is never
    /// replaced.
    #[test]
    fn build_spawn_spec_keeps_caller_supplied_prompt_cache_key() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = draft_with_provider_params(Some(ProviderParamsOverride {
            provider_tag: Some(ProviderTag::OpenAi(OpenAiProviderTag {
                prompt_cache_key: Some("tenant-a:shared-prefix".to_string()),
                ..Default::default()
            })),
            ..Default::default()
        }));

        let spawn = build_spawn_spec(
            &runtime_id,
            &durable_spec(),
            &draft,
            Some(&profile_with_provider("openai")),
        )
        .expect("spawn spec");

        assert_eq!(
            spawn_openai_tag(&spawn).prompt_cache_key.as_deref(),
            Some("tenant-a:shared-prefix")
        );
    }

    /// An unstable key is worse than none: it would move the identity to a
    /// cold backend on every restart. Two independent builds for the same
    /// identity — different runtime ids, as a restart mints — must agree.
    #[test]
    fn build_spawn_spec_prompt_cache_key_is_stable_across_builds() {
        let draft = draft_with_provider_params(Some(ProviderParamsOverride {
            provider_tag: Some(ProviderTag::OpenAi(OpenAiProviderTag {
                prompt_cache_options: Some(implicit_30m()),
                ..Default::default()
            })),
            ..Default::default()
        }));
        let profile = profile_with_provider("openai");

        let first = build_spawn_spec(
            &AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id"),
            &durable_spec(),
            &draft,
            Some(&profile),
        )
        .expect("first spawn spec");
        let second = build_spawn_spec(
            &AgentRuntimeId::parse("rt:agent:alpha:7").expect("runtime id"),
            &durable_spec(),
            &draft,
            Some(&profile),
        )
        .expect("second spawn spec");

        assert_eq!(
            spawn_openai_tag(&first).prompt_cache_key,
            spawn_openai_tag(&second).prompt_cache_key
        );
        assert_eq!(
            spawn_openai_tag(&first).prompt_cache_key.as_deref(),
            Some("mobkit:agent:alpha")
        );
    }

    /// `prompt_cache_key` lives on the OpenAI tag. Fabricating one for an
    /// Anthropic identity is a typed provider-family fault at meerkat's merge
    /// seam (`ProviderTagMismatch`), which would fail the turn.
    #[test]
    fn build_spawn_spec_skips_prompt_cache_key_for_non_openai_identity() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = draft_with_provider_params(Some(ProviderParamsOverride {
            temperature: Some(0.2),
            ..Default::default()
        }));

        let spawn = build_spawn_spec(
            &runtime_id,
            &durable_spec(),
            &draft,
            Some(&profile_with_provider("anthropic")),
        )
        .expect("spawn spec");

        let params = spawn_params(&spawn);
        assert_eq!(params.temperature, Some(0.2));
        assert!(
            params.provider_tag.is_none(),
            "no OpenAI tag may be fabricated for an Anthropic-backed identity"
        );
    }

    /// Backward compatibility: a draft that declares nothing keeps the
    /// field-scoped path byte-for-byte as before. No profile snapshot is
    /// minted, so no identity is moved onto the definition-drift freeze just
    /// because this field now exists.
    #[test]
    fn build_spawn_spec_without_provider_params_keeps_field_scoped_path() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = draft_with_provider_params(None);

        let spawn = build_spawn_spec(
            &runtime_id,
            &durable_spec(),
            &draft,
            Some(&profile_with_provider("openai")),
        )
        .expect("spawn spec");

        assert!(
            spawn.override_profile.is_none(),
            "an undeclared draft must not mint a profile snapshot"
        );
        assert!(spawn.model_override.is_none());
    }

    /// Declared params with no inline definition profile to carry them (a
    /// realm-ref binding) fail closed. Dropping them silently is the failure
    /// mode that reads downstream as "caching just doesn't work".
    #[test]
    fn build_spawn_spec_rejects_declared_provider_params_without_inline_profile() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = draft_with_provider_params(Some(ProviderParamsOverride {
            temperature: Some(0.2),
            ..Default::default()
        }));

        let error = build_spawn_spec(&runtime_id, &durable_spec(), &draft, None)
            .expect_err("no inline profile can carry provider params");

        match error {
            BridgeError::InvalidInput(detail) => {
                assert!(detail.contains("provider_params"), "{detail}");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    /// A provider-family conflict between the draft and the profile is a
    /// typed fault, never a silent union of unrelated provider bags.
    #[test]
    fn build_spawn_spec_rejects_provider_tag_family_conflict() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let mut base_profile = profile_with_provider("openai");
        base_profile.provider_params = Some(ProviderParamsOverride {
            provider_tag: Some(ProviderTag::Anthropic(Default::default())),
            ..Default::default()
        });
        let draft = draft_with_provider_params(Some(ProviderParamsOverride {
            provider_tag: Some(ProviderTag::OpenAi(OpenAiProviderTag {
                prompt_cache_options: Some(implicit_30m()),
                ..Default::default()
            })),
            ..Default::default()
        }));

        let error = build_spawn_spec(&runtime_id, &durable_spec(), &draft, Some(&base_profile))
            .expect_err("provider families must not be unioned");

        match error {
            BridgeError::InvalidInput(detail) => {
                assert!(detail.contains("provider params"), "{detail}");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    /// Realm-ref bindings (no inline base profile) now take the field-scoped
    /// seam instead of skipping the model override with a warning.
    #[test]
    fn build_spawn_spec_model_override_works_without_base_profile() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = AgentBuildDraft {
            compaction_curator: Default::default(),
            model: Some("gpt-test".to_string()),
            system_prompt: None,
            additional_instructions: Vec::new(),
            labels: Default::default(),
            app_context: None,
            external_tools: Vec::new(),
            local_external_tools: LocalExternalToolOverlay::new(Arc::new(EmptyDispatcher)),
            provider_params: None,
        };

        let spawn =
            build_spawn_spec(&runtime_id, &durable_spec(), &draft, None).expect("spawn spec");

        assert_eq!(spawn.model_override.as_deref(), Some("gpt-test"));
        assert!(spawn.override_profile.is_none());
    }

    #[test]
    fn fresh_fallback_collision_classifier_matches_member_already_exists() {
        let error = meerkat_mob::MobError::MemberAlreadyExists(meerkat_mob::AgentIdentity::from(
            "rt-agent-alpha-0",
        ));

        assert!(
            is_member_already_exists_error(&error),
            "fresh fallback must retry recreate-over-running-member collisions"
        );
    }

    #[test]
    fn build_spawn_spec_maps_remote_runtime_binding() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let mut spec = durable_spec();
        spec.backend = Some(meerkat_mob::MobBackendKind::External);
        spec.binding = Some(
            serde_json::from_value(serde_json::json!({
                "kind": "external",
                "address": "tcp://127.0.0.1:4777",
                "identity": {
                    "kind": "ed25519_public_key",
                    "public_key": "ed25519:BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc="
                }
            }))
            .expect("wire binding"),
        );
        let draft = AgentBuildDraft {
            compaction_curator: Default::default(),
            model: None,
            system_prompt: None,
            additional_instructions: Vec::new(),
            labels: Default::default(),
            app_context: None,
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
            provider_params: None,
        };

        let spawn = build_spawn_spec(&runtime_id, &spec, &draft, None).expect("spawn spec");

        // meerkat 0.7: MemberCommsName is fail-closed (no `:` in member-id
        // components), so the roster id is the comms-safe encoding of the
        // public identity `agent:alpha` (see crate::member_comms_id).
        assert_eq!(
            spawn.identity.as_str(),
            crate::member_comms_id::mob_member_id_str("agent:alpha").as_ref()
        );
        assert_eq!(spawn.backend, Some(meerkat_mob::MobBackendKind::External));
        assert!(
            matches!(
                spawn.binding,
                Some(meerkat_mob::RuntimeBinding::External { .. })
            ),
            "expected external runtime binding, got {:?}",
            spawn.binding
        );
        if let Some(meerkat_mob::RuntimeBinding::External {
            address, pubkey, ..
        }) = spawn.binding
        {
            assert_eq!(address.as_str(), "tcp://127.0.0.1:4777");
            assert_eq!(pubkey, [7; 32]);
        }
    }

    #[test]
    fn external_binding_spawn_specs_require_generated_owner_context() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = AgentBuildDraft {
            compaction_curator: Default::default(),
            model: None,
            system_prompt: None,
            additional_instructions: Vec::new(),
            labels: Default::default(),
            app_context: None,
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
            provider_params: None,
        };

        // Session-backed members keep the plain spawn path.
        let session_spawn =
            build_spawn_spec(&runtime_id, &durable_spec(), &draft, None).expect("spawn spec");
        assert!(
            !spawn_spec_requires_generated_owner_context(&session_spawn),
            "session-backed spawns must not require a generated owner context"
        );

        // meerkat 0.7.1 MultiBackendProvisioner::provision_member fails
        // external (peer-only) members closed without a generated owner
        // binding; the bridge must route them through
        // spawn_spec_with_generated_owner_context.
        let mut spec = durable_spec();
        spec.backend = Some(meerkat_mob::MobBackendKind::External);
        spec.binding = Some(
            serde_json::from_value(serde_json::json!({
                "kind": "external",
                "address": "tcp://127.0.0.1:4777",
                "identity": {
                    "kind": "ed25519_public_key",
                    "public_key": "ed25519:BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc="
                }
            }))
            .expect("wire binding"),
        );
        let external_spawn =
            build_spawn_spec(&runtime_id, &spec, &draft, None).expect("spawn spec");
        assert!(
            spawn_spec_requires_generated_owner_context(&external_spawn),
            "external peer-only spawns must carry a generated owner binding on meerkat 0.7.1"
        );
    }

    #[test]
    fn bridge_delivery_repair_covers_missing_bridge_session_snapshot() {
        let error = "session bridge mob error: member rt:review:singleton:0 failed to restore session 019e5fc2-dad4-77e2-abbe-a8a66bc15f66: missing bridge session snapshot for '019e5fc2-dad4-77e2-abbe-a8a66bc15f66'";

        assert!(
            is_repairable_bridge_delivery_error(error),
            "stale bridge-session bindings should be repaired before retrying delivery"
        );
        assert!(
            is_repairable_bridge_delivery_error("missing event injector capability for member"),
            "existing stale event-injector repair path must remain covered"
        );
        assert!(
            is_repairable_bridge_delivery_error(
                "mob member rt:us-president:0 missing required capability interaction_event_injector: autonomous member dispatch"
            ),
            "newer autonomous member dispatch wording should repair and retry instead of dropping the event"
        );
        assert!(
            is_repairable_bridge_delivery_error(
                "previous member cleanup ambiguous for member rt:deep-investigator:singleton:0"
            ),
            "ambiguous Meerkat respawn cleanup should trigger bridge repair instead of failing delivery"
        );
        assert!(
            !is_repairable_bridge_delivery_error("model provider returned rate limit"),
            "ordinary turn failures must not trigger member repair"
        );
    }

    #[test]
    fn bridge_delivery_repair_classifies_topology_restore_failure_as_degraded() {
        let identity = meerkat_mob::AgentIdentity::from("rt:review:singleton:0");
        let receipt = meerkat_mob::MemberRespawnReceipt::new(
            identity.clone(),
            meerkat_mob::AgentRuntimeId::new(identity, meerkat_mob::ids::Generation::INITIAL),
            meerkat_mob::FenceToken::new(1),
            meerkat_mob::FenceToken::new(2),
        );
        let err = MobRespawnError::TopologyRestoreFailed {
            receipt,
            failed_peer_ids: vec![meerkat_mob::RespawnTopologyPeerId::from(
                "initiative:broken",
            )],
        };

        assert_eq!(
            classify_member_repair_respawn_failure(&err),
            MemberRepairRespawnFailure::DegradedTopologyRestore {
                failed_peer_ids: vec!["initiative:broken".to_string()]
            },
            "failed peer edges should degrade bridge repair instead of bricking delivery"
        );
        assert!(
            matches!(
                classify_member_repair_respawn_failure(&MobRespawnError::NoRuntimeControl {
                    identity: meerkat_mob::AgentIdentity::from("rt:review:singleton:0"),
                }),
                MemberRepairRespawnFailure::Fatal(_)
            ),
            "ordinary respawn failures must still fail bridge repair"
        );
    }

    /// RESUME AUTHORS NOTHING (meerkat 0.8.11 prompt contract): even when
    /// the draft carries an explicit customizer prompt, the resume spawn
    /// spec must NOT re-send it. `Message::System` is ordinary ordered
    /// authored transcript content; re-sending assembled configuration at
    /// every activation would append a byte-duplicate System per boot (the
    /// prompt-refresh depth leak, reborn). The +0-on-neutral-resume
    /// invariant is structural — no append command exists on this path —
    /// and mid-thread instruction changes are a caller EXPLICITLY authoring
    /// a System message via `StartTurnRequest.system_messages`, exactly
    /// once, as part of a turn.
    #[test]
    fn resume_spawn_spec_authors_nothing() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = AgentBuildDraft {
            compaction_curator: Default::default(),
            model: None,
            system_prompt: Some("explicit customizer prompt".to_string()),
            additional_instructions: Vec::new(),
            labels: std::collections::BTreeMap::new(),
            app_context: None,
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
            provider_params: None,
        };
        let session_id = meerkat_core::types::SessionId::new();

        let spawn =
            build_resume_spawn_spec(&runtime_id, &durable_spec(), &draft, None, &session_id)
                .expect("resume spawn spec");

        assert_eq!(
            spawn.system_prompt_override, None,
            "resume must not author or re-send prompt configuration; explicit System \
             authoring via a turn is the only mid-thread instruction change"
        );
        match &spawn.launch_mode {
            MemberLaunchMode::Resume { bridge_session_id } => {
                assert_eq!(bridge_session_id, &session_id);
            }
            other => panic!("expected Resume launch mode, got {other:?}"),
        }
    }

    /// The delivery-repair fallback ladder hinges on telling "the durable
    /// snapshot is gone" (fresh respawn is legitimate recovery) apart from
    /// every other resume failure (fresh respawn would abandon a live
    /// transcript — the OB3 `identity_alias_respawn_rotation` class). The
    /// classification is a TYPED variant match: only
    /// `SessionUnavailableForResume { reason: Absent }` authorizes the
    /// fallback, and no error WORDING can impersonate it.
    #[test]
    fn repair_fallback_only_on_missing_durable_snapshot() {
        let session_id = meerkat_core::types::SessionId::new();
        assert!(durable_snapshot_is_typed_absent(
            &meerkat_mob::MobError::SessionUnavailableForResume {
                session_id: session_id.clone(),
                reason: meerkat_mob::error::SessionResumeUnavailableReason::Absent,
                runtime_state: None,
                verdict: None,
            }
        ));
        // Archived-but-intact carries a transcript: never a fresh respawn.
        assert!(!durable_snapshot_is_typed_absent(
            &meerkat_mob::MobError::SessionUnavailableForResume {
                session_id,
                reason: meerkat_mob::error::SessionResumeUnavailableReason::ArchivedNotRevivable,
                runtime_state: Some("archived".to_string()),
                verdict: None,
            }
        ));
        // The wording attack the substring classifier was vulnerable to: an
        // arbitrary error whose MESSAGE contains the magic prose must not
        // authorize a fresh respawn.
        let impersonator = meerkat_mob::MobError::Internal(
            "missing durable session snapshot for '019e5fc2-dad4-77e2-abbe-a8a66bc15f66'"
                .to_string(),
        );
        assert!(
            impersonator
                .to_string()
                .contains("missing durable session snapshot")
        );
        assert!(!durable_snapshot_is_typed_absent(&impersonator));
        // And should the impersonator surface through the legacy respawn
        // ladder instead, it must classify Fatal — no wording reaches a
        // recovery arm there either.
        assert!(
            matches!(
                classify_member_repair_respawn_failure(&MobRespawnError::Mob(impersonator)),
                MemberRepairRespawnFailure::Fatal(_)
            ),
            "an Internal impersonator must stay Fatal in the respawn ladder"
        );
    }

    /// Regression for the wedged typed-absent recovery: `resume_repair_member`
    /// retires the member to VERIFIED roster absence before its resume-spawn
    /// attempt, and `MobHandle::respawn` reads the roster entry — so after a
    /// typed-absent durable loss, routing the fallback through `respawn`
    /// deterministically fails `MemberNotFound`, which classifies Fatal (no
    /// recovery arm handles it) and wedges the identity permanently. The
    /// `DurableSnapshotMissing` arm must therefore spawn fresh DIRECTLY from
    /// the pre-delivery entry: same identity, pre-delivery role + labels,
    /// `Fresh` launch mode.
    #[test]
    fn typed_absent_recovery_spawns_fresh_directly_because_respawn_is_fatal_after_absence() {
        let member_id = MobAgentIdentity::from("rt-agent-alpha-0");

        // The ladder respawn cannot complete for a verified-absent member.
        let respawn_after_verified_absence =
            MobRespawnError::Mob(meerkat_mob::MobError::MemberNotFound(member_id.clone()));
        assert!(
            matches!(
                classify_member_repair_respawn_failure(&respawn_after_verified_absence),
                MemberRepairRespawnFailure::Fatal(_)
            ),
            "MemberNotFound stays Fatal in the respawn ladder — the typed-absent arm \
             must never fall through to handle.respawn"
        );

        // The direct fresh spawn the arm routes to instead.
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("agent_identity".to_string(), "agent:alpha".to_string());
        let spec = fresh_member_spec_from_pre_delivery_entry(
            &member_id,
            meerkat_mob::ProfileName::from("worker"),
            labels.clone(),
        );
        assert_eq!(
            spec.identity.as_str(),
            member_id.as_str(),
            "typed-absent recovery must rebuild under the SAME identity"
        );
        assert_eq!(spec.role_name.as_str(), "worker");
        assert_eq!(spec.labels, Some(labels));
        assert!(
            matches!(spec.launch_mode, MemberLaunchMode::Fresh),
            "the durable snapshot is typed-absent: the rebuild takes a fresh session, \
             never a Resume rebind onto the gone session"
        );

        // No pre-delivery labels → the spec carries none (matches both
        // sanctioned fresh-spawn arms' conditional).
        let bare = fresh_member_spec_from_pre_delivery_entry(
            &member_id,
            meerkat_mob::ProfileName::from("worker"),
            std::collections::BTreeMap::new(),
        );
        assert_eq!(bare.labels, None);
    }

    #[tokio::test]
    async fn reset_snapshot_is_abandoned_before_meerkat_retire() {
        let member_id = MobAgentIdentity::from("rt-agent-alpha-0");
        let session_id = meerkat_core::types::SessionId::new();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        abandon_then_retire_reset_superseded(
            &member_id,
            &session_id,
            {
                let order = Arc::clone(&order);
                move || async move {
                    order.lock().expect("order lock").push("abandon");
                    Ok(())
                }
            },
            {
                let order = Arc::clone(&order);
                move || async move {
                    assert_eq!(
                        order.lock().expect("order lock").as_slice(),
                        ["abandon"],
                        "Meerkat retirement must not start until the exact superseded projection is tombstoned"
                    );
                    order.lock().expect("order lock").push("retire");
                    Ok(())
                }
            },
        )
        .await
        .expect("pre-abandon then retire");

        assert_eq!(
            order.lock().expect("order lock").as_slice(),
            ["abandon", "retire"]
        );
    }

    #[tokio::test]
    async fn reset_snapshot_cas_failure_stays_visible_and_blocks_retire() {
        let member_id = MobAgentIdentity::from("rt-agent-alpha-0");
        let session_id = meerkat_core::types::SessionId::new();
        let retire_called = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let error = abandon_then_retire_reset_superseded(
            &member_id,
            &session_id,
            || async {
                Err(meerkat_store::SessionStoreError::Internal(
                    "exact snapshot CAS mismatch".to_string(),
                ))
            },
            {
                let retire_called = Arc::clone(&retire_called);
                move || async move {
                    retire_called.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                }
            },
        )
        .await
        .expect_err("CAS failure must retain reset cleanup debt");

        assert!(
            error.to_string().contains("exact snapshot CAS mismatch"),
            "the exact abandon failure must remain visible to the cleanup-debt owner: {error}"
        );
        assert!(
            !retire_called.load(std::sync::atomic::Ordering::SeqCst),
            "retirement must not acknowledge cleanup after the exact-CAS abandon failed"
        );
    }

    #[test]
    fn resume_error_classification_is_typed_first() {
        let restore_failed = meerkat_mob::MobError::MemberRestoreFailed {
            member_id: meerkat_mob::ids::AgentIdentity::from("agent-alpha"),
            session_id: None,
            reason: "durable snapshot missing".to_string(),
        };
        assert_eq!(
            classify_resume_error(&restore_failed),
            ResumeRejectionKind::MemberRestoreFailed
        );

        let continuity = meerkat_mob::MobError::Internal(
            "session save rejected: incoming transcript is not a continuation of persisted \
             revision sha256:d57e07"
                .to_string(),
        );
        assert_eq!(
            classify_resume_error(&continuity),
            ResumeRejectionKind::TranscriptContinuity
        );

        // The typed archived refusal classifies from the VARIANT, never the
        // wording: the stable wall consumers park on (OB3 rehearsal).
        let archived = meerkat_mob::MobError::SessionUnavailableForResume {
            session_id: meerkat_core::types::SessionId::new(),
            reason: meerkat_mob::error::SessionResumeUnavailableReason::ArchivedNotRevivable,
            runtime_state: None,
            verdict: None,
        };
        assert_eq!(
            classify_resume_error(&archived),
            ResumeRejectionKind::ArchivedNotRevivable
        );
        // Typed Absent stays out of the archived class (it authorizes the
        // never-persisted fresh fallback, a different door entirely).
        let absent = meerkat_mob::MobError::SessionUnavailableForResume {
            session_id: meerkat_core::types::SessionId::new(),
            reason: meerkat_mob::error::SessionResumeUnavailableReason::Absent,
            runtime_state: None,
            verdict: None,
        };
        assert_eq!(classify_resume_error(&absent), ResumeRejectionKind::Other);

        let other = meerkat_mob::MobError::WiringError("unrelated".to_string());
        assert_eq!(classify_resume_error(&other), ResumeRejectionKind::Other);
    }

    // -----------------------------------------------------------------------
    // Bounded actor admission
    // -----------------------------------------------------------------------

    #[test]
    fn admission_budget_defaults_when_unset_or_unparseable() {
        for raw in [None, Some(""), Some("   "), Some("soon"), Some("-5")] {
            assert_eq!(
                parse_bridge_actor_admission_budget(raw),
                BRIDGE_ACTOR_ADMISSION_BUDGET,
                "unset or unparseable {raw:?} must fall back to the default budget"
            );
        }
    }

    #[test]
    fn admission_budget_honours_configured_value_within_the_clamp() {
        assert_eq!(
            parse_bridge_actor_admission_budget(Some("30")),
            Duration::from_secs(30)
        );
        assert_eq!(
            parse_bridge_actor_admission_budget(Some(" 30 ")),
            Duration::from_secs(30)
        );
        // The floor stops `0` from rejecting every delivery; the ceiling stops
        // a mistyped value from restoring the unbounded hang.
        assert_eq!(
            parse_bridge_actor_admission_budget(Some("0")),
            Duration::from_secs(1)
        );
        assert_eq!(
            parse_bridge_actor_admission_budget(Some("999999")),
            Duration::from_hours(1)
        );
    }

    #[tokio::test]
    async fn responsive_round_trip_passes_through_the_bound_unchanged() {
        let deadline = ActorAdmissionDeadline::new(Duration::from_mins(10));
        let member = MobAgentIdentity::from("rt-agent-alpha-0");

        let value = deadline
            .bound("test.responsive", &member, async { 7_u32 })
            .await
            .expect("a ready round trip must pass through the bound untouched");

        assert_eq!(value, 7);
        // No added latency on the success path: the budget is still intact.
        assert!(
            deadline
                .deadline
                .saturating_duration_since(tokio::time::Instant::now())
                > Duration::from_secs(599)
        );
    }

    #[tokio::test]
    async fn stalled_actor_fails_typed_instead_of_hanging() {
        let deadline = ActorAdmissionDeadline::new(Duration::from_millis(20));
        let member = MobAgentIdentity::from("rt-agent-alpha-0");

        let error = deadline
            .bound(
                "deliver.submit_work",
                &member,
                std::future::pending::<Result<(), meerkat_mob::MobError>>(),
            )
            .await
            .expect_err("a mob actor that never replies must not hang the delivery");

        match error {
            BridgeError::ActorAdmissionTimeout {
                operation,
                identity,
                waited,
            } => {
                assert_eq!(operation, "deliver.submit_work");
                assert_eq!(identity.as_str(), "rt-agent-alpha-0");
                assert!(waited >= Duration::from_millis(20), "waited {waited:?}");
            }
            other => panic!("expected a typed admission timeout, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn admission_timeout_names_the_operation_and_member_and_is_never_repairable() {
        let deadline = ActorAdmissionDeadline::new(Duration::from_millis(5));
        let member = MobAgentIdentity::from("rt-agent-alpha-0");

        let error = deadline
            .bound(
                "deliver.get_member",
                &member,
                std::future::pending::<Result<(), meerkat_mob::MobError>>(),
            )
            .await
            .expect_err("stalled actor");

        let rendered = error.to_string();
        assert!(rendered.contains("deliver.get_member"), "{rendered}");
        assert!(rendered.contains("rt-agent-alpha-0"), "{rendered}");
        // The delivery path classifies repairable errors by substring; a
        // blocked actor must never be routed into member repair.
        assert!(
            !is_repairable_bridge_delivery_error(&rendered),
            "a blocked actor is not stale runtime state: {rendered}"
        );
    }

    #[tokio::test]
    async fn serialized_round_trips_share_one_budget() {
        // The delivery path makes three or four serialized round trips; a
        // per-call timeout would cost a full budget each and make the
        // contained worst case worse than the hang. Once the shared deadline
        // is spent, every later hop must fail immediately.
        let budget = Duration::from_millis(50);
        let deadline = ActorAdmissionDeadline::new(budget);
        let member = MobAgentIdentity::from("rt-agent-alpha-0");

        let mut outcomes = Vec::new();
        // First hop consumes the whole budget.
        outcomes.push(
            deadline
                .bound(
                    "test.hop",
                    &member,
                    std::future::pending::<Result<(), meerkat_mob::MobError>>(),
                )
                .await,
        );

        let after_first = tokio::time::Instant::now();
        for _ in 0..2 {
            outcomes.push(
                deadline
                    .bound(
                        "test.hop",
                        &member,
                        std::future::pending::<Result<(), meerkat_mob::MobError>>(),
                    )
                    .await,
            );
        }
        let spent_after_first = after_first.elapsed();

        assert!(
            outcomes
                .iter()
                .all(|outcome| matches!(outcome, Err(BridgeError::ActorAdmissionTimeout { .. }))),
            "every hop against a dead actor must fail typed"
        );
        assert!(
            spent_after_first < budget,
            "the deadline must not be re-armed per hop: two further hops spent \
             {spent_after_first:?} against a {budget:?} budget"
        );
    }

    /// Heal contract: `DurableTailRecoveryRefused` is a typed refusal only an
    /// external change clears — it must PARK as the terminal `Unprovable`
    /// verdict, not escape as a retryable bridge error that loops the
    /// reconcile repair pass forever (the HomeCore 9/17-Broken shape would
    /// then never reach an operator as a parked reason).
    #[test]
    fn heal_error_tier_recovery_refused_parks_terminal_unprovable() {
        let id = meerkat_core::SessionId::new();
        let verdict = map_committed_boundary_recovery_error(
            meerkat_core::SessionError::DurableTailRecoveryRefused { id: id.clone() },
        );
        match verdict {
            Ok(CommittedBoundaryRepair::Unprovable { reason }) => {
                assert!(
                    reason.contains(&id.to_string()),
                    "park reason must carry the session id for the operator: {reason}"
                );
                assert!(
                    reason.contains("refused"),
                    "park reason must state the refusal: {reason}"
                );
            }
            other => panic!("refused must park as Unprovable, got {other:?}"),
        }
    }

    /// Forked/unverifiable durable evidence is the same class: no retry can
    /// un-fork evidence, so it parks with the reason instead of retrying.
    #[test]
    fn heal_error_tier_quarantined_evidence_parks_terminal_unprovable() {
        let verdict = map_committed_boundary_recovery_error(
            meerkat_core::SessionError::DurableEvidenceQuarantined {
                id: meerkat_core::SessionId::new(),
            },
        );
        assert!(
            matches!(verdict, Ok(CommittedBoundaryRepair::Unprovable { .. })),
            "quarantined evidence must park as Unprovable, got {verdict:?}"
        );
    }

    /// The genuinely retryable tier stays in `Err`: a live session owning the
    /// head mid-turn (`Busy`) and a held tail awaiting the recovery commit
    /// itself (`DurableTailHeldForRecovery`) both clear on a later pass.
    #[test]
    fn heal_error_tier_busy_and_held_stay_retryable_errors() {
        for error in [
            meerkat_core::SessionError::Busy {
                id: meerkat_core::SessionId::new(),
            },
            meerkat_core::SessionError::DurableTailHeldForRecovery {
                id: meerkat_core::SessionId::new(),
            },
        ] {
            let verdict = map_committed_boundary_recovery_error(error);
            assert!(
                matches!(verdict, Err(BridgeError::Mob(_))),
                "retryable-tier errors must stay bridge errors, got {verdict:?}"
            );
        }
    }
}
