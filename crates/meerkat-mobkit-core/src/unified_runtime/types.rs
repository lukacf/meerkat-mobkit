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
