//! Error types, hook definitions, and report structures for the unified runtime.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::mob_handle_runtime::MobRuntimeError;
use crate::runtime::{
    NormalizationError, RuntimeRouteMutationError, RuntimeShutdownReport, SubscribeError,
};

use super::edge_types::{DesiredPeerEdge, EdgeReconcileFailure};

/// Report from dynamic edge reconciliation.
///
/// Best-effort: partial success is reported clearly. Apps decide whether
/// to treat failures as fatal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UnifiedRuntimeReconcileEdgesReport {
    pub desired_edges: Vec<DesiredPeerEdge>,
    pub wired_edges: Vec<DesiredPeerEdge>,
    pub unwired_edges: Vec<DesiredPeerEdge>,
    pub retained_edges: Vec<DesiredPeerEdge>,
    pub preexisting_edges: Vec<DesiredPeerEdge>,
    pub skipped_missing_members: Vec<DesiredPeerEdge>,
    pub pruned_stale_managed_edges: Vec<DesiredPeerEdge>,
    #[serde(default)]
    pub failures: Vec<EdgeReconcileFailure>,
}

impl UnifiedRuntimeReconcileEdgesReport {
    /// True if all desired edges were successfully applied or retained.
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty() && self.skipped_missing_members.is_empty()
    }
}

#[derive(Debug)]
pub enum UnifiedRuntimeBootstrapError {
    Mob(MobRuntimeError),
    Module(crate::runtime::MobkitRuntimeError),
    ModuleStartupThreadPanicked,
    ModuleStartupRollbackFailed {
        startup_error: Box<UnifiedRuntimeBootstrapError>,
        rollback_error: MobRuntimeError,
    },
    PreSpawnHook(String),
    IdentityFirst(String),
    Topology(String),
}

impl Display for UnifiedRuntimeBootstrapError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(err) => write!(f, "failed to bootstrap mob runtime: {err}"),
            Self::Module(err) => write!(f, "failed to bootstrap module runtime: {err:?}"),
            Self::ModuleStartupThreadPanicked => {
                write!(
                    f,
                    "failed to bootstrap module runtime: startup thread panicked"
                )
            }
            Self::PreSpawnHook(err) => {
                write!(f, "pre-spawn hook failed: {err}")
            }
            Self::IdentityFirst(err) => {
                write!(f, "identity-first bootstrap failed: {err}")
            }
            Self::Topology(err) => write!(f, "topology-control bootstrap failed: {err}"),
            Self::ModuleStartupRollbackFailed {
                startup_error,
                rollback_error,
            } => {
                write!(
                    f,
                    "failed to bootstrap unified runtime: startup error ({startup_error}) and rollback failed: {rollback_error}"
                )
            }
        }
    }
}

impl std::error::Error for UnifiedRuntimeBootstrapError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifiedRuntimeBuilderField {
    MobSpec,
    ModuleConfig,
    Timeout,
}

#[derive(Debug)]
pub enum UnifiedRuntimeBuilderError {
    MissingRequiredField(UnifiedRuntimeBuilderField),
    Bootstrap(UnifiedRuntimeBootstrapError),
    /// Failed to read a definition TOML file or create a state directory.
    Io(String),
    /// Failed to parse a mob definition TOML.
    DefinitionLoad(String),
    /// Conflicting builder configuration (e.g., persistent_state + scratch_dir).
    ConflictingConfiguration(String),
    /// Storage layout refusal (file-name twins in the state directory).
    StorageLayout(crate::storage_layout::StorageLayoutError),
    /// A storage provider failed to open the realm's store set, or the
    /// fail-closed durability rule refused it (M4).
    StorageProvider(crate::storage_provider::MobKitStorageProviderError),
}

impl From<crate::storage_layout::StorageLayoutError> for UnifiedRuntimeBuilderError {
    fn from(error: crate::storage_layout::StorageLayoutError) -> Self {
        Self::StorageLayout(error)
    }
}

impl From<crate::storage_provider::MobKitStorageProviderError> for UnifiedRuntimeBuilderError {
    fn from(error: crate::storage_provider::MobKitStorageProviderError) -> Self {
        Self::StorageProvider(error)
    }
}

impl Display for UnifiedRuntimeBuilderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRequiredField(UnifiedRuntimeBuilderField::MobSpec) => {
                write!(f, "missing required builder field: mob_spec or definition")
            }
            Self::MissingRequiredField(UnifiedRuntimeBuilderField::ModuleConfig) => {
                write!(f, "missing required builder field: module_config")
            }
            Self::MissingRequiredField(UnifiedRuntimeBuilderField::Timeout) => {
                write!(f, "missing required builder field: timeout")
            }
            Self::Bootstrap(err) => write!(f, "{err}"),
            Self::Io(msg) => write!(f, "{msg}"),
            Self::DefinitionLoad(msg) => write!(f, "{msg}"),
            Self::ConflictingConfiguration(msg) => write!(f, "conflicting configuration: {msg}"),
            Self::StorageLayout(err) => write!(f, "{err}"),
            Self::StorageProvider(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for UnifiedRuntimeBuilderError {}

#[derive(Debug)]
pub enum UnifiedRuntimeError {
    Normalize(NormalizationError),
    Subscribe(SubscribeError),
    RuntimeShuttingDown,
}

impl Display for UnifiedRuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normalize(err) => write!(f, "failed to normalize unified event: {err:?}"),
            Self::Subscribe(err) => write!(f, "failed to subscribe to unified events: {err:?}"),
            Self::RuntimeShuttingDown => write!(f, "unified runtime is shutting down"),
        }
    }
}

impl std::error::Error for UnifiedRuntimeError {}

impl From<NormalizationError> for UnifiedRuntimeError {
    fn from(value: NormalizationError) -> Self {
        Self::Normalize(value)
    }
}

impl From<SubscribeError> for UnifiedRuntimeError {
    fn from(value: SubscribeError) -> Self {
        Self::Subscribe(value)
    }
}

/// Exact disposition of identity-first lease authority during shutdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityAuthorityReleaseOutcome {
    /// The runtime did not have identity-first authority to release.
    NotConfigured,
    /// Every retained grant was released from the configured provider.
    Released { grant_count: usize },
    /// The provider rejected or failed the exact release operation.
    Failed { error: String },
    /// A reset-superseded session/member cleanup obligation remained after
    /// physical shutdown retries, so provider grants were retained.
    SkippedResetCleanupFailed { error: String },
    /// Physical members did not quiesce, so their grants were deliberately retained.
    SkippedMobStopFailed,
}

#[derive(Debug)]
pub struct UnifiedRuntimeShutdownReport {
    pub drain: ShutdownDrainReport,
    pub module_shutdown: RuntimeShutdownReport,
    pub mob_stop: Result<(), MobRuntimeError>,
    pub identity_authority_release: IdentityAuthorityReleaseOutcome,
}

impl UnifiedRuntimeShutdownReport {
    /// True only when every shutdown phase that owns external authority or
    /// child-process state completed successfully.
    pub fn cleanup_completed(&self) -> bool {
        !self.drain.timed_out
            && self.mob_stop.is_ok()
            && matches!(
                &self.identity_authority_release,
                IdentityAuthorityReleaseOutcome::NotConfigured
                    | IdentityAuthorityReleaseOutcome::Released { .. }
            )
            && self.module_shutdown.orphan_processes == 0
    }
}

#[derive(Debug)]
pub struct UnifiedRuntimeRunReport {
    pub serve_result: std::io::Result<()>,
    pub shutdown: UnifiedRuntimeShutdownReport,
}

/// Report from a rediscover operation (reset + re-run discovery + reconcile edges).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RediscoverReport {
    /// Number of members spawned by discovery.
    pub spawned: Vec<String>,
    /// Edge reconciliation report (if EdgeDiscovery is configured).
    pub edges: UnifiedRuntimeReconcileEdgesReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedRuntimeReconcileRoutingReport {
    pub router_module_loaded: bool,
    pub active_members: Vec<String>,
    pub added_route_keys: Vec<String>,
    pub removed_route_keys: Vec<String>,
}

/// Per-identity reconcile failure — re-export of the canonical
/// meerkat-contracts wire shape so SDK consumers see the same field
/// names whether they go through `mob/reconcile` or `mobkit/reconcile`.
pub use meerkat_contracts::MobReconcileFailureWire as MobReconcileFailure;

/// Roster half of a reconcile pass — re-export of meerkat-contracts'
/// canonical wire shape. `spawned: Vec<MobSpawnReceiptWire>` carries the
/// server-resolved `WireMemberRef` per receipt, replacing the
/// identity-string list mobkit projected before 0.6.
pub use meerkat_contracts::MobReconcileReportWire as MobReconcileReport;

/// Project meerkat's native `ReconcileReport` into the canonical wire shape.
///
/// Mirrors the `mob/reconcile` RPC handler's projection in
/// `meerkat-rpc/src/handlers/mob.rs`, with one mobkit-specific step: the
/// report's roster member ids are comms-safe encodings (meerkat 0.7
/// `MemberCommsName`), and this is a projection boundary, so every id is
/// decoded back to the public alias consoles/SDKs address members by.
pub fn meerkat_reconcile_report_to_wire(
    mob_id: &str,
    report: meerkat_mob::runtime::reconcile::ReconcileReport,
) -> MobReconcileReport {
    use meerkat_contracts::{MobSpawnReceiptWire, WireMemberRef};
    let alias_of =
        |id: &str| -> String { crate::member_comms_id::runtime_alias_str(id).into_owned() };
    MobReconcileReport {
        desired: report
            .desired
            .into_iter()
            .map(|id| alias_of(id.as_str()))
            .collect(),
        retained: report
            .retained
            .into_iter()
            .map(|id| alias_of(id.as_str()))
            .collect(),
        spawned: report
            .spawned
            .into_iter()
            .map(|receipt| {
                let identity_str = alias_of(receipt.agent_identity.as_str());
                MobSpawnReceiptWire {
                    member_ref: WireMemberRef::encode(mob_id, &identity_str),
                    agent_identity: identity_str,
                }
            })
            .collect(),
        retired: report
            .retired
            .into_iter()
            .map(|id| alias_of(id.as_str()))
            .collect(),
        failures: report
            .failures
            .into_iter()
            .map(|failure| MobReconcileFailure {
                agent_identity: alias_of(failure.agent_identity.as_str()),
                stage: match failure.stage {
                    meerkat_mob::runtime::reconcile::ReconcileStage::Spawn => {
                        meerkat_contracts::WireMobReconcileStage::Spawn
                    }
                    meerkat_mob::runtime::reconcile::ReconcileStage::Retire => {
                        meerkat_contracts::WireMobReconcileStage::Retire
                    }
                },
                error: meerkat_contracts::WireMobError {
                    code: meerkat_mob::mob_error_wire_code(&failure.error),
                    message: failure.error.to_string(),
                },
            })
            .collect(),
    }
}

// Eq is dropped because the canonical wire `MobReconcileReportWire` does
// not implement `Eq` (its nested types are PartialEq only).
#[derive(Debug, Clone, PartialEq)]
pub struct UnifiedRuntimeReconcileReport {
    pub mob: MobReconcileReport,
    pub edges: UnifiedRuntimeReconcileEdgesReport,
    pub routing: UnifiedRuntimeReconcileRoutingReport,
}

#[derive(Debug)]
pub enum UnifiedRuntimeReconcileError {
    Mob(MobRuntimeError),
    RouteMutation(RuntimeRouteMutationError),
    /// Meerkat 0.6's `MobHandle::reconcile` collects per-identity failures
    /// into the returned report rather than returning `Err` on first failure.
    /// `UnifiedRuntime::reconcile` re-lifts that into an error variant so
    /// Rust callers using `?` still see failure propagation, while keeping
    /// the full report available for inspection.
    PartialFailure(Box<UnifiedRuntimeReconcileReport>),
}

impl Display for UnifiedRuntimeReconcileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(err) => write!(f, "failed to reconcile mob roster: {err}"),
            Self::RouteMutation(err) => {
                write!(f, "failed to reconcile routing wiring: {err:?}")
            }
            Self::PartialFailure(report) => {
                write!(
                    f,
                    "reconcile completed with {} per-identity failure(s): {:?}",
                    report.mob.failures.len(),
                    report.mob.failures
                )
            }
        }
    }
}

impl std::error::Error for UnifiedRuntimeReconcileError {}

#[derive(Debug)]
pub struct ShutdownDrainReport {
    pub drained_count: usize,
    pub timed_out: bool,
    pub drain_duration_ms: u64,
}

/// Operational error event for alerting.
///
/// Fired via the `on_error` hook when runtime operations fail. Apps
/// match on variants to decide alerting (Slack, PagerDuty, log, etc.).
///
/// Marked `#[non_exhaustive]` — new variants can be added without
/// breaking downstream match arms (use a `_` wildcard).
///
/// **Wired fire points:**
/// - `SpawnFailure` — `mob_ops.rs` spawn error path
/// - `ReconcileIncomplete` — `edge_reconcile.rs` after `reconcile_edges`
/// - `RediscoverFailure` — `lifecycle.rs` rediscover error path
/// - `HostLoopCrash` — `lifecycle.rs` detects `run_failed` agent events during drain
/// - `CheckpointFailure` — via `run_periodic_gc_with_error_callback` in session store
/// - `CompactionPersistenceRejected` — `lifecycle.rs` drain fires the alert the
///   agent-event forwarder extracted from a member `CompactionFailed` event
/// - `IdentityMaterializationFailure` — identity-first peer/fleet hydration skipped a member
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum ErrorEvent {
    SpawnFailure {
        member_id: String,
        profile: String,
        error: String,
    },
    ReconcileIncomplete {
        failures: usize,
        skipped: usize,
    },
    CheckpointFailure {
        session_id: String,
        error: String,
    },
    CompactionPersistenceRejected {
        identity: String,
        session_id: String,
        error: String,
    },
    HostLoopCrash {
        member_id: String,
        error: String,
    },
    RediscoverFailure {
        error: String,
    },
    EventLogFlushFailure {
        error: String,
    },
    IdentityMaterializationFailure {
        identity: String,
        initiator: Option<String>,
        operation: String,
        error: String,
    },
}

impl Display for ErrorEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailure {
                member_id, error, ..
            } => {
                write!(f, "spawn_failure: {member_id}: {error}")
            }
            Self::ReconcileIncomplete { failures, skipped } => {
                write!(
                    f,
                    "reconcile_incomplete: {failures} failures, {skipped} skipped"
                )
            }
            Self::CheckpointFailure { session_id, error } => {
                write!(f, "checkpoint_failure: {session_id}: {error}")
            }
            Self::CompactionPersistenceRejected {
                identity,
                session_id,
                error,
            } => {
                write!(
                    f,
                    "compaction_persistence_rejected: {identity} ({session_id}): {error}"
                )
            }
            Self::HostLoopCrash { member_id, error } => {
                write!(f, "host_loop_crash: {member_id}: {error}")
            }
            Self::RediscoverFailure { error } => {
                write!(f, "rediscover_failure: {error}")
            }
            Self::EventLogFlushFailure { error } => {
                write!(f, "event_log_flush_failure: {error}")
            }
            Self::IdentityMaterializationFailure {
                identity,
                initiator,
                operation,
                error,
            } => {
                if let Some(initiator) = initiator {
                    write!(
                        f,
                        "identity_materialization_failure: {identity} for {initiator} during {operation}: {error}"
                    )
                } else {
                    write!(
                        f,
                        "identity_materialization_failure: {identity} during {operation}: {error}"
                    )
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completed_shutdown_report() -> UnifiedRuntimeShutdownReport {
        UnifiedRuntimeShutdownReport {
            drain: ShutdownDrainReport {
                drained_count: 1,
                timed_out: false,
                drain_duration_ms: 2,
            },
            module_shutdown: RuntimeShutdownReport {
                terminated_modules: vec!["router".to_string()],
                orphan_processes: 0,
            },
            mob_stop: Ok(()),
            identity_authority_release: IdentityAuthorityReleaseOutcome::NotConfigured,
        }
    }

    #[test]
    fn shutdown_cleanup_attestation_requires_every_authority_boundary() {
        let mut report = completed_shutdown_report();
        assert!(report.cleanup_completed());

        report.identity_authority_release =
            IdentityAuthorityReleaseOutcome::Released { grant_count: 1 };
        assert!(report.cleanup_completed());

        let mut report = completed_shutdown_report();
        report.drain.timed_out = true;
        assert!(!report.cleanup_completed());

        let mut report = completed_shutdown_report();
        report.mob_stop = Err(MobRuntimeError::InvalidConfig(
            "mob stop failed".to_string(),
        ));
        assert!(!report.cleanup_completed());

        let mut report = completed_shutdown_report();
        report.identity_authority_release = IdentityAuthorityReleaseOutcome::Failed {
            error: "provider release failed".to_string(),
        };
        assert!(!report.cleanup_completed());

        let mut report = completed_shutdown_report();
        report.identity_authority_release = IdentityAuthorityReleaseOutcome::SkippedMobStopFailed;
        assert!(!report.cleanup_completed());

        let mut report = completed_shutdown_report();
        report.identity_authority_release =
            IdentityAuthorityReleaseOutcome::SkippedResetCleanupFailed {
                error: "superseded member retained".to_string(),
            };
        assert!(!report.cleanup_completed());

        let mut report = completed_shutdown_report();
        report.module_shutdown.orphan_processes = 1;
        assert!(!report.cleanup_completed());
    }
}
