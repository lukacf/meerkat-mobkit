//! Session bridge: connects the identity-first control plane to the Meerkat
//! session pipeline for real session creation, delivery, and retirement.

use std::sync::Arc;

use async_trait::async_trait;
use meerkat_mob::launch::MemberLaunchMode;
use meerkat_mob::{MeerkatId, MobHandle, SpawnMemberSpec};

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
}

impl MobSessionBridge {
    /// Create a new bridge wrapping the given mob handle.
    pub fn new(handle: MobHandle) -> Self {
        Self {
            handle,
            session_store: None,
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

        // Retrieve the session ID from the spawned member
        let member = self
            .handle
            .member(&mid)
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;

        member
            .current_session_id()
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?
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
            session_id: session_id.clone(),
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
                let member = self
                    .handle
                    .member(&mid)
                    .await
                    .map_err(|e2| BridgeError::Mob(e2.to_string()))?;
                member
                    .current_session_id()
                    .await
                    .map_err(|e2| BridgeError::Mob(e2.to_string()))?
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

        // Use internal_turn() to bypass the mob-layer external_addressable
        // check. The identity layer owns addressability enforcement — the
        // bridge is an internal delivery mechanism regardless of whether the
        // identity is Addressable or InternalOnly.
        let receipt = member
            .internal_turn(content.clone())
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;

        Ok(receipt.session_id)
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
