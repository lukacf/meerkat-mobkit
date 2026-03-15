//! Runtime subsystem types — routing, delivery, gating, memory, scheduling, and session persistence.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Offset, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::{
    JwtClaimsValidationConfig, build_jwt_verification_key, inspect_jwt_header, parse_jwks_json,
    parse_oidc_discovery_json, select_jwk_for_token, validate_jwt_with_verification_key,
};
use crate::baseline::{
    BaselineVerificationError, BaselineVerificationReport, verify_meerkat_baseline_symbols,
};
use crate::decisions::{
    AuthPolicy, AuthProvider, BigQueryNaming, ConsoleAccessRequest, ConsolePolicy,
    DecisionPolicyError, ReleaseMetadata, RuntimeOpsPolicy, enforce_console_route_access,
    load_trusted_mobkit_modules_from_toml, parse_release_metadata_json, validate_bigquery_naming,
    validate_release_metadata, validate_runtime_ops_policy,
};
use crate::process::{ProcessBoundaryError, run_process_json_line};
use crate::protocol::parse_unified_event_line;
use crate::rpc::{RpcCapabilities, RpcCapabilitiesError, parse_rpc_capabilities};
use crate::types::{
    EventEnvelope, MobKitConfig, ModuleConfig, ModuleEvent, PreSpawnData, RestartPolicy,
    UnifiedEvent,
};

mod bootstrap;
mod console_ingress;
mod delivery;
mod event_transport;
mod gating;
mod memory;
mod module_boundary;
mod routing;
mod rpc;
mod scheduling;
mod session_store;
mod supervisor;

pub use bootstrap::{start_mobkit_runtime, start_mobkit_runtime_with_options};
pub use console_ingress::{
    ConsoleAgentLiveSnapshot, ConsoleLiveSnapshot, ConsoleRestJsonRequest, ConsoleRestJsonResponse,
    extract_bearer_token_from_header, handle_console_rest_json_route,
    handle_console_rest_json_route_with_snapshot,
};
pub use event_transport::normalize_event_line;
pub use routing::WILDCARD_ROUTE;
pub use routing::route_module_call;
pub use rpc::{
    route_module_call_rpc_json, route_module_call_rpc_subprocess,
    run_rpc_capabilities_boundary_once,
};
pub use scheduling::evaluate_schedules_at_tick;
pub use session_store::{
    BigQueryGcConfig, BigQuerySessionStoreAdapter, BigQuerySessionStoreError, GcErrorCallback,
    JsonFileSessionStore, JsonFileSessionStoreError, JsonStoreLockRecord, SessionPersistenceRow,
    SessionStoreContract, SessionStoreKind, materialize_latest_session_rows,
    materialize_live_session_rows, run_periodic_gc, run_periodic_gc_with_error_callback,
    session_store_contracts,
};
pub use supervisor::{run_discovered_module_once, run_module_boundary_once};

pub(crate) use scheduling::validate_schedules;

use event_transport::{insert_event_sorted, merge_unified_events};
use supervisor::supervise_module_start;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizationError {
    InvalidJson,
    InvalidSchema,
    MissingField(&'static str),
    InvalidFieldType(&'static str),
    SourceMismatch { expected: &'static str, got: String },
}

impl std::fmt::Display for NormalizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => write!(f, "invalid JSON"),
            Self::InvalidSchema => write!(f, "invalid schema"),
            Self::MissingField(field) => write!(f, "missing field: {field}"),
            Self::InvalidFieldType(field) => write!(f, "invalid field type: {field}"),
            Self::SourceMismatch { expected, got } => {
                write!(f, "source mismatch: expected {expected}, got {got}")
            }
        }
    }
}

impl std::error::Error for NormalizationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeBoundaryError {
    Process(ProcessBoundaryError),
    Normalize(NormalizationError),
    Mcp(McpBoundaryError),
}

impl std::fmt::Display for RuntimeBoundaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Process(err) => write!(f, "process boundary: {err}"),
            Self::Normalize(err) => write!(f, "normalization: {err}"),
            Self::Mcp(err) => write!(f, "MCP boundary: {err}"),
        }
    }
}

impl std::error::Error for RuntimeBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Process(err) => Some(err),
            Self::Normalize(err) => Some(err),
            Self::Mcp(err) => Some(err),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpBoundaryError {
    RuntimeUnavailable(String),
    McpRequired {
        module_id: String,
        flow: String,
    },
    Timeout {
        module_id: String,
        operation: String,
        timeout_ms: u64,
    },
    ConnectionFailed {
        module_id: String,
        reason: String,
    },
    ToolListFailed {
        module_id: String,
        reason: String,
    },
    ToolNotFound {
        module_id: String,
        tool: String,
        available_tools: Vec<String>,
    },
    ToolCallFailed {
        module_id: String,
        tool: String,
        reason: String,
    },
    CloseFailed {
        module_id: String,
        reason: String,
    },
    OperationFailedWithCloseFailure {
        primary: Box<McpBoundaryError>,
        close: Box<McpBoundaryError>,
    },
    InvalidToolPayload {
        module_id: String,
        tool: String,
        reason: String,
    },
    InvalidJsonResponse {
        module_id: String,
        tool: String,
        response: String,
    },
}

impl std::fmt::Display for McpBoundaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RuntimeUnavailable(msg) => write!(f, "runtime unavailable: {msg}"),
            Self::McpRequired { module_id, flow } => {
                write!(f, "MCP required for module {module_id} flow {flow}")
            }
            Self::Timeout {
                module_id,
                operation,
                timeout_ms,
            } => {
                write!(
                    f,
                    "timeout for module {module_id} operation {operation} after {timeout_ms}ms"
                )
            }
            Self::ConnectionFailed { module_id, reason } => {
                write!(f, "connection failed for module {module_id}: {reason}")
            }
            Self::ToolListFailed { module_id, reason } => {
                write!(f, "tool list failed for module {module_id}: {reason}")
            }
            Self::ToolNotFound {
                module_id,
                tool,
                available_tools,
            } => {
                write!(
                    f,
                    "tool {tool} not found for module {module_id} (available: {})",
                    available_tools.join(", ")
                )
            }
            Self::ToolCallFailed {
                module_id,
                tool,
                reason,
            } => {
                write!(
                    f,
                    "tool call {tool} failed for module {module_id}: {reason}"
                )
            }
            Self::CloseFailed { module_id, reason } => {
                write!(f, "close failed for module {module_id}: {reason}")
            }
            Self::OperationFailedWithCloseFailure { primary, close } => {
                write!(f, "operation failed: {primary}; close also failed: {close}")
            }
            Self::InvalidToolPayload {
                module_id,
                tool,
                reason,
            } => {
                write!(
                    f,
                    "invalid tool payload for module {module_id} tool {tool}: {reason}"
                )
            }
            Self::InvalidJsonResponse {
                module_id,
                tool,
                response,
            } => {
                write!(
                    f,
                    "invalid JSON response for module {module_id} tool {tool}: {response}"
                )
            }
        }
    }
}

impl std::error::Error for McpBoundaryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigResolutionError {
    ModuleNotConfigured(String),
    ModuleNotDiscovered(String),
}

impl std::fmt::Display for ConfigResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModuleNotConfigured(id) => write!(f, "module not configured: {id}"),
            Self::ModuleNotDiscovered(id) => write!(f, "module not discovered: {id}"),
        }
    }
}

impl std::error::Error for ConfigResolutionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeFromConfigError {
    Config(ConfigResolutionError),
    Runtime(RuntimeBoundaryError),
}

impl std::fmt::Display for RuntimeFromConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(err) => write!(f, "config resolution: {err}"),
            Self::Runtime(err) => write!(f, "runtime boundary: {err}"),
        }
    }
}

impl std::error::Error for RuntimeFromConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(err) => Some(err),
            Self::Runtime(err) => Some(err),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcRuntimeError {
    Process(ProcessBoundaryError),
    Capabilities(RpcCapabilitiesError),
}

impl std::fmt::Display for RpcRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Process(err) => write!(f, "process boundary: {err}"),
            Self::Capabilities(err) => write!(f, "capabilities: {err}"),
        }
    }
}

impl std::error::Error for RpcRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Process(err) => Some(err),
            Self::Capabilities(err) => Some(err),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineRuntimeError {
    Process(ProcessBoundaryError),
    InvalidRepoPathJson,
    MissingRepoRoot,
    InvalidRepoRoot,
    Baseline(BaselineVerificationError),
}

impl std::fmt::Display for BaselineRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Process(err) => write!(f, "process boundary: {err}"),
            Self::InvalidRepoPathJson => write!(f, "invalid repo path JSON"),
            Self::MissingRepoRoot => write!(f, "missing repo root"),
            Self::InvalidRepoRoot => write!(f, "invalid repo root"),
            Self::Baseline(err) => write!(f, "baseline verification: {err}"),
        }
    }
}

impl std::error::Error for BaselineRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Process(err) => Some(err),
            Self::Baseline(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MobkitRuntimeError {
    Config(ConfigResolutionError),
    MemoryBackend(ElephantMemoryStoreError),
}

impl std::fmt::Display for MobkitRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(err) => write!(f, "config resolution: {err}"),
            Self::MemoryBackend(err) => write!(f, "memory backend: {err}"),
        }
    }
}

impl std::error::Error for MobkitRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(err) => Some(err),
            Self::MemoryBackend(err) => Some(err),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionRuntimeError {
    Policy(DecisionPolicyError),
}

impl std::fmt::Display for DecisionRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Policy(err) => write!(f, "decision policy: {err}"),
        }
    }
}

impl std::error::Error for DecisionRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(err) => Some(err),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDecisionInputs {
    pub bigquery: BigQueryNaming,
    pub trusted_mobkit_toml: String,
    pub auth: AuthPolicy,
    pub trusted_oidc: TrustedOidcRuntimeConfig,
    pub console: ConsolePolicy,
    pub ops: RuntimeOpsPolicy,
    pub release_metadata_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDecisionState {
    pub bigquery: BigQueryNaming,
    pub modules: Vec<ModuleConfig>,
    pub auth: AuthPolicy,
    pub trusted_oidc: TrustedOidcRuntimeConfig,
    pub console: ConsolePolicy,
    pub ops: RuntimeOpsPolicy,
    pub release_metadata: ReleaseMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedOidcRuntimeConfig {
    pub discovery_json: String,
    pub jwks_json: String,
    pub audience: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElephantMemoryStoreError {
    InvalidConfig(String),
    Io(String),
    Serialize(String),
    InvalidStoreData(String),
    ExternalCallFailed(String),
}

impl std::fmt::Display for ElephantMemoryStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(msg) => write!(f, "invalid config: {msg}"),
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::Serialize(msg) => write!(f, "serialization error: {msg}"),
            Self::InvalidStoreData(msg) => write!(f, "invalid store data: {msg}"),
            Self::ExternalCallFailed(msg) => write!(f, "external call failed: {msg}"),
        }
    }
}

impl std::error::Error for ElephantMemoryStoreError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElephantMemoryBackendConfig {
    pub endpoint: String,
    pub state_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryBackendConfig {
    Elephant(ElephantMemoryBackendConfig),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ElephantMemoryStoreAdapter {
    endpoint: String,
    state_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeOptions {
    pub on_failure_retry_budget: u32,
    pub always_restart_budget: u32,
    #[serde(default)]
    pub supervisor_restart_backoff_ms: u64,
    #[serde(default)]
    pub supervisor_test_force_terminate_failure: bool,
    #[serde(default)]
    pub memory_backend: Option<MemoryBackendConfig>,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            on_failure_retry_budget: 1,
            always_restart_budget: 1,
            supervisor_restart_backoff_ms: 0,
            supervisor_test_force_terminate_failure: false,
            memory_backend: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleStage {
    MobStarted,
    ModulesStarted,
    MergedStreamStarted,
    ShutdownRequested,
    ShutdownComplete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleEvent {
    pub seq: u64,
    pub stage: LifecycleStage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleHealthState {
    Starting,
    Healthy,
    Failed,
    Restarting,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleHealthTransition {
    pub module_id: String,
    pub from: Option<ModuleHealthState>,
    pub to: ModuleHealthState,
    pub attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorReport {
    pub transitions: Vec<ModuleHealthTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeShutdownReport {
    pub terminated_modules: Vec<String>,
    pub orphan_processes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleDefinition {
    pub schedule_id: String,
    pub interval: String,
    pub timezone: String,
    pub enabled: bool,
    #[serde(default)]
    pub jitter_ms: u64,
    #[serde(default)]
    pub catch_up: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleTrigger {
    pub schedule_id: String,
    pub interval: String,
    pub timezone: String,
    pub due_tick_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleEvaluation {
    pub tick_ms: u64,
    pub due_triggers: Vec<ScheduleTrigger>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulingSupervisorSignal {
    pub module_id: String,
    pub latest_state: ModuleHealthState,
    pub latest_attempt: u32,
    pub restart_observed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleDispatch {
    pub claim_key: String,
    pub schedule_id: String,
    pub interval: String,
    pub timezone: String,
    pub due_tick_ms: u64,
    pub tick_ms: u64,
    pub event_id: String,
    pub supervisor_signal: Option<SchedulingSupervisorSignal>,
    #[serde(default)]
    pub runtime_injection: Option<ScheduleRuntimeInjection>,
    #[serde(default)]
    pub runtime_injection_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRuntimeInjection {
    pub member_id: String,
    pub message: String,
    pub injection_event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleDispatchReport {
    pub tick_ms: u64,
    pub due_count: usize,
    pub dispatched: Vec<ScheduleDispatch>,
    pub skipped_claims: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingResolveRequest {
    pub recipient: String,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub retry_max: Option<u32>,
    #[serde(default)]
    pub backoff_ms: Option<u64>,
    #[serde(default)]
    pub rate_limit_per_minute: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRoute {
    pub route_key: String,
    pub recipient: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub sink: String,
    pub target_module: String,
    #[serde(default)]
    pub retry_max: Option<u32>,
    #[serde(default)]
    pub backoff_ms: Option<u64>,
    #[serde(default)]
    pub rate_limit_per_minute: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingResolution {
    pub route_id: String,
    pub recipient: String,
    pub channel: String,
    pub sink: String,
    pub target_module: String,
    pub retry_max: u32,
    pub backoff_ms: u64,
    pub rate_limit_per_minute: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliverySendRequest {
    pub resolution: RoutingResolution,
    pub payload: Value,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryAttempt {
    pub attempt: u32,
    pub status: String,
    pub backoff_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryRecord {
    pub delivery_id: String,
    pub route_id: String,
    pub recipient: String,
    pub sink: String,
    pub target_module: String,
    pub payload: Value,
    pub status: String,
    pub attempts: Vec<DeliveryAttempt>,
    pub first_attempt_ms: u64,
    pub final_attempt_ms: u64,
    pub idempotency_key: Option<String>,
    pub sink_adapter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryHistoryRequest {
    #[serde(default)]
    pub recipient: Option<String>,
    #[serde(default)]
    pub sink: Option<String>,
    #[serde(default = "default_delivery_history_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryHistoryResponse {
    pub deliveries: Vec<DeliveryRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStoreInfo {
    pub store: String,
    pub record_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryIndexRequest {
    pub entity: String,
    pub topic: String,
    #[serde(default)]
    pub store: Option<String>,
    #[serde(default)]
    pub fact: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub conflict: Option<bool>,
    #[serde(default)]
    pub conflict_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryAssertion {
    pub assertion_id: String,
    pub entity: String,
    pub topic: String,
    pub store: String,
    pub fact: String,
    #[serde(default)]
    pub metadata: Option<Value>,
    pub indexed_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryConflictSignal {
    pub entity: String,
    pub topic: String,
    pub store: String,
    #[serde(default)]
    pub reason: Option<String>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryIndexResult {
    pub entity: String,
    pub topic: String,
    pub store: String,
    #[serde(default)]
    pub assertion_id: Option<String>,
    pub conflict_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryQueryRequest {
    #[serde(default)]
    pub entity: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub store: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryQueryResult {
    pub assertions: Vec<MemoryAssertion>,
    pub conflicts: Vec<MemoryConflictSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
struct PersistedMemoryState {
    #[serde(default)]
    assertions: Vec<MemoryAssertion>,
    #[serde(default)]
    conflicts: Vec<MemoryConflictSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryIndexError {
    EntityRequired,
    TopicRequired,
    UnsupportedStore(String),
    FactRequiredWhenConflictUnset,
    BackendPersistFailed(ElephantMemoryStoreError),
}

impl std::fmt::Display for MemoryIndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EntityRequired => write!(f, "entity is required"),
            Self::TopicRequired => write!(f, "topic is required"),
            Self::UnsupportedStore(store) => write!(f, "unsupported store: {store}"),
            Self::FactRequiredWhenConflictUnset => {
                write!(f, "fact is required when conflict is unset")
            }
            Self::BackendPersistFailed(err) => write!(f, "backend persist failed: {err}"),
        }
    }
}

impl std::error::Error for MemoryIndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BackendPersistFailed(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatingRiskTier {
    R0,
    R1,
    R2,
    R3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatingEvaluateRequest {
    pub action: String,
    pub actor_id: String,
    pub risk_tier: GatingRiskTier,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub requested_approver: Option<String>,
    #[serde(default)]
    pub approval_recipient: Option<String>,
    #[serde(default)]
    pub approval_channel: Option<String>,
    #[serde(default)]
    pub approval_timeout_ms: Option<u64>,
    #[serde(default)]
    pub entity: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatingOutcome {
    Allowed,
    AllowedWithAudit,
    PendingApproval,
    SafeDraft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatingEvaluateResult {
    pub action_id: String,
    pub action: String,
    pub actor_id: String,
    pub risk_tier: GatingRiskTier,
    pub outcome: GatingOutcome,
    #[serde(default)]
    pub pending_id: Option<String>,
    #[serde(default)]
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatingPendingEntry {
    pub pending_id: String,
    pub action_id: String,
    pub action: String,
    pub actor_id: String,
    pub risk_tier: GatingRiskTier,
    #[serde(default)]
    pub requested_approver: Option<String>,
    #[serde(default)]
    pub approval_recipient: Option<String>,
    #[serde(default)]
    pub approval_channel: Option<String>,
    #[serde(default)]
    pub approval_route_id: Option<String>,
    #[serde(default)]
    pub approval_delivery_id: Option<String>,
    pub created_at_ms: u64,
    pub deadline_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatingDecision {
    Approve,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatingDecideRequest {
    pub pending_id: String,
    pub approver_id: String,
    pub decision: GatingDecision,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatingDecisionResult {
    pub pending_id: String,
    pub action_id: String,
    pub approver_id: String,
    pub decision: GatingDecision,
    pub outcome: GatingOutcome,
    pub decided_at_ms: u64,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatingAuditEntry {
    pub audit_id: String,
    pub timestamp_ms: u64,
    pub event_type: String,
    pub action_id: String,
    #[serde(default)]
    pub pending_id: Option<String>,
    pub actor_id: String,
    pub risk_tier: GatingRiskTier,
    pub outcome: GatingOutcome,
    pub detail: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatingDecideError {
    UnknownPendingId(String),
    SelfApprovalForbidden,
    ApproverMismatch { expected: String, provided: String },
}

impl std::fmt::Display for GatingDecideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPendingId(id) => write!(f, "unknown pending id: {id}"),
            Self::SelfApprovalForbidden => write!(f, "self-approval is forbidden"),
            Self::ApproverMismatch { expected, provided } => {
                write!(f, "approver mismatch: expected {expected}, got {provided}")
            }
        }
    }
}

impl std::error::Error for GatingDecideError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeliveryIdempotencyEntry {
    delivery_id: String,
    payload: Value,
    canonical_resolution: RoutingResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct RouterBoundaryOverrides {
    channel: Option<String>,
    sink: Option<String>,
    target_module: Option<String>,
    retry_max: Option<u32>,
    backoff_ms: Option<u64>,
    rate_limit_per_minute: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct DeliveryBoundaryOutcome {
    sink_adapter: Option<String>,
    force_fail: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DeliveryRateWindowKey {
    route_id: String,
    recipient: String,
    sink: String,
    window_start_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MemoryConflictKey {
    entity: String,
    topic: String,
    store: String,
}

#[derive(Debug)]
pub struct MobkitRuntimeHandle {
    config: MobKitConfig,
    runtime_options: RuntimeOptions,
    loaded_modules: BTreeSet<String>,
    live_children: BTreeMap<String, Child>,
    pub lifecycle_events: Vec<LifecycleEvent>,
    pub supervisor_report: SupervisorReport,
    pub merged_events: Vec<EventEnvelope<UnifiedEvent>>,
    scheduling_claims: BTreeSet<String>,
    scheduling_claim_ticks: BTreeMap<u64, Vec<String>>,
    scheduling_last_due_ticks: BTreeMap<String, u64>,
    scheduling_dispatch_sequence: u64,
    routing_sequence: u64,
    routing_resolutions: BTreeMap<String, RoutingResolution>,
    routing_resolution_order: Vec<String>,
    runtime_routes: BTreeMap<String, RuntimeRoute>,
    delivery_sequence: u64,
    delivery_runtime_epoch_ms: u64,
    delivery_now_floor_ms: u64,
    delivery_clock_ms: u64,
    delivery_history: Vec<DeliveryRecord>,
    delivery_idempotency: BTreeMap<String, DeliveryIdempotencyEntry>,
    delivery_idempotency_by_delivery: BTreeMap<String, Vec<String>>,
    delivery_rate_window_counts: BTreeMap<DeliveryRateWindowKey, u32>,
    gating_sequence: u64,
    gating_pending: BTreeMap<String, GatingPendingEntry>,
    gating_pending_order: Vec<String>,
    gating_audit: Vec<GatingAuditEntry>,
    memory_sequence: u64,
    memory_assertions: Vec<MemoryAssertion>,
    memory_conflicts: BTreeMap<MemoryConflictKey, MemoryConflictSignal>,
    memory_backend: Option<ElephantMemoryStoreAdapter>,
    running: bool,
}

impl MobkitRuntimeHandle {
    pub fn lifecycle_events(&self) -> &[LifecycleEvent] {
        &self.lifecycle_events
    }

    pub fn supervisor_report(&self) -> &SupervisorReport {
        &self.supervisor_report
    }

    #[doc(hidden)]
    pub fn inject_test_events(&mut self, events: Vec<EventEnvelope<UnifiedEvent>>) {
        for event in events {
            insert_event_sorted(&mut self.merged_events, event);
        }
    }

    fn next_sequence(counter: &mut u64) -> u64 {
        let seq = *counter;
        *counter = counter.saturating_add(1);
        seq
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleValidationError {
    EmptyScheduleId,
    DuplicateScheduleId(String),
    InvalidTickMs(u64),
    InvalidInterval {
        schedule_id: String,
        interval: String,
    },
    InvalidTimezone {
        schedule_id: String,
        timezone: String,
    },
}

impl std::fmt::Display for ScheduleValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyScheduleId => write!(f, "empty schedule id"),
            Self::DuplicateScheduleId(id) => write!(f, "duplicate schedule id: {id}"),
            Self::InvalidTickMs(ms) => write!(f, "invalid tick ms: {ms}"),
            Self::InvalidInterval {
                schedule_id,
                interval,
            } => {
                write!(f, "invalid interval for schedule {schedule_id}: {interval}")
            }
            Self::InvalidTimezone {
                schedule_id,
                timezone,
            } => {
                write!(f, "invalid timezone for schedule {schedule_id}: {timezone}")
            }
        }
    }
}

impl std::error::Error for ScheduleValidationError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleRouteRequest {
    pub module_id: String,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleRouteResponse {
    pub module_id: String,
    pub method: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleRouteError {
    UnloadedModule(String),
    ModuleRuntime(RuntimeBoundaryError),
    UnexpectedRouteResponse,
}

impl std::fmt::Display for ModuleRouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnloadedModule(id) => write!(f, "unloaded module: {id}"),
            Self::ModuleRuntime(err) => write!(f, "module runtime: {err}"),
            Self::UnexpectedRouteResponse => write!(f, "unexpected route response"),
        }
    }
}

impl std::error::Error for ModuleRouteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ModuleRuntime(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingResolveError {
    RouterModuleNotLoaded,
    DeliveryModuleNotLoaded,
    EmptyRecipient,
    InvalidChannel,
    InvalidRateLimitPerMinute,
    RetryMaxExceedsCap { provided: u32, cap: u32 },
    RouterBoundary(RuntimeBoundaryError),
}

impl std::fmt::Display for RoutingResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RouterModuleNotLoaded => write!(f, "router module not loaded"),
            Self::DeliveryModuleNotLoaded => write!(f, "delivery module not loaded"),
            Self::EmptyRecipient => write!(f, "empty recipient"),
            Self::InvalidChannel => write!(f, "invalid channel"),
            Self::InvalidRateLimitPerMinute => write!(f, "invalid rate limit per minute"),
            Self::RetryMaxExceedsCap { provided, cap } => {
                write!(f, "retry max {provided} exceeds cap {cap}")
            }
            Self::RouterBoundary(err) => write!(f, "router boundary: {err}"),
        }
    }
}

impl std::error::Error for RoutingResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RouterBoundary(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliverySendError {
    DeliveryModuleNotLoaded,
    InvalidRouteTarget(String),
    InvalidRouteId,
    UnknownRouteId(String),
    ForgedResolution,
    InvalidRecipient,
    InvalidSink,
    InvalidIdempotencyKey,
    IdempotencyPayloadMismatch,
    RateLimited {
        sink: String,
        window_start_ms: u64,
        limit: u32,
    },
    DeliveryBoundary(RuntimeBoundaryError),
}

impl std::fmt::Display for DeliverySendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeliveryModuleNotLoaded => write!(f, "delivery module not loaded"),
            Self::InvalidRouteTarget(target) => write!(f, "invalid route target: {target}"),
            Self::InvalidRouteId => write!(f, "invalid route id"),
            Self::UnknownRouteId(id) => write!(f, "unknown route id: {id}"),
            Self::ForgedResolution => write!(f, "forged resolution"),
            Self::InvalidRecipient => write!(f, "invalid recipient"),
            Self::InvalidSink => write!(f, "invalid sink"),
            Self::InvalidIdempotencyKey => write!(f, "invalid idempotency key"),
            Self::IdempotencyPayloadMismatch => write!(f, "idempotency payload mismatch"),
            Self::RateLimited {
                sink,
                window_start_ms,
                limit,
            } => {
                write!(
                    f,
                    "rate limited on sink {sink} (window {window_start_ms}ms, limit {limit})"
                )
            }
            Self::DeliveryBoundary(err) => write!(f, "delivery boundary: {err}"),
        }
    }
}

impl std::error::Error for DeliverySendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DeliveryBoundary(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeRouteMutationError {
    EmptyRouteKey,
    EmptyRecipient,
    InvalidChannel,
    EmptySink,
    EmptyTargetModule,
    InvalidRateLimitPerMinute,
    RetryMaxExceedsCap { provided: u32, cap: u32 },
    RouteNotFound(String),
}

impl std::fmt::Display for RuntimeRouteMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyRouteKey => write!(f, "empty route key"),
            Self::EmptyRecipient => write!(f, "empty recipient"),
            Self::InvalidChannel => write!(f, "invalid channel"),
            Self::EmptySink => write!(f, "empty sink"),
            Self::EmptyTargetModule => write!(f, "empty target module"),
            Self::InvalidRateLimitPerMinute => write!(f, "invalid rate limit per minute"),
            Self::RetryMaxExceedsCap { provided, cap } => {
                write!(f, "retry max {provided} exceeds cap {cap}")
            }
            Self::RouteNotFound(key) => write!(f, "route not found: {key}"),
        }
    }
}

impl std::error::Error for RuntimeRouteMutationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcRouteError {
    InvalidRequest,
    BoundaryProcess(ProcessBoundaryError),
    Route(ModuleRouteError),
    InvalidResponse,
}

impl std::fmt::Display for RpcRouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest => write!(f, "invalid request"),
            Self::BoundaryProcess(err) => write!(f, "boundary process: {err}"),
            Self::Route(err) => write!(f, "route: {err}"),
            Self::InvalidResponse => write!(f, "invalid response"),
        }
    }
}

impl std::error::Error for RpcRouteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BoundaryProcess(err) => Some(err),
            Self::Route(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeMutationError {
    Config(ConfigResolutionError),
    Runtime(RuntimeBoundaryError),
}

impl std::fmt::Display for RuntimeMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(err) => write!(f, "config resolution: {err}"),
            Self::Runtime(err) => write!(f, "runtime boundary: {err}"),
        }
    }
}

impl std::error::Error for RuntimeMutationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(err) => Some(err),
            Self::Runtime(err) => Some(err),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscribeScope {
    Mob,
    Agent,
    Interaction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscribeRequest {
    pub scope: SubscribeScope,
    pub last_event_id: Option<String>,
    pub agent_id: Option<String>,
}

impl Default for SubscribeRequest {
    fn default() -> Self {
        Self {
            scope: SubscribeScope::Mob,
            last_event_id: None,
            agent_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscribeError {
    EmptyCheckpoint,
    UnknownCheckpoint(String),
    MissingAgentId,
    InvalidAgentId,
}

impl std::fmt::Display for SubscribeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyCheckpoint => write!(f, "empty checkpoint"),
            Self::UnknownCheckpoint(id) => write!(f, "unknown checkpoint: {id}"),
            Self::MissingAgentId => write!(f, "missing agent id"),
            Self::InvalidAgentId => write!(f, "invalid agent id"),
        }
    }
}

impl std::error::Error for SubscribeError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscribeKeepAlive {
    pub interval_ms: u64,
    pub event: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscribeResponse {
    pub scope: SubscribeScope,
    pub replay_from_event_id: Option<String>,
    pub keep_alive: SubscribeKeepAlive,
    pub keep_alive_comment: String,
    pub event_frames: Vec<String>,
    pub events: Vec<EventEnvelope<UnifiedEvent>>,
}

const SSE_KEEP_ALIVE_INTERVAL_MS: u64 = 15_000;
const SSE_KEEP_ALIVE_EVENT_NAME: &str = "keep-alive";
const SSE_KEEP_ALIVE_COMMENT_FRAME: &str = ": keep-alive\n\n";
const SUBSCRIBE_REPLAY_EVENT_CAP: usize = 3;
const SCHEDULING_CLAIM_RETENTION_WINDOW_MS: u64 = 86_400_000;
const SCHEDULING_CLAIMS_MAX_RETAINED: usize = 4_096;
const SCHEDULING_LAST_DUE_MAX_RETAINED: usize = 4_096;
const DELIVERY_HISTORY_LIMIT_DEFAULT: usize = 20;
const DELIVERY_HISTORY_LIMIT_MAX: usize = 200;
const ROUTING_RESOLUTION_LIMIT_MAX: usize = 512;
pub const ROUTING_RETRY_MAX_CAP: u32 = 10;
const DELIVERY_RATE_WINDOW_MS: u64 = 60_000;
const DELIVERY_RATE_WINDOWS_RETAINED: u64 = 2;
const DELIVERY_CLOCK_STEP_MS: u64 = 1_000;
const GATING_APPROVAL_TIMEOUT_DEFAULT_MS: u64 = 60_000;
const GATING_AUDIT_MAX_RETAINED: usize = 512;
const GATING_PENDING_MAX_RETAINED: usize = 512;
const MEMORY_ASSERTIONS_MAX_RETAINED: usize = 4_096;
const MEMORY_SUPPORTED_STORES: [&str; 5] = [
    "knowledge_graph",
    "vector",
    "timeline",
    "todo",
    "top_of_mind",
];
const ELEPHANT_HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(2);
// Multi-year bounded lookback so sparse valid cron schedules (for example leap-day)
// are not silently skipped when polling cadence is coarse.
const CRON_LOOKBACK_MINUTES: u64 = 5_270_400;
const CONSOLE_EXPERIENCE_ROUTE: &str = "/console/experience";
const CONSOLE_MODULES_ROUTE: &str = "/console/modules";
const EVENTS_SUBSCRIBE_METHOD: &str = "mobkit/events/subscribe";

fn default_delivery_history_limit() -> usize {
    DELIVERY_HISTORY_LIMIT_DEFAULT
}

pub fn run_meerkat_baseline_verification_once(
    command: &str,
    args: &[String],
    env: &[(String, String)],
    timeout: Duration,
) -> Result<BaselineVerificationReport, BaselineRuntimeError> {
    let line = run_process_json_line(command, args, env, timeout)
        .map_err(BaselineRuntimeError::Process)?;
    let value: Value =
        serde_json::from_str(&line).map_err(|_| BaselineRuntimeError::InvalidRepoPathJson)?;
    let repo = value
        .as_object()
        .and_then(|obj| obj.get("repo_root"))
        .ok_or(BaselineRuntimeError::MissingRepoRoot)?
        .as_str()
        .ok_or(BaselineRuntimeError::InvalidRepoRoot)?;
    if repo.trim().is_empty() {
        return Err(BaselineRuntimeError::InvalidRepoRoot);
    }
    verify_meerkat_baseline_symbols(Some(std::path::Path::new(repo)))
        .map_err(BaselineRuntimeError::Baseline)
}

pub fn build_runtime_decision_state(
    input: RuntimeDecisionInputs,
) -> Result<RuntimeDecisionState, DecisionRuntimeError> {
    validate_bigquery_naming(&input.bigquery).map_err(DecisionRuntimeError::Policy)?;
    let modules = load_trusted_mobkit_modules_from_toml(&input.trusted_mobkit_toml)
        .map_err(DecisionRuntimeError::Policy)?;
    validate_runtime_ops_policy(&input.ops).map_err(DecisionRuntimeError::Policy)?;
    let release_metadata = parse_release_metadata_json(&input.release_metadata_json)
        .map_err(DecisionRuntimeError::Policy)?;
    validate_release_metadata(&release_metadata).map_err(DecisionRuntimeError::Policy)?;
    if input.trusted_oidc.audience.trim().is_empty() {
        return Err(DecisionRuntimeError::Policy(
            DecisionPolicyError::InvalidTrustedAuthConfig(
                "trusted OIDC audience must not be empty".to_string(),
            ),
        ));
    }
    parse_oidc_discovery_json(&input.trusted_oidc.discovery_json).map_err(|err| {
        DecisionRuntimeError::Policy(DecisionPolicyError::InvalidTrustedAuthConfig(format!(
            "invalid trusted OIDC discovery: {err:?}"
        )))
    })?;
    parse_jwks_json(&input.trusted_oidc.jwks_json).map_err(|err| {
        DecisionRuntimeError::Policy(DecisionPolicyError::InvalidTrustedAuthConfig(format!(
            "invalid trusted JWKS: {err:?}"
        )))
    })?;

    Ok(RuntimeDecisionState {
        bigquery: input.bigquery,
        modules,
        auth: input.auth,
        trusted_oidc: input.trusted_oidc,
        console: input.console,
        ops: input.ops,
        release_metadata,
    })
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
