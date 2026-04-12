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
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use futures::StreamExt;
use meerkat::{AgentFactory, Config, build_ephemeral_service};
use meerkat_client::TestClient;
use meerkat_mob::{MobDefinition, MobStorage, ProfileName};
use meerkat_mobkit::identity_first::contracts::RosterProvider;
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentBuildDraft, AgentIdentity, AgentRuntimeId, BridgeError,
    DurabilityPolicy, DurableAgentSpec, IdentityRuntime, IdentityRuntimeConfig,
    LocalContinuityStore, LocalLeaseProvider, MemberInspection, RosterContext, RosterError,
    SessionBridge, SessionSnapshot, restore_flow,
};
use meerkat_mobkit::{
    AuthPolicy, AuthProvider, BigQueryNaming, ConsoleInteractionAccepted, ConsolePolicy,
    DiscoverySpec, IdentityFirstContext, JsonRpcResponse, MobBootstrapOptions, MobBootstrapSpec,
    MobKitConfig, ReplayUnavailableError, RuntimeDecisionInputs, RuntimeOpsPolicy,
    TrustedOidcRuntimeConfig, UnifiedRuntime, build_runtime_decision_state, console_json_router,
    console_json_router_with_runtime, handle_unified_rpc_json,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

struct Fixture {
    _temp_dir: TempDir,
    runtime: UnifiedRuntime,
}

struct StaticRosterProvider {
    specs: Vec<DurableAgentSpec>,
}

struct MockSessionBridge;

#[async_trait]
impl RosterProvider for StaticRosterProvider {
    async fn roster(&self, _context: &RosterContext) -> Result<Vec<DurableAgentSpec>, RosterError> {
        Ok(self.specs.clone())
    }
}

#[async_trait]
impl SessionBridge for MockSessionBridge {
    async fn create_session(
        &self,
        _identity: &AgentIdentity,
        _runtime_id: &AgentRuntimeId,
        _spec: &DurableAgentSpec,
        _draft: &AgentBuildDraft,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        Ok(meerkat_core::types::SessionId::new())
    }

    async fn resume_session(
        &self,
        _identity: &AgentIdentity,
        _runtime_id: &AgentRuntimeId,
        _spec: &DurableAgentSpec,
        _draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
        _snapshot: &SessionSnapshot,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        Ok(session_id.clone())
    }

    async fn deliver(
        &self,
        _runtime_id: &AgentRuntimeId,
        _content: &meerkat_core::ContentInput,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        Ok(meerkat_core::types::SessionId::new())
    }

    async fn checkpoint_session(
        &self,
        _runtime_id: &AgentRuntimeId,
        _session_id: &meerkat_core::types::SessionId,
    ) -> Result<SessionSnapshot, BridgeError> {
        Ok(SessionSnapshot { data: Vec::new() })
    }

    async fn retire_member(&self, _runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
        Ok(())
    }

    async fn inspect_member(
        &self,
        _runtime_id: &AgentRuntimeId,
    ) -> Result<MemberInspection, BridgeError> {
        Ok(MemberInspection {
            output_preview: Some("phase0 inspect preview".to_string()),
            is_final: false,
            peer_reachable_count: 1,
        })
    }
}

fn release_json() -> String {
    include_str!("../../docs/rct/release-targets.json").to_string()
}

fn trusted_toml() -> String {
    r#"
[[modules]]
id = "routing"
command = "router-bin"
args = ["--mode", "fast"]
restart_policy = "always"
"#
    .to_string()
}

fn trusted_oidc() -> TrustedOidcRuntimeConfig {
    TrustedOidcRuntimeConfig {
        discovery_json:
            r#"{"issuer":"https://trusted.mobkit.local","jwks_uri":"https://trusted.mobkit.local/.well-known/jwks.json"}"#
                .to_string(),
        jwks_json: r#"{"keys":[{"kid":"kid-current","kty":"oct","alg":"HS256","k":"cGhhc2UwLWNvbnNvbGUtc3RyZWFtLXNlY3JldA"}]}"#
            .to_string(),
        audience: "meerkat-console".to_string(),
    }
}

fn decision_state(require_app_auth: bool) -> meerkat_mobkit::RuntimeDecisionState {
    build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "phase0_console_dataset".to_string(),
            table: "phase0_console_table".to_string(),
        },
        trusted_mobkit_toml: trusted_toml(),
        auth: AuthPolicy {
            default_provider: AuthProvider::GoogleOAuth,
            email_allowlist: vec!["alice@example.com".to_string()],
        },
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy { require_app_auth },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: release_json(),
    })
    .expect("decision state builds")
}

async fn build_unified_runtime() -> Fixture {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));

    let mut definition = MobDefinition::from_toml(
        r#"
[mob]
id = "test-mob"

[profiles.lead]
model = "gpt-5.2"
external_addressable = true

[profiles.lead.tools]
comms = true
"#,
    )
    .expect("parse mob definition");
    for binding in definition.profiles.values_mut() {
        if let Some(profile) = binding.as_inline_mut() {
            profile.model = "gpt-5.2".to_string();
        }
    }

    let mob_spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(TestClient::default())),
        });
    let module_config = MobKitConfig {
        modules: vec![],
        discovery: DiscoverySpec {
            namespace: "phase0-console-runtime".to_string(),
            modules: vec![],
        },
        pre_spawn: vec![],
    };

    let runtime = UnifiedRuntime::bootstrap(mob_spec, module_config, Duration::from_secs(2))
        .await
        .expect("bootstrap unified runtime");
    Fixture {
        _temp_dir: temp_dir,
        runtime,
    }
}

fn make_identity_spec(identity: &str) -> DurableAgentSpec {
    make_identity_spec_with_addressability(identity, AgentAddressability::Addressable)
}

fn make_identity_spec_with_addressability(
    identity: &str,
    addressability: AgentAddressability,
) -> DurableAgentSpec {
    DurableAgentSpec {
        identity: AgentIdentity::parse(identity).expect("parse identity"),
        profile: ProfileName::from("lead"),
        addressability,
        display_name: None,
        labels: BTreeMap::new(),
        context: None,
        additional_instructions: vec![],
    }
}

async fn build_identity_context(identity: &str) -> IdentityFirstContext {
    let spec = make_identity_spec(identity);
    build_identity_context_for_spec(spec, None).await
}

async fn build_identity_context_with_bridge(identity: &str) -> IdentityFirstContext {
    let spec = make_identity_spec(identity);
    build_identity_context_for_spec(spec, Some(Arc::new(MockSessionBridge))).await
}

async fn build_identity_context_internal_only(identity: &str) -> IdentityFirstContext {
    let spec = make_identity_spec_with_addressability(identity, AgentAddressability::InternalOnly);
    build_identity_context_for_spec(spec, None).await
}

async fn build_identity_context_for_spec(
    spec: DurableAgentSpec,
    bridge: Option<Arc<dyn SessionBridge>>,
) -> IdentityFirstContext {
    let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: Arc::new(LocalContinuityStore::in_memory().expect("continuity store")),
        lease_provider: Arc::new(LocalLeaseProvider::new()),
        runtime_instance_id: "phase0-console-runtime".to_string(),
        has_runtime_store: false,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge,
        default_timeout: None,
    }));
    restore_flow(&runtime, std::slice::from_ref(&spec), None, None)
        .await
        .expect("restore flow");
    IdentityFirstContext {
        runtime,
        roster_provider: Arc::new(StaticRosterProvider { specs: vec![spec] }),
        topology_provider: None,
        customizer: None,
    }
}

fn parse_json_rpc(response: &str) -> JsonRpcResponse {
    serde_json::from_str(response).expect("json-rpc response")
}

#[tokio::test]
async fn phase0_contract_001_mobkit_interact_accepts_identity_dispatch() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;

    let response = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "hello from phase 0",
                    "origin": "console:panel-1"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );

    assert!(
        response.error.is_none(),
        "unexpected rpc error: {:?}",
        response.error
    );
    let accepted: ConsoleInteractionAccepted =
        serde_json::from_value(response.result.expect("interact result"))
            .expect("accepted payload");
    assert_eq!(accepted.identity, "identity:luka");
    assert!(accepted.interaction_id.starts_with("turn-"));

    let shutdown = fixture.runtime.shutdown().await;
    assert!(shutdown.mob_stop.is_ok());
}

#[tokio::test]
async fn phase0_contract_001a_mobkit_interact_rejects_invalid_params() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;

    let response = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "req-2",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "hello from phase 0",
                    "origin": "   "
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );

    let error = response.error.expect("invalid params should reject");
    assert_eq!(error.code, -32602);
    assert!(error.message.contains("origin must be non-empty"));

    let shutdown = fixture.runtime.shutdown().await;
    assert!(shutdown.mob_stop.is_ok());
}

#[tokio::test]
async fn phase0_contract_001b_mobkit_interact_rejects_unknown_identity_synchronously() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;

    let response = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "req-2b",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:missing",
                    "content": "hello from phase 0",
                    "origin": "console:panel-1"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );

    let error = response.error.expect("unknown identity should reject");
    assert_eq!(error.code, -32001);
    assert!(error.message.contains("unknown identity"));

    let shutdown = fixture.runtime.shutdown().await;
    assert!(shutdown.mob_stop.is_ok());
}

#[tokio::test]
async fn phase0_contract_001c_mobkit_interact_rejects_internal_only_identity() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context_internal_only("triage:main").await;

    let response = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "req-2c",
                "method": "mobkit/interact",
                "params": {
                    "identity": "triage:main",
                    "content": "hello from phase 0",
                    "origin": "console:panel-1"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );

    let error = response
        .error
        .expect("internal-only identity should reject");
    assert_eq!(error.code, -32002);
    assert!(error.message.contains("not addressable"));

    let shutdown = fixture.runtime.shutdown().await;
    assert!(shutdown.mob_stop.is_ok());
}

#[tokio::test]
async fn phase0_contract_001d_mobkit_interact_rejects_when_identity_queue_is_full() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context_with_bridge("identity:luka").await;

    for idx in 0..256 {
        let response = parse_json_rpc(
            &handle_unified_rpc_json(
                &fixture.runtime,
                &json!({
                    "jsonrpc": "2.0",
                    "id": format!("seed-{idx}"),
                    "method": "mobkit/interact",
                    "params": {
                        "identity": "identity:luka",
                        "content": format!("hello {idx}"),
                        "origin": "console:panel-capacity"
                    }
                })
                .to_string(),
                Duration::from_secs(1),
                None,
                Some(&identity_ctx),
            )
            .await,
        );
        assert!(
            response.error.is_none(),
            "seed request {idx} should be accepted"
        );
    }

    let overflow = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "overflow",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "one too many",
                    "origin": "console:panel-capacity"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );

    let error = overflow.error.expect("queue overflow should reject");
    assert_eq!(error.code, -32003);
    assert!(error.message.contains("interaction queue at capacity"));

    let shutdown = fixture.runtime.shutdown().await;
    assert!(shutdown.mob_stop.is_ok());
}

#[tokio::test]
async fn phase0_contract_002_console_identity_stream_mounts_and_validates_request() {
    let app = console_json_router(decision_state(false));

    let bad_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/console/identity/stream")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "identity": "   " }).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(bad_response.status(), StatusCode::BAD_REQUEST);
    let bad_body = to_bytes(bad_response.into_body(), 1024 * 1024)
        .await
        .expect("bad body");
    let bad_json: Value = serde_json::from_slice(&bad_body).expect("bad json");
    assert_eq!(bad_json, json!({ "error": "identity must be non-empty" }));

    let ok_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/console/identity/stream")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "identity": "identity:luka" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(ok_response.status(), StatusCode::OK);
    assert!(
        ok_response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"))
    );
}

#[tokio::test]
async fn phase0_contract_002a_console_streams_enforce_console_auth_policy() {
    let app = console_json_router(decision_state(true));

    let events_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/console/events/stream")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("events response");
    assert_eq!(events_response.status(), StatusCode::UNAUTHORIZED);

    let identity_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/console/identity/stream")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "identity": "identity:luka" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("identity response");
    assert_eq!(identity_response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn phase0_contract_002b_console_events_stream_returns_typed_replay_unavailable() {
    let app = console_json_router(decision_state(false));

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/console/events/stream")
                .header("last-event-id", "evt-999")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let replay_error: ReplayUnavailableError =
        serde_json::from_slice(&body).expect("typed replay error");
    assert_eq!(replay_error.error, "replay_unavailable");
    assert_eq!(replay_error.stream, "all_events");
    assert_eq!(replay_error.requested_last_event_id, "evt-999");
    assert!(
        replay_error
            .latest_event_id
            .starts_with("console-stream-all_events-")
    );
}

#[tokio::test]
async fn phase0_contract_003_console_streams_emit_canonical_event_envelopes() {
    let app = console_json_router(decision_state(false));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/console/identity/stream")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "identity": "identity:luka" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.into_body().into_data_stream();
    let chunk = stream
        .next()
        .await
        .expect("initial sse frame")
        .expect("sse bytes");
    let text = String::from_utf8(chunk.to_vec()).expect("utf8 sse chunk");
    let data_line = text
        .lines()
        .find(|line| line.starts_with("data:"))
        .expect("sse data line");
    let envelope: meerkat_mobkit::ConsoleIdentityEventEnvelope =
        serde_json::from_str(data_line.trim_start_matches("data:").trim())
            .expect("typed console envelope");

    assert_eq!(envelope.identity, "identity:luka");
    assert_eq!(envelope.event_type, "subscribed");
    assert!(envelope.event_id.starts_with("console-stream-identity-"));
    assert!(envelope.interaction_id.is_none());
    assert_eq!(envelope.data, json!({ "stream": "identity" }));
}

#[tokio::test]
async fn phase0_runtime_helper_does_not_expose_inert_console_stream_routes() {
    let fixture = build_unified_runtime().await;
    let app = console_json_router_with_runtime(
        decision_state(false),
        fixture.runtime.mob_runtime().clone(),
        None,
        fixture.runtime.event_log_store(),
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/console/identity/stream")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "identity": "identity:luka" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let shutdown = fixture.runtime.shutdown().await;
    assert!(shutdown.mob_stop.is_ok());
}
