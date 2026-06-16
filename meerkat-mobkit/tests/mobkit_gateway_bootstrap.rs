#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args
)]

//! Regression: the DEFAULT (ephemeral) `mobkit_gateway` bootstrap path failed
//! closed on the very first `mobkit/init` in every shipped 0.7.x binary. It
//! attached an explicit runtime adapter that did NOT share the session service's
//! runtime persistence authority, which meerkat 0.7 rejects with
//! `failed to bootstrap local runtime` (the `with_session_runtime_adapter` call
//! that `rpc_gateway` already had was missing). `persistent_sessions` defaults
//! off, so this is what every gateway launch hit — yet no test ever spawned
//! `mobkit_gateway`, so it shipped broken (0.7.5, 0.7.6).
//!
//! These tests drive a real `mobkit/init` for BOTH session modes and assert the
//! local runtime bootstraps. No API keys are needed — bootstrap runs before any
//! provider call. The mob is the gateway's built-in fallback definition (the bug
//! is independent of the mob), so an empty workspace is sufficient.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

/// Spawn the real `mobkit_gateway` binary, send one `mobkit/init` (with the
/// gateway's actual `InitParams` field names), and assert the local runtime
/// bootstraps (an init `result`, not an `error`). The read happens on a worker
/// thread bounded by `recv_timeout` so a bootstrap hang can't wedge the suite.
fn assert_bootstraps(persistent_sessions: bool) {
    let bin = env!("CARGO_BIN_EXE_mobkit_gateway");
    let workspace = TempDir::new().expect("workspace tempdir");
    let store = TempDir::new().expect("store tempdir");

    let mut params = serde_json::Map::new();
    params.insert(
        "workspace_root".into(),
        json!(workspace.path().to_string_lossy()),
    );
    params.insert(
        "store_path".into(),
        json!(store.path().join("store").to_string_lossy()),
    );
    if persistent_sessions {
        params.insert("persistent_sessions".into(), json!(true));
    }
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "mobkit/init",
        "params": Value::Object(params),
    });

    let mut child = Command::new(bin)
        .current_dir(workspace.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mobkit_gateway");

    let mut stdin = child.stdin.take().expect("stdin");
    writeln!(stdin, "{}", serde_json::to_string(&init).unwrap()).expect("write init");
    stdin.flush().expect("flush");

    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        let _ = tx.send(line);
    });

    let line = rx
        .recv_timeout(Duration::from_secs(45))
        .expect("mobkit_gateway did not answer mobkit/init within 45s (bootstrap hung?)");

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();

    let resp: Value = serde_json::from_str(line.trim())
        .unwrap_or_else(|e| panic!("non-JSON init response {:?}: {}", line, e));
    assert!(
        resp.get("result").is_some(),
        "mobkit_gateway failed to bootstrap a local runtime (persistent_sessions={}): {}",
        persistent_sessions,
        resp
    );
}

/// The default mode — this is the path that broke in 0.7.5/0.7.6.
#[test]
fn mobkit_gateway_bootstraps_ephemeral_runtime() {
    assert_bootstraps(false);
}

/// The persistent-sessions mode also goes through `with_session_runtime_adapter`.
#[test]
fn mobkit_gateway_bootstraps_persistent_runtime() {
    assert_bootstraps(true);
}
