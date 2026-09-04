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
use std::path::Path;
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

    // Drain stderr CONCURRENTLY from spawn. Leaving the pipe unread until
    // after init deadlocks under kernel pipe-buffer pressure: when the
    // machine-wide pipe pool is exhausted (macOS degrades fresh pipes to
    // 512-byte buffers; observed with ~1.5k pipes leaked by a co-located
    // agent), the gateway's ~576 bytes of bootstrap tracing overflow the
    // buffer and its 4th stderr write blocks BEFORE the init response is
    // printed — the test then times out on stdout with an almost-empty pipe.
    let stderr_pipe = child.stderr.take().expect("stderr");
    let stderr_drain = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        use std::io::Read;
        let mut reader = stderr_pipe;
        let _ = reader.read_to_end(&mut buffer);
        buffer
    });

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
    let _ = child.wait().expect("gateway exit");
    let stderr_bytes = stderr_drain.join().expect("stderr drain thread");
    let stderr = String::from_utf8_lossy(&stderr_bytes);
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
    // The identity substrate is durable: the canonical continuity store
    // exists in the store dir (M2 spelling on a fresh dir; a legacy
    // `continuity.db` would keep being used where it lies instead).
    let continuity = store.path().join("store").join("continuity.sqlite3");
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
    for name in ["continuity.sqlite3", "continuity.db"] {
        let continuity = store.path().join("store").join(name);
        assert!(
            !continuity.exists(),
            "identity_first: false must not create {}",
            continuity.display()
        );
    }
}

/// Spawn the real binary, send one `mobkit/init` whose params are `extra`
/// plus a fresh workspace and store, and return the first stdout line as
/// JSON (a `result` or an `error`).
fn init_once(extra: Value, timeout: Duration) -> Value {
    let workspace = TempDir::new().expect("workspace tempdir");
    let store = TempDir::new().expect("store tempdir");
    init_once_in(workspace.path(), store.path(), extra, timeout)
}

/// [`init_once`] against caller-owned directories, so a test can inspect what
/// the boot left (or did not leave) on disk. `store_path` is `<store>/store`.
fn init_once_in(workspace: &Path, store: &Path, extra: Value, timeout: Duration) -> Value {
    let bin = env!("CARGO_BIN_EXE_mobkit_gateway");

    let mut params = match extra {
        Value::Object(map) => map,
        other => panic!("init params must be a JSON object, got {other}"),
    };
    params.insert("workspace_root".into(), json!(workspace.to_string_lossy()));
    params.insert(
        "store_path".into(),
        json!(store.join("store").to_string_lossy()),
    );
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "mobkit/init",
        "params": Value::Object(params),
    });

    let mut child = Command::new(bin)
        .current_dir(workspace)
        .env("ANTHROPIC_API_KEY", "sk-ant-regression-test")
        .env("OPENAI_API_KEY", "sk-regression-test")
        .env("XDG_STATE_HOME", workspace.join("xdg-state"))
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
        .recv_timeout(timeout)
        .expect("mobkit_gateway did not answer mobkit/init in time");
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    serde_json::from_str(line.trim())
        .unwrap_or_else(|e| panic!("non-JSON init response {:?}: {}", line, e))
}

/// A `[self_hosted.models]` alias whose server is not declared: parses, fails
/// meerkat's `Config::validate`, and must be refused at init by path on both
/// binaries instead of killing the first member build.
const DANGLING_HOST_CONFIG: &str =
    "[self_hosted.models.gemma-4-31b]\nserver = \"nowhere\"\nremote_model = \"gemma4:31b\"\n";

/// A non-loopback `http_listen` without `allow_remote` is refused at init,
/// before bootstrap: the reply is a `-32602` error ON THE REQUEST ID that
/// names the address and the acknowledgement, not a bootstrapped runtime
/// bound to every interface and not an "internal error" with a null id.
#[test]
fn mobkit_gateway_refuses_a_non_loopback_http_listen_without_allow_remote() {
    let response = init_once(
        json!({ "http_listen": "0.0.0.0:0" }),
        Duration::from_secs(45),
    );
    assert!(
        response.get("result").is_none(),
        "a wildcard bind must not succeed without allow_remote: {response}"
    );
    assert_eq!(response["id"], json!(1), "{response}");
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("0.0.0.0:0"), "{response}");
    assert!(message.contains("allow_remote"), "{response}");
}

/// `http_listen` makes a fixed port possible, so a port another process holds
/// is a realistic init failure. It must be answered before any bootstrap:
/// `-32603` on the request id naming the address, and nothing under the store
/// path (the identity substrate, session store and schedule executor lease
/// all live there and none may have been touched).
#[test]
fn mobkit_gateway_refuses_a_taken_fixed_port_at_init_without_bootstrapping() {
    let holder = std::net::TcpListener::bind("127.0.0.1:0").expect("hold a loopback port");
    let taken = holder.local_addr().expect("held address");
    let workspace = TempDir::new().expect("workspace tempdir");
    let store = TempDir::new().expect("store tempdir");

    let response = init_once_in(
        workspace.path(),
        store.path(),
        json!({ "http_listen": taken.to_string() }),
        Duration::from_secs(45),
    );

    assert!(
        response.get("result").is_none(),
        "a taken port must not produce a runtime: {response}"
    );
    assert_eq!(response["id"], json!(1), "{response}");
    assert_eq!(response["error"]["code"], json!(-32603), "{response}");
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains(&taken.to_string()), "{response}");
    assert!(message.contains("failed to bind"), "{response}");
    let store_dir = store.path().join("store");
    assert!(
        !store_dir.exists(),
        "a refused bind must not have bootstrapped anything under {}",
        store_dir.display()
    );
    drop(holder);
}

/// The host config is validated with meerkat's own rules at init: a dangling
/// server reference is `-32602` on the request id naming the file, on this
/// binary too.
#[test]
fn mobkit_gateway_refuses_a_host_config_that_does_not_validate_by_path() {
    let workspace = TempDir::new().expect("workspace tempdir");
    let store = TempDir::new().expect("store tempdir");
    let config = workspace.path().join("dangling.toml");
    std::fs::write(&config, DANGLING_HOST_CONFIG).expect("write host config");

    let response = init_once_in(
        workspace.path(),
        store.path(),
        json!({ "meerkat_config_path": config.to_string_lossy() }),
        Duration::from_secs(45),
    );

    assert!(response.get("result").is_none(), "{response}");
    assert_eq!(response["id"], json!(1), "{response}");
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("meerkat_config_path"), "{response}");
    assert!(message.contains("dangling.toml"), "{response}");
    assert!(message.contains("references unknown server"), "{response}");
}

/// With the acknowledgement the wildcard bind succeeds. `http_base_url` is
/// the same-host loopback form with the kernel-assigned port (never `:0`),
/// and the advertised base rides beside it with its trailing slash dropped.
#[test]
fn mobkit_gateway_binds_a_non_loopback_http_listen_with_allow_remote() {
    let response = init_once(
        json!({
            "http_listen": "0.0.0.0:0",
            "allow_remote": true,
            "http_public_base_url": "https://mob.example.com/"
        }),
        Duration::from_secs(90),
    );
    assert!(
        response.get("error").is_none(),
        "an acknowledged wildcard bind must succeed: {response}"
    );
    let base = response["result"]["http_base_url"]
        .as_str()
        .unwrap_or_default();
    assert!(base.starts_with("http://127.0.0.1:"), "{response}");
    assert!(!base.ends_with(":0"), "{response}");
    assert_eq!(
        response["result"]["http_public_base_url"],
        json!("https://mob.example.com"),
        "{response}"
    );
}

const RPC_MOB_CONFIG: &str =
    "[mob]\nid = \"http-exposure-test\"\n\n[profiles.default]\nmodel = \"gpt-5.5\"\n";

/// The same two facts on the SDK gateway: spawn `rpc_gateway --persistent`,
/// send one `mobkit/init` carrying `runtime_options`, return the reply.
fn rpc_init_once(runtime_options: Value, timeout: Duration) -> Value {
    let workspace = TempDir::new().expect("workspace tempdir");
    rpc_init_in(
        workspace.path(),
        RPC_MOB_CONFIG,
        json!({}),
        runtime_options,
        timeout,
    )
}

/// [`rpc_init_once`] with the mob definition text, extra TOP-LEVEL init
/// params (`top_level`, an object) and the workspace under the test's control.
fn rpc_init_in(
    workspace: &Path,
    mob_config: &str,
    top_level: Value,
    runtime_options: Value,
    timeout: Duration,
) -> Value {
    let bin = env!("CARGO_BIN_EXE_rpc_gateway");
    let mut params = match top_level {
        Value::Object(map) => map,
        other => panic!("top-level init params must be a JSON object, got {other}"),
    };
    params.insert("mob_config".into(), json!(mob_config));
    params.insert("runtime_options".into(), runtime_options);
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "mobkit/init",
        "params": Value::Object(params),
    });

    let mut child = Command::new(bin)
        .arg("--persistent")
        .current_dir(workspace)
        .env("XDG_STATE_HOME", workspace.join("xdg-state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn rpc_gateway");

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
        .recv_timeout(timeout)
        .expect("rpc_gateway did not answer mobkit/init in time");
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    serde_json::from_str(line.trim())
        .unwrap_or_else(|e| panic!("non-JSON init response {:?}: {}", line, e))
}

/// `rpc_gateway`: a port another process holds is refused at init, before
/// any bootstrap: `-32603` on the request id naming the address, and the
/// declared `persistent_state` directory was never created (the session
/// store, identity substrate and schedule executor lease all live there).
#[test]
fn rpc_gateway_refuses_a_taken_fixed_port_at_init_without_bootstrapping() {
    let holder = std::net::TcpListener::bind("127.0.0.1:0").expect("hold a loopback port");
    let taken = holder.local_addr().expect("held address");
    let workspace = TempDir::new().expect("workspace tempdir");
    let state = workspace.path().join("state");

    let response = rpc_init_in(
        workspace.path(),
        RPC_MOB_CONFIG,
        json!({ "persistent_state": state.to_string_lossy() }),
        json!({ "http_listen": taken.to_string() }),
        Duration::from_secs(45),
    );

    assert!(
        response.get("result").is_none(),
        "a taken port must not produce a runtime: {response}"
    );
    assert_eq!(response["id"], json!(1), "{response}");
    assert_eq!(response["error"]["code"], json!(-32603), "{response}");
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains(&taken.to_string()), "{response}");
    assert!(message.contains("failed to bind"), "{response}");
    assert!(
        !state.exists(),
        "a refused bind must not have bootstrapped anything under {}",
        state.display()
    );
    drop(holder);
}

/// `rpc_gateway`: the four exposure/config options live inside
/// `runtime_options` here; the top-level shape (mobkit_gateway's) is refused
/// with `-32602` naming their home, never bound to loopback in silence.
#[test]
fn rpc_gateway_refuses_top_level_exposure_options_by_naming_runtime_options() {
    let workspace = TempDir::new().expect("workspace tempdir");
    let response = rpc_init_in(
        workspace.path(),
        RPC_MOB_CONFIG,
        json!({ "http_listen": "0.0.0.0:8080", "allow_remote": true }),
        json!({}),
        Duration::from_secs(45),
    );
    assert!(response.get("result").is_none(), "{response}");
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("runtime_options.http_listen"),
        "{response}"
    );
    assert!(
        message.contains("runtime_options.allow_remote"),
        "{response}"
    );
}

/// `rpc_gateway`: a `[self_hosted]` table in `mob_config` is refused at init
/// with `-32602` pointing at `meerkat_config_path`, not dropped by the
/// definition parser.
#[test]
fn rpc_gateway_refuses_host_config_tables_in_mob_config() {
    let workspace = TempDir::new().expect("workspace tempdir");
    let mob_config = format!(
        "{RPC_MOB_CONFIG}\n[self_hosted.servers.local]\nbase_url = \"http://127.0.0.1:11434\"\n"
    );
    let response = rpc_init_in(
        workspace.path(),
        &mob_config,
        json!({}),
        json!({}),
        Duration::from_secs(45),
    );
    assert!(response.get("result").is_none(), "{response}");
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("[self_hosted]"), "{response}");
    assert!(message.contains("meerkat_config_path"), "{response}");
}

/// `rpc_gateway`: a host config that parses but fails meerkat's own
/// `Config::validate` (dangling server reference) is `-32602` at init naming
/// the option and the file, the fail-late class the ingress exists to remove.
#[test]
fn rpc_gateway_refuses_a_host_config_that_does_not_validate_by_path() {
    let workspace = TempDir::new().expect("workspace tempdir");
    let config = workspace.path().join("dangling.toml");
    std::fs::write(&config, DANGLING_HOST_CONFIG).expect("write host config");
    let response = rpc_init_in(
        workspace.path(),
        RPC_MOB_CONFIG,
        json!({}),
        json!({ "meerkat_config_path": config.to_string_lossy() }),
        Duration::from_secs(45),
    );
    assert!(response.get("result").is_none(), "{response}");
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("runtime_options.meerkat_config_path"),
        "{response}"
    );
    assert!(message.contains("dangling.toml"), "{response}");
    assert!(message.contains("references unknown server"), "{response}");
}

/// `rpc_gateway`: a non-loopback `runtime_options.http_listen` with neither
/// enforced console auth nor `allow_remote` is a `-32602` init refusal that
/// names the address and the acknowledgement. `console_require_app_auth =
/// false` (the common local opt-out) does not open the gate either.
#[test]
fn rpc_gateway_refuses_a_non_loopback_http_listen_without_allow_remote() {
    let response = rpc_init_once(
        json!({ "http_listen": "0.0.0.0:0", "console_require_app_auth": false }),
        Duration::from_secs(45),
    );
    assert!(
        response.get("result").is_none(),
        "a wildcard bind must not succeed without allow_remote: {response}"
    );
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("0.0.0.0:0"), "{response}");
    assert!(message.contains("allow_remote"), "{response}");
}

/// `rpc_gateway`: with the acknowledgement the wildcard bind succeeds, the
/// reported `http_base_url` is the same-host loopback form with the real
/// port, and `http_public_base_url` echoes the declared base normalised.
#[test]
fn rpc_gateway_binds_a_non_loopback_http_listen_with_allow_remote() {
    let response = rpc_init_once(
        json!({
            "http_listen": "0.0.0.0:0",
            "allow_remote": true,
            "http_public_base_url": "https://mob.example.com/",
            "console_require_app_auth": false
        }),
        Duration::from_secs(90),
    );
    assert!(
        response.get("error").is_none(),
        "an acknowledged wildcard bind must succeed: {response}"
    );
    let base = response["result"]["http_base_url"]
        .as_str()
        .unwrap_or_default();
    assert!(base.starts_with("http://127.0.0.1:"), "{response}");
    assert!(!base.ends_with(":0"), "{response}");
    assert_eq!(
        response["result"]["http_public_base_url"],
        json!("https://mob.example.com"),
        "{response}"
    );
}

/// The default launch on both binaries is byte-for-byte what every earlier
/// release reported: a loopback `http_base_url` and, new but inert, a null
/// `http_public_base_url`.
#[test]
fn both_gateways_default_to_a_loopback_bind_with_no_advertised_base() {
    let rpc = rpc_init_once(json!({}), Duration::from_secs(90));
    assert!(rpc.get("error").is_none(), "{rpc}");
    let base = rpc["result"]["http_base_url"].as_str().unwrap_or_default();
    assert!(base.starts_with("http://127.0.0.1:"), "{rpc}");
    assert!(rpc["result"]["http_public_base_url"].is_null(), "{rpc}");

    let console = init_once(json!({}), Duration::from_secs(90));
    assert!(console.get("error").is_none(), "{console}");
    let base = console["result"]["http_base_url"]
        .as_str()
        .unwrap_or_default();
    assert!(base.starts_with("http://127.0.0.1:"), "{console}");
    assert!(
        console["result"]["http_public_base_url"].is_null(),
        "{console}"
    );
}
