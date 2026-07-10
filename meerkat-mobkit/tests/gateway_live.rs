//! Gateway-level tests for the live (realtime) surface: opt-in gating,
//! method advertisement, identity-target resolution errors, and the mounted
//! WebSocket route. A full provider round-trip needs a realtime credential
//! and a live provider — out of scope here; the projection/token semantics
//! are unit-tested in `src/live_wiring.rs` and upstream.

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
id = "gateway-live-test"

[profiles.default]
model = "gpt-5.5"
external_addressable = true

[profiles.default.tools]
comms = true
"#;

struct Gateway {
    child: Child,
    stdin: ChildStdin,
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
            stdin,
            lines: rx,
        }
    }

    fn send(&mut self, value: Value) {
        writeln!(
            self.stdin,
            "{}",
            serde_json::to_string(&value).expect("request json")
        )
        .expect("write request");
        self.stdin.flush().expect("flush request");
    }

    fn wait_for_response(&mut self, id: &str, deadline: Duration) -> Value {
        let start = Instant::now();
        while start.elapsed() < deadline {
            let remaining = deadline.saturating_sub(start.elapsed());
            let Ok(line) = self.lines.recv_timeout(remaining) else {
                break;
            };
            let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if message.get("method").is_none()
                && message.get("id").and_then(Value::as_str) == Some(id)
            {
                return message;
            }
        }
        panic!("no response for id {id} within {deadline:?}");
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn init_params(state_dir: &tempfile::TempDir, live: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "mobkit/init",
        "params": {
            "persistent_state": state_dir.path(),
            "mob_config": MOB_CONFIG,
            "runtime_options": { "live": live }
        }
    })
}

#[test]
fn live_methods_answer_unavailable_without_opt_in() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let mut gateway = Gateway::start();
    gateway.send(init_params(&state_dir, json!(false)));
    let init = gateway.wait_for_response("init", Duration::from_mins(1));
    assert!(init["result"]["contract_version"].is_string(), "{init}");

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "open",
        "method": "mobkit/live/open",
        "params": { "identity": "worker-1" }
    }));
    let response = gateway.wait_for_response("open", Duration::from_secs(15));
    assert_eq!(response["error"]["code"], json!(-32050), "{response}");
    assert_eq!(response["error"]["data"]["kind"], json!("live_unavailable"));

    // The catalog does not advertise live methods either.
    gateway.send(json!({
        "jsonrpc": "2.0", "id": "caps", "method": "mobkit/capabilities", "params": {}
    }));
    let caps = gateway.wait_for_response("caps", Duration::from_secs(15));
    let methods = caps["result"]["methods"].as_array().expect("methods");
    assert!(!methods.iter().any(|m| m == "mobkit/live/open"), "{caps}");
}

#[test]
fn live_opt_in_advertises_methods_and_mounts_the_ws_route() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let mut gateway = Gateway::start();
    gateway.send(init_params(&state_dir, json!(true)));
    let init = gateway.wait_for_response("init", Duration::from_mins(1));
    assert!(init["result"]["contract_version"].is_string(), "{init}");
    let base = init["result"]["http_base_url"]
        .as_str()
        .expect("http_base_url")
        .to_string();

    gateway.send(json!({
        "jsonrpc": "2.0", "id": "caps", "method": "mobkit/capabilities", "params": {}
    }));
    let caps = gateway.wait_for_response("caps", Duration::from_secs(15));
    let methods = caps["result"]["methods"].as_array().expect("methods");
    for method in [
        "mobkit/live/open",
        "mobkit/live/status",
        "mobkit/live/close",
        "mobkit/live/refresh",
        "mobkit/live/truncate",
    ] {
        assert!(methods.iter().any(|m| m == method), "missing {method}");
    }

    // Unresolvable member target → typed invalid params, not a hang.
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "open-missing",
        "method": "mobkit/live/open",
        "params": { "identity": "nobody-here" }
    }));
    let response = gateway.wait_for_response("open-missing", Duration::from_secs(15));
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");

    // The WS route is mounted on the gateway's OWN listener: a tokenless
    // GET to /live/ws is rejected by the transport (any HTTP status proves
    // the route exists; an unmounted path would be the axum 404 fallback
    // with an empty body — the transport rejection carries one).
    let url = format!("{base}/live/ws");
    let response = ureq_get_status(&url);
    assert!(
        response != 404,
        "live ws route must be mounted (got 404 from {url})"
    );
}

/// Fix 5: `mobkit/live/truncate` is served (not method-not-found) and
/// answers TYPED errors — invalid params for an empty item_id, and the
/// machine-authority channel-not-found rejection for a channel that was
/// never opened. A full truncate round-trip needs a live provider channel;
/// the command mapping is covered by the shared
/// `live_command_result_from_machine_authority` unit coverage.
#[test]
fn live_truncate_answers_typed_errors_without_a_channel() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let mut gateway = Gateway::start();
    gateway.send(init_params(&state_dir, json!(true)));
    let init = gateway.wait_for_response("init", Duration::from_mins(1));
    assert!(init["result"]["contract_version"].is_string(), "{init}");

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "truncate-empty-item",
        "method": "mobkit/live/truncate",
        "params": {
            "channel_id": "chan-1",
            "item_id": "",
            "content_index": 0,
            "audio_played_ms": 0
        }
    }));
    let response = gateway.wait_for_response("truncate-empty-item", Duration::from_secs(15));
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    assert!(
        response["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("item_id"),
        "{response}"
    );

    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "truncate-unbound",
        "method": "mobkit/live/truncate",
        "params": {
            "channel_id": "no-such-channel",
            "item_id": "item_1",
            "content_index": 0,
            "audio_played_ms": 1200
        }
    }));
    let response = gateway.wait_for_response("truncate-unbound", Duration::from_secs(15));
    let error = response.get("error").expect("typed error, not a hang");
    assert!(
        error["message"]
            .as_str()
            .expect("error message")
            .contains("no-such-channel"),
        "unbound-channel rejection names the channel: {response}"
    );
}

/// Fix 4 parse surface: `runtime_options.live.seed_max_chars` is accepted at
/// init (object form) — a bad value fails init loudly.
#[test]
fn live_object_form_accepts_seed_max_chars() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let mut gateway = Gateway::start();
    gateway.send(init_params(&state_dir, json!({ "seed_max_chars": 200000 })));
    let init = gateway.wait_for_response("init", Duration::from_mins(1));
    assert!(init["result"]["contract_version"].is_string(), "{init}");
}

/// Fix 1 registration surface: `runtime_options.host_runnables` is accepted
/// at init on a persistent gateway (the schedule host composes the callback
/// runnables), and malformed values fail init loudly. The fire path
/// (callback/schedule_fire over the bridge) is unit-tested in
/// `schedule_wiring`; target-kind acceptance is pinned there too.
#[test]
fn init_accepts_host_runnables_and_rejects_duplicates() {
    let state_dir = tempfile::tempdir().expect("state dir");
    let mut gateway = Gateway::start();
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "mobkit/init",
        "params": {
            "persistent_state": state_dir.path(),
            "mob_config": MOB_CONFIG,
            "runtime_options": { "host_runnables": ["digest", "backup.rotate"] }
        }
    }));
    let init = gateway.wait_for_response("init", Duration::from_mins(1));
    assert!(init["result"]["contract_version"].is_string(), "{init}");
    drop(gateway);

    let state_dir = tempfile::tempdir().expect("state dir");
    let mut gateway = Gateway::start();
    gateway.send(json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "mobkit/init",
        "params": {
            "persistent_state": state_dir.path(),
            "mob_config": MOB_CONFIG,
            "runtime_options": { "host_runnables": ["digest", "digest"] }
        }
    }));
    let init = gateway.wait_for_response("init", Duration::from_mins(1));
    assert!(
        init["error"]["message"]
            .as_str()
            .expect("init error")
            .contains("duplicated"),
        "{init}"
    );
}

/// Minimal blocking GET returning the HTTP status (no external HTTP dep —
/// hand-rolled over std TcpStream; the gateway listens on loopback).
fn ureq_get_status(url: &str) -> u16 {
    use std::io::{Read, Write as _};
    let rest = url.strip_prefix("http://").expect("http url");
    let (host, path) = rest.split_once('/').expect("path");
    let mut stream = std::net::TcpStream::connect(host).expect("connect");
    write!(
        stream,
        "GET /{path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let head = String::from_utf8_lossy(&buf);
    head.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status code")
}
