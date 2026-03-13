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
use meerkat_mobkit_core::{
    AuthPolicy, AuthProvider, BigQueryNaming, ConsolePolicy, ConsoleRestJsonRequest,
    RuntimeDecisionInputs, RuntimeOpsPolicy, TrustedOidcRuntimeConfig,
    build_runtime_decision_state, handle_console_rest_json_route,
};
use serde_json::{Value, json};

fn release_json() -> String {
    include_str!("../../docs/rct/release-targets.json").to_string()
}

fn trusted_oidc() -> TrustedOidcRuntimeConfig {
    TrustedOidcRuntimeConfig {
        discovery_json:
            r#"{"issuer":"https://trusted.mobkit.local","jwks_uri":"https://trusted.mobkit.local/.well-known/jwks.json"}"#
                .to_string(),
        jwks_json: r#"{"keys":[{"kid":"kid-current","kty":"oct","alg":"HS256","k":"cGhhc2U4LXRydXN0ZWQtY3VycmVudC1zZWNyZXQ"}]}"#
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

fn decision_state() -> meerkat_mobkit_core::RuntimeDecisionState {
    build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "phase0_contract_dataset".to_string(),
            table: "phase0_contract_table".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy {
            default_provider: AuthProvider::GoogleOAuth,
            email_allowlist: vec!["alice@example.com".to_string()],
        },
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy {
            require_app_auth: false,
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: release_json(),
    })
    .expect("decision state builds")
}

#[test]
fn phase0_contract_004_console_rest_sse_contract_version_is_pinned_and_enforced() {
    let artifact: Value = serde_json::from_str(include_str!(
        "../../docs/rct/console-rest-sse-contract-v0.1.0.json"
    ))
    .expect("contract artifact json should parse");

    assert_eq!(artifact["contract_version"], json!("0.1.0"));
    assert_eq!(artifact["version_pin"], json!("v0.1.0"));

    let state = decision_state();

    let experience_method = artifact["surfaces"]["rest"]["experience"]["method"]
        .as_str()
        .expect("experience method must be present");
    let experience_path = artifact["surfaces"]["rest"]["experience"]["path"]
        .as_str()
        .expect("experience path must be present");
    let experience_response = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: experience_method.to_string(),
            path: experience_path.to_string(),
            auth: None,
        },
    );

    assert_eq!(experience_response.status, 200);
    assert_eq!(
        experience_response.body["contract_version"],
        artifact["contract_version"]
    );
    assert_eq!(
        experience_response.body["base_panel"]["route"],
        json!(experience_path)
    );

    for field in artifact["surfaces"]["rest"]["experience"]["response"]["required_top_level_fields"]
        .as_array()
        .expect("experience required field list must be present")
    {
        let field_name = field
            .as_str()
            .expect("experience required field must be string");
        assert!(
            experience_response.body.get(field_name).is_some(),
            "experience response missing required field: {field_name}"
        );
    }

    let modules_method = artifact["surfaces"]["rest"]["modules"]["method"]
        .as_str()
        .expect("modules method must be present");
    let modules_path = artifact["surfaces"]["rest"]["modules"]["path"]
        .as_str()
        .expect("modules path must be present");
    let modules_response = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: modules_method.to_string(),
            path: modules_path.to_string(),
            auth: None,
        },
    );

    assert_eq!(modules_response.status, 200);
    for field in artifact["surfaces"]["rest"]["modules"]["response"]["required_top_level_fields"]
        .as_array()
        .expect("modules required field list must be present")
    {
        let field_name = field
            .as_str()
            .expect("modules required field must be string");
        assert!(
            modules_response.body.get(field_name).is_some(),
            "modules response missing required field: {field_name}"
        );
    }
    assert!(modules_response.body["modules"].is_array());

    let agent_events_path = artifact["surfaces"]["sse"]["agent_events"]["path"]
        .as_str()
        .expect("agent events path must be present");
    let keep_alive_event = artifact["surfaces"]["sse"]["activity_feed_keep_alive_event"]
        .as_str()
        .expect("keep-alive event must be present");

    assert_eq!(
        experience_response.body["chat_inspector"]["observe_route"],
        json!(agent_events_path)
    );
    assert_eq!(
        experience_response.body["activity_feed"]["keep_alive"]["event"],
        json!(keep_alive_event)
    );
}
