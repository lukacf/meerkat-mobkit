#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args
)]

//! M2 `MobKitStorageLayout` boot coverage: the dual-name fixture matrix per
//! surface. Every surface (builder, `rpc_gateway`, `mobkit_gateway`) must
//! keep booting against a legacy-named state directory (files used where
//! they lie — the resolver never creates a twin), converge fresh
//! directories on the canonical spellings, and refuse loudly when both
//! spellings of one store exist.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use async_trait::async_trait;
use meerkat_mobkit::identity_first::contracts::RosterProvider;
use meerkat_mobkit::identity_first::{DurableAgentSpec, RosterContext, RosterError};
use meerkat_mobkit::unified_runtime::UnifiedRuntimeBuilder;
use serde_json::{Value, json};
use tempfile::TempDir;

fn test_definition() -> meerkat_mob::MobDefinition {
    meerkat_mob::MobDefinition::from_toml(
        r#"
[mob]
id = "storage-layout-boot-test"

[profiles.default]
model = "gpt-5.5"
"#,
    )
    .expect("parse test mob definition")
}

fn touch(path: &Path) {
    std::fs::write(path, b"").expect("seed fixture file");
}

/// An empty desired roster: enough to drive the identity-first arm (and its
/// continuity-store open) without materializing any member.
struct EmptyRosterProvider;

#[async_trait]
impl RosterProvider for EmptyRosterProvider {
    async fn roster(&self, _context: &RosterContext) -> Result<Vec<DurableAgentSpec>, RosterError> {
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// Builder (library surface)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn builder_fresh_state_dir_converges_on_canonical_names() {
    let tmp = TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    let runtime = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .persistent_state(&state)
            .roster_provider(Arc::new(EmptyRosterProvider))
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build(),
    )
    .await
    .expect("fresh persistent build");

    for canonical in [
        "sessions.sqlite3",
        "runtime.sqlite",
        "continuity.sqlite3",
        "mobkit_metadata.sqlite3",
        "mobkit_console.sqlite3",
    ] {
        assert!(
            state.join(canonical).exists(),
            "fresh boot must create canonical {canonical}"
        );
    }
    for legacy in [
        "sessions.db",
        "sessions.sqlite",
        "continuity.db",
        "identity_continuity.sqlite",
        "mobkit_metadata.sqlite",
        "mobkit_console.sqlite",
    ] {
        assert!(
            !state.join(legacy).exists(),
            "fresh boot must not create legacy {legacy}"
        );
    }

    runtime.mob_handle().stop().await.expect("stop");
}

#[tokio::test]
async fn builder_keeps_using_a_legacy_named_state_dir_where_it_lies() {
    let tmp = TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&state).expect("state dir");
    // The builder's own historical spellings (recon census): SQLite opens
    // the zero-byte seeds as empty databases and initializes them in place.
    touch(&state.join("sessions.db"));
    touch(&state.join("identity_continuity.sqlite"));
    touch(&state.join("mobkit_metadata.sqlite"));
    touch(&state.join("mobkit_console.sqlite"));

    let runtime = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .persistent_state(&state)
            .roster_provider(Arc::new(EmptyRosterProvider))
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build(),
    )
    .await
    .expect("legacy-named persistent build");

    for (legacy, canonical) in [
        ("sessions.db", "sessions.sqlite3"),
        ("identity_continuity.sqlite", "continuity.sqlite3"),
        ("mobkit_metadata.sqlite", "mobkit_metadata.sqlite3"),
        ("mobkit_console.sqlite", "mobkit_console.sqlite3"),
    ] {
        assert!(
            state.join(legacy).exists(),
            "legacy {legacy} must keep being used where it lies"
        );
        assert!(
            !state.join(canonical).exists(),
            "boot must not create a canonical twin {canonical} beside {legacy}"
        );
        assert!(
            std::fs::metadata(state.join(legacy))
                .expect("metadata")
                .len()
                > 0,
            "legacy {legacy} must have been opened and initialized in place"
        );
    }

    runtime.mob_handle().stop().await.expect("stop");
}

#[tokio::test]
async fn builder_refuses_session_file_name_twins() {
    let tmp = TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&state).expect("state dir");
    touch(&state.join("sessions.db"));
    touch(&state.join("sessions.sqlite3"));

    let error = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .persistent_state(&state)
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build(),
    )
    .await
    .err()
    .expect("twin spellings must refuse to boot");
    let message = error.to_string();
    assert!(
        message.contains("file-name twins") && message.contains("sessions"),
        "twins refusal must name the slot and point at the doctor: {message}"
    );
}

// ---------------------------------------------------------------------------
// rpc_gateway (SDK stdio surface)
// ---------------------------------------------------------------------------

const RPC_MOB_CONFIG: &str = r#"
[mob]
id = "storage-layout-rpc-test"

[profiles.default]
model = "gpt-5.5"
"#;

/// Spawn the real `rpc_gateway --persistent`, send one `mobkit/init` with
/// `persistent_state`, and return the init response.
fn rpc_gateway_init(state_dir: &Path) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rpc_gateway"))
        .arg("--persistent")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn rpc_gateway");
    let mut stdin = child.stdin.take().expect("gateway stdin");
    let init = json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "mobkit/init",
        "params": {
            "persistent_state": state_dir,
            "mob_config": RPC_MOB_CONFIG,
        }
    });
    writeln!(
        stdin,
        "{}",
        serde_json::to_string(&init).expect("init json")
    )
    .expect("write init");
    stdin.flush().expect("flush init");

    let stdout = child.stdout.take().expect("gateway stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx
        .recv_timeout(Duration::from_secs(90))
        .expect("rpc_gateway did not answer mobkit/init");
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    serde_json::from_str(line.trim()).expect("init response json")
}

#[test]
fn rpc_gateway_fresh_state_dir_converges_on_canonical_names() {
    let state = TempDir::new().expect("state dir");
    let response = rpc_gateway_init(state.path());
    assert!(
        response.get("error").is_none(),
        "fresh init must succeed: {response}"
    );
    for canonical in [
        "sessions.sqlite3",
        "runtime.sqlite",
        "mobkit_metadata.sqlite3",
        "mobkit_console.sqlite3",
    ] {
        assert!(
            state.path().join(canonical).exists(),
            "fresh boot must create canonical {canonical}"
        );
    }
    for legacy in [
        "sessions.db",
        "mobkit_metadata.sqlite",
        "mobkit_console.sqlite",
    ] {
        assert!(
            !state.path().join(legacy).exists(),
            "fresh boot must not create legacy {legacy}"
        );
    }
}

#[test]
fn rpc_gateway_keeps_using_a_legacy_named_state_dir_where_it_lies() {
    let state = TempDir::new().expect("state dir");
    // rpc_gateway's own historical spellings (recon census).
    touch(&state.path().join("sessions.db"));
    touch(&state.path().join("mobkit_metadata.sqlite"));
    touch(&state.path().join("mobkit_console.sqlite"));

    let response = rpc_gateway_init(state.path());
    assert!(
        response.get("error").is_none(),
        "legacy-named init must succeed: {response}"
    );
    for (legacy, canonical) in [
        ("sessions.db", "sessions.sqlite3"),
        ("mobkit_metadata.sqlite", "mobkit_metadata.sqlite3"),
        ("mobkit_console.sqlite", "mobkit_console.sqlite3"),
    ] {
        assert!(
            state.path().join(legacy).exists(),
            "legacy {legacy} must keep being used where it lies"
        );
        assert!(
            !state.path().join(canonical).exists(),
            "boot must not create a canonical twin {canonical} beside {legacy}"
        );
    }
}

#[test]
fn rpc_gateway_refuses_session_file_name_twins() {
    let state = TempDir::new().expect("state dir");
    touch(&state.path().join("sessions.db"));
    touch(&state.path().join("sessions.sqlite3"));

    let response = rpc_gateway_init(state.path());
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("file-name twins") && message.contains("sessions"),
        "twin spellings must fail init with the twins refusal: {response}"
    );
}

// ---------------------------------------------------------------------------
// mobkit_gateway (console/HTTP surface)
// ---------------------------------------------------------------------------

/// Spawn the real `mobkit_gateway`, send one `mobkit/init` pointed at
/// `store_dir` with `persistent_sessions: true`, and return the init
/// response. XDG state is isolated per test (shared-home CI flake class).
fn mobkit_gateway_init(workspace: &Path, store_dir: &Path) -> Value {
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "mobkit/init",
        "params": {
            "workspace_root": workspace,
            "store_path": store_dir,
            "persistent_sessions": true,
        },
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_mobkit_gateway"))
        .current_dir(workspace)
        // Dummy provider secrets: the fallback mob's LLM client is created
        // at bootstrap but never called (mirrors mobkit_gateway_bootstrap).
        .env("ANTHROPIC_API_KEY", "sk-ant-regression-test")
        .env("OPENAI_API_KEY", "sk-regression-test")
        .env("XDG_STATE_HOME", workspace.join("xdg-state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mobkit_gateway");

    let mut stdin = child.stdin.take().expect("stdin");
    writeln!(
        stdin,
        "{}",
        serde_json::to_string(&init).expect("init json")
    )
    .expect("write init");
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
        .expect("mobkit_gateway did not answer mobkit/init");
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    serde_json::from_str(line.trim()).expect("init response json")
}

#[test]
fn mobkit_gateway_fresh_store_dir_converges_on_canonical_names() {
    let workspace = TempDir::new().expect("workspace");
    let store = TempDir::new().expect("store");
    let store_dir = store.path().join("store");

    let response = mobkit_gateway_init(workspace.path(), &store_dir);
    assert!(
        response.get("error").is_none(),
        "fresh init must succeed: {response}"
    );
    for canonical in ["sessions.sqlite3", "runtime.sqlite", "continuity.sqlite3"] {
        assert!(
            store_dir.join(canonical).exists(),
            "fresh boot must create canonical {canonical}"
        );
    }
    for legacy in ["sessions.sqlite", "sessions.db", "continuity.db"] {
        assert!(
            !store_dir.join(legacy).exists(),
            "fresh boot must not create legacy {legacy}"
        );
    }
}

#[test]
fn mobkit_gateway_keeps_using_a_legacy_named_store_dir_where_it_lies() {
    let workspace = TempDir::new().expect("workspace");
    let store = TempDir::new().expect("store");
    let store_dir = store.path().join("store");
    std::fs::create_dir_all(&store_dir).expect("store dir");
    // mobkit_gateway's own historical spellings (recon census).
    touch(&store_dir.join("sessions.sqlite"));
    touch(&store_dir.join("continuity.db"));

    let response = mobkit_gateway_init(workspace.path(), &store_dir);
    assert!(
        response.get("error").is_none(),
        "legacy-named init must succeed: {response}"
    );
    for (legacy, canonical) in [
        ("sessions.sqlite", "sessions.sqlite3"),
        ("continuity.db", "continuity.sqlite3"),
    ] {
        assert!(
            store_dir.join(legacy).exists(),
            "legacy {legacy} must keep being used where it lies"
        );
        assert!(
            !store_dir.join(canonical).exists(),
            "boot must not create a canonical twin {canonical} beside {legacy}"
        );
    }
}
