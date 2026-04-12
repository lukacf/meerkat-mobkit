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
    AuthPolicy, AuthProvider, BigQueryNaming, ConsoleIdentityEventEnvelope, ConsolePolicy,
    DiscoverySpec, IdentityFirstContext, JsonRpcResponse, MobBootstrapOptions, MobBootstrapSpec,
    MobKitConfig, RuntimeDecisionInputs, RuntimeOpsPolicy, TrustedOidcRuntimeConfig,
    UnifiedRuntime, build_runtime_decision_state, handle_unified_rpc_json,
};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Fixture {
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
            output_preview: Some("phase1 inspect preview".to_string()),
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
        jwks_json: r#"{"keys":[{"kid":"kid-current","kty":"oct","alg":"HS256","k":"cGhhc2UxLWNvbnNvbGUtc3RyZWFtLXNlY3JldA"}]}"#
            .to_string(),
        audience: "meerkat-console".to_string(),
    }
}

fn decision_state(require_app_auth: bool) -> meerkat_mobkit::RuntimeDecisionState {
    build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: "phase1_console_dataset".to_string(),
            table: "phase1_console_table".to_string(),
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
            namespace: "phase1-console-runtime".to_string(),
            modules: vec![],
        },
        pre_spawn: vec![],
    };

    let runtime = UnifiedRuntime::bootstrap(mob_spec, module_config, Duration::from_secs(2))
        .await
        .expect("bootstrap unified runtime");

    Fixture { runtime }
}

fn runtime_console_router(fixture: &Fixture) -> axum::Router {
    fixture
        .runtime
        .build_console_json_router(decision_state(false))
}

fn make_identity_spec(identity: &str) -> DurableAgentSpec {
    DurableAgentSpec {
        identity: AgentIdentity::parse(identity).expect("parse identity"),
        profile: ProfileName::from("lead"),
        addressability: AgentAddressability::Addressable,
        display_name: None,
        labels: BTreeMap::new(),
        context: None,
        additional_instructions: vec![],
    }
}

async fn build_identity_context(identity: &str) -> IdentityFirstContext {
    let spec = make_identity_spec(identity);
    let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: Arc::new(LocalContinuityStore::in_memory().expect("continuity store")),
        lease_provider: Arc::new(LocalLeaseProvider::new()),
        runtime_instance_id: "phase1-console-runtime".to_string(),
        has_runtime_store: false,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: None,
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

async fn build_identity_context_with_bridge(
    _fixture: &Fixture,
    identity: &str,
) -> IdentityFirstContext {
    let spec = make_identity_spec(identity);
    let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: Arc::new(LocalContinuityStore::in_memory().expect("continuity store")),
        lease_provider: Arc::new(LocalLeaseProvider::new()),
        runtime_instance_id: "phase1-console-runtime".to_string(),
        has_runtime_store: false,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(Arc::new(MockSessionBridge)),
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

fn parse_result(response: &str) -> Value {
    parse_json_rpc(response)
        .result
        .expect("json-rpc success result")
}

async fn first_sse_envelope(
    app: axum::Router,
    request: Request<Body>,
) -> (StatusCode, Option<ConsoleIdentityEventEnvelope>, String) {
    let response = app.oneshot(request).await.expect("sse response");
    let status = response.status();
    if status != StatusCode::OK {
        return (status, None, String::new());
    }
    let mut stream = response.into_body().into_data_stream();
    let mut text = String::new();
    let mut envelopes = Vec::new();

    for _ in 0..8 {
        let next = tokio::time::timeout(Duration::from_millis(50), stream.next()).await;
        let Some(chunk) = (match next {
            Ok(item) => item,
            Err(_) => break,
        }) else {
            break;
        };
        let chunk = chunk.expect("sse bytes");
        let chunk_text = String::from_utf8(chunk.to_vec()).expect("utf8 sse chunk");
        text.push_str(&chunk_text);

        for block in chunk_text
            .split("\n\n")
            .map(str::trim)
            .filter(|block| !block.is_empty())
        {
            if let Some(data_line) = block.lines().find(|line| line.starts_with("data:")) {
                let envelope: ConsoleIdentityEventEnvelope =
                    serde_json::from_str(data_line.trim_start_matches("data:").trim())
                        .expect("typed console envelope");
                envelopes.push(envelope);
            }
        }
    }

    let preferred = envelopes
        .iter()
        .find(|envelope| envelope.event_type != "subscribed" && envelope.identity != "console:all")
        .cloned()
        .or_else(|| envelopes.first().cloned());

    (status, preferred, text)
}

async fn collected_sse_envelopes(
    app: axum::Router,
    request: Request<Body>,
) -> (StatusCode, Vec<ConsoleIdentityEventEnvelope>, String) {
    let response = app.oneshot(request).await.expect("sse response");
    let status = response.status();
    if status != StatusCode::OK {
        return (status, Vec::new(), String::new());
    }
    let mut stream = response.into_body().into_data_stream();
    let mut text = String::new();
    let mut envelopes = Vec::new();

    for _ in 0..8 {
        let next = tokio::time::timeout(Duration::from_millis(50), stream.next()).await;
        let Some(chunk) = (match next {
            Ok(item) => item,
            Err(_) => break,
        }) else {
            break;
        };
        let chunk = chunk.expect("sse bytes");
        let chunk_text = String::from_utf8(chunk.to_vec()).expect("utf8 sse chunk");
        text.push_str(&chunk_text);

        for block in chunk_text
            .split("\n\n")
            .map(str::trim)
            .filter(|block| !block.is_empty())
        {
            if let Some(data_line) = block.lines().find(|line| line.starts_with("data:")) {
                let envelope: ConsoleIdentityEventEnvelope =
                    serde_json::from_str(data_line.trim_start_matches("data:").trim())
                        .expect("typed console envelope");
                envelopes.push(envelope);
            }
        }
    }

    (status, envelopes, text)
}

#[tokio::test]
async fn choke_001_interact_queue_identity_stream_correlation_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let interact = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-001",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "phase1 target",
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
    let interaction_id = interact["interaction_id"]
        .as_str()
        .expect("interaction id")
        .to_string();

    let (_, envelopes, raw_text) = collected_sse_envelopes(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;

    let envelope = envelopes
        .iter()
        .find(|envelope| envelope.identity == "identity:luka")
        .cloned()
        .or_else(|| envelopes.first().cloned())
        .expect("startup envelope");
    assert_eq!(envelope.identity, "identity:luka");
    assert!(
        raw_text.contains(&interaction_id),
        "CHOKE-001 target: the shared identity stream must eventually surface the accepted interaction_id {interaction_id}; startup-only subscription frames are insufficient"
    );
}

#[tokio::test]
async fn choke_001a_legacy_events_console_envelope_projection_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let (_, envelope, _) = first_sse_envelope(
        runtime_console_router(&fixture),
        Request::builder()
            .method("GET")
            .uri("/console/events/stream")
            .body(Body::empty())
            .expect("request"),
    )
    .await;

    let envelope = envelope.expect("event envelope");
    assert_ne!(
        envelope.event_type, "subscribed",
        "CHOKE-001a target: legacy runtime events should be projected into payload-bearing TYPE-004 envelopes on the all-events feed, not only a startup control frame"
    );
}

#[tokio::test]
async fn choke_001b_interaction_id_terminal_projection_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let interact = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-001b",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "terminal target",
                    "origin": "console:panel-2"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let interaction_id = interact["interaction_id"].as_str().expect("interaction id");

    let (_, envelopes, raw_text) = collected_sse_envelopes(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;

    assert!(
        envelopes.iter().any(|envelope| {
            matches!(
                envelope.event_type.as_str(),
                "interaction_complete" | "interaction_failed"
            ) && envelope.interaction_id.as_deref() == Some(interaction_id)
        }),
        "CHOKE-001b target: the minted interaction_id must flow through to exactly one terminal console event"
    );
    assert!(
        raw_text.contains("interaction_complete") || raw_text.contains("interaction_failed"),
        "CHOKE-001b target: a non-terminal started frame alone is insufficient terminal proof"
    );
}

#[tokio::test]
async fn choke_007_event_log_identity_query_recent_history_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let _ = handle_unified_rpc_json(
        &fixture.runtime,
        &json!({
            "jsonrpc": "2.0",
            "id": "choke-007-interact",
            "method": "mobkit/interact",
            "params": {
                "identity": "identity:luka",
                "content": "history target",
                "origin": "console:panel-history"
            }
        })
        .to_string(),
        Duration::from_secs(1),
        None,
        Some(&identity_ctx),
    )
    .await;
    let response = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-007",
                "method": "mobkit/query_events",
                "params": {
                    "identity": "identity:luka",
                    "limit": 10
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            None,
        )
        .await,
    );

    let events = response["events"].as_array().cloned().unwrap_or_default();
    assert!(
        events
            .iter()
            .any(|event| event.get("interaction_id").is_some()),
        "CHOKE-007 target: identity history should be structured enough to enrich recent-tool and last-activity surfaces without host inference"
    );
}

#[tokio::test]
async fn choke_014_all_events_feed_filters_non_identity_frames_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let (_, envelope, _) = first_sse_envelope(
        runtime_console_router(&fixture),
        Request::builder()
            .method("GET")
            .uri("/console/events/stream")
            .body(Body::empty())
            .expect("request"),
    )
    .await;

    let envelope = envelope.expect("envelope");
    assert_ne!(
        envelope.identity, "console:all",
        "CHOKE-014 target: the all-events console feed must project or filter runtime-global events into identity-attributed envelopes before the host consumes them"
    );
}

#[tokio::test]
async fn choke_013_accepted_interaction_async_failure_terminal_event_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let interact = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-013",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "force async failure path",
                    "origin": "console:panel-3"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let interaction_id = interact["interaction_id"].as_str().expect("interaction id");

    let (_, envelopes, raw_text) = collected_sse_envelopes(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    assert!(
        envelopes.iter().any(|envelope| {
            envelope.event_type == "interaction_failed"
                && envelope.interaction_id.as_deref() == Some(interaction_id)
        }),
        "CHOKE-013 target: once an interaction is accepted, later execution failure must still surface as a terminal interaction_failed frame"
    );
    assert!(
        raw_text.contains("interaction_failed"),
        "CHOKE-013 target: the stream must contain the explicit interaction_failed terminal event"
    );
}

#[tokio::test]
async fn choke_015_interact_lifecycle_race_terminal_outcome_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let interact = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-015-interact",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "race me",
                    "origin": "console:panel-race"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let interaction_id = interact["interaction_id"].as_str().expect("interaction id");
    let retire = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-015-retire",
                "method": "mobkit/retire",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    assert!(
        retire.error.is_none(),
        "retire call must reach the lifecycle boundary"
    );

    let (_, envelopes, raw_text) = collected_sse_envelopes(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    assert!(
        envelopes.iter().any(|envelope| {
            matches!(
                envelope.event_type.as_str(),
                "interaction_complete" | "interaction_failed"
            ) && envelope.interaction_id.as_deref() == Some(interaction_id)
        }),
        "CHOKE-015 target: an accepted turn racing with retire/reset/respawn must still produce one explicit terminal outcome"
    );
    assert!(
        raw_text.contains("interaction_failed") || raw_text.contains("interaction_complete"),
        "CHOKE-015 target: replay text must include an explicit terminal event, not merely the interaction id"
    );
}

#[tokio::test]
async fn choke_008_gating_capabilities_write_path_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let evaluated = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc":"2.0",
                "id":"choke-008-evaluate",
                "method":"mobkit/gating/evaluate",
                "params":{
                    "action":"publish_release",
                    "actor_id":"alice",
                    "risk_tier":"r3",
                    "requested_approver":"bob",
                    "approval_timeout_ms":60_000
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            None,
        )
        .await,
    );
    let decided = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc":"2.0",
                "id":"choke-008-decide",
                "method":"mobkit/gating/decide",
                "params":{
                    "pending_id": evaluated["pending_id"].clone(),
                    "approver_id":"bob",
                    "decision":"approve",
                    "reason":"looks good"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            None,
        )
        .await,
    );

    assert!(
        decided.get("decision") == Some(&json!("approve"))
            && decided.get("outcome") == Some(&json!("allowed")),
        "CHOKE-008 target: capability-driven gating actions should round-trip through the authorized mobkit/gating/decide write path"
    );
}

#[tokio::test]
async fn choke_012_retire_reconnect_replay_window_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let interact = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-012-interact",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "retire replay target",
                    "origin": "console:retire-replay"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let interaction_id = interact["interaction_id"]
        .as_str()
        .expect("interaction id")
        .to_string();
    let (_, initial_envelope, _) = first_sse_envelope(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    let checkpoint_id = initial_envelope.expect("initial checkpoint frame").event_id;
    let retire = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc":"2.0",
                "id":"choke-012-retire",
                "method":"mobkit/retire",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    assert!(retire.error.is_none());

    let replay_request = Request::builder()
        .method("POST")
        .uri("/console/identity/stream")
        .header("content-type", "application/json")
        .header("last-event-id", checkpoint_id.clone())
        .body(Body::from(
            json!({ "identity": "identity:luka" }).to_string(),
        ))
        .expect("request");
    let (status, _, text) =
        collected_sse_envelopes(runtime_console_router(&fixture), replay_request).await;

    if status == StatusCode::OK {
        assert!(
            text.contains(&checkpoint_id) && text.contains(&interaction_id),
            "CHOKE-012 target: reconnect must replay from the retained checkpoint when it remains available"
        );
    } else {
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "CHOKE-012 target: retire/delete must leave reconnects with either a retained replay window or an explicit typed replay failure"
        );
        let error: meerkat_mobkit::ReplayUnavailableError =
            serde_json::from_str(&text).expect("typed replay error");
        assert_eq!(error.error, "replay_unavailable");
        assert_eq!(error.requested_last_event_id, checkpoint_id);
    }
}

#[tokio::test]
async fn choke_005_status_inspect_topology_compose_identity_panel_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context_with_bridge(&fixture, "identity:luka").await;
    let status = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-005-status",
                "method": "mobkit/status_identity",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let inspect = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-005-inspect",
                "method": "mobkit/inspect_identity",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let status_error = status.error.clone();
    let inspect_error = inspect.error.clone();
    let status = status.result.unwrap_or_default();
    let inspect = inspect.result.unwrap_or_default();

    assert!(
        status.get("identity").is_some()
            && inspect.get("identity").is_some()
            && (inspect.get("continuity").is_some() || inspect.get("topology").is_some()),
        "CHOKE-005 target: status, inspect, and topology-backed identity data should compose into one inspect-panel view without host invention; current status_error={status_error:?} inspect_error={inspect_error:?}"
    );
}

#[tokio::test]
async fn choke_010_lifecycle_actions_refresh_console_surfaces_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let retire = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-010-retire",
                "method": "mobkit/retire",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let respawn = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-010-respawn",
                "method": "mobkit/respawn",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let status = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "choke-010-status",
                "method": "mobkit/status_identity",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );

    assert!(retire.error.is_none() && respawn.error.is_none());
    assert!(
        status.get("lifecycle_state").is_some() || status.get("state").is_some(),
        "CHOKE-010 target: console lifecycle actions should be reflected back through the same read-side surfaces the sidebar and inspect panel consume"
    );
}

#[tokio::test]
async fn e2e_002_identity_native_console_turn_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let interact = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-002",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "hello",
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

    let interaction_id = interact["interaction_id"].as_str().expect("interaction id");
    let (_, envelope, raw_text) = first_sse_envelope(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    let envelope = envelope.expect("startup envelope");

    assert!(
        raw_text.contains(interaction_id)
            && matches!(
                envelope.event_type.as_str(),
                "interaction_started" | "interaction_delta" | "interaction_complete"
            ),
        "E2E-002 target: an identity-addressed panel should render a real turn from the shared identity stream rather than only a subscription frame"
    );
}

#[tokio::test]
async fn e2e_005_mixed_event_classes_on_one_identity_stream_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let _ = handle_unified_rpc_json(
        &fixture.runtime,
        &json!({
            "jsonrpc": "2.0",
            "id": "e2e-005-interact",
            "method": "mobkit/interact",
            "params": {
                "identity": "identity:luka",
                "content": "mixed classes",
                "origin": "console:panel-mixed"
            }
        })
        .to_string(),
        Duration::from_secs(1),
        None,
        Some(&identity_ctx),
    )
    .await;
    let _ = handle_unified_rpc_json(
        &fixture.runtime,
        &json!({
            "jsonrpc": "2.0",
            "id": "e2e-005-retire",
            "method": "mobkit/retire",
            "params": { "identity": "identity:luka" }
        })
        .to_string(),
        Duration::from_secs(1),
        None,
        Some(&identity_ctx),
    )
    .await;

    let (_, envelope, raw_text) = first_sse_envelope(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    let envelope = envelope.expect("startup envelope");
    assert!(
        matches!(
            envelope.event_type.as_str(),
            "interaction_started"
                | "interaction_delta"
                | "interaction_complete"
                | "lease_updated"
                | "identity_retired"
        ) || raw_text.contains("identity_retired"),
        "E2E-005 target: lifecycle and turn events should coexist on one identity stream without forcing the host to stitch feeds"
    );
}

#[tokio::test]
async fn e2e_006_day_one_identity_inspect_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context_with_bridge(&fixture, "identity:luka").await;
    let status = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-006-status",
                "method": "mobkit/status_identity",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let inspect = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-006-inspect",
                "method": "mobkit/inspect_identity",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let status_error = status.error.clone();
    let inspect_error = inspect.error.clone();
    let status = status.result.unwrap_or_default();
    let inspect = inspect.result.unwrap_or_default();

    assert!(
        status.get("identity").is_some()
            && inspect.get("identity").is_some()
            && inspect.get("continuity").is_some(),
        "E2E-006 target: day-one inspect should compose status and inspect surfaces around one identity without host invention; current status_error={status_error:?} inspect_error={inspect_error:?}"
    );
}

#[tokio::test]
async fn e2e_007_virtualized_all_events_activity_log_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let (_, envelope, raw_text) = first_sse_envelope(
        runtime_console_router(&fixture),
        Request::builder()
            .method("GET")
            .uri("/console/events/stream")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    let envelope = envelope.expect("startup envelope");
    assert!(
        envelope.identity != "console:all" || raw_text.contains("\"identity\":\"identity:"),
        "E2E-007 target: the all-events feed should carry identity-attributed activity items suitable for one virtualized activity log"
    );
}

#[tokio::test]
async fn e2e_003_two_panels_one_identity_stream_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let interact = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-003",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "shared-stream target",
                    "origin": "console:panel-source"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let interaction_id = interact["interaction_id"]
        .as_str()
        .expect("interaction id")
        .to_string();
    let request = |panel: &str| {
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .header("x-console-panel", panel)
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request")
    };
    let app = runtime_console_router(&fixture);
    let (_, _, first_raw) = first_sse_envelope(app.clone(), request("panel-a")).await;
    let (_, _, second_raw) = first_sse_envelope(app, request("panel-b")).await;

    assert!(
        first_raw.contains(&interaction_id) && second_raw.contains(&interaction_id),
        "E2E-003 target: two panels aimed at one identity should observe the same interaction frames while the host later adds shared-connection pooling"
    );
}

#[tokio::test]
async fn e2e_004_reconnect_and_replay_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let interact = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-004-interact",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "replay target",
                    "origin": "console:replay"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let interaction_id = interact["interaction_id"]
        .as_str()
        .expect("interaction id")
        .to_string();
    let (_, first_envelope, _) = first_sse_envelope(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    let checkpoint_id = first_envelope.expect("initial checkpoint frame").event_id;
    let (status, _, _) = first_sse_envelope(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .header("last-event-id", &checkpoint_id)
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "E2E-004 target: reconnect with Last-Event-ID should replay from the retained identity window when the checkpoint is still available"
    );
    let (_, _, replay_text) = first_sse_envelope(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .header("last-event-id", checkpoint_id.clone())
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    assert!(
        replay_text.contains(&checkpoint_id) && replay_text.contains(&interaction_id),
        "E2E-004 target: the reconnect response must replay a frame from the requested checkpoint, not silently jump to a fresh tail"
    );
}

#[tokio::test]
async fn e2e_009_optional_gating_surface_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let response = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-009",
                "method": "mobkit/gating/pending",
                "params": {}
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            None,
        )
        .await,
    );

    assert!(
        response.get("entries").is_some() || response.get("pending").is_some(),
        "E2E-009 target: when the gating surface is present, the shared panel should be able to bootstrap from the real pending-entry read path"
    );
}

#[tokio::test]
async fn e2e_012_console_lifecycle_actions_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let retire = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-012-retire",
                "method": "mobkit/retire",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );
    let status = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-012-status",
                "method": "mobkit/status_identity",
                "params": { "identity": "identity:luka" }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );

    assert!(retire.error.is_none());
    assert!(
        status.get("lifecycle_state").is_some() || status.get("state").is_some(),
        "E2E-012 target: lifecycle actions should be visible through the same read-side surfaces the console uses"
    );
}

#[tokio::test]
async fn e2e_013_concurrent_turns_one_identity_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let first = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-013a",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "first",
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
    let second = parse_result(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-013b",
                "method": "mobkit/interact",
                "params": {
                    "identity": "identity:luka",
                    "content": "second",
                    "origin": "console:panel-2"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );

    let first_id = first["interaction_id"].as_str().expect("first interaction");
    let second_id = second["interaction_id"]
        .as_str()
        .expect("second interaction");
    assert_ne!(
        first_id, second_id,
        "panel turns must correlate independently"
    );

    let (_, _, raw_text) = first_sse_envelope(
        runtime_console_router(&fixture),
        Request::builder()
            .method("POST")
            .uri("/console/identity/stream")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "identity": "identity:luka" }).to_string(),
            ))
            .expect("request"),
    )
    .await;

    assert!(
        raw_text.contains(first_id) && raw_text.contains(second_id),
        "E2E-013 target: the shared identity stream must carry distinct concurrent interaction ids so panels can filter without premature completion"
    );
}

#[tokio::test]
async fn e2e_015_replay_window_exhausted_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let response = runtime_console_router(&fixture)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/console/identity/stream")
                .header("content-type", "application/json")
                .header("last-event-id", "evt-too-old")
                .body(Body::from(
                    json!({ "identity": "identity:luka" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "E2E-015 target: exhausted replay windows must fail explicitly with typed replay_unavailable instead of silently tailing live"
    );
}

#[tokio::test]
async fn e2e_018_mobkit_interact_rejection_target_defined_red() {
    let fixture = build_unified_runtime().await;
    let identity_ctx = build_identity_context("identity:luka").await;
    let response = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "e2e-018",
                "method": "mobkit/interact",
                "params": {
                    "identity": "   ",
                    "content": "hello",
                    "origin": "console:panel-9"
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            Some(&identity_ctx),
        )
        .await,
    );

    let error = response.error.expect("json-rpc rejection");
    assert_eq!(error.code, -32602);
    assert!(
        error.message.contains("identity must be non-empty"),
        "E2E-018 target: pre-acceptance mobkit/interact rejection must stay synchronous and typed"
    );
}
