//! Bundled SQLite-backed ContinuityStore for `persistent_state(path)` usage.
//!
//! Implements CONTRACT-06. Designed for single-process, local-disk persistence.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension};

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
        let conn = self
            .conn
            .lock()
            .map_err(|e| ContinuityStoreError::Io(format!("lock: {e}")))?;
        conn.query_row(
            "SELECT COALESCE(MAX(t), 0) FROM (
                SELECT MAX(fencing_token) AS t FROM continuity_records
                UNION ALL
                SELECT MAX(fencing_token) AS t FROM session_snapshots
            )",
            [],
            |row| row.get::<_, u64>(0),
        )
        .map_err(|e| ContinuityStoreError::Io(format!("max_fencing_token: {e}")))
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

    async fn delete_session_snapshot_if_current_revision(
        &self,
        session_id: &meerkat_core::types::SessionId,
        expected_current_revision: &str,
    ) -> Result<bool, ContinuityStoreError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| ContinuityStoreError::Io(format!("lock: {e}")))?;
        let tx = conn
            .transaction()
            .map_err(|e| ContinuityStoreError::Io(format!("begin tx: {e}")))?;

        let data = tx
            .query_row(
                "SELECT data FROM session_snapshots WHERE session_id = ?1",
                rusqlite::params![session_id.to_string()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|e| ContinuityStoreError::Io(format!("query snapshot: {e}")))?;

        let Some(data) = data else {
            return Ok(false);
        };
        let session: meerkat_core::Session = serde_json::from_slice(&data).map_err(|e| {
            ContinuityStoreError::Io(format!(
                "deserialize session snapshot for revision check: {e}"
            ))
        })?;
        let current_revision = meerkat_core::session_store::session_projection_cas_token(&session)
            .map_err(|e| ContinuityStoreError::Io(e.to_string()))?;
        if current_revision != expected_current_revision {
            return Ok(false);
        }

        let deleted = tx
            .execute(
                "DELETE FROM session_snapshots WHERE session_id = ?1",
                rusqlite::params![session_id.to_string()],
            )
            .map_err(|e| ContinuityStoreError::Io(format!("delete snapshot: {e}")))?;
        tx.commit()
            .map_err(|e| ContinuityStoreError::Io(format!("commit snapshot delete: {e}")))?;
        Ok(deleted > 0)
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

        // Check fencing token and checkpoint version against the current
        // continuity record for this identity/generation stream. The session
        // id must match the current binding, but a rebind does not reset the
        // generation-scoped checkpoint counter.
        let mut stmt = tx
            .prepare_cached(
                "SELECT session_id, generation, fencing_token, checkpoint_version
                 FROM continuity_records WHERE identity = ?1",
            )
            .map_err(|e| ContinuityStoreError::Io(format!("prepare: {e}")))?;
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
            .map_err(|e| ContinuityStoreError::Io(format!("query: {e}")))?;

        // Drop the statement before further operations on the transaction
        drop(stmt);

        let record_was_present = existing.is_some();
        if let Some((current_session_id, current_generation, current_token, current_version)) =
            existing
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

        // Update the continuity fence and checkpoint version. A snapshot write
        // with a newer fencing token must advance the durable record fence;
        // otherwise an older owner can still pass a later write.
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
        .map_err(|e| ContinuityStoreError::Io(format!("update continuity after snapshot: {e}")))?;
        if record_was_present && tx.changes() == 0 {
            return Err(ContinuityStoreError::NotFound {
                identity: identity.clone(),
            });
        }

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
                checkpoint_version = CASE
                    WHEN continuity_records.session_id = excluded.session_id
                     AND continuity_records.generation = excluded.generation
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

        // Wrap the fence check and BOTH deletes in a single transaction so a
        // crash or I/O error between the two DELETEs cannot leave snapshots
        // gone but the continuity record present (a half-deleted store). Mirrors
        // the multi-statement consistency discipline of save_session_snapshot.
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| ContinuityStoreError::Io(format!("begin tx: {e}")))?;

        // Check fencing token against existing record
        let mut stmt = tx
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
        tx.execute(
            "DELETE FROM session_snapshots WHERE identity = ?1",
            rusqlite::params![identity.as_str()],
        )
        .map_err(|e| ContinuityStoreError::Io(format!("delete snapshots: {e}")))?;

        // Delete the continuity record
        tx.execute(
            "DELETE FROM continuity_records WHERE identity = ?1",
            rusqlite::params![identity.as_str()],
        )
        .map_err(|e| ContinuityStoreError::Io(format!("delete record: {e}")))?;

        tx.commit()
            .map_err(|e| ContinuityStoreError::Io(format!("commit tx: {e}")))?;

        Ok(())
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
}
