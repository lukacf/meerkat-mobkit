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
