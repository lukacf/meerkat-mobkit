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

#[test]
fn persistent_gateway_exits_after_stdin_eof() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let mut gateway = Gateway::start();
    initialize_for_shutdown(&mut gateway, &state_dir);

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
