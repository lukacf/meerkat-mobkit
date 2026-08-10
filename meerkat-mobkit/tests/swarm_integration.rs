#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::collapsible_if,
    clippy::redundant_clone,
    clippy::needless_raw_string_hashes,
    clippy::single_match,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_pattern_matching,
    clippy::ignored_unit_patterns,
    clippy::clone_on_copy,
    clippy::manual_assert,
    clippy::unwrap_in_result,
    clippy::useless_vec,
    clippy::unnecessary_semicolon
)]
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use meerkat::{AgentFactory, Config, build_ephemeral_service};
use meerkat_client::TestClient;
use meerkat_core::{
    AppendSystemContextRequest, AppendSystemContextResult, CommsRuntime, CreateSessionRequest,
    EventStream, RunResult, SessionControlError, SessionError, SessionHistoryPage,
    SessionHistoryQuery, SessionId, SessionQuery, SessionService, SessionServiceCommsExt,
    SessionServiceControlExt, SessionServiceHistoryExt, SessionSummary, SessionView,
    StartTurnRequest, StreamError,
};
use meerkat_mob::{MobDefinition, MobId, MobSessionService, MobState, MobStorage, SpawnMemberSpec};
use meerkat_mobkit::{
    AuthPolicy, BigQueryNaming, ConfigResolutionError, ConsolePolicy, DiscoverySpec, EventEnvelope,
    LifecycleStage, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig, MobkitRuntimeError,
    ModuleConfig, ModuleHealthState, ModuleRouteError, ModuleRouteRequest, ModuleRouteResponse,
    NormalizationError, PreSpawnData, RestartPolicy, RpcRouteError, RuntimeDecisionInputs,
    RuntimeOpsPolicy, RuntimeOptions, ScheduleDefinition, TrustedOidcRuntimeConfig, UnifiedEvent,
    UnifiedRuntime, UnifiedRuntimeBootstrapError, build_runtime_decision_state,
    normalize_event_line, route_module_call, route_module_call_rpc_json,
    route_module_call_rpc_subprocess, start_mobkit_runtime, start_mobkit_runtime_with_options,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

fn shell_module(
    id: &str,
    script: &str,
    restart_policy: RestartPolicy,
) -> meerkat_mobkit::ModuleConfig {
    meerkat_mobkit::ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        restart_policy,
    }
}

const BOUNDARY_ENV_KEY: &str = "MOBKIT_MODULE_BOUNDARY";
const BOUNDARY_ENV_VALUE_MCP: &str = "mcp";

struct UnifiedRuntimeFixture {
    _temp_dir: tempfile::TempDir,
    runtime: UnifiedRuntime,
}

#[derive(Clone)]
struct CheckpointerCancelProbeSessionService {
    inner: Arc<dyn MobSessionService>,
    cancel_calls: Arc<AtomicUsize>,
}

impl CheckpointerCancelProbeSessionService {
    fn new(inner: Arc<dyn MobSessionService>) -> Self {
        Self {
            inner,
            cancel_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn cancel_calls(&self) -> usize {
        self.cancel_calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl SessionService for CheckpointerCancelProbeSessionService {
    async fn create_session(&self, req: CreateSessionRequest) -> Result<RunResult, SessionError> {
        self.inner.create_session(req).await
    }

    async fn start_turn(
        &self,
        id: &SessionId,
        req: StartTurnRequest,
    ) -> Result<RunResult, SessionError> {
        self.inner.start_turn(id, req).await
    }

    async fn interrupt(&self, id: &SessionId) -> Result<(), SessionError> {
        self.inner.interrupt(id).await
    }

    // meerkat 0.8.22 adds the exact-run hard cancel to `SessionService` with an
    // `Err(Unsupported)` default. The inner ephemeral service implements it for
    // real, so inheriting the default here would let this probe answer
    // "unsupported" on its own authority and hide the inner service's exact-run
    // interrupt (same forwarding class as the lib's
    // `delegate_mob_session_service!`).
    async fn interrupt_run_if_current(
        &self,
        id: &SessionId,
        expected_run_id: &meerkat_core::lifecycle::RunId,
    ) -> Result<bool, SessionError> {
        self.inner.interrupt_run_if_current(id, expected_run_id).await
    }

    async fn read(&self, id: &SessionId) -> Result<SessionView, SessionError> {
        self.inner.read(id).await
    }

    async fn list(&self, query: SessionQuery) -> Result<Vec<SessionSummary>, SessionError> {
        self.inner.list(query).await
    }

    async fn archive(&self, id: &SessionId) -> Result<(), SessionError> {
        self.inner.archive(id).await
    }

    async fn subscribe_session_events(&self, id: &SessionId) -> Result<EventStream, StreamError> {
        SessionService::subscribe_session_events(self.inner.as_ref(), id).await
    }
}

#[async_trait::async_trait]
impl SessionServiceCommsExt for CheckpointerCancelProbeSessionService {
    async fn comms_runtime(&self, session_id: &SessionId) -> Option<Arc<dyn CommsRuntime>> {
        self.inner.comms_runtime(session_id).await
    }
}

#[async_trait::async_trait]
impl SessionServiceControlExt for CheckpointerCancelProbeSessionService {
    async fn append_system_context(
        &self,
        id: &SessionId,
        req: AppendSystemContextRequest,
    ) -> Result<AppendSystemContextResult, SessionControlError> {
        self.inner.append_system_context(id, req).await
    }
}

#[async_trait::async_trait]
impl SessionServiceHistoryExt for CheckpointerCancelProbeSessionService {
    async fn read_history(
        &self,
        id: &SessionId,
        query: SessionHistoryQuery,
    ) -> Result<SessionHistoryPage, SessionError> {
        self.inner.read_history(id, query).await
    }
}

#[async_trait::async_trait]
impl MobSessionService for CheckpointerCancelProbeSessionService {
    // The verdict below is issued by `inner`, so the authority bundle it
    // carries must come from the SAME owner: the mob provisioner calls
    // `revalidate_session_resume_authority` on the service it holds (this
    // wrapper), and that trait default compares `self.observe_...` against the
    // bundle the verdict carried. Hardcoding an empty bundle here while
    // forwarding the verdict would reject every persistent-inner resume as
    // `AuthorityChangedDuringMaterialization`. Zero delta for the ephemeral
    // inner used today, whose own observation is exactly the empty bundle.
    async fn observe_session_resume_authority(
        &self,
        session_id: &meerkat::SessionId,
    ) -> Result<meerkat_mob::SessionResumeAuthority, meerkat::SessionError> {
        self.inner.observe_session_resume_authority(session_id).await
    }

    // meerkat 0.8.22 DELETED the required `prepare_session_for_resume` hook
    // this probe used to pass through. Its durable-tail convergence now lives
    // only inside `PersistentSessionService`'s override of
    // `materialize_session_resume_verdict`; the TRAIT DEFAULT of that method is
    // the non-persistent composition (bracketed read-only observations plus a
    // `NonPersistent` receipt, converging nothing). Dropping the dead
    // passthrough and stopping there would compile while silently resuming a
    // persistent inner from stale committed authority, so the delegation moves
    // to the method that now owns convergence. This wrapper only counts
    // `cancel_all_checkpointers`; it has no projection of its own to re-apply
    // to the authorized body, so the verdict is forwarded verbatim.
    //
    // The receipt this verdict now carries is consumed by the two
    // `..._actor_witness_...` creates, which this probe has never forwarded
    // (pre-existing at 0.8.21, when they took no receipt). Left inheriting
    // their defaults deliberately: both refuse with `Err(Unsupported)`, and
    // `execute_session_actor_materialization_under_runtime_turn_boundary` is
    // the sole lowering owner with no non-witness fallback, so an unconsumed
    // receipt fails loudly instead of being silently dropped. This probe's
    // bootstrap-failure assertion never reaches actor materialization anyway.
    async fn materialize_session_resume_verdict(
        &self,
        session_id: &SessionId,
    ) -> Result<meerkat_mob::SessionResumeVerdict, SessionError> {
        self.inner
            .materialize_session_resume_verdict(session_id)
            .await
    }

    async fn create_session_under_runtime_turn_boundary(
        &self,
        req: CreateSessionRequest,
    ) -> Result<RunResult, SessionError> {
        self.inner
            .create_session_under_runtime_turn_boundary(req)
            .await
    }

    async fn acknowledge_committed_runtime_session_boundary_under_turn_finalization_boundary(
        &self,
        session_id: &SessionId,
        authority: &meerkat_core::CommittedSessionBoundaryAuthority,
    ) -> Result<(), SessionError> {
        self.inner
            .acknowledge_committed_runtime_session_boundary_under_turn_finalization_boundary(
                session_id, authority,
            )
            .await
    }

    async fn load_session_for_resume(
        &self,
        session_id: &SessionId,
    ) -> Result<meerkat_mob::ResumeSessionLoad, SessionError> {
        self.inner.load_session_for_resume(session_id).await
    }

    // meerkat 0.8.22: durable transcript fork moved onto the session service
    // with an `Err(Unsupported)` default. Not forwarding would make this probe
    // claim a persistent inner has no fork authority at all.
    async fn fork_persisted_session(
        &self,
        source_session_id: &SessionId,
        message_count: Option<usize>,
        tool_access_policy: Option<meerkat_core::ops::ToolAccessPolicy>,
        target: meerkat_core::DurableSessionForkTarget,
    ) -> Result<meerkat_core::SessionForkResult, SessionError> {
        self.inner
            .fork_persisted_session(source_session_id, message_count, tool_access_policy, target)
            .await
    }

    // meerkat 0.8.22: exact-run machine-authorized hard cancel, trait default
    // `Err(Unsupported)`. The mob provisioner's interrupt handle calls this on
    // the service it was handed (this wrapper) and absorbs only `NotRunning`,
    // so an unforwarded default turns every exact-run cancel into a control
    // failure even though the ephemeral inner implements it.
    async fn interrupt_run_with_machine_authority(
        &self,
        session_id: &SessionId,
        expected_run_id: &meerkat_core::lifecycle::RunId,
        authority: meerkat_runtime::MachineSessionControlAuthority,
    ) -> Result<bool, SessionError> {
        self.inner
            .interrupt_run_with_machine_authority(session_id, expected_run_id, authority)
            .await
    }

    async fn archive_with_mob_lifecycle_authority_under_runtime_turn_boundary(
        &self,
        session_id: &SessionId,
    ) -> Result<(), SessionError> {
        self.inner
            .archive_with_mob_lifecycle_authority_under_runtime_turn_boundary(session_id)
            .await
    }

    // meerkat 0.8.22: member-session disposal moved from the boundary archive
    // above to this deadline-aware sibling, whose trait default is
    // `Err(Unsupported)`. The ephemeral inner implements it explicitly, so an
    // unforwarded default would make every member retirement archive through
    // this probe fail and wedge the roster anchor in `retiring`.
    async fn archive_with_mob_lifecycle_authority_under_runtime_turn_boundary_before(
        &self,
        session_id: &SessionId,
        deadline: meerkat_core::time_compat::Instant,
    ) -> Result<(), SessionError> {
        self.inner
            .archive_with_mob_lifecycle_authority_under_runtime_turn_boundary_before(
                session_id, deadline,
            )
            .await
    }

    async fn discard_live_session_under_runtime_turn_boundary(
        &self,
        session_id: &SessionId,
    ) -> Result<(), SessionError> {
        self.inner
            .discard_live_session_under_runtime_turn_boundary(session_id)
            .await
    }

    fn supports_persistent_sessions(&self) -> bool {
        self.inner.supports_persistent_sessions()
    }

    fn runtime_adapter(&self) -> Option<Arc<meerkat_runtime::MeerkatMachine>> {
        self.inner.runtime_adapter()
    }

    async fn session_belongs_to_mob(&self, session_id: &SessionId, mob_id: &MobId) -> bool {
        self.inner.session_belongs_to_mob(session_id, mob_id).await
    }

    async fn cancel_all_checkpointers(&self) {
        self.cancel_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.cancel_all_checkpointers().await;
    }

    async fn rearm_all_checkpointers(&self) {
        self.inner.rearm_all_checkpointers().await;
    }
}

fn fixture_binary_path() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_mcp_fixture") {
        return PathBuf::from(path);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root");
    let binary_path = workspace_root
        .join("target")
        .join("debug")
        .join("mcp_fixture");
    if binary_path.exists() {
        return binary_path;
    }

    let status = Command::new("cargo")
        .args(["build", "-p", "meerkat-mobkit", "--bin", "mcp_fixture"])
        .current_dir(workspace_root)
        .status()
        .expect("build mcp_fixture");
    assert!(status.success(), "building mcp_fixture must succeed");
    binary_path
}

fn fixture_module(id: &str, fixture_binary: &Path) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: fixture_binary.display().to_string(),
        args: vec!["--module".to_string(), id.to_string()],
        restart_policy: RestartPolicy::Never,
    }
}

fn mcp_env(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = vec![(
        BOUNDARY_ENV_KEY.to_string(),
        BOUNDARY_ENV_VALUE_MCP.to_string(),
    )];
    env.extend(
        extra
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string())),
    );
    env
}

fn spawn_spec(profile: &str, member_id: &str) -> SpawnMemberSpec {
    SpawnMemberSpec::from_wire(
        profile.to_string(),
        member_id.to_string(),
        Some(format!("You are {member_id}. Keep responses concise.").into()),
        None,
        None,
    )
}

fn build_phase1_session_service(temp_dir: &tempfile::TempDir) -> Arc<dyn MobSessionService> {
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = AgentFactory::new(&session_path).comms(true);
    Arc::new(build_ephemeral_service(factory, Config::default(), 16))
}

fn build_phase1_mob_spec_with_session_service(
    session_service: Arc<dyn MobSessionService>,
) -> MobBootstrapSpec {
    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "phase1-unified-mob"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true

[profiles.worker.tools]
comms = true
"#,
    )
    .expect("parse mob definition");

    MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service).with_options(
        MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(TestClient::default())),
        },
    )
}

fn build_phase1_mob_spec(temp_dir: &tempfile::TempDir) -> MobBootstrapSpec {
    build_phase1_mob_spec_with_session_service(build_phase1_session_service(temp_dir))
}

fn reference_runtime_decision_state() -> meerkat_mobkit::RuntimeDecisionState {
    build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "phase1_reference_dataset".to_string(),
            table: "phase1_reference_table".to_string(),
        },
        trusted_mobkit_toml: trusted_modules_toml(),
        auth: AuthPolicy::default(),
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy {
            require_app_auth: false,
            ..ConsolePolicy::default()
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: include_str!("../assets/release-targets.json").to_string(),
    })
    .expect("build decision state")
}

fn trusted_modules_toml() -> String {
    r#"
[[modules]]
id = "router"
command = "router-bin"
args = ["--mode", "fast"]
restart_policy = "always"
"#
    .to_string()
}

fn trusted_oidc() -> TrustedOidcRuntimeConfig {
    TrustedOidcRuntimeConfig {
        discovery_json:
            r#"{"issuer":"https://trusted.mobkit.local","jwks_uri":"https://trusted.mobkit.local/.well-known/jwks.json"}"#
                .to_string(),
        jwks_json: r#"{"keys":[{"kid":"kid-current","kty":"oct","alg":"HS256","k":"cGhhc2U3LXRydXN0ZWQtY3VycmVudC1zZWNyZXQ"}]}"#
            .to_string(),
        audience: "meerkat-console".to_string(),
    }
}

async fn http_get_response(address: SocketAddr, path: &str) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match TcpStream::connect(address).await {
            Ok(mut stream) => {
                let request =
                    format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
                stream
                    .write_all(request.as_bytes())
                    .await
                    .expect("write request");
                stream.flush().await.expect("flush request");

                let mut bytes = Vec::new();
                tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut bytes))
                    .await
                    .expect("read response within timeout")
                    .expect("read response bytes");
                return String::from_utf8(bytes).expect("utf8 response");
            }
            Err(error) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "failed to connect to reference app at {address}: {error}"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
}

async fn build_unified_runtime_fixture(module_config: MobKitConfig) -> UnifiedRuntimeFixture {
    Box::pin(build_unified_runtime_fixture_with_agent_events(
        module_config,
        Vec::new(),
    ))
    .await
}

async fn build_unified_runtime_fixture_with_agent_events(
    module_config: MobKitConfig,
    module_agent_events: Vec<EventEnvelope<UnifiedEvent>>,
) -> UnifiedRuntimeFixture {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = Box::pin(
        UnifiedRuntime::builder()
            .mob_spec(build_phase1_mob_spec(&temp_dir))
            .module_config(module_config)
            .module_agent_events(module_agent_events)
            .timeout(Duration::from_secs(2))
            .build(),
    )
    .await
    .expect("bootstrap unified runtime");

    UnifiedRuntimeFixture {
        _temp_dir: temp_dir,
        runtime,
    }
}

#[tokio::test]
#[ignore]
async fn unified_bootstrap_failure_rolls_back_started_mob_runtime() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let probe = Arc::new(CheckpointerCancelProbeSessionService::new(
        build_phase1_session_service(&temp_dir),
    ));
    let session_service: Arc<dyn MobSessionService> = probe.clone();

    let config = MobKitConfig {
        modules: vec![],
        discovery: DiscoverySpec {
            namespace: "phase1-bootstrap-failure".to_string(),
            modules: vec!["missing-module".to_string()],
        },
        pre_spawn: vec![],
    };

    let error = match UnifiedRuntime::bootstrap(
        build_phase1_mob_spec_with_session_service(session_service),
        config,
        Duration::from_millis(250),
    )
    .await
    {
        Ok(_) => panic!("bootstrap should fail when discovered module is not configured"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        UnifiedRuntimeBootstrapError::Module(MobkitRuntimeError::Config(
            ConfigResolutionError::ModuleNotConfigured(module_id)
        )) if module_id == "missing-module"
    ));
    assert_eq!(
        probe.cancel_calls(),
        1,
        "module bootstrap failure must rollback started mob runtime"
    );
}

#[test]
#[ignore]
fn req_001_startup_ordering_and_graceful_shutdown_kills_tracked_children() {
    let config = MobKitConfig {
        modules: vec![shell_module(
            "mod-a",
            r#"printf '%s\n' "{\"event_id\":\"mod-a-evt\",\"source\":\"module\",\"timestamp_ms\":20,\"event\":{\"kind\":\"module\",\"module\":\"mod-a\",\"event_type\":\"ready\",\"payload\":{\"ok\":true,\"pid\":$$}}}"; exec sleep 30"#,
            RestartPolicy::Never,
        )],
        discovery: meerkat_mobkit::DiscoverySpec {
            namespace: "default".to_string(),
            modules: vec!["mod-a".to_string()],
        },
        pre_spawn: vec![],
    };

    let mut runtime =
        start_mobkit_runtime(config, vec![], Duration::from_secs(1)).expect("runtime starts");

    assert_eq!(
        runtime
            .lifecycle_events()
            .iter()
            .map(|event| event.stage.clone())
            .collect::<Vec<_>>(),
        vec![
            LifecycleStage::MobStarted,
            LifecycleStage::ModulesStarted,
            LifecycleStage::MergedStreamStarted,
        ]
    );

    let pid = runtime
        .merged_events()
        .iter()
        .find_map(|event| match &event.event {
            UnifiedEvent::Module(module) if module.module == "mod-a" => {
                module.payload.get("pid").and_then(|value| value.as_i64())
            }
            _ => None,
        })
        .expect("module pid should be present in payload");

    let shutdown = runtime.shutdown();
    assert_eq!(shutdown.orphan_processes, 0);
    assert_eq!(shutdown.terminated_modules, vec!["mod-a".to_string()]);
    assert!(!runtime.is_running());

    // OS-level proof that the module process is not alive after shutdown.
    let kill_status = Command::new("sh")
        .args(["-c", &format!("kill -0 {pid}")])
        .status()
        .expect("run kill -0");
    assert!(
        !kill_status.success(),
        "module process {pid} is still alive after shutdown"
    );

    assert_eq!(
        runtime
            .lifecycle_events()
            .iter()
            .map(|event| event.stage.clone())
            .collect::<Vec<_>>(),
        vec![
            LifecycleStage::MobStarted,
            LifecycleStage::ModulesStarted,
            LifecycleStage::MergedStreamStarted,
            LifecycleStage::ShutdownRequested,
            LifecycleStage::ShutdownComplete,
        ]
    );
}

#[test]
#[ignore]
fn req_002_supervisor_transitions_and_restart_policy_enforced_with_budgets() {
    let temp = tempfile::tempdir().expect("temp dir");
    let on_failure_state = temp.path().join("on-failure-state");

    let on_failure_script = format!(
        "if [ ! -f '{}' ]; then echo first > '{}'; exit 1; fi; if ! grep -q second '{}'; then echo second > '{}'; exit 1; fi; printf '%s\\n' '{{\"event_id\":\"on-failure-healthy\",\"source\":\"module\",\"timestamp_ms\":30,\"event\":{{\"kind\":\"module\",\"module\":\"on-failure\",\"event_type\":\"ready\",\"payload\":{{\"attempt\":3}}}}}}'",
        on_failure_state.display(),
        on_failure_state.display(),
        on_failure_state.display(),
        on_failure_state.display()
    );

    let always_script = "printf '%s\\n' '{\"event_id\":\"always-healthy\",\"source\":\"module\",\"timestamp_ms\":31,\"event\":{\"kind\":\"module\",\"module\":\"always\",\"event_type\":\"ready\",\"payload\":{\"attempt\":1}}}'".to_string();

    let config = MobKitConfig {
        modules: vec![
            shell_module("never", "exit 1", RestartPolicy::Never),
            shell_module("on-failure", &on_failure_script, RestartPolicy::OnFailure),
            shell_module("always", &always_script, RestartPolicy::Always),
        ],
        discovery: meerkat_mobkit::DiscoverySpec {
            namespace: "default".to_string(),
            modules: vec![
                "never".to_string(),
                "on-failure".to_string(),
                "always".to_string(),
            ],
        },
        pre_spawn: vec![],
    };

    let runtime = start_mobkit_runtime_with_options(
        config,
        vec![],
        Duration::from_secs(1),
        RuntimeOptions {
            on_failure_retry_budget: 2,
            always_restart_budget: 2,
            ..RuntimeOptions::default()
        },
    )
    .expect("runtime starts with supervisor transitions");

    let never = runtime
        .supervisor_report()
        .transitions
        .iter()
        .filter(|transition| transition.module_id == "never")
        .map(|transition| transition.to.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        never,
        vec![
            ModuleHealthState::Starting,
            ModuleHealthState::Failed,
            ModuleHealthState::Stopped,
        ]
    );

    let on_failure = runtime
        .supervisor_report()
        .transitions
        .iter()
        .filter(|transition| transition.module_id == "on-failure")
        .map(|transition| transition.to.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        on_failure,
        vec![
            ModuleHealthState::Starting,
            ModuleHealthState::Failed,
            ModuleHealthState::Restarting,
            ModuleHealthState::Failed,
            ModuleHealthState::Restarting,
            ModuleHealthState::Healthy,
        ]
    );

    let always = runtime
        .supervisor_report()
        .transitions
        .iter()
        .filter(|transition| transition.module_id == "always")
        .map(|transition| transition.to.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        always,
        vec![
            ModuleHealthState::Starting,
            ModuleHealthState::Healthy,
            ModuleHealthState::Restarting,
            ModuleHealthState::Healthy,
            ModuleHealthState::Restarting,
            ModuleHealthState::Healthy,
        ]
    );
}

#[test]
#[ignore]
fn req_003_event_bus_merges_agent_and_module_events_with_deterministic_order() {
    let config = MobKitConfig {
        modules: vec![
            shell_module(
                "mod-a",
                r#"printf '%s\n' '{"event_id":"evt-module-a","source":"module","timestamp_ms":20,"event":{"kind":"module","module":"mod-a","event_type":"ready","payload":{"m":"a"}}}'"#,
                RestartPolicy::Never,
            ),
            shell_module(
                "mod-b",
                r#"printf '%s\n' '{"event_id":"evt-module-b","source":"module","timestamp_ms":20,"event":{"kind":"module","module":"mod-b","event_type":"ready","payload":{"m":"b"}}}'"#,
                RestartPolicy::Never,
            ),
        ],
        discovery: meerkat_mobkit::DiscoverySpec {
            namespace: "default".to_string(),
            modules: vec!["mod-a".to_string(), "mod-b".to_string()],
        },
        pre_spawn: vec![],
    };

    let agent_events = vec![
        EventEnvelope {
            event_id: "evt-agent-early".to_string(),
            source: "agent".to_string(),
            timestamp_ms: 10,
            event: UnifiedEvent::Agent {
                agent_id: "a-1".to_string(),
                event_type: "heartbeat".to_string(),
                payload: None,
            },
        },
        EventEnvelope {
            event_id: "evt-agent-mid".to_string(),
            source: "agent".to_string(),
            timestamp_ms: 20,
            event: UnifiedEvent::Agent {
                agent_id: "a-2".to_string(),
                event_type: "heartbeat".to_string(),
                payload: None,
            },
        },
    ];

    let runtime =
        start_mobkit_runtime(config, agent_events, Duration::from_secs(1)).expect("runtime starts");

    assert_eq!(
        runtime
            .merged_events()
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>(),
        vec![
            "evt-agent-early".to_string(),
            "evt-agent-mid".to_string(),
            "evt-module-a".to_string(),
            "evt-module-b".to_string(),
        ]
    );
    assert!(matches!(
        runtime.merged_events()[0].event,
        UnifiedEvent::Agent { .. }
    ));
    assert!(matches!(
        runtime.merged_events()[2].event,
        UnifiedEvent::Module(_)
    ));
}

#[test]
fn req_003_attribution_integrity_rejects_source_event_mismatch() {
    let mismatched = json!({
        "event_id": "evt-bad",
        "source": "agent",
        "timestamp_ms": 7,
        "event": {
            "kind": "module",
            "module": "mod-x",
            "event_type": "ready",
            "payload": {"ok": true}
        }
    })
    .to_string();

    let err = normalize_event_line(&mismatched).expect_err("mismatch should fail");
    assert_eq!(
        err,
        NormalizationError::SourceMismatch {
            expected: "module",
            got: "agent".to_string(),
        }
    );
}

#[test]
#[ignore]
fn req_004_and_req_005_router_parity_library_and_rpc_with_typed_unloaded_error() {
    let config = MobKitConfig {
        modules: vec![shell_module(
            "router-mod",
            r#"printf '%s\n' '{"event_id":"evt-router","source":"module","timestamp_ms":55,"event":{"kind":"module","module":"router-mod","event_type":"response","payload":{"ok":true,"via":"module"}}}'"#,
            RestartPolicy::Never,
        )],
        discovery: meerkat_mobkit::DiscoverySpec {
            namespace: "default".to_string(),
            modules: vec!["router-mod".to_string()],
        },
        pre_spawn: vec![],
    };

    let runtime =
        start_mobkit_runtime(config, vec![], Duration::from_secs(1)).expect("runtime starts");

    let request = ModuleRouteRequest {
        module_id: "router-mod".to_string(),
        method: "echo".to_string(),
        params: json!({"msg":"hello"}),
    };

    let library_response = route_module_call(&runtime, &request, Duration::from_secs(1))
        .expect("library router call succeeds");
    assert_eq!(library_response.module_id, "router-mod");
    assert_eq!(library_response.method, "echo");
    assert_eq!(
        library_response.payload,
        json!({"ok": true, "via": "module"})
    );

    let rpc_response_json = route_module_call_rpc_json(
        &runtime,
        &serde_json::to_string(&request).expect("serialize request"),
        Duration::from_secs(1),
    )
    .expect("rpc wrapper succeeds");
    let rpc_response: ModuleRouteResponse =
        serde_json::from_str(&rpc_response_json).expect("deserialize rpc response");
    assert_eq!(rpc_response.module_id, "router-mod");
    assert_eq!(rpc_response.method, "echo");
    assert_eq!(rpc_response.payload, json!({"ok": true, "via": "module"}));

    let rpc_subprocess_response_json = route_module_call_rpc_subprocess(
        &runtime,
        "sh",
        &[
            "-c".to_string(),
            format!(
                "printf '%s\\n' '{}'",
                serde_json::to_string(&request).expect("serialize request")
            ),
        ],
        &[],
        Duration::from_secs(1),
    )
    .expect("rpc subprocess boundary succeeds");
    let rpc_subprocess_response: ModuleRouteResponse =
        serde_json::from_str(&rpc_subprocess_response_json)
            .expect("deserialize subprocess rpc response");
    assert_eq!(rpc_subprocess_response.module_id, "router-mod");
    assert_eq!(rpc_subprocess_response.method, "echo");
    assert_eq!(
        rpc_subprocess_response.payload,
        json!({"ok": true, "via": "module"})
    );

    let missing = ModuleRouteRequest {
        module_id: "missing".to_string(),
        method: "echo".to_string(),
        params: json!({}),
    };

    let library_error = route_module_call(&runtime, &missing, Duration::from_secs(1))
        .expect_err("missing module should fail");
    assert_eq!(
        library_error,
        ModuleRouteError::UnloadedModule("missing".to_string())
    );

    let rpc_error = route_module_call_rpc_json(
        &runtime,
        &serde_json::to_string(&missing).expect("serialize request"),
        Duration::from_secs(1),
    )
    .expect_err("rpc missing module should fail");
    assert_eq!(
        rpc_error,
        RpcRouteError::Route(ModuleRouteError::UnloadedModule("missing".to_string()))
    );

    let rpc_subprocess_error = route_module_call_rpc_subprocess(
        &runtime,
        "sh",
        &[
            "-c".to_string(),
            format!(
                "printf '%s\\n' '{}'",
                serde_json::to_string(&missing).expect("serialize request")
            ),
        ],
        &[],
        Duration::from_secs(1),
    )
    .expect_err("rpc subprocess missing module should fail");
    assert_eq!(
        rpc_subprocess_error,
        RpcRouteError::Route(ModuleRouteError::UnloadedModule("missing".to_string()))
    );
}

#[test]
fn req_001_config_error_when_discovery_references_unknown_module() {
    let config = MobKitConfig {
        modules: vec![],
        discovery: meerkat_mobkit::DiscoverySpec {
            namespace: "default".to_string(),
            modules: vec!["ghost".to_string()],
        },
        pre_spawn: vec![],
    };

    let error = start_mobkit_runtime(config, vec![], Duration::from_secs(1))
        .expect_err("unknown module should fail startup");
    assert_eq!(
        error,
        meerkat_mobkit::MobkitRuntimeError::Config(ConfigResolutionError::ModuleNotConfigured(
            "ghost".to_string()
        ))
    );
}

#[tokio::test]
#[ignore]
async fn req_001_unified_owner_starts_and_shuts_down_from_single_object() {
    let config = MobKitConfig {
        modules: vec![shell_module(
            "mod-a",
            r#"printf '%s\n' "{\"event_id\":\"mod-a-unified\",\"source\":\"module\",\"timestamp_ms\":20,\"event\":{\"kind\":\"module\",\"module\":\"mod-a\",\"event_type\":\"ready\",\"payload\":{\"ok\":true}}}"; exec sleep 30"#,
            RestartPolicy::Never,
        )],
        discovery: DiscoverySpec {
            namespace: "phase1-unified".to_string(),
            modules: vec!["mod-a".to_string()],
        },
        pre_spawn: vec![],
    };

    let fixture = Box::pin(build_unified_runtime_fixture(config)).await;
    assert_eq!(
        fixture.runtime.mob_handle().status().await.unwrap(),
        MobState::Running
    );
    assert!(fixture.runtime.module_is_running().await);
    assert_eq!(
        fixture.runtime.loaded_modules().await,
        vec!["mod-a".to_string()]
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert_eq!(shutdown.module_shutdown.orphan_processes, 0);
    assert_eq!(
        shutdown.module_shutdown.terminated_modules,
        vec!["mod-a".to_string()]
    );
    assert!(
        shutdown.mob_stop.is_ok(),
        "mob stop failed at teardown: {:?}",
        shutdown.mob_stop
    );
    assert!(!fixture.runtime.module_is_running().await);
    assert_eq!(
        fixture.runtime.mob_handle().status().await.unwrap(),
        MobState::Stopped
    );
}

#[tokio::test]
#[ignore]
async fn choke_001_unified_subscribe_merges_module_and_agent_events() {
    let config = MobKitConfig {
        modules: vec![shell_module(
            "mod-interaction",
            r#"printf '%s\n' '{"event_id":"evt-module-interaction","source":"module","timestamp_ms":10,"event":{"kind":"module","module":"mod-interaction","event_type":"ready","payload":{"ok":true}}}'"#,
            RestartPolicy::Never,
        )],
        discovery: DiscoverySpec {
            namespace: "phase1-unified".to_string(),
            modules: vec!["mod-interaction".to_string()],
        },
        pre_spawn: vec![],
    };

    let fixture = Box::pin(build_unified_runtime_fixture_with_agent_events(
        config,
        vec![EventEnvelope {
            event_id: "evt-agent-worker-1".to_string(),
            source: "agent".to_string(),
            timestamp_ms: 11,
            event: UnifiedEvent::Agent {
                agent_id: "worker-1".to_string(),
                event_type: "interaction_complete".to_string(),
                payload: Some(json!({"text":"seeded worker event"})),
            },
        }],
    ))
    .await;

    let ready_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let response = fixture
            .runtime
            .subscribe_events(meerkat_mobkit::runtime::SubscribeRequest {
                scope: meerkat_mobkit::runtime::SubscribeScope::Mob,
                last_event_id: None,
                agent_id: None,
            })
            .await
            .expect("subscribe should succeed");
        let has_agent = response.events.iter().any(|event| {
            matches!(&event.event, UnifiedEvent::Agent { agent_id, .. } if agent_id == "worker-1")
        });
        if has_agent {
            break;
        }

        assert!(
            tokio::time::Instant::now() < ready_deadline,
            "expected unified subscribe to include worker-1 agent events"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    fixture
        .runtime
        .dispatch_schedule_tick(
            &[ScheduleDefinition {
                schedule_id: "phase1-merge".to_string(),
                interval: "*/1m".to_string(),
                timezone: "UTC".to_string(),
                enabled: true,
                jitter_ms: 0,
                catch_up: false,
            }],
            9_000_000_000_000_000_000,
        )
        .await
        .expect("dispatch should add a late module event");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let subscribed = loop {
        let response = fixture
            .runtime
            .subscribe_events(meerkat_mobkit::runtime::SubscribeRequest {
                scope: meerkat_mobkit::runtime::SubscribeScope::Mob,
                last_event_id: None,
                agent_id: None,
            })
            .await
            .expect("subscribe should succeed");
        let has_module = response
            .events
            .iter()
            .any(|event| matches!(&event.event, UnifiedEvent::Module(_)));
        let has_agent = response.events.iter().any(|event| {
            matches!(&event.event, UnifiedEvent::Agent { agent_id, .. } if agent_id == "worker-1")
        });
        if has_module && has_agent {
            break response;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "expected unified subscribe to include both module + agent events"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    assert!(
        subscribed
            .events
            .iter()
            .any(|event| { matches!(&event.event, UnifiedEvent::Module(_)) })
    );
    assert!(subscribed.events.iter().any(|event| {
        matches!(&event.event, UnifiedEvent::Agent { agent_id, .. } if agent_id == "worker-1")
    }));

    let shutdown = fixture.runtime.shutdown().await;
    assert!(
        shutdown.mob_stop.is_ok(),
        "mob stop failed at teardown: {:?}",
        shutdown.mob_stop
    );
}

#[tokio::test]
#[ignore]
async fn choke_002_unified_dispatch_executes_mob_runtime_injection_success_path() {
    let fixture_binary = fixture_binary_path();
    let config = MobKitConfig {
        modules: vec![fixture_module("scheduling", &fixture_binary)],
        discovery: DiscoverySpec {
            namespace: "phase1-unified".to_string(),
            modules: vec!["scheduling".to_string()],
        },
        pre_spawn: vec![PreSpawnData {
            module_id: "scheduling".to_string(),
            env: mcp_env(&[
                ("MOBKIT_PHASE_C_SCHEDULING_MEMBER", "worker-1"),
                ("MOBKIT_PHASE_C_SCHEDULING_MESSAGE_PREFIX", "phase1-success"),
                ("MOBKIT_PHASE_C_SCHEDULING_DISABLE_INJECTION", "0"),
            ]),
        }],
    };

    let fixture = Box::pin(build_unified_runtime_fixture(config)).await;
    fixture
        .runtime
        .spawn(spawn_spec("worker", "worker-1"))
        .await
        .expect("spawn worker");

    let dispatch = fixture
        .runtime
        .dispatch_schedule_tick(
            &[ScheduleDefinition {
                schedule_id: "phase1-success".to_string(),
                interval: "*/1m".to_string(),
                timezone: "UTC".to_string(),
                enabled: true,
                jitter_ms: 0,
                catch_up: false,
            }],
            60_000,
        )
        .await
        .expect("dispatch should succeed");

    assert_eq!(dispatch.dispatched.len(), 1);
    assert!(dispatch.dispatched[0].runtime_injection.is_some());
    assert!(dispatch.dispatched[0].runtime_injection_error.is_none());

    let merged = fixture.runtime.module_events().await;
    let executed = merged
        .iter()
        .find(|event| {
            matches!(
                &event.event,
                UnifiedEvent::Module(module_event)
                    if module_event.module == "runtime"
                        && module_event.event_type == "runtime.injection.executed"
            )
        })
        .expect("expected runtime.injection.executed event");
    // Verify the executed event contains the expected payload fields
    match &executed.event {
        UnifiedEvent::Module(module_event) => {
            assert!(module_event.payload.get("member_id").is_some());
            assert!(module_event.payload.get("message").is_some());
        }
        _ => panic!("expected Module event"),
    };

    let shutdown = fixture.runtime.shutdown().await;
    assert!(
        shutdown.mob_stop.is_ok(),
        "mob stop failed at teardown: {:?}",
        shutdown.mob_stop
    );
}

#[tokio::test]
#[ignore]
async fn choke_003_unified_dispatch_surfaces_mob_runtime_injection_failure() {
    let fixture_binary = fixture_binary_path();
    let config = MobKitConfig {
        modules: vec![fixture_module("scheduling", &fixture_binary)],
        discovery: DiscoverySpec {
            namespace: "phase1-unified".to_string(),
            modules: vec!["scheduling".to_string()],
        },
        pre_spawn: vec![PreSpawnData {
            module_id: "scheduling".to_string(),
            env: mcp_env(&[
                ("MOBKIT_PHASE_C_SCHEDULING_MEMBER", "missing-member"),
                ("MOBKIT_PHASE_C_SCHEDULING_MESSAGE_PREFIX", "phase1-failure"),
                ("MOBKIT_PHASE_C_SCHEDULING_DISABLE_INJECTION", "0"),
            ]),
        }],
    };

    let fixture = Box::pin(build_unified_runtime_fixture(config)).await;
    let dispatch = fixture
        .runtime
        .dispatch_schedule_tick(
            &[ScheduleDefinition {
                schedule_id: "phase1-failure".to_string(),
                interval: "*/1m".to_string(),
                timezone: "UTC".to_string(),
                enabled: true,
                jitter_ms: 0,
                catch_up: false,
            }],
            60_000,
        )
        .await
        .expect("dispatch should produce report");

    assert_eq!(dispatch.dispatched.len(), 1);
    assert!(dispatch.dispatched[0].runtime_injection.is_some());
    assert!(dispatch.dispatched[0].runtime_injection_error.is_some());

    let merged = fixture.runtime.module_events().await;
    let failed = merged
        .iter()
        .find(|event| {
            matches!(
                &event.event,
                UnifiedEvent::Module(module_event)
                    if module_event.module == "runtime"
                        && module_event.event_type == "runtime.injection.failed"
            )
        })
        .expect("expected runtime.injection.failed event");
    let error_kind = match &failed.event {
        UnifiedEvent::Module(module_event) => module_event
            .payload
            .get("error_kind")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    };
    assert_eq!(error_kind, "mob_runtime");

    let shutdown = fixture.runtime.shutdown().await;
    assert!(
        shutdown.mob_stop.is_ok(),
        "mob stop failed at teardown: {:?}",
        shutdown.mob_stop
    );
}

#[tokio::test]
#[ignore]
async fn req_001_reference_entrypoint_real_listener_graceful_shutdown_stops_runtime_cleanly() {
    let config = MobKitConfig {
        modules: vec![shell_module(
            "listener-mod",
            r#"printf '%s\n' "{\"event_id\":\"listener-mod-ready\",\"source\":\"module\",\"timestamp_ms\":20,\"event\":{\"kind\":\"module\",\"module\":\"listener-mod\",\"event_type\":\"ready\",\"payload\":{\"ok\":true,\"pid\":$$}}}"; exec sleep 30"#,
            RestartPolicy::Never,
        )],
        discovery: DiscoverySpec {
            namespace: "phase1-reference-entrypoint".to_string(),
            modules: vec!["listener-mod".to_string()],
        },
        pre_spawn: vec![],
    };

    let fixture = Box::pin(build_unified_runtime_fixture(config)).await;
    let module_pid = fixture
        .runtime
        .module_events()
        .await
        .iter()
        .find_map(|event| match &event.event {
            UnifiedEvent::Module(module) if module.module == "listener-mod" => {
                module.payload.get("pid").and_then(|value| value.as_i64())
            }
            _ => None,
        })
        .expect("listener module pid should be present in payload");

    let app = fixture
        .runtime
        .build_reference_app_router(reference_runtime_decision_state());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind reference app listener");
    let address = listener
        .local_addr()
        .expect("resolve reference app listener address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let healthz_response = http_get_response(address, "/healthz").await;
    let mut response_parts = healthz_response.split("\r\n\r\n");
    let response_head = response_parts.next().unwrap_or_default();
    let response_body = response_parts.collect::<Vec<_>>().join("\r\n\r\n");
    assert!(
        response_head.starts_with("HTTP/1.1 200"),
        "expected HTTP 200 for /healthz, got: {response_head}"
    );
    assert!(
        response_body.contains("ok"),
        "expected /healthz response body to contain ok, got: {response_body}"
    );

    shutdown_tx
        .send(())
        .expect("signal graceful shutdown for reference listener");
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("reference listener should stop after graceful shutdown")
        .expect("reference listener task should join")
        .expect("reference listener should stop cleanly");

    let shutdown = fixture.runtime.shutdown().await;
    assert_eq!(shutdown.module_shutdown.orphan_processes, 0);
    assert_eq!(
        shutdown.module_shutdown.terminated_modules,
        vec!["listener-mod".to_string()]
    );
    assert!(
        shutdown.mob_stop.is_ok(),
        "mob stop failed at teardown: {:?}",
        shutdown.mob_stop
    );
    assert!(!fixture.runtime.module_is_running().await);
    assert_eq!(
        fixture.runtime.mob_handle().status().await.unwrap(),
        MobState::Stopped
    );

    let kill_status = Command::new("sh")
        .args(["-c", &format!("kill -0 {module_pid}")])
        .status()
        .expect("run kill -0");
    assert!(
        !kill_status.success(),
        "module process {module_pid} is still alive after shutdown"
    );
}
