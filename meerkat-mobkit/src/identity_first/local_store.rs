//! Bundled SQLite-backed ContinuityStore for `persistent_state(path)` usage.
//!
//! Implements CONTRACT-06. Designed for single-process, local-disk persistence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use async_trait::async_trait;
use meerkat_core::SessionStoreError;
use meerkat_core::session_store::{
    SessionHead, SessionHeadCas, StrandLayout, TranscriptStrandId, reconstruct_rewrite_record,
    session_head_cas_token, strand_layout_for_history, validate_commit_rewrite_transition,
    validate_save_head_transition,
};
use meerkat_core::types::Message;
use meerkat_core::{Session, TranscriptRewriteCommit, TranscriptRewriteRecord};
use rusqlite::{Connection, OptionalExtension, Transaction};

use super::contracts::{
    ContinuityIncrementalSessions, ContinuityStore, ContinuityWriteCursor,
    SessionSnapshotMatchCandidate,
};
use super::types::{
    AgentIdentity, AgentRuntimeId, CheckpointVersion, ContinuityGeneration, ContinuityRecord,
    ContinuityResolveState, ContinuityStoreError, FencingToken, SessionSnapshot,
};

const READ_POOL_SIZE: usize = 4;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS continuity_records (
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

/// Head-canonical session representation (M4b): the durable trio meerkat's
/// incremental persistence contract needs, plus the `(identity, generation)`
/// stamps every mobkit continuity row carries so reset rollback and identity
/// deletion can scope their deletes — including strand rows written in the
/// crash window between `append_messages` and the head write that adopts
/// them.
const SCHEMA_HEAD_CANONICAL: &str = "CREATE TABLE IF NOT EXISTS continuity_session_heads (
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
    );
    CREATE TABLE IF NOT EXISTS continuity_strand_messages (
        session_id     TEXT NOT NULL,
        strand         TEXT NOT NULL,
        seq            INTEGER NOT NULL,
        message_json   BLOB NOT NULL,
        identity       TEXT NOT NULL,
        generation     INTEGER NOT NULL,
        created_at_ms  INTEGER NOT NULL,
        PRIMARY KEY (session_id, strand, seq)
    );
    CREATE TABLE IF NOT EXISTS continuity_session_rewrites (
        session_id     TEXT NOT NULL,
        rewrite_idx    INTEGER NOT NULL,
        parent_strand  TEXT NOT NULL,
        parent_len     INTEGER NOT NULL,
        strand         TEXT NOT NULL,
        strand_len     INTEGER NOT NULL,
        commit_json    BLOB NOT NULL,
        identity       TEXT NOT NULL,
        generation     INTEGER NOT NULL,
        created_at_ms  INTEGER NOT NULL,
        PRIMARY KEY (session_id, rewrite_idx)
    );
    CREATE INDEX IF NOT EXISTS continuity_records_session_idx
        ON continuity_records(session_id);
    CREATE INDEX IF NOT EXISTS continuity_heads_identity_gen_idx
        ON continuity_session_heads(identity, generation);
    CREATE INDEX IF NOT EXISTS continuity_strands_identity_gen_idx
        ON continuity_strand_messages(identity, generation);
    CREATE INDEX IF NOT EXISTS continuity_rewrites_identity_gen_idx
        ON continuity_session_rewrites(identity, generation);";

/// Ledger version that records "this file carries the head-canonical
/// channel". A binary whose `mobkit-continuity` domain tops out below this
/// refuses the file typed at open — correctly, because it would keep writing
/// `session_snapshots.data` while head rows are the byte authority. The
/// lockout is therefore only allowed to exist once head rows can exist:
/// see [`LocalContinuityStore::open`].
pub const HEAD_CANONICAL_SCHEMA_VERSION: i64 = 2;

/// The continuity store's schema domain in the per-file migration ledger.
/// Migration 0001 is the historical two-table DDL (all `CREATE ... IF NOT
/// EXISTS`, so a pre-ledger file converges without its rows being touched);
/// migration 0002 adds the head-canonical trio (DDL-only, additive, zero row
/// rewrites).
///
/// **This domain is NEVER applied by a plain [`LocalContinuityStore::open`].**
/// Applying it stamps v2, which locks every `<= 0.8.5` binary out of the file
/// (`SqliteStoreError::SchemaFromTheFuture`). That lockout is load-bearing
/// only once a head row exists, so it is committed at exactly two moments:
/// by a delta write that actually creates head state (armed inside that
/// write's own transaction, so a REFUSED write leaves the file at v1), and
/// by explicit operator action (`storage-migrate --apply`). Merely
/// launching a new gateway leaves rollback to the previous release intact.
pub(crate) const MOBKIT_CONTINUITY_DOMAIN: meerkat_sqlite::SchemaDomain =
    meerkat_sqlite::SchemaDomain {
        name: "mobkit-continuity",
        migrations: &[
            meerkat_sqlite::Migration {
                version: 1,
                name: "base-schema",
                apply: migration_0001_continuity_schema,
            },
            meerkat_sqlite::Migration {
                version: 2,
                name: "head-canonical-sessions",
                apply: migration_0002_head_canonical_sessions,
            },
        ],
        initialize_current: initialize_current_continuity_schema,
        allowed_existing_versions: &[1, 2],
        released_predecessors: &[meerkat_sqlite::SchemaPredecessor {
            version: 1,
            verify: verify_released_v1_continuity_schema,
        }],
        owned_objects: CONTINUITY_OWNED_OBJECTS,
        retired_objects: &[],
    };

const RELEASED_V1_CONTINUITY_OBJECTS: &[meerkat_sqlite::SchemaObject] = &[
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Table,
        name: "continuity_records",
    },
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Table,
        name: "session_snapshots",
    },
];

const CONTINUITY_OWNED_OBJECTS: &[meerkat_sqlite::SchemaObject] = &[
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Table,
        name: "continuity_records",
    },
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Table,
        name: "session_snapshots",
    },
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Table,
        name: "continuity_session_heads",
    },
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Table,
        name: "continuity_strand_messages",
    },
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Table,
        name: "continuity_session_rewrites",
    },
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Index,
        name: "continuity_records_session_idx",
    },
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Index,
        name: "continuity_heads_identity_gen_idx",
    },
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Index,
        name: "continuity_strands_identity_gen_idx",
    },
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Index,
        name: "continuity_rewrites_identity_gen_idx",
    },
];

fn initialize_current_continuity_schema(
    tx: &rusqlite::Transaction<'_>,
) -> Result<(), rusqlite::Error> {
    migration_0001_continuity_schema(tx)?;
    migration_0002_head_canonical_sessions(tx)
}

/// Frozen v1 verifier honoring the deferred-stamp design: a delta write may
/// commit the head-canonical DDL inside its own transaction and leave the
/// ledger at v1 until head state actually exists, so a v1 file legally
/// carries either the plain two-table v1 catalog or the complete current DDL.
fn verify_released_v1_continuity_schema(conn: &rusqlite::Connection) -> Result<(), String> {
    meerkat_sqlite::verify_released_schema_fingerprint(
        conn,
        &MOBKIT_CONTINUITY_DOMAIN,
        RELEASED_V1_CONTINUITY_OBJECTS,
        migration_0001_continuity_schema,
    )
    .or_else(|plain| {
        meerkat_sqlite::verify_released_schema_fingerprint(
            conn,
            &MOBKIT_CONTINUITY_DOMAIN,
            CONTINUITY_OWNED_OBJECTS,
            initialize_current_continuity_schema,
        )
        .map_err(|full| {
            format!("v1 catalog: {plain}; v1 + deferred head-canonical DDL catalog: {full}")
        })
    })
}

/// The open-time domain: migration 0001 only. Opening a fresh file converges
/// it to v1 exactly as every previous release did, and an already-v2 file is
/// left alone (the version check below runs against the FULL domain, so a v2
/// file is not "from the future").
const MOBKIT_CONTINUITY_BASELINE_DOMAIN: meerkat_sqlite::SchemaDomain =
    meerkat_sqlite::SchemaDomain {
        name: "mobkit-continuity",
        migrations: &[meerkat_sqlite::Migration {
            version: 1,
            name: "base-schema",
            apply: migration_0001_continuity_schema,
        }],
        initialize_current: migration_0001_continuity_schema,
        allowed_existing_versions: &[1],
        released_predecessors: &[],
        owned_objects: RELEASED_V1_CONTINUITY_OBJECTS,
        retired_objects: &[],
    };

fn migration_0001_continuity_schema(tx: &rusqlite::Transaction<'_>) -> Result<(), rusqlite::Error> {
    tx.execute_batch(SCHEMA)
}

fn migration_0002_head_canonical_sessions(
    tx: &rusqlite::Transaction<'_>,
) -> Result<(), rusqlite::Error> {
    tx.execute_batch(SCHEMA_HEAD_CANONICAL)
}

/// Refuse a file whose `mobkit-continuity` ledger is ahead of this binary.
///
/// Local replacement for the retired `meerkat_sqlite::refuse_future_schema`:
/// deliberately ONLY a future-version check, because the deferred-stamp
/// design legally leaves a v1 ledger over committed head-canonical DDL, a
/// shape exact per-version catalog eligibility would refuse at every open.
fn refuse_future_continuity_schema(conn: &Connection) -> Result<(), ContinuityStoreError> {
    let supported = MOBKIT_CONTINUITY_DOMAIN.supported_version();
    let found = meerkat_sqlite::domain_version(conn, MOBKIT_CONTINUITY_DOMAIN.name)
        .map_err(|e| mechanics_err("continuity schema preflight", e))?;
    match found {
        Some(found) if found > supported => Err(mechanics_err(
            "continuity schema preflight",
            meerkat_sqlite::SqliteStoreError::SchemaFromTheFuture {
                domain: MOBKIT_CONTINUITY_DOMAIN.name.to_string(),
                found,
                supported,
            },
        )),
        _ => Ok(()),
    }
}

/// Open-time schema convergence that never commits the one-way v2 bump.
///
/// Returns whether the file already carries the head-canonical channel.
///
/// - future version (> the full domain) => refused typed, nothing mutated;
/// - already v2 => nothing applied, delta channel ready;
/// - already v1 => nothing applied (the v2 bump waits for a delta write);
/// - no ledger row (fresh or pre-ledger file) => baseline v1 applied, exactly
///   as before this release.
fn converge_schema_at_open(conn: &mut Connection) -> Result<bool, ContinuityStoreError> {
    refuse_future_continuity_schema(conn)?;
    let version = meerkat_sqlite::domain_version(conn, MOBKIT_CONTINUITY_DOMAIN.name)
        .map_err(|e| mechanics_err("read continuity ledger", e))?;
    match version {
        Some(version) if version >= HEAD_CANONICAL_SCHEMA_VERSION => Ok(true),
        Some(_) => Ok(false),
        None => {
            meerkat_sqlite::apply_domain_migrations(conn, &MOBKIT_CONTINUITY_BASELINE_DOMAIN)
                .map_err(|e| mechanics_err("apply schema", e))?;
            Ok(false)
        }
    }
}

/// Bring the head-canonical tables into existence INSIDE the caller's
/// transaction, WITHOUT recording the ledger bump.
///
/// This is the half of the v1 -> v2 upgrade that is safe to speculate on:
/// the DDL is additive and `IF NOT EXISTS`, it applies the domain's own
/// migration bodies (never a second copy of the schema), and — because
/// SQLite runs DDL transactionally — a rollback removes the tables again.
/// The ledger stamp, which is the part that locks older binaries out, is
/// [`stamp_head_canonical_ledger_in_txn`] and is written only after the
/// enclosing write has actually created head state.
///
/// Refuses a file whose ledger is ahead of this binary, exactly as
/// `apply_domain_migrations` would, and refuses a file that has lost its
/// continuity ledger row (every opener converges one; its absence under a
/// write transaction is corruption, not a fresh file).
fn converge_head_canonical_schema_in_txn(tx: &Transaction<'_>) -> Result<(), ContinuityStoreError> {
    refuse_future_continuity_schema(tx)?;
    let current = meerkat_sqlite::domain_version(tx, MOBKIT_CONTINUITY_DOMAIN.name)
        .map_err(|e| mechanics_err("read continuity ledger", e))?
        .ok_or_else(|| {
            ContinuityStoreError::Corruption(
                "continuity ledger has no mobkit-continuity row; the file's migration ledger was \
                 removed after it was opened"
                    .to_string(),
            )
        })?;
    for migration in MOBKIT_CONTINUITY_DOMAIN
        .migrations
        .iter()
        .filter(|migration| migration.version > current)
    {
        (migration.apply)(tx).map_err(|e| sqlite_err("apply head-canonical schema", e))?;
    }
    Ok(())
}

/// Whether the file's `mobkit-continuity` ledger row already carries the
/// one-way v2 lockout, read INSIDE the caller's transaction.
///
/// The authority for "is the lockout committed" is this row and nothing
/// else. In particular it is NOT
/// [`LocalContinuityStoreInner::schema_is_head_canonical`], which answers
/// the different question "are the head tables queryable" and latches
/// `true` the moment the tables are observed. Those two facts diverge on
/// purpose: a delta write that creates strand rows but no head row commits
/// the DDL and leaves the ledger at v1 (see
/// [`LocalContinuityStore::delta_write`]), so the tables can exist on a
/// file that is still rollback-safe. Deciding the stamp from the table
/// probe would then skip the bump forever once that state exists — head
/// rows with no lockout, the exact hazard the lockout is for.
fn head_canonical_ledger_stamped_in_txn(
    tx: &Transaction<'_>,
) -> Result<bool, ContinuityStoreError> {
    let version = meerkat_sqlite::domain_version(tx, MOBKIT_CONTINUITY_DOMAIN.name)
        .map_err(|e| mechanics_err("read continuity ledger", e))?;
    Ok(version.is_some_and(|version| version >= HEAD_CANONICAL_SCHEMA_VERSION))
}

/// Does this session have a persisted head row right now?
///
/// The predicate that earns the one-way ledger bump. A head row is what
/// makes head+rows a session's sole byte authority; strand rows that no
/// head adopts are not part of any document (every read path gates on the
/// head row), so a file carrying only those is still correctly served by an
/// older binary from its blob.
fn session_head_exists_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
) -> Result<bool, ContinuityStoreError> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM continuity_session_heads WHERE session_id = ?1)",
        rusqlite::params![id.to_string()],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|e| sqlite_err("probe session head row", e))
}

/// Record the one-way `mobkit-continuity` v2 bump in the caller's
/// transaction — the moment binaries older than this release start being
/// refused the file.
///
/// Written last, so it commits atomically with the head state that earns
/// it. A no-op stamp (file already at v2) is harmless: the row already
/// carries this value.
fn stamp_head_canonical_ledger_in_txn(tx: &Transaction<'_>) -> Result<(), ContinuityStoreError> {
    tx.execute(
        "INSERT INTO main.meerkat_schema (domain, version) VALUES (?1, ?2)
         ON CONFLICT(domain) DO UPDATE SET version = excluded.version",
        rusqlite::params![
            MOBKIT_CONTINUITY_DOMAIN.name,
            MOBKIT_CONTINUITY_DOMAIN.supported_version()
        ],
    )
    .map_err(|e| sqlite_err("stamp head-canonical ledger", e))?;
    Ok(())
}

/// Commit the head-canonical schema (ledger `mobkit-continuity` v1 -> v2).
///
/// Called from `storage-migrate --apply` under the exclusive maintenance
/// fence — the explicit-operator route. (The implicit route, the first
/// delta write that creates head state, arms the same bump inside its own
/// write transaction; see [`LocalContinuityStore::delta_write`].) The DDL
/// is additive and `IF NOT EXISTS`; the ledger stamp is what makes the
/// upgrade one-way.
pub(crate) fn apply_head_canonical_schema(
    conn: &mut Connection,
) -> Result<meerkat_sqlite::LedgerReport, meerkat_sqlite::SqliteStoreError> {
    meerkat_sqlite::apply_domain_migrations(conn, &MOBKIT_CONTINUITY_DOMAIN)
}

/// Classify a raw SQLite failure at the store boundary: busy/locked is
/// transient, corruption is corrupt, everything else keeps the historical
/// `Io` shape. Staleness (fencing-token / checkpoint CAS conflicts) is a
/// store-contract concept decided above this layer, never here.
fn sqlite_err(context: &str, error: rusqlite::Error) -> ContinuityStoreError {
    match meerkat_sqlite::classify_sqlite_error(&error) {
        meerkat_sqlite::SqliteErrorClass::Transient => {
            ContinuityStoreError::Transient(format!("{context}: {error}"))
        }
        meerkat_sqlite::SqliteErrorClass::Corrupt => {
            ContinuityStoreError::Corruption(format!("{context}: {error}"))
        }
        meerkat_sqlite::SqliteErrorClass::Other => {
            ContinuityStoreError::Io(format!("{context}: {error}"))
        }
    }
}

/// Map a shared-mechanics error into the typed store error, routing the
/// wrapped raw SQLite failures through [`sqlite_err`]'s classification. A
/// held maintenance fence is transient by nature (storage is under offline
/// maintenance; the operation may be retried once it lifts).
fn mechanics_err(context: &str, error: meerkat_sqlite::SqliteStoreError) -> ContinuityStoreError {
    match error {
        meerkat_sqlite::SqliteStoreError::Sqlite(sql) => sqlite_err(context, sql),
        meerkat_sqlite::SqliteStoreError::MaintenanceFenceHeld { .. } => {
            ContinuityStoreError::Transient(format!("{context}: {error}"))
        }
        other => ContinuityStoreError::Io(format!("{context}: {other}")),
    }
}

struct ReadConnectionPool {
    available: Mutex<Vec<Connection>>,
    ready: Condvar,
}

impl ReadConnectionPool {
    fn new(connections: Vec<Connection>) -> Self {
        debug_assert!(!connections.is_empty());
        Self {
            available: Mutex::new(connections),
            ready: Condvar::new(),
        }
    }

    fn acquire(&self) -> Result<ReadConnectionGuard<'_>, ContinuityStoreError> {
        let mut available = self
            .available
            .lock()
            .map_err(|e| ContinuityStoreError::Io(format!("read pool lock: {e}")))?;
        loop {
            if let Some(connection) = available.pop() {
                return Ok(ReadConnectionGuard {
                    pool: self,
                    connection: Some(connection),
                });
            }
            available = self
                .ready
                .wait(available)
                .map_err(|e| ContinuityStoreError::Io(format!("read pool wait: {e}")))?;
        }
    }

    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, ContinuityStoreError>,
    ) -> Result<T, ContinuityStoreError> {
        let connection = self.acquire()?;
        operation(connection.connection()?)
    }
}

struct ReadConnectionGuard<'a> {
    pool: &'a ReadConnectionPool,
    connection: Option<Connection>,
}

impl ReadConnectionGuard<'_> {
    fn connection(&self) -> Result<&Connection, ContinuityStoreError> {
        self.connection
            .as_ref()
            .ok_or_else(|| ContinuityStoreError::Io("read pool guard lost its connection".into()))
    }
}

impl Drop for ReadConnectionGuard<'_> {
    fn drop(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        let mut available = self
            .pool
            .available
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        available.push(connection);
        self.pool.ready.notify_one();
    }
}

enum ReadConnections {
    /// `Connection::open_in_memory()` is private to one connection. Reads share
    /// the writer rather than accidentally observing independent databases.
    Writer,
    Pool(ReadConnectionPool),
}

struct LocalContinuityStoreInner {
    /// Database file path; `:memory:` for in-memory stores (where the
    /// per-operation fence guard degrades to a no-op).
    db_path: PathBuf,
    writer: Mutex<Connection>,
    readers: ReadConnections,
    /// Whether the head-canonical TABLES are queryable on this file.
    /// Latched, never cleared: schema evolution is one-way. Read paths that
    /// observe the tables appearing under them (another handle on the same
    /// file committed the DDL) latch it too, so a stale `false` can never
    /// make this handle serve the frozen blob archive as authority.
    ///
    /// This is deliberately NOT "the ledger carries the v2 lockout" — see
    /// [`Self::ledger_is_head_canonical`]. Tables can exist on a file whose
    /// ledger is still v1.
    head_canonical_schema: AtomicBool,
    /// Whether the file's `mobkit-continuity` ledger row already carries the
    /// committed one-way v2 lockout. Latched from a fact that is itself
    /// one-way; a `false` here only means "not known to be stamped", and the
    /// write path re-reads the ledger row inside its own transaction before
    /// acting on it.
    head_canonical_ledger: AtomicBool,
}

impl LocalContinuityStoreInner {
    fn schema_is_head_canonical(&self) -> bool {
        self.head_canonical_schema.load(Ordering::Acquire)
    }

    /// Cached "the one-way lockout is already committed on this file".
    /// Only ever used to SKIP work; never to decide that a bump is owed.
    fn ledger_is_head_canonical(&self) -> bool {
        self.head_canonical_ledger.load(Ordering::Acquire)
    }

    /// Whether the head-canonical tables are queryable on this connection.
    /// Cheap after the first `true` (one relaxed atomic load).
    fn head_tables_available(&self, conn: &Connection) -> Result<bool, ContinuityStoreError> {
        if self.schema_is_head_canonical() {
            return Ok(true);
        }
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' \
                 AND name = 'continuity_session_heads')",
                [],
                |row| row.get(0),
            )
            .map_err(|e| sqlite_err("probe head-canonical tables", e))?;
        if exists {
            self.head_canonical_schema.store(true, Ordering::Release);
        }
        Ok(exists)
    }

    /// Per-operation maintenance-fence guard: the writer and reader pool
    /// hold their connections for the store's lifetime, so the fence
    /// cannot ride the open — every operation takes its own shared guard,
    /// and offline maintenance drains in-flight guards before touching the
    /// file.
    fn operation_fence(&self) -> Result<meerkat_sqlite::OperationGuard, ContinuityStoreError> {
        meerkat_sqlite::OperationGuard::for_database(&self.db_path)
            .map_err(|e| mechanics_err("operation fence", e))
    }

    fn with_reader<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, ContinuityStoreError>,
    ) -> Result<T, ContinuityStoreError> {
        let _fence = self.operation_fence()?;
        match &self.readers {
            ReadConnections::Writer => {
                let connection = self
                    .writer
                    .lock()
                    .map_err(|e| ContinuityStoreError::Io(format!("writer lock: {e}")))?;
                operation(&connection)
            }
            ReadConnections::Pool(pool) => pool.with_connection(operation),
        }
    }

    fn with_writer<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, ContinuityStoreError>,
    ) -> Result<T, ContinuityStoreError> {
        let _fence = self.operation_fence()?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|e| ContinuityStoreError::Io(format!("writer lock: {e}")))?;
        operation(&mut connection)
    }
}

/// SQLite-backed ContinuityStore for the bundled `persistent_state(path)` path.
///
/// Stores ContinuityRecords and SessionSnapshots in a single SQLite database.
/// Enforces compare-and-set on (fencing_token, checkpoint_version). File-backed
/// stores use one serialized writer plus a bounded WAL read pool; async trait
/// operations execute SQLite work on Tokio's blocking workers.
#[derive(Clone)]
pub struct LocalContinuityStore {
    inner: Arc<LocalContinuityStoreInner>,
}

impl LocalContinuityStore {
    /// Open (or create) a local continuity store at the given path.
    ///
    /// # Errors
    ///
    /// Returns `ContinuityStoreError::Io` if the database cannot be opened or
    /// the schema cannot be initialized.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ContinuityStoreError> {
        let path = path.as_ref();
        if path == Path::new(":memory:") {
            return Self::in_memory();
        }

        let mut writer = meerkat_sqlite::open(path, meerkat_sqlite::ConnectionProfile::PRIMARY)
            .map_err(|e| mechanics_err("open writer", e))?;
        // Deliberately NOT `apply_domain_migrations(.., &MOBKIT_CONTINUITY_DOMAIN)`:
        // opening a state directory with a new binary must not commit the
        // one-way ledger v2 bump that locks the previous release out of the
        // file. See `converge_schema_at_open`.
        let head_canonical = converge_schema_at_open(&mut writer)?;

        let mut readers = Vec::with_capacity(READ_POOL_SIZE);
        for index in 0..READ_POOL_SIZE {
            // `ReadOnly` (SQLITE_OPEN_READ_ONLY) is the honest form of the
            // historical `PRAGMA query_only=ON` reader configuration: the
            // connection itself cannot write, and a reader never converts
            // the file's journal mode.
            let reader = meerkat_sqlite::open(path, meerkat_sqlite::ConnectionProfile::ReadOnly)
                .map_err(|e| mechanics_err(&format!("open reader {index}"), e))?;
            readers.push(reader);
        }

        Ok(Self {
            inner: Arc::new(LocalContinuityStoreInner {
                db_path: path.to_path_buf(),
                writer: Mutex::new(writer),
                readers: ReadConnections::Pool(ReadConnectionPool::new(readers)),
                // A stamped ledger implies the tables; the reverse does not
                // hold, so the table latch is seeded from the same fact and
                // widened later by `head_tables_available`.
                head_canonical_schema: AtomicBool::new(head_canonical),
                head_canonical_ledger: AtomicBool::new(head_canonical),
            }),
        })
    }

    /// Open the store and read its fencing-token floor without blocking a
    /// Tokio worker. Async builders and gateways must use this seam because
    /// SQLite open/schema/WAL setup can wait on the filesystem or a database
    /// lock for the configured busy timeout.
    pub async fn open_with_fencing_floor(
        path: impl Into<PathBuf>,
    ) -> Result<(Self, u64), ContinuityStoreError> {
        let path = path.into();
        tokio::task::spawn_blocking(move || {
            let store = Self::open(path)?;
            let fencing_floor = store.max_fencing_token()?;
            Ok((store, fencing_floor))
        })
        .await
        .map_err(|error| {
            ContinuityStoreError::Io(format!(
                "open_with_fencing_floor blocking worker failed: {error}"
            ))
        })?
    }

    /// Commit the head-canonical schema (ledger `mobkit-continuity` v1 -> v2)
    /// into an existing database file as an explicit operator action.
    ///
    /// This is the `storage-migrate --apply` route. It is separate from
    /// [`Self::open`] on purpose: the bump locks binaries older than this
    /// release out of the file (`SchemaFromTheFuture`), so launching a new
    /// gateway must never commit it as a side effect. Runs under whatever
    /// exclusive maintenance fence the caller already holds.
    ///
    /// # Errors
    ///
    /// Returns `ContinuityStoreError::Io` when the file cannot be opened or
    /// the migration cannot be applied.
    pub fn apply_head_canonical_schema_at(
        path: impl AsRef<Path>,
    ) -> Result<bool, ContinuityStoreError> {
        let mut conn =
            meerkat_sqlite::open(path.as_ref(), meerkat_sqlite::ConnectionProfile::PRIMARY)
                .map_err(|e| mechanics_err("open writer for head-canonical migration", e))?;
        let report = apply_head_canonical_schema(&mut conn)
            .map_err(|e| mechanics_err("apply head-canonical schema", e))?;
        Ok(report.migrated())
    }

    /// Offline head-canonical backfill for a legacy v1 corpus.
    ///
    /// The lazy path mints a head row only inside a delta write
    /// ([`ensure_head_canonical_for_write_in_txn`]), so a corpus whose
    /// documents are large enough to make that write expensive can never
    /// leave the whole-document branch under its own steam: the conversion
    /// is gated behind the very write it makes slow. This is the operator
    /// path [`MOBKIT_CONTINUITY_DOMAIN`]'s stamp contract has always named
    /// and never had.
    ///
    /// Contract, in the order it matters:
    /// - **Resumable.** ONE transaction per session. An interrupted run
    ///   leaves every already-converted session converted; re-running
    ///   resumes on the remainder, because the pending set is derived from
    ///   the absence of a head row rather than from a cursor.
    /// - **The blob is retained.** [`migrate_legacy_blob_in_txn`] leaves the
    ///   `session_snapshots` row untouched as a frozen archive.
    /// - **The ledger stamps only on complete conversion.** A partial run
    ///   leaves the file at v1, so rollback to a pre-head-canonical release
    ///   stays available until the whole corpus has crossed. This is why the
    ///   stamp is not folded into the per-session transaction.
    /// - **Dry-run mutates nothing**, including the DDL: a caller inspecting
    ///   a v1 file gets a count and no schema change.
    ///
    /// The caller is responsible for the exclusive maintenance fence; this
    /// function does not take one, exactly as the other `*_at` maintenance
    /// entry points do not.
    ///
    /// # Errors
    ///
    /// Returns [`ContinuityStoreError`] if the file cannot be opened or the
    /// pending set cannot be read. Per-session conversion failures are
    /// collected into the report rather than aborting the run, so one
    /// unconvertible session does not strand the rest.
    pub fn backfill_head_canonical_sessions_at(
        path: impl AsRef<Path>,
        apply: bool,
    ) -> Result<HeadCanonicalBackfillReport, ContinuityStoreError> {
        let mut conn =
            meerkat_sqlite::open(path.as_ref(), meerkat_sqlite::ConnectionProfile::PRIMARY)
                .map_err(|e| mechanics_err("open writer for head-canonical backfill", e))?;

        let pending = pending_head_canonical_sessions(&conn)?;
        let mut report = HeadCanonicalBackfillReport {
            pending_before: pending.len(),
            applied: apply,
            ..HeadCanonicalBackfillReport::default()
        };
        if !apply || pending.is_empty() {
            // Nothing to stamp on an empty pending set either: a corpus with
            // no legacy blobs is not "converted by this run", and claiming
            // otherwise would let an empty run advance the ledger.
            return Ok(report);
        }

        // The DDL half, once, before any conversion. Additive and
        // `IF NOT EXISTS`; still no ledger bump.
        {
            let tx = conn
                .transaction()
                .map_err(|e| sqlite_err("begin head-canonical schema convergence", e))?;
            converge_head_canonical_schema_in_txn(&tx)?;
            tx.commit()
                .map_err(|e| sqlite_err("commit head-canonical schema convergence", e))?;
        }

        for candidate in pending {
            match backfill_one_session(&mut conn, &candidate) {
                Ok(true) => report.converted.push(candidate.session_id.clone()),
                // The blob vanished between census and conversion. Not a
                // failure, but not a conversion either — record it so a
                // complete-conversion claim cannot be made on a corpus that
                // changed under the fence.
                Ok(false) => report.vanished.push(candidate.session_id.clone()),
                Err(error) => report
                    .failures
                    .push((candidate.session_id.clone(), error.to_string())),
            }
        }

        // Stamp ONLY when the corpus is wholly across. Any failure, or any
        // session that disappeared mid-run, leaves the file at v1.
        if report.failures.is_empty()
            && report.vanished.is_empty()
            && report.converted.len() == report.pending_before
        {
            let remaining = pending_head_canonical_sessions(&conn)?;
            if remaining.is_empty() {
                let tx = conn
                    .transaction()
                    .map_err(|e| sqlite_err("begin head-canonical ledger stamp", e))?;
                stamp_head_canonical_ledger_in_txn(&tx)?;
                tx.commit()
                    .map_err(|e| sqlite_err("commit head-canonical ledger stamp", e))?;
                report.ledger_stamped = true;
            } else {
                // Re-census disagreed with the per-session results. Refuse to
                // stamp rather than trust the optimistic count.
                report.failures.push((
                    String::new(),
                    format!(
                        "refusing ledger stamp: {} session(s) still lack a head row after conversion",
                        remaining.len()
                    ),
                ));
            }
        }
        Ok(report)
    }

    /// Open an in-memory store (for testing).
    ///
    /// # Errors
    ///
    /// Returns `ContinuityStoreError::Io` if initialization fails.
    pub fn in_memory() -> Result<Self, ContinuityStoreError> {
        let mut writer =
            Connection::open_in_memory().map_err(|e| sqlite_err("in-memory open", e))?;
        // Same staged convergence as the file path, so the lazy v2 bump is
        // exercised identically in tests and in production.
        let head_canonical = converge_schema_at_open(&mut writer)?;
        Ok(Self {
            inner: Arc::new(LocalContinuityStoreInner {
                db_path: PathBuf::from(":memory:"),
                writer: Mutex::new(writer),
                readers: ReadConnections::Writer,
                head_canonical_schema: AtomicBool::new(head_canonical),
                head_canonical_ledger: AtomicBool::new(head_canonical),
            }),
        })
    }

    /// The highest fencing token ever committed to this store, across BOTH
    /// `continuity_records` and `session_snapshots` (0 if the store is empty).
    ///
    /// The bundled [`LocalLeaseProvider`](super::local_lease::LocalLeaseProvider)
    /// seeds its monotonic counter from this on startup so fencing tokens keep
    /// advancing across process restarts. Without it the provider's in-memory
    /// counter resets to 1 and restore presents a stale token that this store's
    /// compare-and-set rejects — the v0.7.8 "stale fencing token: presented 1,
    /// current N" restart abort.
    ///
    /// # Errors
    ///
    /// Returns `ContinuityStoreError::Io` on a query failure.
    pub fn max_fencing_token(&self) -> Result<u64, ContinuityStoreError> {
        self.inner.with_reader(|connection| {
            // The head-canonical arm only exists once the file carries the
            // channel; on a v1 file the union of the two historical tables
            // IS the whole high-water.
            let sql = if self.inner.head_tables_available(connection)? {
                "SELECT COALESCE(MAX(t), 0) FROM (
                        SELECT MAX(fencing_token) AS t FROM continuity_records
                        UNION ALL
                        SELECT MAX(fencing_token) AS t FROM session_snapshots
                        UNION ALL
                        SELECT MAX(fencing_token) AS t FROM continuity_session_heads
                    )"
            } else {
                "SELECT COALESCE(MAX(t), 0) FROM (
                        SELECT MAX(fencing_token) AS t FROM continuity_records
                        UNION ALL
                        SELECT MAX(fencing_token) AS t FROM session_snapshots
                    )"
            };
            connection
                .query_row(sql, [], |row| row.get::<_, u64>(0))
                .map_err(|e| sqlite_err("max_fencing_token", e))
        })
    }

    async fn run_blocking<T>(
        &self,
        operation_name: &'static str,
        operation: impl FnOnce(Arc<LocalContinuityStoreInner>) -> Result<T, ContinuityStoreError>
        + Send
        + 'static,
    ) -> Result<T, ContinuityStoreError>
    where
        T: Send + 'static,
    {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || operation(inner))
            .await
            .map_err(|e| {
                ContinuityStoreError::Io(format!("{operation_name} blocking worker failed: {e}"))
            })?
    }
}

// ---------------------------------------------------------------------------
// Head-canonical session representation (M4b)
//
// Canonical-representation rule, per session: a `continuity_session_heads`
// row exists => head+rows are the SOLE durable authority for that session,
// and its `session_snapshots` row (if any) is a frozen archive that is never
// read or written again. No head row => the legacy blob behavior is
// byte-for-byte unchanged. This mirrors meerkat-store's own rule for
// `session_heads` vs `sessions` and is what makes the delta channel a
// REPLACEMENT of the byte authority rather than a second one beside it.
//
// Guard semantics are meerkat's published validators verbatim
// (`validate_save_head_transition`, `validate_commit_rewrite_transition`,
// `strand_layout_for_history`, `reconstruct_rewrite_record`), so this store
// mirror can never accept or reject something the meerkat service would not.
// ---------------------------------------------------------------------------

/// Map a continuity-store failure onto the session-store error surface the
/// incremental verbs speak, exactly as the whole-blob adapter save does.
fn session_err(context: &str, error: ContinuityStoreError) -> SessionStoreError {
    SessionStoreError::Internal(format!("{context}: {error}"))
}

fn sqlite_session_err(context: &str, error: rusqlite::Error) -> SessionStoreError {
    session_err(context, sqlite_err(context, error))
}

fn now_millis() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default(),
    )
    .unwrap_or(i64::MAX)
}

/// The stored head row plus its CAS token.
type StoredHead = (SessionHead, String);

fn head_row_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
) -> Result<Option<StoredHead>, SessionStoreError> {
    let row = tx
        .query_row(
            "SELECT head_json, cas_token FROM continuity_session_heads WHERE session_id = ?1",
            rusqlite::params![id.to_string()],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|e| sqlite_session_err("query session head", e))?;
    let Some((head_json, cas_token)) = row else {
        return Ok(None);
    };
    let head: SessionHead =
        serde_json::from_slice(&head_json).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
    Ok(Some((head, cas_token)))
}

/// The `(identity, generation)` a head row belongs to, when one exists.
fn head_owner_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
) -> Result<Option<(String, u64)>, ContinuityStoreError> {
    tx.query_row(
        "SELECT identity, generation FROM continuity_session_heads WHERE session_id = ?1",
        rusqlite::params![id.to_string()],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
    )
    .optional()
    .map_err(|e| sqlite_err("query head owner", e))
}

fn write_head_row_in_txn(
    tx: &Transaction<'_>,
    head: &SessionHead,
    identity: &AgentIdentity,
    generation: ContinuityGeneration,
    version: CheckpointVersion,
    fencing_token: FencingToken,
) -> Result<String, SessionStoreError> {
    let head_json = serde_json::to_vec(head).map_err(SessionStoreError::from)?;
    let cas_token = session_head_cas_token(head)?;
    let message_count = i64::try_from(head.message_count)
        .map_err(|_| SessionStoreError::Corrupted(head.id.clone()))?;
    let rewrite_count = i64::try_from(head.rewrite_count)
        .map_err(|_| SessionStoreError::Corrupted(head.id.clone()))?;
    tx.execute(
        "INSERT INTO continuity_session_heads (
            session_id, identity, generation, checkpoint_version, fencing_token,
            head_revision, message_count, rewrite_count, head_json, cas_token
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(session_id) DO UPDATE SET
            identity = excluded.identity,
            generation = excluded.generation,
            checkpoint_version = excluded.checkpoint_version,
            fencing_token = excluded.fencing_token,
            head_revision = excluded.head_revision,
            message_count = excluded.message_count,
            rewrite_count = excluded.rewrite_count,
            head_json = excluded.head_json,
            cas_token = excluded.cas_token",
        rusqlite::params![
            head.id.to_string(),
            identity.as_str(),
            generation.get(),
            version.get(),
            fencing_token.get(),
            head.head_revision,
            message_count,
            rewrite_count,
            head_json,
            cas_token,
        ],
    )
    .map_err(|e| sqlite_session_err("upsert session head", e))?;
    Ok(cas_token)
}

fn strand_row_count_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    strand: &TranscriptStrandId,
) -> Result<u64, SessionStoreError> {
    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM continuity_strand_messages \
             WHERE session_id = ?1 AND strand = ?2",
            rusqlite::params![id.to_string(), strand.as_str()],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_session_err("count strand rows", e))?;
    u64::try_from(count).map_err(|_| SessionStoreError::Corrupted(id.clone()))
}

fn strand_row_bytes_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    strand: &TranscriptStrandId,
    range: std::ops::Range<u64>,
) -> Result<Vec<Vec<u8>>, SessionStoreError> {
    if range.start >= range.end {
        return Ok(Vec::new());
    }
    let start = i64::try_from(range.start).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
    let end = i64::try_from(range.end).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
    let mut statement = tx
        .prepare_cached(
            "SELECT message_json FROM continuity_strand_messages
             WHERE session_id = ?1 AND strand = ?2 AND seq >= ?3 AND seq < ?4
             ORDER BY seq ASC",
        )
        .map_err(|e| sqlite_session_err("prepare strand read", e))?;
    let rows = statement
        .query_map(
            rusqlite::params![id.to_string(), strand.as_str(), start, end],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(|e| sqlite_session_err("read strand rows", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| sqlite_session_err("read strand rows", e))?;
    if rows.len() as u64 != range.end - range.start {
        return Err(SessionStoreError::Corrupted(id.clone()));
    }
    Ok(rows)
}

fn strand_messages_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    strand: &TranscriptStrandId,
    range: std::ops::Range<u64>,
) -> Result<Vec<Message>, SessionStoreError> {
    strand_row_bytes_in_txn(tx, id, strand, range)?
        .into_iter()
        .map(|bytes| {
            serde_json::from_slice::<Message>(&bytes)
                .map_err(|_| SessionStoreError::Corrupted(id.clone()))
        })
        .collect()
}

/// Append rows under the trait's contiguity / idempotency / immutability
/// contract: `base_seq` may not exceed the current row count; overlapping
/// rows must be byte-identical; shrink is structurally inexpressible.
fn insert_strand_rows_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    strand: &TranscriptStrandId,
    base_seq: u64,
    messages: &[Message],
    identity: &AgentIdentity,
    generation: ContinuityGeneration,
) -> Result<(), SessionStoreError> {
    let existing = strand_row_count_in_txn(tx, id, strand)?;
    if base_seq > existing {
        return Err(SessionStoreError::TranscriptContinuityViolation {
            id: id.clone(),
            previous_revision: format!("strand-rows:{existing}"),
            incoming_revision: format!("append-base-seq:{base_seq}"),
            reason: format!(
                "append at base_seq {base_seq} would leave a gap in strand {strand} with \
                 {existing} rows"
            ),
        });
    }
    let serialized: Vec<Vec<u8>> = messages
        .iter()
        .map(|message| serde_json::to_vec(message).map_err(SessionStoreError::from))
        .collect::<Result<_, _>>()?;
    let overlap_end = existing.min(base_seq + serialized.len() as u64);
    if overlap_end > base_seq {
        let stored = strand_row_bytes_in_txn(tx, id, strand, base_seq..overlap_end)?;
        for (offset, stored_bytes) in stored.iter().enumerate() {
            if stored_bytes != &serialized[offset] {
                return Err(SessionStoreError::TranscriptContinuityViolation {
                    id: id.clone(),
                    previous_revision: format!("strand:{strand} seq:{}", base_seq + offset as u64),
                    incoming_revision: "divergent-bytes".to_string(),
                    reason: format!(
                        "append would overwrite immutable row (strand {strand}, seq {}) with \
                         different bytes",
                        base_seq + offset as u64
                    ),
                });
            }
        }
    }
    let created_at_ms = now_millis();
    for (offset, bytes) in serialized.iter().enumerate() {
        let seq = base_seq + offset as u64;
        if seq < existing {
            continue;
        }
        let seq_i64 = i64::try_from(seq).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
        tx.execute(
            "INSERT INTO continuity_strand_messages
                (session_id, strand, seq, message_json, identity, generation, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                id.to_string(),
                strand.as_str(),
                seq_i64,
                bytes,
                identity.as_str(),
                generation.get(),
                created_at_ms,
            ],
        )
        .map_err(|e| sqlite_session_err("insert strand row", e))?;
    }
    Ok(())
}

struct RewriteRow {
    commit: TranscriptRewriteCommit,
    parent_strand: TranscriptStrandId,
    parent_len: u64,
    strand: TranscriptStrandId,
    strand_len: u64,
}

fn rewrite_row_count_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
) -> Result<u64, SessionStoreError> {
    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM continuity_session_rewrites WHERE session_id = ?1",
            rusqlite::params![id.to_string()],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_session_err("count rewrite rows", e))?;
    u64::try_from(count).map_err(|_| SessionStoreError::Corrupted(id.clone()))
}

/// The adopted rewrite records of a head-canonical session, reconstructed
/// from the persisted rows in the caller's transaction. One place, so every
/// caller reconstructs history identically.
fn rewrite_records_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    max_idx_exclusive: u64,
) -> Result<Vec<TranscriptRewriteRecord>, SessionStoreError> {
    rewrite_rows_in_txn(tx, id, max_idx_exclusive)?
        .into_iter()
        .map(|row| {
            let parent_messages =
                strand_messages_in_txn(tx, id, &row.parent_strand, 0..row.parent_len)?;
            let revision_messages = strand_messages_in_txn(tx, id, &row.strand, 0..row.strand_len)?;
            reconstruct_rewrite_record(id, row.commit, parent_messages, revision_messages)
        })
        .collect()
}

fn rewrite_rows_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    max_idx_exclusive: u64,
) -> Result<Vec<RewriteRow>, SessionStoreError> {
    let limit =
        i64::try_from(max_idx_exclusive).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
    let mut statement = tx
        .prepare_cached(
            "SELECT commit_json, parent_strand, parent_len, strand, strand_len
             FROM continuity_session_rewrites
             WHERE session_id = ?1 AND rewrite_idx < ?2
             ORDER BY rewrite_idx ASC",
        )
        .map_err(|e| sqlite_session_err("prepare rewrite read", e))?;
    let rows = statement
        .query_map(rusqlite::params![id.to_string(), limit], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| sqlite_session_err("read rewrite rows", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| sqlite_session_err("read rewrite rows", e))?;
    rows.into_iter()
        .map(
            |(commit_json, parent_strand, parent_len, strand, strand_len)| {
                let commit: TranscriptRewriteCommit = serde_json::from_slice(&commit_json)
                    .map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
                Ok(RewriteRow {
                    commit,
                    parent_strand: TranscriptStrandId::from_persisted(parent_strand),
                    parent_len: u64::try_from(parent_len)
                        .map_err(|_| SessionStoreError::Corrupted(id.clone()))?,
                    strand: TranscriptStrandId::from_persisted(strand),
                    strand_len: u64::try_from(strand_len)
                        .map_err(|_| SessionStoreError::Corrupted(id.clone()))?,
                })
            },
        )
        .collect()
}

fn insert_rewrite_row_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    rewrite_idx: u64,
    row: &RewriteRow,
    identity: &AgentIdentity,
    generation: ContinuityGeneration,
) -> Result<(), SessionStoreError> {
    let commit_json = serde_json::to_vec(&row.commit).map_err(SessionStoreError::from)?;
    let idx = i64::try_from(rewrite_idx).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
    let parent_len =
        i64::try_from(row.parent_len).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
    let strand_len =
        i64::try_from(row.strand_len).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
    tx.execute(
        "INSERT OR REPLACE INTO continuity_session_rewrites
            (session_id, rewrite_idx, parent_strand, parent_len, strand, strand_len,
             commit_json, identity, generation, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            id.to_string(),
            idx,
            row.parent_strand.as_str(),
            parent_len,
            row.strand.as_str(),
            strand_len,
            commit_json,
            identity.as_str(),
            generation.get(),
            now_millis(),
        ],
    )
    .map_err(|e| sqlite_session_err("insert rewrite row", e))?;
    Ok(())
}

fn blob_session_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
) -> Result<Option<Session>, SessionStoreError> {
    let data = tx
        .query_row(
            "SELECT data FROM session_snapshots WHERE session_id = ?1",
            rusqlite::params![id.to_string()],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(|e| sqlite_session_err("read archived snapshot", e))?;
    let Some(data) = data else {
        return Ok(None);
    };
    match serde_json::from_slice::<Session>(&data) {
        Ok(session) => Ok(Some(session)),
        Err(decode_error) => import_released_blob_in_txn(tx, id, &data, &decode_error).map(Some),
    }
}

/// Same-transaction one-time import of a released 0.8.10 blob row.
///
/// Per the banked 0.8.11 import contract: the public core importer is the
/// sole boundary allowed to interpret released evidence, the non-Clone
/// receipt is consumed by the adoption, the source blob SHA is re-proved
/// against the exact bytes read, nothing mints the retired vocabulary, and
/// every proof failure fails closed. The durable adoption rewrites the
/// payload bytes INSIDE the caller's transaction; the row's cursor custody
/// columns stay exactly as observed. A read-only transaction (the read-pool
/// fallbacks) serves the imported document without adoption - the first
/// write-path decode (head-canonical conversion, delta writes) adopts.
fn import_released_blob_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    source: &[u8],
    decode_error: &serde_json::Error,
) -> Result<Session, SessionStoreError> {
    use sha2::Digest as _;

    let imported = meerkat_core::import_released_0810_session(source).map_err(|import| {
        SessionStoreError::Serialization(format!(
            "continuity blob {id} decodes neither as a current document ({decode_error}) nor as              a released 0.8.10 envelope ({import})"
        ))
    })?;
    let (session, receipt) = imported.into_parts();
    let observed_sha256: [u8; 32] = sha2::Sha256::digest(source).into();
    if receipt.source_document_sha256() != &observed_sha256 {
        return Err(SessionStoreError::Serialization(format!(
            "continuity blob {id} changed during exact released-0.8.10 import"
        )));
    }
    if receipt.session_id() != id {
        return Err(SessionStoreError::Serialization(format!(
            "continuity blob key {id} contains released session {}",
            receipt.session_id()
        )));
    }
    let current = session
        .to_persisted_bytes()
        .map_err(|e| SessionStoreError::Serialization(e.to_string()))?;
    // The receipt is consumed by this durable adoption inside the caller's
    // transaction.
    drop(receipt);
    let changed = match tx.execute(
        "UPDATE session_snapshots SET data = ?2 WHERE session_id = ?1",
        rusqlite::params![id.to_string(), current],
    ) {
        Ok(changed) => changed,
        Err(rusqlite::Error::SqliteFailure(failure, _))
            if failure.code == rusqlite::ErrorCode::ReadOnly =>
        {
            // Read-pool fallback: serve the imported document; the first
            // write-path decode (head-canonical conversion, delta writes)
            // performs the durable adoption.
            tracing::info!(
                session_id = %id,
                "released 0.8.10 blob imported on a read-only connection; durable adoption \
                 follows the first write-path decode"
            );
            return Ok(session);
        }
        Err(e) => return Err(sqlite_session_err("adopt imported released snapshot", e)),
    };
    if changed != 1 {
        return Err(SessionStoreError::Corrupted(id.clone()));
    }
    tracing::info!(
        session_id = %id,
        source_bytes = source.len(),
        current_bytes = current.len(),
        "released 0.8.10 blob imported and durably adopted in-transaction"
    );
    Ok(session)
}

/// Full-vector projection of the 0.8.11 splice-based [`StrandLayout`].
///
/// The continuity schema stores every strand as its complete message vector
/// (there is no strand-link table), so the append-only suffix/splice layout
/// is materialized back into full per-strand rows. The walk mirrors the
/// lineage validation in meerkat's own blob conversion: parent-transition
/// splice, parent-suffix extension, successor replacement splice, tail.
struct MaterializedBlobLayout {
    /// Full rows per strand, in first-appearance order. A strand id extended
    /// across rewrites (exact-append parents) holds its final, longest vector.
    strands: Vec<(TranscriptStrandId, Vec<Message>)>,
    rewrites: Vec<RewriteRow>,
    head_strand: TranscriptStrandId,
}

impl MaterializedBlobLayout {
    fn from_layout(
        id: &meerkat_core::types::SessionId,
        layout: &StrandLayout,
    ) -> Result<Self, SessionStoreError> {
        fn decode_rows(
            id: &meerkat_core::types::SessionId,
            rows: &[Vec<u8>],
        ) -> Result<Vec<Message>, SessionStoreError> {
            rows.iter()
                .map(|bytes| {
                    serde_json::from_slice::<Message>(bytes).map_err(|error| {
                        SessionStoreError::InvalidTranscriptRewrite {
                            id: id.clone(),
                            reason: format!("layout strand row does not decode: {error}"),
                        }
                    })
                })
                .collect()
        }
        fn splice_rows(
            id: &meerkat_core::types::SessionId,
            source: &[Message],
            start: u64,
            end: u64,
            replacement: &[Message],
        ) -> Result<Vec<Message>, SessionStoreError> {
            let start =
                usize::try_from(start).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
            let end = usize::try_from(end).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
            if start > end || end > source.len() {
                return Err(SessionStoreError::Corrupted(id.clone()));
            }
            let mut rows = Vec::with_capacity(source.len() - (end - start) + replacement.len());
            rows.extend_from_slice(&source[..start]);
            rows.extend_from_slice(replacement);
            rows.extend_from_slice(&source[end..]);
            Ok(rows)
        }
        fn upsert(
            strands: &mut Vec<(TranscriptStrandId, Vec<Message>)>,
            strand: &TranscriptStrandId,
            rows: Vec<Message>,
        ) {
            if let Some(entry) = strands.iter_mut().find(|(sid, _)| sid == strand) {
                entry.1 = rows;
            } else {
                strands.push((strand.clone(), rows));
            }
        }

        let mut strands: Vec<(TranscriptStrandId, Vec<Message>)> = Vec::new();
        let mut current = decode_rows(id, &layout.serialized_anchor)?;
        let mut current_strand = layout.anchor_strand.clone();
        upsert(&mut strands, &current_strand, current.clone());
        let mut rewrites = Vec::with_capacity(layout.rewrites.len());
        for rewrite in &layout.rewrites {
            match &rewrite.parent_transition {
                meerkat_core::session_store::PreparedHeadCanonicalParentTransition::ExactAppend => {
                    if rewrite.parent_strand != current_strand {
                        return Err(SessionStoreError::Corrupted(id.clone()));
                    }
                }
                meerkat_core::session_store::PreparedHeadCanonicalParentTransition::ExactSplice(
                    parent_splice,
                ) => {
                    let link = parent_splice.link_splice();
                    let replacement = decode_rows(id, parent_splice.serialized_replacement())?;
                    current = splice_rows(
                        id,
                        &current,
                        link.splice_start,
                        link.splice_end,
                        &replacement,
                    )?;
                    current_strand = rewrite.parent_strand.clone();
                }
            }
            if u64::try_from(current.len()).map_err(|_| SessionStoreError::Corrupted(id.clone()))?
                != rewrite.parent_base_seq
            {
                return Err(SessionStoreError::Corrupted(id.clone()));
            }
            current.extend(decode_rows(id, &rewrite.serialized_parent_suffix)?);
            let parent_len = u64::try_from(current.len())
                .map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
            upsert(&mut strands, &current_strand, current.clone());
            let replacement = decode_rows(id, &rewrite.serialized_replacement)?;
            current = splice_rows(
                id,
                &current,
                rewrite.link_splice.splice_start,
                rewrite.link_splice.successor_end,
                &replacement,
            )?;
            if u64::try_from(current.len()).map_err(|_| SessionStoreError::Corrupted(id.clone()))?
                != rewrite.link_splice.strand_len
            {
                return Err(SessionStoreError::Corrupted(id.clone()));
            }
            current_strand = rewrite.strand.clone();
            upsert(&mut strands, &current_strand, current.clone());
            rewrites.push(RewriteRow {
                commit: rewrite.commit.clone(),
                parent_strand: rewrite.parent_strand.clone(),
                parent_len,
                strand: rewrite.strand.clone(),
                strand_len: rewrite.link_splice.strand_len,
            });
        }
        if layout.head_strand != current_strand {
            return Err(SessionStoreError::Corrupted(id.clone()));
        }
        current.extend(decode_rows(id, &layout.serialized_tail)?);
        if u64::try_from(current.len()).map_err(|_| SessionStoreError::Corrupted(id.clone()))?
            != layout.head_len
        {
            return Err(SessionStoreError::Corrupted(id.clone()));
        }
        upsert(&mut strands, &current_strand, current);
        Ok(Self {
            strands,
            rewrites,
            head_strand: layout.head_strand.clone(),
        })
    }
}

fn layout_for_blob_session(
    session: &Session,
) -> Result<(MaterializedBlobLayout, SessionHead), SessionStoreError> {
    let history = session
        .validated_transcript_history_state()
        .map_err(|err| SessionStoreError::InvalidTranscriptRewrite {
            id: session.id().clone(),
            reason: format!("stored transcript history state is malformed: {err}"),
        })?;
    let layout = strand_layout_for_history(session, history.as_ref())?;
    let materialized = MaterializedBlobLayout::from_layout(session.id(), &layout)?;
    let head = SessionHead::from_session(
        session,
        materialized.head_strand.clone(),
        materialized.rewrites.len() as u64,
    )?;
    Ok((materialized, head))
}

/// One-time per-session migration inside the caller's transaction: lay the
/// legacy blob out as strands + rewrite rows + a head row. The blob row is
/// left untouched as a frozen archive and is never read again once the head
/// row exists. Reads never migrate.
fn migrate_legacy_blob_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    identity: &AgentIdentity,
    generation: ContinuityGeneration,
    version: CheckpointVersion,
    fencing_token: FencingToken,
) -> Result<Option<StoredHead>, SessionStoreError> {
    let Some(session) = blob_session_in_txn(tx, id)? else {
        return Ok(None);
    };
    // Clear any ORPHAN rows first — strand/rewrite rows for this session
    // that no head row adopts.
    //
    // They exist because an append that creates no head state commits its
    // rows and leaves the ledger at v1 (the rollback-safety rule in
    // `delta_write`), so an interrupted creation window, or a rollback to a
    // previous release followed by a re-upgrade, can leave rows behind that
    // disagree with the blob this migration is about to lay out. Without
    // this, `insert_strand_rows_in_txn` would refuse the divergence as an
    // immutability violation and the session would be permanently
    // unwritable.
    //
    // Safe unconditionally: this function is reached only with NO head row
    // for `id`, and every read path gates on the head row, so nothing can
    // observe these rows. The blob is the authority being migrated.
    delete_orphan_head_canonical_rows_in_txn(tx, id)?;
    // This one-time conversion is the ONE phase of a boot guaranteed to be
    // slow (minutes of CPU on a large legacy document) and it previously
    // emitted nothing — a supervised deploy read the silence as a stalled
    // candidate and aborted its activation. Say what is happening, at entry
    // and completion, so a long migration is visibly a long migration.
    let started = std::time::Instant::now();
    tracing::info!(
        session_id = %id,
        identity = %identity,
        messages = session.messages().len(),
        "head-canonical conversion of a legacy blob starting"
    );
    let (layout, head) = layout_for_blob_session(&session)?;
    for (strand, rows) in &layout.strands {
        insert_strand_rows_in_txn(tx, id, strand, 0, rows, identity, generation)?;
    }
    for (idx, rewrite) in layout.rewrites.iter().enumerate() {
        insert_rewrite_row_in_txn(tx, id, idx as u64, rewrite, identity, generation)?;
    }
    let token = write_head_row_in_txn(tx, &head, identity, generation, version, fencing_token)?;
    tracing::info!(
        session_id = %id,
        identity = %identity,
        strands = layout.strands.len(),
        rewrite_rows = layout.rewrites.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "head-canonical conversion of a legacy blob complete"
    );
    Ok(Some((head, token)))
}

/// Head row if present, otherwise migrate a legacy blob in this transaction.
/// The FIRST delta write migrates; reads synthesize without writing.
fn ensure_head_canonical_for_write_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    identity: &AgentIdentity,
    generation: ContinuityGeneration,
    version: CheckpointVersion,
    fencing_token: FencingToken,
) -> Result<Option<StoredHead>, SessionStoreError> {
    if let Some(existing) = head_row_in_txn(tx, id)? {
        return Ok(Some(existing));
    }
    migrate_legacy_blob_in_txn(tx, id, identity, generation, version, fencing_token)
}

fn materialize_slim_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    head: &SessionHead,
) -> Result<Session, SessionStoreError> {
    let messages = strand_messages_in_txn(tx, id, &head.strand, 0..head.message_count)?;
    match head.clone().into_session(messages) {
        Ok(session) => Ok(session),
        // Released 0.8.10 HEAD ROW (session envelope v2): interpretable only
        // through the explicit one-time importer — the head-row lane of the
        // same contract `import_released_blob_in_txn` implements for whole
        // blobs. Every 0.8.10-written head refuses current materialization
        // (`Session::from_head_parts` fails typed on the envelope version),
        // so without this lane an entire released head-canonical fleet is
        // unreadable at resume (HomeCore binding, 17/17 identities:
        // "failed to restore session from head row: ... expected current 3,
        // got 2").
        Err(restore_error)
            if head.version == super::contracts::RELEASED_0810_SESSION_ENVELOPE_VERSION =>
        {
            import_released_head_in_txn(
                tx,
                id,
                head,
                &format!("failed current materialization ({restore_error})"),
            )
        }
        Err(err) => Err(err),
    }
}

/// Serialized-verbatim released envelope, reassembled from the exact durable
/// parts a released 0.8.10 head row commits to. `messages` embeds the exact
/// strand row bytes (`RawValue`), never a re-serialization, so the importer
/// interprets precisely what the released writer persisted.
#[derive(serde::Serialize)]
struct ReleasedHeadEnvelope0810<'a> {
    version: u32,
    id: &'a meerkat_core::types::SessionId,
    messages: &'a [Box<serde_json::value::RawValue>],
    created_at: std::time::SystemTime,
    updated_at: std::time::SystemTime,
    metadata: &'a serde_json::Map<String, serde_json::Value>,
    usage: &'a meerkat_core::Usage,
}

/// One-time released-0.8.10 import for a HEAD-CANONICAL continuity document.
///
/// Same banked import contract as [`import_released_blob_in_txn`], adapted to
/// the head representation: the public core importer is the sole boundary
/// allowed to interpret released evidence, and every proof failure fails
/// closed with the original refusal surfaced (never a healed reading).
///
/// Proof chain, in order:
/// 1. The exact durable strand rows must be the rows the released head
///    committed to: `released_0810_transcript_serialized_rows_digest` (the
///    byte-faithful recomputation of the released transcript digest) must
///    equal `head.head_revision`.
/// 2. The released envelope is reassembled from those exact bytes plus the
///    head's own envelope facts (a released head inlines its full metadata
///    map — `metadata_identity` is a 0.8.11 concept), and handed to
///    `import_released_0810_session`, which re-validates the envelope
///    version and every released metadata shape.
/// 3. The receipt's source digest is re-proved against the exact bytes
///    interpreted, and its session id against the row key.
///
/// This runs on the read pool, so like the blob lane's read-only fallback it
/// serves the imported document WITHOUT durable adoption: the first
/// write-path decode observes the released head, fails its prefix-digest
/// probe against the current algorithm, and rebases the strand under a
/// current-format head — the durable adoption every later read observes.
fn import_released_head_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
    head: &SessionHead,
    refusal_context: &str,
) -> Result<Session, SessionStoreError> {
    use sha2::Digest as _;

    let raw_rows = strand_row_bytes_in_txn(tx, id, &head.strand, 0..head.message_count)?;
    let released_digest = meerkat_core::released_0810_transcript_serialized_rows_digest(&raw_rows)
        .map_err(|digest_error| {
            SessionStoreError::Serialization(format!(
                "continuity head row {id} {refusal_context} and \
                 its strand rows do not admit the released 0.8.10 digest ({digest_error})"
            ))
        })?;
    if released_digest != head.head_revision {
        return Err(SessionStoreError::Serialization(format!(
            "continuity head row {id} {refusal_context} and its \
             strand rows do not match the released head commitment (released digest \
             {released_digest}, head revision {})",
            head.head_revision
        )));
    }
    let messages = raw_rows
        .into_iter()
        .map(|bytes| {
            String::from_utf8(bytes)
                .map_err(|_| SessionStoreError::Corrupted(id.clone()))
                .and_then(|row| {
                    serde_json::value::RawValue::from_string(row)
                        .map_err(|_| SessionStoreError::Corrupted(id.clone()))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let envelope = serde_json::to_vec(&ReleasedHeadEnvelope0810 {
        version: head.version,
        id,
        messages: &messages,
        created_at: head.created_at,
        updated_at: head.updated_at,
        metadata: &head.metadata,
        usage: &head.usage,
    })
    .map_err(|e| SessionStoreError::Serialization(e.to_string()))?;
    let imported = meerkat_core::import_released_0810_session(&envelope).map_err(|import| {
        SessionStoreError::Serialization(format!(
            "continuity head row {id} {refusal_context} and does not interpret as a \
             released 0.8.10 head-canonical document either ({import})"
        ))
    })?;
    let (session, receipt) = imported.into_parts();
    let observed_sha256: [u8; 32] = sha2::Sha256::digest(&envelope).into();
    if receipt.source_document_sha256() != &observed_sha256 {
        return Err(SessionStoreError::Serialization(format!(
            "continuity head row {id} changed during exact released-0.8.10 import"
        )));
    }
    if receipt.session_id() != id {
        return Err(SessionStoreError::Serialization(format!(
            "continuity head key {id} contains released session {}",
            receipt.session_id()
        )));
    }
    drop(receipt);
    tracing::info!(
        session_id = %id,
        released_rows = head.message_count,
        "released 0.8.10 head-canonical document imported on load; durable adoption follows \
         the first write-path decode"
    );
    Ok(session)
}

/// One-time durable adoption of a released 0.8.10 head-canonical document,
/// inside the caller's WRITE transaction (see
/// `ContinuityIncrementalSessions::adopt_released_head_document`).
///
/// A released head with retained rewrites cannot authorize a current
/// mutation - its rewrite-generation authority predates the compact
/// graph/rewrite-prefix carriers, so `session_head_cas_token` refuses it
/// typed and every ordinary write arm is unreachable. Authorization here is
/// the import proof: the stored released document is re-proved through
/// [`import_released_head_in_txn`] (byte proof against the released head
/// commitment + the sanctioned importer + receipt re-proof), `incoming` must
/// be a legal successor of that imported reading (equal or append-extension;
/// the boundary guard refuses genuine divergence typed), and only then is the
/// released representation replaced wholesale with the current-format layout
/// of `incoming` - the same strand/rewrite/head writer the legacy-blob
/// migration uses, so rewrite-carrying documents lay out identically to a
/// converted blob.
fn adopt_released_head_in_txn(
    tx: &Transaction<'_>,
    incoming: &Session,
    identity: &AgentIdentity,
    generation: ContinuityGeneration,
    version: CheckpointVersion,
    fencing_token: FencingToken,
) -> Result<(), SessionStoreError> {
    let id = incoming.id();
    let Some((stored, _token)) = head_row_in_txn(tx, id)? else {
        return Err(SessionStoreError::Internal(format!(
            "released head adoption for session {id} found no durable head row; the adoption \
             lane is only reachable from a stored released head"
        )));
    };
    if stored.version != super::contracts::RELEASED_0810_SESSION_ENVELOPE_VERSION {
        return Err(SessionStoreError::Internal(format!(
            "released head adoption for session {id} found a current head (envelope version \
             {}); refusing to re-adopt a document the ordinary write arms already own",
            stored.version
        )));
    }
    let imported =
        import_released_head_in_txn(tx, id, &stored, "is being adopted on the write path")?;
    meerkat_core::session_store::append_only_save_guard(incoming, Some(&imported))?;
    // The released rows are being REPLACED wholesale inside this transaction;
    // nothing can observe the intermediate state, and the imported reading
    // above is the receipt-proved successor source. The head row itself is
    // upserted by `write_head_row_in_txn`.
    delete_orphan_head_canonical_rows_in_txn(tx, id)?;
    let started = std::time::Instant::now();
    let (layout, head) = layout_for_blob_session(incoming)?;
    for (strand, rows) in &layout.strands {
        insert_strand_rows_in_txn(tx, id, strand, 0, rows, identity, generation)?;
    }
    for (idx, rewrite) in layout.rewrites.iter().enumerate() {
        insert_rewrite_row_in_txn(tx, id, idx as u64, rewrite, identity, generation)?;
    }
    write_head_row_in_txn(tx, &head, identity, generation, version, fencing_token)?;
    tracing::info!(
        session_id = %id,
        released_rows = stored.message_count,
        released_rewrite_count = stored.rewrite_count,
        adopted_rows = head.message_count,
        adopted_rewrite_count = head.rewrite_count,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "released 0.8.10 head-canonical document durably adopted on the write path"
    );
    Ok(())
}

/// Head-canonical compat write for a WHOLE-document verb: delta-append when
/// the incoming transcript extends the persisted head strand, otherwise a
/// `rebase:` strand switch. The archived blob row is never touched.
fn write_head_canonical_session_in_txn(
    tx: &Transaction<'_>,
    session: &Session,
    head: &SessionHead,
    identity: &AgentIdentity,
    generation: ContinuityGeneration,
    version: CheckpointVersion,
    fencing_token: FencingToken,
) -> Result<(), SessionStoreError> {
    let id = session.id();
    let live = session.messages();
    let prev_count = usize::try_from(head.message_count)
        .map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
    let plain_append = live.len() >= prev_count
        && meerkat_core::transcript_messages_digest(&live[..prev_count])
            .map_err(SessionStoreError::from)?
            == head.head_revision;
    let new_head = if plain_append {
        if live.len() > prev_count {
            insert_strand_rows_in_txn(
                tx,
                id,
                &head.strand,
                head.message_count,
                &live[prev_count..],
                identity,
                generation,
            )?;
        }
        // The successor head must commit to the EXACT durable row bytes.
        // Rows 0..prev_count keep the serialization they were written with,
        // which need not equal re-encoding the same typed Messages today, so
        // the stored commitment is EXTENDED by only the appended rows' bytes
        // - mirrors meerkat-core
        // `SessionHead::from_session_with_proved_inline_storage_authority`, the
        // published seam for retained boundaries whose exact row bytes may
        // use an older representation. Re-minting via `from_session` breaks
        // `SessionHead::into_session`'s byte-exact prefix verification on
        // the next cold materialization.
        match head.message_row_prefix.clone() {
            Some(prefix) => {
                let appended_serialized = live[prev_count..]
                    .iter()
                    .map(|message| serde_json::to_vec(message).map_err(SessionStoreError::from))
                    .collect::<Result<Vec<_>, _>>()?;
                let proved = prefix.extend_serialized_rows(&appended_serialized)?;
                SessionHead::from_session_with_proved_inline_storage_authority(
                    session,
                    head.strand.clone(),
                    head.rewrite_prefix.clone(),
                    proved,
                )?
            }
            None => {
                // A pre-0.8.11 head whose row identity was never proved
                // stays unproved rather than inventing a commitment the
                // stored rows may not satisfy.
                let mut unproved =
                    SessionHead::from_session(session, head.strand.clone(), head.rewrite_count)?;
                unproved.message_row_prefix = None;
                unproved.row_lineage_anchor = None;
                unproved
            }
        }
    } else {
        let live_digest =
            meerkat_core::transcript_messages_digest(live).map_err(SessionStoreError::from)?;
        let rebased = TranscriptStrandId::rebase(&live_digest);
        insert_strand_rows_in_txn(tx, id, &rebased, 0, live, identity, generation)?;
        // A fresh strand: every row was just written from these exact
        // instances, so the minted commitment matches the durable bytes.
        SessionHead::from_session(session, rebased, head.rewrite_count)?
    };
    write_head_row_in_txn(tx, &new_head, identity, generation, version, fencing_token)?;
    Ok(())
}

/// The continuity write discipline, enforced inside the same transaction as
/// the rows it authorizes. Byte-for-byte the checks
/// `save_session_snapshot_owned` runs for a whole-blob save, so a delta
/// mutation can never be accepted where a whole-document save would be
/// refused (or the reverse). Returns whether a continuity record exists for
/// the identity — the caller advances it in the same transaction.
fn enforce_continuity_cursor_in_txn(
    tx: &Transaction<'_>,
    identity: &AgentIdentity,
    session_id: &meerkat_core::types::SessionId,
    generation: ContinuityGeneration,
    version: CheckpointVersion,
    fencing_token: FencingToken,
) -> Result<bool, ContinuityStoreError> {
    let existing = tx
        .query_row(
            "SELECT session_id, generation, fencing_token, checkpoint_version
             FROM continuity_records WHERE identity = ?1",
            rusqlite::params![identity.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| sqlite_err("query continuity record", e))?;

    let record_was_present = existing.is_some();
    if let Some((current_session_id, current_generation, current_token, current_version)) = existing
    {
        if current_session_id != session_id.to_string() || current_generation != generation.get() {
            return Err(ContinuityStoreError::NotFound {
                identity: identity.clone(),
            });
        }
        if fencing_token.get() < current_token {
            return Err(ContinuityStoreError::StaleFencingToken {
                identity: identity.clone(),
                presented: fencing_token,
                current: FencingToken::new(current_token),
            });
        }
        if version.get() <= current_version {
            return Err(ContinuityStoreError::StaleCheckpointVersion {
                identity: identity.clone(),
                presented: version,
                current: CheckpointVersion::new(current_version),
            });
        }
    }
    Ok(record_was_present)
}

fn advance_continuity_record_in_txn(
    tx: &Transaction<'_>,
    identity: &AgentIdentity,
    session_id: &meerkat_core::types::SessionId,
    generation: ContinuityGeneration,
    version: CheckpointVersion,
    fencing_token: FencingToken,
    record_was_present: bool,
) -> Result<(), ContinuityStoreError> {
    tx.execute(
        "UPDATE continuity_records
         SET checkpoint_version = ?1, fencing_token = ?2
         WHERE identity = ?3 AND session_id = ?4 AND generation = ?5",
        rusqlite::params![
            version.get(),
            fencing_token.get(),
            identity.as_str(),
            session_id.to_string(),
            generation.get(),
        ],
    )
    .map_err(|e| sqlite_err("advance continuity record", e))?;
    if record_was_present && tx.changes() == 0 {
        return Err(ContinuityStoreError::NotFound {
            identity: identity.clone(),
        });
    }
    Ok(())
}

/// The blob row's ownership check: a session's `session_snapshots` row may
/// never be written — nor superseded by head rows — under a different
/// identity or a different generation than the one that owns it.
///
/// ONE function, called by BOTH write paths (`save_session_snapshot_owned`
/// and every `delta_write`), because two write paths onto the same durable
/// session must not have different accept/reject boundaries. A generation
/// bump always mints a fresh session id, so a mismatch here is genuine
/// cross-owner corruption, never an ordinary lifecycle transition.
fn ensure_snapshot_owner_in_txn(
    tx: &Transaction<'_>,
    session_id: &meerkat_core::types::SessionId,
    identity: &AgentIdentity,
    generation: ContinuityGeneration,
) -> Result<(), ContinuityStoreError> {
    let existing_snapshot_owner = tx
        .query_row(
            "SELECT identity, generation FROM session_snapshots WHERE session_id = ?1",
            rusqlite::params![session_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
        )
        .optional()
        .map_err(|e| sqlite_err("query snapshot owner", e))?;
    if let Some((snapshot_identity, snapshot_generation)) = existing_snapshot_owner
        && (snapshot_identity != identity.as_str() || snapshot_generation != generation.get())
    {
        return Err(ContinuityStoreError::Corruption(format!(
            "session snapshot {session_id} is owned by {snapshot_identity}/generation \
             {snapshot_generation}, not {identity}/generation {generation}"
        )));
    }
    Ok(())
}

/// The head row's ownership check, mirroring the whole-blob path's
/// snapshot-owner corruption check: a session's durable representation may
/// never be written by a different identity or a different generation.
fn ensure_head_owner_in_txn(
    tx: &Transaction<'_>,
    session_id: &meerkat_core::types::SessionId,
    identity: &AgentIdentity,
    generation: ContinuityGeneration,
) -> Result<(), ContinuityStoreError> {
    if let Some((head_identity, head_generation)) = head_owner_in_txn(tx, session_id)?
        && (head_identity != identity.as_str() || head_generation != generation.get())
    {
        return Err(ContinuityStoreError::Corruption(format!(
            "session head {session_id} is owned by {head_identity}/generation \
             {head_generation}, not {identity}/generation {generation}"
        )));
    }
    Ok(())
}

/// Drop the strand and rewrite rows of a session that has NO head row.
///
/// Callers must have established that (`migrate_legacy_blob_in_txn` is the
/// only one, and it runs only when `head_row_in_txn` returned `None`). The
/// head table is deliberately untouched: this clears orphans, it is not a
/// session delete.
fn delete_orphan_head_canonical_rows_in_txn(
    tx: &Transaction<'_>,
    id: &meerkat_core::types::SessionId,
) -> Result<(), SessionStoreError> {
    for table in ["continuity_strand_messages", "continuity_session_rewrites"] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE session_id = ?1"),
            rusqlite::params![id.to_string()],
        )
        .map_err(|e| sqlite_session_err("delete orphan head-canonical rows", e))?;
    }
    Ok(())
}

fn delete_head_canonical_rows_in_txn(
    tx: &Transaction<'_>,
    predicate: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<(), ContinuityStoreError> {
    for table in [
        "continuity_session_heads",
        "continuity_strand_messages",
        "continuity_session_rewrites",
    ] {
        tx.execute(&format!("DELETE FROM {table} WHERE {predicate}"), params)
            .map_err(|e| sqlite_err("delete head-canonical rows", e))?;
    }
    Ok(())
}

#[async_trait]
impl ContinuityStore for LocalContinuityStore {
    async fn resolve_many(
        &self,
        identities: &[AgentIdentity],
    ) -> Result<BTreeMap<AgentIdentity, ContinuityResolveState>, ContinuityStoreError> {
        let identities = identities.to_vec();
        self.run_blocking("resolve_many", move |inner| {
            inner.with_reader(|connection| {
                let mut map = BTreeMap::new();
                for id in &identities {
                    let mut stmt = connection
                        .prepare_cached(
                            "SELECT agent_runtime_id, session_id, generation, checkpoint_version
                             FROM continuity_records WHERE identity = ?1",
                        )
                        .map_err(|e| sqlite_err("prepare", e))?;
                    let row = stmt
                        .query_row(rusqlite::params![id.as_str()], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, u64>(2)?,
                                row.get::<_, u64>(3)?,
                            ))
                        })
                        .optional()
                        .map_err(|e| sqlite_err("query", e))?;
                    match row {
                        Some((runtime_id, session_id_str, generation, cpv)) => {
                            let record = ContinuityRecord {
                                identity: id.clone(),
                                agent_runtime_id: AgentRuntimeId::parse(&runtime_id).map_err(
                                    |e| {
                                        ContinuityStoreError::Corruption(format!(
                                            "invalid runtime_id in store: {e}"
                                        ))
                                    },
                                )?,
                                session_id: meerkat_core::types::SessionId::parse(&session_id_str)
                                    .map_err(|e| {
                                        ContinuityStoreError::Corruption(format!(
                                            "invalid session_id in store: {e}"
                                        ))
                                    })?,
                                generation: ContinuityGeneration::new(generation),
                                checkpoint_version: CheckpointVersion::new(cpv),
                            };
                            map.insert(id.clone(), ContinuityResolveState::Ready { record });
                        }
                        None => {
                            map.insert(id.clone(), ContinuityResolveState::Uninitialized);
                        }
                    }
                }
                Ok(map)
            })
        })
        .await
    }

    async fn resolve_record_by_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<(ContinuityRecord, FencingToken, CheckpointVersion)>, ContinuityStoreError>
    {
        let session_id = session_id.clone();
        self.run_blocking("resolve_record_by_session", move |inner| {
            inner.with_reader(|connection| {
                let mut stmt = connection
                    .prepare_cached(
                        "SELECT identity, agent_runtime_id, generation, checkpoint_version, \
                         fencing_token FROM continuity_records WHERE session_id = ?1",
                    )
                    .map_err(|e| sqlite_err("prepare", e))?;
                let row = stmt
                    .query_row(rusqlite::params![session_id.to_string()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, u64>(2)?,
                            row.get::<_, u64>(3)?,
                            row.get::<_, u64>(4)?,
                        ))
                    })
                    .optional()
                    .map_err(|e| sqlite_err("query", e))?;
                let Some((identity, runtime_id, generation, cpv, token)) = row else {
                    return Ok(None);
                };
                // The substrate's CURRENT checkpoint version for the session:
                // the fence the next write cursor must advance past. The
                // record's own stamp trails it whenever writes landed after
                // the last checkpoint.
                let fence_current: u64 = connection
                    .query_row(
                        "SELECT MAX(v) FROM (\
                             SELECT COALESCE(MAX(checkpoint_version), 0) AS v \
                                 FROM session_snapshots WHERE session_id = ?1 \
                             UNION ALL \
                             SELECT COALESCE(MAX(checkpoint_version), 0) AS v \
                                 FROM continuity_session_heads WHERE session_id = ?1)",
                        rusqlite::params![session_id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(|e| sqlite_err("fence query", e))?;
                let fence_current = fence_current.max(cpv);
                let record = ContinuityRecord {
                    identity: AgentIdentity::parse(&identity).map_err(|e| {
                        ContinuityStoreError::Corruption(format!("invalid identity in store: {e}"))
                    })?,
                    agent_runtime_id: AgentRuntimeId::parse(&runtime_id).map_err(|e| {
                        ContinuityStoreError::Corruption(format!(
                            "invalid runtime_id in store: {e}"
                        ))
                    })?,
                    session_id: session_id.clone(),
                    generation: ContinuityGeneration::new(generation),
                    checkpoint_version: CheckpointVersion::new(cpv),
                };
                Ok(Some((
                    record,
                    FencingToken::new(token),
                    CheckpointVersion::new(fence_current),
                )))
            })
        })
        .await
    }

    async fn load_session_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
        let session_id = session_id.clone();
        self.run_blocking("load_session_snapshot", move |inner| {
            inner.with_reader(|connection| {
                // Head-canonical sessions serve the slim materialization of
                // head+rows; their `session_snapshots` row (if any) is a
                // frozen archive and is never read again.
                if inner.head_tables_available(connection)? {
                    let tx = connection
                        .unchecked_transaction()
                        .map_err(|e| sqlite_err("begin read tx", e))?;
                    if let Some((head, _token)) = head_row_in_txn(&tx, &session_id)
                        .map_err(|e| ContinuityStoreError::Io(e.to_string()))?
                    {
                        let session = materialize_slim_in_txn(&tx, &session_id, &head)
                            .map_err(|e| ContinuityStoreError::Io(e.to_string()))?;
                        let data = serde_json::to_vec(&session).map_err(|e| {
                            ContinuityStoreError::Io(format!(
                                "serialize head-canonical session snapshot: {e}"
                            ))
                        })?;
                        return Ok(Some(SessionSnapshot { data }));
                    }
                }
                let mut stmt = connection
                    .prepare_cached("SELECT data FROM session_snapshots WHERE session_id = ?1")
                    .map_err(|e| sqlite_err("prepare", e))?;
                let row = stmt
                    .query_row(rusqlite::params![session_id.to_string()], |row| {
                        row.get::<_, Vec<u8>>(0)
                    })
                    .optional()
                    .map_err(|e| sqlite_err("query", e))?;
                Ok(row.map(|data| SessionSnapshot { data }))
            })
        })
        .await
    }

    async fn session_snapshot_matches_current(
        &self,
        candidate: SessionSnapshotMatchCandidate,
    ) -> Result<bool, ContinuityStoreError> {
        self.run_blocking("session_snapshot_matches_current", move |inner| {
            inner.with_reader(|connection| {
                // The whole-blob byte-equality probe is a blob-authority
                // concept. On a head-canonical session there is no candidate
                // blob to compare against, so the conservative trait default
                // applies and the caller takes its ordinary guard path.
                if inner.head_tables_available(connection)?
                    && head_owner_in_txn(
                        &connection
                            .unchecked_transaction()
                            .map_err(|e| sqlite_err("begin read tx", e))?,
                        &candidate.session_id,
                    )?
                    .is_some()
                {
                    return Ok(false);
                }
                connection
                    .query_row(
                        "SELECT EXISTS(
                            SELECT 1
                            FROM session_snapshots AS snapshot
                            JOIN continuity_records AS continuity
                              ON continuity.identity = snapshot.identity
                             AND continuity.session_id = snapshot.session_id
                             AND continuity.generation = snapshot.generation
                             AND continuity.checkpoint_version = snapshot.checkpoint_version
                            WHERE snapshot.session_id = ?1
                              AND snapshot.identity = ?2
                              AND snapshot.generation = ?3
                              AND snapshot.checkpoint_version = ?4
                              AND continuity.fencing_token = ?5
                              AND snapshot.fencing_token <= ?5
                              AND snapshot.data = ?6
                        )",
                        rusqlite::params![
                            candidate.session_id.to_string(),
                            candidate.identity.as_str(),
                            candidate.generation.get(),
                            candidate.checkpoint_version.get(),
                            candidate.fencing_token.get(),
                            &candidate.snapshot.data,
                        ],
                        |row| row.get::<_, bool>(0),
                    )
                    .map_err(|e| sqlite_err("match session snapshot", e))
            })
        })
        .await
    }

    async fn delete_session_snapshot_if_current_revision(
        &self,
        session_id: &meerkat_core::types::SessionId,
        expected_current_revision: &str,
    ) -> Result<bool, ContinuityStoreError> {
        let session_id = session_id.clone();
        let expected_current_revision = expected_current_revision.to_string();
        self.run_blocking(
            "delete_session_snapshot_if_current_revision",
            move |inner| {
                inner.with_writer(|connection| {
                    let head_tables = inner.head_tables_available(connection)?;
                    let tx = connection
                        .transaction()
                        .map_err(|e| sqlite_err("begin tx", e))?;

                    // Head-canonical sessions derive the CAS token from the
                    // slim materialization of head+rows, then drop head,
                    // strands, rewrites AND the frozen archive in one tx.
                    let head = if head_tables {
                        head_row_in_txn(&tx, &session_id)
                            .map_err(|e| ContinuityStoreError::Io(e.to_string()))?
                    } else {
                        None
                    };
                    let session = match head.as_ref() {
                        Some((head, _token)) => Some(
                            materialize_slim_in_txn(&tx, &session_id, head)
                                .map_err(|e| ContinuityStoreError::Io(e.to_string()))?,
                        ),
                        None => {
                            let data = tx
                                .query_row(
                                    "SELECT data FROM session_snapshots WHERE session_id = ?1",
                                    rusqlite::params![session_id.to_string()],
                                    |row| row.get::<_, Vec<u8>>(0),
                                )
                                .optional()
                                .map_err(|e| sqlite_err("query snapshot", e))?;
                            match data {
                                Some(data) => {
                                    Some(serde_json::from_slice::<Session>(&data).map_err(|e| {
                                        ContinuityStoreError::Io(format!(
                                            "deserialize session snapshot for revision check: {e}"
                                        ))
                                    })?)
                                }
                                None => None,
                            }
                        }
                    };

                    let Some(session) = session else {
                        return Ok(false);
                    };
                    let current_revision =
                        meerkat_core::session_store::session_projection_cas_token(&session)
                            .map_err(|e| ContinuityStoreError::Io(e.to_string()))?;
                    if current_revision != expected_current_revision {
                        return Ok(false);
                    }

                    let deleted = tx
                        .execute(
                            "DELETE FROM session_snapshots WHERE session_id = ?1",
                            rusqlite::params![session_id.to_string()],
                        )
                        .map_err(|e| sqlite_err("delete snapshot", e))?;
                    let head_deleted = if head_tables {
                        delete_head_canonical_rows_in_txn(
                            &tx,
                            "session_id = ?1",
                            rusqlite::params![session_id.to_string()],
                        )?;
                        head.is_some()
                    } else {
                        false
                    };
                    tx.commit()
                        .map_err(|e| sqlite_err("commit snapshot delete", e))?;
                    Ok(deleted > 0 || head_deleted)
                })
            },
        )
        .await
    }

    async fn save_session_snapshot(
        &self,
        identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        generation: ContinuityGeneration,
        version: CheckpointVersion,
        fencing_token: FencingToken,
        snapshot: &SessionSnapshot,
    ) -> Result<(), ContinuityStoreError> {
        self.save_session_snapshot_owned(
            identity.clone(),
            session_id.clone(),
            generation,
            version,
            fencing_token,
            snapshot.clone(),
        )
        .await
    }

    async fn save_session_snapshot_owned(
        &self,
        identity: AgentIdentity,
        session_id: meerkat_core::types::SessionId,
        generation: ContinuityGeneration,
        version: CheckpointVersion,
        fencing_token: FencingToken,
        snapshot: SessionSnapshot,
    ) -> Result<(), ContinuityStoreError> {
        self.run_blocking("save_session_snapshot", move |inner| {
            inner.with_writer(|connection| {
                let head_tables = inner.head_tables_available(connection)?;
                // Keep the check, snapshot write, and record version/fence
                // advance in one writer transaction.
                let tx = connection
                    .unchecked_transaction()
                    .map_err(|e| sqlite_err("begin tx", e))?;

                let record_was_present = enforce_continuity_cursor_in_txn(
                    &tx,
                    &identity,
                    &session_id,
                    generation,
                    version,
                    fencing_token,
                )?;

                ensure_snapshot_owner_in_txn(&tx, &session_id, &identity, generation)?;
                if head_tables {
                    ensure_head_owner_in_txn(&tx, &session_id, &identity, generation)?;
                }

                // Representation-aware write. A head row means head+rows are
                // this session's byte authority: convert the incoming
                // document into delta rows + a small head instead of
                // upserting the blob, and leave the frozen archive row
                // untouched. Without a head row the legacy blob semantics
                // are byte-for-byte unchanged — an ordinary whole-document
                // save never migrates a session and never stamps ledger v2.
                let head = if head_tables {
                    head_row_in_txn(&tx, &session_id)
                        .map_err(|e| ContinuityStoreError::Io(e.to_string()))?
                } else {
                    None
                };
                match head {
                    Some((head, _token)) => {
                        // Once head+rows are a session's byte authority, a
                        // whole-document write has to be expressible as rows.
                        // Falling back to the blob row here would be the
                        // two-write-authorities failure the representation
                        // rule exists to prevent (the blob would be silently
                        // never read again), so this refuses instead.
                        let session: Session = serde_json::from_slice(&snapshot.data)
                            .map_err(|e| {
                                ContinuityStoreError::Io(format!(
                                    "session {session_id} is head-canonical: a whole-document \
                                     save must carry a serialized session document, not opaque \
                                     bytes ({e})"
                                ))
                            })?;
                        if session.id() != &session_id {
                            return Err(ContinuityStoreError::Corruption(format!(
                                "session snapshot for {session_id} carries session {}",
                                session.id()
                            )));
                        }
                        write_head_canonical_session_in_txn(
                            &tx,
                            &session,
                            &head,
                            &identity,
                            generation,
                            version,
                            fencing_token,
                        )
                        .map_err(|e| ContinuityStoreError::Io(e.to_string()))?;
                    }
                    None => {
                        tx.execute(
                            "INSERT INTO session_snapshots (session_id, identity, generation, checkpoint_version, fencing_token, data)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                             ON CONFLICT(session_id) DO UPDATE SET
                                identity = excluded.identity,
                                generation = excluded.generation,
                                checkpoint_version = excluded.checkpoint_version,
                                fencing_token = excluded.fencing_token,
                                data = excluded.data",
                            rusqlite::params![
                                session_id.to_string(),
                                identity.as_str(),
                                generation.get(),
                                version.get(),
                                fencing_token.get(),
                                &snapshot.data,
                            ],
                        )
                        .map_err(|e| sqlite_err("upsert snapshot", e))?;
                    }
                }

                advance_continuity_record_in_txn(
                    &tx,
                    &identity,
                    &session_id,
                    generation,
                    version,
                    fencing_token,
                    record_was_present,
                )?;

                tx.commit()
                    .map_err(|e| sqlite_err("commit tx", e))?;
                Ok(())
            })
        })
        .await
    }

    async fn upsert_continuity_record(
        &self,
        record: &ContinuityRecord,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let record = record.clone();
        self.run_blocking("upsert_continuity_record", move |inner| {
            inner.with_writer(|connection| {
                let mut stmt = connection
                    .prepare_cached(
                        "SELECT fencing_token, generation FROM continuity_records WHERE identity = ?1",
                    )
                    .map_err(|e| sqlite_err("prepare", e))?;
                let existing = stmt
                    .query_row(rusqlite::params![record.identity.as_str()], |row| {
                        Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?))
                    })
                    .optional()
                    .map_err(|e| sqlite_err("query", e))?;
                drop(stmt);

                if let Some((current_token, current_generation)) = existing {
                    if fencing_token.get() < current_token {
                        return Err(ContinuityStoreError::StaleFencingToken {
                            identity: record.identity.clone(),
                            presented: fencing_token,
                            current: FencingToken::new(current_token),
                        });
                    }
                    if record.generation.get() < current_generation {
                        return Err(ContinuityStoreError::StaleContinuityGeneration {
                            identity: record.identity.clone(),
                            presented: record.generation,
                            current: ContinuityGeneration::new(current_generation),
                        });
                    }
                }

                connection
                    .execute(
                        "INSERT INTO continuity_records (identity, agent_runtime_id, session_id, generation, checkpoint_version, fencing_token)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT(identity) DO UPDATE SET
                            agent_runtime_id = excluded.agent_runtime_id,
                            session_id = excluded.session_id,
                            generation = excluded.generation,
                            checkpoint_version = CASE
                                WHEN continuity_records.generation = excluded.generation
                                THEN MAX(continuity_records.checkpoint_version, excluded.checkpoint_version)
                                ELSE excluded.checkpoint_version
                            END,
                            fencing_token = excluded.fencing_token",
                        rusqlite::params![
                            record.identity.as_str(),
                            record.agent_runtime_id.as_str(),
                            record.session_id.to_string(),
                            record.generation.get(),
                            record.checkpoint_version.get(),
                            fencing_token.get(),
                        ],
                    )
                    .map_err(|e| sqlite_err("upsert record", e))?;
                Ok(())
            })
        })
        .await
    }

    async fn rollback_continuity_record(
        &self,
        expected_attempt: &ContinuityRecord,
        previous: Option<&ContinuityRecord>,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let expected_attempt = expected_attempt.clone();
        let previous = previous.cloned();
        self.run_blocking("rollback_continuity_record", move |inner| {
            inner.with_writer(|connection| {
                let head_tables = inner.head_tables_available(connection)?;
                if previous
                    .as_ref()
                    .is_some_and(|record| record.identity != expected_attempt.identity)
                {
                    return Err(ContinuityStoreError::Corruption(format!(
                        "reset rollback identity mismatch: attempted {}, previous {}",
                        expected_attempt.identity,
                        previous
                            .as_ref()
                            .map(|record| record.identity.as_str())
                            .unwrap_or_default(),
                    )));
                }

                let tx = connection
                    .unchecked_transaction()
                    .map_err(|e| sqlite_err("begin tx", e))?;
                let current = tx
                    .query_row(
                        "SELECT agent_runtime_id, session_id, generation, fencing_token
                         FROM continuity_records WHERE identity = ?1",
                        rusqlite::params![expected_attempt.identity.as_str()],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, u64>(2)?,
                                row.get::<_, u64>(3)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(|e| sqlite_err("query", e))?
                    .ok_or_else(|| ContinuityStoreError::NotFound {
                        identity: expected_attempt.identity.clone(),
                    })?;

                let (current_runtime_id, current_session_id, current_generation, current_token) =
                    current;
                if current_token != fencing_token.get() {
                    return Err(ContinuityStoreError::StaleFencingToken {
                        identity: expected_attempt.identity.clone(),
                        presented: fencing_token,
                        current: FencingToken::new(current_token),
                    });
                }
                if current_runtime_id != expected_attempt.agent_runtime_id.as_str()
                    || current_session_id != expected_attempt.session_id.to_string()
                    || current_generation != expected_attempt.generation.get()
                {
                    return Err(ContinuityStoreError::StaleContinuityGeneration {
                        identity: expected_attempt.identity.clone(),
                        presented: expected_attempt.generation,
                        current: ContinuityGeneration::new(current_generation),
                    });
                }

                // Only the provisional reset generation is abandoned. Older
                // snapshots — blob rows AND head-canonical head/strand/
                // rewrite rows alike — remain the rollback authority for the
                // restored row, while a concurrently advanced generation is
                // protected by the exact CAS above.
                tx.execute(
                    "DELETE FROM session_snapshots WHERE identity = ?1 AND generation = ?2",
                    rusqlite::params![
                        expected_attempt.identity.as_str(),
                        expected_attempt.generation.get(),
                    ],
                )
                .map_err(|e| sqlite_err("delete attempted snapshots", e))?;
                if head_tables {
                    delete_head_canonical_rows_in_txn(
                        &tx,
                        "identity = ?1 AND generation = ?2",
                        rusqlite::params![
                            expected_attempt.identity.as_str(),
                            expected_attempt.generation.get(),
                        ],
                    )?;
                }

                if let Some(previous) = previous {
                    tx.execute(
                        "UPDATE continuity_records
                         SET agent_runtime_id = ?1,
                             session_id = ?2,
                             generation = ?3,
                             checkpoint_version = ?4,
                             fencing_token = ?5
                         WHERE identity = ?6",
                        rusqlite::params![
                            previous.agent_runtime_id.as_str(),
                            previous.session_id.to_string(),
                            previous.generation.get(),
                            previous.checkpoint_version.get(),
                            fencing_token.get(),
                            expected_attempt.identity.as_str(),
                        ],
                    )
                    .map_err(|e| sqlite_err("restore previous record", e))?;
                } else {
                    tx.execute(
                        "DELETE FROM continuity_records WHERE identity = ?1",
                        rusqlite::params![expected_attempt.identity.as_str()],
                    )
                    .map_err(|e| sqlite_err("delete attempted record", e))?;
                }

                tx.commit().map_err(|e| sqlite_err("commit tx", e))?;
                Ok(())
            })
        })
        .await
    }

    /// M4b landed: the bundled store ships the session-delta channel because
    /// head+rows are now its canonical durable session representation, not a
    /// second authority beside `session_snapshots.data`.
    ///
    /// The deferral note this replaces named the exact hazard — a delta
    /// channel bolted beside the blob would create two write authorities
    /// over one session with no reconciliation rule. The rule now exists and
    /// every whole-snapshot verb honors it: a
    /// `continuity_session_heads` row means head+rows are the sole byte
    /// authority for that session, its blob row is a frozen archive that is
    /// never read or written again, whole-document saves convert into delta
    /// rows + a head, the exact-match probe declines, CAS tokens derive from
    /// the slim materialization, and delete/rollback scope all four tables.
    ///
    /// Advertising the capability does NOT mutate the file: the head-canonical
    /// ledger bump is committed by a delta write that actually creates head
    /// state, inside that write's own transaction (see [`Self::open`] and
    /// [`LocalContinuityStore::delta_write`]).
    fn as_incremental_sessions(&self) -> Option<Arc<dyn ContinuityIncrementalSessions>> {
        Some(Arc::new(self.clone()))
    }

    async fn delete_continuity_record(
        &self,
        identity: &AgentIdentity,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let identity = identity.clone();
        self.run_blocking("delete_continuity_record", move |inner| {
            inner.with_writer(|connection| {
                let head_tables = inner.head_tables_available(connection)?;
                // Keep the fence check and both deletes in one transaction so
                // failures cannot leave a half-deleted identity.
                let tx = connection
                    .unchecked_transaction()
                    .map_err(|e| sqlite_err("begin tx", e))?;

                let mut stmt = tx
                    .prepare_cached(
                        "SELECT fencing_token FROM continuity_records WHERE identity = ?1",
                    )
                    .map_err(|e| sqlite_err("prepare", e))?;
                let existing_token = stmt
                    .query_row(rusqlite::params![identity.as_str()], |row| {
                        row.get::<_, u64>(0)
                    })
                    .optional()
                    .map_err(|e| sqlite_err("query", e))?;
                drop(stmt);

                if let Some(current) = existing_token
                    && fencing_token.get() < current
                {
                    return Err(ContinuityStoreError::StaleFencingToken {
                        identity: identity.clone(),
                        presented: fencing_token,
                        current: FencingToken::new(current),
                    });
                }

                tx.execute(
                    "DELETE FROM session_snapshots WHERE identity = ?1",
                    rusqlite::params![identity.as_str()],
                )
                .map_err(|e| sqlite_err("delete snapshots", e))?;
                if head_tables {
                    delete_head_canonical_rows_in_txn(
                        &tx,
                        "identity = ?1",
                        rusqlite::params![identity.as_str()],
                    )?;
                }
                tx.execute(
                    "DELETE FROM continuity_records WHERE identity = ?1",
                    rusqlite::params![identity.as_str()],
                )
                .map_err(|e| sqlite_err("delete record", e))?;
                tx.commit().map_err(|e| sqlite_err("commit tx", e))?;
                Ok(())
            })
        })
        .await
    }
}

// ---------------------------------------------------------------------------
// The session-delta channel (M4b)
// ---------------------------------------------------------------------------

impl LocalContinuityStore {
    /// Read-only enumeration of every durable identity → session binding.
    ///
    /// Operator-maintenance surface (task #63): the repair binary's
    /// `--all-sessions` pass needs the fleet's bindings without knowing
    /// identities up front; each binding then goes through the ordinary
    /// per-session [`ContinuityStore::resolve_record_by_session`] path, so
    /// this adds no new write or trust surface.
    pub async fn list_session_bindings(
        &self,
    ) -> Result<Vec<(AgentIdentity, meerkat_core::types::SessionId)>, ContinuityStoreError> {
        self.run_blocking("list_session_bindings", move |inner| {
            inner.with_reader(|connection| {
                let mut stmt = connection
                    .prepare_cached(
                        "SELECT identity, session_id FROM continuity_records ORDER BY identity",
                    )
                    .map_err(|e| sqlite_err("prepare", e))?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(|e| sqlite_err("query", e))?;
                let mut bindings = Vec::new();
                for row in rows {
                    let (identity, session_id) = row.map_err(|e| sqlite_err("row", e))?;
                    bindings.push((
                        AgentIdentity::parse(&identity).map_err(|e| {
                            ContinuityStoreError::Corruption(format!(
                                "invalid identity in store: {e}"
                            ))
                        })?,
                        meerkat_core::types::SessionId::parse(&session_id).map_err(|e| {
                            ContinuityStoreError::Corruption(format!(
                                "invalid session id in store: {e}"
                            ))
                        })?,
                    ));
                }
                Ok(bindings)
            })
        })
        .await
    }

    /// Run one delta mutation inside ONE writer transaction that enforces
    /// and advances the continuity cursor — and, on a file that does not
    /// carry the head-canonical channel yet, converges the schema and arms
    /// the one-way ledger bump in that same transaction.
    ///
    /// Crash consistency: the head-canonical DDL, the rows, the head row,
    /// the `continuity_records` advance and the ledger v2 stamp commit
    /// together or not at all. So the durable cursor can never point past
    /// durable data, a partial append can never leave a torn document, and
    /// — the property this ordering exists for — **a write that does not
    /// leave a head row behind never arms the v1-writer lockout**.
    ///
    /// Two ways a write fails to earn the bump, and both leave the file
    /// rollback-safe at v1:
    ///
    /// - it is REFUSED (a guard rejection or a typed operation refusal):
    ///   the DDL and the stamp roll back with everything else, and the file
    ///   keeps zero head rows;
    /// - it is ACCEPTED but creates no head state. `append_messages` on a
    ///   session with neither a head row nor a blob to migrate is the real
    ///   case: the service appends and then adopts under two separate
    ///   locks, so the append commits alone. Strand rows that no head
    ///   adopts are not part of any document — every read path gates on the
    ///   head row — so an older binary still correctly serves that session
    ///   from its blob, and locking it out of the whole file would be a
    ///   brick bought for nothing. The rows and the (additive, `IF NOT
    ///   EXISTS`) DDL commit; the stamp waits for the adopting head write.
    ///
    /// The converse — a head row with no stamp — remains impossible, so an
    /// older binary can never mistake a frozen blob archive for authority.
    /// That is why the "is the lockout owed" question is answered from the
    /// ledger row inside this transaction
    /// ([`head_canonical_ledger_stamped_in_txn`]) and never from the
    /// table-existence latch, which this method can now leave `true` on a
    /// v1 file.
    async fn delta_write<T, F>(
        &self,
        operation_name: &'static str,
        cursor: &ContinuityWriteCursor,
        session_id: meerkat_core::types::SessionId,
        operation: F,
    ) -> Result<T, SessionStoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>, &ContinuityWriteCursor) -> Result<T, SessionStoreError>
            + Send
            + 'static,
    {
        let cursor = cursor.clone();
        // The typed session-store error the guards produce rides OUT as a
        // value, never laundered into `Internal`: the accept/reject boundary
        // this store mirrors must be indistinguishable from the meerkat
        // service's, conflict codes included. A carried `Err` skips the
        // commit, so the transaction rolls back on drop.
        let outcome: Result<Result<T, SessionStoreError>, ContinuityStoreError> = self
            .run_blocking(operation_name, move |inner| {
                inner.with_writer(|connection| {
                    let tx = connection
                        .unchecked_transaction()
                        .map_err(|e| sqlite_err("begin tx", e))?;
                    // Is the one-way lockout still owed? Answered from the
                    // ledger row itself, inside this transaction — the
                    // cached flag may only SKIP the read, never decide that
                    // no bump is owed.
                    let lockout_owed = !inner.ledger_is_head_canonical()
                        && !head_canonical_ledger_stamped_in_txn(&tx)?;
                    // Converge the head-canonical schema INSIDE this
                    // transaction (SQLite DDL is transactional): the tables
                    // have to exist before the operation can write a row,
                    // but they vanish again with a rollback. The ledger
                    // stamp that makes the bump one-way is deliberately NOT
                    // written here — it is the last thing before commit,
                    // and only once the write has actually left a head row
                    // behind.
                    if lockout_owed {
                        converge_head_canonical_schema_in_txn(&tx)?;
                    }
                    let record_was_present = enforce_continuity_cursor_in_txn(
                        &tx,
                        &cursor.identity,
                        &session_id,
                        cursor.generation,
                        cursor.checkpoint_version,
                        cursor.fencing_token,
                    )?;
                    // Both owner guards, in the same order and with the same
                    // meaning the whole-document verb applies. A delta
                    // mutation must not be accepted where a whole-document
                    // save onto the same durable session would be refused.
                    ensure_snapshot_owner_in_txn(
                        &tx,
                        &session_id,
                        &cursor.identity,
                        cursor.generation,
                    )?;
                    ensure_head_owner_in_txn(
                        &tx,
                        &session_id,
                        &cursor.identity,
                        cursor.generation,
                    )?;
                    let value = match operation(&tx, &cursor) {
                        Ok(value) => value,
                        Err(typed) => return Ok(Err(typed)),
                    };
                    advance_continuity_record_in_txn(
                        &tx,
                        &cursor.identity,
                        &session_id,
                        cursor.generation,
                        cursor.checkpoint_version,
                        cursor.fencing_token,
                        record_was_present,
                    )?;
                    // Earned only by head state that will be durable when
                    // this transaction commits. An accepted append that
                    // adopts nothing leaves the file at v1.
                    let stamp_lockout =
                        lockout_owed && session_head_exists_in_txn(&tx, &session_id)?;
                    if stamp_lockout {
                        stamp_head_canonical_ledger_in_txn(&tx)?;
                    }
                    tx.commit().map_err(|e| sqlite_err("commit tx", e))?;
                    // Latch only after the commit that made each fact true,
                    // and keep the two facts apart: the DDL committed
                    // whenever the bump was owed, so the tables are
                    // queryable either way, but the lockout latch tracks
                    // the ledger row alone.
                    if lockout_owed {
                        inner.head_canonical_schema.store(true, Ordering::Release);
                    }
                    if stamp_lockout || !lockout_owed {
                        inner.head_canonical_ledger.store(true, Ordering::Release);
                    }
                    Ok(Ok(value))
                })
            })
            .await;
        match outcome {
            Ok(typed) => typed,
            Err(error) => Err(session_err(operation_name, error)),
        }
    }

    /// Read one delta view. Head-canonical sessions read their rows; a
    /// blob-only session is served from the deterministic read-only strand
    /// layout of its archived document (never a write), so the CAS token a
    /// caller derives before migration matches the one the first migrating
    /// write persists.
    async fn delta_read<T, F>(
        &self,
        operation_name: &'static str,
        operation: F,
    ) -> Result<T, SessionStoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>, bool) -> Result<T, SessionStoreError> + Send + 'static,
    {
        let outcome: Result<Result<T, SessionStoreError>, ContinuityStoreError> = self
            .run_blocking(operation_name, move |inner| {
                inner.with_reader(|connection| {
                    let head_tables = inner.head_tables_available(connection)?;
                    let tx = connection
                        .unchecked_transaction()
                        .map_err(|e| sqlite_err("begin read tx", e))?;
                    Ok(operation(&tx, head_tables))
                })
            })
            .await;
        match outcome {
            Ok(typed) => typed,
            Err(error) => Err(session_err(operation_name, error)),
        }
    }
}

#[async_trait]
impl ContinuityIncrementalSessions for LocalContinuityStore {
    async fn append_messages(
        &self,
        cursor: &ContinuityWriteCursor,
        id: &meerkat_core::types::SessionId,
        strand: &TranscriptStrandId,
        base_seq: u64,
        messages: &[Message],
    ) -> Result<(), SessionStoreError> {
        let session_id = id.clone();
        let strand = strand.clone();
        let messages = messages.to_vec();
        let migrate_id = session_id.clone();
        self.delta_write(
            "continuity append_messages",
            cursor,
            session_id,
            move |tx, cursor| {
                // First delta write on a blob-only session migrates it inside
                // this transaction; the blob stays as a frozen archive.
                ensure_head_canonical_for_write_in_txn(
                    tx,
                    &migrate_id,
                    &cursor.identity,
                    cursor.generation,
                    cursor.checkpoint_version,
                    cursor.fencing_token,
                )?;
                insert_strand_rows_in_txn(
                    tx,
                    &migrate_id,
                    &strand,
                    base_seq,
                    &messages,
                    &cursor.identity,
                    cursor.generation,
                )
            },
        )
        .await
    }

    async fn commit_rewrite(
        &self,
        cursor: &ContinuityWriteCursor,
        id: &meerkat_core::types::SessionId,
        record: &TranscriptRewriteRecord,
        expected: SessionHeadCas,
    ) -> Result<SessionHead, SessionStoreError> {
        let session_id = id.clone();
        let record = record.clone();
        let target = session_id.clone();
        self.delta_write(
            "continuity commit_rewrite",
            cursor,
            session_id,
            move |tx, cursor| {
                let stored = ensure_head_canonical_for_write_in_txn(
                    tx,
                    &target,
                    &cursor.identity,
                    cursor.generation,
                    cursor.checkpoint_version,
                    cursor.fencing_token,
                )?
                .ok_or_else(|| SessionStoreError::InvalidTranscriptRewrite {
                    id: target.clone(),
                    reason: "rewrite target has no persisted session head".to_string(),
                })?;
                let (stored_head, stored_token) = &stored;
                // CAS races and stale parents surface as
                // TranscriptRevisionConflict BEFORE the parent strand read,
                // which would otherwise fail on an unrelated shape.
                match &expected {
                    SessionHeadCas::Create => {
                        return Err(SessionStoreError::TranscriptRevisionConflict {
                            id: target.clone(),
                            expected: "<create>".to_string(),
                            actual: stored_token.clone(),
                        });
                    }
                    SessionHeadCas::IfToken(expected_token) => {
                        if expected_token != stored_token {
                            return Err(SessionStoreError::TranscriptRevisionConflict {
                                id: target.clone(),
                                expected: expected_token.clone(),
                                actual: stored_token.clone(),
                            });
                        }
                    }
                }
                if record.commit.parent_revision != stored_head.head_revision {
                    return Err(SessionStoreError::TranscriptRevisionConflict {
                        id: target,
                        expected: record.commit.parent_revision,
                        actual: stored_head.head_revision.clone(),
                    });
                }
                let before = record.commit.messages_before as u64;
                if before > strand_row_count_in_txn(tx, &target, &stored_head.strand)? {
                    return Err(SessionStoreError::InvalidTranscriptRewrite {
                        id: target,
                        reason: format!(
                            "commit messages_before {before} exceeds persisted rows of strand {}",
                            stored_head.strand
                        ),
                    });
                }
                let parent_rows =
                    strand_messages_in_txn(tx, &target, &stored_head.strand, 0..before)?;
                let parent_digest = meerkat_core::transcript_messages_digest(&parent_rows)
                    .map_err(SessionStoreError::from)?;
                let next = validate_commit_rewrite_transition(
                    &target,
                    &record,
                    stored_head,
                    stored_token,
                    &expected,
                    &parent_digest,
                )?;
                insert_rewrite_row_in_txn(
                    tx,
                    &target,
                    stored_head.rewrite_count,
                    &RewriteRow {
                        commit: record.commit.clone(),
                        parent_strand: stored_head.strand.clone(),
                        parent_len: before,
                        strand: next.strand.clone(),
                        strand_len: record.commit.messages_after as u64,
                    },
                    &cursor.identity,
                    cursor.generation,
                )?;
                insert_strand_rows_in_txn(
                    tx,
                    &target,
                    &next.strand,
                    0,
                    &record.revision_body.messages,
                    &cursor.identity,
                    cursor.generation,
                )?;
                Ok(next)
            },
        )
        .await
    }

    async fn save_head(
        &self,
        cursor: &ContinuityWriteCursor,
        head: &SessionHead,
        expected: SessionHeadCas,
    ) -> Result<(), SessionStoreError> {
        let session_id = head.id.clone();
        let head = head.clone();
        self.delta_write(
            "continuity save_head",
            cursor,
            session_id,
            move |tx, cursor| {
                let stored = ensure_head_canonical_for_write_in_txn(
                    tx,
                    &head.id,
                    &cursor.identity,
                    cursor.generation,
                    cursor.checkpoint_version,
                    cursor.fencing_token,
                )?;
                let strand_len = strand_row_count_in_txn(tx, &head.id, &head.strand)?;
                let recorded = rewrite_row_count_in_txn(tx, &head.id)?;
                validate_save_head_transition(
                    &head,
                    stored.as_ref().map(|(h, t)| (h, t.as_str())),
                    &expected,
                    strand_len,
                    recorded,
                )?;
                write_head_row_in_txn(
                    tx,
                    &head,
                    &cursor.identity,
                    cursor.generation,
                    cursor.checkpoint_version,
                    cursor.fencing_token,
                )?;
                Ok(())
            },
        )
        .await
    }

    async fn adopt_released_head_document(
        &self,
        cursor: &ContinuityWriteCursor,
        session: &meerkat_core::Session,
    ) -> Result<(), SessionStoreError> {
        let session = session.clone();
        self.delta_write(
            "continuity adopt_released_head_document",
            cursor,
            session.id().clone(),
            move |tx, cursor| {
                adopt_released_head_in_txn(
                    tx,
                    &session,
                    &cursor.identity,
                    cursor.generation,
                    cursor.checkpoint_version,
                    cursor.fencing_token,
                )
            },
        )
        .await
    }

    async fn session_head_matches_current(
        &self,
        identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        generation: ContinuityGeneration,
        fencing_token: FencingToken,
        head: &SessionHead,
    ) -> Result<bool, SessionStoreError> {
        let identity = identity.clone();
        let session_id = session_id.clone();
        let head = head.clone();
        self.delta_read(
            "continuity session_head_matches_current",
            move |tx, head_tables| {
                if !head_tables {
                    return Ok(false);
                }
                let Some((stored, _token)) = head_row_in_txn(tx, &session_id)? else {
                    return Ok(false);
                };
                if stored != head {
                    return Ok(false);
                }
                // Fence currency, same shape `enforce_continuity_cursor_in_txn`
                // validates on the mutating verbs: the identity's CURRENT
                // record must bind this session and generation, and its fence
                // must EQUAL the presented one. An advanced durable fence
                // makes this probe false so the caller's fencing write verb
                // surfaces the ordinary stale-fence refusal.
                let record = tx
                    .query_row(
                        "SELECT session_id, generation, fencing_token
                         FROM continuity_records WHERE identity = ?1",
                        rusqlite::params![identity.as_str()],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, u64>(1)?,
                                row.get::<_, u64>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(|e| sqlite_session_err("query continuity record", e))?;
                let Some((record_session, record_generation, record_token)) = record else {
                    return Ok(false);
                };
                Ok(record_session == session_id.to_string()
                    && record_generation == generation.get()
                    && record_token == fencing_token.get())
            },
        )
        .await
    }

    async fn load_head(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Option<SessionHead>, SessionStoreError> {
        let id = id.clone();
        self.delta_read("continuity load_head", move |tx, head_tables| {
            if head_tables && let Some((head, _token)) = head_row_in_txn(tx, &id)? {
                return Ok(Some(head));
            }
            let Some(session) = blob_session_in_txn(tx, &id)? else {
                return Ok(None);
            };
            let (_layout, head) = layout_for_blob_session(&session)?;
            Ok(Some(head))
        })
        .await
    }

    async fn load_canonical_head(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Option<SessionHead>, SessionStoreError> {
        let id = id.clone();
        self.delta_read("continuity load_canonical_head", move |tx, head_tables| {
            if !head_tables {
                return Ok(None);
            }
            Ok(head_row_in_txn(tx, &id)?.map(|(head, _token)| head))
        })
        .await
    }

    async fn load_canonical_session(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Option<Session>, SessionStoreError> {
        let id = id.clone();
        self.delta_read(
            "continuity load_canonical_session",
            move |tx, head_tables| {
                if !head_tables {
                    return Ok(None);
                }
                // ONE snapshot over head + rows. `materialize_slim_in_txn`
                // re-derives the transcript digest against `head_revision`, so a
                // torn pair would surface as `Corrupted` rather than silently —
                // but under a single transaction the pair cannot tear at all.
                let Some((head, _token)) = head_row_in_txn(tx, &id)? else {
                    return Ok(None);
                };
                materialize_slim_in_txn(tx, &id, &head).map(Some)
            },
        )
        .await
    }

    async fn load_canonical_previous(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Option<(Session, Vec<meerkat_core::TranscriptRewriteCommit>)>, SessionStoreError>
    {
        let id = id.clone();
        self.delta_read(
            "continuity load_canonical_previous",
            move |tx, head_tables| {
                if !head_tables {
                    return Ok(None);
                }
                let Some((head, _token)) = head_row_in_txn(tx, &id)? else {
                    return Ok(None);
                };
                let rewrite_count = head.rewrite_count;
                let session = materialize_slim_in_txn(tx, &id, &head)?;
                // The commits alone — deliberately not `load_rewrites`,
                // which reconstructs both message bodies of every rewrite
                // and would make a guard read O(rewrites x transcript).
                let adopted = rewrite_rows_in_txn(tx, &id, rewrite_count)?
                    .into_iter()
                    .map(|row| row.commit)
                    .collect();
                Ok(Some((session, adopted)))
            },
        )
        .await
    }

    async fn load_messages(
        &self,
        id: &meerkat_core::types::SessionId,
        strand: &TranscriptStrandId,
        range: std::ops::Range<u64>,
    ) -> Result<Vec<Message>, SessionStoreError> {
        let id = id.clone();
        let strand = strand.clone();
        self.delta_read("continuity load_messages", move |tx, head_tables| {
            if head_tables && head_row_in_txn(tx, &id)?.is_some() {
                return strand_messages_in_txn(tx, &id, &strand, range);
            }
            let Some(session) = blob_session_in_txn(tx, &id)? else {
                return Err(SessionStoreError::NotFound(id));
            };
            let (layout, _head) = layout_for_blob_session(&session)?;
            let rows = layout
                .strands
                .iter()
                .find(|(sid, _)| *sid == strand)
                .map(|(_, rows)| rows.as_slice())
                .ok_or_else(|| SessionStoreError::Corrupted(id.clone()))?;
            let start = usize::try_from(range.start)
                .map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
            let end =
                usize::try_from(range.end).map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
            if start > end || end > rows.len() {
                return Err(SessionStoreError::Corrupted(id.clone()));
            }
            Ok(rows[start..end].to_vec())
        })
        .await
    }

    async fn load_rewrites(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Vec<TranscriptRewriteRecord>, SessionStoreError> {
        let id = id.clone();
        self.delta_read("continuity load_rewrites", move |tx, head_tables| {
            if head_tables && let Some((head, _token)) = head_row_in_txn(tx, &id)? {
                return rewrite_records_in_txn(tx, &id, head.rewrite_count);
            }
            let Some(session) = blob_session_in_txn(tx, &id)? else {
                return Ok(Vec::new());
            };
            let (layout, _head) = layout_for_blob_session(&session)?;
            layout
                .rewrites
                .iter()
                .map(|rewrite| {
                    let parent_len = usize::try_from(rewrite.parent_len)
                        .map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
                    let strand_len = usize::try_from(rewrite.strand_len)
                        .map_err(|_| SessionStoreError::Corrupted(id.clone()))?;
                    let parent_messages = layout
                        .strands
                        .iter()
                        .find(|(sid, _)| *sid == rewrite.parent_strand)
                        .map(|(_, rows)| rows[..parent_len].to_vec())
                        .ok_or_else(|| SessionStoreError::Corrupted(id.clone()))?;
                    let revision_messages = layout
                        .strands
                        .iter()
                        .find(|(sid, _)| *sid == rewrite.strand)
                        .map(|(_, rows)| rows[..strand_len].to_vec())
                        .ok_or_else(|| SessionStoreError::Corrupted(id.clone()))?;
                    reconstruct_rewrite_record(
                        &id,
                        rewrite.commit.clone(),
                        parent_messages,
                        revision_messages,
                    )
                })
                .collect()
        })
        .await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn record(
        identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
    ) -> ContinuityRecord {
        ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt-001").unwrap(),
            session_id: session_id.clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        }
    }

    #[tokio::test]
    async fn fresh_store_stamps_mobkit_continuity_domain() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("continuity.sqlite3");
        let store = LocalContinuityStore::open(&path).expect("open");
        // The store must stay usable while we inspect the ledger.
        assert_eq!(store.max_fencing_token().expect("floor"), 0);
        let probe = Connection::open(&path).expect("probe");
        assert_eq!(
            meerkat_sqlite::domain_version(&probe, "mobkit-continuity").expect("ledger"),
            Some(1)
        );
    }

    /// A pre-ledger file (historical two-table DDL, no meerkat_schema table)
    /// is refused typed at open with its rows left untouched and no ledger
    /// stamped: pre-ledger corpora are below the mobkit 0.8.8 floor, and the
    /// 0.8.11 reset retired silent pre-floor convergence (this test pinned
    /// that convergence until then).
    #[tokio::test]
    async fn legacy_file_is_refused_with_rows_preserved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("continuity.sqlite3");
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let session_id = meerkat_core::types::SessionId::new();
        {
            let conn = Connection::open(&path).expect("legacy create");
            conn.execute_batch(SCHEMA).expect("legacy ddl");
            conn.execute(
                "INSERT INTO continuity_records (identity, agent_runtime_id, session_id, \
                 generation, checkpoint_version, fencing_token) VALUES (?1, 'rt-001', ?2, 3, 5, 7)",
                rusqlite::params![identity.as_str(), session_id.to_string()],
            )
            .expect("legacy record");
            conn.execute(
                "INSERT INTO session_snapshots (session_id, identity, generation, \
                 checkpoint_version, fencing_token, data) VALUES (?1, ?2, 3, 5, 9, X'010203')",
                rusqlite::params![session_id.to_string(), identity.as_str()],
            )
            .expect("legacy snapshot");
        }

        assert!(
            LocalContinuityStore::open(&path).is_err(),
            "opening a pre-ledger continuity database must refuse typed: unledgered owned \
             tables are below the mobkit 0.8.8 floor and must never be silently converged"
        );
        let probe = Connection::open(&path).expect("probe");
        let (generation, checkpoint): (i64, i64) = probe
            .query_row(
                "SELECT generation, checkpoint_version FROM continuity_records \
                 WHERE identity = ?1",
                rusqlite::params![identity.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("legacy record preserved");
        assert_eq!((generation, checkpoint), (3, 5));
        let snapshots: i64 = probe
            .query_row("SELECT COUNT(*) FROM session_snapshots", [], |row| {
                row.get(0)
            })
            .expect("legacy snapshots preserved");
        assert_eq!(snapshots, 1, "the refusal must leave legacy rows untouched");
        assert_eq!(
            meerkat_sqlite::domain_version(&probe, "mobkit-continuity").expect("ledger"),
            None,
            "a refused open must not stamp the ledger"
        );
    }

    #[tokio::test]
    async fn delete_continuity_record_removes_record_and_snapshots_atomically() {
        // Regression: the record + its session snapshots must be deleted as one
        // transaction so a crash between the two DELETEs cannot leave a
        // half-deleted store. Functionally: after a successful delete BOTH the
        // continuity record and the snapshot must be gone.
        let store = LocalContinuityStore::in_memory().expect("in-memory store");
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let session_id = meerkat_core::types::SessionId::new();

        store
            .upsert_continuity_record(&record(&identity, &session_id), FencingToken::new(1))
            .await
            .unwrap();
        store
            .save_session_snapshot(
                &identity,
                &session_id,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(1),
                &SessionSnapshot {
                    data: vec![1, 2, 3],
                },
            )
            .await
            .unwrap();

        // Both present before delete.
        assert!(
            store
                .load_session_snapshot(&session_id)
                .await
                .unwrap()
                .is_some()
        );

        store
            .delete_continuity_record(&identity, FencingToken::new(2))
            .await
            .unwrap();

        // Record gone: resolve returns Uninitialized.
        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .unwrap();
        assert!(matches!(
            resolved.get(&identity),
            Some(ContinuityResolveState::Uninitialized)
        ));
        // Snapshot gone too (same transaction).
        assert!(
            store
                .load_session_snapshot(&session_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn delete_continuity_record_rejects_stale_fencing_token() {
        let store = LocalContinuityStore::in_memory().expect("in-memory store");
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let session_id = meerkat_core::types::SessionId::new();
        store
            .upsert_continuity_record(&record(&identity, &session_id), FencingToken::new(5))
            .await
            .unwrap();

        let err = store
            .delete_continuity_record(&identity, FencingToken::new(2))
            .await
            .expect_err("stale fencing token must be rejected");
        assert!(matches!(
            err,
            ContinuityStoreError::StaleFencingToken { .. }
        ));

        // The record must survive a rejected delete.
        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .unwrap();
        assert!(!matches!(
            resolved.get(&identity),
            Some(ContinuityResolveState::Uninitialized)
        ));
    }

    #[tokio::test]
    async fn continuity_upsert_rejects_generation_regression_even_with_newer_fence() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let session_id = meerkat_core::types::SessionId::new();
        let mut current = record(&identity, &session_id);
        current.generation = ContinuityGeneration::new(1);
        store
            .upsert_continuity_record(&current, FencingToken::new(2))
            .await
            .unwrap();

        let mut stale = current.clone();
        stale.generation = ContinuityGeneration::new(0);
        let error = store
            .upsert_continuity_record(&stale, FencingToken::new(3))
            .await
            .expect_err("a newer fence must not authorize generation rollback");
        assert!(matches!(
            error,
            ContinuityStoreError::StaleContinuityGeneration { .. }
        ));
        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .unwrap();
        let ContinuityResolveState::Ready { record } = &resolved[&identity] else {
            panic!("continuity should remain ready");
        };
        assert_eq!(record.generation, ContinuityGeneration::new(1));
    }

    #[tokio::test]
    async fn same_generation_session_rebind_preserves_durable_checkpoint_head() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let old_session_id = meerkat_core::types::SessionId::new();
        let mut old = record(&identity, &old_session_id);
        old.checkpoint_version = CheckpointVersion::new(10);
        store
            .upsert_continuity_record(&old, FencingToken::new(1))
            .await
            .unwrap();
        store
            .save_session_snapshot(
                &identity,
                &old_session_id,
                old.generation,
                CheckpointVersion::new(11),
                FencingToken::new(2),
                &SessionSnapshot { data: vec![11] },
            )
            .await
            .unwrap();

        let new_session_id = meerkat_core::types::SessionId::new();
        let mut stale_rebind = old;
        stale_rebind.session_id = new_session_id.clone();
        store
            .upsert_continuity_record(&stale_rebind, FencingToken::new(3))
            .await
            .unwrap();

        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .unwrap();
        let ContinuityResolveState::Ready { record } = &resolved[&identity] else {
            panic!("continuity should remain ready");
        };
        assert_eq!(record.session_id, new_session_id);
        assert_eq!(record.checkpoint_version, CheckpointVersion::new(11));
    }

    #[tokio::test]
    async fn reset_rollback_cas_restores_previous_row_and_only_removes_attempt_snapshots() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let previous_session = meerkat_core::types::SessionId::new();
        let mut previous = record(&identity, &previous_session);
        store
            .upsert_continuity_record(&previous, FencingToken::new(1))
            .await
            .unwrap();
        store
            .save_session_snapshot(
                &identity,
                &previous_session,
                previous.generation,
                CheckpointVersion::new(1),
                FencingToken::new(1),
                &SessionSnapshot { data: vec![10] },
            )
            .await
            .unwrap();
        previous.checkpoint_version = CheckpointVersion::new(1);

        let attempted_session = meerkat_core::types::SessionId::new();
        let mut attempted = record(&identity, &attempted_session);
        attempted.agent_runtime_id = AgentRuntimeId::parse("rt:triage:main:1").unwrap();
        attempted.generation = ContinuityGeneration::new(1);
        store
            .upsert_continuity_record(&attempted, FencingToken::new(2))
            .await
            .unwrap();
        store
            .save_session_snapshot(
                &identity,
                &attempted_session,
                attempted.generation,
                CheckpointVersion::new(1),
                FencingToken::new(2),
                &SessionSnapshot { data: vec![20] },
            )
            .await
            .unwrap();

        // The session service may have advanced the attempted checkpoint
        // after reset captured the provisional record. Runtime/session/
        // generation/fence identify the attempt; its checkpoint is not part
        // of the rollback CAS.
        store
            .rollback_continuity_record(&attempted, Some(&previous), FencingToken::new(2))
            .await
            .unwrap();

        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .unwrap();
        assert_eq!(
            resolved.get(&identity),
            Some(&ContinuityResolveState::Ready {
                record: previous.clone(),
            })
        );
        assert_eq!(
            store
                .load_session_snapshot(&previous_session)
                .await
                .unwrap(),
            Some(SessionSnapshot { data: vec![10] })
        );
        assert_eq!(
            store
                .load_session_snapshot(&attempted_session)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn reset_rollback_cas_deletes_uninitialized_attempt_and_its_snapshots() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:new").unwrap();
        let attempted_session = meerkat_core::types::SessionId::new();
        let mut attempted = record(&identity, &attempted_session);
        attempted.agent_runtime_id = AgentRuntimeId::parse("rt:triage:new:1").unwrap();
        attempted.generation = ContinuityGeneration::new(1);
        store
            .upsert_continuity_record(&attempted, FencingToken::new(1))
            .await
            .unwrap();
        store
            .save_session_snapshot(
                &identity,
                &attempted_session,
                attempted.generation,
                CheckpointVersion::new(1),
                FencingToken::new(1),
                &SessionSnapshot { data: vec![30] },
            )
            .await
            .unwrap();

        store
            .rollback_continuity_record(&attempted, None, FencingToken::new(1))
            .await
            .unwrap();

        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .unwrap();
        assert_eq!(
            resolved.get(&identity),
            Some(&ContinuityResolveState::Uninitialized)
        );
        assert!(
            store
                .load_session_snapshot(&attempted_session)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn reset_rollback_cas_cannot_clobber_a_newer_attempt() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let previous_session = meerkat_core::types::SessionId::new();
        let previous = record(&identity, &previous_session);
        store
            .upsert_continuity_record(&previous, FencingToken::new(1))
            .await
            .unwrap();

        let attempted_session = meerkat_core::types::SessionId::new();
        let mut attempted = record(&identity, &attempted_session);
        attempted.agent_runtime_id = AgentRuntimeId::parse("rt:triage:main:1").unwrap();
        attempted.generation = ContinuityGeneration::new(1);
        store
            .upsert_continuity_record(&attempted, FencingToken::new(2))
            .await
            .unwrap();
        store
            .save_session_snapshot(
                &identity,
                &attempted_session,
                attempted.generation,
                CheckpointVersion::new(1),
                FencingToken::new(2),
                &SessionSnapshot { data: vec![40] },
            )
            .await
            .unwrap();

        let newer_session = meerkat_core::types::SessionId::new();
        let mut newer = record(&identity, &newer_session);
        newer.agent_runtime_id = AgentRuntimeId::parse("rt:triage:main:2").unwrap();
        newer.generation = ContinuityGeneration::new(2);
        store
            .upsert_continuity_record(&newer, FencingToken::new(3))
            .await
            .unwrap();

        let error = store
            .rollback_continuity_record(&attempted, Some(&previous), FencingToken::new(2))
            .await
            .expect_err("a stale reset attempt must not overwrite a newer generation");
        assert!(matches!(
            error,
            ContinuityStoreError::StaleFencingToken { .. }
        ));
        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .unwrap();
        assert_eq!(
            resolved.get(&identity),
            Some(&ContinuityResolveState::Ready { record: newer })
        );
        assert_eq!(
            store
                .load_session_snapshot(&attempted_session)
                .await
                .unwrap(),
            Some(SessionSnapshot { data: vec![40] })
        );
    }

    #[tokio::test]
    async fn max_fencing_token_recovers_high_water_across_tables_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("continuity.db");
        let identity = AgentIdentity::parse("identity:parent-1").unwrap();
        let session_id = meerkat_core::types::SessionId::new();
        {
            let store = LocalContinuityStore::open(&path).unwrap();
            assert_eq!(store.max_fencing_token().unwrap(), 0, "empty store -> 0");
            // First boot: continuity record + session snapshot both at token 1.
            store
                .upsert_continuity_record(&record(&identity, &session_id), FencingToken::new(1))
                .await
                .unwrap();
            store
                .save_session_snapshot(
                    &identity,
                    &session_id,
                    ContinuityGeneration::new(0),
                    CheckpointVersion::new(1),
                    FencingToken::new(1),
                    &SessionSnapshot {
                        data: vec![1, 2, 3],
                    },
                )
                .await
                .unwrap();
            // Reconcile re-bumps the continuity record to 15; the snapshot stays
            // at 1 — the two-table divergence from the field report.
            store
                .upsert_continuity_record(&record(&identity, &session_id), FencingToken::new(15))
                .await
                .unwrap();
            assert_eq!(
                store.max_fencing_token().unwrap(),
                15,
                "high-water = MAX over continuity_records (15) and session_snapshots (1)"
            );
        }
        // Restart: the high-water must survive re-opening the same db file.
        let store = LocalContinuityStore::open(&path).unwrap();
        assert_eq!(
            store.max_fencing_token().unwrap(),
            15,
            "high-water must persist across reopen"
        );

        // The session_snapshots arm of the union must actually count: a snapshot
        // whose token exceeds the continuity record (the crash-window case the
        // MAX-over-both-tables query is for) becomes the high-water.
        let snap_only = LocalContinuityStore::in_memory().unwrap();
        let sid = meerkat_core::types::SessionId::new();
        snap_only
            .save_session_snapshot(
                &identity,
                &sid,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(7),
                &SessionSnapshot { data: vec![9] },
            )
            .await
            .unwrap();
        assert_eq!(
            snap_only.max_fencing_token().unwrap(),
            7,
            "high-water must come from session_snapshots when no continuity record is present"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn async_open_keeps_tokio_worker_responsive_while_sqlite_is_locked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("async-open.db");
        let lock = Connection::open(&path).unwrap();
        lock.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; BEGIN IMMEDIATE;")
            .unwrap();

        let open = tokio::spawn({
            let path = path.clone();
            async move { LocalContinuityStore::open_with_fencing_floor(path).await }
        });
        tokio::time::timeout(
            std::time::Duration::from_millis(250),
            tokio::time::sleep(std::time::Duration::from_millis(25)),
        )
        .await
        .expect("the current-thread Tokio worker must remain responsive");
        assert!(
            !open.is_finished(),
            "schema initialization should still be waiting on the held SQLite writer lock"
        );

        lock.execute_batch("ROLLBACK;").unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), open)
            .await
            .expect("async open should finish after releasing the SQLite lock")
            .expect("open task should not panic")
            .expect("store should open and read its fencing floor");
    }

    /// The end-to-end restart regression: a lease provider seeded from the
    /// persisted high-water issues a token that the store accepts on restore,
    /// while a provider that reset to 1 (the v0.7.8 bug) is rejected as stale.
    #[tokio::test]
    async fn lease_fencing_resumes_above_high_water_on_restart() {
        use super::super::contracts::LeaseProvider;
        use super::super::local_lease::LocalLeaseProvider;
        use super::super::types::LeaseAcquireResult;

        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("identity:parent-1").unwrap();
        let session_id = meerkat_core::types::SessionId::new();
        // Pre-restart history: reconcile bumped the continuity record to 15.
        store
            .upsert_continuity_record(&record(&identity, &session_id), FencingToken::new(15))
            .await
            .unwrap();

        // Restart: seed a fresh lease provider from the persisted high-water.
        let high_water = store.max_fencing_token().unwrap();
        assert_eq!(high_water, 15);
        let provider = LocalLeaseProvider::with_floor(high_water);
        let acquired = provider
            .acquire_leases(std::slice::from_ref(&identity), "rt-restart")
            .await
            .unwrap();
        let token = match acquired.get(&identity) {
            Some(LeaseAcquireResult::Acquired(grant)) => grant.fencing_token,
            _ => panic!("expected an acquired lease"),
        };
        assert!(
            token.get() > high_water,
            "resumed token {} must exceed the high-water {high_water}",
            token.get()
        );
        // The restore upsert with the resumed token SUCCEEDS (not stale).
        store
            .upsert_continuity_record(&record(&identity, &session_id), token)
            .await
            .expect("a token resumed above the high-water must be accepted");

        // Prove the bug this fixes: a provider that reset to 1 IS rejected.
        let reset_provider = LocalLeaseProvider::with_floor(0);
        let reset_acquired = reset_provider
            .acquire_leases(std::slice::from_ref(&identity), "rt-reset")
            .await
            .unwrap();
        let reset_token = match reset_acquired.get(&identity) {
            Some(LeaseAcquireResult::Acquired(grant)) => grant.fencing_token,
            _ => panic!("expected an acquired lease"),
        };
        let err = store
            .upsert_continuity_record(&record(&identity, &session_id), reset_token)
            .await
            .expect_err("a reset-to-1 token must be rejected as stale");
        assert!(matches!(
            err,
            ContinuityStoreError::StaleFencingToken { .. }
        ));
    }

    #[tokio::test]
    async fn exact_snapshot_match_requires_the_current_continuity_head() {
        let store = LocalContinuityStore::in_memory().expect("in-memory store");
        let identity = AgentIdentity::parse("agent:exact-match").unwrap();
        let session_id = meerkat_core::types::SessionId::new();
        let snapshot = SessionSnapshot {
            data: vec![1, 3, 3, 7],
        };
        let continuity_record = record(&identity, &session_id);
        store
            .upsert_continuity_record(&continuity_record, FencingToken::new(1))
            .await
            .unwrap();
        store
            .save_session_snapshot(
                &identity,
                &session_id,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(1),
                &snapshot,
            )
            .await
            .unwrap();

        let candidate = SessionSnapshotMatchCandidate {
            identity: identity.clone(),
            session_id: session_id.clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(1),
            fencing_token: FencingToken::new(1),
            snapshot: Arc::new(snapshot),
        };
        assert!(
            store
                .session_snapshot_matches_current(candidate.clone())
                .await
                .unwrap(),
            "the complete durable provenance tuple and bytes should match"
        );

        store
            .upsert_continuity_record(&continuity_record, FencingToken::new(2))
            .await
            .unwrap();
        assert!(
            !store
                .session_snapshot_matches_current(candidate.clone())
                .await
                .unwrap(),
            "a stale presented write fence must not match a newer continuity head"
        );
        assert!(
            store
                .session_snapshot_matches_current(SessionSnapshotMatchCandidate {
                    fencing_token: FencingToken::new(2),
                    ..candidate
                })
                .await
                .unwrap(),
            "the historical row fence is provenance and may precede current write authority"
        );
    }

    #[tokio::test]
    async fn snapshot_save_rejects_another_identity_owning_the_session_id() {
        let store = LocalContinuityStore::in_memory().expect("in-memory store");
        let first = AgentIdentity::parse("agent:first-owner").unwrap();
        let second = AgentIdentity::parse("agent:second-owner").unwrap();
        let session_id = meerkat_core::types::SessionId::new();
        store
            .upsert_continuity_record(&record(&first, &session_id), FencingToken::new(1))
            .await
            .unwrap();
        store
            .save_session_snapshot(
                &first,
                &session_id,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(1),
                &SessionSnapshot { data: vec![1] },
            )
            .await
            .unwrap();
        store
            .upsert_continuity_record(&record(&second, &session_id), FencingToken::new(2))
            .await
            .unwrap();

        let error = store
            .save_session_snapshot(
                &second,
                &session_id,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(2),
                &SessionSnapshot { data: vec![2] },
            )
            .await
            .expect_err("a different identity must not overwrite the session row");
        assert!(matches!(error, ContinuityStoreError::Corruption(_)));
        assert_eq!(
            store.load_session_snapshot(&session_id).await.unwrap(),
            Some(SessionSnapshot { data: vec![1] })
        );
    }

    #[tokio::test]
    async fn snapshot_save_rejects_same_identity_from_another_generation_atomically() {
        let store = LocalContinuityStore::in_memory().expect("in-memory store");
        let identity = AgentIdentity::parse("agent:generation-owner").unwrap();
        let session_id = meerkat_core::types::SessionId::new();
        store
            .upsert_continuity_record(&record(&identity, &session_id), FencingToken::new(1))
            .await
            .unwrap();
        store
            .save_session_snapshot(
                &identity,
                &session_id,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(1),
                &SessionSnapshot { data: vec![1] },
            )
            .await
            .unwrap();

        let identity_for_update = identity.clone();
        store
            .run_blocking("advance-test-generation", move |inner| {
                inner.with_writer(|connection| {
                    connection
                        .execute(
                            "UPDATE continuity_records
                             SET generation = 1, checkpoint_version = 0, fencing_token = 2
                             WHERE identity = ?1",
                            rusqlite::params![identity_for_update.as_str()],
                        )
                        .map_err(|error| {
                            ContinuityStoreError::Io(format!(
                                "advance test continuity generation: {error}"
                            ))
                        })?;
                    Ok(())
                })
            })
            .await
            .unwrap();

        let error = store
            .save_session_snapshot(
                &identity,
                &session_id,
                ContinuityGeneration::new(1),
                CheckpointVersion::new(1),
                FencingToken::new(2),
                &SessionSnapshot { data: vec![2] },
            )
            .await
            .expect_err("a new generation must not overwrite the prior session row");
        assert!(matches!(error, ContinuityStoreError::Corruption(_)));
        assert_eq!(
            store.load_session_snapshot(&session_id).await.unwrap(),
            Some(SessionSnapshot { data: vec![1] }),
            "failed cross-generation save must leave snapshot bytes unchanged"
        );
        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .unwrap();
        let ContinuityResolveState::Ready { record } = resolved.get(&identity).unwrap() else {
            panic!("expected ready continuity head");
        };
        assert_eq!(record.generation, ContinuityGeneration::new(1));
        assert_eq!(record.checkpoint_version, CheckpointVersion::new(0));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_worker_keeps_the_async_executor_responsive() {
        let store = LocalContinuityStore::in_memory().expect("in-memory store");
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let work = tokio::spawn(async move {
            store
                .run_blocking("blocking-worker-test", move |_| {
                    let _ = started_tx.send(());
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    Ok(())
                })
                .await
        });

        started_rx.await.expect("blocking worker started");
        assert!(!work.is_finished(), "worker should still be sleeping");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(
            !work.is_finished(),
            "the current-thread executor should run while SQLite work is blocking"
        );
        work.await.expect("worker task joined").unwrap();
    }

    #[tokio::test]
    async fn file_backed_store_serves_reads_from_a_bounded_pool() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalContinuityStore::open(dir.path().join("read-pool.db")).unwrap();
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut tasks = Vec::with_capacity(READ_POOL_SIZE);

        for _ in 0..READ_POOL_SIZE {
            let store = store.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            let release = release.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .run_blocking("read-pool-test", move |inner| {
                        inner.with_reader(|_| {
                            let now = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                            max_active.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                            while !release.load(std::sync::atomic::Ordering::SeqCst) {
                                std::thread::sleep(std::time::Duration::from_millis(1));
                            }
                            active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                            Ok(())
                        })
                    })
                    .await
            }));
        }

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        while max_active.load(std::sync::atomic::Ordering::SeqCst) < READ_POOL_SIZE
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        release.store(true, std::sync::atomic::Ordering::SeqCst);
        for task in tasks {
            task.await.expect("read task joined").unwrap();
        }
        assert_eq!(
            max_active.load(std::sync::atomic::Ordering::SeqCst),
            READ_POOL_SIZE,
            "all bounded reader connections should be independently usable"
        );
    }

    #[tokio::test]
    async fn cloned_in_memory_store_shares_one_database() {
        let store = LocalContinuityStore::in_memory().expect("in-memory store");
        let reader = store.clone();
        let identity = AgentIdentity::parse("agent:shared-memory").unwrap();
        let session_id = meerkat_core::types::SessionId::new();
        store
            .upsert_continuity_record(&record(&identity, &session_id), FencingToken::new(1))
            .await
            .unwrap();

        let resolved = reader
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .unwrap();
        assert!(matches!(
            resolved.get(&identity),
            Some(ContinuityResolveState::Ready { .. })
        ));
    }

    // -----------------------------------------------------------------
    // M4b: the head-canonical session-delta channel
    // -----------------------------------------------------------------

    fn session_with(texts: &[&str]) -> Session {
        let mut session = Session::new();
        for text in texts {
            session.push(meerkat_core::Message::User(
                meerkat_core::UserMessage::text((*text).to_string()),
            ));
        }
        session
    }

    /// A ledgered-v1 corpus in the shape a real deployment actually has:
    /// the store's own open converges the DDL, the deferred stamp leaves the
    /// ledger at v1, and a blob row exists with no head row adopting it.
    fn plant_ledgered_v1_blob(path: &Path, identity: &AgentIdentity, session: &Session) {
        drop(LocalContinuityStore::open(path).expect("open converges schema"));
        let conn = Connection::open(path).expect("plant");
        conn.execute(
            "INSERT INTO session_snapshots (session_id, identity, generation, \
             checkpoint_version, fencing_token, data) VALUES (?1, ?2, 3, 5, 9, ?3)",
            rusqlite::params![
                session.id().to_string(),
                identity.as_str(),
                serde_json::to_vec(session).expect("encode session")
            ],
        )
        .expect("plant blob row");
    }

    fn continuity_domain_version(path: &Path) -> Option<i64> {
        let conn = Connection::open(path).expect("probe");
        meerkat_sqlite::domain_version(&conn, MOBKIT_CONTINUITY_DOMAIN.name)
            .expect("domain version")
    }

    /// Head rows, tolerating the table's absence — a real v1 corpus has NO
    /// head-canonical tables at all (they are created inside the delta
    /// write's transaction, not at open), so "missing table" is zero rows
    /// rather than an error.
    fn head_row_count(path: &Path) -> i64 {
        let conn = Connection::open(path).expect("probe");
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' \
                 AND name='continuity_session_heads'",
                [],
                |row| row.get(0),
            )
            .expect("probe head table");
        if exists == 0 {
            return 0;
        }
        conn.query_row("SELECT COUNT(*) FROM continuity_session_heads", [], |row| {
            row.get(0)
        })
        .expect("count head rows")
    }

    #[test]
    fn backfill_converts_a_ledgered_v1_corpus_and_stamps_only_on_complete_conversion() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("continuity.sqlite3");
        let identity = AgentIdentity::parse("triage:main").expect("identity");
        let session = session_with(&["hello", "world"]);
        plant_ledgered_v1_blob(&path, &identity, &session);

        // Fixture self-check: the deferred stamp means a converged file is
        // still v1. If this ever fails the fixture is not the shape under
        // test and every assertion below is meaningless.
        assert_eq!(
            continuity_domain_version(&path),
            Some(1),
            "fixture must be a LEDGERED v1 corpus, not a stamped one"
        );
        assert_eq!(head_row_count(&path), 0, "fixture must have no head row");

        // ---- dry run mutates nothing, including the ledger ----
        let dry = LocalContinuityStore::backfill_head_canonical_sessions_at(&path, false)
            .expect("dry run");
        assert_eq!(dry.pending_before, 1);
        assert!(!dry.applied);
        assert!(!dry.ledger_stamped);
        assert!(dry.converted.is_empty());
        assert_eq!(continuity_domain_version(&path), Some(1), "dry run stamped");
        assert_eq!(head_row_count(&path), 0, "dry run converted");

        // ---- apply converts, retains the blob, and stamps ----
        let applied =
            LocalContinuityStore::backfill_head_canonical_sessions_at(&path, true).expect("apply");
        assert_eq!(applied.converted.len(), 1);
        assert!(applied.failures.is_empty(), "{:?}", applied.failures);
        assert!(applied.complete());
        assert!(applied.ledger_stamped, "complete conversion must stamp");
        assert_eq!(head_row_count(&path), 1, "head row not created");
        assert_eq!(continuity_domain_version(&path), Some(2), "not stamped v2");

        // The blob is a frozen archive, never deleted.
        let conn = Connection::open(&path).expect("probe");
        let blobs: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_snapshots", [], |row| {
                row.get(0)
            })
            .expect("count blobs");
        assert_eq!(blobs, 1, "the legacy blob must be retained as an archive");

        // ---- re-running is idempotent and does not re-stamp ----
        let again = LocalContinuityStore::backfill_head_canonical_sessions_at(&path, true)
            .expect("second apply");
        assert_eq!(
            again.pending_before, 0,
            "already-converted session re-listed"
        );
        assert!(
            !again.ledger_stamped,
            "an empty run must not claim to have stamped"
        );
    }

    #[test]
    fn backfill_leaves_the_ledger_at_v1_when_a_session_cannot_convert() {
        // The whole point of deferring the stamp: a corpus that did not fully
        // cross keeps rollback to a pre-head-canonical release available.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("continuity.sqlite3");
        let identity = AgentIdentity::parse("triage:main").expect("identity");
        let session = session_with(&["ok"]);
        plant_ledgered_v1_blob(&path, &identity, &session);
        {
            // A second blob row that cannot decode into a Session.
            let conn = Connection::open(&path).expect("plant");
            conn.execute(
                "INSERT INTO session_snapshots (session_id, identity, generation, \
                 checkpoint_version, fencing_token, data) VALUES (?1, ?2, 3, 5, 9, X'6E6F7065')",
                rusqlite::params![
                    meerkat_core::types::SessionId::new().to_string(),
                    identity.as_str()
                ],
            )
            .expect("plant undecodable blob");
        }

        let applied =
            LocalContinuityStore::backfill_head_canonical_sessions_at(&path, true).expect("apply");
        assert_eq!(applied.pending_before, 2);
        assert!(!applied.failures.is_empty(), "undecodable blob must fail");
        assert!(!applied.complete());
        assert!(
            !applied.ledger_stamped,
            "a partial conversion must NOT stamp — rollback stays available"
        );
        assert_eq!(
            continuity_domain_version(&path),
            Some(1),
            "partial run must leave the file at v1"
        );
    }

    fn cursor(
        identity: &AgentIdentity,
        generation: u64,
        version: u64,
        token: u64,
    ) -> ContinuityWriteCursor {
        ContinuityWriteCursor {
            identity: identity.clone(),
            generation: ContinuityGeneration::new(generation),
            checkpoint_version: CheckpointVersion::new(version),
            fencing_token: FencingToken::new(token),
        }
    }

    fn ledger_version(path: &Path) -> Option<i64> {
        let probe = Connection::open(path).expect("probe");
        meerkat_sqlite::domain_version(&probe, "mobkit-continuity").expect("ledger")
    }

    async fn seed_record(
        store: &LocalContinuityStore,
        identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        token: u64,
    ) {
        store
            .upsert_continuity_record(&record(identity, session_id), FencingToken::new(token))
            .await
            .expect("seed continuity record");
    }

    /// BLOCKER PIN: opening a state directory with this binary must NOT
    /// commit the one-way ledger bump. Rollback to the previous release stays
    /// possible until a delta write actually creates a head row.
    #[tokio::test]
    async fn open_never_stamps_the_head_canonical_ledger_bump() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("continuity.sqlite3");
        {
            let store = LocalContinuityStore::open(&path).expect("open");
            assert_eq!(store.max_fencing_token().expect("floor"), 0);
        }
        assert_eq!(
            ledger_version(&path),
            Some(1),
            "a plain open must leave the file at the rollback-safe baseline"
        );

        // Reopening, resolving, whole-blob saving: still no bump.
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let session_id = meerkat_core::types::SessionId::new();
        {
            let store = LocalContinuityStore::open(&path).expect("reopen");
            seed_record(&store, &identity, &session_id, 1).await;
            store
                .save_session_snapshot(
                    &identity,
                    &session_id,
                    ContinuityGeneration::new(0),
                    CheckpointVersion::new(1),
                    FencingToken::new(1),
                    &SessionSnapshot {
                        data: serde_json::to_vec(&session_with(&["blob turn"])).unwrap(),
                    },
                )
                .await
                .expect("whole-blob save");
        }
        assert_eq!(
            ledger_version(&path),
            Some(1),
            "ordinary whole-document saves must not commit the head-canonical bump"
        );

        // The delta write that CREATES HEAD STATE is where the v1-writer
        // lockout becomes load-bearing, and only there is the bump
        // committed. An append that adopts nothing does not earn it — see
        // `an_accepted_delta_write_that_creates_no_head_state_stays_at_v1`.
        {
            let store = LocalContinuityStore::open(&path).expect("reopen");
            let head_session = session_with(&["delta turn"]);
            let delta_session_id = head_session.id().clone();
            let delta_identity = AgentIdentity::parse("triage:delta").unwrap();
            seed_record(&store, &delta_identity, &delta_session_id, 2).await;
            let root = TranscriptStrandId::root();
            store
                .append_messages(
                    &cursor(&delta_identity, 0, 1, 2),
                    &delta_session_id,
                    &root,
                    0,
                    head_session.messages(),
                )
                .await
                .expect("first delta write");
            assert_eq!(
                ledger_version(&path),
                Some(1),
                "an append that no head adopts creates no authority an older binary \
                 could misread, so it must not commit the lockout"
            );
            let head = SessionHead::from_session(&head_session, root, 0).expect("head");
            store
                .save_head(
                    &cursor(&delta_identity, 0, 2, 2),
                    &head,
                    SessionHeadCas::Create,
                )
                .await
                .expect("adopting head write");
        }
        assert_eq!(
            ledger_version(&path),
            Some(HEAD_CANONICAL_SCHEMA_VERSION),
            "the delta write that creates head state commits the head-canonical bump"
        );
    }

    /// ROLLBACK-SAFETY PIN (N1): the lockout must be earned by HEAD STATE,
    /// not merely by an accepted write.
    ///
    /// `append_messages` on a session with neither a head row nor a blob to
    /// migrate — the real creation-window shape, because the service appends
    /// and adopts under two separate locks — commits rows and no head. Rows
    /// no head adopts are not part of any document (every read path gates on
    /// the head row), so an older binary still correctly serves that session
    /// from its blob. Arming the one-way lockout there buys a brick for
    /// nothing.
    ///
    /// The second half is the trap the first half sets: the DDL DOES commit
    /// with those rows, so the file now has head-canonical tables at ledger
    /// v1. A binary that decided "is the bump owed?" from a table probe
    /// would latch "already head-canonical" on the next open and never stamp
    /// again — head rows with no lockout, which is the very state the
    /// lockout exists to prevent. The reopen below is what catches that.
    #[tokio::test]
    async fn an_accepted_delta_write_that_creates_no_head_state_stays_at_v1() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("continuity.sqlite3");
        let identity = AgentIdentity::parse("triage:append-only").unwrap();
        let document = session_with(&["creation-window turn"]);
        let session_id = document.id().clone();
        let root = TranscriptStrandId::root();

        {
            let store = LocalContinuityStore::open(&path).expect("open");
            seed_record(&store, &identity, &session_id, 1).await;
            store
                .append_messages(
                    &cursor(&identity, 0, 1, 1),
                    &session_id,
                    &root,
                    0,
                    document.messages(),
                )
                .await
                .expect("an append with no adopting head is a legitimate accepted write");
        }

        assert_eq!(
            ledger_version(&path),
            Some(1),
            "an ACCEPTED write that creates zero head state must leave the file \
             rollback-safe at v1"
        );
        assert!(
            head_tables_exist(&path),
            "the rows are durable, so their (additive, IF NOT EXISTS) DDL committed with them"
        );
        // The v1-shaped reader the lockout protects: it must still open.
        {
            let probe = Connection::open(&path).expect("probe");
            refuse_future_schema_model(&probe, &V0_8_5_CONTINUITY_DOMAIN).expect(
                "a previous release must still open a file whose only head-canonical \
                 content is rows no head adopts",
            );
        }
        {
            let probe = Connection::open(&path).expect("probe");
            let heads: i64 = probe
                .query_row("SELECT COUNT(*) FROM continuity_session_heads", [], |row| {
                    row.get(0)
                })
                .expect("count heads");
            assert_eq!(heads, 0, "no head row was created");
            let rows: i64 = probe
                .query_row(
                    "SELECT COUNT(*) FROM continuity_strand_messages",
                    [],
                    |row| row.get(0),
                )
                .expect("count rows");
            assert_eq!(rows, 1, "the appended row is durable");
        }

        // Reopen (a fresh handle, ledger v1, tables present) and land the
        // adopting head. This MUST still stamp.
        {
            let store = LocalContinuityStore::open(&path).expect("reopen");
            // A restore reads before it writes, and the read observes the
            // tables. That is precisely what latches the "head tables are
            // queryable" flag — which is NOT the same fact as "the lockout
            // is committed". A binary that conflated them would now be
            // convinced the bump is already done and never stamp again.
            assert!(
                store
                    .load_canonical_head(&session_id)
                    .await
                    .expect("canonical head probe")
                    .is_none(),
                "rows no head adopts are not a document"
            );
            let head = SessionHead::from_session(&document, root.clone(), 0).expect("head");
            store
                .save_head(&cursor(&identity, 0, 2, 1), &head, SessionHeadCas::Create)
                .await
                .expect("the adopting head write");
        }
        assert_eq!(
            ledger_version(&path),
            Some(HEAD_CANONICAL_SCHEMA_VERSION),
            "the write that finally creates head state must commit the lockout, even \
             though the tables already existed when the handle opened"
        );
        {
            let probe = Connection::open(&path).expect("probe");
            match refuse_future_schema_model(&probe, &V0_8_5_CONTINUITY_DOMAIN) {
                Err(meerkat_sqlite::SqliteStoreError::SchemaFromTheFuture { domain, .. }) => {
                    assert_eq!(domain, "mobkit-continuity");
                }
                other => panic!(
                    "once a head row exists the file must be closed to binaries that would keep \
                     writing the frozen blob archive as authority, got {other:?}"
                ),
            }
        }
    }

    fn head_tables_exist(path: &Path) -> bool {
        let probe = Connection::open(path).expect("probe");
        probe
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' \
                 AND name = 'continuity_session_heads')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .expect("probe head tables")
    }

    /// ROLLBACK-SAFETY PIN: the one-way v1-writer lockout must be EARNED by
    /// a write that actually creates head state. A delta write refused by a
    /// guard, or refused by the operation's own CAS, leaves the file exactly
    /// as it found it — ledger v1, no head-canonical tables — so rolling
    /// back to the previous release stays possible.
    #[tokio::test]
    async fn a_refused_delta_write_never_arms_the_v1_writer_lockout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("continuity.sqlite3");
        let identity = AgentIdentity::parse("triage:refused").unwrap();
        let document = session_with(&["refused turn"]);
        let session_id = document.id().clone();
        let root = TranscriptStrandId::root();
        let head = SessionHead::from_session(&document, root.clone(), 0).expect("head");

        {
            let store = LocalContinuityStore::open(&path).expect("open");
            seed_record(&store, &identity, &session_id, 5).await;

            // (1) refused by a GUARD, before the operation runs at all.
            let stale = store
                .append_messages(
                    &cursor(&identity, 0, 1, 4),
                    &session_id,
                    &root,
                    0,
                    document.messages(),
                )
                .await
                .expect_err("a stale fencing token must refuse the delta write");
            assert!(
                stale.to_string().contains("stale fencing token"),
                "unexpected guard refusal: {stale}"
            );

            // (2) refused by the OPERATION's own CAS, after the guards pass.
            let conflict = store
                .save_head(
                    &cursor(&identity, 0, 1, 5),
                    &head,
                    SessionHeadCas::IfToken("row-sha256:nothing-like-this".to_string()),
                )
                .await
                .expect_err("a head CAS that cannot match must refuse the delta write");
            assert!(
                matches!(
                    conflict,
                    SessionStoreError::TranscriptRevisionConflict { .. }
                ),
                "unexpected operation refusal: {conflict}"
            );
        }
        assert_eq!(
            ledger_version(&path),
            Some(1),
            "a REFUSED delta write must not arm the one-way v1-writer lockout"
        );
        assert!(
            !head_tables_exist(&path),
            "a refused delta write must roll its speculative head-canonical DDL back"
        );

        // The same file still upgrades on a write that DOES create head
        // state. The append alone does not: it adopts nothing, so it commits
        // its rows and its DDL and leaves the file rollback-safe (pinned by
        // `an_accepted_delta_write_that_creates_no_head_state_stays_at_v1`).
        // The head write is what earns the bump.
        {
            let store = LocalContinuityStore::open(&path).expect("reopen");
            store
                .append_messages(
                    &cursor(&identity, 0, 1, 5),
                    &session_id,
                    &root,
                    0,
                    document.messages(),
                )
                .await
                .expect("an accepted delta write");
            assert_eq!(
                ledger_version(&path),
                Some(1),
                "an accepted append that no head adopts is still rollback-safe"
            );
            store
                .save_head(&cursor(&identity, 0, 2, 5), &head, SessionHeadCas::Create)
                .await
                .expect("the adopting head write");
        }
        assert_eq!(
            ledger_version(&path),
            Some(HEAD_CANONICAL_SCHEMA_VERSION),
            "a write that creates head state arms the lockout in the same transaction"
        );
        assert!(head_tables_exist(&path));
    }

    /// ALIGNMENT PIN: the delta channel and the whole-document verb are two
    /// write paths onto ONE durable session, so their accept/reject boundary
    /// must be identical. A session whose `session_snapshots` row is owned by
    /// another `(identity, generation)` is refused by BOTH — the intruder
    /// here holds a perfectly valid continuity cursor of its own, so nothing
    /// but the shared ownership guard can reject it.
    #[tokio::test]
    async fn delta_writes_refuse_the_foreign_snapshot_owner_the_blob_path_refuses() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("continuity.sqlite3");
        let owner = AgentIdentity::parse("triage:owner").unwrap();
        let intruder = AgentIdentity::parse("triage:intruder").unwrap();
        let document = session_with(&["owned turn"]);
        let session_id = document.id().clone();
        let snapshot = SessionSnapshot {
            data: serde_json::to_vec(&document).unwrap(),
        };

        let store = LocalContinuityStore::open(&path).expect("open");
        seed_record(&store, &owner, &session_id, 1).await;
        store
            .save_session_snapshot(
                &owner,
                &session_id,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(1),
                &snapshot,
            )
            .await
            .expect("the owner's whole-document save");

        // The intruder's own continuity record points at the same session id
        // and is current, so the cursor guard passes for it.
        seed_record(&store, &intruder, &session_id, 2).await;

        let blob_refusal = store
            .save_session_snapshot(
                &intruder,
                &session_id,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(2),
                &snapshot,
            )
            .await
            .expect_err("the whole-document verb refuses a foreign snapshot owner");
        assert!(
            matches!(blob_refusal, ContinuityStoreError::Corruption(_)),
            "unexpected whole-document refusal: {blob_refusal}"
        );

        let delta_refusal = store
            .append_messages(
                &cursor(&intruder, 0, 1, 2),
                &session_id,
                &TranscriptStrandId::root(),
                0,
                document.messages(),
            )
            .await
            .expect_err("the delta channel must refuse exactly what the blob path refuses");
        assert!(
            delta_refusal.to_string().contains("is owned by")
                && delta_refusal.to_string().contains("triage:owner"),
            "the delta refusal must be the same ownership corruption: {delta_refusal}"
        );

        // Refused means refused: no rows, and no earned lockout either.
        drop(store);
        assert!(
            !head_tables_exist(&path),
            "a refused delta write must leave no head-canonical rows behind"
        );
        assert_eq!(ledger_version(&path), Some(1));
    }

    /// A v2 file reopens cleanly (never re-applied, never refused) and keeps
    /// serving head-canonical sessions.
    #[tokio::test]
    async fn head_canonical_file_reopens_and_keeps_serving_head_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("continuity.sqlite3");
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let document = session_with(&["one", "two"]);
        let session_id = document.id().clone();
        {
            let store = LocalContinuityStore::open(&path).expect("open");
            seed_record(&store, &identity, &session_id, 1).await;
            let root = TranscriptStrandId::root();
            store
                .append_messages(
                    &cursor(&identity, 0, 1, 1),
                    &session_id,
                    &root,
                    0,
                    document.messages(),
                )
                .await
                .expect("append");
            let head = SessionHead::from_session(&document, root, 0).expect("head");
            store
                .save_head(&cursor(&identity, 0, 2, 1), &head, SessionHeadCas::Create)
                .await
                .expect("save head");
        }
        assert_eq!(ledger_version(&path), Some(HEAD_CANONICAL_SCHEMA_VERSION));

        let store = LocalContinuityStore::open(&path).expect("reopen a head-canonical file");
        assert_eq!(ledger_version(&path), Some(HEAD_CANONICAL_SCHEMA_VERSION));
        let head = store
            .load_canonical_head(&session_id)
            .await
            .expect("load canonical head")
            .expect("head row survives reopen");
        assert_eq!(head.message_count, 2);
        let snapshot = store
            .load_session_snapshot(&session_id)
            .await
            .expect("snapshot")
            .expect("head-canonical sessions serve a synthesized snapshot");
        let loaded: Session = serde_json::from_slice(&snapshot.data).expect("decode");
        assert_eq!(loaded.messages(), document.messages());
    }

    /// Lazy per-session migration: the first delta write on a blob-only
    /// session converts it in the caller's transaction, the document loads
    /// back identically, and the blob row survives untouched as a frozen
    /// archive.
    #[tokio::test]
    async fn first_delta_write_migrates_the_blob_and_leaves_it_as_a_frozen_archive() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let document = session_with(&["one", "two"]);
        let session_id = document.id().clone();
        let blob = serde_json::to_vec(&document).unwrap();
        seed_record(&store, &identity, &session_id, 1).await;
        store
            .save_session_snapshot(
                &identity,
                &session_id,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(1),
                &SessionSnapshot { data: blob.clone() },
            )
            .await
            .unwrap();
        let before = store
            .load_session_snapshot(&session_id)
            .await
            .unwrap()
            .unwrap();

        // Read-only head synthesis must NOT migrate.
        let synthesized = store.load_head(&session_id).await.unwrap().unwrap();
        assert!(
            store
                .load_canonical_head(&session_id)
                .await
                .unwrap()
                .is_none(),
            "reads never migrate a blob-only session"
        );

        let mut extended = document.clone();
        extended.push(meerkat_core::Message::User(
            meerkat_core::UserMessage::text("three".to_string()),
        ));
        store
            .append_messages(
                &cursor(&identity, 0, 2, 1),
                &session_id,
                &synthesized.strand,
                synthesized.message_count,
                &extended.messages()[2..],
            )
            .await
            .expect("first delta write migrates");
        let migrated = store
            .load_canonical_head(&session_id)
            .await
            .unwrap()
            .expect("head row exists after the first delta write");
        assert_eq!(migrated.head_revision, synthesized.head_revision);
        assert_eq!(
            session_head_cas_token(&migrated).unwrap(),
            session_head_cas_token(&synthesized).unwrap(),
            "the deterministic layout makes the pre-migration token match the persisted one"
        );

        // The head still covers the pre-append prefix: the document is
        // unchanged until a head write adopts the appended rows.
        let after = store
            .load_session_snapshot(&session_id)
            .await
            .unwrap()
            .unwrap();
        let after_doc: Session = serde_json::from_slice(&after.data).unwrap();
        let before_doc: Session = serde_json::from_slice(&before.data).unwrap();
        assert_eq!(
            after_doc.messages(),
            before_doc.messages(),
            "unadopted tail rows are invisible to loads (the crash-window contract)"
        );

        // The archived blob row is byte-identical and never read again.
        let archived = store
            .run_blocking("read-archive", {
                let session_id = session_id.clone();
                move |inner| {
                    inner.with_reader(|connection| {
                        connection
                            .query_row(
                                "SELECT data FROM session_snapshots WHERE session_id = ?1",
                                rusqlite::params![session_id.to_string()],
                                |row| row.get::<_, Vec<u8>>(0),
                            )
                            .map_err(|e| sqlite_err("read archive", e))
                    })
                }
            })
            .await
            .unwrap();
        assert_eq!(archived, blob, "the archived blob must stay byte-identical");
    }

    /// A whole-document save on a head-canonical session converts into delta
    /// rows + a head and must NOT rewrite the frozen archive (the
    /// two-write-authorities tripwire).
    #[tokio::test]
    async fn whole_document_save_on_a_head_canonical_session_leaves_the_archive_untouched() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let document = session_with(&["one"]);
        let session_id = document.id().clone();
        let blob = serde_json::to_vec(&document).unwrap();
        seed_record(&store, &identity, &session_id, 1).await;
        store
            .save_session_snapshot(
                &identity,
                &session_id,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(1),
                &SessionSnapshot { data: blob.clone() },
            )
            .await
            .unwrap();
        // Migrate + adopt through the delta channel. The service's own flow:
        // `load_head` synthesizes deterministically from the blob, so the
        // token it derives is the one the migrating write persists and the
        // `IfToken` CAS matches.
        let head = store.load_head(&session_id).await.unwrap().unwrap();
        store
            .save_head(
                &cursor(&identity, 0, 2, 1),
                &head,
                SessionHeadCas::IfToken(session_head_cas_token(&head).unwrap()),
            )
            .await
            .expect("the pre-migration token matches the migrating write");
        store
            .save_head(&cursor(&identity, 0, 3, 1), &head, SessionHeadCas::Create)
            .await
            .expect_err("Create must conflict once the head row exists");

        let mut extended = document.clone();
        extended.push(meerkat_core::Message::User(
            meerkat_core::UserMessage::text("two".to_string()),
        ));
        store
            .save_session_snapshot(
                &identity,
                &session_id,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(4),
                FencingToken::new(1),
                &SessionSnapshot {
                    data: serde_json::to_vec(&extended).unwrap(),
                },
            )
            .await
            .expect("whole-document save converts on a head-canonical session");

        let served = store
            .load_session_snapshot(&session_id)
            .await
            .unwrap()
            .unwrap();
        let served_doc: Session = serde_json::from_slice(&served.data).unwrap();
        assert_eq!(served_doc.messages(), extended.messages());
        let archived = store
            .run_blocking("read-archive", {
                let session_id = session_id.clone();
                move |inner| {
                    inner.with_reader(|connection| {
                        connection
                            .query_row(
                                "SELECT data FROM session_snapshots WHERE session_id = ?1",
                                rusqlite::params![session_id.to_string()],
                                |row| row.get::<_, Vec<u8>>(0),
                            )
                            .map_err(|e| sqlite_err("read archive", e))
                    })
                }
            })
            .await
            .unwrap();
        assert_eq!(
            archived, blob,
            "a head-canonical write must never touch the frozen blob archive"
        );
    }

    /// Per-mutation continuity discipline: the delta verbs apply exactly the
    /// fence / version / binding CAS the whole-blob verb applies, and a
    /// refused mutation commits nothing.
    #[tokio::test]
    async fn delta_writes_enforce_fence_and_version_cas_per_mutation() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let document = session_with(&["one"]);
        let session_id = document.id().clone();
        let root = TranscriptStrandId::root();
        store
            .upsert_continuity_record(&record(&identity, &session_id), FencingToken::new(5))
            .await
            .unwrap();

        let stale_fence = store
            .append_messages(
                &cursor(&identity, 0, 1, 2),
                &session_id,
                &root,
                0,
                document.messages(),
            )
            .await
            .expect_err("a stale fencing token must be refused per append");
        assert!(
            stale_fence.to_string().contains("stale fencing token"),
            "unexpected error: {stale_fence}"
        );
        assert!(
            store
                .load_canonical_head(&session_id)
                .await
                .unwrap()
                .is_none(),
            "a refused delta write commits nothing"
        );

        store
            .append_messages(
                &cursor(&identity, 0, 1, 5),
                &session_id,
                &root,
                0,
                document.messages(),
            )
            .await
            .expect("a current fence is admitted");
        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .unwrap();
        let ContinuityResolveState::Ready { record: advanced } = &resolved[&identity] else {
            panic!("record must stay ready");
        };
        assert_eq!(
            advanced.checkpoint_version,
            CheckpointVersion::new(1),
            "the durable cursor advances atomically with the rows"
        );

        let stale_version = store
            .append_messages(
                &cursor(&identity, 0, 1, 5),
                &session_id,
                &root,
                1,
                document.messages(),
            )
            .await
            .expect_err("a non-advancing checkpoint version must be refused per append");
        assert!(
            stale_version
                .to_string()
                .contains("stale checkpoint version"),
            "unexpected error: {stale_version}"
        );

        let foreign = AgentIdentity::parse("triage:other").unwrap();
        let other_session = meerkat_core::types::SessionId::new();
        store
            .upsert_continuity_record(&record(&foreign, &other_session), FencingToken::new(6))
            .await
            .unwrap();
        let cross = store
            .append_messages(
                &cursor(&foreign, 0, 9, 6),
                &session_id,
                &root,
                1,
                document.messages(),
            )
            .await
            .expect_err("a foreign identity must not write another session's rows");
        assert!(
            cross.to_string().contains("not found") || cross.to_string().contains("owned by"),
            "unexpected error: {cross}"
        );
    }

    /// CAS delete over a head-canonical session: the token derives from the
    /// slim materialization and the delete scrubs head + strands + rewrites
    /// + the archive in one transaction.
    #[tokio::test]
    async fn cas_delete_over_head_canonical_rows_removes_every_table() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let document = session_with(&["one", "two"]);
        let session_id = document.id().clone();
        let root = TranscriptStrandId::root();
        seed_record(&store, &identity, &session_id, 1).await;
        store
            .append_messages(
                &cursor(&identity, 0, 1, 1),
                &session_id,
                &root,
                0,
                document.messages(),
            )
            .await
            .unwrap();
        let head = SessionHead::from_session(&document, root, 0).unwrap();
        store
            .save_head(&cursor(&identity, 0, 2, 1), &head, SessionHeadCas::Create)
            .await
            .unwrap();

        let snapshot = store
            .load_session_snapshot(&session_id)
            .await
            .unwrap()
            .unwrap();
        let served: Session = serde_json::from_slice(&snapshot.data).unwrap();
        let token = meerkat_core::session_store::session_projection_cas_token(&served).unwrap();
        assert!(
            !store
                .delete_session_snapshot_if_current_revision(&session_id, "row-sha256:stale")
                .await
                .unwrap(),
            "a stale token must decline"
        );
        assert!(
            store
                .delete_session_snapshot_if_current_revision(&session_id, &token)
                .await
                .unwrap(),
            "the token derived from head+rows must be accepted"
        );
        assert!(
            store
                .load_session_snapshot(&session_id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .load_canonical_head(&session_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    /// Reset rollback keeps the PRIOR generation's head+rows as the rollback
    /// authority and deletes only the attempted generation's.
    #[tokio::test]
    async fn rollback_scopes_head_canonical_rows_to_the_attempted_generation() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let previous_doc = session_with(&["previous"]);
        let previous_session = previous_doc.id().clone();
        let root = TranscriptStrandId::root();
        let mut previous = record(&identity, &previous_session);
        store
            .upsert_continuity_record(&previous, FencingToken::new(1))
            .await
            .unwrap();
        store
            .append_messages(
                &cursor(&identity, 0, 1, 1),
                &previous_session,
                &root,
                0,
                previous_doc.messages(),
            )
            .await
            .unwrap();
        let previous_head = SessionHead::from_session(&previous_doc, root.clone(), 0).unwrap();
        store
            .save_head(
                &cursor(&identity, 0, 2, 1),
                &previous_head,
                SessionHeadCas::Create,
            )
            .await
            .unwrap();
        previous.checkpoint_version = CheckpointVersion::new(2);

        let attempted_doc = session_with(&["attempted"]);
        let attempted_session = attempted_doc.id().clone();
        let mut attempted = record(&identity, &attempted_session);
        attempted.agent_runtime_id = AgentRuntimeId::parse("rt:triage:main:1").unwrap();
        attempted.generation = ContinuityGeneration::new(1);
        store
            .upsert_continuity_record(&attempted, FencingToken::new(2))
            .await
            .unwrap();
        store
            .append_messages(
                &cursor(&identity, 1, 1, 2),
                &attempted_session,
                &root,
                0,
                attempted_doc.messages(),
            )
            .await
            .unwrap();
        let attempted_head = SessionHead::from_session(&attempted_doc, root, 0).unwrap();
        store
            .save_head(
                &cursor(&identity, 1, 2, 2),
                &attempted_head,
                SessionHeadCas::Create,
            )
            .await
            .unwrap();

        store
            .rollback_continuity_record(&attempted, Some(&previous), FencingToken::new(2))
            .await
            .expect("rollback");

        assert!(
            store
                .load_canonical_head(&attempted_session)
                .await
                .unwrap()
                .is_none(),
            "the attempted generation's head+rows are abandoned"
        );
        let restored = store
            .load_canonical_head(&previous_session)
            .await
            .unwrap()
            .expect("the prior generation stays the rollback authority");
        assert_eq!(restored.head_revision, previous_head.head_revision);
        let served = store
            .load_session_snapshot(&previous_session)
            .await
            .unwrap()
            .expect("the restored session still loads");
        let doc: Session = serde_json::from_slice(&served.data).unwrap();
        assert_eq!(doc.messages(), previous_doc.messages());
    }

    /// Identity deletion scrubs all four tables atomically, and the fencing
    /// floor spans the head table so the lease provider never regresses.
    #[tokio::test]
    async fn identity_delete_scrubs_head_rows_and_the_floor_spans_the_head_table() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let document = session_with(&["one"]);
        let session_id = document.id().clone();
        let root = TranscriptStrandId::root();
        seed_record(&store, &identity, &session_id, 1).await;
        store
            .append_messages(
                &cursor(&identity, 0, 1, 9),
                &session_id,
                &root,
                0,
                document.messages(),
            )
            .await
            .unwrap();
        let head = SessionHead::from_session(&document, root, 0).unwrap();
        store
            .save_head(&cursor(&identity, 0, 2, 9), &head, SessionHeadCas::Create)
            .await
            .unwrap();
        assert_eq!(
            store.max_fencing_token().unwrap(),
            9,
            "the head table participates in the fencing floor"
        );

        store
            .delete_continuity_record(&identity, FencingToken::new(10))
            .await
            .unwrap();
        assert!(
            store
                .load_canonical_head(&session_id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .load_session_snapshot(&session_id)
                .await
                .unwrap()
                .is_none()
        );
        let remaining_rows = store
            .run_blocking("count-strands", move |inner| {
                inner.with_reader(|connection| {
                    connection
                        .query_row(
                            "SELECT COUNT(*) FROM continuity_strand_messages",
                            [],
                            |row| row.get::<_, i64>(0),
                        )
                        .map_err(|e| sqlite_err("count strands", e))
                })
            })
            .await
            .unwrap();
        assert_eq!(
            remaining_rows, 0,
            "strand rows are scrubbed with the identity"
        );
    }

    /// The exact-bytes no-op probe is a blob-authority concept: on a
    /// head-canonical session it declines so the caller takes its ordinary
    /// guard path.
    #[tokio::test]
    async fn exact_snapshot_probe_declines_for_head_canonical_sessions() {
        let store = LocalContinuityStore::in_memory().unwrap();
        let identity = AgentIdentity::parse("triage:main").unwrap();
        let document = session_with(&["one"]);
        let session_id = document.id().clone();
        let root = TranscriptStrandId::root();
        seed_record(&store, &identity, &session_id, 1).await;
        store
            .append_messages(
                &cursor(&identity, 0, 1, 1),
                &session_id,
                &root,
                0,
                document.messages(),
            )
            .await
            .unwrap();
        let head = SessionHead::from_session(&document, root, 0).unwrap();
        store
            .save_head(&cursor(&identity, 0, 2, 1), &head, SessionHeadCas::Create)
            .await
            .unwrap();

        let snapshot = store
            .load_session_snapshot(&session_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !store
                .session_snapshot_matches_current(SessionSnapshotMatchCandidate {
                    identity,
                    session_id,
                    generation: ContinuityGeneration::new(0),
                    checkpoint_version: CheckpointVersion::new(2),
                    fencing_token: FencingToken::new(1),
                    snapshot: Arc::new(snapshot),
                })
                .await
                .unwrap(),
            "head-canonical sessions must decline the whole-blob byte probe"
        );
    }

    // ----------------------------------------------------------------
    // Previous-release model: what a v1-shaped binary sees in the file.
    // ----------------------------------------------------------------

    /// The v0.8.5 continuity schema domain, declared LOCALLY on purpose.
    ///
    /// Not `MOBKIT_CONTINUITY_BASELINE_DOMAIN`: that constant is this
    /// binary's internal staging device and is free to grow. What these
    /// tests must model is the previous RELEASE's version ceiling, which is
    /// frozen at 1 forever. Declaring it here means a future migration
    /// cannot quietly raise the bar the "old binary" is held to and turn
    /// these tests green for the wrong reason.
    const V0_8_5_CONTINUITY_DOMAIN: meerkat_sqlite::SchemaDomain = meerkat_sqlite::SchemaDomain {
        name: "mobkit-continuity",
        migrations: &[meerkat_sqlite::Migration {
            version: 1,
            name: "base-schema",
            apply: migration_0001_continuity_schema,
        }],
        initialize_current: migration_0001_continuity_schema,
        allowed_existing_versions: &[1],
        released_predecessors: &[],
        owned_objects: RELEASED_V1_CONTINUITY_OBJECTS,
        retired_objects: &[],
    };

    /// Local model of the retired `meerkat_sqlite::refuse_future_schema`
    /// check the previous release ran at open: read the ledger row, refuse
    /// when it exceeds the modeled version ceiling.
    fn refuse_future_schema_model(
        conn: &Connection,
        domain: &meerkat_sqlite::SchemaDomain,
    ) -> Result<(), meerkat_sqlite::SqliteStoreError> {
        let supported = domain.supported_version();
        match meerkat_sqlite::domain_version(conn, domain.name)? {
            Some(found) if found > supported => {
                Err(meerkat_sqlite::SqliteStoreError::SchemaFromTheFuture {
                    domain: domain.name.to_string(),
                    found,
                    supported,
                })
            }
            _ => Ok(()),
        }
    }

    /// A stand-in for a release that predates the head-canonical channel:
    /// it supports `mobkit-continuity` up to v1 and reads sessions ONLY from
    /// `session_snapshots.data`.
    ///
    /// `refuse_future_schema` against that ceiling is exactly the check such
    /// a binary runs at open (the head-canonical migration simply does not
    /// exist in it), so this reproduces the real `SchemaFromTheFuture`
    /// lockout without needing the old binary on disk.
    fn v1_shaped_binary_opens(path: &Path) -> Result<(), meerkat_sqlite::SqliteStoreError> {
        let conn = Connection::open(path).expect("probe");
        refuse_future_schema_model(&conn, &V0_8_5_CONTINUITY_DOMAIN)
    }

    /// N1-INTERACTION PIN: keeping the file at v1 for an append that adopts
    /// nothing is what makes rollback possible — and it is also what lets
    /// ORPHAN strand rows outlive a rollback.
    ///
    /// Sequence: an append lands, the adopting head write never does (crash,
    /// or the operator rolls back mid-creation-window). The previous release
    /// can now open the file — that is the whole point — and it writes
    /// whole-document blobs, including ones that DIVERGE from the orphan
    /// rows (a compaction, a rewind). Re-upgrading then migrates that blob
    /// into head+rows over the orphans.
    ///
    /// Before this was handled, `insert_strand_rows_in_txn` refused the
    /// divergence as an immutability violation and the session became
    /// permanently unwritable. The migration clears orphans first: they are
    /// unreachable by construction (every read path gates on the head row),
    /// and the blob is the authority being migrated.
    #[tokio::test]
    async fn re_upgrading_over_orphan_rows_that_diverge_from_the_blob_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("continuity.sqlite3");
        let identity = AgentIdentity::parse("triage:orphans").unwrap();
        let root = TranscriptStrandId::root();
        let interrupted = session_with(&["draft turn that was never adopted"]);
        let session_id = interrupted.id().clone();

        // 1. An append with no adopting head: rows land, ledger stays v1.
        {
            let store = LocalContinuityStore::open(&path).expect("open");
            seed_record(&store, &identity, &session_id, 1).await;
            store
                .append_messages(
                    &cursor(&identity, 0, 1, 1),
                    &session_id,
                    &root,
                    0,
                    interrupted.messages(),
                )
                .await
                .expect("append");
        }
        assert_eq!(ledger_version(&path), Some(1));
        v1_shaped_binary_opens(&path).expect("rollback is possible — that is the point");

        // 2. The previous release writes a whole-document blob that diverges
        //    from the orphan rows at seq 0.
        let divergent = {
            let store = LocalContinuityStore::open(&path).expect("reopen");
            // The same session id carrying a transcript that differs from
            // the orphan rows at seq 0 — a compaction, say.
            let rebuilt = rebuild_with_messages(
                &interrupted,
                vec![meerkat_core::Message::User(
                    meerkat_core::UserMessage::text("post-rollback compaction".to_string()),
                )],
            );
            store
                .save_session_snapshot(
                    &identity,
                    &session_id,
                    ContinuityGeneration::new(0),
                    CheckpointVersion::new(2),
                    FencingToken::new(1),
                    &SessionSnapshot {
                        data: serde_json::to_vec(&rebuilt).unwrap(),
                    },
                )
                .await
                .expect("post-rollback whole-blob save");
            rebuilt
        };

        // 3. Re-upgrade: the first delta write migrates the divergent blob.
        {
            let store = LocalContinuityStore::open(&path).expect("reopen");
            let mut next = divergent.clone();
            next.push(meerkat_core::Message::User(
                meerkat_core::UserMessage::text("turn after re-upgrade".to_string()),
            ));
            let base = divergent.messages().len() as u64;
            store
                .append_messages(
                    &cursor(&identity, 0, 3, 1),
                    &session_id,
                    &root,
                    base,
                    &next.messages()[base as usize..],
                )
                .await
                .expect(
                    "the migrating append must clear orphan rows the blob diverges from, \
                     not refuse the session forever",
                );
            let migrated = store
                .load_canonical_head(&session_id)
                .await
                .expect("head")
                .expect("the blob migrated into head+rows");
            assert_eq!(
                migrated.message_count,
                divergent.messages().len() as u64,
                "the migrated head describes the BLOB, which is the authority"
            );
            let rows = store
                .load_messages(&session_id, &migrated.strand, 0..migrated.message_count)
                .await
                .expect("rows");
            assert_eq!(
                rows,
                divergent.messages(),
                "the orphan rows must be gone, replaced by the blob's transcript"
            );
        }
    }

    /// Rebuild a session document on the SAME id with a different transcript.
    fn rebuild_with_messages(source: &Session, messages: Vec<meerkat_core::Message>) -> Session {
        let mut head =
            SessionHead::from_session(source, TranscriptStrandId::root(), 0).expect("head");
        head.message_count = messages.len() as u64;
        head.head_revision = meerkat_core::transcript_messages_digest(&messages).expect("digest");
        // meerkat 0.8.11: `SessionHead::into_session` verifies the byte-exact
        // row-prefix commitment against the rows it materializes (mirrors
        // meerkat-core `SessionHead::into_session_with_serialized_rows`), so
        // the fabricated head must commit to the REPLACEMENT rows. The
        // source-derived lineage anchor cannot describe them (its fields are
        // store-private), so it is cleared - `None` is the accepted
        // unactivated shape at materialization.
        let serialized = messages
            .iter()
            .map(|message| serde_json::to_vec(message).expect("serialize replacement row"))
            .collect::<Vec<_>>();
        head.message_row_prefix = Some(
            meerkat_core::session_store::SessionMessageRowPrefixAccumulator::empty()
                .extend_serialized_rows(&serialized)
                .expect("recommit row prefix"),
        );
        head.row_lineage_anchor = None;
        head.into_session(messages).expect("rebuild")
    }
}

/// One legacy blob session awaiting head-canonical conversion.
///
/// Every value the conversion needs already lives in the blob row, so the
/// offline driver needs no external state and no running runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLegacySession {
    pub(crate) session_id: String,
    pub(crate) identity: String,
    pub(crate) generation: i64,
    pub(crate) checkpoint_version: i64,
    pub(crate) fencing_token: i64,
}

/// Outcome of an offline head-canonical backfill.
///
/// `converted.len() == pending_before` with empty `failures`/`vanished` is
/// the only shape that stamps the ledger; every other shape leaves the file
/// at v1 and rollback available.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeadCanonicalBackfillReport {
    /// Sessions holding a legacy blob with no head row, before the run.
    pub pending_before: usize,
    /// Session ids that now carry a head row because of this run.
    pub converted: Vec<String>,
    /// Sessions whose blob disappeared between census and conversion.
    pub vanished: Vec<String>,
    /// `(session_id, error)`; an empty session id is a run-level refusal.
    pub failures: Vec<(String, String)>,
    /// True only when the whole corpus crossed in this run.
    pub ledger_stamped: bool,
    /// False for a dry run, which mutates nothing including the DDL.
    pub applied: bool,
}

impl HeadCanonicalBackfillReport {
    /// True when the corpus is wholly head-canonical after this run.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.failures.is_empty()
            && self.vanished.is_empty()
            && self.converted.len() == self.pending_before
    }
}

/// Sessions with a legacy blob row and no head row.
///
/// Returns every blob session when the head table does not exist yet, which
/// is the ordinary v1 shape — a dry run must be able to report the pending
/// count without applying the DDL first.
fn pending_head_canonical_sessions(
    conn: &Connection,
) -> Result<Vec<PendingLegacySession>, ContinuityStoreError> {
    let heads_exist: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='continuity_session_heads'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| sqlite_err("probe head-canonical table", e))?
        > 0;
    let sql = if heads_exist {
        "SELECT s.session_id, s.identity, s.generation, s.checkpoint_version, s.fencing_token \
         FROM session_snapshots s \
         LEFT JOIN continuity_session_heads h ON h.session_id = s.session_id \
         WHERE h.session_id IS NULL \
         ORDER BY s.session_id"
    } else {
        "SELECT session_id, identity, generation, checkpoint_version, fencing_token \
         FROM session_snapshots ORDER BY session_id"
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| sqlite_err("prepare pending legacy session census", e))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PendingLegacySession {
                session_id: row.get(0)?,
                identity: row.get(1)?,
                generation: row.get(2)?,
                checkpoint_version: row.get(3)?,
                fencing_token: row.get(4)?,
            })
        })
        .map_err(|e| sqlite_err("query pending legacy sessions", e))?;
    let mut pending = Vec::new();
    for row in rows {
        pending.push(row.map_err(|e| sqlite_err("read pending legacy session", e))?);
    }
    Ok(pending)
}

/// Convert one legacy blob session in its own transaction.
///
/// `Ok(false)` means the blob was gone by the time the transaction opened —
/// reported rather than silently counted, so a complete-conversion claim
/// cannot be made about a corpus that changed under the fence.
fn backfill_one_session(
    conn: &mut Connection,
    candidate: &PendingLegacySession,
) -> Result<bool, ContinuityStoreError> {
    let session_id = meerkat_core::types::SessionId::parse(&candidate.session_id)
        .map_err(|e| ContinuityStoreError::Io(format!("malformed session id in blob row: {e}")))?;
    let identity = AgentIdentity::parse(&candidate.identity)
        .map_err(|e| ContinuityStoreError::Io(format!("malformed identity in blob row: {e}")))?;
    let tx = conn
        .transaction()
        .map_err(|e| sqlite_err("begin legacy session backfill", e))?;
    // SQLite hands these back as i64. A negative stamp is a corrupt row, not
    // a value to wrap around silently — refuse it and let the report name the
    // session rather than converting it against a fabricated stamp.
    let stamp = |label: &str, value: i64| -> Result<u64, ContinuityStoreError> {
        u64::try_from(value).map_err(|_| {
            ContinuityStoreError::Io(format!(
                "negative {label} ({value}) in blob row for session {}",
                candidate.session_id
            ))
        })
    };
    let generation = stamp("generation", candidate.generation)?;
    let checkpoint_version = stamp("checkpoint_version", candidate.checkpoint_version)?;
    let fencing_token = stamp("fencing_token", candidate.fencing_token)?;
    let migrated = migrate_legacy_blob_in_txn(
        &tx,
        &session_id,
        &identity,
        ContinuityGeneration::new(generation),
        CheckpointVersion::new(checkpoint_version),
        FencingToken::new(fencing_token),
    )
    .map_err(|e| ContinuityStoreError::Io(format!("head-canonical conversion failed: {e}")))?;
    tx.commit()
        .map_err(|e| sqlite_err("commit legacy session backfill", e))?;
    Ok(migrated.is_some())
}
