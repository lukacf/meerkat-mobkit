//! Gateway maintenance subcommand contract (storage-unification M6):
//! `mobkit_gateway storage-migrate --state-dir <dir> [--apply] [--adopt
//! <path>] [--json]` and `mobkit_gateway storage-prune --state-dir <dir>
//! [--apply] [--older-than-days N] [--json]`.
//!
//! Both verbs bypass the stdin init handshake entirely and are dry-run by
//! default; the state directory is mutated only with `--apply`. Exit codes:
//! 0 clean, 1 refusals or fence/store failures, 2 usage errors.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

/// The 0.7.x-era continuity DDL (no `meerkat_schema` ledger): what pre-M3
/// binaries actually wrote — exactly the population the verb converges.
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

/// Create a legacy-named continuity fixture with one adoptable
/// legacy-unverified snapshot (matching record at generation 3 / checkpoint
/// version 4). Returns the session id.
fn seed_legacy_continuity(path: &Path) -> String {
    let conn = rusqlite::Connection::open(path).expect("create fixture db");
    conn.execute_batch(LEGACY_DDL).expect("apply legacy ddl");
    let session = meerkat_core::Session::new();
    let sid = session.id().to_string();
    let legacy = serde_json::to_vec(&session).expect("serialize legacy session");
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

fn run_verb(verb: &str, args: &[&str]) -> VerbOutput {
    let output = Command::new(env!("CARGO_BIN_EXE_mobkit_gateway"))
        .arg(verb)
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
fn migrate_dry_run_is_the_default_and_never_mutates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let legacy = dir.path().join("continuity.db");
    seed_legacy_continuity(&legacy);
    let before = file_digest(&legacy);

    let run = run_verb(
        "storage-migrate",
        &["--state-dir", dir.path().to_str().unwrap(), "--json"],
    );
    assert_eq!(
        run.code, 0,
        "clean dry-run must exit 0; stderr: {}",
        run.stderr
    );
    let report = report_json(&run.stdout);
    assert_eq!(report["mode"], "dry_run");
    assert_eq!(report["renames"][0]["action"], "would-rename");
    assert_eq!(
        report["ledger"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["domain"] == "mobkit-continuity")
            .expect("continuity ledger entry")["action"],
        "would-stamp"
    );
    assert_eq!(report["adoption"]["report"]["adopted"], 1);

    assert!(legacy.is_file(), "dry-run must not rename");
    assert_eq!(
        file_digest(&legacy),
        before,
        "without --apply the database must stay byte-identical"
    );
}

#[test]
fn migrate_apply_renames_stamps_adopts_and_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let legacy = dir.path().join("continuity.db");
    let sid = seed_legacy_continuity(&legacy);

    let first = run_verb(
        "storage-migrate",
        &[
            "--state-dir",
            dir.path().to_str().unwrap(),
            "--apply",
            "--json",
        ],
    );
    assert_eq!(first.code, 0, "stderr: {}", first.stderr);
    let report = report_json(&first.stdout);
    assert_eq!(report["renames"][0]["action"], "renamed");
    assert_eq!(report["adoption"]["report"]["adopted"], 1);

    let canonical = dir.path().join("continuity.sqlite3");
    assert!(canonical.is_file(), "renamed to the canonical spelling");
    assert!(!legacy.exists());
    // The adopted document is verified, at the observed cursor.
    {
        let conn = rusqlite::Connection::open(&canonical).expect("reopen");
        let data: Vec<u8> = conn
            .query_row(
                "SELECT data FROM session_snapshots WHERE session_id = ?1",
                [&sid],
                |row| row.get(0),
            )
            .expect("adopted row");
        let session: meerkat_core::Session =
            serde_json::from_slice(&data).expect("decode adopted document");
        assert!(matches!(
            session.try_checkpoint_state().expect("checkpoint state"),
            meerkat_core::SessionCheckpointState::Verified(_)
        ));
    }
    // The rename marker is a registered backup artifact prune can see.
    let marker = report["renames"][0]["marker"]
        .as_str()
        .expect("rename marker recorded");
    assert!(marker.contains(".pre-"), "{marker}");
    assert!(Path::new(marker).is_file());

    let after_first = file_digest(&canonical);
    let second = run_verb(
        "storage-migrate",
        &[
            "--state-dir",
            dir.path().to_str().unwrap(),
            "--apply",
            "--json",
        ],
    );
    assert_eq!(second.code, 0, "stderr: {}", second.stderr);
    let report = report_json(&second.stdout);
    assert_eq!(report["adoption"]["report"]["already_stamped"], 1);
    assert_eq!(report["adoption"]["report"]["adopted"], 0);
    assert!(report["renames"].as_array().unwrap().is_empty());
    assert_eq!(
        file_digest(&canonical),
        after_first,
        "a second apply must be a byte-identical no-op"
    );
}

#[test]
fn migrate_twins_refuse_then_adopt_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let legacy = dir.path().join("continuity.db");
    let canonical = dir.path().join("continuity.sqlite3");
    seed_legacy_continuity(&legacy);
    seed_legacy_continuity(&canonical);

    // Twins fail closed: exit 1, divergence report, nothing moved.
    let refused = run_verb(
        "storage-migrate",
        &["--state-dir", dir.path().to_str().unwrap(), "--json"],
    );
    assert_eq!(
        refused.code, 1,
        "twins must exit 1; stdout: {}",
        refused.stdout
    );
    let report = report_json(&refused.stdout);
    assert_eq!(report["twins"][0]["slot"], "continuity");
    assert_eq!(report["twins"][0]["resolution"]["kind"], "refused");
    assert!(legacy.is_file());
    assert!(canonical.is_file());

    // --adopt (with --apply) adopts one copy and archives the other.
    let resolved = run_verb(
        "storage-migrate",
        &[
            "--state-dir",
            dir.path().to_str().unwrap(),
            "--apply",
            "--adopt",
            canonical.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(resolved.code, 0, "stderr: {}", resolved.stderr);
    let report = report_json(&resolved.stdout);
    assert_eq!(report["twins"][0]["resolution"]["kind"], "adopted");
    assert!(!legacy.exists(), "the non-adopted copy is archived");
    assert!(canonical.is_file());
}

#[test]
fn usage_errors_exit_2() {
    let run = run_verb("storage-migrate", &[]);
    assert_eq!(run.code, 2, "missing --state-dir must exit 2");
    assert!(run.stderr.contains("usage:"), "stderr: {}", run.stderr);

    let run = run_verb("storage-migrate", &["--state-dir", "/tmp", "--frobnicate"]);
    assert_eq!(run.code, 2, "unknown flags must exit 2");

    let run = run_verb("storage-migrate", &["--state-dir", "/tmp", "--adopt", "/x"]);
    assert_eq!(run.code, 2, "--adopt without --apply must exit 2");
    assert!(run.stderr.contains("--adopt requires --apply"));

    let run = run_verb("storage-prune", &[]);
    assert_eq!(run.code, 2, "missing --state-dir must exit 2");
    let run = run_verb(
        "storage-prune",
        &["--state-dir", "/tmp", "--older-than-days", "x"],
    );
    assert_eq!(run.code, 2, "non-numeric threshold must exit 2");
}

#[test]
fn prune_dry_run_lists_and_apply_deletes_registered_artifacts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let artifact = dir.path().join("continuity.db.pre-0.0.1-1700000000.twin");
    std::fs::write(&artifact, b"backup").expect("artifact");
    let distractor = dir.path().join("continuity.sqlite3");
    std::fs::write(&distractor, b"live").expect("distractor");

    let dry = run_verb(
        "storage-prune",
        &[
            "--state-dir",
            dir.path().to_str().unwrap(),
            "--older-than-days",
            "0",
            "--json",
        ],
    );
    assert_eq!(dry.code, 0, "stderr: {}", dry.stderr);
    let report = report_json(&dry.stdout);
    assert_eq!(report["artifacts"].as_array().map(Vec::len), Some(1));
    assert_eq!(report["artifacts"][0]["action"], "would-delete");
    assert!(artifact.is_file(), "dry-run deletes nothing");

    let applied = run_verb(
        "storage-prune",
        &[
            "--state-dir",
            dir.path().to_str().unwrap(),
            "--apply",
            "--older-than-days",
            "0",
            "--json",
        ],
    );
    assert_eq!(applied.code, 0, "stderr: {}", applied.stderr);
    let report = report_json(&applied.stdout);
    assert_eq!(report["artifacts"][0]["action"], "deleted");
    assert!(!artifact.exists());
    assert!(distractor.is_file(), "unregistered names are never touched");
}

// ---------------------------------------------------------------------------
// storage-downgrade: the rollback path for the one-way mobkit-continuity v2
// ledger bump.
// ---------------------------------------------------------------------------

/// Drive a continuity database at `path` through the real upgrade: a
/// whole-document save, then delta writes that migrate it into head+rows and
/// stamp the ledger at v2. Returns the session id and its final transcript.
async fn upgrade_with_turns(path: &Path) -> (meerkat_core::types::SessionId, Vec<String>) {
    use meerkat_mobkit::identity_first::{
        AgentIdentity, CheckpointVersion, ContinuityGeneration, ContinuityRecord, ContinuityStore,
        FencingToken, LocalContinuityStore, SessionSnapshot,
    };

    let store = std::sync::Arc::new(LocalContinuityStore::open(path).expect("open"));
    let adapter = std::sync::Arc::new(
        meerkat_mobkit::identity_first::ContinuitySessionStoreAdapter::new(
            store.clone() as std::sync::Arc<dyn ContinuityStore>
        ),
    );
    let identity = AgentIdentity::parse("agent:downgrade").expect("identity");
    let mut session = meerkat_core::Session::new();
    session.push(meerkat_core::Message::User(
        meerkat_core::UserMessage::text("turn one".to_string()),
    ));
    let session_id = session.id().clone();
    store
        .upsert_continuity_record(
            &ContinuityRecord {
                identity: identity.clone(),
                agent_runtime_id: meerkat_mobkit::identity_first::AgentRuntimeId::parse(
                    "rt:agent:downgrade:0",
                )
                .expect("runtime id"),
                session_id: session_id.clone(),
                generation: ContinuityGeneration::new(0),
                checkpoint_version: CheckpointVersion::new(0),
            },
            FencingToken::new(1),
        )
        .await
        .expect("seed record");
    store
        .save_session_snapshot(
            &identity,
            &session_id,
            ContinuityGeneration::new(0),
            CheckpointVersion::new(1),
            FencingToken::new(1),
            &SessionSnapshot {
                data: serde_json::to_vec(&session).expect("serialize"),
            },
        )
        .await
        .expect("pre-upgrade whole-blob save");

    adapter
        .register_session(
            &session_id,
            meerkat_mobkit::identity_first::SessionRuntimeState {
                identity,
                generation: ContinuityGeneration::new(0),
                fencing_token: FencingToken::new(1),
                checkpoint_version: CheckpointVersion::new(1),
            },
        )
        .await
        .expect("register");

    // Post-upgrade turns through the incremental channel — the branch
    // `PersistentSessionService` takes for every boundary save once the
    // capability is advertised. The first one migrates the blob into
    // head+rows and stamps the ledger.
    let incremental = meerkat::SessionStore::as_incremental(adapter.clone())
        .expect("the bundled store advertises the delta channel");
    let root = meerkat_core::session_store::TranscriptStrandId::root();
    for text in ["turn two", "turn three"] {
        let base = session.messages().len() as u64;
        session.push(meerkat_core::Message::User(
            meerkat_core::UserMessage::text(text.to_string()),
        ));
        incremental
            .append_messages(
                &session_id,
                &root,
                base,
                &session.messages()[base as usize..],
            )
            .await
            .unwrap_or_else(|error| panic!("append {text}: {error}"));
        let stored = incremental
            .load_head(&session_id)
            .await
            .expect("stored head")
            .expect("the migrating append created the head");
        let head = meerkat_core::session_store::SessionHead::from_session(
            &session,
            root.clone(),
            stored.rewrite_count,
        )
        .expect("head");
        let token =
            meerkat_core::session_store::session_head_cas_token(&stored).expect("cas token");
        incremental
            .save_head(
                &head,
                meerkat_core::session_store::SessionHeadCas::IfToken(token),
            )
            .await
            .unwrap_or_else(|error| panic!("save head {text}: {error}"));
    }

    let texts = session
        .messages()
        .iter()
        .map(|message| format!("{message:?}"))
        .collect();
    (session_id, texts)
}

fn ledger_version(path: &Path, domain: &str) -> Option<i64> {
    let conn = rusqlite::Connection::open(path).expect("probe");
    meerkat_sqlite::domain_version(&conn, domain).expect("ledger")
}

/// The v0.8.5 continuity schema domain, frozen at the previous release's
/// version ceiling. Declared here rather than reached for inside mobkit so a
/// future migration cannot quietly raise the bar the "old binary" is held to.
fn v0_8_5_continuity_domain() -> meerkat_sqlite::SchemaDomain {
    fn base(tx: &rusqlite::Transaction<'_>) -> Result<(), rusqlite::Error> {
        tx.execute_batch(LEGACY_DDL)
    }
    meerkat_sqlite::SchemaDomain {
        name: "mobkit-continuity",
        migrations: &[meerkat_sqlite::Migration {
            version: 1,
            name: "base-schema",
            apply: base,
        }],
    }
}

/// Can a release that predates the head-canonical channel open this file?
/// This is the whole product claim of `storage-downgrade`, and a raw
/// `SELECT data FROM session_snapshots` cannot answer it: that read bypasses
/// the ledger and succeeds identically on a locked-out v2 file.
fn v0_8_5_binary_opens(path: &Path) -> Result<(), meerkat_sqlite::SqliteStoreError> {
    let conn = rusqlite::Connection::open(path).expect("probe");
    meerkat_sqlite::refuse_future_schema(&conn, &v0_8_5_continuity_domain())
}

fn head_tables_exist(path: &Path) -> bool {
    let conn = rusqlite::Connection::open(path).expect("probe");
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' \
         AND name = 'continuity_session_heads')",
        [],
        |row| row.get::<_, bool>(0),
    )
    .expect("probe")
}

/// OPERATOR-VERB PIN: `storage-downgrade` is the shipped rollback for the
/// one-way v2 bump. Dry-run by default and byte-identical; `--apply` returns
/// the file to a shape a previous release can open, KEEPING every turn taken
/// after the upgrade.
#[test]
fn downgrade_dry_run_is_the_default_and_apply_restores_a_v1_readable_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("continuity.sqlite3");
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let (session_id, expected) = runtime.block_on(upgrade_with_turns(&db));

    assert_eq!(
        ledger_version(&db, "mobkit-continuity"),
        Some(2),
        "the turns took the file head-canonical"
    );
    assert!(head_tables_exist(&db));
    // NEGATIVE CONTROL. Without this the test cannot tell "the downgrade
    // opened the door" from "the door was never shut", and the typed variant
    // is what makes it fail if the upgrade ever stops stamping.
    match v0_8_5_binary_opens(&db) {
        Err(meerkat_sqlite::SqliteStoreError::SchemaFromTheFuture { domain, .. }) => {
            assert_eq!(domain, "mobkit-continuity");
        }
        other => panic!("the upgraded file must lock out the previous release, got {other:?}"),
    }

    // Dry run: exercises the whole reconstruction, mutates nothing.
    let before = file_digest(&db);
    let dry = run_verb(
        "storage-downgrade",
        &["--state-dir", dir.path().to_str().unwrap(), "--json"],
    );
    assert_eq!(dry.code, 0, "clean dry run exits 0; stderr: {}", dry.stderr);
    let report = report_json(&dry.stdout);
    assert_eq!(report["mode"], "dry_run");
    assert_eq!(report["downgrade"]["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(report["downgrade"]["applied"], false);
    assert_eq!(
        file_digest(&db),
        before,
        "a dry run must leave the database byte-identical"
    );

    // Apply.
    let applied = run_verb(
        "storage-downgrade",
        &[
            "--state-dir",
            dir.path().to_str().unwrap(),
            "--apply",
            "--json",
        ],
    );
    assert_eq!(
        applied.code, 0,
        "clean apply exits 0; stderr: {}",
        applied.stderr
    );
    let report = report_json(&applied.stdout);
    assert_eq!(report["mode"], "apply");
    assert_eq!(report["downgrade"]["applied"], true);
    assert_eq!(report["downgrade"]["channel_dropped"], true);
    assert_eq!(report["downgrade"]["ledger_after"], 1);

    assert_eq!(ledger_version(&db, "mobkit-continuity"), Some(1));
    assert!(
        !head_tables_exist(&db),
        "the file is back to the v1 table shape"
    );
    v0_8_5_binary_opens(&db)
        .expect("the previous release must be able to open the downgraded file");

    // What a v1-shaped binary reads: the whole-document blob, carrying every
    // post-upgrade turn.
    let conn = rusqlite::Connection::open(&db).expect("probe");
    let data: Vec<u8> = conn
        .query_row(
            "SELECT data FROM session_snapshots WHERE session_id = ?1",
            [session_id.to_string()],
            |row| row.get(0),
        )
        .expect("restored blob");
    let restored: meerkat_core::Session = serde_json::from_slice(&data).expect("v1 reader decodes");
    let restored_texts: Vec<String> = restored
        .messages()
        .iter()
        .map(|message| format!("{message:?}"))
        .collect();
    assert_eq!(
        restored_texts, expected,
        "every post-upgrade turn survives the downgrade"
    );

    // Idempotent: a second apply finds nothing to undo.
    let second = run_verb(
        "storage-downgrade",
        &[
            "--state-dir",
            dir.path().to_str().unwrap(),
            "--apply",
            "--json",
        ],
    );
    assert_eq!(second.code, 0);
    let report = report_json(&second.stdout);
    assert_eq!(report["downgrade"]["sessions"].as_array().unwrap().len(), 0);
    assert_eq!(report["downgrade"]["channel_dropped"], false);
}

#[test]
fn downgrade_usage_errors_exit_2() {
    let run = run_verb("storage-downgrade", &[]);
    assert_eq!(run.code, 2, "missing --state-dir must exit 2");
    assert!(run.stderr.contains("usage:"), "stderr: {}", run.stderr);

    let run = run_verb(
        "storage-downgrade",
        &["--state-dir", "/tmp", "--frobnicate"],
    );
    assert_eq!(run.code, 2, "unknown flags must exit 2");
}
