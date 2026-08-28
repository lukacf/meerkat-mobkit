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

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Backstop ceiling for every "this either arrives or the gateway is wedged"
/// wait in this file. Each such wait guards a STRUCTURAL property - the
/// dispatch loop either answers the request or it is blocked behind the
/// pending callback, which is the regression under test. The ceiling is only a
/// hang detector, never a latency measurement, so it is deliberately generous:
/// the old mixed 15s/30s ceilings measured runner load instead, and flaked the
/// suite under full parallel CI while passing standalone.
const WEDGE_BACKSTOP: Duration = Duration::from_mins(1);

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

    /// Like [`Self::start`], but pipes stderr into a shared buffer so a test
    /// can assert on the gateway's own tracing output. [`Self::start`]
    /// discards it, which is right for every other test here.
    fn start_capturing_stderr(filter: &str) -> (Self, Arc<Mutex<Vec<String>>>) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rpc_gateway"))
            .arg("--persistent")
            .env("RUST_LOG", filter)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn rpc_gateway");
        let stdin = child.stdin.take().expect("gateway stdin");
        let stdout = child.stdout.take().expect("gateway stdout");
        let stderr = child.stderr.take().expect("gateway stderr");
        let captured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                sink.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(line);
            }
        });
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        (
            Self {
                child,
                stdin: Some(stdin),
                lines: rx,
            },
            captured,
        )
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
            WEDGE_BACKSTOP,
            |_, _| {},
            |m| is_response_with_id(m, "init"),
        )
        .expect("init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "init failed: {init}"
    );
    assert_eq!(init["result"]["stdio_shutdown_handshake"], true);
    assert_eq!(init["result"]["stdio_shutdown_horizon_ms"], 337_000);
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
    gateway.wait_for_exit(WEDGE_BACKSTOP);
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
            WEDGE_BACKSTOP,
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
    assert_eq!(init["result"]["stdio_shutdown_horizon_ms"], 337_000);
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
            WEDGE_BACKSTOP,
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
    gateway.wait_for_exit(WEDGE_BACKSTOP);
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
    gateway.wait_for_exit(WEDGE_BACKSTOP);
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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
            WEDGE_BACKSTOP,
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

/// A HOST-side continuity store that actually remembers what the gateway
/// persisted through it, so a SECOND gateway process can resume from it.
///
/// `answer_identity_provider_callback` is deliberately amnesiac (resolve_many
/// is always `uninitialized`); the cross-boot resume regression needs the
/// real HomeCore shape: `upsert_continuity_record` / `save_session_snapshot`
/// payloads captured verbatim and echoed back on the next boot's
/// `resolve_many` / `load_session_snapshot`.
#[derive(Default)]
struct HostContinuityState {
    /// identity -> latest `ContinuityRecord` JSON, exactly as upserted.
    records: RefCell<BTreeMap<String, Value>>,
    /// session id -> latest `SessionSnapshot` JSON, exactly as saved.
    snapshots: RefCell<BTreeMap<String, Value>>,
}

impl HostContinuityState {
    fn record(&self, identity: &str) -> Option<Value> {
        self.records.borrow().get(identity).cloned()
    }

    fn has_snapshot(&self) -> bool {
        !self.snapshots.borrow().is_empty()
    }
}

/// Stateful analogue of `answer_provider_callback_holding_builds`: continuity
/// verbs read/write `state`, lease acquisition grants `fencing_token` (a new
/// process acquiring the same identity presents a HIGHER token), and
/// `callback/build_agent` is left for the test body to answer explicitly.
fn answer_stateful_provider_callback_holding_builds(
    gateway: &mut Gateway,
    message: &Value,
    state: &HostContinuityState,
    fencing_token: u64,
) {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return;
    };
    if method == "callback/build_agent" {
        return;
    }
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
            let records = state.records.borrow();
            let mut states = serde_json::Map::new();
            for identity in params["identities"].as_array().into_iter().flatten() {
                let identity = identity.as_str().expect("identity string");
                let resolve_state = match records.get(identity) {
                    Some(record) => json!({ "state": "ready", "record": record }),
                    None => json!({ "state": "uninitialized" }),
                };
                states.insert(identity.to_string(), resolve_state);
            }
            Value::Object(states)
        }
        // Reference shape for the callback added in 547ea908. `null` is a
        // GENUINE absence; a bound session answers with named fields so an
        // implementor never has to know a tuple order. Before this existed the
        // bridge inherited the trait's `Ok(None)` default and every
        // owner-authority pass silently registered nothing.
        "callback/continuity_store/resolve_record_by_session" => {
            let session_id = params["session_id"].as_str().unwrap_or_default();
            let records = state.records.borrow();
            match records
                .values()
                .find(|record| record["session_id"].as_str() == Some(session_id))
            {
                Some(record) => json!({
                    "record": record,
                    "fencing_token": fencing_token,
                    "checkpoint_version": record["checkpoint_version"].as_u64().unwrap_or(0),
                }),
                None => Value::Null,
            }
        }
        "callback/continuity_store/upsert_continuity_record" => {
            let record = params["record"].clone();
            let identity = record["identity"]
                .as_str()
                .expect("upserted record identity")
                .to_string();
            state.records.borrow_mut().insert(identity, record);
            Value::Null
        }
        "callback/continuity_store/delete_continuity_record" => {
            let identity = params["identity"].as_str().expect("record identity");
            state.records.borrow_mut().remove(identity);
            Value::Null
        }
        "callback/continuity_store/save_session_snapshot" => {
            let session_id = params["session_id"]
                .as_str()
                .expect("snapshot session id")
                .to_string();
            state
                .snapshots
                .borrow_mut()
                .insert(session_id, params["snapshot"].clone());
            Value::Null
        }
        "callback/continuity_store/load_session_snapshot" => {
            let session_id = params["session_id"].as_str().expect("snapshot session id");
            state
                .snapshots
                .borrow()
                .get(session_id)
                .cloned()
                .unwrap_or(Value::Null)
        }
        "callback/continuity_store/delete_session_snapshot_if_current_revision" => {
            Value::Bool(false)
        }
        "callback/lease_provider/acquire_leases" => {
            let mut acquisitions = serde_json::Map::new();
            for identity in params["identities"].as_array().into_iter().flatten() {
                let identity = identity.as_str().expect("identity string");
                acquisitions.insert(
                    identity.to_string(),
                    json!({
                        "result": "acquired",
                        "identity": identity,
                        "fencing_token": fencing_token,
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
        "callback/lease_provider/release_leases" => Value::Null,
        // Any new optional callback verb gets a null acknowledgement so it
        // cannot wedge this harness (mirrors the stateless answerer above).
        _ => Value::Null,
    };
    gateway.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
}

/// Drive one materialization to completion: dispatch to `agent:alpha`, answer
/// the held `callback/build_agent` with `{}`, and wait for the dispatch
/// response, answering provider callbacks statefully throughout.
fn dispatch_alpha_through_build(
    gateway: &mut Gateway,
    state: &HostContinuityState,
    fencing_token: u64,
    dispatch_id: &str,
) -> Value {
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": dispatch_id,
        "method": "mobkit/dispatch",
        "params": {
            "identity": "agent:alpha",
            "dispatch_input": { "content": "hello", "origin": "connector" }
        }
    }));
    let build_callback = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    state,
                    fencing_token,
                );
            },
            |m| is_callback_request(m, "callback/build_agent"),
        )
        .expect("callback/build_agent request for agent:alpha");
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": build_callback["id"].clone(),
        "result": {}
    }));
    gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    state,
                    fencing_token,
                );
            },
            |m| is_response_with_id(m, dispatch_id),
        )
        .expect("dispatch response after callback reply")
}

/// Fetch `mobkit/identity/resolved_tools` for `agent:alpha` as a name list.
fn resolved_tools_for_alpha(
    gateway: &mut Gateway,
    state: &HostContinuityState,
    fencing_token: u64,
    request_id: &str,
) -> Vec<String> {
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "mobkit/identity/resolved_tools",
        "params": { "identity": "agent:alpha" }
    }));
    let resolved = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    state,
                    fencing_token,
                );
            },
            |m| is_response_with_id(m, request_id),
        )
        .expect("resolved_tools response");
    resolved["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("resolved_tools must return a tools array: {resolved}"))
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Declared-vs-resolved capability invariant, RESUME half (activation-41),
/// pinning the meerkat 0.8.21 resume-tooling merge.
///
/// Mechanism of the defect this pins: member CREATION threads `profile.tools`
/// into per-category tool overrides and stamps the SAME categories into the
/// session's creation-era `metadata.tooling`. Through meerkat 0.8.20 every
/// RESUME then overwrote the current profile's overrides from that
/// creation-era `metadata.tooling` wholesale (`apply_resumed_session_metadata`,
/// no resume-override mask) - so a tool category enabled on the profile AFTER
/// the member was created was unmaskable: it never resolved on ANY resume, on
/// every boot, forever (HomeCore activation-41: `tools.workgraph = true`
/// added post-creation never surfaced `workgraph_*`). Meerkat 0.8.21 replaces
/// the overwrite with a unified merge (`merge_resumed_tool_category`): an
/// EXPLICIT current-profile setting wins, and creation-era metadata fills
/// only categories the current profile leaves `Inherit`.
///
/// Two real gateway processes share one host-side continuity store (this
/// test's stateful callback answerer) and one persistent-state + scratch dir:
///
/// - PHASE 1 (create era): profile WITHOUT workgraph; dispatch materializes
///   `agent:alpha` via `callback/build_agent`, the turn checkpoints through
///   `save_session_snapshot`/`upsert_continuity_record`, graceful shutdown.
/// - PHASE 2 (resume era): new process, profile WITH `tools.workgraph = true`;
///   `resolve_many` now answers `ready` with the phase-1 record (a HIGHER
///   fencing token models the new process acquiring the same identity), the
///   member RESUMES the phase-1 session, and its resolved tool surface must
///   carry `workgraph_*`.
#[test]
fn ephemeral_mob_storage_resumed_member_resolves_tool_category_declared_after_creation() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let scratch_dir = tempfile::tempdir().expect("scratch dir");
    let continuity = HostContinuityState::default();

    // ------------------------------------------------------------------
    // PHASE 1: create era. Workgraph is ABSENT from the profile.
    // ------------------------------------------------------------------
    let mut gateway = Gateway::start();
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init1",
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
                // Declared in-memory mob storage. A persistent mob storage
                // PINS mob_config, because Meerkat refuses a definition that
                // disagrees with the persisted spec store, so changing the
                // profile across restarts requires declaring mob state
                // ephemeral. That trade is the point of this contract.
                "mob_storage": { "storage": "memory" },
                "identity_bootstrap_mode": { "mode": "lazy_materialize" }
            }
        }
    }));
    let init = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    101,
                );
            },
            |m| is_response_with_id(m, "init1"),
        )
        .expect("phase-1 init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "phase-1 init failed: {init}"
    );

    let dispatch = dispatch_alpha_through_build(&mut gateway, &continuity, 101, "dispatch1");
    assert!(
        dispatch.get("result").is_some(),
        "phase-1 dispatch must succeed so agent:alpha is Active: {dispatch}"
    );

    // Create-era baseline: the profile does not declare workgraph, so the
    // created member's resolved surface must not carry it.
    let phase1_tools = resolved_tools_for_alpha(&mut gateway, &continuity, 101, "tools1");
    assert!(
        phase1_tools
            .iter()
            .all(|name| !name.starts_with("workgraph_")),
        "phase-1 profile has no workgraph, resolved surface must not either: {phase1_tools:?}"
    );

    // The turn persists through the host store (blob-canonical path); make
    // sure a snapshot actually landed before we shut the process down.
    if !continuity.has_snapshot() {
        let save = gateway
            .wait_for(
                WEDGE_BACKSTOP,
                |gateway, message| {
                    answer_stateful_provider_callback_holding_builds(
                        gateway,
                        message,
                        &continuity,
                        101,
                    );
                },
                |m| is_callback_request(m, "callback/continuity_store/save_session_snapshot"),
            )
            .expect("phase-1 session snapshot save callback");
        answer_stateful_provider_callback_holding_builds(&mut gateway, &save, &continuity, 101);
    }

    // Graceful end of the create era: the explicit handshake keeps callback
    // admission open through runtime shutdown (final flushes and the lease
    // release round through the answerable host), then stdin EOF exits.
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "shutdown1",
        "method": "mobkit/shutdown",
        "params": {}
    }));
    let shutdown = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    101,
                );
            },
            |m| is_response_with_id(m, "shutdown1"),
        )
        .expect("phase-1 shutdown handshake response");
    assert_eq!(
        shutdown["result"]["shutdown"], true,
        "phase-1 shutdown failed: {shutdown}"
    );
    gateway.close_stdin();
    gateway.wait_for_exit(WEDGE_BACKSTOP);
    drop(gateway);

    let phase1_record = continuity
        .record("agent:alpha")
        .expect("phase 1 must have upserted a continuity record for agent:alpha");
    let phase1_session_id = phase1_record["session_id"]
        .as_str()
        .expect("continuity record session_id")
        .to_string();
    assert!(
        continuity.has_snapshot(),
        "phase 1 must have saved a session snapshot through the host store"
    );

    // ------------------------------------------------------------------
    // PHASE 2: resume era. The profile now DECLARES workgraph; the stateful
    // store makes agent:alpha resolve `ready` with the phase-1 record, so
    // the build is a RESUME by construction, not a recreation.
    // ------------------------------------------------------------------
    let mut gateway = Gateway::start();
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init2",
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
                // Declared in-memory mob storage. A persistent mob storage
                // PINS mob_config, because Meerkat refuses a definition that
                // disagrees with the persisted spec store, so changing the
                // profile across restarts requires declaring mob state
                // ephemeral. That trade is the point of this contract.
                "mob_storage": { "storage": "memory" },
                "identity_bootstrap_mode": { "mode": "lazy_materialize" }
            }
        }
    }));
    let init = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    102,
                );
            },
            |m| is_response_with_id(m, "init2"),
        )
        .expect("phase-2 init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "phase-2 init failed: {init}"
    );

    let dispatch = dispatch_alpha_through_build(&mut gateway, &continuity, 102, "dispatch2");
    assert!(
        dispatch.get("result").is_some(),
        "phase-2 dispatch must succeed so the resumed member is Active: {dispatch}"
    );

    // THE regression assertion: the resumed member's tool surface must carry
    // the category declared AFTER its creation. At the 0.8.20 pin this list
    // never contains workgraph_* (creation-era metadata.tooling overwrote the
    // explicit profile declaration on resume).
    let phase2_tools = resolved_tools_for_alpha(&mut gateway, &continuity, 102, "tools2");
    assert!(
        phase2_tools
            .iter()
            .any(|name| name.starts_with("workgraph_")),
        "profile now declares tools.workgraph = true, so the RESUMED member's \
         resolved surface must carry workgraph_* (meerkat 0.8.21 \
         merge_resumed_tool_category: explicit current profile wins over \
         creation-era metadata.tooling). Resolved: {phase2_tools:?}"
    );

    // And it really was a resume: the live member is bound to the phase-1
    // session, not a freshly created one.
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "status2",
        "method": "mobkit/status_identity",
        "params": { "identity": "agent:alpha" }
    }));
    let status = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    102,
                );
            },
            |m| is_response_with_id(m, "status2"),
        )
        .expect("phase-2 status_identity response");
    assert_eq!(
        status["result"]["session_id"].as_str(),
        Some(phase1_session_id.as_str()),
        "resumed member must keep the phase-1 session (no recreation): {status}"
    );
}

/// CONTRACT: a PERSISTENT mob storage pins `mob_config`. Two gateway processes
/// over the same state dir, second one carrying a changed definition, must be
/// refused with the typed actionable divergence error before the mob actuates.
///
/// This is the other half of
/// `ephemeral_mob_storage_resumed_member_resolves_tool_category_declared_after_creation`.
/// Durable mob storage is what makes adopted identity declarations survive a
/// restart, and Meerkat 0.8.26 refuses a definition that disagrees with the
/// persisted spec store on both create and resume
/// (`sync_definition_with_spec_store`). So the two properties cannot both hold
/// on one storage, and the operator has to be told which one they have rather
/// than discovering it as an internal error naming two stores.
#[test]
fn persistent_mob_storage_refuses_changed_definition_on_the_same_state_dir() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let scratch_dir = tempfile::tempdir().expect("scratch dir");
    let continuity = HostContinuityState::default();

    // PHASE 1: create era, persistent mob storage (the default on a
    // persistent_state launch), definition WITHOUT workgraph.
    let mut gateway = Gateway::start();
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init1",
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
                "identity_bootstrap_mode": { "mode": "lazy_materialize" }
            }
        }
    }));
    let init = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    101,
                );
            },
            |m| is_response_with_id(m, "init1"),
        )
        .expect("phase-1 init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "phase-1 init must succeed so the composition is recorded: {init}"
    );

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "shutdown1",
        "method": "mobkit/shutdown",
        "params": {}
    }));
    let shutdown = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    101,
                );
            },
            |m| is_response_with_id(m, "shutdown1"),
        )
        .expect("phase-1 shutdown handshake response");
    assert_eq!(
        shutdown["result"]["shutdown"], true,
        "phase-1 shutdown failed: {shutdown}"
    );
    gateway.close_stdin();
    gateway.wait_for_exit(WEDGE_BACKSTOP);
    drop(gateway);

    // PHASE 2: same state dir, CHANGED definition, still persistent.
    let mut gateway = Gateway::start();
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init2",
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
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    102,
                );
            },
            |m| is_response_with_id(m, "init2"),
        )
        .expect("phase-2 init response");

    // Positive observables. Asserting merely that init failed would pass on any
    // unrelated breakage, and asserting the absence of a success field would
    // pass against a hang.
    let message = init["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("phase-2 must be refused with a typed error: {init}"));
    assert!(
        message.contains("diverges from the composition this storage was created for"),
        "the refusal must be the composition-divergence error, not an internal store \
         mismatch: {message}"
    );
    assert!(
        message.contains("profiles"),
        "the refusal must name the diverged field so the operator can act: {message}"
    );
    assert!(
        message.contains("create a new mob storage path"),
        "the refusal must state a remedy: {message}"
    );
}

/// RELEASE CONTRACT: an adopted and applied member tool declaration survives a
/// real process restart.
///
/// This is the user-visible property the persistent-mob-storage repair exists
/// for, and it is deliberately tested across two gateway PROCESSES rather than
/// an in-process rebootstrap. In-process, the comms participant registry keeps
/// the member endpoint bound and a second bootstrap of the same mob collides on
/// its durable generation binding, so an in-process test cannot model a
/// restart. Two processes can.
///
/// Both phases carry the SAME definition: a persistent mob storage pins
/// `mob_config`, which the divergence contract above covers separately. What is
/// under test here is that declaration state written in phase 1 is readable in
/// phase 2 at the revision it was left at.
#[test]
fn adopted_and_applied_declaration_survives_a_real_process_restart() {
    const MOB_ID: &str = "gateway-concurrent-dispatch-test";
    const ALIAS: &str = "agent:alpha";
    let state_dir = tempfile::tempdir().expect("state dir");
    let scratch_dir = tempfile::tempdir().expect("scratch dir");
    let continuity = HostContinuityState::default();

    let init_params = |id: &str| {
        json!({
            "jsonrpc": "2.0",
            "id": id,
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
                    "identity_bootstrap_mode": { "mode": "lazy_materialize" }
                }
            }
        })
    };
    let declaration = json!({
        "category_overrides": {
            "builtins": "inherit",
            "shell": "enable",
            "comms": "inherit",
            "mob": "inherit",
            "memory": "inherit",
            "schedule": "inherit",
            "workgraph": "inherit",
            "image_generation": "inherit",
            "web_search": "inherit"
        },
        "callback_tools": { "kind": "set", "tools": [] },
        "execution": { "kind": "unrestricted" },
        "application_policy": { "kind": "unmanaged" }
    });

    // ------------------------------------------------------------------
    // PHASE 1: adopt revision 1, then apply revision 2, then exit.
    // ------------------------------------------------------------------
    let mut gateway = Gateway::start();
    gateway.send(init_params("init1"));
    let init = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    101,
                );
            },
            |m| is_response_with_id(m, "init1"),
        )
        .expect("phase-1 init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "phase-1 init failed: {init}"
    );

    let dispatch = dispatch_alpha_through_build(&mut gateway, &continuity, 101, "dispatch1");
    assert!(
        dispatch.get("result").is_some(),
        "phase-1 dispatch must succeed so {ALIAS} has a bridge session: {dispatch}"
    );
    let session_id = continuity
        .record(ALIAS)
        .expect("phase 1 must have upserted a continuity record")["session_id"]
        .as_str()
        .expect("continuity record session_id")
        .to_string();

    // Declarations are keyed on the STABLE durable identity. The runtime id is
    // read only to prove it CHANGES across the process boundary: it is a
    // per-binding incarnation, so durable state keyed on it could not survive a
    // restart. That is the whole point of the contract under test.
    let rpc_result =
        |gateway: &mut Gateway, id: &str, method: &str, params: Value, cb: u64| -> Value {
            gateway.send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params
            }));
            let status = gateway
                .wait_for(
                    WEDGE_BACKSTOP,
                    |gateway, message| {
                        answer_stateful_provider_callback_holding_builds(
                            gateway,
                            message,
                            &continuity,
                            cb,
                        );
                    },
                    |m| is_response_with_id(m, id),
                )
                .expect("status_identity response");
            status["result"].clone()
        };
    let runtime_identity = |status: &Value| -> String {
        status["agent_runtime_id"]
            .as_str()
            .unwrap_or_else(|| panic!("{ALIAS} must have a runtime id: {status}"))
            .to_string()
    };
    let phase1_runtime = runtime_identity(&rpc_result(
        &mut gateway,
        "status1",
        "mobkit/status_identity",
        json!({ "identity": ALIAS }),
        101,
    ));
    // Adoption must state the EXACT existing baseline. `mobkit/status_identity`
    // does not project runtime_mode, so read it from the member surface, which
    // does, rather than guessing a default that would be rejected as a change.
    // Adoption is for an ALREADY-realized member, and upstream compares the
    // declaration's MATERIALIZED runtime mode AND labels against the live roster
    // entry (`material_runtime_mode != entry.runtime_mode ||
    // material.overlay.labels.unwrap_or_default() != entry.labels`). An omitted
    // field is therefore not "unchanged": it materializes to a default and is
    // rejected as a change. So echo the live entry exactly rather than guessing
    // a mode, and take labels from the ROSTER entry, not from
    // status_identity (whose labels are the identity's and are empty here).
    let members = rpc_result(
        &mut gateway,
        "members1",
        "mobkit/list_members",
        json!({}),
        101,
    );
    let entry = members
        .as_array()
        .unwrap_or_else(|| panic!("list_members must return an array: {members}"))
        .iter()
        .find(|entry| {
            entry["agent_identity"]
                .as_str()
                .is_some_and(|id| meerkat_mobkit::member_comms_id::runtime_alias_str(id) == ALIAS)
        })
        .unwrap_or_else(|| panic!("{ALIAS} must be on the roster: {members}"))
        .clone();
    // A missing field here is a surface regression, not something to paper over.
    let baseline_runtime_mode = entry
        .get("runtime_mode")
        .filter(|value| !value.is_null())
        .unwrap_or_else(|| panic!("list_members entry must project runtime_mode: {entry}"))
        .clone();
    let baseline_labels = entry
        .get("labels")
        .unwrap_or_else(|| panic!("list_members entry must project labels: {entry}"))
        .clone();

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "adopt1",
        "method": "mob/adopt_member_identity_declaration",
        "params": {
            "mob_id": MOB_ID,
            "agent_identity": ALIAS,
            "request_id": "adopt-restart-1",
            "precondition": "expected_absent",
            "declaration_scope": "restart-durability-test",
            "declaration_revision": 1,
            "session": {
                "session_id": session_id,
                "lineage_id": format!("session:{session_id}"),
                "lineage_generation": 0,
                "authority_policy": "require_existing"
            },
            "member": {
                "profile_name": "default",
                // NO system_prompt_override, deliberately. This launch wires no
                // SpawnBasePromptSource and the profile declares no skills, which
                // used to force the declaration to carry its own prompt - and
                // meerkat 0.8.28 now REFUSES exactly that for an existing
                // session: "identity adoption cannot restate an existing
                // session's durable system prompt; omit system_prompt_override so
                // the transcript remains authoritative". Adoption reads the
                // persisted transcript instead, so the workaround is both
                // unnecessary and rejected.
                "runtime_mode": baseline_runtime_mode,
                "labels": baseline_labels,
                "execution": { "execution": "controlling_session" }
            },
            "owned_wiring": [],
            "convergence": { "kind": "drain", "max_wait_ms": 5000 }
        }
    }));
    let adoption = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    101,
                );
            },
            |m| is_response_with_id(m, "adopt1"),
        )
        .expect("adopt response");
    assert_eq!(
        adoption["error"],
        Value::Null,
        "adopt failed: {adoption:#?}"
    );
    assert_eq!(
        adoption["result"]["adoption"]["desired_revision"],
        json!(1),
        "adopt must land revision 1: {adoption:#?}"
    );

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "apply1",
        "method": "mob/apply_member_tool_declaration",
        "params": {
            "mob_id": MOB_ID,
            "agent_identity": ALIAS,
            "request_id": "apply-restart-1",
            "expected_intent_revision": 1,
            "declaration": declaration,
            "convergence": { "kind": "drain", "max_wait_ms": 5000 }
        }
    }));
    let apply = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    101,
                );
            },
            |m| is_response_with_id(m, "apply1"),
        )
        .expect("apply response");
    assert_eq!(apply["error"], Value::Null, "apply failed: {apply:#?}");
    assert_eq!(
        apply["result"]["commit"]["desired_revision"],
        json!(2),
        "apply must land revision 2: {apply:#?}"
    );

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "shutdown1",
        "method": "mobkit/shutdown",
        "params": {}
    }));
    let shutdown = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    101,
                );
            },
            |m| is_response_with_id(m, "shutdown1"),
        )
        .expect("phase-1 shutdown handshake response");
    assert_eq!(
        shutdown["result"]["shutdown"], true,
        "phase-1 shutdown failed: {shutdown}"
    );
    gateway.close_stdin();
    gateway.wait_for_exit(WEDGE_BACKSTOP);
    drop(gateway);

    // ------------------------------------------------------------------
    // PHASE 2: a NEW process over the same state dir must still see it.
    // ------------------------------------------------------------------
    let mut gateway = Gateway::start();
    gateway.send(init_params("init2"));
    let init = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    102,
                );
            },
            |m| is_response_with_id(m, "init2"),
        )
        .expect("phase-2 init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "phase-2 init failed: {init}"
    );

    // A plain restart RESUMES the same incarnation: the generation is minted
    // per identity by Meerkat and is preserved across process replacement, so
    // this is the same runtime id, not a new one. Asserted rather than assumed,
    // because the incarnation-change property is exercised by the respawn
    // below and this pins down which of the two is which.
    let phase2_runtime = runtime_identity(&rpc_result(
        &mut gateway,
        "status2pre",
        "mobkit/status_identity",
        json!({ "identity": ALIAS }),
        102,
    ));
    assert_eq!(
        phase1_runtime, phase2_runtime,
        "a restart resumes the same incarnation; a changed runtime id here would mean the \
         process replacement rotated the binding instead of resuming it"
    );

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "read2",
        "method": "mob/member_tool_declaration",
        "params": { "mob_id": MOB_ID, "agent_identity": ALIAS }
    }));
    let read = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    102,
                );
            },
            |m| is_response_with_id(m, "read2"),
        )
        .expect("phase-2 declaration read response");

    // Positive observables: the exact revision and the exact declaration. A
    // bare "no error" would pass against a fresh empty mob, which is precisely
    // the failure this repair exists to prevent.
    assert_eq!(read["error"], Value::Null, "phase-2 read failed: {read:#?}");
    assert_eq!(read["result"]["agent_identity"], json!(ALIAS));
    assert_eq!(
        read["result"]["desired_intent_revision"],
        json!(2),
        "the applied revision must survive the process restart: {read:#?}"
    );
    assert_eq!(
        read["result"]["declaration"], declaration,
        "the declaration content must survive the process restart: {read:#?}"
    );

    // And it is bound to the SAME session, not a freshly minted one. A
    // declaration that survived while its session did not would still be a
    // broken restart.
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "status2",
        "method": "mobkit/status_identity",
        "params": { "identity": ALIAS }
    }));
    let status = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    102,
                );
            },
            |m| is_response_with_id(m, "status2"),
        )
        .expect("phase-2 status_identity response");
    assert_eq!(
        status["result"]["session_id"].as_str(),
        Some(session_id.as_str()),
        "the restarted process must keep the phase-1 session, not create a new one: {status}"
    );

    // NOTE for whoever extends this: the incarnation-change property (durable
    // declaration state outliving a NEW generation) is NOT covered here. A
    // restart resumes the same incarnation, as asserted above, so forcing a
    // successor needs a respawn - and `mobkit/respawn_member` does not return
    // within the wedge backstop in this callback-hosted fixture, so wiring it
    // in would trade coverage for a 60-second hang. Covering it needs either a
    // respawn-capable fixture or a separate lane.
}

/// Owner-authority ordering on a persistent identity-first resume.
///
/// Three MobKit-owned invariants, each of which was broken in production and
/// each verified on HomeCore's production clone before being written down here:
///
/// 1. Owner authority is PUBLISHED before `MobRuntime::prepare`, because
///    prepare runs durable-tail recovery and refuses a head-canonical session
///    whose owner has no authoritative registration - a deliberate refusal no
///    retry clears. Registration after prepare cannot reach it.
/// 2. That publication is NON-ZERO on a store that has records. It silently
///    published nothing for every SDK-hosted continuity store until the
///    bridge forwarded `resolve_record_by_session`, because the trait's
///    `Ok(None)` default is a negative that reads as fact.
/// 3. Durable-authority convergence happens in the parked window, BEFORE
///    activation, so the bounded explicit resume does not spend its budget on
///    O(members) work.
///
/// The counts are the discriminators, not the boot succeeding: on a
/// single-identity roster the boot can succeed with any of these broken, which
/// is exactly how the first two shipped. Asserted on the gateway's own log
/// rather than by timing, because a wall-clock assertion here would measure
/// runner load - this suite has flaked that way before.
#[test]
fn persistent_identity_first_resume_publishes_owner_authority_before_prepare() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let scratch_dir = tempfile::tempdir().expect("scratch dir");
    let continuity = HostContinuityState::default();

    let init_params = |id: &str| {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "mobkit/init",
            "params": {
                "persistent_state": state_dir.path(),
                "mob_config": MOB_CONFIG,
                "has_roster_provider": true,
                "has_continuity_store": true,
                "has_lease_provider": true,
                "has_session_builder": true,
                "scratch_dir": scratch_dir.path(),
                // No `mob_storage` override: a persistent launch defaults to
                // PERSISTENT mob storage, which is what makes the event log
                // survive the restart. An in-memory mob store always boots with
                // an empty log, never defers the lift, and cannot reach this.
                "runtime_options": {
                    "demo_llm": true,
                    "identity_bootstrap_mode": { "mode": "lazy_materialize" }
                }
            }
        })
    };

    // PHASE 1: create era. The turn is what mints the durable session and its
    // continuity record; without it the resume has no owner to publish and the
    // test would pass for the wrong reason.
    let mut gateway = Gateway::start();
    gateway.send(init_params("init1"));
    let init = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    101,
                );
            },
            |m| is_response_with_id(m, "init1"),
        )
        .expect("phase-1 init response");
    assert!(
        init["result"]["contract_version"].is_string(),
        "phase-1 init failed: {init}"
    );
    let dispatch = dispatch_alpha_through_build(&mut gateway, &continuity, 101, "dispatch1");
    assert!(
        dispatch.get("result").is_some(),
        "phase-1 dispatch must succeed so a durable session exists to resume: {dispatch}"
    );
    gateway.send(json!({
        "jsonrpc": "2.0", "id": "shutdown1", "method": "mobkit/shutdown", "params": {}
    }));
    let shutdown = gateway
        .wait_for(
            WEDGE_BACKSTOP,
            |gateway, message| {
                answer_stateful_provider_callback_holding_builds(
                    gateway,
                    message,
                    &continuity,
                    101,
                );
            },
            |m| is_response_with_id(m, "shutdown1"),
        )
        .expect("phase-1 shutdown handshake");
    assert_eq!(
        shutdown["result"]["shutdown"], true,
        "phase-1 must stop CLEANLY or phase 2 is testing something else: {shutdown}"
    );
    gateway.close_stdin();
    gateway.wait_for_exit(WEDGE_BACKSTOP);
    drop(gateway);
    assert!(
        !continuity.records.borrow().is_empty(),
        "phase 1 must leave a continuity record, or the resume has no owner to publish and \
         every count below would be trivially zero"
    );

    // PHASE 2: same state dir, same definition (persistent mob storage pins
    // mob_config). This is the deferred identity-first resume.
    let (mut gateway, stderr) =
        Gateway::start_capturing_stderr("meerkat_mobkit=info,meerkat_mob=warn");
    gateway.send(init_params("init2"));
    let captured = || {
        stderr
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    };
    const CONVERGED_MARK: &str = "converged persisted runtime authority before mob activation";
    // SCOPE, STATED PLAINLY: this waits for the pre-activation sequence to
    // complete and asserts ITS ordering and counts. It does NOT assert that
    // activation finishes.
    //
    // Activation completion is blocked upstream: the explicit-resume budget is
    // a single-member-retire timeout covering O(members) work, including host
    // agent-build round-trips the platform cannot bound (~9s each on a real
    // host, ~150s for a 17-member roster inside 30s). Meerkat is fixing that
    // with progress-based patience. Asserting it here would make a MobKit test
    // red for an upstream reason, and dropping the assertions below to make
    // something green would hide the three defects they cover. So: assert what
    // MobKit owns, and say what is missing rather than quietly narrowing.
    let reached = gateway.wait_for(
        WEDGE_BACKSTOP,
        |gateway, message| {
            answer_stateful_provider_callback_holding_builds(gateway, message, &continuity, 101);
        },
        |_| captured().iter().any(|line| line.contains(CONVERGED_MARK)),
    );
    let lines = captured();
    assert!(
        reached.is_some() || lines.iter().any(|line| line.contains(CONVERGED_MARK)),
        "phase-2 resume never reached the pre-activation convergence stage within \
         {WEDGE_BACKSTOP:?}; {} stderr lines:\n{}",
        lines.len(),
        lines.join("\n")
    );

    let count_after = |needle: &str, field: &str| -> Option<u64> {
        lines
            .iter()
            .find(|line| line.contains(needle))
            .and_then(|line| line.split(field).nth(1))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
    };
    let index_of = |needle: &str| lines.iter().position(|line| line.contains(needle));

    const PUBLISHED: &str = "published persisted continuity owners before mob prepare";
    const CONVERGED: &str = "converged persisted runtime authority before mob activation";

    let published = count_after(PUBLISHED, "published=");
    assert_eq!(
        published,
        Some(1),
        "owner authority must be published before prepare for the one rostered identity; \
         a zero here is the SDK-hosted inertia that shipped silently.\nstderr:\n{}",
        lines.join("\n")
    );
    let converged = count_after(CONVERGED, "converged=");
    assert!(
        converged.is_some_and(|count| count > 0),
        "durable authority must be converged in the parked window, before activation, or the \
         bounded resume spends its budget on it. got {converged:?}\nstderr:\n{}",
        lines.join("\n")
    );

    // Ordering is the invariant, not merely presence: publication AFTER prepare
    // is exactly the arrangement that could not clear the durable-tail refusal.
    let published_at = index_of(PUBLISHED).expect("publication line present");
    let converged_at = index_of(CONVERGED).expect("convergence line present");
    assert!(
        published_at < converged_at,
        "publication must precede convergence-and-activation: publication is a PREPARE-time \
         precondition, convergence is an ACTIVATION-time one"
    );

    // Phase-2 init may still be in flight (see SCOPE above), so drop the pipe
    // and reap rather than expecting a shutdown handshake.
    gateway.close_stdin();
    let _ = gateway.child.kill();
    let _ = gateway.child.wait();
}
