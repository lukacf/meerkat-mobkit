use std::time::Duration;

use meerkat_mobkit_core::{
    handle_mobkit_rpc_json, start_mobkit_runtime, DiscoverySpec, MobKitConfig, ModuleConfig,
    RestartPolicy,
};
use serde_json::{json, Value};

fn parse_response(line: &str) -> Value {
    serde_json::from_str(line).expect("valid rpc response json")
}

fn runtime_for_gating() -> meerkat_mobkit_core::MobkitRuntimeHandle {
    start_mobkit_runtime(
        MobKitConfig {
            modules: vec![],
            discovery: DiscoverySpec {
                namespace: "phase12".to_string(),
                modules: vec![],
            },
            pre_spawn: vec![],
        },
        vec![],
        Duration::from_secs(1),
    )
    .expect("runtime starts")
}

fn shell_module(id: &str, script: &str) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        restart_policy: RestartPolicy::Never,
    }
}

fn runtime_for_gating_with_routing_delivery() -> meerkat_mobkit_core::MobkitRuntimeHandle {
    start_mobkit_runtime(
        MobKitConfig {
            modules: vec![
                shell_module(
                    "router",
                    r#"printf '%s\n' '{"event_id":"evt-router","source":"module","timestamp_ms":10,"event":{"kind":"module","module":"router","event_type":"response","payload":{"via":"router","ok":true}}}'"#,
                ),
                shell_module(
                    "delivery",
                    r#"printf '%s\n' '{"event_id":"evt-delivery","source":"module","timestamp_ms":20,"event":{"kind":"module","module":"delivery","event_type":"ready","payload":{"sink":"memory"}}}'"#,
                ),
            ],
            discovery: DiscoverySpec {
                namespace: "phase12-routing".to_string(),
                modules: vec!["router".to_string(), "delivery".to_string()],
            },
            pre_spawn: vec![],
        },
        vec![],
        Duration::from_secs(1),
    )
    .expect("runtime starts")
}

fn runtime_for_gating_with_forced_failed_delivery() -> meerkat_mobkit_core::MobkitRuntimeHandle {
    start_mobkit_runtime(
        MobKitConfig {
            modules: vec![
                shell_module(
                    "router",
                    r#"printf '%s\n' '{"event_id":"evt-router","source":"module","timestamp_ms":10,"event":{"kind":"module","module":"router","event_type":"response","payload":{"via":"router","ok":true}}}'"#,
                ),
                shell_module(
                    "delivery",
                    r#"printf '%s\n' '{"event_id":"evt-delivery","source":"module","timestamp_ms":20,"event":{"kind":"module","module":"delivery","event_type":"send","payload":{"adapter":"memory","force_fail":true}}}'"#,
                ),
            ],
            discovery: DiscoverySpec {
                namespace: "phase12-routing-fail".to_string(),
                modules: vec!["router".to_string(), "delivery".to_string()],
            },
            pre_spawn: vec![],
        },
        vec![],
        Duration::from_secs(1),
    )
    .expect("runtime starts")
}

#[test]
fn phase12_r3_approval_flow_enforces_approver_constraints_and_audits() {
    let mut runtime = runtime_for_gating_with_routing_delivery();
    let evaluated = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc":"2.0",
            "id":"phase12-r3-evaluate",
            "method":"mobkit/gating/evaluate",
            "params":{
                "action":"publish_release",
                "actor_id":"alice",
                "risk_tier":"r3",
                "requested_approver":"bob",
                "approval_recipient":"approvals@example.com",
                "approval_channel":"transactional",
                "approval_timeout_ms":60_000
            }
        })
        .to_string(),
        Duration::from_secs(1),
    ));
    let pending_before_decision = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-r3-pending-before","method":"mobkit/gating/pending","params":{}}"#,
        Duration::from_secs(1),
    ));
    let self_approve = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc":"2.0",
            "id":"phase12-r3-self-approve",
            "method":"mobkit/gating/decide",
            "params":{
                "pending_id": evaluated["result"]["pending_id"].clone(),
                "approver_id":"alice",
                "decision":"approve"
            }
        })
        .to_string(),
        Duration::from_secs(1),
    ));
    let wrong_approver = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc":"2.0",
            "id":"phase12-r3-wrong-approver",
            "method":"mobkit/gating/decide",
            "params":{
                "pending_id": evaluated["result"]["pending_id"].clone(),
                "approver_id":"carol",
                "decision":"approve"
            }
        })
        .to_string(),
        Duration::from_secs(1),
    ));
    let pending_after_invalid = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-r3-pending-after-invalid","method":"mobkit/gating/pending","params":{}}"#,
        Duration::from_secs(1),
    ));
    let approved = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc":"2.0",
            "id":"phase12-r3-approve",
            "method":"mobkit/gating/decide",
            "params":{
                "pending_id": evaluated["result"]["pending_id"].clone(),
                "approver_id":"bob",
                "decision":"approve"
            }
        })
        .to_string(),
        Duration::from_secs(1),
    ));
    let pending = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-r3-pending","method":"mobkit/gating/pending","params":{}}"#,
        Duration::from_secs(1),
    ));
    let audit = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-r3-audit","method":"mobkit/gating/audit","params":{"limit":20}}"#,
        Duration::from_secs(1),
    ));
    let history = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-r3-history","method":"mobkit/delivery/history","params":{"recipient":"approvals@example.com","limit":5}}"#,
        Duration::from_secs(1),
    ));

    runtime.shutdown();

    let entries = audit["result"]["entries"]
        .as_array()
        .expect("audit entries");
    let events = entries
        .iter()
        .filter_map(|entry| entry.get("event_type").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let pending_created = entries
        .iter()
        .find(|entry| entry.get("event_type") == Some(&json!("pending_created")))
        .expect("pending_created entry");
    let approval_decided = entries
        .iter()
        .find(|entry| entry.get("event_type") == Some(&json!("approval_decided")))
        .expect("approval_decided entry");
    assert_eq!(evaluated["result"]["outcome"], json!("pending_approval"));
    assert_eq!(
        self_approve["error"]["message"],
        json!("Invalid params: approver_id cannot self-approve the action actor")
    );
    assert_eq!(
        wrong_approver["error"]["message"],
        json!("Invalid params: approver_id 'carol' does not match requested_approver 'bob'")
    );
    assert_eq!(approved["result"]["outcome"], json!("allowed"));
    assert_eq!(pending["result"]["pending"], json!([]));
    assert!(events.contains(&"pending_created"));
    assert!(events.contains(&"approval_decided"));
    assert_eq!(
        pending_after_invalid["result"]["pending"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        pending_after_invalid["result"]["pending"][0]["pending_id"],
        pending_before_decision["result"]["pending"][0]["pending_id"]
    );
    assert_eq!(
        pending_created["detail"]["approval_route_id"],
        pending_before_decision["result"]["pending"][0]["approval_route_id"]
    );
    assert_eq!(
        pending_created["detail"]["approval_delivery_id"],
        pending_before_decision["result"]["pending"][0]["approval_delivery_id"]
    );
    assert_eq!(
        approval_decided["detail"]["approval_route_id"],
        pending_before_decision["result"]["pending"][0]["approval_route_id"]
    );
    assert_eq!(
        approval_decided["detail"]["approval_delivery_id"],
        pending_before_decision["result"]["pending"][0]["approval_delivery_id"]
    );
    assert_eq!(
        history["result"]["deliveries"][0]["route_id"],
        pending_before_decision["result"]["pending"][0]["approval_route_id"]
    );
    assert_eq!(
        history["result"]["deliveries"][0]["delivery_id"],
        pending_before_decision["result"]["pending"][0]["approval_delivery_id"]
    );
}

#[test]
fn phase12_risk_tiers_and_timeout_fallback_are_wired_with_audit() {
    let mut runtime = runtime_for_gating();
    let r0 = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc":"2.0",
            "id":"phase12-r0",
            "method":"mobkit/gating/evaluate",
            "params":{"action":"read_status","actor_id":"agent-r0","risk_tier":"r0"}
        })
        .to_string(),
        Duration::from_secs(1),
    ));
    let r1 = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc":"2.0",
            "id":"phase12-r1",
            "method":"mobkit/gating/evaluate",
            "params":{"action":"edit_doc","actor_id":"agent-r1","risk_tier":"r1"}
        })
        .to_string(),
        Duration::from_secs(1),
    ));
    let r2 = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc":"2.0",
            "id":"phase12-r2",
            "method":"mobkit/gating/evaluate",
            "params":{"action":"notify_users","actor_id":"agent-r2","risk_tier":"r2"}
        })
        .to_string(),
        Duration::from_secs(1),
    ));
    let _r3_timeout = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc":"2.0",
            "id":"phase12-r3",
            "method":"mobkit/gating/evaluate",
            "params":{
                "action":"delete_prod_data",
                "actor_id":"agent-r3",
                "risk_tier":"r3",
                "approval_timeout_ms":0
            }
        })
        .to_string(),
        Duration::from_secs(1),
    ));
    let pending = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-pending","method":"mobkit/gating/pending","params":{}}"#,
        Duration::from_secs(1),
    ));
    let audit = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-audit","method":"mobkit/gating/audit","params":{"limit":30}}"#,
        Duration::from_secs(1),
    ));

    runtime.shutdown();

    let entries = audit["result"]["entries"]
        .as_array()
        .expect("audit entries");
    let has_timeout_fallback = entries.iter().any(|entry| {
        entry.get("event_type") == Some(&json!("timeout_fallback"))
            && entry.get("outcome") == Some(&json!("safe_draft"))
    });
    let has_r2_consequence = entries.iter().any(|entry| {
        entry.get("outcome") == Some(&json!("allowed_with_audit"))
            && entry.get("detail").and_then(|detail| detail.get("policy"))
                == Some(&json!("consequence_mode_allow_with_audit_v0_1"))
    });

    assert_eq!(
        (
            r0["result"]["outcome"].clone(),
            r1["result"]["outcome"].clone(),
            r2["result"]["outcome"].clone(),
            pending["result"]["pending"].clone(),
            has_timeout_fallback,
            has_r2_consequence,
        ),
        (
            json!("allowed"),
            json!("allowed"),
            json!("allowed_with_audit"),
            json!([]),
            true,
            true,
        )
    );
}

#[test]
fn phase12_r3_notification_records_error_when_delivery_status_not_sent() {
    let mut runtime = runtime_for_gating_with_forced_failed_delivery();
    let evaluated = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc":"2.0",
            "id":"phase12-r3-delivery-failed",
            "method":"mobkit/gating/evaluate",
            "params":{
                "action":"publish_release",
                "actor_id":"alice",
                "risk_tier":"r3",
                "requested_approver":"bob",
                "approval_recipient":"approvals@example.com",
                "approval_channel":"transactional",
                "approval_timeout_ms":60_000
            }
        })
        .to_string(),
        Duration::from_secs(1),
    ));
    let pending = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-r3-delivery-failed-pending","method":"mobkit/gating/pending","params":{}}"#,
        Duration::from_secs(1),
    ));
    let audit = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-r3-delivery-failed-audit","method":"mobkit/gating/audit","params":{"limit":10}}"#,
        Duration::from_secs(1),
    ));
    let history = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-r3-delivery-failed-history","method":"mobkit/delivery/history","params":{"recipient":"approvals@example.com","limit":5}}"#,
        Duration::from_secs(1),
    ));
    runtime.shutdown();

    let entries = audit["result"]["entries"]
        .as_array()
        .expect("audit entries");
    let pending_created = entries
        .iter()
        .find(|entry| entry.get("event_type") == Some(&json!("pending_created")))
        .expect("pending_created entry");
    let notification_error = pending_created["detail"]["approval_notification_error"]
        .as_str()
        .expect("delivery failure should be explicit");
    let failed_delivery_id = history["result"]["deliveries"][0]["delivery_id"]
        .as_str()
        .expect("history delivery id");

    assert_eq!(evaluated["result"]["outcome"], json!("pending_approval"));
    assert_eq!(
        pending["result"]["pending"][0]["approval_delivery_id"],
        Value::Null
    );
    assert_eq!(
        pending_created["detail"]["approval_delivery_id"],
        Value::Null
    );
    assert!(notification_error.starts_with("delivery_status:failed:"));
    assert!(
        notification_error.ends_with(failed_delivery_id),
        "notification error should include failed delivery id"
    );
    assert_eq!(
        history["result"]["deliveries"][0]["status"],
        json!("failed")
    );
}

#[test]
fn phase12_r3_notification_records_error_when_modules_unavailable() {
    let mut runtime = runtime_for_gating();
    let evaluated = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc":"2.0",
            "id":"phase12-r3-notification-modules-missing",
            "method":"mobkit/gating/evaluate",
            "params":{
                "action":"publish_release",
                "actor_id":"alice",
                "risk_tier":"r3",
                "requested_approver":"bob",
                "approval_recipient":"approvals@example.com",
                "approval_channel":"transactional",
                "approval_timeout_ms":60_000
            }
        })
        .to_string(),
        Duration::from_secs(1),
    ));
    let pending = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-r3-notification-modules-missing-pending","method":"mobkit/gating/pending","params":{}}"#,
        Duration::from_secs(1),
    ));
    let audit = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase12-r3-notification-modules-missing-audit","method":"mobkit/gating/audit","params":{"limit":10}}"#,
        Duration::from_secs(1),
    ));
    runtime.shutdown();

    let entries = audit["result"]["entries"]
        .as_array()
        .expect("audit entries");
    let pending_created = entries
        .iter()
        .find(|entry| entry.get("event_type") == Some(&json!("pending_created")))
        .expect("pending_created entry");

    assert_eq!(evaluated["result"]["outcome"], json!("pending_approval"));
    assert_eq!(
        pending["result"]["pending"][0]["approval_route_id"],
        Value::Null
    );
    assert_eq!(
        pending["result"]["pending"][0]["approval_delivery_id"],
        Value::Null
    );
    assert_eq!(
        pending_created["detail"]["approval_notification_error"],
        json!("notification_modules_unavailable:router,delivery")
    );
}
