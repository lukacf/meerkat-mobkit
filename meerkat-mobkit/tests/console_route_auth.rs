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
use meerkat_mobkit::{
    AuthPolicy, AuthProvider, BigQueryNaming, ConsolePolicy, ConsoleRestJsonRequest,
    RuntimeDecisionInputs, RuntimeOpsPolicy, TrustedOidcRuntimeConfig,
    build_runtime_decision_state, handle_console_rest_json_route,
};
use serde_json::{Value, json};

fn release_json() -> String {
    include_str!("../assets/release-targets.json").to_string()
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

fn decision_state() -> meerkat_mobkit::RuntimeDecisionState {
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
            ..ConsolePolicy::default()
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: release_json(),
    })
    .expect("decision state builds")
}

async fn voice_rpc(
    state: meerkat_mobkit::RuntimeDecisionState,
    method: &str,
    authenticated: bool,
) -> (axum::http::StatusCode, Value) {
    voice_rpc_with_params(
        state,
        method,
        authenticated,
        json!({"identity":"agent-a", "request_id":"request-a"}),
        None,
    )
    .await
}

async fn voice_rpc_with_params(
    mut state: meerkat_mobkit::RuntimeDecisionState,
    method: &str,
    authenticated: bool,
    params: Value,
    access: Option<meerkat_mobkit::AccessController>,
) -> (axum::http::StatusCode, Value) {
    use tower::ServiceExt;
    state.trusted_oidc.discovery_json = json!({
        "issuer": "https://trusted.mobkit.localhost",
        "jwks_uri": "https://trusted.mobkit.localhost/.well-known/jwks.json"
    })
    .to_string();
    let app = meerkat_mobkit::console_json_router_with_aggregator_and_access(
        state,
        meerkat_mobkit::MobKitConsoleAggregator::new(std::sync::Arc::new(
            meerkat_mobkit::InMemoryConsoleLogStore::default(),
        )),
        access,
    )
    .layer(axum::Extension(
        meerkat_mobkit::console_voice::ConsoleVoiceController::default(),
    ));
    let mut request = axum::http::Request::builder()
        .method("POST")
        .uri("/console/rpc")
        .header("content-type", "application/json");
    if authenticated {
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        header.kid = Some("kid-current".to_string());
        let token = jsonwebtoken::encode(
            &header,
            &json!({
                "iss": "https://trusted.mobkit.localhost",
                "aud": "meerkat-console",
                "sub": "alice@example.com",
                "email": "alice@example.com",
                "provider": "google_oauth",
                "exp": chrono::Utc::now().timestamp() + 300
            }),
            &jsonwebtoken::EncodingKey::from_secret(b"phase8-trusted-current-secret"),
        )
        .expect("token");
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .oneshot(
            request
                .body(axum::body::Body::from(
                    json!({
                        "jsonrpc": "2.0", "id": 1, "method": method,
                        "params": params
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    (status, serde_json::from_slice(&body).expect("json"))
}

#[tokio::test]
async fn console_voice_readiness_is_target_scoped_and_false_without_a_host() {
    let contract: Value = serde_json::from_str(include_str!("fixtures/console_voice_v1.json"))
        .expect("shared voice contract");
    let mut state = decision_state();
    state.console.require_app_auth = true;
    let (_, readiness) = voice_rpc_with_params(
        state.clone(),
        "mobkit/console/voice/readiness",
        true,
        json!({"identity":"agent-a"}),
        None,
    )
    .await;
    assert_eq!(readiness["result"], contract["readiness_unavailable"]);
    let (_, malformed) = voice_rpc_with_params(
        state,
        "mobkit/console/voice/readiness",
        true,
        json!({"identity":"agent-a", "request_id":"unexpected"}),
        None,
    )
    .await;
    assert_eq!(malformed["error"]["code"], -32602);
}

#[tokio::test]
async fn console_voice_channel_activation_rechecks_agent_permissions_before_dispatch() {
    let mut state = decision_state();
    state.console.require_app_auth = true;
    let access = meerkat_mobkit::AccessController::new(meerkat_mobkit::AccessControlConfig {
        enabled: true,
        admins: vec!["someone-else@example.com".to_string()],
        ..Default::default()
    })
    .expect("access controller");
    for method in [
        "mobkit/console/voice/readiness",
        "mobkit/console/voice/replacement",
        "mobkit/console/voice/activity",
        "mobkit/live/playback_owner/register",
        "live/webrtc/answer",
        "mobkit/console/voice/answer_received",
    ] {
        let (_, response) = voice_rpc_with_params(
            state.clone(),
            method,
            true,
            json!({"identity":"agent-a", "request_id":"request-a", "channel_id":"channel-a"}),
            Some(access.clone()),
        )
        .await;
        assert_eq!(
            response["error"]["data"]["kind"], "access_denied",
            "{method}: {response}"
        );
    }
    let (_, closed) = voice_rpc_with_params(
        state,
        "mobkit/console/voice/close",
        true,
        json!({"identity":"agent-a", "request_id":"request-a"}),
        Some(access),
    )
    .await;
    assert_eq!(closed["result"]["phase"], "closed");
}

#[tokio::test]
async fn console_voice_http_never_promotes_anonymous_console_access_to_live_authority() {
    for method in [
        "mobkit/console/voice/open",
        "mobkit/console/voice/readiness",
        "mobkit/console/voice/close",
    ] {
        let (status, response) = voice_rpc(decision_state(), method, false).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(response["error"]["data"]["kind"], "access_denied");
    }
}

#[tokio::test]
async fn console_voice_does_not_expose_measured_output_transport() {
    let mut state = decision_state();
    state.console.require_app_auth = true;
    for method in [
        "mobkit/console/voice/outputs/poll",
        "mobkit/console/voice/outputs/ack",
    ] {
        let (_, response) = voice_rpc(state.clone(), method, true).await;
        assert_eq!(response["error"]["code"], -32601);
    }
}

#[tokio::test]
async fn console_voice_http_requires_real_console_auth_and_a_composed_voice_host() {
    let mut state = decision_state();
    state.console.require_app_auth = true;
    let (status, _) = voice_rpc(state.clone(), "mobkit/console/voice/open", false).await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    let (status, response) = voice_rpc(state, "mobkit/console/voice/open", true).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(response["error"]["data"]["kind"], "voice_unavailable");
}

#[tokio::test]
async fn console_voice_http_read_only_blocks_open_but_not_owned_cancel_fencing() {
    let mut state = decision_state();
    state.console.require_app_auth = true;
    state.console.read_only = true;
    let (_, open) = voice_rpc(state.clone(), "mobkit/console/voice/open", true).await;
    assert_eq!(open["error"]["data"]["kind"], "read_only");
    let (_, close) = voice_rpc(state, "mobkit/console/voice/close", true).await;
    assert_eq!(close["result"], json!({"phase":"closed"}));
}

#[test]
fn phase0_contract_004_console_rest_sse_contract_version_is_pinned_and_enforced() {
    let artifact: Value = serde_json::from_str(include_str!(
        "../../docs/rct/console-rest-sse-contract-v0.5.0.json"
    ))
    .expect("contract artifact json should parse");

    assert_eq!(artifact["contract_version"], json!("0.5.0"));
    assert_eq!(artifact["version_pin"], json!("v0.5.0"));

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

    let send_error_codes = artifact["surfaces"]["rpc"]["methods"]["mobkit/console/send"]["errors"]
        [0]["codes"]
        .as_array()
        .expect("send error codes must be present");
    assert!(
        !send_error_codes.contains(&json!(
            meerkat_mobkit::CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE
        )),
        "send must not advertise timeline replay errors"
    );
    let query_timeline_error_codes = artifact["surfaces"]["rpc"]["methods"]
        ["mobkit/console/query_timeline"]["errors"][0]["codes"]
        .as_array()
        .expect("query_timeline error codes must be present");
    assert!(
        query_timeline_error_codes.contains(&json!(
            meerkat_mobkit::CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE
        )),
        "query_timeline must advertise timeline replay errors"
    );

    let timeline_path = artifact["surfaces"]["sse"]["timeline"]["path"]
        .as_str()
        .expect("timeline path must be present");
    let keep_alive_event = artifact["surfaces"]["sse"]["activity_feed_keep_alive_event"]
        .as_str()
        .expect("keep-alive event must be present");

    assert_eq!(
        experience_response.body["chat_inspector"]["observe_route"],
        json!(timeline_path)
    );
    assert_eq!(
        experience_response.body["activity_feed"]["source_route"],
        json!(timeline_path)
    );
    assert_eq!(
        experience_response.body["activity_feed"]["keep_alive"]["event"],
        json!(keep_alive_event)
    );
}
