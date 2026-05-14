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
use axum::http::{Request, StatusCode, header};
use meerkat::{AgentFactory, Config, build_ephemeral_service};
use meerkat_client::TestClient;
use meerkat_core::SessionId;
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::{MobDefinition, MobStorage, SpawnMemberSpec};
use meerkat_mobkit::{
    AuthPolicy, BigQueryNaming, ConsolePolicy, ConsoleRestJsonRequest, DiscoverySpec,
    MobBootstrapOptions, MobBootstrapSpec, MobKitConfig, MobRuntimeError, RuntimeDecisionInputs,
    RuntimeOpsPolicy, TrustedOidcRuntimeConfig, UnifiedRuntime, build_runtime_decision_state,
    console_json_router, handle_console_rest_json_route,
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
    include_str!("../assets/release-targets.json").to_string()
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

fn decision_state(require_app_auth: bool) -> meerkat_mobkit::RuntimeDecisionState {
    build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "phase_h1_dataset".to_string(),
            table: "phase_h1_table".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy {
            default_provider: meerkat_mobkit::AuthProvider::GoogleOAuth,
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

fn assert_mob_stop_allows_boundary_cancel(stop: Result<(), MobRuntimeError>) {
    if let Err(err) = stop {
        assert!(
            err.to_string().contains("cancel_after_boundary"),
            "shutdown failed: {err:?}"
        );
    }
}

async fn build_runtime_fixture() -> RuntimeFixture {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));

    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "phase-h1-console-mob"

[profiles.lead]
model = "gpt-5.5"
external_addressable = true

[profiles.lead.tools]
comms = true
"#,
    )
    .expect("parse console mob definition");

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
        Some(format!("You are {member_id}. Keep responses concise.").into()),
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

async fn spawn_named_members(runtime: &UnifiedRuntime, member_ids: &[&str]) {
    for member_id in member_ids {
        runtime
            .spawn(console_member_spec(member_id))
            .await
            .expect("spawn named console member");
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

async fn query_console_timeline_frames(app: &Router, identity: &str) -> Vec<Value> {
    let query_payload = json!({
        "jsonrpc": "2.0",
        "id": "query-timeline",
        "method": "mobkit/console/query_timeline",
        "params": { "identity": identity }
    });
    let query_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/console/rpc")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(query_payload.to_string()))
                .expect("query request"),
        )
        .await
        .expect("query response");
    assert_eq!(query_response.status(), StatusCode::OK);
    let query_body = to_bytes(query_response.into_body(), 1024 * 1024)
        .await
        .expect("query body");
    let query_json: Value = serde_json::from_slice(&query_body).expect("query json");
    query_json["result"]["frames"]
        .as_array()
        .expect("console timeline frames")
        .clone()
}

#[tokio::test]
#[ignore]
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
                "kind": "mob_agent",
                "profile": "lead",
                "state": "active",
                "wired_to": [],
                "labels": {},
                "group": "lead",
                "addressable": true,
                "affordances": {
                    "addressable": true,
                    "can_send_message": true,
                    "can_retire": true,
                    "can_respawn": true,
                    "runtime_mode": "mob_agent"
                }
            },
            {
                "agent_id": "router",
                "member_id": "router",
                "label": "router",
                "kind": "mob_agent",
                "profile": "lead",
                "state": "active",
                "wired_to": [],
                "labels": {},
                "group": "lead",
                "addressable": true,
                "affordances": {
                    "addressable": true,
                    "can_send_message": true,
                    "can_retire": true,
                    "can_respawn": true,
                    "runtime_mode": "mob_agent"
                }
            }
        ])
    );

    assert_eq!(console_json["contract_version"], json!("0.3.0"));
    assert_eq!(
        console_json["runtime_capabilities"],
        json!({
            "can_spawn_members": true,
            "can_send_messages": true,
            "can_wire_members": true,
            "can_retire_members": true,
            "available_spawn_modes": ["module", "profile"],
            "profile_capabilities": {
                "lead": {
                    "instance_count": 2,
                    "addressable": true,
                    "has_wiring": false,
                }
            }
        })
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert_mob_stop_allows_boundary_cancel(shutdown.mob_stop);
}

#[tokio::test]
async fn live_snapshot_keeps_configured_modules_even_when_runtime_members_differ() {
    let fixture = build_runtime_fixture().await;
    spawn_named_members(&fixture.runtime, &["triage", "billing"]).await;

    let app = fixture
        .runtime
        .build_reference_app_router(decision_state(false));
    let console_json = get_console_experience(&app).await;

    // Topology now shows identity-native nodes from mob members, not config modules.
    let topology_nodes = console_json["topology"]["live_snapshot"]["nodes"]
        .as_array()
        .expect("topology nodes array");
    assert_eq!(topology_nodes.len(), 2);
    assert!(
        topology_nodes.iter().any(|n| n["identity"] == "billing"),
        "expected billing in topology nodes"
    );
    assert!(
        topology_nodes.iter().any(|n| n["identity"] == "triage"),
        "expected triage in topology nodes"
    );
    assert_eq!(
        console_json["topology"]["live_snapshot"]["node_count"],
        json!(2)
    );
    // Health overview loaded_modules still comes from config.
    assert_eq!(
        console_json["health_overview"]["live_snapshot"]["loaded_modules"],
        json!(["delivery", "router"])
    );
    assert_eq!(
        console_json["health_overview"]["live_snapshot"]["loaded_module_count"],
        json!(2)
    );
    assert_eq!(
        console_json["agent_sidebar"]["live_snapshot"]["agents"]
            .as_array()
            .expect("agents array")
            .len(),
        2
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert_mob_stop_allows_boundary_cancel(shutdown.mob_stop);
}

#[tokio::test]
async fn multipart_blob_upload_round_trips_through_reference_router() {
    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "multipart-console-mob"

[profiles.lead]
model = "gpt-5.2"
external_addressable = true

[profiles.lead.tools]
comms = true
"#,
    )
    .expect("parse multipart mob definition");
    let runtime = UnifiedRuntime::builder()
        .definition(definition)
        .module_config(MobKitConfig {
            modules: vec![],
            discovery: DiscoverySpec {
                namespace: "multipart-console".to_string(),
                modules: vec![],
            },
            pre_spawn: vec![],
        })
        .default_llm_client(Arc::new(TestClient::default()))
        .timeout(Duration::from_secs(2))
        .build()
        .await
        .expect("build multipart runtime");

    let app = runtime.build_reference_app_router(decision_state(false));
    let boundary = "mobkit-test-boundary";
    let payload = json!({
        "jsonrpc": "2.0",
        "id": "upload-1",
        "method": "mobkit/blob/upload",
        "params": {
            "upload": {
                "type": "image_upload",
                "upload_id": "upload-1",
                "media_type": "image/png"
            }
        }
    });
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"payload\"\r\n");
    body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    body.extend_from_slice(payload.to_string().as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file:upload-1\"; filename=\"tiny.png\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: image/png\r\n\r\n");
    body.extend_from_slice(b"tiny-png");
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let upload_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/console/rpc/multipart")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("multipart request"),
        )
        .await
        .expect("multipart response");
    assert_eq!(upload_response.status(), StatusCode::OK);
    let upload_body = to_bytes(upload_response.into_body(), 1024 * 1024)
        .await
        .expect("multipart body");
    let upload_json: Value = serde_json::from_slice(&upload_body).expect("upload json");
    let blob_id = upload_json["result"]["blob_id"]
        .as_str()
        .expect("blob id")
        .to_string();
    assert_eq!(upload_json["result"]["media_type"], json!("image/png"));
    assert_eq!(upload_json["result"]["size"], json!(8));

    let blob_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/blobs/{blob_id}"))
                .body(Body::empty())
                .expect("blob request"),
        )
        .await
        .expect("blob response");
    assert_eq!(blob_response.status(), StatusCode::OK);
    assert_eq!(
        blob_response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let blob_body = to_bytes(blob_response.into_body(), 1024 * 1024)
        .await
        .expect("blob body");
    assert_eq!(blob_body.as_ref(), b"tiny-png");

    let shutdown = runtime.shutdown().await;
    assert_mob_stop_allows_boundary_cancel(shutdown.mob_stop);
}

#[tokio::test]
async fn multipart_send_message_projects_text_and_image_into_console_events() {
    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "multipart-send-console-mob"

[profiles.lead]
model = "gpt-5.5"
external_addressable = true

[profiles.lead.tools]
comms = true
"#,
    )
    .expect("parse multipart send mob definition");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");
    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));
    let binary_blob_store: Arc<dyn meerkat_mobkit::BinaryBlobStore> =
        Arc::new(meerkat_mobkit::ObjectStoreBlobStore::memory());
    let mut mob_spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(TestClient::default())),
        });
    mob_spec.binary_blob_store = Some(binary_blob_store);
    let runtime = UnifiedRuntime::bootstrap(
        mob_spec,
        MobKitConfig {
            modules: vec![],
            discovery: DiscoverySpec {
                namespace: "multipart-send-console".to_string(),
                modules: vec![],
            },
            pre_spawn: vec![],
        },
        Duration::from_secs(2),
    )
    .await
    .expect("build multipart send runtime");
    runtime
        .spawn(console_member_spec("analyst"))
        .await
        .expect("spawn analyst member");

    let app = runtime.build_reference_app_router(decision_state(false));
    let boundary = "mobkit-send-boundary";
    let payload = json!({
        "jsonrpc": "2.0",
        "id": "send-1",
        "method": "mobkit/console/send",
        "params": {
            "identity": "analyst",
            "origin": "test",
            "idempotency_key": "multipart-send-image-1",
            "content": [
                { "type": "text", "text": "Describe this inline image." },
                {
                    "type": "image_upload",
                    "part_name": "image-field",
                    "media_type": "image/png"
                }
            ]
        }
    });
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"payload\"\r\n");
    body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    body.extend_from_slice(payload.to_string().as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file:image-field\"; filename=\"tiny.png\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: image/png\r\n\r\n");
    body.extend_from_slice(b"tiny-png");
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let send_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/console/rpc/multipart")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("multipart send request"),
        )
        .await
        .expect("multipart send response");
    assert_eq!(send_response.status(), StatusCode::OK);
    let send_body = to_bytes(send_response.into_body(), 1024 * 1024)
        .await
        .expect("multipart send body");
    let send_json: Value = serde_json::from_slice(&send_body).expect("send json");
    assert!(
        send_json.get("error").is_none() || send_json["error"].is_null(),
        "send failed: {send_json}"
    );
    assert_eq!(send_json["result"]["status"], json!("accepted"));
    assert_eq!(send_json["result"]["identity"], json!("analyst"));
    assert!(send_json["result"]["interaction_id"].as_str().is_some());

    let frames = query_console_timeline_frames(&app, "analyst").await;
    let started = frames
        .iter()
        .find(|frame| frame["kind"] == "user_input" || frame["event"] == "interaction_started")
        .expect("user input timeline frame");
    assert_eq!(
        started["payload"]["content"][0]["text"],
        json!("Describe this inline image.")
    );
    assert_eq!(started["payload"]["content"][1]["type"], json!("image"));
    assert_eq!(started["payload"]["content"][1]["source"], json!("blob"));
    assert_eq!(
        started["payload"]["content"][1]["media_type"],
        json!("image/png")
    );
    assert!(
        started["payload"]["content"][1]["blob_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:"))
    );

    let shutdown = runtime.shutdown().await;
    assert_mob_stop_allows_boundary_cancel(shutdown.mob_stop);
}

#[tokio::test]
#[ignore]
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
        after_retire["agent_sidebar"]["live_snapshot"]["agents"],
        json!([
            {
                "agent_id": "router",
                "member_id": "router",
                "label": "router",
                "kind": "mob_agent",
                "profile": "lead",
                "state": "active",
                "wired_to": [],
                "labels": {},
                "group": "lead",
                "addressable": true,
                "affordances": {
                    "addressable": true,
                    "can_respawn": true,
                    "can_retire": true,
                    "can_send_message": true,
                    "runtime_mode": "mob_agent"
                }
            }
        ])
    );
    assert_eq!(
        after_retire["topology"]["live_snapshot"]["nodes"],
        json!(["delivery", "router"])
    );
    assert_eq!(
        after_retire["topology"]["live_snapshot"]["node_count"],
        json!(2)
    );
    assert_eq!(
        after_retire["health_overview"]["live_snapshot"]["loaded_modules"],
        json!(["delivery", "router"])
    );
    assert_eq!(
        after_retire["health_overview"]["live_snapshot"]["loaded_module_count"],
        json!(2)
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert_mob_stop_allows_boundary_cancel(shutdown.mob_stop);
    let after_stop = get_console_experience(&app).await;
    assert_eq!(
        after_stop["health_overview"]["live_snapshot"]["running"],
        json!(false)
    );
}

#[tokio::test]
#[ignore]
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
#[ignore]
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
    let session_id = meerkat_mobkit::send_message_on_mob(
        &fixture.runtime.mob_handle(),
        selected_agent_id,
        "cross-panel hello".to_string(),
    )
    .await
    .expect("send_message to known agent should succeed");
    SessionId::parse(&session_id).expect("send_message should return a valid session_id");

    // Sending to an unknown agent should fail.
    let unknown_result = meerkat_mobkit::send_message_on_mob(
        &fixture.runtime.mob_handle(),
        "unknown-member-id",
        "should fail".to_string(),
    )
    .await;
    assert!(
        unknown_result.is_err(),
        "send_message to unknown agent should fail"
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert_mob_stop_allows_boundary_cancel(shutdown.mob_stop);
}

#[tokio::test]
#[ignore]
async fn phase_h1_multi_instance_profile_sidebar_enumerates_individual_agents() {
    let fixture = build_runtime_fixture().await;

    // Spawn two agents from the same profile with colon-namespaced IDs,
    // display_name labels, and one non-addressable singleton.
    let mut identity_luka_labels = std::collections::BTreeMap::new();
    identity_luka_labels.insert("display_name".to_string(), "Luka".to_string());
    identity_luka_labels.insert("group".to_string(), "Identity".to_string());

    let mut identity_parent_labels = std::collections::BTreeMap::new();
    identity_parent_labels.insert("display_name".to_string(), "Parent".to_string());
    identity_parent_labels.insert("group".to_string(), "Identity".to_string());

    let mut gate_labels = std::collections::BTreeMap::new();
    gate_labels.insert("addressable".to_string(), "false".to_string());
    gate_labels.insert("singleton".to_string(), "true".to_string());
    gate_labels.insert("group".to_string(), "Internal".to_string());

    let identity_luka = SpawnMemberSpec::new(
        meerkat_mob::ProfileName::from("lead"),
        MeerkatId::from("identity:luka"),
    )
    .with_labels(identity_luka_labels);
    let identity_parent = SpawnMemberSpec::new(
        meerkat_mob::ProfileName::from("lead"),
        MeerkatId::from("identity:parent"),
    )
    .with_labels(identity_parent_labels);
    let gate = SpawnMemberSpec::new(
        meerkat_mob::ProfileName::from("lead"),
        MeerkatId::from("gate:main"),
    )
    .with_labels(gate_labels);

    fixture
        .runtime
        .spawn(identity_luka)
        .await
        .expect("spawn identity:luka");
    fixture
        .runtime
        .spawn(identity_parent)
        .await
        .expect("spawn identity:parent");
    fixture.runtime.spawn(gate).await.expect("spawn gate:main");

    let app = fixture
        .runtime
        .build_reference_app_router(decision_state(false));
    let console_json = get_console_experience(&app).await;

    let agents = console_json["agent_sidebar"]["live_snapshot"]["agents"]
        .as_array()
        .expect("agents array");

    // All 3 individual agents should appear (not collapsed by profile).
    assert_eq!(agents.len(), 3, "expected 3 agents, got {}", agents.len());

    // Verify gate:main — non-addressable singleton.
    let gate_agent = agents
        .iter()
        .find(|a| a["agent_id"] == "gate:main")
        .expect("gate:main should be in sidebar");
    assert_eq!(gate_agent["label"], json!("gate:main")); // no display_name
    assert_eq!(gate_agent["addressable"], json!(false));
    assert_eq!(gate_agent["group"], json!("Internal"));
    assert_eq!(gate_agent["affordances"]["can_send_message"], json!(false));
    assert_eq!(gate_agent["affordances"]["can_retire"], json!(false));
    assert_eq!(gate_agent["affordances"]["can_respawn"], json!(true));

    // Verify identity:luka — display_name label renders as label.
    let luka_agent = agents
        .iter()
        .find(|a| a["agent_id"] == "identity:luka")
        .expect("identity:luka should be in sidebar");
    assert_eq!(luka_agent["label"], json!("Luka"));
    assert_eq!(luka_agent["group"], json!("Identity"));
    assert_eq!(luka_agent["addressable"], json!(true));
    assert_eq!(luka_agent["affordances"]["can_retire"], json!(true));

    // Verify identity:parent — second instance of same profile.
    let parent_agent = agents
        .iter()
        .find(|a| a["agent_id"] == "identity:parent")
        .expect("identity:parent should be in sidebar");
    assert_eq!(parent_agent["label"], json!("Parent"));
    assert_eq!(parent_agent["profile"], json!("lead"));

    // Verify profile_capabilities aggregation.
    let profile_caps = &console_json["runtime_capabilities"]["profile_capabilities"];
    assert_eq!(profile_caps["lead"]["instance_count"], json!(3));

    let shutdown = fixture.runtime.shutdown().await;
    assert_mob_stop_allows_boundary_cancel(shutdown.mob_stop);
}
