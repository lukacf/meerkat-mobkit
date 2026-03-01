use std::time::Duration;

use meerkat_mobkit_core::{
    build_runtime_decision_state, handle_console_rest_json_route, handle_mobkit_rpc_json,
    start_mobkit_runtime, AuthPolicy, AuthProvider, BigQueryNaming, ConsoleAccessRequest,
    ConsolePolicy, ConsoleRestJsonRequest, DiscoverySpec, MobKitConfig, ModuleConfig, PreSpawnData,
    RestartPolicy, RuntimeDecisionInputs, RuntimeOpsPolicy, TrustedOidcRuntimeConfig,
};
use serde_json::{json, Value};

fn shell_module(id: &str, script: &str) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        restart_policy: RestartPolicy::Never,
    }
}

fn runtime_with_router_and_delivery() -> meerkat_mobkit_core::MobkitRuntimeHandle {
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
    include_str!("../../../docs/rct/release-targets.json").to_string()
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

fn decision_state(require_app_auth: bool) -> meerkat_mobkit_core::RuntimeDecisionState {
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
    let allowed = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/experience".to_string(),
            auth: Some(ConsoleAccessRequest {
                provider: AuthProvider::GoogleOAuth,
                email: "alice@example.com".to_string(),
            }),
        },
    );

    assert_eq!(allowed.status, 200);
    assert_eq!(allowed.body["contract_version"], json!("0.1.0"));
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
    assert!(subscribed["result"]["event_frames"][0]
        .as_str()
        .expect("event frame string")
        .starts_with("id: evt-router\nevent: response\ndata: {"));
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
