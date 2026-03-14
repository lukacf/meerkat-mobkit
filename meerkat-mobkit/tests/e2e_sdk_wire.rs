//! End-to-end wire-format tests: SDK request shapes → RPC handler → response shapes.
//!
//! These use the in-process `handle_mobkit_rpc_json` path so they complete in
//! milliseconds with no subprocesses. Each test sends the exact JSON the SDK
//! would produce and asserts the response matches what the SDK parser expects.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::redundant_clone
)]

use std::time::Duration;

use meerkat_mobkit::{
    DiscoverySpec, MobKitConfig, ModuleConfig, PreSpawnData, RestartPolicy, handle_mobkit_rpc_json,
    start_mobkit_runtime,
};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(2);

fn noop_module(id: &str) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec![
            "-c".to_string(),
            format!(
                r#"printf '%s\n' '{{"event_id":"evt-{id}","source":"module","timestamp_ms":1,"event":{{"kind":"module","module":"{id}","event_type":"ready","payload":{{"ok":true}}}}}}'"#,
            ),
        ],
        restart_policy: RestartPolicy::Never,
    }
}

fn test_runtime() -> meerkat_mobkit::MobkitRuntimeHandle {
    let config = MobKitConfig {
        modules: vec![noop_module("alpha"), noop_module("beta")],
        discovery: DiscoverySpec {
            namespace: "e2e-wire".to_string(),
            modules: vec!["alpha".to_string()],
        },
        pre_spawn: vec![
            PreSpawnData {
                module_id: "alpha".to_string(),
                env: vec![],
            },
            PreSpawnData {
                module_id: "beta".to_string(),
                env: vec![],
            },
        ],
    };
    start_mobkit_runtime(config, vec![], TIMEOUT).expect("runtime starts")
}

fn rpc(runtime: &mut meerkat_mobkit::MobkitRuntimeHandle, request: &Value) -> Value {
    let response_str = handle_mobkit_rpc_json(runtime, &request.to_string(), TIMEOUT);
    serde_json::from_str(&response_str).expect("valid JSON response")
}

// ---------------------------------------------------------------------------
// mobkit/status — SDK expects: contract_version, running, loaded_modules
// ---------------------------------------------------------------------------

#[test]
fn e2e_status_response_matches_sdk_contract() {
    let mut rt = test_runtime();
    let resp = rpc(
        &mut rt,
        &json!({
            "jsonrpc": "2.0", "id": "s1", "method": "mobkit/status", "params": {}
        }),
    );
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], "s1");
    let result = &resp["result"];
    assert!(result["contract_version"].is_string());
    assert!(result["running"].is_boolean());
    assert!(result["loaded_modules"].is_array());
}

// ---------------------------------------------------------------------------
// mobkit/capabilities — SDK expects: contract_version, methods[], loaded_modules[]
// ---------------------------------------------------------------------------

#[test]
fn e2e_capabilities_response_matches_sdk_contract() {
    let mut rt = test_runtime();
    let resp = rpc(
        &mut rt,
        &json!({
            "jsonrpc": "2.0", "id": "c1", "method": "mobkit/capabilities", "params": {}
        }),
    );
    let result = &resp["result"];
    assert!(result["contract_version"].is_string());
    assert!(result["methods"].is_array());
    assert!(result["loaded_modules"].is_array());
}

// ---------------------------------------------------------------------------
// mobkit/reconcile — strict param validation
// ---------------------------------------------------------------------------

#[test]
fn e2e_reconcile_requires_string_array() {
    let mut rt = test_runtime();

    // Valid call
    let resp = rpc(
        &mut rt,
        &json!({
            "jsonrpc": "2.0", "id": "r1", "method": "mobkit/reconcile",
            "params": { "modules": ["alpha"] }
        }),
    );
    assert!(resp["result"]["accepted"].as_bool().unwrap());
    assert_eq!(resp["result"]["reconciled_modules"], json!(["alpha"]));

    // Missing modules field → -32602
    let resp = rpc(
        &mut rt,
        &json!({
            "jsonrpc": "2.0", "id": "r2", "method": "mobkit/reconcile",
            "params": {}
        }),
    );
    assert_eq!(resp["error"]["code"], -32602);

    // Non-string entry in modules → -32602
    let resp = rpc(
        &mut rt,
        &json!({
            "jsonrpc": "2.0", "id": "r3", "method": "mobkit/reconcile",
            "params": { "modules": ["alpha", 42] }
        }),
    );
    assert_eq!(resp["error"]["code"], -32602);
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("modules[1]")
    );

    // modules is not an array → -32602
    let resp = rpc(
        &mut rt,
        &json!({
            "jsonrpc": "2.0", "id": "r4", "method": "mobkit/reconcile",
            "params": { "modules": "alpha" }
        }),
    );
    assert_eq!(resp["error"]["code"], -32602);
}

// ---------------------------------------------------------------------------
// Note: mobkit/send_message, mobkit/ensure_member, and mobkit/query_events
// are unified-runtime-only methods (not available via handle_mobkit_rpc_json).
// They are tested in phase4 and the unified handler tests.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// mobkit/spawn_member — both module_id and profile paths
// ---------------------------------------------------------------------------

#[test]
fn e2e_spawn_member_validates_params() {
    let mut rt = test_runtime();

    // Empty module_id → -32602
    let resp = rpc(
        &mut rt,
        &json!({
            "jsonrpc": "2.0", "id": "sp1", "method": "mobkit/spawn_member",
            "params": { "module_id": "" }
        }),
    );
    assert_eq!(resp["error"]["code"], -32602);

    // No params at all → -32602
    let resp = rpc(
        &mut rt,
        &json!({
            "jsonrpc": "2.0", "id": "sp2", "method": "mobkit/spawn_member",
            "params": {}
        }),
    );
    assert_eq!(resp["error"]["code"], -32602);
}

// Note: mobkit/query_events is unified-runtime-only (tested in mob_methods).

// ---------------------------------------------------------------------------
// JSON-RPC envelope — id correlation
// ---------------------------------------------------------------------------

#[test]
fn e2e_response_id_correlates_with_request() {
    let mut rt = test_runtime();

    let resp = rpc(
        &mut rt,
        &json!({
            "jsonrpc": "2.0", "id": "unique-42", "method": "mobkit/status", "params": {}
        }),
    );
    assert_eq!(resp["id"], "unique-42");
    assert_eq!(resp["jsonrpc"], "2.0");
}

#[test]
fn e2e_unknown_method_returns_minus_32601() {
    let mut rt = test_runtime();
    let resp = rpc(
        &mut rt,
        &json!({
            "jsonrpc": "2.0", "id": "u1", "method": "mobkit/nonexistent", "params": {}
        }),
    );
    assert_eq!(resp["error"]["code"], -32601);
}

// Note: mobkit/ensure_member is unified-runtime-only (tested in mob_methods).
