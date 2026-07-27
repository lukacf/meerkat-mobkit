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
//! run will succeed, not a guess that it might. Because that reconstruction
//! runs on a write-capable connection (rolling back is a transaction
//! property, not a connection property), a dry run acquires the SAME
//! exclusive maintenance fence as apply — it never races a live gateway.

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

/// How long a run (either mode — dry run fences too) waits for in-flight
/// store operations to drain before reporting the fence as unavailable.
/// Same budget the migrate pass uses.
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
    /// Databases the fence actually held. Populated in BOTH modes: even a
    /// dry run performs the full reconstruction on a write-capable
    /// connection before rolling back, so it runs under the same fence as
    /// apply.
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
    /// `schema_version` advertised by the document's checkpoint stamp;
    /// `None` means the document carries no stamp at all (a stamp the
    /// probe cannot read refuses the whole pass instead of reaching here).
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
             session from a pre-0.8.9 backup. Restoring continuity.* alone is NOT enough: \
             the runtime store (runtime.sqlite) retains its own full session snapshots, \
             which a rolled-back binary decodes on resume — restore every session-bearing \
             store from the same consistent backup set, and check with the doctor's \
             storage-compatibility census that no store still holds documents requiring \
             >= 0.8.9."
        )
    }
}

/// The advertised format versions one stored document (or head projection)
/// carries. Advertised only — no digest re-verification: the gate is about
/// which stamps OTHER binaries will refuse, not about whether this document
/// is intact. `None` always means the evidence is genuinely ABSENT;
/// present-but-unreadable evidence never reaches this type — the probe
/// refuses it as [`FormatEvidenceError`] instead.
struct DocumentFormatProbe {
    stamp_schema_version: Option<u32>,
    witness_format: Option<u32>,
}

impl DocumentFormatProbe {
    fn barrier(&self, session_id: &str) -> Option<StampSchemaBarrier> {
        let stamp_is_v3 = self
            .stamp_schema_version
            .is_some_and(|version| version >= SESSION_CHECKPOINT_STAMP_SCHEMA_VERSION_WITNESS_V3);
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

/// Why one document's format evidence could not be read. Every variant
/// fails the preflight closed: evidence the probe cannot read could be
/// hiding the witness-v3 barrier, so it is never laundered into "nothing
/// advertised on this axis" — the doctor classifies these same shapes
/// malformed and reports readability unknown, and this pass must not
/// out-certify it.
#[derive(Debug)]
enum FormatEvidenceError {
    /// The envelope itself does not parse as a session document.
    Envelope(serde_json::Error),
    /// A checkpoint stamp is present but is not an object carrying a
    /// numeric `schema_version`.
    MalformedStamp(String),
    /// A transcript-history witness carrier is present but is neither the
    /// bare digest string (v2) nor an object carrying a numeric
    /// `witness_format` (v3+).
    MalformedWitnessCarrier(String),
}

impl fmt::Display for FormatEvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Envelope(error) => {
                write!(f, "does not probe as a session document: {error}")
            }
            Self::MalformedStamp(reason) => {
                write!(
                    f,
                    "carries a checkpoint stamp the probe cannot read ({reason})"
                )
            }
            Self::MalformedWitnessCarrier(reason) => write!(
                f,
                "carries a transcript-history witness carrier the probe cannot read ({reason})"
            ),
        }
    }
}

/// `schema_version` of one PRESENT checkpoint-stamp value. Mirrors the
/// doctor's `classify_stamp_schema`: not an object, or an object without a
/// numeric `schema_version`, is malformed evidence — never "no stamp".
fn classify_stamp_schema(raw: &serde_json::value::RawValue) -> Result<u32, FormatEvidenceError> {
    let malformed = FormatEvidenceError::MalformedStamp;
    let value: serde_json::Value = serde_json::from_str(raw.get())
        .map_err(|error| malformed(format!("not decodable: {error}")))?;
    let Some(fields) = value.as_object() else {
        return Err(malformed(
            "checkpoint stamp is not a JSON object".to_string(),
        ));
    };
    match fields
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
    {
        Some(version) => u32::try_from(version)
            .map_err(|_| malformed(format!("schema_version {version} overflows u32"))),
        None => Err(malformed(
            "checkpoint stamp carries no numeric schema_version".to_string(),
        )),
    }
}

/// Witness format advertised by one PRESENT carrier value. Mirrors the
/// doctor's `classify_witness_format`: a bare digest string is the v2
/// carrier and advertises nothing on this axis (`None`); an object carrier
/// must advertise a numeric `witness_format`; every other shape is
/// malformed evidence — never "nothing advertised".
fn classify_witness_carrier(
    raw: &serde_json::value::RawValue,
) -> Result<Option<u32>, FormatEvidenceError> {
    let malformed = FormatEvidenceError::MalformedWitnessCarrier;
    let value: serde_json::Value = serde_json::from_str(raw.get())
        .map_err(|error| malformed(format!("not decodable: {error}")))?;
    match value {
        serde_json::Value::String(_) => Ok(None),
        serde_json::Value::Object(fields) => match fields
            .get("witness_format")
            .and_then(serde_json::Value::as_u64)
        {
            Some(format) => u32::try_from(format)
                .map(Some)
                .map_err(|_| malformed(format!("witness_format {format} overflows u32"))),
            None => Err(malformed(
                "witness carrier object carries no numeric witness_format".to_string(),
            )),
        },
        _ => Err(malformed(
            "witness carrier is neither a digest string nor an object".to_string(),
        )),
    }
}

/// Cheap structural probe over raw document bytes: borrows the metadata map
/// as raw slices so probing never materializes the (potentially huge)
/// transcript-history state. Works for whole session documents and for
/// `SessionHead` projections alike — both carry a `metadata` object, which
/// is all this reads.
///
/// Fail-closed at the FIELD level, not just the envelope level: a stamp or
/// carrier that is present but unreadable is an error, never `None` —
/// otherwise malformed evidence would flow into
/// [`DocumentFormatProbe::barrier`] as "no barrier" and the pass would
/// claim "verified free of witness-v3 stamps" over evidence it could not
/// read.
fn probe_document_format(bytes: &[u8]) -> Result<DocumentFormatProbe, FormatEvidenceError> {
    #[derive(Deserialize)]
    struct EnvelopeProbe<'a> {
        #[serde(default, borrow)]
        metadata: HashMap<Cow<'a, str>, &'a serde_json::value::RawValue>,
    }
    let envelope: EnvelopeProbe<'_> =
        serde_json::from_slice(bytes).map_err(FormatEvidenceError::Envelope)?;
    let stamp_schema_version = envelope
        .metadata
        .get(meerkat_core::SESSION_CHECKPOINT_STAMP_KEY)
        .copied()
        .map(classify_stamp_schema)
        .transpose()?;
    let witness_format = envelope
        .metadata
        .get(meerkat_core::SESSION_TRANSCRIPT_HISTORY_CHECKPOINT_DIGEST_KEY)
        .copied()
        .map(classify_witness_carrier)
        .transpose()?
        .flatten();
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
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(|error| format!("query {what} census: {error}"))?;
    for row in rows {
        let (session_id, bytes) = row.map_err(|error| format!("read {what} row: {error}"))?;
        // Fail closed on unreadable evidence — envelope-level AND
        // field-level (a present-but-malformed stamp or carrier): the pass
        // cannot prove the absence of the barrier, so it must not claim the
        // result readable.
        let probe = probe_document_format(&bytes)
            .map_err(|error| format!("{what} for session {session_id} {error}"))?;
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
    // would be racing the process that is about to be rolled back. BOTH
    // modes hold it — a "dry" run performs the entire reconstruction on a
    // write-capable connection (rolling back is a transaction property,
    // not a connection property) — and holding it across the census below
    // and the downgrade keeps the census evidence true for the file the
    // downgrade actually sees.
    let fence = match MobKitMaintenanceFence::acquire(state_dir, FENCE_DRAIN_DEADLINE) {
        Ok(fence) => {
            report.fenced_databases = fence.fenced_databases().to_vec();
            fence
        }
        Err(error) => {
            report
                .errors
                .push(format!("maintenance fence not acquirable: {error}"));
            return report;
        }
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
        let witness = meerkat_core::checkpoint::session_transcript_history_witness(&carrier_source)
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
        assert_eq!(
            report.stamp_barriers.len(),
            2,
            "{:?}",
            report.stamp_barriers
        );

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
        assert_eq!(
            carrier_barrier.witness_format,
            Some(witness.witness_format())
        );
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

    /// FAIL-CLOSED PROBE PIN: a checkpoint stamp that is PRESENT but
    /// unreadable must refuse the pass before any destructive step, in both
    /// modes — never be laundered into "no stamp" and certified "verified
    /// free of witness-v3 stamps". The doctor classifies this same shape
    /// malformed and reports readability unknown; the downgrade must not
    /// out-certify it.
    #[test]
    fn malformed_stamp_evidence_refuses_before_any_destruction() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path();
        let db = continuity_db_with_v1_ddl(state);
        let malformed = serde_json::json!({
            "metadata": {
                (meerkat_core::SESSION_CHECKPOINT_STAMP_KEY): { "schema_version": "three" },
            }
        });
        insert_snapshot(
            &db,
            "malformed-stamp-session",
            &serde_json::to_vec(&malformed).expect("serialize"),
        );

        let before = file_digest(&db);
        for mode in [MigrateMode::DryRun, MigrateMode::Apply] {
            let report = downgrade_state_dir(state, mode);
            assert!(report.has_errors(), "{report:?}");
            assert!(
                report.downgrade.is_none(),
                "the refusal must precede the downgrade transaction"
            );
            assert!(
                report.errors.iter().any(|error| {
                    error.contains("cannot prove")
                        && error.contains("malformed-stamp-session")
                        && error.contains("checkpoint stamp")
                }),
                "the refusal must name the session and the unreadable stamp: {:?}",
                report.errors
            );
            let rendered = render_downgrade_report(&report);
            assert!(
                !rendered.contains("previous releases can open this file"),
                "unreadable evidence must never be certified readable: {rendered}"
            );
        }
        assert_eq!(
            file_digest(&db),
            before,
            "a refused run must leave the file byte-identical"
        );
    }

    /// The carrier axis fails closed the same way: an object carrier
    /// without a numeric `witness_format`, or a carrier that is neither a
    /// digest string nor an object, is unreadable evidence — a refusal,
    /// not silence.
    #[test]
    fn malformed_witness_carrier_evidence_refuses_pre_destruction() {
        for carrier in [
            serde_json::json!({ "digest": "sha256:abc" }),
            serde_json::json!(7),
        ] {
            let temp = tempfile::tempdir().expect("tempdir");
            let state = temp.path();
            let db = continuity_db_with_v1_ddl(state);
            let document = serde_json::json!({
                "metadata": {
                    (meerkat_core::SESSION_TRANSCRIPT_HISTORY_CHECKPOINT_DIGEST_KEY): carrier,
                }
            });
            insert_snapshot(
                &db,
                "malformed-carrier-session",
                &serde_json::to_vec(&document).expect("serialize"),
            );

            let report = downgrade_state_dir(state, MigrateMode::DryRun);
            assert!(report.has_errors(), "{report:?}");
            assert!(report.downgrade.is_none());
            assert!(
                report.errors.iter().any(|error| {
                    error.contains("cannot prove")
                        && error.contains("malformed-carrier-session")
                        && error.contains("witness carrier")
                }),
                "the refusal must name the session and the unreadable carrier: {:?}",
                report.errors
            );
        }
    }

    /// The legitimate evidence shapes keep certifying: a document that was
    /// never stamped, and a v2 bare-digest witness carrier under a numeric
    /// pre-v3 stamp, are exactly what the downgrade exists to serve.
    #[test]
    fn absent_stamp_and_bare_digest_carrier_still_certify() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path();
        let db = continuity_db_with_v1_ddl(state);
        let never_stamped = meerkat_core::Session::new();
        insert_snapshot(
            &db,
            &never_stamped.id().to_string(),
            &serde_json::to_vec(&never_stamped).expect("serialize"),
        );
        let bare_digest_carrier = serde_json::json!({
            "metadata": {
                (meerkat_core::SESSION_CHECKPOINT_STAMP_KEY): { "schema_version": 2 },
                (meerkat_core::SESSION_TRANSCRIPT_HISTORY_CHECKPOINT_DIGEST_KEY):
                    "sha256:deadbeef",
            }
        });
        insert_snapshot(
            &db,
            "bare-digest-session",
            &serde_json::to_vec(&bare_digest_carrier).expect("serialize"),
        );

        let report = downgrade_state_dir(state, MigrateMode::DryRun);
        assert!(!report.has_errors(), "{:?}", report.errors);
        assert!(
            report.stamp_barriers.is_empty(),
            "{:?}",
            report.stamp_barriers
        );
        assert!(
            report.downgrade.is_some(),
            "the pass must reach the downgrade"
        );
    }

    /// FENCED DRY RUN PIN: the default dry run executes the whole
    /// reconstruction on a write-capable connection before rolling back, so
    /// it must run under the SAME exclusive maintenance fence as apply —
    /// never beside a live gateway — with the witness census taken under
    /// that fence.
    #[test]
    fn dry_run_holds_the_maintenance_fence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path();
        let session = schema_v1_session("downgrade-fenced");
        let db = continuity_db_with_v1_ddl(state);
        insert_snapshot(
            &db,
            &session.id().to_string(),
            &serde_json::to_vec(&session).expect("serialize"),
        );

        // A clean dry run reports the databases its fence actually held.
        let report = downgrade_state_dir(state, MigrateMode::DryRun);
        assert!(!report.has_errors(), "{:?}", report.errors);
        assert_eq!(report.fenced_databases, vec![db.clone()]);
        assert!(meerkat_sqlite::fence_lock_path(&db).is_file());

        // A foreign fence holder (another process's exclusive lock, with no
        // in-process holder-registry entry) must refuse the dry run
        // outright, exactly as it refuses apply.
        let foreign = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(meerkat_sqlite::fence_lock_path(&db))
            .expect("open foreign lock");
        foreign.try_lock().expect("foreign exclusive lock");
        let refused = downgrade_state_dir(state, MigrateMode::DryRun);
        assert!(refused.has_errors());
        assert!(
            refused.errors[0].contains("maintenance fence not acquirable"),
            "{:?}",
            refused.errors
        );
        assert!(
            refused.downgrade.is_none(),
            "no write-capable connection may open while the fence is refused"
        );
        drop(foreign);
    }

    fn continuity_ledger_version(path: &Path) -> Option<i64> {
        let conn = Connection::open(path).expect("probe");
        meerkat_sqlite::domain_version(&conn, "mobkit-continuity").expect("ledger")
    }

    /// Drive a state dir's continuity file through the REAL upgrade: seed a
    /// v1 whole-document blob, then take one delta-channel turn, which
    /// migrates the session into head+rows and stamps the ledger at v2 —
    /// the exact file shape `storage-downgrade` exists to roll back.
    async fn stamped_head_canonical_state_dir(state: &Path) -> meerkat_core::types::SessionId {
        use crate::identity_first::{
            AgentIdentity, AgentRuntimeId, CheckpointVersion, ContinuityGeneration,
            ContinuityIncrementalSessions as _, ContinuityRecord, ContinuityStore as _,
            ContinuityWriteCursor, FencingToken, SessionSnapshot,
        };

        let store =
            LocalContinuityStore::open(state.join("continuity.sqlite3")).expect("open fixture");
        let identity = AgentIdentity::parse("triage:downgrade").expect("identity");
        let mut document = meerkat_core::Session::new();
        document.push(Message::User(UserMessage::text("turn one".to_string())));
        let session_id = document.id().clone();
        store
            .upsert_continuity_record(
                &ContinuityRecord {
                    identity: identity.clone(),
                    agent_runtime_id: AgentRuntimeId::parse("rt-001").expect("runtime id"),
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
                    data: serde_json::to_vec(&document).expect("serialize"),
                },
            )
            .await
            .expect("pre-upgrade whole-blob save");

        // One post-upgrade turn through the delta channel: the migrating
        // append moves the blob into head+rows and stamps the ledger at v2.
        let root = meerkat_core::TranscriptStrandId::root();
        let base = document.messages().len() as u64;
        document.push(Message::User(UserMessage::text("turn two".to_string())));
        let cursor = ContinuityWriteCursor {
            identity: identity.clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(2),
            fencing_token: FencingToken::new(1),
        };
        store
            .append_messages(
                &cursor,
                &session_id,
                &root,
                base,
                &document.messages()[base as usize..],
            )
            .await
            .expect("delta append");
        let stored = store
            .load_canonical_head(&session_id)
            .await
            .expect("stored head")
            .expect("the migrating append created the head");
        let head = meerkat_core::SessionHead::from_session(&document, root, stored.rewrite_count)
            .expect("project head");
        let token =
            meerkat_core::session_store::session_head_cas_token(&stored).expect("cas token");
        let save_cursor = ContinuityWriteCursor {
            identity,
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(3),
            fencing_token: FencingToken::new(1),
        };
        store
            .save_head(
                &save_cursor,
                &head,
                meerkat_core::session_store::SessionHeadCas::IfToken(token),
            )
            .await
            .expect("save head");
        session_id
    }

    /// DRY-RUN HONESTY PIN on the stamped path: a clean dry run over a
    /// genuinely v2-stamped head-canonical file must report the outcome it
    /// PROVED — the lockout would be lifted — and render the readability
    /// line with the dry-run suffix, while leaving the file byte-identical.
    /// Before this pin, the dry-run report reset `ledger_after` to the
    /// pre-run version, `lockout_lifted()` returned false against its
    /// documented "would leave" contract, and the report never told the
    /// operator that `--apply` would reopen the file to previous releases.
    #[tokio::test]
    async fn stamped_dry_run_reports_would_lift_and_renders_the_dry_run_suffix() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path();
        stamped_head_canonical_state_dir(state).await;
        let db = state.join("continuity.sqlite3");
        assert_eq!(
            continuity_ledger_version(&db),
            Some(2),
            "premise: the fixture is genuinely stamped head-canonical"
        );

        let before = file_digest(&db);
        let report = downgrade_state_dir(state, MigrateMode::DryRun);
        assert!(!report.has_errors(), "{:?}", report.errors);
        let downgrade = report.downgrade.as_ref().expect("the pass ran");
        assert!(!downgrade.applied);
        assert_eq!(downgrade.ledger_before, Some(2));
        assert_eq!(
            downgrade.ledger_after,
            Some(1),
            "the dry run reports the rewind it proved, not the on-disk state"
        );
        assert!(
            report.lockout_lifted(),
            "a clean dry run on a stamped file reports that --apply WOULD lift the lockout"
        );
        assert_eq!(
            file_digest(&db),
            before,
            "a dry run must leave the file byte-identical"
        );
        assert_eq!(
            continuity_ledger_version(&db),
            Some(2),
            "the rollback keeps the stamp on disk"
        );

        let rendered = render_downgrade_report(&report);
        assert!(
            rendered.contains("ledger mobkit-continuity: Some(2) -> Some(1)"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "previous releases can open this file (after --apply; this run rolled back)"
            ),
            "the stamped dry-run path must render the readability line with the dry-run \
             suffix: {rendered}"
        );

        // And apply states it plainly, without the suffix.
        let report = downgrade_state_dir(state, MigrateMode::Apply);
        assert!(!report.has_errors(), "{:?}", report.errors);
        assert!(report.lockout_lifted());
        let rendered = render_downgrade_report(&report);
        assert!(
            rendered.contains("previous releases can open this file —"),
            "{rendered}"
        );
        assert!(!rendered.contains("this run rolled back"), "{rendered}");
        assert_eq!(continuity_ledger_version(&db), Some(1));
    }
}
