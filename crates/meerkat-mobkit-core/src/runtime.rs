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
    build_jwt_verification_key, inspect_jwt_header, parse_jwks_json, parse_oidc_discovery_json,
    select_jwk_for_token, validate_jwt_with_verification_key, JwtClaimsValidationConfig,
};
use crate::baseline::{
    verify_meerkat_baseline_symbols, BaselineVerificationError, BaselineVerificationReport,
};
use crate::decisions::{
    enforce_console_route_access, load_trusted_mobkit_modules_from_toml,
    parse_release_metadata_json, validate_bigquery_naming, validate_release_metadata,
    validate_runtime_ops_policy, AuthPolicy, AuthProvider, BigQueryNaming, ConsoleAccessRequest,
    ConsolePolicy, DecisionPolicyError, ReleaseMetadata, RuntimeOpsPolicy,
};
use crate::process::{run_process_json_line, ProcessBoundaryError};
use crate::protocol::parse_unified_event_line;
use crate::rpc::{parse_rpc_capabilities, RpcCapabilities, RpcCapabilitiesError};
use crate::types::{
    EventEnvelope, MobKitConfig, ModuleConfig, ModuleEvent, PreSpawnData, RestartPolicy,
    UnifiedEvent,
};

mod bootstrap;
mod delivery;
mod event_transport;
mod gating;
mod memory;
mod module_boundary;
mod routing;
mod rpc;
mod scheduling;
mod supervisor;

pub use bootstrap::{start_mobkit_runtime, start_mobkit_runtime_with_options};
pub use event_transport::normalize_event_line;
pub use routing::route_module_call;
pub use rpc::{
    route_module_call_rpc_json, route_module_call_rpc_subprocess,
    run_rpc_capabilities_boundary_once,
};
pub use scheduling::evaluate_schedules_at_tick;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeBoundaryError {
    Process(ProcessBoundaryError),
    Normalize(NormalizationError),
    Mcp(McpBoundaryError),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigResolutionError {
    ModuleNotConfigured(String),
    ModuleNotDiscovered(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeFromConfigError {
    Config(ConfigResolutionError),
    Runtime(RuntimeBoundaryError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcRuntimeError {
    Process(ProcessBoundaryError),
    Capabilities(RpcCapabilitiesError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineRuntimeError {
    Process(ProcessBoundaryError),
    InvalidRepoPathJson,
    MissingRepoRoot,
    InvalidRepoRoot,
    Baseline(BaselineVerificationError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MobkitRuntimeError {
    Config(ConfigResolutionError),
    MemoryBackend(ElephantMemoryStoreError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionRuntimeError {
    Policy(DecisionPolicyError),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStoreKind {
    BigQuery,
    JsonFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStoreContract {
    pub store: SessionStoreKind,
    pub latest_row_per_session: bool,
    pub tombstones_supported: bool,
    pub dedup_read_path: bool,
    pub file_locking: bool,
    pub crash_recovery: bool,
    pub bigquery_dataset: Option<String>,
    pub bigquery_table: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPersistenceRow {
    pub session_id: String,
    pub updated_at_ms: u64,
    pub deleted: bool,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonStoreLockRecord {
    pub owner_pid: u32,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonFileSessionStoreError {
    Io(String),
    Serialize(String),
    InvalidStoreData(String),
    LockHeld { lock_path: String },
    StaleLockRecoveryFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonFileSessionStore {
    data_path: PathBuf,
    lock_path: PathBuf,
    stale_lock_threshold: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BigQuerySessionStoreError {
    Io(String),
    Serialize(String),
    Configuration(String),
    Http(String),
    Api(String),
    InvalidQueryResponse(String),
    ProcessFailed { command: String, stderr: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigQuerySessionStoreAdapter {
    dataset: String,
    table: String,
    project_id: Option<String>,
    api_base_url: String,
    access_token: Option<String>,
    http_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElephantMemoryStoreError {
    InvalidConfig(String),
    Io(String),
    Serialize(String),
    InvalidStoreData(String),
    ExternalCallFailed(String),
}

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

struct JsonFileLockGuard {
    lock_path: PathBuf,
}

impl Drop for JsonFileLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleRestJsonRequest {
    pub method: String,
    pub path: String,
    pub auth: Option<ConsoleAccessRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleRestJsonResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleLiveSnapshot {
    pub running: bool,
    pub loaded_modules: Vec<String>,
}

impl ConsoleLiveSnapshot {
    pub fn new(running: bool, loaded_modules: Vec<String>) -> Self {
        let mut seen = BTreeSet::new();
        let mut deduped_modules = Vec::new();
        for module_id in loaded_modules {
            if seen.insert(module_id.clone()) {
                deduped_modules.push(module_id);
            }
        }
        Self {
            running,
            loaded_modules: deduped_modules,
        }
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcRouteError {
    InvalidRequest,
    BoundaryProcess(ProcessBoundaryError),
    Route(ModuleRouteError),
    InvalidResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeMutationError {
    Config(ConfigResolutionError),
    Runtime(RuntimeBoundaryError),
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

pub fn session_store_contracts(decisions: &RuntimeDecisionState) -> Vec<SessionStoreContract> {
    vec![
        SessionStoreContract {
            store: SessionStoreKind::BigQuery,
            latest_row_per_session: true,
            tombstones_supported: true,
            dedup_read_path: true,
            file_locking: false,
            crash_recovery: false,
            bigquery_dataset: Some(decisions.bigquery.dataset.clone()),
            bigquery_table: Some(decisions.bigquery.table.clone()),
        },
        SessionStoreContract {
            store: SessionStoreKind::JsonFile,
            latest_row_per_session: true,
            tombstones_supported: true,
            dedup_read_path: true,
            file_locking: true,
            crash_recovery: true,
            bigquery_dataset: None,
            bigquery_table: None,
        },
    ]
}

pub fn materialize_latest_session_rows(
    rows: &[SessionPersistenceRow],
) -> Vec<SessionPersistenceRow> {
    let mut latest_by_session: BTreeMap<String, SessionPersistenceRow> = BTreeMap::new();
    for row in rows {
        let should_replace = match latest_by_session.get(&row.session_id) {
            Some(existing) => row.updated_at_ms >= existing.updated_at_ms,
            None => true,
        };
        if should_replace {
            latest_by_session.insert(row.session_id.clone(), row.clone());
        }
    }
    latest_by_session.into_values().collect()
}

pub fn materialize_live_session_rows(rows: &[SessionPersistenceRow]) -> Vec<SessionPersistenceRow> {
    materialize_latest_session_rows(rows)
        .into_iter()
        .filter(|row| !row.deleted)
        .collect()
}

impl JsonFileSessionStore {
    pub fn new(data_path: impl AsRef<Path>) -> Self {
        let data_path = data_path.as_ref().to_path_buf();
        let lock_path = data_path.with_extension("lock");
        Self {
            data_path,
            lock_path,
            stale_lock_threshold: Duration::from_secs(30),
        }
    }

    pub fn with_lock_path(mut self, lock_path: impl AsRef<Path>) -> Self {
        self.lock_path = lock_path.as_ref().to_path_buf();
        self
    }

    pub fn with_stale_lock_threshold(mut self, threshold: Duration) -> Self {
        self.stale_lock_threshold = threshold;
        self
    }

    pub fn data_path(&self) -> &Path {
        &self.data_path
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    pub fn append_rows(
        &self,
        rows: &[SessionPersistenceRow],
    ) -> Result<(), JsonFileSessionStoreError> {
        let _guard = self.acquire_lock()?;
        if let Some(parent) = self.data_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| JsonFileSessionStoreError::Io(err.to_string()))?;
        }

        let mut persisted = self.read_rows()?;
        persisted.extend(rows.iter().cloned());

        let tmp_path = self.data_path.with_extension("tmp");
        let json = serde_json::to_vec_pretty(&persisted)
            .map_err(|err| JsonFileSessionStoreError::Serialize(err.to_string()))?;
        fs::write(&tmp_path, json).map_err(|err| JsonFileSessionStoreError::Io(err.to_string()))?;
        fs::rename(&tmp_path, &self.data_path)
            .map_err(|err| JsonFileSessionStoreError::Io(err.to_string()))?;
        Ok(())
    }

    pub fn read_rows(&self) -> Result<Vec<SessionPersistenceRow>, JsonFileSessionStoreError> {
        if !self.data_path.exists() {
            return Ok(vec![]);
        }
        let bytes = fs::read(&self.data_path)
            .map_err(|err| JsonFileSessionStoreError::Io(err.to_string()))?;
        serde_json::from_slice::<Vec<SessionPersistenceRow>>(&bytes)
            .map_err(|err| JsonFileSessionStoreError::InvalidStoreData(err.to_string()))
    }

    pub fn read_latest_rows(
        &self,
    ) -> Result<Vec<SessionPersistenceRow>, JsonFileSessionStoreError> {
        let rows = self.read_rows()?;
        Ok(materialize_latest_session_rows(&rows))
    }

    pub fn read_live_rows(&self) -> Result<Vec<SessionPersistenceRow>, JsonFileSessionStoreError> {
        let rows = self.read_rows()?;
        Ok(materialize_live_session_rows(&rows))
    }

    fn acquire_lock(&self) -> Result<JsonFileLockGuard, JsonFileSessionStoreError> {
        if let Some(parent) = self.lock_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| JsonFileSessionStoreError::Io(err.to_string()))?;
        }

        let mut attempts = 0_u8;
        loop {
            attempts += 1;
            match OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&self.lock_path)
            {
                Ok(mut file) => {
                    let lock_record = JsonStoreLockRecord {
                        owner_pid: std::process::id(),
                        created_at_ms: current_time_ms(),
                    };
                    let lock_bytes = serde_json::to_vec(&lock_record)
                        .map_err(|err| JsonFileSessionStoreError::Serialize(err.to_string()))?;
                    file.write_all(&lock_bytes)
                        .map_err(|err| JsonFileSessionStoreError::Io(err.to_string()))?;
                    return Ok(JsonFileLockGuard {
                        lock_path: self.lock_path.clone(),
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if attempts >= 2 {
                        return Err(JsonFileSessionStoreError::LockHeld {
                            lock_path: self.lock_path.display().to_string(),
                        });
                    }
                    if self.is_lock_stale()? {
                        fs::remove_file(&self.lock_path).map_err(|remove_err| {
                            JsonFileSessionStoreError::StaleLockRecoveryFailed(
                                remove_err.to_string(),
                            )
                        })?;
                        continue;
                    }
                    return Err(JsonFileSessionStoreError::LockHeld {
                        lock_path: self.lock_path.display().to_string(),
                    });
                }
                Err(err) => return Err(JsonFileSessionStoreError::Io(err.to_string())),
            }
        }
    }

    fn is_lock_stale(&self) -> Result<bool, JsonFileSessionStoreError> {
        let bytes = fs::read(&self.lock_path)
            .map_err(|err| JsonFileSessionStoreError::Io(err.to_string()))?;
        let stale_threshold_ms = self.stale_lock_threshold.as_millis() as u64;
        if let Ok(record) = serde_json::from_slice::<JsonStoreLockRecord>(&bytes) {
            let age_ms = current_time_ms().saturating_sub(record.created_at_ms);
            if age_ms < stale_threshold_ms {
                return Ok(false);
            }
            return Ok(!is_process_alive(record.owner_pid));
        }

        let modified = fs::metadata(&self.lock_path)
            .and_then(|meta| meta.modified())
            .map_err(|err| JsonFileSessionStoreError::Io(err.to_string()))?;
        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default();
        Ok(age >= self.stale_lock_threshold)
    }
}

fn is_process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let status = Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(exit_status) => exit_status.success(),
        // If liveness probing is unavailable, avoid evicting potentially active locks.
        Err(_) => true,
    }
}

impl BigQuerySessionStoreAdapter {
    pub const DEFAULT_API_BASE_URL: &'static str = "https://bigquery.googleapis.com/bigquery/v2";

    pub fn new(
        _legacy_bq_binary: impl AsRef<Path>,
        dataset: impl Into<String>,
        table: impl Into<String>,
    ) -> Self {
        Self::new_native(dataset, table)
    }

    pub fn new_native(dataset: impl Into<String>, table: impl Into<String>) -> Self {
        Self {
            dataset: dataset.into(),
            table: table.into(),
            project_id: None,
            api_base_url: Self::DEFAULT_API_BASE_URL.to_string(),
            access_token: None,
            http_timeout: Duration::from_secs(30),
        }
    }

    pub fn with_project_id(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }

    pub fn with_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        self.api_base_url = api_base_url.into();
        self
    }

    pub fn with_access_token(mut self, access_token: impl Into<String>) -> Self {
        self.access_token = Some(access_token.into());
        self
    }

    pub fn with_http_timeout(mut self, timeout: Duration) -> Self {
        self.http_timeout = timeout;
        self
    }

    pub fn with_bearer_token(self, access_token: impl Into<String>) -> Self {
        self.with_access_token(access_token)
    }

    pub fn table_ref(&self) -> String {
        format!("{}.{}", self.dataset, self.table)
    }

    pub fn stream_insert_rows(
        &self,
        rows: &[SessionPersistenceRow],
    ) -> Result<(), BigQuerySessionStoreError> {
        if rows.is_empty() {
            return Ok(());
        }

        let project_id = self.resolve_project_id()?;
        let access_token = self.resolve_access_token()?;
        let endpoint = format!(
            "{}/projects/{project_id}/datasets/{}/tables/{}/insertAll",
            self.api_base_url(),
            self.dataset,
            self.table
        );

        let mut request_rows = Vec::with_capacity(rows.len());
        for (idx, row) in rows.iter().enumerate() {
            let payload_json = serde_json::to_string(&row.payload)
                .map_err(|err| BigQuerySessionStoreError::Serialize(err.to_string()))?;
            request_rows.push(serde_json::json!({
                "insertId": format!("{}-{}-{idx}", row.session_id, row.updated_at_ms),
                "json": {
                    "session_id": row.session_id,
                    "updated_at_ms": row.updated_at_ms.to_string(),
                    "deleted": row.deleted,
                    "payload": payload_json,
                },
            }));
        }
        let request = serde_json::json!({
            "ignoreUnknownValues": false,
            "skipInvalidRows": false,
            "rows": request_rows,
        });

        let response = self.send_json_request(
            reqwest::Method::POST,
            &endpoint,
            &access_token,
            Some(&request),
        )?;
        if let Some(errors) = response.get("insertErrors").and_then(Value::as_array) {
            if !errors.is_empty() {
                let detail = serde_json::to_string(errors)
                    .unwrap_or_else(|_| "<serialize_error>".to_string());
                return Err(BigQuerySessionStoreError::Api(format!(
                    "BigQuery insertAll returned row errors: {detail}"
                )));
            }
        }

        Ok(())
    }

    pub fn read_rows(&self) -> Result<Vec<SessionPersistenceRow>, BigQuerySessionStoreError> {
        let project_id = self.resolve_project_id()?;
        let access_token = self.resolve_access_token()?;
        let endpoint = format!("{}/projects/{project_id}/queries", self.api_base_url());
        let query = format!(
            "SELECT session_id, updated_at_ms, deleted, payload FROM `{}.{}` ORDER BY updated_at_ms ASC",
            project_id,
            self.table_ref()
        );
        let request = serde_json::json!({
            "query": query,
            "useLegacySql": false,
            "maxResults": 10000,
        });

        let response = self.send_json_request(
            reqwest::Method::POST,
            &endpoint,
            &access_token,
            Some(&request),
        )?;
        parse_bigquery_query_rows(&response)
    }

    pub fn read_latest_rows(
        &self,
    ) -> Result<Vec<SessionPersistenceRow>, BigQuerySessionStoreError> {
        let rows = self.read_rows()?;
        Ok(materialize_latest_session_rows(&rows))
    }

    pub fn read_live_rows(&self) -> Result<Vec<SessionPersistenceRow>, BigQuerySessionStoreError> {
        let rows = self.read_rows()?;
        Ok(materialize_live_session_rows(&rows))
    }

    fn api_base_url(&self) -> &str {
        self.api_base_url.trim_end_matches('/')
    }

    fn resolve_project_id(&self) -> Result<String, BigQuerySessionStoreError> {
        if let Some(project_id) = self.project_id.as_deref() {
            let project = project_id.trim();
            if !project.is_empty() {
                return Ok(project.to_string());
            }
        }

        if let Ok(project_id) = std::env::var("BIGQUERY_PROJECT_ID") {
            let project = project_id.trim();
            if !project.is_empty() {
                return Ok(project.to_string());
            }
        }

        Err(BigQuerySessionStoreError::Configuration(
            "missing BigQuery project_id: call with_project_id(...) or set BIGQUERY_PROJECT_ID"
                .to_string(),
        ))
    }

    fn resolve_access_token(&self) -> Result<String, BigQuerySessionStoreError> {
        if let Some(token) = self.access_token.as_deref() {
            let token = token.trim();
            if !token.is_empty() {
                return Ok(token.to_string());
            }
        }

        for key in [
            "BIGQUERY_ACCESS_TOKEN",
            "GOOGLE_OAUTH_ACCESS_TOKEN",
            "GOOGLE_ACCESS_TOKEN",
        ] {
            if let Ok(token) = std::env::var(key) {
                let token = token.trim();
                if !token.is_empty() {
                    return Ok(token.to_string());
                }
            }
        }

        Err(BigQuerySessionStoreError::Configuration(
            "missing BigQuery access token: call with_access_token(...) or set BIGQUERY_ACCESS_TOKEN"
                .to_string(),
        ))
    }

    fn send_json_request(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        access_token: &str,
        body: Option<&Value>,
    ) -> Result<Value, BigQuerySessionStoreError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(self.http_timeout)
            .build()
            .map_err(|err| BigQuerySessionStoreError::Http(format!("{err:?}")))?;

        let mut request = client
            .request(method, endpoint)
            .bearer_auth(access_token)
            .header("accept", "application/json");
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .json(body);
        }

        let response = request
            .send()
            .map_err(|err| BigQuerySessionStoreError::Http(format!("{err:?}")))?;
        let status = response.status();
        let text = response
            .text()
            .map_err(|err| BigQuerySessionStoreError::Http(format!("{err:?}")))?;

        if !status.is_success() {
            return Err(BigQuerySessionStoreError::Api(format!(
                "BigQuery API request failed (status {}): {}",
                status.as_u16(),
                text
            )));
        }

        if text.trim().is_empty() {
            return Ok(serde_json::json!({}));
        }

        serde_json::from_str::<Value>(&text)
            .map_err(|err| BigQuerySessionStoreError::InvalidQueryResponse(err.to_string()))
    }
}

fn parse_bigquery_query_rows(
    response: &Value,
) -> Result<Vec<SessionPersistenceRow>, BigQuerySessionStoreError> {
    if response.is_array() {
        return serde_json::from_value::<Vec<SessionPersistenceRow>>(response.clone())
            .map_err(|err| BigQuerySessionStoreError::InvalidQueryResponse(err.to_string()));
    }

    let rows = response
        .get("rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut parsed = Vec::with_capacity(rows.len());
    for row in rows {
        parsed.push(parse_bigquery_query_row(&row)?);
    }

    Ok(parsed)
}

fn parse_bigquery_query_row(
    row: &Value,
) -> Result<SessionPersistenceRow, BigQuerySessionStoreError> {
    let fields = row.get("f").and_then(Value::as_array).ok_or_else(|| {
        BigQuerySessionStoreError::InvalidQueryResponse(
            "missing row.f cell array in query response".to_string(),
        )
    })?;
    if fields.len() < 4 {
        return Err(BigQuerySessionStoreError::InvalidQueryResponse(
            "query response row has fewer than 4 columns".to_string(),
        ));
    }

    let session_id = parse_bigquery_string_cell(&fields[0], "session_id")?;
    let updated_at_ms = parse_bigquery_u64_cell(&fields[1], "updated_at_ms")?;
    let deleted = parse_bigquery_bool_cell(&fields[2], "deleted")?;
    let payload = parse_bigquery_payload_cell(&fields[3], "payload")?;

    Ok(SessionPersistenceRow {
        session_id,
        updated_at_ms,
        deleted,
        payload,
    })
}

fn parse_bigquery_string_cell(
    cell: &Value,
    column: &str,
) -> Result<String, BigQuerySessionStoreError> {
    let value = bigquery_cell_value(cell);
    match value {
        Value::String(s) => Ok(s.clone()),
        _ => Err(BigQuerySessionStoreError::InvalidQueryResponse(format!(
            "query column {column} is not a string"
        ))),
    }
}

fn parse_bigquery_u64_cell(cell: &Value, column: &str) -> Result<u64, BigQuerySessionStoreError> {
    let value = bigquery_cell_value(cell);
    match value {
        Value::Number(num) => num.as_u64().ok_or_else(|| {
            BigQuerySessionStoreError::InvalidQueryResponse(format!(
                "query column {column} is not a u64 number"
            ))
        }),
        Value::String(s) => s.parse::<u64>().map_err(|_| {
            BigQuerySessionStoreError::InvalidQueryResponse(format!(
                "query column {column} is not a u64 string"
            ))
        }),
        _ => Err(BigQuerySessionStoreError::InvalidQueryResponse(format!(
            "query column {column} is not a u64 value"
        ))),
    }
}

fn parse_bigquery_bool_cell(cell: &Value, column: &str) -> Result<bool, BigQuerySessionStoreError> {
    let value = bigquery_cell_value(cell);
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::String(s) => match s.as_str() {
            "true" | "TRUE" | "1" => Ok(true),
            "false" | "FALSE" | "0" => Ok(false),
            _ => Err(BigQuerySessionStoreError::InvalidQueryResponse(format!(
                "query column {column} is not a bool string"
            ))),
        },
        _ => Err(BigQuerySessionStoreError::InvalidQueryResponse(format!(
            "query column {column} is not a bool value"
        ))),
    }
}

fn parse_bigquery_payload_cell(
    cell: &Value,
    column: &str,
) -> Result<Value, BigQuerySessionStoreError> {
    let value = bigquery_cell_value(cell);
    match value {
        Value::Null => Ok(serde_json::json!({})),
        Value::String(s) => {
            if s.trim().is_empty() {
                return Ok(serde_json::json!({}));
            }
            serde_json::from_str::<Value>(s).map_err(|_| {
                BigQuerySessionStoreError::InvalidQueryResponse(format!(
                    "query column {column} payload JSON parse failed"
                ))
            })
        }
        _ => Ok(value.clone()),
    }
}

fn bigquery_cell_value(cell: &Value) -> &Value {
    cell.get("v").unwrap_or(cell)
}

pub fn handle_console_rest_json_route(
    decisions: &RuntimeDecisionState,
    request: &ConsoleRestJsonRequest,
) -> ConsoleRestJsonResponse {
    handle_console_rest_json_route_with_snapshot(decisions, request, None)
}

pub fn handle_console_rest_json_route_with_snapshot(
    decisions: &RuntimeDecisionState,
    request: &ConsoleRestJsonRequest,
    live_snapshot: Option<&ConsoleLiveSnapshot>,
) -> ConsoleRestJsonResponse {
    let (base_path, query_params) = split_path_and_query(&request.path);
    if request.method != "GET"
        || (base_path != CONSOLE_MODULES_ROUTE && base_path != CONSOLE_EXPERIENCE_ROUTE)
    {
        return ConsoleRestJsonResponse {
            status: 404,
            body: serde_json::json!({"error":"not_found"}),
        };
    }

    let resolved_auth = match resolve_console_auth(decisions, request.auth.as_ref(), &query_params)
    {
        Ok(auth) => auth,
        Err(error) => {
            return ConsoleRestJsonResponse {
                status: 401,
                body: serde_json::json!({
                    "error":"unauthorized",
                    "reason": console_auth_error_reason(&error),
                }),
            };
        }
    };

    match resolved_auth {
        Some(auth) => {
            if let Err(error) =
                enforce_console_route_access(&decisions.auth, &decisions.console, &auth)
            {
                return ConsoleRestJsonResponse {
                    status: 401,
                    body: serde_json::json!({
                        "error":"unauthorized",
                        "reason": auth_error_reason(&error),
                    }),
                };
            }
        }
        None if decisions.console.require_app_auth => {
            return ConsoleRestJsonResponse {
                status: 401,
                body: serde_json::json!({
                    "error":"unauthorized",
                    "reason":"missing_credentials",
                }),
            };
        }
        None => {}
    }

    let modules: Vec<String> = decisions
        .modules
        .iter()
        .map(|module| module.id.clone())
        .collect();
    let live_snapshot = live_snapshot
        .cloned()
        .unwrap_or_else(|| default_console_live_snapshot(decisions));
    let body = if base_path == CONSOLE_EXPERIENCE_ROUTE {
        build_console_experience_contract(&modules, &live_snapshot)
    } else {
        serde_json::json!({ "modules": modules })
    };
    ConsoleRestJsonResponse { status: 200, body }
}

fn default_console_live_snapshot(decisions: &RuntimeDecisionState) -> ConsoleLiveSnapshot {
    ConsoleLiveSnapshot::new(
        !decisions.modules.is_empty(),
        decisions
            .modules
            .iter()
            .map(|module| module.id.clone())
            .collect(),
    )
}

fn build_console_experience_contract(
    modules: &[String],
    live_snapshot: &ConsoleLiveSnapshot,
) -> Value {
    let module_panels = modules
        .iter()
        .map(|module_id| {
            serde_json::json!({
                "panel_id": format!("module.{module_id}"),
                "module_id": module_id,
                "title": format!("{module_id} module"),
                "route": format!("/console/modules/{module_id}"),
                "capabilities": {
                    "can_render": true,
                    "can_subscribe_activity": true,
                }
            })
        })
        .collect::<Vec<_>>();
    let sidebar_agents = live_snapshot
        .loaded_modules
        .iter()
        .map(|module_id| {
            serde_json::json!({
                "agent_id": module_id,
                "member_id": module_id,
                "label": module_id,
                "kind": "module_agent",
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "contract_version": "0.1.0",
        "base_panel": {
            "panel_id": "console.home",
            "title": "Mob Console",
            "route": CONSOLE_EXPERIENCE_ROUTE,
            "capabilities": {
                "can_render": true,
                "surface": "console",
            }
        },
        "module_panels": module_panels,
        "agent_sidebar": {
            "panel_id": "console.agent_sidebar",
            "title": "Agents",
            "source_method": "mobkit/status",
            "refresh_policy": {
                "mode": "pull",
                "poll_interval_ms": 5000,
            },
            "selection_contract": {
                "selected_agent_id_field": "agent_id",
                "selected_member_id_field": "member_id",
                "emits_scope": "agent",
                "supported_scopes": ["mob", "agent"],
            },
            "list_item_contract": {
                "fields": ["agent_id", "member_id", "label", "kind"],
                "agent_id_field": "agent_id",
                "member_id_field": "member_id",
            },
            "live_snapshot": {
                "agents": sidebar_agents,
            }
        },
        "activity_feed": {
            "panel_id": "console.activity_feed",
            "title": "Activity",
            "transport": "sse",
            "source_method": EVENTS_SUBSCRIBE_METHOD,
            "supported_scopes": ["mob", "agent", "interaction"],
            "default_scope": "mob",
            "request_contract": {
                "scope": "mob|agent|interaction",
                "agent_id": "required when scope=agent",
                "last_event_id": "optional checkpoint from prior event_id",
            },
            "event_contract": {
                "envelope_fields": ["event_id", "source", "timestamp_ms", "event"],
                "event_type_path": "event.event_type",
                "frame_format": "id: <event_id>\\nevent: <event_type>\\ndata: <event_json>\\n\\n",
            },
            "keep_alive": {
                "interval_ms": SSE_KEEP_ALIVE_INTERVAL_MS,
                "event": SSE_KEEP_ALIVE_EVENT_NAME,
                "comment_frame": SSE_KEEP_ALIVE_COMMENT_FRAME,
            }
        },
        "chat_inspector": {
            "panel_id": "console.chat_inspector",
            "title": "Chat Inspector",
            "stream_route": "/interactions/stream",
            "transport": "sse",
            "request_contract": {
                "member_id": "required target member id",
                "message": "required user text to inject",
            },
            "event_contract": {
                "interaction_start_event": "interaction_started",
                "agent_event_type_path": "type",
                "ordered_by": "interaction_id + seq",
            }
        },
        "topology": {
            "panel_id": "console.topology",
            "title": "Topology",
            "source_method": "mobkit/status",
            "route_method": "mobkit/routing/routes/list",
            "refresh_policy": {
                "mode": "pull",
                "poll_interval_ms": 5000,
            },
            "graph_contract": {
                "node_id_field": "module_id",
                "edge_fields": ["from", "to", "route"],
            },
            "live_snapshot": {
                "nodes": &live_snapshot.loaded_modules,
                "node_count": live_snapshot.loaded_modules.len(),
            }
        },
        "health_overview": {
            "panel_id": "console.health_overview",
            "title": "Health",
            "source_method": "mobkit/status",
            "activity_source_method": EVENTS_SUBSCRIBE_METHOD,
            "refresh_policy": {
                "mode": "pull_and_stream",
                "poll_interval_ms": 5000,
            },
            "status_contract": {
                "running_field": "running",
                "loaded_modules_field": "loaded_modules",
            },
            "live_snapshot": {
                "running": live_snapshot.running,
                "loaded_modules": &live_snapshot.loaded_modules,
                "loaded_module_count": live_snapshot.loaded_modules.len(),
            }
        }
    })
}

fn split_path_and_query(path: &str) -> (&str, BTreeMap<String, String>) {
    let (base, query) = path.split_once('?').unwrap_or((path, ""));
    let mut params = BTreeMap::new();
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        if !k.is_empty() {
            params.insert(k.to_string(), v.to_string());
        }
    }
    (base, params)
}

fn resolve_console_auth(
    decisions: &RuntimeDecisionState,
    explicit_auth: Option<&ConsoleAccessRequest>,
    query_params: &BTreeMap<String, String>,
) -> Result<Option<ConsoleAccessRequest>, ConsoleAuthResolutionError> {
    if let Some(auth) = explicit_auth {
        return Ok(Some(auth.clone()));
    }

    if !decisions.console.require_app_auth {
        return Ok(None);
    }

    match query_params.get("auth_token") {
        Some(token) => resolve_console_auth_from_token(decisions, token).map(Some),
        None => Ok(None),
    }
}

fn resolve_console_auth_from_token(
    decisions: &RuntimeDecisionState,
    token: &str,
) -> Result<ConsoleAccessRequest, ConsoleAuthResolutionError> {
    if decisions.trusted_oidc.audience.trim().is_empty() {
        return Err(ConsoleAuthResolutionError::InvalidTrustedOidcConfig);
    }

    let discovery = parse_oidc_discovery_json(&decisions.trusted_oidc.discovery_json)
        .map_err(|_| ConsoleAuthResolutionError::InvalidTrustedOidcConfig)?;
    let jwks = parse_jwks_json(&decisions.trusted_oidc.jwks_json)
        .map_err(|_| ConsoleAuthResolutionError::InvalidTrustedOidcConfig)?;
    let header =
        inspect_jwt_header(token).map_err(|_| ConsoleAuthResolutionError::InvalidTokenHeader)?;

    if header.alg == "HS256"
        && !hs256_allowed_for_development_issuer(&discovery.issuer, &discovery.jwks_uri)
    {
        return Err(ConsoleAuthResolutionError::Hs256NotAllowed);
    }

    let key = select_jwk_for_token(&jwks, header.kid.as_deref(), &header.alg)
        .map_err(|_| ConsoleAuthResolutionError::JwksKeyNotFound)?;
    let verification_key = build_jwt_verification_key(key, &header.alg)
        .map_err(|_| ConsoleAuthResolutionError::InvalidJwksKeyMaterial)?;

    let now_epoch_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let claims = validate_jwt_with_verification_key(
        token,
        &verification_key,
        &JwtClaimsValidationConfig {
            issuer: Some(discovery.issuer),
            audience: Some(decisions.trusted_oidc.audience.clone()),
            now_epoch_seconds,
            leeway_seconds: 30,
        },
    )
    .map_err(|_| ConsoleAuthResolutionError::InvalidToken)?;

    let principal = claims
        .email
        .or(claims.subject)
        .ok_or(ConsoleAuthResolutionError::MissingTokenIdentity)?;
    let provider =
        if claims.actor_type.as_deref() == Some("service") || principal.starts_with("svc:") {
            AuthProvider::ServiceIdentity
        } else {
            match claims.provider.as_deref() {
                Some("google_oauth") => AuthProvider::GoogleOAuth,
                Some("github_oauth") => AuthProvider::GitHubOAuth,
                Some("generic_oidc") => AuthProvider::GenericOidc,
                _ => AuthProvider::GenericOidc,
            }
        };

    Ok(ConsoleAccessRequest {
        provider,
        email: principal,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConsoleAuthResolutionError {
    InvalidTrustedOidcConfig,
    InvalidTokenHeader,
    JwksKeyNotFound,
    InvalidJwksKeyMaterial,
    InvalidToken,
    MissingTokenIdentity,
    Hs256NotAllowed,
}

fn console_auth_error_reason(error: &ConsoleAuthResolutionError) -> &'static str {
    match error {
        ConsoleAuthResolutionError::InvalidTrustedOidcConfig => "invalid_trusted_oidc_config",
        ConsoleAuthResolutionError::InvalidTokenHeader => "invalid_token_header",
        ConsoleAuthResolutionError::JwksKeyNotFound => "jwks_key_not_found",
        ConsoleAuthResolutionError::InvalidJwksKeyMaterial => "invalid_jwks_key_material",
        ConsoleAuthResolutionError::InvalidToken => "invalid_token",
        ConsoleAuthResolutionError::MissingTokenIdentity => "missing_token_identity",
        ConsoleAuthResolutionError::Hs256NotAllowed => "hs256_not_allowed",
    }
}

fn hs256_allowed_for_development_issuer(issuer: &str, jwks_uri: &str) -> bool {
    match (extract_uri_host(issuer), extract_uri_host(jwks_uri)) {
        (Some(issuer_host), Some(jwks_host)) => {
            is_development_host(issuer_host) && is_development_host(jwks_host)
        }
        _ => false,
    }
}

fn extract_uri_host(uri: &str) -> Option<&str> {
    let after_scheme = uri.split_once("://").map_or(uri, |(_, rest)| rest);
    let authority_with_path = after_scheme.split('/').next()?;
    let authority = authority_with_path
        .rsplit('@')
        .next()
        .unwrap_or(authority_with_path);
    if authority.is_empty() {
        return None;
    }

    if let Some(stripped) = authority.strip_prefix('[') {
        let (ipv6_host, _) = stripped.split_once(']')?;
        return if ipv6_host.is_empty() {
            None
        } else {
            Some(ipv6_host)
        };
    }

    let host = authority
        .split_once(':')
        .map_or(authority, |(hostname, _)| hostname);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn is_development_host(host: &str) -> bool {
    let lowercase = host.to_ascii_lowercase();
    lowercase == "localhost"
        || lowercase == "127.0.0.1"
        || lowercase == "::1"
        || lowercase.ends_with(".localhost")
}

fn auth_error_reason(error: &DecisionPolicyError) -> &'static str {
    match error {
        DecisionPolicyError::AuthProviderMismatch => "provider_mismatch",
        DecisionPolicyError::AuthProviderNotSupported => "provider_not_supported",
        DecisionPolicyError::EmailNotAllowlisted => "email_not_allowlisted",
        DecisionPolicyError::InvalidServiceIdentity => "invalid_service_identity",
        DecisionPolicyError::ServiceIdentityNotAllowlisted => "service_identity_not_allowlisted",
        _ => "policy_denied",
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
