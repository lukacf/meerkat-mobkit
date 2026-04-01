//! Bundled SQLite-backed ContinuityStore for `persistent_state(path)` usage.
//!
//! Implements CONTRACT-06. Designed for single-process, local-disk persistence.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use super::contracts::ContinuityStore;
use super::types::{
    AgentIdentity, AgentRuntimeId, CheckpointVersion, ContinuityGeneration, ContinuityRecord,
    ContinuityResolveState, ContinuityStoreError, FencingToken, SessionSnapshot,
};

/// SQLite-backed ContinuityStore for the bundled `persistent_state(path)` path.
///
/// Stores ContinuityRecords and SessionSnapshots in a single SQLite database.
/// Enforces compare-and-set on (fencing_token, checkpoint_version).
pub struct LocalContinuityStore {
    conn: Mutex<Connection>,
}

impl LocalContinuityStore {
    /// Open (or create) a local continuity store at the given path.
    ///
    /// # Errors
    ///
    /// Returns `ContinuityStoreError::Io` if the database cannot be opened or
    /// the schema cannot be initialized.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ContinuityStoreError> {
        let conn =
            Connection::open(path).map_err(|e| ContinuityStoreError::Io(format!("open: {e}")))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| ContinuityStoreError::Io(format!("pragma: {e}")))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS continuity_records (
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
            );",
        )
        .map_err(|e| ContinuityStoreError::Io(format!("schema: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory store (for testing).
    ///
    /// # Errors
    ///
    /// Returns `ContinuityStoreError::Io` if initialization fails.
    pub fn in_memory() -> Result<Self, ContinuityStoreError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| ContinuityStoreError::Io(format!("in-memory open: {e}")))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS continuity_records (
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
            );",
        )
        .map_err(|e| ContinuityStoreError::Io(format!("schema: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

#[async_trait]
impl ContinuityStore for LocalContinuityStore {
    async fn resolve_many(
        &self,
        identities: &[AgentIdentity],
    ) -> Result<BTreeMap<AgentIdentity, ContinuityResolveState>, ContinuityStoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ContinuityStoreError::Io(format!("lock: {e}")))?;
        let mut map = BTreeMap::new();
        for id in identities {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT agent_runtime_id, session_id, generation, checkpoint_version
                     FROM continuity_records WHERE identity = ?1",
                )
                .map_err(|e| ContinuityStoreError::Io(format!("prepare: {e}")))?;
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
                .map_err(|e| ContinuityStoreError::Io(format!("query: {e}")))?;
            match row {
                Some((runtime_id, session_id_str, generation, cpv)) => {
                    let record = ContinuityRecord {
                        identity: id.clone(),
                        agent_runtime_id: AgentRuntimeId::parse(&runtime_id).map_err(|e| {
                            ContinuityStoreError::Corruption(format!(
                                "invalid runtime_id in store: {e}"
                            ))
                        })?,
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
    }

    async fn load_session_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ContinuityStoreError::Io(format!("lock: {e}")))?;
        let mut stmt = conn
            .prepare_cached("SELECT data FROM session_snapshots WHERE session_id = ?1")
            .map_err(|e| ContinuityStoreError::Io(format!("prepare: {e}")))?;
        let row = stmt
            .query_row(rusqlite::params![session_id.to_string()], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .optional()
            .map_err(|e| ContinuityStoreError::Io(format!("query: {e}")))?;
        Ok(row.map(|data| SessionSnapshot { data }))
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
        let conn = self
            .conn
            .lock()
            .map_err(|e| ContinuityStoreError::Io(format!("lock: {e}")))?;

        // Wrap the entire check-upsert-update in a single transaction so that
        // a crash between the snapshot write and the version bump cannot leave
        // the store in an inconsistent state.
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| ContinuityStoreError::Io(format!("begin tx: {e}")))?;

        // Check fencing token and checkpoint version against the continuity record
        let mut stmt = tx
            .prepare_cached(
                "SELECT fencing_token, checkpoint_version FROM continuity_records WHERE identity = ?1",
            )
            .map_err(|e| ContinuityStoreError::Io(format!("prepare: {e}")))?;
        let existing = stmt
            .query_row(rusqlite::params![identity.as_str()], |row| {
                Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?))
            })
            .optional()
            .map_err(|e| ContinuityStoreError::Io(format!("query: {e}")))?;

        // Drop the statement before further operations on the transaction
        drop(stmt);

        if let Some((current_token, current_version)) = existing {
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

        // Upsert the snapshot
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
        .map_err(|e| ContinuityStoreError::Io(format!("upsert snapshot: {e}")))?;

        // Update the checkpoint version in the continuity record
        tx.execute(
            "UPDATE continuity_records SET checkpoint_version = ?1 WHERE identity = ?2",
            rusqlite::params![version.get(), identity.as_str()],
        )
        .map_err(|e| ContinuityStoreError::Io(format!("update cpv: {e}")))?;

        tx.commit()
            .map_err(|e| ContinuityStoreError::Io(format!("commit tx: {e}")))?;

        Ok(())
    }

    async fn upsert_continuity_record(
        &self,
        record: &ContinuityRecord,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ContinuityStoreError::Io(format!("lock: {e}")))?;

        // Check fencing token against existing record
        let mut stmt = conn
            .prepare_cached("SELECT fencing_token FROM continuity_records WHERE identity = ?1")
            .map_err(|e| ContinuityStoreError::Io(format!("prepare: {e}")))?;
        let existing_token = stmt
            .query_row(rusqlite::params![record.identity.as_str()], |row| {
                row.get::<_, u64>(0)
            })
            .optional()
            .map_err(|e| ContinuityStoreError::Io(format!("query: {e}")))?;

        if let Some(current) = existing_token
            && fencing_token.get() < current
        {
            return Err(ContinuityStoreError::StaleFencingToken {
                identity: record.identity.clone(),
                presented: fencing_token,
                current: FencingToken::new(current),
            });
        }

        conn.execute(
            "INSERT INTO continuity_records (identity, agent_runtime_id, session_id, generation, checkpoint_version, fencing_token)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(identity) DO UPDATE SET
                agent_runtime_id = excluded.agent_runtime_id,
                session_id = excluded.session_id,
                generation = excluded.generation,
                checkpoint_version = excluded.checkpoint_version,
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
        .map_err(|e| ContinuityStoreError::Io(format!("upsert record: {e}")))?;

        Ok(())
    }

    async fn delete_continuity_record(
        &self,
        identity: &AgentIdentity,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ContinuityStoreError::Io(format!("lock: {e}")))?;

        // Check fencing token against existing record
        let mut stmt = conn
            .prepare_cached("SELECT fencing_token FROM continuity_records WHERE identity = ?1")
            .map_err(|e| ContinuityStoreError::Io(format!("prepare: {e}")))?;
        let existing_token = stmt
            .query_row(rusqlite::params![identity.as_str()], |row| {
                row.get::<_, u64>(0)
            })
            .optional()
            .map_err(|e| ContinuityStoreError::Io(format!("query: {e}")))?;

        // Drop the statement before further operations
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

        // Delete associated session snapshots
        conn.execute(
            "DELETE FROM session_snapshots WHERE identity = ?1",
            rusqlite::params![identity.as_str()],
        )
        .map_err(|e| ContinuityStoreError::Io(format!("delete snapshots: {e}")))?;

        // Delete the continuity record
        conn.execute(
            "DELETE FROM continuity_records WHERE identity = ?1",
            rusqlite::params![identity.as_str()],
        )
        .map_err(|e| ContinuityStoreError::Io(format!("delete record: {e}")))?;

        Ok(())
    }
}

/// Extension trait for optional row helpers.
trait OptionalRow {
    type Output;
    fn optional(self) -> Result<Option<Self::Output>, rusqlite::Error>;
}

impl<T> OptionalRow for Result<T, rusqlite::Error> {
    type Output = T;
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
