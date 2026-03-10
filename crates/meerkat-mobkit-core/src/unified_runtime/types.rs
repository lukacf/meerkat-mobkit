use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::mob_handle_runtime::{MobReconcileReport, MobRuntimeError};
use crate::runtime::{
    NormalizationError, RuntimeRouteMutationError, RuntimeShutdownReport, ScheduleValidationError,
    SubscribeError,
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
}

impl Display for UnifiedRuntimeBuilderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRequiredField(UnifiedRuntimeBuilderField::MobSpec) => {
                write!(f, "missing required builder field: mob_spec")
            }
            Self::MissingRequiredField(UnifiedRuntimeBuilderField::ModuleConfig) => {
                write!(f, "missing required builder field: module_config")
            }
            Self::MissingRequiredField(UnifiedRuntimeBuilderField::Timeout) => {
                write!(f, "missing required builder field: timeout")
            }
            Self::Bootstrap(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for UnifiedRuntimeBuilderError {}

#[derive(Debug)]
pub enum UnifiedRuntimeError {
    Normalize(NormalizationError),
    Subscribe(SubscribeError),
    ScheduleValidation(ScheduleValidationError),
    RuntimeShuttingDown,
    ScheduleDispatchThreadPanicked,
}

impl Display for UnifiedRuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normalize(err) => write!(f, "failed to normalize unified event: {err:?}"),
            Self::Subscribe(err) => write!(f, "failed to subscribe to unified events: {err:?}"),
            Self::ScheduleValidation(err) => {
                write!(f, "failed to dispatch schedule tick: {err:?}")
            }
            Self::RuntimeShuttingDown => {
                write!(
                    f,
                    "failed to dispatch schedule tick: unified runtime is shutting down"
                )
            }
            Self::ScheduleDispatchThreadPanicked => {
                write!(
                    f,
                    "failed to dispatch schedule tick: dispatch thread panicked"
                )
            }
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

impl From<ScheduleValidationError> for UnifiedRuntimeError {
    fn from(value: ScheduleValidationError) -> Self {
        Self::ScheduleValidation(value)
    }
}

#[derive(Debug)]
pub struct UnifiedRuntimeShutdownReport {
    pub drain: ShutdownDrainReport,
    pub module_shutdown: RuntimeShutdownReport,
    pub mob_stop: Result<(), MobRuntimeError>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedRuntimeReconcileReport {
    pub mob: MobReconcileReport,
    pub edges: UnifiedRuntimeReconcileEdgesReport,
    pub routing: UnifiedRuntimeReconcileRoutingReport,
}

#[derive(Debug)]
pub enum UnifiedRuntimeReconcileError {
    Mob(MobRuntimeError),
    RouteMutation(RuntimeRouteMutationError),
}

impl Display for UnifiedRuntimeReconcileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(err) => write!(f, "failed to reconcile mob roster: {err}"),
            Self::RouteMutation(err) => {
                write!(f, "failed to reconcile routing wiring: {err:?}")
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
    HostLoopCrash {
        member_id: String,
        error: String,
    },
    RediscoverFailure {
        error: String,
    },
}

impl Display for ErrorEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailure { member_id, error, .. } => {
                write!(f, "spawn_failure: {member_id}: {error}")
            }
            Self::ReconcileIncomplete { failures, skipped } => {
                write!(f, "reconcile_incomplete: {failures} failures, {skipped} skipped")
            }
            Self::CheckpointFailure { session_id, error } => {
                write!(f, "checkpoint_failure: {session_id}: {error}")
            }
            Self::HostLoopCrash { member_id, error } => {
                write!(f, "host_loop_crash: {member_id}: {error}")
            }
            Self::RediscoverFailure { error } => {
                write!(f, "rediscover_failure: {error}")
            }
        }
    }
}
