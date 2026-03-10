use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use meerkat::{build_ephemeral_service, AgentFactory, Config};
use meerkat_client::{LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest, TestClient};
use meerkat_core::StopReason;
use meerkat_mob::{MobDefinition, MobState, MobStorage, SpawnMemberSpec};
use meerkat_mobkit_core::runtime::{DeliverySendRequest, RoutingResolveRequest};
use meerkat_mobkit_core::{
    build_runtime_decision_state, AuthPolicy, BigQueryNaming, ConsolePolicy, DiscoverySpec,
    LifecycleStage, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig, ModuleConfig,
    ModuleHealthState, PreSpawnData, RestartPolicy, RuntimeDecisionInputs, RuntimeOpsPolicy,
    RuntimeOptions, ScheduleDefinition, TrustedOidcRuntimeConfig, UnifiedEvent, UnifiedRuntime,
};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

const BOUNDARY_ENV_KEY: &str = "MOBKIT_MODULE_BOUNDARY";
const BOUNDARY_ENV_VALUE_MCP: &str = "mcp";

struct DelayedTestClient {
    delay: Duration,
}

impl DelayedTestClient {
    fn new(delay: Duration) -> Self {
        Self { delay }
    }
}

impl LlmClient for DelayedTestClient {
    fn stream<'a>(
        &'a self,
        _request: &'a LlmRequest,
    ) -> Pin<Box<dyn futures::Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>> {
        let delay = self.delay;
        Box::pin(async_stream::stream! {
            tokio::time::sleep(delay).await;
            yield Ok(LlmEvent::TextDelta {
                delta: "ok".to_string(),
                meta: None,
            });
            tokio::time::sleep(delay).await;
            yield Ok(LlmEvent::Done {
                outcome: LlmDoneOutcome::Success {
                    stop_reason: StopReason::EndTurn,
                },
            });
        })
    }

    fn provider(&self) -> &'static str {
        "phase5-delayed-test"
    }

    fn health_check<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn Future<Output = Result<(), LlmError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Ok(()) })
    }
}

fn spawn_spec(profile: &str, member_id: &str) -> SpawnMemberSpec {
    SpawnMemberSpec::from_wire(
        profile.to_string(),
        member_id.to_string(),
        Some(format!("You are {member_id}. Keep responses concise.")),
        None,
        None,
    )
}

fn shell_module(id: &str, script: &str, restart_policy: RestartPolicy) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        restart_policy,
    }
}

fn forced_crash_then_ready_script(
    module_id: &str,
    state_file: &Path,
    success_attempt: u32,
) -> String {
    let template = r#"attempt_file='__STATE_FILE__'; attempt=0; if [ -f "$attempt_file" ]; then attempt=$(cat "$attempt_file"); fi; attempt=$((attempt + 1)); echo "$attempt" > "$attempt_file"; if [ "$attempt" -lt __SUCCESS_ATTEMPT__ ]; then exit 1; fi; printf '%s\n' "{\"event_id\":\"evt-__MODULE_ID__-ready\",\"source\":\"module\",\"timestamp_ms\":42,\"event\":{\"kind\":\"module\",\"module\":\"__MODULE_ID__\",\"event_type\":\"ready\",\"payload\":{\"attempt\":$attempt,\"pid\":$$}}}"; exec sleep 30"#;
    template
        .replace("__STATE_FILE__", &state_file.display().to_string())
        .replace("__SUCCESS_ATTEMPT__", &success_attempt.to_string())
        .replace("__MODULE_ID__", module_id)
}

fn build_phase5_mob_spec(
    temp_dir: &tempfile::TempDir,
    default_llm_client: Arc<dyn LlmClient>,
) -> MobBootstrapSpec {
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));

    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "phase5-lifecycle-mob"

[profiles.lead]
model = "gpt-5.2"
external_addressable = true

[profiles.lead.tools]
comms = true

[profiles.worker]
model = "gpt-5.2"
external_addressable = true

[profiles.worker.tools]
comms = true
"#,
    )
    .expect("parse phase5 mob definition");

    MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service).with_options(
        MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(default_llm_client),
        },
    )
}

fn decision_state() -> meerkat_mobkit_core::RuntimeDecisionState {
    build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "phase5_reference_dataset".to_string(),
            table: "phase5_reference_table".to_string(),
        },
        trusted_mobkit_toml: trusted_modules_toml(),
        auth: AuthPolicy::default(),
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy {
            require_app_auth: false,
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: include_str!("../../../docs/rct/release-targets.json").to_string(),
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

[[modules]]
id = "delivery"
command = "delivery-bin"
args = ["--sink", "memory"]
restart_policy = "on_failure"

[[modules]]
id = "scheduling"
command = "scheduling-bin"
args = ["--tick", "60"]
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

fn fixture_binary_path() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_phase_c_mcp_fixture") {
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
        .join("phase_c_mcp_fixture");
    if binary_path.exists() {
        return binary_path;
    }

    let status = Command::new("cargo")
        .args([
            "build",
            "-p",
            "meerkat-mobkit-core",
            "--bin",
            "phase_c_mcp_fixture",
        ])
        .current_dir(workspace_root)
        .status()
        .expect("build phase_c_mcp_fixture");
    assert!(
        status.success(),
        "building phase_c_mcp_fixture must succeed"
    );
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


#[tokio::test]
async fn e2e_003_failure_path_module_crash_during_active_sse_stream_recovers_and_shuts_down_ordered(
) {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let state_file = temp_dir.path().join("forced-crash-attempts.txt");
    let crash_script = forced_crash_then_ready_script("forced-crash", &state_file, 2);

    let runtime = UnifiedRuntime::builder()
        .mob_spec(build_phase5_mob_spec(
            &temp_dir,
            Arc::new(DelayedTestClient::new(Duration::from_millis(350))),
        ))
        .module_config(MobKitConfig {
            modules: vec![shell_module(
                "forced-crash",
                &crash_script,
                RestartPolicy::OnFailure,
            )],
            discovery: DiscoverySpec {
                namespace: "phase5-e2e-003".to_string(),
                modules: vec![],
            },
            pre_spawn: vec![],
        })
        .timeout(Duration::from_secs(1))
        .runtime_options(RuntimeOptions {
            on_failure_retry_budget: 1,
            supervisor_restart_backoff_ms: 80,
            ..RuntimeOptions::default()
        })
        .build()
        .await
        .expect("build unified runtime");

    runtime
        .reconcile(vec![spawn_spec("worker", "worker-1")])
        .await
        .expect("reconcile worker");

    // Send a message to create activity while module crash/recovery proceeds.
    runtime
        .send_message("worker-1", "Keep this interaction open briefly while runtime work proceeds.".to_string())
        .await
        .expect("send_message should succeed");

    let added = runtime
        .reconcile_modules(vec!["forced-crash".to_string()], Duration::from_secs(1))
        .await
        .expect("reconcile_modules should recover forced-crash module");
    assert_eq!(added, 1);
    assert_eq!(
        std::fs::read_to_string(&state_file)
            .expect("state file should be written")
            .trim(),
        "2"
    );

    let forced_crash_transitions = runtime
        .module_health_transitions()
        .await
        .into_iter()
        .filter(|transition| transition.module_id == "forced-crash")
        .map(|transition| transition.to)
        .collect::<Vec<_>>();
    assert_eq!(
        forced_crash_transitions,
        vec![
            ModuleHealthState::Starting,
            ModuleHealthState::Failed,
            ModuleHealthState::Restarting,
            ModuleHealthState::Healthy,
        ]
    );

    let shutdown = runtime.shutdown().await;
    assert_eq!(shutdown.module_shutdown.orphan_processes, 0);
    assert_eq!(
        shutdown.module_shutdown.terminated_modules,
        vec!["forced-crash".to_string()]
    );
    shutdown
        .mob_stop
        .expect("mob runtime should stop cleanly after module recovery");

    assert_eq!(
        runtime
            .module_lifecycle_events()
            .await
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
fn e2e_004_happy_path_full_lifecycle_startup_reconcile_dispatch_route_delivery_shutdown() {
    let fixture_binary = fixture_binary_path();
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let module_config = MobKitConfig {
        modules: vec![
            fixture_module("router", &fixture_binary),
            fixture_module("delivery", &fixture_binary),
            fixture_module("scheduling", &fixture_binary),
        ],
        discovery: DiscoverySpec {
            namespace: "phase5-e2e-004".to_string(),
            modules: vec![
                "router".to_string(),
                "delivery".to_string(),
                "scheduling".to_string(),
            ],
        },
        pre_spawn: vec![
            PreSpawnData {
                module_id: "router".to_string(),
                env: mcp_env(&[]),
            },
            PreSpawnData {
                module_id: "delivery".to_string(),
                env: mcp_env(&[]),
            },
            PreSpawnData {
                module_id: "scheduling".to_string(),
                env: mcp_env(&[
                    ("MOBKIT_PHASE_C_SCHEDULING_MEMBER", "worker-1"),
                    ("MOBKIT_PHASE_C_SCHEDULING_MESSAGE_PREFIX", "phase5-happy"),
                    ("MOBKIT_PHASE_C_SCHEDULING_DISABLE_INJECTION", "0"),
                ]),
            },
        ],
    };

    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let runtime = tokio_runtime.block_on(async {
        UnifiedRuntime::bootstrap(
            build_phase5_mob_spec(&temp_dir, Arc::new(TestClient::default())),
            module_config,
            Duration::from_secs(2),
        )
        .await
        .expect("bootstrap unified runtime")
    });

    assert_eq!(runtime.status(), MobState::Running);
    assert!(tokio_runtime.block_on(runtime.module_is_running()));
    assert_eq!(
        tokio_runtime.block_on(runtime.loaded_modules()),
        vec![
            "delivery".to_string(),
            "router".to_string(),
            "scheduling".to_string(),
        ]
    );

    tokio_runtime.block_on(async {
        let reconcile = runtime
            .reconcile(vec![
                spawn_spec("lead", "lead-1"),
                spawn_spec("worker", "worker-1"),
            ])
            .await
            .expect("reconcile lead + worker");
        assert!(reconcile.routing.router_module_loaded);
        assert_eq!(
            reconcile.routing.added_route_keys,
            vec![
                "mob.member.lead-1".to_string(),
                "mob.member.worker-1".to_string(),
            ]
        );
        let mut spawned = reconcile.mob.spawned.clone();
        spawned.sort();
        assert_eq!(spawned, vec!["lead-1".to_string(), "worker-1".to_string()]);

        let app = runtime.build_reference_app_router(decision_state());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let _address = listener.local_addr().expect("listener address");
        let (server_shutdown_tx, server_shutdown_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = server_shutdown_rx.await;
                })
                .await
        });

        // Send a message (fire-and-forget) to drive agent activity.
        runtime
            .send_message("worker-1", "phase5 happy path interaction".to_string())
            .await
            .expect("send_message should succeed");

        let dispatch = runtime
            .dispatch_schedule_tick(
                &[ScheduleDefinition {
                    schedule_id: "phase5-happy".to_string(),
                    interval: "*/1m".to_string(),
                    timezone: "UTC".to_string(),
                    enabled: true,
                    jitter_ms: 0,
                    catch_up: false,
                }],
                60_000,
            )
            .await
            .expect("dispatch schedule tick");
        assert_eq!(dispatch.dispatched.len(), 1);
        assert!(dispatch.dispatched[0].runtime_injection.is_some());
        assert!(dispatch.dispatched[0].runtime_injection_error.is_none());
        assert!(runtime.module_events().await.iter().any(|event| {
            matches!(
                &event.event,
                UnifiedEvent::Module(module_event)
                    if module_event.module == "runtime"
                        && module_event.event_type == "runtime.injection.executed"
            )
        }));

        server_shutdown_tx
            .send(())
            .expect("signal reference app shutdown");
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("reference app should shut down")
            .expect("reference app task should join")
            .expect("reference app should shut down cleanly");
    });

    let resolution = tokio_runtime.block_on(runtime
        .resolve_routing(RoutingResolveRequest {
            recipient: "user@example.com".to_string(),
            channel: Some("transactional".to_string()),
            retry_max: Some(1),
            backoff_ms: Some(125),
            rate_limit_per_minute: Some(10),
        }))
        .expect("routing resolve");
    assert_eq!(resolution.target_module, "delivery");
    assert_eq!(resolution.sink, "email");

    let delivery = tokio_runtime.block_on(runtime
        .send_delivery(DeliverySendRequest {
            resolution: resolution.clone(),
            payload: json!({"message":"phase5 lifecycle happy path"}),
            idempotency_key: Some("phase5-e2e-004".to_string()),
        }))
        .expect("delivery send");
    assert_eq!(delivery.route_id, resolution.route_id);
    assert_eq!(delivery.status, "sent");
    assert_eq!(delivery.target_module, "delivery");
    assert_eq!(
        delivery
            .attempts
            .last()
            .map(|attempt| attempt.status.as_str()),
        Some("sent")
    );

    let shutdown = tokio_runtime.block_on(runtime.shutdown());
    assert_eq!(shutdown.module_shutdown.orphan_processes, 0);
    assert_eq!(
        shutdown.module_shutdown.terminated_modules,
        vec![
            "delivery".to_string(),
            "router".to_string(),
            "scheduling".to_string(),
        ]
    );
    shutdown
        .mob_stop
        .expect("mob runtime should stop cleanly at lifecycle end");

    assert_eq!(
        tokio_runtime.block_on(runtime
            .module_lifecycle_events())
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
