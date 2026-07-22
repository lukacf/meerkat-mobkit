//! In-crate reference store implementations.
//!
//! These exist for two reasons, mirroring the upstream harness's
//! `EmulatedCasSessionStore`: they prove every chapter is satisfiable (a
//! chapter no store can pass pins nothing), and they document the minimal
//! semantics a remote backend must implement.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use async_trait::async_trait;
use meerkat_core::session_store::session_projection_cas_token;
use meerkat_core::types::SessionId;
use meerkat_mobkit::identity_first::{
    AgentIdentity, ContinuityGeneration, ContinuityRecord, ContinuityResolveState, ContinuityStore,
    ContinuityStoreError, FencingToken, SessionSnapshot,
};
use meerkat_mobkit::unified_runtime::{EventLogError, EventLogStore, EventQuery, PersistedEvent};

// ---------------------------------------------------------------------------
// CompatRollbackContinuityStore
// ---------------------------------------------------------------------------

struct StoredRecord {
    record: ContinuityRecord,
    fence: u64,
}

struct StoredSnapshot {
    identity: AgentIdentity,
    generation: ContinuityGeneration,
    data: Vec<u8>,
}

#[derive(Default)]
struct ContinuityInner {
    records: BTreeMap<String, StoredRecord>,
    snapshots: BTreeMap<String, StoredSnapshot>,
}

/// Minimal in-memory `ContinuityStore` that deliberately does NOT override
/// `rollback_continuity_record`, so the trait's non-atomic compatibility
/// default is what executes. Its required methods follow the same CAS
/// discipline `LocalContinuityStore` enforces: fence compare-and-set
/// (monotonic `>=`), strictly-increasing checkpoint versions per save, and
/// generation-monotonic upserts.
#[derive(Default)]
pub struct CompatRollbackContinuityStore {
    inner: Mutex<ContinuityInner>,
}

impl CompatRollbackContinuityStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ContinuityInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait]
impl ContinuityStore for CompatRollbackContinuityStore {
    async fn resolve_many(
        &self,
        identities: &[AgentIdentity],
    ) -> Result<BTreeMap<AgentIdentity, ContinuityResolveState>, ContinuityStoreError> {
        let inner = self.lock();
        Ok(identities
            .iter()
            .map(|identity| {
                let state = inner.records.get(identity.as_str()).map_or(
                    ContinuityResolveState::Uninitialized,
                    |stored| ContinuityResolveState::Ready {
                        record: stored.record.clone(),
                    },
                );
                (identity.clone(), state)
            })
            .collect())
    }

    async fn load_session_snapshot(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
        let inner = self.lock();
        Ok(inner
            .snapshots
            .get(&session_id.to_string())
            .map(|snapshot| SessionSnapshot {
                data: snapshot.data.clone(),
            }))
    }

    async fn delete_session_snapshot_if_current_revision(
        &self,
        session_id: &SessionId,
        expected_current_revision: &str,
    ) -> Result<bool, ContinuityStoreError> {
        let mut inner = self.lock();
        let key = session_id.to_string();
        let Some(snapshot) = inner.snapshots.get(&key) else {
            return Ok(false);
        };
        let session: meerkat_core::Session = serde_json::from_slice(&snapshot.data)
            .map_err(|error| ContinuityStoreError::Io(format!("deserialize snapshot: {error}")))?;
        let current = session_projection_cas_token(&session)
            .map_err(|error| ContinuityStoreError::Io(format!("cas token: {error}")))?;
        if current != expected_current_revision {
            return Ok(false);
        }
        inner.snapshots.remove(&key);
        Ok(true)
    }

    async fn save_session_snapshot(
        &self,
        identity: &AgentIdentity,
        session_id: &SessionId,
        generation: ContinuityGeneration,
        version: meerkat_mobkit::identity_first::CheckpointVersion,
        fencing_token: FencingToken,
        snapshot: &SessionSnapshot,
    ) -> Result<(), ContinuityStoreError> {
        let mut inner = self.lock();
        if let Some(stored) = inner.records.get(identity.as_str()) {
            if stored.record.session_id != *session_id || stored.record.generation != generation {
                return Err(ContinuityStoreError::NotFound {
                    identity: identity.clone(),
                });
            }
            if fencing_token.get() < stored.fence {
                return Err(ContinuityStoreError::StaleFencingToken {
                    identity: identity.clone(),
                    presented: fencing_token,
                    current: FencingToken::new(stored.fence),
                });
            }
            if version.get() <= stored.record.checkpoint_version.get() {
                return Err(ContinuityStoreError::StaleCheckpointVersion {
                    identity: identity.clone(),
                    presented: version,
                    current: stored.record.checkpoint_version,
                });
            }
        }
        let key = session_id.to_string();
        if let Some(existing) = inner.snapshots.get(&key)
            && (existing.identity != *identity || existing.generation != generation)
        {
            return Err(ContinuityStoreError::Corruption(format!(
                "snapshot {session_id} already owned by {}/generation {}",
                existing.identity, existing.generation
            )));
        }
        inner.snapshots.insert(
            key,
            StoredSnapshot {
                identity: identity.clone(),
                generation,
                data: snapshot.data.clone(),
            },
        );
        if let Some(stored) = inner.records.get_mut(identity.as_str()) {
            stored.record.checkpoint_version = version;
            stored.fence = fencing_token.get();
        }
        Ok(())
    }

    async fn upsert_continuity_record(
        &self,
        record: &ContinuityRecord,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let mut inner = self.lock();
        let key = record.identity.as_str().to_string();
        if let Some(stored) = inner.records.get(&key) {
            if fencing_token.get() < stored.fence {
                return Err(ContinuityStoreError::StaleFencingToken {
                    identity: record.identity.clone(),
                    presented: fencing_token,
                    current: FencingToken::new(stored.fence),
                });
            }
            if record.generation.get() < stored.record.generation.get() {
                return Err(ContinuityStoreError::StaleContinuityGeneration {
                    identity: record.identity.clone(),
                    presented: record.generation,
                    current: stored.record.generation,
                });
            }
            // A same-generation rebind must not rewind the version stream; a
            // generation advance resets it (the excluded version wins).
            let mut next = record.clone();
            if record.generation == stored.record.generation {
                next.checkpoint_version = stored
                    .record
                    .checkpoint_version
                    .max(record.checkpoint_version);
            }
            inner.records.insert(
                key,
                StoredRecord {
                    record: next,
                    fence: fencing_token.get(),
                },
            );
        } else {
            inner.records.insert(
                key,
                StoredRecord {
                    record: record.clone(),
                    fence: fencing_token.get(),
                },
            );
        }
        Ok(())
    }

    async fn delete_continuity_record(
        &self,
        identity: &AgentIdentity,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let mut inner = self.lock();
        if let Some(stored) = inner.records.get(identity.as_str())
            && fencing_token.get() < stored.fence
        {
            return Err(ContinuityStoreError::StaleFencingToken {
                identity: identity.clone(),
                presented: fencing_token,
                current: FencingToken::new(stored.fence),
            });
        }
        inner.records.remove(identity.as_str());
        inner
            .snapshots
            .retain(|_, snapshot| snapshot.identity != *identity);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ReferenceEventLogStore
// ---------------------------------------------------------------------------

/// Minimal id-idempotent in-memory `EventLogStore`.
///
/// MobKit bundles no durable production `EventLogStore` (the built-in default
/// is a private null store that drops every event), so this reference
/// implementation both proves the [`crate::chapters::event_log`] chapter is
/// satisfiable and documents the store-side redelivery contract the MobKit
/// flusher's retry loop relies on.
#[derive(Default)]
pub struct ReferenceEventLogStore {
    events: Mutex<BTreeMap<String, PersistedEvent>>,
}

impl ReferenceEventLogStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EventLogStore for ReferenceEventLogStore {
    fn append_batch(
        &self,
        events: Vec<PersistedEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<(), EventLogError>> + Send + '_>> {
        Box::pin(async move {
            let mut map = self
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for event in events {
                // Documented contract: duplicate events (same `id`) are
                // ignored, keeping redelivery exactly-once.
                map.entry(event.id.clone()).or_insert(event);
            }
            Ok(())
        })
    }

    fn query(
        &self,
        query: EventQuery,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PersistedEvent>, EventLogError>> + Send + '_>> {
        Box::pin(async move {
            let map = self
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut rows: Vec<PersistedEvent> = map.values().cloned().collect();
            rows.sort_by_key(|event| event.seq);
            if let Some(after) = query.after_seq {
                rows.retain(|event| event.seq > after);
            }
            if let Some(limit) = query.limit {
                rows.truncate(limit);
            }
            Ok(rows)
        })
    }
}
