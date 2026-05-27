//! Session bridge: connects the identity-first control plane to the Meerkat
//! session pipeline for real session creation, delivery, and retirement.

use std::sync::Arc;

use async_trait::async_trait;
use meerkat_core::types::HandlingMode;
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::launch::MemberLaunchMode;
use meerkat_mob::{
    MobHandle, MobSessionService, SpawnMemberSpec, SpawnSystemPromptOverride, WorkOrigin, WorkRef,
    WorkSpec,
};

use crate::mob_handle_runtime::{content_input_has_images, model_capabilities_for_member};

use super::adapters::{ContinuitySessionStoreAdapter, SessionRuntimeState};
use super::types::{
    AgentBuildDraft, AgentIdentity, AgentRuntimeId, CheckpointVersion, ContinuityGeneration,
    DurableAgentSpec, FencingToken, SessionSnapshot,
};

fn is_missing_event_injector_error(error: &str) -> bool {
    error.contains("missing event injector capability")
}

fn is_missing_bridge_session_snapshot_error(error: &str) -> bool {
    error.contains("missing bridge session snapshot")
}

fn is_repairable_bridge_delivery_error(error: &str) -> bool {
    is_missing_event_injector_error(error) || is_missing_bridge_session_snapshot_error(error)
}

fn is_archive_not_found_cleanup_error(error: &str) -> bool {
    error.contains("disposal completed but ArchiveSession failed")
        && error.contains("cancel-before-retire failed")
        && error.contains("Runtime not ready: running")
}

// ---------------------------------------------------------------------------
// BridgeError
// ---------------------------------------------------------------------------

/// Errors from session bridge operations.
#[derive(Debug)]
pub enum BridgeError {
    /// The underlying mob operation failed.
    Mob(String),
    /// A required field was missing or invalid.
    InvalidInput(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(msg) => write!(f, "session bridge mob error: {msg}"),
            Self::InvalidInput(msg) => write!(f, "session bridge invalid input: {msg}"),
        }
    }
}

impl std::error::Error for BridgeError {}

/// Typed reason a requested resume could not reuse the persisted runtime
/// binding and had to fall back to a fresh member spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeFallbackReason {
    /// The persisted session/runtime identity is incompatible with the current
    /// mob runtime binding.
    RuntimeIdentityIncompatible { detail: String },
}

/// Result of attempting to materialize a persisted identity through resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeSessionOutcome {
    /// The persisted session was resumed as-is.
    Resumed {
        session_id: meerkat_core::types::SessionId,
    },
    /// Resume was rejected for a typed compatibility reason and a fresh member
    /// was spawned instead.
    FreshSpawned {
        session_id: meerkat_core::types::SessionId,
        reason: ResumeFallbackReason,
    },
}

impl ResumeSessionOutcome {
    #[must_use]
    pub fn session_id(&self) -> &meerkat_core::types::SessionId {
        match self {
            Self::Resumed { session_id } | Self::FreshSpawned { session_id, .. } => session_id,
        }
    }

    #[must_use]
    pub fn fallback_reason(&self) -> Option<&ResumeFallbackReason> {
        match self {
            Self::Resumed { .. } => None,
            Self::FreshSpawned { reason, .. } => Some(reason),
        }
    }
}

async fn submit_internal_bridge_work(
    handle: &MobHandle,
    member_id: &MeerkatId,
    content: &meerkat_core::ContentInput,
    handling_mode: HandlingMode,
) -> Result<(), BridgeError> {
    let entry = handle
        .get_member(member_id)
        .await
        .ok_or_else(|| BridgeError::Mob(format!("member not found: {member_id}")))?;
    handle
        .submit_work_with_mode(
            entry.agent_runtime_id.clone(),
            entry.fence_token,
            WorkRef::new(),
            WorkSpec::new(content.clone(), WorkOrigin::Internal),
            handling_mode,
        )
        .await
        .map(|_| ())
        .map_err(|err| BridgeError::Mob(err.to_string()))
}

// ---------------------------------------------------------------------------
// SessionBridge trait
// ---------------------------------------------------------------------------

/// Bridge between the identity-first control plane and the Meerkat session
/// pipeline. Each method maps an identity-layer operation to its concrete
/// mob-level counterpart.
#[async_trait]
pub trait SessionBridge: Send + Sync {
    /// Spawn a new mob member for a freshly-created identity.
    async fn create_session(
        &self,
        identity: &AgentIdentity,
        runtime_id: &AgentRuntimeId,
        spec: &DurableAgentSpec,
        draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::types::SessionId, BridgeError>;

    /// Resume a mob member from a previously checkpointed snapshot.
    ///
    /// The `session_id` comes from the ContinuityRecord — it's the session
    /// that should be loaded from the session store. When the session store
    /// has the data, this performs a true resume (conversation history intact).
    /// When the session is missing, implementations should fall back to a
    /// fresh spawn.
    async fn resume_session(
        &self,
        identity: &AgentIdentity,
        runtime_id: &AgentRuntimeId,
        spec: &DurableAgentSpec,
        draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
        snapshot: &SessionSnapshot,
    ) -> Result<ResumeSessionOutcome, BridgeError>;

    /// Deliver content to an active mob member.
    async fn deliver(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
    ) -> Result<meerkat_core::types::SessionId, BridgeError>;

    /// Deliver content to an active mob member using a caller-selected turn
    /// handling mode. Bridge implementations that do not distinguish modes can
    /// fall back to ordinary delivery.
    async fn deliver_with_mode(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let _ = handling_mode;
        self.deliver(runtime_id, content).await
    }

    /// Checkpoint the current session state for a mob member.
    async fn checkpoint_session(
        &self,
        runtime_id: &AgentRuntimeId,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<SessionSnapshot, BridgeError>;

    /// Retire a mob member.
    async fn retire_member(&self, runtime_id: &AgentRuntimeId) -> Result<(), BridgeError>;

    /// Wire two active same-mob members by their concrete runtime IDs.
    async fn wire_peer(&self, _a: &AgentRuntimeId, _b: &AgentRuntimeId) -> Result<(), BridgeError> {
        Err(BridgeError::Mob("peer wiring not supported".to_string()))
    }

    /// Wire many active same-mob member pairs by their concrete runtime IDs.
    async fn wire_peers_batch(
        &self,
        edges: &[(AgentRuntimeId, AgentRuntimeId)],
    ) -> Result<(), BridgeError> {
        for (a, b) in edges {
            self.wire_peer(a, b).await?;
        }
        Ok(())
    }

    /// Return currently materialized same-mob member wires by concrete runtime IDs.
    async fn current_member_wires(
        &self,
    ) -> Result<Vec<(AgentRuntimeId, AgentRuntimeId)>, BridgeError> {
        Ok(Vec::new())
    }

    /// Unwire two active same-mob members by their concrete runtime IDs.
    async fn unwire_peer(
        &self,
        _a: &AgentRuntimeId,
        _b: &AgentRuntimeId,
    ) -> Result<(), BridgeError> {
        Err(BridgeError::Mob("peer unwiring not supported".to_string()))
    }

    /// Inspect the current execution state of a mob member.
    async fn inspect_member(
        &self,
        _runtime_id: &AgentRuntimeId,
    ) -> Result<MemberInspection, BridgeError> {
        Err(BridgeError::Mob("inspect not supported".to_string()))
    }

    /// Register identity ownership for a concrete bridge session.
    ///
    /// Bridges that install a continuity-backed session store use this to
    /// ensure subsequent Meerkat session saves checkpoint under the durable
    /// identity/generation/fencing tuple.
    async fn register_session_runtime_state(
        &self,
        _session_id: &meerkat_core::types::SessionId,
        _identity: &AgentIdentity,
        _generation: ContinuityGeneration,
        checkpoint_version: CheckpointVersion,
        _fencing_token: FencingToken,
    ) -> Result<CheckpointVersion, BridgeError> {
        Ok(checkpoint_version)
    }

    /// Remove identity ownership metadata for a concrete bridge session.
    ///
    /// This is used when an identity lifecycle operation aborts after session
    /// runtime state was registered but before continuity commits.
    async fn unregister_session_runtime_state(
        &self,
        _session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), BridgeError> {
        Ok(())
    }
}

/// Lightweight inspection of a mob member's current execution state.
#[derive(Debug, Clone)]
pub struct MemberInspection {
    pub output_preview: Option<String>,
    pub is_final: bool,
    pub peer_reachable_count: usize,
}

// ---------------------------------------------------------------------------
// MobSessionBridge — real implementation backed by MobHandle
// ---------------------------------------------------------------------------

/// Concrete `SessionBridge` backed by a `MobHandle`.
///
/// `AgentRuntimeId` is used as the `MeerkatId` at the mob layer — the runtime
/// ID IS the member's mob-level identifier.
pub struct MobSessionBridge {
    handle: MobHandle,
    /// Session store used for checkpoint (loading session data to serialize).
    session_store: Option<Arc<dyn meerkat::SessionStore>>,
    /// Session service used to project the live effective model for capability checks.
    session_service: Option<Arc<dyn MobSessionService>>,
    /// Continuity-backed session store, when installed by the identity-first builder.
    continuity_session_store: Option<Arc<ContinuitySessionStoreAdapter>>,
}

impl MobSessionBridge {
    /// Create a new bridge wrapping the given mob handle.
    pub fn new(handle: MobHandle) -> Self {
        Self {
            handle,
            session_store: None,
            session_service: None,
            continuity_session_store: None,
        }
    }

    /// Create a new bridge with session-service access for live model capability checks.
    pub fn with_session_service(
        handle: MobHandle,
        session_service: Arc<dyn MobSessionService>,
    ) -> Self {
        Self {
            handle,
            session_store: None,
            session_service: Some(session_service),
            continuity_session_store: None,
        }
    }

    /// Create a new bridge with an explicit session store for checkpoint support.
    pub fn with_session_store(
        handle: MobHandle,
        session_store: Arc<dyn meerkat::SessionStore>,
    ) -> Self {
        Self {
            handle,
            session_store: Some(session_store),
            session_service: None,
            continuity_session_store: None,
        }
    }

    /// Create a bridge with checkpoint and live capability support.
    pub fn with_session_store_and_service(
        handle: MobHandle,
        session_store: Arc<dyn meerkat::SessionStore>,
        session_service: Arc<dyn MobSessionService>,
    ) -> Self {
        Self {
            handle,
            session_store: Some(session_store),
            session_service: Some(session_service),
            continuity_session_store: None,
        }
    }

    /// Create a bridge with an identity-owned continuity session store.
    pub fn with_continuity_session_store(
        handle: MobHandle,
        session_store: Arc<ContinuitySessionStoreAdapter>,
        session_service: Option<Arc<dyn MobSessionService>>,
    ) -> Self {
        Self {
            handle,
            session_store: Some(session_store.clone()),
            session_service,
            continuity_session_store: Some(session_store),
        }
    }
}

/// Build a `SpawnMemberSpec` from identity-first types, wiring draft fields.
pub(crate) fn build_spawn_spec(
    runtime_id: &AgentRuntimeId,
    spec: &DurableAgentSpec,
    draft: &AgentBuildDraft,
) -> SpawnMemberSpec {
    let mid = MeerkatId::from(runtime_id.as_str());
    let mut spawn_spec = SpawnMemberSpec::new(spec.profile.clone(), mid);

    if let Some(message) = spec.initial_message.as_ref() {
        spawn_spec = spawn_spec.with_initial_message(message.clone());
    }
    if let Some(runtime_mode) = spec.runtime_mode_override {
        spawn_spec = spawn_spec.with_runtime_mode(runtime_mode);
    }
    if let Some(ref ctx) = draft.app_context {
        spawn_spec = spawn_spec.with_context(ctx.clone());
    }
    let mut labels = draft.labels.clone();
    labels.insert(
        "agent_identity".to_string(),
        spec.identity.as_str().to_string(),
    );
    if !labels.is_empty() {
        spawn_spec = spawn_spec.with_labels(labels);
    }
    if !draft.additional_instructions.is_empty() {
        spawn_spec = spawn_spec.with_additional_instructions(draft.additional_instructions.clone());
    }
    if let Some(model) = draft.model.as_ref() {
        spawn_spec.model_override = Some(model.clone());
    }
    if let Some(system_prompt) = draft.system_prompt.as_ref() {
        spawn_spec.system_prompt_override =
            Some(SpawnSystemPromptOverride::Replace(system_prompt.clone()));
    }
    if let Some(dispatcher) = draft.local_external_tools.dispatcher() {
        spawn_spec.external_tools = Some(dispatcher);
    }

    spawn_spec
}

#[async_trait]
impl SessionBridge for MobSessionBridge {
    async fn create_session(
        &self,
        _identity: &AgentIdentity,
        runtime_id: &AgentRuntimeId,
        spec: &DurableAgentSpec,
        draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let mid = MeerkatId::from(runtime_id.as_str());
        let spawn_spec = build_spawn_spec(runtime_id, spec, draft);

        self.handle
            .spawn_spec(spawn_spec)
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;

        let actual_session_id = self
            .handle
            .resolve_bridge_session_id(&mid)
            .await
            .ok_or_else(|| BridgeError::Mob("member spawned but has no session ID".to_string()))?;
        let _ = session_id;
        Ok(actual_session_id)
    }

    async fn resume_session(
        &self,
        _identity: &AgentIdentity,
        runtime_id: &AgentRuntimeId,
        spec: &DurableAgentSpec,
        draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
        _snapshot: &SessionSnapshot,
    ) -> Result<ResumeSessionOutcome, BridgeError> {
        // Try MemberLaunchMode::Resume first — this loads the existing session
        // from the session store (conversation history intact).
        let mut spawn_spec = build_spawn_spec(runtime_id, spec, draft);
        spawn_spec.launch_mode = MemberLaunchMode::Resume {
            bridge_session_id: session_id.clone(),
        };

        match self.handle.spawn_spec(spawn_spec).await {
            Ok(_) => Ok(ResumeSessionOutcome::Resumed {
                session_id: session_id.clone(),
            }),
            Err(e) => {
                // Resume can fail if the old session's comms identity is still
                // claimed (e.g., in-process restart where the previous mob actor
                // hasn't fully terminated). Fall back to a fresh spawn.
                tracing::warn!(
                    identity = %_identity,
                    session_id = %session_id,
                    error = %e,
                    reason = "runtime_identity_incompatible",
                    "resume_session incompatible with current runtime binding, falling back to fresh spawn"
                );
                let fresh_spec = build_spawn_spec(runtime_id, spec, draft);
                self.handle
                    .spawn_spec(fresh_spec)
                    .await
                    .map_err(|e2| BridgeError::Mob(e2.to_string()))?;

                let mid = MeerkatId::from(runtime_id.as_str());
                let session_id = self
                    .handle
                    .resolve_bridge_session_id(&mid)
                    .await
                    .ok_or_else(|| {
                        BridgeError::Mob(
                            "member spawned (fresh fallback) but has no session ID".to_string(),
                        )
                    })?;
                Ok(ResumeSessionOutcome::FreshSpawned {
                    session_id,
                    reason: ResumeFallbackReason::RuntimeIdentityIncompatible {
                        detail: e.to_string(),
                    },
                })
            }
        }
    }

    async fn deliver(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let mid = MeerkatId::from(runtime_id.as_str());
        let member_entry_before_delivery = self.handle.get_member(&mid).await;
        if content_input_has_images(content) {
            let member_entry = self.handle.get_member(&mid).await.ok_or_else(|| {
                BridgeError::Mob("member not found while checking image capability".to_string())
            })?;
            let caps = model_capabilities_for_member(
                &self.handle,
                self.session_service.as_ref(),
                &member_entry.agent_identity,
            )
            .await;
            if !caps.image_input {
                return Err(BridgeError::InvalidInput(
                    "target member model cannot accept image input".to_string(),
                ));
            }
        }

        // Submit internal work directly through the mob work lane so delivery
        // acks at runtime ingress rather than waiting for the full turn to
        // complete. The identity layer owns addressability enforcement — the
        // bridge is an internal delivery mechanism regardless of whether the
        // identity is Addressable or InternalOnly.
        match submit_internal_bridge_work(&self.handle, &mid, content, HandlingMode::Queue).await {
            Ok(()) => {}
            Err(err) if is_repairable_bridge_delivery_error(&err.to_string()) => {
                tracing::warn!(
                    runtime_id = %runtime_id,
                    error = %err,
                    "identity bridge delivery found stale runtime state; repairing member before retry"
                );
                match self.handle.respawn(mid.clone(), None).await {
                    Ok(_) => {}
                    Err(respawn_err)
                        if is_archive_not_found_cleanup_error(&respawn_err.to_string()) =>
                    {
                        if self.handle.get_member(&mid).await.is_none()
                            && let Some(entry) = member_entry_before_delivery
                        {
                            let mut spec = SpawnMemberSpec::new(entry.role.clone(), mid.clone());
                            if !entry.labels.is_empty() {
                                spec = spec.with_labels(entry.labels.clone());
                            }
                            self.handle
                                .ensure_member(spec)
                                .await
                                .map_err(|e| BridgeError::Mob(e.to_string()))?;
                        }
                    }
                    Err(respawn_err) => return Err(BridgeError::Mob(respawn_err.to_string())),
                }
                submit_internal_bridge_work(&self.handle, &mid, content, HandlingMode::Queue)
                    .await?;
            }
            Err(err) => return Err(BridgeError::Mob(err.to_string())),
        }

        // Meerkat 0.6: MemberDeliveryReceipt no longer carries session_id.
        // Query the bridge session id directly from the mob handle.
        self.handle
            .resolve_bridge_session_id(&mid)
            .await
            .ok_or_else(|| {
                BridgeError::Mob("member has no bridge session after deliver".to_string())
            })
    }

    async fn deliver_with_mode(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let mid = MeerkatId::from(runtime_id.as_str());
        let member_entry_before_delivery = self.handle.get_member(&mid).await;
        if content_input_has_images(content) {
            let member_entry = self.handle.get_member(&mid).await.ok_or_else(|| {
                BridgeError::Mob("member not found while checking image capability".to_string())
            })?;
            let caps = model_capabilities_for_member(
                &self.handle,
                self.session_service.as_ref(),
                &member_entry.agent_identity,
            )
            .await;
            if !caps.image_input {
                return Err(BridgeError::InvalidInput(
                    "target member model cannot accept image input".to_string(),
                ));
            }
        }

        match submit_internal_bridge_work(&self.handle, &mid, content, handling_mode).await {
            Ok(()) => {}
            Err(err) if is_repairable_bridge_delivery_error(&err.to_string()) => {
                tracing::warn!(
                    runtime_id = %runtime_id,
                    error = %err,
                    "identity bridge delivery found stale runtime state; repairing member before retry"
                );
                match self.handle.respawn(mid.clone(), None).await {
                    Ok(_) => {}
                    Err(respawn_err)
                        if is_archive_not_found_cleanup_error(&respawn_err.to_string()) =>
                    {
                        if self.handle.get_member(&mid).await.is_none()
                            && let Some(entry) = member_entry_before_delivery
                        {
                            let mut spec = SpawnMemberSpec::new(entry.role.clone(), mid.clone());
                            if !entry.labels.is_empty() {
                                spec = spec.with_labels(entry.labels.clone());
                            }
                            self.handle
                                .ensure_member(spec)
                                .await
                                .map_err(|e| BridgeError::Mob(e.to_string()))?;
                        }
                    }
                    Err(respawn_err) => return Err(BridgeError::Mob(respawn_err.to_string())),
                }
                submit_internal_bridge_work(&self.handle, &mid, content, handling_mode).await?;
            }
            Err(err) => return Err(BridgeError::Mob(err.to_string())),
        }

        self.handle
            .resolve_bridge_session_id(&mid)
            .await
            .ok_or_else(|| {
                BridgeError::Mob("member has no bridge session after deliver".to_string())
            })
    }

    async fn checkpoint_session(
        &self,
        _runtime_id: &AgentRuntimeId,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<SessionSnapshot, BridgeError> {
        let store = self.session_store.as_ref().ok_or_else(|| {
            BridgeError::InvalidInput(
                "checkpoint requires a session store but none was configured".to_string(),
            )
        })?;

        let session = store
            .load(session_id)
            .await
            .map_err(|e| BridgeError::Mob(format!("failed to load session for checkpoint: {e}")))?
            .ok_or_else(|| {
                BridgeError::Mob(format!(
                    "session {session_id} not found in store for checkpoint"
                ))
            })?;

        let data = serde_json::to_vec(&session)
            .map_err(|e| BridgeError::Mob(format!("failed to serialize session: {e}")))?;

        Ok(SessionSnapshot { data })
    }

    async fn retire_member(&self, runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
        let mid = MeerkatId::from(runtime_id.as_str());
        match self.handle.retire(mid).await {
            Ok(()) => Ok(()),
            Err(err) if is_archive_not_found_cleanup_error(&err.to_string()) => Ok(()),
            Err(err) => Err(BridgeError::Mob(err.to_string())),
        }
    }

    async fn wire_peer(&self, a: &AgentRuntimeId, b: &AgentRuntimeId) -> Result<(), BridgeError> {
        self.handle
            .wire(
                meerkat_mob::AgentIdentity::from(a.as_str()),
                MeerkatId::from(b.as_str()),
            )
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))
    }

    async fn wire_peers_batch(
        &self,
        edges: &[(AgentRuntimeId, AgentRuntimeId)],
    ) -> Result<(), BridgeError> {
        self.handle
            .wire_members_batch(edges.iter().map(|(a, b)| {
                (
                    meerkat_mob::AgentIdentity::from(a.as_str()),
                    meerkat_mob::AgentIdentity::from(b.as_str()),
                )
            }))
            .await
            .map(|_| ())
            .map_err(|e| BridgeError::Mob(e.to_string()))
    }

    async fn current_member_wires(
        &self,
    ) -> Result<Vec<(AgentRuntimeId, AgentRuntimeId)>, BridgeError> {
        let members = self.handle.list_members_including_retiring().await;
        let active_ids = members
            .iter()
            .map(|member| member.agent_identity.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        let mut edges = std::collections::BTreeSet::new();
        for member in &members {
            let a = member.agent_identity.to_string();
            for peer in &member.wired_to {
                let b = peer.to_string();
                if !active_ids.contains(&b) {
                    continue;
                }
                let key = if a <= b {
                    (a.clone(), b)
                } else {
                    (b, a.clone())
                };
                edges.insert(key);
            }
        }
        Ok(edges
            .into_iter()
            .filter_map(|(a, b)| {
                Some((
                    AgentRuntimeId::parse(&a).ok()?,
                    AgentRuntimeId::parse(&b).ok()?,
                ))
            })
            .collect())
    }

    async fn unwire_peer(&self, a: &AgentRuntimeId, b: &AgentRuntimeId) -> Result<(), BridgeError> {
        match self
            .handle
            .unwire(
                meerkat_mob::AgentIdentity::from(a.as_str()),
                MeerkatId::from(b.as_str()),
            )
            .await
        {
            Ok(()) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                if message.contains("peer not found") || message.contains("not wired") {
                    Ok(())
                } else {
                    Err(BridgeError::Mob(message))
                }
            }
        }
    }

    async fn inspect_member(
        &self,
        runtime_id: &AgentRuntimeId,
    ) -> Result<MemberInspection, BridgeError> {
        let mid = MeerkatId::from(runtime_id.as_str());
        let snap = self
            .handle
            .member_status(&mid)
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;
        Ok(MemberInspection {
            output_preview: snap.output_preview.clone(),
            is_final: snap.is_final,
            peer_reachable_count: snap
                .peer_connectivity
                .as_ref()
                .map(|pc| pc.reachable_peer_count)
                .unwrap_or(0),
        })
    }

    async fn register_session_runtime_state(
        &self,
        session_id: &meerkat_core::types::SessionId,
        identity: &AgentIdentity,
        generation: ContinuityGeneration,
        checkpoint_version: CheckpointVersion,
        fencing_token: FencingToken,
    ) -> Result<CheckpointVersion, BridgeError> {
        if let Some(adapter) = self.continuity_session_store.as_ref() {
            return adapter
                .register_session(
                    session_id,
                    SessionRuntimeState {
                        identity: identity.clone(),
                        generation,
                        checkpoint_version,
                        fencing_token,
                    },
                )
                .await
                .map_err(|err| BridgeError::Mob(format!("continuity register_session: {err}")));
        }
        Ok(checkpoint_version)
    }

    async fn unregister_session_runtime_state(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), BridgeError> {
        if let Some(adapter) = self.continuity_session_store.as_ref() {
            adapter
                .unregister_session(session_id)
                .await
                .map_err(|err| BridgeError::Mob(format!("continuity unregister_session: {err}")))?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use meerkat_core::agent::AgentToolDispatcher;
    use meerkat_core::types::ToolCallView;
    use meerkat_core::{ToolDef, error::ToolError, ops::ToolDispatchOutcome};
    use meerkat_mob::MobRuntimeMode;

    use super::*;
    use crate::identity_first::{AgentAddressability, LocalExternalToolOverlay};

    struct EmptyDispatcher;

    #[async_trait]
    impl AgentToolDispatcher for EmptyDispatcher {
        fn tools(&self) -> Arc<[Arc<ToolDef>]> {
            Arc::from([])
        }

        async fn dispatch(
            &self,
            _call: ToolCallView<'_>,
        ) -> Result<ToolDispatchOutcome, ToolError> {
            Err(ToolError::ExecutionFailed {
                message: "not implemented".to_string(),
            })
        }
    }

    fn durable_spec() -> DurableAgentSpec {
        DurableAgentSpec {
            identity: AgentIdentity::parse("agent:alpha").expect("identity"),
            profile: meerkat_mob::ProfileName::from("worker"),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: Default::default(),
            context: None,
            additional_instructions: Vec::new(),
            initial_message: Some(meerkat_core::ContentInput::Text("hello".to_string())),
            runtime_mode_override: Some(MobRuntimeMode::TurnDriven),
        }
    }

    #[test]
    fn build_spawn_spec_maps_identity_first_overrides() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("team".to_string(), "ops".to_string());
        let draft = AgentBuildDraft {
            model: Some("gpt-test".to_string()),
            system_prompt: Some("system override".to_string()),
            additional_instructions: vec!["stay focused".to_string()],
            labels,
            app_context: Some(serde_json::json!({"ticket": 7})),
            external_tools: Vec::new(),
            local_external_tools: LocalExternalToolOverlay::new(Arc::new(EmptyDispatcher)),
        };

        let spawn = build_spawn_spec(&runtime_id, &durable_spec(), &draft);

        assert_eq!(spawn.model_override.as_deref(), Some("gpt-test"));
        assert_eq!(
            spawn.system_prompt_override,
            Some(SpawnSystemPromptOverride::Replace(
                "system override".to_string()
            ))
        );
        assert!(spawn.external_tools.is_some());
        assert_eq!(spawn.runtime_mode, Some(MobRuntimeMode::TurnDriven));
        assert_eq!(
            spawn.initial_message,
            Some(meerkat_core::ContentInput::Text("hello".to_string()))
        );
        assert_eq!(
            spawn
                .labels
                .as_ref()
                .and_then(|labels| labels.get("team"))
                .map(String::as_str),
            Some("ops")
        );
        assert_eq!(
            spawn
                .labels
                .as_ref()
                .and_then(|labels| labels.get("agent_identity"))
                .map(String::as_str),
            Some("agent:alpha")
        );
    }

    #[test]
    fn bridge_delivery_repair_covers_missing_bridge_session_snapshot() {
        let error = "session bridge mob error: member rt:review:singleton:0 failed to restore session 019e5fc2-dad4-77e2-abbe-a8a66bc15f66: missing bridge session snapshot for '019e5fc2-dad4-77e2-abbe-a8a66bc15f66'";

        assert!(
            is_repairable_bridge_delivery_error(error),
            "stale bridge-session bindings should be repaired before retrying delivery"
        );
        assert!(
            is_repairable_bridge_delivery_error("missing event injector capability for member"),
            "existing stale event-injector repair path must remain covered"
        );
        assert!(
            !is_repairable_bridge_delivery_error("model provider returned rate limit"),
            "ordinary turn failures must not trigger member repair"
        );
    }
}
