//! Gateway maintenance subcommand contract (storage-unification H3):
//! `mobkit_gateway storage-adopt-checkpoints (--db <path> | --state-dir <dir>)
//! [--apply] [--json]`.
//!
//! The verb bypasses the stdin init handshake entirely, resolves the
//! continuity database (explicit `--db`, or canonical-name-first probing over
//! `--state-dir`), runs the H3 adoption walk, prints the report (text or
//! JSON), and exits 0 only when the walk is clean. Dry-run is the default;
//! the database is mutated only with `--apply`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// The 0.7.x-era continuity DDL (no `meerkat_schema` ledger): what pre-M3
/// binaries actually wrote, which is exactly the population the verb heals.
const LEGACY_DDL: &str = "CREATE TABLE continuity_records (
        identity       TEXT PRIMARY KEY,
        agent_runtime_id TEXT NOT NULL,
        session_id     TEXT NOT NULL,
        generation     INTEGER NOT NULL,
        checkpoint_version INTEGER NOT NULL,
        fencing_token  INTEGER NOT NULL
    );
    CREATE TABLE session_snapshots (
        session_id     TEXT PRIMARY KEY,
        identity       TEXT NOT NULL,
        generation     INTEGER NOT NULL,
        checkpoint_version INTEGER NOT NULL,
        fencing_token  INTEGER NOT NULL,
        data           BLOB NOT NULL
    );";

fn legacy_session_bytes() -> (String, Vec<u8>) {
    let session = meerkat_core::Session::new();
    let id = session.id().to_string();
    let bytes = serde_json::to_vec(&session).expect("serialize legacy session");
    (id, bytes)
}

/// Create a fixture continuity db at `path` with one adoptable
/// legacy-unverified snapshot bound to a matching record at generation 3 /
/// checkpoint version 4. Returns the session id.
fn seed_adoptable_db(path: &Path) -> String {
    let conn = rusqlite::Connection::open(path).expect("create fixture db");
    conn.execute_batch(LEGACY_DDL).expect("apply legacy ddl");
    let (sid, legacy) = legacy_session_bytes();
    conn.execute(
        "INSERT INTO continuity_records \
         (identity, agent_runtime_id, session_id, generation, checkpoint_version, fencing_token) \
         VALUES ('test:alice', 'rt-1', ?1, 3, 4, 7)",
        [&sid],
    )
    .expect("insert record");
    conn.execute(
        "INSERT INTO session_snapshots \
         (session_id, identity, generation, checkpoint_version, fencing_token, data) \
         VALUES (?1, 'test:alice', 3, 4, 7, ?2)",
        rusqlite::params![sid, legacy],
    )
    .expect("insert snapshot");
    sid
}

fn file_digest(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("read db file");
    format!("{:x}", Sha256::digest(&bytes))
}

struct VerbOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_verb(args: &[&str]) -> VerbOutput {
    let output = Command::new(env!("CARGO_BIN_EXE_mobkit_gateway"))
        .arg("storage-adopt-checkpoints")
        .args(args)
        .output()
        .expect("spawn mobkit_gateway");
    VerbOutput {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn report_json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap_or_else(|error| {
        panic!("stdout is not a JSON report ({error}): {stdout}");
    })
}

#[test]
fn dry_run_is_the_default_and_never_mutates_the_database() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("continuity.db");
    seed_adoptable_db(&db);
    let before = file_digest(&db);

    let run = run_verb(&["--db", db.to_str().unwrap(), "--json"]);
    assert_eq!(
        run.code, 0,
        "clean dry-run must exit 0; stderr: {}",
        run.stderr
    );
    let report = report_json(&run.stdout);
    assert_eq!(report["scanned"], 1);
    assert_eq!(
        report["adopted"], 1,
        "dry-run counts what apply would adopt"
    );
    assert_eq!(report["refused"].as_array().map(Vec::len), Some(0));

    assert_eq!(
        file_digest(&db),
        before,
        "without --apply the database must stay byte-identical"
    );
}

#[test]
fn apply_mutates_binds_the_observed_cursor_and_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("continuity.db");
    let sid = seed_adoptable_db(&db);
    let before = file_digest(&db);

    let first = run_verb(&["--db", db.to_str().unwrap(), "--apply", "--json"]);
    assert_eq!(first.code, 0, "stderr: {}", first.stderr);
    let report = report_json(&first.stdout);
    assert_eq!(report["adopted"], 1);
    assert_ne!(file_digest(&db), before, "--apply must rewrite the row");

    // The rewritten payload carries the observed cursor; the row's
    // bookkeeping columns are untouched.
    {
        let conn = rusqlite::Connection::open(&db).expect("reopen fixture");
        let (generation, version, fence, data): (u64, u64, u64, Vec<u8>) = conn
            .query_row(
                "SELECT generation, checkpoint_version, fencing_token, data \
                 FROM session_snapshots WHERE session_id = ?1",
                [&sid],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .expect("adopted row");
        assert_eq!((generation, version, fence), (3, 4, 7));
        let session: meerkat_core::Session =
            serde_json::from_slice(&data).expect("decode adopted document");
        match session.try_checkpoint_state().expect("checkpoint state") {
            meerkat_core::SessionCheckpointState::Verified(stamp) => {
                assert_eq!(stamp.generation(), meerkat_core::SessionGeneration::new(3));
                assert_eq!(
                    stamp.checkpoint_revision(),
                    meerkat_core::SessionCheckpointRevision::new(4)
                );
            }
            other => panic!("expected a verified document, got {other:?}"),
        }
    }

    let after_first = file_digest(&db);
    let second = run_verb(&["--db", db.to_str().unwrap(), "--apply", "--json"]);
    assert_eq!(second.code, 0, "stderr: {}", second.stderr);
    let report = report_json(&second.stdout);
    assert_eq!(report["already_stamped"], 1);
    assert_eq!(report["adopted"], 0);
    assert_eq!(
        file_digest(&db),
        after_first,
        "a second apply must be a byte-identical no-op"
    );
}

#[test]
fn state_dir_probing_resolves_canonical_and_legacy_spellings() {
    // Canonical spelling.
    let dir = tempfile::tempdir().expect("tempdir");
    let canonical = dir.path().join("continuity.sqlite3");
    seed_adoptable_db(&canonical);
    let run = run_verb(&["--state-dir", dir.path().to_str().unwrap(), "--json"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(report_json(&run.stdout)["adopted"], 1);

    // Legacy spelling probes too (canonical-name-first resolver).
    let dir = tempfile::tempdir().expect("tempdir");
    let legacy = dir.path().join("continuity.db");
    seed_adoptable_db(&legacy);
    let run = run_verb(&["--state-dir", dir.path().to_str().unwrap(), "--json"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(report_json(&run.stdout)["adopted"], 1);

    // Twins refuse, exit 1 — the resolver never picks one silently.
    let dir = tempfile::tempdir().expect("tempdir");
    seed_adoptable_db(&dir.path().join("continuity.sqlite3"));
    seed_adoptable_db(&dir.path().join("continuity.db"));
    let run = run_verb(&["--state-dir", dir.path().to_str().unwrap()]);
    assert_eq!(run.code, 1, "twins must refuse; stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("file-name twins"),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn refusals_and_operational_failures_exit_1_usage_errors_exit_2() {
    // Refusal: a decodable session document filed under the wrong row key.
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("continuity.db");
    {
        let conn = rusqlite::Connection::open(&db).expect("create fixture db");
        conn.execute_batch(LEGACY_DDL).expect("apply legacy ddl");
        let (_, legacy) = legacy_session_bytes();
        conn.execute(
            "INSERT INTO continuity_records \
             (identity, agent_runtime_id, session_id, generation, checkpoint_version, fencing_token) \
             VALUES ('test:alice', 'rt-1', 'session-wrong-key', 0, 1, 1)",
            [],
        )
        .expect("insert record");
        conn.execute(
            "INSERT INTO session_snapshots \
             (session_id, identity, generation, checkpoint_version, fencing_token, data) \
             VALUES ('session-wrong-key', 'test:alice', 0, 1, 1, ?1)",
            rusqlite::params![legacy],
        )
        .expect("insert snapshot");
    }
    let run = run_verb(&["--db", db.to_str().unwrap(), "--json"]);
    assert_eq!(run.code, 1, "refusals must exit 1; stderr: {}", run.stderr);
    let report = report_json(&run.stdout);
    assert_eq!(report["refused"].as_array().map(Vec::len), Some(1));

    // Operational failure: missing database.
    let missing: PathBuf = dir.path().join("missing.db");
    let run = run_verb(&["--db", missing.to_str().unwrap()]);
    assert_eq!(
        run.code, 1,
        "missing db must exit 1; stdout: {}",
        run.stdout
    );

    // Usage errors: no source, unknown flag.
    let run = run_verb(&[]);
    assert_eq!(run.code, 2, "missing --db/--state-dir must exit 2");
    assert!(run.stderr.contains("usage:"), "stderr: {}", run.stderr);
    let run = run_verb(&["--frobnicate"]);
    assert_eq!(run.code, 2, "unknown flags must exit 2");
}
