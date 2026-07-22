//! Bundled SQLite-backed ContinuityStore for `persistent_state(path)` usage.
//!
//! Implements CONTRACT-06. Designed for single-process, local-disk persistence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension};

use super::contracts::{ContinuityStore, SessionSnapshotMatchCandidate};
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

/// The continuity store's schema domain in the per-file migration ledger.
/// Migration 0001 is the historical two-table DDL (all `CREATE ... IF NOT
/// EXISTS`, so a pre-ledger file converges without its rows being touched).
pub(crate) const MOBKIT_CONTINUITY_DOMAIN: meerkat_sqlite::SchemaDomain =
    meerkat_sqlite::SchemaDomain {
        name: "mobkit-continuity",
        migrations: &[meerkat_sqlite::Migration {
            version: 1,
            name: "base-schema",
            apply: migration_0001_continuity_schema,
        }],
    };

fn migration_0001_continuity_schema(tx: &rusqlite::Transaction<'_>) -> Result<(), rusqlite::Error> {
    tx.execute_batch(SCHEMA)
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
}

impl LocalContinuityStoreInner {
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
        meerkat_sqlite::apply_domain_migrations(&mut writer, &MOBKIT_CONTINUITY_DOMAIN)
            .map_err(|e| mechanics_err("apply schema", e))?;

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

    /// Open an in-memory store (for testing).
    ///
    /// # Errors
    ///
    /// Returns `ContinuityStoreError::Io` if initialization fails.
    pub fn in_memory() -> Result<Self, ContinuityStoreError> {
        let mut writer =
            Connection::open_in_memory().map_err(|e| sqlite_err("in-memory open", e))?;
        meerkat_sqlite::apply_domain_migrations(&mut writer, &MOBKIT_CONTINUITY_DOMAIN)
            .map_err(|e| mechanics_err("apply schema", e))?;
        Ok(Self {
            inner: Arc::new(LocalContinuityStoreInner {
                db_path: PathBuf::from(":memory:"),
                writer: Mutex::new(writer),
                readers: ReadConnections::Writer,
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
            connection
                .query_row(
                    "SELECT COALESCE(MAX(t), 0) FROM (
                        SELECT MAX(fencing_token) AS t FROM continuity_records
                        UNION ALL
                        SELECT MAX(fencing_token) AS t FROM session_snapshots
                    )",
                    [],
                    |row| row.get::<_, u64>(0),
                )
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

    async fn load_session_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
        let session_id = session_id.clone();
        self.run_blocking("load_session_snapshot", move |inner| {
            inner.with_reader(|connection| {
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
                    let tx = connection
                        .transaction()
                        .map_err(|e| sqlite_err("begin tx", e))?;

                    let data = tx
                        .query_row(
                            "SELECT data FROM session_snapshots WHERE session_id = ?1",
                            rusqlite::params![session_id.to_string()],
                            |row| row.get::<_, Vec<u8>>(0),
                        )
                        .optional()
                        .map_err(|e| sqlite_err("query snapshot", e))?;

                    let Some(data) = data else {
                        return Ok(false);
                    };
                    let session: meerkat_core::Session =
                        serde_json::from_slice(&data).map_err(|e| {
                            ContinuityStoreError::Io(format!(
                                "deserialize session snapshot for revision check: {e}"
                            ))
                        })?;
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
                    tx.commit()
                        .map_err(|e| sqlite_err("commit snapshot delete", e))?;
                    Ok(deleted > 0)
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
                // Keep the check, snapshot upsert, and record version/fence
                // advance in one writer transaction.
                let tx = connection
                    .unchecked_transaction()
                    .map_err(|e| sqlite_err("begin tx", e))?;

                let mut stmt = tx
                    .prepare_cached(
                        "SELECT session_id, generation, fencing_token, checkpoint_version
                         FROM continuity_records WHERE identity = ?1",
                    )
                    .map_err(|e| sqlite_err("prepare", e))?;
                let existing = stmt
                    .query_row(rusqlite::params![identity.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, u64>(1)?,
                            row.get::<_, u64>(2)?,
                            row.get::<_, u64>(3)?,
                        ))
                    })
                    .optional()
                    .map_err(|e| sqlite_err("query", e))?;
                drop(stmt);

                let record_was_present = existing.is_some();
                if let Some((
                    current_session_id,
                    current_generation,
                    current_token,
                    current_version,
                )) = existing
                {
                    if current_session_id != session_id.to_string()
                        || current_generation != generation.get()
                    {
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

                let existing_snapshot_owner = tx
                    .query_row(
                        "SELECT identity, generation FROM session_snapshots WHERE session_id = ?1",
                        rusqlite::params![session_id.to_string()],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
                    )
                    .optional()
                    .map_err(|e| sqlite_err("query snapshot owner", e))?;
                if let Some((snapshot_identity, snapshot_generation)) = existing_snapshot_owner
                    && (snapshot_identity != identity.as_str()
                        || snapshot_generation != generation.get())
                {
                    return Err(ContinuityStoreError::Corruption(format!(
                        "session snapshot {session_id} is owned by {snapshot_identity}/generation \
                         {snapshot_generation}, not {identity}/generation {generation}"
                    )));
                }

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
                .map_err(|e| sqlite_err("update continuity after snapshot", e))?;
                if record_was_present && tx.changes() == 0 {
                    return Err(ContinuityStoreError::NotFound {
                        identity: identity.clone(),
                    });
                }

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
                // snapshots remain the rollback authority for the restored
                // row, while a concurrently advanced generation is protected
                // by the exact CAS above.
                tx.execute(
                    "DELETE FROM session_snapshots WHERE identity = ?1 AND generation = ?2",
                    rusqlite::params![
                        expected_attempt.identity.as_str(),
                        expected_attempt.generation.get(),
                    ],
                )
                .map_err(|e| sqlite_err("delete attempted snapshots", e))?;

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

    async fn delete_continuity_record(
        &self,
        identity: &AgentIdentity,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let identity = identity.clone();
        self.run_blocking("delete_continuity_record", move |inner| {
            inner.with_writer(|connection| {
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

    #[tokio::test]
    async fn legacy_file_opens_converges_and_preserves_rows() {
        // A pre-ledger file (historical two-table DDL, no meerkat_schema
        // table) must open, be stamped, and keep its rows readable.
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

        let store = LocalContinuityStore::open(&path).expect("open legacy");
        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .expect("resolve");
        match resolved.get(&identity) {
            Some(ContinuityResolveState::Ready { record }) => {
                assert_eq!(record.generation.get(), 3);
                assert_eq!(record.checkpoint_version.get(), 5);
            }
            other => panic!("legacy record must survive the port: {other:?}"),
        }
        assert_eq!(
            store.max_fencing_token().expect("floor"),
            9,
            "fencing floor spans both legacy tables"
        );
        let probe = Connection::open(&path).expect("probe");
        assert_eq!(
            meerkat_sqlite::domain_version(&probe, "mobkit-continuity").expect("ledger"),
            Some(1)
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
}
