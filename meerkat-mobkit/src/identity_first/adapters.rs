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
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// H3 lazy-at-restore checkpoint adoption: when a load under a registered
    /// continuity cursor decodes legacy-unverified, stamp it via
    /// `meerkat_core::adopt_legacy_session` with that observed cursor and
    /// persist the adopted bytes through the store's own CAS at the next
    /// checkpoint version. Off by default; the identity-first gateway
    /// wirings enable it via [`Self::with_lazy_checkpoint_adoption`] (it is
    /// the fleet-unbrick behavior for 0.7.x-era snapshot bytes).
    lazy_checkpoint_adoption: bool,
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
            lazy_checkpoint_adoption: false,
        }
    }

    /// Enable (or disable) H3 lazy-at-restore checkpoint adoption. A named,
    /// greppable wiring decision: on identity-first gateways the continuity
    /// store is the session authority, and without adoption a 0.7.x-era
    /// snapshot hard-fails every resume. On fleets whose continuity rows
    /// record a nonzero generation floor this observed-cursor path must run
    /// before meerkat's own INITIAL-cursor lazy migration first touches the
    /// session (a prematurely stamped lower generation is sticky).
    #[must_use]
    pub fn with_lazy_checkpoint_adoption(mut self, enabled: bool) -> Self {
        self.lazy_checkpoint_adoption = enabled;
        self
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
        if let Some(existing) = self.lookup_session(&session_key) {
            if existing.identity != state.identity || existing.generation != state.generation {
                return Err(meerkat_store::SessionStoreError::Internal(format!(
                    "session ownership conflict for {session_id}: registered owner {}/generation {} cannot be replaced by {}/generation {}",
                    existing.identity, existing.generation, state.identity, state.generation
                )));
            }
            if state.fencing_token < existing.fencing_token {
                return Err(meerkat_store::SessionStoreError::Internal(format!(
                    "session ownership conflict for {session_id}: fencing token {} cannot regress to {}",
                    existing.fencing_token, state.fencing_token
                )));
            }
        }
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
        {
            let mut versions = self
                .versions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let counter = versions
                .entry(session_key.clone())
                .or_insert_with(|| AtomicU64::new(checkpoint_version));
            counter.fetch_max(checkpoint_version, Ordering::Relaxed);
        }

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
                    // The provider may have committed the attempted version
                    // and lost only its acknowledgement. Restore registry and
                    // marker state, but never rewind the monotonic allocator.
                    self.restore_registration_state(session_id, previous_registry);
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
    ) {
        let key = session_id.to_string();
        {
            let mut registry = self
                .session_registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match previous_registry {
                Some(state) => {
                    registry.insert(key, state);
                }
                None => {
                    registry.remove(&key);
                }
            }
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
        Ok(self
            .load_persisted_session_with_bytes(id)
            .await?
            .map(|(session, _)| session))
    }

    /// Load the durable snapshot, returning both the decoded document and the
    /// exact raw bytes. Lazy checkpoint adoption stamps a legacy document by
    /// taking byte custody of the source BLOB, so the decoded form alone is
    /// not enough.
    async fn load_persisted_session_with_bytes(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Option<(meerkat_core::Session, Vec<u8>)>, meerkat_store::SessionStoreError> {
        let snapshot = self.store.load_session_snapshot(id).await.map_err(|e| {
            meerkat_store::SessionStoreError::Internal(format!("continuity load: {e}"))
        })?;
        match snapshot {
            Some(snap) => {
                let session: meerkat_core::Session = serde_json::from_slice(&snap.data)
                    .map_err(|e| meerkat_store::SessionStoreError::Serialization(e.to_string()))?;
                if session.id() != id {
                    return Err(meerkat_store::SessionStoreError::Serialization(format!(
                        "continuity snapshot key {id} contains session {}",
                        session.id()
                    )));
                }
                Ok(Some((session, snap.data)))
            }
            None => Ok(None),
        }
    }

    /// H3 lazy-at-restore adoption of one legacy-unverified snapshot, called
    /// under the session lock from `load`. The observed cursor is the
    /// registered continuity tuple (registration precedes resume, so the
    /// cursor is in hand before the first load). The adopted bytes persist
    /// through the store's own CAS at the NEXT checkpoint version — the
    /// version bump is the sanctioned lazy-shape trade-off (the trait cannot
    /// rewrite in place at the same version); the stamp inside the bytes
    /// still binds the observed cursor. Every failure passes the raw legacy
    /// document through unchanged so upstream keeps its typed fail-closed
    /// behavior; nothing is ever half-adopted.
    async fn lazy_adopt_legacy_snapshot(
        &self,
        id: &meerkat_core::types::SessionId,
        session: meerkat_core::Session,
        raw: &[u8],
    ) -> meerkat_core::Session {
        // Cheap structural probe first: only legacy-unverified documents are
        // candidates, and this avoids a full canonical-digest verification on
        // every ordinary load of an already-typed document.
        let is_legacy = matches!(
            meerkat_core::session_checkpoint_metadata_state(session.id(), session.metadata()),
            Ok(meerkat_core::SessionCheckpointMetadataState::LegacyUnverified { .. })
        );
        if !is_legacy {
            return session;
        }
        let Some(state) = self.lookup_session(&id.to_string()) else {
            // No registered cursor to observe. Pass the legacy document
            // through; meerkat's resolver owns the INITIAL-cursor migration
            // for cursorless reads.
            return session;
        };
        let observed_generation = meerkat_core::SessionGeneration::new(state.generation.get());
        // The observed revision is the DURABLE cursor the runtime registered
        // from the continuity record — never the in-memory version allocator,
        // which advances before persistence: after a failed save the
        // allocator sits ahead of the durable row, and stamping from it would
        // certify the legacy bytes under a revision that never committed.
        // The allocator's only role here is minting the NEXT version inside
        // `save_registered_snapshot` for the store's own CAS write.
        let observed_revision =
            meerkat_core::SessionCheckpointRevision::new(state.checkpoint_version.get());
        let adopted =
            match meerkat_core::adopt_legacy_session(raw, observed_generation, observed_revision) {
                Ok(adopted) => adopted,
                Err(error) => {
                    tracing::warn!(
                        session_id = %id,
                        %error,
                        "lazy checkpoint adoption refused; passing the legacy document through"
                    );
                    return session;
                }
            };
        match self
            .save_registered_snapshot(id, adopted.serialized, state)
            .await
        {
            Ok(committed_version) => {
                tracing::info!(
                    session_id = %id,
                    observed_generation = observed_generation.get(),
                    observed_checkpoint_revision = observed_revision.get(),
                    committed_version = committed_version.get(),
                    "lazy checkpoint adoption stamped a legacy continuity snapshot at restore"
                );
                adopted.session
            }
            Err(error) => {
                // Persisting failed: behave exactly as if adoption were off.
                // Returning the adopted document over a still-legacy durable
                // row would hand upstream a verified authority the store
                // cannot corroborate.
                tracing::warn!(
                    session_id = %id,
                    %error,
                    "lazy checkpoint adoption could not persist the adopted bytes; \
                     passing the legacy document through"
                );
                session
            }
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
        // Meerkat 0.8.2's PersistentSessionService owns projection rollback
        // and invokes the authoritative-projection CAS seam when its generated
        // classifier permits repair. This adapter remains a strict store; it
        // must not run a second recovery classifier.
        meerkat_core::session_store::append_only_save_guard(session, previous.as_ref())?;
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
        // This is a compare-and-set against the visible durable projection.
        // Pre-registration pending bytes are not a durable current revision
        // and must not influence the expected `None`/revision predicate.
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
        let _guard = self.lock_session(id).await;
        if self.session_was_superseded(id) {
            return Ok(None);
        }
        // Registration is observed realization state, not transcript history.
        // Missing durable bytes stay missing so upstream can classify them;
        // never synthesize an empty session.
        let Some((session, raw)) = self.load_persisted_session_with_bytes(id).await? else {
            return Ok(None);
        };
        if !self.lazy_checkpoint_adoption {
            return Ok(Some(session));
        }
        Ok(Some(
            self.lazy_adopt_legacy_snapshot(id, session, &raw).await,
        ))
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

    /// Genuine forwarding of the incremental capability (M4b): `Some` exactly
    /// when the continuity substrate advertises a session-delta channel
    /// ([`super::contracts::ContinuityStore::as_incremental_sessions`]). The
    /// returned wrapper preserves the adapter's registration and fencing
    /// discipline — every delta mutation requires a registered,
    /// non-suspended, non-superseded session and serializes on the same
    /// per-session lock as the whole-blob paths. With the bundled
    /// `LocalContinuityStore` (which defers the channel) and the JSON-RPC
    /// `GatewayContinuityStore` (whole-snapshot wire verbs only) this stays
    /// `None`: the H2 whole-blob degradation report remains truthful.
    fn as_incremental(
        self: Arc<Self>,
    ) -> Option<Arc<dyn meerkat_core::session_store::IncrementalSessionStore>> {
        let inner = self.store.as_incremental_sessions()?;
        Some(Arc::new(ContinuityIncrementalSessionStore {
            adapter: self,
            inner,
        }))
    }
}

// ---------------------------------------------------------------------------
// Incremental forwarding wrapper (M4b)
// ---------------------------------------------------------------------------

/// The incremental session-store view the adapter returns when its
/// substrate advertises [`super::contracts::ContinuityStore::as_incremental_sessions`].
///
/// Split of responsibilities:
/// - the substrate's channel owns the durable delta contract (append
///   contiguity/idempotency, rewrite verification, head CAS) **and** the
///   continuity write discipline (fence CAS + version monotonicity per
///   mutation) — that is the advertised capability's documented obligation;
/// - this wrapper owns the adapter-side session lifecycle: delta mutations
///   are admitted only for sessions registered with the identity runtime and
///   are refused while a session is suspended (authority rotation),
///   unregistered, or superseded — under the same per-session lock the
///   whole-blob save paths serialize on (H3's lazy adoption included).
///
/// Whole-document `SessionStore` verbs delegate to the adapter unchanged, so
/// mixed consumers observe one behavior.
pub struct ContinuityIncrementalSessionStore {
    adapter: Arc<ContinuitySessionStoreAdapter>,
    inner: Arc<dyn meerkat_core::session_store::IncrementalSessionStore>,
}

impl ContinuityIncrementalSessionStore {
    fn ensure_registered_for_delta_write(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        if self.adapter.session_was_superseded(id) {
            return Err(meerkat_store::SessionStoreError::Internal(format!(
                "session {id} was superseded by a committed continuity reset; \
                 incremental writes are refused"
            )));
        }
        self.adapter.ensure_session_mutation_allowed(id)?;
        if self.adapter.lookup_session(&id.to_string()).is_none() {
            return Err(meerkat_store::SessionStoreError::Internal(format!(
                "session {id} is not registered with the identity runtime; \
                 incremental writes require a registered continuity cursor"
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl meerkat::SessionStore for ContinuityIncrementalSessionStore {
    async fn save(
        &self,
        session: &meerkat_core::Session,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        self.adapter.save(session).await
    }

    async fn save_transcript_rewrite(
        &self,
        session: &meerkat_core::Session,
        commit: &meerkat_core::TranscriptRewriteCommit,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        self.adapter.save_transcript_rewrite(session, commit).await
    }

    async fn save_authoritative_projection(
        &self,
        session: &meerkat_core::Session,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        self.adapter.save_authoritative_projection(session).await
    }

    async fn save_authoritative_projection_if_current_revision(
        &self,
        session: &meerkat_core::Session,
        expected_current_revision: Option<String>,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        self.adapter
            .save_authoritative_projection_if_current_revision(session, expected_current_revision)
            .await
    }

    async fn load(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::Session>, meerkat_store::SessionStoreError> {
        self.adapter.load(id).await
    }

    async fn list(
        &self,
        filter: meerkat_store::SessionFilter,
    ) -> Result<Vec<meerkat_core::SessionMeta>, meerkat_store::SessionStoreError> {
        self.adapter.list(filter).await
    }

    async fn delete(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        self.adapter.delete(id).await
    }

    async fn delete_if_current_revision(
        &self,
        id: &meerkat_core::types::SessionId,
        expected_current_revision: &str,
    ) -> Result<bool, meerkat_store::SessionStoreError> {
        self.adapter
            .delete_if_current_revision(id, expected_current_revision)
            .await
    }

    fn as_incremental(
        self: Arc<Self>,
    ) -> Option<Arc<dyn meerkat_core::session_store::IncrementalSessionStore>> {
        Some(self)
    }
}

#[async_trait]
impl meerkat_core::session_store::IncrementalSessionStore for ContinuityIncrementalSessionStore {
    async fn append_messages(
        &self,
        id: &meerkat_core::types::SessionId,
        strand: &meerkat_core::session_store::TranscriptStrandId,
        base_seq: u64,
        messages: &[meerkat_core::Message],
    ) -> Result<(), meerkat_store::SessionStoreError> {
        let _guard = self.adapter.lock_session(id).await;
        self.ensure_registered_for_delta_write(id)?;
        self.inner
            .append_messages(id, strand, base_seq, messages)
            .await
    }

    async fn commit_rewrite(
        &self,
        id: &meerkat_core::types::SessionId,
        record: &meerkat_core::TranscriptRewriteRecord,
        expected: meerkat_core::session_store::SessionHeadCas,
    ) -> Result<meerkat_core::session_store::SessionHead, meerkat_store::SessionStoreError> {
        let _guard = self.adapter.lock_session(id).await;
        self.ensure_registered_for_delta_write(id)?;
        self.inner.commit_rewrite(id, record, expected).await
    }

    async fn save_head(
        &self,
        head: &meerkat_core::session_store::SessionHead,
        expected: meerkat_core::session_store::SessionHeadCas,
    ) -> Result<(), meerkat_store::SessionStoreError> {
        let _guard = self.adapter.lock_session(&head.id).await;
        self.ensure_registered_for_delta_write(&head.id)?;
        self.inner.save_head(head, expected).await
    }

    async fn load_head(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::session_store::SessionHead>, meerkat_store::SessionStoreError>
    {
        if self.adapter.session_was_superseded(id) {
            return Ok(None);
        }
        self.inner.load_head(id).await
    }

    async fn load_messages(
        &self,
        id: &meerkat_core::types::SessionId,
        strand: &meerkat_core::session_store::TranscriptStrandId,
        range: std::ops::Range<u64>,
    ) -> Result<Vec<meerkat_core::Message>, meerkat_store::SessionStoreError> {
        if self.adapter.session_was_superseded(id) {
            return Ok(Vec::new());
        }
        self.inner.load_messages(id, strand, range).await
    }

    async fn load_rewrites(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<Vec<meerkat_core::TranscriptRewriteRecord>, meerkat_store::SessionStoreError> {
        if self.adapter.session_was_superseded(id) {
            return Ok(Vec::new());
        }
        self.inner.load_rewrites(id).await
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
        commit_then_fail_save: AtomicBool,
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
                commit_then_fail_save: AtomicBool::new(false),
                fail_delete_once: AtomicBool::new(false),
                block_next_save: AtomicBool::new(false),
                save_entered: tokio::sync::Semaphore::new(0),
                release_save: tokio::sync::Semaphore::new(0),
            }
        }

        fn fail_saves(&self, fail: bool) {
            self.fail_save.store(fail, AtomicOrdering::SeqCst);
        }

        fn commit_then_fail_next_save(&self) {
            self.commit_then_fail_save
                .store(true, AtomicOrdering::SeqCst);
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
                .await?;
            if self
                .commit_then_fail_save
                .swap(false, AtomicOrdering::SeqCst)
            {
                return Err(ContinuityStoreError::Io(
                    "synthetic lost save acknowledgement".to_string(),
                ));
            }
            Ok(())
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
    async fn continuity_session_store_adapter_lost_ack_consumes_checkpoint_version() {
        let inner = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let fail_store = Arc::new(FailSaveContinuityStore::new(inner.clone()));
        let adapter = ContinuitySessionStoreAdapter::new(fail_store.clone());
        let session = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:lost-ack").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:lost-ack:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(23);
        inner
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        meerkat::SessionStore::save(&adapter, &session)
            .await
            .expect("queue pre-registration snapshot");

        let state = SessionRuntimeState {
            identity: identity.clone(),
            generation: record.generation,
            fencing_token,
            checkpoint_version: record.checkpoint_version,
        };
        fail_store.commit_then_fail_next_save();
        adapter
            .register_session(session.id(), state.clone())
            .await
            .expect_err("first flush commits version 1 but loses its acknowledgement");

        let effective = adapter
            .register_session(session.id(), state)
            .await
            .expect("retry must allocate a fresh checkpoint version");
        assert_eq!(effective, CheckpointVersion::new(2));
        let resolved = inner
            .resolve_many(std::slice::from_ref(&identity))
            .await
            .expect("resolve");
        let ContinuityResolveState::Ready { record } = resolved.get(&identity).expect("record")
        else {
            panic!("expected ready record");
        };
        assert_eq!(record.checkpoint_version, CheckpointVersion::new(2));
    }

    #[tokio::test]
    async fn lazy_adoption_failed_save_is_not_observable_and_retry_stamps_durable_cursor() {
        let inner = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let fail_store = Arc::new(FailSaveContinuityStore::new(inner.clone()));
        let adapter = ContinuitySessionStoreAdapter::new(fail_store.clone())
            .with_lazy_checkpoint_adoption(true);

        let session = meerkat_core::Session::new();
        let sid = session.id().clone();
        let legacy = serde_json::to_vec(&session).expect("serialize legacy session");
        let identity = AgentIdentity::parse("agent:lazy-fail").expect("identity");
        let fencing_token = FencingToken::new(5);
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:lazy-fail:0").expect("runtime id"),
            session_id: sid.clone(),
            generation: ContinuityGeneration::new(3),
            checkpoint_version: CheckpointVersion::new(0),
        };
        inner
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        inner
            .save_session_snapshot(
                &identity,
                &sid,
                ContinuityGeneration::new(3),
                CheckpointVersion::new(4),
                fencing_token,
                &SessionSnapshot {
                    data: legacy.clone(),
                },
            )
            .await
            .expect("seed legacy snapshot");
        adapter
            .register_session(
                &sid,
                SessionRuntimeState {
                    identity: identity.clone(),
                    generation: ContinuityGeneration::new(3),
                    fencing_token,
                    checkpoint_version: CheckpointVersion::new(4),
                },
            )
            .await
            .expect("register session");

        // The failed adoption save mints (and burns) version 5 in the
        // allocator but commits nothing; the caller must see the legacy
        // document, never a stamped-but-uncommitted verification.
        fail_store.fail_saves(true);
        let loaded = meerkat::SessionStore::load(&adapter, &sid)
            .await
            .expect("load under failing save")
            .expect("session present");
        assert!(
            matches!(
                loaded.try_checkpoint_state().expect("checkpoint state"),
                meerkat_core::SessionCheckpointState::LegacyUnverified { .. }
            ),
            "a failed adoption save must pass the legacy document through"
        );
        let durable = inner
            .load_session_snapshot(&sid)
            .await
            .expect("durable load")
            .expect("snapshot present");
        assert_eq!(
            durable.data, legacy,
            "a failed adoption save must leave the durable legacy bytes untouched"
        );

        // Retry after the store recovers: the stamp must bind the durable
        // registered cursor (4), not the unpersisted allocator mint (5).
        fail_store.fail_saves(false);
        let adopted = meerkat::SessionStore::load(&adapter, &sid)
            .await
            .expect("load after recovery")
            .expect("session present");
        let stamp = match adopted.try_checkpoint_state().expect("checkpoint state") {
            meerkat_core::SessionCheckpointState::Verified(stamp) => stamp,
            other => panic!("expected an adopted document, got {other:?}"),
        };
        assert_eq!(stamp.generation(), meerkat_core::SessionGeneration::new(3));
        assert_eq!(
            stamp.checkpoint_revision(),
            meerkat_core::SessionCheckpointRevision::new(4),
            "the stamp must bind the durable cursor, never a revision that only \
             the in-memory allocator ever saw"
        );

        // The durable copy carries the same stamp.
        let durable = inner
            .load_session_snapshot(&sid)
            .await
            .expect("durable load after adoption")
            .expect("snapshot present");
        let durable_session: meerkat_core::Session =
            serde_json::from_slice(&durable.data).expect("decode adopted snapshot");
        match durable_session
            .try_checkpoint_state()
            .expect("durable checkpoint state")
        {
            meerkat_core::SessionCheckpointState::Verified(durable_stamp) => {
                assert_eq!(durable_stamp, stamp);
            }
            other => panic!("expected a verified durable document, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_rejects_owner_generation_and_fence_regression() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store);
        let session = meerkat_core::Session::new();
        let first = SessionRuntimeState {
            identity: AgentIdentity::parse("agent:owner-a").expect("identity"),
            generation: ContinuityGeneration::new(4),
            fencing_token: FencingToken::new(10),
            checkpoint_version: CheckpointVersion::new(7),
        };
        adapter
            .register_session(session.id(), first.clone())
            .await
            .expect("initial registration");

        let foreign_owner = SessionRuntimeState {
            identity: AgentIdentity::parse("agent:owner-b").expect("identity"),
            ..first.clone()
        };
        adapter
            .register_session(session.id(), foreign_owner)
            .await
            .expect_err("a session id cannot be rebound to another identity");

        let foreign_generation = SessionRuntimeState {
            generation: ContinuityGeneration::new(5),
            ..first.clone()
        };
        adapter
            .register_session(session.id(), foreign_generation)
            .await
            .expect_err("a session id cannot be rebound to another generation");

        let greater = SessionRuntimeState {
            fencing_token: FencingToken::new(11),
            ..first.clone()
        };
        adapter
            .register_session(session.id(), greater.clone())
            .await
            .expect("a monotonic fence may replace the prior write authority");
        adapter
            .suspend_session(session.id())
            .await
            .expect("suspend replacement authority");

        let regressed = SessionRuntimeState {
            fencing_token: FencingToken::new(9),
            ..first.clone()
        };
        adapter
            .register_session(session.id(), regressed)
            .await
            .expect_err("suspension must never authorize fence regression");
        adapter
            .register_session(session.id(), greater.clone())
            .await
            .expect("the same current fence resumes suspended persistence");
        assert_eq!(
            adapter.lookup_session(&session.id().to_string()),
            Some(greater),
            "rejected registrations must not replace the owner"
        );
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

    /// Projection rollback is owned by Meerkat's PersistentSessionService.
    /// The raw store adapter remains append-only and must not independently
    /// reinterpret a stamped longer row as permission to fabricate rollback.
    #[tokio::test]
    #[allow(deprecated)] // Exercises compatibility with legacy stamped rows.
    async fn raw_save_rejects_stamped_checkpoint_residue_rollback() {
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
        stamped_head
            .set_runtime_checkpoint_provenance()
            .expect("stamp legacy runtime checkpoint provenance");
        meerkat::SessionStore::save(&adapter, &stamped_head)
            .await
            .expect("checkpointer save of the stamped head");

        // The authoritative Meerkat service may classify and drive this
        // repair through its CAS seam. A direct adapter call may not.
        let error = meerkat::SessionStore::save(&adapter, &authority)
            .await
            .expect_err("raw save must reject transcript rollback");
        assert!(
            error.to_string().contains("transcript")
                || error.to_string().contains("monotonicity")
                || error.to_string().contains("continuation"),
            "unexpected rollback error: {error}"
        );
        let loaded = meerkat::SessionStore::load(&adapter, authority.id())
            .await
            .expect("load")
            .expect("session");
        assert_eq!(
            loaded.messages().len(),
            stamped_head.messages().len(),
            "rejected raw rollback must leave the durable stamped row unchanged"
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
    #[allow(deprecated)] // Exercises compatibility with legacy stamped rows.
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
        stamped_fork
            .set_runtime_checkpoint_provenance()
            .expect("stamp legacy runtime checkpoint provenance");
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
                .expect("load before delete")
                .is_none(),
            "registration alone must not fabricate a session document"
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
    async fn continuity_session_store_adapter_projection_cas_ignores_pending_bytes() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let mut session = meerkat_core::Session::new();
        session.set_metadata("projection", json!("first-pending"));
        meerkat::SessionStore::save_authoritative_projection_if_current_revision(
            &adapter, &session, None,
        )
        .await
        .expect("first missing-row CAS");

        session.set_metadata("projection", json!("latest-pending"));
        meerkat::SessionStore::save_authoritative_projection_if_current_revision(
            &adapter, &session, None,
        )
        .await
        .expect("pending bytes are not a visible durable revision");

        let identity = AgentIdentity::parse("agent:projection-pending").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:projection-pending:0")
                .expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(31);
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
            .expect("flush latest pending projection");
        let loaded = meerkat::SessionStore::load(&adapter, session.id())
            .await
            .expect("load")
            .expect("snapshot");
        assert_eq!(
            loaded.metadata().get("projection"),
            Some(&json!("latest-pending"))
        );
    }

    #[tokio::test]
    async fn continuity_session_store_adapter_rejects_snapshot_with_foreign_embedded_id() {
        let store = Arc::new(LocalContinuityStore::in_memory().expect("store"));
        let adapter = ContinuitySessionStoreAdapter::new(store.clone());
        let requested = meerkat_core::Session::new();
        let foreign = meerkat_core::Session::new();
        let identity = AgentIdentity::parse("agent:foreign-snapshot-id").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:foreign-snapshot-id:0")
                .expect("runtime id"),
            session_id: requested.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let fencing_token = FencingToken::new(41);
        store
            .upsert_continuity_record(&record, fencing_token)
            .await
            .expect("seed record");
        let bytes = serde_json::to_vec(&foreign).expect("serialize foreign session");
        store
            .save_session_snapshot(
                &identity,
                requested.id(),
                record.generation,
                CheckpointVersion::new(1),
                fencing_token,
                &SessionSnapshot { data: bytes },
            )
            .await
            .expect("seed corrupt keyed snapshot");

        let error = meerkat::SessionStore::load(&adapter, requested.id())
            .await
            .expect_err("embedded foreign session id must be explicit corruption");
        assert!(error.to_string().contains("contains session"));
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

    /// A continuity store advertising the M4b incremental capability: the
    /// whole-blob verbs delegate to an in-memory `LocalContinuityStore`,
    /// the delta channel to `meerkat_store::MemoryStore` (which implements
    /// the upstream incremental contract).
    struct IncrementalCapableStore {
        inner: Arc<LocalContinuityStore>,
        incremental: Arc<meerkat_store::MemoryStore>,
    }

    impl IncrementalCapableStore {
        fn new() -> Self {
            Self {
                inner: Arc::new(LocalContinuityStore::in_memory().expect("store")),
                incremental: Arc::new(meerkat_store::MemoryStore::new()),
            }
        }
    }

    #[async_trait]
    impl ContinuityStore for IncrementalCapableStore {
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

        async fn save_session_snapshot(
            &self,
            identity: &AgentIdentity,
            session_id: &meerkat_core::types::SessionId,
            generation: ContinuityGeneration,
            version: CheckpointVersion,
            fencing_token: FencingToken,
            snapshot: &SessionSnapshot,
        ) -> Result<(), ContinuityStoreError> {
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

        fn as_incremental_sessions(
            &self,
        ) -> Option<Arc<dyn meerkat_core::session_store::IncrementalSessionStore>> {
            Some(self.incremental.clone())
        }
    }

    /// M4b: the adapter genuinely forwards `as_incremental` — `None` over the
    /// bundled store (the deliberate deferral), `Some` when the substrate
    /// advertises the capability.
    #[tokio::test]
    async fn adapter_forwards_incremental_capability_only_when_substrate_advertises() {
        let bundled = Arc::new(ContinuitySessionStoreAdapter::new(Arc::new(
            LocalContinuityStore::in_memory().expect("store"),
        )));
        assert!(
            meerkat::SessionStore::as_incremental(bundled).is_none(),
            "the bundled LocalContinuityStore defers the delta channel (M4b)"
        );

        let capable = Arc::new(ContinuitySessionStoreAdapter::new(Arc::new(
            IncrementalCapableStore::new(),
        )));
        assert!(
            meerkat::SessionStore::as_incremental(capable).is_some(),
            "an advertising substrate must surface through the adapter"
        );
    }

    /// M4b: incremental mutations preserve the adapter's registration and
    /// rotation discipline — refused before registration, admitted after,
    /// refused again while suspended.
    #[tokio::test]
    async fn incremental_mutations_respect_registration_and_suspension() {
        let store = Arc::new(IncrementalCapableStore::new());
        let adapter = Arc::new(ContinuitySessionStoreAdapter::new(
            store.clone() as Arc<dyn ContinuityStore>
        ));
        let incremental = meerkat::SessionStore::as_incremental(adapter.clone())
            .expect("advertising substrate forwards");

        let session = meerkat_core::Session::new();
        let root = meerkat_core::session_store::TranscriptStrandId::root();
        let message =
            meerkat_core::Message::User(meerkat_core::UserMessage::text("delta turn".to_string()));

        let unregistered = incremental
            .append_messages(session.id(), &root, 0, std::slice::from_ref(&message))
            .await
            .expect_err("unregistered delta writes must be refused");
        assert!(
            unregistered.to_string().contains("not registered"),
            "the refusal must name the registration requirement: {unregistered}"
        );

        let identity = AgentIdentity::parse("agent:incremental").expect("identity");
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:incremental:0").expect("runtime id"),
            session_id: session.id().clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        store
            .upsert_continuity_record(&record, FencingToken::new(1))
            .await
            .expect("seed record");
        adapter
            .register_session(
                session.id(),
                SessionRuntimeState {
                    identity,
                    generation: record.generation,
                    fencing_token: FencingToken::new(1),
                    checkpoint_version: record.checkpoint_version,
                },
            )
            .await
            .expect("register");

        incremental
            .append_messages(session.id(), &root, 0, std::slice::from_ref(&message))
            .await
            .expect("registered delta writes delegate to the substrate channel");
        let mut document = session.clone();
        document.push(message.clone());
        let head =
            meerkat_core::session_store::SessionHead::from_session(&document, root.clone(), 0)
                .expect("head from session");
        incremental
            .save_head(&head, meerkat_core::session_store::SessionHeadCas::Create)
            .await
            .expect("registered head writes delegate to the substrate channel");
        let rows = incremental
            .load_messages(session.id(), &root, 0..1)
            .await
            .expect("read back");
        assert_eq!(
            rows.len(),
            1,
            "the delta row must be durable in the channel"
        );

        adapter
            .suspend_session(session.id())
            .await
            .expect("suspend");
        let suspended = incremental
            .append_messages(session.id(), &root, 1, std::slice::from_ref(&message))
            .await
            .expect_err("suspended sessions must refuse delta writes");
        assert!(
            suspended.to_string().contains("suspended"),
            "the refusal must name the suspension: {suspended}"
        );
    }
}
