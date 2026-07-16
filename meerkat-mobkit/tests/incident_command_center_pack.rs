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

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use futures::{Stream, stream};
use meerkat::AgentToolDispatcher;
use meerkat_client::{LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::lifecycle::run_primitive::ModelId;
use meerkat_core::types::{ContentInput, HandlingMode};
use meerkat_core::{AgentEvent, Provider, StopReason};
use meerkat_mob::{MobDefinition, MobError, SpawnMemberSpec};
use meerkat_mobkit::mob_handle_runtime::MobRuntimeError;
use meerkat_mobkit::{MemberTurnOptions, UnifiedRuntime};
use serde_json::{Value, json};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

#[path = "../../examples/001-incident-command-center-pack/incident_command_center.rs"]
mod incident_command_center;

use incident_command_center::{
    IncidentSessionHook, IncidentToolDispatcher, build_runtime_bundle_with_default_client,
    scenario_path,
};

#[derive(Clone, Default)]
struct IncidentPackTestClient {
    requested_models: Arc<std::sync::Mutex<Vec<String>>>,
}

impl IncidentPackTestClient {
    fn requested_models(&self) -> Vec<String> {
        self.requested_models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl meerkat_client::LlmClient for IncidentPackTestClient {
    fn project_replay_messages(
        &self,
        messages: &[meerkat_core::Message],
    ) -> Result<Vec<meerkat_core::Message>, LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(
        &'a self,
        request: &'a LlmRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>> {
        self.requested_models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request.model.clone());
        Box::pin(stream::iter([
            Ok(LlmEvent::TextDelta {
                delta: "ok".to_string(),
                meta: None,
            }),
            Ok(LlmEvent::Done {
                outcome: LlmDoneOutcome::Success {
                    stop_reason: StopReason::EndTurn,
                },
            }),
        ]))
    }

    // meerkat 0.7: LlmClient::provider returns the typed Provider.
    fn provider(&self) -> meerkat::Provider {
        meerkat::Provider::OpenAI
    }

    fn health_check<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), LlmError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Ok(()) })
    }
}

async fn json_response(app: axum::Router, request: Request<Body>) -> Value {
    let response = app.oneshot(request).await.expect("router response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body bytes");
    serde_json::from_slice(&body).expect("json body")
}

#[test]
fn incident_local_tools_are_witnessed_for_delegate_inheritance() {
    // meerkat 0.7 made meerkat_mob::snapshot private; the same provenance
    // contract is asserted through the public meerkat_core::tool_scope
    // witness derivation the snapshot used internally.
    let tools = IncidentToolDispatcher { inner: None }.tools();
    let defs: Vec<meerkat_core::types::ToolDef> =
        tools.iter().map(|tool| (**tool).clone()).collect();
    let filter = meerkat_core::tool_scope::ToolFilter::Allow(
        defs.iter().map(|def| def.name.to_string()).collect(),
    );
    let witnesses = meerkat_core::tool_scope::filter_witnesses_for_tool_defs(&defs, &filter);

    assert!(
        witnesses.contains_key("inspect_service"),
        "inspect_service must carry provenance so delegate can inherit it"
    );
    assert!(
        witnesses.contains_key("analyze_customer_impact"),
        "analyze_customer_impact must carry provenance so delegate can inherit it"
    );
}

// Regression: the incident tool dispatcher must COMPOSE over any external
// tools already installed on the build (the agent-memory recorder's `memory`
// tool), not replace them. The original hook overwrote build.external_tools,
// silently dropping the recorder so `memory` never reached the model and the
// Memory panel stayed empty.
#[tokio::test]
async fn incident_dispatcher_composes_inner_external_tools() {
    use meerkat_core::types::{ToolCallView, ToolDef, ToolResult};
    use meerkat_core::{ToolDispatchOutcome, ToolError};

    struct FakeInner;
    #[async_trait::async_trait]
    impl AgentToolDispatcher for FakeInner {
        fn tools(&self) -> Arc<[Arc<ToolDef>]> {
            vec![Arc::new(ToolDef {
                name: "memory".into(),
                description: "stand-in recorder".to_string(),
                input_schema: json!({"type": "object"}),
                provenance: None,
            })]
            .into()
        }
        async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
            Ok(ToolResult::new(call.id.to_string(), "recorded".to_string(), false).into())
        }
    }

    let dispatcher = IncidentToolDispatcher {
        inner: Some(Arc::new(FakeInner)),
    };
    let names: Vec<String> = dispatcher
        .tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.iter().any(|n| n == "inspect_service"));
    assert!(names.iter().any(|n| n == "analyze_customer_impact"));
    assert!(
        names.iter().any(|n| n == "memory"),
        "composed inner tool (memory recorder) must surface alongside incident tools: {names:?}"
    );

    // And an unknown-to-us call must delegate to the inner dispatcher (rather
    // than NotFound) — proving the recorder's tool is actually callable.
    let empty_args = serde_json::value::RawValue::from_string("{}".to_string()).unwrap();
    dispatcher
        .dispatch(ToolCallView {
            id: "call-1",
            name: "memory",
            args: &empty_args,
        })
        .await
        .expect("unknown tool must route to the inner recorder, not NotFound");

    // With no inner, an unknown tool is genuinely absent.
    let bare = IncidentToolDispatcher { inner: None };
    let miss = bare
        .dispatch(ToolCallView {
            id: "call-2",
            name: "memory",
            args: &empty_args,
        })
        .await;
    assert!(matches!(miss, Err(ToolError::NotFound { .. })));
}

// Regression: before_create must COMPOSE both external_tools AND
// additional_instructions over what the agent-memory customizer already
// installed on the build. Overwriting additional_instructions dropped the
// build-time memory-recall injection, so a respawned/fresh member never
// recalled its stored memories (the "respawn forgets my name" report).
#[tokio::test]
async fn incident_hook_composes_memory_injection_and_tools() {
    use meerkat_core::config::SystemPromptOverride;
    use meerkat_core::service::{CreateSessionRequest, InitialTurnPolicy, SessionBuildOptions};
    use meerkat_mobkit::SessionHook;

    // Stand in for the agent-memory customizer's pre-installed build state:
    // a recalled-memory injection line + a recorder external-tool dispatcher.
    let build = SessionBuildOptions {
        additional_instructions: Some(vec!["MEMORY-RECALL: operator name is Luka.".to_string()]),
        external_tools: Some(Arc::new(RecorderMarkerDispatcher)),
        ..SessionBuildOptions::default()
    };

    let mut req = CreateSessionRequest {
        model: "gpt-5.5".to_string(),
        prompt: meerkat_core::ContentInput::Text(String::new()),
        system_prompt: SystemPromptOverride::Inherit,
        max_tokens: None,
        event_tx: None,
        initial_turn: InitialTurnPolicy::Defer,
        build: Some(build),
        labels: None,
        deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
        injected_context: Vec::new(),
    };

    IncidentSessionHook
        .before_create(&mut req)
        .await
        .expect("before_create");

    let build = req.build.expect("build present");
    let instructions = build.additional_instructions.expect("instructions present");
    assert!(
        instructions
            .iter()
            .any(|line| line.contains("operator name is Luka")),
        "the pre-installed memory-recall injection must survive (composed, not overwritten): {instructions:?}"
    );
    assert!(
        instructions
            .iter()
            .any(|line| line.contains("incident command center")),
        "the incident framing must also be present"
    );
    let tool_names: Vec<String> = build
        .external_tools
        .expect("external tools present")
        .tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        tool_names.iter().any(|n| n == "memory-marker"),
        "the pre-installed recorder tool must survive alongside incident tools: {tool_names:?}"
    );
    assert!(tool_names.iter().any(|n| n == "inspect_service"));
}

struct RecorderMarkerDispatcher;

#[async_trait::async_trait]
impl AgentToolDispatcher for RecorderMarkerDispatcher {
    fn tools(&self) -> Arc<[Arc<meerkat_core::types::ToolDef>]> {
        vec![Arc::new(meerkat_core::types::ToolDef {
            name: "memory-marker".into(),
            description: "stand-in recorder".to_string(),
            input_schema: json!({"type": "object"}),
            provenance: None,
        })]
        .into()
    }
    async fn dispatch(
        &self,
        call: meerkat_core::types::ToolCallView<'_>,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        Ok(
            meerkat_core::types::ToolResult::new(call.id.to_string(), "ok".to_string(), false)
                .into(),
        )
    }
}

#[tokio::test]
async fn incident_pack_exposes_seeded_stock_console_state() {
    let bundle = Box::pin(build_runtime_bundle_with_default_client(
        &scenario_path().expect("incident scenario path"),
        Arc::new(IncidentPackTestClient::default()),
    ))
    .await
    .expect("incident runtime bundle");
    let app = bundle
        .runtime
        .build_reference_app_router(bundle.decisions.clone());

    let experience = json_response(
        app.clone(),
        Request::builder()
            .uri("/console/experience")
            .body(Body::empty())
            .expect("experience request"),
    )
    .await;

    assert_eq!(experience["contract_version"], json!("0.4.0"));
    let agents = experience["agent_sidebar"]["live_snapshot"]["agents"]
        .as_array()
        .expect("agent rows");
    let commander = agents
        .iter()
        .find(|entry| entry["identity"] == json!("incident-commander"))
        .expect("incident commander row");
    assert_eq!(commander["watched"], json!(true));
    assert_eq!(commander["alertLevel"], json!("critical"));

    let health_monitor = agents
        .iter()
        .find(|entry| entry["identity"] == json!("health-monitor"))
        .expect("health monitor row");
    assert_eq!(health_monitor["degraded"], json!(true));
    assert_eq!(health_monitor["degradedReason"], json!("peer_unreachable"));

    let topology_nodes = experience["topology"]["live_snapshot"]["nodes"]
        .as_array()
        .expect("topology nodes");
    let commander_node = topology_nodes
        .iter()
        .find(|entry| entry["identity"] == json!("incident-commander"))
        .expect("incident commander topology node");
    let wired_to = commander_node["wired_to"]
        .as_array()
        .expect("wired_to array");
    assert!(
        wired_to.contains(&json!("payments-sre")) || wired_to.contains(&json!("merchant-comms")),
        "incident commander should have seeded runtime wiring"
    );

    let filter_presets = experience["activity_feed"]["filter_presets"]
        .as_array()
        .expect("activity filter presets");
    assert!(
        filter_presets
            .iter()
            .any(|preset| preset["id"] == json!("watched-only")),
        "watched-only filter preset should be projected"
    );

    let routes = json_response(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/console/rpc")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": "incident-pack-routes",
                    "method": "mobkit/routing/routes/list",
                    "params": {},
                })
                .to_string(),
            ))
            .expect("routes request"),
    )
    .await;
    assert!(
        routes.get("error").is_none() || routes["error"].is_null(),
        "routing rpc should succeed: {routes:?}"
    );
    let route_rows = routes["result"]["routes"]
        .as_array()
        .unwrap_or_else(|| panic!("routes array missing in response: {routes:?}"));
    assert!(
        route_rows
            .iter()
            .any(|route| route["route_key"] == json!("incident-statuspage")),
        "seeded statuspage route should exist"
    );

    let pending = json_response(
        app,
        Request::builder()
            .method("POST")
            .uri("/console/rpc")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": "incident-pack-gating",
                    "method": "mobkit/gating/pending",
                    "params": {},
                })
                .to_string(),
            ))
            .expect("gating request"),
    )
    .await;
    assert!(
        pending.get("error").is_none() || pending["error"].is_null(),
        "gating rpc should succeed: {pending:?}"
    );
    let pending_rows = pending["result"]["pending"]
        .as_array()
        .unwrap_or_else(|| panic!("pending array missing in response: {pending:?}"));
    assert!(
        !pending_rows.is_empty(),
        "seeded gating pending entry should exist"
    );
}

/// Regression test: the seeded R3 gating action must deliver its approval
/// notification even though the bundle is built inside an active tokio
/// runtime. `UnifiedRuntime::evaluate_gating_action` previously ran the
/// router/delivery MCP boundary calls directly on the runtime thread, so the
/// notification silently failed with
/// `cannot execute blocking MCP boundary call inside an active tokio runtime`
/// and the pending request timed out to safe_draft.
#[tokio::test]
async fn incident_pack_gating_approval_notification_fires_from_async_context() {
    let bundle = Box::pin(build_runtime_bundle_with_default_client(
        &scenario_path().expect("incident scenario path"),
        Arc::new(IncidentPackTestClient::default()),
    ))
    .await
    .expect("incident runtime bundle");
    let app = bundle
        .runtime
        .build_reference_app_router(bundle.decisions.clone());

    let pending = json_response(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/console/rpc")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": "incident-pack-gating-pending",
                    "method": "mobkit/gating/pending",
                    "params": {},
                })
                .to_string(),
            ))
            .expect("gating pending request"),
    )
    .await;
    let pending_rows = pending["result"]["pending"]
        .as_array()
        .unwrap_or_else(|| panic!("pending array missing in response: {pending:?}"));
    let seeded = pending_rows
        .iter()
        .find(|entry| entry["action"] == json!("publish_status_update"))
        .unwrap_or_else(|| panic!("seeded publish_status_update pending entry: {pending:?}"));
    assert!(
        seeded["approval_route_id"].is_string(),
        "approval route must be resolved through the router module: {seeded:?}"
    );
    assert!(
        seeded["approval_delivery_id"].is_string(),
        "approval notification must be delivered through the delivery module: {seeded:?}"
    );

    let audit = json_response(
        app,
        Request::builder()
            .method("POST")
            .uri("/console/rpc")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": "incident-pack-gating-audit",
                    "method": "mobkit/gating/audit",
                    "params": { "limit": 100 },
                })
                .to_string(),
            ))
            .expect("gating audit request"),
    )
    .await;
    let entries = audit["result"]["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("audit entries missing in response: {audit:?}"));
    let pending_created = entries
        .iter()
        .find(|entry| {
            entry["event_type"] == json!("pending_created")
                && entry["action_id"] == seeded["action_id"]
        })
        .unwrap_or_else(|| panic!("pending_created audit entry: {audit:?}"));
    assert!(
        pending_created["detail"]["approval_notification_error"].is_null(),
        "approval notification must not fail: {pending_created:?}"
    );
}

#[tokio::test]
async fn unified_runtime_member_turn_returns_exact_session_and_committed_completion() {
    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "member-turn-wrapper-test"

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "turn_driven"
external_addressable = true

[profiles.worker.tools]
comms = true
"#,
    )
    .expect("parse turn-driven test definition");
    let llm_client = Arc::new(IncidentPackTestClient::default());
    let runtime = Box::pin(
        UnifiedRuntime::builder()
            .definition(definition)
            .default_llm_client(llm_client.clone())
            .timeout(Duration::from_secs(5))
            .build(),
    )
    .await
    .expect("build turn-driven unified runtime");
    runtime
        .spawn(SpawnMemberSpec::from_wire(
            "worker".to_string(),
            "console-worker".to_string(),
            None,
            None,
            None,
        ))
        .await
        .expect("spawn turn-driven console worker");

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(64);
    let mut admission = runtime
        .start_member_turn(
            "console-worker",
            ContentInput::Text("report status".to_string()),
            HandlingMode::Queue,
            MemberTurnOptions::new()
                .with_model(ModelId::new("gpt-5.6"))
                .with_provider(Provider::OpenAI),
            Some(event_tx),
        )
        .await
        .expect("admit completion-bearing member turn");
    assert_eq!(
        admission.turn.session_id().map(ToString::to_string),
        Some(admission.session_id.clone()),
        "the wrapper must expose the exact bridge session captured by admission"
    );
    let applied_identity = tokio::time::timeout(
        Duration::from_secs(2),
        admission.turn.wait_for_applied_llm_identity(),
    )
    .await
    .expect("applied identity acknowledgement should resolve")
    .expect("stock runtime should apply the requested identity")
    .expect("turn requested a non-default identity");
    assert_eq!(applied_identity.model, "gpt-5.6");
    assert_eq!(applied_identity.provider, Provider::OpenAI);

    let receipt = tokio::time::timeout(Duration::from_secs(2), admission.turn.wait())
        .await
        .expect("turn completion should resolve")
        .expect("turn should commit successfully");
    assert_eq!(receipt.handling_mode, HandlingMode::Queue);

    let mut observed_text_output = false;
    let mut observed_committed_completion = false;
    let mut observed_events = Vec::new();
    while !observed_committed_completion {
        let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
            .await
            .expect("live member-turn event should arrive")
            .expect("member-turn event channel should remain open through completion");
        observed_events.push(format!("{:?}", event.payload));
        match event.payload {
            AgentEvent::TextDelta { delta } => {
                observed_text_output |= delta == "ok";
            }
            AgentEvent::TextComplete { content } => {
                observed_text_output |= content == "ok";
            }
            AgentEvent::RunCompleted { session_id, .. } => {
                assert_eq!(
                    session_id.to_string(),
                    admission.session_id,
                    "the committed terminal must carry the exact admitted session"
                );
                observed_committed_completion = true;
            }
            AgentEvent::RunFailed { error_report, .. } => {
                panic!("member turn unexpectedly failed: {error_report:?}")
            }
            _ => {}
        }
    }
    assert!(
        observed_text_output,
        "the stock incident-console path must forward model output before completion; observed {observed_events:?}"
    );
    assert!(
        llm_client
            .requested_models()
            .iter()
            .any(|model| model == "gpt-5.6"),
        "the stock incident-console constructor must deliver the applied model to LlmRequest"
    );

    let _ = runtime.shutdown().await;
}

#[tokio::test]
async fn stock_incident_commander_rejects_unrepresentable_tracked_turn() {
    let bundle = Box::pin(build_runtime_bundle_with_default_client(
        &scenario_path().expect("incident scenario path"),
        Arc::new(IncidentPackTestClient::default()),
    ))
    .await
    .expect("incident runtime bundle");
    let (event_tx, _event_rx) = tokio::sync::mpsc::channel(1);

    let error = bundle
        .runtime
        .start_member_turn(
            "incident-commander",
            ContentInput::Text("tracked console turn".to_string()),
            HandlingMode::Queue,
            MemberTurnOptions::default(),
            Some(event_tx),
        )
        .await
        .expect_err("stock autonomous profiles must not silently drop tracking semantics");
    assert!(
        matches!(
            error,
            MobRuntimeError::Mob(MobError::UnsupportedForMode { .. })
        ),
        "expected typed unsupported-mode rejection, got {error}"
    );

    let _ = bundle.runtime.shutdown().await;
}
