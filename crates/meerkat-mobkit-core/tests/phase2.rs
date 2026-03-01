use meerkat_mobkit_core::{
    build_runtime_decision_state, enforce_console_route_access, handle_console_rest_json_route,
    AuthPolicy, AuthProvider, BigQueryNaming, ConsoleAccessRequest, ConsolePolicy,
    ConsoleRestJsonRequest, DecisionPolicyError, DecisionRuntimeError, RuntimeDecisionInputs,
    RuntimeOpsPolicy, TrustedOidcRuntimeConfig,
};

fn trusted_toml() -> String {
    r#"
[[modules]]
id = "scheduler"
command = "scheduler-bin"
args = ["--poll-ms", "250"]
restart_policy = "on_failure"

[[modules]]
id = "router"
command = "router-bin"
args = ["--mode", "fast"]
restart_policy = "always"
"#
    .to_string()
}

fn release_json() -> String {
    include_str!("../../../docs/rct/release-targets.json").to_string()
}

fn trusted_oidc() -> TrustedOidcRuntimeConfig {
    TrustedOidcRuntimeConfig {
        discovery_json:
            r#"{"issuer":"https://trusted.mobkit.local","jwks_uri":"https://trusted.mobkit.local/.well-known/jwks.json"}"#
                .to_string(),
        jwks_json: r#"{"keys":[{"kid":"kid-current","kty":"oct","alg":"HS256","k":"cGhhc2U3LXRydXN0ZWQtY3VycmVudC1zZWNyZXQ"},{"kid":"kid-next","kty":"oct","alg":"HS256","k":"cGhhc2U3LXRydXN0ZWQtbmV4dC1zZWNyZXQ"}]}"#.to_string(),
        audience: "meerkat-console".to_string(),
    }
}

#[test]
fn dec_001_dec_003_dec_005_dec_006_dec_007_runtime_decision_state_wiring() {
    let state = build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "custom_dataset_v1".to_string(),
            table: "events_2026".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy {
            default_provider: AuthProvider::GoogleOAuth,
            email_allowlist: vec!["alice@example.com".to_string()],
        },
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy::default(),
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: release_json(),
    })
    .expect("runtime decision wiring should validate");

    assert_eq!(state.bigquery.dataset, "custom_dataset_v1");
    assert_eq!(state.modules.len(), 2);
    assert_eq!(state.ops.replica_count, 1);
    assert!(!state.ops.metrics.enforce_slo_targets);
    assert_eq!(state.release_metadata.support_matrix, "same-as-meerkat");
}

#[test]
fn dec_004_console_rest_route_uses_auth_middleware_policy() {
    let state = build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "ds".to_string(),
            table: "tb".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy {
            default_provider: AuthProvider::GoogleOAuth,
            email_allowlist: vec!["alice@example.com".to_string()],
        },
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy {
            require_app_auth: true,
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: release_json(),
    })
    .expect("state build");

    let authorized = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/modules".to_string(),
            auth: Some(ConsoleAccessRequest {
                provider: AuthProvider::GoogleOAuth,
                email: "alice@example.com".to_string(),
            }),
        },
    );
    assert_eq!(authorized.status, 200);
    assert_eq!(
        authorized.body["modules"].as_array().map(|v| v.len()),
        Some(2)
    );

    let unauthorized = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/modules".to_string(),
            auth: Some(ConsoleAccessRequest {
                provider: AuthProvider::GoogleOAuth,
                email: "mallory@example.com".to_string(),
            }),
        },
    );
    assert_eq!(unauthorized.status, 401);
}

#[test]
fn dec_002_auth_provider_mismatch_and_bypass_branches() {
    let auth = AuthPolicy {
        default_provider: AuthProvider::GoogleOAuth,
        email_allowlist: vec!["alice@example.com".to_string()],
    };

    let mismatch = enforce_console_route_access(
        &auth,
        &ConsolePolicy::default(),
        &ConsoleAccessRequest {
            provider: AuthProvider::TestProvider,
            email: "alice@example.com".to_string(),
        },
    )
    .expect_err("provider mismatch should reject");
    assert_eq!(mismatch, DecisionPolicyError::AuthProviderMismatch);

    let bypass = enforce_console_route_access(
        &auth,
        &ConsolePolicy {
            require_app_auth: false,
        },
        &ConsoleAccessRequest {
            provider: AuthProvider::GoogleOAuth,
            email: "nobody@example.com".to_string(),
        },
    );
    assert!(bypass.is_ok());
}

#[test]
fn dec_004_console_route_bypass_when_app_auth_disabled() {
    let state = build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "ds".to_string(),
            table: "tb".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy::default(),
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy {
            require_app_auth: false,
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: release_json(),
    })
    .expect("state build");

    let response = handle_console_rest_json_route(
        &state,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path: "/console/modules".to_string(),
            auth: None,
        },
    );
    assert_eq!(response.status, 200);
}

#[test]
fn dec_002_auth_provider_mismatch_branch() {
    let mismatch = enforce_console_route_access(
        &AuthPolicy {
            default_provider: AuthProvider::GoogleOAuth,
            email_allowlist: vec![],
        },
        &ConsolePolicy::default(),
        &ConsoleAccessRequest {
            provider: AuthProvider::GoogleOAuth,
            email: "unknown@example.com".to_string(),
        },
    )
    .expect_err("allowlist branch should reject");
    assert_eq!(mismatch, DecisionPolicyError::EmailNotAllowlisted);
}

#[test]
fn dec_006_reject_slo_enforcement_in_v01() {
    let err = build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "ds".to_string(),
            table: "tb".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy::default(),
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy::default(),
        ops: RuntimeOpsPolicy {
            replica_count: 1,
            metrics: meerkat_mobkit_core::MetricsPolicy {
                enforce_slo_targets: true,
            },
        },
        release_metadata_json: release_json(),
    })
    .expect_err("slo enforcement should be rejected in v0.1");

    assert_eq!(
        err,
        DecisionRuntimeError::Policy(DecisionPolicyError::SloTargetsNotSupportedV01)
    );
}

#[test]
fn dec_007_invalid_metadata_branches() {
    let duplicate = r#"{"targets":["crates.io","crates.io","npm","pypi","github-releases"],"support_matrix":"same-as-meerkat"}"#;
    let err = build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "ds".to_string(),
            table: "tb".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy::default(),
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy::default(),
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: duplicate.to_string(),
    })
    .expect_err("duplicate should fail");
    assert_eq!(
        err,
        DecisionRuntimeError::Policy(DecisionPolicyError::DuplicateReleaseTarget(
            "crates.io".to_string()
        ))
    );

    let missing = r#"{"targets":["crates.io","npm","pypi"],"support_matrix":"same-as-meerkat"}"#;
    let err = build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "ds".to_string(),
            table: "tb".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy::default(),
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy::default(),
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: missing.to_string(),
    })
    .expect_err("missing should fail");
    assert_eq!(
        err,
        DecisionRuntimeError::Policy(DecisionPolicyError::MissingReleaseTarget(
            "github-releases".to_string()
        ))
    );

    let invalid_matrix =
        r#"{"targets":["crates.io","npm","pypi","github-releases"],"support_matrix":"custom"}"#;
    let err = build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "ds".to_string(),
            table: "tb".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy::default(),
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy::default(),
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: invalid_matrix.to_string(),
    })
    .expect_err("invalid matrix should fail");
    assert_eq!(
        err,
        DecisionRuntimeError::Policy(DecisionPolicyError::InvalidSupportMatrix(
            "custom".to_string()
        ))
    );
}
