//! TWO-PROCESS cross-mob round trip: the full-fidelity proof that two real
//! `rpc_gateway --persistent` OS processes can wire members over the
//! cross-mob control channel and deliver messages in both directions with
//! nothing shared but TCP sockets.
//!
//! Two children are spawned from the shipped binary (the
//! `identity_first_subprocess_reboot.rs` harness idiom), each with:
//!
//! * `--control-listen tcp://127.0.0.1:<port>` - the cross-mob control
//!   listener, with the port probed up front so BOTH contact directories
//!   can be written at init time (A needs B's address and B needs A's; an
//!   ephemeral `:0` bind on both sides would be circular). The init
//!   response's new `control_listen_address` field is asserted against the
//!   requested address.
//! * `runtime_options.member_comms_address = "127.0.0.1:0"` - every member
//!   binds its own signed envelope listener, so the descriptors installed
//!   by the wire are dialable across the process boundary.
//! * `runtime_options.contacts_toml` - the inline contact directory
//!   pointing at the OTHER process's control listener.
//! * `runtime_options.demo_llm` - deterministic `TestClient`, no network.
//!
//! Proven across the process boundary:
//!
//! 1. `mobkit/cross_mob/wire` from A reaches B's control listener over real
//!    TCP, installs signed descriptors on BOTH sides (asserted via each
//!    gateway's own `mobkit/get_member` `wired_to` projection - the far
//!    side's row is the reverse leg).
//! 2. `mobkit/cross_mob/send` delivers A -> B and B -> A: each send returns
//!    the receiving member's bridge session id from the OTHER process, the
//!    injection triggers a turn on the receiver, and the marker text
//!    minted in the SENDING process becomes durable in the RECEIVING
//!    process's state directory - polled while the child is alive, then
//!    asserted again from disk after the child exited.
//! 3. `mobkit/cross_mob/unwire` from A clears the peering on BOTH sides.
//!
//! # What is deliberately NOT here
//!
//! A member-driven signed PeerMessage between the two processes. Members
//! only emit peer messages from inside an agent turn (the comms `send`
//! tool); `demo_llm` makes no tool calls, and no RPC surface invokes a
//! member's session tools directly (`mobkit/call_tool` routes to modules).
//! Driving it would need a live LLM (the e2e-live lane) or a test-only
//! RPC. The signed envelope plane itself - real TCP dial, ingress trust
//! check against the wired descriptor, ack signed by the addressed
//! member's keypair - is already proven socket-for-socket by
//! `tests/cross_mob_control_round_trip.rs`; what THIS file adds is the
//! process boundary, which the control plane and both wire legs do cross.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args
)]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const INIT_TIMEOUT: Duration = Duration::from_mins(3);
const RPC_TIMEOUT: Duration = Duration::from_mins(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_mins(3);
const EXIT_TIMEOUT: Duration = Duration::from_mins(1);

const MOB_A: &str = r#"
[mob]
id = "two-proc-a"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true
runtime_mode = "turn_driven"

[profiles.worker.tools]
comms = true
"#;

const MOB_B: &str = r#"
[mob]
id = "two-proc-b"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true
runtime_mode = "turn_driven"

[profiles.worker.tools]
comms = true
"#;

const MARKER_A_TO_B: &str = "cross-mob-marker-alpha-to-bravo-7f3c";
const MARKER_B_TO_A: &str = "cross-mob-marker-bravo-to-alpha-2e91";

/// Probe a free loopback port. The port is released before the gateway
/// binds it, so another process could race onto it; the same accepted
/// caveat as `cross_mob_tcp.rs::ephemeral_port` (both directories must be
/// written before either child boots, so `:0` cannot be used here).
fn probe_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("probe port")
        .local_addr()
        .expect("probe addr")
        .port()
}

struct Gateway {
    label: String,
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    stderr: Arc<Mutex<Vec<String>>>,
}

impl Gateway {
    fn start(label: &str, control_listen: &str) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rpc_gateway"));
        command
            .arg("--persistent")
            .arg("--control-listen")
            .arg(control_listen)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn rpc_gateway --persistent");

        let stdin = child.stdin.take().expect("gateway stdin");
        let stdout = child.stdout.take().expect("gateway stdout");
        let child_stderr = child.stderr.take().expect("gateway stderr");

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stderr_sink = stderr.clone();
        thread::spawn(move || {
            for line in BufReader::new(child_stderr).lines() {
                let Ok(line) = line else { break };
                let Ok(mut sink) = stderr_sink.lock() else {
                    break;
                };
                if sink.len() < 500 {
                    sink.push(line);
                }
            }
        });

        Self {
            label: label.to_string(),
            child,
            stdin: Some(stdin),
            lines: rx,
            stderr,
        }
    }

    fn send(&mut self, value: &Value) {
        let stdin = self.stdin.as_mut().expect("gateway stdin remains open");
        writeln!(
            stdin,
            "{}",
            serde_json::to_string(value).expect("serialize request")
        )
        .expect("write request to gateway stdin");
        stdin.flush().expect("flush gateway stdin");
    }

    fn close_stdin(&mut self) {
        drop(self.stdin.take());
    }

    fn stderr_tail(&self) -> String {
        let Ok(sink) = self.stderr.lock() else {
            return "[gateway stderr unavailable: poisoned lock]".to_string();
        };
        let start = sink.len().saturating_sub(40);
        format!(
            "--- [{}] gateway stderr (last {} of {} lines) ---\n{}",
            self.label,
            sink.len() - start,
            sink.len(),
            sink[start..].join("\n")
        )
    }

    /// Send one request and return its response. This harness declares no
    /// host providers, so any `callback/*` arriving is a composition bug
    /// the test must fail on, not stub around.
    fn call(&mut self, id: &str, method: &str, params: Value, deadline: Duration) -> Value {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        let start = Instant::now();
        loop {
            let Some(remaining) = deadline.checked_sub(start.elapsed()) else {
                panic!(
                    "[{}] no response to {method} within {:?}\n{}",
                    self.label,
                    deadline,
                    self.stderr_tail()
                );
            };
            let Ok(line) = self.lines.recv_timeout(remaining) else {
                panic!(
                    "[{}] gateway stdout closed while waiting for {method}\n{}",
                    self.label,
                    self.stderr_tail()
                );
            };
            let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if let Some(callback) = message.get("method").and_then(Value::as_str) {
                assert!(
                    !callback.starts_with("callback/"),
                    "[{}] unexpected host callback {callback}: this harness declares no \
                     providers, so a stub answer would invalidate the run",
                    self.label
                );
                continue; // notification
            }
            if message.get("id").and_then(Value::as_str) == Some(id) {
                assert!(
                    message.get("error").is_none(),
                    "[{}] {method} failed: {message}\n{}",
                    self.label,
                    self.stderr_tail()
                );
                return message;
            }
        }
    }

    fn shutdown_and_reap(&mut self) {
        let response = self.call("shutdown", "mobkit/shutdown", json!({}), SHUTDOWN_TIMEOUT);
        assert_eq!(
            response["result"]["shutdown"],
            json!(true),
            "[{}] shutdown completed without cleanup attestation (wedge): {response}\n{}",
            self.label,
            self.stderr_tail()
        );
        self.close_stdin();
        let status = self.wait_for_exit(EXIT_TIMEOUT);
        assert!(
            status.success(),
            "[{}] rpc_gateway exited with {status}\n{}",
            self.label,
            self.stderr_tail()
        );
    }

    fn wait_for_exit(&mut self, deadline: Duration) -> ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().expect("poll gateway exit") {
                return status;
            }
            if start.elapsed() >= deadline {
                let tail = self.stderr_tail();
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!(
                    "[{}] rpc_gateway did not exit within {:?}\n{}",
                    self.label, deadline, tail
                );
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Boot one gateway: spawn with `--control-listen`, init with the inline
/// contact directory pointing at the peer, and assert the init response
/// reports the requested control address back.
fn boot(
    label: &str,
    state: &Path,
    mob_config: &str,
    control_listen: &str,
    contacts_toml: &str,
) -> Gateway {
    let mut gateway = Gateway::start(label, control_listen);
    let response = gateway.call(
        "init",
        "mobkit/init",
        json!({
            "persistent_state": state,
            "mob_config": mob_config,
            "runtime_options": {
                "demo_llm": true,
                "member_comms_address": "127.0.0.1:0",
                "contacts_toml": contacts_toml,
            }
        }),
        INIT_TIMEOUT,
    );
    assert_eq!(
        response["result"]["control_listen_address"],
        json!(control_listen),
        "[{label}] init response must report the bound control-listener address: {response}"
    );
    gateway
}

fn spawn_member(gateway: &mut Gateway, member: &str) {
    let response = gateway.call(
        "spawn",
        "mobkit/spawn_member",
        json!({ "profile": "worker", "meerkat_id": member }),
        RPC_TIMEOUT,
    );
    assert_eq!(
        response["result"]["accepted"],
        json!(true),
        "[{}] spawn_member {member} not accepted: {response}",
        gateway.label
    );
}

fn wired_to(gateway: &mut Gateway, member: &str) -> Vec<String> {
    let response = gateway.call(
        "get-member",
        "mobkit/get_member",
        json!({ "member_id": member }),
        RPC_TIMEOUT,
    );
    response["result"]["wired_to"]
        .as_array()
        .map(|peers| {
            peers
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Scan every regular file under a state
/// directory for `marker` bytes. Schema-agnostic on purpose - the claim is
/// exactly "text minted in the OTHER process reached this process's
/// durable storage", not any particular table layout.
fn state_dir_contains_marker(state: &Path, marker: &str) -> bool {
    fn scan(path: &Path, needle: &[u8], hit: &mut bool) {
        if *hit {
            return;
        }
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                scan(&entry_path, needle, hit);
            } else if let Ok(bytes) = std::fs::read(&entry_path)
                && bytes.windows(needle.len()).any(|window| window == needle)
            {
                *hit = true;
            }
            if *hit {
                return;
            }
        }
    }
    let mut hit = false;
    scan(state, marker.as_bytes(), &mut hit);
    hit
}

/// Wait for `marker` to become durable in `state` while the child is still
/// alive. The injected turn runs asynchronously in the receiving process;
/// polling the durable bytes (WAL included) is the deterministic
/// completion signal that does not depend on any schema or event surface.
fn wait_for_durable_marker(label: &str, state: &Path, marker: &str, deadline: Duration) {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if state_dir_contains_marker(state, marker) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("[{label}] marker '{marker}' did not become durable within {deadline:?}");
}

#[test]
fn two_process_cross_mob_wire_and_delivery_round_trip() {
    let state_a = tempfile::tempdir().expect("state dir a");
    let state_b = tempfile::tempdir().expect("state dir b");
    let port_a = probe_port();
    let port_b = probe_port();
    let control_a = format!("tcp://127.0.0.1:{port_a}");
    let control_b = format!("tcp://127.0.0.1:{port_b}");
    let contacts_a = format!("[mobs]\n\"two-proc-b\" = \"{control_b}\"\n");
    let contacts_b = format!("[mobs]\n\"two-proc-a\" = \"{control_a}\"\n");

    let mut gateway_a = boot("A", state_a.path(), MOB_A, &control_a, &contacts_a);
    let mut gateway_b = boot("B", state_b.path(), MOB_B, &control_b, &contacts_b);

    spawn_member(&mut gateway_a, "alice");
    spawn_member(&mut gateway_b, "bob");

    // Wire across the process boundary: A's runtime talks to B's control
    // listener over real TCP (LookupMember for bob's facts, then the Wire
    // request carrying alice's member address + pubkey).
    let response = gateway_a.call(
        "wire",
        "mobkit/cross_mob/wire",
        json!({
            "local_member_id": "alice",
            "remote_member_id": "bob",
            "remote_mob_id": "two-proc-b",
        }),
        RPC_TIMEOUT,
    );
    assert_eq!(response["result"]["accepted"], json!(true));

    // Both rosters project the peering - B's row is the reverse leg,
    // installed by a request that crossed the process boundary.
    let alice_peers = wired_to(&mut gateway_a, "alice");
    assert!(
        alice_peers.iter().any(|peer| peer.contains("bob")),
        "[A] alice must be wired to bob, got {alice_peers:?}"
    );
    let bob_peers = wired_to(&mut gateway_b, "bob");
    assert!(
        bob_peers.iter().any(|peer| peer.contains("alice")),
        "[B] bob must be wired to alice (reverse leg across processes), got {bob_peers:?}"
    );

    // Delivery A -> B: the response's session id is minted in B's process.
    let response = gateway_a.call(
        "send-a-to-b",
        "mobkit/cross_mob/send",
        json!({
            "from_member_id": "alice",
            "remote_member_id": "bob",
            "remote_mob_id": "two-proc-b",
            "content": MARKER_A_TO_B,
        }),
        RPC_TIMEOUT,
    );
    assert!(
        response["result"]["session_id"].is_string(),
        "[A] cross_mob/send returned no remote session id: {response}"
    );

    // Delivery B -> A, over B's own contact directory and A's listener.
    let response = gateway_b.call(
        "send-b-to-a",
        "mobkit/cross_mob/send",
        json!({
            "from_member_id": "bob",
            "remote_member_id": "alice",
            "remote_mob_id": "two-proc-a",
            "content": MARKER_B_TO_A,
        }),
        RPC_TIMEOUT,
    );
    assert!(
        response["result"]["session_id"].is_string(),
        "[B] cross_mob/send returned no remote session id: {response}"
    );

    // The injections trigger a turn on each receiving member (same
    // member-door as a direct send); wait for the marker bytes to become
    // durable in each receiver's state directory before shutting down, so
    // shutdown quiescing cannot race an unstarted turn.
    wait_for_durable_marker("B", state_b.path(), MARKER_A_TO_B, RPC_TIMEOUT);
    wait_for_durable_marker("A", state_a.path(), MARKER_B_TO_A, RPC_TIMEOUT);

    // Unwire across the boundary and confirm both projections drop it.
    let response = gateway_a.call(
        "unwire",
        "mobkit/cross_mob/unwire",
        json!({
            "local_member_id": "alice",
            "remote_member_id": "bob",
            "remote_mob_id": "two-proc-b",
        }),
        RPC_TIMEOUT,
    );
    assert_eq!(response["result"]["accepted"], json!(true));
    let alice_peers = wired_to(&mut gateway_a, "alice");
    assert!(
        !alice_peers.iter().any(|peer| peer.contains("bob")),
        "[A] alice must no longer be wired to bob after unwire, got {alice_peers:?}"
    );
    let bob_peers = wired_to(&mut gateway_b, "bob");
    assert!(
        !bob_peers.iter().any(|peer| peer.contains("alice")),
        "[B] bob must no longer be wired to alice after remote unwire, got {bob_peers:?}"
    );

    // Clean shutdown, then read the durable proof from disk: the marker
    // minted in the OTHER process must be present in each receiver's state
    // directory, and must NOT appear in the sender's own durable state
    // (guards against a false positive from some shared path).
    gateway_a.shutdown_and_reap();
    gateway_b.shutdown_and_reap();

    assert!(
        state_dir_contains_marker(state_b.path(), MARKER_A_TO_B),
        "marker sent A -> B not found in B's durable state directory"
    );
    assert!(
        state_dir_contains_marker(state_a.path(), MARKER_B_TO_A),
        "marker sent B -> A not found in A's durable state directory"
    );
    assert!(
        !state_dir_contains_marker(state_a.path(), MARKER_A_TO_B),
        "A -> B marker unexpectedly present in A's own durable state; the delivery assert \
         would be meaningless"
    );
    assert!(
        !state_dir_contains_marker(state_b.path(), MARKER_B_TO_A),
        "B -> A marker unexpectedly present in B's own durable state; the delivery assert \
         would be meaningless"
    );
}
