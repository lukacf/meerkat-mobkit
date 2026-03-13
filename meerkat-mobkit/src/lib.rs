//! MobKit core — orchestration engine for the Meerkat multi-agent runtime.

pub mod auth;
pub mod baseline;
pub mod config_convention;
pub mod decisions;
pub mod governance;
pub mod http_auth;
pub mod http_console;
pub mod http_interactions;
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
    Jwk, JwksCache, JwksCacheConfig, JwksCacheError, JwksDocument, JwtHeaderView,
    JwtValidationConfig, JwtValidationError, OidcContractError, OidcDiscoveryDocument,
    ValidatedJwt, extract_hs256_shared_secret, inspect_jwt_header, parse_jwks_json,
    parse_oidc_discovery_json, select_jwk_for_token, validate_jwt_locally,
};
pub use baseline::{
    BaselineVerificationError, BaselineVerificationReport, DEFAULT_MEERKAT_REPO,
    REQUIRED_MEERKAT_SYMBOLS, verify_meerkat_baseline_symbols,
};
pub use config_convention::ConventionalPaths;
pub use decisions::{
    AuthPolicy, AuthProvider, BigQueryNaming, ConsoleAccessRequest, ConsolePolicy,
    DecisionPolicyError, MetricsPolicy, REQUIRED_RELEASE_TARGETS, ReleaseMetadata,
    RuntimeOpsPolicy, enforce_console_route_access, load_trusted_mobkit_modules_from_toml,
    parse_release_metadata_json, validate_bigquery_naming, validate_release_metadata,
    validate_runtime_ops_policy,
};
pub use governance::{
    GovernanceValidationError, STRICT_TRACEABILITY_STATUSES, validate_governance_state,
    validate_phase0_governance_contracts, validate_traceability_statuses,
};
pub use http_auth::{auth_middleware, with_auth_layer};
pub use http_console::{
    ConsoleJsonState, console_frontend_app_js_handler, console_frontend_index_handler,
    console_frontend_router, console_json_handler, console_json_router,
    console_json_router_with_runtime,
};
pub use http_interactions::interaction_stream_router;
pub use http_sse::{
    AgentEventSubscribeFn, MobEventSubscribeFn, agent_event_sse, agent_events_sse_router,
    mob_events_sse_router,
};
pub use mob_handle_runtime::{
    MobBootstrapOptions, MobBootstrapSpec, MobMemberSnapshot, MobReconcileOptions,
    MobReconcileReport, MobRuntimeError, RealMobRuntime,
};
pub use mocks::{MockModuleProcess, MockProcessError};
pub use process::{ProcessBoundaryError, run_process_json_line};
pub use protocol::{ProtocolParseError, parse_module_event_line, parse_unified_event_line};
pub use rpc::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, MOBKIT_CONTRACT_VERSION,
    handle_console_ingress_json, handle_mobkit_rpc_json, handle_unified_rpc_json,
};
pub use rpc::{RpcCapabilities, RpcCapabilitiesError, parse_rpc_capabilities};
pub use runtime::{
    BaselineRuntimeError, BigQueryGcConfig, BigQuerySessionStoreAdapter, BigQuerySessionStoreError,
    ConfigResolutionError, ConsoleAgentLiveSnapshot, ConsoleLiveSnapshot, ConsoleRestJsonRequest,
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
    TrustedOidcRuntimeConfig, WILDCARD_ROUTE, build_runtime_decision_state,
    evaluate_schedules_at_tick, handle_console_rest_json_route,
    handle_console_rest_json_route_with_snapshot, materialize_latest_session_rows,
    materialize_live_session_rows, normalize_event_line, route_module_call,
    route_module_call_rpc_json, route_module_call_rpc_subprocess, run_discovered_module_once,
    run_meerkat_baseline_verification_once, run_module_boundary_once,
    run_rpc_capabilities_boundary_once, session_store_contracts, start_mobkit_runtime,
    start_mobkit_runtime_with_options,
};
pub use types::{
    AgentDiscoverySpec, DiscoverySpec, EventEnvelope, MobKitConfig, ModuleConfig, ModuleEvent,
    PreSpawnData, RestartPolicy, UnifiedEvent,
};
pub use unified_runtime::{
    DesiredPeerEdge, DesiredPeerEdgeError, Discovery, EdgeDiscovery, EdgeReconcileFailure,
    ErrorEvent, ErrorHook, EventLogConfig, EventLogStore, EventQuery, PersistedEvent,
    PostReconcileHook, PostSpawnHook, PreSpawnContext, PreSpawnHook, RediscoverReport,
    ShutdownDrainReport, UnifiedRuntime, UnifiedRuntimeBootstrapError, UnifiedRuntimeBuilder,
    UnifiedRuntimeBuilderError, UnifiedRuntimeBuilderField, UnifiedRuntimeError,
    UnifiedRuntimeReconcileEdgesReport, UnifiedRuntimeReconcileError,
    UnifiedRuntimeReconcileReport, UnifiedRuntimeReconcileRoutingReport, UnifiedRuntimeRunReport,
    UnifiedRuntimeShutdownReport, discovery_spec_to_spawn_spec,
};
