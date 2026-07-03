//! Session bridge: connects the identity-first control plane to the Meerkat
//! session pipeline for real session creation, delivery, and retirement.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use meerkat_core::types::HandlingMode;
use meerkat_mob::ids::AgentIdentity as MobAgentIdentity;
use meerkat_mob::launch::MemberLaunchMode;
use meerkat_mob::{
    MobHandle, MobSessionService, SpawnMemberSpec, SpawnSystemPromptOverride, WorkOrigin, WorkRef,
    WorkSpec,
};

use crate::mob_handle_runtime::{
    content_input_has_images, is_previous_member_cleanup_ambiguous_error,
    is_recoverable_lifecycle_cleanup_error, is_recoverable_session_owned_retire_cleanup_error,
    model_capabilities_for_member, topology_restore_failed_peer_ids,
};

use super::adapters::{ContinuitySessionStoreAdapter, SessionRuntimeState};
use super::types::{
    AgentBuildDraft, AgentIdentity, AgentRuntimeId, CheckpointVersion, ContinuityGeneration,
    DurableAgentSpec, FencingToken, SessionSnapshot,
};

fn is_missing_event_injector_error(error: &str) -> bool {
    error.contains("missing event injector capability")
        || (error.contains("missing required capability")
            && error.contains("interaction_event_injector"))
}

fn is_missing_bridge_session_snapshot_error(error: &str) -> bool {
    error.contains("missing bridge session snapshot")
}

fn is_repairable_bridge_delivery_error(error: &str) -> bool {
    is_missing_event_injector_error(error)
        || is_missing_bridge_session_snapshot_error(error)
        || is_previous_member_cleanup_ambiguous_error(error)
}

fn is_recoverable_bridge_respawn_cleanup_error(error: &str) -> bool {
    is_recoverable_lifecycle_cleanup_error(error)
}

fn is_member_already_exists_error(error: &meerkat_mob::MobError) -> bool {
    matches!(error, meerkat_mob::MobError::MemberAlreadyExists(_))
}

#[derive(Debug, PartialEq, Eq)]
enum MemberRepairRespawnFailure {
    DegradedTopologyRestore { failed_peer_ids: Vec<String> },
    RecoverableCleanup,
    Fatal(String),
}

fn classify_member_repair_respawn_failure(
    error: &meerkat_mob::MobRespawnError,
) -> MemberRepairRespawnFailure {
    if let Some(failed_peer_ids) = topology_restore_failed_peer_ids(error) {
        return MemberRepairRespawnFailure::DegradedTopologyRestore { failed_peer_ids };
    }
    if is_recoverable_bridge_respawn_cleanup_error(&error.to_string()) {
        return MemberRepairRespawnFailure::RecoverableCleanup;
    }
    MemberRepairRespawnFailure::Fatal(error.to_string())
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
    member_id: &MobAgentIdentity,
    content: &meerkat_core::ContentInput,
    injected_context: &[meerkat_core::ContentInput],
    handling_mode: HandlingMode,
) -> Result<(), BridgeError> {
    let entry = handle
        .get_member(member_id)
        .await
        .map_err(|err| BridgeError::Mob(err.to_string()))?
        .ok_or_else(|| BridgeError::Mob(format!("member not found: {member_id}")))?;
    // Ask 1: attach ambient memory recall as a separate typed injected-context
    // body rather than fusing it into the user's message text. WorkSpec carries
    // it to the StartTurnRequest, where meerkat stamps each entry as the
    // InjectedContext transcript role (excluded from compaction indexing).
    let mut spec = WorkSpec::new(content.clone(), WorkOrigin::Internal);
    if !injected_context.is_empty() {
        spec = spec.with_injected_context(injected_context.to_vec());
    }
    handle
        .submit_work_with_mode(
            entry.agent_runtime_id.clone(),
            entry.fence_token,
            WorkRef::new(),
            spec,
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

    /// Deliver content plus a separate `injected_context` body (meerkat
    /// 0.7.12 ask 1: typed ambient injection alongside — not fused into —
    /// the user's message). Bridges that do not carry injected context fall
    /// back to plain delivery of the user content, dropping the injection.
    async fn deliver_with_mode_and_context(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        injected_context: &[meerkat_core::ContentInput],
        handling_mode: HandlingMode,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let _ = injected_context;
        self.deliver_with_mode(runtime_id, content, handling_mode)
            .await
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
/// `AgentRuntimeId` is usually used as the `MobAgentIdentity` at the mob layer. Real
/// external bindings are the exception: Meerkat's external peer names require
/// identifier-safe `<mob>/<profile>/<member>` segments, so the bridge maps the
/// runtime ID to the durable identity for those members.
pub struct MobSessionBridge {
    handle: MobHandle,
    /// Session store used for checkpoint (loading session data to serialize).
    session_store: Option<Arc<dyn meerkat::SessionStore>>,
    /// Session service used to project the live effective model for capability checks.
    session_service: Option<Arc<dyn MobSessionService>>,
    /// Continuity-backed session store, when installed by the identity-first builder.
    continuity_session_store: Option<Arc<ContinuitySessionStoreAdapter>>,
    runtime_members: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    runtime_sessions: Arc<tokio::sync::RwLock<HashMap<String, meerkat_core::types::SessionId>>>,
    /// Lazily-minted ops-owner bridge session for external (peer-only)
    /// members, used when the mob was created without machine-bound owner
    /// bridge-session authority. Stable for the bridge lifetime so every
    /// external member shares one generated operation owner.
    generated_external_owner_session: std::sync::OnceLock<meerkat_core::types::SessionId>,
}

impl MobSessionBridge {
    /// Create a new bridge wrapping the given mob handle.
    pub fn new(handle: MobHandle) -> Self {
        Self {
            handle,
            session_store: None,
            session_service: None,
            continuity_session_store: None,
            runtime_members: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            runtime_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            generated_external_owner_session: std::sync::OnceLock::new(),
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
            runtime_members: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            runtime_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            generated_external_owner_session: std::sync::OnceLock::new(),
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
            runtime_members: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            runtime_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            generated_external_owner_session: std::sync::OnceLock::new(),
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
            runtime_members: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            runtime_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            generated_external_owner_session: std::sync::OnceLock::new(),
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
            runtime_members: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            runtime_sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            generated_external_owner_session: std::sync::OnceLock::new(),
        }
    }

    async fn remember_runtime_member(
        &self,
        runtime_id: &AgentRuntimeId,
        member_id: &MobAgentIdentity,
    ) {
        self.runtime_members.write().await.insert(
            runtime_id.as_str().to_string(),
            member_id.as_str().to_string(),
        );
    }

    async fn remember_runtime_session(
        &self,
        runtime_id: &AgentRuntimeId,
        session_id: &meerkat_core::types::SessionId,
    ) {
        self.runtime_sessions
            .write()
            .await
            .insert(runtime_id.as_str().to_string(), session_id.clone());
    }

    async fn forget_runtime_member(&self, runtime_id: &AgentRuntimeId) {
        self.runtime_members
            .write()
            .await
            .remove(runtime_id.as_str());
        self.runtime_sessions
            .write()
            .await
            .remove(runtime_id.as_str());
    }

    async fn member_id_for_runtime_id(&self, runtime_id: &AgentRuntimeId) -> MobAgentIdentity {
        let members = self.runtime_members.read().await;
        members
            .get(runtime_id.as_str())
            .map(|member| MobAgentIdentity::from(member.as_str()))
            // The recompute fallback must mint the same comms-safe roster id
            // as the spawn path (meerkat 0.7 MemberCommsName rejects `:`).
            .unwrap_or_else(|| crate::member_comms_id::mob_member_id(runtime_id.as_str()))
    }

    async fn runtime_session_id(
        &self,
        runtime_id: &AgentRuntimeId,
    ) -> Option<meerkat_core::types::SessionId> {
        self.runtime_sessions
            .read()
            .await
            .get(runtime_id.as_str())
            .cloned()
    }

    /// Resolve the inline definition profile backing `spec.profile`, used as
    /// the base when a draft model override must be projected into the typed
    /// `override_profile` spawn owner. Realm-ref bindings resolve to `None`.
    fn base_profile_for_spec(&self, spec: &DurableAgentSpec) -> Option<meerkat_mob::Profile> {
        self.handle
            .definition()
            .resolve_inline_profile(&spec.profile)
            .cloned()
    }

    async fn resolve_runtime_session_id(
        &self,
        runtime_id: &AgentRuntimeId,
        member_id: &MobAgentIdentity,
        missing_message: &'static str,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        if let Some(session_id) = self.handle.resolve_bridge_session_id(member_id).await {
            self.remember_runtime_session(runtime_id, &session_id).await;
            return Ok(session_id);
        }

        // `get_member` faults (machine command/transport errors) must not be
        // laundered into "member absent"; surface them to the caller.
        if member_id.as_str() != runtime_id.as_str()
            && self
                .handle
                .get_member(member_id)
                .await
                .map_err(|err| BridgeError::Mob(err.to_string()))?
                .is_some()
            && let Some(session_id) = self.runtime_session_id(runtime_id).await
        {
            return Ok(session_id);
        }

        Err(BridgeError::Mob(missing_message.to_string()))
    }

    async fn repair_member_for_delivery(
        &self,
        runtime_id: &AgentRuntimeId,
        member_id: &MobAgentIdentity,
        member_entry_before_delivery: Option<(meerkat_mob::ProfileName, BTreeMap<String, String>)>,
    ) -> Result<(), BridgeError> {
        match self.handle.respawn(member_id.clone(), None).await {
            Ok(_) => Ok(()),
            Err(respawn_err) => match classify_member_repair_respawn_failure(&respawn_err) {
                MemberRepairRespawnFailure::DegradedTopologyRestore { failed_peer_ids } => {
                    tracing::warn!(
                        runtime_id = %runtime_id,
                        member_id = %member_id,
                        failed_peer_count = failed_peer_ids.len(),
                        failed_peer_ids = ?failed_peer_ids,
                        "identity bridge respawn restored member with isolated peer edges; continuing delivery"
                    );
                    // meerkat-mob raises this only after the member/session is live; only peer edges are incomplete.
                    Ok(())
                }
                MemberRepairRespawnFailure::RecoverableCleanup => {
                    // A `get_member` fault must not be read as "member absent"
                    // (that would trigger a spurious re-spawn); fail the
                    // delivery repair instead.
                    if self
                        .handle
                        .get_member(member_id)
                        .await
                        .map_err(|err| BridgeError::Mob(err.to_string()))?
                        .is_none()
                        && let Some((role, labels)) = member_entry_before_delivery
                    {
                        let mut spec = SpawnMemberSpec::new(role, member_id.clone());
                        if !labels.is_empty() {
                            spec = spec.with_labels(labels);
                        }
                        self.handle
                            .ensure_member(spec)
                            .await
                            .map_err(|e| BridgeError::Mob(e.to_string()))?;
                    }
                    Ok(())
                }
                MemberRepairRespawnFailure::Fatal(message) => Err(BridgeError::Mob(message)),
            },
        }
    }

    /// Ops-owner bridge session for external (peer-only) member operations.
    ///
    /// meerkat 0.7.1 fails external member provisioning closed unless the
    /// spawn carries a generated owner binding (owner bridge session + ops
    /// registry; see `MultiBackendProvisioner::provision_member`). Prefer the
    /// mob's machine-bound owner bridge-session authority when it exists;
    /// otherwise mint one stable session id for this bridge — the runtime
    /// adapter creates local session resources for it on demand, exactly like
    /// meerkat's own external smoke supervisors.
    fn external_owner_bridge_session_id(&self) -> meerkat_core::types::SessionId {
        if let Some(authority) = self.handle.owner_bridge_session_lifecycle_authority() {
            return authority.bridge_session_id;
        }
        self.generated_external_owner_session
            .get_or_init(meerkat_core::types::SessionId::new)
            .clone()
    }

    /// Spawn a mob member from a fully-built spec, attaching the generated
    /// owner context that meerkat 0.7.1 requires for external (peer-only)
    /// bindings. Session-backed specs spawn through the plain path.
    async fn spawn_member_spec(
        &self,
        spawn_spec: SpawnMemberSpec,
    ) -> Result<(), meerkat_mob::MobError> {
        if spawn_spec_requires_generated_owner_context(&spawn_spec) {
            let owner_session_id = self.external_owner_bridge_session_id();
            Box::pin(
                self.handle
                    .spawn_spec_with_generated_owner_context(spawn_spec, owner_session_id),
            )
            .await
            .map(|_| ())
        } else {
            Box::pin(self.handle.spawn_spec(spawn_spec))
                .await
                .map(|_| ())
        }
    }

    async fn spawn_member_spec_replacing_collision(
        &self,
        runtime_id: &AgentRuntimeId,
        member_id: &MobAgentIdentity,
        spawn_spec: SpawnMemberSpec,
    ) -> Result<(), meerkat_mob::MobError> {
        match self.spawn_member_spec(spawn_spec.clone()).await {
            Ok(()) => Ok(()),
            Err(error) if is_member_already_exists_error(&error) => {
                tracing::warn!(
                    runtime_id = %runtime_id,
                    member_id = %member_id,
                    error = %error,
                    "fresh-spawn fallback collided with an existing member; retiring and retrying with adopted spec"
                );
                match self.handle.retire(member_id.clone()).await {
                    Ok(()) => {}
                    Err(err)
                        if is_recoverable_session_owned_retire_cleanup_error(&err.to_string()) => {}
                    Err(err) => return Err(err),
                }
                self.forget_runtime_member(runtime_id).await;
                self.spawn_member_spec(spawn_spec).await
            }
            Err(error) => Err(error),
        }
    }
}

/// Project meerkat 0.7's tri-state peer connectivity into an inspect-level
/// reachable count, when the tri-state resolves one.
///
/// Only a resolved probe ([`WirePeerConnectivity::Known`]) contributes a
/// live count. The not-applicable / probe-timed-out arms (and an uncomputed
/// projection) return `None` so the caller falls back to the machine-owned
/// wiring degree (`wired_to.len()`) instead of projecting 0 — a freshly
/// wired member has peers regardless of whether a live probe resolved, and
/// the sibling console alias surface computes the same wire field from
/// `wired_to`; the two surfaces must agree.
fn peer_reachable_count_from_connectivity(
    connectivity: Option<&meerkat_contracts::WirePeerConnectivity>,
) -> Option<usize> {
    match connectivity {
        Some(meerkat_contracts::WirePeerConnectivity::Known { snapshot }) => {
            Some(snapshot.reachable_peer_count)
        }
        Some(
            meerkat_contracts::WirePeerConnectivity::NotApplicable
            | meerkat_contracts::WirePeerConnectivity::ProbeTimedOut,
        )
        | None => None,
    }
}

/// External (peer-only) member provisioning on meerkat 0.7.1 requires a
/// machine-minted owner context; plain `spawn_spec` fails closed by design.
pub(crate) fn spawn_spec_requires_generated_owner_context(spawn_spec: &SpawnMemberSpec) -> bool {
    matches!(
        spawn_spec.binding,
        Some(meerkat_mob::RuntimeBinding::External { .. })
    )
}

fn spec_uses_external_binding(spec: &DurableAgentSpec) -> bool {
    matches!(spec.backend, Some(meerkat_mob::MobBackendKind::External))
        || matches!(
            spec.binding.as_ref(),
            Some(meerkat_contracts::WireRuntimeBinding::External { .. })
        )
}

fn member_id_for_spawn_spec(
    runtime_id: &AgentRuntimeId,
    spec: &DurableAgentSpec,
) -> MobAgentIdentity {
    // meerkat 0.7's `MemberCommsName` is fail-closed: roster member ids must
    // be identifier-safe (no `:`). MobKit's public alias space — durable
    // identities like `review:singleton` and runtime ids like
    // `rt:review:singleton:0` — is unchanged; the roster id is the comms-safe
    // encoding (identity for already-safe names).
    if spec_uses_external_binding(spec) {
        crate::member_comms_id::mob_member_id(spec.identity.as_str())
    } else {
        crate::member_comms_id::mob_member_id(runtime_id.as_str())
    }
}

/// Build a `SpawnMemberSpec` from identity-first types, wiring draft fields.
///
/// `base_profile` is the resolved definition profile for `spec.profile`; it is
/// only needed when the draft carries a model override (meerkat 0.7 removed
/// `SpawnMemberSpec::model_override` in favor of the typed `override_profile`
/// owner, so a model override is expressed as the role profile with the model
/// swapped).
pub(crate) fn build_spawn_spec(
    runtime_id: &AgentRuntimeId,
    spec: &DurableAgentSpec,
    draft: &AgentBuildDraft,
    base_profile: Option<&meerkat_mob::Profile>,
) -> SpawnMemberSpec {
    let mid = member_id_for_spawn_spec(runtime_id, spec);
    let mut spawn_spec = SpawnMemberSpec::new(spec.profile.clone(), mid);

    if let Some(message) = spec.initial_message.as_ref() {
        spawn_spec = spawn_spec.with_initial_message(message.clone());
    }
    if let Some(runtime_mode) = spec.runtime_mode_override {
        spawn_spec = spawn_spec.with_runtime_mode(runtime_mode);
    }
    spawn_spec.backend = spec.backend;
    if let Some(binding) = spec.binding.clone() {
        spawn_spec.binding = runtime_binding_from_wire(binding);
    }
    if let Some(ref ctx) = draft.app_context {
        spawn_spec = spawn_spec.with_context(ctx.clone());
    }
    let mut labels = draft.labels.clone();
    labels.insert(
        "agent_identity".to_string(),
        spec.identity.as_str().to_string(),
    );
    labels.insert(
        "profile_name".to_string(),
        spec.profile.as_str().to_string(),
    );
    if !labels.is_empty() {
        spawn_spec = spawn_spec.with_labels(labels);
    }
    if !draft.additional_instructions.is_empty() {
        spawn_spec = spawn_spec.with_additional_instructions(draft.additional_instructions.clone());
    }
    if let Some(model) = draft.model.as_ref() {
        match base_profile {
            Some(base) => {
                let mut profile = base.clone();
                profile.model = model.clone();
                // The base profile's pinned provider (and self-hosted server
                // binding) belongs to its original model id; clear it so the
                // catalog re-infers the provider for the overridden model.
                profile.provider = None;
                profile.self_hosted_server_id = None;
                spawn_spec.override_profile = Some(profile);
            }
            None => {
                // No resolvable inline base profile (realm-ref binding or
                // unknown role). Leave `override_profile` unset so the spawn
                // resolves through the definition's canonical path; the model
                // override cannot be applied without a base profile.
                tracing::warn!(
                    identity = %spec.identity,
                    profile = %spec.profile,
                    model = %model,
                    "model override skipped: role profile is not an inline definition profile"
                );
            }
        }
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

fn runtime_binding_from_wire(
    binding: meerkat_contracts::WireRuntimeBinding,
) -> Option<meerkat_mob::RuntimeBinding> {
    match binding {
        meerkat_contracts::WireRuntimeBinding::Session => {
            Some(meerkat_mob::RuntimeBinding::Session)
        }
        meerkat_contracts::WireRuntimeBinding::External {
            address,
            bootstrap_token,
            identity,
        } => {
            let resolved = identity.resolve().ok()?;
            Some(meerkat_mob::RuntimeBinding::External {
                peer_id: resolved.peer_id.to_string(),
                address,
                bootstrap_token,
                pubkey: resolved.pubkey,
            })
        }
    }
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
        let mid = member_id_for_spawn_spec(runtime_id, spec);
        let spawn_spec = build_spawn_spec(
            runtime_id,
            spec,
            draft,
            self.base_profile_for_spec(spec).as_ref(),
        );

        self.spawn_member_spec(spawn_spec)
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;
        self.remember_runtime_member(runtime_id, &mid).await;
        self.remember_runtime_session(runtime_id, session_id).await;

        self.resolve_runtime_session_id(runtime_id, &mid, "member spawned but has no session ID")
            .await
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
        if spec_uses_external_binding(spec) {
            let mut spawn_spec = build_spawn_spec(
                runtime_id,
                spec,
                draft,
                self.base_profile_for_spec(spec).as_ref(),
            );
            spawn_spec.launch_mode = MemberLaunchMode::Resume {
                bridge_session_id: session_id.clone(),
            };
            let mid = member_id_for_spawn_spec(runtime_id, spec);
            self.spawn_member_spec(spawn_spec)
                .await
                .map_err(|e| BridgeError::Mob(e.to_string()))?;
            self.remember_runtime_member(runtime_id, &mid).await;
            self.remember_runtime_session(runtime_id, session_id).await;
            return Ok(ResumeSessionOutcome::Resumed {
                session_id: session_id.clone(),
            });
        }

        // Try MemberLaunchMode::Resume first — this loads the existing session
        // from the session store (conversation history intact).
        let mut spawn_spec = build_spawn_spec(
            runtime_id,
            spec,
            draft,
            self.base_profile_for_spec(spec).as_ref(),
        );
        spawn_spec.launch_mode = MemberLaunchMode::Resume {
            bridge_session_id: session_id.clone(),
        };

        let mid = member_id_for_spawn_spec(runtime_id, spec);

        match self.spawn_member_spec(spawn_spec).await {
            Ok(()) => {
                self.remember_runtime_member(runtime_id, &mid).await;
                self.remember_runtime_session(runtime_id, session_id).await;
                Ok(ResumeSessionOutcome::Resumed {
                    session_id: session_id.clone(),
                })
            }
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
                let fresh_spec = build_spawn_spec(
                    runtime_id,
                    spec,
                    draft,
                    self.base_profile_for_spec(spec).as_ref(),
                );
                self.spawn_member_spec_replacing_collision(runtime_id, &mid, fresh_spec)
                    .await
                    .map_err(|e2| BridgeError::Mob(e2.to_string()))?;

                self.remember_runtime_member(runtime_id, &mid).await;
                let session_id = self
                    .resolve_runtime_session_id(
                        runtime_id,
                        &mid,
                        "member spawned (fresh fallback) but has no session ID",
                    )
                    .await?;
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
        let mid = self.member_id_for_runtime_id(runtime_id).await;
        // Best-effort repair material: a faulted lookup degrades to "no
        // pre-delivery entry" (the delivery itself will surface the fault).
        let member_entry_before_delivery = self
            .handle
            .get_member(&mid)
            .await
            .ok()
            .flatten()
            .map(|entry| (entry.role, entry.labels));
        if content_input_has_images(content) {
            let member_entry = self
                .handle
                .get_member(&mid)
                .await
                .map_err(|err| BridgeError::Mob(err.to_string()))?
                .ok_or_else(|| {
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
        match submit_internal_bridge_work(&self.handle, &mid, content, &[], HandlingMode::Queue)
            .await
        {
            Ok(()) => {}
            Err(err) if is_repairable_bridge_delivery_error(&err.to_string()) => {
                tracing::warn!(
                    runtime_id = %runtime_id,
                    error = %err,
                    "identity bridge delivery found stale runtime state; repairing member before retry"
                );
                Box::pin(self.repair_member_for_delivery(
                    runtime_id,
                    &mid,
                    member_entry_before_delivery,
                ))
                .await?;
                submit_internal_bridge_work(&self.handle, &mid, content, &[], HandlingMode::Queue)
                    .await?;
            }
            Err(err) => return Err(BridgeError::Mob(err.to_string())),
        }

        // Meerkat 0.6: MemberDeliveryReceipt no longer carries session_id.
        // Query the bridge session id directly from the mob handle.
        self.resolve_runtime_session_id(
            runtime_id,
            &mid,
            "member has no bridge session after deliver",
        )
        .await
    }

    async fn deliver_with_mode(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        self.deliver_with_mode_and_context(runtime_id, content, &[], handling_mode)
            .await
    }

    async fn deliver_with_mode_and_context(
        &self,
        runtime_id: &AgentRuntimeId,
        content: &meerkat_core::ContentInput,
        injected_context: &[meerkat_core::ContentInput],
        handling_mode: HandlingMode,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        let mid = self.member_id_for_runtime_id(runtime_id).await;
        // Best-effort repair material: a faulted lookup degrades to "no
        // pre-delivery entry" (the delivery itself will surface the fault).
        let member_entry_before_delivery = self
            .handle
            .get_member(&mid)
            .await
            .ok()
            .flatten()
            .map(|entry| (entry.role, entry.labels));
        if content_input_has_images(content) {
            let member_entry = self
                .handle
                .get_member(&mid)
                .await
                .map_err(|err| BridgeError::Mob(err.to_string()))?
                .ok_or_else(|| {
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

        match submit_internal_bridge_work(
            &self.handle,
            &mid,
            content,
            injected_context,
            handling_mode,
        )
        .await
        {
            Ok(()) => {}
            Err(err) if is_repairable_bridge_delivery_error(&err.to_string()) => {
                tracing::warn!(
                    runtime_id = %runtime_id,
                    error = %err,
                    "identity bridge delivery found stale runtime state; repairing member before retry"
                );
                Box::pin(self.repair_member_for_delivery(
                    runtime_id,
                    &mid,
                    member_entry_before_delivery,
                ))
                .await?;
                submit_internal_bridge_work(
                    &self.handle,
                    &mid,
                    content,
                    injected_context,
                    handling_mode,
                )
                .await?;
            }
            Err(err) => return Err(BridgeError::Mob(err.to_string())),
        }

        self.resolve_runtime_session_id(
            runtime_id,
            &mid,
            "member has no bridge session after deliver",
        )
        .await
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
        let mid = self.member_id_for_runtime_id(runtime_id).await;
        match self.handle.retire(mid).await {
            Ok(()) => {
                self.forget_runtime_member(runtime_id).await;
                Ok(())
            }
            // All callers of `retire_member` are identity-first session-owned
            // agents, so a mob-archive miss (NotFound for a registered runtime
            // session) is the expected outcome of disposing one — not an
            // orphan. Tolerate it so reset/delete_identity complete instead of
            // bricking the identity until a process restart.
            Err(err) if is_recoverable_session_owned_retire_cleanup_error(&err.to_string()) => {
                // Disposal completed and the member left the roster, so the
                // runtime-member mapping is stale — forget it (matching the
                // success path) instead of leaking one entry per tolerated
                // retire across the process lifetime.
                self.forget_runtime_member(runtime_id).await;
                Ok(())
            }
            Err(err) => Err(BridgeError::Mob(err.to_string())),
        }
    }

    async fn wire_peer(&self, a: &AgentRuntimeId, b: &AgentRuntimeId) -> Result<(), BridgeError> {
        let member_a = self.member_id_for_runtime_id(a).await;
        let member_b = self.member_id_for_runtime_id(b).await;
        self.handle
            .wire(
                meerkat_mob::AgentIdentity::from(member_a.as_str()),
                member_b,
            )
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))
    }

    async fn wire_peers_batch(
        &self,
        edges: &[(AgentRuntimeId, AgentRuntimeId)],
    ) -> Result<(), BridgeError> {
        let mut member_edges = Vec::with_capacity(edges.len());
        for (a, b) in edges {
            let member_a = self.member_id_for_runtime_id(a).await;
            let member_b = self.member_id_for_runtime_id(b).await;
            member_edges.push((
                meerkat_mob::AgentIdentity::from(member_a.as_str()),
                meerkat_mob::AgentIdentity::from(member_b.as_str()),
            ));
        }
        self.handle
            .wire_members_batch(member_edges)
            .await
            .map(|_| ())
            .map_err(|e| BridgeError::Mob(e.to_string()))
    }

    async fn current_member_wires(
        &self,
    ) -> Result<Vec<(AgentRuntimeId, AgentRuntimeId)>, BridgeError> {
        let members = self.handle.list_members_including_retiring().await;
        let runtime_members = self.runtime_members.read().await;
        let member_runtimes = runtime_members
            .iter()
            .map(|(runtime, member)| (member.clone(), runtime.clone()))
            .collect::<HashMap<_, _>>();
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
                // Fallback for members not in the in-memory map: the roster id
                // is the comms-safe encoding of the runtime alias; decode it.
                let a = member_runtimes
                    .get(&a)
                    .cloned()
                    .unwrap_or_else(|| crate::member_comms_id::runtime_alias_str(&a).into_owned());
                let b = member_runtimes
                    .get(&b)
                    .cloned()
                    .unwrap_or_else(|| crate::member_comms_id::runtime_alias_str(&b).into_owned());
                Some((
                    AgentRuntimeId::parse(&a).ok()?,
                    AgentRuntimeId::parse(&b).ok()?,
                ))
            })
            .collect())
    }

    async fn unwire_peer(&self, a: &AgentRuntimeId, b: &AgentRuntimeId) -> Result<(), BridgeError> {
        let member_a = self.member_id_for_runtime_id(a).await;
        let member_b = self.member_id_for_runtime_id(b).await;
        match self
            .handle
            .unwire(
                meerkat_mob::AgentIdentity::from(member_a.as_str()),
                member_b,
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
        let mid = self.member_id_for_runtime_id(runtime_id).await;
        let snap = self
            .handle
            .member_status(&mid)
            .await
            .map_err(|e| BridgeError::Mob(e.to_string()))?;
        let peer_reachable_count =
            match peer_reachable_count_from_connectivity(snap.peer_connectivity.as_ref()) {
                Some(count) => count,
                None => self
                    .handle
                    .get_member(&mid)
                    .await
                    .ok()
                    .flatten()
                    .map(|entry| entry.wired_to.len())
                    .unwrap_or(0),
            };
        Ok(MemberInspection {
            output_preview: snap.output_preview.clone(),
            is_final: snap.is_final,
            peer_reachable_count,
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
    use meerkat_mob::{MobRespawnError, MobRuntimeMode};

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
            backend: None,
            binding: None,
        }
    }

    /// Regression: meerkat 0.7.1 made `member_status` peer connectivity a
    /// tri-state. Only the `Known` arm carries a live count; the
    /// not-applicable / probe-timed-out arms must defer to the machine-owned
    /// wiring degree (`wired_to.len()`) instead of projecting 0, or
    /// `peer_reachable_count` reads 0 for freshly role-wired members and
    /// topology verification breaks.
    #[test]
    fn peer_reachable_count_tri_state_defers_to_wiring_when_unresolved() {
        use meerkat_contracts::{WirePeerConnectivity, WirePeerConnectivitySnapshot};

        let known = WirePeerConnectivity::Known {
            snapshot: WirePeerConnectivitySnapshot {
                reachable_peer_count: 3,
                unknown_peer_count: 0,
                unreachable_peers: Vec::new(),
            },
        };
        assert_eq!(
            peer_reachable_count_from_connectivity(Some(&known)),
            Some(3),
            "a resolved probe owns the count"
        );
        assert_eq!(
            peer_reachable_count_from_connectivity(Some(&WirePeerConnectivity::NotApplicable)),
            None,
            "not-applicable must defer to the wiring fallback"
        );
        assert_eq!(
            peer_reachable_count_from_connectivity(Some(&WirePeerConnectivity::ProbeTimedOut)),
            None,
            "probe timeout must defer to the wiring fallback"
        );
        assert_eq!(peer_reachable_count_from_connectivity(None), None);
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

        let base_profile: meerkat_mob::Profile =
            serde_json::from_value(serde_json::json!({"model": "base-model"}))
                .expect("minimal profile");
        let spawn = build_spawn_spec(&runtime_id, &durable_spec(), &draft, Some(&base_profile));

        assert_eq!(
            spawn
                .override_profile
                .as_ref()
                .map(|profile| profile.model.as_str()),
            Some("gpt-test")
        );
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
        assert_eq!(
            spawn
                .labels
                .as_ref()
                .and_then(|labels| labels.get("profile_name"))
                .map(String::as_str),
            Some("worker"),
            "identity-first spawn labels must carry the adopted profile so the \
             SDK build callback sees the roster profile, not a checkpoint default"
        );
        assert_eq!(
            spawn.role_name.as_str(),
            "worker",
            "SpawnMemberSpec role remains the authoritative mob profile"
        );
    }

    #[test]
    fn fresh_fallback_collision_classifier_matches_member_already_exists() {
        let error = meerkat_mob::MobError::MemberAlreadyExists(meerkat_mob::AgentIdentity::from(
            "rt-agent-alpha-0",
        ));

        assert!(
            is_member_already_exists_error(&error),
            "fresh fallback must retry recreate-over-running-member collisions"
        );
    }

    #[test]
    fn build_spawn_spec_maps_remote_runtime_binding() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let mut spec = durable_spec();
        spec.backend = Some(meerkat_mob::MobBackendKind::External);
        spec.binding = Some(
            serde_json::from_value(serde_json::json!({
                "kind": "external",
                "address": "tcp://127.0.0.1:4777",
                "identity": {
                    "kind": "ed25519_public_key",
                    "public_key": "ed25519:BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc="
                }
            }))
            .expect("wire binding"),
        );
        let draft = AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: Vec::new(),
            labels: Default::default(),
            app_context: None,
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
        };

        let spawn = build_spawn_spec(&runtime_id, &spec, &draft, None);

        // meerkat 0.7: MemberCommsName is fail-closed (no `:` in member-id
        // components), so the roster id is the comms-safe encoding of the
        // public identity `agent:alpha` (see crate::member_comms_id).
        assert_eq!(
            spawn.identity.as_str(),
            crate::member_comms_id::mob_member_id_str("agent:alpha").as_ref()
        );
        assert_eq!(spawn.backend, Some(meerkat_mob::MobBackendKind::External));
        assert!(
            matches!(
                spawn.binding,
                Some(meerkat_mob::RuntimeBinding::External { .. })
            ),
            "expected external runtime binding, got {:?}",
            spawn.binding
        );
        if let Some(meerkat_mob::RuntimeBinding::External {
            address, pubkey, ..
        }) = spawn.binding
        {
            assert_eq!(address.as_str(), "tcp://127.0.0.1:4777");
            assert_eq!(pubkey, [7; 32]);
        }
    }

    #[test]
    fn external_binding_spawn_specs_require_generated_owner_context() {
        let runtime_id = AgentRuntimeId::parse("rt:agent:alpha:0").expect("runtime id");
        let draft = AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: Vec::new(),
            labels: Default::default(),
            app_context: None,
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
        };

        // Session-backed members keep the plain spawn path.
        let session_spawn = build_spawn_spec(&runtime_id, &durable_spec(), &draft, None);
        assert!(
            !spawn_spec_requires_generated_owner_context(&session_spawn),
            "session-backed spawns must not require a generated owner context"
        );

        // meerkat 0.7.1 MultiBackendProvisioner::provision_member fails
        // external (peer-only) members closed without a generated owner
        // binding; the bridge must route them through
        // spawn_spec_with_generated_owner_context.
        let mut spec = durable_spec();
        spec.backend = Some(meerkat_mob::MobBackendKind::External);
        spec.binding = Some(
            serde_json::from_value(serde_json::json!({
                "kind": "external",
                "address": "tcp://127.0.0.1:4777",
                "identity": {
                    "kind": "ed25519_public_key",
                    "public_key": "ed25519:BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc="
                }
            }))
            .expect("wire binding"),
        );
        let external_spawn = build_spawn_spec(&runtime_id, &spec, &draft, None);
        assert!(
            spawn_spec_requires_generated_owner_context(&external_spawn),
            "external peer-only spawns must carry a generated owner binding on meerkat 0.7.1"
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
            is_repairable_bridge_delivery_error(
                "mob member rt:us-president:0 missing required capability interaction_event_injector: autonomous member dispatch"
            ),
            "newer autonomous member dispatch wording should repair and retry instead of dropping the event"
        );
        assert!(
            is_repairable_bridge_delivery_error(
                "previous member cleanup ambiguous for member rt:deep-investigator:singleton:0"
            ),
            "ambiguous Meerkat respawn cleanup should trigger bridge repair instead of failing delivery"
        );
        assert!(
            !is_repairable_bridge_delivery_error("model provider returned rate limit"),
            "ordinary turn failures must not trigger member repair"
        );
    }

    #[test]
    fn bridge_delivery_repair_classifies_topology_restore_failure_as_degraded() {
        let identity = meerkat_mob::AgentIdentity::from("rt:review:singleton:0");
        let receipt = meerkat_mob::MemberRespawnReceipt::new(
            identity.clone(),
            meerkat_mob::AgentRuntimeId::new(identity, meerkat_mob::ids::Generation::INITIAL),
            meerkat_mob::FenceToken::new(1),
            meerkat_mob::FenceToken::new(2),
        );
        let err = MobRespawnError::TopologyRestoreFailed {
            receipt,
            failed_peer_ids: vec![meerkat_mob::RespawnTopologyPeerId::from(
                "initiative:broken",
            )],
        };

        assert_eq!(
            classify_member_repair_respawn_failure(&err),
            MemberRepairRespawnFailure::DegradedTopologyRestore {
                failed_peer_ids: vec!["initiative:broken".to_string()]
            },
            "failed peer edges should degrade bridge repair instead of bricking delivery"
        );
        assert!(
            matches!(
                classify_member_repair_respawn_failure(&MobRespawnError::NoRuntimeControl {
                    identity: meerkat_mob::AgentIdentity::from("rt:review:singleton:0"),
                }),
                MemberRepairRespawnFailure::Fatal(_)
            ),
            "ordinary respawn failures must still fail bridge repair"
        );
    }
}
