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
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(msg) => write!(f, "session bridge mob error: {msg}"),
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
    /// Any other resume-time failure.
    Other,
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

async fn submit_internal_bridge_work(
    handle: &MobHandle,
    member_id: &MobAgentIdentity,
    content: &meerkat_core::ContentInput,
    injected_context: &[meerkat_core::ContentInput],
    handling_mode: HandlingMode,
    interaction_id: Option<&str>,
    deadline: &ActorAdmissionDeadline,
) -> Result<(), BridgeError> {
    let entry = deadline
        .bound(
            "deliver.get_member",
            member_id,
            handle.get_member(member_id),
        )
        .await?
        .map_err(|err| BridgeError::Mob(err.to_string()))?
        .ok_or_else(|| BridgeError::Mob(format!("member not found: {member_id}")))?;
    // Ask 1: attach ambient memory recall as a separate typed injected-context
    // body rather than fusing it into the user's message text. WorkSpec carries
    // it to the StartTurnRequest, where meerkat stamps each entry as the
    // InjectedContext transcript role (excluded from compaction indexing).
    let mut spec = WorkSpec::new(content.clone(), WorkOrigin::Internal);
    if !injected_context.is_empty() {
        spec = spec.with_injected_context(injected_context.to_vec());
    }
    // meerkat 0.7.25 ask 15 addendum: a host-supplied interaction id rides
    // WorkSpec into runtime admission, so this turn's live events AND its
    // committed transcript messages carry the SAME id the console minted at
    // send time — the exact live↔history join the console dedup needs. Only
    // UUID-form ids exist here (the identity-first console send mints v5
    // UUIDs); anything else is skipped rather than corrupted.
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
    deadline
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
        .map(|_| ())
        .map_err(|err| BridgeError::Mob(err.to_string()))
}

// ---------------------------------------------------------------------------
// SessionBridge trait
// ---------------------------------------------------------------------------

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

    /// Deliver content to an active mob member.
    async fn deliver(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
    ) -> Result<meerkat_core::types::SessionId, BridgeError>;

    /// Deliver content to an active mob member using a caller-selected turn
    /// handling mode. Bridge implementations that do not distinguish modes can
    /// fall back to ordinary delivery.
    async fn deliver_with_mode(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let _ = handling_mode;
        self.deliver(runtime_id, content).await
    }

    /// Deliver content plus a separate `injected_context` body (meerkat
    /// 0.7.12 ask 1: typed ambient injection alongside — not fused into —
    /// the user's message) and an optional host-minted interaction id
    /// (meerkat 0.7.25 ask 15 addendum: threaded into runtime admission so
    /// transcript messages join the caller's live interaction frames).
    /// Bridges that do not carry injected context fall back to plain
    /// delivery of the user content, dropping the injection and the id.
    async fn deliver_with_mode_and_context(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        injected_context: &[meerkat_core::ContentInput],
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let _ = injected_context;
        let _ = interaction_id;
        self.deliver_with_mode(runtime_id, content, handling_mode)
            .await
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
}

/// Lightweight inspection of a mob member's current execution state.
#[derive(Debug, Clone)]
pub struct MemberInspection {
    pub output_preview: Option<String>,
    pub is_final: bool,
    pub peer_reachable_count: usize,
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
    /// Budget one delivery attempt may spend waiting on the mob actor.
    /// Resolved once at construction so the success path pays nothing per
    /// call. See [`BRIDGE_ACTOR_ADMISSION_BUDGET`].
    actor_admission_budget: Duration,
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
            actor_admission_budget: bridge_actor_admission_budget(),
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
            actor_admission_budget: bridge_actor_admission_budget(),
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
            actor_admission_budget: bridge_actor_admission_budget(),
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
            actor_admission_budget: bridge_actor_admission_budget(),
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
            actor_admission_budget: bridge_actor_admission_budget(),
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
                Ok(()) => return Ok(()),
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
                    return Ok(());
                }
                Err(RepairResumeFailure::Rejected(err)) => return Err(err),
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
            // whole-profile snapshot (provider cleared for catalog
            // re-inference) for pinned profiles only — accepting the
            // definition-drift freeze `model_override` was built to end.
            Some(base) if base.provider.is_some() || base.self_hosted_server_id.is_some() => {
                let mut profile = base.clone();
                profile.model = model.clone();
                profile.provider = None;
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
/// - `system_prompt_override` is cleared (Inherit): the persisted System
///   message is authoritative on resume. Re-sending the draft's explicit
///   prompt makes meerkat re-assemble the prompt, and on meerkat ≤0.7.14 that
///   trips the session store's transcript-continuity guard whenever the
///   persisted prompt carries runtime context appends — the exact cold-restart
///   transcript-loss class (HomeCore). Dynamic per-boot context belongs in
///   runtime system-context appends, never the base prompt.
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
            Err(error) => {
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

    async fn deliver(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
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
                return Err(BridgeError::InvalidInput(
                    "target member model cannot accept image input".to_string(),
                ));
            }
        }

        // Submit internal work directly through the mob work lane so delivery
        // acks at runtime ingress rather than waiting for the full turn to
        // complete. The identity layer owns addressability enforcement — the
        // bridge is an internal delivery mechanism regardless of whether the
        // identity is Addressable or InternalOnly.
        match submit_internal_bridge_work(
            &self.handle,
            &mid,
            content,
            &[],
            HandlingMode::Queue,
            None,
            &deadline,
        )
        .await
        {
            Ok(()) => {}
            // A blocked actor is not stale runtime state: keep the typed
            // timeout instead of routing it into member repair (which cannot
            // unblock an actor) or laundering it into `Mob(String)` below.
            Err(err @ BridgeError::ActorAdmissionTimeout { .. }) => return Err(err),
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
                submit_internal_bridge_work(
                    &self.handle,
                    &mid,
                    content,
                    &[],
                    HandlingMode::Queue,
                    None,
                    &deadline,
                )
                .await?;
            }
            Err(err) => return Err(BridgeError::Mob(err.to_string())),
        }

        // Meerkat 0.6: MemberDeliveryReceipt no longer carries session_id.
        // Query the bridge session id directly from the mob handle.
        self.resolve_runtime_session_id(
            runtime_id,
            &mid,
            "member has no bridge session after deliver",
            &deadline,
        )
        .await
    }

    async fn deliver_with_mode(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        self.deliver_with_mode_and_context(runtime_id, content, &[], handling_mode, None)
            .await
    }

    async fn deliver_with_mode_and_context(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        injected_context: &[meerkat_core::ContentInput],
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
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
                return Err(BridgeError::InvalidInput(
                    "target member model cannot accept image input".to_string(),
                ));
            }
        }

        match submit_internal_bridge_work(
            &self.handle,
            &mid,
            content,
            injected_context,
            handling_mode,
            interaction_id,
            &deadline,
        )
        .await
        {
            Ok(()) => {}
            // A blocked actor is not stale runtime state: keep the typed
            // timeout instead of routing it into member repair (which cannot
            // unblock an actor) or laundering it into `Mob(String)` below.
            Err(err @ BridgeError::ActorAdmissionTimeout { .. }) => return Err(err),
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
                submit_internal_bridge_work(
                    &self.handle,
                    &mid,
                    content,
                    injected_context,
                    handling_mode,
                    interaction_id,
                    &deadline,
                )
                .await?;
            }
            Err(err) => return Err(BridgeError::Mob(err.to_string())),
        }

        self.resolve_runtime_session_id(
            runtime_id,
            &mid,
            "member has no bridge session after deliver",
            &deadline,
        )
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use std::sync::Arc;

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
            "stale provider pin must be cleared for catalog re-inference"
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

    /// The persisted System message is authoritative on resume: even when the
    /// draft carries an explicit customizer prompt, the resume spawn spec must
    /// NOT re-send it (meerkat ≤0.7.14 re-assembles the prompt and trips the
    /// transcript-continuity guard — the HomeCore cold-restart loss class).
    #[test]
    fn resume_spawn_spec_inherits_persisted_system_prompt() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = AgentBuildDraft {
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
            "resume must inherit the persisted System message, never re-send the base prompt"
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
            }
        ));
        // Archived-but-intact carries a transcript: never a fresh respawn.
        assert!(!durable_snapshot_is_typed_absent(
            &meerkat_mob::MobError::SessionUnavailableForResume {
                session_id,
                reason: meerkat_mob::error::SessionResumeUnavailableReason::ArchivedNotRevivable,
                runtime_state: Some("archived".to_string()),
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
}
