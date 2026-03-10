use std::sync::Arc;

use meerkat::{build_ephemeral_service, AgentFactory, Config};
use meerkat_client::TestClient;
use meerkat_mob::{MobStorage, Prefab, SpawnMemberSpec};
use meerkat_mobkit_core::{
    build_runtime_decision_state, handle_console_ingress_json, AuthPolicy, BigQueryNaming,
    ConsolePolicy, DiscoverySpec, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig,
    PreSpawnData, RuntimeDecisionInputs, RuntimeOpsPolicy, ScheduleDefinition, SubscribeRequest,
    TrustedOidcRuntimeConfig, UnifiedRuntime,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path)?;

    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));

    let mut definition = Prefab::CodingSwarm.definition();
    for profile in definition.profiles.values_mut() {
        profile.model = "gpt-5.2".to_string();
    }

    let runtime = UnifiedRuntime::builder()
        .mob_spec(
            MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
                .with_options(MobBootstrapOptions {
                    allow_ephemeral_sessions: true,
                    notify_orchestrator_on_resume: true,
                    default_llm_client: Some(Arc::new(TestClient::default())),
                }),
        )
        .module_config(MobKitConfig {
            modules: vec![],
            discovery: DiscoverySpec {
                namespace: "reference-app".to_string(),
                modules: vec![],
            },
            pre_spawn: Vec::<PreSpawnData>::new(),
        })
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .await?;

    runtime.reconcile(reference_member_specs()).await?;
    runtime.subscribe_events(SubscribeRequest::default()).await?;
    let empty_schedules = Vec::<ScheduleDefinition>::new();
    runtime
        .dispatch_schedule_tick(&empty_schedules, 60_000)
        .await?;
    let _ = runtime.reconcile_modules(Vec::new(), std::time::Duration::from_secs(1)).await;
    let loaded_modules = runtime.loaded_modules().await;
    if loaded_modules.iter().any(|module| module == "router")
        && loaded_modules.iter().any(|module| module == "delivery")
    {
        if let Ok(resolution) =
            runtime.resolve_routing(meerkat_mobkit_core::runtime::RoutingResolveRequest {
                recipient: "sample@example.com".to_string(),
                channel: Some("transactional".to_string()),
                retry_max: Some(1),
                backoff_ms: Some(250),
                rate_limit_per_minute: Some(2),
            }).await
        {
            let _ = runtime.send_delivery(meerkat_mobkit_core::runtime::DeliverySendRequest {
                resolution,
                payload: serde_json::json!({"message":"reference-app smoke"}),
                idempotency_key: Some("reference-app-smoke".to_string()),
            }).await;
        }
    }

    let decisions = build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "reference_app_dataset".to_string(),
            table: "reference_app_table".to_string(),
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
    .map_err(|err| std::io::Error::other(format!("failed to build console decisions: {err:?}")))?;
    let _console_ingress_preview = handle_console_ingress_json(
        &decisions,
        r#"{"method":"GET","path":"/console/modules","auth":null}"#,
    );
    let _console_router = runtime.build_console_json_router(decisions.clone());

    let listen_addr =
        std::env::var("MOBKIT_REF_ADDR").unwrap_or_else(|_| "127.0.0.1:3210".to_string());
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    println!("reference app listening on http://{listen_addr}");
    println!("GET  /console");
    println!("GET  /console/experience");
    println!("GET  /console/modules");
    if std::env::var("MOBKIT_REF_HTTP_MODE").ok().as_deref() == Some("serve") {
        runtime.serve(listener, decisions).await?;
        return Ok(());
    }

    let run_report = runtime
        .run(listener, decisions, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    let shutdown = run_report.shutdown;
    if shutdown.module_shutdown.orphan_processes != 0 {
        return Err(std::io::Error::other(format!(
            "runtime shutdown left {} orphan process(es)",
            shutdown.module_shutdown.orphan_processes
        ))
        .into());
    }
    shutdown
        .mob_stop
        .map_err(|err| std::io::Error::other(format!("failed to stop mob runtime: {err}")))?;
    run_report.serve_result?;
    Ok(())
}

fn reference_member_specs() -> Vec<SpawnMemberSpec> {
    ["router", "delivery"]
        .into_iter()
        .map(|member_id| {
            SpawnMemberSpec::from_wire(
                "lead".to_string(),
                member_id.to_string(),
                Some(format!("You are {member_id}. Keep responses concise.")),
                None,
                None,
            )
        })
        .collect()
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
