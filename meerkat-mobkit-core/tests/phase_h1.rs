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
    clippy::useless_vec
)]
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use meerkat::{AgentFactory, Config, build_ephemeral_service};
use meerkat_client::TestClient;
use meerkat_core::SessionId;
use meerkat_mob::{MeerkatId, MobStorage, Prefab, SpawnMemberSpec};
use meerkat_mobkit_core::{
    AuthPolicy, BigQueryNaming, ConsolePolicy, ConsoleRestJsonRequest, DiscoverySpec,
    MobBootstrapOptions, MobBootstrapSpec, MobKitConfig, RuntimeDecisionInputs, RuntimeOpsPolicy,
    TrustedOidcRuntimeConfig, UnifiedRuntime, build_runtime_decision_state, console_json_router,
    handle_console_rest_json_route,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

struct RuntimeFixture {
    _temp_dir: TempDir,
    runtime: UnifiedRuntime,
}

fn trusted_toml() -> String {
    r#"
[[modules]]
id = "router"
command = "router-bin"
args = ["--mode", "fast"]
restart_policy = "always"

[[modules]]
id = "delivery"
command = "delivery-bin"
args = ["--sink", "test"]
restart_policy = "on_failure"
"#
    .to_string()
}

fn release_json() -> String {
    include_str!("../../docs/rct/release-targets.json").to_string()
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

fn decision_state(require_app_auth: bool) -> meerkat_mobkit_core::RuntimeDecisionState {
    build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "phase_h1_dataset".to_string(),
            table: "phase_h1_table".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy {
            default_provider: meerkat_mobkit_core::AuthProvider::GoogleOAuth,
            email_allowlist: vec![
                "alice@example.com".to_string(),
                "svc:deploy-bot".to_string(),
            ],
        },
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy { require_app_auth },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: release_json(),
    })
    .expect("decision state builds")
}

async fn build_runtime_fixture() -> RuntimeFixture {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));

    let mut definition = Prefab::CodingSwarm.definition();
    for profile in definition.profiles.values_mut() {
        profile.model = "gpt-5.2".to_string();
    }

    let mob_spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(TestClient::default())),
        });
    let module_config = MobKitConfig {
        modules: vec![],
        discovery: DiscoverySpec {
            namespace: "phase-h1".to_string(),
            modules: vec![],
        },
        pre_spawn: vec![],
    };
    let runtime = UnifiedRuntime::bootstrap(mob_spec, module_config, Duration::from_secs(2))
        .await
        .expect("bootstrap unified runtime");

    RuntimeFixture {
        _temp_dir: temp_dir,
        runtime,
    }
}

fn console_member_spec(member_id: &str) -> SpawnMemberSpec {
    SpawnMemberSpec::from_wire(
        "lead".to_string(),
        MeerkatId::from(member_id).to_string(),
        Some(format!("You are {member_id}. Keep responses concise.")),
        None,
        None,
    )
}

async fn spawn_console_members(runtime: &UnifiedRuntime) {
    for member_id in ["router", "delivery"] {
        runtime
            .spawn(console_member_spec(member_id))
            .await
            .expect("spawn console member");
    }
}

async fn get_console_experience(app: &Router) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/console/experience")
                .body(Body::empty())
                .expect("console request"),
        )
        .await
        .expect("console response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("console body");
    serde_json::from_slice(&body).expect("console json")
}

#[tokio::test]
async fn phase_h1_req_001_reference_style_router_mounts_console_and_sse() {
    let fixture = build_runtime_fixture().await;
    spawn_console_members(&fixture.runtime).await;

    let app = fixture
        .runtime
        .build_reference_app_router(decision_state(false));
    let health_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/healthz")
                .body(Body::empty())
                .expect("health request"),
        )
        .await
        .expect("health response");
    assert_eq!(health_response.status(), StatusCode::OK);

    let console_entry_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/console")
                .body(Body::empty())
                .expect("console entry request"),
        )
        .await
        .expect("console entry response");
    let console_entry_status = console_entry_response.status();
    let console_entry_content_type = console_entry_response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let console_entry_body = to_bytes(console_entry_response.into_body(), 1024 * 1024)
        .await
        .expect("console entry body");
    let console_entry_text = String::from_utf8(console_entry_body.to_vec()).expect("console html");
    assert_eq!(console_entry_status, StatusCode::OK);
    assert!(
        console_entry_content_type.starts_with("text/html"),
        "expected text/html content-type, got: {console_entry_content_type}"
    );
    assert!(console_entry_text.contains("<div id=\"root\"></div>"));
    assert!(console_entry_text.contains("/console/assets/console-app.js"));

    let console_asset_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/console/assets/console-app.js")
                .body(Body::empty())
                .expect("console asset request"),
        )
        .await
        .expect("console asset response");
    let console_asset_status = console_asset_response.status();
    let console_asset_content_type = console_asset_response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let console_asset_body = to_bytes(console_asset_response.into_body(), 1024 * 1024)
        .await
        .expect("console asset body");
    let console_asset_text = String::from_utf8(console_asset_body.to_vec()).expect("console js");
    assert_eq!(console_asset_status, StatusCode::OK);
    assert!(
        console_asset_content_type.starts_with("application/javascript"),
        "expected application/javascript content-type, got: {console_asset_content_type}"
    );
    assert!(console_asset_text.contains("createConsoleApp"));

    let console_json = get_console_experience(&app).await;
    assert_eq!(
        console_json["agent_sidebar"]["panel_id"],
        json!("console.agent_sidebar")
    );
    assert_eq!(
        console_json["chat_inspector"]["panel_id"],
        json!("console.chat_inspector")
    );
    assert_eq!(
        console_json["topology"]["panel_id"],
        json!("console.topology")
    );
    assert_eq!(
        console_json["health_overview"]["panel_id"],
        json!("console.health_overview")
    );
    assert_eq!(
        console_json["agent_sidebar"]["live_snapshot"]["agents"],
        json!([
            {
                "agent_id": "delivery",
                "member_id": "delivery",
                "label": "delivery",
                "kind": "module_agent"
            },
            {
                "agent_id": "router",
                "member_id": "router",
                "label": "router",
                "kind": "module_agent"
            }
        ])
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert!(shutdown.mob_stop.is_ok());
}

#[tokio::test]
async fn phase_h1_live_snapshot_tracks_runtime_drift() {
    let fixture = build_runtime_fixture().await;
    spawn_console_members(&fixture.runtime).await;

    let app = fixture
        .runtime
        .build_reference_app_router(decision_state(false));
    let initial = get_console_experience(&app).await;

    assert_eq!(
        initial["health_overview"]["live_snapshot"]["running"],
        json!(true)
    );
    assert_eq!(
        initial["health_overview"]["live_snapshot"]["loaded_modules"],
        json!(["delivery", "router"])
    );

    let reconcile = fixture
        .runtime
        .reconcile(vec![console_member_spec("router")])
        .await
        .expect("reconcile delivery retirement");
    assert_eq!(reconcile.mob.retired, vec!["delivery".to_string()]);

    let after_retire = get_console_experience(&app).await;
    assert_eq!(
        after_retire["topology"]["live_snapshot"]["nodes"],
        json!(["router"])
    );
    assert_eq!(
        after_retire["topology"]["live_snapshot"]["node_count"],
        json!(1)
    );
    assert_eq!(
        after_retire["health_overview"]["live_snapshot"]["loaded_modules"],
        json!(["router"])
    );
    assert_eq!(
        after_retire["health_overview"]["live_snapshot"]["loaded_module_count"],
        json!(1)
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert!(shutdown.mob_stop.is_ok());
    let after_stop = get_console_experience(&app).await;
    assert_eq!(
        after_stop["health_overview"]["live_snapshot"]["running"],
        json!(false)
    );
}

#[tokio::test]
async fn phase_h1_console_modules_route_honors_auth_mode() {
    let open_state = decision_state(false);
    let direct_open = handle_console_rest_json_route(
        &open_state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/modules".to_string(),
            auth: None,
        },
    );
    let open_app = console_json_router(open_state);
    let open_response = open_app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/console/modules")
                .body(Body::empty())
                .expect("open request"),
        )
        .await
        .expect("open response");
    let open_status = open_response.status();
    let open_body = to_bytes(open_response.into_body(), 1024 * 1024)
        .await
        .expect("open body");
    let open_json: Value = serde_json::from_slice(&open_body).expect("open json");

    assert_eq!(open_status, StatusCode::OK);
    assert_eq!(direct_open.status, 200);
    assert_eq!(open_json, direct_open.body);
    assert_eq!(open_json["modules"], json!(["router", "delivery"]));

    let state = decision_state(true);
    let direct = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/modules".to_string(),
            auth: None,
        },
    );
    let app = console_json_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/console/modules")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let response_status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let json_body: Value = serde_json::from_slice(&body).expect("json body");

    assert_eq!(response_status, StatusCode::UNAUTHORIZED);
    assert_eq!(direct.status, 401);
    assert_eq!(
        StatusCode::from_u16(direct.status).expect("status code"),
        response_status
    );
    assert_eq!(
        direct.body,
        json!({"error":"unauthorized","reason":"missing_credentials"})
    );
    assert_eq!(json_body, direct.body);
}

#[tokio::test]
async fn phase_h1_cross_panel_sidebar_agent_streams_and_unknown_member_rejected() {
    let fixture = build_runtime_fixture().await;
    spawn_console_members(&fixture.runtime).await;

    let app = fixture
        .runtime
        .build_reference_app_router(decision_state(false));
    let console_json = get_console_experience(&app).await;

    let selected_agent_id = console_json["agent_sidebar"]["live_snapshot"]["agents"]
        .as_array()
        .expect("agents array")
        .first()
        .and_then(|agent| agent.get("agent_id"))
        .and_then(Value::as_str)
        .expect("selected agent_id");
    assert_eq!(
        console_json["agent_sidebar"]["live_snapshot"]["agents"][0]["member_id"],
        json!(selected_agent_id)
    );

    // Sending to a known agent should succeed via send_message.
    let session_id = fixture
        .runtime
        .send_message(selected_agent_id, "cross-panel hello".to_string())
        .await
        .expect("send_message to known agent should succeed");
    SessionId::parse(&session_id).expect("send_message should return a valid session_id");

    // Sending to an unknown agent should fail.
    let unknown_result = fixture
        .runtime
        .send_message("unknown-member-id", "should fail".to_string())
        .await;
    assert!(
        unknown_result.is_err(),
        "send_message to unknown agent should fail"
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert!(shutdown.mob_stop.is_ok());
}
