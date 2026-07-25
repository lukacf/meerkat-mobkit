//! Digest-format marker stamping for session documents (storage-migrate
//! case 6, the HomeCore cutover ask).
//!
//! Meerkat 0.8.6 gates the decode-time legacy heal probe (a full
//! head-transcript hash on EVERY decode) on the `digest_format` marker
//! inside the persisted transcript-history state. Documents written by
//! pre-marker builds carry current-format digests but no marker, so every
//! boot and every surviving idle-path decode re-pays an O(document) hash —
//! on a real fleet (~82 MB documents) that is ~0.1 core of steady-state
//! burn and ~1 s/MB of boot time. Decoding stamps the marker in memory and
//! any re-save persists it, but idle fleets never save: without an explicit
//! stamping write point, a fleet migrated by pre-marker builds never gets
//! the win at cutover.
//!
//! This module is that write point. It walks every full-envelope session
//! document in one store, and for each row that is **verified but
//! marker-less** it decodes the document (meerkat's decode auto-stamps the
//! marker in the in-memory state), requires the typed checkpoint to verify
//! (`try_checkpoint_state() == Verified` — full verification is correct on
//! a migration write path), re-serializes, and rewrites the payload bytes
//! in place. Bookkeeping columns are never touched. The rewrite is safe
//! because the checkpoint digest is marker-INVARIANT: meerkat's canonical
//! checkpoint history value carries only `{head, commits, revisions}` and
//! the digest-format marker is documented as "a compatibility convenience,
//! not an integrity boundary" — the existing verified stamp keeps verifying
//! before and after the respelling.
//!
//! # Population (the inverse gate of continuity checkpoint adoption)
//!
//! Checkpoint adoption (case 4) stamps LEGACY-UNVERIFIED rows and — because
//! `meerkat_core::adopt_legacy_session` re-serializes — its output already
//! carries the marker. Rows it classifies `already_stamped` (verified) are
//! deliberately left byte-untouched there; the verified-but-marker-less
//! subset of those rows is exactly this walk's population. Legacy rows seen
//! here are counted and left for adoption, which runs earlier in the same
//! pass.
//!
//! # Stores
//!
//! Identity fleets hold full-envelope session documents in three places:
//!
//! - **continuity** (`continuity.sqlite3`, mobkit-owned):
//!   `session_snapshots.data`, the session-store authority on the
//!   identity-first surface. The in-place `UPDATE` re-asserts the observed
//!   `(session_id, identity, generation, checkpoint_version,
//!   fencing_token)` tuple, mirroring the adoption walk.
//! - **runtime** (`runtime.sqlite`, meerkat-owned):
//!   `runtime_session_snapshots.session_snapshot`, a second full copy
//!   decoded on every authoritative load. The `UPDATE` is a byte-exact
//!   compare-and-swap on the observed payload (the raw-SQL spelling of
//!   `RuntimeStore::replace_session_snapshot_if_current`), under the same
//!   exclusive fence; bookkeeping stays with the owning meerkat store.
//! - **sessions** (`sessions.sqlite3`, meerkat-owned): only LEGACY headless
//!   rows (`sessions.session_json` with no `session_heads` row) ever decode
//!   full envelopes; once a head row exists the blob is a frozen,
//!   never-read migration archive and is left untouched. The `UPDATE` is a
//!   byte-exact compare-and-swap on the observed payload.
//!
//! Like the adoption walk, apply mode composes with the already-held
//! [`crate::storage_migrate::MobKitMaintenanceFence`] pass fence
//! ([`stamp_session_document_markers_already_fenced`]); dry-run takes its
//! own short-lived fence and a read-only connection, so byte-identity is a
//! connection property rather than walk discipline. Every row is
//! classified, never skipped silently.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

/// Mirror of meerkat-core's `TRANSCRIPT_DIGEST_FORMAT_CURRENT` (pinned
/// meerkat 0.8.6, `meerkat-core/src/session.rs`): revision strings stamped
/// `>= 2` were written by the content-addressed digest format, so decode
/// skips the per-decode legacy heal probe. The constant is crate-private
/// upstream; the `freshly_written_documents_probe_current` test pins this
/// mirror against the pinned meerkat's writer.
pub(crate) const TRANSCRIPT_DIGEST_FORMAT_CURRENT: u32 = 2;

/// How long a standalone (dry-run) walk waits for in-flight store
/// operations to drain before reporting the fence as unavailable.
const FENCE_DRAIN_DEADLINE: Duration = Duration::from_secs(10);

/// Invocation mode for the marker-stamping walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerStampMode {
    /// Census only. The database is opened read-only and stays
    /// byte-identical; the report counts what an apply run would do.
    DryRun,
    /// Rewrite verified marker-less rows in place (one transaction for the
    /// whole batch).
    Apply,
}

/// Which session-document store shape to walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionDocumentStore {
    /// `session_snapshots` in the mobkit continuity database.
    Continuity,
    /// `runtime_session_snapshots` in meerkat's runtime store.
    RuntimeSnapshots,
    /// Legacy headless `sessions` rows in meerkat's session store.
    SessionStore,
}

impl SessionDocumentStore {
    /// The table whose absence classifies the file as "not this store"
    /// (skipped by the verb, never an error).
    fn required_table(self) -> &'static str {
        match self {
            Self::Continuity => "session_snapshots",
            Self::RuntimeSnapshots => "runtime_session_snapshots",
            Self::SessionStore => "sessions",
        }
    }

    /// Stable label used in reports.
    pub fn label(self) -> &'static str {
        match self {
            Self::Continuity => "continuity",
            Self::RuntimeSnapshots => "runtime",
            Self::SessionStore => "sessions",
        }
    }
}

/// One row the walk refused, with the reason. Refusals never abort the
/// walk; they surface in the report (and as a nonzero maintenance-verb exit
/// code) for the operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkerStampRefusal {
    /// Row key (`session_id`, or `runtime_id` for runtime snapshots).
    pub key: String,
    pub reason: String,
}

/// Census of one marker-stamping walk over a session-document store.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMarkerStampReport {
    /// Rows visited (for the session store: headless rows only —
    /// head-canonical rows never decode the full envelope and are out of
    /// this walk's population by design).
    pub scanned: usize,
    /// Rows whose transcript-history state already carries the current
    /// digest-format marker (untouched, probed without a full decode).
    pub already_current: usize,
    /// Rows without retained transcript-history state: there is nothing to
    /// mark and the decode-time heal probe never fires for them.
    pub no_transcript_history: usize,
    /// Marker-less rows that decode as legacy-unverified documents:
    /// checkpoint adoption (case 4, which runs earlier in the same pass)
    /// owns those; counted and left alone here.
    pub legacy_unverified: usize,
    /// Verified marker-less rows rewritten in place (in
    /// [`MarkerStampMode::DryRun`]: rows that would be).
    pub stamped: usize,
    /// Rows whose payload does not decode as a meerkat session document.
    pub undecodable: usize,
    /// Rows refused typed (digest-mismatch stamps, key mismatches, rows
    /// that moved mid-walk), with reasons.
    pub refused: Vec<MarkerStampRefusal>,
}

impl SessionMarkerStampReport {
    /// Whether the walk completed without refusals.
    pub fn is_clean(&self) -> bool {
        self.refused.is_empty()
    }
}

impl fmt::Display for SessionMarkerStampReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "scanned:               {}", self.scanned)?;
        writeln!(f, "already current:       {}", self.already_current)?;
        writeln!(f, "no transcript history: {}", self.no_transcript_history)?;
        writeln!(f, "legacy unverified:     {}", self.legacy_unverified)?;
        writeln!(f, "stamped:               {}", self.stamped)?;
        writeln!(f, "undecodable:           {}", self.undecodable)?;
        write!(f, "refused:               {}", self.refused.len())?;
        for refusal in &self.refused {
            write!(f, "\n  {}: {}", refusal.key, refusal.reason)?;
        }
        Ok(())
    }
}

/// Failure of a marker-stamping walk itself (never a per-row
/// classification).
#[derive(Debug)]
pub enum MarkerStampError {
    /// The exclusive maintenance fence could not be acquired.
    FenceUnavailable { path: PathBuf, detail: String },
    /// The database could not be opened under the maintenance profile.
    Open { path: PathBuf, detail: String },
    /// The file does not carry the walked store's table — not this store
    /// (the verb reports it as skipped, never as an error).
    MissingTable { path: PathBuf, table: &'static str },
    /// A SQL failure mid-walk (the apply transaction is rolled back).
    Sql(String),
}

impl fmt::Display for MarkerStampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FenceUnavailable { path, detail } => write!(
                f,
                "maintenance fence unavailable for {}: {detail}",
                path.display()
            ),
            Self::Open { path, detail } => {
                write!(
                    f,
                    "cannot open {} for maintenance: {detail}",
                    path.display()
                )
            }
            Self::MissingTable { path, table } => write!(
                f,
                "{} carries no {table} table; not this session-document store",
                path.display()
            ),
            Self::Sql(detail) => write!(f, "marker stamping SQL failure: {detail}"),
        }
    }
}

impl std::error::Error for MarkerStampError {}

/// Cheap structural marker probe over raw document bytes: borrows the
/// metadata map as raw slices so probing an already-current row never
/// materializes the (potentially huge) transcript-history state.
#[derive(Deserialize)]
struct EnvelopeMarkerProbe<'a> {
    #[serde(default, borrow)]
    metadata: HashMap<Cow<'a, str>, &'a serde_json::value::RawValue>,
}

#[derive(Deserialize)]
struct HistoryStateMarkerProbe {
    #[serde(default)]
    digest_format: u32,
}

/// Marker state of one raw session document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentMarkerState {
    /// No retained transcript-history state: nothing to mark.
    NoTranscriptHistory,
    /// The state carries the current (or newer) digest-format marker.
    Current,
    /// The state exists but the marker is absent (or a stale generation).
    MarkerLess,
}

fn document_marker_state(bytes: &[u8]) -> Result<DocumentMarkerState, serde_json::Error> {
    let envelope: EnvelopeMarkerProbe<'_> = serde_json::from_slice(bytes)?;
    let Some(raw) = envelope
        .metadata
        .get(meerkat_core::SESSION_TRANSCRIPT_HISTORY_STATE_KEY)
    else {
        return Ok(DocumentMarkerState::NoTranscriptHistory);
    };
    let probe: HistoryStateMarkerProbe = serde_json::from_str(raw.get())?;
    if probe.digest_format >= TRANSCRIPT_DIGEST_FORMAT_CURRENT {
        Ok(DocumentMarkerState::Current)
    } else {
        Ok(DocumentMarkerState::MarkerLess)
    }
}

/// Standalone walk: acquires its own exclusive maintenance fence on
/// `db_path` (used by dry-run, where the migrate pass holds no fence).
pub fn stamp_session_document_markers_blocking(
    db_path: &Path,
    store: SessionDocumentStore,
    mode: MarkerStampMode,
) -> Result<SessionMarkerStampReport, MarkerStampError> {
    if !db_path.is_file() {
        return Err(MarkerStampError::Open {
            path: db_path.to_path_buf(),
            detail: "database file does not exist".to_string(),
        });
    }
    let _fence = meerkat_sqlite::ExclusiveFence::acquire(db_path, FENCE_DRAIN_DEADLINE).map_err(
        |error| MarkerStampError::FenceUnavailable {
            path: db_path.to_path_buf(),
            detail: error.to_string(),
        },
    )?;
    walk_quiesced(db_path, store, mode)
}

/// Walk for a caller that **already holds** the exclusive maintenance fence
/// on `db_path` — the M6 `storage-migrate` pass fences every state-dir
/// database for its whole run, and the fence file lock is not re-entrant.
/// Safety net: the walk still *tries* the fence non-blocking, exactly like
/// the adoption walk's already-fenced entry point, so the quiescence
/// contract holds even for a caller that lied.
pub fn stamp_session_document_markers_already_fenced(
    db_path: &Path,
    store: SessionDocumentStore,
    mode: MarkerStampMode,
) -> Result<SessionMarkerStampReport, MarkerStampError> {
    if !db_path.is_file() {
        return Err(MarkerStampError::Open {
            path: db_path.to_path_buf(),
            detail: "database file does not exist".to_string(),
        });
    }
    let _fence = meerkat_sqlite::ExclusiveFence::try_acquire(db_path).map_err(|error| {
        MarkerStampError::FenceUnavailable {
            path: db_path.to_path_buf(),
            detail: error.to_string(),
        }
    })?;
    walk_quiesced(db_path, store, mode)
}

/// The walk body, assuming the database is already quiesced (the caller
/// holds the exclusive maintenance fence, one way or another).
fn walk_quiesced(
    db_path: &Path,
    store: SessionDocumentStore,
    mode: MarkerStampMode,
) -> Result<SessionMarkerStampReport, MarkerStampError> {
    // DryRun opens read-only, making byte-identity a property of the
    // connection rather than of walk discipline.
    let profile = meerkat_sqlite::ConnectionProfile::Maintenance {
        write: matches!(mode, MarkerStampMode::Apply),
    };
    let mut conn =
        meerkat_sqlite::open(db_path, profile).map_err(|error| MarkerStampError::Open {
            path: db_path.to_path_buf(),
            detail: error.to_string(),
        })?;

    if !table_exists(&conn, store.required_table())? {
        return Err(MarkerStampError::MissingTable {
            path: db_path.to_path_buf(),
            table: store.required_table(),
        });
    }

    // One transaction for the whole batch: Apply commits atomically,
    // DryRun's deferred read transaction pins a consistent view.
    let behavior = match mode {
        MarkerStampMode::Apply => TransactionBehavior::Immediate,
        MarkerStampMode::DryRun => TransactionBehavior::Deferred,
    };
    let tx = conn
        .transaction_with_behavior(behavior)
        .map_err(|error| MarkerStampError::Sql(format!("begin: {error}")))?;

    let report = match store {
        SessionDocumentStore::Continuity => walk_continuity_rows(&tx, mode)?,
        SessionDocumentStore::RuntimeSnapshots => walk_runtime_rows(&tx, mode)?,
        SessionDocumentStore::SessionStore => walk_session_store_rows(&tx, mode)?,
    };

    match mode {
        MarkerStampMode::Apply => tx
            .commit()
            .map_err(|error| MarkerStampError::Sql(format!("commit: {error}")))?,
        MarkerStampMode::DryRun => drop(tx),
    }

    Ok(report)
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, MarkerStampError> {
    let present: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| MarkerStampError::Sql(format!("schema probe: {error}")))?;
    Ok(present.is_some())
}

fn sql_err(context: &'static str) -> impl Fn(rusqlite::Error) -> MarkerStampError {
    move |error| MarkerStampError::Sql(format!("{context}: {error}"))
}

/// Per-row classification shared by every store walk. Returns the
/// re-serialized (marker-stamped) bytes when the row is a verified
/// marker-less document that must be rewritten; every other disposition is
/// recorded on the report directly.
///
/// `expected_session_id` is asserted against the embedded document identity
/// when the row key IS a session id (continuity, session store).
fn respelled_document(
    report: &mut SessionMarkerStampReport,
    key: &str,
    expected_session_id: Option<&str>,
    bytes: &[u8],
) -> Option<Vec<u8>> {
    match document_marker_state(bytes) {
        Ok(DocumentMarkerState::Current) => {
            report.already_current += 1;
            return None;
        }
        Ok(DocumentMarkerState::NoTranscriptHistory) => {
            report.no_transcript_history += 1;
            return None;
        }
        Ok(DocumentMarkerState::MarkerLess) => {}
        Err(error) => {
            tracing::warn!(
                key = %key,
                %error,
                "marker stamping: payload does not probe as a session document"
            );
            report.undecodable += 1;
            return None;
        }
    }

    let session: meerkat_core::Session = match serde_json::from_slice(bytes) {
        Ok(session) => session,
        Err(error) => {
            tracing::warn!(
                key = %key,
                %error,
                "marker stamping: payload does not decode as a session document"
            );
            report.undecodable += 1;
            return None;
        }
    };
    if let Some(expected) = expected_session_id
        && session.id().to_string() != expected
    {
        report.refused.push(MarkerStampRefusal {
            key: key.to_string(),
            reason: format!(
                "row key does not match embedded session id {}",
                session.id()
            ),
        });
        return None;
    }

    // Full verification on this write path: only a document whose existing
    // typed checkpoint verifies against its exact content may be respelled.
    match session.try_checkpoint_state() {
        Ok(meerkat_core::SessionCheckpointState::Verified(_)) => {}
        Ok(meerkat_core::SessionCheckpointState::LegacyUnverified { .. }) => {
            // Checkpoint adoption's population, not ours.
            report.legacy_unverified += 1;
            return None;
        }
        Err(error) => {
            report.refused.push(MarkerStampRefusal {
                key: key.to_string(),
                reason: format!("checkpoint state unreadable: {error}"),
            });
            return None;
        }
    }

    // Decoding stamped the in-memory state (meerkat 0.8.6 decode-time heal
    // gate); re-serialization persists the marker. The checkpoint digest is
    // marker-invariant, so the verified stamp inside the bytes keeps
    // verifying.
    let respelled = match serde_json::to_vec(&session) {
        Ok(bytes) => bytes,
        Err(error) => {
            report.refused.push(MarkerStampRefusal {
                key: key.to_string(),
                reason: format!("re-serialization failed: {error}"),
            });
            return None;
        }
    };
    if !matches!(
        document_marker_state(&respelled),
        Ok(DocumentMarkerState::Current)
    ) {
        report.refused.push(MarkerStampRefusal {
            key: key.to_string(),
            reason: "re-serialized document does not carry the current digest-format marker"
                .to_string(),
        });
        return None;
    }
    Some(respelled)
}

/// Continuity snapshot rows; key columns mirror the adoption walk, and the
/// in-place `UPDATE` re-asserts the observed tuple while leaving
/// `generation` / `checkpoint_version` / `fencing_token` untouched.
fn walk_continuity_rows(
    tx: &Connection,
    mode: MarkerStampMode,
) -> Result<SessionMarkerStampReport, MarkerStampError> {
    struct RowMeta {
        session_id: String,
        identity: String,
        generation: u64,
        checkpoint_version: u64,
        fencing_token: u64,
    }

    let rows: Vec<RowMeta> = {
        let mut stmt = tx
            .prepare(
                "SELECT session_id, identity, generation, checkpoint_version, fencing_token \
                 FROM session_snapshots ORDER BY session_id",
            )
            .map_err(sql_err("prepare snapshots"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RowMeta {
                    session_id: row.get(0)?,
                    identity: row.get(1)?,
                    generation: row.get(2)?,
                    checkpoint_version: row.get(3)?,
                    fencing_token: row.get(4)?,
                })
            })
            .map_err(sql_err("query snapshots"))?;
        rows.collect::<Result<_, _>>()
            .map_err(sql_err("read snapshot row"))?
    };

    let mut report = SessionMarkerStampReport::default();
    for row in rows {
        report.scanned += 1;
        let data: Vec<u8> = tx
            .query_row(
                "SELECT data FROM session_snapshots WHERE session_id = ?1",
                [&row.session_id],
                |r| r.get(0),
            )
            .map_err(sql_err("load snapshot payload"))?;
        let Some(respelled) = respelled_document(
            &mut report,
            &row.session_id,
            Some(row.session_id.as_str()),
            &data,
        ) else {
            continue;
        };
        if matches!(mode, MarkerStampMode::Apply) {
            let changed = tx
                .execute(
                    "UPDATE session_snapshots SET data = ?1 \
                     WHERE session_id = ?2 AND identity = ?3 AND generation = ?4 \
                       AND checkpoint_version = ?5 AND fencing_token = ?6",
                    rusqlite::params![
                        respelled,
                        row.session_id,
                        row.identity,
                        row.generation,
                        row.checkpoint_version,
                        row.fencing_token,
                    ],
                )
                .map_err(sql_err("rewrite snapshot payload"))?;
            if changed != 1 {
                report.refused.push(MarkerStampRefusal {
                    key: row.session_id.clone(),
                    reason: "snapshot row moved during the fenced walk".to_string(),
                });
                continue;
            }
        }
        tracing::info!(
            session_id = %row.session_id,
            identity = %row.identity,
            applied = matches!(mode, MarkerStampMode::Apply),
            "marker stamping: respelled verified continuity snapshot with the digest-format marker"
        );
        report.stamped += 1;
    }
    Ok(report)
}

/// Runtime session snapshots (meerkat-owned): the schema carries no cursor
/// columns, so the in-place `UPDATE` is a byte-exact compare-and-swap on
/// the observed payload — the raw-SQL spelling of
/// `RuntimeStore::replace_session_snapshot_if_current`, under the held
/// exclusive fence.
fn walk_runtime_rows(
    tx: &Connection,
    mode: MarkerStampMode,
) -> Result<SessionMarkerStampReport, MarkerStampError> {
    let runtime_ids: Vec<String> = {
        let mut stmt = tx
            .prepare("SELECT runtime_id FROM runtime_session_snapshots ORDER BY runtime_id")
            .map_err(sql_err("prepare runtime snapshots"))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql_err("query runtime snapshots"))?;
        rows.collect::<Result<_, _>>()
            .map_err(sql_err("read runtime snapshot row"))?
    };

    let mut report = SessionMarkerStampReport::default();
    for runtime_id in runtime_ids {
        report.scanned += 1;
        let data: Vec<u8> = tx
            .query_row(
                "SELECT session_snapshot FROM runtime_session_snapshots WHERE runtime_id = ?1",
                [&runtime_id],
                |r| r.get(0),
            )
            .map_err(sql_err("load runtime snapshot payload"))?;
        let Some(respelled) = respelled_document(&mut report, &runtime_id, None, &data) else {
            continue;
        };
        if matches!(mode, MarkerStampMode::Apply) {
            let changed = tx
                .execute(
                    "UPDATE runtime_session_snapshots SET session_snapshot = ?1 \
                     WHERE runtime_id = ?2 AND session_snapshot = ?3",
                    rusqlite::params![respelled, runtime_id, data],
                )
                .map_err(sql_err("rewrite runtime snapshot payload"))?;
            if changed != 1 {
                report.refused.push(MarkerStampRefusal {
                    key: runtime_id.clone(),
                    reason: "runtime snapshot row moved during the fenced walk".to_string(),
                });
                continue;
            }
        }
        tracing::info!(
            runtime_id = %runtime_id,
            applied = matches!(mode, MarkerStampMode::Apply),
            "marker stamping: respelled verified runtime session snapshot with the \
             digest-format marker"
        );
        report.stamped += 1;
    }
    Ok(report)
}

/// Legacy headless session-store rows (meerkat-owned): once a
/// `session_heads` row exists the blob is a frozen migration archive that
/// is never read again, so only headless rows are in the population. The
/// in-place `UPDATE` is a byte-exact compare-and-swap on the observed
/// payload.
fn walk_session_store_rows(
    tx: &Connection,
    mode: MarkerStampMode,
) -> Result<SessionMarkerStampReport, MarkerStampError> {
    let has_heads = table_exists(tx, "session_heads")?;
    let enumerate_sql = if has_heads {
        "SELECT s.session_id FROM sessions s \
         WHERE NOT EXISTS (SELECT 1 FROM session_heads h WHERE h.session_id = s.session_id) \
         ORDER BY s.session_id"
    } else {
        "SELECT session_id FROM sessions ORDER BY session_id"
    };
    let session_ids: Vec<String> = {
        let mut stmt = tx
            .prepare(enumerate_sql)
            .map_err(sql_err("prepare headless sessions"))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql_err("query headless sessions"))?;
        rows.collect::<Result<_, _>>()
            .map_err(sql_err("read headless session row"))?
    };

    let mut report = SessionMarkerStampReport::default();
    for session_id in session_ids {
        report.scanned += 1;
        let data: Vec<u8> = tx
            .query_row(
                "SELECT session_json FROM sessions WHERE session_id = ?1",
                [&session_id],
                |r| r.get(0),
            )
            .map_err(sql_err("load session payload"))?;
        let Some(respelled) =
            respelled_document(&mut report, &session_id, Some(session_id.as_str()), &data)
        else {
            continue;
        };
        if matches!(mode, MarkerStampMode::Apply) {
            let changed = tx
                .execute(
                    "UPDATE sessions SET session_json = ?1 \
                     WHERE session_id = ?2 AND session_json = ?3",
                    rusqlite::params![respelled, session_id, data],
                )
                .map_err(sql_err("rewrite session payload"))?;
            if changed != 1 {
                report.refused.push(MarkerStampRefusal {
                    key: session_id.clone(),
                    reason: "session row moved during the fenced walk".to_string(),
                });
                continue;
            }
        }
        tracing::info!(
            session_id = %session_id,
            applied = matches!(mode, MarkerStampMode::Apply),
            "marker stamping: respelled verified headless session row with the \
             digest-format marker"
        );
        report.stamped += 1;
    }
    Ok(report)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use meerkat_core::{
        Message, SessionCheckpointProvenance, SessionCheckpointStamp, SessionCheckpointState,
        TranscriptRewriteReason, TranscriptRewriteSelection, types::UserMessage,
    };
    use rusqlite::Connection;
    use sha2::{Digest, Sha256};
    use std::fs;

    /// The continuity schema exactly as the adoption-walk fixtures spell it.
    const CONTINUITY_DDL: &str = "CREATE TABLE continuity_records (
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

    /// Pinned meerkat 0.8.6 runtime-store snapshot schema.
    const RUNTIME_DDL: &str = "CREATE TABLE runtime_session_snapshots (
            runtime_id TEXT PRIMARY KEY,
            session_snapshot BLOB NOT NULL
        );";

    /// Pinned meerkat 0.8.6 session-store schema (legacy blob + head rows).
    const SESSIONS_DDL: &str = "CREATE TABLE sessions (
            session_id TEXT PRIMARY KEY,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            message_count INTEGER NOT NULL,
            total_tokens INTEGER NOT NULL,
            metadata_json TEXT NOT NULL,
            session_json BLOB NOT NULL
        );
        CREATE TABLE session_heads (
            session_id TEXT PRIMARY KEY,
            version INTEGER NOT NULL,
            strand TEXT NOT NULL,
            head_revision TEXT NOT NULL,
            message_count INTEGER NOT NULL,
            rewrite_count INTEGER NOT NULL,
            total_tokens INTEGER NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            metadata_json TEXT NOT NULL,
            head_json BLOB NOT NULL,
            cas_token TEXT NOT NULL
        );";

    /// A session with retained transcript-history state (one real audited
    /// rewrite) and a verified root checkpoint stamp.
    fn verified_history_session(tag: &str) -> meerkat_core::Session {
        let mut session = meerkat_core::Session::new();
        session.push(Message::User(UserMessage::text(format!(
            "{tag} old context one"
        ))));
        session.push(Message::User(UserMessage::text(format!(
            "{tag} old context two"
        ))));
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
        session
            .install_checkpoint_stamp(stamp)
            .expect("install root stamp");
        session
    }

    /// Serialize a session and strip the digest-format marker — the exact
    /// bytes a pre-marker (0.8.4-class) writer persisted for a verified,
    /// history-bearing document. The checkpoint digest is marker-invariant,
    /// so the stamp keeps verifying on the stripped bytes.
    fn strip_marker(session: &meerkat_core::Session) -> Vec<u8> {
        let mut document = serde_json::to_value(session).expect("serialize session");
        document["metadata"][meerkat_core::SESSION_TRANSCRIPT_HISTORY_STATE_KEY]
            .as_object_mut()
            .expect("history state object")
            .remove("digest_format")
            .expect("current writers stamp the marker");
        serde_json::to_vec(&document).expect("serialize marker-less document")
    }

    fn file_digest(path: &Path) -> String {
        format!("{:x}", Sha256::digest(fs::read(path).expect("read db")))
    }

    fn continuity_fixture(path: &Path, rows: &[(&str, &[u8])]) {
        let conn = Connection::open(path).expect("create continuity fixture");
        conn.execute_batch(CONTINUITY_DDL).expect("ddl");
        for (index, (session_id, data)) in rows.iter().enumerate() {
            let identity = format!("test:member-{index}");
            conn.execute(
                "INSERT INTO continuity_records \
                 (identity, agent_runtime_id, session_id, generation, checkpoint_version, \
                  fencing_token) VALUES (?1, 'rt-1', ?2, 3, 4, 7)",
                rusqlite::params![identity, session_id],
            )
            .expect("insert record");
            conn.execute(
                "INSERT INTO session_snapshots \
                 (session_id, identity, generation, checkpoint_version, fencing_token, data) \
                 VALUES (?1, ?2, 3, 4, 7, ?3)",
                rusqlite::params![session_id, identity, data],
            )
            .expect("insert snapshot");
        }
    }

    fn continuity_row(path: &Path, session_id: &str) -> Vec<u8> {
        let conn = Connection::open(path).expect("reopen");
        conn.query_row(
            "SELECT data FROM session_snapshots WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .expect("row present")
    }

    #[test]
    fn freshly_written_documents_probe_current() {
        // Pins the crate-local TRANSCRIPT_DIGEST_FORMAT_CURRENT mirror
        // against the pinned meerkat writer: a document this build writes
        // must probe Current, and stripping the marker must probe
        // MarkerLess.
        let session = verified_history_session("probe-current");
        let bytes = serde_json::to_vec(&session).expect("serialize");
        assert!(matches!(
            document_marker_state(&bytes),
            Ok(DocumentMarkerState::Current)
        ));
        assert!(matches!(
            document_marker_state(&strip_marker(&session)),
            Ok(DocumentMarkerState::MarkerLess)
        ));

        let history_less = meerkat_core::Session::new();
        let bytes = serde_json::to_vec(&history_less).expect("serialize");
        assert!(matches!(
            document_marker_state(&bytes),
            Ok(DocumentMarkerState::NoTranscriptHistory)
        ));
        assert!(document_marker_state(b"not json").is_err());
    }

    #[test]
    fn continuity_walk_stamps_verified_marker_less_rows_and_is_idempotent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = temp.path().join("continuity.sqlite3");

        let marker_less = verified_history_session("continuity-stamp");
        let marker_less_id = marker_less.id().to_string();
        let marker_less_bytes = strip_marker(&marker_less);

        let current = verified_history_session("continuity-current");
        let current_id = current.id().to_string();
        let current_bytes = serde_json::to_vec(&current).expect("serialize current");

        // A marker-less LEGACY (unstamped) document: adoption's population.
        // Strip the checkpoint stamp so the document classifies
        // LegacyUnverified.
        let legacy = verified_history_session("continuity-legacy");
        let legacy_id = legacy.id().to_string();
        let mut legacy_doc: serde_json::Value =
            serde_json::from_slice(&strip_marker(&legacy)).expect("decode value");
        legacy_doc["metadata"]
            .as_object_mut()
            .expect("metadata")
            .remove(meerkat_core::SESSION_CHECKPOINT_STAMP_KEY)
            .expect("stamped fixture");
        let legacy_bytes = serde_json::to_vec(&legacy_doc).expect("legacy bytes");

        // A history-less document (nothing to mark).
        let plain = meerkat_core::Session::new();
        let plain_id = plain.id().to_string();
        let plain_bytes = serde_json::to_vec(&plain).expect("plain bytes");

        continuity_fixture(
            &db,
            &[
                (marker_less_id.as_str(), marker_less_bytes.as_slice()),
                (current_id.as_str(), current_bytes.as_slice()),
                (legacy_id.as_str(), legacy_bytes.as_slice()),
                (plain_id.as_str(), plain_bytes.as_slice()),
            ],
        );

        // Dry-run: census only, bytes untouched.
        let before = file_digest(&db);
        let dry = stamp_session_document_markers_blocking(
            &db,
            SessionDocumentStore::Continuity,
            MarkerStampMode::DryRun,
        )
        .expect("dry-run walk");
        assert_eq!(dry.scanned, 4);
        assert_eq!(dry.stamped, 1);
        assert_eq!(dry.already_current, 1);
        assert_eq!(dry.legacy_unverified, 1);
        assert_eq!(dry.no_transcript_history, 1);
        assert!(dry.is_clean(), "{:?}", dry.refused);
        assert_eq!(file_digest(&db), before, "dry-run must be byte-identical");

        // Apply: the verified marker-less row is respelled in place; the
        // legacy row is left for adoption; bookkeeping stays untouched.
        let apply = stamp_session_document_markers_blocking(
            &db,
            SessionDocumentStore::Continuity,
            MarkerStampMode::Apply,
        )
        .expect("apply walk");
        assert_eq!(apply.stamped, 1);
        assert!(apply.is_clean(), "{:?}", apply.refused);

        let rewritten = continuity_row(&db, &marker_less_id);
        assert!(matches!(
            document_marker_state(&rewritten),
            Ok(DocumentMarkerState::Current)
        ));
        let session: meerkat_core::Session =
            serde_json::from_slice(&rewritten).expect("decode rewritten row");
        assert!(matches!(
            session.try_checkpoint_state().expect("checkpoint state"),
            SessionCheckpointState::Verified(_)
        ));
        assert_eq!(
            continuity_row(&db, &legacy_id),
            legacy_bytes,
            "legacy-unverified rows stay byte-identical (adoption owns them)"
        );
        let (generation, version, token): (u64, u64, u64) = {
            let conn = Connection::open(&db).expect("reopen");
            conn.query_row(
                "SELECT generation, checkpoint_version, fencing_token \
                 FROM session_snapshots WHERE session_id = ?1",
                [&marker_less_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("bookkeeping row")
        };
        assert_eq!(
            (generation, version, token),
            (3, 4, 7),
            "bookkeeping columns must stay exactly as observed"
        );

        // Idempotence: a second apply stamps nothing new.
        let second = stamp_session_document_markers_blocking(
            &db,
            SessionDocumentStore::Continuity,
            MarkerStampMode::Apply,
        )
        .expect("second apply walk");
        assert_eq!(second.stamped, 0);
        assert_eq!(second.already_current, 2);
        assert!(second.is_clean(), "{:?}", second.refused);
    }

    #[test]
    fn digest_mismatch_rows_are_refused_never_respelled() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = temp.path().join("continuity.sqlite3");

        // A marker-less document whose content was corrupted after stamping
        // (usage is covered by the checkpoint digest but not by transcript
        // validation, so the document still decodes): the stamp no longer
        // verifies, so respelling must refuse.
        let session = verified_history_session("continuity-corrupt");
        let session_id = session.id().to_string();
        let mut document: serde_json::Value =
            serde_json::from_slice(&strip_marker(&session)).expect("decode value");
        document["usage"]["input_tokens"] = serde_json::json!(987_654);
        let corrupted = serde_json::to_vec(&document).expect("corrupted bytes");

        continuity_fixture(&db, &[(session_id.as_str(), corrupted.as_slice())]);
        let before = file_digest(&db);
        let walk = stamp_session_document_markers_blocking(
            &db,
            SessionDocumentStore::Continuity,
            MarkerStampMode::Apply,
        )
        .expect("apply walk");
        assert_eq!(walk.stamped, 0);
        assert_eq!(walk.refused.len(), 1, "{walk}");
        assert!(
            walk.refused[0]
                .reason
                .contains("checkpoint state unreadable"),
            "{}",
            walk.refused[0].reason
        );
        assert_eq!(file_digest(&db), before, "refused rows stay byte-identical");
    }

    #[test]
    fn runtime_walk_respells_snapshot_copies() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = temp.path().join("runtime.sqlite");
        let session = verified_history_session("runtime-stamp");
        let marker_less = strip_marker(&session);
        {
            let conn = Connection::open(&db).expect("create runtime fixture");
            conn.execute_batch(RUNTIME_DDL).expect("ddl");
            conn.execute(
                "INSERT INTO runtime_session_snapshots (runtime_id, session_snapshot) \
                 VALUES (?1, ?2)",
                rusqlite::params![format!("session-runtime:{}", session.id()), marker_less],
            )
            .expect("insert runtime snapshot");
        }

        let apply = stamp_session_document_markers_blocking(
            &db,
            SessionDocumentStore::RuntimeSnapshots,
            MarkerStampMode::Apply,
        )
        .expect("apply walk");
        assert_eq!(apply.scanned, 1);
        assert_eq!(apply.stamped, 1);
        assert!(apply.is_clean(), "{:?}", apply.refused);

        let conn = Connection::open(&db).expect("reopen");
        let rewritten: Vec<u8> = conn
            .query_row(
                "SELECT session_snapshot FROM runtime_session_snapshots",
                [],
                |row| row.get(0),
            )
            .expect("row");
        assert!(matches!(
            document_marker_state(&rewritten),
            Ok(DocumentMarkerState::Current)
        ));
        let decoded: meerkat_core::Session =
            serde_json::from_slice(&rewritten).expect("decode rewritten snapshot");
        assert!(matches!(
            decoded.try_checkpoint_state().expect("checkpoint state"),
            SessionCheckpointState::Verified(_)
        ));
    }

    #[test]
    fn session_store_walk_respells_only_headless_rows() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = temp.path().join("sessions.sqlite3");
        let headless = verified_history_session("sessions-headless");
        let headless_id = headless.id().to_string();
        let head_canonical = verified_history_session("sessions-headed");
        let head_canonical_id = head_canonical.id().to_string();
        {
            let conn = Connection::open(&db).expect("create sessions fixture");
            conn.execute_batch(SESSIONS_DDL).expect("ddl");
            for (id, bytes) in [
                (&headless_id, strip_marker(&headless)),
                (&head_canonical_id, strip_marker(&head_canonical)),
            ] {
                conn.execute(
                    "INSERT INTO sessions (session_id, created_at_ms, updated_at_ms, \
                     message_count, total_tokens, metadata_json, session_json) \
                     VALUES (?1, 0, 0, 1, 0, '{}', ?2)",
                    rusqlite::params![id, bytes],
                )
                .expect("insert session row");
            }
            // The second session is head-canonical: its blob is a frozen,
            // never-read migration archive.
            conn.execute(
                "INSERT INTO session_heads (session_id, version, strand, head_revision, \
                 message_count, rewrite_count, total_tokens, created_at_ms, updated_at_ms, \
                 metadata_json, head_json, cas_token) \
                 VALUES (?1, 1, 'root', 'sha256:0', 1, 1, 0, 0, 0, '{}', X'7B7D', 'cas')",
                rusqlite::params![head_canonical_id],
            )
            .expect("insert head row");
        }

        let head_blob_before = {
            let conn = Connection::open(&db).expect("reopen");
            conn.query_row(
                "SELECT session_json FROM sessions WHERE session_id = ?1",
                [&head_canonical_id],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .expect("head-canonical blob")
        };

        let apply = stamp_session_document_markers_blocking(
            &db,
            SessionDocumentStore::SessionStore,
            MarkerStampMode::Apply,
        )
        .expect("apply walk");
        assert_eq!(
            apply.scanned, 1,
            "head-canonical rows are out of the population"
        );
        assert_eq!(apply.stamped, 1);
        assert!(apply.is_clean(), "{:?}", apply.refused);

        let conn = Connection::open(&db).expect("reopen");
        let rewritten: Vec<u8> = conn
            .query_row(
                "SELECT session_json FROM sessions WHERE session_id = ?1",
                [&headless_id],
                |row| row.get(0),
            )
            .expect("headless row");
        assert!(matches!(
            document_marker_state(&rewritten),
            Ok(DocumentMarkerState::Current)
        ));
        let frozen: Vec<u8> = conn
            .query_row(
                "SELECT session_json FROM sessions WHERE session_id = ?1",
                [&head_canonical_id],
                |row| row.get(0),
            )
            .expect("head-canonical row");
        assert_eq!(
            frozen, head_blob_before,
            "head-canonical archive blobs stay byte-identical"
        );
    }

    #[test]
    fn missing_table_classifies_as_not_this_store() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = temp.path().join("runtime.sqlite");
        drop(Connection::open(&db).expect("create empty db"));
        let error = stamp_session_document_markers_blocking(
            &db,
            SessionDocumentStore::RuntimeSnapshots,
            MarkerStampMode::DryRun,
        )
        .expect_err("empty file is not this store");
        assert!(matches!(error, MarkerStampError::MissingTable { .. }));
    }
}
