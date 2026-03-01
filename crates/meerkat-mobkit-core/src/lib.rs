pub mod auth;
pub mod baseline;
pub mod decisions;
pub mod mocks;
pub mod process;
pub mod protocol;
pub mod rpc;
pub mod runtime;
pub mod types;

pub use auth::{
    extract_hs256_shared_secret, inspect_jwt_header, parse_jwks_json, parse_oidc_discovery_json,
    select_jwk_for_token, validate_jwt_locally, Jwk, JwksDocument, JwtHeaderView,
    JwtValidationConfig, JwtValidationError, OidcContractError, OidcDiscoveryDocument,
    ValidatedJwt,
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
pub use mocks::{MockModuleProcess, MockProcessError};
pub use process::{run_process_json_line, ProcessBoundaryError};
pub use protocol::{parse_module_event_line, parse_unified_event_line, ProtocolParseError};
pub use rpc::{
    handle_console_ingress_json, handle_mobkit_rpc_json, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse, MOBKIT_CONTRACT_VERSION,
};
pub use rpc::{parse_rpc_capabilities, RpcCapabilities, RpcCapabilitiesError};
pub use runtime::{
    build_runtime_decision_state, evaluate_schedules_at_tick, handle_console_rest_json_route,
    materialize_latest_session_rows, materialize_live_session_rows, normalize_event_line,
    route_module_call, route_module_call_rpc_json, route_module_call_rpc_subprocess,
    run_discovered_module_once, run_meerkat_baseline_verification_once, run_module_boundary_once,
    run_rpc_capabilities_boundary_once, session_store_contracts, start_mobkit_runtime,
    start_mobkit_runtime_with_options, BaselineRuntimeError, BigQuerySessionStoreAdapter,
    BigQuerySessionStoreError, ConfigResolutionError, ConsoleRestJsonRequest,
    ConsoleRestJsonResponse, DecisionRuntimeError, ElephantMemoryBackendConfig,
    ElephantMemoryStoreError, GatingAuditEntry, GatingDecideError, GatingDecideRequest,
    GatingDecision, GatingDecisionResult, GatingEvaluateRequest, GatingEvaluateResult,
    GatingOutcome, GatingPendingEntry, GatingRiskTier, JsonFileSessionStore,
    JsonFileSessionStoreError, JsonStoreLockRecord, LifecycleEvent, LifecycleStage,
    MemoryAssertion, MemoryBackendConfig, MemoryConflictSignal, MemoryIndexError,
    MemoryIndexRequest, MemoryIndexResult, MemoryQueryRequest, MemoryQueryResult, MemoryStoreInfo,
    MobkitRuntimeError, MobkitRuntimeHandle, ModuleHealthState, ModuleHealthTransition,
    ModuleRouteError, ModuleRouteRequest, ModuleRouteResponse, NormalizationError, RpcRouteError,
    RpcRuntimeError, RuntimeBoundaryError, RuntimeDecisionInputs, RuntimeDecisionState,
    RuntimeFromConfigError, RuntimeMutationError, RuntimeOptions, RuntimeRoute,
    RuntimeRouteMutationError, RuntimeShutdownReport, ScheduleDefinition, ScheduleDispatch,
    ScheduleDispatchReport, ScheduleEvaluation, ScheduleTrigger, SchedulingSupervisorSignal,
    SessionPersistenceRow, SessionStoreContract, SessionStoreKind, SupervisorReport,
    TrustedOidcRuntimeConfig,
};
pub use types::{
    DiscoverySpec, EventEnvelope, MobKitConfig, ModuleConfig, ModuleEvent, PreSpawnData,
    RestartPolicy, UnifiedEvent,
};
