#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args
)]
//! Regression: `mobkit/read_session_history` rejects non-positive
//! `limit` instead of silently treating it as "no limit".
//!
//! Pre-fix, the param parser only errored when `limit` was not a
//! `Number`. A negative integer (or `0`) returned `None` from
//! `as_u64()` and was collapsed with the legitimate `null`/missing
//! case, surfacing the entire history regardless of caller intent.

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

async fn build_runtime() -> UnifiedRuntime {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");
    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));
    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "session-history-mob"

[profiles.lead]
model = "gpt-5.2"
external_addressable = true

[profiles.lead.tools]
comms = true

[flows.demo]
description = "demo"

[flows.demo.steps.first]
role = "lead"
message = "first"
"#,
    )
    .expect("definition");
    let mob_spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(TestClient::default())),
        });
    let module_config = MobKitConfig {
        modules: vec![],
        discovery: DiscoverySpec {
            namespace: "session-history-test".to_string(),
            modules: vec![],
        },
        pre_spawn: vec![],
    };
    let runtime = UnifiedRuntime::bootstrap(mob_spec, module_config, Duration::from_secs(2))
        .await
        .expect("bootstrap");
    // `_temp_dir` deliberately leaked: the runtime keeps file handles
    // beyond this fn; cleanup happens at process exit.
    std::mem::forget(temp_dir);
    runtime
}

fn parse(response: &str) -> JsonRpcResponse {
    serde_json::from_str(response).expect("json-rpc")
}

async fn read_history(runtime: &UnifiedRuntime, limit: Value) -> JsonRpcResponse {
    parse(
        &handle_unified_rpc_json(
            runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "history",
                "method": "mobkit/read_session_history",
                "params": { "session_id": "any", "limit": limit }
            })
            .to_string(),
            Duration::from_secs(2),
            None,
            None,
        )
        .await,
    )
}

#[tokio::test]
async fn read_session_history_rejects_negative_limit() {
    let runtime = build_runtime().await;
    let response = read_history(&runtime, json!(-1)).await;
    let err = response.error.as_ref().expect("must error on -1");
    assert_eq!(
        err.code, -32602,
        "negative limit must be -32602, got {}",
        err.code
    );
    assert!(
        err.message.contains("positive integer") || err.message.contains(">= 1"),
        "error message must explain the constraint, got: {}",
        err.message
    );
    let _ = runtime.shutdown().await;
}

#[tokio::test]
async fn read_session_history_rejects_zero_limit() {
    let runtime = build_runtime().await;
    let response = read_history(&runtime, json!(0)).await;
    let err = response.error.as_ref().expect("must error on 0");
    assert_eq!(
        err.code, -32602,
        "zero limit must be -32602, got {}",
        err.code
    );
    let _ = runtime.shutdown().await;
}

#[tokio::test]
async fn read_session_history_accepts_null_limit_as_no_limit() {
    let runtime = build_runtime().await;
    let response = read_history(&runtime, Value::Null).await;
    // The session doesn't exist; we expect either a 404-like error or
    // an empty result, but NOT a -32602 "limit must be positive".
    if let Some(err) = response.error.as_ref() {
        assert_ne!(
            err.code, -32602,
            "null limit must not be rejected as invalid params, got {}: {}",
            err.code, err.message
        );
    }
    let _ = runtime.shutdown().await;
}
