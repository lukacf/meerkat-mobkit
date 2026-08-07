//! Regression: the rpc_gateway stdin dispatch loop must serve RPC requests
//! CONCURRENTLY. A turn- or build-running RPC can block on a host callback
//! round-trip (`callback/build_agent`, `callback/call_tool`), and the host
//! may issue further RPCs from inside that callback (HomeCore issued
//! `mobkit/agent_memory/recall` from a callback tool handler). With the old
//! sequential loop those reentrant requests queued behind the blocked RPC
//! until the callback timed out.
//!
//! The test drives the real binary: it triggers a spawn whose build round-
//! trips `callback/build_agent`, withholds the callback response, and
//! asserts an interleaved `mobkit/status` still answers.
//!
//! The recall-specific regression goes further: it parks the build of ONE
//! identity on its unanswered callback and asserts
//! `mobkit/agent_memory/recall` for that SAME identity still answers through
//! the full runtime path (identity status read + memory provider). This is
//! byte-for-byte HomeCore's re-entrancy shape - the host calling back into
//! the gateway from inside `callback/build_agent` - and it must never queue
//! behind the parked build.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::uninlined_format_args
)]

use std::cell::Cell;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const MOB_CONFIG: &str = r#"
[mob]
id = "gateway-concurrent-dispatch-test"

[profiles.default]
model = "gpt-5.5"
external_addressable = true

[profiles.default.tools]
comms = true
"#;

/// Same mob, but the profile DECLARES workgraph. Used by the
/// declared-vs-resolved regression (activation-41): a declared category must
/// be present on the member's resolved tool surface.
const MOB_CONFIG_WORKGRAPH: &str = r#"
[mob]
id = "gateway-concurrent-dispatch-test"

[profiles.default]
model = "gpt-5.5"
external_addressable = true

[profiles.default.tools]
comms = true
workgraph = true
"#;

struct Gateway {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
}

impl Gateway {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rpc_gateway"))
            .arg("--persistent")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn rpc_gateway");
        let stdin = child.stdin.take().expect("gateway stdin");
        let stdout = child.stdout.take().expect("gateway stdout");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            lines: rx,
        }
    }

    fn send(&mut self, value: Value) {
        let stdin = self.stdin.as_mut().expect("gateway stdin remains open");
        writeln!(
            stdin,
            "{}",
            serde_json::to_string(&value).expect("request json")
        )
        .expect("write request");
        stdin.flush().expect("flush request");
    }

    fn close_stdin(&mut self) {
        drop(self.stdin.take());
    }

    fn wait_for_exit(&mut self, deadline: Duration) {
        let start = Instant::now();
        while start.elapsed() < deadline {
            if self.child.try_wait().expect("poll gateway exit").is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        panic!("rpc_gateway did not exit within {deadline:?}");
    }

    /// Read messages until `predicate` matches one; unmatched messages are
    /// handed to `on_other` (e.g. to auto-acknowledge unrelated callbacks).
    fn wait_for(
        &mut self,
        deadline: Duration,
        mut on_other: impl FnMut(&mut Self, &Value),
        predicate: impl Fn(&Value) -> bool,
    ) -> Option<Value> {
        let start = Instant::now();
        while start.elapsed() < deadline {
            let remaining = deadline.saturating_sub(start.elapsed());
            let Ok(line) = self.lines.recv_timeout(remaining) else {
                return None;
            };
            let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if predicate(&message) {
                return Some(message);
            }
            on_other(self, &message);
        }
        None
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn is_response_with_id(message: &Value, id: &str) -> bool {
    message.get("method").is_none() && message.get("id").and_then(Value::as_str) == Some(id)
}

fn is_callback_request(message: &Value, method: &str) -> bool {
    message.get("method").and_then(Value::as_str) == Some(method) && message.get("id").is_some()
}

fn initialize_for_shutdown(gateway: &mut Gateway, state_dir: &tempfile::TempDir) {
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "mobkit/init",
        "params": {
            "persistent_state": state_dir.path(),
            "mob_config": MOB_CONFIG
        }
    }));
    let init = gateway
        .wait_for(
            Duration::from_mins(1),
            |_, _| {},
            |m| is_response_with_id(m, "init"),
        )
        .expect("init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "init failed: {init}"
    );
    assert_eq!(init["result"]["stdio_shutdown_handshake"], true);
    assert_eq!(init["result"]["stdio_shutdown_horizon_ms"], 335_000);
}

fn answer_identity_provider_callback(
    gateway: &mut Gateway,
    message: &Value,
    release_seen: &Cell<bool>,
) {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return;
    };
    let Some(id) = message.get("id").cloned() else {
        return;
    };
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));

    let result = match method {
        "callback/roster_provider/roster" => json!([{
            "identity": "agent:alpha",
            "profile": "default",
            "addressability": "addressable",
            "display_name": null,
            "labels": {},
            "context": null,
            "additional_instructions": []
        }]),
        "callback/continuity_store/resolve_many" => {
            let mut states = serde_json::Map::new();
            for identity in params["identities"].as_array().into_iter().flatten() {
                let identity = identity.as_str().expect("identity string");
                states.insert(identity.to_string(), json!({ "state": "uninitialized" }));
            }
            Value::Object(states)
        }
        "callback/continuity_store/load_session_snapshot" => Value::Null,
        "callback/continuity_store/delete_session_snapshot_if_current_revision" => {
            Value::Bool(false)
        }
        "callback/continuity_store/save_session_snapshot"
        | "callback/continuity_store/upsert_continuity_record"
        | "callback/continuity_store/delete_continuity_record" => Value::Null,
        "callback/lease_provider/acquire_leases" => {
            let mut acquisitions = serde_json::Map::new();
            for identity in params["identities"].as_array().into_iter().flatten() {
                let identity = identity.as_str().expect("identity string");
                acquisitions.insert(
                    identity.to_string(),
                    json!({
                        "result": "acquired",
                        "identity": identity,
                        "fencing_token": 101,
                        "ttl": 600_000
                    }),
                );
            }
            Value::Object(acquisitions)
        }
        "callback/lease_provider/renew_leases" => {
            let mut renewals = serde_json::Map::new();
            for grant in params["grants"].as_array().into_iter().flatten() {
                let identity = grant["identity"].as_str().expect("grant identity");
                renewals.insert(
                    identity.to_string(),
                    json!({
                        "result": "renewed",
                        "identity": identity,
                        "fencing_token": grant["fencing_token"],
                        "ttl": grant["ttl"]
                    }),
                );
            }
            Value::Object(renewals)
        }
        "callback/lease_provider/release_leases" => {
            assert_eq!(
                params["grants"],
                json!([{
                    "identity": "agent:alpha",
                    "fencing_token": 101,
                    "ttl": 600_000
                }]),
                "shutdown must release the exact externally acquired grant"
            );
            release_seen.set(true);
            Value::Null
        }
        other => panic!("unexpected identity provider callback: {other}"),
    };
    gateway.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
}

#[test]
fn persistent_gateway_exits_after_stdin_eof() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let mut gateway = Gateway::start();
    initialize_for_shutdown(&mut gateway, &state_dir);

    gateway.close_stdin();
    gateway.wait_for_exit(Duration::from_secs(15));
}

#[test]
fn explicit_shutdown_keeps_callbacks_open_until_external_leases_are_released() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let scratch_dir = tempfile::tempdir().expect("scratch dir");
    let mut gateway = Gateway::start();
    let release_seen = Cell::new(false);

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "mobkit/init",
        "params": {
            "persistent_state": state_dir.path(),
            "mob_config": MOB_CONFIG,
            "has_roster_provider": true,
            "has_continuity_store": true,
            "has_lease_provider": true,
            "scratch_dir": scratch_dir.path(),
            "runtime_options": {
                "demo_llm": true,
                "identity_bootstrap_mode": { "mode": "eager_materialize" }
            }
        }
    }));
    let init = gateway
        .wait_for(
            Duration::from_mins(1),
            |gateway, message| {
                answer_identity_provider_callback(gateway, message, &release_seen);
            },
            |message| is_response_with_id(message, "init"),
        )
        .expect("identity-first init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "identity-first init failed: {init}"
    );
    assert_eq!(init["result"]["stdio_shutdown_handshake"], true);
    assert_eq!(init["result"]["stdio_shutdown_horizon_ms"], 335_000);
    assert!(
        !release_seen.get(),
        "bootstrap must retain its acquired lease"
    );

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "shutdown",
        "method": "mobkit/shutdown",
        "params": {}
    }));
    let shutdown = gateway
        .wait_for(
            Duration::from_mins(1),
            |gateway, message| {
                answer_identity_provider_callback(gateway, message, &release_seen);
            },
            |message| is_response_with_id(message, "shutdown"),
        )
        .expect("shutdown handshake response");

    assert_eq!(
        shutdown["result"]["shutdown"], true,
        "shutdown failed: {shutdown}"
    );
    assert!(
        release_seen.get(),
        "shutdown response arrived before the external lease release callback"
    );

    // Match SDK behavior: close stdin only after the gateway confirms runtime
    // cleanup, which also releases Tokio's blocking stdin helper.
    gateway.close_stdin();
    gateway.wait_for_exit(Duration::from_secs(15));
}

#[cfg(unix)]
#[test]
fn persistent_gateway_exits_after_sigint_with_stdin_open() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let mut gateway = Gateway::start();
    initialize_for_shutdown(&mut gateway, &state_dir);

    let status = Command::new("kill")
        .args(["-INT", &gateway.child.id().to_string()])
        .status()
        .expect("send SIGINT to rpc_gateway");
    assert!(status.success(), "kill -INT failed: {status}");
    gateway.wait_for_exit(Duration::from_mins(1));
}

#[test]
fn rpc_dispatch_serves_requests_while_a_callback_is_pending() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let mut gateway = Gateway::start();

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "mobkit/init",
        "params": {
            "persistent_state": state_dir.path(),
            "mob_config": MOB_CONFIG,
            "has_session_builder": true
        }
    }));
    let init = gateway
        .wait_for(
            Duration::from_mins(1),
            |_, _| {},
            |m| is_response_with_id(m, "init"),
        )
        .expect("init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "init failed: {init}"
    );

    // Trigger a member build; with has_session_builder the gateway round-trips
    // callback/build_agent and the spawn RPC blocks until we answer it.
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "spawn",
        "method": "mobkit/spawn_member",
        "params": { "profile": "default", "meerkat_id": "worker-1" }
    }));
    let build_callback = gateway
        .wait_for(
            Duration::from_mins(1),
            |_, _| {},
            |m| is_callback_request(m, "callback/build_agent"),
        )
        .expect("callback/build_agent request");
    let callback_id = build_callback["id"].clone();

    // The regression: with the callback UNANSWERED (the spawn handler is
    // parked), an interleaved request must still be served. The sequential
    // loop queued it behind the spawn until the callback timed out.
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "status",
        "method": "mobkit/status",
        "params": {}
    }));
    let status = gateway
        .wait_for(
            Duration::from_secs(15),
            |_, _| {},
            |m| is_response_with_id(m, "status"),
        )
        .expect("mobkit/status must answer while callback/build_agent is pending");
    assert!(
        status["result"]["contract_version"].is_string(),
        "status failed: {status}"
    );

    // Unblock the build and let the spawn finish (success or a build error —
    // either proves the handler was parked on the callback, not lost).
    // Auto-acknowledge any further callbacks (e.g. callback/after_create).
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": callback_id,
        "result": {}
    }));
    let spawn = gateway
        .wait_for(
            Duration::from_mins(1),
            |gateway, message| {
                if message.get("method").is_some()
                    && let Some(id) = message.get("id").cloned()
                {
                    gateway.send(json!({ "jsonrpc": "2.0", "id": id, "result": {} }));
                }
            },
            |m| is_response_with_id(m, "spawn"),
        )
        .expect("spawn response after callback reply");
    assert!(
        spawn.get("result").is_some() || spawn.get("error").is_some(),
        "spawn response malformed: {spawn}"
    );
}

/// Methods `answer_identity_provider_callback` knows how to answer with a
/// schema-correct result. Everything else gets a `null` acknowledgement so a
/// new optional callback verb cannot wedge this harness, EXCEPT
/// `callback/build_agent`, which the recall regression deliberately withholds.
const KNOWN_PROVIDER_CALLBACKS: &[&str] = &[
    "callback/roster_provider/roster",
    "callback/continuity_store/resolve_many",
    "callback/continuity_store/load_session_snapshot",
    "callback/continuity_store/delete_session_snapshot_if_current_revision",
    "callback/continuity_store/save_session_snapshot",
    "callback/continuity_store/upsert_continuity_record",
    "callback/continuity_store/delete_continuity_record",
    "callback/lease_provider/acquire_leases",
    "callback/lease_provider/renew_leases",
    "callback/lease_provider/release_leases",
];

fn answer_provider_callback_holding_builds(
    gateway: &mut Gateway,
    message: &Value,
    release_seen: &Cell<bool>,
) {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return;
    };
    if method == "callback/build_agent" {
        return;
    }
    if KNOWN_PROVIDER_CALLBACKS.contains(&method) {
        answer_identity_provider_callback(gateway, message, release_seen);
        return;
    }
    if let Some(id) = message.get("id").cloned() {
        gateway.send(json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null }));
    }
}

/// Declared-vs-resolved capability invariant, CREATE half (activation-41).
/// A callback-built member whose profile declares `tools.workgraph = true`
/// must expose `workgraph_*` on its resolved tool surface when the member is
/// CREATED (the create path threads profile.tools -> category overrides;
/// meerkat-mob build.rs:253). The RESUME half is the open upstream defect:
/// apply_resumed_session_metadata overwrites tool-category overrides from
/// creation-era metadata.tooling with no resume-override mask (build.rs:472,
/// profile.rs:260) - a resumed member created before the flag flip never
/// resolves the category. This test pins the half that works so the upstream
/// merge has a green baseline, and `mobkit/identity/resolved_tools` is the
/// instrument both fleets now gate on.
#[test]
fn callback_built_member_resolves_declared_workgraph_tools_on_create() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let scratch_dir = tempfile::tempdir().expect("scratch dir");
    let mut gateway = Gateway::start();
    let release_seen = Cell::new(false);

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "mobkit/init",
        "params": {
            "persistent_state": state_dir.path(),
            "mob_config": MOB_CONFIG_WORKGRAPH,
            "has_roster_provider": true,
            "has_continuity_store": true,
            "has_lease_provider": true,
            "has_session_builder": true,
            "scratch_dir": scratch_dir.path(),
            "runtime_options": {
                "demo_llm": true,
                "identity_bootstrap_mode": { "mode": "lazy_materialize" }
            }
        }
    }));
    let init = gateway
        .wait_for(
            Duration::from_mins(1),
            |gateway, message| {
                answer_provider_callback_holding_builds(gateway, message, &release_seen);
            },
            |m| is_response_with_id(m, "init"),
        )
        .expect("identity-first init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "identity-first init failed: {init}"
    );

    // Trigger the callback-built materialization (fresh member: CREATE path)
    // and answer the build callback immediately.
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "dispatch",
        "method": "mobkit/dispatch",
        "params": {
            "identity": "agent:alpha",
            "dispatch_input": { "content": "hello", "origin": "connector" }
        }
    }));
    let build_callback = gateway
        .wait_for(
            Duration::from_mins(1),
            |gateway, message| {
                answer_provider_callback_holding_builds(gateway, message, &release_seen);
            },
            |m| is_callback_request(m, "callback/build_agent"),
        )
        .expect("callback/build_agent request for agent:alpha");
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": build_callback["id"].clone(),
        "result": {}
    }));
    let dispatch = gateway
        .wait_for(
            Duration::from_mins(1),
            |gateway, message| {
                answer_provider_callback_holding_builds(gateway, message, &release_seen);
            },
            |m| is_response_with_id(m, "dispatch"),
        )
        .expect("dispatch response after callback reply");
    assert!(
        dispatch.get("result").is_some(),
        "dispatch must succeed so the member is Active with a live session: {dispatch}"
    );

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "tools",
        "method": "mobkit/identity/resolved_tools",
        "params": { "identity": "agent:alpha" }
    }));
    let resolved = gateway
        .wait_for(
            Duration::from_secs(30),
            |gateway, message| {
                answer_provider_callback_holding_builds(gateway, message, &release_seen);
            },
            |m| is_response_with_id(m, "tools"),
        )
        .expect("resolved_tools response");
    let tools: Vec<String> = resolved["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("resolved_tools must return a tools array: {resolved}"))
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    assert!(
        tools.iter().any(|name| name.starts_with("workgraph_")),
        "profile declares tools.workgraph = true, so a CREATED member's \
         resolved surface must carry workgraph_* (fail-open declared-vs-\
         resolved divergence, activation-41 class). Resolved: {tools:?}"
    );
}

/// HomeCore's exact re-entrancy shape (seam inventory row 6): the host calls
/// `mobkit/agent_memory/recall` from INSIDE a callback the gateway is waiting
/// on, for the very identity whose build is parked on that callback. The
/// recall must be served concurrently through the full runtime path
/// (identity status read + configured memory provider), not queue behind the
/// parked build until the callback deadline. This is the deadlock that drove
/// HomeCore to read the memory sqlite directly; it was fixed by the
/// concurrent dispatch loop (#260) and this test pins the recall path
/// specifically, locks and all.
#[test]
fn agent_memory_recall_answers_while_the_same_identity_build_is_parked() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let scratch_dir = tempfile::tempdir().expect("scratch dir");
    let mut gateway = Gateway::start();
    let release_seen = Cell::new(false);

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "mobkit/init",
        "params": {
            "persistent_state": state_dir.path(),
            "mob_config": MOB_CONFIG,
            "has_roster_provider": true,
            "has_continuity_store": true,
            "has_lease_provider": true,
            "has_session_builder": true,
            "scratch_dir": scratch_dir.path(),
            "runtime_options": {
                "demo_llm": true,
                "agent_memory": true,
                "identity_bootstrap_mode": { "mode": "lazy_materialize" }
            }
        }
    }));
    let init = gateway
        .wait_for(
            Duration::from_mins(1),
            |gateway, message| {
                answer_provider_callback_holding_builds(gateway, message, &release_seen);
            },
            |m| is_response_with_id(m, "init"),
        )
        .expect("identity-first init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "identity-first init failed: {init}"
    );

    // Lazy bootstrap registered agent:alpha without materializing it. This
    // dispatch triggers the callback-built materialization and parks the
    // dispatch RPC on the unanswered callback/build_agent.
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "dispatch",
        "method": "mobkit/dispatch",
        "params": {
            "identity": "agent:alpha",
            "dispatch_input": { "content": "hello", "origin": "connector" }
        }
    }));
    let build_callback = gateway
        .wait_for(
            Duration::from_mins(1),
            |gateway, message| {
                answer_provider_callback_holding_builds(gateway, message, &release_seen);
            },
            |m| is_callback_request(m, "callback/build_agent"),
        )
        .expect("callback/build_agent request for agent:alpha");
    let callback_id = build_callback["id"].clone();

    // THE regression: with agent:alpha's build parked (its lifecycle
    // authority held across the callback await), recall for that SAME
    // identity must still answer. A recall that queues behind the build - on
    // the dispatch loop or on any runtime lock the build holds - times this
    // wait out.
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "recall",
        "method": "mobkit/agent_memory/recall",
        "params": { "identity": "agent:alpha" }
    }));
    let recall = gateway
        .wait_for(
            Duration::from_secs(15),
            |gateway, message| {
                answer_provider_callback_holding_builds(gateway, message, &release_seen);
            },
            |m| is_response_with_id(m, "recall"),
        )
        .expect("agent_memory/recall must answer while callback/build_agent is pending");
    assert!(
        recall["result"]["records"].is_array(),
        "recall must succeed through the configured memory provider, not \
         merely fail fast: {recall}"
    );

    // Unblock the parked build and prove the dispatch was parked, not lost.
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": callback_id,
        "result": {}
    }));
    let dispatch = gateway
        .wait_for(
            Duration::from_mins(1),
            |gateway, message| {
                answer_provider_callback_holding_builds(gateway, message, &release_seen);
            },
            |m| is_response_with_id(m, "dispatch"),
        )
        .expect("dispatch response after callback reply");
    assert!(
        dispatch.get("result").is_some() || dispatch.get("error").is_some(),
        "dispatch response malformed: {dispatch}"
    );
}
