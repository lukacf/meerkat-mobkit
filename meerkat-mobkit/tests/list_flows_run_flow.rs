#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args
)]
//! Integration test for `mobkit/list_flows` and `mobkit/run_flow`.
//!
//! Boots a `UnifiedRuntime` against a mob definition that declares a
//! single flow, then exercises the new RPC surface end-to-end:
//! list -> run -> status.

use std::sync::Arc;
use std::time::Duration;

use meerkat::{AgentFactory, Config, build_ephemeral_service};
use meerkat_client::TestClient;
use meerkat_mob::{MobDefinition, MobStorage};
use meerkat_mobkit::{
    DiscoverySpec, JsonRpcResponse, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig,
    UnifiedRuntime, handle_unified_rpc_json,
};
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    _temp_dir: TempDir,
    runtime: UnifiedRuntime,
}

async fn build_unified_runtime_with_flow() -> Fixture {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));

    // Mob definition with one flow ("demo") so list_flows has something to
    // return. The flow has a single trivial step targeting the lead role.
    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "flow-test-mob"

[profiles.lead]
model = "gpt-5.5"
external_addressable = true

[profiles.lead.tools]
comms = true

[flows.demo]
description = "demo flow"

[flows.demo.steps.first]
role = "lead"
message = "first"
"#,
    )
    .expect("parse mob definition");

    let mob_spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(TestClient::default())),
        });
    let module_config = MobKitConfig {
        modules: vec![],
        discovery: DiscoverySpec {
            namespace: "list-flows-run-flow-test".to_string(),
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

fn parse_json_rpc(response: &str) -> JsonRpcResponse {
    serde_json::from_str(response).expect("json-rpc response")
}

#[tokio::test]
async fn list_flows_returns_configured_flow_ids() {
    let fixture = build_unified_runtime_with_flow().await;

    let response = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "list-1",
                "method": "mobkit/list_flows",
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
        response.error.is_none(),
        "list_flows should not error: {:?}",
        response.error
    );
    let result = response.result.expect("list_flows result");
    let flows = result
        .get("flows")
        .and_then(Value::as_array)
        .expect("flows array");
    let flow_ids: Vec<String> = flows
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(
        flow_ids.iter().any(|id| id == "demo"),
        "demo flow should be advertised, got {:?}",
        flow_ids
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert!(
        shutdown.mob_stop.is_ok(),
        "mob stop failed at teardown: {:?}",
        shutdown.mob_stop
    );
}

#[tokio::test]
async fn run_flow_starts_run_and_status_observes_it() {
    let fixture = build_unified_runtime_with_flow().await;

    // Start the flow.
    let run_response = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "run-1",
                "method": "mobkit/run_flow",
                "params": {
                    "flow_id": "demo",
                    "params": { "hello": "world" }
                }
            })
            .to_string(),
            Duration::from_secs(2),
            None,
            None,
        )
        .await,
    );

    assert!(
        run_response.error.is_none(),
        "run_flow should not error: {:?}",
        run_response.error
    );
    let run_result = run_response.result.expect("run_flow result");
    let run_id = run_result
        .get("run_id")
        .and_then(Value::as_str)
        .expect("run_id string")
        .to_string();
    assert!(!run_id.is_empty(), "run_id must be non-empty");

    // Status should be observable for the freshly minted run id.
    let status_response = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "status-1",
                "method": "mobkit/flow_status",
                "params": { "run_id": run_id }
            })
            .to_string(),
            Duration::from_secs(2),
            None,
            None,
        )
        .await,
    );

    assert!(
        status_response.error.is_none(),
        "flow_status should not error: {:?}",
        status_response.error
    );
    // flow_status returns either the MobRun JSON or null. We require the
    // run is observable (not null) immediately after the run_flow call.
    let status = status_response.result.expect("flow_status result");
    assert!(
        !status.is_null(),
        "freshly started run should be observable in flow_status, got null"
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert!(
        shutdown.mob_stop.is_ok(),
        "mob stop failed at teardown: {:?}",
        shutdown.mob_stop
    );
}

#[tokio::test]
async fn run_flow_rejects_unknown_flow_id_with_invalid_params() {
    let fixture = build_unified_runtime_with_flow().await;

    let response = parse_json_rpc(
        &handle_unified_rpc_json(
            &fixture.runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "run-bad",
                "method": "mobkit/run_flow",
                "params": {
                    "flow_id": "does-not-exist",
                    "params": null
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            None,
        )
        .await,
    );

    let error = response.error.expect("unknown flow should error");
    assert_eq!(
        error.code, -32602,
        "unknown flow id should map to invalid-params (-32602), got {}: {}",
        error.code, error.message
    );

    let shutdown = fixture.runtime.shutdown().await;
    assert!(
        shutdown.mob_stop.is_ok(),
        "mob stop failed at teardown: {:?}",
        shutdown.mob_stop
    );
}
