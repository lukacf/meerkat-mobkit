//! Issue #254 item 4: `/agents/{id}/events` must accept every public member
//! spelling. A runtime ALIAS ("rt:lead:0") encodes straight to its roster id,
//! but a DURABLE IDENTITY ("lead") has no encodable roster form — the route
//! resolves it through the roster's `agent_identity` label (the same
//! durable→roster mapping `list_members` projects). Unknown ids keep the
//! proper 404 (`member_not_found`) — never an error payload inside a 200
//! stream.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use meerkat_client::TestClient;
use meerkat_mob::{MobDefinition, SpawnMemberSpec};
use meerkat_mobkit::{
    AuthPolicy, AuthProvider, BigQueryNaming, ConsolePolicy, RuntimeDecisionInputs,
    RuntimeOpsPolicy, TrustedOidcRuntimeConfig, UnifiedRuntime, build_runtime_decision_state,
};
use tower::ServiceExt;

fn open_console_decisions() -> meerkat_mobkit::RuntimeDecisionState {
    build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "agent_events_identity_test".to_string(),
            table: "events".to_string(),
        },
        trusted_mobkit_toml: "modules = []\n".to_string(),
        auth: AuthPolicy {
            default_provider: AuthProvider::GoogleOAuth,
            email_allowlist: vec![],
        },
        trusted_oidc: TrustedOidcRuntimeConfig {
            discovery_json:
                r#"{"issuer":"https://trusted.mobkit.local","jwks_uri":"https://trusted.mobkit.local/.well-known/jwks.json"}"#
                    .to_string(),
            jwks_json: r#"{"keys":[{"kid":"kid-current","kty":"oct","alg":"HS256","k":"cGhhc2U3LXRydXN0ZWQtY3VycmVudC1zZWNyZXQ"}]}"#
                .to_string(),
            audience: "meerkat-console".to_string(),
        },
        console: ConsolePolicy {
            require_app_auth: false,
            ..ConsolePolicy::default()
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: include_str!("../assets/release-targets.json").to_string(),
    })
    .expect("decision state builds")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "upstream meerkat-mob actor defect family (S3; NOT covered by #929, \
            lead-confirmed 2026-07-31): GET /agents/<id>/events returns HTTP 500 on \
            the member-observation route. Reproduces serially at ed7e42b75. Re-arm \
            on the upstream fix SHA."]
async fn agent_events_route_resolves_durable_identities_and_aliases() {
    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "agent-events-identity-test"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true

[profiles.worker.tools]
comms = true
"#,
    )
    .expect("definition parses");
    let runtime = UnifiedRuntime::builder()
        .definition(definition)
        .default_llm_client(Arc::new(TestClient::default()))
        .build()
        .await
        .expect("runtime builds");

    // Identity-first-shaped member: runtime-alias id + durable identity label.
    let mut labels = BTreeMap::new();
    labels.insert("agent_identity".to_string(), "lead".to_string());
    runtime
        .mob_handle()
        .spawn_spec(
            SpawnMemberSpec::from_wire(
                "worker".to_string(),
                meerkat_mobkit::member_comms_id::mob_member_id_str("rt:lead:0").into_owned(),
                Some("You are the lead.".into()),
                None,
                None,
            )
            .with_labels(labels),
        )
        .await
        .expect("member spawns");

    let app = runtime.build_reference_app_router(open_console_decisions());

    for (path, expected, label) in [
        (
            "/agents/lead/events",
            StatusCode::OK,
            "durable identity must resolve via the roster label",
        ),
        (
            "/agents/rt:lead:0/events",
            StatusCode::OK,
            "runtime alias must encode directly",
        ),
        (
            "/agents/nobody-here/events",
            StatusCode::NOT_FOUND,
            "unknown ids must 404, never stream an error body",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(path)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), expected, "{label}: GET {path}");
    }

    let _ = runtime.mob_handle().stop().await;
}
