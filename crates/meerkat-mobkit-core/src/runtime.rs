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
    extract_hs256_shared_secret, inspect_jwt_header, parse_jwks_json, parse_oidc_discovery_json,
    select_jwk_for_token, validate_jwt_locally, JwtValidationConfig,
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
    InvalidQueryResponse(String),
    ProcessFailed { command: String, stderr: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigQuerySessionStoreAdapter {
    bq_binary: PathBuf,
    dataset: String,
    table: String,
    project_id: Option<String>,
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
pub struct RuntimeOptions {
    pub on_failure_retry_budget: u32,
    pub always_restart_budget: u32,
    #[serde(default)]
    pub memory_backend: Option<MemoryBackendConfig>,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            on_failure_retry_budget: 1,
            always_restart_budget: 1,
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
const ROUTER_RESOLVE_REQUEST_ENV: &str = "MOBKIT_ROUTING_RESOLVE_REQUEST";
const DELIVERY_SEND_REQUEST_ENV: &str = "MOBKIT_DELIVERY_SEND_REQUEST";
// Multi-year bounded lookback so sparse valid cron schedules (for example leap-day)
// are not silently skipped when polling cadence is coarse.
const CRON_LOOKBACK_MINUTES: u64 = 5_270_400;
const CONSOLE_EXPERIENCE_ROUTE: &str = "/console/experience";
const CONSOLE_MODULES_ROUTE: &str = "/console/modules";
const EVENTS_SUBSCRIBE_METHOD: &str = "mobkit/events/subscribe";

fn default_delivery_history_limit() -> usize {
    DELIVERY_HISTORY_LIMIT_DEFAULT
}

pub fn evaluate_schedules_at_tick(
    schedules: &[ScheduleDefinition],
    tick_ms: u64,
) -> Result<ScheduleEvaluation, ScheduleValidationError> {
    validate_schedule_tick_ms_supported(tick_ms)?;
    validate_schedules(schedules)?;
    let mut due_triggers = schedules
        .iter()
        .filter(|schedule| schedule.enabled)
        .filter_map(|schedule| {
            let canonical_schedule_id = canonical_schedule_id(&schedule.schedule_id);
            let interval =
                parse_schedule_interval(&schedule.interval).expect("validated schedule interval");
            let timezone =
                parse_schedule_timezone(&schedule.timezone).expect("validated schedule timezone");
            let due_tick_ms = latest_due_tick_at_or_before(
                &canonical_schedule_id,
                &interval,
                &timezone,
                schedule.jitter_ms,
                tick_ms,
            )?;
            if due_tick_ms != tick_ms {
                return None;
            }
            Some(ScheduleTrigger {
                schedule_id: canonical_schedule_id,
                interval: schedule.interval.clone(),
                timezone: schedule.timezone.clone(),
                due_tick_ms,
            })
        })
        .collect::<Vec<_>>();

    due_triggers.sort_by(|left, right| {
        left.due_tick_ms
            .cmp(&right.due_tick_ms)
            .then_with(|| left.schedule_id.cmp(&right.schedule_id))
            .then_with(|| left.interval.cmp(&right.interval))
            .then_with(|| left.timezone.cmp(&right.timezone))
    });

    Ok(ScheduleEvaluation {
        tick_ms,
        due_triggers,
    })
}

pub(crate) fn validate_schedules(
    schedules: &[ScheduleDefinition],
) -> Result<(), ScheduleValidationError> {
    let mut seen = BTreeSet::new();
    for schedule in schedules {
        let canonical_schedule_id = canonical_schedule_id(&schedule.schedule_id);
        if canonical_schedule_id.is_empty() {
            return Err(ScheduleValidationError::EmptyScheduleId);
        }
        if !seen.insert(canonical_schedule_id.clone()) {
            return Err(ScheduleValidationError::DuplicateScheduleId(
                canonical_schedule_id,
            ));
        }
        if parse_schedule_interval(&schedule.interval).is_none() {
            return Err(ScheduleValidationError::InvalidInterval {
                schedule_id: canonical_schedule_id.clone(),
                interval: schedule.interval.clone(),
            });
        }
        if parse_schedule_timezone(&schedule.timezone).is_none() {
            return Err(ScheduleValidationError::InvalidTimezone {
                schedule_id: canonical_schedule_id,
                timezone: schedule.timezone.clone(),
            });
        }
    }
    Ok(())
}

pub fn normalize_event_line(line: &str) -> Result<EventEnvelope<UnifiedEvent>, NormalizationError> {
    if let Ok(envelope) = parse_unified_event_line(line) {
        return enforce_source_consistency(envelope);
    }

    let value: Value = serde_json::from_str(line).map_err(|_| NormalizationError::InvalidJson)?;
    let object = value.as_object().ok_or(NormalizationError::InvalidSchema)?;

    let event_id = required_string(object.get("event_id"), "event_id")?;
    let source = required_string(object.get("source"), "source")?;
    let timestamp_ms = required_u64(object.get("timestamp_ms"), "timestamp_ms")?;

    if let Some(module) = object.get("module") {
        let module = required_string(Some(module), "module")?;
        let event_type = required_string(object.get("event_type"), "event_type")?;
        let payload = object
            .get("payload")
            .ok_or(NormalizationError::MissingField("payload"))?
            .clone();
        return enforce_source_consistency(EventEnvelope {
            event_id,
            source,
            timestamp_ms,
            event: UnifiedEvent::Module(ModuleEvent {
                module,
                event_type,
                payload,
            }),
        });
    }

    let agent_id = required_string(object.get("agent_id"), "agent_id")?;
    let event_type = required_string(object.get("event_type"), "event_type")?;

    enforce_source_consistency(EventEnvelope {
        event_id,
        source,
        timestamp_ms,
        event: UnifiedEvent::Agent {
            agent_id,
            event_type,
        },
    })
}

pub fn run_module_boundary_once(
    module: &ModuleConfig,
    pre_spawn: Option<&PreSpawnData>,
    timeout: Duration,
) -> Result<EventEnvelope<UnifiedEvent>, RuntimeBoundaryError> {
    run_module_boundary_with_env(module, pre_spawn, &[], timeout)
}

fn run_module_boundary_with_env(
    module: &ModuleConfig,
    pre_spawn: Option<&PreSpawnData>,
    extra_env: &[(String, String)],
    timeout: Duration,
) -> Result<EventEnvelope<UnifiedEvent>, RuntimeBoundaryError> {
    let env = pre_spawn
        .filter(|data| data.module_id == module.id)
        .map(|data| data.env.clone())
        .unwrap_or_default()
        .into_iter()
        .chain(extra_env.iter().cloned())
        .collect::<Vec<_>>();
    let line = run_process_json_line(&module.command, &module.args, &env, timeout)
        .map_err(RuntimeBoundaryError::Process)?;
    normalize_event_line(&line).map_err(RuntimeBoundaryError::Normalize)
}

pub fn run_discovered_module_once(
    config: &MobKitConfig,
    module_id: &str,
    timeout: Duration,
) -> Result<EventEnvelope<UnifiedEvent>, RuntimeFromConfigError> {
    let module = config
        .modules
        .iter()
        .find(|module| module.id == module_id)
        .ok_or_else(|| {
            RuntimeFromConfigError::Config(ConfigResolutionError::ModuleNotConfigured(
                module_id.to_string(),
            ))
        })?;

    if !config.discovery.modules.iter().any(|id| id == module_id) {
        return Err(RuntimeFromConfigError::Config(
            ConfigResolutionError::ModuleNotDiscovered(module_id.to_string()),
        ));
    }

    let pre_spawn = config
        .pre_spawn
        .iter()
        .find(|data| data.module_id == module_id);
    run_module_boundary_once(module, pre_spawn, timeout).map_err(RuntimeFromConfigError::Runtime)
}

pub fn run_rpc_capabilities_boundary_once(
    command: &str,
    args: &[String],
    env: &[(String, String)],
    timeout: Duration,
) -> Result<RpcCapabilities, RpcRuntimeError> {
    let line =
        run_process_json_line(command, args, env, timeout).map_err(RpcRuntimeError::Process)?;
    parse_rpc_capabilities(&line).map_err(RpcRuntimeError::Capabilities)
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
    pub fn new(
        bq_binary: impl AsRef<Path>,
        dataset: impl Into<String>,
        table: impl Into<String>,
    ) -> Self {
        Self {
            bq_binary: bq_binary.as_ref().to_path_buf(),
            dataset: dataset.into(),
            table: table.into(),
            project_id: None,
        }
    }

    pub fn with_project_id(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }

    pub fn table_ref(&self) -> String {
        format!("{}.{}", self.dataset, self.table)
    }

    pub fn stream_insert_rows(
        &self,
        rows: &[SessionPersistenceRow],
    ) -> Result<(), BigQuerySessionStoreError> {
        let mut command = Command::new(&self.bq_binary);
        command.arg("insert").arg(self.table_ref());
        if let Some(project) = &self.project_id {
            command.arg(format!("--project_id={project}"));
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|err| BigQuerySessionStoreError::Io(err.to_string()))?;

        if let Some(stdin) = child.stdin.as_mut() {
            for row in rows {
                let line = serde_json::to_string(row)
                    .map_err(|err| BigQuerySessionStoreError::Serialize(err.to_string()))?;
                stdin
                    .write_all(line.as_bytes())
                    .map_err(|err| BigQuerySessionStoreError::Io(err.to_string()))?;
                stdin
                    .write_all(b"\n")
                    .map_err(|err| BigQuerySessionStoreError::Io(err.to_string()))?;
            }
        } else {
            return Err(BigQuerySessionStoreError::Io(
                "missing stdin for bq insert command".to_string(),
            ));
        }

        let output = child
            .wait_with_output()
            .map_err(|err| BigQuerySessionStoreError::Io(err.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(BigQuerySessionStoreError::ProcessFailed {
                command: self.bq_binary.display().to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        }
    }

    pub fn read_rows(&self) -> Result<Vec<SessionPersistenceRow>, BigQuerySessionStoreError> {
        let mut command = Command::new(&self.bq_binary);
        command
            .arg("query")
            .arg("--nouse_legacy_sql")
            .arg("--format=json")
            .arg(format!(
                "SELECT session_id, updated_at_ms, deleted, payload FROM `{}` ORDER BY updated_at_ms ASC",
                self.table_ref()
            ));
        if let Some(project) = &self.project_id {
            command.arg(format!("--project_id={project}"));
        }

        let output = command
            .output()
            .map_err(|err| BigQuerySessionStoreError::Io(err.to_string()))?;
        if !output.status.success() {
            return Err(BigQuerySessionStoreError::ProcessFailed {
                command: self.bq_binary.display().to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        serde_json::from_slice::<Vec<SessionPersistenceRow>>(&output.stdout)
            .map_err(|err| BigQuerySessionStoreError::InvalidQueryResponse(err.to_string()))
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
}

impl ElephantMemoryStoreAdapter {
    fn from_config(config: &ElephantMemoryBackendConfig) -> Result<Self, ElephantMemoryStoreError> {
        let endpoint = config.endpoint.trim();
        if endpoint.is_empty() {
            return Err(ElephantMemoryStoreError::InvalidConfig(
                "memory backend endpoint must not be empty".to_string(),
            ));
        }
        let state_path = config.state_path.trim();
        if state_path.is_empty() {
            return Err(ElephantMemoryStoreError::InvalidConfig(
                "memory backend state_path must not be empty".to_string(),
            ));
        }
        Ok(Self {
            endpoint: endpoint.to_string(),
            state_path: PathBuf::from(state_path),
        })
    }

    fn ensure_remote_health(&self) -> Result<(), ElephantMemoryStoreError> {
        let health_url = format!("{}/v1/health", self.endpoint.trim_end_matches('/'));
        let parsed = parse_http_url(&health_url)?;
        let authority = format!("{}:{}", parsed.host, parsed.port);
        let mut addrs = authority.to_socket_addrs().map_err(|err| {
            ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck resolve failed for '{health_url}': {err}"
            ))
        })?;
        let addr = addrs.next().ok_or_else(|| {
            ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck resolve failed for '{health_url}': no socket addresses"
            ))
        })?;
        let mut stream = TcpStream::connect_timeout(&addr, ELEPHANT_HEALTHCHECK_TIMEOUT).map_err(
            |err| {
                ElephantMemoryStoreError::ExternalCallFailed(format!(
                    "healthcheck connect failed for '{health_url}': {err}"
                ))
            },
        )?;
        stream
            .set_read_timeout(Some(ELEPHANT_HEALTHCHECK_TIMEOUT))
            .map_err(|err| {
                ElephantMemoryStoreError::ExternalCallFailed(format!(
                    "healthcheck timeout setup failed for '{health_url}': {err}"
                ))
            })?;
        stream
            .set_write_timeout(Some(ELEPHANT_HEALTHCHECK_TIMEOUT))
            .map_err(|err| {
                ElephantMemoryStoreError::ExternalCallFailed(format!(
                    "healthcheck timeout setup failed for '{health_url}': {err}"
                ))
            })?;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            parsed.path, parsed.host
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|err| ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck write failed for '{health_url}': {err}"
            )))?;
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        let bytes_read = reader.read_line(&mut status_line).map_err(|err| {
            ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck read failed for '{health_url}': {err}"
            ))
        })?;
        if bytes_read == 0 {
            return Err(ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck read failed for '{health_url}': empty response"
            )));
        }
        let status_code = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or_else(|| {
                ElephantMemoryStoreError::ExternalCallFailed(format!(
                    "healthcheck parse failed for '{health_url}': invalid status line '{}'",
                    status_line.trim()
                ))
            })?;
        if (200..300).contains(&status_code) {
            Ok(())
        } else {
            Err(ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck status failed for '{health_url}': HTTP {status_code}"
            )))
        }
    }

    fn read_state(&self) -> Result<PersistedMemoryState, ElephantMemoryStoreError> {
        self.ensure_remote_health()?;
        if !self.state_path.exists() {
            return Ok(PersistedMemoryState::default());
        }
        let bytes = fs::read(&self.state_path)
            .map_err(|err| ElephantMemoryStoreError::Io(err.to_string()))?;
        serde_json::from_slice::<PersistedMemoryState>(&bytes)
            .map_err(|err| ElephantMemoryStoreError::InvalidStoreData(err.to_string()))
    }

    fn write_state(&self, state: &PersistedMemoryState) -> Result<(), ElephantMemoryStoreError> {
        self.ensure_remote_health()?;
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| ElephantMemoryStoreError::Io(err.to_string()))?;
        }
        let tmp_path = self.state_path.with_extension("tmp");
        let json = serde_json::to_vec_pretty(state)
            .map_err(|err| ElephantMemoryStoreError::Serialize(err.to_string()))?;
        fs::write(&tmp_path, json).map_err(|err| ElephantMemoryStoreError::Io(err.to_string()))?;
        fs::rename(&tmp_path, &self.state_path)
            .map_err(|err| ElephantMemoryStoreError::Io(err.to_string()))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedHttpUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_http_url(url: &str) -> Result<ParsedHttpUrl, ElephantMemoryStoreError> {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .ok_or_else(|| {
            ElephantMemoryStoreError::InvalidConfig(format!(
                "memory backend endpoint must start with http:// (got '{trimmed}')"
            ))
        })?;
    if without_scheme.is_empty() {
        return Err(ElephantMemoryStoreError::InvalidConfig(
            "memory backend endpoint host must not be empty".to_string(),
        ));
    }
    let (authority, path_suffix) = without_scheme
        .split_once('/')
        .map(|(left, right)| (left, format!("/{right}")))
        .unwrap_or((without_scheme, "/".to_string()));
    if authority.is_empty() {
        return Err(ElephantMemoryStoreError::InvalidConfig(
            "memory backend endpoint host must not be empty".to_string(),
        ));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, raw_port)) if !host.is_empty() && raw_port.chars().all(|c| c.is_ascii_digit()) => {
            let parsed = raw_port.parse::<u16>().map_err(|_| {
                ElephantMemoryStoreError::InvalidConfig(format!(
                    "memory backend endpoint port is invalid in '{trimmed}'"
                ))
            })?;
            (host.to_string(), parsed)
        }
        _ => (authority.to_string(), 80_u16),
    };
    if host.is_empty() {
        return Err(ElephantMemoryStoreError::InvalidConfig(
            "memory backend endpoint host must not be empty".to_string(),
        ));
    }
    Ok(ParsedHttpUrl {
        host,
        port,
        path: path_suffix,
    })
}

impl Drop for JsonFileLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

pub fn start_mobkit_runtime(
    config: MobKitConfig,
    agent_events: Vec<EventEnvelope<UnifiedEvent>>,
    timeout: Duration,
) -> Result<MobkitRuntimeHandle, MobkitRuntimeError> {
    start_mobkit_runtime_with_options(config, agent_events, timeout, RuntimeOptions::default())
}

pub fn start_mobkit_runtime_with_options(
    config: MobKitConfig,
    agent_events: Vec<EventEnvelope<UnifiedEvent>>,
    timeout: Duration,
    options: RuntimeOptions,
) -> Result<MobkitRuntimeHandle, MobkitRuntimeError> {
    let delivery_runtime_epoch_ms = current_time_ms();
    let mut lifecycle_events = Vec::new();
    let mut seq = 0_u64;
    lifecycle_events.push(LifecycleEvent {
        seq,
        stage: LifecycleStage::MobStarted,
    });
    seq += 1;

    let mut supervisor_transitions = Vec::new();
    let mut module_events = Vec::new();
    let mut loaded_modules = BTreeSet::new();
    let mut live_children = BTreeMap::new();

    for module_id in &config.discovery.modules {
        let module = config
            .modules
            .iter()
            .find(|module| &module.id == module_id)
            .ok_or_else(|| {
                MobkitRuntimeError::Config(ConfigResolutionError::ModuleNotConfigured(
                    module_id.clone(),
                ))
            })?;

        let pre_spawn = config
            .pre_spawn
            .iter()
            .find(|data| data.module_id == *module_id);

        let (event, child, mut transitions) =
            supervise_module_start(module, pre_spawn, timeout, &options);
        supervisor_transitions.append(&mut transitions);
        if let (Some(event), Some(child)) = (event, child) {
            loaded_modules.insert(module_id.clone());
            live_children.insert(module_id.clone(), child);
            module_events.push(event);
        }
    }

    lifecycle_events.push(LifecycleEvent {
        seq,
        stage: LifecycleStage::ModulesStarted,
    });
    seq += 1;

    let merged_events = merge_unified_events(module_events, agent_events);
    lifecycle_events.push(LifecycleEvent {
        seq,
        stage: LifecycleStage::MergedStreamStarted,
    });

    let memory_backend = match options.memory_backend.as_ref() {
        Some(MemoryBackendConfig::Elephant(config)) => Some(
            ElephantMemoryStoreAdapter::from_config(config)
                .map_err(MobkitRuntimeError::MemoryBackend)?,
        ),
        None => None,
    };
    let persisted_memory = match memory_backend.as_ref() {
        Some(backend) => backend
            .read_state()
            .map_err(MobkitRuntimeError::MemoryBackend)?,
        None => PersistedMemoryState::default(),
    };
    let mut memory_assertions = persisted_memory
        .assertions
        .into_iter()
        .filter_map(|assertion| {
            let entity = MobkitRuntimeHandle::canonical_memory_token(&assertion.entity)?;
            let topic = MobkitRuntimeHandle::canonical_memory_token(&assertion.topic)?;
            let store = MobkitRuntimeHandle::canonical_memory_store(&assertion.store)?;
            let fact = assertion.fact.trim();
            if fact.is_empty() {
                return None;
            }
            Some(MemoryAssertion {
                assertion_id: assertion.assertion_id,
                entity,
                topic,
                store,
                fact: fact.to_string(),
                metadata: assertion.metadata,
                indexed_at_ms: assertion.indexed_at_ms,
            })
        })
        .collect::<Vec<_>>();
    while memory_assertions.len() > MEMORY_ASSERTIONS_MAX_RETAINED {
        memory_assertions.remove(0);
    }
    let mut memory_conflicts = BTreeMap::new();
    for signal in persisted_memory.conflicts {
        let Some(entity) = MobkitRuntimeHandle::canonical_memory_token(&signal.entity) else {
            continue;
        };
        let Some(topic) = MobkitRuntimeHandle::canonical_memory_token(&signal.topic) else {
            continue;
        };
        let Some(store) = MobkitRuntimeHandle::canonical_memory_store(&signal.store) else {
            continue;
        };
        let normalized_signal = MemoryConflictSignal {
            entity: entity.clone(),
            topic: topic.clone(),
            store: store.clone(),
            reason: signal
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            updated_at_ms: signal.updated_at_ms,
        };
        memory_conflicts.insert(
            MemoryConflictKey {
                entity,
                topic,
                store,
            },
            normalized_signal,
        );
    }
    let memory_sequence = memory_assertions
        .iter()
        .filter_map(|assertion| parse_memory_assertion_sequence(&assertion.assertion_id))
        .max()
        .map(|last_sequence| last_sequence.saturating_add(1))
        .unwrap_or(memory_assertions.len() as u64);

    Ok(MobkitRuntimeHandle {
        config,
        loaded_modules,
        live_children,
        lifecycle_events,
        supervisor_report: SupervisorReport {
            transitions: supervisor_transitions,
        },
        merged_events,
        scheduling_claims: BTreeSet::new(),
        scheduling_claim_ticks: BTreeMap::new(),
        scheduling_last_due_ticks: BTreeMap::new(),
        scheduling_dispatch_sequence: 0,
        routing_sequence: 0,
        routing_resolutions: BTreeMap::new(),
        routing_resolution_order: Vec::new(),
        runtime_routes: BTreeMap::new(),
        delivery_sequence: 0,
        delivery_runtime_epoch_ms,
        delivery_now_floor_ms: 0,
        delivery_clock_ms: 0,
        delivery_history: Vec::new(),
        delivery_idempotency: BTreeMap::new(),
        delivery_idempotency_by_delivery: BTreeMap::new(),
        delivery_rate_window_counts: BTreeMap::new(),
        gating_sequence: 0,
        gating_pending: BTreeMap::new(),
        gating_pending_order: Vec::new(),
        gating_audit: Vec::new(),
        memory_sequence,
        memory_assertions,
        memory_conflicts,
        memory_backend,
        running: true,
    })
}

fn parse_memory_assertion_sequence(assertion_id: &str) -> Option<u64> {
    assertion_id
        .strip_prefix("memory-assert-")
        .and_then(|suffix| suffix.parse::<u64>().ok())
}

pub fn route_module_call(
    runtime: &MobkitRuntimeHandle,
    request: &ModuleRouteRequest,
    timeout: Duration,
) -> Result<ModuleRouteResponse, ModuleRouteError> {
    if !runtime.loaded_modules.contains(&request.module_id) {
        return Err(ModuleRouteError::UnloadedModule(request.module_id.clone()));
    }

    let module = runtime
        .config
        .modules
        .iter()
        .find(|module| module.id == request.module_id)
        .ok_or_else(|| ModuleRouteError::UnloadedModule(request.module_id.clone()))?;
    let pre_spawn = runtime
        .config
        .pre_spawn
        .iter()
        .find(|data| data.module_id == request.module_id);

    let envelope = run_module_boundary_once(module, pre_spawn, timeout)
        .map_err(ModuleRouteError::ModuleRuntime)?;

    match envelope.event {
        UnifiedEvent::Module(event) if event.module == request.module_id => {
            Ok(ModuleRouteResponse {
                module_id: request.module_id.clone(),
                method: request.method.clone(),
                payload: event.payload,
            })
        }
        _ => Err(ModuleRouteError::UnexpectedRouteResponse),
    }
}

pub fn route_module_call_rpc_json(
    runtime: &MobkitRuntimeHandle,
    request_json: &str,
    timeout: Duration,
) -> Result<String, RpcRouteError> {
    let request: ModuleRouteRequest =
        serde_json::from_str(request_json).map_err(|_| RpcRouteError::InvalidRequest)?;
    let response = route_module_call(runtime, &request, timeout).map_err(RpcRouteError::Route)?;
    serde_json::to_string(&response).map_err(|_| RpcRouteError::InvalidResponse)
}

pub fn route_module_call_rpc_subprocess(
    runtime: &MobkitRuntimeHandle,
    command: &str,
    args: &[String],
    env: &[(String, String)],
    timeout: Duration,
) -> Result<String, RpcRouteError> {
    let request_json = run_process_json_line(command, args, env, timeout)
        .map_err(RpcRouteError::BoundaryProcess)?;
    route_module_call_rpc_json(runtime, &request_json, timeout)
}

pub fn handle_console_rest_json_route(
    decisions: &RuntimeDecisionState,
    request: &ConsoleRestJsonRequest,
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
        Err(reason) => {
            return ConsoleRestJsonResponse {
                status: 401,
                body: serde_json::json!({
                    "error":"unauthorized",
                    "reason": reason,
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
    let body = if base_path == CONSOLE_EXPERIENCE_ROUTE {
        build_console_experience_contract(&modules)
    } else {
        serde_json::json!({ "modules": modules })
    };
    ConsoleRestJsonResponse { status: 200, body }
}

fn build_console_experience_contract(modules: &[String]) -> Value {
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
        "activity_feed": {
            "transport": "sse",
            "source_method": EVENTS_SUBSCRIBE_METHOD,
            "supported_scopes": ["mob", "agent", "interaction"],
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
) -> Result<Option<ConsoleAccessRequest>, &'static str> {
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
) -> Result<ConsoleAccessRequest, &'static str> {
    if decisions.trusted_oidc.audience.trim().is_empty() {
        return Err("invalid_trusted_oidc_config");
    }

    let discovery = parse_oidc_discovery_json(&decisions.trusted_oidc.discovery_json)
        .map_err(|_| "invalid_trusted_oidc_config")?;
    let jwks = parse_jwks_json(&decisions.trusted_oidc.jwks_json)
        .map_err(|_| "invalid_trusted_oidc_config")?;
    let header = inspect_jwt_header(token).map_err(|_| "invalid_token_header")?;
    let key = select_jwk_for_token(&jwks, header.kid.as_deref(), &header.alg)
        .map_err(|_| "jwks_key_not_found")?;
    let shared_secret =
        extract_hs256_shared_secret(key).map_err(|_| "invalid_jwks_key_material")?;

    let now_epoch_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let claims = validate_jwt_locally(
        token,
        &JwtValidationConfig {
            shared_secret,
            issuer: Some(discovery.issuer),
            audience: Some(decisions.trusted_oidc.audience.clone()),
            now_epoch_seconds,
            leeway_seconds: 30,
        },
    )
    .map_err(|_| "invalid_token")?;

    let principal = claims
        .email
        .or(claims.subject)
        .ok_or("missing_token_identity")?;
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

impl MobkitRuntimeHandle {
    fn refresh_delivery_clocks_from_now(&mut self) {
        let observed_now_ms = current_time_ms().saturating_sub(self.delivery_runtime_epoch_ms);
        self.delivery_now_floor_ms = self.delivery_now_floor_ms.max(observed_now_ms);
        self.delivery_clock_ms = self.delivery_clock_ms.max(self.delivery_now_floor_ms);
    }

    fn next_route_resolved_timestamp_ms(&mut self) -> u64 {
        self.refresh_delivery_clocks_from_now();
        let latest_merged_timestamp_ms = self
            .merged_events
            .last()
            .map_or(0, |event| event.timestamp_ms);
        let timestamp_ms = self.delivery_clock_ms.max(latest_merged_timestamp_ms);
        self.delivery_clock_ms = timestamp_ms;
        timestamp_ms
    }

    fn scoped_idempotency_key(route_id: &str, idempotency_key: &str) -> String {
        format!("{route_id}:{idempotency_key}")
    }

    fn remember_routing_resolution(&mut self, resolution: RoutingResolution) {
        let route_id = resolution.route_id.clone();
        self.routing_resolutions
            .insert(route_id.clone(), resolution);
        self.routing_resolution_order.push(route_id);
        while self.routing_resolution_order.len() > ROUTING_RESOLUTION_LIMIT_MAX {
            let oldest_route_id = self.routing_resolution_order.remove(0);
            self.routing_resolutions.remove(&oldest_route_id);
        }
    }

    fn trusted_resolution_for_delivery(
        &self,
        provided: &RoutingResolution,
    ) -> Result<RoutingResolution, DeliverySendError> {
        let route_id = provided.route_id.trim();
        if route_id.is_empty() {
            return Err(DeliverySendError::InvalidRouteId);
        }
        let Some(trusted) = self.routing_resolutions.get(route_id) else {
            return Err(DeliverySendError::UnknownRouteId(route_id.to_string()));
        };
        if trusted != provided {
            return Err(DeliverySendError::ForgedResolution);
        }
        Ok(trusted.clone())
    }

    fn prune_delivery_rate_window_counts(&mut self, current_window_start_ms: u64) {
        let earliest_window_start_ms = current_window_start_ms.saturating_sub(
            DELIVERY_RATE_WINDOW_MS.saturating_mul(DELIVERY_RATE_WINDOWS_RETAINED - 1),
        );
        self.delivery_rate_window_counts
            .retain(|key, _| key.window_start_ms >= earliest_window_start_ms);
    }

    fn next_routing_sequence(&mut self) -> u64 {
        let sequence = self.routing_sequence;
        self.routing_sequence = self.routing_sequence.saturating_add(1);
        sequence
    }

    fn next_delivery_sequence(&mut self) -> u64 {
        let sequence = self.delivery_sequence;
        self.delivery_sequence = self.delivery_sequence.saturating_add(1);
        sequence
    }

    fn next_gating_sequence(&mut self) -> u64 {
        let sequence = self.gating_sequence;
        self.gating_sequence = self.gating_sequence.saturating_add(1);
        sequence
    }

    fn next_memory_sequence(&mut self) -> u64 {
        let sequence = self.memory_sequence;
        self.memory_sequence = self.memory_sequence.saturating_add(1);
        sequence
    }

    fn canonical_memory_token(raw: &str) -> Option<String> {
        let token = raw.trim().to_ascii_lowercase();
        if token.is_empty() {
            None
        } else {
            Some(token)
        }
    }

    fn canonical_memory_store(raw: &str) -> Option<String> {
        let store = Self::canonical_memory_token(raw)?;
        if MEMORY_SUPPORTED_STORES.contains(&store.as_str()) {
            Some(store)
        } else {
            None
        }
    }

    fn default_memory_store() -> String {
        "knowledge_graph".to_string()
    }

    fn memory_conflict_for_reference(
        &self,
        entity: Option<&str>,
        topic: Option<&str>,
    ) -> Option<MemoryConflictSignal> {
        let canonical_entity = entity.and_then(Self::canonical_memory_token);
        let canonical_topic = topic.and_then(Self::canonical_memory_token);
        match (canonical_entity, canonical_topic) {
            (Some(entity), Some(topic)) => self
                .memory_conflicts
                .values()
                .find(|signal| signal.entity == entity && signal.topic == topic)
                .cloned(),
            (Some(entity), None) => self
                .memory_conflicts
                .values()
                .find(|signal| signal.entity == entity)
                .cloned(),
            (None, Some(topic)) => self
                .memory_conflicts
                .values()
                .find(|signal| signal.topic == topic)
                .cloned(),
            (None, None) => None,
        }
    }

    fn append_gating_audit(&mut self, mut entry: GatingAuditEntry) {
        let audit_sequence = self.next_gating_sequence();
        entry.audit_id = format!("gate-audit-{audit_sequence:06}");
        entry.timestamp_ms = current_time_ms();
        self.gating_audit.push(entry);
        while self.gating_audit.len() > GATING_AUDIT_MAX_RETAINED {
            self.gating_audit.remove(0);
        }
    }

    fn refresh_gating_timeouts(&mut self) {
        let now_ms = current_time_ms();
        let expired = self
            .gating_pending
            .iter()
            .filter(|(_, entry)| now_ms >= entry.deadline_at_ms)
            .map(|(pending_id, _)| pending_id.clone())
            .collect::<Vec<_>>();
        for pending_id in expired {
            if let Some(expired_entry) = self.gating_pending.remove(&pending_id) {
                self.gating_pending_order
                    .retain(|candidate| candidate != &pending_id);
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "timeout_fallback".to_string(),
                    action_id: expired_entry.action_id.clone(),
                    pending_id: Some(pending_id),
                    actor_id: expired_entry.actor_id,
                    risk_tier: expired_entry.risk_tier,
                    outcome: GatingOutcome::SafeDraft,
                    detail: serde_json::json!({
                        "fallback": "safe_draft",
                        "reason": "approval_timeout"
                    }),
                });
            }
        }
    }

    fn upsert_gating_pending_entry(&mut self, entry: GatingPendingEntry) {
        let pending_id = entry.pending_id.clone();
        self.gating_pending.insert(pending_id.clone(), entry);
        self.gating_pending_order
            .retain(|candidate| candidate != &pending_id);
        self.gating_pending_order.push(pending_id);
        while self.gating_pending_order.len() > GATING_PENDING_MAX_RETAINED {
            let oldest = self.gating_pending_order.remove(0);
            self.gating_pending.remove(&oldest);
        }
    }

    pub fn memory_stores(&self) -> Vec<MemoryStoreInfo> {
        MEMORY_SUPPORTED_STORES
            .iter()
            .map(|store| MemoryStoreInfo {
                store: (*store).to_string(),
                record_count: self
                    .memory_assertions
                    .iter()
                    .filter(|assertion| assertion.store == *store)
                    .count()
                    + self
                        .memory_conflicts
                        .values()
                        .filter(|signal| signal.store == *store)
                        .count(),
            })
            .collect()
    }

    fn persist_memory_state(&self) -> Result<(), MemoryIndexError> {
        let Some(backend) = self.memory_backend.as_ref() else {
            return Ok(());
        };
        let state = PersistedMemoryState {
            assertions: self.memory_assertions.clone(),
            conflicts: self.memory_conflicts.values().cloned().collect::<Vec<_>>(),
        };
        backend
            .write_state(&state)
            .map_err(MemoryIndexError::BackendPersistFailed)
    }

    pub fn memory_index(
        &mut self,
        request: MemoryIndexRequest,
    ) -> Result<MemoryIndexResult, MemoryIndexError> {
        let entity = Self::canonical_memory_token(&request.entity)
            .ok_or(MemoryIndexError::EntityRequired)?;
        let topic =
            Self::canonical_memory_token(&request.topic).ok_or(MemoryIndexError::TopicRequired)?;
        let store = match request.store.as_deref() {
            None => Self::default_memory_store(),
            Some(raw_store) => Self::canonical_memory_store(raw_store)
                .ok_or_else(|| MemoryIndexError::UnsupportedStore(raw_store.trim().to_string()))?,
        };
        let fact = request
            .fact
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let conflict = request.conflict.unwrap_or(false);
        if fact.is_none() && !conflict {
            return Err(MemoryIndexError::FactRequiredWhenConflictUnset);
        }

        let previous_memory_assertions = self.memory_assertions.clone();
        let previous_memory_conflicts = self.memory_conflicts.clone();
        let previous_memory_sequence = self.memory_sequence;

        let mut assertion_id = None;
        if let Some(fact) = fact {
            let assertion_sequence = self.next_memory_sequence();
            let assertion = MemoryAssertion {
                assertion_id: format!("memory-assert-{assertion_sequence:06}"),
                entity: entity.clone(),
                topic: topic.clone(),
                store: store.clone(),
                fact,
                metadata: request.metadata.clone(),
                indexed_at_ms: current_time_ms(),
            };
            assertion_id = Some(assertion.assertion_id.clone());
            self.memory_assertions.push(assertion);
            while self.memory_assertions.len() > MEMORY_ASSERTIONS_MAX_RETAINED {
                self.memory_assertions.remove(0);
            }
        }

        if conflict {
            let conflict_key = MemoryConflictKey {
                entity: entity.clone(),
                topic: topic.clone(),
                store: store.clone(),
            };
            self.memory_conflicts.insert(
                conflict_key,
                MemoryConflictSignal {
                    entity: entity.clone(),
                    topic: topic.clone(),
                    store: store.clone(),
                    reason: request
                        .conflict_reason
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string),
                    updated_at_ms: current_time_ms(),
                },
            );
        }
        if let Err(error) = self.persist_memory_state() {
            self.memory_assertions = previous_memory_assertions;
            self.memory_conflicts = previous_memory_conflicts;
            self.memory_sequence = previous_memory_sequence;
            return Err(error);
        }

        let conflict_active = self
            .memory_conflict_for_reference(Some(entity.as_str()), Some(topic.as_str()))
            .is_some();

        Ok(MemoryIndexResult {
            entity,
            topic,
            store,
            assertion_id,
            conflict_active,
        })
    }

    pub fn memory_query(&self, request: MemoryQueryRequest) -> MemoryQueryResult {
        let entity = request
            .entity
            .as_deref()
            .and_then(Self::canonical_memory_token);
        let topic = request
            .topic
            .as_deref()
            .and_then(Self::canonical_memory_token);
        let store = request
            .store
            .as_deref()
            .and_then(Self::canonical_memory_store);
        let assertions = self
            .memory_assertions
            .iter()
            .filter(|assertion| {
                entity
                    .as_ref()
                    .is_none_or(|value| assertion.entity.as_str() == value.as_str())
            })
            .filter(|assertion| {
                topic
                    .as_ref()
                    .is_none_or(|value| assertion.topic.as_str() == value.as_str())
            })
            .filter(|assertion| {
                store
                    .as_ref()
                    .is_none_or(|value| assertion.store.as_str() == value.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        let conflicts = self
            .memory_conflicts
            .values()
            .filter(|signal| {
                entity
                    .as_ref()
                    .is_none_or(|value| signal.entity.as_str() == value.as_str())
            })
            .filter(|signal| {
                topic
                    .as_ref()
                    .is_none_or(|value| signal.topic.as_str() == value.as_str())
            })
            .filter(|signal| {
                store
                    .as_ref()
                    .is_none_or(|value| signal.store.as_str() == value.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        MemoryQueryResult {
            assertions,
            conflicts,
        }
    }

    fn replay_delivery_for_scoped_idempotency(
        &mut self,
        provided_resolution: &RoutingResolution,
        idempotency_key: &str,
        payload: &Value,
    ) -> Result<Option<DeliveryRecord>, DeliverySendError> {
        let route_id = provided_resolution.route_id.trim();
        let scoped_key = Self::scoped_idempotency_key(route_id, idempotency_key);
        let Some(entry) = self.delivery_idempotency.get(&scoped_key).cloned() else {
            return Ok(None);
        };

        if entry.payload != *payload {
            return Err(DeliverySendError::IdempotencyPayloadMismatch);
        }
        if let Some(trusted_resolution) = self.routing_resolutions.get(route_id) {
            if trusted_resolution != provided_resolution {
                return Err(DeliverySendError::ForgedResolution);
            }
        } else if entry.canonical_resolution != *provided_resolution {
            return Err(DeliverySendError::ForgedResolution);
        }
        if let Some(existing) = self
            .delivery_history
            .iter()
            .find(|record| record.delivery_id == entry.delivery_id)
        {
            return Ok(Some(existing.clone()));
        }

        self.delivery_idempotency.remove(&scoped_key);
        let mut remove_reverse_index = false;
        if let Some(scoped_keys) = self
            .delivery_idempotency_by_delivery
            .get_mut(&entry.delivery_id)
        {
            scoped_keys.retain(|candidate| candidate != &scoped_key);
            remove_reverse_index = scoped_keys.is_empty();
        }
        if remove_reverse_index {
            self.delivery_idempotency_by_delivery
                .remove(&entry.delivery_id);
        }

        Ok(None)
    }

    fn is_module_loaded(&self, module_id: &str) -> bool {
        self.loaded_modules.contains(module_id)
    }

    fn next_scheduling_dispatch_sequence(&mut self) -> u64 {
        let sequence = self.scheduling_dispatch_sequence;
        self.scheduling_dispatch_sequence = self.scheduling_dispatch_sequence.saturating_add(1);
        sequence
    }

    pub fn shutdown(&mut self) -> RuntimeShutdownReport {
        let mut seq = self
            .lifecycle_events
            .last()
            .map_or(0, |event| event.seq + 1);
        self.lifecycle_events.push(LifecycleEvent {
            seq,
            stage: LifecycleStage::ShutdownRequested,
        });
        seq += 1;
        self.lifecycle_events.push(LifecycleEvent {
            seq,
            stage: LifecycleStage::ShutdownComplete,
        });
        self.running = false;

        let terminated_modules: Vec<String> = self.loaded_modules.iter().cloned().collect();
        self.loaded_modules.clear();

        let mut orphan_processes = 0_u32;
        let children = std::mem::take(&mut self.live_children);
        for (_, mut child) in children {
            if !terminate_child(&mut child) {
                orphan_processes += 1;
            }
        }

        RuntimeShutdownReport {
            terminated_modules,
            orphan_processes,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn loaded_modules(&self) -> Vec<String> {
        self.loaded_modules.iter().cloned().collect()
    }

    fn module_and_prespawn(
        &self,
        module_id: &str,
    ) -> Option<(&ModuleConfig, Option<&PreSpawnData>)> {
        let module = self
            .config
            .modules
            .iter()
            .find(|module| module.id == module_id)?;
        let pre_spawn = self
            .config
            .pre_spawn
            .iter()
            .find(|data| data.module_id == module_id);
        Some((module, pre_spawn))
    }

    fn parse_router_boundary_overrides(
        envelope: &EventEnvelope<UnifiedEvent>,
    ) -> RouterBoundaryOverrides {
        let mut overrides = RouterBoundaryOverrides::default();
        let UnifiedEvent::Module(event) = &envelope.event else {
            return overrides;
        };
        if event.module != "router" {
            return overrides;
        }
        let Some(payload) = event.payload.as_object() else {
            return overrides;
        };

        if let Some(channel) = payload.get("channel").and_then(Value::as_str) {
            let channel = channel.trim();
            if !channel.is_empty() {
                overrides.channel = Some(channel.to_string());
            }
        }
        if let Some(sink) = payload.get("sink").and_then(Value::as_str) {
            let sink = sink.trim();
            if !sink.is_empty() {
                overrides.sink = Some(sink.to_string());
            }
        }
        if let Some(target_module) = payload.get("target_module").and_then(Value::as_str) {
            let target_module = target_module.trim();
            if !target_module.is_empty() {
                overrides.target_module = Some(target_module.to_string());
            }
        }
        if let Some(retry_max) = payload
            .get("retry_max")
            .and_then(Value::as_u64)
            .and_then(|raw| u32::try_from(raw).ok())
        {
            overrides.retry_max = Some(retry_max);
        }
        if let Some(backoff_ms) = payload.get("backoff_ms").and_then(Value::as_u64) {
            overrides.backoff_ms = Some(backoff_ms);
        }
        if let Some(rate_limit_per_minute) = payload
            .get("rate_limit_per_minute")
            .and_then(Value::as_u64)
            .and_then(|raw| u32::try_from(raw).ok())
        {
            overrides.rate_limit_per_minute = Some(rate_limit_per_minute);
        }

        overrides
    }

    fn parse_delivery_boundary_outcome(
        envelope: &EventEnvelope<UnifiedEvent>,
    ) -> DeliveryBoundaryOutcome {
        let mut outcome = DeliveryBoundaryOutcome::default();
        let UnifiedEvent::Module(event) = &envelope.event else {
            return outcome;
        };
        if event.module != "delivery" {
            return outcome;
        }
        let Some(payload) = event.payload.as_object() else {
            return outcome;
        };
        if let Some(adapter) = payload.get("adapter").and_then(Value::as_str) {
            let adapter = adapter.trim();
            if !adapter.is_empty() {
                outcome.sink_adapter = Some(adapter.to_string());
            }
        }
        outcome.force_fail = payload
            .get("force_fail")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        outcome
    }

    fn matching_runtime_route(&self, recipient: &str, channel: &str) -> Option<&RuntimeRoute> {
        self.runtime_routes.values().find(|route| {
            if route.recipient != recipient {
                return false;
            }
            route
                .channel
                .as_deref()
                .is_none_or(|candidate| candidate == channel)
        })
    }

    pub fn list_runtime_routes(&self) -> Vec<RuntimeRoute> {
        self.runtime_routes.values().cloned().collect()
    }

    pub fn add_runtime_route(
        &mut self,
        route: RuntimeRoute,
    ) -> Result<RuntimeRoute, RuntimeRouteMutationError> {
        let route_key = route.route_key.trim();
        if route_key.is_empty() {
            return Err(RuntimeRouteMutationError::EmptyRouteKey);
        }
        let recipient = route.recipient.trim();
        if recipient.is_empty() {
            return Err(RuntimeRouteMutationError::EmptyRecipient);
        }
        if route
            .channel
            .as_ref()
            .is_some_and(|channel| channel.trim().is_empty())
        {
            return Err(RuntimeRouteMutationError::InvalidChannel);
        }
        if route.sink.trim().is_empty() {
            return Err(RuntimeRouteMutationError::EmptySink);
        }
        if route.target_module.trim().is_empty() {
            return Err(RuntimeRouteMutationError::EmptyTargetModule);
        }
        if route
            .retry_max
            .is_some_and(|retry_max| retry_max > ROUTING_RETRY_MAX_CAP)
        {
            return Err(RuntimeRouteMutationError::RetryMaxExceedsCap {
                provided: route.retry_max.unwrap_or_default(),
                cap: ROUTING_RETRY_MAX_CAP,
            });
        }
        if route.rate_limit_per_minute == Some(0) {
            return Err(RuntimeRouteMutationError::InvalidRateLimitPerMinute);
        }

        let canonical = RuntimeRoute {
            route_key: route_key.to_string(),
            recipient: recipient.to_string(),
            channel: route
                .channel
                .map(|channel| channel.trim().to_string())
                .filter(|channel| !channel.is_empty()),
            sink: route.sink.trim().to_string(),
            target_module: route.target_module.trim().to_string(),
            retry_max: route.retry_max,
            backoff_ms: route.backoff_ms,
            rate_limit_per_minute: route.rate_limit_per_minute,
        };
        self.runtime_routes
            .insert(canonical.route_key.clone(), canonical.clone());
        Ok(canonical)
    }

    pub fn delete_runtime_route(
        &mut self,
        route_key: &str,
    ) -> Result<RuntimeRoute, RuntimeRouteMutationError> {
        let route_key = route_key.trim();
        if route_key.is_empty() {
            return Err(RuntimeRouteMutationError::EmptyRouteKey);
        }
        self.runtime_routes
            .remove(route_key)
            .ok_or_else(|| RuntimeRouteMutationError::RouteNotFound(route_key.to_string()))
    }

    pub fn resolve_routing(
        &mut self,
        request: RoutingResolveRequest,
    ) -> Result<RoutingResolution, RoutingResolveError> {
        if !self.is_module_loaded("router") {
            return Err(RoutingResolveError::RouterModuleNotLoaded);
        }
        if !self.is_module_loaded("delivery") {
            return Err(RoutingResolveError::DeliveryModuleNotLoaded);
        }

        let recipient = request.recipient.trim();
        if recipient.is_empty() {
            return Err(RoutingResolveError::EmptyRecipient);
        }
        let boundary_request_json = serde_json::to_string(&request).unwrap_or_default();

        let channel = request
            .channel
            .unwrap_or_else(|| "notification".to_string());
        let channel = channel.trim();
        if channel.is_empty() {
            return Err(RoutingResolveError::InvalidChannel);
        }
        let mut retry_max = request.retry_max.unwrap_or(1);
        if retry_max > ROUTING_RETRY_MAX_CAP {
            return Err(RoutingResolveError::RetryMaxExceedsCap {
                provided: retry_max,
                cap: ROUTING_RETRY_MAX_CAP,
            });
        }
        let mut rate_limit_per_minute = request.rate_limit_per_minute.unwrap_or(2);
        if rate_limit_per_minute == 0 {
            return Err(RoutingResolveError::InvalidRateLimitPerMinute);
        }
        let mut resolved_channel = channel.to_string();
        let mut backoff_ms = request.backoff_ms.unwrap_or(250);

        let mut sink = if recipient.contains('@') {
            "email"
        } else if recipient.starts_with('+') {
            "sms"
        } else {
            "webhook"
        }
        .to_string();
        let mut target_module = "delivery".to_string();

        if let Some((router_module, pre_spawn)) = self.module_and_prespawn("router") {
            let boundary_response = run_module_boundary_with_env(
                router_module,
                pre_spawn,
                &[(
                    ROUTER_RESOLVE_REQUEST_ENV.to_string(),
                    boundary_request_json,
                )],
                Duration::from_secs(1),
            )
            .map_err(RoutingResolveError::RouterBoundary)?;
            let overrides = Self::parse_router_boundary_overrides(&boundary_response);
            if let Some(override_channel) = overrides.channel {
                resolved_channel = override_channel;
            }
            if let Some(override_sink) = overrides.sink {
                sink = override_sink;
            }
            if let Some(override_target_module) = overrides.target_module {
                target_module = override_target_module;
            }
            if let Some(override_retry_max) = overrides.retry_max {
                retry_max = override_retry_max;
            }
            if let Some(override_backoff_ms) = overrides.backoff_ms {
                backoff_ms = override_backoff_ms;
            }
            if let Some(override_rate_limit) = overrides.rate_limit_per_minute {
                rate_limit_per_minute = override_rate_limit;
            }
        }
        if retry_max > ROUTING_RETRY_MAX_CAP {
            return Err(RoutingResolveError::RetryMaxExceedsCap {
                provided: retry_max,
                cap: ROUTING_RETRY_MAX_CAP,
            });
        }
        if rate_limit_per_minute == 0 {
            return Err(RoutingResolveError::InvalidRateLimitPerMinute);
        }
        if let Some(route_override) = self.matching_runtime_route(recipient, &resolved_channel) {
            sink = route_override.sink.clone();
            target_module = route_override.target_module.clone();
            retry_max = route_override.retry_max.unwrap_or(retry_max);
            backoff_ms = route_override.backoff_ms.unwrap_or(backoff_ms);
            rate_limit_per_minute = route_override
                .rate_limit_per_minute
                .unwrap_or(rate_limit_per_minute);
        }
        if retry_max > ROUTING_RETRY_MAX_CAP {
            return Err(RoutingResolveError::RetryMaxExceedsCap {
                provided: retry_max,
                cap: ROUTING_RETRY_MAX_CAP,
            });
        }
        if rate_limit_per_minute == 0 {
            return Err(RoutingResolveError::InvalidRateLimitPerMinute);
        }

        let route_sequence = self.next_routing_sequence();
        let route_id = format!("route-{route_sequence:06}");
        let resolution = RoutingResolution {
            route_id: route_id.clone(),
            recipient: recipient.to_string(),
            channel: resolved_channel,
            sink,
            target_module,
            retry_max,
            backoff_ms,
            rate_limit_per_minute,
        };
        self.remember_routing_resolution(resolution.clone());
        let event_id = format!("evt-routing-{route_sequence:06}");
        let resolved_timestamp_ms = self.next_route_resolved_timestamp_ms();
        insert_event_sorted(
            &mut self.merged_events,
            EventEnvelope {
                event_id,
                source: "module".to_string(),
                timestamp_ms: resolved_timestamp_ms,
                event: UnifiedEvent::Module(ModuleEvent {
                    module: "router".to_string(),
                    event_type: "resolved".to_string(),
                    payload: serde_json::to_value(&resolution).unwrap_or(Value::Null),
                }),
            },
        );

        Ok(resolution)
    }

    pub fn send_delivery(
        &mut self,
        request: DeliverySendRequest,
    ) -> Result<DeliveryRecord, DeliverySendError> {
        if !self.is_module_loaded("delivery") {
            return Err(DeliverySendError::DeliveryModuleNotLoaded);
        }
        if let Some(idempotency_key) = request.idempotency_key.as_ref() {
            if idempotency_key.trim().is_empty() {
                return Err(DeliverySendError::InvalidIdempotencyKey);
            }
        }

        if let Some(idempotency_key) = request.idempotency_key.as_deref() {
            if let Some(existing) = self.replay_delivery_for_scoped_idempotency(
                &request.resolution,
                idempotency_key,
                &request.payload,
            )? {
                return Ok(existing);
            }
        }

        let trusted_resolution = self.trusted_resolution_for_delivery(&request.resolution)?;
        if trusted_resolution.target_module != "delivery" {
            return Err(DeliverySendError::InvalidRouteTarget(
                trusted_resolution.target_module,
            ));
        }
        if trusted_resolution.recipient.trim().is_empty() {
            return Err(DeliverySendError::InvalidRecipient);
        }
        if trusted_resolution.sink.trim().is_empty() {
            return Err(DeliverySendError::InvalidSink);
        }

        let scoped_idempotency_key = request.idempotency_key.as_ref().map(|idempotency_key| {
            Self::scoped_idempotency_key(&trusted_resolution.route_id, idempotency_key)
        });
        self.refresh_delivery_clocks_from_now();
        let rate_window_now_ms = self.delivery_now_floor_ms;
        let window_start_ms = rate_window_now_ms - (rate_window_now_ms % DELIVERY_RATE_WINDOW_MS);
        self.prune_delivery_rate_window_counts(window_start_ms);
        let rate_key = DeliveryRateWindowKey {
            route_id: trusted_resolution.route_id.clone(),
            recipient: trusted_resolution.recipient.clone(),
            sink: trusted_resolution.sink.clone(),
            window_start_ms,
        };
        let current_count = self
            .delivery_rate_window_counts
            .get(&rate_key)
            .copied()
            .unwrap_or(0);
        if current_count >= trusted_resolution.rate_limit_per_minute {
            return Err(DeliverySendError::RateLimited {
                sink: trusted_resolution.sink.clone(),
                window_start_ms,
                limit: trusted_resolution.rate_limit_per_minute,
            });
        }
        let first_attempt_ms = self
            .delivery_clock_ms
            .saturating_add(DELIVERY_CLOCK_STEP_MS);
        self.delivery_clock_ms = first_attempt_ms;
        self.delivery_rate_window_counts
            .insert(rate_key, current_count.saturating_add(1));

        let delivery_request_json = serde_json::to_string(&request).unwrap_or_default();
        let boundary_outcome =
            if let Some((delivery_module, pre_spawn)) = self.module_and_prespawn("delivery") {
                let envelope = run_module_boundary_with_env(
                    delivery_module,
                    pre_spawn,
                    &[(DELIVERY_SEND_REQUEST_ENV.to_string(), delivery_request_json)],
                    Duration::from_secs(1),
                )
                .map_err(DeliverySendError::DeliveryBoundary)?;
                Self::parse_delivery_boundary_outcome(&envelope)
            } else {
                DeliveryBoundaryOutcome::default()
            };

        let should_force_fail = request
            .payload
            .get("force_fail")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || boundary_outcome.force_fail;
        let mut attempts = Vec::new();
        let status = if should_force_fail {
            "failed".to_string()
        } else {
            "sent".to_string()
        };
        let total_attempts = trusted_resolution.retry_max.saturating_add(1);
        for attempt in 1..=total_attempts {
            let is_final_attempt = attempt == total_attempts;
            attempts.push(DeliveryAttempt {
                attempt,
                status: if is_final_attempt {
                    status.clone()
                } else {
                    "transient_failure".to_string()
                },
                backoff_ms: if is_final_attempt {
                    0
                } else {
                    trusted_resolution.backoff_ms
                },
            });
        }

        let delivery_sequence = self.next_delivery_sequence();
        let delivery_id = format!("delivery-{delivery_sequence:06}");
        let final_attempt_ms = first_attempt_ms.saturating_add(
            trusted_resolution
                .backoff_ms
                .saturating_mul(u64::from(trusted_resolution.retry_max)),
        );
        let record = DeliveryRecord {
            delivery_id: delivery_id.clone(),
            route_id: trusted_resolution.route_id.clone(),
            recipient: trusted_resolution.recipient.clone(),
            sink: trusted_resolution.sink.clone(),
            target_module: trusted_resolution.target_module.clone(),
            payload: request.payload.clone(),
            status: status.clone(),
            attempts,
            first_attempt_ms,
            final_attempt_ms,
            idempotency_key: request.idempotency_key.clone(),
            sink_adapter: boundary_outcome.sink_adapter.clone(),
        };

        insert_event_sorted(
            &mut self.merged_events,
            EventEnvelope {
                event_id: format!("evt-delivery-{delivery_sequence:06}"),
                source: "module".to_string(),
                timestamp_ms: final_attempt_ms,
                event: UnifiedEvent::Module(ModuleEvent {
                    module: "delivery".to_string(),
                    event_type: "send".to_string(),
                    payload: serde_json::json!({
                        "delivery_id": record.delivery_id,
                        "route_id": record.route_id,
                        "recipient": record.recipient,
                        "sink": record.sink,
                        "status": record.status,
                        "attempts": record.attempts,
                    }),
                }),
            },
        );
        self.delivery_clock_ms = self.delivery_clock_ms.max(final_attempt_ms);

        if let Some(scoped_key) = scoped_idempotency_key {
            self.delivery_idempotency.insert(
                scoped_key.clone(),
                DeliveryIdempotencyEntry {
                    delivery_id: delivery_id.clone(),
                    payload: request.payload.clone(),
                    canonical_resolution: trusted_resolution.clone(),
                },
            );
            self.delivery_idempotency_by_delivery
                .entry(delivery_id.clone())
                .or_default()
                .push(scoped_key);
        }
        self.delivery_history.push(record.clone());
        while self.delivery_history.len() > DELIVERY_HISTORY_LIMIT_MAX {
            let evicted = self.delivery_history.remove(0);
            if let Some(scoped_keys) = self
                .delivery_idempotency_by_delivery
                .remove(&evicted.delivery_id)
            {
                for scoped_key in scoped_keys {
                    self.delivery_idempotency.remove(&scoped_key);
                }
            }
        }

        Ok(record)
    }

    pub fn delivery_history(&self, request: DeliveryHistoryRequest) -> DeliveryHistoryResponse {
        let recipient_filter = request
            .recipient
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        let sink_filter = request
            .sink
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());

        let deliveries = self
            .delivery_history
            .iter()
            .filter(|record| {
                recipient_filter
                    .as_ref()
                    .is_none_or(|recipient| record.recipient == **recipient)
            })
            .filter(|record| {
                sink_filter
                    .as_ref()
                    .is_none_or(|sink| record.sink == **sink)
            })
            .rev()
            .take(request.limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();

        DeliveryHistoryResponse { deliveries }
    }

    pub fn evaluate_gating_action(
        &mut self,
        request: GatingEvaluateRequest,
    ) -> GatingEvaluateResult {
        self.refresh_gating_timeouts();
        let action = request.action.trim().to_string();
        let actor_id = request.actor_id.trim().to_string();
        let requested_approver = request
            .requested_approver
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let approval_recipient = request
            .approval_recipient
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let approval_channel = request
            .approval_channel
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let entity = request
            .entity
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let topic = request
            .topic
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let action_sequence = self.next_gating_sequence();
        let action_id = format!("gate-action-{action_sequence:06}");
        let risk_tier = request.risk_tier.clone();

        if matches!(request.risk_tier, GatingRiskTier::R2 | GatingRiskTier::R3) {
            if !self.memory_conflicts.is_empty() && (entity.is_none() || topic.is_none()) {
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "conflict_blocked".to_string(),
                    action_id: action_id.clone(),
                    pending_id: None,
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    outcome: GatingOutcome::SafeDraft,
                    detail: serde_json::json!({
                        "policy": "memory_conflict_context_required_v0_1",
                        "reason": "memory_conflict_context_missing",
                        "action": action.clone(),
                        "reference": {
                            "entity": entity,
                            "topic": topic,
                        },
                        "missing_context": {
                            "entity": entity.is_none(),
                            "topic": topic.is_none(),
                        },
                        "conflict_count": self.memory_conflicts.len(),
                    }),
                });
                return GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::SafeDraft,
                    pending_id: None,
                    fallback_reason: Some("memory_conflict_context_missing".to_string()),
                };
            }
            if let Some(conflict) =
                self.memory_conflict_for_reference(entity.as_deref(), topic.as_deref())
            {
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "conflict_blocked".to_string(),
                    action_id: action_id.clone(),
                    pending_id: None,
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    outcome: GatingOutcome::SafeDraft,
                    detail: serde_json::json!({
                        "policy": "memory_conflict_block_v0_1",
                        "reason": "memory_conflict",
                        "action": action.clone(),
                        "reference": {
                            "entity": entity,
                            "topic": topic,
                        },
                        "conflict": conflict,
                    }),
                });
                return GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::SafeDraft,
                    pending_id: None,
                    fallback_reason: Some("memory_conflict".to_string()),
                };
            }
        }

        match request.risk_tier {
            GatingRiskTier::R0 | GatingRiskTier::R1 => {
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "evaluated".to_string(),
                    action_id: action_id.clone(),
                    pending_id: None,
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    outcome: GatingOutcome::Allowed,
                    detail: serde_json::json!({
                        "policy": "allow_immediate",
                        "rationale": request.rationale,
                        "action": action,
                    }),
                });
                GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::Allowed,
                    pending_id: None,
                    fallback_reason: None,
                }
            }
            GatingRiskTier::R2 => {
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "evaluated".to_string(),
                    action_id: action_id.clone(),
                    pending_id: None,
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    outcome: GatingOutcome::AllowedWithAudit,
                    detail: serde_json::json!({
                        "policy": "consequence_mode_allow_with_audit_v0_1",
                        "rationale": request.rationale,
                        "action": action,
                    }),
                });
                GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::AllowedWithAudit,
                    pending_id: None,
                    fallback_reason: None,
                }
            }
            GatingRiskTier::R3 => {
                let pending_sequence = self.next_gating_sequence();
                let pending_id = format!("gate-pending-{pending_sequence:06}");
                let created_at_ms = current_time_ms();
                let timeout_ms = request
                    .approval_timeout_ms
                    .unwrap_or(GATING_APPROVAL_TIMEOUT_DEFAULT_MS);
                let mut approval_route_id = None;
                let mut approval_delivery_id = None;
                let mut approval_notification_error = None;

                if let (Some(recipient), Some(channel)) =
                    (approval_recipient.as_ref(), approval_channel.as_ref())
                {
                    if self.is_module_loaded("router") && self.is_module_loaded("delivery") {
                        match self.resolve_routing(RoutingResolveRequest {
                            recipient: recipient.clone(),
                            channel: Some(channel.clone()),
                            retry_max: None,
                            backoff_ms: None,
                            rate_limit_per_minute: None,
                        }) {
                            Ok(resolution) => {
                                approval_route_id = Some(resolution.route_id.clone());
                                match self.send_delivery(DeliverySendRequest {
                                    resolution,
                                    payload: serde_json::json!({
                                        "kind": "gating_approval_request",
                                        "pending_id": pending_id.clone(),
                                        "action_id": action_id.clone(),
                                        "action": action.clone(),
                                        "actor_id": actor_id.clone(),
                                        "risk_tier": risk_tier.clone(),
                                        "requested_approver": requested_approver.clone(),
                                        "deadline_at_ms": created_at_ms.saturating_add(timeout_ms),
                                    }),
                                    idempotency_key: Some(format!("gating-approval-{pending_id}")),
                                }) {
                                    Ok(record) => {
                                        if record.status == "sent" {
                                            approval_delivery_id = Some(record.delivery_id);
                                        } else {
                                            approval_notification_error = Some(format!(
                                                "delivery_status:{}:{}",
                                                record.status, record.delivery_id
                                            ));
                                        }
                                    }
                                    Err(err) => {
                                        approval_notification_error =
                                            Some(format!("delivery:{err:?}"));
                                    }
                                }
                            }
                            Err(err) => {
                                approval_notification_error = Some(format!("routing:{err:?}"));
                            }
                        }
                    } else {
                        let mut missing_modules = Vec::new();
                        if !self.is_module_loaded("router") {
                            missing_modules.push("router");
                        }
                        if !self.is_module_loaded("delivery") {
                            missing_modules.push("delivery");
                        }
                        approval_notification_error = Some(format!(
                            "notification_modules_unavailable:{}",
                            missing_modules.join(",")
                        ));
                    }
                }
                let pending_entry = GatingPendingEntry {
                    pending_id: pending_id.clone(),
                    action_id: action_id.clone(),
                    action: action.clone(),
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    requested_approver,
                    approval_recipient,
                    approval_channel,
                    approval_route_id,
                    approval_delivery_id,
                    created_at_ms,
                    deadline_at_ms: created_at_ms.saturating_add(timeout_ms),
                };
                self.upsert_gating_pending_entry(pending_entry.clone());
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "pending_created".to_string(),
                    action_id: action_id.clone(),
                    pending_id: Some(pending_id.clone()),
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    outcome: GatingOutcome::PendingApproval,
                    detail: serde_json::json!({
                        "requested_approver": pending_entry.requested_approver.clone(),
                        "approval_recipient": pending_entry.approval_recipient.clone(),
                        "approval_channel": pending_entry.approval_channel.clone(),
                        "approval_route_id": pending_entry.approval_route_id.clone(),
                        "approval_delivery_id": pending_entry.approval_delivery_id.clone(),
                        "approval_notification_error": approval_notification_error,
                        "deadline_at_ms": pending_entry.deadline_at_ms,
                        "action": action,
                    }),
                });
                GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::PendingApproval,
                    pending_id: Some(pending_id),
                    fallback_reason: None,
                }
            }
        }
    }

    pub fn list_gating_pending(&mut self) -> Vec<GatingPendingEntry> {
        self.refresh_gating_timeouts();
        self.gating_pending_order
            .iter()
            .filter_map(|pending_id| self.gating_pending.get(pending_id).cloned())
            .collect()
    }

    pub fn decide_gating_action(
        &mut self,
        request: GatingDecideRequest,
    ) -> Result<GatingDecisionResult, GatingDecideError> {
        self.refresh_gating_timeouts();
        let decision = request.decision.clone();
        let reason = request.reason.clone();
        let pending_id = request.pending_id.trim().to_string();
        let approver_id = request.approver_id.trim().to_string();
        let pending_entry = self
            .gating_pending
            .remove(&pending_id)
            .ok_or_else(|| GatingDecideError::UnknownPendingId(pending_id.clone()))?;
        self.gating_pending_order
            .retain(|candidate| candidate != &pending_id);

        if matches!(decision, GatingDecision::Approve) && approver_id == pending_entry.actor_id {
            self.upsert_gating_pending_entry(pending_entry.clone());
            return Err(GatingDecideError::SelfApprovalForbidden);
        }
        if let Some(expected_approver) = pending_entry.requested_approver.as_deref() {
            if expected_approver != approver_id {
                self.upsert_gating_pending_entry(pending_entry.clone());
                return Err(GatingDecideError::ApproverMismatch {
                    expected: expected_approver.to_string(),
                    provided: approver_id,
                });
            }
        }

        let (outcome, event_type) = match decision {
            GatingDecision::Approve => (GatingOutcome::Allowed, "approval_decided"),
            GatingDecision::Reject => (GatingOutcome::SafeDraft, "rejection_decided"),
        };
        let decided_at_ms = current_time_ms();
        self.append_gating_audit(GatingAuditEntry {
            audit_id: String::new(),
            timestamp_ms: 0,
            event_type: event_type.to_string(),
            action_id: pending_entry.action_id.clone(),
            pending_id: Some(pending_id.clone()),
            actor_id: pending_entry.actor_id.clone(),
            risk_tier: pending_entry.risk_tier.clone(),
            outcome: outcome.clone(),
            detail: serde_json::json!({
                "approver_id": approver_id,
                "decision": decision.clone(),
                "reason": reason.clone(),
                "approval_route_id": pending_entry.approval_route_id.clone(),
                "approval_delivery_id": pending_entry.approval_delivery_id.clone(),
            }),
        });
        Ok(GatingDecisionResult {
            pending_id,
            action_id: pending_entry.action_id,
            approver_id,
            decision,
            outcome,
            decided_at_ms,
            reason,
        })
    }

    pub fn gating_audit_entries(&mut self, limit: usize) -> Vec<GatingAuditEntry> {
        self.refresh_gating_timeouts();
        self.gating_audit
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn merged_events(&self) -> Vec<EventEnvelope<UnifiedEvent>> {
        self.merged_events.clone()
    }

    pub fn delivery_rate_window_count_entries(&self) -> usize {
        self.delivery_rate_window_counts.len()
    }

    pub fn evaluate_schedule_tick(
        &self,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> Result<ScheduleEvaluation, ScheduleValidationError> {
        evaluate_schedules_at_tick(schedules, tick_ms)
    }

    pub fn dispatch_schedule_tick(
        &mut self,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> Result<ScheduleDispatchReport, ScheduleValidationError> {
        validate_schedule_tick_ms_supported(tick_ms)?;
        validate_schedules(schedules)?;
        self.prune_schedule_claims(tick_ms);
        self.prune_scheduling_last_due_ticks(tick_ms);
        let mut due_triggers = schedules
            .iter()
            .filter(|schedule| schedule.enabled)
            .filter_map(|schedule| {
                let canonical_schedule_id = canonical_schedule_id(&schedule.schedule_id);
                let interval = parse_schedule_interval(&schedule.interval)
                    .expect("validated schedule interval");
                let timezone = parse_schedule_timezone(&schedule.timezone)
                    .expect("validated schedule timezone");
                let due_tick_ms = latest_due_tick_at_or_before(
                    &canonical_schedule_id,
                    &interval,
                    &timezone,
                    schedule.jitter_ms,
                    tick_ms,
                )?;
                let last_due_tick = self
                    .scheduling_last_due_ticks
                    .get(&canonical_schedule_id)
                    .copied();
                if schedule.catch_up {
                    if last_due_tick.is_some_and(|last| last >= due_tick_ms) {
                        return None;
                    }
                } else if last_due_tick
                    .is_some_and(|last| last >= due_tick_ms && due_tick_ms != tick_ms)
                {
                    return None;
                }
                Some((schedule, canonical_schedule_id, due_tick_ms))
            })
            .collect::<Vec<_>>();
        due_triggers.sort_by(
            |(left_schedule, left_schedule_id, left_due_tick),
             (right_schedule, right_schedule_id, right_due_tick)| {
                left_due_tick
                    .cmp(right_due_tick)
                    .then_with(|| left_schedule_id.cmp(right_schedule_id))
                    .then_with(|| left_schedule.interval.cmp(&right_schedule.interval))
                    .then_with(|| left_schedule.timezone.cmp(&right_schedule.timezone))
            },
        );
        let mut dispatched = Vec::new();
        let mut skipped_claims = Vec::new();
        let scheduling_signal = self.scheduling_supervisor_signal();
        let mut supervisor_restart_emitted = false;

        for (trigger, canonical_schedule_id, due_tick_ms) in due_triggers.iter() {
            let claim_key = format!("{canonical_schedule_id}:{due_tick_ms}");
            if !self.record_schedule_claim(claim_key.clone(), tick_ms) {
                skipped_claims.push(claim_key);
                continue;
            }
            self.scheduling_last_due_ticks
                .insert(canonical_schedule_id.clone(), *due_tick_ms);
            self.prune_scheduling_last_due_ticks(tick_ms);

            let event_sequence = self.next_scheduling_dispatch_sequence();
            let event_id = format!(
                "evt-schedule-{}-{due_tick_ms}-{event_sequence}",
                canonical_schedule_id
            );
            insert_event_sorted(
                &mut self.merged_events,
                EventEnvelope {
                    event_id: event_id.clone(),
                    source: "module".to_string(),
                    timestamp_ms: tick_ms,
                    event: UnifiedEvent::Module(ModuleEvent {
                        module: "scheduling".to_string(),
                        event_type: "dispatch".to_string(),
                        payload: serde_json::json!({
                            "schedule_id": canonical_schedule_id,
                            "interval": trigger.interval,
                            "timezone": trigger.timezone,
                            "tick_ms": tick_ms,
                            "due_tick_ms": due_tick_ms,
                            "claim_key": claim_key,
                            "supervisor_signal": scheduling_signal,
                        }),
                    }),
                },
            );

            if let Some(signal) = &scheduling_signal {
                if signal.restart_observed && !supervisor_restart_emitted {
                    insert_event_sorted(
                        &mut self.merged_events,
                        EventEnvelope {
                            event_id: format!(
                                "evt-scheduling-supervisor-{tick_ms}-{event_sequence}",
                            ),
                            source: "module".to_string(),
                            timestamp_ms: tick_ms,
                            event: UnifiedEvent::Module(ModuleEvent {
                                module: "scheduling".to_string(),
                                event_type: "supervisor.restart".to_string(),
                                payload: serde_json::json!({
                                    "module_id": signal.module_id,
                                    "latest_state": signal.latest_state,
                                    "latest_attempt": signal.latest_attempt,
                                    "restart_observed": signal.restart_observed,
                                }),
                            }),
                        },
                    );
                    supervisor_restart_emitted = true;
                }
            }

            dispatched.push(ScheduleDispatch {
                claim_key,
                schedule_id: canonical_schedule_id.clone(),
                interval: trigger.interval.clone(),
                timezone: trigger.timezone.clone(),
                due_tick_ms: *due_tick_ms,
                tick_ms,
                event_id,
                supervisor_signal: scheduling_signal.clone(),
            });
        }

        Ok(ScheduleDispatchReport {
            tick_ms,
            due_count: due_triggers.len(),
            dispatched,
            skipped_claims,
        })
    }

    pub fn subscribe_events(
        &self,
        request: SubscribeRequest,
    ) -> Result<SubscribeResponse, SubscribeError> {
        if let Some(checkpoint) = request.last_event_id.as_ref() {
            if checkpoint.trim().is_empty() {
                return Err(SubscribeError::EmptyCheckpoint);
            }
        }

        if matches!(request.scope, SubscribeScope::Agent) {
            let agent_id = request
                .agent_id
                .as_deref()
                .ok_or(SubscribeError::MissingAgentId)?;
            if agent_id.trim().is_empty() {
                return Err(SubscribeError::InvalidAgentId);
            }
        }

        let scoped_events = self
            .merged_events
            .iter()
            .filter(|event| event_matches_request(event, &request))
            .cloned()
            .collect::<Vec<_>>();
        let bounded_scoped_events = scoped_events
            .iter()
            .rev()
            .take(SUBSCRIBE_REPLAY_EVENT_CAP)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();

        let replay_events = match request.last_event_id.as_ref() {
            Some(checkpoint) => {
                let start_idx = bounded_scoped_events
                    .iter()
                    .position(|event| event.event_id == *checkpoint)
                    .ok_or_else(|| SubscribeError::UnknownCheckpoint(checkpoint.clone()))?;
                bounded_scoped_events[start_idx..].to_vec()
            }
            None => bounded_scoped_events,
        };
        let event_frames = replay_events
            .iter()
            .map(build_sse_event_frame)
            .collect::<Vec<_>>();

        Ok(SubscribeResponse {
            scope: request.scope,
            replay_from_event_id: request.last_event_id,
            keep_alive: SubscribeKeepAlive {
                interval_ms: SSE_KEEP_ALIVE_INTERVAL_MS,
                event: SSE_KEEP_ALIVE_EVENT_NAME.to_string(),
            },
            keep_alive_comment: SSE_KEEP_ALIVE_COMMENT_FRAME.to_string(),
            event_frames,
            events: replay_events,
        })
    }

    fn record_schedule_claim(&mut self, claim_key: String, tick_ms: u64) -> bool {
        if !self.scheduling_claims.insert(claim_key.clone()) {
            return false;
        }
        self.scheduling_claim_ticks
            .entry(tick_ms)
            .or_default()
            .push(claim_key);
        true
    }

    fn prune_schedule_claims(&mut self, current_tick_ms: u64) {
        let cutoff_tick = current_tick_ms.saturating_sub(SCHEDULING_CLAIM_RETENTION_WINDOW_MS);
        let expired_ticks = self
            .scheduling_claim_ticks
            .keys()
            .copied()
            .take_while(|tick| *tick < cutoff_tick)
            .collect::<Vec<_>>();
        for tick in expired_ticks {
            if let Some(keys) = self.scheduling_claim_ticks.remove(&tick) {
                for key in keys {
                    self.scheduling_claims.remove(&key);
                }
            }
        }

        while self.scheduling_claims.len() > SCHEDULING_CLAIMS_MAX_RETAINED {
            let Some(oldest_tick) = self.scheduling_claim_ticks.keys().next().copied() else {
                break;
            };
            if let Some(keys) = self.scheduling_claim_ticks.remove(&oldest_tick) {
                for key in keys {
                    self.scheduling_claims.remove(&key);
                }
            } else {
                break;
            }
        }
    }

    fn prune_scheduling_last_due_ticks(&mut self, current_tick_ms: u64) {
        let cutoff_tick = current_tick_ms.saturating_sub(SCHEDULING_CLAIM_RETENTION_WINDOW_MS);
        self.scheduling_last_due_ticks
            .retain(|_, due_tick| *due_tick >= cutoff_tick);

        while self.scheduling_last_due_ticks.len() > SCHEDULING_LAST_DUE_MAX_RETAINED {
            let Some(oldest_schedule_id) = self
                .scheduling_last_due_ticks
                .iter()
                .min_by(|(left_id, left_due), (right_id, right_due)| {
                    left_due.cmp(right_due).then_with(|| left_id.cmp(right_id))
                })
                .map(|(schedule_id, _)| schedule_id.clone())
            else {
                break;
            };
            self.scheduling_last_due_ticks.remove(&oldest_schedule_id);
        }
    }

    pub fn reconcile_modules(
        &mut self,
        modules: Vec<String>,
        timeout: Duration,
    ) -> Result<usize, RuntimeMutationError> {
        for module_id in &modules {
            if self
                .config
                .modules
                .iter()
                .all(|configured| configured.id != *module_id)
            {
                return Err(RuntimeMutationError::Config(
                    ConfigResolutionError::ModuleNotConfigured(module_id.clone()),
                ));
            }
        }

        self.config.discovery.modules = modules.clone();
        let mut added = 0_usize;
        for module_id in modules {
            if self.loaded_modules.contains(&module_id) {
                continue;
            }
            self.spawn_member(&module_id, timeout)?;
            added += 1;
        }
        Ok(added)
    }

    pub fn spawn_member(
        &mut self,
        module_id: &str,
        timeout: Duration,
    ) -> Result<(), RuntimeMutationError> {
        let module = self
            .config
            .modules
            .iter()
            .find(|module| module.id == module_id)
            .ok_or_else(|| {
                RuntimeMutationError::Config(ConfigResolutionError::ModuleNotConfigured(
                    module_id.to_string(),
                ))
            })?;

        let pre_spawn = self
            .config
            .pre_spawn
            .iter()
            .find(|data| data.module_id == module_id);

        let event = run_module_boundary_once(module, pre_spawn, timeout)
            .map_err(RuntimeMutationError::Runtime)?;

        if !self
            .config
            .discovery
            .modules
            .iter()
            .any(|configured| configured == module_id)
        {
            self.config.discovery.modules.push(module_id.to_string());
        }
        self.loaded_modules.insert(module_id.to_string());
        insert_event_sorted(&mut self.merged_events, event);
        Ok(())
    }

    fn scheduling_supervisor_signal(&self) -> Option<SchedulingSupervisorSignal> {
        let module_transitions = self
            .supervisor_report
            .transitions
            .iter()
            .filter(|transition| transition.module_id == "scheduling")
            .collect::<Vec<_>>();
        let latest = module_transitions.last()?;
        let restart_observed = module_transitions
            .iter()
            .any(|transition| transition.to == ModuleHealthState::Restarting);
        Some(SchedulingSupervisorSignal {
            module_id: latest.module_id.clone(),
            latest_state: latest.to.clone(),
            latest_attempt: latest.attempt,
            restart_observed,
        })
    }
}

fn supervise_module_start(
    module: &ModuleConfig,
    pre_spawn: Option<&PreSpawnData>,
    timeout: Duration,
    options: &RuntimeOptions,
) -> (
    Option<EventEnvelope<UnifiedEvent>>,
    Option<Child>,
    Vec<ModuleHealthTransition>,
) {
    let mut transitions = vec![ModuleHealthTransition {
        module_id: module.id.clone(),
        from: None,
        to: ModuleHealthState::Starting,
        attempt: 0,
    }];

    let mut attempts = 0_u32;
    let mut state = ModuleHealthState::Starting;

    loop {
        attempts += 1;
        let result = spawn_module_capture_first_event(module, pre_spawn, timeout);

        match result {
            Ok((event, mut child)) => {
                transitions.push(ModuleHealthTransition {
                    module_id: module.id.clone(),
                    from: Some(state.clone()),
                    to: ModuleHealthState::Healthy,
                    attempt: attempts,
                });

                let should_restart = match module.restart_policy {
                    RestartPolicy::Always => attempts <= options.always_restart_budget,
                    _ => false,
                };

                if should_restart {
                    transitions.push(ModuleHealthTransition {
                        module_id: module.id.clone(),
                        from: Some(ModuleHealthState::Healthy),
                        to: ModuleHealthState::Restarting,
                        attempt: attempts,
                    });
                    let _ = terminate_child(&mut child);
                    state = ModuleHealthState::Restarting;
                    continue;
                }

                return (Some(event), Some(child), transitions);
            }
            Err(_) => {
                transitions.push(ModuleHealthTransition {
                    module_id: module.id.clone(),
                    from: Some(state.clone()),
                    to: ModuleHealthState::Failed,
                    attempt: attempts,
                });

                let should_retry = match module.restart_policy {
                    RestartPolicy::Never => false,
                    RestartPolicy::OnFailure => attempts <= options.on_failure_retry_budget,
                    RestartPolicy::Always => attempts <= options.always_restart_budget,
                };

                if should_retry {
                    transitions.push(ModuleHealthTransition {
                        module_id: module.id.clone(),
                        from: Some(ModuleHealthState::Failed),
                        to: ModuleHealthState::Restarting,
                        attempt: attempts,
                    });
                    state = ModuleHealthState::Restarting;
                    continue;
                }

                transitions.push(ModuleHealthTransition {
                    module_id: module.id.clone(),
                    from: Some(ModuleHealthState::Failed),
                    to: ModuleHealthState::Stopped,
                    attempt: attempts,
                });
                return (None, None, transitions);
            }
        }
    }
}

fn spawn_module_capture_first_event(
    module: &ModuleConfig,
    pre_spawn: Option<&PreSpawnData>,
    timeout: Duration,
) -> Result<(EventEnvelope<UnifiedEvent>, Child), RuntimeBoundaryError> {
    let env = pre_spawn
        .filter(|data| data.module_id == module.id)
        .map(|data| data.env.clone())
        .unwrap_or_default();

    let mut child = Command::new(&module.command)
        .args(&module.args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| {
            RuntimeBoundaryError::Process(ProcessBoundaryError::SpawnFailed(err.to_string()))
        })?;

    let stdout = child.stdout.take().ok_or(RuntimeBoundaryError::Process(
        ProcessBoundaryError::MissingStdout,
    ))?;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).map_err(|err| err.to_string());
        let _ = tx.send((result, line));
    });

    match rx.recv_timeout(timeout) {
        Ok((Ok(0), _)) => {
            let _ = child.wait();
            Err(RuntimeBoundaryError::Process(
                ProcessBoundaryError::EmptyOutput,
            ))
        }
        Ok((Ok(_), mut line)) => {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            match normalize_event_line(&line) {
                Ok(event) => Ok((event, child)),
                Err(err) => {
                    let _ = terminate_child(&mut child);
                    Err(RuntimeBoundaryError::Normalize(err))
                }
            }
        }
        Ok((Err(err), _)) => {
            let _ = terminate_child(&mut child);
            Err(RuntimeBoundaryError::Process(ProcessBoundaryError::Io(err)))
        }
        Err(_) => {
            let _ = terminate_child(&mut child);
            Err(RuntimeBoundaryError::Process(
                ProcessBoundaryError::Timeout {
                    timeout_ms: timeout.as_millis() as u64,
                },
            ))
        }
    }
}

fn terminate_child(child: &mut Child) -> bool {
    match child.try_wait() {
        Ok(Some(_)) => true,
        Ok(None) => {
            if child.kill().is_err() {
                return false;
            }
            child.wait().is_ok()
        }
        Err(_) => false,
    }
}

fn merge_unified_events(
    mut module_events: Vec<EventEnvelope<UnifiedEvent>>,
    mut agent_events: Vec<EventEnvelope<UnifiedEvent>>,
) -> Vec<EventEnvelope<UnifiedEvent>> {
    let mut merged = Vec::with_capacity(module_events.len() + agent_events.len());
    merged.append(&mut module_events);
    merged.append(&mut agent_events);
    merged.sort_by(|left, right| {
        left.timestamp_ms
            .cmp(&right.timestamp_ms)
            .then_with(|| left.event_id.cmp(&right.event_id))
            .then_with(|| left.source.cmp(&right.source))
    });
    merged
}

fn event_matches_request(event: &EventEnvelope<UnifiedEvent>, request: &SubscribeRequest) -> bool {
    match request.scope {
        SubscribeScope::Mob => true,
        SubscribeScope::Agent => match &event.event {
            UnifiedEvent::Agent { agent_id, .. } => request
                .agent_id
                .as_deref()
                .map(|selected| selected == agent_id)
                .unwrap_or(false),
            UnifiedEvent::Module(_) => false,
        },
        SubscribeScope::Interaction => match &event.event {
            UnifiedEvent::Agent { event_type, .. } => event_type.starts_with("interaction"),
            UnifiedEvent::Module(module_event) => {
                module_event.event_type.starts_with("interaction")
            }
        },
    }
}

fn build_sse_event_frame(event: &EventEnvelope<UnifiedEvent>) -> String {
    let event_name = match &event.event {
        UnifiedEvent::Agent { event_type, .. } => event_type.as_str(),
        UnifiedEvent::Module(module_event) => module_event.event_type.as_str(),
    };
    let payload = serde_json::to_string(&event.event).unwrap_or_else(|_| "{}".to_string());
    format!(
        "id: {}\nevent: {}\ndata: {}\n\n",
        event.event_id, event_name, payload
    )
}

fn enforce_source_consistency(
    envelope: EventEnvelope<UnifiedEvent>,
) -> Result<EventEnvelope<UnifiedEvent>, NormalizationError> {
    let expected = match &envelope.event {
        UnifiedEvent::Agent { .. } => "agent",
        UnifiedEvent::Module(_) => "module",
    };
    if envelope.source != expected {
        return Err(NormalizationError::SourceMismatch {
            expected,
            got: envelope.source,
        });
    }
    Ok(envelope)
}

fn required_string(
    value: Option<&Value>,
    field: &'static str,
) -> Result<String, NormalizationError> {
    let value = value.ok_or(NormalizationError::MissingField(field))?;
    let text = value
        .as_str()
        .ok_or(NormalizationError::InvalidFieldType(field))?;
    Ok(text.to_string())
}

fn required_u64(value: Option<&Value>, field: &'static str) -> Result<u64, NormalizationError> {
    let value = value.ok_or(NormalizationError::MissingField(field))?;
    value
        .as_u64()
        .ok_or(NormalizationError::InvalidFieldType(field))
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedInterval {
    Marker { interval_ms: u64 },
    Cron(CronExpression),
}

impl ParsedInterval {
    fn jitter_base_interval_ms(&self) -> u64 {
        match self {
            Self::Marker { interval_ms } => *interval_ms,
            // Five-field cron expressions are minute-based.
            Self::Cron(_) => 60_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedTimezone {
    FixedOffsetMs(i64),
    Iana(chrono_tz::Tz),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CronExpression {
    minute: CronFieldSet,
    hour: CronFieldSet,
    day_of_month: CronFieldSet,
    month: CronFieldSet,
    day_of_week: CronFieldSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CronFieldSet {
    any: bool,
    min: u32,
    allowed: Vec<bool>,
}

impl CronExpression {
    fn parse(expression: &str) -> Option<Self> {
        let fields = expression.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 5 {
            return None;
        }
        let parsed = Self {
            minute: parse_cron_field(fields[0], 0, 59, false)?,
            hour: parse_cron_field(fields[1], 0, 23, false)?,
            day_of_month: parse_cron_field(fields[2], 1, 31, false)?,
            month: parse_cron_field(fields[3], 1, 12, false)?,
            day_of_week: parse_cron_field(fields[4], 0, 7, true)?,
        };

        // Keep standard DOM/DOW OR semantics. Only reject expressions that can never fire
        // when day-of-week is wildcard and the selected day-of-month never exists in selected months.
        if parsed.day_of_week.any
            && !parsed.day_of_month.any
            && !parsed.has_possible_day_of_month_for_selected_months()
        {
            return None;
        }

        Some(parsed)
    }

    fn matches(&self, local: &LocalDateTimeFields) -> bool {
        if !self.minute.matches(local.minute)
            || !self.hour.matches(local.hour)
            || !self.month.matches(local.month)
        {
            return false;
        }

        let dom_match = self.day_of_month.matches(local.day_of_month);
        let dow_match = self.day_of_week.matches(local.day_of_week);

        if self.day_of_month.any && self.day_of_week.any {
            true
        } else if self.day_of_month.any {
            dow_match
        } else if self.day_of_week.any {
            dom_match
        } else {
            dom_match || dow_match
        }
    }

    fn has_possible_day_of_month_for_selected_months(&self) -> bool {
        for month in 1..=12 {
            if !self.month.matches(month) {
                continue;
            }
            let max_day = max_day_for_month_with_feb_29(month);
            for day in 1..=max_day {
                if self.day_of_month.matches(day) {
                    return true;
                }
            }
        }
        false
    }
}

impl CronFieldSet {
    fn matches(&self, value: u32) -> bool {
        if value < self.min {
            return false;
        }
        let idx = (value - self.min) as usize;
        self.allowed.get(idx).copied().unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalDateTimeFields {
    minute: u32,
    hour: u32,
    day_of_month: u32,
    month: u32,
    day_of_week: u32,
    second: u32,
    subsec_nanos: u32,
}

fn parse_cron_field(
    field: &str,
    min: u32,
    max: u32,
    map_sunday_seven_to_zero: bool,
) -> Option<CronFieldSet> {
    let mut allowed = vec![false; (max - min + 1) as usize];

    for raw_token in field.split(',') {
        let token = raw_token.trim();
        if token.is_empty() {
            return None;
        }
        let (base, step) = match token.split_once('/') {
            Some((base, step)) => {
                let step = step.parse::<u32>().ok()?;
                if step == 0 {
                    return None;
                }
                (base.trim(), step)
            }
            None => (token, 1),
        };

        if base == "*" {
            let mut value = min;
            while value <= max {
                let mapped = normalize_cron_value(value, map_sunday_seven_to_zero);
                let idx = (mapped - min) as usize;
                allowed[idx] = true;
                match value.checked_add(step) {
                    Some(next) => value = next,
                    None => break,
                }
            }
            continue;
        }

        if let Some((start, end)) = base.split_once('-') {
            let start = parse_cron_raw_value(start.trim(), min, max)?;
            let end = parse_cron_raw_value(end.trim(), min, max)?;
            if start > end {
                return None;
            }
            let mut value = start;
            while value <= end {
                let mapped = normalize_cron_value(value, map_sunday_seven_to_zero);
                let idx = (mapped - min) as usize;
                allowed[idx] = true;
                match value.checked_add(step) {
                    Some(next) => value = next,
                    None => break,
                }
            }
            continue;
        }

        let value = parse_cron_value(base, min, max, map_sunday_seven_to_zero)?;
        let idx = (value - min) as usize;
        allowed[idx] = true;
    }

    if allowed.iter().all(|allowed| !allowed) {
        return None;
    }

    let any = cron_field_is_semantic_wildcard(min, max, map_sunday_seven_to_zero, &allowed);
    Some(CronFieldSet { any, min, allowed })
}

fn cron_field_is_semantic_wildcard(
    min: u32,
    max: u32,
    map_sunday_seven_to_zero: bool,
    allowed: &[bool],
) -> bool {
    let mut covered = vec![false; allowed.len()];
    for raw in min..=max {
        let mapped = normalize_cron_value(raw, map_sunday_seven_to_zero);
        if mapped < min || mapped > max {
            return false;
        }
        let mapped_idx = (mapped - min) as usize;
        covered[mapped_idx] = true;
    }

    covered
        .iter()
        .enumerate()
        .filter(|(_, is_semantic_value)| **is_semantic_value)
        .all(|(idx, _)| allowed.get(idx).copied().unwrap_or(false))
}

fn parse_cron_value(raw: &str, min: u32, max: u32, map_sunday_seven_to_zero: bool) -> Option<u32> {
    let value = normalize_cron_value(
        parse_cron_raw_value(raw, min, max)?,
        map_sunday_seven_to_zero,
    );
    if value < min || value > max {
        return None;
    }
    Some(value)
}

fn parse_cron_raw_value(raw: &str, min: u32, max: u32) -> Option<u32> {
    let value = raw.parse::<u32>().ok()?;
    if value < min || value > max {
        return None;
    }
    Some(value)
}

fn normalize_cron_value(value: u32, map_sunday_seven_to_zero: bool) -> u32 {
    if map_sunday_seven_to_zero && value == 7 {
        0
    } else {
        value
    }
}

fn max_day_for_month_with_feb_29(month: u32) -> u32 {
    match month {
        2 => 29,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn parse_interval_marker_ms(interval: &str) -> Option<u64> {
    let marker = interval.trim().to_ascii_lowercase();
    let marker = marker.strip_prefix("*/")?;
    if marker.len() < 2 {
        return None;
    }
    let (count_part, unit_part) = marker.split_at(marker.len() - 1);
    let count = count_part.parse::<u64>().ok()?;
    if count == 0 {
        return None;
    }
    let unit_ms = match unit_part {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return None,
    };
    count.checked_mul(unit_ms)
}

fn parse_schedule_interval(interval: &str) -> Option<ParsedInterval> {
    parse_interval_marker_ms(interval)
        .map(|interval_ms| ParsedInterval::Marker { interval_ms })
        .or_else(|| CronExpression::parse(interval.trim()).map(ParsedInterval::Cron))
}

fn deterministic_jitter_offset_ms(schedule_id: &str, jitter_ms: u64, interval_ms: u64) -> u64 {
    if jitter_ms == 0 || interval_ms <= 1 {
        return 0;
    }
    let mut hash = 1_469_598_103_934_665_603_u64;
    for byte in schedule_id.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    let max_jitter = jitter_ms.min(interval_ms.saturating_sub(1));
    hash % (max_jitter + 1)
}

fn parse_schedule_timezone(timezone: &str) -> Option<ParsedTimezone> {
    let timezone = timezone.trim();
    if timezone.is_empty() {
        return None;
    }
    parse_timezone_offset_ms(timezone)
        .map(ParsedTimezone::FixedOffsetMs)
        .or_else(|| {
            timezone
                .parse::<chrono_tz::Tz>()
                .ok()
                .map(ParsedTimezone::Iana)
        })
}

fn parse_timezone_offset_ms(timezone: &str) -> Option<i64> {
    let tz = timezone.trim();
    if tz.is_empty() {
        return None;
    }
    if tz.eq_ignore_ascii_case("utc") || tz == "Z" {
        return Some(0);
    }
    let offset = tz
        .strip_prefix("UTC")
        .or_else(|| tz.strip_prefix("utc"))
        .or_else(|| tz.strip_prefix("GMT"))
        .or_else(|| tz.strip_prefix("gmt"))
        .unwrap_or(tz);
    parse_hhmm_offset(offset)
}

fn parse_hhmm_offset(offset: &str) -> Option<i64> {
    if offset.is_empty() {
        return Some(0);
    }
    let sign = if offset.starts_with('+') {
        1_i64
    } else if offset.starts_with('-') {
        -1_i64
    } else {
        return None;
    };
    let body = &offset[1..];
    let (hours, minutes) = if let Some((h, m)) = body.split_once(':') {
        (h, m)
    } else if body.len() == 4 {
        body.split_at(2)
    } else {
        return None;
    };
    let hours = hours.parse::<i64>().ok()?;
    let minutes = minutes.parse::<i64>().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    let total_minutes = hours.saturating_mul(60).saturating_add(minutes);
    Some(sign.saturating_mul(total_minutes).saturating_mul(60_000))
}

fn utc_datetime_from_tick_ms(tick_ms: u64) -> Option<chrono::DateTime<Utc>> {
    let tick_ms = i64::try_from(tick_ms).ok()?;
    chrono::DateTime::<Utc>::from_timestamp_millis(tick_ms)
}

fn local_fields_at_tick(timezone: &ParsedTimezone, tick_ms: u64) -> Option<LocalDateTimeFields> {
    let utc = utc_datetime_from_tick_ms(tick_ms)?;
    let (minute, hour, day_of_month, month, day_of_week, second, subsec_nanos) = match timezone {
        ParsedTimezone::FixedOffsetMs(offset_ms) => {
            let offset_seconds = i32::try_from(offset_ms / 1_000).ok()?;
            let offset = chrono::FixedOffset::east_opt(offset_seconds)?;
            let local = utc.with_timezone(&offset);
            (
                local.minute(),
                local.hour(),
                local.day(),
                local.month(),
                local.weekday().num_days_from_sunday(),
                local.second(),
                local.nanosecond(),
            )
        }
        ParsedTimezone::Iana(timezone) => {
            let local = utc.with_timezone(timezone);
            (
                local.minute(),
                local.hour(),
                local.day(),
                local.month(),
                local.weekday().num_days_from_sunday(),
                local.second(),
                local.nanosecond(),
            )
        }
    };
    Some(LocalDateTimeFields {
        minute,
        hour,
        day_of_month,
        month,
        day_of_week,
        second,
        subsec_nanos,
    })
}

fn timezone_offset_ms_at_tick(timezone: &ParsedTimezone, tick_ms: u64) -> Option<i64> {
    match timezone {
        ParsedTimezone::FixedOffsetMs(offset) => Some(*offset),
        ParsedTimezone::Iana(tz) => {
            let utc = utc_datetime_from_tick_ms(tick_ms)?;
            let local = utc.with_timezone(tz);
            Some(i64::from(local.offset().fix().local_minus_utc()).saturating_mul(1_000))
        }
    }
}

fn latest_due_marker_tick_at_or_before(
    interval_ms: u64,
    timezone: &ParsedTimezone,
    tick_ms: u64,
) -> Option<u64> {
    match timezone {
        ParsedTimezone::FixedOffsetMs(timezone_offset_ms) => {
            latest_due_marker_tick_at_or_before_with_offset(
                interval_ms,
                *timezone_offset_ms,
                tick_ms,
            )
        }
        ParsedTimezone::Iana(_) => {
            let mut timezone_offset_ms = timezone_offset_ms_at_tick(timezone, tick_ms)?;
            for _ in 0..4 {
                let due_tick = latest_due_marker_tick_at_or_before_with_offset(
                    interval_ms,
                    timezone_offset_ms,
                    tick_ms,
                )?;
                let due_offset_ms = timezone_offset_ms_at_tick(timezone, due_tick)?;
                if due_offset_ms == timezone_offset_ms {
                    return Some(due_tick);
                }
                timezone_offset_ms = due_offset_ms;
            }
            latest_due_marker_tick_at_or_before_with_offset(
                interval_ms,
                timezone_offset_ms,
                tick_ms,
            )
        }
    }
}

fn latest_due_marker_tick_at_or_before_with_offset(
    interval_ms: u64,
    timezone_offset_ms: i64,
    tick_ms: u64,
) -> Option<u64> {
    let local_tick = i128::from(tick_ms) + i128::from(timezone_offset_ms);
    if local_tick < 0 {
        return None;
    }
    let local_tick = local_tick as u64;
    let latest_due_local_tick = local_tick - (local_tick % interval_ms);
    let due_tick = i128::from(latest_due_local_tick) - i128::from(timezone_offset_ms);
    if due_tick < 0 {
        return None;
    }
    Some(due_tick as u64)
}

fn canonical_schedule_id(schedule_id: &str) -> String {
    schedule_id.trim().to_string()
}

fn validate_schedule_tick_ms_supported(tick_ms: u64) -> Result<(), ScheduleValidationError> {
    if tick_ms > i64::MAX as u64 {
        return Err(ScheduleValidationError::InvalidTickMs(tick_ms));
    }
    Ok(())
}

fn insert_event_sorted(
    events: &mut Vec<EventEnvelope<UnifiedEvent>>,
    event: EventEnvelope<UnifiedEvent>,
) {
    let insertion_index = events
        .binary_search_by(|existing| {
            existing
                .timestamp_ms
                .cmp(&event.timestamp_ms)
                .then_with(|| existing.event_id.cmp(&event.event_id))
                .then_with(|| existing.source.cmp(&event.source))
        })
        .unwrap_or_else(|index| index);
    events.insert(insertion_index, event);
}

fn latest_due_cron_tick_at_or_before(
    cron: &CronExpression,
    timezone: &ParsedTimezone,
    tick_ms: u64,
) -> Option<u64> {
    let mut candidate = tick_ms - (tick_ms % 60_000);
    for _ in 0..=CRON_LOOKBACK_MINUTES {
        let fields = local_fields_at_tick(timezone, candidate)?;
        if fields.second == 0 && fields.subsec_nanos == 0 && cron.matches(&fields) {
            return Some(candidate);
        }
        candidate = candidate.checked_sub(60_000)?;
    }
    None
}

fn latest_due_tick_at_or_before(
    schedule_id: &str,
    interval: &ParsedInterval,
    timezone: &ParsedTimezone,
    jitter_ms: u64,
    tick_ms: u64,
) -> Option<u64> {
    let jitter_offset_ms =
        deterministic_jitter_offset_ms(schedule_id, jitter_ms, interval.jitter_base_interval_ms());
    let tick_without_jitter = tick_ms.checked_sub(jitter_offset_ms)?;
    let due_without_jitter = match interval {
        ParsedInterval::Marker { interval_ms } => {
            latest_due_marker_tick_at_or_before(*interval_ms, timezone, tick_without_jitter)?
        }
        ParsedInterval::Cron(cron) => {
            latest_due_cron_tick_at_or_before(cron, timezone, tick_without_jitter)?
        }
    };
    due_without_jitter.checked_add(jitter_offset_ms)
}
