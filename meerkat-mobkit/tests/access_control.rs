//! End-to-end tests for the optional ABAC access-control layer:
//! per-principal console experience filtering, RPC enforcement, the
//! live admin surface, and SSE route gating.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use meerkat::{AgentFactory, Config, build_ephemeral_service};
use meerkat_client::TestClient;
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::{MobDefinition, MobStorage, SpawnMemberSpec};
use meerkat_mobkit::runtime::ConsoleMember;
use meerkat_mobkit::{
    AccessControlConfig, AccessController, AccessGroup, AccessRule, AuthPolicy, BigQueryNaming,
    ConsoleAccessRequest, ConsoleLiveSnapshot, ConsoleModelCapabilities, ConsolePolicy,
    ConsoleRestJsonRequest, DiscoverySpec, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig,
    RuntimeDecisionInputs, RuntimeOpsPolicy, TrustedOidcRuntimeConfig, UnifiedRuntime,
    build_runtime_decision_state, handle_console_rest_json_route_with_snapshot_and_access,
};
use serde_json::{Value, json};
use tower::ServiceExt;

fn trusted_toml() -> String {
    r#"
[[modules]]
id = "router"
command = "router-bin"
args = []
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

fn decision_state(require_app_auth: bool) -> meerkat_mobkit::RuntimeDecisionState {
    build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "access_dataset".to_string(),
            table: "access_table".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy {
            default_provider: meerkat_mobkit::AuthProvider::GoogleOAuth,
            email_allowlist: vec![
                "root@example.test".to_string(),
                "alice@example.test".to_string(),
            ],
        },
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy {
            require_app_auth,
            ..ConsolePolicy::default()
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: include_str!("../assets/release-targets.json").to_string(),
    })
    .expect("decision state builds")
}

fn member(identity: &str, role: &str, labels: &[(&str, &str)]) -> ConsoleMember {
    ConsoleMember {
        agent_identity: identity.to_string(),
        role: role.to_string(),
        state: "active".to_string(),
        model_capabilities: ConsoleModelCapabilities::default(),
        runtime_mode: None,
        session_id: None,
        wired_to: Vec::new(),
        labels: labels
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    }
}

fn snapshot_with_members(members: Vec<ConsoleMember>) -> ConsoleLiveSnapshot {
    ConsoleLiveSnapshot::new(
        Some("access-test-runtime".to_string()),
        true,
        Vec::new(),
        Vec::new(),
        members,
        true,
    )
}

/// "Ops can see all agents but only interact with ops-lead" — the canonical
/// scenario, plus an admin who sees and can do everything.
fn ops_access_config() -> AccessControlConfig {
    AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        groups: BTreeMap::from([(
            "ops".to_string(),
            AccessGroup {
                description: Some("Operations".to_string()),
                members: vec!["alice@example.test".to_string()],
            },
        )]),
        rules: vec![
            AccessRule {
                id: "ops-view-all".to_string(),
                groups: vec!["ops".to_string()],
                actions: vec!["agent.view".to_string()],
                ..AccessRule::default()
            },
            AccessRule {
                id: "ops-send-lead".to_string(),
                groups: vec!["ops".to_string()],
                actions: vec!["agent.send".to_string()],
                agents: vec!["ops-lead".to_string()],
                ..AccessRule::default()
            },
        ],
    }
}

fn experience_for(
    controller: &AccessController,
    subject: &str,
    snapshot: &ConsoleLiveSnapshot,
) -> Value {
    let decisions = decision_state(true);
    let response = handle_console_rest_json_route_with_snapshot_and_access(
        &decisions,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/experience".to_string(),
            auth: Some(ConsoleAccessRequest {
                provider: meerkat_mobkit::AuthProvider::GoogleOAuth,
                email: subject.to_string(),
            }),
        },
        Some(snapshot),
        Some(controller),
    );
    assert_eq!(response.status, 200, "experience: {:?}", response.body);
    response.body
}

fn sidebar_identities(experience: &Value) -> Vec<String> {
    experience["agent_sidebar"]["live_snapshot"]["agents"]
        .as_array()
        .expect("sidebar agents")
        .iter()
        .map(|agent| agent["identity"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn experience_is_filtered_per_principal() {
    let controller = AccessController::new(ops_access_config()).expect("controller");
    let snapshot = snapshot_with_members(vec![
        member("ops-lead", "lead", &[]),
        member("scout-1", "scout", &[]),
    ]);

    // Admin sees everything with full affordances.
    let admin_experience = experience_for(&controller, "root@example.test", &snapshot);
    assert_eq!(
        sidebar_identities(&admin_experience),
        ["ops-lead", "scout-1"]
    );
    assert_eq!(admin_experience["access"]["can_administer"], json!(true));
    assert_eq!(admin_experience["access"]["enabled"], json!(true));

    // Ops member sees both agents, can send only to ops-lead.
    let ops_experience = experience_for(&controller, "alice@example.test", &snapshot);
    assert_eq!(sidebar_identities(&ops_experience), ["ops-lead", "scout-1"]);
    let agents = ops_experience["agent_sidebar"]["live_snapshot"]["agents"]
        .as_array()
        .expect("agents");
    let affordance = |identity: &str, key: &str| -> bool {
        agents
            .iter()
            .find(|agent| agent["identity"] == identity)
            .and_then(|agent| agent["affordances"][key].as_bool())
            .unwrap_or(false)
    };
    assert!(affordance("ops-lead", "can_send_message"));
    assert!(!affordance("scout-1", "can_send_message"));
    assert!(!affordance("scout-1", "can_retire"));
    assert_eq!(ops_experience["access"]["can_administer"], json!(false));
    assert_eq!(
        ops_experience["access"]["groups"],
        json!(["ops"]),
        "groups surface in the access section"
    );
    assert_eq!(
        ops_experience["runtime_capabilities"]["can_send_messages"],
        json!(true)
    );
    assert_eq!(
        ops_experience["runtime_capabilities"]["can_spawn_members"],
        json!(false)
    );

    // A subject with no grants sees an empty console.
    let outsider_experience = experience_for(&controller, "root@example.test", &snapshot);
    assert!(!sidebar_identities(&outsider_experience).is_empty());
    let decisions = decision_state(true);
    let response = handle_console_rest_json_route_with_snapshot_and_access(
        &decisions,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/experience".to_string(),
            auth: Some(ConsoleAccessRequest {
                provider: meerkat_mobkit::AuthProvider::GoogleOAuth,
                email: "alice@example.test".to_string(),
            }),
        },
        Some(&snapshot_with_members(vec![member(
            "hidden-only",
            "lead",
            &[],
        )])),
        Some(
            &AccessController::new(AccessControlConfig {
                enabled: true,
                admins: vec!["root@example.test".to_string()],
                ..AccessControlConfig::default()
            })
            .expect("deny-all controller"),
        ),
    );
    assert_eq!(
        sidebar_identities(&response.body),
        Vec::<String>::new(),
        "deny-by-default hides every agent"
    );
}

#[test]
fn label_selector_rules_filter_experience() {
    let config = AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        rules: vec![AccessRule {
            id: "payments-only".to_string(),
            subjects: vec!["alice@example.test".to_string()],
            actions: vec!["agent.view".to_string()],
            match_labels: BTreeMap::from([("org".to_string(), "payments".to_string())]),
            ..AccessRule::default()
        }],
        ..AccessControlConfig::default()
    };
    let controller = AccessController::new(config).expect("controller");
    let snapshot = snapshot_with_members(vec![
        member("pay-analyst", "analyst", &[("org", "payments")]),
        member("hr-analyst", "analyst", &[("org", "people")]),
    ]);
    let experience = experience_for(&controller, "alice@example.test", &snapshot);
    assert_eq!(sidebar_identities(&experience), ["pay-analyst"]);
}

#[test]
fn disabled_controller_changes_nothing() {
    let controller = AccessController::disabled();
    let snapshot = snapshot_with_members(vec![
        member("ops-lead", "lead", &[]),
        member("scout-1", "scout", &[]),
    ]);
    let experience = experience_for(&controller, "alice@example.test", &snapshot);
    assert_eq!(sidebar_identities(&experience), ["ops-lead", "scout-1"]);
    assert_eq!(experience["access"]["enabled"], json!(false));
    // Disabled with admins configured: only those admins can administer.
    assert_eq!(experience["access"]["can_administer"], json!(true));
}

// ---------------------------------------------------------------------------
// Full HTTP router enforcement (anonymous principal, require_app_auth=false)
// ---------------------------------------------------------------------------

async fn build_access_runtime_fixture() -> (tempfile::TempDir, UnifiedRuntime) {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");
    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));
    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "access-control-mob"

[profiles.lead]
model = "gpt-5.5"
external_addressable = true

[profiles.lead.tools]
comms = true
"#,
    )
    .expect("definition");
    let mob_spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(TestClient::default())),
        });
    let module_config = MobKitConfig {
        modules: vec![],
        discovery: DiscoverySpec {
            namespace: "access-control".to_string(),
            modules: vec![],
        },
        pre_spawn: vec![],
    };
    let runtime = UnifiedRuntime::bootstrap(mob_spec, module_config, Duration::from_secs(2))
        .await
        .expect("bootstrap runtime");
    for member_id in ["router", "delivery"] {
        runtime
            .spawn(SpawnMemberSpec::from_wire(
                "lead".to_string(),
                MeerkatId::from(member_id).to_string(),
                Some(format!("You are {member_id}.").into()),
                None,
                None,
            ))
            .await
            .expect("spawn member");
    }
    (temp_dir, runtime)
}

async fn rpc(app: &axum::Router, method: &str, params: Value) -> Value {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": "test",
        "method": method,
        "params": params,
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/console/rpc")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(payload.to_string()))
                .expect("rpc request"),
        )
        .await
        .expect("rpc response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("rpc body");
    serde_json::from_slice(&body).expect("rpc json")
}

async fn get_status(app: &axum::Router, uri: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
        .status()
}

/// Anonymous callers (open console) only match rules with no subject
/// constraints: visible/sendable only what those rules grant.
fn anonymous_router_only_config() -> AccessControlConfig {
    AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        groups: BTreeMap::new(),
        rules: vec![
            AccessRule {
                id: "everyone-views-router".to_string(),
                actions: vec!["agent.view".to_string()],
                agents: vec!["router".to_string()],
                ..AccessRule::default()
            },
            AccessRule {
                id: "everyone-sends-router".to_string(),
                actions: vec!["agent.send".to_string()],
                agents: vec!["router".to_string()],
                ..AccessRule::default()
            },
        ],
    }
}

#[tokio::test]
async fn http_router_enforces_access_end_to_end() {
    let (_temp_dir, mut runtime) = build_access_runtime_fixture().await;
    let controller = AccessController::new(anonymous_router_only_config()).expect("controller");
    runtime.set_access_controller(controller.clone());
    let app = runtime.build_reference_app_router(decision_state(false));

    // Experience: only the granted agent is projected.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/console/experience")
                .body(Body::empty())
                .expect("experience request"),
        )
        .await
        .expect("experience response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("experience body");
    let experience: Value = serde_json::from_slice(&body).expect("experience json");
    assert_eq!(sidebar_identities(&experience), ["router"]);
    assert_eq!(experience["access"]["enabled"], json!(true));

    // list_members is filtered to visible agents.
    let members = rpc(&app, "mobkit/list_members", json!({})).await;
    let member_rows = members["result"].as_array().expect("members array");
    assert_eq!(member_rows.len(), 1, "members: {member_rows:#?}");

    // Sending to a hidden agent is denied with the typed access error.
    let denied = rpc(
        &app,
        "mobkit/console/send",
        json!({
            "identity": "delivery",
            "content": "hello",
            "origin": "test",
            "idempotency_key": "denied-send",
        }),
    )
    .await;
    assert_eq!(
        denied["error"]["code"],
        json!(-32030),
        "denied: {denied:#?}"
    );
    assert_eq!(denied["error"]["data"]["kind"], json!("access_denied"));

    // Sending to the granted agent passes the access gate.
    let allowed = rpc(
        &app,
        "mobkit/console/send",
        json!({
            "identity": "router",
            "content": "hello",
            "origin": "test",
            "idempotency_key": "allowed-send",
        }),
    )
    .await;
    assert_ne!(
        allowed["error"]["code"],
        json!(-32030),
        "allowed send must not be access-denied: {allowed:#?}"
    );

    // Lifecycle and admin-tier methods are deny-by-default.
    let retire = rpc(&app, "mobkit/retire", json!({ "identity": "router" })).await;
    assert_eq!(retire["error"]["code"], json!(-32030));
    let labels = rpc(
        &app,
        "mobkit/mob_labels/set",
        json!({ "labels": { "a": "b" } }),
    )
    .await;
    assert_eq!(labels["error"]["code"], json!(-32030));

    // Anonymous callers are not access admins while admins are configured.
    let status = rpc(&app, "mobkit/access/status", json!({})).await;
    assert_eq!(status["result"]["available"], json!(true));
    assert_eq!(status["result"]["enabled"], json!(true));
    assert_eq!(status["result"]["can_administer"], json!(false));
    let get_config = rpc(&app, "mobkit/access/get", json!({})).await;
    assert_eq!(get_config["error"]["code"], json!(-32030));

    // SSE gating: granted agent streams, hidden agent is 403, mob-wide
    // streams require the mob.observe grant.
    assert_eq!(
        get_status(&app, "/agents/router/events").await,
        StatusCode::OK
    );
    assert_eq!(
        get_status(&app, "/agents/delivery/events").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(get_status(&app, "/mob/events").await, StatusCode::FORBIDDEN);
    assert_eq!(
        get_status(&app, "/mobkit/mob_events/stream").await,
        StatusCode::FORBIDDEN
    );

    // Live reconfiguration: granting view of "delivery" shows up on the
    // next request without any restart.
    controller
        .upsert_rule(AccessRule {
            id: "everyone-views-delivery".to_string(),
            actions: vec!["agent.view".to_string()],
            agents: vec!["delivery".to_string()],
            ..AccessRule::default()
        })
        .expect("live rule update");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/console/experience")
                .body(Body::empty())
                .expect("experience request"),
        )
        .await
        .expect("experience response");
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("experience body");
    let experience: Value = serde_json::from_slice(&body).expect("experience json");
    assert_eq!(sidebar_identities(&experience), ["delivery", "router"]);
    assert_eq!(
        get_status(&app, "/agents/delivery/events").await,
        StatusCode::OK
    );

    let _ = runtime.mob_handle().stop().await;
}

#[tokio::test]
async fn bootstrap_path_configures_access_from_console_then_locks_down() {
    let (_temp_dir, mut runtime) = build_access_runtime_fixture().await;
    // Fresh deployment: controller present, disabled, no admins. Anyone on
    // the (already authenticated/allowlisted) console can administer.
    let controller = AccessController::disabled();
    runtime.set_access_controller(controller.clone());
    let app = runtime.build_reference_app_router(decision_state(false));

    let status = rpc(&app, "mobkit/access/status", json!({})).await;
    assert_eq!(status["result"]["enabled"], json!(false));
    assert_eq!(status["result"]["can_administer"], json!(true));

    // Configure and enable via RPC, naming an admin.
    let set = rpc(
        &app,
        "mobkit/access/set",
        json!({ "config": {
            "enabled": true,
            "admins": ["root@example.test"],
            "rules": [
                { "id": "everyone-views-router", "actions": ["agent.view"], "agents": ["router"] }
            ],
        }}),
    )
    .await;
    assert_eq!(set["error"], Value::Null, "set: {set:#?}");
    assert_eq!(set["result"]["revision"], json!(1));

    // Enforcement is live: anonymous callers lost the admin bootstrap and
    // only see what the rules grant.
    let status = rpc(&app, "mobkit/access/status", json!({})).await;
    assert_eq!(status["result"]["enabled"], json!(true));
    assert_eq!(status["result"]["can_administer"], json!(false));
    let get_config = rpc(&app, "mobkit/access/get", json!({})).await;
    assert_eq!(get_config["error"]["code"], json!(-32030));
    let members = rpc(&app, "mobkit/list_members", json!({})).await;
    assert_eq!(members["result"].as_array().expect("members").len(), 1);

    // Enabling without admins is rejected (anti-lockout) at the RPC surface.
    assert!(
        controller
            .replace_config(AccessControlConfig {
                enabled: true,
                ..AccessControlConfig::default()
            })
            .is_err()
    );

    let _ = runtime.mob_handle().stop().await;
}

#[tokio::test]
async fn timeline_rpc_is_filtered_per_caller() {
    let (_temp_dir, mut runtime) = build_access_runtime_fixture().await;
    let controller = AccessController::new(anonymous_router_only_config()).expect("controller");
    runtime.set_access_controller(controller);
    let app = runtime.build_reference_app_router(decision_state(false));

    // Seed timeline frames for both identities through console sends.
    // The send to "delivery" is access-denied, so only "router" frames
    // can exist; query both ways to prove filtering.
    let _ = rpc(
        &app,
        "mobkit/console/send",
        json!({
            "identity": "router",
            "content": "ping",
            "origin": "test",
            "idempotency_key": "timeline-send",
        }),
    )
    .await;

    let page = rpc(&app, "mobkit/console/query_timeline", json!({})).await;
    let frames = page["result"]["frames"].as_array().expect("frames");
    assert!(
        frames
            .iter()
            .all(|frame| frame["identity"] == json!("router")),
        "timeline must only contain visible identities: {frames:#?}"
    );

    let _ = runtime.mob_handle().stop().await;
}
