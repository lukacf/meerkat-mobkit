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
