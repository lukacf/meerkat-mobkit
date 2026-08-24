//! Identity-first runtime: delivery, status, lifecycle, and ownership enforcement.
//!
//! This module implements the behavioral core of identity-first continuity:
//! - Delivery: `send()` and `dispatch()` with addressability and lease enforcement
//! - Status: `status()` returning `IdentityStatus`
//! - Lifecycle: `retire()`, `respawn()`, `reset()`, `delete_identity()`
//! - Ownership: lease tracking, fencing, and invariant enforcement

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock, Weak};
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};
use meerkat_core::types::{HandlingMode, SessionId};
use tokio::sync::{Mutex, Notify, RwLock, broadcast, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};

use super::agent_memory::{
    AgentMemoryError, AgentMemoryForgetResult, AgentMemoryRecallRequest, AgentMemoryRecord,
    AgentMemoryRuntimeInjector, NewAgentMemory,
};
use super::bridge::{
    BridgeAdmissionError, BridgeError, BridgeTurnError, CommittedBoundaryRepair,
    ResumeRejectionKind, SessionBridge, archived_not_revivable_park_reason,
};
use super::contracts::{
    AgentCustomizer, ContinuityStore, LeaseProvider, RosterProvider, TopologyProvider,
};
use super::types::{
    AgentAddressability, AgentBuildContext, AgentBuildDraft, AgentIdentity, AgentRuntimeId,
    AgentRuntimeServices, CheckpointVersion, CompletionCursor, CompletionProgress,
    ContinuityFailure, ContinuityFailureKind, ContinuityGeneration, ContinuityHealth,
    ContinuityRecord, ContinuityStoreError, ContinuityUnrecoverable, DispatchAdmission,
    DispatchInput, DurabilityPolicy, DurableAgentSpec, FencingToken, HostRejectedBuildPark,
    IdentityBootstrapEntry, IdentityBootstrapMode, IdentityBootstrapState, IdentityBootstrapStatus,
    IdentityLifecycleState, IdentityStatus, LeaseGrant, LeaseInfo, ManagedPeerEdge, NotAddressable,
    RosterContext, SendAdmission, SessionSnapshot, TopologyContext,
};
use crate::memory::records::{
    ManifestTier, MemoryId, MemoryKind, MemoryScope, NewMemoryRecord, RecordMeta, UsageEvent,
};

const MANAGED_PEER_RECONCILE_CONCURRENCY: usize = 64;
/// Poll cadence for [`IdentityRuntime::wait_for_completion`]. The cursor is
/// advanced by an event, so this only bounds observation latency.
const COMPLETION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MATERIALIZATION_FAILURE_BACKOFF: Duration = Duration::from_secs(30);
const RAW_MEMBER_ALIAS_LOCK_SWEEP_MIN: usize = 256;
const BACKGROUND_WARM_CANCELLED: &str =
    "identity background warm cancelled before session installation";
fn durable_spec_uses_external_binding(spec: &DurableAgentSpec) -> bool {
    matches!(spec.backend, Some(meerkat_mob::MobBackendKind::External))
        || matches!(
            spec.binding.as_ref(),
            Some(meerkat_contracts::WireRuntimeBinding::External { .. })
        )
}

fn interaction_id_for_delivery<'a>(
    spec: &DurableAgentSpec,
    interaction_id: Option<&'a str>,
) -> Option<&'a str> {
    // Meerkat 0.8.2 deliberately rejects transcript interaction ids on
    // remotely hosted / peer-only member turns: that metadata carrier is not
    // representable on the wire path. MobKit still correlates the response
    // through its pending-interaction ledger and the peer terminal event.
    (!durable_spec_uses_external_binding(spec))
        .then_some(interaction_id)
        .flatten()
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// The identity embodiment a completion-bearing delivery was admitted onto.
///
/// Compared by value across the unlocked window. Any inequality means the
/// identity was reset, retired or rebound while its turn ran, and the turn's
/// result belongs to an embodiment that no longer exists.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedIncarnation {
    runtime_id: Option<AgentRuntimeId>,
    generation: Option<u64>,
    fencing_token: Option<FencingToken>,
}

/// Which bridge verb the shared send core delivers through.
///
/// A typed mode rather than a bool, and one shared body rather than two, for
/// the same reason the bridge's own submit path is shaped this way: the parts
/// that differ between the lanes are trivial, and the parts that must NOT
/// differ - lease acquisition, alias pinning, defanging, memory injection,
/// session reconciliation - are the ones a second copy would silently drift on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendCommitMode {
    /// Return once the work is admitted. The historical behaviour of `send`.
    Ingress,
    /// Return only after this exact turn has committed its terminal boundary.
    /// Fails closed when completion cannot be observed.
    AwaitCommit,
}

/// One invocation of the shared identity delivery state machine.
///
/// Keeping the lane-specific carriers in one value makes the common send body
/// explicit without growing a positional parameter list that is easy to wire
/// incorrectly when a new carrier is added.
struct SendRequest<'a> {
    expected_alias: Option<&'a str>,
    content: &'a meerkat_core::ContentInput,
    system_prompt: Option<&'a str>,
    handling_mode: HandlingMode,
    interaction_id: Option<&'a str>,
    commit_mode: SendCommitMode,
}

/// Map the admission edge of a completion-bearing delivery. Post-admission
/// outcomes are unrepresentable in this input type.
fn admission_phase_error(
    identity: &AgentIdentity,
    err: BridgeAdmissionError,
) -> IdentityRuntimeError {
    let identity = identity.clone();
    match err {
        BridgeAdmissionError::CompletionUnsupported(reason) => {
            IdentityRuntimeError::CompletionUnavailable { identity, reason }
        }
        BridgeAdmissionError::Mob(detail)
        | BridgeAdmissionError::InvalidInput(detail)
        | BridgeAdmissionError::InvariantViolation(detail) => {
            IdentityRuntimeError::AdmissionFailed { identity, detail }
        }
        other @ (BridgeAdmissionError::ResumeRejected { .. }
        | BridgeAdmissionError::ActorAdmissionTimeout { .. }) => {
            IdentityRuntimeError::AdmissionFailed {
                identity,
                detail: other.to_string(),
            }
        }
    }
}

/// Map the terminal edge of an admitted turn. Pre-admission outcomes are
/// unrepresentable in this input type.
fn turn_phase_error(identity: &AgentIdentity, err: BridgeTurnError) -> IdentityRuntimeError {
    let identity = identity.clone();
    match err {
        BridgeTurnError::CompletionFailed(detail) => {
            IdentityRuntimeError::CompletionFailed { identity, detail }
        }
        BridgeTurnError::PostAdmissionResolutionFailed(detail) => {
            IdentityRuntimeError::PostAdmissionResolutionFailed { identity, detail }
        }
    }
}

/// Errors from identity-first runtime operations.
#[derive(Debug)]
pub enum IdentityRuntimeError {
    /// Target identity is not registered/active.
    UnknownIdentity(AgentIdentity),
    /// send() rejected: target is InternalOnly.
    NotAddressable(NotAddressable),
    /// PHASE 1 of 4. A completion-bearing send could not be honoured AT ALL.
    ///
    /// Nothing was submitted: either no bridge is installed, the identity has
    /// no bound runtime id, or the bridge is ingress-only. The completion lane
    /// fails closed here rather than degrading to the admission-only path -
    /// degrading would report success for a turn that was never awaited.
    ///
    /// Safe to fall back or fail; nothing ran.
    CompletionUnavailable {
        identity: AgentIdentity,
        reason: String,
    },
    /// PHASE 2 of 4. A completion-bearing send failed AT ADMISSION.
    ///
    /// The turn never started. Distinct from [`Self::CompletionFailed`] on
    /// purpose: this names a delivery that did not land, so retry semantics are
    /// the ordinary admission ones, not "the turn already ran".
    AdmissionFailed {
        identity: AgentIdentity,
        detail: String,
    },
    /// PHASE 3 of 4. The work WAS admitted and its turn reached a FAILED
    /// terminal.
    ///
    /// The turn RAN. Never retry - a retry would run the identity's turn a
    /// second time.
    CompletionFailed {
        identity: AgentIdentity,
        detail: String,
    },
    /// PHASE 4 of 4. The turn was admitted, reached a SUCCESSFUL terminal, and
    /// only then did projecting its session id fail.
    ///
    /// The member did the work. Non-retryable: repairing cannot undo a turn
    /// that already ran, and resubmitting would run it twice.
    PostAdmissionResolutionFailed {
        identity: AgentIdentity,
        detail: String,
    },
    /// The turn was admitted and ran, and the identity was RESET, RETIRED or
    /// REBOUND while it ran.
    ///
    /// Distinct from [`Self::PostAdmissionResolutionFailed`]: there the
    /// embodiment is intact and only the projection failed; here the embodiment
    /// the turn belonged to no longer exists. Non-retryable, and deliberately
    /// NOT reconciled - binding the live incarnation to a dead turn's session
    /// is worse than losing the attribution.
    PostAdmissionSuperseded {
        identity: AgentIdentity,
        detail: String,
    },
    /// Operation rejected: no active lease for this identity.
    NoActiveLease(AgentIdentity),
    /// Fail-closed single-embodiment guard: the identity's durable lease is
    /// held by another live runtime instance — a second live embodiment is
    /// refused, loudly and with the holder named. This is a POLICY point,
    /// not a structural assumption: multi-bind / forked-session semantics
    /// (future work) relax exactly this arm.
    AlreadyEmbodied {
        identity: AgentIdentity,
        holder: String,
    },
    /// Operation rejected: lease was lost.
    LeaseLost(AgentIdentity),
    /// Operation rejected: identity is not in a state that permits this operation.
    InvalidState {
        identity: AgentIdentity,
        state: IdentityLifecycleState,
        operation: &'static str,
    },
    /// Dispatch rejected BEFORE bridge admission: the delivery identity is
    /// half-formed (idempotency key without a correlation id, or vice
    /// versa) or fails upstream canonical validation (non-nil canonical
    /// UUID correlation). Fail-closed on purpose: a degraded delivery under
    /// a broken identity is a silent dedup hole - the caller gets dedup or
    /// this refusal, never at-least-once. NO delivery occurs.
    InvalidDeliveryIdentity {
        identity: AgentIdentity,
        detail: String,
    },
    /// Continuity store error.
    Store(ContinuityStoreError),
    /// Lease provider error.
    Lease(super::types::LeaseError),
    /// Duplicate identities in roster.
    DuplicateIdentity(AgentIdentity),
    /// Stale fencing token on checkpoint.
    StaleFencingToken {
        identity: AgentIdentity,
        presented: FencingToken,
        current: FencingToken,
    },
    /// Stale checkpoint version.
    StaleCheckpointVersion {
        identity: AgentIdentity,
        presented: CheckpointVersion,
        current: CheckpointVersion,
    },
    StaleContinuityGeneration {
        identity: AgentIdentity,
        presented: ContinuityGeneration,
        current: ContinuityGeneration,
    },
    /// A lifecycle request named an old generated runtime alias after the
    /// durable identity had already advanced to another generation.
    StaleRuntimeAlias {
        identity: AgentIdentity,
        requested: String,
        current: Option<AgentRuntimeId>,
    },
    /// A completion wait presented a baseline from a superseded runtime
    /// incarnation. Turn counts do not carry across incarnations, so this is
    /// reported rather than resolved either way — the caller must capture a
    /// fresh baseline instead of inferring that its turn did or did not run.
    CompletionIncarnationChanged {
        identity: AgentIdentity,
        baseline: CompletionCursor,
        observed: CompletionCursor,
    },
    /// The identity is parked because the HOST deterministically rejected
    /// its build (the candidate-mode effect gate class): the app-side
    /// `callback/build_agent` round trip completed and the host answered
    /// with an error. No automatic retry — a roster/policy (spec) change
    /// clears the park; `reason` carries the host's rejection for operators.
    HostRejectedBuild {
        identity: AgentIdentity,
        reason: String,
    },
    /// A concrete member resume reached a typed terminal refusal. The
    /// continuity record is preserved and the failure remains attributable to
    /// this identity when an eager fleet restore converts it into a visible
    /// `RestoreOutcome::Broken`.
    EmbodimentRejected(Box<ContinuityFailure>),
    /// Generic I/O or internal error.
    Internal(String),
}

impl std::fmt::Display for IdentityRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownIdentity(id) => write!(f, "unknown identity: {id}"),
            Self::NotAddressable(err) => write!(f, "{err}"),
            Self::CompletionUnavailable { identity, reason } => write!(
                f,
                "completion-bearing send to {identity} cannot be honoured: {reason}; \
                 NOTHING was submitted"
            ),
            Self::AdmissionFailed { identity, detail } => write!(
                f,
                "completion-bearing send to {identity} failed at admission: {detail}; \
                 the turn never started"
            ),
            Self::CompletionFailed { identity, detail } => write!(
                f,
                "completion-bearing send to {identity} was admitted and then failed: {detail}; \
                 the turn RAN - do not retry"
            ),
            Self::PostAdmissionResolutionFailed { identity, detail } => write!(
                f,
                "completion-bearing send to {identity} COMPLETED and then its session \
                 projection failed: {detail}; the turn RAN - do not retry"
            ),
            Self::PostAdmissionSuperseded { identity, detail } => write!(
                f,
                "completion-bearing send to {identity} COMPLETED, but the identity was \
                 superseded while it ran: {detail}; the turn RAN against an embodiment \
                 that no longer exists - do not retry, and its session was NOT rebound"
            ),
            Self::NoActiveLease(id) => write!(f, "no active lease for {id}"),
            Self::AlreadyEmbodied { identity, holder } => write!(
                f,
                "identity {identity} is already embodied by runtime instance '{holder}' \
                 (single-embodiment guard: refusing a second live bind)"
            ),
            Self::LeaseLost(id) => write!(f, "lease lost for {id}"),
            Self::InvalidState {
                identity,
                state,
                operation,
            } => write!(
                f,
                "cannot {operation} identity {identity} in state {state:?}"
            ),
            Self::InvalidDeliveryIdentity { identity, detail } => write!(
                f,
                "dispatch to {identity} refused before bridge admission: invalid delivery \
                 identity ({detail}); dedup or typed refusal, never a degraded delivery"
            ),
            Self::Store(err) => write!(f, "continuity store: {err}"),
            Self::Lease(err) => write!(f, "lease provider: {err}"),
            Self::DuplicateIdentity(id) => write!(f, "duplicate identity in roster: {id}"),
            Self::StaleFencingToken {
                identity,
                presented,
                current,
            } => write!(
                f,
                "stale fencing token for {identity}: presented {presented}, current {current}"
            ),
            Self::StaleCheckpointVersion {
                identity,
                presented,
                current,
            } => write!(
                f,
                "stale checkpoint version for {identity}: presented {presented}, current {current}"
            ),
            Self::StaleContinuityGeneration {
                identity,
                presented,
                current,
            } => write!(
                f,
                "stale continuity generation for {identity}: presented {presented}, current {current}"
            ),
            Self::StaleRuntimeAlias {
                identity,
                requested,
                current,
            } => write!(
                f,
                "stale runtime alias for {identity}: requested {requested}, current {}",
                current
                    .as_ref()
                    .map(AgentRuntimeId::as_str)
                    .unwrap_or("<none>")
            ),
            Self::CompletionIncarnationChanged {
                identity,
                baseline,
                observed,
            } => write!(
                f,
                "completion baseline {baseline} for {identity} belongs to a superseded runtime \
                 incarnation (now {observed}); capture a fresh baseline"
            ),
            Self::HostRejectedBuild { identity, reason } => write!(
                f,
                "identity {identity} is parked: the host deterministically rejected its build \
                 ({reason}); a roster/policy (spec) change clears the park"
            ),
            Self::EmbodimentRejected(failure) => write!(
                f,
                "identity {} embodiment was rejected: {}",
                failure.identity, failure.detail
            ),
            Self::Internal(msg) => write!(f, "internal: {msg}"),
        }
    }
}

/// Digest of the exact roster spec, for [`HostRejectedBuildPark`] scoping.
/// Process-local (std hasher over the canonical JSON form): the park itself
/// is in-memory, so cross-process stability is not required.
pub(crate) fn durable_spec_digest(spec: &DurableAgentSpec) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    match serde_json::to_string(spec) {
        Ok(json) => json.hash(&mut hasher),
        // A roster spec is plain data; serialization cannot realistically
        // fail. Degrade to "spec never changes" rather than panicking.
        Err(_) => spec.identity.as_str().hash(&mut hasher),
    }
    hasher.finish()
}

/// A build failure whose root cause is the HOST's own answer: the
/// `callback/build_agent` round trip COMPLETED and the app returned an error
/// (rpc_gateway mints `callback/build_agent failed: callback error: <host
/// message>` for exactly this case — see `StdioCallbackBridge::call`).
/// Transport-tier callback failures ("callback transport closed", timeouts,
/// dropped channels) deliberately do NOT match: those are retryable and stay
/// on the existing reconcile/repair lanes (their backoff is the upstream
/// meerkat fix, not this park).
pub(crate) fn is_host_rejected_build_error(detail: &str) -> bool {
    detail.contains("callback/build_agent failed: callback error:")
}

impl std::error::Error for IdentityRuntimeError {}

impl From<ContinuityStoreError> for IdentityRuntimeError {
    fn from(err: ContinuityStoreError) -> Self {
        match err {
            ContinuityStoreError::StaleFencingToken {
                identity,
                presented,
                current,
            } => Self::StaleFencingToken {
                identity,
                presented,
                current,
            },
            ContinuityStoreError::StaleCheckpointVersion {
                identity,
                presented,
                current,
            } => Self::StaleCheckpointVersion {
                identity,
                presented,
                current,
            },
            ContinuityStoreError::StaleContinuityGeneration {
                identity,
                presented,
                current,
            } => Self::StaleContinuityGeneration {
                identity,
                presented,
                current,
            },
            other => Self::Store(other),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-identity runtime state
// ---------------------------------------------------------------------------

/// Tracks the live state for a single identity within the runtime.
#[derive(Debug, Clone)]
pub(crate) struct IdentityEntry {
    pub spec: DurableAgentSpec,
    /// Bootstrap pass that last accepted `spec` as this entry's desired
    /// roster projection. Foreground materialization binds readiness writes
    /// to this entry-owned generation only after acquiring the lifecycle
    /// lock, so a pass that wins that lock cannot be mistaken for either its
    /// predecessor or successor merely because the global status epoch moved.
    pub bootstrap_generation: u64,
    pub state: IdentityLifecycleState,
    pub continuity: Option<ContinuityRecord>,
    pub lease: Option<LeaseEntry>,
    /// Exact authority that a completed lower-plane transition tried and
    /// failed to release. This is deliberately separate from `lease`: Broken
    /// identities must not advertise or renew active authority, but repair
    /// still needs the exact fencing token to retry the provider release
    /// before any reacquire attempt.
    pub pending_lease_release: Option<LeaseGrant>,
    pub checkpoint_version: CheckpointVersion,
    /// Whether a durable runtime_store is available (affects dispatch ack semantics).
    pub has_runtime_store: bool,
    /// Terminal heal verdict from the bridge's heal authority. While set,
    /// the continuity repair supervisor must not re-attempt this identity and
    /// reconcile must not cosmetically reset it to Dormant — either would
    /// restart the 2026-07-29 heal/re-Break loop. Cleared by any non-Broken
    /// lifecycle projection (a real recovery or an operator reset).
    pub continuity_unrecoverable: Option<ContinuityUnrecoverable>,
    /// Typed park for a build the host deterministically rejected (the
    /// candidate-mode effect gate class). While set AND the recorded spec
    /// digest still matches `spec`, materialization fails fast typed (no
    /// bridge/callback churn) and the repair supervisor skips the identity.
    /// A changed spec clears it — the retry is then permitted.
    pub host_rejected_build_park: Option<HostRejectedBuildPark>,
}

/// Tracks a held lease for an identity.
#[derive(Debug, Clone)]
pub(crate) struct LeaseEntry {
    pub fencing_token: FencingToken,
    pub ttl: Duration,
    pub acquired_at: Instant,
}

impl LeaseEntry {
    pub fn is_expired(&self) -> bool {
        self.acquired_at.elapsed() > self.ttl
    }

    pub fn ttl_remaining(&self) -> Duration {
        self.ttl.saturating_sub(self.acquired_at.elapsed())
    }

    pub fn is_healthy(&self) -> bool {
        // Healthy if more than 20% TTL remains
        let remaining = self.ttl_remaining();
        remaining > self.ttl / 5
    }
}

// ---------------------------------------------------------------------------
// Identity-scoped events
// ---------------------------------------------------------------------------

/// Events emitted for a specific identity, used by `subscribe()`.
#[derive(Debug, Clone)]
pub enum IdentityEvent {
    /// Lifecycle state changed.
    StateChanged {
        identity: AgentIdentity,
        new_state: IdentityLifecycleState,
    },
    /// Lease acquired or renewed.
    LeaseUpdated {
        identity: AgentIdentity,
        fencing_token: FencingToken,
    },
    /// Lease lost.
    LeaseLost { identity: AgentIdentity },
    /// Checkpoint completed.
    CheckpointCompleted {
        identity: AgentIdentity,
        version: CheckpointVersion,
    },
    /// Resume could not reuse a persisted runtime binding and materialization
    /// fresh-spawned a member instead.
    ResumeFallback {
        identity: AgentIdentity,
        reason: super::bridge::ResumeFallbackReason,
    },
}

/// Per-identity event channel capacity.
const IDENTITY_EVENT_CHANNEL_CAPACITY: usize = 64;
const DEFAULT_LEASE_RENEWAL_MAX_POLL_INTERVAL: Duration = Duration::from_mins(1);
const DEFAULT_LEASE_RENEWAL_MIN_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Base delay for the lease-renewal failure backoff. The renewal tick's
/// normal cadence is TTL-derived (down to a 10ms floor), so a lease provider
/// that errors persistently would otherwise retry — and warn — at that floor
/// rate. On failure we back off from this base, doubling toward the max poll
/// interval, so a backend outage can't spin the renewal task.
const LEASE_RENEWAL_FAILURE_BACKOFF_BASE: Duration = Duration::from_secs(1);

fn lease_renewal_failure_backoff(
    consecutive_failures: u32,
    max_poll_interval: Duration,
) -> Duration {
    LEASE_RENEWAL_FAILURE_BACKOFF_BASE
        .saturating_mul(1u32 << consecutive_failures.min(6))
        .min(max_poll_interval)
}

// ---------------------------------------------------------------------------
// IdentityRuntime
// ---------------------------------------------------------------------------

/// Configuration for the identity-first runtime.
pub struct IdentityRuntimeConfig {
    pub continuity_store: Arc<dyn ContinuityStore>,
    pub lease_provider: Arc<dyn LeaseProvider>,
    pub runtime_instance_id: String,
    pub has_runtime_store: bool,
    pub durability_policy: DurabilityPolicy,
    /// Optional session bridge for real session delivery. When `None`,
    /// delivery operations validate invariants but do not forward to
    /// the Meerkat session pipeline (useful for tests).
    pub bridge: Option<Arc<dyn SessionBridge>>,
    /// Default timeout for wait_for_output / wait_for_output_containing.
    /// Defaults to 90 seconds if not set.
    pub default_timeout: Option<Duration>,
}

#[derive(Clone)]
pub struct IdentityFirstRuntimeContext {
    pub runtime: Arc<IdentityRuntime>,
    pub roster_provider: Arc<dyn RosterProvider>,
    pub topology_provider: Option<Arc<dyn TopologyProvider>>,
    pub customizer: Option<Arc<dyn AgentCustomizer>>,
    mob_definition: Option<meerkat_mob::MobDefinition>,
    bootstrap_mode: IdentityBootstrapMode,
}

impl IdentityFirstRuntimeContext {
    pub(crate) async fn topology_snapshot_inputs(
        &self,
    ) -> Result<(Vec<DurableAgentSpec>, Vec<ManagedPeerEdge>), IdentityRuntimeError> {
        let previous_identities = self.runtime.registered_identities().await;
        let roster = self
            .roster_provider
            .roster(&RosterContext {
                mob_definition: self.mob_definition.clone(),
                previous_identities,
            })
            .await
            .map_err(|error| IdentityRuntimeError::Internal(format!("roster provider: {error}")))?;
        let identities = roster
            .iter()
            .map(|spec| spec.identity.clone())
            .collect::<Vec<_>>();
        let declared = match self.topology_provider.as_deref() {
            Some(provider) => provider
                .compute_edges(
                    &identities,
                    &TopologyContext {
                        roster: roster.clone(),
                    },
                )
                .await
                .map_err(|error| {
                    IdentityRuntimeError::Internal(format!("topology provider: {error}"))
                })?,
            None => self.runtime.desired_peer_edges.read().await.clone(),
        };
        Ok((roster, declared))
    }

    pub fn new(
        runtime: Arc<IdentityRuntime>,
        roster_provider: Arc<dyn RosterProvider>,
        topology_provider: Option<Arc<dyn TopologyProvider>>,
        customizer: Option<Arc<dyn AgentCustomizer>>,
        mob_definition: Option<meerkat_mob::MobDefinition>,
    ) -> Self {
        Self::new_with_lazy_materialization(
            runtime,
            roster_provider,
            topology_provider,
            customizer,
            mob_definition,
            false,
        )
    }

    pub fn new_with_lazy_materialization(
        runtime: Arc<IdentityRuntime>,
        roster_provider: Arc<dyn RosterProvider>,
        topology_provider: Option<Arc<dyn TopologyProvider>>,
        customizer: Option<Arc<dyn AgentCustomizer>>,
        mob_definition: Option<meerkat_mob::MobDefinition>,
        lazy_materialization: bool,
    ) -> Self {
        Self::new_with_bootstrap_mode(
            runtime,
            roster_provider,
            topology_provider,
            customizer,
            mob_definition,
            if lazy_materialization {
                IdentityBootstrapMode::LazyMaterialize
            } else {
                IdentityBootstrapMode::EagerMaterialize
            },
        )
    }

    /// Construct a context that preserves the complete startup policy across
    /// later roster reconciliation.
    pub fn new_with_bootstrap_mode(
        runtime: Arc<IdentityRuntime>,
        roster_provider: Arc<dyn RosterProvider>,
        topology_provider: Option<Arc<dyn TopologyProvider>>,
        customizer: Option<Arc<dyn AgentCustomizer>>,
        mob_definition: Option<meerkat_mob::MobDefinition>,
        bootstrap_mode: IdentityBootstrapMode,
    ) -> Self {
        runtime.set_reset_roster_provider_context(
            Some(roster_provider.clone()),
            mob_definition.clone(),
        );
        Self {
            runtime,
            roster_provider,
            topology_provider,
            customizer,
            mob_definition,
            bootstrap_mode,
        }
    }

    pub fn bootstrap_mode(&self) -> &IdentityBootstrapMode {
        &self.bootstrap_mode
    }

    /// Apply the configured bootstrap policy to an already-resolved roster.
    /// The same helper is used at startup and during reconcile so a lazy
    /// deployment can never accidentally hydrate the full fleet.
    pub async fn bootstrap_roster(
        &self,
        roster: &[DurableAgentSpec],
    ) -> Result<super::orchestrator::RestoreFlowResult, IdentityRuntimeError> {
        let _controller = self.runtime.bootstrap_controller.lock().await;
        let generation = self
            .runtime
            .begin_identity_bootstrap_pending(self.bootstrap_mode.clone());
        if let Err(error) = self.prepare_controlled_bootstrap().await {
            self.runtime.fail_identity_bootstrap(generation, &error);
            return Err(error);
        }
        self.apply_roster_controlled(generation, roster).await
    }

    /// Apply a roster under the runtime's single bootstrap controller.
    ///
    /// Startup and the public refresh API now share ONE restore path. The
    /// former split (bootstrap skipped snapshot payloads for opted-out
    /// bridges, public refresh always loaded them) meant console add-member
    /// and every reconcile pass paid a full session-blob read per Ready
    /// member for a payload nothing reads. `restore_flow` owns the single
    /// read-on-need rule; see its docs.
    async fn prepare_controlled_bootstrap(&self) -> Result<(), IdentityRuntimeError> {
        if self.runtime.bootstrap_shutdown.load(Ordering::Acquire) {
            return Err(IdentityRuntimeError::Internal(
                "identity bootstrap is shutting down".to_string(),
            ));
        }
        // Stop admitting new warm items and let any materialization already in
        // flight reach a transaction boundary before applying the next roster.
        self.runtime.request_identity_bootstrap_stop();
        self.runtime.join_identity_bootstrap_task().await;
        if self.runtime.bootstrap_shutdown.load(Ordering::Acquire) {
            return Err(IdentityRuntimeError::Internal(
                "identity bootstrap is shutting down".to_string(),
            ));
        }
        Ok(())
    }

    async fn apply_roster_controlled(
        &self,
        generation: u64,
        roster: &[DurableAgentSpec],
    ) -> Result<super::orchestrator::RestoreFlowResult, IdentityRuntimeError> {
        if let Err(message) = self.bootstrap_mode.validate() {
            let error = IdentityRuntimeError::Internal(message);
            self.runtime
                .begin_identity_bootstrap(generation, self.bootstrap_mode.clone(), roster)
                .await;
            self.runtime.fail_identity_bootstrap(generation, &error);
            return Err(error);
        }
        // Publish the new pass before any provider/restore await. RPC dispatch
        // is concurrent, so retaining the previous complete/ready snapshot here
        // would let a readiness waiter falsely pass while reconcile is active.
        self.runtime
            .begin_identity_bootstrap(generation, self.bootstrap_mode.clone(), roster)
            .await;
        // Converge the physical runtime before either restore policy publishes
        // the new roster. The restore flows only iterate desired identities;
        // without this boundary a removed member can remain live (and leased)
        // while the reduced bootstrap snapshot reports ready. Likewise, an
        // already-active member must not keep running an old build after its
        // desired spec changes.
        if let Err(error) = self
            .runtime
            .reconcile_roster_members(roster, generation)
            .await
        {
            self.runtime.fail_identity_bootstrap(generation, &error);
            return Err(error);
        }
        // One eager path for both startup and refresh: the snapshot-policy
        // twin is gone, so there is nothing left for the caller to select.
        let result = match &self.bootstrap_mode {
            IdentityBootstrapMode::EagerMaterialize => {
                super::orchestrator::restore_flow(
                    &self.runtime,
                    roster,
                    self.topology_provider.as_deref(),
                    self.customizer.as_deref(),
                )
                .await
            }
            IdentityBootstrapMode::LazyMaterialize
            | IdentityBootstrapMode::LazyWithBackgroundWarm { .. } => {
                super::orchestrator::lazy_register_flow(
                    &self.runtime,
                    roster,
                    self.topology_provider.as_deref(),
                )
                .await
            }
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.runtime.fail_identity_bootstrap(generation, &error);
                return Err(error);
            }
        };
        if self.runtime.bootstrap_shutdown.load(Ordering::Acquire) {
            let error =
                IdentityRuntimeError::Internal("identity bootstrap is shutting down".to_string());
            self.runtime.fail_identity_bootstrap(generation, &error);
            return Err(error);
        }
        self.runtime
            .install_identity_bootstrap(generation, self.bootstrap_mode.clone(), roster, &result)
            .await;
        Ok(result)
    }

    pub async fn refresh_desired_topology(
        &self,
    ) -> Result<super::orchestrator::RestoreFlowResult, IdentityRuntimeError> {
        // One runtime owns one controller from roster discovery through task
        // installation. Provider calls are user code and can be slow; publish
        // the in-flight pass before awaiting them so concurrent readiness RPCs
        // cannot observe the previous terminal snapshot.
        let _controller = self.runtime.bootstrap_controller.lock().await;
        let generation = self
            .runtime
            .begin_identity_bootstrap_pending(self.bootstrap_mode.clone());
        if let Err(error) = self.prepare_controlled_bootstrap().await {
            self.runtime.fail_identity_bootstrap(generation, &error);
            return Err(error);
        }
        let roster = match self
            .roster_provider
            .roster(&RosterContext {
                mob_definition: self.mob_definition.clone(),
                previous_identities: Vec::new(),
            })
            .await
        {
            Ok(roster) => roster,
            Err(err) => {
                let error = IdentityRuntimeError::Internal(format!("roster provider: {err}"));
                self.runtime.fail_identity_bootstrap(generation, &error);
                return Err(error);
            }
        };

        self.apply_roster_controlled(generation, &roster).await
    }

    /// Cancellation-safe reconcile for RPC/host request boundaries.
    /// Dropping the caller only drops its result receiver; the runtime owns
    /// the pass until every acquired lease and bridge mutation reaches an
    /// explicit commit or rollback boundary.
    pub async fn refresh_desired_topology_tracked(
        self: &Arc<Self>,
    ) -> Result<super::orchestrator::RestoreFlowResult, IdentityRuntimeError> {
        let context = Arc::clone(self);
        let runtime = Arc::clone(&self.runtime);
        runtime
            .run_tracked_foreground(async move { context.refresh_desired_topology().await })
            .await
    }

    /// Background repair for Broken identities. A rejected resume degrades the
    /// identity to Broken while preserving the durable session; the documented
    /// contract is "the next reconcile retries the resume" — but without this
    /// task, nothing runs that reconcile: delivery refuses Broken identities
    /// (REQ-13 fails loudly), `materialize` refuses the Broken state, and the
    /// only retries were a manual `mobkit/reconcile_identity` RPC or a process
    /// restart. HomeCore 0.7.23 sat with 14 preserved-but-parked identities
    /// because of exactly that gap.
    ///
    /// The loop sleeps, and only when at least one identity is Broken re-runs
    /// [`Self::refresh_desired_topology`] — the same idempotent flow the
    /// reconcile RPC runs (eager: `restore_flow` retries the resume; lazy:
    /// `lazy_register_flow` re-registers a store-Ready identity as Dormant so
    /// on-demand materialization retries). Backoff doubles while identities
    /// stay Broken — persistent causes (e.g. an upstream store regression)
    /// produce bounded log noise, and transient causes (disk full, lock
    /// contention) heal without a restart.
    pub fn spawn_broken_identity_repair_task(
        self: Arc<Self>,
        policy: ContinuityRepairPolicy,
    ) -> JoinHandle<()> {
        tokio::spawn(self.run_broken_identity_repair_loop(policy, None))
    }

    pub(crate) fn spawn_tracked_broken_identity_repair_task(
        self: Arc<Self>,
        policy: ContinuityRepairPolicy,
    ) -> TrackedContinuityRepairTask {
        let (cancel, receiver) = watch::channel(false);
        let join = tokio::spawn(self.run_broken_identity_repair_loop(policy, Some(receiver)));
        TrackedContinuityRepairTask { cancel, join }
    }

    async fn run_broken_identity_repair_loop(
        self: Arc<Self>,
        policy: ContinuityRepairPolicy,
        mut cancellation: Option<watch::Receiver<bool>>,
    ) {
        let mut backoff = policy.initial_backoff;
        // Bounded non-identical retries (OB3 0.8.12-era evidence): a repair
        // pass whose failure comes back byte-identical N times in a row is a
        // deterministic wall, and each blind retry re-executes the pass's
        // DESTRUCTIVE dispose steps against the same blocking precondition.
        // Track consecutive per-identity failure signatures and park typed
        // after [`REPAIR_IDENTICAL_FAILURE_PARK_ATTEMPTS`].
        let mut identical_failure_streaks: HashMap<AgentIdentity, (String, u32)> = HashMap::new();
        loop {
            if let Some(cancellation) = cancellation.as_mut() {
                if *cancellation.borrow() {
                    return;
                }
                tokio::select! {
                    () = tokio::time::sleep(backoff) => {}
                    changed = cancellation.changed() => {
                        match changed {
                            Ok(()) if *cancellation.borrow() => return,
                            Ok(()) => continue,
                            // Dropping the runtime-owned supervisor drops the
                            // only sender. Treat that as cancellation; looping
                            // on the permanently closed receiver would spin a
                            // detached task at 100% CPU.
                            Err(_) => return,
                        }
                    }
                }
            } else {
                tokio::time::sleep(backoff).await;
            }
            let broken = self.runtime.broken_identities().await;
            if broken.is_empty() {
                backoff = policy.initial_backoff;
                continue;
            }
            // Heal must be REAL before reconcile runs: reconcile alone only
            // resets the runtime entry (lazy mode re-registers Dormant) while
            // the durable head can stay an intra-turn projection that the
            // next materialization re-Breaks — the measured 2026-07-29
            // production heal/re-Break loop. Drive the bridge's heal
            // authority FIRST; only identities whose durable head is (now)
            // committed — or whose bridge has no heal seam — proceed.
            let mut repairable = Vec::new();
            let mut recovery_failures = 0usize;
            for identity in &broken {
                if self
                    .runtime
                    .continuity_unrecoverable(identity)
                    .await
                    .is_some()
                {
                    // Terminal typed verdict already recorded: stable across
                    // calls, so re-healing every cycle is exactly the loop
                    // this replaces. Operators act on the surfaced reason.
                    continue;
                }
                if self
                    .runtime
                    .host_rejected_build_park(identity)
                    .await
                    .is_some()
                {
                    // The host's gate rejects this exact spec
                    // deterministically: reattempting re-asks the same
                    // question and burns a build + callback round trip per
                    // cycle. A spec change clears the park (checked inside
                    // the accessor); until then this identity is parked, not
                    // repaired.
                    continue;
                }
                match self.attempt_committed_boundary_recovery(identity).await {
                    BrokenRepairDisposition::Repairable => repairable.push(identity.clone()),
                    BrokenRepairDisposition::Unprovable => {}
                    BrokenRepairDisposition::RetryLater => recovery_failures += 1,
                }
            }
            if repairable.is_empty() {
                // Nothing eligible this pass: either every Broken identity
                // carries a terminal verdict (idle at base cadence — a cheap
                // read, no reconcile churn) or recovery itself failed
                // transiently (back off before retrying recovery).
                backoff = if recovery_failures > 0 {
                    (backoff * 2).min(policy.max_backoff)
                } else {
                    policy.initial_backoff
                };
                if cancellation
                    .as_ref()
                    .is_some_and(|cancellation| *cancellation.borrow())
                {
                    return;
                }
                continue;
            }
            tracing::info!(
                broken = repairable.len(),
                "continuity repair: retrying restore for Broken identities"
            );
            let pass = match self.refresh_desired_topology().await {
                Ok(pass) => pass,
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "continuity repair reconcile failed; backing off"
                    );
                    backoff = (backoff * 2).min(policy.max_backoff);
                    if cancellation
                        .as_ref()
                        .is_some_and(|cancellation| *cancellation.borrow())
                    {
                        return;
                    }
                    continue;
                }
            };
            let still_broken = self.runtime.repairable_broken_identities().await;
            let healed = repairable
                .iter()
                .filter(|id| !still_broken.contains(id))
                .count();
            if healed > 0 {
                tracing::info!(
                    healed,
                    still_broken = still_broken.len(),
                    "continuity repair healed identities"
                );
            }
            for identity in &repairable {
                if !still_broken.contains(identity) {
                    identical_failure_streaks.remove(identity);
                    continue;
                }
                let Some(super::orchestrator::RestoreOutcome::Broken(failure)) =
                    pass.outcomes.get(identity)
                else {
                    // No comparable typed failure for this pass (the identity
                    // broke through a different door); a streak cannot be
                    // byte-compared across shapes.
                    identical_failure_streaks.remove(identity);
                    continue;
                };
                let signature = format!("{:?}: {}", failure.kind, failure.detail);
                let streak = identical_failure_streaks
                    .entry(identity.clone())
                    .or_insert_with(|| (signature.clone(), 0));
                if streak.0 == signature {
                    streak.1 += 1;
                } else {
                    *streak = (signature.clone(), 1);
                }
                if streak.1 >= REPAIR_IDENTICAL_FAILURE_PARK_ATTEMPTS {
                    identical_failure_streaks.remove(identity);
                    tracing::error!(
                        %identity,
                        attempts = REPAIR_IDENTICAL_FAILURE_PARK_ATTEMPTS,
                        blocking_failure = %signature,
                        "continuity repair failed byte-identically on every attempt; \
                         parking the identity typed instead of re-executing destructive \
                         repair steps on a timer. Operator path back after fixing the \
                         blocking failure: restart the gateway (the park is \
                         process-local; boot re-attempts repair once) or reset the \
                         identity via `mobkit/reset` (deliberate fresh start)"
                    );
                    if !self
                        .runtime
                        .mark_continuity_unrecoverable(
                            identity,
                            format!(
                                "continuity repair parked after \
                                 {REPAIR_IDENTICAL_FAILURE_PARK_ATTEMPTS} consecutive \
                                 byte-identical repair failures; blocking failure: \
                                 {signature}. After fixing it, restart the gateway \
                                 (process-local park; boot re-attempts repair) or reset \
                                 the identity via `mobkit/reset`"
                            ),
                        )
                        .await
                    {
                        tracing::debug!(
                            %identity,
                            "identity left Broken before the repair park could be recorded"
                        );
                    }
                }
            }
            backoff = if still_broken.is_empty() && recovery_failures == 0 {
                policy.initial_backoff
            } else {
                (backoff * 2).min(policy.max_backoff)
            };
            if cancellation
                .as_ref()
                .is_some_and(|cancellation| *cancellation.borrow())
            {
                return;
            }
        }
    }

    /// Ask the bridge's heal authority to drive this Broken identity's
    /// durable session head to a strict-resume-acceptable committed boundary,
    /// and translate the verdict into what the repair pass may do next.
    async fn attempt_committed_boundary_recovery(
        &self,
        identity: &AgentIdentity,
    ) -> BrokenRepairDisposition {
        let Some(bridge) = self.runtime.bridge() else {
            // Metadata-only runtime (tests): reconcile owns the retry.
            return BrokenRepairDisposition::Repairable;
        };
        let Some(session_id) = self.runtime.continuity_session_id(identity).await else {
            // No durable session bound (e.g. the store failed before a record
            // existed): there is no head to heal; reconcile retries as before.
            return BrokenRepairDisposition::Repairable;
        };
        match bridge.recover_committed_boundary(&session_id).await {
            Ok(
                CommittedBoundaryRepair::AlreadyCommitted | CommittedBoundaryRepair::Unsupported,
            ) => BrokenRepairDisposition::Repairable,
            Ok(CommittedBoundaryRepair::Recovered) => {
                tracing::info!(
                    %identity,
                    %session_id,
                    "continuity heal: recovery persisted a committed durable head; \
                     proceeding to reconcile"
                );
                BrokenRepairDisposition::Repairable
            }
            Ok(CommittedBoundaryRepair::Unprovable { reason }) => {
                tracing::error!(
                    %identity,
                    %session_id,
                    reason = %reason,
                    "continuity heal verdict: durable head unprovable; parking the \
                     identity as Broken until an operator intervenes"
                );
                if !self
                    .runtime
                    .mark_continuity_unrecoverable(identity, reason)
                    .await
                {
                    // The entry left Broken between the read and the mark
                    // (an operator reset raced us); nothing to park.
                    tracing::debug!(
                        %identity,
                        "unprovable verdict arrived after the identity left Broken"
                    );
                }
                BrokenRepairDisposition::Unprovable
            }
            Err(error) => {
                // Only the error tier is retryable per the heal contract
                // (Busy mid-turn, store I/O, CAS races).
                tracing::warn!(
                    %identity,
                    %session_id,
                    error = %error,
                    "committed-boundary recovery failed; retrying next repair pass"
                );
                BrokenRepairDisposition::RetryLater
            }
        }
    }
}

/// What the repair pass may do with one Broken identity after consulting the
/// bridge's heal authority.
enum BrokenRepairDisposition {
    /// Reconcile may retry this identity now (head committed, recovered, or
    /// no heal seam to consult).
    Repairable,
    /// Terminal typed verdict recorded; excluded until an operator clears it.
    Unprovable,
    /// The recovery attempt itself failed transiently; retry next pass.
    RetryLater,
}

/// Runtime-owned repair supervisor with cooperative idle cancellation.
///
/// Cancellation is observed while sleeping or between passes. An active
/// restore pass is joined to its explicit commit/rollback boundary instead of
/// being raw-aborted after lease acquisition.
pub(crate) struct TrackedContinuityRepairTask {
    cancel: watch::Sender<bool>,
    join: JoinHandle<()>,
}

impl TrackedContinuityRepairTask {
    pub(crate) fn cancel(&self) {
        let _ = self.cancel.send(true);
    }

    pub(crate) async fn cancel_and_join(self) {
        self.cancel();
        let _ = self.join.await;
    }
}

/// Runtime-owned lease-renewal supervisor with cooperative cancellation.
///
/// Cancellation is observed while the supervisor is idle or between ticks.
/// An in-flight renewal is always joined through publication of the provider's
/// returned fencing token so final shutdown releases current authority.
pub(crate) struct TrackedLeaseRenewalTask {
    cancel: watch::Sender<bool>,
    join: JoinHandle<()>,
}

impl TrackedLeaseRenewalTask {
    pub(crate) fn cancel(&self) {
        let _ = self.cancel.send(true);
    }

    pub(crate) async fn cancel_and_join(self) {
        self.cancel();
        let _ = self.join.await;
    }
}

/// Consecutive byte-identical repair failures tolerated for one identity
/// before the repair supervisor parks it typed
/// ([`IdentityRuntime::mark_continuity_unrecoverable`]) instead of
/// re-executing the pass's destructive dispose steps on a timer.
const REPAIR_IDENTICAL_FAILURE_PARK_ATTEMPTS: u32 = 3;

/// Retry cadence for [`IdentityFirstRuntimeContext::spawn_broken_identity_repair_task`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContinuityRepairPolicy {
    /// Delay before the first check and after every fully-healed pass.
    pub initial_backoff: Duration,
    /// Ceiling for the doubling backoff while identities stay Broken.
    pub max_backoff: Duration,
}

impl Default for ContinuityRepairPolicy {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_secs(30),
            max_backoff: Duration::from_mins(10),
        }
    }
}

/// Weak keyed locks serialize claims on the shared raw/durable alias
/// namespace without retaining every caller-chosen alias for the lifetime of
/// the runtime. Sweeps grow geometrically while many aliases are live (as in
/// fleet bootstrap), avoiding a full-map scan for every new roster member.
struct RawMemberAliasLockTable {
    entries: BTreeMap<String, Weak<Mutex<()>>>,
    next_sweep_len: usize,
    #[cfg(test)]
    sweep_count: usize,
}

impl Default for RawMemberAliasLockTable {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            next_sweep_len: RAW_MEMBER_ALIAS_LOCK_SWEEP_MIN,
            #[cfg(test)]
            sweep_count: 0,
        }
    }
}

impl RawMemberAliasLockTable {
    fn sweep_if_needed(&mut self) {
        if self.entries.len() < self.next_sweep_len {
            return;
        }
        self.entries.retain(|_, lock| lock.upgrade().is_some());
        self.next_sweep_len = self
            .entries
            .len()
            .saturating_mul(2)
            .max(RAW_MEMBER_ALIAS_LOCK_SWEEP_MIN);
        #[cfg(test)]
        {
            self.sweep_count += 1;
        }
    }
}

/// The identity-first runtime tracks active identities and enforces delivery,
/// ownership, and lifecycle invariants.
pub struct IdentityRuntime {
    entries: RwLock<BTreeMap<AgentIdentity, IdentityEntry>>,
    event_channels: RwLock<BTreeMap<AgentIdentity, broadcast::Sender<IdentityEvent>>>,
    continuity_store: Arc<dyn ContinuityStore>,
    lease_provider: Arc<dyn LeaseProvider>,
    runtime_instance_id: String,
    has_runtime_store: bool,
    durability_policy: DurabilityPolicy,
    bridge: Option<Arc<dyn SessionBridge>>,
    reset_roster_source: StdRwLock<Option<ResetRosterSource>>,
    runtime_services: AgentRuntimeServices,
    managed_peer_edges: RwLock<BTreeSet<(AgentIdentity, AgentIdentity)>>,
    managed_peer_reconcile_lock: Mutex<()>,
    desired_peer_edges: RwLock<Vec<ManagedPeerEdge>>,
    topology_controller: StdRwLock<Option<crate::topology_control::TopologyController>>,
    materialization_locks: RwLock<BTreeMap<AgentIdentity, Arc<Mutex<()>>>>,
    best_effort_materialization_locks: RwLock<BTreeMap<AgentIdentity, Arc<Mutex<()>>>>,
    lifecycle_locks: RwLock<BTreeMap<AgentIdentity, Arc<Mutex<()>>>>,
    raw_member_alias_locks: RwLock<RawMemberAliasLockTable>,
    customizer: RwLock<Option<Arc<dyn AgentCustomizer>>>,
    agent_memory: RwLock<Option<AgentMemoryRuntimeInjector>>,
    lease_renewal_notify: Notify,
    /// Exact grants whose restore task failed before an IdentityEntry existed.
    /// These must outlive the failed task so reconcile or shutdown can retry
    /// provider release instead of leaving invisible, non-expiring authority.
    pending_unactivated_lease_releases: RwLock<Vec<LeaseGrant>>,
    pending_unactivated_lease_release_gate: Mutex<()>,
    default_timeout: Duration,
    materialization_failure_backoff: RwLock<BTreeMap<AgentIdentity, MaterializationFailureBackoff>>,
    error_hook: StdRwLock<Option<crate::unified_runtime::ErrorHook>>,
    bootstrap_status: watch::Sender<IdentityBootstrapStatus>,
    bootstrap_generation: StdMutex<u64>,
    bootstrap_controller: Mutex<()>,
    bootstrap_task: Mutex<Option<JoinHandle<()>>>,
    bootstrap_cancel: StdRwLock<Option<watch::Sender<bool>>>,
    bootstrap_shutdown: AtomicBool,
    foreground_operations: Mutex<JoinSet<()>>,
    foreground_cancel: watch::Sender<bool>,
    foreground_shutdown: AtomicBool,
    /// Post-commit cleanup for reset-superseded bridge generations. The debt
    /// is recorded before task spawn, survives task failure/timeout, and is
    /// retried synchronously before shutdown can attest cleanup or release
    /// identity fencing authority.
    pending_reset_bridge_cleanups: Arc<RwLock<BTreeMap<String, PendingResetBridgeCleanup>>>,
    reset_bridge_cleanup_tasks: Mutex<JoinSet<()>>,
    /// Per-identity completed-turn counters ([`CompletionCursor`]).
    ///
    /// RETAINED for the process lifetime — never pruned on retire, reset, or
    /// delete. Dropping an entry would let a re-registered identity republish
    /// a cursor it already published, which is the one thing a completion
    /// cursor must never do. The map is bounded by the identities this process
    /// has seen, i.e. by roster size.
    completion_cursors: StdMutex<BTreeMap<AgentIdentity, CompletionCursor>>,
}

/// One generated member alias plus the lifecycle lock owned by its durable
/// identity runtime. Cross-runtime topology code resolves all endpoints first,
/// then acquires these targets in one global order before inspecting or
/// mutating either side.
pub(crate) struct MemberAliasLifecycleTarget {
    runtime: Arc<IdentityRuntime>,
    identity: AgentIdentity,
    alias: String,
    lock: Arc<Mutex<()>>,
}

struct MultiRuntimeForegroundCompletion(watch::Sender<bool>);

impl Drop for MultiRuntimeForegroundCompletion {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

/// Internal result of one dispatch attempt: the caller-facing admission plus
/// the concrete bridge session that accepted the work (scheduler delivery
/// needs the latter; RPC callers do not).
struct DispatchOutcome {
    admission: DispatchAdmission,
    session_id: Option<SessionId>,
}

/// Exact result of the one concrete embodiment door shared by eager restore,
/// lazy foreground materialization, and background warming.
pub(crate) struct EmbodimentOutcome {
    pub(crate) record: ContinuityRecord,
    pub(crate) resumed: bool,
    pub(crate) draft: AgentBuildDraft,
}

/// Eager-only inputs to the shared embodiment transaction. Lazy foreground
/// and background callers use the registered entry's spec and customizer.
#[derive(Default)]
pub(crate) struct EmbodimentOverrides<'a> {
    pub(crate) spec: Option<&'a DurableAgentSpec>,
    pub(crate) customizer: Option<&'a dyn AgentCustomizer>,
}

#[derive(Clone)]
struct ResetRosterSource {
    provider: Arc<dyn RosterProvider>,
    mob_definition: Option<meerkat_mob::MobDefinition>,
}

#[derive(Debug, Clone)]
struct MaterializationFailureBackoff {
    suppress_until: Instant,
    error: String,
}

#[derive(Clone)]
struct PendingResetMemoryCapture {
    injector: AgentMemoryRuntimeInjector,
    identity: AgentIdentity,
    session_key: String,
    generation: u64,
}

impl PendingResetMemoryCapture {
    async fn run(&self) {
        // Repeat the synchronous marks in the owned task so a raw reset
        // future dropped after debt publication cannot let the distiller read
        // evidence before the reset quarantine boundary exists.
        self.injector.note_reset_boundary(&self.session_key);
        self.injector
            .note_session_generation(&self.identity, &self.session_key, self.generation);
        self.injector
            .distill_before_rotation(
                &self.identity,
                &self.session_key,
                crate::memory::distiller::DistillCause::Reset,
            )
            .await;
    }
}

#[derive(Clone)]
struct PendingResetBridgeCleanup {
    runtime_id: Option<AgentRuntimeId>,
    session_id: Option<SessionId>,
    memory_capture: Option<PendingResetMemoryCapture>,
}

impl PartialEq for PendingResetBridgeCleanup {
    fn eq(&self, other: &Self) -> bool {
        self.runtime_id == other.runtime_id && self.session_id == other.session_id
    }
}

impl Eq for PendingResetBridgeCleanup {}

impl PendingResetBridgeCleanup {
    fn key(&self) -> String {
        format!(
            "{}|{}",
            self.runtime_id
                .as_ref()
                .map(AgentRuntimeId::as_str)
                .unwrap_or("-"),
            self.session_id
                .as_ref()
                .map(std::string::ToString::to_string)
                .as_deref()
                .unwrap_or("-")
        )
    }
}

impl IdentityRuntime {
    /// Create a new identity runtime with the given configuration.
    pub fn new(config: IdentityRuntimeConfig) -> Self {
        let (bootstrap_status, _) = watch::channel(IdentityBootstrapStatus::empty(
            IdentityBootstrapMode::EagerMaterialize,
        ));
        let (foreground_cancel, _) = watch::channel(false);
        Self {
            entries: RwLock::new(BTreeMap::new()),
            event_channels: RwLock::new(BTreeMap::new()),
            continuity_store: config.continuity_store,
            lease_provider: config.lease_provider,
            runtime_instance_id: config.runtime_instance_id,
            has_runtime_store: config.has_runtime_store,
            durability_policy: config.durability_policy,
            bridge: config.bridge,
            reset_roster_source: StdRwLock::new(None),
            runtime_services: AgentRuntimeServices::empty(),
            managed_peer_edges: RwLock::new(BTreeSet::new()),
            managed_peer_reconcile_lock: Mutex::new(()),
            desired_peer_edges: RwLock::new(Vec::new()),
            topology_controller: StdRwLock::new(None),
            materialization_locks: RwLock::new(BTreeMap::new()),
            best_effort_materialization_locks: RwLock::new(BTreeMap::new()),
            lifecycle_locks: RwLock::new(BTreeMap::new()),
            raw_member_alias_locks: RwLock::new(RawMemberAliasLockTable::default()),
            customizer: RwLock::new(None),
            agent_memory: RwLock::new(None),
            lease_renewal_notify: Notify::new(),
            pending_unactivated_lease_releases: RwLock::new(Vec::new()),
            pending_unactivated_lease_release_gate: Mutex::new(()),
            default_timeout: config.default_timeout.unwrap_or(Duration::from_secs(90)),
            materialization_failure_backoff: RwLock::new(BTreeMap::new()),
            error_hook: StdRwLock::new(None),
            bootstrap_status,
            bootstrap_generation: StdMutex::new(0),
            bootstrap_controller: Mutex::new(()),
            bootstrap_task: Mutex::new(None),
            bootstrap_cancel: StdRwLock::new(None),
            bootstrap_shutdown: AtomicBool::new(false),
            foreground_operations: Mutex::new(JoinSet::new()),
            foreground_cancel,
            foreground_shutdown: AtomicBool::new(false),
            pending_reset_bridge_cleanups: Arc::new(RwLock::new(BTreeMap::new())),
            reset_bridge_cleanup_tasks: Mutex::new(JoinSet::new()),
            completion_cursors: StdMutex::new(BTreeMap::new()),
        }
    }

    pub fn with_runtime_services(mut self, runtime_services: AgentRuntimeServices) -> Self {
        self.runtime_services = runtime_services;
        self
    }

    pub fn with_reset_roster_provider(self, provider: Arc<dyn RosterProvider>) -> Self {
        self.set_reset_roster_provider(Some(provider));
        self
    }

    pub fn with_reset_roster_provider_context(
        self,
        provider: Arc<dyn RosterProvider>,
        mob_definition: Option<meerkat_mob::MobDefinition>,
    ) -> Self {
        self.set_reset_roster_provider_context(Some(provider), mob_definition);
        self
    }

    pub(crate) fn runtime_services(&self) -> AgentRuntimeServices {
        self.runtime_services.clone()
    }

    /// Current typed bootstrap snapshot. Reading it never waits on an
    /// in-flight materialization.
    pub fn identity_bootstrap_status(&self) -> IdentityBootstrapStatus {
        self.identity_bootstrap_status_with_generation().1
    }

    pub(crate) fn identity_bootstrap_status_with_generation(
        &self,
    ) -> (u64, IdentityBootstrapStatus) {
        let generation = self
            .bootstrap_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (*generation, self.bootstrap_status.borrow().clone())
    }

    pub(crate) fn subscribe_identity_bootstrap_status(
        &self,
    ) -> watch::Receiver<IdentityBootstrapStatus> {
        self.bootstrap_status.subscribe()
    }

    /// Wait until every tracked identity has reached Active or Broken.
    /// Broken is terminal (and `ready == false`), so callers receive a useful
    /// failure snapshot instead of hanging forever.
    pub async fn wait_identity_bootstrap_terminal(
        &self,
        timeout: Duration,
    ) -> (IdentityBootstrapStatus, bool) {
        let (status, timed_out, _) = self
            .wait_identity_bootstrap_terminal_with_generation(timeout)
            .await;
        (status, timed_out)
    }

    pub(crate) async fn wait_identity_bootstrap_terminal_with_generation(
        &self,
        timeout: Duration,
    ) -> (IdentityBootstrapStatus, bool, u64) {
        let mut receiver = self.subscribe_identity_bootstrap_status();
        let wait = async {
            loop {
                let (generation, snapshot) = self.identity_bootstrap_status_with_generation();
                if snapshot.complete && snapshot.materialization_terminal() {
                    return (snapshot, generation);
                }
                if receiver.changed().await.is_err() {
                    let (generation, snapshot) = self.identity_bootstrap_status_with_generation();
                    return (snapshot, generation);
                }
            }
        };
        match tokio::time::timeout(timeout, wait).await {
            Ok((snapshot, generation)) => (snapshot, false, generation),
            Err(_) => {
                let (generation, snapshot) = self.identity_bootstrap_status_with_generation();
                (snapshot, true, generation)
            }
        }
    }

    fn request_identity_bootstrap_stop(&self) {
        if let Some(cancel) = self
            .bootstrap_cancel
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            let _ = cancel.send(true);
        }
    }

    async fn join_identity_bootstrap_task(&self) {
        if let Some(task) = self.bootstrap_task.lock().await.take() {
            let _ = task.await;
        }
        *self
            .bootstrap_cancel
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Serialize status mutation with pass supersession. The generation lock
    /// closes the check-then-write race that an atomic epoch alone would leave
    /// between a retiring warm task and a newly-published reconcile barrier.
    fn modify_bootstrap_status<F>(&self, expected_generation: Option<u64>, modify: F) -> bool
    where
        F: FnOnce(&mut IdentityBootstrapStatus),
    {
        let current_generation = self
            .bootstrap_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if expected_generation.is_some_and(|expected| expected != *current_generation) {
            return false;
        }
        self.bootstrap_status.send_modify(modify);
        true
    }

    fn replace_bootstrap_status(
        &self,
        expected_generation: u64,
        status: IdentityBootstrapStatus,
    ) -> bool {
        let current_generation = self
            .bootstrap_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if expected_generation != *current_generation {
            return false;
        }
        self.bootstrap_status.send_replace(status);
        true
    }

    /// Close the controller, cooperatively cancel warm operations before
    /// bridge/member installation, and join the tracked task. An acquired but
    /// uninstalled lease is released explicitly; operations past that boundary
    /// reach their explicit commit/rollback path so no external lease or
    /// bridge session can be leaked by a raw task abort.
    pub(crate) async fn cancel_identity_bootstrap(&self) {
        self.bootstrap_shutdown.store(true, Ordering::Release);
        self.request_identity_bootstrap_stop();
        let _controller = self.bootstrap_controller.lock().await;
        self.request_identity_bootstrap_stop();
        self.join_identity_bootstrap_task().await;
        self.modify_bootstrap_status(None, |snapshot| {
            for entry in snapshot.identities.values_mut() {
                if entry.state == IdentityBootstrapState::Warming {
                    entry.state = IdentityBootstrapState::Dormant;
                }
            }
            snapshot.complete = true;
            snapshot.refresh_aggregates();
        });
    }

    async fn begin_identity_bootstrap(
        &self,
        generation: u64,
        mode: IdentityBootstrapMode,
        roster: &[DurableAgentSpec],
    ) {
        let entries = self.entries.read().await;
        let identities = roster
            .iter()
            .map(|spec| {
                let lifecycle = entries.get(&spec.identity).map(|entry| entry.state);
                let state = match lifecycle {
                    Some(IdentityLifecycleState::Active) => IdentityBootstrapState::Active,
                    _ if matches!(&mode, IdentityBootstrapMode::EagerMaterialize) => {
                        IdentityBootstrapState::Warming
                    }
                    _ => IdentityBootstrapState::Dormant,
                };
                (
                    spec.identity.clone(),
                    IdentityBootstrapEntry { state, error: None },
                )
            })
            .collect();
        drop(entries);
        let mut status = IdentityBootstrapStatus {
            mode,
            complete: false,
            ready: false,
            error: None,
            counts: Default::default(),
            identities,
        };
        status.refresh_aggregates();
        // A roster consisting only of already-active identities still has a
        // reconcile pass in flight. `complete` is the barrier guard even when
        // the aggregate states themselves happen to look ready.
        status.complete = false;
        status.ready = false;
        self.replace_bootstrap_status(generation, status);
    }

    fn begin_identity_bootstrap_pending(&self, mode: IdentityBootstrapMode) -> u64 {
        let mut generation = self
            .bootstrap_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *generation = generation.wrapping_add(1);
        if *generation == 0 {
            *generation = 1;
        }
        let current_generation = *generation;
        self.bootstrap_status.send_modify(|snapshot| {
            snapshot.mode = mode;
            snapshot.complete = false;
            snapshot.ready = false;
            snapshot.error = None;
            // Per-identity causes are pass-scoped. `begin_identity_bootstrap`
            // already rebuilds every entry with `error: None`, but it runs only
            // AFTER the prepare/provider await, and `fail_identity_bootstrap`
            // is reachable before that (prepare failure in `bootstrap_roster`,
            // roster-provider failure in `refresh_desired_topology`). Without
            // this reset, its `entry.error.is_none()` guard cannot tell "this
            // identity produced its own cause during THIS pass" from "left
            // over from the last pass", and an early failure would report a
            // previous pass's cause beside the current pass's `error`.
            // Only causes are cleared: `refresh_aggregates` reads `entry.state`
            // exclusively, so no count, `ready`, or `materialization_terminal`
            // value moves here.
            for entry in snapshot.identities.values_mut() {
                entry.error = None;
            }
        });
        current_generation
    }

    /// Record a PASS-level bootstrap failure.
    ///
    /// Terminality is preserved deliberately: the barrier rule is
    /// `complete && dormant == 0 && warming == 0`
    /// ([`IdentityBootstrapStatus::materialization_terminal`]), and an eager
    /// pass seeds the whole roster `Warming`, so leaving those entries alone
    /// would hang `wait_identity_bootstrap_terminal` until its timeout
    /// instead of returning a truthful failure snapshot.
    ///
    /// What must NOT happen is attributing the pass cause to each identity as
    /// if it were that identity's own. Before embodiment became per identity,
    /// one member's `bridge create_session:` failure was copied onto 16 peers
    /// (HomeCore activation-33 shape). Member failures now bypass this method,
    /// while a genuine pass failure stamps only what is known: the pass died
    /// before this identity's own outcome was recorded. A cause the identity actually produced
    /// during the pass (`mark_bootstrap_from_lifecycle`,
    /// `mark_bootstrap_materialization_finished`) is authoritative and is
    /// never overwritten. That equivalence between "the entry already has a
    /// cause" and "this pass produced it" is not free: it holds because
    /// `begin_identity_bootstrap_pending` resets per-entry causes when the
    /// pass opens, which is the only reason this `is_none()` guard cannot
    /// resurrect a previous pass's cause. Member embodiment failures no longer
    /// enter this path; they park only their own identity. The pass cause still rides
    /// `snapshot.error`, which the type documents as the pass-level slot and
    /// which every reader (RPC status, `wait_identity_bootstrap`, the Python
    /// SDK model) already parses.
    fn fail_identity_bootstrap(&self, generation: u64, error: &IdentityRuntimeError) {
        self.modify_bootstrap_status(Some(generation), |snapshot| {
            let detail = error.to_string();
            let unattributed = format!(
                "identity bootstrap pass failed before this identity reached its own \
                 outcome: {detail}"
            );
            snapshot.complete = true;
            snapshot.error = Some(detail);
            for entry in snapshot.identities.values_mut() {
                if entry.state != IdentityBootstrapState::Active {
                    entry.state = IdentityBootstrapState::Broken;
                    if entry.error.is_none() {
                        entry.error = Some(unattributed.clone());
                    }
                }
            }
            snapshot.refresh_aggregates();
        });
    }

    #[cfg(test)]
    pub(crate) fn test_supersede_identity_bootstrap_ready(&self) {
        let generation =
            self.begin_identity_bootstrap_pending(IdentityBootstrapMode::EagerMaterialize);
        self.modify_bootstrap_status(Some(generation), |snapshot| {
            snapshot.identities.clear();
            snapshot.complete = true;
            snapshot.error = None;
            snapshot.refresh_aggregates();
        });
    }

    #[cfg(test)]
    pub(crate) fn test_fail_identity_bootstrap(&self, detail: &str) {
        let generation =
            self.begin_identity_bootstrap_pending(IdentityBootstrapMode::EagerMaterialize);
        self.modify_bootstrap_status(Some(generation), |snapshot| {
            snapshot.identities.clear();
            if let Ok(identity) = AgentIdentity::parse("agent:test-bootstrap-failure") {
                snapshot.identities.insert(
                    identity,
                    IdentityBootstrapEntry {
                        state: IdentityBootstrapState::Dormant,
                        error: None,
                    },
                );
            }
            snapshot.refresh_aggregates();
        });
        self.fail_identity_bootstrap(
            generation,
            &IdentityRuntimeError::Internal(detail.to_string()),
        );
    }

    async fn install_identity_bootstrap(
        self: &Arc<Self>,
        generation: u64,
        mode: IdentityBootstrapMode,
        roster: &[DurableAgentSpec],
        result: &super::orchestrator::RestoreFlowResult,
    ) {
        let background_concurrency = match mode {
            IdentityBootstrapMode::LazyWithBackgroundWarm { concurrency } => Some(concurrency),
            _ => None,
        };
        let mut status = IdentityBootstrapStatus {
            mode: mode.clone(),
            complete: background_concurrency.is_none(),
            ready: false,
            error: None,
            counts: Default::default(),
            identities: BTreeMap::new(),
        };
        for spec in roster {
            let broken_outcome = result.outcomes.get(&spec.identity).and_then(|outcome| {
                if let super::orchestrator::RestoreOutcome::Broken(failure) = outcome {
                    Some(failure.detail.clone())
                } else {
                    None
                }
            });
            let lifecycle = self
                .status(&spec.identity)
                .await
                .ok()
                .map(|item| item.state);
            let (state, error) = match (broken_outcome, lifecycle) {
                // A store-projected Broken result is itself authoritative.
                // restore_flow intentionally does not fabricate a lifecycle
                // entry for it, so consulting lifecycle alone would default
                // to Dormant and make a terminal wait barrier time out.
                (Some(detail), _) => (IdentityBootstrapState::Broken, Some(detail)),
                (None, Some(IdentityLifecycleState::Active)) => {
                    (IdentityBootstrapState::Active, None)
                }
                (None, Some(IdentityLifecycleState::Broken)) => {
                    (IdentityBootstrapState::Broken, None)
                }
                (None, _) => (IdentityBootstrapState::Dormant, None),
            };
            status.identities.insert(
                spec.identity.clone(),
                IdentityBootstrapEntry { state, error },
            );
        }
        status.refresh_aggregates();
        if !self.replace_bootstrap_status(generation, status.clone()) {
            return;
        }

        let Some(concurrency) = background_concurrency else {
            return;
        };
        let identities = status
            .identities
            .iter()
            .filter(|(_, entry)| entry.state == IdentityBootstrapState::Dormant)
            .map(|(identity, _)| identity.clone())
            .collect::<Vec<_>>();
        if identities.is_empty() {
            self.modify_bootstrap_status(Some(generation), |snapshot| {
                snapshot.complete = true;
                snapshot.refresh_aggregates();
            });
            return;
        }

        let runtime = Arc::clone(self);
        let (cancel, task_cancel) = watch::channel(false);
        let task = tokio::spawn(async move {
            stream::iter(identities.into_iter().map(|identity| {
                let runtime = Arc::clone(&runtime);
                let mut cancel = task_cancel.clone();
                async move {
                    if *cancel.borrow() {
                        return;
                    }
                    let Some(result) = runtime
                        .materialize_for_background(&identity, &mut cancel, generation)
                        .await
                    else {
                        return;
                    };
                    if let Err(error) = result {
                        tracing::warn!(
                            %identity,
                            error = %error,
                            "identity background warm failed"
                        );
                        runtime
                            .record_best_effort_materialization_failure(
                                &identity,
                                None,
                                "background_warm",
                                &error,
                            )
                            .await;
                    }
                }
            }))
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;
            if !*task_cancel.borrow() {
                runtime.modify_bootstrap_status(Some(generation), |snapshot| {
                    snapshot.complete = true;
                    snapshot.refresh_aggregates();
                });
            }
        });
        *self
            .bootstrap_cancel
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cancel);
        *self.bootstrap_task.lock().await = Some(task);
    }

    fn mark_bootstrap_materialization_started(
        &self,
        identity: &AgentIdentity,
        generation: Option<u64>,
    ) {
        self.modify_bootstrap_status(generation, |snapshot| {
            let Some(entry) = snapshot.identities.get_mut(identity) else {
                return;
            };
            entry.state = IdentityBootstrapState::Warming;
            entry.error = None;
            snapshot.refresh_aggregates();
        });
    }

    fn mark_bootstrap_from_lifecycle(
        &self,
        identity: &AgentIdentity,
        lifecycle: IdentityLifecycleState,
        error: Option<String>,
    ) {
        self.modify_bootstrap_status(None, |snapshot| {
            let Some(entry) = snapshot.identities.get_mut(identity) else {
                return;
            };
            entry.state = match lifecycle {
                IdentityLifecycleState::Active => IdentityBootstrapState::Active,
                IdentityLifecycleState::Broken => IdentityBootstrapState::Broken,
                IdentityLifecycleState::Dormant
                | IdentityLifecycleState::Retiring
                | IdentityLifecycleState::Suspended
                | IdentityLifecycleState::Uninitialized => IdentityBootstrapState::Dormant,
            };
            entry.error = if entry.state == IdentityBootstrapState::Broken {
                error
            } else {
                None
            };
            snapshot.refresh_aggregates();
        });
    }

    fn mark_bootstrap_materialization_finished(
        &self,
        identity: &AgentIdentity,
        result: &Result<ContinuityRecord, IdentityRuntimeError>,
        generation: Option<u64>,
    ) {
        self.modify_bootstrap_status(generation, |snapshot| {
            let Some(entry) = snapshot.identities.get_mut(identity) else {
                return;
            };
            match result {
                Ok(_) => {
                    entry.state = IdentityBootstrapState::Active;
                    entry.error = None;
                }
                Err(error) => {
                    entry.state = IdentityBootstrapState::Broken;
                    entry.error = Some(error.to_string());
                }
            }
            snapshot.refresh_aggregates();
        });
    }

    fn mark_bootstrap_materialization_cancelled(
        &self,
        identity: &AgentIdentity,
        generation: Option<u64>,
    ) {
        self.modify_bootstrap_status(generation, |snapshot| {
            if let Some(entry) = snapshot.identities.get_mut(identity) {
                entry.state = IdentityBootstrapState::Dormant;
                entry.error = None;
                snapshot.refresh_aggregates();
            }
        });
    }

    /// Exact concrete member ids represented by the tracked bootstrap roster.
    /// Used only after `ready == true`, preventing a false-ready snapshot of a
    /// partially warmed mob.
    pub async fn identity_bootstrap_member_ids(&self) -> Vec<meerkat_mob::ids::AgentIdentity> {
        let tracked = self.identity_bootstrap_status();
        self.identity_bootstrap_member_ids_for_status(&tracked)
            .await
    }

    pub(crate) async fn identity_bootstrap_member_ids_for_status(
        &self,
        tracked: &IdentityBootstrapStatus,
    ) -> Vec<meerkat_mob::ids::AgentIdentity> {
        let entries = self.entries.read().await;
        tracked
            .identities
            .keys()
            .filter_map(|identity| {
                // The roster is keyed by the encoded DURABLE identity for every
                // binding. This used to mirror the old spawn-side lowering
                // (identity for external, runtime id for local); keeping that
                // split would report member ids no roster row answers to, and
                // the bootstrap status would look healthy while naming nothing.
                let entry = entries.get(identity)?;
                // A continuity row still gates inclusion: an identity with no
                // binding is not a bootstrapped member.
                entry.continuity.as_ref()?;
                Some(crate::member_comms_id::roster_member_id_for_identity(
                    identity.as_str(),
                ))
            })
            .collect()
    }

    pub async fn set_agent_customizer(&self, customizer: Option<Arc<dyn AgentCustomizer>>) {
        *self.customizer.write().await = customizer;
    }

    pub async fn set_agent_memory(&self, injector: Option<AgentMemoryRuntimeInjector>) {
        *self.agent_memory.write().await = injector;
    }

    pub async fn agent_memory_supports_recall(&self) -> bool {
        self.agent_memory.read().await.is_some()
    }

    pub async fn agent_memory_supports_remember(&self) -> bool {
        self.agent_memory
            .read()
            .await
            .as_ref()
            .is_some_and(|injector| injector.provider().supports_remember())
    }

    pub async fn agent_memory_supports_forget(&self) -> bool {
        self.agent_memory
            .read()
            .await
            .as_ref()
            .is_some_and(|injector| injector.provider().supports_forget())
    }

    pub async fn agent_memory_supports_update(&self) -> bool {
        self.agent_memory
            .read()
            .await
            .as_ref()
            .is_some_and(|injector| injector.provider().supports_supersede())
    }

    pub async fn agent_memory_supports_manifest(&self) -> bool {
        self.agent_memory
            .read()
            .await
            .as_ref()
            .is_some_and(|injector| injector.provider().supports_manifest())
    }

    pub async fn remember_agent_memory(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        memory: NewAgentMemory,
    ) -> Result<AgentMemoryRecord, AgentMemoryError> {
        self.status(identity)
            .await
            .map_err(|err| AgentMemoryError::InvalidConfig(err.to_string()))?;
        let provider = self
            .agent_memory
            .read()
            .await
            .as_ref()
            .map(AgentMemoryRuntimeInjector::provider)
            .ok_or_else(|| {
                AgentMemoryError::InvalidConfig("agent memory is not configured".to_string())
            })?;
        provider.remember(realm, identity, memory).await
    }

    pub async fn forget_agent_memory(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        memory_id: &str,
    ) -> Result<AgentMemoryForgetResult, AgentMemoryError> {
        self.status(identity)
            .await
            .map_err(|err| AgentMemoryError::InvalidConfig(err.to_string()))?;
        let provider = self
            .agent_memory
            .read()
            .await
            .as_ref()
            .map(AgentMemoryRuntimeInjector::provider)
            .ok_or_else(|| {
                AgentMemoryError::InvalidConfig("agent memory is not configured".to_string())
            })?;
        provider.forget(realm, identity, memory_id).await
    }

    /// Supersede `memory_id` within its lineage (the D4 fix): the new
    /// title/body/tags become the active record; the prior stays
    /// retrievable with provenance.
    pub async fn update_agent_memory(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        memory_id: &str,
        memory: NewAgentMemory,
    ) -> Result<MemoryId, AgentMemoryError> {
        self.status(identity)
            .await
            .map_err(|err| AgentMemoryError::InvalidConfig(err.to_string()))?;
        let provider = self
            .agent_memory
            .read()
            .await
            .as_ref()
            .map(AgentMemoryRuntimeInjector::provider)
            .ok_or_else(|| {
                AgentMemoryError::InvalidConfig("agent memory is not configured".to_string())
            })?;
        let scope = MemoryScope::Identity {
            realm: realm.to_string(),
            identity: identity.as_str().to_string(),
        };
        let record = NewMemoryRecord {
            kind: MemoryKind::Fact,
            title: memory.title,
            description: String::new(),
            body: memory.body,
            tags: memory.tags,
            evidence: Vec::new(),
            verification: None,
        };
        provider.supersede(&scope, memory_id, record).await
    }

    /// Tiered metadata manifest for the identity's own scope (§8.3).
    pub async fn manifest_agent_memory(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        tier: ManifestTier,
    ) -> Result<Vec<RecordMeta>, AgentMemoryError> {
        self.status(identity)
            .await
            .map_err(|err| AgentMemoryError::InvalidConfig(err.to_string()))?;
        let provider = self
            .agent_memory
            .read()
            .await
            .as_ref()
            .map(AgentMemoryRuntimeInjector::provider)
            .ok_or_else(|| {
                AgentMemoryError::InvalidConfig("agent memory is not configured".to_string())
            })?;
        let scope = MemoryScope::Identity {
            realm: realm.to_string(),
            identity: identity.as_str().to_string(),
        };
        provider.manifest(&[scope], tier).await
    }

    pub async fn recall_agent_memory(
        &self,
        request: AgentMemoryRecallRequest,
    ) -> Result<Vec<AgentMemoryRecord>, AgentMemoryError> {
        self.status(&request.identity)
            .await
            .map_err(|err| AgentMemoryError::InvalidConfig(err.to_string()))?;
        let provider = self
            .agent_memory
            .read()
            .await
            .as_ref()
            .map(AgentMemoryRuntimeInjector::provider)
            .ok_or_else(|| {
                AgentMemoryError::InvalidConfig("agent memory is not configured".to_string())
            })?;
        let records = provider.recall(request).await?;
        // §9.2: explicit recall reads mark usage mechanically. Telemetry
        // never fails the read — providers without usage support
        // (markdown) return Unsupported, which is downgraded here.
        if !records.is_empty() {
            let ids: Vec<MemoryId> = records
                .iter()
                .map(|record| record.memory_id.clone())
                .collect();
            if let Err(err) = provider.mark_usage(&ids, UsageEvent::ExplicitRecall).await {
                tracing::debug!(error = %err, "agent memory explicit-recall usage marking skipped");
            }
        }
        Ok(records)
    }

    /// Attach the roster provider reset should consult for current specs.
    pub fn set_reset_roster_provider(&self, provider: Option<Arc<dyn RosterProvider>>) {
        self.set_reset_roster_provider_context(provider, None);
    }

    /// Attach the roster provider and context reset should consult for current specs.
    pub fn set_reset_roster_provider_context(
        &self,
        provider: Option<Arc<dyn RosterProvider>>,
        mob_definition: Option<meerkat_mob::MobDefinition>,
    ) {
        let source = provider.map(|provider| ResetRosterSource {
            provider,
            mob_definition,
        });
        match self.reset_roster_source.write() {
            Ok(mut stored_source) => *stored_source = source,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "identity runtime reset roster source lock poisoned; dropping provider update"
                );
            }
        }
    }

    async fn adopt_current_roster_spec_for_reset(&self, identity: &AgentIdentity) {
        let source = match self.reset_roster_source.read() {
            Ok(stored_source) => stored_source.clone(),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "reset: roster source lock poisoned; rebuilding on stored spec"
                );
                None
            }
        };
        if let Some(source) = source {
            self.adopt_roster_spec_with_context(
                &source.provider,
                identity,
                source.mob_definition.clone(),
            )
            .await;
        }
    }

    /// Attach a best-effort operational error hook used for alerting.
    pub fn set_error_hook(&self, hook: Option<crate::unified_runtime::ErrorHook>) {
        match self.error_hook.write() {
            Ok(mut stored_hook) => *stored_hook = hook,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "identity runtime error hook lock poisoned; dropping hook update"
                );
            }
        }
    }

    /// Spawn a background supervisor that renews active identity leases before
    /// they reach their TTL deadline.
    pub fn spawn_lease_renewal_task(self: Arc<Self>) -> JoinHandle<()> {
        self.spawn_lease_renewal_task_with_poll_interval(DEFAULT_LEASE_RENEWAL_MAX_POLL_INTERVAL)
    }

    /// Spawn the runtime-owned renewal supervisor. Unlike the public
    /// fire-and-forget helper, this handle cooperatively cancels while idle
    /// and joins any in-flight renewal through provider, continuity-store,
    /// bridge, and local-publication commit boundaries.
    pub(crate) fn spawn_tracked_lease_renewal_task(self: Arc<Self>) -> TrackedLeaseRenewalTask {
        self.spawn_tracked_lease_renewal_task_with_poll_interval(
            DEFAULT_LEASE_RENEWAL_MAX_POLL_INTERVAL,
        )
    }

    pub(crate) fn spawn_tracked_lease_renewal_task_with_poll_interval(
        self: Arc<Self>,
        max_poll_interval: Duration,
    ) -> TrackedLeaseRenewalTask {
        let (cancel, receiver) = watch::channel(false);
        let join = tokio::spawn(self.run_lease_renewal_loop(max_poll_interval, Some(receiver)));
        TrackedLeaseRenewalTask { cancel, join }
    }

    /// Spawn a lease renewal supervisor with a caller-provided maximum poll
    /// interval. Embedders can use this for shorter external lease TTLs; tests
    /// use it to exercise renewal without waiting on wall-clock TTLs.
    pub fn spawn_lease_renewal_task_with_poll_interval(
        self: Arc<Self>,
        max_poll_interval: Duration,
    ) -> JoinHandle<()> {
        tokio::spawn(self.run_lease_renewal_loop(max_poll_interval, None))
    }

    async fn run_lease_renewal_loop(
        self: Arc<Self>,
        max_poll_interval: Duration,
        mut cancellation: Option<watch::Receiver<bool>>,
    ) {
        let max_poll_interval = max_poll_interval.max(DEFAULT_LEASE_RENEWAL_MIN_POLL_INTERVAL);
        let mut consecutive_failures: u32 = 0;
        loop {
            if cancellation
                .as_ref()
                .is_some_and(|cancellation| *cancellation.borrow())
            {
                return;
            }
            let base = self.lease_renewal_sleep_interval(max_poll_interval).await;
            // While the provider is failing, hold off at least the backoff
            // delay so a persistent outage retries at a bounded rate instead
            // of the TTL-derived floor (down to 10ms).
            let sleep = if consecutive_failures > 0 {
                base.max(lease_renewal_failure_backoff(
                    consecutive_failures,
                    max_poll_interval,
                ))
            } else {
                base
            };
            if let Some(cancellation) = cancellation.as_mut() {
                tokio::select! {
                    () = tokio::time::sleep(sleep) => {}
                    () = self.lease_renewal_notify.notified() => {}
                    changed = cancellation.changed() => {
                        match changed {
                            Ok(()) if *cancellation.borrow() => return,
                            Ok(()) => continue,
                            Err(_) => return,
                        }
                    }
                }
            } else {
                tokio::select! {
                    () = tokio::time::sleep(sleep) => {}
                    () = self.lease_renewal_notify.notified() => {}
                }
            }

            // Do not select cancellation against this future. Once renewal
            // has entered the provider, the returned token may already be
            // authoritative; the runtime must publish that exact grant before
            // shutdown performs its final release.
            match self.renew_due_leases_once().await {
                Ok(_) => consecutive_failures = 0,
                Err(err) => {
                    // Warn once, then debounce to debug so a backend outage
                    // can't flood the log at the renewal cadence.
                    if consecutive_failures == 0 {
                        tracing::warn!(
                            error = %err,
                            "identity-first proactive lease renewal tick failed; backing off"
                        );
                    } else {
                        tracing::debug!(
                            error = %err,
                            consecutive_failures,
                            "identity-first lease renewal still failing; backing off"
                        );
                    }
                    consecutive_failures = consecutive_failures.saturating_add(1);
                }
            }
            if cancellation
                .as_ref()
                .is_some_and(|cancellation| *cancellation.borrow())
            {
                return;
            }
        }
    }

    async fn lease_renewal_sleep_interval(&self, max_poll_interval: Duration) -> Duration {
        let entries = self.entries.read().await;
        entries
            .values()
            .filter(|entry| entry.state == IdentityLifecycleState::Active)
            .filter_map(|entry| entry.lease.as_ref())
            .map(|lease| (lease.ttl / 10).max(DEFAULT_LEASE_RENEWAL_MIN_POLL_INTERVAL))
            .min()
            .unwrap_or(max_poll_interval)
            .min(max_poll_interval)
    }

    /// Renew every active lease that has entered the runtime's renewal window.
    pub async fn renew_due_leases_once(&self) -> Result<usize, IdentityRuntimeError> {
        let due = {
            let entries = self.entries.read().await;
            entries
                .iter()
                .filter(|(_, entry)| entry.state == IdentityLifecycleState::Active)
                .filter_map(|(identity, entry)| {
                    entry
                        .lease
                        .as_ref()
                        .filter(|lease| !lease.is_healthy())
                        .map(|_| identity.clone())
                })
                .collect::<Vec<_>>()
        };

        let mut renewed = 0;
        let mut first_error = None;
        for identity in due {
            let lifecycle_lock = self.lifecycle_lock_for(&identity).await;
            let _lifecycle_guard = lifecycle_lock.lock().await;
            match self.ensure_active_lease(&identity).await {
                Ok(_) => renewed += 1,
                Err(err) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }
        if let Some(err) = first_error {
            Err(err)
        } else {
            Ok(renewed)
        }
    }

    async fn release_uninstalled_materialize_lease(&self, grant: &LeaseGrant) -> Option<String> {
        self.release_or_park_untracked_leases(std::slice::from_ref(grant))
            .await
            .err()
            .map(|err| err.to_string())
    }

    async fn cancel_uninstalled_background_materialization(
        &self,
        grant: &LeaseGrant,
    ) -> IdentityRuntimeError {
        let cleanup_error = self.release_uninstalled_materialize_lease(grant).await;
        IdentityRuntimeError::Internal(
            cleanup_error
                .map(|error| format!("{BACKGROUND_WARM_CANCELLED}; lease cleanup failed: {error}"))
                .unwrap_or_else(|| BACKGROUND_WARM_CANCELLED.to_string()),
        )
    }

    pub async fn set_desired_peer_edges(&self, edges: Vec<ManagedPeerEdge>) {
        *self.desired_peer_edges.write().await = edges;
    }

    pub(crate) fn set_topology_controller(
        &self,
        controller: crate::topology_control::TopologyController,
    ) {
        *self
            .topology_controller
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(controller);
    }

    fn topology_controller(&self) -> Option<crate::topology_control::TopologyController> {
        self.topology_controller
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub async fn desired_peer_edges(&self) -> Vec<ManagedPeerEdge> {
        let declared = self.desired_peer_edges.read().await.clone();
        match self.topology_controller() {
            Some(controller) => {
                // Materialization/console reads must not observe the target
                // intent while a topology transaction is between WAL and
                // terminal commit/rollback.
                let _admission = controller.mutation_guard().await;
                match controller.compose_managed_peer_edges(&declared).await {
                    Ok(edges) => edges,
                    Err(error) => {
                        tracing::error!(error = %error, "failed to compose identity topology overlay");
                        Vec::new()
                    }
                }
            }
            None => declared,
        }
    }

    async fn registered_identities(&self) -> Vec<AgentIdentity> {
        self.entries.read().await.keys().cloned().collect()
    }

    async fn reachable_peer_identities(&self, identity: &AgentIdentity) -> Vec<AgentIdentity> {
        self.desired_peer_edges()
            .await
            .iter()
            .filter_map(|edge| {
                if edge.a() == identity {
                    Some(edge.b().clone())
                } else if edge.b() == identity {
                    Some(edge.a().clone())
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    #[must_use]
    pub fn has_session_bridge(&self) -> bool {
        self.bridge.is_some()
    }

    pub(crate) async fn logical_peer_edges(
        &self,
    ) -> Result<Vec<ManagedPeerEdge>, IdentityRuntimeError> {
        let Some(bridge) = self.bridge.as_ref() else {
            return Ok(Vec::new());
        };
        let runtime_identities: BTreeMap<AgentRuntimeId, AgentIdentity> = self
            .entries
            .read()
            .await
            .iter()
            .filter_map(|(identity, entry)| {
                entry
                    .continuity
                    .as_ref()
                    .map(|record| (record.agent_runtime_id.clone(), identity.clone()))
            })
            .collect();
        let runtime_edges = bridge.current_member_wires().await.map_err(|error| {
            IdentityRuntimeError::Internal(format!("bridge current_member_wires: {error}"))
        })?;
        Ok(runtime_edges
            .into_iter()
            .filter_map(|(runtime_a, runtime_b)| {
                let a = runtime_identities.get(&runtime_a)?.clone();
                let b = runtime_identities.get(&runtime_b)?.clone();
                ManagedPeerEdge::new(a, b).ok()
            })
            .collect())
    }

    pub(crate) async fn logical_peer_edges_any_half(
        &self,
    ) -> Result<Vec<ManagedPeerEdge>, IdentityRuntimeError> {
        let Some(bridge) = self.bridge.as_ref() else {
            return Ok(Vec::new());
        };
        let runtime_identities: BTreeMap<AgentRuntimeId, AgentIdentity> = self
            .entries
            .read()
            .await
            .iter()
            .filter_map(|(identity, entry)| {
                entry
                    .continuity
                    .as_ref()
                    .map(|record| (record.agent_runtime_id.clone(), identity.clone()))
            })
            .collect();
        let runtime_edges = bridge
            .current_member_wires_any_half()
            .await
            .map_err(|error| {
                IdentityRuntimeError::Internal(format!(
                    "bridge current_member_wires_any_half: {error}"
                ))
            })?;
        Ok(runtime_edges
            .into_iter()
            .filter_map(|(runtime_a, runtime_b)| {
                let a = runtime_identities.get(&runtime_a)?.clone();
                let b = runtime_identities.get(&runtime_b)?.clone();
                ManagedPeerEdge::new(a, b).ok()
            })
            .collect())
    }

    pub(crate) async fn managed_peer_edges_snapshot(
        &self,
    ) -> BTreeSet<(AgentIdentity, AgentIdentity)> {
        self.managed_peer_edges.read().await.clone()
    }

    pub(crate) async fn retain_managed_peer_edges(
        &self,
        edges: &BTreeSet<(AgentIdentity, AgentIdentity)>,
    ) {
        self.managed_peer_edges
            .write()
            .await
            .extend(edges.iter().cloned());
    }

    /// Logical identity actuator used only while the shared topology
    /// controller's admission lock is already held by TopologyRuntimeHandle.
    pub(crate) async fn mutate_managed_peer_edge_admitted(
        &self,
        action: crate::topology_control::TopologyAction,
        edge: &ManagedPeerEdge,
    ) -> Result<(), IdentityRuntimeError> {
        let _guard = self.managed_peer_reconcile_lock.lock().await;
        let _lifecycle_guards = self
            .lifecycle_guards_for([edge.a().clone(), edge.b().clone()])
            .await;
        let Some(bridge) = self.bridge.clone() else {
            return Err(IdentityRuntimeError::Internal(
                "topology mutation requires a session bridge".to_string(),
            ));
        };
        let (runtime_a, runtime_b) = {
            let entries = self.entries.read().await;
            let resolve = |identity: &AgentIdentity| {
                entries
                    .get(identity)
                    .filter(|entry| entry.state == IdentityLifecycleState::Active)
                    .and_then(|entry| entry.continuity.as_ref())
                    .map(|record| record.agent_runtime_id.clone())
                    .ok_or_else(|| {
                        IdentityRuntimeError::Internal(format!(
                            "topology endpoint is not active: {identity}"
                        ))
                    })
            };
            (resolve(edge.a())?, resolve(edge.b())?)
        };
        let key = (edge.a().clone(), edge.b().clone());
        let current = bridge.current_member_wires().await.map_err(|error| {
            IdentityRuntimeError::Internal(format!("bridge current_member_wires: {error}"))
        })?;
        let actual = current.iter().any(|(a, b)| {
            (a == &runtime_a && b == &runtime_b) || (a == &runtime_b && b == &runtime_a)
        });
        let any_half = bridge
            .current_member_wires_any_half()
            .await
            .map_err(|error| {
                IdentityRuntimeError::Internal(format!(
                    "bridge current_member_wires_any_half: {error}"
                ))
            })?
            .iter()
            .any(|(a, b)| {
                (a == &runtime_a && b == &runtime_b) || (a == &runtime_b && b == &runtime_a)
            });
        match action {
            crate::topology_control::TopologyAction::Connect if !actual => bridge
                .wire_peers_batch(&[(runtime_a, runtime_b)])
                .await
                .map_err(|error| {
                    IdentityRuntimeError::Internal(format!("bridge wire_peers_batch: {error}"))
                })?,
            crate::topology_control::TopologyAction::Reconnect => {
                if any_half {
                    bridge
                        .unwire_peer(&runtime_a, &runtime_b)
                        .await
                        .map_err(|error| {
                            IdentityRuntimeError::Internal(format!("bridge unwire_peer: {error}"))
                        })?;
                }
                bridge
                    .wire_peers_batch(&[(runtime_a, runtime_b)])
                    .await
                    .map_err(|error| {
                        IdentityRuntimeError::Internal(format!("bridge wire_peers_batch: {error}"))
                    })?;
            }
            crate::topology_control::TopologyAction::Disconnect if any_half => bridge
                .unwire_peer(&runtime_a, &runtime_b)
                .await
                .map_err(|error| {
                    IdentityRuntimeError::Internal(format!("bridge unwire_peer: {error}"))
                })?,
            _ => {}
        }
        let mut managed = self.managed_peer_edges.write().await;
        if matches!(action, crate::topology_control::TopologyAction::Disconnect) {
            managed.remove(&key);
        } else {
            managed.insert(key);
        }
        Ok(())
    }

    /// Apply identity-first managed topology to the concrete mob graph.
    ///
    /// Topology providers return stable logical identities. The mob comms graph
    /// is keyed by active runtime member IDs, so this resolves each endpoint
    /// through continuity records before calling the same-mob bridge wire APIs.
    pub async fn reconcile_managed_peer_edges(
        &self,
        desired_edges: &[ManagedPeerEdge],
    ) -> Result<(), IdentityRuntimeError> {
        let topology_controller = self.topology_controller();
        let _topology_guard = match topology_controller.as_ref() {
            Some(controller) => Some(controller.mutation_guard().await),
            None => None,
        };
        if let Some(controller) = topology_controller.as_ref() {
            controller
                .prepare_pending_recovery()
                .await
                .map_err(|error| {
                    IdentityRuntimeError::Internal(format!("topology recovery journal: {error}"))
                })?;
        }
        let pending_recovery_edges = if let Some(controller) = topology_controller.as_ref() {
            controller
                .pending_local_recovery_edges()
                .await
                .map_err(|error| {
                    IdentityRuntimeError::Internal(format!("topology recovery ownership: {error}"))
                })?
        } else {
            BTreeSet::new()
        };
        if !pending_recovery_edges.is_empty() {
            self.retain_managed_peer_edges(&pending_recovery_edges)
                .await;
        }
        let composed_edges;
        let desired_edges = if let Some(controller) = topology_controller.as_ref() {
            composed_edges = controller
                .compose_managed_peer_edges(desired_edges)
                .await
                .map_err(|error| {
                    IdentityRuntimeError::Internal(format!("topology overlay: {error}"))
                })?;
            composed_edges.as_slice()
        } else {
            desired_edges
        };
        let result = self
            .reconcile_managed_peer_edges_admitted(desired_edges)
            .await;
        let mut recovery_inspection_error = None;
        if let Some(controller) = topology_controller.as_ref() {
            let recovery_complete = if result.is_ok() && !pending_recovery_edges.is_empty() {
                match self
                    .pending_recovery_is_physically_complete(desired_edges, &pending_recovery_edges)
                    .await
                {
                    Ok(complete) => complete,
                    Err(error) => {
                        recovery_inspection_error = Some(error);
                        false
                    }
                }
            } else {
                false
            };
            controller
                .finalize_recovered_pending(result.is_ok() && recovery_complete)
                .await
                .map_err(|error| {
                    IdentityRuntimeError::Internal(format!("topology recovery receipt: {error}"))
                })?;
        }
        result?;
        if let Some(error) = recovery_inspection_error {
            return Err(error);
        }
        Ok(())
    }

    pub(crate) async fn pending_recovery_is_physically_complete(
        &self,
        desired_edges: &[ManagedPeerEdge],
        pending_recovery_edges: &BTreeSet<(AgentIdentity, AgentIdentity)>,
    ) -> Result<bool, IdentityRuntimeError> {
        if self.bridge.is_none() {
            return Ok(false);
        }
        let active_identities = self
            .entries
            .read()
            .await
            .iter()
            .filter_map(|(identity, entry)| {
                (entry.state == IdentityLifecycleState::Active && entry.continuity.is_some())
                    .then_some(identity.clone())
            })
            .collect::<BTreeSet<_>>();
        if pending_recovery_edges
            .iter()
            .any(|(a, b)| !active_identities.contains(a) || !active_identities.contains(b))
        {
            return Ok(false);
        }
        let desired = desired_edges
            .iter()
            .map(|edge| (edge.a().clone(), edge.b().clone()))
            .collect::<BTreeSet<_>>();
        let actual = self
            .logical_peer_edges()
            .await?
            .into_iter()
            .map(|edge| (edge.a().clone(), edge.b().clone()))
            .collect::<BTreeSet<_>>();
        let actual_any_half = self
            .logical_peer_edges_any_half()
            .await?
            .into_iter()
            .map(|edge| (edge.a().clone(), edge.b().clone()))
            .collect::<BTreeSet<_>>();

        Ok(desired.is_subset(&actual)
            && pending_recovery_edges
                .difference(&desired)
                .all(|edge| !actual_any_half.contains(edge)))
    }

    /// Reconcile an already-composed desired topology while the caller holds
    /// the shared topology-controller admission guard.
    pub(crate) async fn reconcile_managed_peer_edges_admitted(
        &self,
        desired_edges: &[ManagedPeerEdge],
    ) -> Result<(), IdentityRuntimeError> {
        let _guard = self.managed_peer_reconcile_lock.lock().await;
        let Some(bridge) = self.bridge.clone() else {
            return Ok(());
        };

        let managed_snapshot = self.managed_peer_edges.read().await.clone();
        let topology_identities = desired_edges
            .iter()
            .flat_map(|edge| [edge.a().clone(), edge.b().clone()])
            .chain(
                managed_snapshot
                    .iter()
                    .flat_map(|(a, b)| [a.clone(), b.clone()]),
            );
        let _lifecycle_guards = self.lifecycle_guards_for(topology_identities).await;

        let (known_runtimes, active_runtimes): (
            BTreeMap<AgentIdentity, AgentRuntimeId>,
            BTreeMap<AgentIdentity, AgentRuntimeId>,
        ) = {
            let entries = self.entries.read().await;
            let known = entries
                .iter()
                .filter_map(|(identity, entry)| {
                    entry
                        .continuity
                        .as_ref()
                        .map(|record| (identity.clone(), record.agent_runtime_id.clone()))
                })
                .collect::<BTreeMap<_, _>>();
            let active = entries
                .iter()
                .filter_map(|(identity, entry)| {
                    if entry.state != IdentityLifecycleState::Active {
                        return None;
                    }
                    entry
                        .continuity
                        .as_ref()
                        .map(|record| (identity.clone(), record.agent_runtime_id.clone()))
                })
                .collect();
            (known, active)
        };
        let runtime_identities: BTreeMap<AgentRuntimeId, AgentIdentity> = known_runtimes
            .iter()
            .map(|(identity, runtime_id)| (runtime_id.clone(), identity.clone()))
            .collect();
        let current_logical_edges: Option<BTreeSet<(AgentIdentity, AgentIdentity)>> =
            match bridge.current_member_wires().await {
                Ok(current_runtime_edges) => Some(
                    current_runtime_edges
                        .iter()
                        .filter_map(|(runtime_a, runtime_b)| {
                            let a = runtime_identities.get(runtime_a)?;
                            let b = runtime_identities.get(runtime_b)?;
                            if a <= b {
                                Some((a.clone(), b.clone()))
                            } else {
                                Some((b.clone(), a.clone()))
                            }
                        })
                        .collect(),
                ),
                Err(err) => {
                    tracing::debug!(
                        error = %err,
                        "identity-first topology reconcile could not inspect current member wires"
                    );
                    None
                }
            };
        let current_any_half_edges: Option<BTreeSet<(AgentIdentity, AgentIdentity)>> =
            match bridge.current_member_wires_any_half().await {
                Ok(current_runtime_edges) => Some(
                    current_runtime_edges
                        .iter()
                        .filter_map(|(runtime_a, runtime_b)| {
                            let a = runtime_identities.get(runtime_a)?;
                            let b = runtime_identities.get(runtime_b)?;
                            if a <= b {
                                Some((a.clone(), b.clone()))
                            } else {
                                Some((b.clone(), a.clone()))
                            }
                        })
                        .collect(),
                ),
                Err(error) => {
                    tracing::debug!(
                        %error,
                        "identity-first topology reconcile could not inspect orphan wire halves"
                    );
                    None
                }
            };

        let desired: BTreeSet<(AgentIdentity, AgentIdentity)> = desired_edges
            .iter()
            .map(|edge| (edge.a().clone(), edge.b().clone()))
            .collect();

        let edge_is_managed_and_live = |edge: &(AgentIdentity, AgentIdentity)| {
            // Managed-but-missing live edges are retried deliberately so tolerant topology restores self-heal.
            managed_snapshot.contains(edge)
                && current_logical_edges
                    .as_ref()
                    .is_none_or(|edges| edges.contains(edge))
        };
        let retained_logical_edges: Vec<(AgentIdentity, AgentIdentity)> = desired
            .iter()
            .filter(|edge| !edge_is_managed_and_live(edge))
            .filter(|edge| {
                current_logical_edges
                    .as_ref()
                    .is_some_and(|edges| edges.contains(*edge))
            })
            .filter(|(a, b)| active_runtimes.contains_key(a) && active_runtimes.contains_key(b))
            .cloned()
            .collect();
        let to_wire: Vec<(AgentIdentity, AgentIdentity, AgentRuntimeId, AgentRuntimeId)> = desired
            .iter()
            .filter(|edge| !edge_is_managed_and_live(edge))
            .filter(|edge| {
                current_logical_edges
                    .as_ref()
                    .is_none_or(|edges| !edges.contains(*edge))
            })
            .filter_map(|(a, b)| {
                let runtime_a = active_runtimes.get(a)?;
                let runtime_b = active_runtimes.get(b)?;
                Some((a.clone(), b.clone(), runtime_a.clone(), runtime_b.clone()))
            })
            .collect();

        let stale: Vec<(AgentIdentity, AgentIdentity)> = managed_snapshot
            .iter()
            .filter(|edge| !desired.contains(*edge))
            .cloned()
            .collect();
        let to_unwire: Vec<(AgentIdentity, AgentIdentity, AgentRuntimeId, AgentRuntimeId)> = stale
            .iter()
            .filter_map(|(a, b)| {
                let runtime_a = active_runtimes.get(a)?;
                let runtime_b = active_runtimes.get(b)?;
                if current_any_half_edges
                    .as_ref()
                    .is_some_and(|edges| !edges.contains(&(a.clone(), b.clone())))
                {
                    return None;
                }
                Some((a.clone(), b.clone(), runtime_a.clone(), runtime_b.clone()))
            })
            .collect();

        let wire_logical_edges = to_wire
            .iter()
            .map(|(a, b, _, _)| (a.clone(), b.clone()))
            .collect::<Vec<_>>();
        let wire_runtime_edges = to_wire
            .iter()
            .map(|(_, _, runtime_a, runtime_b)| (runtime_a.clone(), runtime_b.clone()))
            .collect::<Vec<_>>();
        if !wire_runtime_edges.is_empty() {
            bridge
                .wire_peers_batch(&wire_runtime_edges)
                .await
                .map_err(|e| {
                    IdentityRuntimeError::Internal(format!("bridge wire_peers_batch: {e}"))
                })?;
        }

        let unwire_results =
            stream::iter(to_unwire.into_iter().map(|(a, b, runtime_a, runtime_b)| {
                let bridge = bridge.clone();
                async move {
                    let result = bridge
                        .unwire_peer(&runtime_a, &runtime_b)
                        .await
                        .map_err(|e| format!("{e}"));
                    (a, b, result)
                }
            }))
            .buffer_unordered(MANAGED_PEER_RECONCILE_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

        let mut managed = self.managed_peer_edges.write().await;
        for (a, b) in retained_logical_edges {
            managed.insert((a, b));
        }
        for (a, b) in wire_logical_edges {
            managed.insert((a, b));
        }

        for (a, b) in stale {
            let key = (a.clone(), b.clone());
            if current_any_half_edges
                .as_ref()
                .is_some_and(|edges| !edges.contains(&key))
            {
                managed.remove(&key);
            }
        }
        for (a, b, result) in unwire_results {
            result
                .map_err(|e| IdentityRuntimeError::Internal(format!("bridge unwire_peer: {e}")))?;
            managed.remove(&(a, b));
        }

        Ok(())
    }

    /// Emit an event for the given identity. Best-effort — no error if no subscribers.
    async fn emit_event(&self, identity: &AgentIdentity, event: IdentityEvent) {
        let channels = self.event_channels.read().await;
        if let Some(tx) = channels.get(identity) {
            let _ = tx.send(event);
        }
    }

    fn emit_error(&self, event: crate::unified_runtime::types::ErrorEvent) {
        let hook = match self.error_hook.read() {
            Ok(stored_hook) => stored_hook.clone(),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "identity runtime error hook lock poisoned; dropping error event"
                );
                None
            }
        };
        // The identity runtime owns a hook slot of its own, so it is a second
        // place an ErrorEvent can vanish when no host wired one. Same default
        // sink as the unified-runtime fire point.
        crate::unified_runtime::log_error_event(&event, hook.is_some());
        if let Some(hook) = hook {
            tokio::spawn(async move {
                let () = hook(event).await;
            });
        }
    }

    async fn materialization_backoff_error(&self, identity: &AgentIdentity) -> Option<String> {
        let backoffs = self.materialization_failure_backoff.read().await;
        let backoff = backoffs.get(identity)?;
        if Instant::now() < backoff.suppress_until {
            Some(backoff.error.clone())
        } else {
            None
        }
    }

    async fn clear_materialization_backoff(&self, identity: &AgentIdentity) {
        self.materialization_failure_backoff
            .write()
            .await
            .remove(identity);
    }

    async fn record_best_effort_materialization_failure(
        &self,
        identity: &AgentIdentity,
        initiator: Option<&AgentIdentity>,
        operation: &'static str,
        err: &IdentityRuntimeError,
    ) {
        let error = err.to_string();
        let suppress_until = Instant::now() + MATERIALIZATION_FAILURE_BACKOFF;
        self.materialization_failure_backoff.write().await.insert(
            identity.clone(),
            MaterializationFailureBackoff {
                suppress_until,
                error: error.clone(),
            },
        );
        self.emit_error(
            crate::unified_runtime::types::ErrorEvent::IdentityMaterializationFailure {
                identity: identity.to_string(),
                initiator: initiator.map(ToString::to_string),
                operation: operation.to_string(),
                error,
            },
        );
    }

    // -----------------------------------------------------------------------
    // Registration / activation
    // -----------------------------------------------------------------------

    /// Register an identity entry in the runtime (called during restore flow).
    pub async fn register(
        &self,
        spec: DurableAgentSpec,
        state: IdentityLifecycleState,
        continuity: Option<ContinuityRecord>,
        lease: Option<LeaseGrant>,
    ) {
        let identity = spec.identity.clone();
        let bootstrap_generation = self.identity_bootstrap_status_with_generation().0;
        let cpv = continuity
            .as_ref()
            .map(|r| r.checkpoint_version)
            .unwrap_or(CheckpointVersion::new(0));
        let lease_entry = lease.map(|g| LeaseEntry {
            fencing_token: g.fencing_token,
            ttl: g.ttl,
            acquired_at: Instant::now(),
        });
        let has_active_lease = state == IdentityLifecycleState::Active && lease_entry.is_some();
        {
            let mut entries = self.entries.write().await;
            // A terminal heal verdict is about the durable head, not this
            // entry instance: re-projecting Broken (a repair retry, an eager
            // reconcile) must not silently forget it, while any non-Broken
            // projection is a real lifecycle transition that supersedes it
            // (2026-07-29 heal/re-Break incident).
            let continuity_unrecoverable = if state == IdentityLifecycleState::Broken {
                entries
                    .get(&identity)
                    .and_then(|existing| existing.continuity_unrecoverable.clone())
            } else {
                None
            };
            // A host-rejected-build park is about the SPEC, not this entry
            // instance: re-registration (a reconcile pass, a repair retry)
            // with the same spec must not forget it — retrying an unchanged
            // spec against a deterministic gate re-burns a build + callback
            // round trip for the same answer. A changed spec clears it.
            let host_rejected_build_park = entries.get(&identity).and_then(|existing| {
                existing
                    .host_rejected_build_park
                    .clone()
                    .filter(|park| park.spec_digest == durable_spec_digest(&spec))
            });
            let entry = IdentityEntry {
                spec,
                bootstrap_generation,
                state,
                continuity,
                lease: lease_entry,
                pending_lease_release: None,
                checkpoint_version: cpv,
                has_runtime_store: self.has_runtime_store,
                continuity_unrecoverable,
                host_rejected_build_park,
            };
            entries.insert(identity.clone(), entry);
        }

        // Create event channel for this identity
        let (tx, _) = broadcast::channel(IDENTITY_EVENT_CHANNEL_CAPACITY);
        self.event_channels
            .write()
            .await
            .insert(identity.clone(), tx);
        if has_active_lease {
            self.lease_renewal_notify.notify_one();
        }
        self.mark_bootstrap_from_lifecycle(&identity, state, None);
    }

    async fn materialization_lock_for(&self, identity: &AgentIdentity) -> Arc<Mutex<()>> {
        if let Some(lock) = self.materialization_locks.read().await.get(identity) {
            return lock.clone();
        }
        let mut locks = self.materialization_locks.write().await;
        locks
            .entry(identity.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn best_effort_materialization_lock_for(
        &self,
        identity: &AgentIdentity,
    ) -> Arc<Mutex<()>> {
        if let Some(lock) = self
            .best_effort_materialization_locks
            .read()
            .await
            .get(identity)
        {
            return lock.clone();
        }
        let mut locks = self.best_effort_materialization_locks.write().await;
        locks
            .entry(identity.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub(crate) async fn lifecycle_lock_for(&self, identity: &AgentIdentity) -> Arc<Mutex<()>> {
        if let Some(lock) = self.lifecycle_locks.read().await.get(identity) {
            return lock.clone();
        }
        let mut locks = self.lifecycle_locks.write().await;
        locks
            .entry(identity.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub(crate) async fn raw_member_alias_lock(&self, alias: &str) -> Arc<Mutex<()>> {
        let alias = crate::member_comms_id::runtime_alias_str(alias.trim()).into_owned();
        if let Some(lock) = self
            .raw_member_alias_locks
            .read()
            .await
            .entries
            .get(&alias)
            .and_then(Weak::upgrade)
        {
            return lock;
        }
        let mut table = self.raw_member_alias_locks.write().await;
        table.sweep_if_needed();
        // Another caller may have installed the key after our read miss. The
        // write-side upgrade is the authority boundary that prevents two live
        // mutexes from serializing the same alias independently.
        if let Some(lock) = table.entries.get(&alias).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Mutex::new(()));
        table.entries.insert(alias, Arc::downgrade(&lock));
        lock
    }

    #[cfg(test)]
    pub(crate) async fn raw_member_alias_lock_metrics(&self) -> (usize, usize, usize) {
        let table = self.raw_member_alias_locks.read().await;
        (table.entries.len(), table.next_sweep_len, table.sweep_count)
    }

    pub(crate) async fn ensure_raw_member_alias_available(
        &self,
        identity: &AgentIdentity,
    ) -> Result<(), IdentityRuntimeError> {
        if let Some(bridge) = self.bridge.as_ref()
            && bridge
                .raw_member_alias_exists(identity.as_str())
                .await
                .map_err(|error| {
                    IdentityRuntimeError::Internal(format!(
                        "raw member namespace inspection for {identity}: {error}"
                    ))
                })?
        {
            return Err(IdentityRuntimeError::Internal(format!(
                "durable identity '{identity}' collides with an existing raw member alias"
            )));
        }
        Ok(())
    }

    /// Resolve an alias into a lifecycle-lock target. Classic members return
    /// `None`; generated aliases always resolve to an authority target even
    /// after deletion so validation under the lock fails closed.
    pub(crate) async fn member_alias_lifecycle_target(
        self: &Arc<Self>,
        alias: &str,
    ) -> Result<Option<MemberAliasLifecycleTarget>, IdentityRuntimeError> {
        let alias = crate::member_comms_id::runtime_alias_str(alias).into_owned();
        match self.identity_for_member_mutation(&alias).await {
            Some(identity) => {
                // Fail stale/deleted generated aliases before unrelated
                // operation prerequisites (for example cross-mob directory
                // lookup). The tracked operation validates again after taking
                // the lifecycle lock, so this preflight does not become the
                // authority boundary or introduce a TOCTOU gap.
                self.ensure_expected_member_alias_current(&identity, &alias)
                    .await?;
                Ok(Some(MemberAliasLifecycleTarget {
                    runtime: Arc::clone(self),
                    lock: self.lifecycle_lock_for(&identity).await,
                    identity,
                    alias,
                }))
            }
            None if crate::member_comms_id::is_reserved_generated_alias(&alias) => {
                Err(IdentityRuntimeError::Internal(format!(
                    "generated member alias requires identity authority: {alias}"
                )))
            }
            None => Ok(None),
        }
    }

    /// Acquire member-alias lifecycle targets across one or more identity
    /// runtimes in a deterministic process-global order, then validate every
    /// alias while its owning lock is held. Duplicate identities acquire one
    /// lock but still validate every spelling/generation.
    pub(crate) async fn acquire_member_alias_lifecycle_targets(
        mut targets: Vec<MemberAliasLifecycleTarget>,
    ) -> Result<Vec<tokio::sync::OwnedMutexGuard<()>>, IdentityRuntimeError> {
        targets.sort_by(|a, b| {
            a.runtime
                .runtime_instance_id
                .cmp(&b.runtime.runtime_instance_id)
                .then_with(|| {
                    (Arc::as_ptr(&a.runtime) as usize).cmp(&(Arc::as_ptr(&b.runtime) as usize))
                })
                .then_with(|| a.identity.cmp(&b.identity))
                .then_with(|| a.alias.cmp(&b.alias))
        });

        let mut guards = Vec::with_capacity(targets.len());
        let mut held: Option<(usize, AgentIdentity)> = None;
        for target in targets {
            let key = (
                Arc::as_ptr(&target.runtime) as usize,
                target.identity.clone(),
            );
            if held.as_ref() != Some(&key) {
                guards.push(target.lock.lock_owned().await);
                held = Some(key);
            }
            target
                .runtime
                .ensure_expected_member_alias_current(&target.identity, &target.alias)
                .await?;
            let state = target
                .runtime
                .entries
                .read()
                .await
                .get(&target.identity)
                .map(|entry| entry.state)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(target.identity.clone()))?;
            if state != IdentityLifecycleState::Active {
                return Err(IdentityRuntimeError::InvalidState {
                    identity: target.identity,
                    state,
                    operation: "mutate member alias",
                });
            }
        }
        Ok(guards)
    }

    /// Cancellation-safe operation spanning one or more alias targets. The
    /// first target in global order supervises the transaction to its explicit
    /// commit/rollback boundary if the request future is dropped.
    pub(crate) async fn run_member_alias_targets_operation_tracked<T, F, Fut>(
        targets: Vec<MemberAliasLifecycleTarget>,
        operation: F,
    ) -> Result<T, IdentityRuntimeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, String>> + Send + 'static,
    {
        let mut runtimes = targets
            .iter()
            .map(|target| Arc::clone(&target.runtime))
            .collect::<Vec<_>>();
        runtimes.sort_by(|a, b| {
            a.runtime_instance_id
                .cmp(&b.runtime_instance_id)
                .then_with(|| (Arc::as_ptr(a) as usize).cmp(&(Arc::as_ptr(b) as usize)))
        });
        runtimes.dedup_by(|a, b| Arc::ptr_eq(a, b));
        let transaction = async move {
            let _guards = Self::acquire_member_alias_lifecycle_targets(targets).await?;
            operation().await.map_err(IdentityRuntimeError::Internal)
        };
        if !runtimes.is_empty() {
            return Self::run_tracked_foreground_multi(runtimes, transaction).await;
        }
        transaction.await
    }

    /// Register one compound transaction with every participating runtime.
    /// Non-owner shutdowns wait on a completion task in their own JoinSet,
    /// while the globally first runtime owns the actual operation and result.
    async fn run_tracked_foreground_multi<T, F>(
        runtimes: Vec<Arc<Self>>,
        operation: F,
    ) -> Result<T, IdentityRuntimeError>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, IdentityRuntimeError>> + Send + 'static,
    {
        let Some((supervisor, participants)) = runtimes.split_first() else {
            return operation.await;
        };
        if runtimes
            .iter()
            .any(|runtime| runtime.foreground_shutdown.load(Ordering::Acquire))
        {
            return Err(IdentityRuntimeError::Internal(
                "identity runtime is shutting down".to_string(),
            ));
        }

        let (completion, _) = watch::channel(false);
        for runtime in participants {
            let mut operations = runtime.foreground_operations.lock().await;
            if runtime.foreground_shutdown.load(Ordering::Acquire) {
                completion.send_replace(true);
                return Err(IdentityRuntimeError::Internal(
                    "identity runtime is shutting down".to_string(),
                ));
            }
            while let Some(result) = operations.try_join_next() {
                if let Err(error) = result {
                    tracing::error!(
                        error = %error,
                        "tracked foreground identity operation panicked"
                    );
                }
            }
            let mut completed = completion.subscribe();
            operations.spawn(async move {
                while !*completed.borrow() && completed.changed().await.is_ok() {}
            });
        }

        let (sender, receiver) = oneshot::channel();
        {
            let mut operations = supervisor.foreground_operations.lock().await;
            if supervisor.foreground_shutdown.load(Ordering::Acquire) {
                completion.send_replace(true);
                return Err(IdentityRuntimeError::Internal(
                    "identity runtime is shutting down".to_string(),
                ));
            }
            while let Some(result) = operations.try_join_next() {
                if let Err(error) = result {
                    tracing::error!(
                        error = %error,
                        "tracked foreground identity operation panicked"
                    );
                }
            }
            operations.spawn(async move {
                let _completion = MultiRuntimeForegroundCompletion(completion);
                let _ = sender.send(operation.await);
            });
        }
        receiver.await.map_err(|_| {
            IdentityRuntimeError::Internal(
                "tracked foreground identity operation terminated without a result".to_string(),
            )
        })?
    }

    /// Acquire several identity lifecycle locks in stable identity order.
    /// Topology operations span two or more generated aliases; global ordering
    /// prevents opposite-direction edge requests from deadlocking.
    async fn lifecycle_guards_for(
        &self,
        identities: impl IntoIterator<Item = AgentIdentity>,
    ) -> Vec<tokio::sync::OwnedMutexGuard<()>> {
        let identities = identities.into_iter().collect::<BTreeSet<_>>();
        let mut guards = Vec::with_capacity(identities.len());
        for identity in identities {
            guards.push(self.lifecycle_lock_for(&identity).await.lock_owned().await);
        }
        guards
    }

    /// Run an externally-cancellable operation under runtime ownership.
    ///
    /// Dropping the caller only drops the result receiver; the transaction
    /// remains in the runtime's join set and reaches its explicit
    /// commit/rollback boundary. Graceful shutdown closes admission and joins
    /// every such task before lease renewal or the mob actor is stopped.
    async fn run_tracked_foreground<T, F>(
        self: &Arc<Self>,
        operation: F,
    ) -> Result<T, IdentityRuntimeError>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, IdentityRuntimeError>> + Send + 'static,
    {
        if self.foreground_shutdown.load(Ordering::Acquire) {
            return Err(IdentityRuntimeError::Internal(
                "identity runtime is shutting down".to_string(),
            ));
        }
        let (sender, receiver) = oneshot::channel();
        {
            let mut operations = self.foreground_operations.lock().await;
            if self.foreground_shutdown.load(Ordering::Acquire) {
                return Err(IdentityRuntimeError::Internal(
                    "identity runtime is shutting down".to_string(),
                ));
            }
            while let Some(result) = operations.try_join_next() {
                if let Err(error) = result {
                    tracing::error!(
                        error = %error,
                        "tracked foreground identity operation panicked"
                    );
                }
            }
            operations.spawn(async move {
                let _ = sender.send(operation.await);
            });
        }
        receiver.await.map_err(|_| {
            IdentityRuntimeError::Internal(
                "tracked foreground identity operation terminated without a result".to_string(),
            )
        })?
    }

    pub(crate) fn close_foreground_operations(&self) {
        self.foreground_shutdown.store(true, Ordering::Release);
        // `watch::Sender::send` discards the value when no receiver exists.
        // A just-admitted JoinSet task may not have subscribed yet, so retain
        // shutdown truth for future receivers explicitly.
        self.foreground_cancel.send_replace(true);
    }

    pub(crate) async fn join_foreground_operations(&self) {
        let mut operations = self.foreground_operations.lock().await;
        while let Some(result) = operations.join_next().await {
            if let Err(error) = result {
                tracing::error!(
                    error = %error,
                    "tracked foreground identity operation panicked during shutdown"
                );
            }
        }
    }

    /// Preserve exact grants acquired by a restore task that failed before it
    /// could publish an [`IdentityEntry`]. Entry-scoped pending-release state
    /// cannot represent this phase, so the runtime owns a small orphan-grant
    /// ledger until reconcile or shutdown successfully releases it.
    pub(crate) async fn park_unactivated_lease_releases(&self, grants: &[LeaseGrant]) {
        let mut pending = self.pending_unactivated_lease_releases.write().await;
        for grant in grants {
            if !pending.iter().any(|parked| {
                parked.identity == grant.identity && parked.fencing_token == grant.fencing_token
            }) {
                pending.push(grant.clone());
            }
        }
    }

    /// Release grants that do not yet have an IdentityEntry capable of
    /// retaining retry state. A provider failure atomically transfers their
    /// exact fencing tokens into the runtime-owned orphan ledger.
    pub(crate) async fn release_or_park_untracked_leases(
        &self,
        grants: &[LeaseGrant],
    ) -> Result<(), super::types::LeaseError> {
        let _release_guard = self.pending_unactivated_lease_release_gate.lock().await;
        match self.lease_provider.release_leases(grants).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.park_unactivated_lease_releases(grants).await;
                Err(error)
            }
        }
    }

    /// Retry every grant parked before lifecycle publication. This runs under
    /// the restore controller before any new batch acquisition, and again from
    /// shutdown after all identity work has joined.
    pub(crate) async fn release_parked_unactivated_leases(
        &self,
    ) -> Result<usize, IdentityRuntimeError> {
        self.release_parked_unactivated_leases_matching(None).await
    }

    /// Retry parked pre-publication grants for one identity before a direct
    /// lazy materialization attempts to acquire fresh authority. The caller
    /// holds that identity's lifecycle lock, so another same-identity path
    /// cannot park a replacement grant between this drain and acquisition.
    async fn release_parked_unactivated_leases_for_identity(
        &self,
        identity: &AgentIdentity,
    ) -> Result<usize, IdentityRuntimeError> {
        self.release_parked_unactivated_leases_matching(Some(identity))
            .await
    }

    async fn release_parked_unactivated_leases_matching(
        &self,
        identity: Option<&AgentIdentity>,
    ) -> Result<usize, IdentityRuntimeError> {
        let _release_guard = self.pending_unactivated_lease_release_gate.lock().await;
        let grants = self
            .pending_unactivated_lease_releases
            .read()
            .await
            .iter()
            .filter(|grant| identity.is_none_or(|identity| grant.identity == *identity))
            .cloned()
            .collect::<Vec<_>>();
        if grants.is_empty() {
            return Ok(0);
        }
        self.lease_provider
            .release_leases(&grants)
            .await
            .map_err(IdentityRuntimeError::Lease)?;
        let mut pending = self.pending_unactivated_lease_releases.write().await;
        pending.retain(|parked| {
            !grants.iter().any(|released| {
                released.identity == parked.identity
                    && released.fencing_token == parked.fencing_token
            })
        });
        Ok(grants.len())
    }

    /// Release every durable identity grant after the lower mob plane has
    /// quiesced. Successful release clears the in-memory authority so the
    /// method is idempotent; failures retain the exact grants for inspection
    /// or a subsequent retry.
    pub(crate) async fn release_all_leases_for_shutdown(
        &self,
    ) -> Result<usize, IdentityRuntimeError> {
        let _parked_release_guard = self.pending_unactivated_lease_release_gate.lock().await;
        let mut grants = self
            .entries
            .read()
            .await
            .iter()
            .filter_map(|(identity, entry)| {
                entry.pending_lease_release.clone().or_else(|| {
                    entry.lease.as_ref().map(|lease| LeaseGrant {
                        identity: identity.clone(),
                        fencing_token: lease.fencing_token,
                        ttl: lease.ttl,
                    })
                })
            })
            .collect::<Vec<_>>();
        let parked_grants = self.pending_unactivated_lease_releases.read().await.clone();
        for grant in parked_grants {
            if !grants.iter().any(|existing| {
                existing.identity == grant.identity && existing.fencing_token == grant.fencing_token
            }) {
                grants.push(grant);
            }
        }
        if grants.is_empty() {
            return Ok(0);
        }

        self.lease_provider
            .release_leases(&grants)
            .await
            .map_err(IdentityRuntimeError::Lease)?;

        let mut entries = self.entries.write().await;
        let mut lifecycle_updates = Vec::new();
        for grant in &grants {
            if let Some(entry) = entries.get_mut(&grant.identity) {
                let released_active_lease = entry
                    .lease
                    .as_ref()
                    .is_some_and(|lease| lease.fencing_token == grant.fencing_token);
                if released_active_lease {
                    entry.lease = None;
                }
                if entry
                    .pending_lease_release
                    .as_ref()
                    .is_some_and(|pending| pending.fencing_token == grant.fencing_token)
                {
                    entry.pending_lease_release = None;
                }
                if released_active_lease
                    && matches!(
                        entry.state,
                        IdentityLifecycleState::Active | IdentityLifecycleState::Suspended
                    )
                {
                    entry.state = IdentityLifecycleState::Dormant;
                    lifecycle_updates.push(grant.identity.clone());
                }
            }
        }
        drop(entries);
        let mut pending = self.pending_unactivated_lease_releases.write().await;
        pending.retain(|parked| {
            !grants.iter().any(|released| {
                released.identity == parked.identity
                    && released.fencing_token == parked.fencing_token
            })
        });
        drop(pending);
        for identity in lifecycle_updates {
            self.mark_bootstrap_from_lifecycle(&identity, IdentityLifecycleState::Dormant, None);
        }
        Ok(grants.len())
    }

    /// Materialize a dormant identity into a concrete mob member/session.
    ///
    /// This is the lazy counterpart to eager `restore_flow`: it performs the
    /// expensive bridge create/resume and snapshot load only when an identity is
    /// actually touched. Parallel calls for one identity coalesce on a
    /// per-identity lock and re-check state after acquiring it.
    pub async fn materialize(
        &self,
        identity: &AgentIdentity,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        self.materialize_with_expected_member_alias(identity, None)
            .await
    }

    async fn materialize_with_expected_member_alias(
        &self,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        self.materialize_with_expected_member_alias_after_inner(
            identity,
            expected_alias,
            std::future::ready(()),
        )
        .await
    }

    /// Complete a foreground materialization with an internal seam after the
    /// lifecycle transaction commits. Production callers use an immediately
    /// ready future; deterministic concurrency tests pause here to exercise
    /// superseding roster passes without adding runtime-visible hooks.
    async fn materialize_with_expected_member_alias_after_inner<F>(
        &self,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
        after_inner: F,
    ) -> Result<ContinuityRecord, IdentityRuntimeError>
    where
        F: Future<Output = ()>,
    {
        // `embody_identity` binds this attempt to the generation stored on
        // the IdentityEntry after it acquires the lifecycle lock. Sampling the
        // global generation here is insufficient: a newer reconcile can
        // publish G+1 and win that lock before this future reaches the entry,
        // in which case the materialized spec belongs to G+1, not G.
        let mut bootstrap_generation = None;
        let mut shutdown = self.foreground_cancel.subscribe();
        let result = self
            .embody_identity(
                identity,
                expected_alias,
                Some(&mut shutdown),
                None,
                &mut bootstrap_generation,
                EmbodimentOverrides::default(),
            )
            .await
            .map(|outcome| outcome.record);
        after_inner.await;
        if matches!(
            &result,
            Err(IdentityRuntimeError::Internal(message)) if message == BACKGROUND_WARM_CANCELLED
        ) {
            if let Some(generation) = bootstrap_generation {
                self.mark_bootstrap_materialization_cancelled(identity, Some(generation));
            }
            return result;
        }
        // Alias validation happens under the lifecycle lock before
        // `embody_identity` marks bootstrap work as started. A stale alias
        // therefore must leave the replacement generation's exact readiness
        // state untouched.
        if matches!(&result, Err(IdentityRuntimeError::StaleRuntimeAlias { .. })) {
            return result;
        }
        if result.is_ok() {
            let desired_edges = self.desired_peer_edges.read().await.clone();
            let has_pending_topology_recovery = match self.topology_controller() {
                Some(controller) => controller.has_pending().await,
                None => false,
            };
            if (!desired_edges.is_empty() || has_pending_topology_recovery)
                && let Err(error) = self.reconcile_managed_peer_edges(&desired_edges).await
            {
                tracing::warn!(
                    identity = %identity,
                    %error,
                    "identity materialized with topology reconcile warning"
                );
            }
        }
        if let Some(generation) = bootstrap_generation {
            self.mark_bootstrap_materialization_finished(identity, &result, Some(generation));
        }
        result
    }

    /// Cancellation-safe materialization for request/host boundaries.
    pub async fn materialize_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        self.run_tracked_foreground(async move { runtime.materialize(&identity).await })
            .await
    }

    /// Cancellation-safe compatibility restore used by embedders that pass
    /// an RPC identity context without attaching an
    /// [`IdentityFirstRuntimeContext`] to the unified runtime.
    pub(crate) async fn restore_flow_tracked(
        self: &Arc<Self>,
        roster: Vec<DurableAgentSpec>,
        topology_provider: Option<Arc<dyn TopologyProvider>>,
        customizer: Option<Arc<dyn AgentCustomizer>>,
    ) -> Result<super::orchestrator::RestoreFlowResult, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        self.run_tracked_foreground(async move {
            super::orchestrator::restore_flow(
                runtime.as_ref(),
                &roster,
                topology_provider.as_deref(),
                customizer.as_deref(),
            )
            .await
        })
        .await
    }

    /// Cancellation-safe retirement for RPC/host request boundaries.
    pub async fn retire_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        self.run_tracked_foreground(async move { runtime.retire(&identity).await })
            .await
    }

    /// Cancellation-safe retirement that atomically rejects an old generated
    /// runtime alias under the same lifecycle lock as the mutation.
    pub async fn retire_member_alias_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: &str,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.to_string();
        self.run_tracked_foreground(async move {
            runtime
                .retire_with_expected_member_alias(&identity, Some(&expected_alias))
                .await
        })
        .await
    }

    /// Retire identity authority and its captured lower-plane generations
    /// under one lifecycle lock. Enumeration performed by `cleanup` therefore
    /// observes the generation actually retired, even when this request had
    /// waited behind a concurrent reset.
    pub(crate) async fn retire_and_cleanup_live_members_tracked<T, F, Fut>(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
        cleanup: F,
    ) -> Result<(FencingToken, T), IdentityRuntimeError>
    where
        T: Send + 'static,
        F: FnOnce(Option<AgentRuntimeId>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.map(str::to_owned);
        self.run_tracked_foreground(async move {
            let lifecycle_lock = runtime.lifecycle_lock_for(&identity).await;
            let _lifecycle_guard = lifecycle_lock.lock().await;
            if let Some(expected_alias) = expected_alias.as_deref() {
                runtime
                    .ensure_expected_member_alias_current(&identity, expected_alias)
                    .await?;
            }
            let retired_alias = runtime
                .entries
                .read()
                .await
                .get(&identity)
                .and_then(|entry| entry.continuity.as_ref())
                .map(|record| record.agent_runtime_id.clone());
            let token = runtime.retire_locked(&identity).await?;
            let metadata = cleanup(retired_alias).await;
            Ok((token, metadata))
        })
        .await
    }

    /// Cancellation-safe respawn for RPC/host request boundaries.
    pub async fn respawn_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        self.run_tracked_foreground(async move { runtime.respawn(&identity).await })
            .await
    }

    /// Cancellation-safe respawn that atomically rejects an old generated
    /// runtime alias under the same lifecycle lock as the mutation.
    pub async fn respawn_member_alias_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: &str,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.to_string();
        self.run_tracked_foreground(async move {
            runtime
                .respawn_with_expected_member_alias(&identity, Some(&expected_alias))
                .await
        })
        .await
    }

    /// Execute a lower member-plane mutation while pinning a generated alias
    /// to the durable identity generation that owns it. This is the common
    /// authority boundary for legacy member RPCs (for example force-cancel)
    /// that cannot otherwise express identity continuity.
    pub(crate) async fn run_member_alias_operation_tracked<T, F, Fut>(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: &str,
        operation: F,
    ) -> Result<T, IdentityRuntimeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, String>> + Send + 'static,
    {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.to_string();
        self.run_tracked_foreground(async move {
            let lifecycle_lock = runtime.lifecycle_lock_for(&identity).await;
            let _lifecycle_guard = lifecycle_lock.lock().await;
            runtime
                .ensure_expected_member_alias_current(&identity, &expected_alias)
                .await?;
            operation().await.map_err(IdentityRuntimeError::Internal)
        })
        .await
    }

    /// Cancellation-safe identity-first respawn for RPC and console boundaries.
    ///
    /// A durable identity already owns an authoritative Meerkat session. Its
    /// respawn operation therefore fences the old owner, refreshes the persisted
    /// runtime state, and reactivates that exact continuity record in place. It
    /// must not call the raw member-plane respawn convenience: that operation
    /// creates a new session and would silently turn a non-destructive identity
    /// recovery into a destructive reset.
    pub(crate) async fn respawn_identity_in_place_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        match expected_alias {
            Some(expected_alias) => {
                self.respawn_member_alias_tracked(identity, expected_alias)
                    .await
            }
            None => self.respawn_tracked(identity).await,
        }
    }

    /// Cancellation-safe live-session rebind for RPC/host boundaries.
    pub async fn rebind_session_after_live_respawn_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        session_id: SessionId,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        self.run_tracked_foreground(async move {
            runtime
                .rebind_session_after_live_respawn(&identity, session_id)
                .await
        })
        .await
    }

    /// Cancellation-safe live-session rebind pinned to the generation that
    /// initiated the lower-level member respawn.
    pub async fn rebind_session_after_live_respawn_member_alias_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: &str,
        session_id: SessionId,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.to_string();
        self.run_tracked_foreground(async move {
            runtime
                .rebind_session_after_live_respawn_with_expected_member_alias(
                    &identity,
                    Some(&expected_alias),
                    session_id,
                )
                .await
        })
        .await
    }

    /// Cancellation-safe destructive reset for RPC/host boundaries.
    pub async fn reset_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        self.run_tracked_foreground(async move { runtime.reset(&identity).await })
            .await
    }

    /// Cancellation-safe reset that atomically rejects an old generated
    /// runtime alias under the same lifecycle lock as the mutation.
    pub async fn reset_member_alias_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: &str,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.to_string();
        self.run_tracked_foreground(async move {
            runtime
                .reset_with_expected_member_alias(&identity, Some(&expected_alias))
                .await
        })
        .await
    }

    /// Cancellation-safe identity deletion for RPC/host boundaries.
    pub async fn delete_identity_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
    ) -> Result<(), IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        self.run_tracked_foreground(async move { runtime.delete_identity(&identity).await })
            .await
    }

    /// Cancellation-safe deletion that atomically rejects an old generated
    /// runtime alias under the same lifecycle lock as the mutation.
    pub async fn delete_identity_member_alias_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: &str,
    ) -> Result<(), IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.to_string();
        self.run_tracked_foreground(async move {
            runtime
                .delete_identity_with_expected_member_alias(&identity, Some(&expected_alias))
                .await
        })
        .await
    }

    /// Delete identity authority and clean its captured concrete generations
    /// in the same lifecycle transaction.
    pub(crate) async fn delete_identity_and_cleanup_live_members_tracked<T, F, Fut>(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
        cleanup: F,
    ) -> Result<T, IdentityRuntimeError>
    where
        T: Send + 'static,
        F: FnOnce(Option<AgentRuntimeId>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.map(str::to_owned);
        self.run_tracked_foreground(async move {
            let lifecycle_lock = runtime.lifecycle_lock_for(&identity).await;
            let _lifecycle_guard = lifecycle_lock.lock().await;
            if let Some(expected_alias) = expected_alias.as_deref() {
                runtime
                    .ensure_expected_member_alias_current(&identity, expected_alias)
                    .await?;
            }
            let deleted_alias = runtime
                .entries
                .read()
                .await
                .get(&identity)
                .and_then(|entry| entry.continuity.as_ref())
                .map(|record| record.agent_runtime_id.clone());
            runtime.delete_identity_locked(&identity).await?;
            Ok(cleanup(deleted_alias).await)
        })
        .await
    }

    /// Background warming observes controller cancellation after lease
    /// acquisition bookkeeping but before bridge/member installation. A
    /// cancelled grant is explicitly released; after customization completes,
    /// the operation is joined to its explicit commit/rollback boundary.
    async fn materialize_for_background(
        &self,
        identity: &AgentIdentity,
        cancellation: &mut watch::Receiver<bool>,
        generation: u64,
    ) -> Option<Result<ContinuityRecord, IdentityRuntimeError>> {
        let mut bound_generation = None;
        let result = self
            .embody_identity(
                identity,
                None,
                Some(cancellation),
                Some(generation),
                &mut bound_generation,
                EmbodimentOverrides::default(),
            )
            .await
            .map(|outcome| outcome.record);
        if matches!(
            &result,
            Err(IdentityRuntimeError::Internal(message)) if message == BACKGROUND_WARM_CANCELLED
        ) {
            self.mark_bootstrap_materialization_cancelled(identity, Some(generation));
            return None;
        }
        if result.is_ok() {
            let desired_edges = self.desired_peer_edges.read().await.clone();
            if !desired_edges.is_empty()
                && let Err(error) = self.reconcile_managed_peer_edges(&desired_edges).await
            {
                tracing::warn!(
                    identity = %identity,
                    %error,
                    "background identity materialized with topology reconcile warning"
                );
            }
        }
        self.mark_bootstrap_materialization_finished(identity, &result, Some(generation));
        Some(result)
    }

    pub(crate) async fn embody_identity(
        &self,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
        mut cancellation: Option<&mut watch::Receiver<bool>>,
        expected_bootstrap_generation: Option<u64>,
        bound_bootstrap_generation: &mut Option<u64>,
        overrides: EmbodimentOverrides<'_>,
    ) -> Result<EmbodimentOutcome, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        let bootstrap_generation = {
            let entries = self.entries.read().await;
            entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?
                .bootstrap_generation
        };
        *bound_bootstrap_generation = Some(bootstrap_generation);
        // A background item belongs to one controller generation. If a newer
        // roster pass converged the entry before this item acquired lifecycle
        // authority, treat the old work as cancelled instead of materializing
        // the newer spec on behalf of a superseded pass.
        if expected_bootstrap_generation.is_some_and(|expected| expected != bootstrap_generation) {
            return Err(IdentityRuntimeError::Internal(
                BACKGROUND_WARM_CANCELLED.to_string(),
            ));
        }
        let raw_alias_lock = self.raw_member_alias_lock(identity.as_str()).await;
        let _raw_alias_guard = raw_alias_lock.lock().await;
        self.ensure_raw_member_alias_available(identity).await?;
        if let Some(expected_alias) = expected_alias {
            self.ensure_expected_member_alias_current(identity, expected_alias)
                .await?;
        }
        let lock = self.materialization_lock_for(identity).await;
        let _guard = lock.lock().await;

        let (spec, continuity, state) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            (
                overrides
                    .spec
                    .cloned()
                    .unwrap_or_else(|| entry.spec.clone()),
                entry.continuity.clone(),
                entry.state,
            )
        };
        if state == IdentityLifecycleState::Active {
            // Converged eager restore still validates the time-sensitive
            // external lease. This is part of the shared embodiment door, not
            // a second restore implementation: healthy authority is reused,
            // due authority is renewed, and lost authority parks this member.
            let record = self.reuse_active_restore_state(&spec).await?;
            self.clear_materialization_backoff(identity).await;
            return Ok(EmbodimentOutcome {
                record,
                resumed: true,
                draft: AgentBuildDraft {
                    model: None,
                    system_prompt: None,
                    additional_instructions: spec.additional_instructions.clone(),
                    labels: spec.labels.clone(),
                    app_context: spec.context.clone(),
                    external_tools: Vec::new(),
                    local_external_tools: Default::default(),
                    provider_params: None,
                    compaction_curator: Default::default(),
                },
            });
        }
        // Host-rejected-build park: the app-side gate answered this exact
        // spec with a deterministic rejection. Fail fast typed — every
        // attempt would otherwise burn a full member build plus a callback
        // round trip for the same answer. A spec change clears the park
        // (checked inside the accessor).
        if let Some(park) = self.host_rejected_build_park(identity).await {
            return Err(IdentityRuntimeError::HostRejectedBuild {
                identity: identity.clone(),
                reason: park.reason,
            });
        }
        let original_continuity = continuity.clone();
        let continuity = if durable_spec_uses_external_binding(&spec) {
            None
        } else {
            continuity
        };

        match state {
            IdentityLifecycleState::Dormant | IdentityLifecycleState::Uninitialized => {}
            IdentityLifecycleState::Broken
            | IdentityLifecycleState::Retiring
            | IdentityLifecycleState::Suspended => {
                return Err(IdentityRuntimeError::InvalidState {
                    identity: identity.clone(),
                    state,
                    operation: "materialize",
                });
            }
            IdentityLifecycleState::Active => unreachable!("active handled above"),
        }
        // Begin readiness bookkeeping only after ownership validation and
        // only for an identity that will actually materialize. An old alias
        // can now fail without a transient or terminal mutation of the
        // replacement generation's bootstrap entry.
        self.mark_bootstrap_materialization_started(identity, Some(bootstrap_generation));

        // A previous customization/installation failure can leave its exact
        // unactivated grant parked when provider cleanup itself fails. Strict
        // providers reject reacquisition while that grant still exists, so a
        // direct lazy retry must drain this identity's orphan before acquiring
        // fresh authority. Other identities' cleanup failures remain isolated.
        self.release_parked_unactivated_leases_for_identity(identity)
            .await?;

        // Establish single-embodiment ownership before invoking arbitrary
        // host customizer code, preserving the public ordering contract. No
        // bridge/member state exists yet, so every exit below can still
        // release this uninstalled grant explicitly.
        let lease_results = self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
            .map_err(IdentityRuntimeError::Lease)?;
        let grant = match lease_results.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(grant)) => grant.clone(),
            Some(super::types::LeaseAcquireResult::AlreadyHeld { holder, .. }) => {
                tracing::error!(
                    %identity,
                    holder = %holder,
                    "single-embodiment guard: refusing to materialize an identity whose \
                     durable lease is held by another live runtime instance"
                );
                return Err(IdentityRuntimeError::AlreadyEmbodied {
                    identity: identity.clone(),
                    holder: holder.clone(),
                });
            }
            None => return Err(IdentityRuntimeError::NoActiveLease(identity.clone())),
        };
        if let Some(record) = continuity.as_ref()
            && let Err(err) = self
                .continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
        {
            let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
            return Err(IdentityRuntimeError::Internal(format!(
                "continuity upsert before materialize: {err}{}",
                cleanup_error
                    .as_ref()
                    .map(|e| format!("; lease cleanup failed: {e}"))
                    .unwrap_or_default(),
            )));
        }

        if cancellation
            .as_ref()
            .is_some_and(|cancellation| *cancellation.borrow())
        {
            return Err(self
                .cancel_uninstalled_background_materialization(&grant)
                .await);
        }

        let active_peers = self.entries.read().await.keys().cloned().collect();
        let managed_edges = self.desired_peer_edges.read().await.clone();
        let build_context = AgentBuildContext {
            identity: identity.clone(),
            active_peers,
            managed_edges,
            runtime_services: self.runtime_services(),
        };
        let mut draft = AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: spec.additional_instructions.clone(),
            labels: spec.labels.clone(),
            app_context: spec.context.clone(),
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
            provider_params: None,
            compaction_curator: Default::default(),
        };
        let installed_customizer = self.customizer.read().await.clone();
        if let Some(customizer) = overrides.customizer.or(installed_customizer.as_deref()) {
            let customize = customizer.customize_build(&build_context, &spec, &mut draft);
            tokio::pin!(customize);
            let customize_result = if let Some(cancellation) = cancellation.as_mut() {
                let cancellation = &mut **cancellation;
                tokio::select! {
                    result = &mut customize => result,
                    changed = cancellation.changed() => {
                        if changed.is_ok() && *cancellation.borrow() {
                            return Err(
                                self.cancel_uninstalled_background_materialization(&grant)
                                    .await,
                            );
                        }
                        customize.await
                    }
                }
            } else {
                customize.await
            };
            if let Err(err) = customize_result {
                let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "customizer: {err}{}",
                    cleanup_error
                        .as_ref()
                        .map(|e| format!("; lease cleanup failed: {e}"))
                        .unwrap_or_default(),
                )));
            }
        }

        let mut abandoned_session_registrations: Vec<SessionId> = Vec::new();
        let mut resumed = false;
        let mut record = if let Some(mut record) = continuity {
            let resume_snapshot = if self
                .bridge
                .as_ref()
                .is_none_or(|bridge| bridge.requires_resume_snapshot())
            {
                match self
                    .continuity_store
                    .load_session_snapshot(&record.session_id)
                    .await
                {
                    Ok(snapshot) => snapshot,
                    Err(err) => {
                        let cleanup_error =
                            self.release_uninstalled_materialize_lease(&grant).await;
                        return Err(IdentityRuntimeError::Internal(format!(
                            "load session snapshot before materialize: {err}{}",
                            cleanup_error
                                .as_ref()
                                .map(|e| format!("; lease cleanup failed: {e}"))
                                .unwrap_or_default(),
                        )));
                    }
                }
            } else {
                None
            };

            if let Some(bridge) = self.bridge.as_ref() {
                if let Err(err) = bridge
                    .register_session_runtime_state(
                        &record.session_id,
                        identity,
                        record.generation,
                        record.checkpoint_version,
                        grant.fencing_token,
                    )
                    .await
                {
                    let unregister_error = Self::unregister_bridge_session_runtime_states(
                        bridge.as_ref(),
                        std::slice::from_ref(&record.session_id),
                    )
                    .await;
                    let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
                    return Err(IdentityRuntimeError::Internal(format!(
                        "bridge register_session_runtime_state: {err}{}{}",
                        unregister_error
                            .as_ref()
                            .map(|e| format!("; unregister session failed: {e}"))
                            .unwrap_or_default(),
                        cleanup_error
                            .as_ref()
                            .map(|e| format!("; lease cleanup failed: {e}"))
                            .unwrap_or_default(),
                    )));
                }
                let registered_session_id = record.session_id.clone();
                let empty_snapshot = SessionSnapshot { data: Vec::new() };
                let snapshot = resume_snapshot.as_ref().unwrap_or(&empty_snapshot);
                let outcome = bridge
                    .resume_session(
                        identity,
                        &record.agent_runtime_id,
                        &spec,
                        &draft,
                        &record.session_id,
                        snapshot,
                    )
                    .await;
                // An attach that reconciled custody is NOT yet resumed. Register
                // the owning identity from THIS record - the only authoritative
                // source of the generation, checkpoint version and fencing token
                // - before anything can commit a runtime boundary. Without it a
                // head-canonical session reaches the continuity adapter with an
                // unregistered owner and is correctly refused, far from here.
                //
                // Registration failure leaves a retryable pending state: no
                // retire, no replacement, and never a Resumed report.
                let outcome = match outcome {
                    Ok(super::bridge::ResumeSessionOutcome::AttachedPendingRegistration {
                        session_id,
                    }) => {
                        if let Some(bridge) = self.bridge.as_ref() {
                            bridge
                                .register_session_runtime_state(
                                    &session_id,
                                    identity,
                                    record.generation,
                                    record.checkpoint_version,
                                    grant.fencing_token,
                                )
                                .await
                                .map_err(|err| {
                                    IdentityRuntimeError::Internal(format!(
                                        "owner registration after attach of {session_id} failed, \
                                         so the session is not resumable yet (retryable: the \
                                         occupant is untouched and retry is idempotent): {err}"
                                    ))
                                })?;
                        }
                        super::bridge::ResumeSessionOutcome::Resumed { session_id }
                    }
                    Ok(outcome) => outcome,
                    Err(err) => {
                        // A rejected resume must NEVER abandon the durable
                        // session (the transcript is the only copy). Keep the
                        // identity → session binding intact — no retire, no
                        // continuity rebind — mark the identity Broken with
                        // the error attached so this send fails loudly, and
                        // let the next reconcile retry the resume. Unregister
                        // only the session-runtime-state bookkeeping (it is
                        // re-registered on retry).
                        tracing::error!(
                            %identity,
                            session_id = %registered_session_id,
                            error = %err,
                            "materialize resume rejected; marking identity Broken and preserving \
                             the durable session for reconcile retry"
                        );
                        let unregister_error = Self::unregister_bridge_session_runtime_states(
                            bridge.as_ref(),
                            std::slice::from_ref(&registered_session_id),
                        )
                        .await;
                        let lease_cleanup_error =
                            self.release_uninstalled_materialize_lease(&grant).await;
                        {
                            let mut entries = self.entries.write().await;
                            if let Some(entry) = entries.get_mut(identity) {
                                entry.state = IdentityLifecycleState::Broken;
                                entry.lease = None;
                            }
                        }
                        self.emit_event(
                            identity,
                            IdentityEvent::StateChanged {
                                identity: identity.clone(),
                                new_state: IdentityLifecycleState::Broken,
                            },
                        )
                        .await;
                        {
                            let rejection = err.to_string();
                            if is_host_rejected_build_error(&rejection) {
                                self.mark_host_rejected_build_park(identity, rejection)
                                    .await;
                            }
                        }
                        // Repair honesty (OB3 rehearsal): the typed
                        // ArchivedNotRevivable refusal is a stable,
                        // deterministic wall - record the terminal verdict on
                        // this FIRST refusal so the repair supervisor parks
                        // instead of heal-looping (the roster heal succeeds,
                        // this materialize precondition never does).
                        if let BridgeError::ResumeRejected {
                            kind: ResumeRejectionKind::ArchivedNotRevivable,
                            detail,
                        } = &err
                            && !self
                                .mark_continuity_unrecoverable(
                                    identity,
                                    archived_not_revivable_park_reason(
                                        &registered_session_id,
                                        detail,
                                    ),
                                )
                                .await
                        {
                            tracing::debug!(
                                %identity,
                                "identity left Broken before the archived-not-revivable \
                                 park could be recorded"
                            );
                        }
                        let detail = format!(
                            "bridge resume_session rejected (identity degraded, durable session \
                             preserved for reconcile retry): {err}{}{}",
                            unregister_error
                                .as_ref()
                                .map(|e| format!("; unregister session failed: {e}"))
                                .unwrap_or_default(),
                            lease_cleanup_error
                                .as_ref()
                                .map(|e| format!("; lease cleanup failed: {e}"))
                                .unwrap_or_default(),
                        );
                        let kind = if matches!(
                            err,
                            BridgeError::ResumeRejected {
                                kind: ResumeRejectionKind::ArchivedNotRevivable,
                                ..
                            }
                        ) {
                            ContinuityFailureKind::CheckpointUnrecoverable
                        } else {
                            ContinuityFailureKind::ResumeRejected
                        };
                        return Err(IdentityRuntimeError::EmbodimentRejected(Box::new(
                            ContinuityFailure {
                                identity: identity.clone(),
                                kind,
                                record: Some(record.clone()),
                                detail,
                            },
                        )));
                    }
                };
                resumed = outcome.fallback_reason().is_none();
                if let Some(reason) = outcome.fallback_reason().cloned() {
                    tracing::warn!(
                        %identity,
                        reason = ?reason,
                        "lazy identity materialization fresh-spawned after typed resume fallback"
                    );
                    self.emit_event(
                        identity,
                        IdentityEvent::ResumeFallback {
                            identity: identity.clone(),
                            reason,
                        },
                    )
                    .await;
                }
                let effective_session_id = outcome.session_id().clone();
                if effective_session_id != registered_session_id {
                    // §8.4 trigger (b): the resume fallback abandoned the
                    // registered session — harvest it detached (materialize
                    // is a hot path; the session store read stays valid).
                    if let Some(injector) = self.agent_memory.read().await.as_ref() {
                        let abandoned_key = registered_session_id.to_string();
                        injector.note_session_generation(
                            identity,
                            &abandoned_key,
                            record.generation.get(),
                        );
                        injector.spawn_rotation_distillation(
                            identity,
                            &abandoned_key,
                            crate::memory::distiller::DistillCause::ResumeFallback,
                        );
                    }
                    abandoned_session_registrations.push(registered_session_id);
                }
                record.session_id = effective_session_id;
            } else {
                resumed = resume_snapshot.is_some();
            }
            record
        } else {
            let new_runtime_id =
                AgentRuntimeId::parse(&format!("rt:{identity}:0")).map_err(|err| {
                    IdentityRuntimeError::Internal(format!("failed to mint runtime id: {err}"))
                })?;
            let mut record = ContinuityRecord {
                identity: identity.clone(),
                agent_runtime_id: new_runtime_id,
                session_id: meerkat_core::types::SessionId::new(),
                generation: ContinuityGeneration::new(0),
                checkpoint_version: CheckpointVersion::new(0),
            };
            if let Err(err) = self
                .continuity_store
                .upsert_continuity_record(&record, grant.fencing_token)
                .await
            {
                let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "continuity upsert before materialize create: {err}{}",
                    cleanup_error
                        .as_ref()
                        .map(|e| format!("; lease cleanup failed: {e}"))
                        .unwrap_or_default(),
                )));
            }
            if let Some(bridge) = self.bridge.as_ref() {
                let provisional_session_id = record.session_id.clone();
                if let Err(err) = bridge
                    .register_session_runtime_state(
                        &record.session_id,
                        identity,
                        record.generation,
                        record.checkpoint_version,
                        grant.fencing_token,
                    )
                    .await
                    .map_err(|err| {
                        IdentityRuntimeError::Internal(format!(
                            "bridge register_session_runtime_state: {err}"
                        ))
                    })
                {
                    let unregister_error = Self::unregister_bridge_session_runtime_states(
                        bridge.as_ref(),
                        std::slice::from_ref(&provisional_session_id),
                    )
                    .await;
                    let delete_error = self
                        .continuity_store
                        .delete_continuity_record(identity, grant.fencing_token)
                        .await
                        .err();
                    let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
                    if let Some(delete_error) = delete_error {
                        return Err(IdentityRuntimeError::Internal(format!(
                            "{err}{}; tentative continuity cleanup failed: {delete_error}{}",
                            unregister_error
                                .as_ref()
                                .map(|e| format!("; unregister session failed: {e}"))
                                .unwrap_or_default(),
                            cleanup_error
                                .as_ref()
                                .map(|e| format!("; lease cleanup failed: {e}"))
                                .unwrap_or_default(),
                        )));
                    }
                    if let Some(cleanup_error) = cleanup_error {
                        return Err(IdentityRuntimeError::Internal(format!(
                            "{err}{}; lease cleanup failed: {cleanup_error}",
                            unregister_error
                                .as_ref()
                                .map(|e| format!("; unregister session failed: {e}"))
                                .unwrap_or_default(),
                        )));
                    }
                    if let Some(unregister_error) = unregister_error {
                        return Err(IdentityRuntimeError::Internal(format!(
                            "{err}; unregister session failed: {unregister_error}"
                        )));
                    }
                    return Err(err);
                }
                let created_session_id = bridge
                    .create_session(
                        identity,
                        &record.agent_runtime_id,
                        &spec,
                        &draft,
                        &record.session_id,
                    )
                    .await
                    .map_err(|err| {
                        IdentityRuntimeError::Internal(format!("bridge create_session: {err}"))
                    });
                if let Err(err) = &created_session_id {
                    let detail = err.to_string();
                    if is_host_rejected_build_error(&detail) {
                        self.mark_host_rejected_build_park(identity, detail).await;
                    }
                }
                match created_session_id {
                    Ok(session_id) => {
                        if session_id != provisional_session_id {
                            abandoned_session_registrations.push(provisional_session_id);
                        }
                        record.session_id = session_id;
                    }
                    Err(err) => {
                        let unregister_error = Self::unregister_bridge_session_runtime_states(
                            bridge.as_ref(),
                            std::slice::from_ref(&provisional_session_id),
                        )
                        .await;
                        let cleanup_error =
                            bridge.retire_member(&record.agent_runtime_id).await.err();
                        let delete_error = self
                            .continuity_store
                            .delete_continuity_record(identity, grant.fencing_token)
                            .await
                            .err();
                        let lease_cleanup_error =
                            self.release_uninstalled_materialize_lease(&grant).await;
                        if unregister_error.is_some()
                            || cleanup_error.is_some()
                            || delete_error.is_some()
                            || lease_cleanup_error.is_some()
                        {
                            return Err(IdentityRuntimeError::Internal(format!(
                                "{err}{}{}{}{}",
                                unregister_error
                                    .as_ref()
                                    .map(|e| format!("; unregister session failed: {e}"))
                                    .unwrap_or_default(),
                                cleanup_error
                                    .as_ref()
                                    .map(|e| format!("; cleanup retire failed: {e}"))
                                    .unwrap_or_default(),
                                delete_error
                                    .as_ref()
                                    .map(|e| format!("; tentative continuity cleanup failed: {e}"))
                                    .unwrap_or_default(),
                                lease_cleanup_error
                                    .as_ref()
                                    .map(|e| format!("; lease cleanup failed: {e}"))
                                    .unwrap_or_default(),
                            )));
                        }
                        return Err(err);
                    }
                }
            }
            record
        };

        if let Err(err) = self
            .continuity_store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await
        {
            let unregister_error = if let Some(bridge) = self.bridge.as_ref() {
                let mut sessions_to_unregister = abandoned_session_registrations.clone();
                sessions_to_unregister.push(record.session_id.clone());
                Self::unregister_bridge_session_runtime_states(
                    bridge.as_ref(),
                    &sessions_to_unregister,
                )
                .await
            } else {
                None
            };
            let cleanup_error = if let Some(bridge) = self.bridge.as_ref() {
                bridge.retire_member(&record.agent_runtime_id).await.err()
            } else {
                None
            };
            let restore_error = self
                .restore_continuity_after_materialize_failure(
                    identity,
                    original_continuity.as_ref(),
                    &grant,
                )
                .await;
            let lease_cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
            if unregister_error.is_some()
                || cleanup_error.is_some()
                || restore_error.is_some()
                || lease_cleanup_error.is_some()
            {
                return Err(IdentityRuntimeError::Internal(format!(
                    "continuity upsert after materialize: {err}{}{}{}{}",
                    unregister_error
                        .as_ref()
                        .map(|e| format!("; unregister session failed: {e}"))
                        .unwrap_or_default(),
                    cleanup_error
                        .as_ref()
                        .map(|e| format!("; cleanup retire failed: {e}"))
                        .unwrap_or_default(),
                    restore_error
                        .as_ref()
                        .map(|e| format!("; continuity rollback failed: {e}"))
                        .unwrap_or_default(),
                    lease_cleanup_error
                        .as_ref()
                        .map(|e| format!("; lease cleanup failed: {e}"))
                        .unwrap_or_default(),
                )));
            }
            return Err(IdentityRuntimeError::Internal(format!(
                "continuity upsert after materialize: {err}"
            )));
        }
        if let Some(bridge) = self.bridge.as_ref() {
            let register_result = bridge
                .register_session_runtime_state(
                    &record.session_id,
                    identity,
                    record.generation,
                    record.checkpoint_version,
                    grant.fencing_token,
                )
                .await;
            let effective_checkpoint_version = match register_result {
                Ok(version) => version,
                Err(err) => {
                    let mut sessions_to_unregister = abandoned_session_registrations.clone();
                    sessions_to_unregister.push(record.session_id.clone());
                    let unregister_error = Self::unregister_bridge_session_runtime_states(
                        bridge.as_ref(),
                        &sessions_to_unregister,
                    )
                    .await;
                    let cleanup_error = bridge.retire_member(&record.agent_runtime_id).await.err();
                    let restore_error = self
                        .restore_continuity_after_materialize_failure(
                            identity,
                            original_continuity.as_ref(),
                            &grant,
                        )
                        .await;
                    let lease_cleanup_error =
                        self.release_uninstalled_materialize_lease(&grant).await;
                    return Err(IdentityRuntimeError::Internal(format!(
                        "bridge register actual session runtime state: {err}{}{}{}{}",
                        unregister_error
                            .as_ref()
                            .map(|e| format!("; unregister session failed: {e}"))
                            .unwrap_or_default(),
                        cleanup_error
                            .as_ref()
                            .map(|e| format!("; cleanup retire failed: {e}"))
                            .unwrap_or_default(),
                        restore_error
                            .as_ref()
                            .map(|e| format!("; continuity rollback failed: {e}"))
                            .unwrap_or_default(),
                        lease_cleanup_error
                            .as_ref()
                            .map(|e| format!("; lease cleanup failed: {e}"))
                            .unwrap_or_default(),
                    )));
                }
            };
            record.checkpoint_version = effective_checkpoint_version;
            if let Some(err) = Self::unregister_bridge_session_runtime_states(
                bridge.as_ref(),
                &abandoned_session_registrations,
            )
            .await
            {
                let actual_unregister_error = Self::unregister_bridge_session_runtime_states(
                    bridge.as_ref(),
                    std::slice::from_ref(&record.session_id),
                )
                .await;
                let unregister_error = actual_unregister_error
                    .map(|actual_err| format!("{err}; actual session: {actual_err}"))
                    .unwrap_or(err);
                let cleanup_error = bridge.retire_member(&record.agent_runtime_id).await.err();
                let restore_error = self
                    .restore_continuity_after_materialize_failure(
                        identity,
                        original_continuity.as_ref(),
                        &grant,
                    )
                    .await;
                let lease_cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "bridge unregister abandoned session runtime state: {unregister_error}{}{}{}",
                    cleanup_error
                        .as_ref()
                        .map(|e| format!("; cleanup retire failed: {e}"))
                        .unwrap_or_default(),
                    restore_error
                        .as_ref()
                        .map(|e| format!("; continuity rollback failed: {e}"))
                        .unwrap_or_default(),
                    lease_cleanup_error
                        .as_ref()
                        .map(|e| format!("; lease cleanup failed: {e}"))
                        .unwrap_or_default(),
                )));
            }
        }

        {
            let mut entries = self.entries.write().await;
            let entry = entries
                .get_mut(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            entry.continuity = Some(record.clone());
            entry.lease = Some(Self::lease_entry_from_grant(&grant));
            entry.state = IdentityLifecycleState::Active;
            entry.checkpoint_version = record.checkpoint_version;
        }
        self.emit_event(
            identity,
            IdentityEvent::StateChanged {
                identity: identity.clone(),
                new_state: IdentityLifecycleState::Active,
            },
        )
        .await;
        self.clear_materialization_backoff(identity).await;
        Ok(EmbodimentOutcome {
            record,
            resumed,
            draft,
        })
    }

    /// Project one member-attributable embodiment failure into the durable
    /// identity runtime without turning it into a fleet-level bootstrap
    /// failure. The returned payload is the exact typed outcome installed by
    /// eager restore. Pass-level roster, topology, and batch-store failures do
    /// not enter this door.
    pub(crate) async fn park_embodiment_failure(
        &self,
        identity: &AgentIdentity,
        error: &IdentityRuntimeError,
    ) -> ContinuityFailure {
        let explicit_failure = match error {
            IdentityRuntimeError::EmbodimentRejected(failure) => Some((**failure).clone()),
            _ => None,
        };
        let (record, transitioned) = {
            let mut entries = self.entries.write().await;
            let record = entries
                .get(identity)
                .and_then(|entry| entry.continuity.clone());
            let mut transitioned = false;
            if let Some(entry) = entries.get_mut(identity) {
                transitioned = entry.state != IdentityLifecycleState::Broken;
                entry.state = IdentityLifecycleState::Broken;
            }
            (record, transitioned)
        };
        if transitioned {
            self.emit_event(
                identity,
                IdentityEvent::StateChanged {
                    identity: identity.clone(),
                    new_state: IdentityLifecycleState::Broken,
                },
            )
            .await;
        }

        explicit_failure.unwrap_or_else(|| ContinuityFailure {
            identity: identity.clone(),
            kind: match error {
                IdentityRuntimeError::Store(_) => ContinuityFailureKind::StoreUnavailable,
                _ => ContinuityFailureKind::EmbodimentFailed,
            },
            record,
            detail: error.to_string(),
        })
    }

    async fn best_effort_materialize_identity(
        &self,
        identity: AgentIdentity,
        initiator: Option<&AgentIdentity>,
        operation: &'static str,
    ) -> Option<ContinuityRecord> {
        let attempt_lock = self.best_effort_materialization_lock_for(&identity).await;
        let _attempt_guard = attempt_lock.lock().await;

        if let Some(error) = self.materialization_backoff_error(&identity).await {
            tracing::debug!(
                identity = %identity,
                initiator = initiator.map(ToString::to_string).as_deref(),
                error = %error,
                "identity best-effort materialization skipped due to materialization backoff"
            );
            return None;
        }

        match self.materialize(&identity).await {
            Ok(record) => {
                self.clear_materialization_backoff(&identity).await;
                Some(record)
            }
            Err(err) => {
                tracing::warn!(
                    identity = %identity,
                    initiator = initiator.map(ToString::to_string).as_deref(),
                    error = %err,
                    "identity best-effort materialization skipped identity after materialization failure"
                );
                self.record_best_effort_materialization_failure(
                    &identity, initiator, operation, &err,
                )
                .await;
                None
            }
        }
    }

    async fn materialize_all_records(
        &self,
    ) -> Vec<(
        AgentIdentity,
        Result<ContinuityRecord, IdentityRuntimeError>,
    )> {
        let identities = self.registered_identities().await;
        stream::iter(identities.into_iter().map(|identity| async move {
            let result = self.materialize(&identity).await;
            (identity, result)
        }))
        .buffer_unordered(MANAGED_PEER_RECONCILE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
    }

    /// Materialize all identities currently registered with the runtime.
    ///
    /// Fleet hydration is best-effort: one member that cannot build is skipped
    /// and surfaced through logs/error hooks rather than aborting unrelated
    /// members.
    pub async fn materialize_all(&self) -> Result<Vec<ContinuityRecord>, IdentityRuntimeError> {
        let identities = self.registered_identities().await;
        let records = stream::iter(identities.into_iter().map(|identity| async move {
            self.best_effort_materialize_identity(identity, None, "materialize_all")
                .await
        }))
        .buffer_unordered(MANAGED_PEER_RECONCILE_CONCURRENCY)
        .filter_map(async move |record| record)
        .collect::<Vec<_>>()
        .await;

        let desired_edges = self.desired_peer_edges.read().await.clone();
        if !desired_edges.is_empty()
            && let Err(err) = self.reconcile_managed_peer_edges(&desired_edges).await
        {
            tracing::warn!(
                error = %err,
                "identity materialize_all completed with topology reconcile warning"
            );
        }

        Ok(records)
    }

    /// Materialize all identities and fail if any registered identity cannot
    /// hydrate. Flow admission uses this strict variant so a run is not accepted
    /// with a partially materialized identity-first fleet.
    pub async fn materialize_all_required(
        &self,
    ) -> Result<Vec<ContinuityRecord>, IdentityRuntimeError> {
        let results = self.materialize_all_records().await;
        let mut records = Vec::with_capacity(results.len());
        let mut failures = Vec::new();

        for (identity, result) in results {
            match result {
                Ok(record) => records.push(record),
                Err(err) => failures.push(format!("{identity}: {err}")),
            }
        }

        if !failures.is_empty() {
            return Err(IdentityRuntimeError::Internal(format!(
                "identity-first required materialization failed for {} identities: {}",
                failures.len(),
                failures.join("; ")
            )));
        }

        let desired_edges = self.desired_peer_edges.read().await.clone();
        if !desired_edges.is_empty() {
            self.reconcile_managed_peer_edges(&desired_edges).await?;
        }

        Ok(records)
    }

    /// Cancellation-safe strict fleet hydration for flow/request boundaries.
    pub async fn materialize_all_required_tracked(
        self: &Arc<Self>,
    ) -> Result<Vec<ContinuityRecord>, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        self.run_tracked_foreground(async move { runtime.materialize_all_required().await })
            .await
    }

    /// Ensure an active identity's desired peer neighborhood exists in the
    /// concrete mob graph before ordinary communication starts.
    pub async fn materialize_reachable_peers(
        &self,
        identity: &AgentIdentity,
    ) -> Result<Vec<ContinuityRecord>, IdentityRuntimeError> {
        let peers = self.reachable_peer_identities(identity).await;
        let records = stream::iter(peers.into_iter().map(|peer| async move {
            self.best_effort_materialize_identity(
                peer,
                Some(identity),
                "materialize_reachable_peers",
            )
            .await
        }))
        .buffer_unordered(MANAGED_PEER_RECONCILE_CONCURRENCY)
        .filter_map(async move |record| record)
        .collect::<Vec<_>>()
        .await;

        let desired_edges = self.desired_peer_edges.read().await.clone();
        if !desired_edges.is_empty()
            && let Err(err) = self.reconcile_managed_peer_edges(&desired_edges).await
        {
            tracing::warn!(
                identity = %identity,
                error = %err,
                "identity peer materialization completed with topology reconcile warning"
            );
        }

        Ok(records)
    }

    // -----------------------------------------------------------------------
    // Subscribe — REQ-06
    // -----------------------------------------------------------------------

    /// Subscribe to identity-scoped events.
    ///
    /// Returns a broadcast receiver that yields `IdentityEvent` items for
    /// state changes, lease updates, lease loss, and checkpoint completions.
    pub async fn subscribe(
        &self,
        identity: &AgentIdentity,
    ) -> Result<broadcast::Receiver<IdentityEvent>, IdentityRuntimeError> {
        let channels = self.event_channels.read().await;
        let tx = channels
            .get(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        Ok(tx.subscribe())
    }

    /// Update the spec for an existing identity (used during reconciliation).
    pub async fn update_spec(&self, spec: DurableAgentSpec) -> Result<(), IdentityRuntimeError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(&spec.identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(spec.identity.clone()))?;
        entry.spec = spec;
        Ok(())
    }

    /// Release every exact provider grant retained by a Broken entry. The
    /// caller owns this identity's lifecycle lock.
    ///
    /// Most failed lower-plane transitions already park authority in
    /// `pending_lease_release`, but rollback of an originally Active entry can
    /// deliberately retain its current grant in `lease` while projecting
    /// Broken. Move that live grant into pending state before the provider await
    /// so cancellation, a provider failure, and shutdown all retain the exact
    /// fencing token. A failed release aborts reconcile before lazy registration
    /// can overwrite the entry or restore can reacquire authority.
    async fn release_broken_lease_locked(
        &self,
        identity: &AgentIdentity,
    ) -> Result<bool, IdentityRuntimeError> {
        let pending = {
            let mut entries = self.entries.write().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            if entry.state != IdentityLifecycleState::Broken {
                return Ok(false);
            }

            let existing_pending = entry.pending_lease_release.clone();
            let retained_lease = entry.lease.as_ref().map(|lease| LeaseGrant {
                identity: identity.clone(),
                fencing_token: lease.fencing_token,
                ttl: lease.ttl,
            });
            if let (Some(pending), Some(retained)) =
                (existing_pending.as_ref(), retained_lease.as_ref())
                && pending.fencing_token != retained.fencing_token
            {
                return Err(IdentityRuntimeError::Internal(format!(
                    "Broken identity {identity} retained conflicting fencing tokens {} and {}",
                    pending.fencing_token, retained.fencing_token
                )));
            }

            let grant = existing_pending.or(retained_lease);
            if let Some(grant) = grant.as_ref() {
                let entry = entries
                    .get_mut(identity)
                    .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
                entry.lease = None;
                entry.pending_lease_release = Some(grant.clone());
            }
            grant
        };
        let Some(grant) = pending else {
            return Ok(false);
        };

        if let Err(error) = self
            .lease_provider
            .release_leases(std::slice::from_ref(&grant))
            .await
        {
            self.mark_bootstrap_from_lifecycle(
                identity,
                IdentityLifecycleState::Broken,
                Some(format!("pending lease release retry: {error}")),
            );
            return Err(IdentityRuntimeError::Lease(error));
        }

        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        match entry.pending_lease_release.as_ref() {
            Some(current) if current.fencing_token == grant.fencing_token => {
                entry.pending_lease_release = None;
            }
            Some(current) => {
                return Err(IdentityRuntimeError::Internal(format!(
                    "pending lease release for {identity} changed from fencing token {} to {} under lifecycle authority",
                    grant.fencing_token, current.fencing_token
                )));
            }
            None => {
                return Err(IdentityRuntimeError::Internal(format!(
                    "pending lease release for {identity} disappeared under lifecycle authority"
                )));
            }
        }
        Ok(true)
    }

    /// Prepare an existing Broken entry for metadata replacement while the
    /// caller owns its lifecycle lock. The concrete member is disposed before
    /// the exact retained provider grant is moved through the pending-release
    /// ledger. A failure leaves the entry Broken and the authority visible for
    /// retry or shutdown.
    pub(crate) async fn prepare_broken_identity_for_registration(
        &self,
        identity: &AgentIdentity,
    ) -> Result<(), IdentityRuntimeError> {
        self.cleanup_broken_lower_plane_locked(identity).await?;
        self.release_broken_lease_locked(identity).await?;
        Ok(())
    }

    /// Dispose the concrete member and its session-store authority retained by
    /// a continuity-backed Broken entry. Lease loss is fail-closed at the
    /// identity plane, but it cannot synchronously remove the lower member;
    /// every roster reconcile that will reproject the Broken entry must do so
    /// before abandoning or reusing the runtime alias. The real bridge treats
    /// an already-retired member as success, and session unregister is
    /// idempotent, making retries safe when a later exact-grant cleanup step
    /// fails.
    ///
    /// This deliberately does not mutate `lease` or `pending_lease_release`:
    /// provider authority is an independent exact-token cleanup obligation.
    async fn cleanup_broken_lower_plane_locked(
        &self,
        identity: &AgentIdentity,
    ) -> Result<(), IdentityRuntimeError> {
        let continuity = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            (entry.state == IdentityLifecycleState::Broken)
                .then(|| entry.continuity.clone())
                .flatten()
        };
        let (Some(bridge), Some(record)) = (self.bridge.as_ref(), continuity) else {
            return Ok(());
        };

        let retire_error = bridge.retire_member(&record.agent_runtime_id).await.err();
        let unregister_error = bridge
            .unregister_session_runtime_state(&record.session_id)
            .await
            .err();
        match (retire_error, unregister_error) {
            (None, None) => Ok(()),
            (retire_error, unregister_error) => Err(IdentityRuntimeError::Internal(format!(
                "cleanup Broken lower-plane state for {identity}{}{}",
                retire_error
                    .map(|error| format!("; retire member: {error}"))
                    .unwrap_or_default(),
                unregister_error
                    .map(|error| format!("; unregister session: {error}"))
                    .unwrap_or_default()
            ))),
        }
    }

    async fn accept_bootstrap_spec_locked(
        &self,
        spec: DurableAgentSpec,
        generation: u64,
    ) -> Result<(), IdentityRuntimeError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(&spec.identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(spec.identity.clone()))?;
        entry.spec = spec;
        entry.bootstrap_generation = generation;
        Ok(())
    }

    /// Converge currently registered embodiments with a newly desired roster.
    ///
    /// Restore flows only walk the desired roster, so removal and replacement
    /// must happen first. Each transition shares the same per-identity lock as
    /// foreground materialization and lifecycle RPCs. Same-profile changes are
    /// REQ-33 hot reloads: publish the new registry metadata while preserving
    /// the exact live session and grant. Profile changes retire to Dormant
    /// before the new spec is installed; the selected eager/background/lazy
    /// policy then decides when to rebuild the physical member.
    async fn reconcile_roster_members(
        &self,
        roster: &[DurableAgentSpec],
        generation: u64,
    ) -> Result<(), IdentityRuntimeError> {
        self.reconcile_roster_members_after_lifecycle_lock(roster, generation, |_| {
            std::future::ready(())
        })
        .await
    }

    /// Internal deterministic seam used by concurrency regressions. The hook
    /// runs with each identity's lifecycle lock held and before the entry is
    /// inspected or mutated.
    async fn reconcile_roster_members_after_lifecycle_lock<H, F>(
        &self,
        roster: &[DurableAgentSpec],
        generation: u64,
        mut after_lifecycle_lock: H,
    ) -> Result<(), IdentityRuntimeError>
    where
        H: FnMut(&AgentIdentity) -> F,
        F: Future<Output = ()>,
    {
        Self::validate_roster_uniqueness(roster)?;
        let desired = roster
            .iter()
            .map(|spec| (spec.identity.clone(), spec.clone()))
            .collect::<BTreeMap<_, _>>();
        let registered = self.registered_identities().await;

        for identity in registered
            .iter()
            .filter(|identity| !desired.contains_key(*identity))
        {
            let lifecycle_lock = self.lifecycle_lock_for(identity).await;
            let _lifecycle_guard = lifecycle_lock.lock().await;
            after_lifecycle_lock(identity).await;
            self.cleanup_broken_lower_plane_locked(identity).await?;
            self.release_broken_lease_locked(identity).await?;
            let state = self
                .entries
                .read()
                .await
                .get(identity)
                .map(|entry| entry.state);
            let Some(state) = state else {
                continue;
            };
            match state {
                IdentityLifecycleState::Active => {
                    self.retire_locked(identity).await?;
                }
                IdentityLifecycleState::Dormant
                | IdentityLifecycleState::Broken
                | IdentityLifecycleState::Uninitialized => {
                    let lease = self
                        .entries
                        .read()
                        .await
                        .get(identity)
                        .and_then(|entry| entry.lease.as_ref())
                        .map(|lease| LeaseGrant {
                            identity: identity.clone(),
                            fencing_token: lease.fencing_token,
                            ttl: lease.ttl,
                        });
                    if let Some(lease) = lease {
                        self.lease_provider
                            .release_leases(std::slice::from_ref(&lease))
                            .await
                            .map_err(IdentityRuntimeError::Lease)?;
                    }
                }
                IdentityLifecycleState::Retiring | IdentityLifecycleState::Suspended => {
                    return Err(IdentityRuntimeError::InvalidState {
                        identity: identity.clone(),
                        state,
                        operation: "reconcile_roster_remove",
                    });
                }
            }
            self.event_channels.write().await.remove(identity);
            self.entries.write().await.remove(identity);
        }

        for (identity, desired_spec) in desired {
            let lifecycle_lock = self.lifecycle_lock_for(&identity).await;
            let _lifecycle_guard = lifecycle_lock.lock().await;
            after_lifecycle_lock(&identity).await;
            // Initial bootstrap reaches this loop before lazy/eager restore has
            // registered the desired identities. Pending-release recovery only
            // applies to an existing lifecycle entry.
            if !self.entries.read().await.contains_key(&identity) {
                continue;
            }
            let reconciling_broken_entry = {
                let entries = self.entries.read().await;
                entries
                    .get(&identity)
                    .is_some_and(|entry| entry.state == IdentityLifecycleState::Broken)
            };
            if reconciling_broken_entry {
                self.cleanup_broken_lower_plane_locked(&identity).await?;
            }
            // This runs before the same-spec fast path. A failed retire may
            // leave the desired metadata unchanged while the physical member
            // is already gone and its exact provider grant is still held.
            self.release_broken_lease_locked(&identity).await?;
            let current = self
                .entries
                .read()
                .await
                .get(&identity)
                .map(|entry| (entry.spec.clone(), entry.state));
            let Some((current_spec, state)) = current else {
                continue;
            };
            if current_spec == desired_spec {
                self.accept_bootstrap_spec_locked(desired_spec, generation)
                    .await?;
                continue;
            }
            if matches!(
                state,
                IdentityLifecycleState::Retiring | IdentityLifecycleState::Suspended
            ) {
                return Err(IdentityRuntimeError::InvalidState {
                    identity,
                    state,
                    operation: "reconcile_roster_replace",
                });
            }
            // REQ-33 and `compute_reconcile_actions` classify every
            // same-profile change (addressability, labels, display name,
            // context, and instructions) as a metadata hot reload. Retiring
            // an Active member here needlessly rotates its exact grant and can
            // collide with a still-draining session during eager restore.
            if current_spec.profile == desired_spec.profile {
                self.accept_bootstrap_spec_locked(desired_spec, generation)
                    .await?;
                continue;
            }
            match state {
                IdentityLifecycleState::Active => {
                    self.retire_locked(&identity).await?;
                }
                IdentityLifecycleState::Dormant
                | IdentityLifecycleState::Broken
                | IdentityLifecycleState::Uninitialized => {}
                IdentityLifecycleState::Retiring | IdentityLifecycleState::Suspended => {
                    return Err(IdentityRuntimeError::InvalidState {
                        identity,
                        state,
                        operation: "reconcile_roster_replace",
                    });
                }
            }
            self.accept_bootstrap_spec_locked(desired_spec, generation)
                .await?;
        }

        Ok(())
    }

    /// Adopt the roster's CURRENT spec for `identity` into the in-memory entry,
    /// so a subsequent [`reset`](Self::reset) rebuilds the regenerated session
    /// on the current profile instead of carrying the stored one forward.
    ///
    /// Best-effort: logs and leaves the stored spec in place if the roster
    /// can't be resolved or no longer lists the identity (reset's primary job —
    /// the destructive continuity reset — must not fail because the roster
    /// provider hiccuped). The runtime stays roster-agnostic; the provider is
    /// supplied by the caller (the reset RPC handler), which owns it.
    pub async fn adopt_roster_spec(
        &self,
        roster_provider: &Arc<dyn RosterProvider>,
        identity: &AgentIdentity,
    ) {
        self.adopt_roster_spec_with_context(roster_provider, identity, None)
            .await;
    }

    async fn adopt_roster_spec_with_context(
        &self,
        roster_provider: &Arc<dyn RosterProvider>,
        identity: &AgentIdentity,
        mob_definition: Option<meerkat_mob::MobDefinition>,
    ) {
        match roster_provider
            .roster(&RosterContext {
                mob_definition,
                previous_identities: Vec::new(),
            })
            .await
        {
            Ok(specs) => {
                if let Some(spec) = specs.into_iter().find(|s| &s.identity == identity)
                    && let Err(err) = self.update_spec(spec).await
                {
                    tracing::warn!(
                        identity = %identity,
                        error = %err,
                        "reset: failed to adopt current roster spec; rebuilding on stored spec",
                    );
                }
            }
            Err(err) => {
                tracing::warn!(
                    identity = %identity,
                    error = %err,
                    "reset: roster provider failed; rebuilding on stored spec",
                );
            }
        }
    }

    /// Update the lease for an identity.
    pub async fn update_lease(
        &self,
        identity: &AgentIdentity,
        grant: LeaseGrant,
    ) -> Result<(), IdentityRuntimeError> {
        let fencing_token = grant.fencing_token;
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        entry.lease = Some(LeaseEntry {
            fencing_token,
            ttl: grant.ttl,
            acquired_at: Instant::now(),
        });
        drop(entries);
        self.lease_renewal_notify.notify_one();
        self.emit_event(
            identity,
            IdentityEvent::LeaseUpdated {
                identity: identity.clone(),
                fencing_token,
            },
        )
        .await;
        Ok(())
    }

    /// Mark a lease as lost for an identity (INV-02).
    pub async fn mark_lease_lost(
        &self,
        identity: &AgentIdentity,
    ) -> Result<(), IdentityRuntimeError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        entry.state = IdentityLifecycleState::Broken;
        entry.lease = None;
        drop(entries);
        self.mark_bootstrap_from_lifecycle(
            identity,
            IdentityLifecycleState::Broken,
            Some("external lease authority was lost".to_string()),
        );
        self.emit_event(
            identity,
            IdentityEvent::LeaseLost {
                identity: identity.clone(),
            },
        )
        .await;
        Ok(())
    }

    /// Remove an identity from the runtime.
    #[allow(dead_code)]
    pub(crate) async fn remove(&self, identity: &AgentIdentity) -> Option<IdentityEntry> {
        self.event_channels.write().await.remove(identity);
        self.entries.write().await.remove(identity)
    }

    /// Set the lifecycle state for an identity.
    pub async fn set_state(
        &self,
        identity: &AgentIdentity,
        state: IdentityLifecycleState,
    ) -> Result<(), IdentityRuntimeError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        entry.state = state;
        drop(entries);
        if state == IdentityLifecycleState::Active {
            self.lease_renewal_notify.notify_one();
        }
        self.emit_event(
            identity,
            IdentityEvent::StateChanged {
                identity: identity.clone(),
                new_state: state,
            },
        )
        .await;
        Ok(())
    }

    /// Fail the exact active embodiment when its live session has been torn
    /// down underneath the identity layer. A stale close notification from a
    /// replaced generation is ignored, while the current embodiment becomes
    /// `Broken` so the existing continuity repair supervisor can rebuild it.
    pub(crate) async fn mark_active_runtime_broken(
        &self,
        identity: &AgentIdentity,
        expected_runtime_id: &str,
        expected_fencing_token: u64,
        detail: &str,
    ) -> Result<bool, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        let changed = {
            let mut entries = self.entries.write().await;
            let entry = entries
                .get_mut(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            let is_current_active = entry.state == IdentityLifecycleState::Active
                && entry
                    .continuity
                    .as_ref()
                    .is_some_and(|record| record.agent_runtime_id.as_str() == expected_runtime_id)
                && entry
                    .lease
                    .as_ref()
                    .is_some_and(|lease| lease.fencing_token.get() == expected_fencing_token);
            if is_current_active {
                // Retain the exact live lease. Broken repair owns lower-plane
                // cleanup and releases that grant before rematerializing.
                entry.state = IdentityLifecycleState::Broken;
            }
            is_current_active
        };
        if changed {
            self.mark_bootstrap_from_lifecycle(
                identity,
                IdentityLifecycleState::Broken,
                Some(detail.to_string()),
            );
            self.emit_event(
                identity,
                IdentityEvent::StateChanged {
                    identity: identity.clone(),
                    new_state: IdentityLifecycleState::Broken,
                },
            )
            .await;
        }
        Ok(changed)
    }

    // -----------------------------------------------------------------------
    // Lease checking (INV-01, INV-02)
    // -----------------------------------------------------------------------

    /// Check that the identity has an active, non-expired lease.
    /// Returns the fencing token if valid.
    fn check_lease(entry: &IdentityEntry) -> Result<FencingToken, IdentityRuntimeError> {
        match &entry.lease {
            Some(lease) if !lease.is_expired() => Ok(lease.fencing_token),
            Some(_) => Err(IdentityRuntimeError::LeaseLost(entry.spec.identity.clone())),
            None => Err(IdentityRuntimeError::NoActiveLease(
                entry.spec.identity.clone(),
            )),
        }
    }

    fn lease_entry_from_grant(grant: &LeaseGrant) -> LeaseEntry {
        LeaseEntry {
            fencing_token: grant.fencing_token,
            ttl: grant.ttl,
            acquired_at: Instant::now(),
        }
    }

    /// Transfer a provider-returned active grant into runtime ownership before
    /// any fallible continuity or bridge projection. The provider call is the
    /// authority commit: once it returns, retaining only the previous token is
    /// unsafe because exact-token release would become a no-op.
    async fn stage_active_grant(
        &self,
        identity: &AgentIdentity,
        expected_previous: Option<FencingToken>,
        grant: &LeaseGrant,
    ) -> Result<Option<ContinuityRecord>, IdentityRuntimeError> {
        let staged = {
            let mut entries = self.entries.write().await;
            (|| {
                if grant.identity != *identity {
                    return Err(IdentityRuntimeError::Internal(format!(
                        "provider returned grant for {} while publishing active authority for {identity}",
                        grant.identity
                    )));
                }
                let entry = entries
                    .get_mut(identity)
                    .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
                if entry.state != IdentityLifecycleState::Active {
                    return Err(IdentityRuntimeError::InvalidState {
                        identity: identity.clone(),
                        state: entry.state,
                        operation: "stage_active_grant",
                    });
                }
                let current = entry
                    .lease
                    .as_ref()
                    .ok_or_else(|| IdentityRuntimeError::NoActiveLease(identity.clone()))?;
                if let Some(expected) = expected_previous
                    && current.fencing_token != expected
                {
                    return Err(IdentityRuntimeError::Internal(format!(
                        "active lease for {identity} changed from fencing token {expected} to {} before renewed authority could be staged",
                        current.fencing_token
                    )));
                }
                if let Some(pending) = entry.pending_lease_release.as_ref() {
                    return Err(IdentityRuntimeError::Internal(format!(
                        "active identity {identity} already has pending fencing token {} while staging {}",
                        pending.fencing_token, grant.fencing_token
                    )));
                }

                let continuity = entry.continuity.clone();
                // Broken + pending is the fail-closed intermediate state.
                // The lifecycle lock prevents another operation from
                // observing it as a completed transition, while a dropped
                // caller still leaves repair/shutdown an exact token.
                entry.state = IdentityLifecycleState::Broken;
                entry.lease = None;
                entry.pending_lease_release = Some(grant.clone());
                Ok(continuity)
            })()
        };

        match staged {
            Ok(continuity) => Ok(continuity),
            Err(error) => {
                if let Err(cleanup_error) = self
                    .release_or_park_untracked_leases(std::slice::from_ref(grant))
                    .await
                {
                    return Err(IdentityRuntimeError::Internal(format!(
                        "{error}; exact grant cleanup failed: {cleanup_error}"
                    )));
                }
                Err(error)
            }
        }
    }

    async fn fail_staged_active_grant(
        &self,
        identity: &AgentIdentity,
        grant: &LeaseGrant,
        detail: String,
    ) {
        let retained_by_entry = {
            let mut entries = self.entries.write().await;
            entries.get_mut(identity).is_some_and(|entry| {
                if entry
                    .pending_lease_release
                    .as_ref()
                    .is_some_and(|pending| pending.fencing_token == grant.fencing_token)
                {
                    entry.state = IdentityLifecycleState::Broken;
                    entry.lease = None;
                    true
                } else {
                    false
                }
            })
        };
        if !retained_by_entry {
            self.park_unactivated_lease_releases(std::slice::from_ref(grant))
                .await;
        }
        self.mark_bootstrap_from_lifecycle(identity, IdentityLifecycleState::Broken, Some(detail));
        self.emit_event(
            identity,
            IdentityEvent::StateChanged {
                identity: identity.clone(),
                new_state: IdentityLifecycleState::Broken,
            },
        )
        .await;
    }

    async fn suspend_bridge_continuity(
        &self,
        continuity: Option<&ContinuityRecord>,
    ) -> Result<(), IdentityRuntimeError> {
        let (Some(bridge), Some(record)) = (self.bridge.as_ref(), continuity) else {
            return Ok(());
        };
        bridge
            .suspend_session_runtime_state(&record.session_id)
            .await
            .map_err(|error| {
                IdentityRuntimeError::Internal(format!(
                    "suspend bridge session runtime state before authority rotation: {error}"
                ))
            })
    }

    async fn resume_bridge_continuity(
        &self,
        identity: &AgentIdentity,
        continuity: Option<&ContinuityRecord>,
        grant: &LeaseGrant,
    ) -> Result<(), IdentityRuntimeError> {
        let (Some(bridge), Some(record)) = (self.bridge.as_ref(), continuity) else {
            return Ok(());
        };
        bridge
            .register_session_runtime_state(
                &record.session_id,
                identity,
                record.generation,
                record.checkpoint_version,
                grant.fencing_token,
            )
            .await
            .map(|_| ())
            .map_err(|error| {
                IdentityRuntimeError::Internal(format!(
                    "resume bridge session runtime state after pre-commit renewal failure: {error}"
                ))
            })
    }

    async fn break_existing_active_grant(
        &self,
        identity: &AgentIdentity,
        grant: &LeaseGrant,
        detail: String,
    ) {
        let retained = {
            let mut entries = self.entries.write().await;
            entries.get_mut(identity).is_some_and(|entry| {
                if entry
                    .lease
                    .as_ref()
                    .is_some_and(|lease| lease.fencing_token == grant.fencing_token)
                {
                    entry.state = IdentityLifecycleState::Broken;
                    entry.lease = None;
                    entry.pending_lease_release = Some(grant.clone());
                    true
                } else {
                    false
                }
            })
        };
        if !retained {
            self.park_unactivated_lease_releases(std::slice::from_ref(grant))
                .await;
        }
        self.mark_bootstrap_from_lifecycle(identity, IdentityLifecycleState::Broken, Some(detail));
        self.emit_event(
            identity,
            IdentityEvent::StateChanged {
                identity: identity.clone(),
                new_state: IdentityLifecycleState::Broken,
            },
        )
        .await;
    }

    async fn commit_staged_active_grant(
        &self,
        identity: &AgentIdentity,
        grant: &LeaseGrant,
        continuity: Option<ContinuityRecord>,
    ) -> Result<(), IdentityRuntimeError> {
        let committed = {
            let mut entries = self.entries.write().await;
            match entries.get_mut(identity) {
                Some(entry)
                    if entry
                        .pending_lease_release
                        .as_ref()
                        .is_some_and(|pending| pending.fencing_token == grant.fencing_token) =>
                {
                    if let Some(record) = continuity {
                        entry.checkpoint_version = record.checkpoint_version;
                        entry.continuity = Some(record);
                    }
                    entry.pending_lease_release = None;
                    entry.lease = Some(Self::lease_entry_from_grant(grant));
                    entry.state = IdentityLifecycleState::Active;
                    Ok(())
                }
                Some(entry) => Err(IdentityRuntimeError::Internal(format!(
                    "staged fencing token {} for {identity} was replaced before commit (state {:?})",
                    grant.fencing_token, entry.state
                ))),
                None => Err(IdentityRuntimeError::UnknownIdentity(identity.clone())),
            }
        };
        if let Err(error) = committed {
            self.park_unactivated_lease_releases(std::slice::from_ref(grant))
                .await;
            return Err(error);
        }
        Ok(())
    }

    async fn publish_active_grant(
        &self,
        identity: &AgentIdentity,
        expected_previous: Option<FencingToken>,
        grant: &LeaseGrant,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        let mut continuity = self
            .stage_active_grant(identity, expected_previous, grant)
            .await?;

        // `ensure_active_lease` suspends before invoking the provider so no
        // stale-token save can overlap the authority rotation. Keep this
        // idempotent suspension here as a defensive boundary for callers that
        // already hold a provider-returned active grant (restore compatibility).
        if let Err(error) = self.suspend_bridge_continuity(continuity.as_ref()).await {
            self.fail_staged_active_grant(identity, grant, error.to_string())
                .await;
            return Err(error);
        }

        if let Some(record) = continuity.as_mut() {
            if let Err(error) = self
                .continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
            {
                let runtime_error = IdentityRuntimeError::Store(error);
                self.fail_staged_active_grant(
                    identity,
                    grant,
                    format!("active grant continuity publication failed: {runtime_error}"),
                )
                .await;
                return Err(runtime_error);
            }
            if let Some(bridge) = self.bridge.as_ref() {
                match bridge
                    .register_session_runtime_state(
                        &record.session_id,
                        identity,
                        record.generation,
                        record.checkpoint_version,
                        grant.fencing_token,
                    )
                    .await
                {
                    Ok(version) => {
                        record.checkpoint_version = CheckpointVersion::new(
                            record.checkpoint_version.get().max(version.get()),
                        );
                    }
                    Err(error) => {
                        let runtime_error = IdentityRuntimeError::Internal(format!(
                            "bridge refresh session runtime state after active grant publication: {error}"
                        ));
                        let _ = self.suspend_bridge_continuity(Some(record)).await;
                        self.fail_staged_active_grant(identity, grant, runtime_error.to_string())
                            .await;
                        return Err(runtime_error);
                    }
                }
            }
        }

        let session_to_reference = continuity.as_ref().map(|record| record.session_id.clone());
        if let Err(error) = self
            .commit_staged_active_grant(identity, grant, continuity)
            .await
        {
            if let (Some(bridge), Some(session_id)) =
                (self.bridge.as_ref(), session_to_reference.as_ref())
            {
                let _ = bridge.suspend_session_runtime_state(session_id).await;
            }
            return Err(error);
        }
        self.lease_renewal_notify.notify_one();
        self.emit_event(
            identity,
            IdentityEvent::LeaseUpdated {
                identity: identity.clone(),
                fencing_token: grant.fencing_token,
            },
        )
        .await;
        Ok(grant.fencing_token)
    }

    async fn ensure_active_lease(
        &self,
        identity: &AgentIdentity,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        let (grant, continuity) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            let lease = match &entry.lease {
                Some(lease) if lease.is_healthy() => return Ok(lease.fencing_token),
                Some(lease) => lease,
                None => return Err(IdentityRuntimeError::NoActiveLease(identity.clone())),
            };
            (
                LeaseGrant {
                    identity: identity.clone(),
                    fencing_token: lease.fencing_token,
                    ttl: lease.ttl,
                },
                entry.continuity.clone(),
            )
        };

        // Drain every already-admitted persistence mutation before asking the
        // provider to rotate N to N+1. Once this returns, all session-store
        // mutations fail closed until register_session_runtime_state publishes
        // the exact replacement token.
        if let Err(suspend_error) = self.suspend_bridge_continuity(continuity.as_ref()).await {
            if let Err(resume_error) = self
                .resume_bridge_continuity(identity, continuity.as_ref(), &grant)
                .await
            {
                self.break_existing_active_grant(
                    identity,
                    &grant,
                    format!("{suspend_error}; bridge rollback failed: {resume_error}"),
                )
                .await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "{suspend_error}; bridge rollback failed: {resume_error}"
                )));
            }
            return Err(suspend_error);
        }

        let renewed = match self
            .lease_provider
            .renew_leases(std::slice::from_ref(&grant))
            .await
        {
            Ok(renewed) => renewed,
            Err(error) => {
                let runtime_error = IdentityRuntimeError::Lease(error);
                if let Err(resume_error) = self
                    .resume_bridge_continuity(identity, continuity.as_ref(), &grant)
                    .await
                {
                    self.break_existing_active_grant(
                        identity,
                        &grant,
                        format!("{runtime_error}; bridge rollback failed: {resume_error}"),
                    )
                    .await;
                    return Err(IdentityRuntimeError::Internal(format!(
                        "{runtime_error}; bridge rollback failed: {resume_error}"
                    )));
                }
                return Err(runtime_error);
            }
        };
        let renewed_grant = match renewed.get(identity) {
            Some(super::types::LeaseRenewResult::Renewed(grant)) => grant.clone(),
            Some(super::types::LeaseRenewResult::Lost { .. }) | None => {
                self.mark_lease_lost(identity).await?;
                return Err(IdentityRuntimeError::LeaseLost(identity.clone()));
            }
        };

        self.publish_active_grant(identity, Some(grant.fencing_token), &renewed_grant)
            .await
    }

    async fn mark_lifecycle_in_progress(
        &self,
        identity: &AgentIdentity,
        state: IdentityLifecycleState,
    ) -> Result<IdentityEntry, IdentityRuntimeError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        let snapshot = entry.clone();
        entry.state = state;
        entry.lease = None;
        drop(entries);
        self.mark_bootstrap_materialization_started(identity, None);
        Ok(snapshot)
    }

    async fn restore_entry(&self, identity: &AgentIdentity, entry: IdentityEntry) {
        let state = entry.state;
        self.entries.write().await.insert(identity.clone(), entry);
        self.mark_bootstrap_from_lifecycle(identity, state, None);
    }

    async fn restore_entry_with_grant(
        &self,
        identity: &AgentIdentity,
        mut entry: IdentityEntry,
        grant: &LeaseGrant,
    ) {
        let restore_live_lease = entry.state == IdentityLifecycleState::Active;
        if let Some(record) = entry.continuity.as_ref() {
            if let Err(err) = self
                .continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
            {
                tracing::warn!(
                    %identity,
                    error = %err,
                    "failed to advance restored continuity fencing token after lifecycle failure"
                );
                entry.state = IdentityLifecycleState::Broken;
            } else if let Some(bridge) = self.bridge.as_ref()
                && let Err(err) = bridge
                    .register_session_runtime_state(
                        &record.session_id,
                        identity,
                        record.generation,
                        record.checkpoint_version,
                        grant.fencing_token,
                    )
                    .await
            {
                tracing::warn!(
                    %identity,
                    error = %err,
                    "failed to refresh restored session runtime state after lifecycle failure"
                );
                entry.state = IdentityLifecycleState::Broken;
            }
        }
        // Preserve ownership for an originally Active entry even if bridge
        // repair marks it Broken; repair still needs the current fenced
        // grant. Originally non-Active entries must not retain a lease that
        // their restored local state does not expose.
        let retain_grant = restore_live_lease;
        entry.lease = retain_grant.then(|| Self::lease_entry_from_grant(grant));
        if !retain_grant {
            entry.pending_lease_release = Some(grant.clone());
            match self
                .lease_provider
                .release_leases(std::slice::from_ref(grant))
                .await
            {
                Ok(()) => entry.pending_lease_release = None,
                Err(err) => {
                    tracing::warn!(
                        %identity,
                        error = %err,
                        "failed to release lifecycle rollback lease for non-active identity; exact grant parked for retry"
                    );
                    entry.state = IdentityLifecycleState::Broken;
                }
            }
        }
        self.restore_entry(identity, entry).await;
        if retain_grant {
            self.lease_renewal_notify.notify_one();
        }
    }

    /// Reuse an already-active embodiment during roster reconciliation.
    ///
    /// The lifecycle reservation held by the restore controller makes this
    /// snapshot stable. Crucially, this path preserves the exact live lease
    /// grant instead of reacquiring/rotating authority for a member that is
    /// already running.
    pub(crate) async fn reuse_active_restore_state(
        &self,
        spec: &DurableAgentSpec,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        // The restore controller still owns this identity's lifecycle guard,
        // so validate time-sensitive external authority before snapshotting or
        // mutating the local projection. Healthy grants return unchanged;
        // due grants publish their exact renewal, and Lost authority marks the
        // identity Broken instead of reporting a successful active reconcile.
        self.ensure_active_lease(&spec.identity).await?;

        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(&spec.identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(spec.identity.clone()))?;
        if entry.state != IdentityLifecycleState::Active {
            return Err(IdentityRuntimeError::InvalidState {
                identity: spec.identity.clone(),
                state: entry.state,
                operation: "reuse active restore state",
            });
        }
        if entry.lease.is_none() {
            return Err(IdentityRuntimeError::NoActiveLease(spec.identity.clone()));
        }
        let record = entry.continuity.clone().ok_or_else(|| {
            IdentityRuntimeError::Internal(format!(
                "active identity {} has no continuity record",
                spec.identity
            ))
        })?;
        entry.spec = spec.clone();
        Ok(record)
    }

    /// Publish a fail-closed local state and relinquish the fresh external
    /// grant acquired for the failed lifecycle transaction.
    ///
    /// These ambiguous failure paths deliberately publish Broken with no local
    /// lease. Keeping the matching provider grant alive would make that
    /// projection a lie and could block another runtime from repairing the
    /// identity until the provider TTL expires (forever for the bundled
    /// single-process provider).
    async fn restore_broken_entry_and_release_grant(
        &self,
        identity: &AgentIdentity,
        mut entry: IdentityEntry,
        grant: &LeaseGrant,
    ) {
        entry.state = IdentityLifecycleState::Broken;
        entry.lease = None;
        // Park the exact grant before the provider call. If release fails (or
        // this future is abandoned after publication), reconcile/shutdown can
        // retry the same fencing token instead of leaving an invisible owner.
        entry.pending_lease_release = Some(grant.clone());
        self.restore_entry(identity, entry).await;
        match self
            .lease_provider
            .release_leases(std::slice::from_ref(grant))
            .await
        {
            Ok(()) => {
                let mut entries = self.entries.write().await;
                if let Some(entry) = entries.get_mut(identity)
                    && entry
                        .pending_lease_release
                        .as_ref()
                        .is_some_and(|pending| pending.fencing_token == grant.fencing_token)
                {
                    entry.pending_lease_release = None;
                }
            }
            Err(err) => {
                self.mark_bootstrap_from_lifecycle(
                    identity,
                    IdentityLifecycleState::Broken,
                    Some(format!("pending lease release: {err}")),
                );
                tracing::warn!(
                    %identity,
                    error = %err,
                    "failed to release lease after lifecycle transaction became Broken; exact grant parked for retry"
                );
            }
        }
    }

    async fn restore_broken_entry_with_fenced_store(
        &self,
        identity: &AgentIdentity,
        mut entry: IdentityEntry,
        grant: &LeaseGrant,
    ) {
        entry.state = IdentityLifecycleState::Broken;
        entry.lease = None;
        if let Some(record) = entry.continuity.as_ref()
            && let Err(err) = self
                .continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
        {
            tracing::warn!(
                %identity,
                error = %err,
                "failed to preserve fenced continuity record for broken identity"
            );
        }
        self.restore_broken_entry_and_release_grant(identity, entry, grant)
            .await;
    }

    /// A failed reset rollback means the caller's pre-reset entry is no
    /// longer authoritative. Re-resolve under the lifecycle transaction and
    /// publish only the store's actual record; if the store itself cannot be
    /// read, fail closed without continuity rather than claiming the old
    /// generation still owns the durable row.
    async fn restore_broken_entry_from_authoritative_continuity(
        &self,
        identity: &AgentIdentity,
        mut entry: IdentityEntry,
        grant: &LeaseGrant,
    ) {
        let authoritative = self
            .continuity_store
            .resolve_many(std::slice::from_ref(identity))
            .await;
        entry.continuity = match authoritative {
            Ok(resolved) => match resolved.get(identity) {
                Some(super::types::ContinuityResolveState::Ready { record }) => {
                    Some(record.clone())
                }
                Some(super::types::ContinuityResolveState::Broken { failure }) => {
                    failure.record.clone()
                }
                Some(super::types::ContinuityResolveState::Uninitialized) | None => None,
            },
            Err(error) => {
                tracing::warn!(
                    %identity,
                    error = %error,
                    "failed to resolve authoritative continuity after reset rollback failure"
                );
                None
            }
        };
        entry.checkpoint_version = entry
            .continuity
            .as_ref()
            .map(|record| record.checkpoint_version)
            .unwrap_or(CheckpointVersion::new(0));
        self.restore_broken_entry_and_release_grant(identity, entry, grant)
            .await;
    }

    async fn mark_rebind_failure_broken(
        &self,
        identity: &AgentIdentity,
        mut entry: IdentityEntry,
        grant: &LeaseGrant,
        rebound_record: &ContinuityRecord,
    ) {
        entry.state = IdentityLifecycleState::Broken;
        entry.lease = None;
        entry.checkpoint_version = rebound_record.checkpoint_version;
        entry.continuity = Some(rebound_record.clone());
        if let Err(err) = self
            .continuity_store
            .upsert_continuity_record(rebound_record, grant.fencing_token)
            .await
        {
            tracing::warn!(
                %identity,
                session_id = %rebound_record.session_id,
                error = %err,
                "failed to preserve rebound continuity after live respawn rebind failure"
            );
        }
        self.restore_broken_entry_and_release_grant(identity, entry, grant)
            .await;
    }

    async fn restore_entry_after_reset_bridge_failure(
        &self,
        identity: &AgentIdentity,
        expected_attempt: &ContinuityRecord,
        entry: IdentityEntry,
        grant: &LeaseGrant,
        force_broken: bool,
    ) -> Option<ContinuityStoreError> {
        let rollback_error = self
            .continuity_store
            .rollback_continuity_record(
                expected_attempt,
                entry.continuity.as_ref(),
                grant.fencing_token,
            )
            .await
            .err();
        if rollback_error.is_some() {
            self.restore_broken_entry_from_authoritative_continuity(identity, entry, grant)
                .await;
        } else if force_broken {
            self.restore_broken_entry_and_release_grant(identity, entry, grant)
                .await;
        } else {
            self.restore_entry_with_grant(identity, entry, grant).await;
        }
        rollback_error
    }

    async fn restore_continuity_after_materialize_failure(
        &self,
        identity: &AgentIdentity,
        previous: Option<&ContinuityRecord>,
        grant: &LeaseGrant,
    ) -> Option<ContinuityStoreError> {
        match previous {
            Some(record) => self
                .continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
                .err(),
            None => self
                .continuity_store
                .delete_continuity_record(identity, grant.fencing_token)
                .await
                .err(),
        }
    }

    async fn unregister_bridge_session_runtime_states(
        bridge: &dyn SessionBridge,
        session_ids: &[SessionId],
    ) -> Option<String> {
        let mut errors = Vec::new();
        let mut seen = BTreeSet::new();
        for session_id in session_ids {
            if !seen.insert(session_id.to_string()) {
                continue;
            }
            if let Err(err) = bridge.unregister_session_runtime_state(session_id).await {
                errors.push(format!("{session_id}: {err}"));
            }
        }
        (!errors.is_empty()).then(|| errors.join("; "))
    }

    async fn advance_existing_continuity_fence(
        &self,
        identity: &AgentIdentity,
        entry: &IdentityEntry,
        grant: &LeaseGrant,
    ) -> Result<(), IdentityRuntimeError> {
        if let Some(record) = entry.continuity.as_ref() {
            self.continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
                .map_err(IdentityRuntimeError::Store)?;
        }
        let _ = identity;
        Ok(())
    }

    async fn refresh_existing_session_runtime_state(
        &self,
        identity: &AgentIdentity,
        record: &ContinuityRecord,
        grant: &LeaseGrant,
    ) -> Result<CheckpointVersion, IdentityRuntimeError> {
        let Some(bridge) = self.bridge.as_ref() else {
            return Ok(record.checkpoint_version);
        };
        bridge
            .register_session_runtime_state(
                &record.session_id,
                identity,
                record.generation,
                record.checkpoint_version,
                grant.fencing_token,
            )
            .await
            .map_err(|err| {
                IdentityRuntimeError::Internal(format!(
                    "bridge refresh session runtime state: {err}"
                ))
            })
    }

    // -----------------------------------------------------------------------
    // Delivery: send() — REQ-01, REQ-03
    // -----------------------------------------------------------------------

    /// Send conversational content and return only after the runtime has
    /// COMMITTED this exact turn's terminal boundary.
    ///
    /// Same body, same enforcement, same memory/defang/alias handling as
    /// [`Self::send`]; the single difference is the bridge verb. Use this when
    /// the caller needs PROOF the turn finished rather than proof it was
    /// admitted.
    ///
    /// Why this exists rather than "send, then watch the event stream": a
    /// session-wide `RunCompleted`/`RunFailed` cannot authorize a specific
    /// turn, because queued turns share a `session_id`. Waiting on one means
    /// some OTHER turn's terminal can satisfy the wait, so a test built that
    /// way can pass while the behaviour it claims to prove is broken. A timer
    /// is worse still: it elapses whether the turn succeeded, failed closed, or
    /// never ran at all.
    ///
    /// FAILS CLOSED. If no bridge is installed, or the identity has no bound
    /// runtime id, or the bridge is ingress-only, this returns a typed error.
    /// It never silently degrades to the admission-only path - that would
    /// report success for a turn whose outcome is unknown.
    pub async fn send_awaiting_commit(
        &self,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_awaiting_commit_with_mode(identity, content, HandlingMode::Queue)
            .await
    }

    /// [`Self::send_awaiting_commit`] with an explicit turn handling mode.
    pub async fn send_awaiting_commit_with_mode(
        &self,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_awaiting_commit_with_mode_and_interaction(identity, content, handling_mode, None)
            .await
    }

    /// [`Self::send_awaiting_commit`] carrying a host-minted interaction id, so
    /// the completed turn can be looked up EXACTLY afterwards by the identity
    /// the caller stamped on it - rather than by "the last request", by a
    /// request count, or by polling content, none of which name one turn.
    pub async fn send_awaiting_commit_with_mode_and_interaction(
        &self,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_core(
            identity,
            SendRequest {
                expected_alias: None,
                content,
                system_prompt: None,
                handling_mode,
                interaction_id,
                commit_mode: SendCommitMode::AwaitCommit,
            },
        )
        .await
        .map(|(token, _)| token)
    }

    /// [`Self::send_awaiting_commit`] carrying one ordinary System message
    /// authored for THIS exact turn (meerkat 0.8.11 `WorkSpec::system_prompt`,
    /// appended at the turn's admitted transcript boundary).
    ///
    /// Per-turn content, never member or session configuration - the same
    /// carrier the ingress authored path uses, on the lane that can prove the
    /// turn finished.
    pub async fn send_awaiting_commit_with_system_prompt(
        &self,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
        system_prompt: Option<&str>,
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_core(
            identity,
            SendRequest {
                expected_alias: None,
                content,
                system_prompt,
                handling_mode,
                interaction_id,
                commit_mode: SendCommitMode::AwaitCommit,
            },
        )
        .await
        .map(|(token, _)| token)
    }

    /// Send conversational content to an addressable identity.
    ///
    /// Enforces:
    /// - Identity must be registered and active
    /// - Identity must be Addressable (REQ-03)
    /// - Lease must be held (INV-01)
    /// - Lease must not be lost (INV-02)
    ///
    /// Returns the fencing token for the delivery (caller uses it for checkpoint).
    pub async fn send(
        &self,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_with_mode(identity, content, HandlingMode::Queue)
            .await
    }

    /// Cancellation-safe queue send for RPC/host request boundaries.
    pub async fn send_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_with_mode_tracked(identity, content, HandlingMode::Queue)
            .await
    }

    /// Send conversational content using an explicit turn handling mode.
    ///
    /// This is the identity-first counterpart to the mob member send path used
    /// by the console. Ordinary API callers can keep using [`Self::send`],
    /// which preserves queue semantics.
    pub async fn send_with_mode(
        &self,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_with_mode_and_interaction(identity, content, handling_mode, None)
            .await
    }

    /// Cancellation-safe send that also returns the completion baseline to
    /// wait past.
    ///
    /// This is the delivery entry point every surface should use when the
    /// caller intends to wait for the answer: it is the only one that hands
    /// back a [`CompletionCursor`] captured before delivery, which is what
    /// makes "wait for MY turn" expressible without comparing output text.
    /// `expected_alias` pins the delivery to a generated runtime alias the
    /// caller already resolved; pass `None` for the durable identity.
    pub async fn send_admission_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<SendAdmission, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.map(ToString::to_string);
        let content = content.clone();
        let interaction_id = interaction_id.map(ToString::to_string);
        self.run_tracked_foreground(async move {
            runtime
                .send_with_mode_and_interaction_with_expected_member_alias(
                    &identity,
                    expected_alias.as_deref(),
                    &content,
                    handling_mode,
                    interaction_id.as_deref(),
                )
                .await
                .map(|(fencing_token, completion_baseline)| SendAdmission {
                    fencing_token,
                    completion_baseline,
                })
        })
        .await
    }

    /// Cancellation-safe explicit-mode send for RPC/host request boundaries.
    pub async fn send_with_mode_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_with_mode_and_interaction_tracked(identity, content, handling_mode, None)
            .await
    }

    /// Cancellation-safe interaction send for RPC/host request boundaries.
    pub async fn send_with_mode_and_interaction_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_admission_tracked(identity, None, content, handling_mode, interaction_id)
            .await
            .map(|admission| admission.fencing_token)
    }

    /// Cancellation-safe interaction send pinned to the generated runtime
    /// alias that the caller resolved. Validation and delivery share the
    /// identity lifecycle lock, so a concurrent reset cannot retarget the
    /// request onto the replacement generation.
    pub async fn send_with_mode_and_interaction_member_alias_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: &str,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_admission_tracked(
            identity,
            Some(expected_alias),
            content,
            handling_mode,
            interaction_id,
        )
        .await
        .map(|admission| admission.fencing_token)
    }

    /// [`Self::send_with_mode`] with a host-minted interaction id (meerkat
    /// 0.7.25 ask 15 addendum). The id rides `WorkSpec` into runtime
    /// admission, so the turn's live events and its committed transcript
    /// messages carry the same identity the console stamped on its frames —
    /// the exact live↔history join the console dedup needs. Only UUID-form
    /// ids thread; others are delivered without one.
    pub async fn send_with_mode_and_interaction(
        &self,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_with_mode_and_interaction_with_expected_member_alias(
            identity,
            None,
            content,
            handling_mode,
            interaction_id,
        )
        .await
        .map(|(token, _)| token)
    }

    async fn send_with_mode_and_interaction_with_expected_member_alias(
        &self,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
        interaction_id: Option<&str>,
    ) -> Result<(FencingToken, CompletionCursor), IdentityRuntimeError> {
        self.send_core(
            identity,
            SendRequest {
                expected_alias,
                content,
                system_prompt: None,
                handling_mode,
                interaction_id,
                commit_mode: SendCommitMode::Ingress,
            },
        )
        .await
    }

    /// The ONE send body. Both lanes run it; the only difference is which
    /// bridge verb the delivery step calls, which is exactly the part that must
    /// not drift. A second copy would let the completion lane diverge from the
    /// ingress lane on lease acquisition, alias pinning, memory injection,
    /// defanging or session reconciliation - every one of which is load-bearing
    /// and none of which is visible from a test.
    async fn send_core(
        &self,
        identity: &AgentIdentity,
        request: SendRequest<'_>,
    ) -> Result<(FencingToken, CompletionCursor), IdentityRuntimeError> {
        let SendRequest {
            expected_alias,
            content,
            system_prompt,
            handling_mode,
            interaction_id,
            commit_mode,
        } = request;
        let should_materialize = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;

            // REQ-03: reject send to InternalOnly
            if entry.spec.addressability == AgentAddressability::InternalOnly {
                return Err(IdentityRuntimeError::NotAddressable(NotAddressable {
                    identity: identity.clone(),
                    addressability: entry.spec.addressability,
                }));
            }
            entry.state == IdentityLifecycleState::Dormant
                || entry.state == IdentityLifecycleState::Uninitialized
        };
        if should_materialize {
            self.materialize_with_expected_member_alias(identity, expected_alias)
                .await?;
        }
        // Live steers are latency-sensitive operator input for an already
        // active turn. Ordinary sends may hydrate the reachable topology first,
        // but a steer must reach the current session boundary before the tool
        // turn resumes; background/full-fleet materialization owns the peers.
        if handling_mode != HandlingMode::Steer {
            if let Some(expected_alias) = expected_alias {
                let lifecycle_lock = self.lifecycle_lock_for(identity).await;
                let _lifecycle_guard = lifecycle_lock.lock().await;
                self.ensure_expected_member_alias_current(identity, expected_alias)
                    .await?;
            }
            self.materialize_reachable_peers(identity).await?;
        }

        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        if let Some(expected_alias) = expected_alias {
            self.ensure_expected_member_alias_current(identity, expected_alias)
                .await?;
        }
        {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            if entry.state != IdentityLifecycleState::Active {
                return Err(IdentityRuntimeError::InvalidState {
                    identity: identity.clone(),
                    state: entry.state,
                    operation: "send",
                });
            }
        }

        let mut token = self.ensure_active_lease(identity).await?;
        // Read the completion baseline BEFORE delivery is attempted. The turn
        // this send starts can only complete after this point, so a caller
        // waiting past the baseline cannot miss it. The converse ambiguity is
        // deliberate and documented: on an identity receiving concurrent
        // traffic, another delivery's completion can also satisfy the wait.
        // Waiting too little beats the failure this replaces (waiting forever).
        let completion_baseline = self.rebase_completion_cursor(identity, token);
        let (runtime_id, memory_session_key, memory_generation, bridge_interaction_id) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            (
                entry
                    .continuity
                    .as_ref()
                    .map(|c| c.agent_runtime_id.clone()),
                // Scopes the injector's cross-turn dedup + cumulative budget.
                entry.continuity.as_ref().map(|c| c.session_id.to_string()),
                entry.continuity.as_ref().map(|c| c.generation.get()),
                interaction_id_for_delivery(&entry.spec, interaction_id),
            )
        };
        let (content_to_deliver, injected_context) = self
            .prepare_member_delivery(
                identity,
                content,
                memory_session_key.as_deref(),
                memory_generation,
                handling_mode == HandlingMode::Steer,
            )
            .await?;

        // Deliver through the session bridge when available.
        //
        // AwaitCommit FAILS CLOSED here and nowhere else: if there is no bridge
        // or no runtime id, the ingress lane legitimately no-ops, but the
        // completion lane must NOT - silently taking the ingress path would
        // return success for a turn that was never awaited, which is the exact
        // false-success this API exists to remove.
        if commit_mode == SendCommitMode::AwaitCommit
            && (self.bridge.is_none() || runtime_id.is_none())
        {
            return Err(IdentityRuntimeError::CompletionUnavailable {
                identity: identity.clone(),
                reason: if self.bridge.is_none() {
                    "no session bridge is installed on this runtime".to_string()
                } else {
                    "the identity has no bound agent runtime id".to_string()
                },
            });
        }
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id) {
            let delivered_session_id = match commit_mode {
                // Validated -> Delivered. One await, all of it bounded, all of
                // it under the lock. Unchanged.
                SendCommitMode::Ingress => bridge
                    .deliver_with_mode_context_and_system_prompt(
                        rid,
                        &content_to_deliver,
                        system_prompt,
                        &injected_context,
                        handling_mode,
                        bridge_interaction_id,
                    )
                    .await
                    .map_err(|e| IdentityRuntimeError::Internal(format!("bridge deliver: {e}")))?,

                // Validated -> Admitted(receipt) -> [UNLOCK] -> Terminal ->
                // [RELOCK] -> Revalidated -> Reconciled | Superseded.
                //
                // The lock covers admission and bounded session resolution and
                // NOTHING else. Awaiting an LLM turn under it would serialise
                // same-identity sends behind a model call and block every
                // lifecycle operation - reset, retire, alias rebind - for the
                // turn's whole duration.
                SendCommitMode::AwaitCommit => {
                    // ADMITTED. Bounded; still under the lock.
                    let receipt = bridge
                        .begin_awaiting_commit(
                            rid,
                            &content_to_deliver,
                            system_prompt,
                            &injected_context,
                            handling_mode,
                            bridge_interaction_id,
                        )
                        .await
                        .map_err(|err| admission_phase_error(identity, err))?;

                    // CAPTURE the incarnation we admitted onto, so a reset,
                    // retire or alias rebind during the unlocked window is
                    // detectable rather than silently reconciled onto.
                    let captured = self.capture_incarnation(identity).await?;

                    // UNLOCK. Nothing is held from here.
                    drop(_lifecycle_guard);

                    // TERMINAL. The LLM turn. The only long step.
                    let terminal = receipt.wait().await;

                    // RELOCK.
                    let _relock_guard = lifecycle_lock.lock().await;

                    // DETECT supersede WITHOUT returning on it. What the turn
                    // did and whether its embodiment still exists are
                    // INDEPENDENT axes, so neither may swallow the other. A
                    // post-lock read failure IS a supersede - the identity was
                    // retired or deleted while the turn ran - and must never
                    // surface as a pre-admission-shaped UnknownIdentity, which
                    // would say nothing was delivered.
                    let supersede: Option<String> = match self.capture_incarnation(identity).await {
                        Err(IdentityRuntimeError::UnknownIdentity(_)) => {
                            Some("the identity no longer exists".to_string())
                        }
                        Err(other) => Some(format!("the identity could not be re-read: {other}")),
                        Ok(current) if current != captured => {
                            Some(format!("admitted onto {captured:?}, now {current:?}"))
                        }
                        // Re-run the ORIGINAL expected alias through the
                        // authoritative check rather than comparing a copy
                        // of it. Any failure is a POST-admission supersede.
                        Ok(_) => match expected_alias {
                            Some(expected) => match self
                                .ensure_expected_member_alias_current(identity, expected)
                                .await
                            {
                                Ok(()) => None,
                                Err(err) => Some(format!(
                                    "the expected member alias {expected} is no longer \
                                         current: {err}"
                                )),
                            },
                            None => None,
                        },
                    };

                    // REVALIDATE BEFORE RECONCILE is load-bearing rather than
                    // defensive: `reconcile_delivered_session_locked` treats a
                    // session mismatch as "the bridge rotated the session" and
                    // calls `rebind_session_after_live_respawn_locked`, which
                    // SUSPENDS the identity and RE-ACQUIRES its leases. After a
                    // reset during the unlocked window, current belongs to the
                    // NEW incarnation and delivered to the dead OLD turn, so
                    // reconciling first would bind the live incarnation to a
                    // dead turn's session - an active corruption path with a
                    // lifecycle mutation in it.
                    return match (terminal, supersede) {
                        // The turn did not reach a clean success. Nothing was
                        // superseded, so this is the ordinary phase mapping.
                        (Err(err), None) => Err(turn_phase_error(identity, err)),
                        // Both axes fired. Which one leads depends on what the
                        // turn actually did, and the two phases mean opposite
                        // things about that.
                        (
                            Err(BridgeTurnError::PostAdmissionResolutionFailed(detail)),
                            Some(sup),
                        ) => Err(IdentityRuntimeError::PostAdmissionSuperseded {
                            identity: identity.clone(),
                            detail: format!(
                                "{sup}; the turn also failed to project its session: \
                                     {detail}"
                            ),
                        }),
                        // The turn RAN AND FAILED. That is what an operator
                        // acts on, so it leads; the supersede is secondary.
                        (Err(BridgeTurnError::CompletionFailed(detail)), Some(sup)) => {
                            Err(IdentityRuntimeError::CompletionFailed {
                                identity: identity.clone(),
                                detail: format!(
                                    "{detail}; additionally, the identity was superseded \
                                     while the turn ran: {sup}"
                                ),
                            })
                        }
                        // The turn SUCCEEDED against an embodiment that is
                        // gone. Do NOT reconcile: losing the attribution beats
                        // binding the live incarnation to a dead turn's session.
                        (Ok(_), Some(sup)) => Err(IdentityRuntimeError::PostAdmissionSuperseded {
                            identity: identity.clone(),
                            detail: sup,
                        }),
                        // RECONCILED. Same relocked guard the ingress lane
                        // would have held throughout.
                        (Ok(delivered), None) => {
                            if let Some(rebound_token) = self
                                .reconcile_delivered_session_locked(identity, delivered)
                                .await?
                            {
                                token = rebound_token;
                            }
                            Ok((token, completion_baseline))
                        }
                    };
                }
            };
            if let Some(rebound_token) = self
                .reconcile_delivered_session_locked(identity, delivered_session_id)
                .await?
            {
                token = rebound_token;
            }
        }

        Ok((token, completion_baseline))
    }

    /// The exact incarnation a delivery was admitted onto.
    ///
    /// Captured under the lifecycle lock at admission and compared under it
    /// again after the turn, so any reset, retire or alias rebind during the
    /// unlocked window is DETECTED rather than silently reconciled onto.
    ///
    /// THREE fields, all read from authoritative state: a fresh spawn moves
    /// `runtime_id`, a resume moves `generation`, a lease re-acquisition moves
    /// `fencing_token`.
    ///
    /// The member alias is deliberately NOT captured here. Deriving it from
    /// `agent_runtime_id` would have made it a duplicate of a field already in
    /// this struct rather than an independent signal, so it could never have
    /// detected the alias divergence it was there for. The alias is instead
    /// revalidated after relock by re-running the ORIGINAL expected alias
    /// through `ensure_expected_member_alias_current`, which is the authority.
    async fn capture_incarnation(
        &self,
        identity: &AgentIdentity,
    ) -> Result<CapturedIncarnation, IdentityRuntimeError> {
        let entries = self.entries.read().await;
        let entry = entries
            .get(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        Ok(CapturedIncarnation {
            runtime_id: entry
                .continuity
                .as_ref()
                .map(|c| c.agent_runtime_id.clone()),
            generation: entry.continuity.as_ref().map(|c| c.generation.get()),
            fencing_token: entry.lease.as_ref().map(|lease| lease.fencing_token),
        })
    }

    // -----------------------------------------------------------------------
    // Delivery: dispatch() — REQ-02
    // -----------------------------------------------------------------------

    /// Dispatch internal content to any identity (Addressable or InternalOnly).
    ///
    /// Enforces:
    /// - Identity must be registered and active
    /// - Lease must be held (INV-01)
    /// - Lease must not be lost (INV-02)
    ///
    /// Returns (fencing_token, is_durable) where is_durable indicates whether
    /// the dispatch is backed by a runtime_store (REQ-04).
    pub async fn dispatch(
        &self,
        identity: &AgentIdentity,
        input: &DispatchInput,
    ) -> Result<(FencingToken, bool), IdentityRuntimeError> {
        self.dispatch_with_expected_member_alias(identity, None, input)
            .await
            .map(|outcome| (outcome.admission.fencing_token, outcome.admission.durable))
    }

    /// The ONE delivery preparation both member doors run (task #54): the
    /// send door always had it; the dispatch door previously delivered raw,
    /// so internal dispatches (schedules foremost) skipped defanging, taint
    /// session attribution, and ambient memory injection for the member's
    /// whole lifetime (HomeCore: zero surface=Turn injection-ledger rows).
    ///
    /// Steer is latency-sensitive live operator input: it bypasses both
    /// memory injection and inbound defanging by design. Every other
    /// delivery is defanged first (§9.1 anti-spoofing - even with injection
    /// off, forged memory envelopes are an inbound threat) and only then
    /// considered for ambient injection. Ask 1: the user content and the
    /// ambient recall travel as SEPARATE bodies - the (defanged) message and
    /// the recall as its own typed injected-context body, never fused.
    /// §10.1 taint hook: `note_current_session` keeps session attribution
    /// authoritative ahead of the async observe stream; the generation bind
    /// feeds the Distiller's EvidenceRefs (§8.4).
    async fn prepare_member_delivery(
        &self,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
        memory_session_key: Option<&str>,
        memory_generation: Option<u64>,
        steer: bool,
    ) -> Result<(meerkat_core::ContentInput, Vec<meerkat_core::ContentInput>), IdentityRuntimeError>
    {
        if steer {
            return Ok((content.clone(), Vec::new()));
        }
        match self.agent_memory.read().await.clone() {
            Some(injector) => {
                if let Some(session_key) = memory_session_key {
                    injector.note_current_session(identity, session_key);
                    if let Some(generation) = memory_generation {
                        injector.note_session_generation(identity, session_key, generation);
                    }
                }
                let defanged = injector.defang_inbound(identity, content);
                let injected_context = injector
                    .inject_for_turn(identity, memory_session_key, &defanged)
                    .await
                    .map_err(|err| {
                        IdentityRuntimeError::Internal(format!("agent memory recall: {err}"))
                    })?;
                Ok((defanged, injected_context))
            }
            None => Ok((content.clone(), Vec::new())),
        }
    }

    async fn dispatch_with_expected_member_alias(
        &self,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
        input: &DispatchInput,
    ) -> Result<DispatchOutcome, IdentityRuntimeError> {
        let should_materialize = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            entry.state == IdentityLifecycleState::Dormant
                || entry.state == IdentityLifecycleState::Uninitialized
        };
        if should_materialize {
            self.materialize_with_expected_member_alias(identity, expected_alias)
                .await?;
        }
        if let Some(expected_alias) = expected_alias {
            let lifecycle_lock = self.lifecycle_lock_for(identity).await;
            let _lifecycle_guard = lifecycle_lock.lock().await;
            self.ensure_expected_member_alias_current(identity, expected_alias)
                .await?;
        }
        self.materialize_reachable_peers(identity).await?;

        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        if let Some(expected_alias) = expected_alias {
            self.ensure_expected_member_alias_current(identity, expected_alias)
                .await?;
        }
        {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            if entry.state != IdentityLifecycleState::Active {
                return Err(IdentityRuntimeError::InvalidState {
                    identity: identity.clone(),
                    state: entry.state,
                    operation: "dispatch",
                });
            }
        }

        let mut token = self.ensure_active_lease(identity).await?;
        // Same pre-delivery baseline contract as the send path — see
        // `send_with_mode_and_interaction_with_expected_member_alias`.
        let completion_baseline = self.rebase_completion_cursor(identity, token);
        let (is_durable, runtime_id, memory_session_key, memory_generation) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            // REQ-04: durability depends on runtime_store
            let is_durable = entry.has_runtime_store;

            let runtime_id = entry
                .continuity
                .as_ref()
                .map(|c| c.agent_runtime_id.clone());

            (
                is_durable,
                runtime_id,
                entry.continuity.as_ref().map(|c| c.session_id.to_string()),
                entry.continuity.as_ref().map(|c| c.generation.get()),
            )
        };

        // Task #54: the dispatch door runs the SAME delivery preparation as
        // the send door (defang, taint session attribution, ambient memory
        // injection). Dispatch has no Steer mode; `DispatchOrigin` rides the
        // input untouched.
        let (content_to_deliver, injected_context) = self
            .prepare_member_delivery(
                identity,
                &input.content,
                memory_session_key.as_deref(),
                memory_generation,
                false,
            )
            .await?;
        // Task #50 fail-closed matrix, validated BEFORE bridge admission:
        // (None, None) is an ordinary delivery; a full pair validates
        // upstream (canonical non-nil UUID correlation) and carries; EVERY
        // other combination - half-pair, invalid UUID - is a typed refusal
        // and NO delivery occurs. Never warn-and-degrade: a degraded
        // delivery under a broken identity is a silent dedup hole.
        let delivery_identity = match (&input.idempotency_key, &input.correlation_id) {
            (None, None) => None,
            (Some(idempotency_key), Some(correlation_id)) => {
                // App-supplied correlations canonicalize deterministically
                // instead of refusing on value shape (HomeCore admission
                // break: source-string correlations were tolerated through
                // the 0.8.15 pair and refused by 0.8.16 identity threading).
                // Half-pairs below stay typed refusals - structure is still
                // fail closed; only the VALUE domain is widened, and ONLY
                // for the app-reachable dispatch origins (Connector, plus
                // the RPC surface's System default). Code-owned lanes -
                // Scheduler, Policy, Flow - mint their own canonical UUIDs,
                // so a non-canonical value there is a defect to refuse
                // typed (the task #50 matrix), never to mask.
                let app_lane = matches!(
                    input.origin,
                    super::types::DispatchOrigin::Connector | super::types::DispatchOrigin::System
                );
                let canonical = if app_lane {
                    crate::member_comms_id::canonical_correlation_id(correlation_id.as_str())
                } else {
                    std::borrow::Cow::Borrowed(correlation_id.as_str())
                };
                if canonical.as_ref() != correlation_id.as_str() {
                    tracing::info!(
                        identity = %identity,
                        canonical_correlation = %canonical,
                        source_len = correlation_id.as_str().len(),
                        "app-supplied delivery correlation canonicalized to UUIDv5 \
                         for bridge admission"
                    );
                }
                Some(
                    meerkat_mob::MobDeliveryIdentity::new(
                        idempotency_key.as_str(),
                        canonical.as_ref(),
                    )
                    .map_err(|error| {
                        IdentityRuntimeError::InvalidDeliveryIdentity {
                            identity: identity.clone(),
                            detail: error.to_string(),
                        }
                    })?,
                )
            }
            (Some(_), None) => {
                return Err(IdentityRuntimeError::InvalidDeliveryIdentity {
                    identity: identity.clone(),
                    detail: "idempotency key without a correlation id (half-pair)".to_string(),
                });
            }
            (None, Some(_)) => {
                return Err(IdentityRuntimeError::InvalidDeliveryIdentity {
                    identity: identity.clone(),
                    detail: "correlation id without an idempotency key (half-pair)".to_string(),
                });
            }
        };

        // Deliver through the session bridge when available. When the dedup
        // carrier is present its correlation id also rides as the
        // interaction id.
        let mut dispatched_session_id = None;
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id) {
            let mut delivery =
                super::bridge::BridgeDelivery::new(content_to_deliver.clone(), HandlingMode::Queue);
            delivery.injected_context = injected_context.clone();
            delivery.interaction_id = delivery_identity
                .as_ref()
                .map(|identity| identity.correlation_id.clone());
            delivery.delivery_identity = delivery_identity.clone();
            let delivered_session_id = bridge
                .deliver_admitted(rid, delivery)
                .await
                .map_err(|e| IdentityRuntimeError::Internal(format!("bridge dispatch: {e}")))?;
            dispatched_session_id = Some(delivered_session_id.clone());
            if let Some(rebound_token) = self
                .reconcile_delivered_session_locked(identity, delivered_session_id)
                .await?
            {
                token = rebound_token;
            }
        }

        Ok(DispatchOutcome {
            admission: DispatchAdmission {
                fencing_token: token,
                durable: is_durable,
                completion_baseline,
            },
            session_id: dispatched_session_id,
        })
    }

    /// Cancellation-safe dispatch for RPC/host request boundaries.
    pub async fn dispatch_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        input: &DispatchInput,
    ) -> Result<(FencingToken, bool), IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let input = input.clone();
        self.run_tracked_foreground(async move { runtime.dispatch(&identity, &input).await })
            .await
    }

    /// Cancellation-safe dispatch pinned to the generated runtime alias that
    /// the caller resolved.
    pub async fn dispatch_member_alias_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: &str,
        input: &DispatchInput,
    ) -> Result<(FencingToken, bool), IdentityRuntimeError> {
        self.dispatch_admission_tracked(identity, Some(expected_alias), input)
            .await
            .map(|admission| (admission.fencing_token, admission.durable))
    }

    /// Cancellation-safe dispatch that also returns the completion baseline to
    /// wait past. Dispatch counterpart of [`Self::send_admission_tracked`];
    /// pass `expected_alias = None` for the durable identity.
    pub async fn dispatch_admission_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
        input: &DispatchInput,
    ) -> Result<DispatchAdmission, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.map(ToString::to_string);
        let input = input.clone();
        self.run_tracked_foreground(async move {
            runtime
                .dispatch_with_expected_member_alias(&identity, expected_alias.as_deref(), &input)
                .await
                .map(|outcome| outcome.admission)
        })
        .await
    }

    /// Scheduler delivery pinned to a generated alias, returning the exact
    /// bridge session that accepted the work while the lifecycle lock held.
    pub(crate) async fn dispatch_member_alias_with_session_tracked(
        self: &Arc<Self>,
        identity: &AgentIdentity,
        expected_alias: &str,
        input: &DispatchInput,
    ) -> Result<Option<SessionId>, IdentityRuntimeError> {
        let runtime = Arc::clone(self);
        let identity = identity.clone();
        let expected_alias = expected_alias.to_string();
        let input = input.clone();
        self.run_tracked_foreground(async move {
            runtime
                .dispatch_with_expected_member_alias(&identity, Some(&expected_alias), &input)
                .await
                .map(|outcome| outcome.session_id)
        })
        .await
    }

    // -----------------------------------------------------------------------
    // Status: status() — REQ-07
    // -----------------------------------------------------------------------

    /// Return the full identity status for the given identity.
    pub async fn status(
        &self,
        identity: &AgentIdentity,
    ) -> Result<IdentityStatus, IdentityRuntimeError> {
        let entries = self.entries.read().await;
        let entry = entries
            .get(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;

        let lease_info = entry.lease.as_ref().map(|l| LeaseInfo {
            fencing_token: l.fencing_token,
            ttl_remaining: l.ttl_remaining(),
            healthy: l.is_healthy(),
        });

        let continuity_health = Some(ContinuityHealth {
            store_reachable: true, // tracked per-store in production
            durability_policy: self.durability_policy.clone(),
            last_checkpoint_version: if entry.checkpoint_version.get() > 0 {
                Some(entry.checkpoint_version)
            } else {
                None
            },
        });

        Ok(IdentityStatus {
            identity: identity.clone(),
            state: entry.state,
            agent_runtime_id: entry
                .continuity
                .as_ref()
                .map(|c| c.agent_runtime_id.clone()),
            session_id: entry.continuity.as_ref().map(|c| c.session_id.clone()),
            profile: Some(entry.spec.profile.clone()),
            runtime_mode: entry.spec.runtime_mode_override,
            addressability: entry.spec.addressability,
            display_name: entry.spec.display_name.clone(),
            labels: entry.spec.labels.clone(),
            generation: entry.continuity.as_ref().map(|c| c.generation),
            checkpoint_version: if entry.checkpoint_version.get() > 0 {
                Some(entry.checkpoint_version)
            } else {
                None
            },
            lease: lease_info,
            continuity_health,
            continuity_unrecoverable: entry.continuity_unrecoverable.clone(),
        })
    }

    /// Return statuses for every registered identity without materializing
    /// dormant members.
    pub async fn statuses(&self) -> Vec<IdentityStatus> {
        let identities = self
            .entries
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut statuses = Vec::with_capacity(identities.len());
        for identity in identities {
            if let Ok(status) = self.status(&identity).await {
                statuses.push(status);
            }
        }
        statuses
    }

    // -----------------------------------------------------------------------
    // Lifecycle: retire() — REQ-08
    // -----------------------------------------------------------------------

    /// Retire an identity. Validates lease ownership and retires the mob member.
    pub async fn retire(
        &self,
        identity: &AgentIdentity,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.retire_with_expected_member_alias(identity, None).await
    }

    async fn retire_with_expected_member_alias(
        &self,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        if let Some(expected_alias) = expected_alias {
            self.ensure_expected_member_alias_current(identity, expected_alias)
                .await?;
        }
        self.retire_locked(identity).await
    }

    async fn retire_locked(
        &self,
        identity: &AgentIdentity,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.ensure_active_lease(identity).await?;
        let registered_entry = self
            .mark_lifecycle_in_progress(identity, IdentityLifecycleState::Retiring)
            .await?;
        let _previous_token = match Self::check_lease(&registered_entry) {
            Ok(token) => token,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(err);
            }
        };
        let runtime_id = registered_entry
            .continuity
            .as_ref()
            .map(|c| c.agent_runtime_id.clone());
        let session_id = registered_entry
            .continuity
            .as_ref()
            .map(|c| c.session_id.clone());

        let acquire_result = match self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::Lease(err));
            }
        };

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };
        if let Err(err) = self
            .advance_existing_continuity_fence(identity, &registered_entry, &grant)
            .await
        {
            self.restore_broken_entry_and_release_grant(identity, registered_entry, &grant)
                .await;
            return Err(err);
        }
        // The durable fence now belongs to the retire transaction. Refresh
        // the bridge adapter before retirement so Meerkat's terminal archive
        // projection carries that same token. Leaving the live session on the
        // prior token makes ArchiveSession fail closed with a stale fence and
        // strands the member in Retiring.
        if let Some(record) = registered_entry.continuity.as_ref()
            && let Err(error) = self
                .refresh_existing_session_runtime_state(identity, record, &grant)
                .await
        {
            self.restore_broken_entry_with_fenced_store(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Internal(format!(
                "bridge refresh session authority before retire: {error}"
            )));
        }

        // §8.4 trigger (b): distill the outgoing session's tail BEFORE the
        // member retires. Best-effort and bounded — retirement proceeds at
        // the distiller's pre-rotation timeout.
        if let Some(injector) = self.agent_memory.read().await.clone() {
            if let Some(session_id) = session_id.as_ref() {
                injector
                    .distill_before_rotation(
                        identity,
                        &session_id.to_string(),
                        crate::memory::distiller::DistillCause::Retire,
                    )
                    .await;
            }
            // §8.5 exit interview: queue the retired identity's store for
            // the next dream's harvest sub-phase.
            injector
                .note_identity_retired(
                    identity,
                    session_id
                        .as_ref()
                        .map(std::string::ToString::to_string)
                        .as_deref(),
                    "retire",
                )
                .await;
        }

        // Retire the mob member through the session bridge when available.
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id)
            && let Err(err) = bridge.retire_member(rid).await
        {
            self.restore_entry_with_grant(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Internal(format!(
                "bridge retire: {err}"
            )));
        }
        if let (Some(bridge), Some(session_id)) = (&self.bridge, &session_id)
            && let Err(err) = bridge.unregister_session_runtime_state(session_id).await
        {
            self.restore_broken_entry_and_release_grant(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Internal(format!(
                "bridge unregister retired session: {err}"
            )));
        }

        let release_result = self
            .lease_provider
            .release_leases(std::slice::from_ref(&grant))
            .await;
        let (state, bootstrap_error) = match &release_result {
            Ok(()) => (IdentityLifecycleState::Retiring, None),
            Err(error) => (
                IdentityLifecycleState::Broken,
                Some(format!("lease release after retire: {error}")),
            ),
        };
        {
            let mut entries = self.entries.write().await;
            let entry = entries
                .get_mut(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            entry.state = state;
            entry.lease = None;
            entry.pending_lease_release = release_result.as_ref().err().map(|_| grant.clone());
        }
        self.mark_bootstrap_from_lifecycle(identity, state, bootstrap_error);
        release_result.map_err(IdentityRuntimeError::Lease)?;
        Ok(grant.fencing_token)
    }

    // -----------------------------------------------------------------------
    // Lifecycle: respawn() — REQ-09
    // -----------------------------------------------------------------------

    /// Respawn: non-destructive recovery.
    ///
    /// 1. Fence the current owner
    /// 2. Attempt final checkpoint
    /// 3. Reactivate from authoritative continuity with same record + runtime ID
    /// 4. ContinuityGeneration does NOT advance
    pub async fn respawn(
        &self,
        identity: &AgentIdentity,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        self.respawn_with_expected_member_alias(identity, None)
            .await
    }

    async fn respawn_with_expected_member_alias(
        &self,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        if let Some(expected_alias) = expected_alias {
            self.ensure_expected_member_alias_current(identity, expected_alias)
                .await?;
        }
        self.respawn_locked(identity).await
    }

    /// Perform the identity-side half of respawn while the caller holds the
    /// per-identity lifecycle lock.
    async fn respawn_locked(
        &self,
        identity: &AgentIdentity,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let registered_entry = self
            .mark_lifecycle_in_progress(identity, IdentityLifecycleState::Suspended)
            .await?;

        // Fence the old owner by re-acquiring the lease
        let acquire_result = match self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::Lease(err));
            }
        };

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };

        if let Err(err) = self
            .advance_existing_continuity_fence(identity, &registered_entry, &grant)
            .await
        {
            self.restore_broken_entry_and_release_grant(identity, registered_entry, &grant)
                .await;
            return Err(err);
        }

        // Resolve current continuity state
        let resolved = match self
            .continuity_store
            .resolve_many(std::slice::from_ref(identity))
            .await
        {
            Ok(resolved) => resolved,
            Err(err) => {
                self.restore_entry_with_grant(identity, registered_entry.clone(), &grant)
                    .await;
                return Err(IdentityRuntimeError::Store(err));
            }
        };

        let record = match resolved.get(identity) {
            Some(super::types::ContinuityResolveState::Ready { record }) => record.clone(),
            Some(super::types::ContinuityResolveState::Broken { failure }) => {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "broken continuity for {identity}: {}",
                    failure.detail
                )));
            }
            Some(super::types::ContinuityResolveState::Uninitialized) => {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "cannot respawn uninitialized identity {identity}"
                )));
            }
            None => {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::Store(
                    ContinuityStoreError::NotFound {
                        identity: identity.clone(),
                    },
                ));
            }
        };

        // §8.4 trigger (b): respawn is a recovery boundary — harvest the
        // session's window before the runtime refreshes (the SessionId does
        // not rotate here; the cursor stays valid). Bounded; respawn
        // proceeds at the pre-rotation timeout.
        if let Some(injector) = self.agent_memory.read().await.clone() {
            let session_key = record.session_id.to_string();
            injector.note_session_generation(identity, &session_key, record.generation.get());
            injector
                .distill_before_rotation(
                    identity,
                    &session_key,
                    crate::memory::distiller::DistillCause::Respawn,
                )
                .await;
        }

        let effective_checkpoint_version = match self
            .refresh_existing_session_runtime_state(identity, &record, &grant)
            .await
        {
            Ok(version) => version,
            Err(err) => {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(err);
            }
        };
        let mut record = record;
        record.checkpoint_version = effective_checkpoint_version;

        // Update runtime state: same record, new lease, back to Active
        let mut entries = self.entries.write().await;
        let Some(entry) = entries.get_mut(identity) else {
            drop(entries);
            if let Err(err) = self
                .release_or_park_untracked_leases(std::slice::from_ref(&grant))
                .await
            {
                tracing::warn!(
                    %identity,
                    error = %err,
                    "failed to release lease after respawn entry disappeared; exact grant parked for retry"
                );
            }
            return Err(IdentityRuntimeError::UnknownIdentity(identity.clone()));
        };
        entry.continuity = Some(record.clone());
        entry.lease = Some(LeaseEntry {
            fencing_token: grant.fencing_token,
            ttl: grant.ttl,
            acquired_at: Instant::now(),
        });
        entry.state = IdentityLifecycleState::Active;
        entry.checkpoint_version = record.checkpoint_version;
        drop(entries);
        self.mark_bootstrap_from_lifecycle(identity, IdentityLifecycleState::Active, None);

        Ok(record)
    }

    /// Rebind continuity to the concrete session created by a lower-level
    /// member respawn. This keeps identity-first status aligned when a control
    /// surface refreshes the mob member outside the identity runtime bridge.
    pub async fn rebind_session_after_live_respawn(
        &self,
        identity: &AgentIdentity,
        session_id: SessionId,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        self.rebind_session_after_live_respawn_with_expected_member_alias(
            identity, None, session_id,
        )
        .await
    }

    async fn rebind_session_after_live_respawn_with_expected_member_alias(
        &self,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
        session_id: SessionId,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        if let Some(expected_alias) = expected_alias {
            self.ensure_expected_member_alias_current(identity, expected_alias)
                .await?;
        }
        self.rebind_session_after_live_respawn_locked(identity, session_id)
            .await
    }

    async fn reconcile_delivered_session_locked(
        &self,
        identity: &AgentIdentity,
        delivered_session_id: SessionId,
    ) -> Result<Option<FencingToken>, IdentityRuntimeError> {
        let current_session_id = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            entry
                .continuity
                .as_ref()
                .map(|record| record.session_id.clone())
        };

        let Some(current_session_id) = current_session_id else {
            return Ok(None);
        };
        if current_session_id == delivered_session_id {
            return Ok(None);
        }

        tracing::warn!(
            %identity,
            old_session_id = %current_session_id,
            new_session_id = %delivered_session_id,
            "identity bridge delivery returned a rotated session; rebinding continuity"
        );
        self.rebind_session_after_live_respawn_locked(identity, delivered_session_id)
            .await?;

        let entries = self.entries.read().await;
        let entry = entries
            .get(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        Ok(entry.lease.as_ref().map(|lease| lease.fencing_token))
    }

    async fn rebind_session_after_live_respawn_locked(
        &self,
        identity: &AgentIdentity,
        session_id: SessionId,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let registered_entry = self
            .mark_lifecycle_in_progress(identity, IdentityLifecycleState::Suspended)
            .await?;

        let acquire_result = match self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::Lease(err));
            }
        };

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };

        let mut record = match registered_entry.continuity.as_ref() {
            Some(record) => record.clone(),
            None => {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::UnknownIdentity(identity.clone()));
            }
        };
        if let Err(err) = self
            .advance_existing_continuity_fence(identity, &registered_entry, &grant)
            .await
        {
            if let Some(bridge) = self.bridge.as_ref()
                && let Err(unregister_err) =
                    bridge.unregister_session_runtime_state(&session_id).await
            {
                tracing::warn!(
                    %identity,
                    session_id = %session_id,
                    error = %unregister_err,
                    "failed to unregister rebound session after continuity fence failure"
                );
            }
            self.restore_broken_entry_and_release_grant(identity, registered_entry, &grant)
                .await;
            return Err(err);
        }
        let previous_session_id = record.session_id.clone();
        record.session_id = session_id;

        if let Err(err) = self
            .continuity_store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await
        {
            if let Some(bridge) = self.bridge.as_ref()
                && let Err(unregister_err) = bridge
                    .unregister_session_runtime_state(&record.session_id)
                    .await
            {
                tracing::warn!(
                    %identity,
                    session_id = %record.session_id,
                    error = %unregister_err,
                    "failed to unregister rebound session after continuity upsert failure"
                );
            }
            self.mark_rebind_failure_broken(identity, registered_entry, &grant, &record)
                .await;
            return Err(IdentityRuntimeError::Store(err));
        }

        // A lower session-store save can advance the durable checkpoint head
        // while a respawn is rebinding to a different session id. The local
        // store preserves that same-generation maximum; read the effective
        // row back before seeding bridge runtime state so neither the in-memory
        // entry nor the new session counter rewinds to the stale captured
        // version.
        let effective = match self
            .continuity_store
            .resolve_many(std::slice::from_ref(identity))
            .await
        {
            Ok(effective) => effective,
            Err(error) => {
                self.mark_rebind_failure_broken(identity, registered_entry, &grant, &record)
                    .await;
                return Err(IdentityRuntimeError::Store(error));
            }
        };
        match effective.get(identity) {
            Some(super::types::ContinuityResolveState::Ready {
                record: effective_record,
            }) if effective_record.session_id == record.session_id
                && effective_record.generation == record.generation =>
            {
                record = effective_record.clone();
            }
            other => {
                self.mark_rebind_failure_broken(identity, registered_entry, &grant, &record)
                    .await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "rebind continuity read-back did not return the committed session/generation: {other:?}"
                )));
            }
        }

        if let Some(bridge) = self.bridge.as_ref() {
            match bridge
                .register_session_runtime_state(
                    &record.session_id,
                    identity,
                    record.generation,
                    record.checkpoint_version,
                    grant.fencing_token,
                )
                .await
            {
                Ok(version) => {
                    record.checkpoint_version =
                        CheckpointVersion::new(record.checkpoint_version.get().max(version.get()));
                }
                Err(err) => {
                    if let Err(unregister_err) = bridge
                        .unregister_session_runtime_state(&record.session_id)
                        .await
                    {
                        tracing::warn!(
                            %identity,
                            session_id = %record.session_id,
                            error = %unregister_err,
                            "failed to unregister rebound session after bridge register failure"
                        );
                    }
                    self.mark_rebind_failure_broken(identity, registered_entry, &grant, &record)
                        .await;
                    return Err(IdentityRuntimeError::Internal(format!(
                        "bridge rebind respawned session runtime state: {err}"
                    )));
                }
            }
            if previous_session_id != record.session_id
                && let Err(err) = bridge
                    .unregister_session_runtime_state(&previous_session_id)
                    .await
            {
                tracing::warn!(
                    %identity,
                    session_id = %previous_session_id,
                    error = %err,
                    "failed to unregister previous session after live respawn rebind"
                );
            }
        }

        if let Err(err) = self
            .continuity_store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await
        {
            if let Some(bridge) = self.bridge.as_ref()
                && let Err(unregister_err) = bridge
                    .unregister_session_runtime_state(&record.session_id)
                    .await
            {
                tracing::warn!(
                    %identity,
                    session_id = %record.session_id,
                    error = %unregister_err,
                    "failed to unregister rebound session after final continuity upsert failure"
                );
            }
            self.mark_rebind_failure_broken(identity, registered_entry, &grant, &record)
                .await;
            return Err(IdentityRuntimeError::Store(err));
        }

        self.register(
            registered_entry.spec,
            IdentityLifecycleState::Active,
            Some(record.clone()),
            Some(grant),
        )
        .await;
        Ok(record)
    }

    // -----------------------------------------------------------------------
    // Lifecycle: reset() — REQ-10
    // -----------------------------------------------------------------------

    /// Reset: destructive continuity reset.
    ///
    /// 1. Fence old owner
    /// 2. Advance ContinuityGeneration
    /// 3. Create fresh continuity under the same AgentIdentity
    /// 4. Old-owner late writes rejected by stale fencing token
    pub async fn reset(
        &self,
        identity: &AgentIdentity,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        self.reset_with_expected_member_alias(identity, None).await
    }

    async fn reset_with_expected_member_alias(
        &self,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let mut foreground_shutdown = self.foreground_cancel.subscribe();
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        if let Some(expected_alias) = expected_alias {
            self.ensure_expected_member_alias_current(identity, expected_alias)
                .await?;
        }
        // Re-profile before snapshotting the lifecycle entry so the rebuilt
        // session uses the current roster spec, not the old checkpoint spec.
        self.adopt_current_roster_spec_for_reset(identity).await;
        let registered_entry = self
            .mark_lifecycle_in_progress(identity, IdentityLifecycleState::Suspended)
            .await?;

        // INV-05: fence the old owner first
        let acquire_result = match self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::Lease(err));
            }
        };

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };
        if let Err(err) = self
            .advance_existing_continuity_fence(identity, &registered_entry, &grant)
            .await
        {
            self.restore_broken_entry_and_release_grant(identity, registered_entry, &grant)
                .await;
            return Err(err);
        }

        // Resolve to get current generation
        let resolved = match self
            .continuity_store
            .resolve_many(std::slice::from_ref(identity))
            .await
        {
            Ok(resolved) => resolved,
            Err(err) => {
                self.restore_entry_with_grant(identity, registered_entry.clone(), &grant)
                    .await;
                return Err(IdentityRuntimeError::Store(err));
            }
        };

        let current_gen = match resolved.get(identity) {
            Some(super::types::ContinuityResolveState::Ready { record }) => record.generation,
            Some(super::types::ContinuityResolveState::Uninitialized) => {
                ContinuityGeneration::new(0)
            }
            _ => ContinuityGeneration::new(0),
        };

        // Advance generation
        let new_gen = ContinuityGeneration::new(current_gen.get() + 1);
        let new_session_id = meerkat_core::types::SessionId::new();
        let new_runtime_id = AgentRuntimeId::parse(&format!("rt:{identity}:{}", new_gen.get()))
            .map_err(|e| {
                IdentityRuntimeError::Internal(format!("failed to mint runtime id: {e}"))
            })?;

        let new_record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: new_runtime_id,
            session_id: new_session_id,
            generation: new_gen,
            checkpoint_version: CheckpointVersion::new(0),
        };
        let spec = registered_entry.spec.clone();
        let mut draft = super::types::AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: spec.additional_instructions.clone(),
            labels: spec.labels.clone(),
            app_context: spec.context.clone(),
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
            provider_params: None,
            compaction_curator: Default::default(),
        };
        if self.bridge.is_some() {
            let active_peers = self.entries.read().await.keys().cloned().collect();
            let managed_edges = self.desired_peer_edges.read().await.clone();
            let build_context = AgentBuildContext {
                identity: identity.clone(),
                active_peers,
                managed_edges,
                runtime_services: self.runtime_services(),
            };
            if let Some(customizer) = self.customizer.read().await.clone() {
                let customize = customizer.customize_build(&build_context, &spec, &mut draft);
                tokio::pin!(customize);
                let customize_result = if *foreground_shutdown.borrow() {
                    None
                } else {
                    tokio::select! {
                        result = &mut customize => Some(result),
                        changed = foreground_shutdown.changed() => {
                            if changed.is_ok() && *foreground_shutdown.borrow() {
                                None
                            } else {
                                Some(customize.await)
                            }
                        }
                    }
                };
                let Some(customize_result) = customize_result else {
                    self.restore_entry_with_grant(identity, registered_entry, &grant)
                        .await;
                    return Err(IdentityRuntimeError::Internal(
                        "identity reset cancelled during shutdown before session installation"
                            .to_string(),
                    ));
                };
                if let Err(err) = customize_result {
                    self.restore_entry_with_grant(identity, registered_entry, &grant)
                        .await;
                    return Err(IdentityRuntimeError::Internal(format!(
                        "customizer after reset: {err}"
                    )));
                }
            }
        }

        // Bridge: ONE authoritative successor transition. This used to retire the
        // old mob member and then create a fresh one, which the durable-roster
        // contract makes impossible - the successor occupies the same roster row,
        // so the create collided with its own predecessor. Respawn is the single
        // primitive that retires terminally and spawns the exact successor, and
        // splitting it back into retire + create would reopen the compensation
        // races it exists to close.
        if let Some(bridge) = &self.bridge {
            if let Err(err) = self
                .continuity_store
                .upsert_continuity_record(&new_record, grant.fencing_token)
                .await
            {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::Store(err));
            }
            let provisional_record = new_record.clone();

            let old_runtime_id = registered_entry
                .continuity
                .as_ref()
                .map(|c| c.agent_runtime_id.clone());
            let old_session_id = registered_entry
                .continuity
                .as_ref()
                .map(|c| c.session_id.clone());

            let successor = bridge
                .reset_member_to_successor(identity, &spec, &draft)
                .await
                .map_err(|e| {
                    IdentityRuntimeError::Internal(format!("bridge reset successor: {e}"))
                });
            let successor = match successor {
                Ok(successor) => successor,
                Err(err) => {
                    // No cleanup retire here. The provisional runtime id was
                    // never created as a roster row - respawn either committed
                    // its own successor or did not - so retiring it would report
                    // a spurious cleanup failure on top of the real error.
                    let cleanup_error: Option<BridgeError> = None;
                    let delete_error = self
                        .restore_entry_after_reset_bridge_failure(
                            identity,
                            &provisional_record,
                            registered_entry.clone(),
                            &grant,
                            cleanup_error.is_some(),
                        )
                        .await;
                    if cleanup_error.is_some() || delete_error.is_some() {
                        return Err(IdentityRuntimeError::Internal(format!(
                            "{err}{}{}",
                            cleanup_error
                                .as_ref()
                                .map(|e| format!("; cleanup retire failed: {e}"))
                                .unwrap_or_default(),
                            delete_error
                                .as_ref()
                                .map(|e| format!("; tentative continuity cleanup failed: {e}"))
                                .unwrap_or_default()
                        )));
                    }
                    return Err(err);
                }
            };
            // Commit continuity from what the machine actually committed. Both
            // atoms come from the successor binding: the generation is
            // MobMachine's to mint, so a pre-computed runtime id would be a
            // guess that happened to be right until it was not.
            let mut new_record = new_record;
            new_record.agent_runtime_id = successor.agent_runtime_id.clone();
            let session_id = successor.session_id;
            new_record.session_id = session_id;
            tracing::debug!(
                identity = %identity,
                runtime_id = %new_record.agent_runtime_id,
                session_id = %new_record.session_id,
                "reset bridge create_session completed",
            );

            if let Err(err) = self
                .continuity_store
                .upsert_continuity_record(&new_record, grant.fencing_token)
                .await
            {
                let unregister_error = Self::unregister_bridge_session_runtime_states(
                    bridge.as_ref(),
                    std::slice::from_ref(&new_record.session_id),
                )
                .await;
                let cleanup_error = bridge
                    .retire_member(&new_record.agent_runtime_id)
                    .await
                    .err();
                let delete_error = self
                    .restore_entry_after_reset_bridge_failure(
                        identity,
                        &provisional_record,
                        registered_entry.clone(),
                        &grant,
                        unregister_error.is_some() || cleanup_error.is_some(),
                    )
                    .await;
                if unregister_error.is_some() || cleanup_error.is_some() || delete_error.is_some() {
                    return Err(IdentityRuntimeError::Internal(format!(
                        "continuity upsert actual session after reset: {err}{}{}{}",
                        unregister_error
                            .as_ref()
                            .map(|e| format!("; unregister session failed: {e}"))
                            .unwrap_or_default(),
                        cleanup_error
                            .as_ref()
                            .map(|e| format!("; cleanup retire failed: {e}"))
                            .unwrap_or_default(),
                        delete_error
                            .as_ref()
                            .map(|e| format!("; tentative continuity cleanup failed: {e}"))
                            .unwrap_or_default(),
                    )));
                }
                return Err(IdentityRuntimeError::Store(err));
            }

            let register_result = bridge
                .register_session_runtime_state(
                    &new_record.session_id,
                    identity,
                    new_record.generation,
                    new_record.checkpoint_version,
                    grant.fencing_token,
                )
                .await;
            let effective_checkpoint_version = match register_result {
                Ok(version) => version,
                Err(err) => {
                    let unregister_error = Self::unregister_bridge_session_runtime_states(
                        bridge.as_ref(),
                        std::slice::from_ref(&new_record.session_id),
                    )
                    .await;
                    let cleanup_error = bridge
                        .retire_member(&new_record.agent_runtime_id)
                        .await
                        .err();
                    let mut detail =
                        format!("bridge register actual session runtime state after reset: {err}");
                    if let Some(unregister_error) = unregister_error.as_ref() {
                        detail
                            .push_str(&format!("; unregister session failed: {unregister_error}"));
                    }
                    if let Some(cleanup_error) = cleanup_error.as_ref() {
                        detail.push_str(&format!("; cleanup retire failed: {cleanup_error}"));
                    }
                    let delete_error = self
                        .restore_entry_after_reset_bridge_failure(
                            identity,
                            &new_record,
                            registered_entry.clone(),
                            &grant,
                            unregister_error.is_some() || cleanup_error.is_some(),
                        )
                        .await;
                    if let Some(delete_error) = delete_error {
                        return Err(IdentityRuntimeError::Internal(format!(
                            "{detail}; tentative continuity cleanup failed: {delete_error}"
                        )));
                    }
                    return Err(IdentityRuntimeError::Internal(detail));
                }
            };
            new_record.checkpoint_version = effective_checkpoint_version;
            tracing::debug!(
                identity = %identity,
                runtime_id = %new_record.agent_runtime_id,
                session_id = %new_record.session_id,
                checkpoint_version = new_record.checkpoint_version.get(),
                "reset bridge session runtime state registered",
            );

            if let Err(err) = self
                .continuity_store
                .upsert_continuity_record(&new_record, grant.fencing_token)
                .await
            {
                tracing::warn!(
                    identity = %identity,
                    runtime_id = %new_record.agent_runtime_id,
                    session_id = %new_record.session_id,
                    error = %err,
                    "reset final continuity upsert failed after bridge materialization; rolling back new generation",
                );
                let unregister_error = bridge
                    .unregister_session_runtime_state(&new_record.session_id)
                    .await
                    .err();
                let cleanup_error = bridge
                    .retire_member(&new_record.agent_runtime_id)
                    .await
                    .err();
                let rollback_error = self
                    .continuity_store
                    .rollback_continuity_record(
                        &new_record,
                        registered_entry.continuity.as_ref(),
                        grant.fencing_token,
                    )
                    .await
                    .err();
                if rollback_error.is_some() {
                    self.restore_broken_entry_from_authoritative_continuity(
                        identity,
                        registered_entry,
                        &grant,
                    )
                    .await;
                } else {
                    self.restore_broken_entry_and_release_grant(identity, registered_entry, &grant)
                        .await;
                }
                if unregister_error.is_some() || cleanup_error.is_some() || rollback_error.is_some()
                {
                    return Err(IdentityRuntimeError::Internal(format!(
                        "continuity upsert after reset: {err}{}{}{}",
                        unregister_error
                            .as_ref()
                            .map(|e| format!("; unregister session failed: {e}"))
                            .unwrap_or_default(),
                        cleanup_error
                            .as_ref()
                            .map(|e| format!("; cleanup retire failed: {e}"))
                            .unwrap_or_default(),
                        rollback_error
                            .as_ref()
                            .map(|e| format!("; continuity rollback failed: {e}"))
                            .unwrap_or_default()
                    )));
                }
                return Err(IdentityRuntimeError::Store(err));
            }

            // Update runtime state
            let mut entries = self.entries.write().await;
            let Some(entry) = entries.get_mut(identity) else {
                drop(entries);
                tracing::warn!(
                    identity = %identity,
                    runtime_id = %new_record.agent_runtime_id,
                    session_id = %new_record.session_id,
                    "reset entry disappeared after bridge materialization; rolling back new generation",
                );
                let _ = bridge
                    .unregister_session_runtime_state(&new_record.session_id)
                    .await;
                let _ = bridge.retire_member(&new_record.agent_runtime_id).await;
                if let Err(err) = self
                    .continuity_store
                    .rollback_continuity_record(
                        &new_record,
                        registered_entry.continuity.as_ref(),
                        grant.fencing_token,
                    )
                    .await
                {
                    tracing::warn!(
                        %identity,
                        error = %err,
                        "failed to roll back continuity after reset entry disappeared"
                    );
                }
                if let Err(err) = self
                    .release_or_park_untracked_leases(std::slice::from_ref(&grant))
                    .await
                {
                    tracing::warn!(
                        %identity,
                        error = %err,
                        "failed to release lease after reset entry disappeared; exact grant parked for retry"
                    );
                }
                return Err(IdentityRuntimeError::UnknownIdentity(identity.clone()));
            };
            entry.continuity = Some(new_record.clone());
            entry.lease = Some(Self::lease_entry_from_grant(&grant));
            entry.state = IdentityLifecycleState::Active;
            entry.checkpoint_version = new_record.checkpoint_version;
            tracing::debug!(
                identity = %identity,
                runtime_id = %new_record.agent_runtime_id,
                session_id = %new_record.session_id,
                "reset completed",
            );
            drop(entries);

            // The prior bridge projection remains rollback authority until
            // the final continuity row and in-memory entry both commit. In
            // particular, blocking continuity stores introduce scheduler
            // yield points here, so scheduling cleanup before the final
            // upsert can unregister the old session while rollback is still
            // possible.
            // NO old-member retire debt. The authoritative successor transition
            // (respawn) terminally retired the predecessor row inside the same
            // operation, so scheduling a retire here would manufacture debt for
            // work another authority already committed - and because respawn
            // always advances the generation, the old/new filter below would
            // never skip it. Reset would return success while the runtime
            // accumulated retryable cleanup it could never discharge.
            //
            // `retire_reset_superseded_member` is deliberately NOT made tolerant
            // of an absent member: that would hide wrong ownership here and mask
            // genuinely missing members everywhere else it is used.
            let cleanup_old_runtime_id: Option<AgentRuntimeId> = None;
            let _ = &old_runtime_id;
            // The old SESSION is still MobKit's to unregister: it is a
            // continuity concern rather than a roster row, so respawn does not
            // touch it. If that unregistration fails it stays exact and
            // retryable, unchanged.
            let cleanup_old_session_id = old_session_id
                .as_ref()
                .filter(|old_session_id| *old_session_id != &new_record.session_id)
                .cloned();
            let reset_memory_injector = self.agent_memory.read().await.clone();
            let reset_memory_capture = reset_memory_injector.as_ref().and_then(|injector| {
                registered_entry.continuity.as_ref().map(|old_continuity| {
                    PendingResetMemoryCapture {
                        injector: injector.clone(),
                        identity: identity.clone(),
                        session_key: old_continuity.session_id.to_string(),
                        generation: old_continuity.generation.get(),
                    }
                })
            });
            // Record exact cleanup debt immediately after the durable and
            // in-memory replacement commit. The owned debt carries its memory
            // capture prerequisite, so reset remains latency-neutral while
            // graceful shutdown still joins/retries the complete sequence.
            let reset_bridge_cleanup = self
                .record_old_bridge_cleanup_after_reset(
                    cleanup_old_runtime_id,
                    cleanup_old_session_id,
                    reset_memory_capture.clone(),
                )
                .await;
            // §10.1: reset is the deliberate clean-slate boundary — clear
            // session taint explicitly (rotation clears implicitly; this
            // also drops pending pre-attribution taint). Mark the outgoing
            // boundary synchronously; the runtime-owned cleanup task performs
            // bounded distillation before it may CAS-delete the superseded
            // session projection. This preserves detached reset latency
            // without racing the evidence source.
            if let Some(injector) = reset_memory_injector.as_ref() {
                injector.clear_taint_for_identity(identity);
                injector.note_session_generation(
                    identity,
                    &new_record.session_id.to_string(),
                    new_record.generation.get(),
                );
                if let Some(old_continuity) = registered_entry.continuity.as_ref() {
                    let old_session_key = old_continuity.session_id.to_string();
                    injector.note_reset_boundary(&old_session_key);
                    injector.note_session_generation(
                        identity,
                        &old_session_key,
                        old_continuity.generation.get(),
                    );
                }
            }

            if let Some(cleanup) = reset_bridge_cleanup {
                self.spawn_old_bridge_cleanup_after_reset(bridge.clone(), cleanup)
                    .await;
            }
            tracing::debug!(
                identity = %identity,
                runtime_id = %new_record.agent_runtime_id,
                session_id = %new_record.session_id,
                "reset old bridge cleanup debt recorded after continuity commit",
            );
            self.mark_bootstrap_from_lifecycle(identity, IdentityLifecycleState::Active, None);
            return Ok(new_record);
        }

        // Persist the new record (fencing token from new lease protects against old writes)
        if let Err(err) = self
            .continuity_store
            .upsert_continuity_record(&new_record, grant.fencing_token)
            .await
        {
            self.restore_entry_with_grant(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Store(err));
        }

        // No bridge — update runtime state only (validation mode)
        let mut entries = self.entries.write().await;
        let Some(entry) = entries.get_mut(identity) else {
            drop(entries);
            if let Err(err) = self
                .continuity_store
                .rollback_continuity_record(
                    &new_record,
                    registered_entry.continuity.as_ref(),
                    grant.fencing_token,
                )
                .await
            {
                tracing::warn!(
                    %identity,
                    error = %err,
                    "failed to roll back continuity after reset validation entry disappeared"
                );
            }
            if let Err(err) = self
                .release_or_park_untracked_leases(std::slice::from_ref(&grant))
                .await
            {
                tracing::warn!(
                    %identity,
                    error = %err,
                    "failed to release lease after reset validation entry disappeared; exact grant parked for retry"
                );
            }
            return Err(IdentityRuntimeError::UnknownIdentity(identity.clone()));
        };
        entry.continuity = Some(new_record.clone());
        entry.lease = Some(Self::lease_entry_from_grant(&grant));
        entry.state = IdentityLifecycleState::Active;
        entry.checkpoint_version = CheckpointVersion::new(0);
        drop(entries);
        if let Some(injector) = self.agent_memory.read().await.as_ref() {
            injector.clear_taint_for_identity(identity);
            // No-bridge (validation) reset: same §8.4 boundary semantics.
            if let Some(old_continuity) = registered_entry.continuity.as_ref() {
                let old_session_key = old_continuity.session_id.to_string();
                injector.note_reset_boundary(&old_session_key);
                injector.spawn_rotation_distillation(
                    identity,
                    &old_session_key,
                    crate::memory::distiller::DistillCause::Reset,
                );
            }
        }

        self.mark_bootstrap_from_lifecycle(identity, IdentityLifecycleState::Active, None);
        Ok(new_record)
    }

    // -----------------------------------------------------------------------
    // Lifecycle: delete_identity() — REQ-11
    // -----------------------------------------------------------------------

    /// Delete an identity: removes continuity record.
    ///
    /// 1. Fence old owner
    /// 2. Remove ContinuityRecord
    /// 3. Future bootstrap treats identity as Uninitialized
    pub async fn delete_identity(
        &self,
        identity: &AgentIdentity,
    ) -> Result<(), IdentityRuntimeError> {
        self.delete_identity_with_expected_member_alias(identity, None)
            .await
    }

    async fn delete_identity_with_expected_member_alias(
        &self,
        identity: &AgentIdentity,
        expected_alias: Option<&str>,
    ) -> Result<(), IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        if let Some(expected_alias) = expected_alias {
            self.ensure_expected_member_alias_current(identity, expected_alias)
                .await?;
        }
        self.delete_identity_locked(identity).await
    }

    async fn delete_identity_locked(
        &self,
        identity: &AgentIdentity,
    ) -> Result<(), IdentityRuntimeError> {
        let registered_entry = self
            .mark_lifecycle_in_progress(identity, IdentityLifecycleState::Retiring)
            .await?;
        let runtime_id = registered_entry
            .continuity
            .as_ref()
            .map(|c| c.agent_runtime_id.clone());
        let session_id = registered_entry
            .continuity
            .as_ref()
            .map(|c| c.session_id.clone());

        // INV-05: fence the old owner first
        let acquire_result = match self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::Lease(err));
            }
        };

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };
        if let Err(err) = self
            .advance_existing_continuity_fence(identity, &registered_entry, &grant)
            .await
        {
            self.restore_broken_entry_and_release_grant(identity, registered_entry, &grant)
                .await;
            return Err(err);
        }
        // The durable fence now belongs to the delete transaction. Refresh
        // the bridge adapter before retirement so Meerkat's terminal archive
        // projection carries that same token. Leaving the live session on the
        // prior token makes ArchiveSession fail closed, retains a Retiring Mob
        // anchor, and later prevents strict shutdown attestation.
        if let Some(record) = registered_entry.continuity.as_ref()
            && let Err(error) = self
                .refresh_existing_session_runtime_state(identity, record, &grant)
                .await
        {
            self.restore_broken_entry_with_fenced_store(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Internal(format!(
                "bridge refresh session authority before delete: {error}"
            )));
        }

        // §8.4 trigger (b): delete is the identity's LAST boundary — harvest
        // the outgoing session before teardown (its exit-interview analog,
        // §8.5, is the steward's; the distillate is what it will read).
        // Bounded; deletion proceeds at the pre-rotation timeout.
        if let Some(injector) = self.agent_memory.read().await.clone() {
            if let Some(session_id) = session_id.as_ref() {
                let session_key = session_id.to_string();
                injector
                    .distill_before_rotation(
                        identity,
                        &session_key,
                        crate::memory::distiller::DistillCause::Delete,
                    )
                    .await;
                // Ask 2 GC: delete permanently abandons this session id, and
                // its knowledge has just been distilled into the identity-keyed
                // MobKit store (which the exit-interview harvest below reads —
                // a DIFFERENT store from the meerkat session-memory scope). So
                // reclaiming the orphaned meerkat scope here frees dead
                // re-embed weight without starving any downstream read.
                injector
                    .drop_orphaned_session_scope(
                        &session_key,
                        crate::memory::distiller::DistillCause::Delete,
                    )
                    .await;
            }
            // §8.5 exit interview (delete is the identity's LAST boundary).
            injector
                .note_identity_retired(
                    identity,
                    session_id
                        .as_ref()
                        .map(std::string::ToString::to_string)
                        .as_deref(),
                    "delete",
                )
                .await;
        }

        // Retire the mob member through the session bridge before removing
        // the continuity record. This ensures the mob actor is cleaned up.
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id)
            && let Err(err) = bridge.retire_member(rid).await
        {
            self.restore_entry_with_grant(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Internal(format!(
                "bridge retire before delete: {err}"
            )));
        }

        if let (Some(bridge), Some(session_id)) = (&self.bridge, &session_id)
            && let Some(err) = Self::unregister_bridge_session_runtime_states(
                bridge.as_ref(),
                std::slice::from_ref(session_id),
            )
            .await
        {
            self.restore_broken_entry_with_fenced_store(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Internal(format!(
                "bridge unregister session before delete: {err}"
            )));
        }

        // Destroy-deprojection (2026-07-31 verdict): the durable session row
        // must not outlive the identity - a leftover external body is
        // exactly what the ephemeral-runtime-store activation mint would
        // faithfully resurrect on the next cold pod. Projection writes are
        // already quiesced (member retired, session unregistered) and this
        // delete transaction owns the ADVANCED identity fence, so the
        // revision-CAS delete cannot race a live writer. `delete_continuity_record` below removes any
        // remainder atomically for conforming stores; a store that cannot
        // support session-scoped deletion (`Ok(false)` default) or a row the
        // current decoder cannot token (an unimported released envelope)
        // keeps the record-scoped contract and is surfaced loudly instead of
        // silently retained.
        if let Some(session_id) = session_id.as_ref() {
            match self
                .continuity_store
                .load_session_snapshot(session_id)
                .await
            {
                Ok(Some(snapshot)) => {
                    let cas_token = serde_json::from_slice::<meerkat_core::Session>(&snapshot.data)
                        .ok()
                        .and_then(|current| {
                            meerkat_core::session_store::session_projection_cas_token(&current).ok()
                        });
                    match cas_token {
                        Some(token) => match self
                            .continuity_store
                            .delete_session_snapshot_if_current_revision(session_id, &token)
                            .await
                        {
                            Ok(true) => {}
                            Ok(false) => {
                                tracing::warn!(
                                    identity = %identity,
                                    session_id = %session_id,
                                    "session-scoped snapshot delete unsupported or superseded; \
                                     relying on delete_continuity_record's record-scoped \
                                     deletion - a non-conforming external store may retain a \
                                     resurrectable session body"
                                );
                            }
                            Err(err) => {
                                self.restore_broken_entry_and_release_grant(
                                    identity,
                                    registered_entry,
                                    &grant,
                                )
                                .await;
                                return Err(IdentityRuntimeError::Store(err));
                            }
                        },
                        None => {
                            tracing::warn!(
                                identity = %identity,
                                session_id = %session_id,
                                "durable session row cannot be revision-tokened for CAS \
                                 delete; relying on delete_continuity_record's record-scoped \
                                 deletion"
                            );
                        }
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    self.restore_broken_entry_and_release_grant(identity, registered_entry, &grant)
                        .await;
                    return Err(IdentityRuntimeError::Store(err));
                }
            }
        }

        // Remove authoritative continuity record from the store
        if let Err(err) = self
            .continuity_store
            .delete_continuity_record(identity, grant.fencing_token)
            .await
        {
            self.restore_broken_entry_and_release_grant(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Store(err));
        }

        // Authoritative deletion is committed before ownership is released,
        // so another holder can never race the continuity delete. Surface a
        // provider release failure explicitly; the still-held external grant
        // then remains the fail-closed ownership fence.
        let release_result = self
            .lease_provider
            .release_leases(std::slice::from_ref(&grant))
            .await;

        match release_result {
            Ok(()) => {
                self.event_channels.write().await.remove(identity);
                self.entries.write().await.remove(identity);
                // The identity remains part of the desired bootstrap roster
                // until a roster reconcile removes it. Successful deletion
                // therefore makes readiness Dormant.
                self.mark_bootstrap_from_lifecycle(identity, IdentityLifecycleState::Dormant, None);
                Ok(())
            }
            Err(error) => {
                // The physical member and continuity row are already gone,
                // but the provider still owns this exact fencing grant. Keep
                // an explicit Broken tombstone so reconcile and shutdown can
                // retry it; removing the entry here would make non-expiring
                // provider authority permanently unreachable.
                let mut entries = self.entries.write().await;
                let entry = entries
                    .get_mut(identity)
                    .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
                entry.state = IdentityLifecycleState::Broken;
                entry.continuity = None;
                entry.lease = None;
                entry.pending_lease_release = Some(grant);
                drop(entries);
                self.mark_bootstrap_from_lifecycle(
                    identity,
                    IdentityLifecycleState::Broken,
                    Some(format!("lease release after delete: {error}")),
                );
                Err(IdentityRuntimeError::Lease(error))
            }
        }
    }

    // -----------------------------------------------------------------------
    // Checkpoint — REQ-14, REQ-15, REQ-16, REQ-17
    // -----------------------------------------------------------------------

    /// Save a checkpoint snapshot. Enforces version ordering and fencing.
    pub async fn checkpoint(
        &self,
        identity: &AgentIdentity,
        snapshot: &SessionSnapshot,
    ) -> Result<CheckpointVersion, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            if entry.state != IdentityLifecycleState::Active {
                return Err(IdentityRuntimeError::InvalidState {
                    identity: identity.clone(),
                    state: entry.state,
                    operation: "checkpoint",
                });
            }
        }

        let token = self.ensure_active_lease(identity).await?;
        let (record, new_version) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            let record = entry
                .continuity
                .as_ref()
                .ok_or_else(|| {
                    IdentityRuntimeError::Internal(format!("no continuity record for {identity}"))
                })?
                .clone();

            let new_version = CheckpointVersion::new(entry.checkpoint_version.get() + 1);
            (record, new_version)
        };

        // REQ-15 + REQ-16: store enforces version ordering and fencing
        self.continuity_store
            .save_session_snapshot(
                identity,
                &record.session_id,
                record.generation,
                new_version,
                token,
                snapshot,
            )
            .await?;

        // Update local checkpoint version
        {
            let mut entries = self.entries.write().await;
            if let Some(entry) = entries.get_mut(identity) {
                entry.checkpoint_version = new_version;
            }
        }

        self.emit_event(
            identity,
            IdentityEvent::CheckpointCompleted {
                identity: identity.clone(),
                version: new_version,
            },
        )
        .await;

        Ok(new_version)
    }

    // -----------------------------------------------------------------------
    // Roster inspection — REQ-32
    // -----------------------------------------------------------------------

    /// Return all active identities with their specs and status.
    pub async fn roster_inspect(
        &self,
    ) -> BTreeMap<AgentIdentity, (DurableAgentSpec, IdentityStatus)> {
        let entries = self.entries.read().await;
        let mut result = BTreeMap::new();
        for (identity, entry) in entries.iter() {
            let lease_info = entry.lease.as_ref().map(|l| LeaseInfo {
                fencing_token: l.fencing_token,
                ttl_remaining: l.ttl_remaining(),
                healthy: l.is_healthy(),
            });
            let continuity_health = Some(ContinuityHealth {
                store_reachable: true,
                durability_policy: self.durability_policy.clone(),
                last_checkpoint_version: if entry.checkpoint_version.get() > 0 {
                    Some(entry.checkpoint_version)
                } else {
                    None
                },
            });
            let status = IdentityStatus {
                identity: identity.clone(),
                state: entry.state,
                agent_runtime_id: entry
                    .continuity
                    .as_ref()
                    .map(|c| c.agent_runtime_id.clone()),
                session_id: entry.continuity.as_ref().map(|c| c.session_id.clone()),
                profile: Some(entry.spec.profile.clone()),
                runtime_mode: entry.spec.runtime_mode_override,
                addressability: entry.spec.addressability,
                display_name: entry.spec.display_name.clone(),
                labels: entry.spec.labels.clone(),
                generation: entry.continuity.as_ref().map(|c| c.generation),
                checkpoint_version: if entry.checkpoint_version.get() > 0 {
                    Some(entry.checkpoint_version)
                } else {
                    None
                },
                lease: lease_info,
                continuity_health,
                continuity_unrecoverable: entry.continuity_unrecoverable.clone(),
            };
            result.insert(identity.clone(), (entry.spec.clone(), status));
        }
        result
    }

    // -----------------------------------------------------------------------
    // Roster uniqueness validation — INV-06
    // -----------------------------------------------------------------------

    /// Validate that a roster contains no duplicate identities.
    pub fn validate_roster_uniqueness(
        specs: &[DurableAgentSpec],
    ) -> Result<(), IdentityRuntimeError> {
        let mut seen = std::collections::BTreeSet::new();
        for spec in specs {
            if !seen.insert(&spec.identity) {
                return Err(IdentityRuntimeError::DuplicateIdentity(
                    spec.identity.clone(),
                ));
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Accessors for internal state
    // -----------------------------------------------------------------------

    /// Get the current entries (read-only snapshot).
    #[allow(dead_code)]
    pub(crate) async fn entries(&self) -> BTreeMap<AgentIdentity, IdentityEntry> {
        self.entries.read().await.clone()
    }

    /// Check if an identity is registered.
    pub async fn contains(&self, identity: &AgentIdentity) -> bool {
        self.entries.read().await.contains_key(identity)
    }

    /// Check if an identity is registered AND in Active state.
    pub async fn is_active(&self, identity: &AgentIdentity) -> bool {
        self.entries
            .read()
            .await
            .get(identity)
            .is_some_and(|e| e.state == IdentityLifecycleState::Active)
    }

    fn identity_from_member_alias(alias: &str) -> Option<(AgentIdentity, bool)> {
        let alias = crate::member_comms_id::runtime_alias_str(alias);
        let alias = alias.as_ref();
        alias
            .strip_prefix("rt:")
            .and_then(|rest| rest.rsplit_once(':'))
            .filter(|(identity, generation)| {
                !identity.is_empty()
                    && !generation.is_empty()
                    && generation.chars().all(|ch| ch.is_ascii_digit())
            })
            .and_then(|(identity, _)| AgentIdentity::parse(identity).ok())
            .map(|identity| (identity, true))
            .or_else(|| {
                AgentIdentity::parse(alias)
                    .ok()
                    .map(|identity| (identity, false))
            })
    }

    /// Parse the durable owner encoded in the reserved generated-alias
    /// namespace, whether or not that identity is still registered. Mutating
    /// member surfaces use this to fail closed after a concurrent delete
    /// instead of treating an orphaned `rt:*` alias as an ordinary mob member.
    pub(crate) fn identity_for_generated_member_alias(alias: &str) -> Option<AgentIdentity> {
        Self::identity_from_member_alias(alias).and_then(|(identity, generated)| {
            if generated { Some(identity) } else { None }
        })
    }

    /// Resolve a mutating member request without allowing a generated alias
    /// to fall back to the raw mob plane after concurrent deletion. Plain
    /// durable identities retain their historical registered-only behavior.
    pub(crate) async fn identity_for_member_mutation(&self, alias: &str) -> Option<AgentIdentity> {
        match Self::identity_from_member_alias(alias) {
            Some((identity, true)) => Some(identity),
            Some((identity, false)) => self.contains(&identity).await.then_some(identity),
            None => None,
        }
    }

    async fn ensure_expected_member_alias_current(
        &self,
        identity: &AgentIdentity,
        expected_alias: &str,
    ) -> Result<(), IdentityRuntimeError> {
        let canonical_alias = crate::member_comms_id::runtime_alias_str(expected_alias);
        let Some((alias_identity, generated_runtime_alias)) =
            Self::identity_from_member_alias(canonical_alias.as_ref())
        else {
            return Err(IdentityRuntimeError::UnknownIdentity(identity.clone()));
        };
        if alias_identity != *identity {
            return Err(IdentityRuntimeError::UnknownIdentity(alias_identity));
        }
        let entries = self.entries.read().await;
        let entry = entries
            .get(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        if generated_runtime_alias {
            let current = entry
                .continuity
                .as_ref()
                .map(|record| record.agent_runtime_id.clone());
            if current.as_ref().map(AgentRuntimeId::as_str) != Some(canonical_alias.as_ref()) {
                return Err(IdentityRuntimeError::StaleRuntimeAlias {
                    identity: identity.clone(),
                    requested: canonical_alias.into_owned(),
                    current,
                });
            }
        }
        Ok(())
    }

    /// Resolve a member alias to the durable identity that OWNS it, if any.
    ///
    /// Accepts both a generated runtime alias (`rt:<identity>:<generation>`)
    /// and a plain identity string; returns the identity only when it is
    /// actually registered in this runtime. Member-scoped RPCs use this to
    /// route lifecycle mutations of identity-owned members through the
    /// identity authority — a classic `handle.retire()`/`respawn()` on such
    /// a member would mutate it behind the IdentityRuntime's back (stale
    /// continuity binding, generation drift), which is the doctrine's
    /// "mob plane must not mangle durables" rule.
    pub async fn owned_identity_for_member_alias(&self, alias: &str) -> Option<AgentIdentity> {
        let (identity, _) = Self::identity_from_member_alias(alias)?;
        self.contains(&identity).await.then_some(identity)
    }

    /// Identities currently in the Broken lifecycle state.
    pub async fn broken_identities(&self) -> Vec<AgentIdentity> {
        self.entries
            .read()
            .await
            .iter()
            .filter(|(_, entry)| entry.state == IdentityLifecycleState::Broken)
            .map(|(identity, _)| identity.clone())
            .collect()
    }

    /// Broken identities that do NOT carry a terminal heal verdict — the set
    /// the continuity repair supervisor is allowed to keep retrying.
    pub async fn repairable_broken_identities(&self) -> Vec<AgentIdentity> {
        self.entries
            .read()
            .await
            .iter()
            .filter(|(_, entry)| {
                entry.state == IdentityLifecycleState::Broken
                    && entry.continuity_unrecoverable.is_none()
                    && entry
                        .host_rejected_build_park
                        .as_ref()
                        .is_none_or(|park| park.spec_digest != durable_spec_digest(&entry.spec))
            })
            .map(|(identity, _)| identity.clone())
            .collect()
    }

    /// The terminal heal verdict recorded for an identity, if any.
    pub async fn continuity_unrecoverable(
        &self,
        identity: &AgentIdentity,
    ) -> Option<ContinuityUnrecoverable> {
        self.entries
            .read()
            .await
            .get(identity)
            .and_then(|entry| entry.continuity_unrecoverable.clone())
    }

    /// Record a terminal heal verdict against a Broken identity.
    ///
    /// Returns `false` (without writing) when the identity is unknown or no
    /// longer Broken — the verdict only ever parks an already-Broken entry;
    /// it never degrades a live one. While recorded, the repair supervisor
    /// skips the identity and reconcile keeps its Broken projection instead
    /// of cosmetically resetting it (2026-07-29 heal/re-Break incident).
    pub async fn mark_continuity_unrecoverable(
        &self,
        identity: &AgentIdentity,
        reason: String,
    ) -> bool {
        let marked = {
            let mut entries = self.entries.write().await;
            match entries.get_mut(identity) {
                Some(entry) if entry.state == IdentityLifecycleState::Broken => {
                    entry.continuity_unrecoverable = Some(ContinuityUnrecoverable {
                        reason: reason.clone(),
                    });
                    true
                }
                _ => false,
            }
        };
        if marked {
            // Keep the bootstrap status surface honest about WHY the
            // identity stays broken (operators read this, not the log).
            self.mark_bootstrap_from_lifecycle(
                identity,
                IdentityLifecycleState::Broken,
                Some(reason),
            );
        }
        marked
    }

    /// Clear a previously recorded terminal heal verdict (operator retry).
    pub async fn clear_continuity_unrecoverable(&self, identity: &AgentIdentity) -> bool {
        let mut entries = self.entries.write().await;
        match entries.get_mut(identity) {
            Some(entry) => entry.continuity_unrecoverable.take().is_some(),
            None => false,
        }
    }

    /// The ACTIVE host-rejected-build park for an identity: `Some` only
    /// while the identity's current spec still digests to the parked value.
    /// A mismatch (the roster/policy changed) clears the park in place and
    /// returns `None` — the retry is permitted.
    pub async fn host_rejected_build_park(
        &self,
        identity: &AgentIdentity,
    ) -> Option<HostRejectedBuildPark> {
        let mut entries = self.entries.write().await;
        let entry = entries.get_mut(identity)?;
        let park = entry.host_rejected_build_park.clone()?;
        if park.spec_digest == durable_spec_digest(&entry.spec) {
            Some(park)
        } else {
            entry.host_rejected_build_park = None;
            None
        }
    }

    /// Park an identity whose build the host deterministically rejected,
    /// scoped to the identity's CURRENT spec. Operator-visible through the
    /// bootstrap status surface and the warn line; no automatic retry until
    /// the spec changes.
    pub(crate) async fn mark_host_rejected_build_park(
        &self,
        identity: &AgentIdentity,
        reason: String,
    ) -> bool {
        let marked_state = {
            let mut entries = self.entries.write().await;
            match entries.get_mut(identity) {
                Some(entry) => {
                    entry.host_rejected_build_park = Some(HostRejectedBuildPark {
                        reason: reason.clone(),
                        spec_digest: durable_spec_digest(&entry.spec),
                    });
                    Some(entry.state)
                }
                None => None,
            }
        };
        let Some(state) = marked_state else {
            return false;
        };
        tracing::warn!(
            identity = %identity,
            reason = %reason,
            "host deterministically rejected this identity's build; parking (no automatic \
             retry) until the roster spec changes"
        );
        // Keep the bootstrap status surface honest about WHY the identity is
        // stuck (operators read this, not the log). The detail renders on
        // Broken projections; for a Dormant park the typed materialize error
        // carries the reason to every send instead.
        self.mark_bootstrap_from_lifecycle(identity, state, Some(reason));
        true
    }

    /// Clear a host-rejected-build park (operator retry with an unchanged
    /// spec — e.g. after fixing the app-side gate's policy out of band).
    pub async fn clear_host_rejected_build_park(&self, identity: &AgentIdentity) -> bool {
        let mut entries = self.entries.write().await;
        match entries.get_mut(identity) {
            Some(entry) => entry.host_rejected_build_park.take().is_some(),
            None => false,
        }
    }

    /// The durable session currently bound to an identity, if any.
    pub(crate) async fn continuity_session_id(
        &self,
        identity: &AgentIdentity,
    ) -> Option<SessionId> {
        self.entries
            .read()
            .await
            .get(identity)
            .and_then(|entry| entry.continuity.as_ref().map(|c| c.session_id.clone()))
    }

    /// Get the continuity store reference.
    pub fn continuity_store(&self) -> &Arc<dyn ContinuityStore> {
        &self.continuity_store
    }

    /// Get the lease provider reference.
    pub fn lease_provider(&self) -> &Arc<dyn LeaseProvider> {
        &self.lease_provider
    }

    /// Get the runtime instance ID.
    pub fn runtime_instance_id(&self) -> &str {
        &self.runtime_instance_id
    }

    /// Get the durability policy.
    pub fn durability_policy(&self) -> &DurabilityPolicy {
        &self.durability_policy
    }

    /// Get whether a runtime store is configured.
    pub fn has_runtime_store(&self) -> bool {
        self.has_runtime_store
    }

    /// Get the session bridge reference, if configured.
    pub fn bridge(&self) -> Option<&Arc<dyn SessionBridge>> {
        self.bridge.as_ref()
    }

    // -----------------------------------------------------------------------
    // Convenience methods
    // -----------------------------------------------------------------------

    /// Send plain text to an addressable identity.
    pub async fn send_text(
        &self,
        identity: &AgentIdentity,
        text: impl Into<String>,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send(identity, &meerkat_core::ContentInput::Text(text.into()))
            .await
    }

    /// Dispatch plain text with system origin.
    pub async fn dispatch_text(
        &self,
        identity: &AgentIdentity,
        text: impl Into<String>,
    ) -> Result<(FencingToken, bool), IdentityRuntimeError> {
        self.dispatch(identity, &DispatchInput::system(text)).await
    }

    /// Execute the restore flow for the given roster.
    pub async fn restore_flow(
        &self,
        roster: &[DurableAgentSpec],
        topology_provider: Option<&dyn super::contracts::TopologyProvider>,
        customizer: Option<&dyn super::contracts::AgentCustomizer>,
    ) -> Result<super::orchestrator::RestoreFlowResult, IdentityRuntimeError> {
        super::orchestrator::restore_flow(self, roster, topology_provider, customizer).await
    }

    /// Resolve the AgentRuntimeId for a registered identity.
    pub async fn runtime_id_for(
        &self,
        identity: &AgentIdentity,
    ) -> Result<AgentRuntimeId, IdentityRuntimeError> {
        let entries = self.entries.read().await;
        let entry = entries
            .get(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        entry
            .continuity
            .as_ref()
            .map(|c| c.agent_runtime_id.clone())
            .ok_or_else(|| {
                IdentityRuntimeError::Internal(format!("no continuity record for {identity}"))
            })
    }

    /// Inspect the current execution state of an identity via the bridge.
    pub async fn inspect(
        &self,
        identity: &AgentIdentity,
    ) -> Result<super::bridge::MemberInspection, IdentityRuntimeError> {
        let runtime_id = self.runtime_id_for(identity).await?;
        let bridge = self
            .bridge
            .as_ref()
            .ok_or_else(|| IdentityRuntimeError::Internal("no bridge configured".to_string()))?;
        bridge
            .inspect_member(&runtime_id)
            .await
            .map_err(|e| IdentityRuntimeError::Internal(format!("inspect: {e}")))
    }

    // -----------------------------------------------------------------------
    // Turn-completion cursor
    // -----------------------------------------------------------------------

    /// Completion epoch of a REGISTERED identity: its live lease token, or
    /// token 0 when it holds no grant (dormant, or lease lost). `None` when
    /// the identity is not registered at all.
    async fn registered_completion_epoch(&self, identity: &AgentIdentity) -> Option<FencingToken> {
        let entries = self.entries.read().await;
        entries.get(identity).map(|entry| {
            entry
                .lease
                .as_ref()
                .map_or_else(|| FencingToken::new(0), |lease| lease.fencing_token)
        })
    }

    /// Move the stored cursor onto `epoch` if the lease incarnation advanced,
    /// and return it. Never rewinds: a stale epoch leaves the cursor alone.
    fn rebase_completion_cursor(
        &self,
        identity: &AgentIdentity,
        epoch: FencingToken,
    ) -> CompletionCursor {
        let mut cursors = self
            .completion_cursors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cursor = cursors
            .entry(identity.clone())
            .or_insert_with(|| CompletionCursor::start(epoch));
        *cursor = cursor.rebased(epoch);
        *cursor
    }

    /// Whatever was last published for `identity`, without minting a ledger
    /// entry. Reads for unregistered names go through here so probing
    /// arbitrary strings cannot grow the map.
    fn retained_completion_cursor(&self, identity: &AgentIdentity) -> CompletionCursor {
        self.completion_cursors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(identity)
            .copied()
            .unwrap_or_default()
    }

    /// Current [`CompletionCursor`] for `identity`.
    ///
    /// A registered identity's cursor is rebased onto its live lease
    /// incarnation first, so a poller observes an incarnation change
    /// immediately rather than comparing against a turn count that no longer
    /// means anything. An identity that is gone (retired, deleted, or never
    /// registered) reports its last published cursor — retained precisely so
    /// this read cannot rewind.
    pub async fn completion_cursor(&self, identity: &AgentIdentity) -> CompletionCursor {
        match self.registered_completion_epoch(identity).await {
            Some(epoch) => self.rebase_completion_cursor(identity, epoch),
            None => self.retained_completion_cursor(identity),
        }
    }

    /// Record that a turn completed for `identity`, advancing its cursor by
    /// one within the current lease incarnation.
    ///
    /// Production drives this from `AgentEvent::RunCompleted` on the always-on
    /// identity agent-event monitor. It is deliberately event-driven rather
    /// than derived from a polled projection: a poll cannot distinguish "new
    /// turn, identical text" from "no new turn", which is the entire defect
    /// this cursor exists to close.
    ///
    /// Not idempotent by design — one observed completion advances the cursor
    /// once, which is exactly what a poller comparing against a pre-delivery
    /// baseline needs.
    pub async fn record_turn_completed(&self, identity: &AgentIdentity) -> CompletionCursor {
        // A completion racing deregistration still counts under the last
        // known epoch rather than being dropped.
        let epoch = self
            .registered_completion_epoch(identity)
            .await
            .unwrap_or_else(|| self.retained_completion_cursor(identity).epoch);
        let mut cursors = self
            .completion_cursors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cursor = cursors
            .entry(identity.clone())
            .or_insert_with(|| CompletionCursor::start(epoch));
        *cursor = cursor.rebased(epoch).advanced();
        *cursor
    }

    /// Wait until a turn completes past `baseline`, or the timeout expires.
    ///
    /// This is the correct completion barrier: it compares cursors, never
    /// output text, so two consecutive turns emitting byte-identical text are
    /// still two distinct completions. `baseline` is the
    /// `completion_baseline` returned by [`Self::dispatch_admission_tracked`]
    /// or [`Self::send_admission_tracked`].
    ///
    /// A genuinely stalled turn still times out rather than hanging forever,
    /// and an incarnation change is reported as its own error rather than
    /// being read as either completion or continued waiting.
    pub async fn wait_for_completion(
        &self,
        identity: &AgentIdentity,
        baseline: CompletionCursor,
        timeout: Duration,
    ) -> Result<CompletionCursor, IdentityRuntimeError> {
        let deadline = Instant::now() + timeout;
        loop {
            let cursor = self.completion_cursor(identity).await;
            match cursor.progress_since(baseline) {
                CompletionProgress::Completed => return Ok(cursor),
                CompletionProgress::IncarnationChanged => {
                    return Err(IdentityRuntimeError::CompletionIncarnationChanged {
                        identity: identity.clone(),
                        baseline,
                        observed: cursor,
                    });
                }
                CompletionProgress::Pending => {}
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(IdentityRuntimeError::Internal(format!(
                    "timed out after {}s waiting for a turn past {baseline} on {identity}",
                    timeout.as_secs_f64()
                )));
            }
            tokio::time::sleep(
                COMPLETION_POLL_INTERVAL.min(deadline.saturating_duration_since(now)),
            )
            .await;
        }
    }

    /// The configured default timeout for wait operations.
    pub fn default_timeout(&self) -> Duration {
        self.default_timeout
    }

    async fn execute_reset_bridge_cleanup(
        bridge: &Arc<dyn SessionBridge>,
        cleanup: &PendingResetBridgeCleanup,
    ) -> Result<(), BridgeError> {
        match (&cleanup.runtime_id, &cleanup.session_id) {
            (Some(runtime_id), Some(session_id)) => {
                bridge
                    .retire_reset_superseded_member(runtime_id, session_id)
                    .await
            }
            (Some(runtime_id), None) => bridge.retire_member(runtime_id).await,
            (None, Some(session_id)) => bridge.unregister_session_runtime_state(session_id).await,
            (None, None) => Ok(()),
        }
    }

    async fn clear_reset_bridge_cleanup_if_current(
        pending: &RwLock<BTreeMap<String, PendingResetBridgeCleanup>>,
        key: &str,
        cleanup: &PendingResetBridgeCleanup,
    ) {
        let mut pending = pending.write().await;
        if pending.get(key) == Some(cleanup) {
            pending.remove(key);
        }
    }

    async fn run_reset_memory_capture_if_pending(
        pending: &RwLock<BTreeMap<String, PendingResetBridgeCleanup>>,
        key: &str,
        cleanup: &mut PendingResetBridgeCleanup,
    ) {
        let Some(capture) = cleanup.memory_capture.take() else {
            return;
        };
        capture.run().await;

        // Publish the state-machine advance only after the bounded capture
        // returns. Cancellation before this write leaves the capture on the
        // authoritative debt and a shutdown retry runs it again.
        let mut pending = pending.write().await;
        if let Some(current) = pending.get_mut(key)
            && current == cleanup
        {
            current.memory_capture = None;
        }
    }

    async fn record_old_bridge_cleanup_after_reset(
        &self,
        old_runtime_id: Option<AgentRuntimeId>,
        old_session_id: Option<SessionId>,
        memory_capture: Option<PendingResetMemoryCapture>,
    ) -> Option<PendingResetBridgeCleanup> {
        if old_runtime_id.is_none() && old_session_id.is_none() {
            return None;
        }
        let cleanup = PendingResetBridgeCleanup {
            runtime_id: old_runtime_id,
            session_id: old_session_id,
            memory_capture,
        };
        let key = cleanup.key();
        self.pending_reset_bridge_cleanups
            .write()
            .await
            .insert(key, cleanup.clone());
        Some(cleanup)
    }

    async fn spawn_old_bridge_cleanup_after_reset(
        &self,
        bridge: Arc<dyn SessionBridge>,
        mut cleanup: PendingResetBridgeCleanup,
    ) {
        let key = cleanup.key();
        let runtime_instance_id = self.runtime_instance_id.clone();
        let pending = self.pending_reset_bridge_cleanups.clone();
        let mut tasks = self.reset_bridge_cleanup_tasks.lock().await;
        while let Some(result) = tasks.try_join_next() {
            if let Err(error) = result {
                tracing::error!(
                    runtime_instance_id = %self.runtime_instance_id,
                    %error,
                    "reset bridge cleanup task panicked"
                );
            }
        }
        tasks.spawn(async move {
            Self::run_reset_memory_capture_if_pending(&pending, &key, &mut cleanup).await;
            match Self::execute_reset_bridge_cleanup(&bridge, &cleanup).await {
                Ok(()) => {
                    Self::clear_reset_bridge_cleanup_if_current(&pending, &key, &cleanup).await;
                }
                Err(error) => tracing::warn!(
                    runtime_instance_id = %runtime_instance_id,
                    cleanup_key = %key,
                    %error,
                    "reset-superseded bridge cleanup failed; retaining exact shutdown debt",
                ),
            }
        });
    }

    /// Join every post-commit reset cleanup task after foreground lifecycle
    /// admission is closed. The exact debt remains in the ledger on failure
    /// and is retried synchronously before physical mob shutdown.
    pub(crate) async fn join_reset_bridge_cleanup_tasks(&self) {
        let mut tasks = self.reset_bridge_cleanup_tasks.lock().await;
        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result {
                tracing::error!(
                    runtime_instance_id = %self.runtime_instance_id,
                    %error,
                    "reset bridge cleanup task panicked"
                );
            }
        }
    }

    /// Retry all exact reset cleanup debt. Success is the only path that
    /// removes an obligation; callers must retain identity grants when this
    /// returns an error.
    pub(crate) async fn drain_pending_reset_bridge_cleanups(
        &self,
    ) -> Result<usize, IdentityRuntimeError> {
        let Some(bridge) = self.bridge.as_ref() else {
            return Ok(0);
        };
        let pending = self
            .pending_reset_bridge_cleanups
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut completed = 0_usize;
        let mut errors = Vec::new();
        for mut cleanup in pending {
            let key = cleanup.key();
            Self::run_reset_memory_capture_if_pending(
                &self.pending_reset_bridge_cleanups,
                &key,
                &mut cleanup,
            )
            .await;
            match Self::execute_reset_bridge_cleanup(bridge, &cleanup).await {
                Ok(()) => {
                    Self::clear_reset_bridge_cleanup_if_current(
                        &self.pending_reset_bridge_cleanups,
                        &key,
                        &cleanup,
                    )
                    .await;
                    completed += 1;
                }
                Err(error) => errors.push(format!("{key}: {error}")),
            }
        }
        if errors.is_empty() {
            Ok(completed)
        } else {
            Err(IdentityRuntimeError::Internal(format!(
                "reset bridge cleanup debt remains: {}",
                errors.join("; ")
            )))
        }
    }

    /// Poll until the identity produces an output_preview, or timeout.
    ///
    /// **Unsound as a completion barrier.** `output_preview` is the last
    /// committed assistant text, so this returns immediately when a PREVIOUS
    /// turn already left a preview, and it cannot tell "new turn, identical
    /// text" from "no new turn" at all. Use
    /// [`Self::wait_for_completion`] with the `completion_baseline` from
    /// [`Self::send_admission_tracked`] / [`Self::dispatch_admission_tracked`]
    /// when you need to wait for a specific turn. Retained for callers that
    /// only need "has this identity ever spoken".
    pub async fn wait_for_output(
        &self,
        identity: &AgentIdentity,
        timeout: Duration,
    ) -> Result<String, IdentityRuntimeError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(inspection) = self.inspect(identity).await
                && let Some(preview) = inspection.output_preview
            {
                return Ok(preview);
            }
            if Instant::now() >= deadline {
                return Err(IdentityRuntimeError::Internal(format!(
                    "timed out waiting for output from {identity}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Poll until output_preview contains the given substring, or timeout.
    pub async fn wait_for_output_containing(
        &self,
        identity: &AgentIdentity,
        needle: &str,
        timeout: Duration,
    ) -> Result<String, IdentityRuntimeError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(inspection) = self.inspect(identity).await
                && let Some(ref preview) = inspection.output_preview
                && preview.contains(needle)
            {
                return Ok(preview.clone());
            }
            if Instant::now() >= deadline {
                return Err(IdentityRuntimeError::Internal(format!(
                    "timed out waiting for output containing '{needle}' from {identity}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

/// Wire two identities across mobs, resolving runtime IDs from both IdentityRuntimes.
///
/// This is a convenience function for same-process cross-mob scenarios where both
/// IdentityRuntimes are available. It resolves the AgentRuntimeId for each identity
/// and delegates to `UnifiedRuntime::wire_cross_mob()`.
pub async fn wire_cross_mob_by_identity(
    local_irt: &IdentityRuntime,
    local_identity: &AgentIdentity,
    remote_irt: &IdentityRuntime,
    remote_identity: &AgentIdentity,
    local_unified: &crate::UnifiedRuntime,
    remote_mob_id: &str,
) -> Result<(), IdentityRuntimeError> {
    let local_rt = local_irt.runtime_id_for(local_identity).await?;
    let remote_rt = remote_irt.runtime_id_for(remote_identity).await?;
    Box::pin(local_unified.wire_cross_mob(local_rt.as_str(), remote_rt.as_str(), remote_mob_id))
        .await
        .map_err(|e| IdentityRuntimeError::Internal(format!("wire_cross_mob: {e}")))
}

#[cfg(test)]
mod reset_reprofile_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::{Mutex as AsyncMutex, RwLock as AsyncRwLock};

    use super::super::bridge::{
        BridgeDelivery, BridgeError, MemberInspection, ResumeSessionOutcome,
    };
    use super::super::contracts::RosterProvider;
    use super::super::local_lease::LocalLeaseProvider;
    use super::super::local_store::LocalContinuityStore;
    use super::super::types::{
        AgentBuildDraft, ContinuityResolveState, CustomizerError, LeaseAcquireResult, LeaseError,
        LeaseRenewResult, RosterError, SessionSnapshot,
    };

    struct MutableRoster {
        specs: AsyncRwLock<Vec<DurableAgentSpec>>,
    }

    struct LostRenewLeaseProvider {
        inner: LocalLeaseProvider,
        lose_renewals: AtomicBool,
    }

    impl Default for LostRenewLeaseProvider {
        fn default() -> Self {
            Self {
                inner: LocalLeaseProvider::new(),
                lose_renewals: AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl LeaseProvider for LostRenewLeaseProvider {
        async fn acquire_leases(
            &self,
            identities: &[AgentIdentity],
            runtime_instance: &str,
        ) -> Result<BTreeMap<AgentIdentity, LeaseAcquireResult>, LeaseError> {
            self.inner
                .acquire_leases(identities, runtime_instance)
                .await
        }

        async fn renew_leases(
            &self,
            grants: &[LeaseGrant],
        ) -> Result<BTreeMap<AgentIdentity, LeaseRenewResult>, LeaseError> {
            if self.lose_renewals.load(Ordering::SeqCst) {
                return Ok(grants
                    .iter()
                    .map(|grant| {
                        (
                            grant.identity.clone(),
                            LeaseRenewResult::Lost {
                                identity: grant.identity.clone(),
                            },
                        )
                    })
                    .collect());
            }
            self.inner.renew_leases(grants).await
        }

        async fn release_leases(&self, grants: &[LeaseGrant]) -> Result<(), LeaseError> {
            self.inner.release_leases(grants).await
        }
    }

    struct RecordingReleaseLeaseProvider {
        inner: LocalLeaseProvider,
        fail_next_release: AtomicBool,
        release_attempts: AsyncMutex<Vec<LeaseGrant>>,
    }

    impl Default for RecordingReleaseLeaseProvider {
        fn default() -> Self {
            Self {
                inner: LocalLeaseProvider::new(),
                fail_next_release: AtomicBool::new(false),
                release_attempts: AsyncMutex::new(Vec::new()),
            }
        }
    }

    impl RecordingReleaseLeaseProvider {
        fn fail_next_release(&self) {
            self.fail_next_release.store(true, Ordering::SeqCst);
        }

        async fn release_attempts(&self) -> Vec<LeaseGrant> {
            self.release_attempts.lock().await.clone()
        }
    }

    #[async_trait::async_trait]
    impl LeaseProvider for RecordingReleaseLeaseProvider {
        async fn acquire_leases(
            &self,
            identities: &[AgentIdentity],
            runtime_instance: &str,
        ) -> Result<BTreeMap<AgentIdentity, LeaseAcquireResult>, LeaseError> {
            self.inner
                .acquire_leases(identities, runtime_instance)
                .await
        }

        async fn renew_leases(
            &self,
            grants: &[LeaseGrant],
        ) -> Result<BTreeMap<AgentIdentity, LeaseRenewResult>, LeaseError> {
            self.inner.renew_leases(grants).await
        }

        async fn release_leases(&self, grants: &[LeaseGrant]) -> Result<(), LeaseError> {
            self.release_attempts.lock().await.extend_from_slice(grants);
            if self.fail_next_release.swap(false, Ordering::SeqCst) {
                return Err(LeaseError::ProviderUnavailable(
                    "synthetic retained Broken lease release failure".to_string(),
                ));
            }
            self.inner.release_leases(grants).await
        }
    }

    struct GatedResetContinuityStore {
        inner: Arc<LocalContinuityStore>,
        upsert_calls: AtomicUsize,
        fail_on_call: AtomicUsize,
        failure_started: Notify,
        release_failure: Notify,
    }

    #[derive(Default)]
    struct GatedResetCustomizer {
        entered: Notify,
    }

    impl GatedResetCustomizer {
        async fn wait_for_entry(&self) {
            self.entered.notified().await;
        }
    }

    #[async_trait::async_trait]
    impl AgentCustomizer for GatedResetCustomizer {
        async fn customize_build(
            &self,
            _context: &AgentBuildContext,
            _spec: &DurableAgentSpec,
            _draft: &mut AgentBuildDraft,
        ) -> Result<(), CustomizerError> {
            self.entered.notify_one();
            futures::future::pending::<()>().await;
            Ok(())
        }
    }

    impl GatedResetContinuityStore {
        fn new() -> Result<Self, ContinuityStoreError> {
            Ok(Self {
                inner: Arc::new(LocalContinuityStore::in_memory()?),
                upsert_calls: AtomicUsize::new(0),
                fail_on_call: AtomicUsize::new(usize::MAX),
                failure_started: Notify::new(),
                release_failure: Notify::new(),
            })
        }

        fn fail_after_successful_upserts(&self, successful_upserts: usize) {
            let current = self.upsert_calls.load(Ordering::SeqCst);
            self.fail_on_call
                .store(current + successful_upserts + 1, Ordering::SeqCst);
        }

        async fn wait_for_failure(&self) {
            self.failure_started.notified().await;
        }

        fn release_failure(&self) {
            self.release_failure.notify_one();
        }
    }

    #[async_trait::async_trait]
    impl ContinuityStore for GatedResetContinuityStore {
        async fn resolve_many(
            &self,
            identities: &[AgentIdentity],
        ) -> Result<BTreeMap<AgentIdentity, ContinuityResolveState>, ContinuityStoreError> {
            self.inner.resolve_many(identities).await
        }

        async fn load_session_snapshot(
            &self,
            session_id: &SessionId,
        ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
            self.inner.load_session_snapshot(session_id).await
        }

        async fn save_session_snapshot(
            &self,
            identity: &AgentIdentity,
            session_id: &SessionId,
            generation: ContinuityGeneration,
            version: CheckpointVersion,
            fencing_token: FencingToken,
            snapshot: &SessionSnapshot,
        ) -> Result<(), ContinuityStoreError> {
            self.inner
                .save_session_snapshot(
                    identity,
                    session_id,
                    generation,
                    version,
                    fencing_token,
                    snapshot,
                )
                .await
        }

        async fn upsert_continuity_record(
            &self,
            record: &ContinuityRecord,
            fencing_token: FencingToken,
        ) -> Result<(), ContinuityStoreError> {
            let call = self.upsert_calls.fetch_add(1, Ordering::SeqCst) + 1;
            if self
                .fail_on_call
                .compare_exchange(call, usize::MAX, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                self.failure_started.notify_one();
                self.release_failure.notified().await;
                return Err(ContinuityStoreError::Io(
                    "gated final reset upsert failure".to_string(),
                ));
            }
            self.inner
                .upsert_continuity_record(record, fencing_token)
                .await
        }

        async fn rollback_continuity_record(
            &self,
            expected_attempt: &ContinuityRecord,
            previous: Option<&ContinuityRecord>,
            fencing_token: FencingToken,
        ) -> Result<(), ContinuityStoreError> {
            self.inner
                .rollback_continuity_record(expected_attempt, previous, fencing_token)
                .await
        }

        async fn delete_continuity_record(
            &self,
            identity: &AgentIdentity,
            fencing_token: FencingToken,
        ) -> Result<(), ContinuityStoreError> {
            self.inner
                .delete_continuity_record(identity, fencing_token)
                .await
        }
    }

    impl MutableRoster {
        fn new(specs: Vec<DurableAgentSpec>) -> Self {
            Self {
                specs: AsyncRwLock::new(specs),
            }
        }

        async fn set(&self, specs: Vec<DurableAgentSpec>) {
            *self.specs.write().await = specs;
        }
    }

    #[async_trait::async_trait]
    impl RosterProvider for MutableRoster {
        async fn roster(
            &self,
            _context: &RosterContext,
        ) -> Result<Vec<DurableAgentSpec>, RosterError> {
            Ok(self.specs.read().await.clone())
        }
    }

    #[derive(Default)]
    struct RecordingBridge {
        create_profiles: AsyncMutex<Vec<String>>,
        create_delay: Duration,
        creates_in_flight: AtomicUsize,
        max_creates_in_flight: AtomicUsize,
        retired_runtime_ids: AsyncMutex<Vec<String>>,
        hanging_retire_runtime_ids: AsyncMutex<BTreeSet<String>>,
        failing_register_session_ids: AsyncMutex<BTreeSet<String>>,
        failing_unregister_session_ids: AsyncMutex<BTreeSet<String>>,
        registered_fencing_tokens: AsyncMutex<Vec<FencingToken>>,
        authority_transitions: AsyncMutex<Vec<String>>,
        /// Successor generations minted by `reset_member_to_successor`, so a
        /// test can observe that the transition happened and under which
        /// incarnation.
        successor_generations: AsyncMutex<Vec<String>>,
    }

    impl RecordingBridge {
        async fn create_profiles(&self) -> Vec<String> {
            self.create_profiles.lock().await.clone()
        }

        async fn successor_generations(&self) -> Vec<String> {
            self.successor_generations.lock().await.clone()
        }

        fn max_creates_in_flight(&self) -> usize {
            self.max_creates_in_flight.load(Ordering::SeqCst)
        }

        async fn retired_runtime_ids(&self) -> Vec<String> {
            self.retired_runtime_ids.lock().await.clone()
        }

        async fn hang_retire_for(&self, runtime_id: &AgentRuntimeId) {
            self.hanging_retire_runtime_ids
                .lock()
                .await
                .insert(runtime_id.to_string());
        }

        async fn fail_unregister_for(&self, session_id: &SessionId) {
            self.failing_unregister_session_ids
                .lock()
                .await
                .insert(session_id.to_string());
        }

        async fn allow_unregister_for(&self, session_id: &SessionId) {
            self.failing_unregister_session_ids
                .lock()
                .await
                .remove(&session_id.to_string());
        }

        async fn fail_register_for(&self, session_id: &SessionId) {
            self.failing_register_session_ids
                .lock()
                .await
                .insert(session_id.to_string());
        }

        async fn registered_fencing_tokens(&self) -> Vec<FencingToken> {
            self.registered_fencing_tokens.lock().await.clone()
        }

        async fn authority_transitions(&self) -> Vec<String> {
            self.authority_transitions.lock().await.clone()
        }
    }

    #[async_trait::async_trait]
    impl SessionBridge for RecordingBridge {
        /// Mirrors respawn, INCLUDING its limitation.
        ///
        /// A successor keeps the predecessor's profile, so this deliberately
        /// does NOT record `spec.profile` into `create_profiles`. Recording it
        /// would make this double able to reprofile when Meerkat cannot, and the
        /// reprofile capability tests would go green against a capability that
        /// does not exist.
        async fn reset_member_to_successor(
            &self,
            identity: &AgentIdentity,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
        ) -> Result<crate::identity_first::bridge::ResetSuccessorBinding, BridgeError> {
            let generation = self.successor_generations.lock().await.len() as u64 + 1;
            let alias = format!("rt:{}:{generation}", identity.as_str());
            self.successor_generations.lock().await.push(alias.clone());
            let agent_runtime_id = AgentRuntimeId::parse(&alias).map_err(|error| {
                BridgeError::Mob(format!("test double minted an unusable successor: {error}"))
            })?;
            Ok(crate::identity_first::bridge::ResetSuccessorBinding {
                agent_runtime_id,
                session_id: SessionId::new(),
            })
        }

        async fn create_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            session_id: &SessionId,
        ) -> Result<SessionId, BridgeError> {
            let in_flight = self.creates_in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_creates_in_flight
                .fetch_max(in_flight, Ordering::SeqCst);
            tokio::time::sleep(self.create_delay).await;
            self.creates_in_flight.fetch_sub(1, Ordering::SeqCst);
            self.create_profiles
                .lock()
                .await
                .push(spec.profile.to_string());
            Ok(session_id.clone())
        }

        async fn resume_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            _session_id: &SessionId,
            _snapshot: &SessionSnapshot,
        ) -> Result<ResumeSessionOutcome, BridgeError> {
            Err(BridgeError::Mob(
                "resume not used in reset test".to_string(),
            ))
        }

        async fn deliver_admitted(
            &self,
            _runtime_id: &AgentRuntimeId,
            _delivery: BridgeDelivery,
        ) -> Result<SessionId, BridgeError> {
            Err(BridgeError::Mob(
                "deliver not used in reset test".to_string(),
            ))
        }

        async fn checkpoint_session(
            &self,
            _runtime_id: &AgentRuntimeId,
            _session_id: &SessionId,
        ) -> Result<SessionSnapshot, BridgeError> {
            Err(BridgeError::Mob(
                "checkpoint not used in reset test".to_string(),
            ))
        }

        async fn retire_member(&self, runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
            self.retired_runtime_ids
                .lock()
                .await
                .push(runtime_id.to_string());
            if self
                .hanging_retire_runtime_ids
                .lock()
                .await
                .contains(runtime_id.as_str())
            {
                futures::future::pending::<()>().await;
            }
            Ok(())
        }

        async fn inspect_member(
            &self,
            _runtime_id: &AgentRuntimeId,
        ) -> Result<MemberInspection, BridgeError> {
            Err(BridgeError::Mob(
                "inspect not used in reset test".to_string(),
            ))
        }

        async fn register_session_runtime_state(
            &self,
            session_id: &SessionId,
            _identity: &AgentIdentity,
            _generation: ContinuityGeneration,
            checkpoint_version: CheckpointVersion,
            fencing_token: FencingToken,
        ) -> Result<CheckpointVersion, BridgeError> {
            self.registered_fencing_tokens
                .lock()
                .await
                .push(fencing_token);
            self.authority_transitions
                .lock()
                .await
                .push(format!("register:{}", fencing_token.get()));
            if self
                .failing_register_session_ids
                .lock()
                .await
                .contains(&session_id.to_string())
            {
                return Err(BridgeError::Mob(
                    "synthetic live-session rebind failure".to_string(),
                ));
            }
            Ok(checkpoint_version)
        }

        async fn suspend_session_runtime_state(
            &self,
            session_id: &SessionId,
        ) -> Result<(), BridgeError> {
            self.authority_transitions
                .lock()
                .await
                .push(format!("suspend:{session_id}"));
            Ok(())
        }

        async fn unregister_session_runtime_state(
            &self,
            session_id: &SessionId,
        ) -> Result<(), BridgeError> {
            if self
                .failing_unregister_session_ids
                .lock()
                .await
                .contains(&session_id.to_string())
            {
                return Err(BridgeError::Mob("old session still draining".to_string()));
            }
            Ok(())
        }
    }

    /// Bridge used to prove REQ-33 metadata hot reloads never cross the
    /// retire/resume boundary. Initial creation and ordinary dispatch work;
    /// any attempted resume fails loudly, reproducing the real gateway's
    /// still-draining-member collision without an LLM or network dependency.
    #[derive(Default)]
    struct HotReloadBridge {
        sessions: AsyncMutex<BTreeMap<String, SessionId>>,
        retire_calls: AtomicUsize,
        resume_calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl SessionBridge for HotReloadBridge {
        async fn create_session(
            &self,
            _identity: &AgentIdentity,
            runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            session_id: &SessionId,
        ) -> Result<SessionId, BridgeError> {
            self.sessions
                .lock()
                .await
                .insert(runtime_id.to_string(), session_id.clone());
            Ok(session_id.clone())
        }

        async fn resume_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            _session_id: &SessionId,
            _snapshot: &SessionSnapshot,
        ) -> Result<ResumeSessionOutcome, BridgeError> {
            self.resume_calls.fetch_add(1, Ordering::SeqCst);
            Err(BridgeError::Mob(
                "same-profile metadata hot reload attempted session resume".to_string(),
            ))
        }

        async fn deliver_admitted(
            &self,
            runtime_id: &AgentRuntimeId,
            _delivery: BridgeDelivery,
        ) -> Result<SessionId, BridgeError> {
            self.sessions
                .lock()
                .await
                .get(runtime_id.as_str())
                .cloned()
                .ok_or_else(|| BridgeError::Mob(format!("missing session for {runtime_id}")))
        }

        async fn checkpoint_session(
            &self,
            _runtime_id: &AgentRuntimeId,
            _session_id: &SessionId,
        ) -> Result<SessionSnapshot, BridgeError> {
            Err(BridgeError::Mob(
                "checkpoint not used in hot-reload test".to_string(),
            ))
        }

        async fn retire_member(&self, runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
            self.retire_calls.fetch_add(1, Ordering::SeqCst);
            self.sessions.lock().await.remove(runtime_id.as_str());
            Ok(())
        }

        async fn inspect_member(
            &self,
            _runtime_id: &AgentRuntimeId,
        ) -> Result<MemberInspection, BridgeError> {
            Err(BridgeError::Mob(
                "inspect not used in hot-reload test".to_string(),
            ))
        }
    }

    /// Lower-plane model for lease-loss reconciliation. Resume rejects a
    /// duplicate concrete alias, so the tests prove Broken cleanup happens
    /// before profile replacement rather than merely observing call counts.
    #[derive(Default)]
    struct LostCleanupBridge {
        members: AsyncMutex<BTreeSet<String>>,
        session_runtime_states: AsyncMutex<BTreeSet<String>>,
        retire_calls: AtomicUsize,
        unregister_calls: AtomicUsize,
        unregistered_sessions: AsyncMutex<Vec<String>>,
        resume_collisions: AtomicUsize,
    }

    impl LostCleanupBridge {
        async fn member_count(&self) -> usize {
            self.members.lock().await.len()
        }

        async fn session_runtime_state_count(&self) -> usize {
            self.session_runtime_states.lock().await.len()
        }
    }

    async fn force_renewal_lost(
        runtime: &IdentityRuntime,
        lease_provider: &LostRenewLeaseProvider,
        identity: &AgentIdentity,
    ) -> Result<(), Box<dyn std::error::Error>> {
        lease_provider.lose_renewals.store(true, Ordering::SeqCst);
        {
            let mut entries = runtime.entries.write().await;
            let entry = entries
                .get_mut(identity)
                .ok_or("identity disappeared before forced renewal")?;
            let lease = entry
                .lease
                .as_mut()
                .ok_or("active identity has no lease before forced renewal")?;
            lease.ttl = Duration::ZERO;
        }
        assert!(matches!(
            runtime.renew_due_leases_once().await,
            Err(IdentityRuntimeError::LeaseLost(lost)) if lost == *identity
        ));
        Ok(())
    }

    #[async_trait::async_trait]
    impl SessionBridge for LostCleanupBridge {
        async fn create_session(
            &self,
            _identity: &AgentIdentity,
            runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            session_id: &SessionId,
        ) -> Result<SessionId, BridgeError> {
            if !self.members.lock().await.insert(runtime_id.to_string()) {
                return Err(BridgeError::Mob(format!(
                    "member collision for {runtime_id}"
                )));
            }
            Ok(session_id.clone())
        }

        async fn resume_session(
            &self,
            _identity: &AgentIdentity,
            runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            session_id: &SessionId,
            _snapshot: &SessionSnapshot,
        ) -> Result<ResumeSessionOutcome, BridgeError> {
            if !self.members.lock().await.insert(runtime_id.to_string()) {
                self.resume_collisions.fetch_add(1, Ordering::SeqCst);
                return Err(BridgeError::Mob(format!(
                    "member collision for {runtime_id}"
                )));
            }
            Ok(ResumeSessionOutcome::Resumed {
                session_id: session_id.clone(),
            })
        }

        async fn deliver_admitted(
            &self,
            _runtime_id: &AgentRuntimeId,
            _delivery: BridgeDelivery,
        ) -> Result<SessionId, BridgeError> {
            Err(BridgeError::Mob(
                "deliver not used in lost cleanup test".to_string(),
            ))
        }

        async fn checkpoint_session(
            &self,
            _runtime_id: &AgentRuntimeId,
            _session_id: &SessionId,
        ) -> Result<SessionSnapshot, BridgeError> {
            Err(BridgeError::Mob(
                "checkpoint not used in lost cleanup test".to_string(),
            ))
        }

        async fn retire_member(&self, runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
            self.retire_calls.fetch_add(1, Ordering::SeqCst);
            self.members.lock().await.remove(runtime_id.as_str());
            Ok(())
        }

        async fn inspect_member(
            &self,
            _runtime_id: &AgentRuntimeId,
        ) -> Result<MemberInspection, BridgeError> {
            Err(BridgeError::Mob(
                "inspect not used in lost cleanup test".to_string(),
            ))
        }

        async fn register_session_runtime_state(
            &self,
            session_id: &SessionId,
            _identity: &AgentIdentity,
            _generation: ContinuityGeneration,
            checkpoint_version: CheckpointVersion,
            _fencing_token: FencingToken,
        ) -> Result<CheckpointVersion, BridgeError> {
            self.session_runtime_states
                .lock()
                .await
                .insert(session_id.to_string());
            Ok(checkpoint_version)
        }

        async fn unregister_session_runtime_state(
            &self,
            session_id: &SessionId,
        ) -> Result<(), BridgeError> {
            self.unregister_calls.fetch_add(1, Ordering::SeqCst);
            self.unregistered_sessions
                .lock()
                .await
                .push(session_id.to_string());
            self.session_runtime_states
                .lock()
                .await
                .remove(&session_id.to_string());
            Ok(())
        }
    }

    fn durable_spec(identity: AgentIdentity, profile: &str) -> DurableAgentSpec {
        DurableAgentSpec {
            identity,
            profile: meerkat_mob::ProfileName::from(profile),
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

    /// Always answers create with the host's deterministic rejection — the
    /// exact string shape rpc_gateway mints when the app-side
    /// `callback/build_agent` round trip COMPLETES with an error.
    #[derive(Default)]
    struct HostRejectingBridge {
        create_attempts: std::sync::atomic::AtomicUsize,
    }

    impl HostRejectingBridge {
        fn attempts(&self) -> usize {
            self.create_attempts
                .load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl SessionBridge for HostRejectingBridge {
        async fn create_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            _session_id: &SessionId,
        ) -> Result<SessionId, BridgeError> {
            self.create_attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(BridgeError::Mob(
                "spawn_member: callback/build_agent failed: callback error: candidate-mode \
                 effect gate refused this build"
                    .to_string(),
            ))
        }

        async fn resume_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            _session_id: &SessionId,
            _snapshot: &SessionSnapshot,
        ) -> Result<ResumeSessionOutcome, BridgeError> {
            Err(BridgeError::Mob(
                "resume not used in host-reject test".to_string(),
            ))
        }

        async fn deliver_admitted(
            &self,
            _runtime_id: &AgentRuntimeId,
            _delivery: BridgeDelivery,
        ) -> Result<SessionId, BridgeError> {
            Err(BridgeError::Mob(
                "deliver not used in host-reject test".to_string(),
            ))
        }

        async fn checkpoint_session(
            &self,
            _runtime_id: &AgentRuntimeId,
            _session_id: &SessionId,
        ) -> Result<SessionSnapshot, BridgeError> {
            Err(BridgeError::Mob(
                "checkpoint not used in host-reject test".to_string(),
            ))
        }

        async fn retire_member(&self, _runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
            Ok(())
        }
    }

    /// Herd-investigation park: a build the HOST deterministically rejects
    /// (the candidate-mode effect gate) parks the identity typed on the
    /// FIRST attempt — no repair-loop churn — and a roster spec change
    /// re-admits exactly one new attempt.
    #[tokio::test]
    async fn host_rejected_build_parks_identity_until_spec_changes()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:gated")?;
        let bridge = Arc::new(HostRejectingBridge::default());
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "host-reject-park-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));
        runtime
            .register(
                durable_spec(identity.clone(), "domain"),
                IdentityLifecycleState::Dormant,
                None,
                None,
            )
            .await;

        // The first attempt reaches the host exactly once; the deterministic
        // rejection parks the identity typed.
        assert!(
            runtime.materialize(&identity).await.is_err(),
            "gated build must fail"
        );
        assert_eq!(bridge.attempts(), 1);
        let park = runtime
            .host_rejected_build_park(&identity)
            .await
            .ok_or("the first host rejection must park the identity")?;
        assert!(park.reason.contains("callback error"));

        // Parked: the next attempt fails fast typed WITHOUT reaching the
        // host, and the repair supervisor's Broken selection excludes the
        // identity even when a reconcile re-registers it Broken with the
        // same spec.
        match runtime.materialize(&identity).await {
            Err(IdentityRuntimeError::HostRejectedBuild { reason, .. }) => {
                assert!(reason.contains("callback error"));
            }
            other => return Err(format!("expected the typed park, got {other:?}").into()),
        }
        assert_eq!(
            bridge.attempts(),
            1,
            "a parked identity must not re-ask the host"
        );
        runtime
            .register(
                durable_spec(identity.clone(), "domain"),
                IdentityLifecycleState::Broken,
                None,
                None,
            )
            .await;
        assert!(
            runtime.repairable_broken_identities().await.is_empty(),
            "the repair supervisor must skip a parked identity"
        );

        // A spec change clears the park: the retry is permitted and reaches
        // the host exactly once more.
        let mut changed = durable_spec(identity.clone(), "domain");
        changed
            .labels
            .insert("policy_epoch".to_string(), "2".to_string());
        runtime
            .register(changed, IdentityLifecycleState::Dormant, None, None)
            .await;
        assert!(
            runtime.host_rejected_build_park(&identity).await.is_none(),
            "a spec change must clear the park"
        );
        assert!(runtime.materialize(&identity).await.is_err());
        assert_eq!(
            bridge.attempts(),
            2,
            "a changed spec re-admits exactly one new build attempt"
        );
        Ok(())
    }

    #[test]
    fn external_delivery_omits_unrepresentable_interaction_id_carrier()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("target-smoke")?;
        let mut external = durable_spec(identity.clone(), "target");
        external.backend = Some(meerkat_mob::MobBackendKind::External);
        assert_eq!(
            interaction_id_for_delivery(&external, Some("interaction-1")),
            None
        );

        let local = durable_spec(identity, "target");
        assert_eq!(
            interaction_id_for_delivery(&local, Some("interaction-1")),
            Some("interaction-1")
        );
        Ok(())
    }

    async fn lazy_context_with_broken_retained_lease(
        identity: AgentIdentity,
        spec: DurableAgentSpec,
        roster: Arc<MutableRoster>,
        lease_provider: Arc<RecordingReleaseLeaseProvider>,
        runtime_instance_id: &str,
    ) -> Result<
        (
            Arc<IdentityRuntime>,
            IdentityFirstRuntimeContext,
            LeaseGrant,
        ),
        Box<dyn std::error::Error>,
    > {
        let continuity_store = Arc::new(LocalContinuityStore::in_memory()?);
        let acquired = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), runtime_instance_id)
            .await?;
        let grant = match acquired.get(&identity) {
            Some(LeaseAcquireResult::Acquired(grant)) => grant.clone(),
            other => return Err(format!("expected retained lease grant, got {other:?}").into()),
        };
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse(&format!("rt:{identity}:0"))?,
            session_id: SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        continuity_store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await?;
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store,
            lease_provider,
            runtime_instance_id: runtime_instance_id.to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        runtime
            .register(
                spec,
                IdentityLifecycleState::Active,
                Some(record),
                Some(grant.clone()),
            )
            .await;
        // Model the public rollback state exercised by
        // `identity_first_runtime_reset_register_failure_cleans_new_member_and_preserves_old_continuity`:
        // the identity is fail-closed Broken while its exact current grant is
        // retained in `entry.lease` for repair.
        runtime
            .entries
            .write()
            .await
            .get_mut(&identity)
            .ok_or("seeded identity disappeared")?
            .state = IdentityLifecycleState::Broken;
        let context = IdentityFirstRuntimeContext::new_with_bootstrap_mode(
            runtime.clone(),
            roster,
            None,
            None,
            None,
            IdentityBootstrapMode::LazyMaterialize,
        );
        Ok((runtime, context, grant))
    }

    async fn active_alias_runtime(
        runtime_instance_id: &str,
        identity: &str,
    ) -> Result<(Arc<IdentityRuntime>, String), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse(identity)?;
        let alias = format!("rt:{identity}:0");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse(&alias)?,
            session_id: SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: runtime_instance_id.to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        runtime
            .register(
                durable_spec(identity, "domain"),
                IdentityLifecycleState::Active,
                Some(record),
                None,
            )
            .await;
        Ok((runtime, alias))
    }

    async fn alias_target(
        runtime: &Arc<IdentityRuntime>,
        alias: &str,
    ) -> Result<MemberAliasLifecycleTarget, Box<dyn std::error::Error>> {
        runtime
            .member_alias_lifecycle_target(alias)
            .await?
            .ok_or_else(|| {
                format!("generated alias did not resolve to lifecycle target: {alias}").into()
            })
    }

    #[tokio::test]
    async fn permanent_stream_loss_breaks_only_the_current_active_embodiment()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:stream-loss")?;
        let (runtime, alias) =
            active_alias_runtime("stream-loss-runtime", identity.as_str()).await?;
        runtime
            .update_lease(
                &identity,
                LeaseGrant {
                    identity: identity.clone(),
                    fencing_token: FencingToken::new(7),
                    ttl: Duration::from_mins(1),
                },
            )
            .await?;

        assert!(
            runtime
                .mark_active_runtime_broken(&identity, &alias, 7, "event stream closed")
                .await?
        );
        assert_eq!(
            runtime.status(&identity).await?.state,
            IdentityLifecycleState::Broken
        );
        assert!(
            runtime
                .entries
                .read()
                .await
                .get(&identity)
                .and_then(|entry| entry.lease.as_ref())
                .is_some(),
            "repair must retain the exact live lease for fenced cleanup"
        );
        Ok(())
    }

    #[tokio::test]
    async fn stale_stream_loss_does_not_break_replacement_embodiment()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:replacement")?;
        let (runtime, _) = active_alias_runtime("replacement-runtime", identity.as_str()).await?;

        assert!(
            !runtime
                .mark_active_runtime_broken(
                    &identity,
                    "rt:domain:replacement:stale",
                    0,
                    "stale event stream closed",
                )
                .await?
        );
        assert_eq!(
            runtime.status(&identity).await?.state,
            IdentityLifecycleState::Active
        );
        Ok(())
    }

    #[tokio::test]
    async fn stale_fence_stream_loss_does_not_break_same_alias_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:replacement-fence")?;
        let (runtime, alias) =
            active_alias_runtime("replacement-fence-runtime", identity.as_str()).await?;
        runtime
            .update_lease(
                &identity,
                LeaseGrant {
                    identity: identity.clone(),
                    fencing_token: FencingToken::new(8),
                    ttl: Duration::from_mins(1),
                },
            )
            .await?;

        assert!(
            !runtime
                .mark_active_runtime_broken(&identity, &alias, 7, "old-fence event stream closed",)
                .await?
        );
        assert_eq!(
            runtime.status(&identity).await?.state,
            IdentityLifecycleState::Active
        );
        Ok(())
    }

    #[tokio::test]
    async fn compound_alias_targets_sort_opposite_orders_without_deadlock()
    -> Result<(), Box<dyn std::error::Error>> {
        let (runtime_a, alias_a) = active_alias_runtime("00-alias-runtime", "domain:alpha").await?;
        let (runtime_b, alias_b) = active_alias_runtime("01-alias-runtime", "domain:beta").await?;

        // Hold the globally first lock while both transactions are admitted.
        // Correct ordering makes both wait on A without touching B. An input-
        // ordered implementation lets the reverse request take B first and
        // then deadlocks once the forward request receives A.
        let held_a = alias_target(&runtime_a, &alias_a)
            .await?
            .lock
            .clone()
            .lock_owned()
            .await;
        let completed = Arc::new(AtomicUsize::new(0));

        let forward = tokio::spawn({
            let targets = vec![
                alias_target(&runtime_a, &alias_a).await?,
                alias_target(&runtime_b, &alias_b).await?,
            ];
            let completed = Arc::clone(&completed);
            async move {
                IdentityRuntime::run_member_alias_targets_operation_tracked(targets, move || {
                    let completed = Arc::clone(&completed);
                    async move {
                        completed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                })
                .await
            }
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if !runtime_a.foreground_operations.lock().await.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await?;

        let reverse = tokio::spawn({
            let targets = vec![
                alias_target(&runtime_b, &alias_b).await?,
                alias_target(&runtime_a, &alias_a).await?,
            ];
            let completed = Arc::clone(&completed);
            async move {
                IdentityRuntime::run_member_alias_targets_operation_tracked(targets, move || {
                    let completed = Arc::clone(&completed);
                    async move {
                        completed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                })
                .await
            }
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if runtime_a.foreground_operations.lock().await.len() >= 2 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await?;
        tokio::task::yield_now().await;

        let b_probe = alias_target(&runtime_b, &alias_b)
            .await?
            .lock
            .try_lock_owned()
            .map_err(|_| "reverse-order request acquired B before globally-first A")?;
        drop(b_probe);
        drop(held_a);

        tokio::time::timeout(Duration::from_secs(2), forward).await???;
        tokio::time::timeout(Duration::from_secs(2), reverse).await???;
        assert_eq!(completed.load(Ordering::SeqCst), 2);

        runtime_a.close_foreground_operations();
        runtime_b.close_foreground_operations();
        tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(
                runtime_a.join_foreground_operations(),
                runtime_b.join_foreground_operations()
            );
        })
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn dropped_compound_alias_caller_still_completes_operation_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        let (runtime_a, alias_a) = active_alias_runtime("00-drop-runtime", "domain:alpha").await?;
        let (runtime_b, alias_b) = active_alias_runtime("01-drop-runtime", "domain:beta").await?;
        let targets = vec![
            alias_target(&runtime_a, &alias_a).await?,
            alias_target(&runtime_b, &alias_b).await?,
        ];
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let caller = tokio::spawn({
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            let completed = Arc::clone(&completed);
            async move {
                IdentityRuntime::run_member_alias_targets_operation_tracked(targets, move || {
                    let entered = Arc::clone(&entered);
                    let release = Arc::clone(&release);
                    let completed = Arc::clone(&completed);
                    async move {
                        entered.add_permits(1);
                        release
                            .acquire()
                            .await
                            .map_err(|error| error.to_string())?
                            .forget();
                        completed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                })
                .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), entered.acquire())
            .await??
            .forget();

        caller.abort();
        match caller.await {
            Err(error) => assert!(error.is_cancelled()),
            Ok(result) => {
                return Err(format!("aborted caller unexpectedly returned: {result:?}").into());
            }
        }
        release.add_permits(1);

        runtime_a.close_foreground_operations();
        runtime_b.close_foreground_operations();
        tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(
                runtime_a.join_foreground_operations(),
                runtime_b.join_foreground_operations()
            );
        })
        .await?;
        assert_eq!(
            completed.load(Ordering::SeqCst),
            1,
            "runtime-owned compound transaction must reach its boundary"
        );
        Ok(())
    }

    #[tokio::test]
    async fn either_compound_alias_runtime_shutdown_waits_for_operation_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        for shutdown_first_runtime in [true, false] {
            let suffix = if shutdown_first_runtime {
                "first"
            } else {
                "second"
            };
            let (runtime_a, alias_a) =
                active_alias_runtime(&format!("00-shutdown-{suffix}"), "domain:alpha").await?;
            let (runtime_b, alias_b) =
                active_alias_runtime(&format!("01-shutdown-{suffix}"), "domain:beta").await?;
            let targets = vec![
                alias_target(&runtime_a, &alias_a).await?,
                alias_target(&runtime_b, &alias_b).await?,
            ];
            let entered = Arc::new(tokio::sync::Semaphore::new(0));
            let release = Arc::new(tokio::sync::Semaphore::new(0));
            let operation = tokio::spawn({
                let entered = Arc::clone(&entered);
                let release = Arc::clone(&release);
                async move {
                    IdentityRuntime::run_member_alias_targets_operation_tracked(
                        targets,
                        move || {
                            let entered = Arc::clone(&entered);
                            let release = Arc::clone(&release);
                            async move {
                                entered.add_permits(1);
                                release
                                    .acquire()
                                    .await
                                    .map_err(|error| error.to_string())?
                                    .forget();
                                Ok(())
                            }
                        },
                    )
                    .await
                }
            });
            tokio::time::timeout(Duration::from_secs(2), entered.acquire())
                .await??
                .forget();

            let shutting_down = if shutdown_first_runtime {
                Arc::clone(&runtime_a)
            } else {
                Arc::clone(&runtime_b)
            };
            shutting_down.close_foreground_operations();
            let mut shutdown = tokio::spawn(async move {
                shutting_down.join_foreground_operations().await;
            });
            assert!(
                tokio::time::timeout(Duration::from_millis(50), &mut shutdown)
                    .await
                    .is_err(),
                "shutdown of {suffix} participating runtime returned before compound boundary"
            );

            release.add_permits(1);
            tokio::time::timeout(Duration::from_secs(2), &mut shutdown).await??;
            tokio::time::timeout(Duration::from_secs(2), operation).await???;

            runtime_a.close_foreground_operations();
            runtime_b.close_foreground_operations();
            tokio::time::timeout(Duration::from_secs(2), async {
                tokio::join!(
                    runtime_a.join_foreground_operations(),
                    runtime_b.join_foreground_operations()
                );
            })
            .await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn restore_flow_bounds_parallel_member_creation() -> Result<(), Box<dyn std::error::Error>>
    {
        let specs = (0..8)
            .map(|index| {
                Ok(durable_spec(
                    AgentIdentity::parse(&format!("domain:restore-{index}"))?,
                    "domain",
                ))
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        let bridge = Arc::new(RecordingBridge {
            create_delay: Duration::from_millis(25),
            ..RecordingBridge::default()
        });
        let runtime = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "parallel-restore-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        });

        let result = super::super::orchestrator::restore_flow(&runtime, &specs, None, None).await?;

        assert_eq!(result.outcomes.len(), specs.len());
        assert!(
            bridge.max_creates_in_flight() > 1,
            "member creation should no longer be serial"
        );
        assert!(
            bridge.max_creates_in_flight()
                <= super::super::orchestrator::IDENTITY_RESTORE_CONCURRENCY,
            "restore concurrency must remain bounded"
        );
        Ok(())
    }

    #[tokio::test]
    async fn reset_reprofiles_session_from_runtime_configured_roster_provider()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:security")?;
        let roster = Arc::new(MutableRoster::new(vec![durable_spec(
            identity.clone(),
            "domain",
        )]));
        let bridge = Arc::new(RecordingBridge::default());
        let runtime = Arc::new(
            IdentityRuntime::new(IdentityRuntimeConfig {
                continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
                lease_provider: Arc::new(LocalLeaseProvider::new()),
                runtime_instance_id: "reset-reprofile-test".to_string(),
                has_runtime_store: true,
                durability_policy: DurabilityPolicy::SyncWriteThrough,
                bridge: Some(bridge.clone()),
                default_timeout: None,
            })
            .with_reset_roster_provider(roster.clone()),
        );

        super::super::orchestrator::restore_flow(
            &runtime,
            &roster
                .roster(&RosterContext {
                    mob_definition: None,
                    previous_identities: Vec::new(),
                })
                .await?,
            None,
            None,
        )
        .await?;
        roster
            .set(vec![durable_spec(identity.clone(), "security")])
            .await;

        let record = runtime.reset(&identity).await?;

        assert_eq!(record.generation.get(), 1);
        assert_eq!(
            bridge.create_profiles().await,
            vec!["domain".to_string(), "security".to_string()]
        );
        let status = runtime.status(&identity).await?;
        assert_eq!(
            status.profile.map(|profile| profile.to_string()).as_deref(),
            Some("security")
        );
        Ok(())
    }

    #[tokio::test]
    async fn shutdown_joins_reset_after_outer_abort_at_final_upsert()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:security")?;
        let roster = Arc::new(MutableRoster::new(vec![durable_spec(
            identity.clone(),
            "domain",
        )]));
        let bridge = Arc::new(RecordingBridge::default());
        let store = Arc::new(GatedResetContinuityStore::new()?);
        let runtime = Arc::new(
            IdentityRuntime::new(IdentityRuntimeConfig {
                continuity_store: store.clone(),
                lease_provider: Arc::new(LocalLeaseProvider::new()),
                runtime_instance_id: "tracked-reset-shutdown-test".to_string(),
                has_runtime_store: true,
                durability_policy: DurabilityPolicy::SyncWriteThrough,
                bridge: Some(bridge.clone()),
                default_timeout: None,
            })
            .with_reset_roster_provider(roster.clone()),
        );

        super::super::orchestrator::restore_flow(
            &runtime,
            &roster
                .roster(&RosterContext {
                    mob_definition: None,
                    previous_identities: Vec::new(),
                })
                .await?,
            None,
            None,
        )
        .await?;
        let old_record = match store
            .resolve_many(std::slice::from_ref(&identity))
            .await?
            .remove(&identity)
        {
            Some(ContinuityResolveState::Ready { record }) => record,
            other => return Err(format!("expected initial continuity, got {other:?}").into()),
        };
        store.fail_after_successful_upserts(3);

        let outer = tokio::spawn({
            let runtime = Arc::clone(&runtime);
            let identity = identity.clone();
            async move { runtime.reset_tracked(&identity).await }
        });
        // Bounded. This waits for an injected failure at the FINAL continuity
        // upsert, which is unreachable whenever reset refuses earlier - for
        // example the reprofile guard refusing a drifted spec before the
        // destructive step. Unbounded, that turns into a suite-blocking hang
        // that reports nothing; bounded, it names itself.
        tokio::time::timeout(Duration::from_secs(10), store.wait_for_failure())
            .await
            .expect(
                "reset never reached the final continuity upsert, so the injected failure could \
                 not fire; reset refused earlier in the flow",
            );
        outer.abort();
        match outer.await {
            Err(error) => assert!(error.is_cancelled()),
            Ok(result) => {
                return Err(
                    format!("outer reset waiter unexpectedly completed: {result:?}").into(),
                );
            }
        }

        runtime.close_foreground_operations();
        let mut join = tokio::spawn({
            let runtime = Arc::clone(&runtime);
            async move { runtime.join_foreground_operations().await }
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut join)
                .await
                .is_err(),
            "shutdown must wait for the runtime-owned reset transaction"
        );
        assert!(
            bridge.retired_runtime_ids().await.is_empty(),
            "rollback cleanup cannot run before the final upsert resolves"
        );

        store.release_failure();
        tokio::time::timeout(Duration::from_secs(2), &mut join)
            .await
            .map_err(|_| "shutdown did not join reset rollback")??;

        let resolved = store.resolve_many(std::slice::from_ref(&identity)).await?;
        assert_eq!(
            resolved.get(&identity),
            Some(&ContinuityResolveState::Ready {
                record: old_record.clone(),
            }),
            "abandoned reset must roll durable continuity back before shutdown completes"
        );
        let status = runtime.status(&identity).await?;
        assert_eq!(
            status.agent_runtime_id.as_ref(),
            Some(&old_record.agent_runtime_id)
        );
        assert_eq!(status.state, IdentityLifecycleState::Broken);
        assert_eq!(
            bridge.retired_runtime_ids().await,
            vec!["rt:domain:security:1".to_string()],
            "rollback must retire only the tentative reset generation"
        );
        Ok(())
    }

    #[tokio::test]
    async fn shutdown_cancels_reset_customizer_and_restores_old_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:security")?;
        let spec = durable_spec(identity.clone(), "domain");
        let bridge = Arc::new(RecordingBridge::default());
        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: store.clone(),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "reset-customizer-shutdown-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));

        super::super::orchestrator::restore_flow(&runtime, std::slice::from_ref(&spec), None, None)
            .await?;
        let old_record = match store
            .resolve_many(std::slice::from_ref(&identity))
            .await?
            .remove(&identity)
        {
            Some(ContinuityResolveState::Ready { record }) => record,
            other => return Err(format!("expected initial continuity, got {other:?}").into()),
        };

        let customizer = Arc::new(GatedResetCustomizer::default());
        runtime.set_agent_customizer(Some(customizer.clone())).await;
        let outer = tokio::spawn({
            let runtime = Arc::clone(&runtime);
            let identity = identity.clone();
            async move { runtime.reset_tracked(&identity).await }
        });
        tokio::time::timeout(Duration::from_secs(2), customizer.wait_for_entry())
            .await
            .map_err(|_| "reset did not enter the gated customizer")?;
        outer.abort();
        match outer.await {
            Err(error) => assert!(error.is_cancelled()),
            Ok(result) => {
                return Err(
                    format!("outer reset waiter unexpectedly completed: {result:?}").into(),
                );
            }
        }

        runtime.close_foreground_operations();
        tokio::time::timeout(Duration::from_secs(2), runtime.join_foreground_operations())
            .await
            .map_err(|_| "shutdown hung on the reset customizer")?;

        let resolved = store.resolve_many(std::slice::from_ref(&identity)).await?;
        assert_eq!(
            resolved.get(&identity),
            Some(&ContinuityResolveState::Ready {
                record: old_record.clone(),
            }),
            "shutdown cancellation must roll durable continuity back to the old generation"
        );
        let status = runtime.status(&identity).await?;
        assert_eq!(status.state, IdentityLifecycleState::Active);
        assert_eq!(
            status.agent_runtime_id.as_ref(),
            Some(&old_record.agent_runtime_id)
        );
        assert_eq!(status.session_id.as_ref(), Some(&old_record.session_id));
        assert_eq!(bridge.create_profiles().await, vec!["domain".to_string()]);
        assert!(
            bridge.retired_runtime_ids().await.is_empty(),
            "cancellation before session installation must not touch either bridge generation"
        );
        Ok(())
    }

    #[tokio::test]
    async fn shutdown_cancelled_dormant_reset_releases_temporary_lease()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:dormant")?;
        let bridge = Arc::new(RecordingBridge::default());
        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let runtime_instance_id = "dormant-reset-customizer-shutdown-test";
        let acquired = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), runtime_instance_id)
            .await?;
        let initial_grant = match acquired.get(&identity) {
            Some(super::super::types::LeaseAcquireResult::Acquired(grant)) => grant.clone(),
            other => return Err(format!("expected initial lease, got {other:?}").into()),
        };
        let old_record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:domain:dormant:0")?,
            session_id: SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        store
            .upsert_continuity_record(&old_record, initial_grant.fencing_token)
            .await?;
        lease_provider
            .release_leases(std::slice::from_ref(&initial_grant))
            .await?;
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: store.clone(),
            lease_provider: lease_provider.clone(),
            runtime_instance_id: runtime_instance_id.to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));
        runtime
            .register(
                durable_spec(identity.clone(), "domain"),
                IdentityLifecycleState::Dormant,
                Some(old_record.clone()),
                None,
            )
            .await;

        let customizer = Arc::new(GatedResetCustomizer::default());
        runtime.set_agent_customizer(Some(customizer.clone())).await;
        let outer = tokio::spawn({
            let runtime = Arc::clone(&runtime);
            let identity = identity.clone();
            async move { runtime.reset_tracked(&identity).await }
        });
        tokio::time::timeout(Duration::from_secs(2), customizer.wait_for_entry())
            .await
            .map_err(|_| "dormant reset did not enter the gated customizer")?;
        outer.abort();
        match outer.await {
            Err(error) => assert!(error.is_cancelled()),
            Ok(result) => {
                return Err(
                    format!("outer dormant reset unexpectedly completed: {result:?}").into(),
                );
            }
        }
        runtime.close_foreground_operations();
        tokio::time::timeout(Duration::from_secs(2), runtime.join_foreground_operations())
            .await
            .map_err(|_| "shutdown hung on the dormant reset customizer")?;

        let status = runtime.status(&identity).await?;
        assert_eq!(status.state, IdentityLifecycleState::Dormant);
        assert!(status.lease.is_none());
        let resolved = store.resolve_many(std::slice::from_ref(&identity)).await?;
        assert_eq!(
            resolved.get(&identity),
            Some(&ContinuityResolveState::Ready {
                record: old_record.clone(),
            })
        );
        let failover = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), "dormant-reset-failover")
            .await?;
        assert!(
            matches!(
                failover.get(&identity),
                Some(super::super::types::LeaseAcquireResult::Acquired(_))
            ),
            "shutdown rollback must release the dormant reset lease: {failover:?}"
        );
        assert!(bridge.create_profiles().await.is_empty());
        assert!(bridge.retired_runtime_ids().await.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn tracked_identity_respawn_preserves_authoritative_continuity()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:durable-respawn")?;
        let spec = durable_spec(identity.clone(), "domain");
        let bridge = Arc::new(RecordingBridge::default());
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "durable-respawn".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));
        super::super::orchestrator::restore_flow(&runtime, std::slice::from_ref(&spec), None, None)
            .await?;

        let before = runtime.status(&identity).await?;
        let runtime_alias = before
            .agent_runtime_id
            .clone()
            .ok_or("missing durable runtime id")?;
        let respawned = runtime
            .respawn_identity_in_place_tracked(&identity, Some(runtime_alias.as_str()))
            .await?;

        assert_eq!(Some(respawned.session_id), before.session_id);
        assert_eq!(Some(respawned.agent_runtime_id), before.agent_runtime_id);
        assert_eq!(Some(respawned.generation), before.generation);
        assert_eq!(
            runtime.status(&identity).await?.state,
            IdentityLifecycleState::Active
        );
        assert!(
            bridge.retired_runtime_ids().await.is_empty(),
            "identity respawn must not retire and recreate the authoritative session"
        );
        Ok(())
    }

    #[tokio::test]
    async fn stale_alias_materialization_preserves_active_and_dormant_bootstrap_status()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:bootstrap-alias")?;
        let spec = durable_spec(identity.clone(), "domain");
        let current_alias = AgentRuntimeId::parse("rt:domain:bootstrap-alias:1")?;
        let stale_alias = "rt:domain:bootstrap-alias:0";
        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let grants = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), "bootstrap-alias-status")
            .await?;
        let grant = match grants.get(&identity) {
            Some(super::super::types::LeaseAcquireResult::Acquired(grant)) => grant.clone(),
            other => return Err(format!("expected acquired lease, got {other:?}").into()),
        };
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: current_alias,
            session_id: SessionId::new(),
            generation: ContinuityGeneration::new(1),
            checkpoint_version: CheckpointVersion::new(0),
        };
        store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await?;
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: store,
            lease_provider,
            runtime_instance_id: "bootstrap-alias-status".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        runtime
            .register(
                spec.clone(),
                IdentityLifecycleState::Active,
                Some(record),
                Some(grant),
            )
            .await;

        let active_generation =
            runtime.begin_identity_bootstrap_pending(IdentityBootstrapMode::LazyMaterialize);
        runtime
            .begin_identity_bootstrap(
                active_generation,
                IdentityBootstrapMode::LazyMaterialize,
                std::slice::from_ref(&spec),
            )
            .await;
        runtime.modify_bootstrap_status(Some(active_generation), |status| {
            status.complete = true;
            status.refresh_aggregates();
        });
        let active_before = runtime.identity_bootstrap_status();
        assert_eq!(
            active_before
                .identities
                .get(&identity)
                .map(|entry| entry.state),
            Some(IdentityBootstrapState::Active)
        );
        assert!(matches!(
            runtime
                .materialize_with_expected_member_alias(&identity, Some(stale_alias))
                .await,
            Err(IdentityRuntimeError::StaleRuntimeAlias { .. })
        ));
        assert_eq!(runtime.identity_bootstrap_status(), active_before);

        runtime.retire(&identity).await?;
        let dormant_generation =
            runtime.begin_identity_bootstrap_pending(IdentityBootstrapMode::LazyMaterialize);
        runtime
            .begin_identity_bootstrap(
                dormant_generation,
                IdentityBootstrapMode::LazyMaterialize,
                std::slice::from_ref(&spec),
            )
            .await;
        runtime.modify_bootstrap_status(Some(dormant_generation), |status| {
            status.complete = true;
            status.refresh_aggregates();
        });
        let dormant_before = runtime.identity_bootstrap_status();
        assert_eq!(
            dormant_before
                .identities
                .get(&identity)
                .map(|entry| entry.state),
            Some(IdentityBootstrapState::Dormant)
        );
        assert!(matches!(
            runtime
                .materialize_with_expected_member_alias(&identity, Some(stale_alias))
                .await,
            Err(IdentityRuntimeError::StaleRuntimeAlias { .. })
        ));
        assert_eq!(runtime.identity_bootstrap_status(), dormant_before);
        Ok(())
    }

    #[tokio::test]
    async fn eager_reconcile_hot_reloads_active_metadata_without_retire_or_resume()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("identity:luka")?;
        let mut initial_spec = durable_spec(identity.clone(), "personal");
        initial_spec
            .labels
            .insert("revision".to_string(), "v1".to_string());
        let roster = Arc::new(MutableRoster::new(vec![initial_spec.clone()]));
        let bridge = Arc::new(HotReloadBridge::default());
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "eager-hot-reload-contract".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));
        let context = IdentityFirstRuntimeContext::new_with_bootstrap_mode(
            runtime.clone(),
            roster.clone(),
            None,
            None,
            None,
            IdentityBootstrapMode::EagerMaterialize,
        );
        context
            .bootstrap_roster(std::slice::from_ref(&initial_spec))
            .await?;

        let before = runtime.status(&identity).await?;
        assert_eq!(before.state, IdentityLifecycleState::Active);
        let before_session = before.session_id.clone();
        let before_runtime_id = before.agent_runtime_id.clone();
        let before_generation = before.generation;
        let before_token = before
            .lease
            .as_ref()
            .ok_or("initial active identity must have a lease")?
            .fencing_token;

        let mut updated_spec = initial_spec;
        updated_spec.addressability = AgentAddressability::InternalOnly;
        updated_spec
            .labels
            .insert("timezone".to_string(), "Europe/Stockholm".to_string());
        updated_spec
            .labels
            .insert("revision".to_string(), "v2".to_string());
        roster.set(vec![updated_spec]).await;

        let result = context.refresh_desired_topology().await?;
        assert!(matches!(
            result.outcomes.get(&identity),
            Some(super::super::orchestrator::RestoreOutcome::Resumed { .. })
        ));
        let after = runtime.status(&identity).await?;
        assert_eq!(after.state, IdentityLifecycleState::Active);
        assert_eq!(after.addressability, AgentAddressability::InternalOnly);
        assert_eq!(
            after.labels.get("timezone").map(String::as_str),
            Some("Europe/Stockholm")
        );
        assert_eq!(after.session_id, before_session);
        assert_eq!(after.agent_runtime_id, before_runtime_id);
        assert_eq!(after.generation, before_generation);
        assert_eq!(
            after
                .lease
                .as_ref()
                .ok_or("hot-reloaded identity must retain its lease")?
                .fencing_token,
            before_token,
            "same-profile reconcile must preserve exact live authority"
        );
        assert_eq!(bridge.retire_calls.load(Ordering::SeqCst), 0);
        assert_eq!(bridge.resume_calls.load(Ordering::SeqCst), 0);

        assert!(matches!(
            runtime
                .send(
                    &identity,
                    &meerkat_core::ContentInput::Text("external".to_string()),
                )
                .await,
            Err(IdentityRuntimeError::NotAddressable(_))
        ));
        let (dispatch_token, durable) = runtime
            .dispatch(&identity, &DispatchInput::system("system notice"))
            .await?;
        assert_eq!(dispatch_token, before_token);
        assert!(durable);
        Ok(())
    }

    #[tokio::test]
    async fn lazy_unchanged_reconcile_retained_broken_lease_failure_stays_shutdown_visible()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("identity:broken-retained-unchanged")?;
        let spec = durable_spec(identity.clone(), "personal");
        let roster = Arc::new(MutableRoster::new(vec![spec.clone()]));
        let lease_provider = Arc::new(RecordingReleaseLeaseProvider::default());
        let (runtime, context, retained_grant) = lazy_context_with_broken_retained_lease(
            identity.clone(),
            spec,
            roster,
            lease_provider.clone(),
            "broken-retained-unchanged",
        )
        .await?;

        lease_provider.fail_next_release();
        let error = match context.refresh_desired_topology().await {
            Err(error) => error,
            Ok(_) => return Err("failed exact release allowed lazy registration".into()),
        };
        assert!(
            error
                .to_string()
                .contains("synthetic retained Broken lease release failure")
        );
        let status = runtime.status(&identity).await?;
        assert_eq!(status.state, IdentityLifecycleState::Broken);
        assert!(
            status.lease.is_none(),
            "failed release authority must be pending, never advertised as active"
        );
        assert_eq!(
            runtime
                .entries
                .read()
                .await
                .get(&identity)
                .and_then(|entry| entry.pending_lease_release.as_ref())
                .map(|grant| grant.fencing_token),
            Some(retained_grant.fencing_token),
            "the exact retained token must survive provider failure"
        );
        assert_eq!(
            lease_provider
                .release_attempts()
                .await
                .iter()
                .map(|grant| grant.fencing_token)
                .collect::<Vec<_>>(),
            vec![retained_grant.fencing_token]
        );

        assert_eq!(
            runtime.release_all_leases_for_shutdown().await?,
            1,
            "shutdown must retain visibility of the staged exact grant"
        );
        assert_eq!(
            lease_provider
                .release_attempts()
                .await
                .iter()
                .map(|grant| grant.fencing_token)
                .collect::<Vec<_>>(),
            vec![retained_grant.fencing_token, retained_grant.fencing_token],
            "shutdown must retry the same exact fencing token"
        );
        assert!(
            runtime
                .entries
                .read()
                .await
                .get(&identity)
                .is_some_and(|entry| {
                    entry.lease.is_none() && entry.pending_lease_release.is_none()
                })
        );

        context.refresh_desired_topology().await?;
        let recovered = runtime.status(&identity).await?;
        assert_eq!(recovered.state, IdentityLifecycleState::Dormant);
        assert!(recovered.lease.is_none());
        let failover = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), "other-runtime")
            .await?;
        assert!(matches!(
            failover.get(&identity),
            Some(LeaseAcquireResult::Acquired(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn lazy_profile_replace_releases_retained_broken_lease_before_overwrite()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("identity:broken-retained-replace")?;
        let original = durable_spec(identity.clone(), "personal-v1");
        let roster = Arc::new(MutableRoster::new(vec![original.clone()]));
        let lease_provider = Arc::new(RecordingReleaseLeaseProvider::default());
        let (runtime, context, retained_grant) = lazy_context_with_broken_retained_lease(
            identity.clone(),
            original,
            roster.clone(),
            lease_provider.clone(),
            "broken-retained-replace",
        )
        .await?;

        roster
            .set(vec![durable_spec(identity.clone(), "personal-v2")])
            .await;
        context.refresh_desired_topology().await?;

        let status = runtime.status(&identity).await?;
        assert_eq!(status.state, IdentityLifecycleState::Dormant);
        assert_eq!(
            status.profile.as_ref().map(ToString::to_string).as_deref(),
            Some("personal-v2")
        );
        assert!(status.lease.is_none());
        assert_eq!(
            lease_provider
                .release_attempts()
                .await
                .iter()
                .map(|grant| grant.fencing_token)
                .collect::<Vec<_>>(),
            vec![retained_grant.fencing_token],
            "profile replacement must release the retained exact token before lazy registration"
        );
        assert!(
            runtime
                .entries
                .read()
                .await
                .get(&identity)
                .is_some_and(|entry| entry.pending_lease_release.is_none())
        );
        let failover = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), "replacement-failover")
            .await?;
        assert!(matches!(
            failover.get(&identity),
            Some(LeaseAcquireResult::Acquired(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn lease_lost_then_roster_remove_cleans_lower_plane_before_dropping_entry()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("identity:lost-remove")?;
        let spec = durable_spec(identity.clone(), "personal");
        let roster = Arc::new(MutableRoster::new(vec![spec.clone()]));
        let bridge = Arc::new(LostCleanupBridge::default());
        let lease_provider = Arc::new(LostRenewLeaseProvider::default());
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: lease_provider.clone(),
            runtime_instance_id: "lost-remove-cleanup".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));
        let context = IdentityFirstRuntimeContext::new_with_bootstrap_mode(
            runtime.clone(),
            roster.clone(),
            None,
            None,
            None,
            IdentityBootstrapMode::EagerMaterialize,
        );
        context
            .bootstrap_roster(std::slice::from_ref(&spec))
            .await?;
        assert_eq!(bridge.member_count().await, 1);
        assert_eq!(bridge.session_runtime_state_count().await, 1);

        force_renewal_lost(&runtime, &lease_provider, &identity).await?;
        assert_eq!(
            runtime.status(&identity).await?.state,
            IdentityLifecycleState::Broken
        );
        assert_eq!(bridge.member_count().await, 1);
        let unregisters_after_lost = bridge.unregister_calls.load(Ordering::SeqCst);

        roster.set(Vec::new()).await;
        context.refresh_desired_topology().await?;
        assert!(!runtime.contains(&identity).await);
        assert_eq!(
            bridge.member_count().await,
            0,
            "stale member survived removal"
        );
        assert_eq!(
            bridge.session_runtime_state_count().await,
            0,
            "stale session authority survived removal"
        );
        assert_eq!(bridge.retire_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            bridge.unregister_calls.load(Ordering::SeqCst),
            unregisters_after_lost + 1,
            "roster removal must idempotently unregister after Lost"
        );
        Ok(())
    }

    #[tokio::test]
    async fn lease_lost_then_profile_replace_cleans_old_member_before_resume()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("identity:lost-replace")?;
        let original = durable_spec(identity.clone(), "personal-v1");
        let roster = Arc::new(MutableRoster::new(vec![original.clone()]));
        let bridge = Arc::new(LostCleanupBridge::default());
        let lease_provider = Arc::new(LostRenewLeaseProvider::default());
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: lease_provider.clone(),
            runtime_instance_id: "lost-replace-cleanup".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));
        let context = IdentityFirstRuntimeContext::new_with_bootstrap_mode(
            runtime.clone(),
            roster.clone(),
            None,
            None,
            None,
            IdentityBootstrapMode::EagerMaterialize,
        );
        context
            .bootstrap_roster(std::slice::from_ref(&original))
            .await?;
        let registered_after_bootstrap: BTreeSet<String> =
            bridge.session_runtime_states.lock().await.clone();
        force_renewal_lost(&runtime, &lease_provider, &identity).await?;
        let unregisters_after_lost = bridge.unregister_calls.load(Ordering::SeqCst);

        let replacement = durable_spec(identity.clone(), "personal-v2");
        roster.set(vec![replacement]).await;
        context.refresh_desired_topology().await?;

        let status = runtime.status(&identity).await?;
        assert_eq!(status.state, IdentityLifecycleState::Active);
        assert_eq!(
            status.profile.as_ref().map(ToString::to_string).as_deref(),
            Some("personal-v2")
        );
        // Measured at the meerkat 0.8.22 / mobkit 0.8.16 pair (release-lead
        // fallback ruling 2026-08-13, dual-driver-unattributed): the
        // retire+unregister cleanup sequence runs TWICE inside the single
        // refresh window on the same runtime/session (the old binding ran it
        // once). Phase-bracketed diagnostics attribute the first pair to
        // reconcile_roster_members; the second pair's frames were inlined
        // beyond attribution and the driver pair is an OPEN follow-up on the
        // release record (candidate lead: retire_reset_superseded_member's
        // single caller). The redundancy is idempotent and the final state is
        // fully correct, so the counts are pinned at the measured values -
        // if either count changes again, RE-DERIVE the driver set; do not
        // pattern-match the number. Collapse of the double-drive is a tracked
        // next-release item per the fast-track ruling.
        assert_eq!(
            bridge.retire_calls.load(Ordering::SeqCst),
            2,
            "measured at the pair: reconcile cleanup plus one unattributed idempotent repeat"
        );
        assert_eq!(
            bridge.unregister_calls.load(Ordering::SeqCst),
            unregisters_after_lost + 2,
            "measured at the pair: one unregister per retire inside the refresh window"
        );
        let unregistered_sessions_log: Vec<String> =
            bridge.unregistered_sessions.lock().await.clone();
        assert!(
            unregistered_sessions_log
                .iter()
                .all(|session| registered_after_bootstrap.contains(session)),
            "every unregister must target the pre-loss session, never the replacement: {unregistered_sessions_log:?}"
        );
        assert_eq!(
            bridge.resume_collisions.load(Ordering::SeqCst),
            0,
            "replacement encountered the stale lower-plane alias"
        );
        assert_eq!(bridge.member_count().await, 1);
        assert_eq!(bridge.session_runtime_state_count().await, 1);
        Ok(())
    }

    #[tokio::test]
    async fn foreground_materialization_completion_cannot_overwrite_newer_lazy_reconcile_status()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:foreground-generation")?;
        let mut v1 = durable_spec(identity.clone(), "domain");
        v1.labels
            .insert("roster_revision".to_string(), "v1".to_string());
        let mut v2 = v1.clone();
        v2.profile = meerkat_mob::ProfileName::from("replacement");
        v2.labels
            .insert("roster_revision".to_string(), "v2".to_string());

        let roster = Arc::new(MutableRoster::new(vec![v1.clone()]));
        let bridge = Arc::new(RecordingBridge::default());
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "foreground-generation-fence".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));
        let context = IdentityFirstRuntimeContext::new_with_bootstrap_mode(
            runtime.clone(),
            roster.clone(),
            None,
            None,
            None,
            IdentityBootstrapMode::LazyMaterialize,
        );
        context.bootstrap_roster(std::slice::from_ref(&v1)).await?;
        let (v1_generation, initial) = runtime.identity_bootstrap_status_with_generation();
        assert!(initial.complete);
        assert!(!initial.ready);
        assert_eq!(
            initial.identities.get(&identity).map(|entry| entry.state),
            Some(IdentityBootstrapState::Dormant)
        );

        // Pause the v1 foreground call after its lifecycle transaction has
        // committed Active but before its outer readiness bookkeeping runs.
        // The lifecycle lock is free at this seam, so a v2 roster pass can
        // retire the v1 member and install a new lazy Dormant snapshot.
        let completion_entered = Arc::new(Notify::new());
        let release_completion = Arc::new(Notify::new());
        let materialize = tokio::spawn({
            let runtime = runtime.clone();
            let identity = identity.clone();
            let completion_entered = completion_entered.clone();
            let release_completion = release_completion.clone();
            async move {
                runtime
                    .materialize_with_expected_member_alias_after_inner(
                        &identity,
                        None,
                        async move {
                            completion_entered.notify_one();
                            release_completion.notified().await;
                        },
                    )
                    .await
            }
        });
        completion_entered.notified().await;
        assert_eq!(
            runtime.status(&identity).await?.state,
            IdentityLifecycleState::Active
        );

        roster.set(vec![v2]).await;
        context.refresh_desired_topology().await?;
        let after_v2 = runtime.identity_bootstrap_status();
        assert!(after_v2.complete);
        assert!(!after_v2.ready);
        assert_eq!(
            after_v2.identities.get(&identity).map(|entry| entry.state),
            Some(IdentityBootstrapState::Dormant)
        );
        let lifecycle = runtime.status(&identity).await?;
        assert_eq!(lifecycle.state, IdentityLifecycleState::Dormant);
        assert_eq!(
            lifecycle.labels.get("roster_revision").map(String::as_str),
            Some("v2")
        );
        assert_eq!(bridge.retired_runtime_ids().await.len(), 1);

        // Let the older v1 outer future run its terminal status update. Its
        // captured generation must make the update a no-op against v2.
        release_completion.notify_one();
        materialize.await??;
        assert_eq!(runtime.identity_bootstrap_status(), after_v2);
        assert_eq!(
            runtime.status(&identity).await?.state,
            IdentityLifecycleState::Dormant
        );
        assert_ne!(
            runtime.identity_bootstrap_status_with_generation().0,
            v1_generation
        );
        Ok(())
    }

    #[tokio::test]
    async fn foreground_materialization_uses_generation_of_reconcile_that_wins_lifecycle_lock()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:foreground-reconcile-first")?;
        let spec = durable_spec(identity.clone(), "domain");
        let roster = Arc::new(MutableRoster::new(vec![spec.clone()]));
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "foreground-reconcile-first".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let context = IdentityFirstRuntimeContext::new_with_bootstrap_mode(
            runtime.clone(),
            roster,
            None,
            None,
            None,
            IdentityBootstrapMode::LazyMaterialize,
        );
        context
            .bootstrap_roster(std::slice::from_ref(&spec))
            .await?;
        let first_generation = runtime.identity_bootstrap_status_with_generation().0;

        // Publish G+1 with an unchanged desired spec, then pause its reconcile
        // while it owns the lifecycle lock. This is the ordering that a
        // pre-lock global generation sample gets wrong: materialization starts
        // during G+1 and must operate on the G+1-stamped entry, even though the
        // entry itself was originally registered by G.
        let next_generation =
            runtime.begin_identity_bootstrap_pending(IdentityBootstrapMode::LazyMaterialize);
        assert_ne!(next_generation, first_generation);
        runtime
            .begin_identity_bootstrap(
                next_generation,
                IdentityBootstrapMode::LazyMaterialize,
                std::slice::from_ref(&spec),
            )
            .await;

        let reconcile_holds_lock = Arc::new(Notify::new());
        let release_reconcile = Arc::new(Notify::new());
        let paused = Arc::new(AtomicBool::new(false));
        let reconcile = tokio::spawn({
            let runtime = runtime.clone();
            let spec = spec.clone();
            let reconcile_holds_lock = reconcile_holds_lock.clone();
            let release_reconcile = release_reconcile.clone();
            let paused = paused.clone();
            async move {
                runtime
                    .reconcile_roster_members_after_lifecycle_lock(
                        std::slice::from_ref(&spec),
                        next_generation,
                        move |_| {
                            let should_pause = !paused.swap(true, Ordering::SeqCst);
                            let reconcile_holds_lock = reconcile_holds_lock.clone();
                            let release_reconcile = release_reconcile.clone();
                            async move {
                                if should_pause {
                                    reconcile_holds_lock.notify_one();
                                    release_reconcile.notified().await;
                                }
                            }
                        },
                    )
                    .await
            }
        });
        reconcile_holds_lock.notified().await;

        let materialize = tokio::spawn({
            let runtime = runtime.clone();
            let identity = identity.clone();
            async move { runtime.materialize(&identity).await }
        });
        // Poll the spawned future until it queues behind the lifecycle owner.
        tokio::task::yield_now().await;
        release_reconcile.notify_one();
        reconcile.await??;
        materialize.await??;

        let (published_generation, status) = runtime.identity_bootstrap_status_with_generation();
        assert_eq!(published_generation, next_generation);
        assert_eq!(
            status.identities.get(&identity).map(|entry| entry.state),
            Some(IdentityBootstrapState::Active),
            "G+1 materialization completion was misbound to the older pass"
        );
        let entries = runtime.entries.read().await;
        assert_eq!(
            entries
                .get(&identity)
                .map(|entry| entry.bootstrap_generation),
            Some(next_generation),
            "unchanged roster acceptance must still stamp the new generation"
        );
        Ok(())
    }

    #[tokio::test]
    async fn stale_runtime_alias_preflight_cannot_mutate_new_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:alias-race")?;
        let old_alias = "rt:domain:alias-race:0";
        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let acquired = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), "alias-race-test")
            .await?;
        let grant = match acquired.get(&identity) {
            Some(super::super::types::LeaseAcquireResult::Acquired(grant)) => grant.clone(),
            other => return Err(format!("expected initial lease, got {other:?}").into()),
        };
        let old_record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse(old_alias)?,
            session_id: SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        store
            .upsert_continuity_record(&old_record, grant.fencing_token)
            .await?;
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: store,
            lease_provider,
            runtime_instance_id: "alias-race-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        runtime
            .register(
                durable_spec(identity.clone(), "domain"),
                IdentityLifecycleState::Active,
                Some(old_record),
                Some(grant),
            )
            .await;

        let preflight_identity = runtime
            .owned_identity_for_member_alias(old_alias)
            .await
            .ok_or("old alias did not pass ownership preflight")?;
        let new_record = runtime.reset_tracked(&identity).await?;
        assert_eq!(new_record.generation, ContinuityGeneration::new(1));

        let respawn_error = match runtime
            .respawn_member_alias_tracked(&preflight_identity, old_alias)
            .await
        {
            Ok(_) => return Err("old alias respawned the replacement generation".into()),
            Err(error) => error,
        };
        assert!(matches!(
            respawn_error,
            IdentityRuntimeError::StaleRuntimeAlias { ref requested, .. }
                if requested == old_alias
        ));
        let content = meerkat_core::ContentInput::Text("stale alias delivery".to_string());
        let send_error = match runtime
            .send_with_mode_and_interaction_member_alias_tracked(
                &preflight_identity,
                old_alias,
                &content,
                HandlingMode::Queue,
                None,
            )
            .await
        {
            Ok(_) => return Err("old alias delivered to the replacement generation".into()),
            Err(error) => error,
        };
        assert!(matches!(
            send_error,
            IdentityRuntimeError::StaleRuntimeAlias { ref requested, .. }
                if requested == old_alias
        ));
        let dispatch = DispatchInput {
            content,
            origin: super::super::types::DispatchOrigin::System,
            correlation_id: None,
            idempotency_key: None,
        };
        let dispatch_error = match runtime
            .dispatch_member_alias_tracked(&preflight_identity, old_alias, &dispatch)
            .await
        {
            Ok(_) => return Err("old alias dispatched to the replacement generation".into()),
            Err(error) => error,
        };
        assert!(matches!(
            dispatch_error,
            IdentityRuntimeError::StaleRuntimeAlias { ref requested, .. }
                if requested == old_alias
        ));
        let rebind_error = match runtime
            .rebind_session_after_live_respawn_member_alias_tracked(
                &preflight_identity,
                old_alias,
                SessionId::new(),
            )
            .await
        {
            Ok(_) => return Err("old alias rebound the replacement generation".into()),
            Err(error) => error,
        };
        assert!(matches!(
            rebind_error,
            IdentityRuntimeError::StaleRuntimeAlias { ref requested, .. }
                if requested == old_alias
        ));
        let retire_error = match runtime
            .retire_member_alias_tracked(&preflight_identity, old_alias)
            .await
        {
            Ok(_) => return Err("old alias retired the replacement generation".into()),
            Err(error) => error,
        };
        assert!(matches!(
            retire_error,
            IdentityRuntimeError::StaleRuntimeAlias { ref requested, .. }
                if requested == old_alias
        ));
        let reset_error = match runtime
            .reset_member_alias_tracked(&preflight_identity, old_alias)
            .await
        {
            Ok(_) => return Err("old alias reset the replacement generation".into()),
            Err(error) => error,
        };
        assert!(matches!(
            reset_error,
            IdentityRuntimeError::StaleRuntimeAlias { ref requested, .. }
                if requested == old_alias
        ));
        let delete_error = match runtime
            .delete_identity_member_alias_tracked(&preflight_identity, old_alias)
            .await
        {
            Ok(()) => return Err("old alias deleted the replacement generation".into()),
            Err(error) => error,
        };
        assert!(matches!(
            delete_error,
            IdentityRuntimeError::StaleRuntimeAlias { ref requested, .. }
                if requested == old_alias
        ));
        let status = runtime.status(&identity).await?;
        assert_eq!(status.state, IdentityLifecycleState::Active);
        assert_eq!(status.generation, Some(ContinuityGeneration::new(1)));
        assert_eq!(
            status.agent_runtime_id.as_ref(),
            Some(&new_record.agent_runtime_id)
        );
        Ok(())
    }

    #[tokio::test]
    async fn reset_reprofiles_session_from_identity_first_context_roster_provider()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:security")?;
        let roster = Arc::new(MutableRoster::new(vec![durable_spec(
            identity.clone(),
            "domain",
        )]));
        let bridge = Arc::new(RecordingBridge::default());
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "reset-reprofile-context-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));
        let context =
            IdentityFirstRuntimeContext::new(runtime.clone(), roster.clone(), None, None, None);

        context.refresh_desired_topology().await?;
        roster
            .set(vec![durable_spec(identity.clone(), "security")])
            .await;

        let record = runtime.reset(&identity).await?;

        assert_eq!(record.generation.get(), 1);
        assert_eq!(
            bridge.create_profiles().await,
            vec!["domain".to_string(), "security".to_string()]
        );
        let status = runtime.status(&identity).await?;
        assert_eq!(
            status.profile.map(|profile| profile.to_string()).as_deref(),
            Some("security")
        );
        Ok(())
    }

    #[tokio::test]
    async fn reset_records_exact_cleanup_debt_without_waiting_for_hung_old_retire()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:security")?;
        let roster = Arc::new(MutableRoster::new(vec![durable_spec(
            identity.clone(),
            "domain",
        )]));
        let bridge = Arc::new(RecordingBridge::default());
        let runtime = Arc::new(
            IdentityRuntime::new(IdentityRuntimeConfig {
                continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
                lease_provider: Arc::new(LocalLeaseProvider::new()),
                runtime_instance_id: "reset-skips-old-retire-test".to_string(),
                has_runtime_store: true,
                durability_policy: DurabilityPolicy::SyncWriteThrough,
                bridge: Some(bridge.clone()),
                default_timeout: Some(Duration::from_millis(50)),
            })
            .with_reset_roster_provider(roster.clone()),
        );

        super::super::orchestrator::restore_flow(
            &runtime,
            &roster
                .roster(&RosterContext {
                    mob_definition: None,
                    previous_identities: Vec::new(),
                })
                .await?,
            None,
            None,
        )
        .await?;

        // The old incarnation's retire is rigged to HANG. Nothing should ever
        // attempt it: the authoritative successor transition retires the
        // predecessor itself, so MobKit must not schedule that work again. If it
        // did, this reset would either block on the hung retire or leave debt
        // behind, and both are observable below.
        let old_runtime_id = AgentRuntimeId::parse("rt:domain:security:0")?;
        bridge.hang_retire_for(&old_runtime_id).await;
        // Spec deliberately NOT drifted. Reprofile capability is covered by its
        // own tests, which are red until the upstream successor-spec operation
        // lands; this one is about phantom cleanup debt.
        roster
            .set(vec![durable_spec(identity.clone(), "domain")])
            .await;

        let record = tokio::time::timeout(Duration::from_secs(1), runtime.reset(&identity))
            .await
            .map_err(|_| "reset blocked, which means it attempted the hung old-member retire")??;

        assert_eq!(record.generation.get(), 1);
        // Positive: the successor transition happened.
        assert!(
            !bridge.successor_generations().await.is_empty(),
            "reset must go through the authoritative successor transition"
        );
        // And it manufactured NO member-retire debt. Respawn already retired the
        // predecessor inside the same transition, so any runtime_id debt here is
        // work scheduled against a row another authority destroyed - debt that
        // can never discharge, on every reset, while reset reports success.
        let member_retire_debt: Vec<String> = runtime
            .pending_reset_bridge_cleanups
            .read()
            .await
            .values()
            .filter_map(|cleanup| cleanup.runtime_id.as_ref().map(ToString::to_string))
            .collect();
        assert!(
            member_retire_debt.is_empty(),
            "reset must not record old-member retire debt for work the successor transition \
             already committed: {member_retire_debt:?}"
        );
        assert!(
            !bridge
                .retired_runtime_ids()
                .await
                .contains(&old_runtime_id.to_string()),
            "the predecessor must not be retired through the bridge a second time"
        );
        // The successor keeps the predecessor's profile, which is what respawn
        // promises. The reprofile case is deliberately not exercised here.
        let status = runtime.status(&identity).await?;
        assert_eq!(
            status.profile.map(|profile| profile.to_string()).as_deref(),
            Some("domain")
        );
        Ok(())
    }

    #[tokio::test]
    async fn reset_cleanup_failure_stays_retryable_after_new_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:security")?;
        let roster = Arc::new(MutableRoster::new(vec![durable_spec(
            identity.clone(),
            "domain",
        )]));
        let bridge = Arc::new(RecordingBridge::default());
        let runtime = Arc::new(
            IdentityRuntime::new(IdentityRuntimeConfig {
                continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
                lease_provider: Arc::new(LocalLeaseProvider::new()),
                runtime_instance_id: "reset-unregister-failure-test".to_string(),
                has_runtime_store: true,
                durability_policy: DurabilityPolicy::SyncWriteThrough,
                bridge: Some(bridge.clone()),
                default_timeout: Some(Duration::from_millis(50)),
            })
            .with_reset_roster_provider(roster.clone()),
        );

        super::super::orchestrator::restore_flow(
            &runtime,
            &roster
                .roster(&RosterContext {
                    mob_definition: None,
                    previous_identities: Vec::new(),
                })
                .await?,
            None,
            None,
        )
        .await?;

        let old_status = runtime.status(&identity).await?;
        let Some(old_session_id) = old_status.session_id else {
            return Err("initial session id missing".into());
        };
        bridge.fail_unregister_for(&old_session_id).await;
        // Spec deliberately NOT drifted: this test is about the old SESSION
        // unregistration staying exact and retryable after the successor
        // generation commits. Session cleanup remains MobKit's concern - the
        // successor transition does not touch it - so it is unaffected by the
        // reprofile capability gap that keeps its sibling tests red.
        roster
            .set(vec![durable_spec(identity.clone(), "domain")])
            .await;

        let record = tokio::time::timeout(Duration::from_secs(1), runtime.reset(&identity))
            .await
            .map_err(|_| "reset timed out waiting for old session unregister cleanup")??;

        assert_eq!(record.generation.get(), 1);
        assert!(
            !bridge.successor_generations().await.is_empty(),
            "the successor generation must have committed before cleanup is judged"
        );
        runtime.join_reset_bridge_cleanup_tasks().await;
        assert_eq!(
            runtime.pending_reset_bridge_cleanups.read().await.len(),
            1,
            "failed unregister must retain exact cleanup debt"
        );
        bridge.allow_unregister_for(&old_session_id).await;
        assert_eq!(runtime.drain_pending_reset_bridge_cleanups().await?, 1);
        assert!(
            runtime
                .pending_reset_bridge_cleanups
                .read()
                .await
                .is_empty()
        );
        let status = runtime.status(&identity).await?;
        assert_eq!(
            status.profile.map(|profile| profile.to_string()).as_deref(),
            Some("domain")
        );
        Ok(())
    }

    #[tokio::test]
    async fn active_renewal_suspends_bridge_before_publishing_replacement_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        struct RotatingLeaseProvider;

        #[async_trait::async_trait]
        impl LeaseProvider for RotatingLeaseProvider {
            async fn acquire_leases(
                &self,
                _identities: &[AgentIdentity],
                _runtime_instance: &str,
            ) -> Result<BTreeMap<AgentIdentity, LeaseAcquireResult>, LeaseError> {
                Ok(BTreeMap::new())
            }

            async fn renew_leases(
                &self,
                grants: &[LeaseGrant],
            ) -> Result<BTreeMap<AgentIdentity, LeaseRenewResult>, LeaseError> {
                Ok(grants
                    .iter()
                    .map(|grant| {
                        let renewed = LeaseGrant {
                            identity: grant.identity.clone(),
                            fencing_token: FencingToken::new(grant.fencing_token.get() + 1),
                            ttl: Duration::from_mins(5),
                        };
                        (grant.identity.clone(), LeaseRenewResult::Renewed(renewed))
                    })
                    .collect())
            }

            async fn release_leases(&self, _grants: &[LeaseGrant]) -> Result<(), LeaseError> {
                Ok(())
            }
        }

        let identity = AgentIdentity::parse("domain:renewal-barrier")?;
        let runtime_instance = "active-renewal-barrier-test";
        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let lease = Arc::new(RotatingLeaseProvider);
        let bridge = Arc::new(RecordingBridge::default());
        let first = LeaseGrant {
            identity: identity.clone(),
            fencing_token: FencingToken::new(1),
            ttl: Duration::ZERO,
        };
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:domain:renewal-barrier:0")?,
            session_id: SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        store
            .upsert_continuity_record(&record, first.fencing_token)
            .await?;
        bridge
            .register_session_runtime_state(
                &record.session_id,
                &identity,
                record.generation,
                record.checkpoint_version,
                first.fencing_token,
            )
            .await?;
        let runtime = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: store,
            lease_provider: lease,
            runtime_instance_id: runtime_instance.to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        });
        runtime
            .register(
                durable_spec(identity.clone(), "domain"),
                IdentityLifecycleState::Active,
                Some(record.clone()),
                Some(first.clone()),
            )
            .await;

        let renewed_token = runtime.ensure_active_lease(&identity).await?;
        assert!(renewed_token > first.fencing_token);
        assert_eq!(
            bridge.authority_transitions().await,
            vec![
                format!("register:{}", first.fencing_token.get()),
                format!("suspend:{}", record.session_id),
                format!("suspend:{}", record.session_id),
                format!("register:{}", renewed_token.get()),
            ],
            "the old bridge authority must be quiesced before replacement publication"
        );
        assert_eq!(
            runtime
                .status(&identity)
                .await?
                .lease
                .map(|lease| lease.fencing_token),
            Some(renewed_token)
        );
        Ok(())
    }

    #[tokio::test]
    async fn active_grant_bridge_failure_parks_rotated_token_for_exact_shutdown_release()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:bridge-fence")?;
        let runtime_instance = "active-grant-bridge-failure-test";
        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let lease = Arc::new(LocalLeaseProvider::new());
        let bridge = Arc::new(RecordingBridge::default());
        let first = match lease
            .acquire_leases(std::slice::from_ref(&identity), runtime_instance)
            .await?
            .remove(&identity)
        {
            Some(LeaseAcquireResult::Acquired(grant)) => grant,
            other => return Err(format!("initial lease was not acquired: {other:?}").into()),
        };
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:domain:bridge-fence:0")?,
            session_id: SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        store
            .upsert_continuity_record(&record, first.fencing_token)
            .await?;
        let runtime = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: store,
            lease_provider: lease.clone(),
            runtime_instance_id: runtime_instance.to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        });
        runtime
            .register(
                durable_spec(identity.clone(), "domain"),
                IdentityLifecycleState::Active,
                Some(record.clone()),
                Some(first.clone()),
            )
            .await;

        let rotated = match lease
            .renew_leases(std::slice::from_ref(&first))
            .await?
            .remove(&identity)
        {
            Some(LeaseRenewResult::Renewed(grant)) => grant,
            other => return Err(format!("lease did not rotate: {other:?}").into()),
        };
        runtime
            .publish_active_grant(&identity, None, &rotated)
            .await?;
        {
            let entries = runtime.entries.read().await;
            let entry = entries
                .get(&identity)
                .ok_or("identity entry disappeared after active grant refresh")?;
            assert_eq!(entry.state, IdentityLifecycleState::Active);
            assert_eq!(
                entry.lease.as_ref().map(|lease| lease.fencing_token),
                Some(rotated.fencing_token)
            );
        }

        let failed_grant = match lease
            .renew_leases(std::slice::from_ref(&rotated))
            .await?
            .remove(&identity)
        {
            Some(LeaseRenewResult::Renewed(grant)) => grant,
            other => return Err(format!("second lease did not rotate: {other:?}").into()),
        };
        bridge.fail_register_for(&record.session_id).await;
        let error = match runtime
            .publish_active_grant(&identity, None, &failed_grant)
            .await
        {
            Ok(_) => return Err("bridge publication failure unexpectedly succeeded".into()),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("synthetic live-session rebind failure")
        );
        assert_eq!(
            bridge.registered_fencing_tokens().await,
            vec![rotated.fencing_token, failed_grant.fencing_token],
            "every active refresh must project the provider-committed token into the bridge"
        );
        {
            let entries = runtime.entries.read().await;
            let entry = entries
                .get(&identity)
                .ok_or("identity entry disappeared after bridge failure")?;
            assert_eq!(entry.state, IdentityLifecycleState::Broken);
            assert!(entry.lease.is_none());
            assert_eq!(
                entry
                    .pending_lease_release
                    .as_ref()
                    .map(|grant| grant.fencing_token),
                Some(failed_grant.fencing_token)
            );
        }

        assert_eq!(runtime.release_all_leases_for_shutdown().await?, 1);
        let failover = lease
            .acquire_leases(std::slice::from_ref(&identity), "bridge-failure-failover")
            .await?;
        assert!(matches!(
            failover.get(&identity),
            Some(LeaseAcquireResult::Acquired(_))
        ));
        Ok(())
    }
}

#[cfg(test)]
mod lease_renewal_backoff_tests {
    use super::*;

    /// Regression: a lease provider that errors persistently must not spin the
    /// renewal task at the TTL-derived floor (down to 10ms). The failure
    /// backoff grows from a 1s base and caps at the max poll interval.
    #[test]
    fn lease_renewal_failure_backoff_grows_and_caps() {
        let max = Duration::from_mins(1);
        assert_eq!(
            lease_renewal_failure_backoff(0, max),
            LEASE_RENEWAL_FAILURE_BACKOFF_BASE
        );
        assert_eq!(
            lease_renewal_failure_backoff(1, max),
            LEASE_RENEWAL_FAILURE_BACKOFF_BASE * 2
        );
        assert_eq!(lease_renewal_failure_backoff(6, max), max);
        // Saturates at the cap for arbitrarily many failures (no shift overflow).
        assert_eq!(lease_renewal_failure_backoff(99, max), max);
        assert!(lease_renewal_failure_backoff(2, max) > lease_renewal_failure_backoff(1, max));
    }
}

#[cfg(test)]
mod continuity_repair_supervisor_tests {
    use super::*;
    use crate::identity_first::bridge::{
        BridgeDelivery, BridgeError, ResumeSessionOutcome, SessionBridge,
    };
    use crate::identity_first::types::{AgentBuildDraft, SessionSnapshot};
    use crate::identity_first::{LocalContinuityStore, LocalLeaseProvider, MutableRosterProvider};

    /// A resume path that is deterministically wedged: every attempt fails
    /// with the SAME error bytes, and every attempt is counted. This is the
    /// shape whose blind re-execution the bounded-identical-retry park
    /// exists to stop (each real repair attempt re-runs destructive dispose
    /// steps against the same blocking precondition).
    struct IdenticallyWedgedResumeBridge {
        resume_attempts: std::sync::atomic::AtomicUsize,
    }

    impl IdenticallyWedgedResumeBridge {
        fn attempts(&self) -> usize {
            self.resume_attempts
                .load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl SessionBridge for IdenticallyWedgedResumeBridge {
        async fn create_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            _session_id: &SessionId,
        ) -> Result<SessionId, BridgeError> {
            Err(BridgeError::Mob(
                "create not expected: the identity resolves Ready".to_string(),
            ))
        }

        async fn resume_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            _session_id: &SessionId,
            _snapshot: &SessionSnapshot,
        ) -> Result<ResumeSessionOutcome, BridgeError> {
            self.resume_attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(BridgeError::Mob(
                "collision retire blocked: disposal precondition holds (wedged for park test)"
                    .to_string(),
            ))
        }

        async fn deliver_admitted(
            &self,
            _runtime_id: &AgentRuntimeId,
            _delivery: BridgeDelivery,
        ) -> Result<SessionId, BridgeError> {
            Err(BridgeError::Mob("deliver not used".to_string()))
        }

        async fn checkpoint_session(
            &self,
            _runtime_id: &AgentRuntimeId,
            _session_id: &SessionId,
        ) -> Result<SessionSnapshot, BridgeError> {
            Err(BridgeError::Mob("checkpoint not used".to_string()))
        }

        async fn retire_member(&self, _runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
            Ok(())
        }
    }

    fn park_test_spec(identity: AgentIdentity) -> DurableAgentSpec {
        DurableAgentSpec {
            identity,
            profile: meerkat_mob::ProfileName::from("domain"),
            addressability: crate::identity_first::AgentAddressability::Addressable,
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

    /// Task #48 (c) — bounded non-identical retries: a Broken identity whose
    /// repair fails byte-identically on three consecutive passes is parked
    /// TYPED (`continuity_unrecoverable`, reason naming the blocking
    /// failure), and the supervisor never re-executes the repair afterwards.
    #[tokio::test]
    async fn repair_loop_parks_typed_after_three_byte_identical_failures()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:wedged")?;
        let spec = park_test_spec(identity.clone());
        let session_id = SessionId::new();
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse(&format!("rt:{identity}:0"))?,
            session_id: session_id.clone(),
            generation: ContinuityGeneration::new(1),
            checkpoint_version: CheckpointVersion::new(1),
        };
        let continuity_store = Arc::new(LocalContinuityStore::in_memory()?);
        continuity_store
            .upsert_continuity_record(&record, FencingToken::new(1))
            .await?;
        let bridge = Arc::new(IdenticallyWedgedResumeBridge {
            resume_attempts: std::sync::atomic::AtomicUsize::new(0),
        });
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store,
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "identical-failure-park-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));
        runtime
            .register(
                spec.clone(),
                IdentityLifecycleState::Broken,
                Some(record),
                None,
            )
            .await;
        let context = Arc::new(IdentityFirstRuntimeContext::new(
            Arc::clone(&runtime),
            Arc::new(MutableRosterProvider::new(vec![spec])),
            None,
            None,
            None,
        ));
        let task = context.spawn_tracked_broken_identity_repair_task(ContinuityRepairPolicy {
            initial_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(10),
        });

        // The park must arrive after EXACTLY the bounded attempt count.
        let deadline = Instant::now() + Duration::from_secs(30);
        let park = loop {
            if let Some(park) = runtime.continuity_unrecoverable(&identity).await {
                break park;
            }
            if Instant::now() > deadline {
                task.cancel_and_join().await;
                return Err("repair loop never parked the identically-failing identity".into());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert!(
            park.reason.contains("byte-identical"),
            "park reason must name the bounded-identical-retry cause: {}",
            park.reason
        );
        assert!(
            park.reason
                .contains("disposal precondition holds (wedged for park test)"),
            "park reason must carry the blocking failure verbatim: {}",
            park.reason
        );
        let attempts_at_park = bridge.attempts();
        assert_eq!(
            attempts_at_park, 3,
            "the destructive repair must run exactly the bounded attempt count"
        );

        // Parked = no further destructive re-execution on the timer.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            bridge.attempts(),
            attempts_at_park,
            "a parked identity must not be re-repaired on the timer"
        );
        assert!(
            runtime
                .repairable_broken_identities()
                .await
                .iter()
                .all(|id| id != &identity),
            "a parked identity must leave the repairable set"
        );
        task.cancel_and_join().await;
        Ok(())
    }

    #[tokio::test]
    async fn repair_loop_exits_when_supervisor_sender_is_dropped()
    -> Result<(), Box<dyn std::error::Error>> {
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "dropped-repair-supervisor-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let context = Arc::new(IdentityFirstRuntimeContext::new(
            runtime,
            Arc::new(MutableRosterProvider::new(Vec::new())),
            None,
            None,
            None,
        ));
        let TrackedContinuityRepairTask { cancel, join } = context
            .spawn_tracked_broken_identity_repair_task(ContinuityRepairPolicy {
                initial_backoff: Duration::from_mins(1),
                max_backoff: Duration::from_mins(1),
            });

        // This is what happens if the owning runtime is dropped without an
        // explicit shutdown: JoinHandle detaches, while the sender disappears.
        drop(cancel);
        tokio::time::timeout(Duration::from_millis(100), join)
            .await
            .map_err(|_| "repair loop spun after its cancellation channel closed")??;
        Ok(())
    }
}

#[cfg(test)]
mod foreground_shutdown_tests {
    use super::*;
    use crate::identity_first::{LocalContinuityStore, LocalLeaseProvider};

    #[test]
    fn foreground_shutdown_value_is_retained_for_late_subscribers()
    -> Result<(), ContinuityStoreError> {
        let runtime = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "late-foreground-cancel-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        });

        assert_eq!(runtime.foreground_cancel.receiver_count(), 0);
        runtime.close_foreground_operations();
        let receiver = runtime.foreground_cancel.subscribe();
        assert!(
            *receiver.borrow(),
            "a task subscribed after close must still observe shutdown"
        );
        Ok(())
    }
}

#[cfg(test)]
mod bootstrap_failure_attribution_tests {
    use super::*;
    use crate::identity_first::{LocalContinuityStore, LocalLeaseProvider};

    fn make_runtime() -> Result<IdentityRuntime, ContinuityStoreError> {
        Ok(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "bootstrap-failure-attribution-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }))
    }

    /// Before embodiment became per identity, `restore_flow` failed the whole
    /// pass on one member's error and copied that cause onto every peer. The
    /// pass-failure mechanism remains for fleet-level roster, topology, and
    /// batch-store failures. This pins the three things that must hold there:
    /// terminality (or the wait barrier hangs), the pass cause in the
    /// pass-level slot, and no borrowed cause on a bystander.
    #[test]
    fn pass_failure_keeps_terminality_without_borrowing_one_members_cause()
    -> Result<(), Box<dyn std::error::Error>> {
        let runtime = make_runtime()?;
        let culprit = AgentIdentity::parse("agent:culprit")?;
        let bystander = AgentIdentity::parse("agent:bystander")?;
        let survivor = AgentIdentity::parse("agent:survivor")?;
        let culprit_cause = "bridge create_session: culprit exploded";

        let generation =
            runtime.begin_identity_bootstrap_pending(IdentityBootstrapMode::EagerMaterialize);
        assert!(
            runtime.modify_bootstrap_status(Some(generation), |snapshot| {
                snapshot.identities.insert(
                    culprit.clone(),
                    IdentityBootstrapEntry {
                        state: IdentityBootstrapState::Broken,
                        error: Some(culprit_cause.to_string()),
                    },
                );
                snapshot.identities.insert(
                    bystander.clone(),
                    IdentityBootstrapEntry {
                        state: IdentityBootstrapState::Warming,
                        error: None,
                    },
                );
                snapshot.identities.insert(
                    survivor.clone(),
                    IdentityBootstrapEntry {
                        state: IdentityBootstrapState::Active,
                        error: None,
                    },
                );
                snapshot.refresh_aggregates();
            }),
            "seeding must land on the generation under test"
        );

        // Positive control: prove the pre-failure snapshot is NOT already in
        // the state the post-failure assertions claim. Without this, a seeding
        // bug that produced a Broken, cause-less bystander would let every
        // assertion below pass vacuously.
        let before = runtime.identity_bootstrap_status();
        assert!(before.error.is_none());
        assert!(!before.materialization_terminal());
        let before_bystander = before
            .identities
            .get(&bystander)
            .ok_or_else(|| "seeded bystander entry".to_string())?;
        assert_eq!(before_bystander.state, IdentityBootstrapState::Warming);
        assert!(before_bystander.error.is_none());

        // `IdentityRuntimeError`'s `Display` prefixes `Internal` ("internal:
        // {msg}"), and `fail_identity_bootstrap` stamps `error.to_string()`,
        // not the inner message. Pin the rendered form so the test tracks the
        // mechanism instead of one variant's wording.
        let pass_error = IdentityRuntimeError::Internal(culprit_cause.to_string());
        let pass_detail = pass_error.to_string();
        runtime.fail_identity_bootstrap(generation, &pass_error);

        let after = runtime.identity_bootstrap_status();
        // The pass cause rides the slot the type documents for it.
        assert_eq!(after.error.as_deref(), Some(pass_detail.as_str()));
        // Terminality survives: `wait_identity_bootstrap_terminal` returns a
        // truthful failure snapshot instead of waiting out its timeout.
        assert!(after.complete);
        assert!(after.materialization_terminal());
        assert!(!after.ready);

        // An identity that already reached Active is not retro-broken.
        let survivor_entry = after
            .identities
            .get(&survivor)
            .ok_or_else(|| "survivor entry".to_string())?;
        assert_eq!(survivor_entry.state, IdentityBootstrapState::Active);
        assert!(survivor_entry.error.is_none());

        // The identity that actually failed keeps ITS own cause verbatim.
        let culprit_entry = after
            .identities
            .get(&culprit)
            .ok_or_else(|| "culprit entry".to_string())?;
        assert_eq!(culprit_entry.state, IdentityBootstrapState::Broken);
        assert_eq!(culprit_entry.error.as_deref(), Some(culprit_cause));

        // The bystander transitions Warming -> Broken for terminality, but
        // must not report the culprit's cause as its own, and must not be
        // left cause-less either (17 Broken with no reason on record was the
        // original operator complaint).
        let bystander_entry = after
            .identities
            .get(&bystander)
            .ok_or_else(|| "bystander entry".to_string())?;
        assert_eq!(bystander_entry.state, IdentityBootstrapState::Broken);
        let bystander_error = bystander_entry
            .error
            .as_deref()
            .ok_or_else(|| "a Broken entry must carry a reason".to_string())?;
        assert_ne!(
            bystander_error, culprit_cause,
            "the culprit's raw cause must never be reported as the bystander's own"
        );
        // The discriminating assertion: the rendered pass detail is EXACTLY
        // what the previous implementation stamped on every non-Active entry,
        // so this is the one that fails if the borrowed-cause behaviour comes
        // back. (`assert_ne!` against `culprit_cause` alone would pass under
        // the old code too, because `Display` prefixes it.)
        assert_ne!(
            bystander_error, pass_detail,
            "a bystander must never carry the raw pass detail as its own cause"
        );
        assert!(
            bystander_error.starts_with("identity bootstrap pass failed before this identity"),
            "an unattributed entry must be labelled pass-level: {bystander_error}"
        );
        Ok(())
    }

    /// The `entry.error.is_none()` guard above is only truthful because a
    /// pass RESETS per-entry causes when it opens. Two failure paths stamp
    /// entries that `begin_identity_bootstrap` has not yet republished -
    /// `bootstrap_roster`'s prepare failure and `refresh_desired_topology`'s
    /// roster-provider failure - so without the reset a pass would report the
    /// PREVIOUS pass's cause per identity beside its own `status.error`.
    #[test]
    fn a_new_pass_does_not_report_the_previous_passs_cause()
    -> Result<(), Box<dyn std::error::Error>> {
        let runtime = make_runtime()?;
        let identity = AgentIdentity::parse("agent:stale")?;
        let first_cause = "pass one: resume rejected";

        let first =
            runtime.begin_identity_bootstrap_pending(IdentityBootstrapMode::EagerMaterialize);
        assert!(
            runtime.modify_bootstrap_status(Some(first), |snapshot| {
                snapshot.identities.insert(
                    identity.clone(),
                    IdentityBootstrapEntry {
                        state: IdentityBootstrapState::Broken,
                        error: Some(first_cause.to_string()),
                    },
                );
                snapshot.refresh_aggregates();
            }),
            "seeding must land on the first generation"
        );

        // Positive control: the stale cause really IS on the entry before the
        // next pass opens, so the assertions below observe a transition rather
        // than a state that was never there.
        let stale = runtime.identity_bootstrap_status();
        let stale_entry = stale
            .identities
            .get(&identity)
            .ok_or_else(|| "seeded pass-one entry".to_string())?;
        assert_eq!(stale_entry.error.as_deref(), Some(first_cause));

        // Pass two fails before `begin_identity_bootstrap` republishes entries.
        let second =
            runtime.begin_identity_bootstrap_pending(IdentityBootstrapMode::EagerMaterialize);
        let cleared = runtime.identity_bootstrap_status();
        let cleared_entry = cleared
            .identities
            .get(&identity)
            .ok_or_else(|| "entry survives the pass boundary".to_string())?;
        assert!(
            cleared_entry.error.is_none(),
            "opening a pass must drop the previous pass's cause"
        );
        assert_eq!(
            cleared_entry.state,
            IdentityBootstrapState::Broken,
            "only causes are pass-scoped; states are not touched here"
        );

        let pass_error =
            IdentityRuntimeError::Internal("pass two: roster provider down".to_string());
        let pass_detail = pass_error.to_string();
        runtime.fail_identity_bootstrap(second, &pass_error);

        let after = runtime.identity_bootstrap_status();
        assert_eq!(after.error.as_deref(), Some(pass_detail.as_str()));
        let entry_error = after
            .identities
            .get(&identity)
            .and_then(|entry| entry.error.as_deref())
            .ok_or_else(|| "a Broken entry must carry a reason".to_string())?;
        assert!(
            entry_error.contains("pass two: roster provider down"),
            "an entry stamped by pass two must name pass two: {entry_error}"
        );
        assert!(
            !entry_error.contains("pass one"),
            "pass one's cause must not survive into pass two: {entry_error}"
        );
        Ok(())
    }
}
