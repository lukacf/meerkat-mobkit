//! H3 — continuity snapshot checkpoint adoption (storage-unification plan).
//!
//! Session bytes written by pre-typed (0.7.x-era) MobKit live inside
//! `session_snapshots.data` in the continuity database. On 0.8.x they decode
//! as `LegacyUnverified` and hard-fail every committed-authority read, so a
//! resume through the identity-first path refuses (correctly — refusing is
//! what preserves the transcript) and the identity stays `Broken` on every
//! reconcile retry. Meerkat PR #909's machine-owned lazy migration heals the
//! documents meerkat itself reads (its session store and runtime snapshots);
//! it never sees continuity snapshots. This module is the continuity-side
//! half: it stamps those bytes via the exported
//! [`meerkat_core::adopt_legacy_session`] helper with the **observed cursor**
//! the snapshot row itself records, validated against the matching
//! `continuity_records` row.
//!
//! # The sanctioned invocation shape
//!
//! - **Batch, in a maintenance window** ([`adopt_continuity_snapshots`],
//!   surfaced as the `mobkit_gateway storage-adopt-checkpoints` subcommand).
//!   Enumeration and rewrite happen via direct SQL on the continuity
//!   database: the [`ContinuityStore`](super::contracts::ContinuityStore)
//!   trait has no enumeration API, and its `save_session_snapshot` CAS
//!   requires a strictly-increasing `CheckpointVersion`, so an in-place
//!   rewrite at the same version is inexpressible through the trait. That is
//!   exactly why this is a **fenced maintenance operation**: the batch holds
//!   the [`meerkat_sqlite::ExclusiveFence`] on the database file for the
//!   whole walk, opens a `Maintenance` connection, and rewrites each row's
//!   `data` bytes in place — the row's `generation`, `checkpoint_version`,
//!   and `fencing_token` columns are deliberately **not** changed. Adoption
//!   changes document bytes, not continuity bookkeeping; the typed stamp
//!   inside the bytes binds to the observed cursor the row already records.
//!
//! A second shape — lazy-at-restore adoption inside
//! [`ContinuitySessionStoreAdapter`](super::adapters::ContinuitySessionStoreAdapter),
//! enabled per wiring site via `with_lazy_checkpoint_adoption(true)` — was
//! removed under the 0.8.10-floor policy: `LegacyUnverified` rows cannot
//! exist in any supported corpus, so an always-on unbrick path for 0.7.x-era
//! bytes has no remaining audience. Pre-0.8.10 corpora go through the batch
//! maintenance verb.
//!
//! # Ordering constraint (nonzero-generation fleets)
//!
//! Meerkat's lazy auto-migration seeds `INITIAL` cursors, and a verified
//! document never re-migrates — a prematurely stamped lower generation is
//! sticky. On any fleet whose continuity rows record a nonzero generation
//! floor, this adoption (the batch verb, using the observed cursor) must run
//! **before** meerkat's lazy path first touches those sessions.
//! Generation-0 fleets are unaffected. The batch verb
//! composes with state-generation deploy machinery: clone the generation, run
//! the batch against the clone as a deploy data transition, boot the
//! candidate, flip only on proof.
//!
//! # Classification
//!
//! Every `session_snapshots` row is classified, never skipped silently:
//! already stamped (verified) rows are counted and left untouched;
//! legacy-unverified rows whose matching record agrees with the row's own
//! `checkpoint_version` are adopted (the stamp binds the row's version
//! custody); rows whose owning record was rebound away or whose generation
//! was superseded are **stale** — reported, never adopted, never an error
//! for the walk; rows whose record cursor diverges from the row's own
//! `checkpoint_version` (a lost or raced snapshot write) are refused typed —
//! bytes are never certified at a version that does not belong to them;
//! undecodable payloads are reported; helper refusals (typed-but-broken
//! documents, key/embedded-id mismatches) land in `refused` with the reason.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::local_store::MOBKIT_CONTINUITY_DOMAIN;

/// How long the exclusive maintenance fence waits for in-flight store
/// operations to drain before reporting the fence as unavailable.
const FENCE_DRAIN_DEADLINE: Duration = Duration::from_secs(10);

/// Invocation mode for [`adopt_continuity_snapshots`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptionMode {
    /// Census only. The database is opened read-only and stays
    /// byte-identical; the report counts what an apply run would do.
    DryRun,
    /// Rewrite legacy rows in place (one transaction for the whole batch).
    Apply,
}

/// One row the adoption helper refused, with the reason. Refusals never abort
/// the walk; they surface in the report (and as a nonzero maintenance-verb
/// exit code) for the operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptionRefusal {
    pub session_id: String,
    pub reason: String,
}

/// Census of one adoption walk over `session_snapshots`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuityAdoptionReport {
    /// Rows visited.
    pub scanned: usize,
    /// Rows already carrying a verified typed stamp (untouched).
    pub already_stamped: usize,
    /// Rows adopted (in [`AdoptionMode::DryRun`]: rows that would be).
    pub adopted: usize,
    /// Rows whose owning continuity record was rebound to another session or
    /// advanced to a newer generation — stale snapshot rows, reported and
    /// left alone.
    pub stale_rows: usize,
    /// Rows whose payload does not decode as a meerkat session document.
    pub undecodable: usize,
    /// Rows the stamping helper (or its preconditions) refused, with reasons.
    pub refused: Vec<AdoptionRefusal>,
}

impl ContinuityAdoptionReport {
    /// Whether the walk completed without refusals.
    pub fn is_clean(&self) -> bool {
        self.refused.is_empty()
    }
}

impl fmt::Display for ContinuityAdoptionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "scanned:         {}", self.scanned)?;
        writeln!(f, "already stamped: {}", self.already_stamped)?;
        writeln!(f, "adopted:         {}", self.adopted)?;
        writeln!(f, "stale rows:      {}", self.stale_rows)?;
        writeln!(f, "undecodable:     {}", self.undecodable)?;
        write!(f, "refused:         {}", self.refused.len())?;
        for refusal in &self.refused {
            write!(f, "\n  {}: {}", refusal.session_id, refusal.reason)?;
        }
        Ok(())
    }
}

/// Failure of the adoption walk itself (never a per-row classification).
#[derive(Debug)]
pub enum ContinuityAdoptionError {
    /// The exclusive maintenance fence could not be acquired (another
    /// process holds it, or in-flight store operations did not drain).
    FenceUnavailable { path: PathBuf, detail: String },
    /// The database could not be opened under the maintenance profile.
    Open { path: PathBuf, detail: String },
    /// The file exists but does not carry the continuity schema.
    NotAContinuityDatabase { path: PathBuf, detail: String },
    /// The file's schema ledger records a version newer than this build
    /// understands; refusing before any mutation.
    SchemaFromTheFuture { path: PathBuf, ledger_version: i64 },
    /// A SQL failure mid-walk (the apply transaction is rolled back).
    Sql(String),
    /// The blocking worker running the walk failed.
    Worker(String),
}

impl fmt::Display for ContinuityAdoptionError {
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
            Self::NotAContinuityDatabase { path, detail } => write!(
                f,
                "{} is not a continuity database: {detail}",
                path.display()
            ),
            Self::SchemaFromTheFuture {
                path,
                ledger_version,
            } => write!(
                f,
                "{} records continuity schema version {ledger_version}, newer than this \
                 build supports; upgrade the binary before adopting",
                path.display()
            ),
            Self::Sql(detail) => write!(f, "continuity adoption SQL failure: {detail}"),
            Self::Worker(detail) => {
                write!(f, "continuity adoption worker failure: {detail}")
            }
        }
    }
}

impl std::error::Error for ContinuityAdoptionError {}

/// Walk every `session_snapshots` row in the continuity database at
/// `db_path`, adopting legacy-unverified session documents into typed
/// checkpoint authority with the observed cursor from the matching
/// `continuity_records` row. See the module docs for the full contract.
///
/// Holds the exclusive maintenance fence on the database file for the whole
/// batch. Store operations from the same process pass their per-operation
/// guards via the fence's holder self-admission; foreign processes fail typed
/// until the fence drops.
///
/// # Errors
///
/// Returns [`ContinuityAdoptionError`] when the fence cannot be acquired, the
/// file cannot be opened or is not a continuity database, its schema ledger
/// is from the future, or SQL fails mid-walk. Per-row conditions never error
/// the walk; they are classified in the report.
pub async fn adopt_continuity_snapshots(
    db_path: &Path,
    mode: AdoptionMode,
) -> Result<ContinuityAdoptionReport, ContinuityAdoptionError> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || adopt_continuity_snapshots_blocking(&db_path, mode))
        .await
        .map_err(|error| ContinuityAdoptionError::Worker(error.to_string()))?
}

/// Blocking core of [`adopt_continuity_snapshots`]. Exposed for callers that
/// are not on a Tokio runtime (the maintenance subcommand's harness, future
/// `StorageMigrator` participation under Meerkat's Phase 6 fence).
pub fn adopt_continuity_snapshots_blocking(
    db_path: &Path,
    mode: AdoptionMode,
) -> Result<ContinuityAdoptionReport, ContinuityAdoptionError> {
    adopt_with_fence_deadline(db_path, mode, FENCE_DRAIN_DEADLINE)
}

/// Adoption walk for a caller that **already holds** the exclusive
/// maintenance fence on `db_path` — the M6 `storage-migrate` verb fences
/// every state-dir database for its whole pass, and the fence file lock is
/// not re-entrant (a second acquisition in the same process blocks on the
/// first), so re-acquiring here would deadlock the combined pass against
/// itself. This is the sanctioned fence-composition seam: the M6 fence is
/// meerkat's Phase 6 primitive, and case 4 runs under that same fence pass
/// instead of releasing and re-taking it (which would open a window for a
/// foreign holder mid-migration).
///
/// Safety net: the walk still *tries* the fence non-blocking. If no one
/// actually holds it, it is taken here (and released when the walk ends), so
/// the quiescence contract holds even for a caller that lied.
pub fn adopt_continuity_snapshots_already_fenced(
    db_path: &Path,
    mode: AdoptionMode,
) -> Result<ContinuityAdoptionReport, ContinuityAdoptionError> {
    if !db_path.is_file() {
        return Err(ContinuityAdoptionError::Open {
            path: db_path.to_path_buf(),
            detail: "database file does not exist".to_string(),
        });
    }
    let _fence = meerkat_sqlite::ExclusiveFence::try_acquire(db_path).map_err(|error| {
        ContinuityAdoptionError::FenceUnavailable {
            path: db_path.to_path_buf(),
            detail: error.to_string(),
        }
    })?;
    adopt_quiesced(db_path, mode)
}

fn adopt_with_fence_deadline(
    db_path: &Path,
    mode: AdoptionMode,
    fence_deadline: Duration,
) -> Result<ContinuityAdoptionReport, ContinuityAdoptionError> {
    // Refuse a missing file before creating any fence lock file next to it.
    if !db_path.is_file() {
        return Err(ContinuityAdoptionError::Open {
            path: db_path.to_path_buf(),
            detail: "database file does not exist".to_string(),
        });
    }

    // Fence first: quiesce every fence-aware store on this file, then keep
    // the fence for the whole batch so nothing interleaves with the walk.
    let _fence =
        meerkat_sqlite::ExclusiveFence::acquire(db_path, fence_deadline).map_err(|error| {
            ContinuityAdoptionError::FenceUnavailable {
                path: db_path.to_path_buf(),
                detail: error.to_string(),
            }
        })?;

    adopt_quiesced(db_path, mode)
}

/// The walk body, assuming the database is already quiesced (the caller
/// holds the exclusive maintenance fence, one way or another).
fn adopt_quiesced(
    db_path: &Path,
    mode: AdoptionMode,
) -> Result<ContinuityAdoptionReport, ContinuityAdoptionError> {
    // DryRun opens read-only, making byte-identity a property of the
    // connection rather than of walk discipline.
    let profile = meerkat_sqlite::ConnectionProfile::Maintenance {
        write: matches!(mode, AdoptionMode::Apply),
    };
    let mut conn =
        meerkat_sqlite::open(db_path, profile).map_err(|error| ContinuityAdoptionError::Open {
            path: db_path.to_path_buf(),
            detail: error.to_string(),
        })?;

    ensure_continuity_schema(&conn, db_path)?;

    // One transaction for the whole batch: Apply commits atomically,
    // DryRun's deferred read transaction pins a consistent view.
    let behavior = match mode {
        AdoptionMode::Apply => TransactionBehavior::Immediate,
        AdoptionMode::DryRun => TransactionBehavior::Deferred,
    };
    let tx = conn
        .transaction_with_behavior(behavior)
        .map_err(|error| ContinuityAdoptionError::Sql(format!("begin: {error}")))?;

    let report = walk_snapshot_rows(&tx, mode)?;

    match mode {
        AdoptionMode::Apply => tx
            .commit()
            .map_err(|error| ContinuityAdoptionError::Sql(format!("commit: {error}")))?,
        // Dropping the deferred read transaction rolls back nothing — there
        // is nothing to roll back on a read-only connection.
        AdoptionMode::DryRun => drop(tx),
    }

    Ok(report)
}

/// The continuity schema must be present, and a ledgered file must not be
/// from the future. A pre-ledger file (0.7.x-era, never opened by an M3
/// binary) has no `meerkat_schema` table and is exactly the population this
/// verb exists for.
fn ensure_continuity_schema(
    conn: &Connection,
    db_path: &Path,
) -> Result<(), ContinuityAdoptionError> {
    for table in ["continuity_records", "session_snapshots"] {
        let present: Option<String> = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| ContinuityAdoptionError::Sql(format!("schema probe: {error}")))?;
        if present.is_none() {
            return Err(ContinuityAdoptionError::NotAContinuityDatabase {
                path: db_path.to_path_buf(),
                detail: format!("missing table {table}"),
            });
        }
    }
    let ledger_version = meerkat_sqlite::domain_version(conn, MOBKIT_CONTINUITY_DOMAIN.name)
        .map_err(|error| ContinuityAdoptionError::Sql(format!("ledger probe: {error}")))?;
    if let Some(version) = ledger_version
        && version > MOBKIT_CONTINUITY_DOMAIN.supported_version()
    {
        return Err(ContinuityAdoptionError::SchemaFromTheFuture {
            path: db_path.to_path_buf(),
            ledger_version: version,
        });
    }
    Ok(())
}

/// The observed cursor for one identity, as recorded by its continuity row.
struct RecordCursor {
    session_id: String,
    generation: u64,
    checkpoint_version: u64,
}

/// Small per-row metadata; payloads are fetched one row at a time so the
/// walk never buffers every snapshot blob at once.
struct SnapshotRowMeta {
    session_id: String,
    identity: String,
    generation: u64,
    checkpoint_version: u64,
    fencing_token: u64,
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, rusqlite::Error> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn walk_snapshot_rows(
    tx: &Connection,
    mode: AdoptionMode,
) -> Result<ContinuityAdoptionReport, ContinuityAdoptionError> {
    fn sql_err(context: &'static str) -> impl Fn(rusqlite::Error) -> ContinuityAdoptionError {
        move |error| ContinuityAdoptionError::Sql(format!("{context}: {error}"))
    }

    let records: HashMap<String, RecordCursor> = {
        let mut stmt = tx
            .prepare(
                "SELECT identity, session_id, generation, checkpoint_version \
                 FROM continuity_records",
            )
            .map_err(sql_err("prepare records"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    RecordCursor {
                        session_id: row.get(1)?,
                        generation: row.get(2)?,
                        checkpoint_version: row.get(3)?,
                    },
                ))
            })
            .map_err(sql_err("query records"))?;
        let mut map = HashMap::new();
        for row in rows {
            let (identity, cursor) = row.map_err(sql_err("read record row"))?;
            map.insert(identity, cursor);
        }
        map
    };

    // Canonical-representation rule (M4b): a `continuity_session_heads` row
    // means that session's blob is a frozen archive that is never read or
    // written again. Adopting into an archive would write bytes nothing
    // serves and would re-stamp a document the head row already supersedes;
    // head-canonical documents adopt live through the adapter's load path.
    let head_canonical_sql = if table_exists(tx, "continuity_session_heads")
        .map_err(sql_err("probe head-canonical tables"))?
    {
        " WHERE session_id NOT IN (SELECT session_id FROM continuity_session_heads)"
    } else {
        ""
    };

    let snapshot_rows: Vec<SnapshotRowMeta> = {
        let mut stmt = tx
            .prepare(&format!(
                "SELECT session_id, identity, generation, checkpoint_version, fencing_token \
                 FROM session_snapshots{head_canonical_sql} ORDER BY session_id"
            ))
            .map_err(sql_err("prepare snapshots"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SnapshotRowMeta {
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

    let mut report = ContinuityAdoptionReport::default();
    for row in snapshot_rows {
        report.scanned += 1;
        let data: Vec<u8> = tx
            .query_row(
                "SELECT data FROM session_snapshots WHERE session_id = ?1",
                [&row.session_id],
                |r| r.get(0),
            )
            .map_err(sql_err("load snapshot payload"))?;

        let session: meerkat_core::Session = match serde_json::from_slice(&data) {
            Ok(session) => session,
            Err(error) => {
                tracing::warn!(
                    session_id = %row.session_id,
                    %error,
                    "continuity adoption: snapshot payload does not decode as a session document"
                );
                report.undecodable += 1;
                continue;
            }
        };

        if session.id().to_string() != row.session_id {
            report.refused.push(AdoptionRefusal {
                session_id: row.session_id.clone(),
                reason: format!(
                    "snapshot row key does not match embedded session id {}",
                    session.id()
                ),
            });
            continue;
        }

        match session.try_checkpoint_state() {
            Ok(meerkat_core::SessionCheckpointState::Verified(_)) => {
                report.already_stamped += 1;
                continue;
            }
            Ok(meerkat_core::SessionCheckpointState::LegacyUnverified { .. }) => {}
            Err(error) => {
                // Decodable, stamped, but the stamp fails verification
                // (digest mismatch or malformed provenance). Never
                // re-stamped; the operator owns this row.
                report.refused.push(AdoptionRefusal {
                    session_id: row.session_id.clone(),
                    reason: format!("checkpoint state unreadable: {error}"),
                });
                continue;
            }
        }

        // The observed cursor comes from the matching continuity record: the
        // record must still bind this identity to this session at this
        // generation. A rebound-away or generation-superseded snapshot is a
        // stale row: classified and reported, never adopted, never an error.
        let cursor = records.get(&row.identity).filter(|cursor| {
            cursor.session_id == row.session_id && cursor.generation == row.generation
        });
        let Some(cursor) = cursor else {
            tracing::info!(
                session_id = %row.session_id,
                identity = %row.identity,
                generation = row.generation,
                "continuity adoption: stale snapshot row (no matching continuity record)"
            );
            report.stale_rows += 1;
            continue;
        };

        // The stamp certifies the row's BYTES, so the revision it binds must
        // be the one those bytes were saved under — the row's own
        // checkpoint_version. A record cursor that advanced past the row (or
        // lags it) means a snapshot write or record advance was lost or
        // raced: certifying the bytes at either version would be a truth
        // inversion, so the row is refused typed for the operator.
        if row.checkpoint_version != cursor.checkpoint_version {
            report.refused.push(AdoptionRefusal {
                session_id: row.session_id.clone(),
                reason: format!(
                    "checkpoint version divergence: snapshot row holds version {} but the \
                     continuity record's cursor is {}",
                    row.checkpoint_version, cursor.checkpoint_version
                ),
            });
            continue;
        }

        // Generation and checkpoint_version are pinned equal to the record's
        // cursor by the filter and divergence refusal above; stamping from
        // the row makes the byte-custody binding explicit.
        let adopted = match meerkat_core::adopt_legacy_session(
            &data,
            meerkat_core::SessionGeneration::new(row.generation),
            meerkat_core::SessionCheckpointRevision::new(row.checkpoint_version),
        ) {
            Ok(adopted) => adopted,
            Err(error) => {
                report.refused.push(AdoptionRefusal {
                    session_id: row.session_id.clone(),
                    reason: error.to_string(),
                });
                continue;
            }
        };

        if matches!(mode, AdoptionMode::Apply) {
            // In-place byte rewrite. generation / checkpoint_version /
            // fencing_token stay exactly as observed: the stamp binds to
            // that cursor, and rewriting bookkeeping would forge continuity
            // history. The WHERE clause re-asserts the observed tuple —
            // belt and braces under the already-held exclusive fence.
            let changed = tx
                .execute(
                    "UPDATE session_snapshots SET data = ?1 \
                     WHERE session_id = ?2 AND identity = ?3 AND generation = ?4 \
                       AND checkpoint_version = ?5 AND fencing_token = ?6",
                    rusqlite::params![
                        adopted.serialized,
                        row.session_id,
                        row.identity,
                        row.generation,
                        row.checkpoint_version,
                        row.fencing_token,
                    ],
                )
                .map_err(sql_err("rewrite snapshot payload"))?;
            if changed != 1 {
                report.refused.push(AdoptionRefusal {
                    session_id: row.session_id.clone(),
                    reason: "snapshot row moved during the fenced walk".to_string(),
                });
                continue;
            }
        }
        tracing::info!(
            session_id = %row.session_id,
            identity = %row.identity,
            observed_generation = row.generation,
            observed_checkpoint_revision = row.checkpoint_version,
            applied = matches!(mode, AdoptionMode::Apply),
            "continuity adoption: stamped legacy session snapshot with the observed cursor"
        );
        report.adopted += 1;
    }

    Ok(report)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    /// The exact 0.7.x-era continuity DDL: two tables, no `meerkat_schema`
    /// ledger. Raw fixtures model what pre-M3 binaries actually wrote.
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

    fn fixture_db(dir: &Path) -> PathBuf {
        let path = dir.join("continuity.db");
        let conn = Connection::open(&path).expect("create fixture db");
        conn.execute_batch(LEGACY_DDL).expect("apply legacy ddl");
        path
    }

    fn insert_record(
        conn: &Connection,
        identity: &str,
        session_id: &str,
        generation: u64,
        checkpoint_version: u64,
        fencing_token: u64,
    ) {
        conn.execute(
            "INSERT INTO continuity_records \
             (identity, agent_runtime_id, session_id, generation, checkpoint_version, fencing_token) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![identity, "rt-1", session_id, generation, checkpoint_version, fencing_token],
        )
        .expect("insert record");
    }

    fn insert_snapshot(
        conn: &Connection,
        session_id: &str,
        identity: &str,
        generation: u64,
        checkpoint_version: u64,
        fencing_token: u64,
        data: &[u8],
    ) {
        conn.execute(
            "INSERT INTO session_snapshots \
             (session_id, identity, generation, checkpoint_version, fencing_token, data) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                session_id,
                identity,
                generation,
                checkpoint_version,
                fencing_token,
                data
            ],
        )
        .expect("insert snapshot");
    }

    fn snapshot_row(db: &Path, session_id: &str) -> (u64, u64, u64, Vec<u8>) {
        let conn = Connection::open(db).expect("open for row read");
        conn.query_row(
            "SELECT generation, checkpoint_version, fencing_token, data \
             FROM session_snapshots WHERE session_id = ?1",
            [session_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            },
        )
        .expect("snapshot row")
    }

    fn file_digest(path: &Path) -> String {
        let bytes = std::fs::read(path).expect("read db file");
        format!("{:x}", Sha256::digest(&bytes))
    }

    fn verified_stamp(bytes: &[u8]) -> meerkat_core::SessionCheckpointStamp {
        let session: meerkat_core::Session =
            serde_json::from_slice(bytes).expect("decode adopted session");
        match session.try_checkpoint_state().expect("checkpoint state") {
            meerkat_core::SessionCheckpointState::Verified(stamp) => stamp,
            other => panic!("expected a verified document, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn apply_binds_observed_cursor_and_rewrites_bytes_in_place() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = fixture_db(dir.path());
        let (sid, legacy) = legacy_session_bytes();
        {
            let conn = Connection::open(&db).expect("open fixture");
            insert_record(&conn, "test:alice", &sid, 3, 4, 7);
            insert_snapshot(&conn, &sid, "test:alice", 3, 4, 7, &legacy);
        }

        let report = adopt_continuity_snapshots(&db, AdoptionMode::Apply)
            .await
            .expect("apply walk");
        assert_eq!(report.scanned, 1);
        assert_eq!(report.adopted, 1);
        assert_eq!(report.already_stamped, 0);
        assert_eq!(report.stale_rows, 0);
        assert_eq!(report.undecodable, 0);
        assert!(report.is_clean());

        let (generation, version, fence, data) = snapshot_row(&db, &sid);
        // Continuity bookkeeping untouched: bytes changed, cursor columns not.
        assert_eq!((generation, version, fence), (3, 4, 7));
        assert_ne!(data, legacy, "apply must rewrite the payload in place");
        let stamp = verified_stamp(&data);
        // The stamp binds the OBSERVED cursor from the record — generation 3,
        // never INITIAL.
        assert_eq!(stamp.generation(), meerkat_core::SessionGeneration::new(3));
        assert_eq!(
            stamp.checkpoint_revision(),
            meerkat_core::SessionCheckpointRevision::new(4)
        );
    }

    #[tokio::test]
    async fn already_stamped_rows_are_skipped_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = fixture_db(dir.path());
        let (sid, legacy) = legacy_session_bytes();
        let adopted = meerkat_core::adopt_legacy_session(
            &legacy,
            meerkat_core::SessionGeneration::new(2),
            meerkat_core::SessionCheckpointRevision::new(9),
        )
        .expect("pre-adopt fixture");
        {
            let conn = Connection::open(&db).expect("open fixture");
            insert_record(&conn, "test:alice", &sid, 2, 9, 1);
            insert_snapshot(&conn, &sid, "test:alice", 2, 9, 1, &adopted.serialized);
        }

        let report = adopt_continuity_snapshots(&db, AdoptionMode::Apply)
            .await
            .expect("apply walk");
        assert_eq!(report.scanned, 1);
        assert_eq!(report.already_stamped, 1);
        assert_eq!(report.adopted, 0);
        let (_, _, _, data) = snapshot_row(&db, &sid);
        assert_eq!(
            data, adopted.serialized,
            "verified rows must not be rewritten"
        );
    }

    #[tokio::test]
    async fn stale_rows_are_reported_not_adopted_and_never_error_the_walk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = fixture_db(dir.path());
        // Row 1: no continuity record at all (identity deleted / never bound).
        let (orphan_sid, orphan_bytes) = legacy_session_bytes();
        // Row 2: the record was rebound to a NEWER session id; this snapshot
        // is keyed by the rebound-away session.
        let (old_sid, old_bytes) = legacy_session_bytes();
        // Row 3: adoptable control row, proving stale rows do not stop it.
        let (live_sid, live_bytes) = legacy_session_bytes();
        {
            let conn = Connection::open(&db).expect("open fixture");
            insert_snapshot(&conn, &orphan_sid, "test:ghost", 0, 1, 1, &orphan_bytes);
            insert_record(&conn, "test:bob", "session-rebound-elsewhere", 0, 6, 3);
            insert_snapshot(&conn, &old_sid, "test:bob", 0, 2, 2, &old_bytes);
            insert_record(&conn, "test:alice", &live_sid, 0, 5, 4);
            insert_snapshot(&conn, &live_sid, "test:alice", 0, 5, 4, &live_bytes);
        }

        let report = adopt_continuity_snapshots(&db, AdoptionMode::Apply)
            .await
            .expect("apply walk");
        assert_eq!(report.scanned, 3);
        assert_eq!(report.stale_rows, 2);
        assert_eq!(report.adopted, 1);
        assert!(report.is_clean());

        // The stale rows keep their legacy bytes exactly.
        assert_eq!(snapshot_row(&db, &orphan_sid).3, orphan_bytes);
        assert_eq!(snapshot_row(&db, &old_sid).3, old_bytes);
        let stamp = verified_stamp(&snapshot_row(&db, &live_sid).3);
        assert_eq!(
            stamp.checkpoint_revision(),
            meerkat_core::SessionCheckpointRevision::new(5)
        );
    }

    #[tokio::test]
    async fn version_divergence_between_row_and_record_is_refused_not_promoted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = fixture_db(dir.path());
        // Row behind record: the record's cursor advanced to 9 but these
        // bytes were saved at 4 — a newer snapshot write was lost or raced.
        let (behind_sid, behind_bytes) = legacy_session_bytes();
        // Row ahead of record: the record advance was lost instead.
        let (ahead_sid, ahead_bytes) = legacy_session_bytes();
        // Adoptable control row proving divergent rows do not stop the walk.
        let (live_sid, live_bytes) = legacy_session_bytes();
        {
            let conn = Connection::open(&db).expect("open fixture");
            insert_record(&conn, "test:behind", &behind_sid, 3, 9, 7);
            insert_snapshot(&conn, &behind_sid, "test:behind", 3, 4, 7, &behind_bytes);
            insert_record(&conn, "test:ahead", &ahead_sid, 0, 2, 1);
            insert_snapshot(&conn, &ahead_sid, "test:ahead", 0, 5, 1, &ahead_bytes);
            insert_record(&conn, "test:alice", &live_sid, 0, 6, 2);
            insert_snapshot(&conn, &live_sid, "test:alice", 0, 6, 2, &live_bytes);
        }

        let report = adopt_continuity_snapshots(&db, AdoptionMode::Apply)
            .await
            .expect("apply walk");
        assert_eq!(report.scanned, 3);
        assert_eq!(report.adopted, 1);
        assert_eq!(report.stale_rows, 0);
        assert_eq!(report.refused.len(), 2);
        assert!(!report.is_clean());
        let mut refused_ids: Vec<&str> = report
            .refused
            .iter()
            .map(|refusal| refusal.session_id.as_str())
            .collect();
        refused_ids.sort_unstable();
        let mut expected = [behind_sid.as_str(), ahead_sid.as_str()];
        expected.sort_unstable();
        assert_eq!(refused_ids, expected);
        for refusal in &report.refused {
            assert!(
                refusal.reason.contains("checkpoint version divergence"),
                "divergence must be refused typed, got: {}",
                refusal.reason
            );
        }

        // Divergent rows keep their legacy bytes exactly: never certified at
        // a version that does not belong to them, in either direction.
        assert_eq!(snapshot_row(&db, &behind_sid).3, behind_bytes);
        assert_eq!(snapshot_row(&db, &ahead_sid).3, ahead_bytes);
        let stamp = verified_stamp(&snapshot_row(&db, &live_sid).3);
        assert_eq!(
            stamp.checkpoint_revision(),
            meerkat_core::SessionCheckpointRevision::new(6)
        );
    }

    #[tokio::test]
    async fn dry_run_is_a_census_and_leaves_the_database_byte_identical() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = fixture_db(dir.path());
        let (sid, legacy) = legacy_session_bytes();
        {
            let conn = Connection::open(&db).expect("open fixture");
            insert_record(&conn, "test:alice", &sid, 1, 2, 1);
            insert_snapshot(&conn, &sid, "test:alice", 1, 2, 1, &legacy);
        }
        let before = file_digest(&db);

        let report = adopt_continuity_snapshots(&db, AdoptionMode::DryRun)
            .await
            .expect("dry-run walk");
        assert_eq!(report.scanned, 1);
        assert_eq!(report.adopted, 1, "dry-run counts what apply would adopt");

        assert_eq!(
            file_digest(&db),
            before,
            "dry-run must leave the database byte-identical"
        );
    }

    #[tokio::test]
    async fn second_apply_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = fixture_db(dir.path());
        let (sid, legacy) = legacy_session_bytes();
        {
            let conn = Connection::open(&db).expect("open fixture");
            insert_record(&conn, "test:alice", &sid, 0, 3, 2);
            insert_snapshot(&conn, &sid, "test:alice", 0, 3, 2, &legacy);
        }

        let first = adopt_continuity_snapshots(&db, AdoptionMode::Apply)
            .await
            .expect("first apply");
        assert_eq!(first.adopted, 1);
        let after_first = file_digest(&db);

        let second = adopt_continuity_snapshots(&db, AdoptionMode::Apply)
            .await
            .expect("second apply");
        assert_eq!(second.scanned, 1);
        assert_eq!(second.already_stamped, 1);
        assert_eq!(second.adopted, 0);
        assert_eq!(
            file_digest(&db),
            after_first,
            "a re-run must be a byte-identical no-op"
        );
    }

    #[tokio::test]
    async fn undecodable_and_mismatched_rows_are_classified() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = fixture_db(dir.path());
        let (sid, legacy) = legacy_session_bytes();
        {
            let conn = Connection::open(&db).expect("open fixture");
            insert_snapshot(&conn, "session-garbage", "test:alice", 0, 1, 1, b"not json");
            // A decodable document filed under the WRONG row key.
            insert_record(&conn, "test:bob", "session-other-key", 0, 1, 1);
            insert_snapshot(&conn, "session-other-key", "test:bob", 0, 1, 1, &legacy);
        }
        let _ = sid;

        let report = adopt_continuity_snapshots(&db, AdoptionMode::Apply)
            .await
            .expect("apply walk");
        assert_eq!(report.scanned, 2);
        assert_eq!(report.undecodable, 1);
        assert_eq!(report.refused.len(), 1);
        assert_eq!(report.refused[0].session_id, "session-other-key");
        assert!(!report.is_clean());
    }

    #[test]
    fn foreign_fence_holder_yields_a_typed_fence_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = fixture_db(dir.path());
        // Simulate ANOTHER process holding the exclusive fence: a raw
        // exclusive lock on the fence file, without this process's holder
        // registry entry.
        let lock_path = meerkat_sqlite::fence_lock_path(&db);
        let foreign = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .expect("open fence lock file");
        foreign.try_lock().expect("foreign exclusive lock");

        let err = adopt_with_fence_deadline(&db, AdoptionMode::DryRun, Duration::from_millis(50))
            .expect_err("held fence must refuse the walk");
        assert!(matches!(
            err,
            ContinuityAdoptionError::FenceUnavailable { .. }
        ));
        drop(foreign);
    }

    #[test]
    fn missing_database_refuses_before_creating_anything() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nope.db");
        let err = adopt_continuity_snapshots_blocking(&missing, AdoptionMode::DryRun)
            .expect_err("missing file must refuse");
        assert!(matches!(err, ContinuityAdoptionError::Open { .. }));
        assert!(
            !meerkat_sqlite::fence_lock_path(&missing).exists(),
            "refusal must not create a fence lock file"
        );
    }

    #[test]
    fn non_continuity_database_is_refused_typed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("other.db");
        {
            let conn = Connection::open(&db).expect("create db");
            conn.execute_batch("CREATE TABLE unrelated (x INTEGER);")
                .expect("ddl");
        }
        let err = adopt_continuity_snapshots_blocking(&db, AdoptionMode::DryRun)
            .expect_err("wrong schema must refuse");
        assert!(matches!(
            err,
            ContinuityAdoptionError::NotAContinuityDatabase { .. }
        ));
    }
}
