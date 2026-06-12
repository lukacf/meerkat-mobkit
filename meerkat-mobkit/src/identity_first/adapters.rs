//! Compatibility adapters bridging legacy MobKit traits to identity-first contracts.
//!
//! - [`DiscoveryRosterAdapter`]: `Discovery` → `RosterProvider` (CONTRACT-08, REQ-27, REQ-28)
//! - [`EdgeDiscoveryTopologyAdapter`]: `EdgeDiscovery` → `TopologyProvider` (CONTRACT-09, REQ-29)
//! - [`ContinuitySessionStoreAdapter`]: `ContinuityStore` → `SessionStore` (CONTRACT-10)
//! - [`SessionHookCustomizerAdapter`]: `SessionHook` → `AgentCustomizer` (CONTRACT-11, REQ-30)

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::contracts::{AgentCustomizer, RosterProvider, TopologyProvider};
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
    /// Serializes persisted session writes, including projection CAS load/write pairs.
    save_guard: tokio::sync::Mutex<()>,
}

impl ContinuitySessionStoreAdapter {
    pub fn new(store: Arc<dyn super::contracts::ContinuityStore>) -> Self {
        Self {
            store,
            versions: Mutex::new(HashMap::new()),
            session_registry: Mutex::new(HashMap::new()),
            pending_unregistered: Mutex::new(HashMap::new()),
            unregistered_sessions: Mutex::new(HashSet::new()),
            save_guard: tokio::sync::Mutex::new(()),
        }
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
        let _guard = self.save_guard.lock().await;
        let session_key = session_id.to_string();
        let checkpoint_version = state.checkpoint_version.get();
        let previous_registry = {
            let mut registry = self
                .session_registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            registry.insert(session_key.clone(), state.clone())
        };
        self.unregistered_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&session_key);

        let previous_version = {
            let mut versions = self
                .versions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let counter = versions
                .entry(session_key)
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
                    return Err(err);
                }
            }
        }
        Ok(effective_checkpoint_version)
    }

    /// Update the fencing token for a session (e.g., after lease renewal).
    #[allow(dead_code)]
    pub(crate) fn update_fencing_token(
        &self,
        session_id: &meerkat_core::types::SessionId,
        token: super::types::FencingToken,
    ) {
        let mut registry = self
            .session_registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(state) = registry.get_mut(&session_id.to_string()) {
            state.fencing_token = token;
        }
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
    }

    pub(crate) async fn unregister_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), ContinuityStoreError> {
        let _guard = self.save_guard.lock().await;
        self.forget_session(session_id);
        self.unregistered_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_id.to_string());
        Ok(())
    }

    fn session_was_unregistered(&self, session_id: &meerkat_core::types::SessionId) -> bool {
        self.unregistered_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&session_id.to_string())
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
            .save_session_snapshot(
                &state.identity,
                session_id,
                state.generation,
                checkpoint_version,
                state.fencing_token,
                &snapshot,
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
        let _guard = self.save_guard.lock().await;
        if self.session_was_unregistered(session.id()) {
            return Err(meerkat_store::SessionStoreError::Internal(format!(
                "session {} was unregistered from identity runtime state",
                session.id()
            )));
        }
        let previous = self.load_previous_session_for_save(session.id()).await?;
        meerkat_core::session_store::append_only_save_guard(session, previous.as_ref())?;
        let data = serde_json::to_vec(session)
            .map_err(|e| meerkat_store::SessionStoreError::Serialization(e.to_string()))?;
        let sid_str = session.id().to_string();

        // Use real identity/generation/fencing from the runtime registry.
        match self.lookup_session(&sid_str) {
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
        let _guard = self.save_guard.lock().await;
        if self.session_was_unregistered(session.id()) {
            return Err(meerkat_store::SessionStoreError::Internal(format!(
                "session {} was unregistered from identity runtime state",
                session.id()
            )));
        }
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
        let data = serde_json::to_vec(session)
            .map_err(|e| meerkat_store::SessionStoreError::Serialization(e.to_string()))?;
        let sid_str = session.id().to_string();

        let _guard = self.save_guard.lock().await;
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
        let _guard = self.save_guard.lock().await;
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
        let _guard = self.save_guard.lock().await;
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
        let _guard = self.save_guard.lock().await;
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
    use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

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
    }

    impl FailSaveContinuityStore {
        fn new(inner: Arc<LocalContinuityStore>) -> Self {
            Self {
                inner,
                fail_save: AtomicBool::new(false),
            }
        }

        fn fail_saves(&self, fail: bool) {
            self.fail_save.store(fail, AtomicOrdering::SeqCst);
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
