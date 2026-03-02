use std::sync::Arc;

use meerkat::{build_ephemeral_service, AgentFactory, Config};
use meerkat_client::TestClient;
use meerkat_mob::{MeerkatId, MobStorage, Prefab, SpawnMemberSpec};
use meerkat_mobkit_core::{
    build_reference_app_router, build_runtime_decision_state, AuthPolicy, BigQueryNaming,
    ConsolePolicy, MobBootstrapOptions, MobBootstrapSpec, RealMobRuntime, RuntimeDecisionInputs,
    RuntimeOpsPolicy, TrustedOidcRuntimeConfig,
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

    let runtime = RealMobRuntime::bootstrap(
        MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service).with_options(
            MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: Some(Arc::new(TestClient::default())),
            },
        ),
    )
    .await?;

    spawn_reference_members(&runtime).await?;

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

    let app = build_reference_app_router(decisions, runtime);

    let listen_addr =
        std::env::var("MOBKIT_REF_ADDR").unwrap_or_else(|_| "127.0.0.1:3210".to_string());
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    println!("reference app listening on http://{listen_addr}");
    println!("GET  /console/experience");
    println!("GET  /console/modules");
    println!(
        "POST /interactions/stream with JSON: {{\"member_id\":\"router\",\"message\":\"hello\"}}"
    );
    axum::serve(listener, app).await?;
    Ok(())
}

async fn spawn_reference_members(
    runtime: &RealMobRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    for member_id in ["router", "delivery"] {
        runtime
            .spawn(SpawnMemberSpec::from_wire(
                "lead".to_string(),
                MeerkatId::from(member_id).to_string(),
                Some(format!("You are {member_id}. Keep responses concise.")),
                None,
                None,
            ))
            .await?;
    }

    Ok(())
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
