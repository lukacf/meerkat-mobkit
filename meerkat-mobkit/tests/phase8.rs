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
use std::time::Duration;

use meerkat_mobkit::{
    AuthPolicy, AuthProvider, BigQueryNaming, ConsoleAccessRequest, ConsoleLiveSnapshot,
    ConsolePolicy, ConsoleRestJsonRequest, DiscoverySpec, MobKitConfig, ModuleConfig, PreSpawnData,
    RestartPolicy, RuntimeDecisionInputs, RuntimeOpsPolicy, TrustedOidcRuntimeConfig,
    build_runtime_decision_state, handle_console_rest_json_route,
    handle_console_rest_json_route_with_snapshot, handle_mobkit_rpc_json, start_mobkit_runtime,
};
use serde_json::{Value, json};

fn shell_module(id: &str, script: &str) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        restart_policy: RestartPolicy::Never,
    }
}

fn runtime_with_router_and_delivery() -> meerkat_mobkit::MobkitRuntimeHandle {
    let config = MobKitConfig {
        modules: vec![
            shell_module(
                "router",
                r#"printf '%s\n' '{"event_id":"evt-router","source":"module","timestamp_ms":10,"event":{"kind":"module","module":"router","event_type":"response","payload":{"via":"router","ok":true}}}'"#,
            ),
            shell_module(
                "delivery",
                r#"printf '%s\n' '{"event_id":"evt-delivery","source":"module","timestamp_ms":20,"event":{"kind":"module","module":"delivery","event_type":"ready","payload":{"sink":"memory"}}}'"#,
            ),
        ],
        discovery: DiscoverySpec {
            namespace: "phase8".to_string(),
            modules: vec!["router".to_string()],
        },
        pre_spawn: vec![
            PreSpawnData {
                module_id: "router".to_string(),
                env: vec![],
            },
            PreSpawnData {
                module_id: "delivery".to_string(),
                env: vec![],
            },
        ],
    };

    start_mobkit_runtime(config, vec![], Duration::from_secs(1)).expect("runtime starts")
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

fn decision_state(require_app_auth: bool) -> meerkat_mobkit::RuntimeDecisionState {
    build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "phase8_dataset".to_string(),
            table: "phase8_table".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy {
            default_provider: AuthProvider::GoogleOAuth,
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

fn parse_response(line: &str) -> Value {
    serde_json::from_str(line).expect("valid rpc response json")
}

#[test]
fn phase8_console_001_capability_driven_rendering_contract() {
    let state = decision_state(true);
    let authorized_auth = ConsoleAccessRequest {
        provider: AuthProvider::GoogleOAuth,
        email: "alice@example.com".to_string(),
    };
    let allowed = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/experience".to_string(),
            auth: Some(authorized_auth.clone()),
        },
    );
    let modules_response = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/modules".to_string(),
            auth: Some(authorized_auth),
        },
    );

    assert_eq!(allowed.status, 200);
    assert_eq!(modules_response.status, 200);
    assert_eq!(allowed.body["contract_version"], json!("0.2.0"));
    assert_eq!(
        allowed.body["base_panel"]["panel_id"],
        json!("console.home")
    );
    assert_eq!(
        allowed.body["base_panel"]["route"],
        json!("/console/experience")
    );
    assert_eq!(
        allowed.body["module_panels"],
        json!([
            {
                "panel_id":"module.router",
                "module_id":"router",
                "title":"router module",
                "route":"/console/modules/router",
                "capabilities":{"can_render":true,"can_subscribe_activity":true}
            },
            {
                "panel_id":"module.delivery",
                "module_id":"delivery",
                "title":"delivery module",
                "route":"/console/modules/delivery",
                "capabilities":{"can_render":true,"can_subscribe_activity":true}
            }
        ])
    );
    assert_eq!(
        allowed.body["agent_sidebar"]["panel_id"],
        json!("console.agent_sidebar")
    );
    assert_eq!(
        allowed.body["agent_sidebar"]["source_method"],
        json!("mobkit/status")
    );
    assert_eq!(
        allowed.body["agent_sidebar"]["selection_contract"]["supported_scopes"],
        json!(["mob", "agent"])
    );
    assert_eq!(
        allowed.body["agent_sidebar"]["selection_contract"]["selected_member_id_field"],
        json!("member_id")
    );
    assert_eq!(
        allowed.body["agent_sidebar"]["list_item_contract"]["member_id_field"],
        json!("member_id")
    );
    assert_eq!(
        allowed.body["agent_sidebar"]["live_snapshot"]["agents"],
        json!([
            {
                "agent_id":"router",
                "member_id":"router",
                "label":"router",
                "kind":"module_agent"
            },
            {
                "agent_id":"delivery",
                "member_id":"delivery",
                "label":"delivery",
                "kind":"module_agent"
            }
        ])
    );
    assert_eq!(
        allowed.body["chat_inspector"]["panel_id"],
        json!("console.chat_inspector")
    );
    assert_eq!(
        allowed.body["chat_inspector"]["send_method"],
        json!("mobkit/send_message")
    );
    assert_eq!(
        allowed.body["chat_inspector"]["observe_route"],
        json!("/interactions/stream")
    );
    assert_eq!(
        allowed.body["topology"]["panel_id"],
        json!("console.topology")
    );
    assert_eq!(
        allowed.body["topology"]["source_method"],
        json!("mobkit/status")
    );
    assert!(allowed.body["topology"]["live_snapshot"].is_object());
    assert_eq!(
        allowed.body["topology"]["live_snapshot"]["nodes"],
        modules_response.body["modules"]
    );
    assert_eq!(
        allowed.body["topology"]["live_snapshot"]["node_count"],
        json!(
            modules_response.body["modules"]
                .as_array()
                .expect("modules array")
                .len()
        )
    );
    assert_eq!(
        allowed.body["health_overview"]["panel_id"],
        json!("console.health_overview")
    );
    assert_eq!(
        allowed.body["health_overview"]["source_method"],
        json!("mobkit/status")
    );
    assert!(allowed.body["health_overview"]["live_snapshot"].is_object());
    assert_eq!(
        allowed.body["health_overview"]["live_snapshot"]["loaded_modules"],
        modules_response.body["modules"]
    );
    assert_eq!(
        allowed.body["health_overview"]["live_snapshot"]["loaded_module_count"],
        json!(
            modules_response.body["modules"]
                .as_array()
                .expect("modules array")
                .len()
        )
    );
}

#[test]
fn phase8_console_live_snapshot_prefers_runtime_state_over_config_modules() {
    let state = decision_state(false);
    let runtime_snapshot =
        ConsoleLiveSnapshot::new(false, vec!["router".to_string()], Vec::new(), false);
    let response = handle_console_rest_json_route_with_snapshot(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/experience".to_string(),
            auth: None,
        },
        Some(&runtime_snapshot),
    );

    assert_eq!(response.status, 200);
    assert_eq!(
        response.body["module_panels"]
            .as_array()
            .expect("module_panels array")
            .len(),
        2
    );
    assert_eq!(
        response.body["topology"]["live_snapshot"]["nodes"],
        json!(["router"])
    );
    assert_eq!(
        response.body["health_overview"]["live_snapshot"]["loaded_modules"],
        json!(["router"])
    );
    assert_eq!(
        response.body["health_overview"]["live_snapshot"]["loaded_module_count"],
        json!(1)
    );
    assert_eq!(
        response.body["health_overview"]["live_snapshot"]["running"],
        json!(false)
    );
}

#[test]
fn phase8_req_003_choke_104_unified_activity_feed_contract_over_events() {
    let state = decision_state(false);
    let experience = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/experience".to_string(),
            auth: None,
        },
    );
    let mut runtime = runtime_with_router_and_delivery();
    let subscribed = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase8-events","method":"mobkit/events/subscribe","params":{"scope":"mob"}}"#,
        Duration::from_secs(1),
    ));
    runtime.shutdown();

    assert_eq!(
        experience.body["activity_feed"]["source_method"],
        json!("mobkit/events/subscribe")
    );
    assert_eq!(
        experience.body["activity_feed"]["supported_scopes"],
        json!(["mob", "agent", "interaction"])
    );
    assert_eq!(
        experience.body["activity_feed"]["panel_id"],
        json!("console.activity_feed")
    );
    assert_eq!(
        experience.body["activity_feed"]["default_scope"],
        json!("mob")
    );
    assert_eq!(
        experience.body["activity_feed"]["keep_alive"]["interval_ms"],
        subscribed["result"]["keep_alive"]["interval_ms"]
    );
    assert_eq!(
        experience.body["activity_feed"]["keep_alive"]["event"],
        subscribed["result"]["keep_alive"]["event"]
    );
    assert_eq!(
        experience.body["activity_feed"]["keep_alive"]["comment_frame"],
        subscribed["result"]["keep_alive_comment"]
    );
    assert_eq!(
        subscribed["result"]["events"][0]["event_id"],
        json!("evt-router")
    );
    assert_eq!(
        subscribed["result"]["events"][0]["event"]["event_type"],
        json!("response")
    );
    assert!(
        subscribed["result"]["event_frames"][0]
            .as_str()
            .expect("event frame string")
            .starts_with("id: evt-router\nevent: response\ndata: {")
    );
}

#[test]
fn phase8_console_002_auth_protected_access_remains_enforced() {
    let state = decision_state(true);

    let missing = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/experience".to_string(),
            auth: None,
        },
    );
    let denied_provider = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/experience".to_string(),
            auth: Some(ConsoleAccessRequest {
                provider: AuthProvider::GitHubOAuth,
                email: "alice@example.com".to_string(),
            }),
        },
    );
    let allowed = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/experience".to_string(),
            auth: Some(ConsoleAccessRequest {
                provider: AuthProvider::ServiceIdentity,
                email: "svc:deploy-bot".to_string(),
            }),
        },
    );

    assert_eq!(missing.status, 401);
    assert_eq!(
        missing.body,
        json!({"error":"unauthorized","reason":"missing_credentials"})
    );
    assert_eq!(denied_provider.status, 401);
    assert_eq!(
        denied_provider.body,
        json!({"error":"unauthorized","reason":"provider_mismatch"})
    );
    assert_eq!(allowed.status, 200);
}
