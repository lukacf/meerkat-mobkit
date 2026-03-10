pub mod auth;
pub mod baseline;
pub mod config_convention;
pub mod decisions;
pub mod governance;
pub mod http_auth;
pub mod http_console;
pub mod http_sse;
pub mod mob_handle_runtime;
pub mod mocks;
pub mod process;
pub mod protocol;
pub mod rpc;
pub mod runtime;
pub mod types;
pub mod unified_runtime;

pub use auth::{
    extract_hs256_shared_secret, inspect_jwt_header, parse_jwks_json, parse_oidc_discovery_json,
    select_jwk_for_token, validate_jwt_locally, Jwk, JwksCache, JwksCacheConfig, JwksCacheError,
    JwksDocument, JwtHeaderView, JwtValidationConfig, JwtValidationError, OidcContractError,
    OidcDiscoveryDocument, ValidatedJwt,
};
pub use baseline::{
    verify_meerkat_baseline_symbols, BaselineVerificationError, BaselineVerificationReport,
    DEFAULT_MEERKAT_REPO, REQUIRED_MEERKAT_SYMBOLS,
};
pub use decisions::{
    enforce_console_route_access, load_trusted_mobkit_modules_from_toml,
    parse_release_metadata_json, validate_bigquery_naming, validate_release_metadata,
    validate_runtime_ops_policy, AuthPolicy, AuthProvider, BigQueryNaming, ConsoleAccessRequest,
    ConsolePolicy, DecisionPolicyError, MetricsPolicy, ReleaseMetadata, RuntimeOpsPolicy,
    REQUIRED_RELEASE_TARGETS,
};
pub use governance::{
    validate_governance_state, validate_phase0_governance_contracts,
    validate_traceability_statuses, GovernanceValidationError, STRICT_TRACEABILITY_STATUSES,
};
pub use http_console::{
    console_frontend_app_js_handler, console_frontend_index_handler, console_frontend_router,
    console_json_handler, console_json_router, console_json_router_with_runtime, ConsoleJsonState,
};
pub use http_auth::{auth_middleware, with_auth_layer};
pub use http_sse::{
    agent_event_sse, agent_events_sse_router,
    mob_events_sse_router, AgentEventSubscribeFn, MobEventSubscribeFn,
};
pub use mob_handle_runtime::{
    MobBootstrapOptions, MobBootstrapSpec, MobMemberSnapshot, MobReconcileOptions,
    MobReconcileReport, MobRuntimeError, RealMobRuntime,
};
pub use mocks::{MockModuleProcess, MockProcessError};
pub use process::{run_process_json_line, ProcessBoundaryError};
pub use protocol::{parse_module_event_line, parse_unified_event_line, ProtocolParseError};
pub use rpc::{
    handle_console_ingress_json, handle_mobkit_rpc_json, handle_unified_rpc_json, JsonRpcError,
    JsonRpcRequest, JsonRpcResponse, MOBKIT_CONTRACT_VERSION,
};
pub use rpc::{parse_rpc_capabilities, RpcCapabilities, RpcCapabilitiesError};
pub use runtime::{
    build_runtime_decision_state, evaluate_schedules_at_tick, handle_console_rest_json_route,
    handle_console_rest_json_route_with_snapshot, materialize_latest_session_rows,
    materialize_live_session_rows, normalize_event_line, route_module_call,
    route_module_call_rpc_json, route_module_call_rpc_subprocess, run_discovered_module_once,
    run_meerkat_baseline_verification_once, run_module_boundary_once,
    run_rpc_capabilities_boundary_once, session_store_contracts, start_mobkit_runtime,
    start_mobkit_runtime_with_options, BaselineRuntimeError, BigQuerySessionStoreAdapter,
    BigQuerySessionStoreError, ConfigResolutionError, ConsoleLiveSnapshot, ConsoleRestJsonRequest,
    ConsoleRestJsonResponse, DecisionRuntimeError, ElephantMemoryBackendConfig,
    ElephantMemoryStoreError, GatingAuditEntry, GatingDecideError, GatingDecideRequest,
    GatingDecision, GatingDecisionResult, GatingEvaluateRequest, GatingEvaluateResult,
    GatingOutcome, GatingPendingEntry, GatingRiskTier, JsonFileSessionStore,
    JsonFileSessionStoreError, JsonStoreLockRecord, LifecycleEvent, LifecycleStage,
    McpBoundaryError, MemoryAssertion, MemoryBackendConfig, MemoryConflictSignal, MemoryIndexError,
    MemoryIndexRequest, MemoryIndexResult, MemoryQueryRequest, MemoryQueryResult, MemoryStoreInfo,
    MobkitRuntimeError, MobkitRuntimeHandle, ModuleHealthState, ModuleHealthTransition,
    ModuleRouteError, ModuleRouteRequest, ModuleRouteResponse, NormalizationError, RpcRouteError,
    RpcRuntimeError, RuntimeBoundaryError, RuntimeDecisionInputs, RuntimeDecisionState,
    RuntimeFromConfigError, RuntimeMutationError, RuntimeOptions, RuntimeRoute,
    RuntimeRouteMutationError, RuntimeShutdownReport, ScheduleDefinition, ScheduleDispatch,
    ScheduleDispatchReport, ScheduleEvaluation, ScheduleRuntimeInjection, ScheduleTrigger,
    SchedulingSupervisorSignal, SessionPersistenceRow, SessionStoreContract, SessionStoreKind,
    SubscribeRequest, SubscribeResponse, SubscribeScope, SupervisorReport,
    TrustedOidcRuntimeConfig, WILDCARD_ROUTE, BigQueryGcConfig,
};
pub use types::{
    AgentDiscoverySpec, DiscoverySpec, EventEnvelope, MobKitConfig, ModuleConfig, ModuleEvent,
    PreSpawnData, RestartPolicy, UnifiedEvent,
};
pub use config_convention::ConventionalPaths;
pub use unified_runtime::{
    discovery_spec_to_spawn_spec, DesiredPeerEdge, DesiredPeerEdgeError, Discovery, ErrorEvent,
    ErrorHook, EventLogConfig, EventLogStore, EventQuery, PersistedEvent, RediscoverReport,
    EdgeDiscovery, EdgeReconcileFailure, PostReconcileHook, PostSpawnHook, PreSpawnContext,
    PreSpawnHook, ShutdownDrainReport, UnifiedRuntime, UnifiedRuntimeBootstrapError,
    UnifiedRuntimeBuilder, UnifiedRuntimeBuilderError, UnifiedRuntimeBuilderField,
    UnifiedRuntimeError, UnifiedRuntimeReconcileEdgesReport, UnifiedRuntimeReconcileError,
    UnifiedRuntimeReconcileReport, UnifiedRuntimeReconcileRoutingReport,
    UnifiedRuntimeRunReport, UnifiedRuntimeShutdownReport,
};
