//! Compatibility adapters bridging legacy MobKit traits to identity-first contracts.
//!
//! - [`DiscoveryRosterAdapter`]: `Discovery` → `RosterProvider` (CONTRACT-08, REQ-27, REQ-28)
//! - [`EdgeDiscoveryTopologyAdapter`]: `EdgeDiscovery` → `TopologyProvider` (CONTRACT-09, REQ-29)
//! - [`ContinuitySessionStoreAdapter`]: `ContinuityStore` → `SessionStore` (CONTRACT-10)
//! - [`SessionHookCustomizerAdapter`]: `SessionHook` → `AgentCustomizer` (CONTRACT-11, REQ-30)

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;

use super::contracts::{
    AgentCustomizer, RosterProvider, SessionSnapshotMatchCandidate, TopologyProvider,
};
use super::types::{
    AgentAddressability, AgentBuildContext, AgentBuildDraft, AgentIdentity, ContinuityStoreError,
    CustomizerError, DurableAgentSpec, ManagedPeerEdge, RosterContext, RosterError,
    TopologyContext, TopologyError,
};
use crate::mob_handle_runtime::{SessionCreatedContext, SessionHook};
use crate::types::AgentDiscoverySpec;
use crate::unified_runtime::edge_types::{Discovery, EdgeDiscovery};

// ---------------------------------------------------------------------------
// CONTRACT-08 / REQ-27 / REQ-28: Discovery → RosterProvider
// ---------------------------------------------------------------------------

/// Adapts a legacy `Discovery` trait impl into a `RosterProvider`.
///
/// In-process mutable roster: the desired-identity list for hosts where the
/// roster is operator-driven rather than app-provided (the identity-first
/// console gateway: seeded from init params, extended by
/// `mobkit/ensure_member`, shrunk by identity deletion). `roster()` returns a
/// snapshot; mutations take effect on the next reconcile
/// (`restore_flow` / `mobkit/reconcile_identity` / the Broken-identity
/// repair task).
#[derive(Default)]
pub struct MutableRosterProvider {
    roster: std::sync::RwLock<Vec<DurableAgentSpec>>,
}

impl MutableRosterProvider {
    pub fn new(initial: Vec<DurableAgentSpec>) -> Self {
        Self {
            roster: std::sync::RwLock::new(initial),
        }
    }

    /// Insert or replace (by identity) a desired spec.
    pub fn upsert(&self, spec: DurableAgentSpec) {
        let mut roster = self
            .roster
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match roster
            .iter_mut()
            .find(|entry| entry.identity == spec.identity)
        {
            Some(entry) => *entry = spec,
            None => roster.push(spec),
        }
    }

    /// Remove an identity from the desired roster. Returns whether it was
    /// present. Removal does NOT retire the live identity — reconcile owns
    /// convergence.
    pub fn remove(&self, identity: &AgentIdentity) -> bool {
        let mut roster = self
            .roster
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = roster.len();
        roster.retain(|entry| &entry.identity != identity);
        roster.len() != before
    }

    pub fn snapshot(&self) -> Vec<DurableAgentSpec> {
        self.roster
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[async_trait::async_trait]
impl RosterProvider for MutableRosterProvider {
    async fn roster(&self, _context: &RosterContext) -> Result<Vec<DurableAgentSpec>, RosterError> {
        Ok(self.snapshot())
    }
}

/// Maps `AgentDiscoverySpec` to `DurableAgentSpec` per REQ-27:
/// - `meerkat_id` → `identity` (parsed as `AgentIdentity`)
/// - `profile` → `profile`
/// - `labels` → `labels`
/// - `context` → `context`
/// - `additional_instructions` → `additional_instructions`
/// - `resume_session_id` → ignored
/// - `addressability` → `Addressable`
/// - `display_name` → `None`
pub struct DiscoveryRosterAdapter {
    inner: Box<dyn Discovery>,
}

impl DiscoveryRosterAdapter {
    pub fn new(discovery: impl Discovery + 'static) -> Self {
        Self {
            inner: Box::new(discovery),
        }
    }
}

/// Convert an `AgentDiscoverySpec` to a `DurableAgentSpec` per REQ-27.
pub fn agent_discovery_to_durable(
    spec: &AgentDiscoverySpec,
) -> Result<DurableAgentSpec, RosterError> {
    let identity = AgentIdentity::parse(&spec.meerkat_id)
        .map_err(|e| RosterError::Io(format!("invalid meerkat_id: {e}")))?;
    Ok(DurableAgentSpec {
        identity,
        profile: meerkat_mob::ProfileName::from(spec.profile.as_str()),
        addressability: AgentAddressability::Addressable,
        display_name: None,
        labels: spec.labels.clone().unwrap_or_default(),
        context: spec.context.clone(),
        additional_instructions: spec.additional_instructions.clone(),
        initial_message: None,
        runtime_mode_override: None,
        backend: None,
        binding: None,
    })
}

#[async_trait]
impl RosterProvider for DiscoveryRosterAdapter {
    async fn roster(&self, _context: &RosterContext) -> Result<Vec<DurableAgentSpec>, RosterError> {
        let specs = self.inner.discover(serde_json::Value::Null).await;
        specs.iter().map(agent_discovery_to_durable).collect()
    }
}

// ---------------------------------------------------------------------------
// CONTRACT-09 / REQ-29: EdgeDiscovery → TopologyProvider
// ---------------------------------------------------------------------------

/// Adapts a legacy `EdgeDiscovery` trait impl into a `TopologyProvider`.
///
/// Parses `DesiredPeerEdge` endpoint strings as `AgentIdentity` to produce
/// `ManagedPeerEdge` instances.
pub struct EdgeDiscoveryTopologyAdapter {
    inner: Box<dyn EdgeDiscovery>,
}

impl EdgeDiscoveryTopologyAdapter {
    pub fn new(edge_discovery: impl EdgeDiscovery + 'static) -> Self {
        Self {
            inner: Box::new(edge_discovery),
        }
    }
}

#[async_trait]
impl TopologyProvider for EdgeDiscoveryTopologyAdapter {
    async fn compute_edges(
        &self,
        _target_identities: &[AgentIdentity],
        context: &TopologyContext,
    ) -> Result<Vec<ManagedPeerEdge>, TopologyError> {
        // Project the roster context to EdgeMemberView so legacy EdgeDiscovery
        // impls see real member identities/labels instead of an empty vec.
        let member_views: Vec<crate::unified_runtime::edge_types::EdgeMemberView> = context
            .roster
            .iter()
            .map(|spec| crate::unified_runtime::edge_types::EdgeMemberView {
                agent_identity: spec.identity.as_str().to_string(),
                role: spec.profile.as_str().to_string(),
                wired_to: std::collections::BTreeSet::new(),
                labels: spec.labels.clone(),
            })
            .collect();

        let desired_edges = self.inner.discover_edges(member_views).await;
        let mut edges = Vec::with_capacity(desired_edges.len());
        for edge in &desired_edges {
            let (a_str, b_str) = edge.endpoints();
            let a = AgentIdentity::parse(a_str)
                .map_err(|e| TopologyError::InvalidEdge(format!("endpoint {a_str:?}: {e}")))?;
            let b = AgentIdentity::parse(b_str)
                .map_err(|e| TopologyError::InvalidEdge(format!("endpoint {b_str:?}: {e}")))?;
            let managed = ManagedPeerEdge::new(a, b)
                .map_err(|e| TopologyError::InvalidEdge(format!("{e}")))?;
            edges.push(managed);
        }
        Ok(edges)
    }
}

// ---------------------------------------------------------------------------
// CONTRACT-10: ContinuityStore → SessionStore adapter
// ---------------------------------------------------------------------------

/// Runtime state for a session, used by the adapter to resolve identity/fencing.
#[derive(Clone)]
pub(crate) struct SessionRuntimeState {
    pub identity: AgentIdentity,
    pub generation: super::types::ContinuityGeneration,
    pub fencing_token: super::types::FencingToken,
    pub checkpoint_version: super::types::CheckpointVersion,
}

/// Adapts a `ContinuityStore` to the Meerkat `SessionStore` interface.
///
/// On the external-authoritative path, this is the session persistence layer.
/// No separate local SQLite is created under scratch_dir — the ContinuityStore
/// is the single authoritative session truth.
///
/// The adapter maintains a session→identity registry populated by the runtime
/// during restore/activate. This maps SessionId to the owning identity's
/// continuity parameters (identity, generation, fencing_token).
pub struct ContinuitySessionStoreAdapter {
    store: Arc<dyn super::contracts::ContinuityStore>,
    /// Per-session monotonic version counter to satisfy CAS on repeated saves.
    versions: Mutex<HashMap<String, AtomicU64>>,
    /// Session→identity mapping, populated by the runtime.
    session_registry: Mutex<HashMap<String, SessionRuntimeState>>,
    /// Session saves that arrive before the bridge can publish the owning
    /// identity. These are flushed immediately when the session is registered.
    pending_unregistered: Mutex<HashMap<String, Vec<u8>>>,
    /// Sessions that were explicitly unregistered. Later writes from those
    /// actors must fail closed instead of becoming pre-registration pending
    /// snapshots for a future session with the same id.
    unregistered_sessions: Mutex<HashSet<String>>,
    /// Sessions whose persistence authority is temporarily quiesced while the
    /// identity runtime rotates an external fencing grant. Unlike permanent
    /// unregistration, suspension preserves the registry/version state so a
    /// successful publication can resume the same session at the new token.
    suspended_sessions: Mutex<HashSet<String>>,
    /// Sessions deliberately superseded by a committed destructive reset.
    /// Their old physical member is already quiescing, so archive projection
    /// writes are acknowledged without crossing into the replacement
    /// identity generation. The exact snapshot is CAS-deleted before this
    /// marker is published and ordinary unregistration clears it after the
    /// retained Mob cleanup anchor reaches structural absence.
    superseded_sessions: Mutex<HashSet<String>>,
    /// Per-session serialization for registry, pending, version, and durable
    /// load/guard/write transitions. Weak values let inactive locks be reclaimed
    /// without ever creating a second lock while an operation or waiter exists.
    session_locks: Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
}

impl ContinuitySessionStoreAdapter {
    pub fn new(store: Arc<dyn super::contracts::ContinuityStore>) -> Self {
        Self {
            store,
            versions: Mutex::new(HashMap::new()),
            session_registry: Mutex::new(HashMap::new()),
            pending_unregistered: Mutex::new(HashMap::new()),
            unregistered_sessions: Mutex::new(HashSet::new()),
            suspended_sessions: Mutex::new(HashSet::new()),
            superseded_sessions: Mutex::new(HashSet::new()),
            session_locks: Mutex::new(HashMap::new()),
        }
    }

    fn session_lock(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Arc<tokio::sync::Mutex<()>> {
        const PRUNE_THRESHOLD: usize = 1_024;

        let key = session_id.to_string();
        let mut locks = self
            .session_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            return lock;
        }
        if locks.len() >= PRUNE_THRESHOLD {
            locks.retain(|_, lock| lock.strong_count() > 0);
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(key, Arc::downgrade(&lock));
        lock
    }

    async fn lock_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.session_lock(session_id).lock_owned().await
    }

    /// Register a session with its owning identity's runtime state.
    ///
    /// Called by the runtime during restore/activate to wire real
    /// identity/generation/fencing data into the adapter.
    #[allow(dead_code)]
    pub(crate) async fn register_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
        state: SessionRuntimeState,
    ) -> Result<super::types::CheckpointVersion, meerkat_store::SessionStoreError> {
        let _guard = self.lock_session(session_id).await;
        let session_key = session_id.to_string();
        let checkpoint_version = state.checkpoint_version.get();
        let was_unregistered = self
            .unregistered_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&session_key);
        let was_suspended = self
            .suspended_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&session_key);
        let was_superseded = self
            .superseded_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&session_key);
        let previous_registry = {
            let mut registry = self
                .session_registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            registry.insert(session_key.clone(), state.clone())
        };
        let previous_version = {
            let mut versions = self
                .versions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let counter = versions
                .entry(session_key.clone())
                .or_insert_with(|| AtomicU64::new(checkpoint_version));
            let previous_version = counter.load(Ordering::Relaxed);
            counter.fetch_max(checkpoint_version, Ordering::Relaxed);
            previous_version
        };

        let pending = {
            let pending = self
                .pending_unregistered
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            pending.get(&session_id.to_string()).cloned()
        };
        let mut effective_checkpoint_version = self.current_version(session_id);
        if let Some(data) = pending {
            let flush_result = self.save_registered_snapshot(session_id, data, state).await;
            match flush_result {
                Ok(version) => {
                    self.pending_unregistered
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&session_id.to_string());
                    effective_checkpoint_version = version;
                }
                Err(err) => {
                    self.restore_registration_state(
                        session_id,
                        previous_registry,
                        previous_version,
                    );
                    if was_unregistered {
                        self.unregistered_sessions
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .insert(session_key.clone());
                    }
                    if was_suspended {
                        self.suspended_sessions
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .insert(session_key.clone());
                    }
                    if was_superseded {
                        self.superseded_sessions
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .insert(session_key.clone());
                    }
                    return Err(err);
                }
            }
        }
        Ok(effective_checkpoint_version)
    }

    fn forget_session(&self, session_id: &meerkat_core::types::SessionId) {
        let key = session_id.to_string();
        self.session_registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&key);
        self.pending_unregistered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&key);
        self.versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&key);
        self.suspended_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&key);
    }

    /// Quiesce persistence for one registered session. Acquiring the existing
    /// per-session lock is the drain barrier: every save admitted before this
    /// call completes first, and every later mutation observes `Suspended`.
    pub(crate) async fn suspend_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), ContinuityStoreError> {
        let _guard = self.lock_session(session_id).await;
        self.suspended_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_id.to_string());
        Ok(())
    }

    pub(crate) async fn unregister_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), ContinuityStoreError> {
        let _guard = self.lock_session(session_id).await;
        self.forget_session(session_id);
        self.superseded_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&session_id.to_string());
        self.unregistered_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_id.to_string());
        Ok(())
    }

    /// Permanently abandon a reset-superseded session after the replacement
    /// continuity generation has committed.
    ///
    /// Reset changes the identity-keyed continuity head before the old Mob
    /// member can be retired. Leaving the old snapshot behind makes Meerkat's
    /// archive path try to save that stale generation through the new fence,
    /// which correctly fails and retains a `Retiring` cleanup anchor. Delete
    /// the exact old snapshot under its document CAS first, then publish a
    /// superseded tombstone. The exact Mob retirement retry may acknowledge
    /// its generated terminal archive write without crossing the replacement
    /// fence; ordinary unregistration clears the tombstone only after the
    /// physical roster anchor is structurally absent.
    pub(crate) async fn abandon_superseded_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        let _guard = self.lock_session(session_id).await;
        let session_key = session_id.to_string();

        // Quiesce every later projection write before observing and deleting
        // the old document. Keep suspension in place on failure so a retry
        // cannot race a stale runtime save.
        self.suspended_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_key.clone());

        if let Some(session) = self.load_persisted_session(session_id).await? {
            let current_revision =
                meerkat_core::session_store::session_projection_cas_token(&session)?;
            let deleted = self
                .store
                .delete_session_snapshot_if_current_revision(session_id, &current_revision)
                .await
                .map_err(|error| {
                    meerkat_store::SessionStoreError::Internal(format!(
                        "continuity abandon superseded session: {error}"
                    ))
                })?;
            if !deleted {
                return Err(meerkat_store::SessionStoreError::Internal(format!(
                    "continuity abandon did not delete superseded session snapshot {session_id}"
                )));
            }
        }

        self.forget_session(session_id);
        self.superseded_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_key);
        Ok(())
    }

    fn session_was_superseded(&self, session_id: &meerkat_core::types::SessionId) -> bool {
        self.superseded_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&session_id.to_string())
    }

    fn session_was_unregistered(&self, session_id: &meerkat_core::types::SessionId) -> bool {
        self.unregistered_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&session_id.to_string())
    }

    fn session_was_suspended(&self, session_id: &meerkat_core::types::SessionId) -> bool {
        self.suspended_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&session_id.to_string())
    }

    fn ensure_session_mutation_allowed(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        if self.session_was_unregistered(session_id) {
            return Err(meerkat_store::SessionStoreError::Internal(format!(
                "session {session_id} was unregistered from identity runtime state"
            )));
        }
        if self.session_was_suspended(session_id) {
            return Err(meerkat_store::SessionStoreError::Internal(format!(
                "session {session_id} persistence is suspended during identity authority rotation"
            )));
        }
        Ok(())
    }

    /// Get the next checkpoint version for a session, starting at 1.
    fn next_version(&self, session_id: &str) -> u64 {
        let mut map = self
            .versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let counter = map
            .entry(session_id.to_string())
            .or_insert_with(|| AtomicU64::new(0));
        counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn restore_registration_state(
        &self,
        session_id: &meerkat_core::types::SessionId,
        previous_registry: Option<SessionRuntimeState>,
        previous_version: u64,
    ) {
        let key = session_id.to_string();
        {
            let mut registry = self
                .session_registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match previous_registry {
                Some(state) => {
                    registry.insert(key.clone(), state);
                }
                None => {
                    registry.remove(&key);
                }
            }
        }
        let mut versions = self
            .versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if previous_version == 0 {
            versions.remove(&key);
        } else {
            versions
                .entry(key)
                .or_insert_with(|| AtomicU64::new(previous_version))
                .store(previous_version, Ordering::Relaxed);
        }
    }

    fn current_version(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> super::types::CheckpointVersion {
        let map = self
            .versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let version = map
            .get(&session_id.to_string())
            .map(|counter| counter.load(Ordering::Relaxed))
            .unwrap_or(0);
        super::types::CheckpointVersion::new(version)
    }

    /// Look up the runtime state for a session.
    fn lookup_session(&self, session_id: &str) -> Option<SessionRuntimeState> {
        let registry = self
            .session_registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        registry.get(session_id).cloned()
    }

    async fn load_persisted_session(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::Session>, meerkat_store::SessionStoreError> {
        let snapshot = self.store.load_session_snapshot(id).await.map_err(|e| {
            meerkat_store::SessionStoreError::Internal(format!("continuity load: {e}"))
        })?;
        match snapshot {
            Some(snap) => {
                let session: meerkat_core::Session = serde_json::from_slice(&snap.data)
                    .map_err(|e| meerkat_store::SessionStoreError::Serialization(e.to_string()))?;
                Ok(Some(session))
            }
            None => Ok(None),
        }
    }

    async fn load_previous_session_for_save(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::Session>, meerkat_store::SessionStoreError> {
        if let Some(session) = self.load_persisted_session(id).await? {
            return Ok(Some(session));
        }
        let pending = self
            .pending_unregistered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&id.to_string())
            .cloned();
        pending
            .map(|data| {
                serde_json::from_slice(&data)
                    .map_err(|e| meerkat_store::SessionStoreError::Serialization(e.to_string()))
            })
            .transpose()
    }

    async fn save_registered_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
        data: Vec<u8>,
        state: SessionRuntimeState,
    ) -> Result<super::types::CheckpointVersion, meerkat_store::SessionStoreError> {
        let version = self.next_version(&session_id.to_string());
        let checkpoint_version = super::types::CheckpointVersion::new(version);
        let snapshot = super::types::SessionSnapshot { data };
        self.store
            .save_session_snapshot_owned(
                state.identity,
                session_id.clone(),
                state.generation,
                checkpoint_version,
                state.fencing_token,
                snapshot,
            )
            .await
            .map_err(|e| {
                meerkat_store::SessionStoreError::Internal(format!("continuity save: {e}"))
            })?;
        Ok(checkpoint_version)
    }
}

/// Mirror of meerkat-session's `runtime_projection_rollback_authorized`
/// (persistent.rs, meerkat 0.7.24 write-half of the torn-shutdown fix): two
/// pure observations — the persisted row is a faithful continuation of the
/// incoming authority transcript (the same run-boundary proof the save guard
/// uses), and the row carries the intra-turn checkpointer's typed provenance
/// stamp — drive the canonical `SessionDocumentMachine`, which owns the
/// disposition. `RebuildToAuthority` (both observations true) authorizes the
/// save to converge the row back onto committed truth, discarding the
/// unacknowledged tail; an unstamped row or a genuine content fork resolves
/// `RejectDivergent` and the caller keeps failing closed.
fn runtime_projection_rollback_authorized(
    session: &meerkat_core::Session,
    previous: &meerkat_core::Session,
) -> Result<bool, meerkat_store::SessionStoreError> {
    use meerkat_core::session_document::{
        SessionDocumentEffect, SessionDocumentKey, SessionDocumentMachineAuthority,
    };

    let row_continues_authority =
        meerkat_core::session_store::run_boundary_snapshot_save_guard(previous, Some(session))
            .is_ok();
    let row_is_runtime_checkpoint = previous.has_runtime_checkpoint_provenance();
    let mut authority = SessionDocumentMachineAuthority::new();
    let effects = authority
        .resolve_runtime_projection_rollback(
            SessionDocumentKey::new(session.id().to_string()),
            row_continues_authority,
            row_is_runtime_checkpoint,
        )
        .map_err(|err| {
            meerkat_store::SessionStoreError::Internal(format!(
                "session document authority rejected runtime-projection rollback resolution \
                 for session {}: {err}",
                session.id()
            ))
        })?;
    let disposition = effects
        .iter()
        .find_map(|effect| match effect {
            SessionDocumentEffect::RuntimeProjectionRollbackResolved { disposition } => {
                Some(*disposition)
            }
            _ => None,
        })
        .ok_or_else(|| {
            meerkat_store::SessionStoreError::Internal(format!(
                "session document authority returned no runtime-projection rollback \
                 disposition for session {}",
                session.id()
            ))
        })?;
    Ok(matches!(
        disposition,
        meerkat_core::generated::session_document::RuntimeProjectionRollbackDisposition::RebuildToAuthority
    ))
}

#[async_trait]
impl meerkat::SessionStore for ContinuitySessionStoreAdapter {
    async fn save(
        &self,
        session: &meerkat_core::Session,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        let _guard = self.lock_session(session.id()).await;
        if self.session_was_superseded(session.id()) {
            // The old runtime has already been quiesced by the first retire
            // attempt. Meerkat's retry still realizes its generated Archived
            // document action before removing the retained roster anchor, but
            // reset deliberately discarded this document when it committed
            // the replacement generation. Acknowledge that terminal write
            // without crossing the new identity fence.
            return Ok(());
        }
        self.ensure_session_mutation_allowed(session.id())?;

        // Serialize once up front. Stores that can prove the exact bytes and
        // ownership/CAS tuple are already durable avoid loading and reparsing
        // the previous full session document. The ordinary save guard still
        // validates the incoming document against its byte-identical durable
        // predecessor before the no-op is accepted.
        let snapshot = Arc::new(super::types::SessionSnapshot {
            data: serde_json::to_vec(session)
                .map_err(|e| meerkat_store::SessionStoreError::Serialization(e.to_string()))?,
        });
        let sid_str = session.id().to_string();
        let state = self.lookup_session(&sid_str);
        if let Some(state) = state.as_ref() {
            let candidate = SessionSnapshotMatchCandidate {
                identity: state.identity.clone(),
                session_id: session.id().clone(),
                generation: state.generation,
                checkpoint_version: self.current_version(session.id()),
                fencing_token: state.fencing_token,
                snapshot: snapshot.clone(),
            };
            let matches = self
                .store
                .session_snapshot_matches_current(candidate)
                .await
                .map_err(|e| {
                    meerkat_store::SessionStoreError::Internal(format!(
                        "continuity snapshot match: {e}"
                    ))
                })?;
            if matches {
                meerkat_core::session_store::append_only_save_guard(session, Some(session))?;
                return Ok(());
            }
        }

        let previous = self.load_previous_session_for_save(session.id()).await?;
        if let Err(save_error) =
            meerkat_core::session_store::append_only_save_guard(session, previous.as_ref())
        {
            // Bug B-2 (HomeCore, 2026-07-09): the torn-shutdown save wedge.
            // The intra-turn checkpointer can leave the continuity head
            // carrying stamped mid-turn content the machine never
            // acknowledged; after restart, the resume's first save of the
            // (shorter) committed authority trips the append-only guard and
            // the identity degrades forever. meerkat 0.7.24 fixed this in
            // meerkat-session's projection-save shell
            // (`runtime_projection_rollback_authorized`), but this adapter
            // calls the raw guard — mirror the same machine-owned
            // disposition here. `RebuildToAuthority` requires BOTH
            // observations (stamped head + faithful continuation of the
            // incoming authority); anything else keeps failing closed with
            // the original guard error.
            let rollback_authorized = match previous.as_ref() {
                Some(previous) => runtime_projection_rollback_authorized(session, previous)?,
                None => false,
            };
            if !rollback_authorized {
                return Err(save_error);
            }
            tracing::warn!(
                session_id = %session.id(),
                error = %save_error,
                "continuity head carried uncommitted intra-turn checkpoint residue; \
                 rebuilding the row to committed authority (RebuildToAuthority)"
            );
        }
        let snapshot = Arc::try_unwrap(snapshot).unwrap_or_else(|snapshot| (*snapshot).clone());
        let data = snapshot.data;

        // Use real identity/generation/fencing from the runtime registry.
        match state {
            Some(state) => {
                self.save_registered_snapshot(session.id(), data, state)
                    .await?;
            }
            None => {
                // PersistentSessionService can save during member creation
                // before the bridge call returns. Hold that first snapshot
                // until the identity runtime registers the real owner; never
                // checkpoint under a synthetic `_session:*` identity.
                tracing::warn!(
                    session_id = %sid_str,
                    "ContinuitySessionStoreAdapter: delaying save until runtime state is registered"
                );
                let mut pending = self
                    .pending_unregistered
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                pending.insert(sid_str, data);
            }
        }
        Ok(())
    }

    async fn save_transcript_rewrite(
        &self,
        session: &meerkat_core::Session,
        commit: &meerkat_core::TranscriptRewriteCommit,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        let _guard = self.lock_session(session.id()).await;
        if self.session_was_superseded(session.id()) {
            return Ok(());
        }
        self.ensure_session_mutation_allowed(session.id())?;
        let previous = self.load_previous_session_for_save(session.id()).await?;
        meerkat_core::session_store::transcript_rewrite_save_guard(
            session,
            previous.as_ref(),
            commit,
        )?;
        let data = serde_json::to_vec(session)
            .map_err(|e| meerkat_store::SessionStoreError::Serialization(e.to_string()))?;
        let sid_str = session.id().to_string();

        match self.lookup_session(&sid_str) {
            Some(state) => {
                self.save_registered_snapshot(session.id(), data, state)
                    .await?;
            }
            None => {
                let mut pending = self
                    .pending_unregistered
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                pending.insert(sid_str, data);
            }
        }
        Ok(())
    }

    async fn save_authoritative_projection(
        &self,
        session: &meerkat_core::Session,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        let _guard = self.lock_session(session.id()).await;
        if self.session_was_superseded(session.id()) {
            return Ok(());
        }
        self.ensure_session_mutation_allowed(session.id())?;
        let data = serde_json::to_vec(session)
            .map_err(|e| meerkat_store::SessionStoreError::Serialization(e.to_string()))?;
        let sid_str = session.id().to_string();
        match self.lookup_session(&sid_str) {
            Some(state) => {
                self.save_registered_snapshot(session.id(), data, state)
                    .await?;
            }
            None => {
                let mut pending = self
                    .pending_unregistered
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                pending.insert(sid_str, data);
            }
        }
        Ok(())
    }

    async fn save_authoritative_projection_if_current_revision(
        &self,
        session: &meerkat_core::Session,
        expected_current_revision: Option<String>,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        let _guard = self.lock_session(session.id()).await;
        if self.session_was_superseded(session.id()) {
            return Ok(());
        }
        self.ensure_session_mutation_allowed(session.id())?;
        let previous = self.load_persisted_session(session.id()).await?;
        meerkat_core::session_store::authoritative_projection_current_revision_guard(
            session,
            previous.as_ref(),
            expected_current_revision.as_deref(),
        )?;
        let data = serde_json::to_vec(session)
            .map_err(|e| meerkat_store::SessionStoreError::Serialization(e.to_string()))?;
        let sid_str = session.id().to_string();
        match self.lookup_session(&sid_str) {
            Some(state) => {
                self.save_registered_snapshot(session.id(), data, state)
                    .await?;
                Ok(())
            }
            None => {
                let mut pending = self
                    .pending_unregistered
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                pending.insert(sid_str, data);
                Ok(())
            }
        }
    }

    async fn load(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::Session>, meerkat_store::SessionStoreError> {
        if self.session_was_superseded(id) {
            return Ok(None);
        }
        match self.load_persisted_session(id).await? {
            Some(session) => Ok(Some(session)),
            None if self.lookup_session(&id.to_string()).is_some() => {
                Ok(Some(meerkat_core::Session::with_id(id.clone())))
            }
            None => Ok(None),
        }
    }

    async fn list(
        &self,
        _filter: meerkat_store::SessionFilter,
    ) -> Result<Vec<meerkat_core::SessionMeta>, meerkat_store::SessionStoreError> {
        // Listing is not supported through the continuity store adapter.
        // The continuity model is identity-keyed, not session-list-keyed.
        Ok(Vec::new())
    }

    async fn delete(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        let _guard = self.lock_session(id).await;
        if self.session_was_superseded(id) {
            return Ok(());
        }
        self.ensure_session_mutation_allowed(id)?;
        let Some(session) = self.load_persisted_session(id).await? else {
            self.forget_session(id);
            return Ok(());
        };
        let current_revision = meerkat_core::session_store::session_projection_cas_token(&session)?;
        let deleted = self
            .store
            .delete_session_snapshot_if_current_revision(id, &current_revision)
            .await
            .map_err(|e| {
                meerkat_store::SessionStoreError::Internal(format!("continuity delete: {e}"))
            })?;
        if !deleted {
            return Err(meerkat_store::SessionStoreError::Internal(format!(
                "continuity delete did not remove session snapshot {id}"
            )));
        }
        self.forget_session(id);
        Ok(())
    }

    async fn delete_if_current_revision(
        &self,
        id: &meerkat_core::types::SessionId,
        expected_current_revision: &str,
    ) -> Result<bool, meerkat_store::SessionStoreError> {
        let _guard = self.lock_session(id).await;
        if self.session_was_superseded(id) {
            return Ok(false);
        }
        self.ensure_session_mutation_allowed(id)?;
        let Some(session) = self.load_persisted_session(id).await? else {
            self.forget_session(id);
            return Ok(false);
        };
        let current_revision = meerkat_core::session_store::session_projection_cas_token(&session)?;
        if current_revision != expected_current_revision {
            return Ok(false);
        }
        let deleted = self
            .store
            .delete_session_snapshot_if_current_revision(id, expected_current_revision)
            .await
            .map_err(|e| {
                meerkat_store::SessionStoreError::Internal(format!(
                    "continuity delete_if_current_revision: {e}"
                ))
            })?;
        if deleted {
            self.forget_session(id);
        }
        Ok(deleted)
    }
}

// ---------------------------------------------------------------------------
// CONTRACT-11 / REQ-30: SessionHook → AgentCustomizer adapter
// ---------------------------------------------------------------------------

/// Adapts a legacy `SessionHook` to the `AgentCustomizer` trait.
///
/// Constructs a synthetic `CreateSessionRequest` from the `AgentBuildDraft`,
/// lets the hook mutate it, and writes supported mutations back. Unsupported
/// field mutations (e.g., `resume_session`) are detected and logged as warnings.
pub struct SessionHookCustomizerAdapter {
    hook: Arc<dyn SessionHook>,
}

impl SessionHookCustomizerAdapter {
    pub fn new(hook: Arc<dyn SessionHook>) -> Self {
        Self { hook }
    }
}

#[async_trait]
impl AgentCustomizer for SessionHookCustomizerAdapter {
    async fn customize_build(
        &self,
        _context: &AgentBuildContext,
        spec: &DurableAgentSpec,
        draft: &mut AgentBuildDraft,
    ) -> Result<(), CustomizerError> {
        // Build a synthetic CreateSessionRequest from the draft
        let mut req = meerkat_core::service::CreateSessionRequest {
            model: draft.model.clone().unwrap_or_default(),
            prompt: meerkat_core::ContentInput::Text(String::new()),
            // Meerkat 0.7: per-request system prompt is the typed tri-state
            // override; a draft prompt maps to an explicit `Set`.
            system_prompt: match draft.system_prompt.clone() {
                Some(prompt) => meerkat_core::config::SystemPromptOverride::Set(prompt),
                None => meerkat_core::config::SystemPromptOverride::Inherit,
            },
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: if draft.labels.is_empty() {
                None
            } else {
                Some(draft.labels.clone())
            },
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            // Meerkat 0.7.12: typed per-request ambient injection (upstream
            // ask #1). This synthetic request only carries build-draft state;
            // memory injection rides its own path, so it stays empty here.
            injected_context: Vec::new(),
        };

        // Snapshot "before" state for unsupported-mutation detection (REQ-30).
        // Supported: model, system_prompt, labels — written back to draft.
        // Unsupported: everything else — warn if hook mutated them.
        let prompt_before = req.prompt.clone();
        let max_tokens_before = req.max_tokens;
        let event_tx_was_some = req.event_tx.is_some();
        let initial_turn_before = req.initial_turn;
        let build_before_is_none = req.build.is_none();

        self.hook
            .before_create(&mut req)
            .await
            .map_err(|e| CustomizerError::BuildFailed(format!("session hook: {e}")))?;

        // Detect unsupported mutations by comparing before/after (REQ-30).
        // Warn for each mutated field — mutations are NOT applied to the draft.
        let mut unsupported_mutations: Vec<&str> = Vec::new();

        if req.prompt != prompt_before {
            unsupported_mutations.push("prompt");
        }
        if req.max_tokens != max_tokens_before {
            unsupported_mutations.push("max_tokens");
        }
        if req.event_tx.is_some() != event_tx_was_some {
            unsupported_mutations.push("event_tx");
        }
        if req.initial_turn != initial_turn_before {
            unsupported_mutations.push("initial_turn");
        }
        // Meerkat 0.7 removed the flat `render_metadata` / `skill_references`
        // request fields; both now live only on the typed
        // `build.initial_turn_metadata` carrier, which the `build` mutation
        // detection below already covers.
        if let Some(ref build) = req.build {
            if build_before_is_none {
                // Hook created a build block — any build.* is unsupported
                unsupported_mutations.push("build");
                if build.resume_session.is_some() {
                    unsupported_mutations.push("build.resume_session");
                }
            } else if build.resume_session.is_some() {
                // build existed before but hook added resume_session
                unsupported_mutations.push("build.resume_session");
            }
        }

        if !unsupported_mutations.is_empty() {
            tracing::warn!(
                identity = %spec.identity,
                fields = ?unsupported_mutations,
                "SessionHook mutated unsupported CreateSessionRequest fields — \
                 these mutations are NOT applied in the identity-first model. \
                 Migrate to AgentCustomizer."
            );
        }

        // Apply supported mutations back to the draft.
        //
        // NOTE: `additional_instructions` is part of the AgentCustomizer mutation
        // surface (REQ-30), but `CreateSessionRequest` does not expose it as a
        // field. Legacy hooks therefore cannot modify additional_instructions —
        // the draft's existing value from the DurableAgentSpec passes through
        // untouched. Native `AgentCustomizer` impls CAN mutate it directly.
        if !req.model.is_empty() {
            draft.model = Some(req.model);
        }
        draft.system_prompt = req.system_prompt.as_set_prompt().map(ToString::to_string);
        draft.labels = req.labels.unwrap_or_default();

        Ok(())
    }

    async fn after_create(
        &self,
        _identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        context: &SessionCreatedContext,
    ) -> Result<(), CustomizerError> {
        self.hook.after_create(session_id, context).await;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
    use std::time::Duration;

    use serde_json::json;

    use super::super::contracts::ContinuityStore;
    use super::super::local_store::LocalContinuityStore;
    use super::super::types::{
        AgentIdentity, AgentRuntimeId, CheckpointVersion, ContinuityGeneration, ContinuityRecord,
        ContinuityResolveState, ContinuityStoreError, FencingToken, SessionSnapshot,
    };
    use super::*;

    struct FailSaveContinuityStore {
        inner: Arc<LocalContinuityStore>,
        fail_save: AtomicBool,
        fail_delete_once: AtomicBool,
        block_next_save: AtomicBool,
        save_entered: tokio::sync::Semaphore,
        release_save: tokio::sync::Semaphore,
    }

    impl FailSaveContinuityStore {
        fn new(inner: Arc<LocalContinuityStore>) -> Self {
            Self {
                inner,
                fail_save: AtomicBool::new(false),
                fail_delete_once: AtomicBool::new(false),
                block_next_save: AtomicBool::new(false),
                save_entered: tokio::sync::Semaphore::new(0),
                release_save: tokio::sync::Semaphore::new(0),
            }
        }

        fn fail_saves(&self, fail: bool) {
            self.fail_save.store(fail, AtomicOrdering::SeqCst);
        }

        fn fail_next_delete(&self) {
            self.fail_delete_once.store(true, AtomicOrdering::SeqCst);
        }

        fn block_one_save(&self) {
            self.block_next_save.store(true, AtomicOrdering::SeqCst);
        }

        async fn wait_for_blocked_save(&self) {
            self.save_entered
                .acquire()
                .await
                .expect("save-entered semaphore remains open")
                .forget();
        }

        fn release_blocked_save(&self) {
            self.release_save.add_permits(1);
        }
    }

    struct ConcurrentLoadStore {
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
        rendezvous: tokio::sync::Barrier,
    }

    impl ConcurrentLoadStore {
        fn new(expected_concurrent_loads: usize) -> Self {
            Self {
                in_flight: AtomicUsize::new(0),
                max_in_flight: AtomicUsize::new(0),
                rendezvous: tokio::sync::Barrier::new(expected_concurrent_loads),
            }
        }
    }

    #[async_trait]
    impl ContinuityStore for FailSaveContinuityStore {
        async fn resolve_many(
            &self,
            identities: &[AgentIdentity],
        ) -> Result<
            std::collections::BTreeMap<AgentIdentity, ContinuityResolveState>,
            ContinuityStoreError,
        > {
            self.inner.resolve_many(identities).await
        }

        async fn load_session_snapshot(
            &self,
            session_id: &meerkat_core::types::SessionId,
        ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
            self.inner.load_session_snapshot(session_id).await
        }

        async fn delete_session_snapshot_if_current_revision(
            &self,
            session_id: &meerkat_core::types::SessionId,
            expected_current_revision: &str,
        ) -> Result<bool, ContinuityStoreError> {
            if self.fail_delete_once.swap(false, AtomicOrdering::SeqCst) {
                return Err(ContinuityStoreError::Io(
                    "synthetic superseded snapshot delete failure".to_string(),
                ));
            }
            self.inner
                .delete_session_snapshot_if_current_revision(session_id, expected_current_revision)
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
            if self.block_next_save.swap(false, AtomicOrdering::SeqCst) {
                self.save_entered.add_permits(1);
                self.release_save
                    .acquire()
                    .await
                    .expect("release-save semaphore remains open")
                    .forget();
            }
            if self.fail_save.load(AtomicOrdering::SeqCst) {
                return Err(ContinuityStoreError::Io("forced save failure".to_string()));
            }
            self.inner
                .save_session_snapshot(
                    identity,
                    session_id,
                    generation,
                    version,
                    fencing_token,
                    snapshot,
                )
                .await
        }

        async fn upsert_continuity_record(
            &self,
            record: &ContinuityRecord,
            fencing_token: FencingToken,
        ) -> Result<(), ContinuityStoreError> {
            self.inner
                .upsert_continuity_record(record, fencing_token)
                .await
        }

        async fn delete_continuity_record(
            &self,
            identity: &AgentIdentity,
            fencing_token: FencingToken,
        ) -> Result<(), ContinuityStoreError> {
            self.inner
                .delete_continuity_record(identity, fencing_token)
                .await
        }
    }

    #[async_trait]
    impl ContinuityStore for ConcurrentLoadStore {
        async fn resolve_many(
            &self,
            identities: &[AgentIdentity],
        ) -> Result<
            std::collections::BTreeMap<AgentIdentity, ContinuityResolveState>,
            ContinuityStoreError,
        > {
            Ok(identities
                .iter()
                .cloned()
                .map(|identity| (identity, ContinuityResolveState::Uninitialized))
                .collect())
        }

        async fn load_session_snapshot(
            &self,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
            let now = self.in_flight.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            self.max_in_flight.fetch_max(now, AtomicOrdering::SeqCst);
            self.rendezvous.wait().await;
            self.in_flight.fetch_sub(1, AtomicOrdering::SeqCst);
            Ok(None)
        }

        async fn save_session_snapshot(
            &self,
            _identity: &AgentIdentity,
            _session_id: &meerkat_core::types::SessionId,
            _generation: ContinuityGeneration,
            _version: CheckpointVersion,
            _fencing_token: FencingToken,
            _snapshot: &SessionSnapshot,
        ) -> Result<(), ContinuityStoreError> {
            Ok(())
        }

        async fn upsert_continuity_record(
            &self,
            _record: &ContinuityRecord,
            _fencing_token: FencingToken,
        ) -> Result<(), ContinuityStoreError> {
            Ok(())
        }

        async fn delete_continuity_record(
            &self,
            _identity: &AgentIdentity,
            _fencing_token: FencingToken,
        ) -> Result<(), ContinuityStoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_parallelizes_different_sessions() {
        let store = Arc::new(ConcurrentLoadStore::new(2));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let first = meerkat_core::Session::new();
        let second = meerkat_core::Session::new();
        assert_ne!(first.id(), second.id());

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            let (first_result, second_result) = tokio::join!(
                meerkat::SessionStore::save(&adapter, &first),
                meerkat::SessionStore::save(&adapter, &second),
            );
            first_result.expect("first save");
            second_result.expect("second save");
        })
        .await
        .expect("different session IDs must not share one global save lock");

        assert_eq!(
            store.max_in_flight.load(AtomicOrdering::SeqCst),
            2,
            "both independent session loads should overlap"
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_exact_resave_is_a_noop() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:exact-resave").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:exact-resave:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(3);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");

        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("initial save");
        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("exact resave");

        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .expect("resolve");
        let ContinuityResolveState::Ready { record } = resolved.get(&identity).expect("record")
        else {
            panic!("expected ready record");
        };
        assert_eq!(
            record.checkpoint_version,
            CheckpointVersion::new(1),
            "an exact durable resave must not manufacture a new checkpoint"
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_abandons_only_superseded_snapshot() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let identity = AgentIdentity::parse("agent:reset-abandon").expect("identity");
        let old_session = meerkat_core::Session::new();
        let old_record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:reset-abandon:0")
                .expect("old runtime id"),
            session_id: old_session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let old_fence = FencingToken::new(3);
        store
            .upsert_continuity_record(&old_record, old_fence)
            .await
            .expect("seed old record");
        adapter
            .register_session(
                old_session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: old_record.generation,
                    fencing_token: old_fence,
                    checkpoint_version: old_record.checkpoint_version,
                },
            )
            .await
            .expect("register old session");
        meerkat::SessionStore::save(&adapter, &old_session)
            .await
            .expect("persist old session");

        let replacement_session = meerkat_core::types::SessionId::new();
        let replacement_record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:reset-abandon:1")
                .expect("replacement runtime id"),
            session_id: replacement_session.clone(),
            generation: ContinuityGeneration::new(1),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let replacement_fence = FencingToken::new(4);
        store
            .upsert_continuity_record(&replacement_record, replacement_fence)
            .await
            .expect("commit replacement record");

        adapter
            .abandon_superseded_session(old_session.id())
            .await
            .expect("abandon old projection");
        assert!(
            store
                .load_session_snapshot(old_session.id())
                .await
                .expect("load old snapshot")
                .is_none(),
            "the exact superseded snapshot must be CAS-deleted"
        );
        assert_eq!(
            store
                .resolve_many(std::slice::from_ref(&identity))
                .await
                .expect("resolve replacement")
                .get(&identity),
            Some(&ContinuityResolveState::Ready {
                record: replacement_record
            }),
            "abandonment must not disturb the replacement continuity head"
        );

        // Meerkat's exact retained retire retry realizes an Archived document
        // write before removing its roster anchor. The superseded tombstone
        // acknowledges that write without recreating the old snapshot.
        meerkat::SessionStore::save_authoritative_projection(&adapter, &old_session)
            .await
            .expect("terminal superseded projection is acknowledged");
        assert!(
            store
                .load_session_snapshot(old_session.id())
                .await
                .expect("reload old snapshot")
                .is_none()
        );

        adapter
            .unregister_session(old_session.id())
            .await
            .expect("finalize old session authority");
        assert!(
            meerkat::SessionStore::save(&adapter, &old_session)
                .await
                .is_err(),
            "late writes must fail closed after structural member absence"
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_retries_failed_superseded_snapshot_cas() {
        let inner = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let store = Arc::new(FailSaveContinuityStore::new(inner.clone()));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let identity = AgentIdentity::parse("agent:reset-abandon-retry").expect("identity");
        let session = meerkat_core::Session::new();
        let old_record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:reset-abandon-retry:0")
                .expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let old_fence = FencingToken::new(7);
        store
            .upsert_continuity_record(&old_record, old_fence)
            .await
            .expect("seed old record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: old_record.generation,
                    fencing_token: old_fence,
                    checkpoint_version: old_record.checkpoint_version,
                },
            )
            .await
            .expect("register old session");
        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("persist old session");
        let replacement = ContinuityRecord {
            identity,
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:reset-abandon-retry:1")
                .expect("replacement runtime id"),
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(1),
            checkpoint_version: CheckpointVersion::new(0),
        };
        store
            .upsert_continuity_record(&replacement, FencingToken::new(8))
            .await
            .expect("commit replacement");

        store.fail_next_delete();
        assert!(
            adapter
                .abandon_superseded_session(session.id())
                .await
                .is_err(),
            "the injected CAS failure must remain visible"
        );
        assert!(adapter.session_was_suspended(session.id()));
        assert!(
            inner
                .load_session_snapshot(session.id())
                .await
                .expect("snapshot after failed abandon")
                .is_some()
        );

        adapter
            .abandon_superseded_session(session.id())
            .await
            .expect("retry exact CAS abandon");
        assert!(adapter.session_was_superseded(session.id()));
        assert!(
            inner
                .load_session_snapshot(session.id())
                .await
                .expect("snapshot after retry")
                .is_none()
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_exact_bytes_do_not_mask_a_newer_fence() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:exact-stale-fence").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:exact-stale-fence:0")
                .expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(3);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");
        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("initial save");

        store
            .upsert_continuity_record(&record, FencingToken::new(4))
            .await
            .expect("advance durable fence");
        let error = meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect_err("the stale registered fence must still be rejected");
        assert!(
            error.to_string().contains("stale fencing token"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_seeds_registered_checkpoint_version() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:restored").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:restored:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(2),
            checkpoint_version: CheckpointVersion::new(5),
        };
        let fencing_token = FencingToken::new(9);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");

        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");

        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("save should advance from restored checkpoint");
        let effective_version = adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("post-save register should report advanced version");
        assert_eq!(effective_version, CheckpointVersion::new(6));
        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .expect("resolve");
        let ContinuityResolveState::Ready { record } = resolved.get(&identity).expect("record")
        else {
            panic!("expected ready record");
        };
        assert_eq!(record.checkpoint_version, CheckpointVersion::new(6));
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_flushes_pending_save_under_registered_identity() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:fresh").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:fresh:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(3);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");

        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("unregistered save should be delayed, not written under fallback identity");
        assert!(
            store
                .load_session_snapshot(session.id())
                .await
                .expect("load before register")
                .is_none(),
            "unregistered save must not be visible in continuity store"
        );

        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register flushes pending");

        assert!(
            store
                .load_session_snapshot(session.id())
                .await
                .expect("load after register")
                .is_some(),
            "pending save should flush under the registered identity"
        );
        let resolved = store
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .expect("resolve");
        let ContinuityResolveState::Ready { record } = resolved.get(&identity).expect("record")
        else {
            panic!("expected ready record");
        };
        assert_eq!(record.checkpoint_version, CheckpointVersion::new(1));
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_rejects_saves_after_unregister() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:retired").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:retired:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(9);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");

        adapter
            .unregister_session(session.id())
            .await
            .expect("unregister");
        let err = meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect_err("post-unregister save must fail closed");
        assert!(
            err.to_string().contains("was unregistered"),
            "unexpected error: {err}"
        );
        assert!(
            store
                .load_session_snapshot(session.id())
                .await
                .expect("load")
                .is_none(),
            "post-unregister save must not be queued as pending"
        );

        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("registering the same id later should not flush stale pending data");
        assert!(
            store
                .load_session_snapshot(session.id())
                .await
                .expect("load after re-register")
                .is_none(),
            "stale post-unregister save must not flush on a later registration"
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_suspension_blocks_every_mutation_until_reregister() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let mut session = meerkat_core::Session::new();
        session.append_external_user_content(meerkat_core::ContentInput::Text(
            "before rotation".to_string(),
        ));
        let identity = AgentIdentity::parse("agent:suspended").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:suspended:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let old_token = FencingToken::new(7);
        store
            .upsert_continuity_record(&record, old_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: record.generation,
                    fencing_token: old_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");
        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("seed snapshot");

        let parent_revision = session.transcript_revision().expect("parent revision");
        let mut rewritten = session.clone();
        let rewrite_commit = rewritten
            .commit_transcript_rewrite(
                meerkat_core::TranscriptRewriteSelection::MessageRange { start: 0, end: 1 },
                vec![meerkat_core::Message::User(
                    meerkat_core::UserMessage::text("rewritten".to_string()),
                )],
                meerkat_core::TranscriptRewriteReason::new("rotation-test"),
                Some("mobkit-test".to_string()),
                Some(parent_revision),
            )
            .expect("rewrite commit");

        adapter
            .suspend_session(session.id())
            .await
            .expect("suspend");

        let save_error = meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect_err("ordinary save must fail while suspended");
        let rewrite_error =
            meerkat::SessionStore::save_transcript_rewrite(&adapter, &rewritten, &rewrite_commit)
                .await
                .expect_err("transcript rewrite must fail while suspended");
        let projection_error =
            meerkat::SessionStore::save_authoritative_projection(&adapter, &session)
                .await
                .expect_err("authoritative projection must fail while suspended");
        let projection_cas_error =
            meerkat::SessionStore::save_authoritative_projection_if_current_revision(
                &adapter, &session, None,
            )
            .await
            .expect_err("authoritative projection CAS must fail while suspended");
        let delete_error = meerkat::SessionStore::delete(&adapter, session.id())
            .await
            .expect_err("delete must fail while suspended");
        let delete_cas_error = meerkat::SessionStore::delete_if_current_revision(
            &adapter,
            session.id(),
            "row-sha256:any",
        )
        .await
        .expect_err("delete CAS must fail while suspended");

        for error in [
            save_error,
            rewrite_error,
            projection_error,
            projection_cas_error,
            delete_error,
            delete_cas_error,
        ] {
            assert!(
                error.to_string().contains("persistence is suspended"),
                "unexpected suspension error: {error}"
            );
        }

        let new_token = FencingToken::new(8);
        store
            .upsert_continuity_record(&record, new_token)
            .await
            .expect("publish replacement authority");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token: new_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("re-register replacement authority");
        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("writes resume only after exact replacement registration");
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_suspension_drains_admitted_save() {
        let inner = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let store = Arc::new(FailSaveContinuityStore::new(inner.clone()));
        let adapter = Arc::new(ContinuitySessionStoreAdapter::new(store.clone()));
        let session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:suspend-drain").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:suspend-drain:0")
                .expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(17);
        inner
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");

        store.block_one_save();
        let save_adapter = adapter.clone();
        let save_session = session.clone();
        let save_task = tokio::spawn(async move {
            meerkat::SessionStore::save(save_adapter.as_ref(), &save_session).await
        });
        store.wait_for_blocked_save().await;

        let suspend_adapter = adapter.clone();
        let session_id = session.id().clone();
        let mut suspend_task =
            tokio::spawn(async move { suspend_adapter.suspend_session(&session_id).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut suspend_task)
                .await
                .is_err(),
            "suspension must wait for the already-admitted save to leave the session lock"
        );

        store.release_blocked_save();
        save_task
            .await
            .expect("save task joins")
            .expect("admitted save completes before suspension");
        suspend_task
            .await
            .expect("suspend task joins")
            .expect("suspension completes after drain");

        let error = meerkat::SessionStore::save(adapter.as_ref(), &session)
            .await
            .expect_err("later saves must see the suspension barrier");
        assert!(error.to_string().contains("persistence is suspended"));
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_register_keeps_pending_snapshot_on_flush_failure() {
        let inner = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let fail_store = Arc::new(FailSaveContinuityStore::new(inner.clone()));
        let adapter = ContinuitySessionStoreAdapter::new(fail_store.clone());
        let mut session = meerkat_core::Session::new();
        session.set_metadata("pending", json!(true));
        let identity = AgentIdentity::parse("agent:pending-fail").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:pending-fail:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(14);
        inner
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");

        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("pending save");
        fail_store.fail_saves(true);
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect_err("forced pending flush failure");
        assert!(
            meerkat::SessionStore::load(&adapter, session.id())
                .await
                .expect("load after failed register")
                .is_none(),
            "failed register must not leave a synthetic registered session"
        );

        fail_store.fail_saves(false);
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("retry register should flush preserved pending snapshot");
        let loaded = meerkat::SessionStore::load(&adapter, session.id())
            .await
            .expect("load after retry")
            .expect("snapshot");
        assert_eq!(loaded.metadata().get("pending"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_delete_if_current_revision_removes_matching_snapshot()
    {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:quarantine").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:quarantine:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(4);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");
        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("save snapshot");

        let stale_revision = "row-sha256:not-current".to_string();
        assert!(
            !meerkat::SessionStore::delete_if_current_revision(
                &adapter,
                session.id(),
                &stale_revision
            )
            .await
            .expect("stale delete should be clean"),
            "stale revision must not delete"
        );
        assert!(
            store
                .load_session_snapshot(session.id())
                .await
                .expect("load after stale")
                .is_some(),
            "stale CAS delete must leave snapshot in place"
        );

        let current_revision =
            meerkat_core::session_store::session_projection_cas_token(&session).expect("revision");
        assert!(
            meerkat::SessionStore::delete_if_current_revision(
                &adapter,
                session.id(),
                &current_revision
            )
            .await
            .expect("matching delete should succeed"),
            "matching revision should delete"
        );
        assert!(
            store
                .load_session_snapshot(session.id())
                .await
                .expect("load after delete")
                .is_none(),
            "matching CAS delete must remove the continuity snapshot"
        );
        assert!(
            meerkat::SessionStore::load(&adapter, session.id())
                .await
                .expect("adapter load after delete")
                .is_none(),
            "adapter must not synthesize a session after successful CAS delete"
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_save_rejects_transcript_shrink() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let mut session = meerkat_core::Session::new();
        session.append_external_user_content(meerkat_core::ContentInput::Text("first".to_string()));
        session
            .append_external_user_content(meerkat_core::ContentInput::Text("second".to_string()));
        let identity = AgentIdentity::parse("agent:append-only").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:append-only:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(12);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");
        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("initial save");

        let mut stale = meerkat_core::Session::with_id(session.id().clone());
        stale.append_external_user_content(meerkat_core::ContentInput::Text("first".to_string()));
        let err = meerkat::SessionStore::save(&adapter, &stale)
            .await
            .expect_err("plain save must reject transcript shrink");
        assert!(
            err.to_string().contains("transcript")
                || err.to_string().contains("monotonicity")
                || err.to_string().contains("continuity"),
            "unexpected shrink error: {err}"
        );
    }

    /// Bug B-2 (HomeCore field wedge): a torn shutdown leaves the continuity
    /// head carrying the intra-turn checkpointer's STAMPED mid-turn content;
    /// the resume's first save of the shorter committed authority must
    /// converge the row back onto authority (`RebuildToAuthority`), not
    /// wedge on `MonotonicityViolation` forever.
    #[tokio::test]
    async fn save_rebuilds_stamped_checkpoint_residue_to_authority() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let mut authority = meerkat_core::Session::new();
        authority
            .append_external_user_content(meerkat_core::ContentInput::Text("first".to_string()));
        authority
            .append_external_user_content(meerkat_core::ContentInput::Text("second".to_string()));
        let identity = AgentIdentity::parse("agent:torn-shutdown").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:torn-shutdown:0")
                .expect("runtime id"),
            session_id: authority.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(21);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                authority.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");
        meerkat::SessionStore::save(&adapter, &authority)
            .await
            .expect("boundary save of the authority");

        // The intra-turn checkpointer persists a STAMPED head strictly ahead
        // of the boundary commit; the host dies before the boundary lands.
        let mut stamped_head = authority.clone();
        stamped_head
            .append_external_user_content(meerkat_core::ContentInput::Text("mid-turn".to_string()));
        stamped_head.set_runtime_checkpoint_provenance();
        meerkat::SessionStore::save(&adapter, &stamped_head)
            .await
            .expect("checkpointer save of the stamped head");

        // Restart: the read source served the runtime snapshot (authority);
        // the resume's first save of that shorter content used to trip
        // MonotonicityViolation and wedge the identity.
        meerkat::SessionStore::save(&adapter, &authority)
            .await
            .expect("authority save must rebuild the stamped residue, not wedge");
        let loaded = meerkat::SessionStore::load(&adapter, authority.id())
            .await
            .expect("load")
            .expect("session");
        assert_eq!(
            loaded.messages().len(),
            authority.messages().len(),
            "row must be rebuilt to committed authority (tail discarded)"
        );
    }

    /// The rollback requires BOTH observations: a head that extends the
    /// authority but does NOT carry the checkpointer's provenance stamp is
    /// out-of-band divergence and must keep failing closed.
    #[tokio::test]
    async fn save_keeps_rejecting_unstamped_longer_heads() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let mut authority = meerkat_core::Session::new();
        authority
            .append_external_user_content(meerkat_core::ContentInput::Text("first".to_string()));
        let identity = AgentIdentity::parse("agent:unstamped").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:unstamped:0").expect("runtime id"),
            session_id: authority.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(22);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                authority.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");
        meerkat::SessionStore::save(&adapter, &authority)
            .await
            .expect("initial save");
        let mut unstamped_head = authority.clone();
        unstamped_head
            .append_external_user_content(meerkat_core::ContentInput::Text("tail".to_string()));
        meerkat::SessionStore::save(&adapter, &unstamped_head)
            .await
            .expect("longer head appends fine");

        meerkat::SessionStore::save(&adapter, &authority)
            .await
            .expect_err("unstamped longer head must keep failing closed");
    }

    /// A stamped head whose content FORKS from the incoming authority (not a
    /// faithful continuation) resolves `RejectDivergent` — stamp alone never
    /// authorizes the rollback.
    #[tokio::test]
    async fn save_keeps_rejecting_stamped_forked_heads() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let mut base = meerkat_core::Session::new();
        base.append_external_user_content(meerkat_core::ContentInput::Text("first".to_string()));
        let identity = AgentIdentity::parse("agent:forked").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:forked:0").expect("runtime id"),
            session_id: base.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(23);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                base.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");
        let mut stamped_fork = base.clone();
        stamped_fork
            .append_external_user_content(meerkat_core::ContentInput::Text("forked".to_string()));
        stamped_fork.set_runtime_checkpoint_provenance();
        meerkat::SessionStore::save(&adapter, &stamped_fork)
            .await
            .expect("seed the stamped head");

        // A DIVERGENT authority (same length as base + different tail) is a
        // content fork relative to the persisted head, not its prefix.
        let mut diverged = base.clone();
        diverged
            .append_external_user_content(meerkat_core::ContentInput::Text("other".to_string()));
        diverged
            .append_external_user_content(meerkat_core::ContentInput::Text("branch".to_string()));
        meerkat::SessionStore::save(&adapter, &diverged)
            .await
            .expect_err("stamped but forked head must keep failing closed");
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_saves_transcript_rewrite() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let mut session = meerkat_core::Session::new();
        session.append_external_user_content(meerkat_core::ContentInput::Text("first".to_string()));
        session
            .append_external_user_content(meerkat_core::ContentInput::Text("second".to_string()));
        let identity = AgentIdentity::parse("agent:rewrite").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:rewrite:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(13);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");
        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("initial save");

        let parent_revision = session.transcript_revision().expect("parent revision");
        let mut rewritten = session.clone();
        let commit = rewritten
            .commit_transcript_rewrite(
                meerkat_core::TranscriptRewriteSelection::MessageRange { start: 0, end: 1 },
                vec![meerkat_core::Message::User(
                    meerkat_core::UserMessage::text("compacted first".to_string()),
                )],
                meerkat_core::TranscriptRewriteReason::new("test"),
                Some("mobkit-test".to_string()),
                Some(parent_revision),
            )
            .expect("rewrite commit");

        meerkat::SessionStore::save_transcript_rewrite(&adapter, &rewritten, &commit)
            .await
            .expect("rewrite save should be supported");
        let loaded = meerkat::SessionStore::load(&adapter, session.id())
            .await
            .expect("load rewritten")
            .expect("rewritten session");
        assert_eq!(loaded.messages().len(), rewritten.messages().len());
        assert_eq!(
            loaded.transcript_revision().expect("loaded revision"),
            commit.revision
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_delete_removes_current_snapshot() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:delete").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:delete:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(7);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");
        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("save snapshot");

        meerkat::SessionStore::delete(&adapter, session.id())
            .await
            .expect("delete should remove current snapshot");
        assert!(
            store
                .load_session_snapshot(session.id())
                .await
                .expect("load after delete")
                .is_none(),
            "delete must not be a successful no-op"
        );
        assert!(
            meerkat::SessionStore::load(&adapter, session.id())
                .await
                .expect("adapter load after delete")
                .is_none(),
            "adapter must forget registry state after delete"
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_queues_unregistered_authoritative_projection() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let session = meerkat_core::Session::new();

        meerkat::SessionStore::save_authoritative_projection(&adapter, &session)
            .await
            .expect("create-time authoritative projection should queue before registration");
        assert!(
            meerkat::SessionStore::load(&adapter, session.id())
                .await
                .expect("load")
                .is_none(),
            "pending authoritative projection must stay invisible until registration"
        );

        let identity = AgentIdentity::parse("agent:queued").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:queued:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(7);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register flushes pending authoritative projection");
        assert!(
            meerkat::SessionStore::load(&adapter, session.id())
                .await
                .expect("load after register")
                .is_some(),
            "registration must flush the pending authoritative projection"
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_delete_forgets_registered_session_without_snapshot() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:delete-empty").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:delete-empty:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(11);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");
        assert!(
            meerkat::SessionStore::load(&adapter, session.id())
                .await
                .expect("synthetic load before delete")
                .is_some()
        );

        meerkat::SessionStore::delete(&adapter, session.id())
            .await
            .expect("delete with no persisted snapshot should be idempotent");
        assert!(
            meerkat::SessionStore::load(&adapter, session.id())
                .await
                .expect("load after delete")
                .is_none(),
            "delete must forget registry state when no persisted row exists"
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_authoritative_projection_cas_guards_rewrites() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let mut session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:projection").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:projection:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(5);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");

        meerkat::SessionStore::save_authoritative_projection_if_current_revision(
            &adapter, &session, None,
        )
        .await
        .expect("initial projection should accept missing current revision");
        let original_revision =
            meerkat_core::session_store::session_projection_cas_token(&session).expect("revision");

        let mut stale_rewrite = session.clone();
        stale_rewrite.set_metadata("projection", json!("stale"));
        let stale_error = meerkat::SessionStore::save_authoritative_projection_if_current_revision(
            &adapter,
            &stale_rewrite,
            Some("row-sha256:not-current".to_string()),
        )
        .await
        .expect_err("stale CAS projection must reject");
        assert!(
            stale_error.to_string().contains("not a continuation"),
            "unexpected stale error: {stale_error}"
        );

        let loaded = meerkat::SessionStore::load(&adapter, session.id())
            .await
            .expect("load")
            .expect("snapshot");
        assert_eq!(
            meerkat_core::session_store::session_projection_cas_token(&loaded).expect("revision"),
            original_revision,
            "stale authoritative projection must leave stored row unchanged"
        );

        session.set_metadata("projection", json!("current"));
        meerkat::SessionStore::save_authoritative_projection_if_current_revision(
            &adapter,
            &session,
            Some(original_revision),
        )
        .await
        .expect("matching CAS projection should save");

        let loaded = meerkat::SessionStore::load(&adapter, session.id())
            .await
            .expect("load after save")
            .expect("snapshot after save");
        assert_eq!(loaded.metadata().get("projection"), Some(&json!("current")));
    }
}
