//! Mob member lifecycle management — bootstrap, spawn, reconcile, and roster queries.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use meerkat::{AgentFactory, Config, FactoryAgentBuilder, SessionStore};
use meerkat_client::LlmClient;
use meerkat_core::AgentSessionStore;
use meerkat_core::service::{CreateSessionRequest, SessionError};
use meerkat_mob::MobRun;
use meerkat_mob::launch::{ForkContext, MemberLaunchMode};
use meerkat_mob::{
    HelperOptions, HelperResult, MeerkatId, MemberRef, MemberSessionRef, MemberState, MobBuilder,
    MobDefinition, MobError, MobHandle, MobMemberSnapshot as RichMobMemberSnapshot,
    MobSessionService, MobState, MobStorage, ProfileName, RosterEntry, RunId, SpawnMemberSpec,
};
use meerkat_store::StoreAdapter;
use serde::{Deserialize, Serialize};

/// Member state constant for active members.
pub const MEMBER_STATE_ACTIVE: &str = "active";
/// Member state constant for members transitioning to retired.
pub const MEMBER_STATE_RETIRING: &str = "retiring";

/// Options for bootstrapping a mob runtime.
#[derive(Clone, Default)]
pub struct MobBootstrapOptions {
    pub allow_ephemeral_sessions: bool,
    pub notify_orchestrator_on_resume: bool,
    pub default_llm_client: Option<Arc<dyn LlmClient>>,
}

/// Hook called before each session is created. Receives the mutable
/// `CreateSessionRequest` so the app can inject external tools, augment the
/// system prompt, set labels, override the model, etc.
///
/// The hook runs **before** `create_session` captures labels and LLM identity,
/// so all mutations are reflected in session metadata, not just the agent build.
///
/// ```rust,ignore
/// let spec = MobBootstrapSpec::persistent_with_hook(
///     definition, storage, store_path, 64, session_store,
///     |req: &mut CreateSessionRequest| {
///         // Inject per-agent tools
///         let build = req.build.get_or_insert_with(SessionBuildOptions::default);
///         build.external_tools = Some(my_tools());
///         // Augment system prompt
///         req.system_prompt = Some(format!(
///             "{}\n{}",
///             req.system_prompt.as_deref().unwrap_or(""),
///             my_dynamic_context(),
///         ));
///     },
/// );
/// ```
pub type PreBuildHook = Arc<dyn Fn(&mut CreateSessionRequest) + Send + Sync>;

/// Wraps a `MobSessionService`, applying a `PreBuildHook` to the
/// `CreateSessionRequest` in `create_session()` before delegating.
///
/// The hook runs before labels and LLM identity are captured by the inner
/// session service, so mutations to `req.labels`, `req.model`, `req.build`,
/// and `req.system_prompt` are fully reflected in session metadata.
struct PreBuildMobSessionService {
    inner: Arc<dyn MobSessionService>,
    hook: PreBuildHook,
}

// All delegation uses fully-qualified paths to avoid import issues with
// types that live in different meerkat_core sub-modules.

#[async_trait]
impl meerkat_core::service::SessionService for PreBuildMobSessionService {
    async fn create_session(
        &self,
        mut req: CreateSessionRequest,
    ) -> Result<meerkat_core::types::RunResult, SessionError> {
        (self.hook)(&mut req);
        self.inner.create_session(req).await
    }

    async fn start_turn(
        &self,
        id: &meerkat_core::types::SessionId,
        req: meerkat_core::service::StartTurnRequest,
    ) -> Result<meerkat_core::types::RunResult, SessionError> {
        self.inner.start_turn(id, req).await
    }

    async fn interrupt(&self, id: &meerkat_core::types::SessionId) -> Result<(), SessionError> {
        self.inner.interrupt(id).await
    }

    async fn set_session_client(
        &self,
        id: &meerkat_core::types::SessionId,
        client: Arc<dyn meerkat_core::AgentLlmClient>,
    ) -> Result<(), SessionError> {
        self.inner.set_session_client(id, client).await
    }

    async fn hot_swap_session_llm_identity(
        &self,
        id: &meerkat_core::types::SessionId,
        client: Arc<dyn meerkat_core::AgentLlmClient>,
        identity: meerkat_core::session::SessionLlmIdentity,
    ) -> Result<(), SessionError> {
        self.inner
            .hot_swap_session_llm_identity(id, client, identity)
            .await
    }

    async fn update_session_keep_alive(
        &self,
        id: &meerkat_core::types::SessionId,
        keep_alive: bool,
    ) -> Result<(), SessionError> {
        self.inner.update_session_keep_alive(id, keep_alive).await
    }

    async fn has_live_session(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<bool, SessionError> {
        self.inner.has_live_session(id).await
    }

    async fn set_session_tool_filter(
        &self,
        id: &meerkat_core::types::SessionId,
        filter: meerkat_core::ToolFilter,
    ) -> Result<(), SessionError> {
        self.inner.set_session_tool_filter(id, filter).await
    }

    async fn read(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::service::SessionView, SessionError> {
        self.inner.read(id).await
    }

    async fn list(
        &self,
        query: meerkat_core::service::SessionQuery,
    ) -> Result<Vec<meerkat_core::service::SessionSummary>, SessionError> {
        self.inner.list(query).await
    }

    async fn archive(&self, id: &meerkat_core::types::SessionId) -> Result<(), SessionError> {
        self.inner.archive(id).await
    }

    async fn subscribe_session_events(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::comms::EventStream, meerkat_core::comms::StreamError> {
        meerkat_core::service::SessionService::subscribe_session_events(self.inner.as_ref(), id)
            .await
    }
}

#[async_trait]
impl meerkat_core::service::SessionServiceCommsExt for PreBuildMobSessionService {
    async fn comms_runtime(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Option<Arc<dyn meerkat_core::agent::CommsRuntime>> {
        self.inner.comms_runtime(session_id).await
    }
}

#[async_trait]
impl meerkat_core::service::SessionServiceControlExt for PreBuildMobSessionService {
    async fn append_system_context(
        &self,
        id: &meerkat_core::types::SessionId,
        req: meerkat_core::service::AppendSystemContextRequest,
    ) -> Result<
        meerkat_core::service::AppendSystemContextResult,
        meerkat_core::service::SessionControlError,
    > {
        self.inner.append_system_context(id, req).await
    }
}

#[async_trait]
impl meerkat_core::service::SessionServiceHistoryExt for PreBuildMobSessionService {
    async fn read_history(
        &self,
        id: &meerkat_core::types::SessionId,
        query: meerkat_core::service::SessionHistoryQuery,
    ) -> Result<meerkat_core::service::SessionHistoryPage, SessionError> {
        self.inner.read_history(id, query).await
    }
}

#[async_trait]
impl MobSessionService for PreBuildMobSessionService {
    fn supports_persistent_sessions(&self) -> bool {
        self.inner.supports_persistent_sessions()
    }

    fn runtime_adapter(&self) -> Option<Arc<meerkat_runtime::RuntimeSessionAdapter>> {
        self.inner.runtime_adapter()
    }

    async fn session_belongs_to_mob(
        &self,
        session_id: &meerkat_core::types::SessionId,
        mob_id: &meerkat_mob::MobId,
    ) -> bool {
        self.inner.session_belongs_to_mob(session_id, mob_id).await
    }

    async fn load_persisted_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::session::Session>, SessionError> {
        self.inner.load_persisted_session(session_id).await
    }

    async fn apply_runtime_turn(
        &self,
        session_id: &meerkat_core::types::SessionId,
        run_id: meerkat_core::lifecycle::RunId,
        req: meerkat_core::service::StartTurnRequest,
        boundary: meerkat_core::lifecycle::run_primitive::RunApplyBoundary,
        contributing_input_ids: Vec<meerkat_core::lifecycle::InputId>,
    ) -> Result<meerkat_core::lifecycle::core_executor::CoreApplyOutput, SessionError> {
        self.inner
            .apply_runtime_turn(session_id, run_id, req, boundary, contributing_input_ids)
            .await
    }

    async fn discard_live_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner.discard_live_session(session_id).await
    }

    async fn cancel_all_checkpointers(&self) {
        self.inner.cancel_all_checkpointers().await;
    }

    async fn rearm_all_checkpointers(&self) {
        self.inner.rearm_all_checkpointers().await;
    }
}

/// Specification for bootstrapping a mob runtime from a definition, storage, and session service.
pub struct MobBootstrapSpec {
    pub definition: MobDefinition,
    pub storage: MobStorage,
    pub session_service: Arc<dyn MobSessionService>,
    pub options: MobBootstrapOptions,
    /// Explicit runtime adapter — bypasses `session_service.runtime_adapter()`.
    ///
    /// Used by `persistent()` to supply the adapter directly so the session
    /// service's `runtime_store` can stay `None` (keeping the checkpointer
    /// enabled). See meerkat-session#checkpointer-enabled-flag.
    pub runtime_adapter: Option<Arc<meerkat_runtime::RuntimeSessionAdapter>>,
}

impl MobBootstrapSpec {
    pub fn new(
        definition: MobDefinition,
        storage: MobStorage,
        session_service: Arc<dyn MobSessionService>,
    ) -> Self {
        Self {
            definition,
            storage,
            session_service,
            options: MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: None,
            },
            runtime_adapter: None,
        }
    }

    pub fn with_options(mut self, options: MobBootstrapOptions) -> Self {
        self.options = options;
        self
    }

    /// Build an ephemeral session service with a correctly wired `AgentFactory`.
    ///
    /// If `session_store` is provided, it is set on the `FactoryAgentBuilder` so
    /// that agents use the given store instead of falling back to JSONL.
    pub fn ephemeral(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Option<Arc<dyn AgentSessionStore>>,
    ) -> Self {
        Self::ephemeral_inner(
            definition,
            storage,
            store_path,
            max_sessions,
            session_store,
            None,
        )
    }

    /// Like [`ephemeral`](Self::ephemeral), but with a pre-build hook that is
    /// called before each agent is constructed. Use this to inject external
    /// tools, augment system prompts, or set per-agent labels.
    pub fn ephemeral_with_hook(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Option<Arc<dyn AgentSessionStore>>,
        hook: impl Fn(&mut CreateSessionRequest) + Send + Sync + 'static,
    ) -> Self {
        Self::ephemeral_inner(
            definition,
            storage,
            store_path,
            max_sessions,
            session_store,
            Some(Arc::new(hook)),
        )
    }

    fn ephemeral_inner(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Option<Arc<dyn AgentSessionStore>>,
        hook: Option<PreBuildHook>,
    ) -> Self {
        let factory = AgentFactory::new(&store_path)
            .builtins(true)
            .shell(true)
            .mob(true)
            .comms(true)
            .memory(true);
        let config = Config::default();
        let mut builder = FactoryAgentBuilder::new(factory, config);
        if let Some(store) = session_store {
            builder.default_session_store = Some(store);
        }
        let session_service: Arc<dyn MobSessionService> = Arc::new(
            meerkat_session::EphemeralSessionService::new(builder, max_sessions),
        );
        let session_service = match hook {
            Some(h) => Arc::new(PreBuildMobSessionService {
                inner: session_service,
                hook: h,
            }) as Arc<dyn MobSessionService>,
            None => session_service,
        };
        Self::new(definition, storage, session_service)
    }

    /// Build a persistent session service with a correctly wired `AgentFactory`.
    ///
    /// The `session_store` is used in two places:
    /// 1. As the persistence backend for `PersistentSessionService` (checkpoint/restore).
    /// 2. Adapted via `StoreAdapter` and set on `FactoryAgentBuilder.default_session_store`
    ///    so that agents use it directly instead of falling back to JSONL.
    pub fn persistent(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Arc<dyn SessionStore>,
    ) -> Self {
        Self::persistent_inner(
            definition,
            storage,
            store_path,
            max_sessions,
            session_store,
            None,
        )
    }

    /// Like [`persistent`](Self::persistent), but with a pre-build hook that
    /// is called before each agent is constructed. Use this to inject external
    /// tools, augment system prompts, or set per-agent labels.
    pub fn persistent_with_hook(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Arc<dyn SessionStore>,
        hook: impl Fn(&mut CreateSessionRequest) + Send + Sync + 'static,
    ) -> Self {
        Self::persistent_inner(
            definition,
            storage,
            store_path,
            max_sessions,
            session_store,
            Some(Arc::new(hook)),
        )
    }

    fn persistent_inner(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Arc<dyn SessionStore>,
        hook: Option<PreBuildHook>,
    ) -> Self {
        let factory = AgentFactory::new(&store_path)
            .builtins(true)
            .shell(true)
            .mob(true)
            .comms(true)
            .memory(true);
        let config = Config::default();
        let mut builder = FactoryAgentBuilder::new(factory, config);
        builder.default_session_store = Some(Arc::new(StoreAdapter::new(session_store.clone())));
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(meerkat_store::FsBlobStore::new(store_path.join("blobs")));
        // Supply runtime_store as None so the session checkpointer stays enabled
        // (the checkpointer owns persistence when no runtime store is present).
        // The runtime adapter is provided directly on the spec via ephemeral() —
        // this supports comms drain / keep-alive without implying runtime-backed
        // persistence ownership.
        let session_service: Arc<dyn MobSessionService> =
            Arc::new(meerkat_session::PersistentSessionService::new(
                builder,
                max_sessions,
                session_store,
                None,
                blob_store,
            ));
        let session_service = match hook {
            Some(h) => Arc::new(PreBuildMobSessionService {
                inner: session_service,
                hook: h,
            }) as Arc<dyn MobSessionService>,
            None => session_service,
        };
        let adapter = Arc::new(meerkat_runtime::RuntimeSessionAdapter::ephemeral());
        let mut spec = Self::new(definition, storage, session_service);
        spec.runtime_adapter = Some(adapter);
        spec
    }
}

/// Error returned by mob runtime operations.
#[derive(Debug)]
pub enum MobRuntimeError {
    Mob(MobError),
    InvalidInput(&'static str),
}

impl std::fmt::Display for MobRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(err) => write!(f, "{err}"),
            Self::InvalidInput(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for MobRuntimeError {}

impl From<MobError> for MobRuntimeError {
    fn from(value: MobError) -> Self {
        Self::Mob(value)
    }
}

/// Point-in-time snapshot of a mob member's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobMemberSnapshot {
    pub meerkat_id: String,
    pub profile: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub wired_to: Vec<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub labels: std::collections::BTreeMap<String, String>,
}

/// Report from a reconcile operation showing desired, retained, spawned, and retired members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MobReconcileReport {
    pub desired: Vec<String>,
    pub retained: Vec<String>,
    pub spawned: Vec<String>,
    #[serde(default)]
    pub retired: Vec<String>,
}

/// Options controlling reconciliation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobReconcileOptions {
    pub retire_stale: bool,
}

impl Default for MobReconcileOptions {
    fn default() -> Self {
        Self { retire_stale: true }
    }
}

fn snapshot_from_entry(entry: RosterEntry) -> MobMemberSnapshot {
    let mut wired_to: Vec<String> = entry.wired_to.into_iter().map(|p| p.to_string()).collect();
    wired_to.sort();
    MobMemberSnapshot {
        meerkat_id: entry.meerkat_id.to_string(),
        profile: entry.profile.to_string(),
        state: match entry.state {
            MemberState::Active => MEMBER_STATE_ACTIVE.to_string(),
            MemberState::Retiring => MEMBER_STATE_RETIRING.to_string(),
        },
        runtime_mode: Some(entry.runtime_mode.to_string()),
        session_id: entry.member_ref.session_id().map(ToString::to_string),
        wired_to,
        labels: entry.labels,
    }
}

/// Live mob runtime backed by a `MobHandle`.
#[derive(Clone)]
pub struct RealMobRuntime {
    handle: MobHandle,
}

impl RealMobRuntime {
    pub async fn bootstrap(spec: MobBootstrapSpec) -> Result<Self, MobRuntimeError> {
        let mut builder = MobBuilder::new(spec.definition, spec.storage);

        // Set the runtime adapter BEFORE with_session_service — MobBuilder only
        // pulls the adapter from the session service when runtime_adapter is None.
        if let Some(adapter) = spec.runtime_adapter {
            builder = builder.with_runtime_adapter(adapter);
        }

        builder = builder
            .with_session_service(spec.session_service)
            .allow_ephemeral_sessions(spec.options.allow_ephemeral_sessions)
            .notify_orchestrator_on_resume(spec.options.notify_orchestrator_on_resume);

        if let Some(client) = spec.options.default_llm_client {
            builder = builder.with_default_llm_client(client);
        }

        let handle = builder.create().await?;
        Ok(Self { handle })
    }

    pub fn from_handle(handle: MobHandle) -> Self {
        Self { handle }
    }

    pub fn handle(&self) -> MobHandle {
        self.handle.clone()
    }

    pub fn status(&self) -> MobState {
        self.handle.status()
    }

    pub async fn discover(&self) -> Vec<MobMemberSnapshot> {
        self.handle
            .list_all_members()
            .await
            .into_iter()
            .map(snapshot_from_entry)
            .collect()
    }

    pub async fn get_member(&self, member_id: &str) -> Option<MobMemberSnapshot> {
        self.handle
            .get_member(&MeerkatId::from(member_id))
            .await
            .map(snapshot_from_entry)
    }

    pub async fn retire_member(&self, member_id: &str) -> Result<(), MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        self.handle
            .retire(MeerkatId::from(member_id))
            .await
            .map_err(Into::into)
    }

    pub async fn respawn_member(&self, member_id: &str) -> Result<(), MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        self.handle
            .respawn(MeerkatId::from(member_id), None)
            .await
            .map(|_receipt| ())
            .map_err(|err| MobRuntimeError::Mob(MobError::Internal(err.to_string())))
    }

    pub async fn spawn(&self, spec: SpawnMemberSpec) -> Result<MemberRef, MobRuntimeError> {
        self.handle.spawn_spec(spec).await.map_err(Into::into)
    }

    pub async fn spawn_many(
        &self,
        specs: Vec<SpawnMemberSpec>,
    ) -> Result<Vec<MemberRef>, MobRuntimeError> {
        let futs = specs.into_iter().map(|spec| self.handle.spawn_spec(spec));
        futures::future::try_join_all(futs)
            .await
            .map_err(Into::into)
    }

    pub async fn reconcile(
        &self,
        desired_specs: Vec<SpawnMemberSpec>,
    ) -> Result<MobReconcileReport, MobRuntimeError> {
        self.reconcile_with_options(desired_specs, MobReconcileOptions::default())
            .await
    }

    pub async fn reconcile_with_options(
        &self,
        desired_specs: Vec<SpawnMemberSpec>,
        options: MobReconcileOptions,
    ) -> Result<MobReconcileReport, MobRuntimeError> {
        let existing_active_members = self
            .handle
            .list_members()
            .await
            .into_iter()
            .map(|entry| entry.meerkat_id.to_string())
            .collect::<BTreeSet<_>>();
        let mut known = existing_active_members.clone();

        let mut desired = Vec::new();
        let mut retained = Vec::new();
        let mut spawned = Vec::new();
        let mut retired = Vec::new();
        let mut seen = BTreeSet::new();

        for spec in desired_specs {
            let member_id = spec.meerkat_id.to_string();
            if !seen.insert(member_id.clone()) {
                continue;
            }
            desired.push(member_id.clone());
            if known.contains(&member_id) {
                retained.push(member_id);
                continue;
            }
            self.handle.spawn_spec(spec).await?;
            known.insert(member_id.clone());
            spawned.push(member_id);
        }

        if options.retire_stale {
            let desired_set = desired.iter().cloned().collect::<BTreeSet<_>>();
            for stale_member_id in existing_active_members
                .into_iter()
                .filter(|member_id| !desired_set.contains(member_id))
            {
                self.handle
                    .retire(MeerkatId::from(stale_member_id.clone()))
                    .await?;
                retired.push(stale_member_id);
            }
        }

        Ok(MobReconcileReport {
            desired,
            retained,
            spawned,
            retired,
        })
    }

    pub async fn stop(&self) -> Result<(), MobRuntimeError> {
        self.handle.stop().await.map_err(Into::into)
    }

    pub async fn resume(&self) -> Result<(), MobRuntimeError> {
        self.handle.resume().await.map_err(Into::into)
    }

    /// Send a message to a member and return the accepting session ID.
    pub async fn send_message(
        &self,
        member_id: &str,
        content: impl Into<meerkat_core::ContentInput>,
    ) -> Result<String, MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        let content = content.into();
        let is_empty = match &content {
            meerkat_core::ContentInput::Text(s) => s.trim().is_empty(),
            meerkat_core::ContentInput::Blocks(blocks) => blocks.is_empty(),
        };
        if is_empty {
            return Err(MobRuntimeError::InvalidInput("content must not be empty"));
        }
        let mid = MeerkatId::from(member_id);
        let receipt = self
            .handle
            .member(&mid)
            .await?
            .send(content, meerkat_core::types::HandlingMode::Queue)
            .await?;
        Ok(receipt.session_id.to_string())
    }

    /// Find members matching a label key-value pair.
    pub async fn find_members(&self, label_key: &str, label_value: &str) -> Vec<MobMemberSnapshot> {
        self.discover()
            .await
            .into_iter()
            .filter(|m| m.labels.get(label_key).is_some_and(|v| v == label_value))
            .collect()
    }

    /// Ensure a member exists, spawning from spec if missing.
    ///
    /// Idempotent — returns Ok if the member already exists.
    pub async fn ensure_member(
        &self,
        spec: SpawnMemberSpec,
    ) -> Result<MobMemberSnapshot, MobRuntimeError> {
        let meerkat_id = spec.meerkat_id.clone();
        // Check roster first
        if let Some(entry) = self.handle.get_member(&meerkat_id).await {
            return Ok(snapshot_from_entry(entry));
        }
        // Spawn
        match self.handle.spawn_spec(spec).await {
            Ok(_member_ref) => {}
            Err(MobError::MeerkatAlreadyExists(_)) => {
                // Concurrent spawn — fine
            }
            Err(err) => return Err(err.into()),
        }
        // Return current state
        let entry = self
            .handle
            .get_member(&meerkat_id)
            .await
            .ok_or(MobRuntimeError::Mob(MobError::MeerkatNotFound(meerkat_id)))?;
        Ok(snapshot_from_entry(entry))
    }

    // -----------------------------------------------------------------------
    // 0.5 API surface
    // -----------------------------------------------------------------------

    /// Detailed execution snapshot for a single member.
    pub async fn member_status(
        &self,
        member_id: &str,
    ) -> Result<RichMobMemberSnapshot, MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        self.handle
            .member_status(&MeerkatId::from(member_id))
            .await
            .map_err(Into::into)
    }

    /// Forcefully cancel a member (immediate teardown, no graceful retire).
    pub async fn force_cancel_member(&self, member_id: &str) -> Result<(), MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        self.handle
            .force_cancel_member(MeerkatId::from(member_id))
            .await
            .map_err(Into::into)
    }

    /// Spawn a short-lived helper member, wait for completion, retire it, and return the result.
    pub async fn spawn_helper(
        &self,
        meerkat_id: &str,
        task: &str,
        options: HelperOptions,
    ) -> Result<HelperResult, MobRuntimeError> {
        if meerkat_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput(
                "meerkat_id must not be empty",
            ));
        }
        self.handle
            .spawn_helper(MeerkatId::from(meerkat_id), task, options)
            .await
            .map_err(Into::into)
    }

    /// Fork from an existing member's context, wait for completion, retire, and return.
    pub async fn fork_helper(
        &self,
        source_member_id: &str,
        meerkat_id: &str,
        task: &str,
        fork_context: ForkContext,
        options: HelperOptions,
    ) -> Result<HelperResult, MobRuntimeError> {
        if source_member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput(
                "source_member_id must not be empty",
            ));
        }
        if meerkat_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput(
                "meerkat_id must not be empty",
            ));
        }
        self.handle
            .fork_helper(
                &MeerkatId::from(source_member_id),
                MeerkatId::from(meerkat_id),
                task,
                fork_context,
                options,
            )
            .await
            .map_err(Into::into)
    }

    /// Attach a member to an existing session (resume mode).
    pub async fn attach_existing_session(
        &self,
        profile: &str,
        meerkat_id: &str,
        session_id_str: &str,
    ) -> Result<RichMobMemberSnapshot, MobRuntimeError> {
        if profile.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("profile must not be empty"));
        }
        if meerkat_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput(
                "meerkat_id must not be empty",
            ));
        }
        if session_id_str.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput(
                "session_id must not be empty",
            ));
        }
        let session_id = meerkat_core::types::SessionId::parse(session_id_str)
            .map_err(|_| MobRuntimeError::InvalidInput("invalid session_id format"))?;
        let mid = MeerkatId::from(meerkat_id);
        let mut spec = SpawnMemberSpec::new(ProfileName::from(profile), mid.clone());
        spec.launch_mode = MemberLaunchMode::Resume { session_id };
        self.handle.spawn_spec(spec).await?;
        self.handle.member_status(&mid).await.map_err(Into::into)
    }

    /// Cancel a running flow by its run ID.
    pub async fn cancel_flow(&self, run_id_str: &str) -> Result<(), MobRuntimeError> {
        if run_id_str.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("run_id must not be empty"));
        }
        let run_id: RunId = run_id_str
            .parse()
            .map_err(|_| MobRuntimeError::InvalidInput("invalid run_id format"))?;
        self.handle.cancel_flow(run_id).await.map_err(Into::into)
    }

    /// Query the status of a flow run.
    pub async fn flow_status(&self, run_id_str: &str) -> Result<Option<MobRun>, MobRuntimeError> {
        if run_id_str.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("run_id must not be empty"));
        }
        let run_id: RunId = run_id_str
            .parse()
            .map_err(|_| MobRuntimeError::InvalidInput("invalid run_id format"))?;
        self.handle.flow_status(run_id).await.map_err(Into::into)
    }

    /// Collect all members that have reached a terminal state.
    pub async fn collect_completed(&self) -> Vec<(String, RichMobMemberSnapshot)> {
        self.handle
            .collect_completed()
            .await
            .into_iter()
            .map(|(mid, snapshot)| (mid.to_string(), snapshot))
            .collect()
    }

    /// Get the current session ID for a member (if any).
    /// Returns Ok(None) if the member doesn't exist.
    pub async fn member_current_session_id(
        &self,
        member_id: &str,
    ) -> Result<Option<String>, MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        let mid = MeerkatId::from(member_id);
        match self.handle.member(&mid).await {
            Ok(member) => {
                let session_id = member.current_session_id().await?;
                Ok(session_id.map(|sid| sid.to_string()))
            }
            Err(MobError::MeerkatNotFound(_)) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Get a reference to a member's current session bridge.
    /// Returns Ok(None) if the member doesn't exist.
    pub async fn member_session_ref(
        &self,
        member_id: &str,
    ) -> Result<Option<MemberSessionRef>, MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        let mid = MeerkatId::from(member_id);
        match self.handle.member(&mid).await {
            Ok(member) => member.session_ref().await.map_err(Into::into),
            Err(MobError::MeerkatNotFound(_)) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Regression: MobBootstrapSpec::persistent must supply a runtime adapter
    /// so the mob actor spawns the comms drain. The adapter is provided directly
    /// on the spec (not via session service's runtime_store) to keep the session
    /// checkpointer enabled.
    #[test]
    fn persistent_bootstrap_provides_runtime_adapter() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let Ok(sqlite) = meerkat_store::SqliteSessionStore::open(store_path.join("sessions.db"))
        else {
            panic!("failed to open sqlite session store");
        };
        let session_store: Arc<dyn SessionStore> = Arc::new(sqlite);
        let Ok(definition) = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"test\"\n") else {
            panic!("failed to parse minimal mob definition");
        };
        let spec = MobBootstrapSpec::persistent(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            session_store,
        );
        // The spec must carry a runtime adapter for comms drain.
        assert!(
            spec.runtime_adapter.is_some(),
            "persistent bootstrap must set runtime_adapter on the spec"
        );
        // The session service must NOT have a runtime_store (keeps checkpointer enabled).
        assert!(
            spec.session_service.runtime_adapter().is_none(),
            "session service should not have runtime_store (checkpointer must stay enabled)"
        );
    }
}
