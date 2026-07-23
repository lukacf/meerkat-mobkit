//! Legacy-data axis: synthetic fixtures with genuine schema scar tissue.
//!
//! Release-day incidents have been legacy-shape issues, not fresh-store
//! bugs. These chapters fabricate the exact on-disk shapes older MobKit
//! lines persisted and pin that today's bundled stores open them, preserve
//! their rows, and keep their write discipline. Both chapters are scoped to
//! the bundled implementations by construction — they fabricate those
//! stores' schemas.

use std::path::Path;

use meerkat_core::SessionCheckpointState;
use meerkat_mobkit::identity_first::{
    AgentIdentity, AgentMemoryProvider, CheckpointVersion, ContinuityGeneration,
    ContinuityResolveState, ContinuityStore, ContinuityStoreError, FencingToken,
    LocalContinuityStore, NewAgentMemory, SessionSnapshot,
};
use meerkat_mobkit::memory::SqliteAgentMemoryStore;
use meerkat_store_conformance::ConformanceFailure;

use crate::fixtures;
use crate::steps::Steps;

/// The pre-ledger continuity schema exactly as `LocalContinuityStore` has
/// always created it (`identity_first/local_store.rs` DDL): two tables, no
/// schema ledger, no secondary indexes.
const LEGACY_CONTINUITY_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS continuity_records (
        identity       TEXT PRIMARY KEY,
        agent_runtime_id TEXT NOT NULL,
        session_id     TEXT NOT NULL,
        generation     INTEGER NOT NULL,
        checkpoint_version INTEGER NOT NULL,
        fencing_token  INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS session_snapshots (
        session_id     TEXT PRIMARY KEY,
        identity       TEXT NOT NULL,
        generation     INTEGER NOT NULL,
        checkpoint_version INTEGER NOT NULL,
        fencing_token  INTEGER NOT NULL,
        data           BLOB NOT NULL
    );";

/// A pre-ledger continuity database holding a session snapshot whose payload
/// is an UNSTAMPED current-envelope Meerkat session (the exact bytes a 0.7.x
/// fleet persisted) must open, resolve its record, serve the snapshot bytes
/// unchanged (still `LegacyUnverified` — adoption is H3's job, never a side
/// effect of open), keep its fencing/version CAS, and report the persisted
/// fencing high-water on open.
pub async fn legacy_continuity_database(dir: &Path) -> Result<(), ConformanceFailure> {
    let steps = Steps::chapter("legacy_continuity_database");
    let db_path = dir.join("continuity.db");

    // --- fabricate the legacy database ---------------------------------------
    let step = "fabricate_legacy_database";
    let session = fixtures::session_with_texts(&["legacy turn one", "legacy turn two"]);
    let legacy_blob = fixtures::legacy_session_blob(&session)?;
    let session_key = session.id().to_string();
    {
        let connection = steps.wrap(step, rusqlite::Connection::open(&db_path))?;
        steps.wrap(step, connection.execute_batch(LEGACY_CONTINUITY_SCHEMA))?;
        steps.wrap(
            step,
            connection.execute(
                "INSERT INTO continuity_records \
                 (identity, agent_runtime_id, session_id, generation, checkpoint_version, \
                  fencing_token) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["legacy:main", "rt-legacy", session_key, 3_u64, 7_u64, 9_u64],
            ),
        )?;
        steps.wrap(
            step,
            connection.execute(
                "INSERT INTO session_snapshots \
                 (session_id, identity, generation, checkpoint_version, fencing_token, data) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![session_key, "legacy:main", 3_u64, 7_u64, 9_u64, legacy_blob],
            ),
        )?;
    }

    // --- open + fencing floor ---------------------------------------------------
    let step = "open_reports_persisted_fencing_floor";
    let (store, floor) = steps.wrap(
        step,
        LocalContinuityStore::open_with_fencing_floor(db_path.clone()).await,
    )?;
    steps.ensure(
        step,
        floor == 9,
        format!("open must report the persisted fencing high-water (expected 9, got {floor})"),
    )?;

    // --- record resolves ----------------------------------------------------------
    let step = "legacy_record_resolves";
    let identity = steps.wrap(step, AgentIdentity::parse("legacy:main"))?;
    let resolved = steps.wrap(
        step,
        store.resolve_many(std::slice::from_ref(&identity)).await,
    )?;
    match resolved.get(&identity) {
        Some(ContinuityResolveState::Ready { record }) => {
            steps.ensure(
                step,
                record.session_id == *session.id()
                    && record.generation.get() == 3
                    && record.checkpoint_version.get() == 7,
                format!("the legacy record must resolve unchanged, got {record:?}"),
            )?;
        }
        other => {
            return Err(steps.fail(
                step,
                format!("the legacy record must resolve Ready, got {other:?}"),
            ));
        }
    }

    // --- snapshot bytes served unchanged --------------------------------------------
    let step = "legacy_snapshot_bytes_unchanged";
    let snapshot = steps
        .wrap(step, store.load_session_snapshot(session.id()).await)?
        .ok_or_else(|| steps.fail(step, "the legacy snapshot must load"))?;
    steps.ensure(
        step,
        snapshot.data == legacy_blob,
        "the legacy snapshot bytes must be served byte-for-byte unchanged — open must never \
         rewrite or adopt them (adoption is H3's explicit maintenance job)",
    )?;
    let parsed: meerkat_core::Session = steps.wrap(step, serde_json::from_slice(&snapshot.data))?;
    steps.ensure(
        step,
        matches!(
            steps.wrap(step, parsed.try_checkpoint_state())?,
            SessionCheckpointState::LegacyUnverified { .. }
        ),
        "the served legacy document must still report LegacyUnverified",
    )?;

    // --- CAS still works over the legacy rows ------------------------------------------
    let step = "cas_still_enforced";
    let mut advanced = session;
    fixtures::push_text(&mut advanced, "post-upgrade turn");
    let advanced_snapshot = fixtures::session_snapshot(&advanced)?;
    steps.wrap(
        step,
        store
            .save_session_snapshot(
                &identity,
                advanced.id(),
                ContinuityGeneration::new(3),
                CheckpointVersion::new(8),
                FencingToken::new(9),
                &advanced_snapshot,
            )
            .await,
    )?;
    match store
        .save_session_snapshot(
            &identity,
            advanced.id(),
            ContinuityGeneration::new(3),
            CheckpointVersion::new(9),
            FencingToken::new(8),
            &SessionSnapshot {
                data: advanced_snapshot.data.clone(),
            },
        )
        .await
    {
        Err(ContinuityStoreError::StaleFencingToken { .. }) => {}
        Err(other) => {
            return Err(steps.fail(
                step,
                format!("a stale fence must stay rejected on a legacy database, got: {other}"),
            ));
        }
        Ok(()) => {
            return Err(steps.fail(
                step,
                "a stale fence must stay rejected on a legacy database",
            ));
        }
    }
    Ok(())
}

/// The pre-`ever_quarantined` / pre-`proposals.taint` agent-memory schema:
/// `SCHEMA_SQL` as it existed before those columns, so opening the store
/// exercises the `ensure_column` upgrade path.
const LEGACY_MEMORY_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS records (
    memory_id       TEXT PRIMARY KEY,
    scope_kind      TEXT NOT NULL,
    scope_key       TEXT NOT NULL,
    kind            TEXT NOT NULL,
    title           TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    body            TEXT NOT NULL,
    tags            TEXT NOT NULL DEFAULT '[]',
    provenance      TEXT NOT NULL,
    trust           TEXT NOT NULL,
    status_kind     TEXT NOT NULL,
    status_detail   TEXT,
    supersedes      TEXT,
    derived_from    TEXT NOT NULL DEFAULT '[]',
    working_set_rank INTEGER,
    rank_set_at_ms  INTEGER,
    content_hash    TEXT NOT NULL,
    created_at_ms   INTEGER NOT NULL,
    updated_at_ms   INTEGER NOT NULL,
    usage_stats     TEXT NOT NULL DEFAULT '{}',
    tombstoned_at_ms INTEGER
);
CREATE INDEX IF NOT EXISTS records_scope_idx
    ON records(scope_kind, scope_key, status_kind);
CREATE INDEX IF NOT EXISTS records_scope_hash_idx
    ON records(scope_kind, scope_key, content_hash);

CREATE TABLE IF NOT EXISTS proposals (
    proposal_id   TEXT PRIMARY KEY,
    scope_kind    TEXT NOT NULL,
    scope_key     TEXT NOT NULL,
    record        TEXT NOT NULL,
    author        TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS audit (
    audit_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    stage_token   TEXT NOT NULL,
    op_index      INTEGER NOT NULL,
    op_kind       TEXT NOT NULL,
    memory_id     TEXT,
    detail        TEXT NOT NULL,
    applied_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS stage (
    token         TEXT PRIMARY KEY,
    batch         TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);
";

/// The load-bearing backfill sentinel. Preserved byte-for-byte from
/// `memory/sqlite_store.rs`; operator flows key off this exact string.
const TAINT_SENTINEL: &str = "pre-migration proposal: propose-time taint fact unrecoverable";

/// A memory-realm database with genuine `ensure_column` scar tissue: rows
/// written before `records.ever_quarantined` and `proposals.taint` existed.
/// Opening the store must add the columns, preserve every legacy row
/// unchanged, backfill `ever_quarantined = 1` for quarantined rows, and
/// stamp pending proposals with the exact taint sentinel — applied rows stay
/// NULL.
pub async fn legacy_memory_database(root: &Path) -> Result<(), ConformanceFailure> {
    let steps = Steps::chapter("legacy_memory_database");
    // The bundled store keys one database per realm:
    // `<root>/<encoded-realm>.sqlite3`; "default" encodes to itself.
    let db_path = root.join("default.sqlite3");

    // --- fabricate the legacy realm database -----------------------------------
    let step = "fabricate_legacy_database";
    {
        let connection = steps.wrap(step, rusqlite::Connection::open(&db_path))?;
        steps.wrap(step, connection.execute_batch(LEGACY_MEMORY_SCHEMA))?;
        for (memory_id, title, body, status_kind, hash) in [
            (
                "legacy-mem-active",
                "Legacy active title",
                "legacy active body",
                "active",
                "legacy-hash-active",
            ),
            (
                "legacy-mem-quarantined",
                "Legacy quarantined title",
                "legacy quarantined body",
                "quarantined",
                "legacy-hash-quarantined",
            ),
        ] {
            steps.wrap(
                step,
                connection.execute(
                    "INSERT INTO records \
                     (memory_id, scope_kind, scope_key, kind, title, description, body, tags, \
                      provenance, trust, status_kind, content_hash, created_at_ms, \
                      updated_at_ms, usage_stats) \
                     VALUES (?1, 'identity', 'conformance:memory', 'fact', ?2, '', ?3, '[]', \
                             '{\"source\":\"legacy-fixture\"}', 'application', ?4, ?5, 1000, \
                             1000, '{}')",
                    rusqlite::params![memory_id, title, body, status_kind, hash],
                ),
            )?;
        }
        for (proposal_id, status) in [
            ("legacy-prop-pending", "pending"),
            ("legacy-prop-applied", "applied"),
        ] {
            steps.wrap(
                step,
                connection.execute(
                    "INSERT INTO proposals \
                     (proposal_id, scope_kind, scope_key, record, author, status, created_at_ms) \
                     VALUES (?1, 'identity', 'conformance:memory', '{}', 'application', ?2, 1000)",
                    rusqlite::params![proposal_id, status],
                ),
            )?;
        }
    }

    // --- open the store and force the realm connection (ensure_column runs) -----
    let step = "open_runs_ensure_column_upgrade";
    let store = steps.wrap(step, SqliteAgentMemoryStore::open(root))?;
    let identity = steps.wrap(step, AgentIdentity::parse("conformance:upgrade"))?;
    steps.wrap(
        step,
        store
            .remember(
                "default",
                &identity,
                NewAgentMemory {
                    title: "Upgrade trigger".to_string(),
                    body: "Written to force the realm connection open.".to_string(),
                    tags: Vec::new(),
                },
            )
            .await,
    )?;

    // --- legacy rows preserved, backfills applied ---------------------------------
    let step = "rows_preserved_and_backfilled";
    let connection = steps.wrap(step, rusqlite::Connection::open(&db_path))?;
    let (title, body, quarantined_flag): (String, String, i64) = steps.wrap(
        step,
        connection.query_row(
            "SELECT title, body, ever_quarantined FROM records WHERE memory_id = ?1",
            rusqlite::params!["legacy-mem-quarantined"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ),
    )?;
    steps.ensure(
        step,
        title == "Legacy quarantined title" && body == "legacy quarantined body",
        "legacy record content must survive the ensure_column upgrade unchanged",
    )?;
    steps.ensure(
        step,
        quarantined_flag == 1,
        "the ever_quarantined backfill must mark quarantined legacy rows",
    )?;
    let active_flag: i64 = steps.wrap(
        step,
        connection.query_row(
            "SELECT ever_quarantined FROM records WHERE memory_id = ?1",
            rusqlite::params!["legacy-mem-active"],
            |row| row.get(0),
        ),
    )?;
    steps.ensure(
        step,
        active_flag == 0,
        "active legacy rows must keep the ever_quarantined default of 0",
    )?;

    let step = "taint_sentinel_byte_for_byte";
    let pending_taint: Option<String> = steps.wrap(
        step,
        connection.query_row(
            "SELECT taint FROM proposals WHERE proposal_id = ?1",
            rusqlite::params!["legacy-prop-pending"],
            |row| row.get(0),
        ),
    )?;
    steps.ensure(
        step,
        pending_taint.as_deref() == Some(TAINT_SENTINEL),
        format!(
            "pending legacy proposals must carry the exact backfill sentinel {TAINT_SENTINEL:?}, \
             got {pending_taint:?} — operator flows key off these bytes"
        ),
    )?;
    let applied_taint: Option<String> = steps.wrap(
        step,
        connection.query_row(
            "SELECT taint FROM proposals WHERE proposal_id = ?1",
            rusqlite::params!["legacy-prop-applied"],
            |row| row.get(0),
        ),
    )?;
    steps.ensure(
        step,
        applied_taint.is_none(),
        "non-pending legacy proposals must stay untainted (NULL)",
    )?;
    Ok(())
}
