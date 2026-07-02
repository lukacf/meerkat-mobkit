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
// meerkat 0.7: the MeerkatId alias was deleted; member ids are AgentIdentity.
use meerkat_mob::ids::AgentIdentity as MeerkatId;
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
use sha2::{Digest, Sha256};
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
                // Authenticated + allowlisted but granted nothing by the rules,
                // for the deny-by-default "outsider sees an empty console" case.
                "carol@example.test".to_string(),
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
        .map(|agent| {
            // Member rows carry `identity`; module-fallback rows carry only
            // `agent_id`. Use whichever identifies the row.
            agent["identity"]
                .as_str()
                .filter(|value| !value.is_empty())
                .or_else(|| agent["agent_id"].as_str())
                .unwrap_or_default()
                .to_string()
        })
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

    // An authenticated subject that no rule grants sees an empty console,
    // even though the config has allow rules for others (deny-by-default).
    let outsider_experience = experience_for(&controller, "carol@example.test", &snapshot);
    assert_eq!(
        sidebar_identities(&outsider_experience),
        Vec::<String>::new(),
        "an ungranted authenticated subject sees no agents"
    );
    assert_eq!(
        outsider_experience["access"]["can_administer"],
        json!(false)
    );
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
fn module_fallback_rows_are_filtered_for_denied_callers() {
    // When every roster member is filtered out, the sidebar falls back to
    // module-agent rows built from `loaded_modules`; those must be gated
    // by `agent.view` like any other agent row.
    let controller = AccessController::new(AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        ..AccessControlConfig::default()
    })
    .expect("deny-all controller");
    let snapshot = ConsoleLiveSnapshot::new(
        Some("access-test-runtime".to_string()),
        true,
        vec!["router".to_string()],
        Vec::new(),
        Vec::new(),
        false,
    );
    let denied = experience_for(&controller, "alice@example.test", &snapshot);
    assert_eq!(
        sidebar_identities(&denied),
        Vec::<String>::new(),
        "module fallback rows must not leak to denied callers"
    );
    let admin = experience_for(&controller, "root@example.test", &snapshot);
    assert_eq!(sidebar_identities(&admin).len(), 1, "admin keeps modules");

    // Partial grant: with two module rows and a rule granting view of only
    // one, the sidebar must show exactly that one (not all-or-nothing).
    let partial = AccessController::new(AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        rules: vec![AccessRule {
            id: "carol-views-router".to_string(),
            subjects: vec!["carol@example.test".to_string()],
            actions: vec!["agent.view".to_string()],
            agents: vec!["router".to_string()],
            ..AccessRule::default()
        }],
        ..AccessControlConfig::default()
    })
    .expect("partial controller");
    let two_modules = ConsoleLiveSnapshot::new(
        Some("access-test-runtime".to_string()),
        true,
        vec!["router".to_string(), "delivery".to_string()],
        Vec::new(),
        Vec::new(),
        false,
    );
    let scoped = experience_for(&partial, "carol@example.test", &two_modules);
    assert_eq!(
        sidebar_identities(&scoped),
        ["router"],
        "only the granted module row is visible"
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
fn identity_first_console_identity_drives_filtering() {
    // Identity-first: the console identity (labels.agent_identity) differs
    // from the runtime member id (member.agent_identity). Rules written in
    // console-identity terms must filter correctly, and the runtime member id
    // must resolve back via the agent_id fallback — neither direction may
    // leak the agent the caller is not granted.
    let config = AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        rules: vec![
            AccessRule {
                id: "alice-views-lead-by-identity".to_string(),
                subjects: vec!["alice@example.test".to_string()],
                actions: vec!["agent.view".to_string()],
                agents: vec!["identity:ops-lead".to_string()],
                ..AccessRule::default()
            },
            AccessRule {
                id: "alice-sends-lead-by-runtime-id".to_string(),
                subjects: vec!["alice@example.test".to_string()],
                actions: vec!["agent.send".to_string()],
                // Written against the runtime member id, not the console identity.
                agents: vec!["member-runtime-7".to_string()],
                ..AccessRule::default()
            },
        ],
        ..AccessControlConfig::default()
    };
    let controller = AccessController::new(config).expect("controller");
    let snapshot = snapshot_with_members(vec![
        // Console identity (label) != runtime member id (agent_identity).
        member(
            "member-runtime-7",
            "lead",
            &[("agent_identity", "identity:ops-lead")],
        ),
        member(
            "member-runtime-8",
            "scout",
            &[("agent_identity", "identity:scout-9")],
        ),
    ]);
    let experience = experience_for(&controller, "alice@example.test", &snapshot);
    // Only the granted agent appears, keyed by its console identity.
    assert_eq!(sidebar_identities(&experience), ["identity:ops-lead"]);
    let agents = experience["agent_sidebar"]["live_snapshot"]["agents"]
        .as_array()
        .expect("agents");
    let lead = agents
        .iter()
        .find(|agent| agent["identity"] == "identity:ops-lead")
        .expect("lead row");
    // The send grant was written against the runtime member id; it must still
    // resolve via the agent_id fallback.
    assert_eq!(lead["affordances"]["can_send_message"], json!(true));
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
    // Wire-contract pin: the published SDKs index `state` on member rows
    // (Python MemberSnapshot.from_dict does `data["state"]`), so the meerkat
    // 0.7 `status` projection must keep emitting the `state` key in the
    // console state vocabulary.
    assert_eq!(
        member_rows[0]["state"],
        json!("active"),
        "member rows must carry the `state` wire key: {member_rows:#?}"
    );

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

    // Plumbing and flow-state reads that enumerate identities without
    // per-agent filtering are gated behind their operating tiers.
    for method in [
        "mobkit/routing/routes/list",
        "mobkit/delivery/history",
        "mobkit/cross_mob/directory",
        "mobkit/mob_labels/get",
        "mobkit/list_runs",
        "mobkit/list_flows",
    ] {
        let denied = rpc(&app, method, json!({})).await;
        assert_eq!(
            denied["error"]["code"],
            json!(-32030),
            "{method} must be access-gated: {denied:#?}"
        );
    }

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
async fn multipart_send_denied_does_not_write_blob_before_access_gate() {
    // Regression: the multipart `mobkit/console/send` path must run the
    // `agent.send` ABAC gate on the target identity BEFORE externalizing /
    // persisting uploaded image bytes. Otherwise a caller denied send to an
    // identity can still write attacker-supplied bytes into that identity's
    // blob store (a pre-auth side effect / storage-amplification vector).
    let (_temp_dir, mut runtime) = build_access_runtime_fixture().await;
    let controller = AccessController::new(anonymous_router_only_config()).expect("controller");
    runtime.set_access_controller(controller);
    let app = runtime.build_reference_app_router(decision_state(false));

    // `delivery` is NOT granted agent.send (only `router` is).
    let image_bytes = b"denied-multipart-png-bytes";
    let boundary = "mobkit-access-boundary";
    let payload = json!({
        "jsonrpc": "2.0",
        "id": "denied-multipart-send",
        "method": "mobkit/console/send",
        "params": {
            "identity": "delivery",
            "origin": "test",
            "idempotency_key": "denied-multipart-send",
            "content": [
                { "type": "image_upload", "upload_id": "u1", "media_type": "image/png" }
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
        b"Content-Disposition: form-data; name=\"file:u1\"; filename=\"x.png\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: image/png\r\n\r\n");
    body.extend_from_slice(image_bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let response = app
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
    assert_eq!(response.status(), StatusCode::OK);
    let resp_body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("multipart body");
    let resp_json: Value = serde_json::from_slice(&resp_body).expect("multipart json");
    assert_eq!(
        resp_json["error"]["code"],
        json!(-32030),
        "denied multipart send must be access-denied: {resp_json:#?}"
    );
    assert_eq!(resp_json["error"]["data"]["kind"], json!("access_denied"));

    // The uploaded bytes must NOT have been written: the content-addressed
    // blob is not retrievable (the gate fired before externalization). The
    // blob id hashes `media_type || 0x00 || bytes` (see compute_blob_id).
    let mut hasher = Sha256::new();
    hasher.update(b"image/png");
    hasher.update([0]);
    hasher.update(image_bytes);
    let blob_id = format!("sha256:{:x}", hasher.finalize());
    let blob_status = get_status(&app, &format!("/blobs/{blob_id}")).await;
    assert_eq!(
        blob_status,
        StatusCode::NOT_FOUND,
        "denied upload bytes must not be persisted before the access gate"
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

/// A label-keyed rule must resolve on the timeline read path even when the
/// caller never hit `/console/experience` first — the handler primes the
/// attribute cache from the roster itself. Guards the seam-priming fix
/// (shared by the windowed REST handler, the SSE timeline stream, and the
/// RPC/SSE event surfaces): without priming, the label rule would fail closed
/// and the labelled agent's frames would wrongly vanish.
#[tokio::test]
async fn timeline_role_rules_resolve_without_prior_experience() {
    let (_temp_dir, mut runtime) = build_access_runtime_fixture().await;
    // Everyone may view + send agents whose role is "lead" (the fixture
    // members' profile). Role is an attribute that only resolves through the
    // primed cache — an identity-only surface can't see it otherwise.
    let controller = AccessController::new(AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        rules: vec![AccessRule {
            id: "lead-role-only".to_string(),
            actions: vec!["agent.view".to_string(), "agent.send".to_string()],
            roles: vec!["lead".to_string()],
            ..AccessRule::default()
        }],
        ..AccessControlConfig::default()
    })
    .expect("controller");
    runtime.set_access_controller(controller);
    let app = runtime.build_reference_app_router(decision_state(false));

    // The send gate itself resolves the role only because the RPC seam primes
    // the cache — without priming this would fail closed (`-32030`) and no
    // frame would exist.
    let send = rpc(
        &app,
        "mobkit/console/send",
        json!({
            "identity": "router",
            "content": "ping",
            "origin": "test",
            "idempotency_key": "role-cold-send",
        }),
    )
    .await;
    assert_eq!(
        send["error"],
        Value::Null,
        "role-keyed send must resolve cold: {send:#?}"
    );

    // Query the timeline cold (no prior /console/experience). The per-frame
    // `agent.view` filter resolves the role only because the read path primes;
    // without priming the role allow would fail closed and the frame would
    // wrongly vanish.
    // Querying the timeline cold likewise resolves the role attribute through
    // the read-path priming (the response is access-denied only if priming is
    // missing).
    let page = rpc(
        &app,
        "mobkit/console/query_timeline",
        json!({ "mode": "recent", "limit": 200 }),
    )
    .await;
    assert_eq!(
        page["error"],
        Value::Null,
        "cold timeline query must succeed: {page:#?}"
    );
    // Any frames returned belong only to role-matched agents.
    if let Some(frames) = page["result"]["frames"].as_array() {
        assert!(
            frames
                .iter()
                .all(|frame| frame["identity"] == json!("router")),
            "only role-matched agents are visible: {frames:#?}"
        );
    }

    // The SSE timeline stream is reachable for the caller and primes the same
    // cache before its per-frame filter.
    assert_eq!(
        get_status(&app, "/console/timeline/stream").await,
        StatusCode::OK
    );

    let _ = runtime.mob_handle().stop().await;
}

/// Regression: REST `/console/send` evaluates the `agent.send` decision
/// before any cache-warming request (experience/timeline/RPC) has run, so
/// the handler must prime the attribute cache itself. Without priming, a
/// label-scoped deny rule does not match on a cold cache (the decision
/// degrades to a bare-identity resource) and a scripted caller whose FIRST
/// request is `/console/send` reaches a member the rule was meant to
/// exclude.
#[tokio::test]
async fn rest_console_send_resolves_label_deny_on_cold_cache() {
    let (_temp_dir, mut runtime) = build_access_runtime_fixture().await;
    // A member the deny rule is scoped to by label.
    runtime
        .spawn(
            SpawnMemberSpec::from_wire(
                "lead".to_string(),
                MeerkatId::from("red-shadow").to_string(),
                Some("You are red-shadow.".into()),
                None,
                None,
            )
            .with_labels(BTreeMap::from([("team".to_string(), "red".to_string())])),
        )
        .await
        .expect("spawn labeled member");
    let controller = AccessController::new(AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        rules: vec![
            AccessRule {
                id: "everyone-sends".to_string(),
                actions: vec!["agent.send".to_string()],
                ..AccessRule::default()
            },
            AccessRule {
                id: "deny-red-team-send".to_string(),
                effect: meerkat_mobkit::AccessEffect::Deny,
                actions: vec!["agent.send".to_string()],
                match_labels: BTreeMap::from([("team".to_string(), "red".to_string())]),
                ..AccessRule::default()
            },
        ],
        ..AccessControlConfig::default()
    })
    .expect("controller");
    runtime.set_access_controller(controller);
    let app = runtime.build_reference_app_router(decision_state(false));

    let rest_send = |identity: &'static str, idempotency_key: &'static str| {
        let app = app.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/console/send")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "identity": identity,
                            "content": "hello",
                            "origin": "test",
                            "idempotency_key": idempotency_key,
                        })
                        .to_string(),
                    ))
                    .expect("send request"),
            )
            .await
            .expect("send response")
            .status()
        }
    };

    // FIRST request of the process: the label-scoped deny must already
    // resolve — without handler-level priming this read 200.
    assert_eq!(
        rest_send("red-shadow", "cold-deny-send").await,
        StatusCode::FORBIDDEN,
        "label-scoped deny must resolve on a cold cache"
    );

    // Members outside the deny scope still pass the same gate cold.
    assert_ne!(
        rest_send("router", "cold-allow-send").await,
        StatusCode::FORBIDDEN,
        "non-matching members must keep passing the send gate"
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
    // can exist; query to prove filtering.
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

    let page = rpc(
        &app,
        "mobkit/console/query_timeline",
        json!({ "mode": "recent", "limit": 200 }),
    )
    .await;
    let frames = page["result"]["frames"].as_array().expect("frames");
    assert!(
        frames
            .iter()
            .all(|frame| frame["identity"] == json!("router")),
        "timeline must only contain visible identities: {frames:#?}"
    );

    let _ = runtime.mob_handle().stop().await;
}

/// `mob.observe` opens the whole-mob event surface, but per-agent
/// `agent.view` still filters which agents' events flow through it — a
/// mob.observe grant must not reveal the events/lifecycle of an agent the
/// caller is denied `agent.view` on.
#[tokio::test]
async fn mob_observe_does_not_bypass_per_agent_view() {
    let (_temp_dir, mut runtime) = build_access_runtime_fixture().await;
    // Anonymous callers may observe the mob and view "router" only.
    let controller = AccessController::new(AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        rules: vec![
            AccessRule {
                id: "everyone-observes".to_string(),
                actions: vec!["mob.observe".to_string()],
                ..AccessRule::default()
            },
            AccessRule {
                id: "everyone-views-router".to_string(),
                actions: vec!["agent.view".to_string()],
                agents: vec!["router".to_string()],
                ..AccessRule::default()
            },
        ],
        ..AccessControlConfig::default()
    })
    .expect("controller");
    runtime.set_access_controller(controller);
    let app = runtime.build_reference_app_router(decision_state(false));

    // The mob.observe gate opens the surface (not a -32030 denial)...
    let page = rpc(&app, "mobkit/mob_events/query", json!({})).await;
    assert_eq!(
        page["error"],
        Value::Null,
        "observe surface open: {page:#?}"
    );
    let events = page["result"]["events"].as_array().expect("events");
    // ...but every agent-attributed event belongs to the one viewable agent.
    // "delivery" was spawned in the fixture; its lifecycle must not leak.
    assert!(
        events.iter().all(|event| {
            event["agent_identity"].is_null() || event["agent_identity"] == json!("router")
        }),
        "mob.observe must not surface denied agents' events: {events:#?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event["agent_identity"] == json!("router")),
        "router's own ledger entries should still be visible: {events:#?}"
    );

    let _ = runtime.mob_handle().stop().await;
}

// ---------------------------------------------------------------------------
// §10.3 memory read actions + §9.3 console Memory panel
// ---------------------------------------------------------------------------

/// Seed a bundled store with one record per scope, a supersede chain, a
/// quarantined record, an injection-ledger row, one steward dream's audit
/// rows, and two gated promotions parked in the quarantine queue
/// (`gate-mob-promotion` targeting mob scope, `gate-delivery-promotion`
/// targeting the "delivery" identity). Returns (store, router_active_id,
/// router_quarantined_id, delivery_id, mob_record_id, operator_record_id).
async fn seeded_memory_store(
    root: &std::path::Path,
) -> (
    meerkat_mobkit::SqliteAgentMemoryStore,
    String,
    String,
    String,
    String,
    String,
) {
    use meerkat_mobkit::memory::records::{
        InjectionLogEntry, InjectionSurface, MemoryAuthor, MemoryKind,
    };
    use meerkat_mobkit::{
        AgentMemoryProvider, MemoryScope, NewMemoryRecord, SqliteAgentMemoryStore,
        StagedMemoryStore, StagedMutationBatch, StagedOp, TrustTier,
    };

    struct AlwaysQuarantine;
    impl meerkat_mobkit::memory::taint::LlmWriteGate for AlwaysQuarantine {
        fn quarantine_reason(
            &self,
            author: &MemoryAuthor,
            _kind: meerkat_mobkit::memory::staged::StagedBatchKind,
            _evidence: &[meerkat_mobkit::memory::records::EvidenceRef],
        ) -> Option<String> {
            author.is_llm().then(|| "test taint".to_string())
        }
    }

    let store = SqliteAgentMemoryStore::open(root).expect("open store");
    let record = |title: &str| NewMemoryRecord {
        kind: MemoryKind::Fact,
        title: title.to_string(),
        description: format!("{title} description"),
        body: format!("{title} body"),
        tags: Vec::new(),
        evidence: Vec::new(),
        verification: None,
    };
    let identity_scope = |identity: &str| MemoryScope::Identity {
        realm: "default".to_string(),
        identity: identity.to_string(),
    };

    let router_root = store
        .remember_authored(
            &identity_scope("router"),
            record("Router root"),
            MemoryAuthor::Operator,
        )
        .await
        .expect("router root");
    let router_tip = store
        .supersede_authored(
            &identity_scope("router"),
            &router_root.memory_id,
            record("Router tip"),
            MemoryAuthor::Operator,
        )
        .await
        .expect("router tip");
    let delivery = store
        .remember_authored(
            &identity_scope("delivery"),
            record("Delivery fact"),
            MemoryAuthor::Operator,
        )
        .await
        .expect("delivery record");
    let mob_record = store
        .remember_authored(
            &MemoryScope::Mob {
                realm: "default".to_string(),
                mob: "access-control-mob".to_string(),
            },
            record("Mob convention"),
            MemoryAuthor::Operator,
        )
        .await
        .expect("mob record");
    store
        .remember_authored(
            &MemoryScope::Realm {
                realm: "default".to_string(),
            },
            record("Realm fact"),
            MemoryAuthor::Operator,
        )
        .await
        .expect("realm record");
    // Operator scope is live (P4 provisional keying) and carries cross-mob
    // personal facts — the panel gates it on operator.memory.read.
    let operator_record = store
        .remember_authored(
            &MemoryScope::Operator {
                realm: "default".to_string(),
                operator: "op-luka".to_string(),
            },
            record("Operator preference"),
            MemoryAuthor::Operator,
        )
        .await
        .expect("operator record");

    // Quarantined write: LLM author through the installed write gate.
    store.set_llm_write_gate(Arc::new(AlwaysQuarantine));
    let quarantined = store
        .remember_authored(
            &identity_scope("router"),
            record("Router quarantined claim"),
            MemoryAuthor::Agent {
                identity: "router".to_string(),
            },
        )
        .await
        .expect("quarantined record");
    assert!(
        matches!(
            quarantined.status,
            meerkat_mobkit::memory::records::RecordStatus::Quarantined { .. }
        ),
        "seed record should land quarantined: {quarantined:?}"
    );

    // One injection-ledger row for the tip record.
    store
        .log_injections(
            "default",
            &[InjectionLogEntry {
                record_id: router_tip.memory_id.clone(),
                identity: "router".to_string(),
                session_key: Some("sess-1".to_string()),
                surface: InjectionSurface::Build,
                at_ms: 1,
            }],
        )
        .await
        .expect("injection row");

    // One steward dream commit → audit rows for the dreams surface.
    let token = store
        .stage(StagedMutationBatch {
            kind: meerkat_mobkit::memory::staged::StagedBatchKind::FreshWrite,
            realm: "default".to_string(),
            author: MemoryAuthor::Steward {
                run_id: "run-dream-1".to_string(),
            },
            ops: vec![StagedOp::Create {
                id: None,
                scope: MemoryScope::Mob {
                    realm: "default".to_string(),
                    mob: "access-control-mob".to_string(),
                },
                record: record("Dream consolidated"),
                trust: TrustTier::AgentObserved,
                derived_from: Vec::new(),
                rationale: Some("consolidated during dream".to_string()),
                created_at_ms: None,
                updated_at_ms: None,
            }],
        })
        .await
        .expect("stage dream batch");
    store.commit(token).await.expect("commit dream batch");

    // Two gated promotions parked in the queue (staged, never committed):
    // the quarantine panel gates each row on the target scope's read grant.
    let stage_promotion = |scope: MemoryScope, title: &str| {
        let store = store.clone();
        let record = record(title);
        async move {
            store
                .stage(StagedMutationBatch {
                    kind: meerkat_mobkit::memory::staged::StagedBatchKind::FreshWrite,
                    realm: "default".to_string(),
                    author: MemoryAuthor::Steward {
                        run_id: "run-gate-1".to_string(),
                    },
                    ops: vec![StagedOp::Create {
                        id: None,
                        scope,
                        record,
                        trust: TrustTier::AgentObserved,
                        derived_from: Vec::new(),
                        rationale: Some("gated promotion".to_string()),
                        created_at_ms: None,
                        updated_at_ms: None,
                    }],
                })
                .await
                .expect("stage promotion batch")
        }
    };
    let mob_stage = stage_promotion(
        MemoryScope::Mob {
            realm: "default".to_string(),
            mob: "access-control-mob".to_string(),
        },
        "Promoted mob claim",
    )
    .await;
    store
        .record_pending_promotion(
            "default",
            meerkat_mobkit::memory::PendingPromotion {
                pending_id: "gate-mob-promotion".to_string(),
                stage_token: mob_stage.token,
                record_id: quarantined.memory_id.clone(),
                scope_kind: "mob".to_string(),
                scope_key: "access-control-mob".to_string(),
                rationale: Some("steward: mob-wide convention".to_string()),
                status: "pending".to_string(),
                created_at_ms: 2,
            },
        )
        .await
        .expect("mob promotion row");
    let delivery_stage =
        stage_promotion(identity_scope("delivery"), "Promoted delivery claim").await;
    store
        .record_pending_promotion(
            "default",
            meerkat_mobkit::memory::PendingPromotion {
                pending_id: "gate-delivery-promotion".to_string(),
                stage_token: delivery_stage.token,
                record_id: quarantined.memory_id.clone(),
                scope_kind: "identity".to_string(),
                scope_key: "delivery".to_string(),
                rationale: Some("steward: delivery personal fact".to_string()),
                status: "pending".to_string(),
                created_at_ms: 3,
            },
        )
        .await
        .expect("delivery promotion row");

    (
        store,
        router_tip.memory_id,
        quarantined.memory_id,
        delivery.memory_id,
        mob_record.memory_id,
        operator_record.memory_id,
    )
}

#[tokio::test]
async fn memory_panel_reads_seeded_store_without_access_control() {
    let (_temp_dir, runtime) = build_access_runtime_fixture().await;
    let memory_dir = tempfile::tempdir().expect("memory dir");
    let (store, tip_id, quarantined_id, _delivery_id, _mob_id, _operator_id) =
        seeded_memory_store(memory_dir.path()).await;
    runtime.set_memory_panel_store(store);
    let app = runtime.build_reference_app_router(decision_state(false));

    // Capability advertisement follows the provider-dependent pattern.
    let capabilities = rpc(&app, "mobkit/capabilities", json!({})).await;
    let methods = capabilities["result"]["methods"]
        .as_array()
        .expect("methods");
    for method in [
        "mobkit/memory/panel/records",
        "mobkit/memory/panel/record",
        "mobkit/memory/panel/quarantine",
        "mobkit/memory/panel/dreams",
    ] {
        assert!(
            methods.iter().any(|value| value == method),
            "{method} must be advertised: {methods:#?}"
        );
    }

    // Records: every scope visible, list rows body-free.
    let records = rpc(&app, "mobkit/memory/panel/records", json!({})).await;
    assert_eq!(records["error"], Value::Null, "{records:#?}");
    let rows = records["result"]["records"].as_array().expect("records");
    assert!(rows.len() >= 5, "all seeded records: {rows:#?}");
    assert!(
        rows.iter().all(|row| row.get("body").is_none()),
        "list rows must be body-free: {rows:#?}"
    );
    assert!(
        rows.iter()
            .any(|row| row["status"]["status"] == json!("quarantined")),
        "quarantined row visible without enforcement: {rows:#?}"
    );

    // Identity filter narrows to that identity's scope.
    let router_rows = rpc(
        &app,
        "mobkit/memory/panel/records",
        json!({ "identity": "router" }),
    )
    .await;
    let router_rows = router_rows["result"]["records"]
        .as_array()
        .expect("router rows")
        .clone();
    assert!(!router_rows.is_empty());
    assert!(
        router_rows
            .iter()
            .all(|row| row["scope"]["identity"] == json!("router")),
        "identity filter leaked other scopes: {router_rows:#?}"
    );

    // Record detail: body + supersede chain + injection usage.
    let detail = rpc(
        &app,
        "mobkit/memory/panel/record",
        json!({ "memory_id": tip_id }),
    )
    .await;
    assert_eq!(detail["error"], Value::Null, "{detail:#?}");
    assert_eq!(detail["result"]["record"]["body"], json!("Router tip body"));
    let chain = detail["result"]["chain"].as_array().expect("chain");
    assert_eq!(chain.len(), 2, "root + tip: {chain:#?}");
    assert_eq!(chain[0]["status"]["status"], json!("superseded"));
    assert_eq!(chain[1]["id"], json!(tip_id));
    let injections = detail["result"]["injections"]
        .as_array()
        .expect("injections");
    assert_eq!(injections.len(), 1, "{injections:#?}");
    assert_eq!(injections[0]["surface"], json!("build"));

    // Quarantine queue: records plus both seeded promotions, tokenless.
    let quarantine = rpc(&app, "mobkit/memory/panel/quarantine", json!({})).await;
    assert_eq!(quarantine["error"], Value::Null, "{quarantine:#?}");
    let queue = quarantine["result"]["records"].as_array().expect("queue");
    assert!(
        queue.iter().any(|row| row["id"] == json!(quarantined_id)),
        "{queue:#?}"
    );
    let promotions = quarantine["result"]["pending_promotions"]
        .as_array()
        .expect("promotions");
    assert_eq!(promotions.len(), 2, "{promotions:#?}");
    assert!(
        promotions
            .iter()
            .all(|row| row.get("stage_token").is_none()),
        "stage_token is a commit capability and must never surface: {promotions:#?}"
    );

    // Dream history from steward audit rows.
    let dreams = rpc(&app, "mobkit/memory/panel/dreams", json!({})).await;
    assert_eq!(dreams["error"], Value::Null, "{dreams:#?}");
    let runs = dreams["result"]["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 1, "{runs:#?}");
    assert_eq!(runs[0]["run_id"], json!("run-dream-1"));
    assert_eq!(runs[0]["op_kinds"]["create"], json!(1));

    // Experience advertises the panel affordances.
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
    assert_eq!(experience["memory"]["available"], json!(true));
    assert_eq!(experience["memory"]["can_read"], json!(true));
    assert_eq!(experience["memory"]["can_review_quarantine"], json!(true));

    let _ = runtime.mob_handle().stop().await;
}

#[tokio::test]
async fn memory_panel_enforces_scope_actions_end_to_end() {
    let (_temp_dir, mut runtime) = build_access_runtime_fixture().await;
    let memory_dir = tempfile::tempdir().expect("memory dir");
    let (store, tip_id, quarantined_id, delivery_id, mob_id, operator_id) =
        seeded_memory_store(memory_dir.path()).await;
    runtime.set_memory_panel_store(store);

    // Anonymous callers: view + EXPLICIT memory read on "router" only. The
    // config mentions a memory action, so it is taken literally — no
    // compat rewrite, no mob/realm/quarantine grants.
    let controller = AccessController::new(AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        rules: vec![
            AccessRule {
                id: "view-router".to_string(),
                actions: vec!["agent.view".to_string()],
                agents: vec!["router".to_string()],
                ..AccessRule::default()
            },
            AccessRule {
                id: "read-router-memory".to_string(),
                actions: vec!["agent.memory.read".to_string()],
                agents: vec!["router".to_string()],
                ..AccessRule::default()
            },
        ],
        ..AccessControlConfig::default()
    })
    .expect("controller");
    runtime.set_access_controller(controller.clone());
    let app = runtime.build_reference_app_router(decision_state(false));

    // Experience affordances: readable, not reviewer.
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
    assert_eq!(experience["memory"]["can_read"], json!(true));
    assert_eq!(experience["memory"]["can_review_quarantine"], json!(false));

    // Unscoped listing is row-filtered to the granted identity scope; the
    // quarantined router record needs the review grant on top.
    let records = rpc(&app, "mobkit/memory/panel/records", json!({})).await;
    assert_eq!(records["error"], Value::Null, "{records:#?}");
    let rows = records["result"]["records"].as_array().expect("records");
    assert!(
        rows.iter()
            .all(|row| row["scope"]["identity"] == json!("router")),
        "only router-scope rows may survive: {rows:#?}"
    );
    assert!(
        rows.iter().all(|row| row["id"] != json!(quarantined_id)),
        "quarantined rows need the review grant: {rows:#?}"
    );

    // Identity-keyed listing for a denied identity fails the entry gate.
    let denied = rpc(
        &app,
        "mobkit/memory/panel/records",
        json!({ "identity": "delivery" }),
    )
    .await;
    assert_eq!(denied["error"]["code"], json!(-32030), "{denied:#?}");

    // Record detail enforcement is post-load, per record scope.
    let allowed = rpc(
        &app,
        "mobkit/memory/panel/record",
        json!({ "memory_id": tip_id }),
    )
    .await;
    assert_eq!(allowed["error"], Value::Null, "{allowed:#?}");
    for (memory_id, action) in [
        (&delivery_id, "agent.memory.read"),
        (&mob_id, "mob.memory.read"),
        (&operator_id, "operator.memory.read"),
        (&quarantined_id, "memory.quarantine.review"),
    ] {
        let denied = rpc(
            &app,
            "mobkit/memory/panel/record",
            json!({ "memory_id": memory_id }),
        )
        .await;
        assert_eq!(denied["error"]["code"], json!(-32030), "{denied:#?}");
        assert_eq!(
            denied["error"]["data"]["action"],
            json!(action),
            "{denied:#?}"
        );
    }

    // Quarantine queue and dream history are gated.
    let quarantine = rpc(&app, "mobkit/memory/panel/quarantine", json!({})).await;
    assert_eq!(quarantine["error"]["code"], json!(-32030));
    let dreams = rpc(&app, "mobkit/memory/panel/dreams", json!({})).await;
    assert_eq!(dreams["error"]["code"], json!(-32030));

    // §10.3 migration: recall requires read AND view. This config grants
    // both on router (explicitly), neither on delivery.
    let recall_router = rpc(
        &app,
        "mobkit/agent_memory/recall",
        json!({ "identity": "router", "selection": "always" }),
    )
    .await;
    assert_ne!(
        recall_router["error"]["code"],
        json!(-32030),
        "router recall passes the access gate: {recall_router:#?}"
    );
    let recall_delivery = rpc(
        &app,
        "mobkit/agent_memory/recall",
        json!({ "identity": "delivery", "selection": "always" }),
    )
    .await;
    assert_eq!(recall_delivery["error"]["code"], json!(-32030));

    // Capabilities intersect: the resource-less panel reads the caller can
    // never use disappear; agent-scoped ones stay (enforced per call).
    let capabilities = rpc(&app, "mobkit/capabilities", json!({})).await;
    let methods = capabilities["result"]["methods"]
        .as_array()
        .expect("methods")
        .clone();
    assert!(
        methods
            .iter()
            .any(|value| value == "mobkit/memory/panel/records")
    );
    for hidden in [
        "mobkit/memory/panel/dreams",
        "mobkit/memory/panel/quarantine",
    ] {
        assert!(
            methods.iter().all(|value| value != hidden),
            "{hidden} requires a grant this caller lacks: {methods:#?}"
        );
    }

    // Live grant of the reviewer + unscoped read opens the gated surfaces —
    // but NOT operator scope, which needs its own explicit grant.
    controller
        .upsert_rule(AccessRule {
            id: "reviewer".to_string(),
            actions: vec![
                "agent.memory.read".to_string(),
                "mob.memory.read".to_string(),
                "memory.quarantine.review".to_string(),
            ],
            ..AccessRule::default()
        })
        .expect("live reviewer grant");
    let dreams = rpc(&app, "mobkit/memory/panel/dreams", json!({})).await;
    assert_eq!(dreams["error"], Value::Null, "{dreams:#?}");
    let quarantine = rpc(&app, "mobkit/memory/panel/quarantine", json!({})).await;
    assert_eq!(quarantine["error"], Value::Null, "{quarantine:#?}");
    let queue = quarantine["result"]["records"].as_array().expect("queue");
    assert!(
        queue.iter().any(|row| row["id"] == json!(quarantined_id)),
        "{queue:#?}"
    );
    // Promotion rows ride the target scope's read grant: mob.memory.read
    // admits the mob-targeted promotion, while the delivery-identity one
    // stays hidden — the unscoped read grant lacks agent.view on delivery.
    let promotions = quarantine["result"]["pending_promotions"]
        .as_array()
        .expect("promotions");
    assert!(
        promotions
            .iter()
            .any(|row| row["pending_id"] == json!("gate-mob-promotion")),
        "{promotions:#?}"
    );
    assert!(
        promotions
            .iter()
            .all(|row| row["pending_id"] != json!("gate-delivery-promotion")),
        "identity promotions must not ride the unscoped read grant: {promotions:#?}"
    );

    // Operator-scope rows stay hidden behind operator.memory.read: absent
    // from unscoped listings, denied on detail, denied as a scope filter —
    // an unscoped agent.memory.read grant is deliberately not enough.
    let records = rpc(&app, "mobkit/memory/panel/records", json!({})).await;
    let rows = records["result"]["records"].as_array().expect("records");
    assert!(
        rows.iter().all(|row| row["id"] != json!(operator_id)),
        "operator rows must not ride the unscoped read grant: {rows:#?}"
    );
    let denied_operator = rpc(
        &app,
        "mobkit/memory/panel/record",
        json!({ "memory_id": operator_id }),
    )
    .await;
    assert_eq!(denied_operator["error"]["code"], json!(-32030));
    let denied_scope = rpc(
        &app,
        "mobkit/memory/panel/records",
        json!({ "scope": "operator" }),
    )
    .await;
    assert_eq!(denied_scope["error"]["code"], json!(-32030));

    controller
        .upsert_rule(AccessRule {
            id: "operator-reader".to_string(),
            actions: vec!["operator.memory.read".to_string()],
            ..AccessRule::default()
        })
        .expect("live operator grant");
    let operator_detail = rpc(
        &app,
        "mobkit/memory/panel/record",
        json!({ "memory_id": operator_id }),
    )
    .await;
    assert_eq!(
        operator_detail["error"],
        Value::Null,
        "{operator_detail:#?}"
    );

    let _ = runtime.mob_handle().stop().await;
}

/// §10.3 on the quarantine queue itself: `memory.quarantine.review` is the
/// entry gate but never sufficient per row — each queue record and each
/// pending promotion still needs the caller's read grant on its (target)
/// scope, so a reviewer-only principal sees an empty queue rather than
/// cross-scope titles, scope keys, and steward rationales.
#[tokio::test]
async fn memory_panel_quarantine_queue_filters_rows_per_scope() {
    let (_temp_dir, mut runtime) = build_access_runtime_fixture().await;
    let memory_dir = tempfile::tempdir().expect("memory dir");
    let (store, _tip_id, quarantined_id, _delivery_id, _mob_id, _operator_id) =
        seeded_memory_store(memory_dir.path()).await;
    runtime.set_memory_panel_store(store);

    // Enforcement on, zero rules: the anonymous caller holds no grants.
    let controller = AccessController::new(AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        rules: Vec::new(),
        ..AccessControlConfig::default()
    })
    .expect("controller");
    runtime.set_access_controller(controller.clone());
    let app = runtime.build_reference_app_router(decision_state(false));

    // Non-reviewers fail the entry gate.
    let denied = rpc(&app, "mobkit/memory/panel/quarantine", json!({})).await;
    assert_eq!(denied["error"]["code"], json!(-32030), "{denied:#?}");

    // Reviewer-only principal: past the entry gate, but with no per-scope
    // read grant every row is filtered — records AND promotions come back
    // empty instead of leaking cross-scope metadata.
    controller
        .upsert_rule(AccessRule {
            id: "reviewer".to_string(),
            actions: vec!["memory.quarantine.review".to_string()],
            ..AccessRule::default()
        })
        .expect("reviewer grant");
    let quarantine = rpc(&app, "mobkit/memory/panel/quarantine", json!({})).await;
    assert_eq!(quarantine["error"], Value::Null, "{quarantine:#?}");
    let queue = quarantine["result"]["records"].as_array().expect("queue");
    assert!(
        queue.is_empty(),
        "reviewer-only must see no queue records: {queue:#?}"
    );
    let promotions = quarantine["result"]["pending_promotions"]
        .as_array()
        .expect("promotions");
    assert!(
        promotions.is_empty(),
        "reviewer-only must see no promotions: {promotions:#?}"
    );

    // Reviewer + read/view on "router": exactly router's quarantined record
    // appears; both promotions (mob-targeted, delivery-targeted) stay hidden.
    controller
        .upsert_rule(AccessRule {
            id: "router-reader".to_string(),
            actions: vec!["agent.memory.read".to_string(), "agent.view".to_string()],
            agents: vec!["router".to_string()],
            ..AccessRule::default()
        })
        .expect("router grant");
    let quarantine = rpc(&app, "mobkit/memory/panel/quarantine", json!({})).await;
    let queue = quarantine["result"]["records"].as_array().expect("queue");
    assert_eq!(queue.len(), 1, "{queue:#?}");
    assert_eq!(queue[0]["id"], json!(quarantined_id));
    let promotions = quarantine["result"]["pending_promotions"]
        .as_array()
        .expect("promotions");
    assert!(
        promotions.is_empty(),
        "router grants must not expose mob/delivery promotions: {promotions:#?}"
    );

    // Reviewer + mob.memory.read: the mob-targeted promotion appears; the
    // delivery-identity promotion still needs read+view on "delivery".
    controller
        .upsert_rule(AccessRule {
            id: "mob-reader".to_string(),
            actions: vec!["mob.memory.read".to_string()],
            ..AccessRule::default()
        })
        .expect("mob grant");
    let quarantine = rpc(&app, "mobkit/memory/panel/quarantine", json!({})).await;
    let promotions = quarantine["result"]["pending_promotions"]
        .as_array()
        .expect("promotions");
    assert_eq!(promotions.len(), 1, "{promotions:#?}");
    assert_eq!(promotions[0]["pending_id"], json!("gate-mob-promotion"));

    // Reviewer + read/view on "delivery": its promotion joins the queue.
    controller
        .upsert_rule(AccessRule {
            id: "delivery-reader".to_string(),
            actions: vec!["agent.memory.read".to_string(), "agent.view".to_string()],
            agents: vec!["delivery".to_string()],
            ..AccessRule::default()
        })
        .expect("delivery grant");
    let quarantine = rpc(&app, "mobkit/memory/panel/quarantine", json!({})).await;
    let promotions = quarantine["result"]["pending_promotions"]
        .as_array()
        .expect("promotions");
    assert_eq!(promotions.len(), 2, "{promotions:#?}");
    assert!(
        promotions
            .iter()
            .any(|row| row["pending_id"] == json!("gate-delivery-promotion")),
        "{promotions:#?}"
    );

    let _ = runtime.mob_handle().stop().await;
}

#[tokio::test]
async fn recall_read_action_migration_compat_rule_both_ways() {
    let (_temp_dir, mut runtime) = build_access_runtime_fixture().await;

    // Memory-naive config (no memory action anywhere): agent.memory.read is
    // implicitly granted wherever agent.view is granted, so pre-migration
    // recall behavior is preserved.
    let naive = AccessController::new(AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        rules: vec![AccessRule {
            id: "view-router".to_string(),
            actions: vec!["agent.view".to_string()],
            agents: vec!["router".to_string()],
            ..AccessRule::default()
        }],
        ..AccessControlConfig::default()
    })
    .expect("naive controller");
    let (config, _) = naive.snapshot();
    assert!(
        config.rules[0]
            .actions
            .contains(&"agent.memory.read".to_string()),
        "compat rewrite materializes the read grant: {config:#?}"
    );
    runtime.set_access_controller(naive);
    let app = runtime.build_reference_app_router(decision_state(false));
    let recall_router = rpc(
        &app,
        "mobkit/agent_memory/recall",
        json!({ "identity": "router", "selection": "always" }),
    )
    .await;
    assert_ne!(
        recall_router["error"]["code"],
        json!(-32030),
        "naive config keeps recall working on view grants: {recall_router:#?}"
    );
    let recall_delivery = rpc(
        &app,
        "mobkit/agent_memory/recall",
        json!({ "identity": "delivery", "selection": "always" }),
    )
    .await;
    assert_eq!(recall_delivery["error"]["code"], json!(-32030));

    // Explicit config (mentions a memory action anywhere): taken literally.
    // View on router without a read grant now denies recall on the read
    // action.
    let explicit = AccessController::new(AccessControlConfig {
        enabled: true,
        admins: vec!["root@example.test".to_string()],
        rules: vec![
            AccessRule {
                id: "view-router".to_string(),
                actions: vec!["agent.view".to_string()],
                agents: vec!["router".to_string()],
                ..AccessRule::default()
            },
            AccessRule {
                id: "unrelated-memory-rule".to_string(),
                actions: vec!["agent.memory.read".to_string()],
                agents: vec!["someone-else".to_string()],
                ..AccessRule::default()
            },
        ],
        ..AccessControlConfig::default()
    })
    .expect("explicit controller");
    runtime.set_access_controller(explicit);
    let app = runtime.build_reference_app_router(decision_state(false));
    let denied = rpc(
        &app,
        "mobkit/agent_memory/recall",
        json!({ "identity": "router", "selection": "always" }),
    )
    .await;
    assert_eq!(denied["error"]["code"], json!(-32030), "{denied:#?}");
    assert_eq!(
        denied["error"]["data"]["action"],
        json!("agent.memory.read"),
        "explicit configs are taken literally: {denied:#?}"
    );

    let _ = runtime.mob_handle().stop().await;
}
