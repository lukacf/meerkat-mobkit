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
        // Dummy provider secrets: an empty workspace boots the fallback mob whose
        // agent's LLM client is CREATED at bootstrap (it needs a secret present)
        // but never CALLED, so placeholders suffice and the test stays
        // key-independent / CI-safe.
        .env("ANTHROPIC_API_KEY", "sk-ant-regression-test")
        .env("OPENAI_API_KEY", "sk-regression-test")
        // Isolate the gateway's state dir (peer keys) into this test's temp
        // workspace: the default ~/.local/state/meerkat-mobkit is shared
        // across concurrent test processes on CI runners and intermittently
        // fails the peer-key load/mint (a recurring CI flake).
        .env("XDG_STATE_HOME", workspace.path().join("xdg-state"))
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
    // The share-authority bug failed `UnifiedRuntime::bootstrap` with exactly
    // "failed to bootstrap local runtime". A `result` (the normal case with the
    // dummy secret), or any later/unrelated error, means the local runtime
    // bootstrapped past meerkat 0.7's persistence-authority check — which is what
    // this regression guards.
    let err_msg = resp
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    assert!(
        resp.get("result").is_some() || !err_msg.contains("failed to bootstrap local runtime"),
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

/// Both gateway binaries must self-identify via `--version` (name + version),
/// so operators can tell which of the two they have without hashing the file —
/// the gap that turned a mislabeled binary into a multi-day investigation.
#[test]
fn gateways_report_name_and_version() {
    for (bin, needle) in [
        (env!("CARGO_BIN_EXE_mobkit_gateway"), "mobkit_gateway"),
        (env!("CARGO_BIN_EXE_rpc_gateway"), "rpc_gateway"),
    ] {
        let out = Command::new(bin)
            .arg("--version")
            .output()
            .expect("run --version");
        assert!(out.status.success(), "{needle} --version exited non-zero");
        let stdout = String::from_utf8_lossy(&out.stdout);
        // Assert the ACTUAL crate version, not just "some digit", so this test
        // catches a stale/wrong baked-in version (the BUILD.bazel drift class).
        assert!(
            stdout.contains(needle) && stdout.contains(env!("CARGO_PKG_VERSION")),
            "{needle} --version did not print name + version {}: {stdout:?}",
            env!("CARGO_PKG_VERSION")
        );
    }
}

/// `mobkit_gateway` is the console/HTTP gateway, not the SDK's stdin-RPC gateway
/// (that is `rpc_gateway`). If the SDK — which drives a gateway by sending
/// JSON-RPC over stdin — is misconfigured to spawn `mobkit_gateway`, init still
/// succeeds (one stdin line), but the *next* RPC must get a clear error instead
/// of an infinite hang. This is the regression test for the HomeCore reconcile
/// "deadlock": before the fail-loud guard, `mobkit_gateway` read only the init
/// line and then served HTTP forever, silently ignoring `reconcile_identity`.
#[test]
fn mobkit_gateway_rejects_post_init_stdin_rpc_loudly() {
    let bin = env!("CARGO_BIN_EXE_mobkit_gateway");
    let workspace = TempDir::new().expect("workspace tempdir");
    let store = TempDir::new().expect("store tempdir");

    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "mobkit/init",
        "params": {
            "workspace_root": workspace.path().to_string_lossy(),
            "store_path": store.path().join("store").to_string_lossy(),
        }
    });
    let reconcile =
        json!({ "jsonrpc": "2.0", "id": 2, "method": "mobkit/reconcile_identity", "params": {} });

    let mut child = Command::new(bin)
        .current_dir(workspace.path())
        .env("ANTHROPIC_API_KEY", "sk-ant-regression-test")
        .env("OPENAI_API_KEY", "sk-regression-test")
        // Isolated state dir (peer keys) — see the sibling spawn above; the
        // shared ~/.local/state default is a recurring CI peer-key flake.
        .env("XDG_STATE_HOME", workspace.path().join("xdg-state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mobkit_gateway");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    writeln!(stdin, "{}", serde_json::to_string(&init).unwrap()).expect("write init");
    stdin.flush().expect("flush init");
    let init_resp = rx
        .recv_timeout(Duration::from_secs(45))
        .expect("no init response within 45s");
    let init_v: Value = serde_json::from_str(init_resp.trim())
        .unwrap_or_else(|e| panic!("non-JSON init response {init_resp:?}: {e}"));
    assert!(
        init_v.get("result").is_some(),
        "init did not return a result (cannot exercise the post-init guard): {init_resp}"
    );

    writeln!(stdin, "{}", serde_json::to_string(&reconcile).unwrap()).expect("write reconcile");
    stdin.flush().expect("flush reconcile");
    let reconcile_resp = rx.recv_timeout(Duration::from_secs(20)).expect(
        "mobkit_gateway did not answer a post-init stdin RPC within 20s — the silent-hang regressed",
    );

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();

    let v: Value = serde_json::from_str(reconcile_resp.trim())
        .unwrap_or_else(|e| panic!("non-JSON reconcile response {reconcile_resp:?}: {e}"));
    let msg = v
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    assert!(
        msg.contains("rpc_gateway"),
        "post-init stdin RPC must fail loudly and point at rpc_gateway, got: {reconcile_resp}"
    );
}

/// meerkat-studio K2 root cause on this binary: `mobkit_gateway` had no
/// tracing subscriber, so every WARN/ERROR in the process — runtime
/// failures, console internal-error logs, the schedule claim watchdog's
/// stall diagnosis — was silently dropped. Pin that the binary emits
/// tracing on stderr: with RUST_LOG=info the bootstrap path logs at least
/// one INFO line.
#[test]
fn mobkit_gateway_emits_tracing_on_stderr() {
    let bin = env!("CARGO_BIN_EXE_mobkit_gateway");
    let workspace = TempDir::new().expect("workspace tempdir");
    let store = TempDir::new().expect("store tempdir");

    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "mobkit/init",
        "params": {
            "workspace_root": workspace.path().to_string_lossy(),
            "store_path": store.path().join("store").to_string_lossy(),
        },
    });

    let mut child = Command::new(bin)
        .current_dir(workspace.path())
        .env("ANTHROPIC_API_KEY", "sk-ant-regression-test")
        .env("OPENAI_API_KEY", "sk-regression-test")
        .env("XDG_STATE_HOME", workspace.path().join("xdg-state"))
        .env("RUST_LOG", "info")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mobkit_gateway");

    let mut stdin = child.stdin.take().expect("stdin");
    writeln!(stdin, "{}", serde_json::to_string(&init).unwrap()).expect("write init");
    stdin.flush().expect("flush");

    // Wait for the init response so bootstrap tracing has happened.
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        let _ = tx.send(line);
    });
    let _init_response = rx
        .recv_timeout(Duration::from_mins(1))
        .expect("gateway responded to init");

    let _ = stdin;
    let _ = child.kill();
    let output = child.wait_with_output().expect("gateway output");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("INFO") || stderr.contains("WARN"),
        "mobkit_gateway must emit tracing on stderr (RUST_LOG=info); \
         got stderr: {stderr:?}"
    );
}

/// Doctrine default-on: a plain init (no identity_first param) boots the
/// gateway with the durable-identity substrate (continuity store + leases +
/// identity console surface) constructed from the existing store paths.
/// `identity_first: false` remains as a one-release opt-out (tested below).
#[test]
fn mobkit_gateway_bootstraps_identity_first_runtime() {
    let bin = env!("CARGO_BIN_EXE_mobkit_gateway");
    let workspace = TempDir::new().expect("workspace tempdir");
    let store = TempDir::new().expect("store tempdir");

    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "mobkit/init",
        "params": {
            "workspace_root": workspace.path().to_string_lossy(),
            "store_path": store.path().join("store").to_string_lossy(),
            "persistent_sessions": true,
            "identity_roster": [{
                "identity": "personal:alice",
                "profile": "alpha",
            }],
        },
    });

    let mut child = Command::new(bin)
        .current_dir(workspace.path())
        .env("ANTHROPIC_API_KEY", "sk-ant-regression-test")
        .env("OPENAI_API_KEY", "sk-regression-test")
        .env("XDG_STATE_HOME", workspace.path().join("xdg-state"))
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
        .recv_timeout(Duration::from_secs(90))
        .expect("gateway responded to identity-first init");
    let _ = child.kill();
    let _ = child.wait();
    let response: Value = serde_json::from_str(line.trim()).expect("init response json");
    assert!(
        response.get("error").is_none(),
        "identity-first init must succeed: {response}"
    );
    assert!(
        response["result"]["http_base_url"].is_string(),
        "init response carries the http base url: {response}"
    );
    // The identity substrate is durable: continuity.db exists in the store.
    let continuity = store.path().join("store").join("continuity.db");
    assert!(
        continuity.exists(),
        "identity-first boot must create {}",
        continuity.display()
    );
}

/// The one-release opt-out: `identity_first: false` boots the pure mob-plane
/// gateway — no continuity store is created.
#[test]
fn mobkit_gateway_identity_first_opt_out_skips_the_substrate() {
    let bin = env!("CARGO_BIN_EXE_mobkit_gateway");
    let workspace = TempDir::new().expect("workspace tempdir");
    let store = TempDir::new().expect("store tempdir");

    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "mobkit/init",
        "params": {
            "workspace_root": workspace.path().to_string_lossy(),
            "store_path": store.path().join("store").to_string_lossy(),
            "identity_first": false,
        },
    });

    let mut child = Command::new(bin)
        .current_dir(workspace.path())
        .env("ANTHROPIC_API_KEY", "sk-ant-regression-test")
        .env("OPENAI_API_KEY", "sk-regression-test")
        .env("XDG_STATE_HOME", workspace.path().join("xdg-state"))
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
        .recv_timeout(Duration::from_mins(1))
        .expect("gateway responded to opt-out init");
    let _ = child.kill();
    let _ = child.wait();
    let response: Value = serde_json::from_str(line.trim()).expect("init response json");
    assert!(
        response.get("error").is_none(),
        "opt-out init must succeed: {response}"
    );
    let continuity = store.path().join("store").join("continuity.db");
    assert!(
        !continuity.exists(),
        "identity_first: false must not create {}",
        continuity.display()
    );
}
