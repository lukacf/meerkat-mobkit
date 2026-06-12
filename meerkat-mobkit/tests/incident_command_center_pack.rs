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
use meerkat_core::StopReason;
use serde_json::{Value, json};
use std::pin::Pin;
use std::sync::Arc;
use tower::ServiceExt;

#[path = "../../examples/001-incident-command-center-pack/incident_command_center.rs"]
mod incident_command_center;

use incident_command_center::{
    IncidentToolDispatcher, build_runtime_bundle_with_default_client, scenario_path,
};

struct IncidentPackTestClient;

impl meerkat_client::LlmClient for IncidentPackTestClient {
    fn stream<'a>(
        &'a self,
        _request: &'a LlmRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>> {
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
    let tools = IncidentToolDispatcher.tools();
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

#[tokio::test]
async fn incident_pack_exposes_seeded_stock_console_state() {
    let bundle = Box::pin(build_runtime_bundle_with_default_client(
        &scenario_path().expect("incident scenario path"),
        Arc::new(IncidentPackTestClient),
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
