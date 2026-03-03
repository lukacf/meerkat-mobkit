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
    handle_console_rest_json_route, handle_console_rest_json_route_with_snapshot,
    ConsoleLiveSnapshot, ConsoleRestJsonRequest, ConsoleRestJsonResponse,
};
pub use event_transport::normalize_event_line;
pub use routing::route_module_call;
pub use rpc::{
    route_module_call_rpc_json, route_module_call_rpc_subprocess,
    run_rpc_capabilities_boundary_once,
};
pub use scheduling::evaluate_schedules_at_tick;
pub use session_store::{
    materialize_latest_session_rows, materialize_live_session_rows, session_store_contracts,
    BigQuerySessionStoreAdapter, BigQuerySessionStoreError, JsonFileSessionStore,
    JsonFileSessionStoreError, JsonStoreLockRecord, SessionPersistenceRow, SessionStoreContract,
    SessionStoreKind,
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
pub const WILDCARD_ROUTE: &str = "*";
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
