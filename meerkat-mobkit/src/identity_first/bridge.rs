//! Session bridge: connects the identity-first control plane to the Meerkat
//! session pipeline for real session creation, delivery, and retirement.

use std::sync::Arc;

use async_trait::async_trait;
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::launch::MemberLaunchMode;
use meerkat_mob::{MobHandle, MobSessionService, SpawnMemberSpec};

use crate::mob_handle_runtime::{content_input_has_images, model_capabilities_for_member};

use super::types::{
    AgentBuildDraft, AgentIdentity, AgentRuntimeId, DurableAgentSpec, SessionSnapshot,
};

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
    ) -> Result<meerkat_core::types::SessionId, BridgeError>;

    /// Deliver content to an active mob member.
    async fn deliver(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
    ) -> Result<meerkat_core::types::SessionId, BridgeError>;

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
}

impl MobSessionBridge {
    /// Create a new bridge wrapping the given mob handle.
    pub fn new(handle: MobHandle) -> Self {
        Self {
            handle,
            session_store: None,
            session_service: None,
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
        }
    }
}

/// Build a `SpawnMemberSpec` from identity-first types, wiring draft fields.
fn build_spawn_spec(
    runtime_id: &AgentRuntimeId,
    spec: &DurableAgentSpec,
    draft: &AgentBuildDraft,
) -> SpawnMemberSpec {
    let mid = MeerkatId::from(runtime_id.as_str());
    let mut spawn_spec = SpawnMemberSpec::new(spec.profile.clone(), mid);

    if let Some(ref ctx) = draft.app_context {
        spawn_spec = spawn_spec.with_context(ctx.clone());
    }
    if !draft.labels.is_empty() {
        spawn_spec = spawn_spec.with_labels(draft.labels.clone());
    }
    if !draft.additional_instructions.is_empty() {
        spawn_spec = spawn_spec.with_additional_instructions(draft.additional_instructions.clone());
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
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let mid = MeerkatId::from(runtime_id.as_str());
        let spawn_spec = build_spawn_spec(runtime_id, spec, draft);

        self.handle
            .spawn_spec(spawn_spec)
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;

        self.handle
            .resolve_bridge_session_id(&mid)
            .await
            .ok_or_else(|| BridgeError::Mob("member spawned but has no session ID".to_string()))
    }

    async fn resume_session(
        &self,
        _identity: &AgentIdentity,
        runtime_id: &AgentRuntimeId,
        spec: &DurableAgentSpec,
        draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
        _snapshot: &SessionSnapshot,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        // Try MemberLaunchMode::Resume first — this loads the existing session
        // from the session store (conversation history intact).
        let mut spawn_spec = build_spawn_spec(runtime_id, spec, draft);
        spawn_spec.launch_mode = MemberLaunchMode::Resume {
            bridge_session_id: session_id.clone(),
        };

        match self.handle.spawn_spec(spawn_spec).await {
            Ok(_) => Ok(session_id.clone()),
            Err(e) => {
                // Resume can fail if the old session's comms identity is still
                // claimed (e.g., in-process restart where the previous mob actor
                // hasn't fully terminated). Fall back to a fresh spawn.
                tracing::warn!(
                    identity = %_identity,
                    session_id = %session_id,
                    error = %e,
                    "resume_session failed, falling back to fresh spawn"
                );
                let fresh_spec = build_spawn_spec(runtime_id, spec, draft);
                self.handle
                    .spawn_spec(fresh_spec)
                    .await
                    .map_err(|e2| BridgeError::Mob(e2.to_string()))?;

                let mid = MeerkatId::from(runtime_id.as_str());
                self.handle
                    .resolve_bridge_session_id(&mid)
                    .await
                    .ok_or_else(|| {
                        BridgeError::Mob(
                            "member spawned (fresh fallback) but has no session ID".to_string(),
                        )
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
        let member = self
            .handle
            .member(&mid)
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;
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

        // Use internal_turn() to bypass the mob-layer external_addressable
        // check. The identity layer owns addressability enforcement — the
        // bridge is an internal delivery mechanism regardless of whether the
        // identity is Addressable or InternalOnly.
        let _receipt = member
            .internal_turn(content.clone())
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;

        // Meerkat 0.6: MemberDeliveryReceipt no longer carries session_id.
        // Query the bridge session id directly from the mob handle.
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
        self.handle
            .retire(mid)
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;
        Ok(())
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

    async fn unwire_peer(&self, a: &AgentRuntimeId, b: &AgentRuntimeId) -> Result<(), BridgeError> {
        self.handle
            .unwire(
                meerkat_mob::AgentIdentity::from(a.as_str()),
                MeerkatId::from(b.as_str()),
            )
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))
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
}
