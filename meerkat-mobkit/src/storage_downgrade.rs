//! Head-canonical continuity DOWNGRADE: the rollback path for the one-way
//! `mobkit-continuity` v1 → v2 ledger bump, behind
//! `mobkit_gateway storage-downgrade`.
//!
//! # Why this verb exists
//!
//! Incremental continuity persistence (M4b) makes head+rows the canonical
//! durable representation of a session and stamps the file's
//! `mobkit-continuity` ledger domain at v2. That stamp is deliberately
//! one-way: a binary that does not understand head rows must NOT be allowed
//! to keep writing `session_snapshots.data` while head rows are the byte
//! authority, so every older release refuses the file at open with
//! `SchemaFromTheFuture`.
//!
//! The stamp is not applied by merely launching the new release — it rides
//! the first delta write that actually creates head state (see
//! `LocalContinuityStore::delta_write`). But once the capability is
//! advertised, EVERY boundary save routes through the incremental branch, so
//! in practice the first turn stamps the file. That made the effective
//! rollback window exactly one turn, and the only documented recovery
//! "restore `continuity.*` from a backup taken before that turn" — which
//! means discarding every turn taken since.
//!
//! This verb replaces that with a real downgrade: re-materialize every
//! head+rows session back into a whole-document `session_snapshots` blob,
//! drop the head-canonical trio, and rewind the ledger row to v1. The
//! post-upgrade turns are kept, and the previous release can open the file
//! again.
//!
//! # What this verb can NOT undo: the witness-v3 stamp door
//!
//! meerkat 0.8.9 mints session checkpoint stamps with `schema_version` 3
//! ([`meerkat_core::checkpoint::SESSION_CHECKPOINT_STAMP_SCHEMA_VERSION_WITNESS_V3`])
//! whenever the document's canonical digest folds a FORMAT-3
//! transcript-history witness. 0.8.8-and-older binaries refuse those stamps
//! typed, per DOCUMENT, regardless of how the bytes are stored — and there
//! is deliberately no API to re-mint an older-schema stamp. For such
//! documents there is no honest "downgrade to previous-release-readable" at
//! all: converting the representation would produce whole-document blobs
//! the older binary still refuses, with the success message lying about it.
//! This pass therefore probes every document the downgraded file would
//! carry (head rows it would re-materialize, plus headless snapshot blobs
//! it leaves untouched) and REFUSES — typed, per session, before any
//! destructive step — when one advertises stamp schema >= 3 or a carried
//! witness format >= 3 (see [`StampSchemaBarrier`]). This verb takes no
//! target-version argument, so the refusal is unconditional; the success
//! message only claims what the probe verified.
//!
//! # Shape
//!
//! Mirrors [`crate::storage_migrate`]: one blocking library function over a
//! state directory, run under the same [`MobKitMaintenanceFence`] (MobKit
//! builds no second fence), dry-run by default, JSON- or text-renderable
//! report, nonzero exit on refusals. The per-database transactional work is
//! [`LocalContinuityStore::downgrade_head_canonical_at`]; this module owns
//! locating the database, fencing it, and reporting.
//!
//! # What "dry run" means here
//!
//! Not an estimate. A dry run performs the ENTIRE reconstruction —
//! including the per-document reader simulation that decides fidelity — and
//! then rolls the transaction back. So a clean dry run is evidence the apply
//! run will succeed, not a guess that it might.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use meerkat_core::checkpoint::SESSION_CHECKPOINT_STAMP_SCHEMA_VERSION_WITNESS_V3;
use meerkat_core::generated::session_persistence_version_authority as persistence_versions;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::identity_first::{DowngradeFidelity, HeadCanonicalDowngrade, LocalContinuityStore};
use crate::storage_layout::MobKitStorageLayout;
use crate::storage_migrate::{MigrateMode, MobKitMaintenanceFence};

/// How long an apply run waits for in-flight store operations to drain
/// before reporting the fence as unavailable. Same budget the migrate pass
/// uses.
const FENCE_DRAIN_DEADLINE: Duration = Duration::from_secs(10);

/// The full `storage-downgrade` report for one state directory.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MobKitDowngradeReport {
    /// Dry-run or apply.
    #[serde(default)]
    pub mode: MigrateMode,
    /// The state directory operated on.
    #[serde(default)]
    pub state_dir: PathBuf,
    /// The continuity database resolved for this state dir, when one exists.
    #[serde(default)]
    pub continuity_db: Option<PathBuf>,
    /// Databases the fence covers (in apply mode, the fence actually held).
    #[serde(default)]
    pub fenced_databases: Vec<PathBuf>,
    /// The per-database transactional outcome, when the pass got that far.
    #[serde(default)]
    pub downgrade: Option<HeadCanonicalDowngrade>,
    /// Session documents whose checkpoint stamp schema (or carried witness
    /// format) is the meerkat 0.8.9+ witness-v3 one-way door. Non-empty
    /// means the pass refused BEFORE the downgrade transaction ran — the
    /// file is byte-identical — because no representation conversion can
    /// make these documents readable by older binaries. Each entry is also
    /// mirrored into [`Self::errors`].
    #[serde(default)]
    pub stamp_barriers: Vec<StampSchemaBarrier>,
    /// Refusals and failures. Non-empty means the file was NOT downgraded.
    #[serde(default)]
    pub errors: Vec<String>,
}

impl MobKitDowngradeReport {
    fn new(mode: MigrateMode, state_dir: &Path) -> Self {
        Self {
            mode,
            state_dir: state_dir.to_path_buf(),
            ..Self::default()
        }
    }

    /// Whether the pass refused or failed.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Sessions whose retained rewrite history could not be re-inlined into
    /// a document a reader accepts. Their turn content is intact; their
    /// branch/rewind history is not. Reported loudly rather than treated as
    /// a failure, because refusing the downgrade over it would leave the
    /// operator with a file no previous release can open at all.
    #[must_use]
    pub fn lossy_session_count(&self) -> usize {
        self.downgrade
            .as_ref()
            .map(|downgrade| downgrade.lossy_sessions().len())
            .unwrap_or(0)
    }

    /// Whether the run left (or, in dry run, would leave) the continuity
    /// file openable by a pre-head-canonical binary. This is the LEDGER
    /// lockout axis only; the per-document witness-v3 stamp door is gated
    /// separately and refuses the whole pass via [`Self::stamp_barriers`]
    /// before the downgrade runs, so a report with a downgrade outcome has
    /// already been verified free of that barrier.
    #[must_use]
    pub fn lockout_lifted(&self) -> bool {
        self.downgrade
            .as_ref()
            .is_some_and(HeadCanonicalDowngrade::lockout_lifted)
    }
}

/// One session document the pass refused to downgrade: its checkpoint stamp
/// (or transcript-history witness carrier) advertises the witness-v3 format
/// meerkat 0.8.9 introduced, which 0.8.8-and-older binaries refuse typed
/// per document. The barrier is the stamp/witness FORMAT, not the storage
/// representation this verb converts, so no representation downgrade can
/// lift it — refusal is the only honest outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StampSchemaBarrier {
    /// The session whose document carries the barrier.
    pub session_id: String,
    /// `schema_version` advertised by the document's checkpoint stamp,
    /// when the stamp carries a readable one.
    pub stamp_schema_version: Option<u32>,
    /// `witness_format` advertised by the document's typed
    /// transcript-history witness carrier, when the object form is present
    /// (the v2 carrier is a bare digest string and advertises nothing).
    pub witness_format: Option<u32>,
}

impl fmt::Display for StampSchemaBarrier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session {}: ", self.session_id)?;
        match (self.stamp_schema_version, self.witness_format) {
            (Some(schema), Some(witness)) => write!(
                f,
                "checkpoint stamp schema_version {schema} over a format-{witness} \
                 transcript-history witness"
            )?,
            (Some(schema), None) => write!(f, "checkpoint stamp schema_version {schema}")?,
            (None, Some(witness)) => {
                write!(f, "format-{witness} transcript-history witness carrier")?;
            }
            (None, None) => write!(f, "witness-v3 checkpoint evidence")?,
        }
        write!(
            f,
            " — this document was written by meerkat >= 0.8.9 and cannot be made readable \
             by older binaries by representation conversion alone (the stamp/witness format \
             is the barrier, not the storage representation); the downgrade was refused \
             before modifying anything. Roll the fleet forward to 0.8.9+ or restore this \
             session from a pre-0.8.9 backup."
        )
    }
}

/// The advertised format versions one stored document (or head projection)
/// carries. Advertised only — no digest re-verification: the gate is about
/// which stamps OTHER binaries will refuse, not about whether this document
/// is intact.
struct DocumentFormatProbe {
    stamp_schema_version: Option<u32>,
    witness_format: Option<u32>,
}

impl DocumentFormatProbe {
    fn barrier(&self, session_id: &str) -> Option<StampSchemaBarrier> {
        let stamp_is_v3 = self.stamp_schema_version.is_some_and(|version| {
            version >= SESSION_CHECKPOINT_STAMP_SCHEMA_VERSION_WITNESS_V3
        });
        let witness_is_v3 = self.witness_format.is_some_and(|format| {
            format >= persistence_versions::TRANSCRIPT_HISTORY_WITNESS_FORMAT
        });
        (stamp_is_v3 || witness_is_v3).then(|| StampSchemaBarrier {
            session_id: session_id.to_string(),
            stamp_schema_version: self.stamp_schema_version,
            witness_format: self.witness_format,
        })
    }
}

/// Cheap structural probe over raw document bytes: borrows the metadata map
/// as raw slices so probing never materializes the (potentially huge)
/// transcript-history state. Works for whole session documents and for
/// `SessionHead` projections alike — both carry a `metadata` object, which
/// is all this reads.
fn probe_document_format(bytes: &[u8]) -> Result<DocumentFormatProbe, serde_json::Error> {
    #[derive(Deserialize)]
    struct EnvelopeProbe<'a> {
        #[serde(default, borrow)]
        metadata: HashMap<Cow<'a, str>, &'a serde_json::value::RawValue>,
    }
    #[derive(Deserialize)]
    struct StampSchemaProbe {
        #[serde(default)]
        schema_version: Option<u32>,
    }
    #[derive(Deserialize)]
    struct WitnessCarrierProbe {
        #[serde(default)]
        witness_format: Option<u32>,
    }
    let envelope: EnvelopeProbe<'_> = serde_json::from_slice(bytes)?;
    let stamp_schema_version = envelope
        .metadata
        .get(meerkat_core::SESSION_CHECKPOINT_STAMP_KEY)
        .and_then(|raw| serde_json::from_str::<StampSchemaProbe>(raw.get()).ok())
        .and_then(|stamp| stamp.schema_version);
    // The v2 carrier is a bare digest string; only the v3+ object form
    // advertises a witness_format, so a failed object parse means "nothing
    // advertised on this axis", never an error.
    let witness_format = envelope
        .metadata
        .get(meerkat_core::SESSION_TRANSCRIPT_HISTORY_CHECKPOINT_DIGEST_KEY)
        .and_then(|raw| serde_json::from_str::<WitnessCarrierProbe>(raw.get()).ok())
        .and_then(|carrier| carrier.witness_format);
    Ok(DocumentFormatProbe {
        stamp_schema_version,
        witness_format,
    })
}

/// Scan every session document the downgraded file would leave behind for
/// the meerkat 0.8.9+ witness-v3 read barrier: head rows about to be
/// re-materialized into whole-document blobs (the re-materialized document
/// inherits the head's metadata, stamp included), plus whole-document
/// snapshot rows with no head row, which stay in the file exactly as
/// stored. Snapshot rows shadowed by a head row are deliberately out of the
/// population — the downgrade overwrites those frozen archives, so only the
/// head's own metadata decides what the downgraded document will carry.
///
/// Read-only (maintenance read profile), and run BEFORE
/// [`LocalContinuityStore::downgrade_head_canonical_at`], so a refusal
/// leaves the file byte-identical.
fn witness_v3_barriers(db_path: &Path) -> Result<Vec<StampSchemaBarrier>, String> {
    let conn = meerkat_sqlite::open(
        db_path,
        meerkat_sqlite::ConnectionProfile::Maintenance { write: false },
    )
    .map_err(|error| format!("open {} read-only: {error}", db_path.display()))?;
    let heads_present = probe_table_exists(&conn, "continuity_session_heads")?;
    let mut barriers = Vec::new();
    if heads_present {
        collect_barriers(
            &conn,
            "SELECT session_id, head_json FROM continuity_session_heads ORDER BY session_id",
            "head row",
            &mut barriers,
        )?;
    }
    if probe_table_exists(&conn, "session_snapshots")? {
        let sql = if heads_present {
            "SELECT s.session_id, s.data FROM session_snapshots AS s \
             WHERE NOT EXISTS (SELECT 1 FROM continuity_session_heads AS h \
                               WHERE h.session_id = s.session_id) \
             ORDER BY s.session_id"
        } else {
            "SELECT session_id, data FROM session_snapshots ORDER BY session_id"
        };
        collect_barriers(&conn, sql, "session snapshot", &mut barriers)?;
    }
    Ok(barriers)
}

fn collect_barriers(
    conn: &rusqlite::Connection,
    sql: &str,
    what: &str,
    barriers: &mut Vec<StampSchemaBarrier>,
) -> Result<(), String> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|error| format!("prepare {what} census: {error}"))?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)))
        .map_err(|error| format!("query {what} census: {error}"))?;
    for row in rows {
        let (session_id, bytes) = row.map_err(|error| format!("read {what} row: {error}"))?;
        // Fail closed on an unprobeable document: the pass cannot prove the
        // absence of the barrier, so it must not claim the result readable.
        let probe = probe_document_format(&bytes).map_err(|error| {
            format!(
                "{what} for session {session_id} does not probe as a session document: {error}"
            )
        })?;
        if let Some(barrier) = probe.barrier(&session_id) {
            barriers.push(barrier);
        }
    }
    Ok(())
}

fn probe_table_exists(conn: &rusqlite::Connection, table: &str) -> Result<bool, String> {
    let present: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("schema probe for {table}: {error}"))?;
    Ok(present.is_some())
}

/// Undo the head-canonical continuity upgrade for one MobKit state
/// directory.
///
/// Blocking (async callers should wrap it in `spawn_blocking`); the caller
/// renders the report and maps [`MobKitDowngradeReport::has_errors`] to a
/// nonzero exit.
///
/// Fail-closed at every step: an unresolvable layout, an unacquirable
/// fence, a ledger ahead of this binary, or head rows without the v2 stamp
/// all abort the pass with the file untouched. Additionally, any session
/// document that advertises the meerkat 0.8.9+ witness-v3 stamp schema (or
/// carried witness format) refuses the whole pass BEFORE the downgrade
/// transaction runs — that barrier is per document, older binaries refuse
/// it typed no matter how the bytes are stored, and this verb has no
/// representation-level answer to it (see [`StampSchemaBarrier`]). The
/// downgrade itself is one transaction per database, with the ledger rewind
/// as its last statement, so a crash mid-pass leaves the file exactly as
/// head-canonical as it was.
#[must_use]
pub fn downgrade_state_dir(state_dir: &Path, mode: MigrateMode) -> MobKitDowngradeReport {
    let mut report = MobKitDowngradeReport::new(mode, state_dir);
    if !state_dir.is_dir() {
        report.errors.push(format!(
            "state directory {} does not exist",
            state_dir.display()
        ));
        return report;
    }
    let apply = mode == MigrateMode::Apply;
    let layout = MobKitStorageLayout::with_injected_roots(state_dir.to_path_buf(), None);

    let resolved = match layout.continuity_db() {
        Ok(resolved) => resolved,
        Err(error) => {
            report
                .errors
                .push(format!("continuity locator unresolved: {error}"));
            return report;
        }
    };
    if !resolved.path.is_file() {
        report.errors.push(format!(
            "no continuity database at {} — nothing to downgrade",
            resolved.path.display()
        ));
        return report;
    }
    report.continuity_db = Some(resolved.path.clone());

    // The fence covers the whole state directory, not just the continuity
    // file: a downgrade that ran while a gateway held other stores open
    // would be racing the process that is about to be rolled back.
    let fence = if apply {
        match MobKitMaintenanceFence::acquire(state_dir, FENCE_DRAIN_DEADLINE) {
            Ok(fence) => {
                report.fenced_databases = fence.fenced_databases().to_vec();
                Some(fence)
            }
            Err(error) => {
                report
                    .errors
                    .push(format!("maintenance fence not acquirable: {error}"));
                return report;
            }
        }
    } else {
        None
    };

    // The witness-v3 stamp door (meerkat >= 0.8.9) is per DOCUMENT and is
    // not lifted by representation conversion, so it is gated here — before
    // the downgrade transaction — leaving a refused file byte-identical.
    // This verb takes no target-version argument, so the refusal is
    // unconditional: a downgraded file must be readable by the releases the
    // success message names, all of which predate the v3 vocabulary.
    match witness_v3_barriers(&resolved.path) {
        Ok(barriers) if barriers.is_empty() => {}
        Ok(barriers) => {
            report
                .errors
                .extend(barriers.iter().map(StampSchemaBarrier::to_string));
            report.stamp_barriers = barriers;
            drop(fence);
            return report;
        }
        Err(error) => {
            report.errors.push(format!(
                "witness-v3 preflight failed — cannot prove the file is free of \
                 meerkat >= 0.8.9 stamp/witness formats, refusing: {error}"
            ));
            drop(fence);
            return report;
        }
    }

    match LocalContinuityStore::downgrade_head_canonical_at(&resolved.path, apply) {
        Ok(downgrade) => report.downgrade = Some(downgrade),
        Err(error) => report.errors.push(format!(
            "continuity head-canonical downgrade failed: {error}"
        )),
    }
    drop(fence);
    report
}

/// Human-readable rendering of a downgrade report.
pub fn render_downgrade_report(report: &MobKitDowngradeReport) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let mode = match report.mode {
        MigrateMode::Apply => "apply",
        _ => "dry-run",
    };
    let _ = writeln!(
        out,
        "Continuity head-canonical downgrade ({mode}) over {}:",
        report.state_dir.display()
    );
    if let Some(path) = &report.continuity_db {
        let _ = writeln!(out, "  continuity database: {}", path.display());
    }
    match &report.downgrade {
        Some(downgrade) => {
            let _ = writeln!(
                out,
                "  ledger mobkit-continuity: {:?} -> {:?}",
                downgrade.ledger_before, downgrade.ledger_after
            );
            let _ = writeln!(
                out,
                "  head-canonical channel dropped: {}",
                downgrade.channel_dropped
            );
            let _ = writeln!(
                out,
                "  sessions re-materialized into whole-document snapshots: {}",
                downgrade.sessions.len()
            );
            for session in &downgrade.sessions {
                let fidelity = match &session.fidelity {
                    DowngradeFidelity::Full => "history preserved".to_string(),
                    DowngradeFidelity::NoHistory => "no history to preserve".to_string(),
                    DowngradeFidelity::HistoryDropped { reason } => {
                        format!("HISTORY DROPPED: {reason}")
                    }
                };
                let _ = writeln!(
                    out,
                    "    {} [{}] {} message(s), {} rewrite(s), {} bytes — {fidelity}",
                    session.session_id,
                    session.identity,
                    session.messages,
                    session.rewrites,
                    session.bytes
                );
            }
            let lossy = downgrade.lossy_sessions();
            if !lossy.is_empty() {
                let _ = writeln!(
                    out,
                    "  WARNING: {} session(s) lost their retained transcript rewrite history. \
                     Turn content is intact; branch/rewind history for those sessions is not.",
                    lossy.len()
                );
            }
            if downgrade.lockout_lifted() {
                let _ = writeln!(
                    out,
                    "  previous releases can open this file{} — verified free of meerkat \
                     0.8.9+ witness-v3 stamps, the per-document barrier no representation \
                     conversion lifts",
                    if downgrade.applied {
                        ""
                    } else {
                        " (after --apply; this run rolled back)"
                    }
                );
            }
        }
        None => {
            let _ = writeln!(out, "  no downgrade performed");
        }
    }
    for error in &report.errors {
        let _ = writeln!(out, "  error: {error}");
    }
    out
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use meerkat_core::{
        Message, SessionCheckpointProvenance, SessionCheckpointStamp, TranscriptRewriteReason,
        TranscriptRewriteSelection, types::UserMessage,
    };
    use rusqlite::Connection;
    use sha2::{Digest, Sha256};
    use std::fs;

    /// The v1 continuity schema exactly as the migrate-pass fixtures spell it.
    const V1_CONTINUITY_DDL: &str = "CREATE TABLE continuity_records (
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

    /// The head-canonical head table with the exact columns
    /// `LocalContinuityStore`'s `SCHEMA_HEAD_CANONICAL` creates.
    const HEAD_TABLE_DDL: &str = "CREATE TABLE continuity_session_heads (
            session_id     TEXT PRIMARY KEY,
            identity       TEXT NOT NULL,
            generation     INTEGER NOT NULL,
            checkpoint_version INTEGER NOT NULL,
            fencing_token  INTEGER NOT NULL,
            head_revision  TEXT NOT NULL,
            message_count  INTEGER NOT NULL,
            rewrite_count  INTEGER NOT NULL,
            head_json      BLOB NOT NULL,
            cas_token      TEXT NOT NULL
        );";

    /// A graph-bearing session stamped by THIS meerkat: 0.8.9 mints the
    /// witness-v3 stamp schema on such documents, which is the exact
    /// one-way door the downgrade must refuse. The assert pins the premise.
    fn witness_v3_session(tag: &str) -> meerkat_core::Session {
        let mut session = meerkat_core::Session::new();
        session.push(Message::User(UserMessage::text(format!("{tag} old one"))));
        session.push(Message::User(UserMessage::text(format!("{tag} old two"))));
        session
            .commit_transcript_rewrite(
                TranscriptRewriteSelection::MessageRange { start: 0, end: 2 },
                vec![Message::User(UserMessage::text(format!(
                    "{tag} replacement"
                )))],
                TranscriptRewriteReason::new("edit"),
                None,
                None,
            )
            .expect("commit fixture rewrite");
        let stamp =
            SessionCheckpointStamp::root(&session, SessionCheckpointProvenance::SessionCreated)
                .expect("mint root stamp");
        assert_eq!(
            stamp.schema_version(),
            SESSION_CHECKPOINT_STAMP_SCHEMA_VERSION_WITNESS_V3,
            "premise: 0.8.9 mints witness-v3 stamps on graph-bearing documents"
        );
        session
            .install_checkpoint_stamp(stamp)
            .expect("install root stamp");
        session
    }

    /// A graph-free session stamped by THIS meerkat: no transcript-history
    /// witness, so the stamp keeps the v1 schema and stays readable by
    /// older binaries. The assert pins the premise.
    fn schema_v1_session(tag: &str) -> meerkat_core::Session {
        let mut session = meerkat_core::Session::new();
        session.push(Message::User(UserMessage::text(format!("{tag} only turn"))));
        let stamp =
            SessionCheckpointStamp::root(&session, SessionCheckpointProvenance::SessionCreated)
                .expect("mint root stamp");
        assert_eq!(
            stamp.schema_version(),
            meerkat_core::SESSION_CHECKPOINT_STAMP_SCHEMA_VERSION,
            "premise: graph-free documents keep minting the v1 stamp schema"
        );
        session
            .install_checkpoint_stamp(stamp)
            .expect("install root stamp");
        session
    }

    fn continuity_db_with_v1_ddl(state_dir: &Path) -> PathBuf {
        let db = state_dir.join("continuity.sqlite3");
        let conn = Connection::open(&db).expect("create continuity fixture");
        conn.execute_batch(V1_CONTINUITY_DDL).expect("v1 ddl");
        db
    }

    fn insert_snapshot(db: &Path, session_id: &str, data: &[u8]) {
        let conn = Connection::open(db).expect("reopen fixture");
        conn.execute(
            "INSERT INTO session_snapshots \
             (session_id, identity, generation, checkpoint_version, fencing_token, data) \
             VALUES (?1, 'test:member-0', 3, 4, 7, ?2)",
            rusqlite::params![session_id, data],
        )
        .expect("insert snapshot");
    }

    fn insert_head_row(db: &Path, session_id: &str, head_json: &[u8]) {
        let conn = Connection::open(db).expect("reopen fixture");
        conn.execute(
            "INSERT INTO continuity_session_heads \
             (session_id, identity, generation, checkpoint_version, fencing_token, \
              head_revision, message_count, rewrite_count, head_json, cas_token) \
             VALUES (?1, 'test:member-0', 3, 4, 7, 'sha256:0', 1, 1, ?2, 'cas')",
            rusqlite::params![session_id, head_json],
        )
        .expect("insert head row");
    }

    fn file_digest(path: &Path) -> String {
        format!("{:x}", Sha256::digest(fs::read(path).expect("read db")))
    }

    #[test]
    fn apply_refuses_witness_v3_snapshot_before_touching_the_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path();
        let session = witness_v3_session("downgrade-refuse");
        let session_id = session.id().to_string();
        let db = continuity_db_with_v1_ddl(state);
        insert_snapshot(
            &db,
            &session_id,
            &serde_json::to_vec(&session).expect("serialize"),
        );

        let before = file_digest(&db);
        let report = downgrade_state_dir(state, MigrateMode::Apply);
        assert!(report.has_errors(), "{report:?}");
        assert!(
            report.downgrade.is_none(),
            "the refusal must precede the downgrade transaction"
        );
        assert_eq!(report.stamp_barriers.len(), 1);
        let barrier = &report.stamp_barriers[0];
        assert_eq!(barrier.session_id, session_id);
        assert_eq!(
            barrier.stamp_schema_version,
            Some(SESSION_CHECKPOINT_STAMP_SCHEMA_VERSION_WITNESS_V3)
        );
        assert!(
            report.errors.iter().any(|error| error.contains("0.8.9")),
            "the refusal must name the version door: {:?}",
            report.errors
        );
        assert_eq!(
            file_digest(&db),
            before,
            "a refused apply run must leave the file byte-identical"
        );
        let rendered = render_downgrade_report(&report);
        assert!(rendered.contains("error: session"), "{rendered}");
        assert!(
            !rendered.contains("previous releases can open this file"),
            "a refused run must not claim readability: {rendered}"
        );
    }

    #[test]
    fn refusal_covers_head_rows_and_the_witness_carrier_axis() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path();
        let db = continuity_db_with_v1_ddl(state);
        {
            let conn = Connection::open(&db).expect("reopen fixture");
            conn.execute_batch(HEAD_TABLE_DDL).expect("head ddl");
        }

        // A REAL head projection of a witness-v3 document: the downgraded
        // whole-document blob inherits exactly this metadata (stamp and
        // typed witness carrier included).
        let stamped = witness_v3_session("downgrade-head");
        let stamped_id = stamped.id().to_string();
        let head = meerkat_core::SessionHead::from_session(
            &stamped,
            meerkat_core::TranscriptStrandId::root(),
            1,
        )
        .expect("project head");
        insert_head_row(
            &db,
            &stamped_id,
            &serde_json::to_vec(&head).expect("serialize head"),
        );

        // A head whose metadata carries ONLY the typed v3 witness carrier —
        // the carrier axis must refuse on its own, without a stamp.
        let carrier_source = witness_v3_session("downgrade-carrier");
        let witness =
            meerkat_core::checkpoint::session_transcript_history_witness(&carrier_source)
                .expect("derive witness")
                .expect("graph-bearing documents carry a witness");
        let carrier_only = serde_json::json!({
            "metadata": {
                (meerkat_core::SESSION_TRANSCRIPT_HISTORY_CHECKPOINT_DIGEST_KEY):
                    witness.to_carried_value(),
            }
        });
        insert_head_row(
            &db,
            "carrier-only-session",
            &serde_json::to_vec(&carrier_only).expect("serialize carrier head"),
        );

        let report = downgrade_state_dir(state, MigrateMode::DryRun);
        assert!(report.has_errors(), "{report:?}");
        assert!(report.downgrade.is_none());
        assert_eq!(report.stamp_barriers.len(), 2, "{:?}", report.stamp_barriers);

        let stamped_barrier = report
            .stamp_barriers
            .iter()
            .find(|barrier| barrier.session_id == stamped_id)
            .expect("head-row barrier");
        assert_eq!(
            stamped_barrier.stamp_schema_version,
            Some(SESSION_CHECKPOINT_STAMP_SCHEMA_VERSION_WITNESS_V3)
        );
        assert_eq!(
            stamped_barrier.witness_format,
            Some(witness.witness_format()),
            "the head projection re-carries the witness format"
        );

        let carrier_barrier = report
            .stamp_barriers
            .iter()
            .find(|barrier| barrier.session_id == "carrier-only-session")
            .expect("carrier-only barrier");
        assert_eq!(carrier_barrier.stamp_schema_version, None);
        assert_eq!(carrier_barrier.witness_format, Some(witness.witness_format()));
    }

    #[test]
    fn schema_v1_documents_pass_and_the_success_message_names_the_verified_floor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path();
        let session = schema_v1_session("downgrade-clean");
        let db = continuity_db_with_v1_ddl(state);
        insert_snapshot(
            &db,
            &session.id().to_string(),
            &serde_json::to_vec(&session).expect("serialize"),
        );

        for mode in [MigrateMode::DryRun, MigrateMode::Apply] {
            let report = downgrade_state_dir(state, mode);
            assert!(!report.has_errors(), "{:?}", report.errors);
            assert!(report.stamp_barriers.is_empty());
            let downgrade = report.downgrade.as_ref().expect("the pass ran");
            assert!(downgrade.lockout_lifted());
            let rendered = render_downgrade_report(&report);
            assert!(
                rendered.contains("previous releases can open this file"),
                "{rendered}"
            );
            assert!(
                rendered.contains("witness-v3"),
                "the readability claim must name what was verified: {rendered}"
            );
        }
    }
}
