//! Mob member lifecycle management — bootstrap, spawn, reconcile, and roster queries.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use meerkat::{AgentFactory, Config, FactoryAgentBuilder, SessionStore};
use meerkat_client::types::LlmStream;
use meerkat_client::{LlmClient, LlmRequest};
use meerkat_core::agent::CommsRuntime;
use meerkat_core::service::{
    CreateSessionRequest, SessionError, SessionHistoryPage, SessionHistoryQuery,
    SessionServiceHistoryExt,
};
use meerkat_core::{AgentSessionStore, AssistantBlock, Message, Provider};
use meerkat_mob::{
    MobBuilder, MobDefinition, MobError, MobHandle, MobSessionService, MobStorage, Profile,
    ProfileName, SpawnMemberSpec,
};
use meerkat_runtime::input_state::{InputStatePersistenceRecord, StoredInputState};
use meerkat_runtime::store::MachineLifecycleCommit;
use meerkat_store::StoreAdapter;
use serde::Serialize;
use serde_json::Value;

use crate::blob_store::{
    Base64BlobStoreAdapter, BinaryBlobStore, BlobStoreInjection, ObjectStoreBlobStore,
};
use crate::console_spawn::{
    ConsoleSpawnSink, SharedConsoleSpawnSinkSlot, new_console_spawn_sink_slot,
};
use crate::storage_health::{
    BlobDurability, BlobStoreResolutionError, ResolvedStorageSummary, RuntimeStoreResolutionError,
    StorageResolutionError, StorageSlotSummary, blob_slot_summary, probe_session_store_incremental,
    scratch_ring_buffer_slots,
};

pub(crate) const DELEGATE_IDLE_RETIRE_SECS_LABEL: &str = "implicit_delegate_idle_retire_secs";
pub(crate) const DELEGATE_IDLE_RETIRE_DISABLED_LABEL: &str = "disabled";

pub(crate) fn is_previous_member_cleanup_ambiguous_error(error: &str) -> bool {
    error.contains("previous member cleanup ambiguous for member ")
}

pub(crate) fn is_recoverable_lifecycle_cleanup_error(error: &str) -> bool {
    is_previous_member_cleanup_ambiguous_error(error)
        || (error.contains("disposal completed but ArchiveSession failed")
            && (
                // Cancel/retire race: the runtime was still running when the
                // archive step tried to cancel it.
                (error.contains("cancel-before-retire failed")
                    && error.contains("Runtime not ready: running"))
                // meerkat 0.7.1: the session machine of an idle member sits in
                // `Stopped`, whose DSL authority rejects the archive step's
                // final `Retire` input. Disposal already completed — the
                // member left the roster (retire) or is anchored for cleanup
                // retry (respawn) — so the failed bookkeeping transition must
                // not fail the lifecycle operation.
                || (error.contains("guard rejected transition from Stopped")
                    && error.contains("input::Retire"))
                // meerkat 0.7.1 retire performs a final fenced continuity
                // save for the old session. Identity-first reset/delete flows
                // advance or remove the mobkit-owned continuity record before
                // retiring the old generation, so that save is intentionally
                // stale; disposal itself completed.
                || (error.contains("continuity save")
                    && (error.contains("continuity record not found")
                        || error.contains("stale fencing token")))
            ))
}

/// The mob lifecycle authority's `ArchiveSession` step legitimately found no
/// session to archive because the retired member is a SESSION-OWNED
/// identity-first agent (built via `Bridge::create_session` / a
/// `SessionAgentBuilder` roster), not a spawned mob member tracked by that
/// authority.
///
/// meerkat-mob disposes the member (succeeds), then runs `ArchiveSession`; the
/// authority has no record for the session, and because the runtime adapter
/// still holds it the archive helper escalates the miss to a fatal "disposal
/// completed but ArchiveSession failed: ... NotFound for registered runtime
/// session". For a session-owned identity that archive record never existed —
/// disposal already completed — so the cleanup retire must not fail the
/// lifecycle op (otherwise reset/delete_identity brick the identity until a
/// process restart).
///
/// Requires the `disposal completed` prefix so an aborted disposal (the session
/// genuinely never tore down) stays fail-closed.
fn is_session_owned_archive_absent_cleanup_error(error: &str) -> bool {
    error.contains("disposal completed but ArchiveSession failed")
        && error.contains("NotFound for registered runtime session")
}

/// Retire-cleanup tolerance for the identity-first SESSION-OWNED bridge.
///
/// Every caller of [`Bridge::retire_member`](crate::identity_first) lives in
/// `identity_first/`, where members are built session-owned (`create_session`),
/// never spawned. So in addition to the shared recoverable-cleanup cases it
/// also tolerates the session-owned archive-absent error above.
///
/// This is DELIBERATELY kept out of [`is_recoverable_lifecycle_cleanup_error`],
/// which the mob-member reset/retire paths use (via `lifecycle_archive_cleanup_completed`)
/// and which MUST keep rejecting archive-NotFound to surface genuinely orphaned
/// spawned members.
pub(crate) fn is_recoverable_session_owned_retire_cleanup_error(error: &str) -> bool {
    is_recoverable_lifecycle_cleanup_error(error)
        || is_session_owned_archive_absent_cleanup_error(error)
}

/// True when a session archive failed only at its final runtime-retire
/// realization because the session machine sits in `Stopped`.
///
/// meerkat 0.7.1 splits the archive across two realizations, committed
/// document-first: the durable session-document lifecycle commit lands,
/// then `MachineSessionArchiveProtocol::retire_session` drives the machine
/// `Retire` transition. The session machine accepts `Retire` from
/// Idle/Attached/Running/Retired but NOT from `Stopped` — and an idle mob
/// member's runtime is stopped (and persisted as `Stopped`) between turns,
/// so member retire/respawn disposal deterministically fails here.
/// meerkat-mob's own archive helper (`retire_runtime_session_for_archive`)
/// explicitly treats `Stopped` as already-retired; the meerkat-session
/// protocol misses that tolerance.
pub(crate) fn is_stopped_session_archive_retire_rejection(error: &str) -> bool {
    error.contains("machine archive retire failed")
        && error.contains("guard rejected transition from Stopped")
        && error.contains("input::Retire")
}

pub(crate) fn topology_restore_failed_peer_ids(
    error: &meerkat_mob::MobRespawnError,
) -> Option<Vec<String>> {
    match error {
        meerkat_mob::MobRespawnError::TopologyRestoreFailed {
            receipt: _,
            failed_peer_ids,
        } => Some(failed_peer_ids.iter().map(ToString::to_string).collect()),
        _ => None,
    }
}

pub(crate) fn topology_restore_warning_json(failed_peer_ids: &[String]) -> Value {
    serde_json::json!({
        "kind": "topology_restore_degraded",
        "failed_peer_ids": failed_peer_ids,
    })
}

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

/// Member state constant for active members.
pub const MEMBER_STATE_ACTIVE: &str = "active";
/// Member state constant for members transitioning to retired.
pub const MEMBER_STATE_RETIRING: &str = "retiring";

/// Project a machine-owned member status into the console's member-state
/// string vocabulary.
///
/// Meerkat 0.7 replaced the roster-owned two-state `MemberState` with the
/// machine-projected [`meerkat_mob::MobMemberStatus`]; the legacy
/// active/retiring pair keeps the existing console constants and the newer
/// machine statuses (broken/completed/unknown) project as their canonical
/// snake_case names.
pub(crate) fn member_status_state_string(status: meerkat_mob::MobMemberStatus) -> String {
    match status {
        meerkat_mob::MobMemberStatus::Active => MEMBER_STATE_ACTIVE.to_string(),
        meerkat_mob::MobMemberStatus::Retiring => MEMBER_STATE_RETIRING.to_string(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

/// Options for bootstrapping a mob runtime.
#[derive(Clone, Default)]
pub struct MobBootstrapOptions {
    pub allow_ephemeral_sessions: bool,
    pub notify_orchestrator_on_resume: bool,
    pub default_llm_client: Option<Arc<dyn LlmClient>>,
}

/// Wraps an LLM client and strips provider-emitted evidence blocks that are
/// useful for UI/citation projection but unsafe to replay into the next
/// stateless provider request.
pub struct ReplaySanitizingLlmClient {
    inner: Arc<dyn LlmClient>,
}

type SharedDefaultLlmClientSlot = Arc<std::sync::RwLock<Option<Arc<dyn LlmClient>>>>;

fn session_llm_reconfigure_blueprint(
    builder: &FactoryAgentBuilder,
    store_path: &Path,
) -> (
    meerkat::session_runtime::llm_reconfigure::SessionRuntimeLlmReconfigureHostBlueprint,
    SharedDefaultLlmClientSlot,
) {
    let default_client_slot = Arc::new(std::sync::RwLock::new(None::<Arc<dyn LlmClient>>));
    let blueprint =
        meerkat::session_runtime::llm_reconfigure::SessionRuntimeLlmReconfigureHostBlueprint::new(
            builder,
            store_path.join("session-llm-reconfigure-config-state.json"),
            Arc::clone(&default_client_slot),
        );
    (blueprint, default_client_slot)
}

impl ReplaySanitizingLlmClient {
    pub fn new(inner: Arc<dyn LlmClient>) -> Self {
        Self { inner }
    }

    pub fn wrap(inner: Arc<dyn LlmClient>) -> Arc<dyn LlmClient> {
        Arc::new(Self::new(inner))
    }
}

/// Provider-agnostic claim for the mob-wide default LLM client.
///
/// The default client (test stubs, `demo_llm` gateways) is installed as the
/// raw `llm_client_override` on EVERY member build, across whatever providers
/// the definition's profiles resolve to — including a provider a member MOVES
/// to when a definition edit changes its model and the resume-override pair
/// applies (model, provider) atomically from the catalog. A concrete
/// [`LlmClient::provider`] claim turns that composition into a typed factory
/// rejection ("raw LLM client override claims provider 'openai' but canonical
/// model ... belongs to 'anthropic'") the moment any member's canonical
/// provider differs from the claim — the OB3 pair-coherence fix surfaced
/// exactly this on the resume path. `Provider::Other` is meerkat's typed
/// "serves any provider" claim; the member's canonical (model, provider)
/// identity keeps coming from the build config and catalog, never from this
/// client.
///
/// Deliberately NOT applied to per-session `llm_client_override`s entering
/// through `CreateSessionRequest`
/// ([`sanitize_create_session_request_llm_override`]): those are scoped to
/// one session, and a concrete claim contradicting that session's canonical
/// provider is a real composition error the factory guard exists to catch.
struct ProviderAgnosticLlmClient {
    inner: Arc<dyn LlmClient>,
}

impl ProviderAgnosticLlmClient {
    fn wrap(inner: Arc<dyn LlmClient>) -> Arc<dyn LlmClient> {
        Arc::new(Self { inner })
    }
}

#[async_trait]
impl LlmClient for ProviderAgnosticLlmClient {
    fn project_replay_messages(
        &self,
        messages: &[Message],
    ) -> Result<Vec<Message>, meerkat_client::LlmError> {
        self.inner.project_replay_messages(messages)
    }

    fn stream<'a>(&'a self, request: &'a LlmRequest) -> LlmStream<'a> {
        self.inner.stream(request)
    }

    fn provider(&self) -> meerkat_core::Provider {
        meerkat_core::Provider::Other
    }

    async fn health_check(&self) -> Result<(), meerkat_client::LlmError> {
        self.inner.health_check().await
    }

    fn compile_schema(
        &self,
        output_schema: &meerkat_core::OutputSchema,
    ) -> Result<meerkat_core::schema::CompiledSchema, meerkat_core::schema::SchemaError> {
        self.inner.compile_schema(output_schema)
    }
}

/// Agent-layer companion to [`ReplaySanitizingLlmClient`].
///
/// Meerkat session services can also receive already-adapted
/// `AgentLlmClient`s through live replacement and hot-swap APIs. Sanitize at
/// that boundary too so provider-emitted server tool telemetry is never
/// replayed into the next stateless model request just because the client
/// entered below the raw `LlmClient` adapter seam.
pub struct ReplaySanitizingAgentLlmClient {
    inner: Arc<dyn meerkat_core::AgentLlmClient>,
}

impl ReplaySanitizingAgentLlmClient {
    pub fn new(inner: Arc<dyn meerkat_core::AgentLlmClient>) -> Self {
        Self { inner }
    }

    pub fn wrap(
        inner: Arc<dyn meerkat_core::AgentLlmClient>,
    ) -> Arc<dyn meerkat_core::AgentLlmClient> {
        Arc::new(Self::new(inner))
    }
}

#[async_trait]
impl meerkat_core::AgentLlmClient for ReplaySanitizingAgentLlmClient {
    async fn stream_response(
        &self,
        messages: &[Message],
        tools: &[Arc<meerkat_core::ToolDef>],
        max_tokens: u32,
        temperature: Option<f32>,
        provider_params: Option<&meerkat_core::lifecycle::run_primitive::ProviderParamsOverride>,
    ) -> Result<meerkat_core::agent::LlmStreamResult, meerkat_core::AgentError> {
        let sanitized: Vec<Message> = messages
            .iter()
            .cloned()
            .map(sanitize_message_for_stateless_replay)
            .collect();
        self.inner
            .stream_response(&sanitized, tools, max_tokens, temperature, provider_params)
            .await
    }

    fn provider(&self) -> meerkat_core::Provider {
        self.inner.provider()
    }

    fn model(&self) -> &str {
        self.inner.model()
    }

    fn prepare_model_fallback(
        &self,
        failure: &meerkat_core::AgentError,
    ) -> Option<meerkat_core::agent::AgentLlmFallbackSwitch> {
        self.inner.prepare_model_fallback(failure)
    }

    fn commit_model_fallback(
        &self,
        previous_identity: &meerkat_core::SessionLlmIdentity,
        target_identity: &meerkat_core::SessionLlmIdentity,
    ) -> Result<(), meerkat_core::AgentError> {
        self.inner
            .commit_model_fallback(previous_identity, target_identity)
    }

    fn active_model_fallback_identity(&self) -> Option<meerkat_core::SessionLlmIdentity> {
        self.inner.active_model_fallback_identity()
    }

    fn compile_model_fallback_schema(
        &self,
        target_identity: &meerkat_core::SessionLlmIdentity,
        output_schema: &meerkat_core::OutputSchema,
    ) -> Result<meerkat_core::schema::CompiledSchema, meerkat_core::AgentError> {
        self.inner
            .compile_model_fallback_schema(target_identity, output_schema)
    }

    fn begin_stream_output_observation(&self) {
        self.inner.begin_stream_output_observation();
    }

    fn stream_output_observed(&self) -> bool {
        self.inner.stream_output_observed()
    }

    fn compile_schema(
        &self,
        output_schema: &meerkat_core::OutputSchema,
    ) -> Result<meerkat_core::schema::CompiledSchema, meerkat_core::schema::SchemaError> {
        self.inner.compile_schema(output_schema)
    }
}

#[async_trait]
impl LlmClient for ReplaySanitizingLlmClient {
    fn project_replay_messages(
        &self,
        messages: &[Message],
    ) -> Result<Vec<Message>, meerkat_client::LlmError> {
        let sanitized: Vec<Message> = messages
            .iter()
            .cloned()
            .map(sanitize_message_for_stateless_replay)
            .collect();
        self.inner.project_replay_messages(&sanitized)
    }

    fn stream<'a>(&'a self, request: &'a LlmRequest) -> LlmStream<'a> {
        let inner = Arc::clone(&self.inner);
        let sanitized = sanitize_llm_request_for_stateless_replay(request);
        Box::pin(async_stream::stream! {
            let mut stream = inner.stream(&sanitized);
            while let Some(event) = stream.next().await {
                if runtime_turn_diagnostics_enabled() && event.is_err() {
                    tracing::error!("mobkit llm client stream error");
                }
                yield event;
            }
        })
    }

    fn provider(&self) -> meerkat_core::Provider {
        self.inner.provider()
    }

    async fn health_check(&self) -> Result<(), meerkat_client::LlmError> {
        self.inner.health_check().await
    }

    fn compile_schema(
        &self,
        output_schema: &meerkat_core::OutputSchema,
    ) -> Result<meerkat_core::schema::CompiledSchema, meerkat_core::schema::SchemaError> {
        self.inner.compile_schema(output_schema)
    }
}

/// Async hook called before each session is created. Receives the mutable
/// `CreateSessionRequest` so the app can inject external tools, augment the
/// system prompt, set labels, override the model, load session resume data
/// from external stores, etc.
///
/// The hook runs **before** `create_session` captures labels and LLM identity,
/// so all mutations are reflected in session metadata, not just the agent build.
///
/// ```rust,ignore
/// let spec = MobBootstrapSpec::persistent_with_hook(
///     definition, storage, store_path, 64, session_store,
///     |req: &mut CreateSessionRequest| {
///         Box::pin(async move {
///             // Async: load session from external store
///             let session = my_store.load_by_owner(&owner_id).await;
///             if let Some(s) = session {
///                 let build = req.build.get_or_insert_with(SessionBuildOptions::default);
///                 build.resume_session = Some(s);
///             }
///             // Sync: inject tools, augment prompt
///             let build = req.build.get_or_insert_with(SessionBuildOptions::default);
///             build.external_tools = Some(my_tools());
///             Ok(())
///         })
///     },
/// )?;
/// ```
pub(crate) type PreBuildHook = Arc<
    dyn Fn(
            &mut CreateSessionRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), SessionError>> + Send + '_>,
        > + Send
        + Sync,
>;

/// Optional post-creation hook invoked after `create_session` succeeds.
pub type AfterCreateHook = Arc<
    dyn Fn(
            meerkat_core::types::SessionId,
            SessionCreatedContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

/// Wraps a `MobSessionService`, applying a `PreBuildHook` to the
/// `CreateSessionRequest` in `create_session()` before delegating.
///
/// The hook runs before labels and LLM identity are captured by the inner
/// session service, so mutations to `req.labels`, `req.model`, `req.build`,
/// and `req.system_prompt` are fully reflected in session metadata.
struct PreBuildMobSessionService {
    inner: Arc<dyn MobSessionService>,
    hook: PreBuildHook,
    /// §10.1 dispatch-time taint join (`crate::memory::dispatch_taint`):
    /// present ONLY on the wrapper `MobBootstrapSpec::new` installs - the
    /// one layer every spec has exactly once - so `with_*` re-wraps never
    /// double-decorate. The slot is late-bound: compositions fill it when
    /// the memory stack attaches.
    dispatch_taint: Option<crate::memory::dispatch_taint::DispatchTaintSlot>,
    after_create_hook: Option<AfterCreateHook>,
    runtime_adapter_override: Option<Arc<meerkat_runtime::MeerkatMachine>>,
    /// Installed only on the persistent runtime-backed path: absorbs the
    /// identity reconcile loop's repeated authoritative reads of unchanged
    /// session documents (see `SessionDocumentReadAbsorber`).
    session_read_absorber: Option<Arc<SessionDocumentReadAbsorber>>,
    /// RuntimeStore whose store-owned lifecycle facts overlay the resume-seam
    /// read (persistent runtime-backed path only).
    ///
    /// At meerkat 0.8.11 the archive protocol never rewrites session BODIES
    /// to carry archive authority — the absorbing terminal is a
    /// RuntimeStore-owned fact (the catalog entry committed with the physical
    /// authority, or the Retired/Destroyed machine lifecycle row) — while the
    /// mob resume seam still classifies from the body terminal. Without this
    /// overlay a runtime-archived session reads as `Revivable` WITHOUT its
    /// archived terminal, which is exactly the "archived collapses into
    /// active/absent" host confusion the typed seam exists to prevent (hosts
    /// rotated identities off intact preserved transcripts, the 0.8.6 field
    /// failure).
    archived_terminal_authority: Option<Arc<dyn meerkat_runtime::RuntimeStore>>,
}

impl PreBuildMobSessionService {
    /// Whether an `Unsupported` boundary acknowledgement from the inner
    /// service is completed by this wrapper: true exactly on the
    /// bounded-bridge shape, where `runtime_adapter_override` installs the
    /// machine that owns the committed boundary and the inner (ephemeral)
    /// service has no durable projection of its own to fence.
    fn absorbs_unsupported_boundary_acknowledgement(&self) -> bool {
        self.runtime_adapter_override.is_some()
    }

    /// Project the RuntimeStore-owned archived terminal onto a revivable
    /// resume read (see `archived_terminal_authority`). Read-side only: the
    /// returned document copy carries the store-owned fact; nothing is
    /// written. The probe mirrors meerkat-session's
    /// `session_archived_by_runtime_store_authority`: the catalog entry's
    /// committed terminal, else the Retired/Destroyed machine lifecycle row.
    async fn overlay_runtime_archived_terminal(
        &self,
        load: meerkat_mob::ResumeSessionLoad,
    ) -> Result<meerkat_mob::ResumeSessionLoad, SessionError> {
        let meerkat_mob::ResumeSessionLoad::Revivable(mut session) = load else {
            return Ok(load);
        };
        if session.lifecycle_terminal().is_some() {
            return Ok(meerkat_mob::ResumeSessionLoad::Revivable(session));
        }
        let Some(store) = self.archived_terminal_authority.as_ref() else {
            return Ok(meerkat_mob::ResumeSessionLoad::Revivable(session));
        };
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(session.id());
        let store_error = |detail: String| {
            SessionError::Agent(meerkat_core::error::AgentError::InternalError(detail))
        };
        let archived_by_catalog = store
            .load_runtime_session_catalog_entry(&runtime_id)
            .await
            .map_err(|e| {
                store_error(format!(
                    "archived-terminal overlay: runtime catalog read for session {}: {e}",
                    session.id()
                ))
            })?
            .is_some_and(|entry| {
                entry.lifecycle_terminal() == Some(meerkat_core::SessionLifecycleTerminal::Archived)
            });
        let archived = archived_by_catalog
            || matches!(
                meerkat_runtime::store::load_runtime_state(store.as_ref(), &runtime_id)
                    .await
                    .map_err(|e| {
                        store_error(format!(
                            "archived-terminal overlay: runtime lifecycle read for session {}: {e}",
                            session.id()
                        ))
                    })?,
                Some(
                    meerkat_runtime::RuntimeState::Retired
                        | meerkat_runtime::RuntimeState::Destroyed
                )
            );
        if archived {
            session
                .set_lifecycle_terminal(meerkat_core::SessionLifecycleTerminal::Archived)
                .map_err(|e| {
                    store_error(format!(
                        "archived-terminal overlay: terminal projection for session {}: {e}",
                        session.id()
                    ))
                })?;
        }
        Ok(meerkat_mob::ResumeSessionLoad::Revivable(session))
    }

    async fn prepare_create_request(
        &self,
        mut req: CreateSessionRequest,
    ) -> Result<(CreateSessionRequest, SessionCreatedContext), SessionError> {
        (self.hook)(&mut req).await?;
        ensure_shell_tooling_build_substrate(&mut req);
        sanitize_create_session_request_llm_override(&mut req);
        // After the user hook (composes over any decorator it set) and after
        // sanitize (which only touches the raw llm_client_override).
        if let Some(slot) = self.dispatch_taint.as_ref() {
            crate::memory::dispatch_taint::attach_member_taint_decorator(&mut req, slot);
        }

        let context = SessionCreatedContext {
            model: req.model.clone(),
            labels: req.labels.clone().unwrap_or_default(),
            system_prompt: req.system_prompt.as_set_prompt().map(ToString::to_string),
        };
        Ok((req, context))
    }

    async fn complete_create(
        &self,
        result: meerkat_core::types::RunResult,
        context: SessionCreatedContext,
    ) -> meerkat_core::types::RunResult {
        if let Some(ref after_hook) = self.after_create_hook {
            after_hook(result.session_id.clone(), context).await;
        }
        result
    }

    /// Authoritative session read behind `load_persisted_session`: with an
    /// absorber installed, an unchanged document (per the runtime-store write
    /// epoch) is served from the absorbed copy instead of re-reading and
    /// re-verifying it through the inner service.
    async fn load_persisted_session_absorbed(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::session::Session>, SessionError> {
        let Some(absorber) = self.session_read_absorber.as_ref() else {
            return self.inner.load_persisted_session(session_id).await;
        };
        if let Some(session) = absorber.lookup(session_id) {
            return Ok(Some(session));
        }
        let epoch = absorber.observe_epoch(session_id);
        let loaded = self.inner.load_persisted_session(session_id).await?;
        match loaded.as_ref() {
            Some(session) => absorber.admit(session_id, epoch, session),
            None => absorber.evict(session_id),
        }
        Ok(loaded)
    }
}

fn no_op_pre_build_hook() -> PreBuildHook {
    Arc::new(|_req: &mut CreateSessionRequest| Box::pin(async { Ok(()) }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DelegateIdleRetireOverride {
    Disabled,
    Seconds(u64),
}

#[derive(Clone, Default)]
pub(crate) struct ImplicitDelegateRetirementOverrides {
    inner: Arc<tokio::sync::RwLock<BTreeMap<(String, String), DelegateIdleRetireOverride>>>,
}

impl ImplicitDelegateRetirementOverrides {
    pub(crate) async fn set(
        &self,
        mob_id: impl Into<String>,
        member_id: impl Into<String>,
        override_policy: DelegateIdleRetireOverride,
    ) {
        self.inner
            .write()
            .await
            .insert((mob_id.into(), member_id.into()), override_policy);
    }

    pub(crate) async fn get(
        &self,
        mob_id: &str,
        member_id: &str,
    ) -> Option<DelegateIdleRetireOverride> {
        self.inner
            .read()
            .await
            .get(&(mob_id.to_string(), member_id.to_string()))
            .copied()
    }
}

pub(crate) type SharedIdentityRuntimeSlot =
    Arc<std::sync::RwLock<Option<Arc<crate::identity_first::IdentityRuntime>>>>;

struct AutoWireParentMobToolsFactory {
    inner: Arc<dyn meerkat_core::service::MobToolsFactory>,
    implicit_delegate_retirement_overrides: ImplicitDelegateRetirementOverrides,
    console_spawn_sink: SharedConsoleSpawnSinkSlot,
    identity_runtime: SharedIdentityRuntimeSlot,
    protected_mob_id: String,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl meerkat_core::service::MobToolsFactory for AutoWireParentMobToolsFactory {
    async fn build_mob_tools(
        &self,
        args: meerkat_core::service::MobToolsBuildArgs,
    ) -> Result<Arc<dyn meerkat_core::AgentToolDispatcher>, Box<dyn std::error::Error + Send + Sync>>
    {
        let spawner_comms_name = args.comms_name.clone();
        let inner = self.inner.build_mob_tools(args).await?;
        Ok(Arc::new(AutoWireParentMobToolDispatcher {
            inner,
            implicit_delegate_retirement_overrides: self
                .implicit_delegate_retirement_overrides
                .clone(),
            console_spawn_sink: Arc::clone(&self.console_spawn_sink),
            identity_runtime: Arc::clone(&self.identity_runtime),
            protected_mob_id: self.protected_mob_id.clone(),
            spawner_comms_name,
        }))
    }
}

struct AutoWireParentMobToolDispatcher {
    inner: Arc<dyn meerkat_core::AgentToolDispatcher>,
    implicit_delegate_retirement_overrides: ImplicitDelegateRetirementOverrides,
    /// Late-bound console sink; empty until a console-bearing runtime
    /// installs one, in which case successful spawns project into it.
    console_spawn_sink: SharedConsoleSpawnSinkSlot,
    /// Identity authority attaches after the raw mob tool factory is built.
    /// Every dispatcher reads this shared slot at call time.
    identity_runtime: SharedIdentityRuntimeSlot,
    protected_mob_id: String,
    /// Comms name of the agent owning this tool surface — identifies the
    /// spawning parent for console lineage.
    spawner_comms_name: Option<String>,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl meerkat_core::AgentToolDispatcher for AutoWireParentMobToolDispatcher {
    fn tools(&self) -> Arc<[Arc<meerkat_core::types::ToolDef>]> {
        self.inner
            .tools()
            .iter()
            .map(|tool| {
                if tool.name == "delegate" {
                    Arc::new(delegate_tool_def_with_idle_retire_secs(tool))
                } else if tool.name == "mob_spawn_member" {
                    Arc::new(mob_spawn_tool_def_with_idle_retire_secs(tool))
                } else {
                    Arc::clone(tool)
                }
            })
            .collect::<Vec<_>>()
            .into()
    }

    async fn dispatch(
        &self,
        call: meerkat_core::types::ToolCallView<'_>,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        self.dispatch_with_context(call, &meerkat_core::ToolDispatchContext::default())
            .await
    }

    async fn dispatch_with_context(
        &self,
        call: meerkat_core::types::ToolCallView<'_>,
        context: &meerkat_core::ToolDispatchContext,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        if matches!(
            call.name,
            "delegate"
                | "mob_spawn_member"
                | "mob_retire_member"
                | "mob_wire"
                | "mob_unwire"
                | "mob_destroy"
                | "spawn_member"
                | "spawn_many_members"
                | "retire_member"
                | "force_cancel_member"
                | "member_status"
                | "wire_members"
                | "unwire_members"
        ) {
            let args = serde_json::from_str::<Value>(call.args.get()).map_err(|error| {
                meerkat_core::ToolError::invalid_arguments(call.name, error.to_string())
            })?;
            if let Some((field, alias)) = reserved_raw_member_tool_argument(call.name, &args) {
                return Err(meerkat_core::ToolError::invalid_arguments(
                    call.name,
                    format!(
                        "{field} '{alias}' uses MobKit's reserved rt:* / mk-- member namespace; use a public non-reserved alias or the IdentityRuntime authority"
                    ),
                ));
            }
            let identity_runtime = self
                .identity_runtime
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some(identity_runtime) = identity_runtime {
                for (field, alias) in raw_member_tool_arguments(call.name, &args) {
                    let alias = crate::member_comms_id::runtime_alias_str(&alias).into_owned();
                    if identity_runtime
                        .identity_for_member_mutation(&alias)
                        .await
                        .is_some()
                    {
                        return Err(meerkat_core::ToolError::invalid_arguments(
                            call.name,
                            format!(
                                "{field} '{alias}' is owned by the attached IdentityRuntime; use the identity lifecycle/topology authority"
                            ),
                        ));
                    }
                }
                if call.name == "mob_destroy"
                    && args.get("mob_id").and_then(Value::as_str)
                        == Some(self.protected_mob_id.as_str())
                {
                    return Err(meerkat_core::ToolError::invalid_arguments(
                        call.name,
                        format!(
                            "mob '{}' is owned by the attached IdentityRuntime and cannot be destroyed through raw agent tools",
                            self.protected_mob_id
                        ),
                    ));
                }
            }
        }
        if call.name == "delegate" {
            return self.dispatch_delegate(call, context).await;
        }
        if call.name == "mob_spawn_member" {
            return self.dispatch_mob_spawn_member(call, context).await;
        }
        if crate::console_spawn::is_console_spawn_tool(call.name) {
            // Spawn variants this wrapper does not otherwise intercept
            // (e.g. spawn_member/spawn_many_members surfaces) still get
            // their members projected into the console.
            let args = serde_json::from_str::<Value>(call.args.get()).ok();
            let name = call.name.to_string();
            let outcome = self.inner.dispatch_with_context(call, context).await?;
            if let Some(args) = args {
                self.project_spawn_to_console(&name, &args, &outcome).await;
            }
            return Ok(outcome);
        }
        self.inner.dispatch_with_context(call, context).await
    }

    fn capabilities(&self) -> meerkat_core::agent::DispatcherCapabilities {
        self.inner.capabilities()
    }

    fn bind_ops_lifecycle(
        self: Arc<Self>,
        registry: Arc<dyn meerkat_core::ops_lifecycle::OpsLifecycleRegistry>,
        owner_bridge_session_id: meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::agent::BindOutcome, meerkat_core::agent::OpsLifecycleBindError> {
        let owned = Arc::try_unwrap(self)
            .map_err(|_| meerkat_core::agent::OpsLifecycleBindError::SharedOwnership)?;
        let outcome = owned
            .inner
            .bind_ops_lifecycle(registry, owner_bridge_session_id)?;
        let was_bound = outcome.was_bound();
        let dispatcher = Arc::new(Self {
            inner: outcome.into_dispatcher(),
            implicit_delegate_retirement_overrides: owned.implicit_delegate_retirement_overrides,
            console_spawn_sink: owned.console_spawn_sink,
            identity_runtime: owned.identity_runtime,
            protected_mob_id: owned.protected_mob_id,
            spawner_comms_name: owned.spawner_comms_name,
        });
        Ok(if was_bound {
            meerkat_core::agent::BindOutcome::Bound(dispatcher)
        } else {
            meerkat_core::agent::BindOutcome::Skipped(dispatcher)
        })
    }
}

/// Return the first raw mob-tool argument that attempts to create or mutate
/// an identity-runtime alias. Agent tools dispatch directly to meerkat-mob's
/// lower plane, so permitting either the public or encoded form here would
/// bypass durable lifecycle and topology authority.
fn reserved_raw_member_tool_argument(
    tool_name: &str,
    args: &Value,
) -> Option<(&'static str, String)> {
    raw_member_tool_arguments(tool_name, args)
        .into_iter()
        .find(|(_, value)| {
            crate::member_comms_id::is_reserved_generated_alias(value)
                || crate::member_comms_id::uses_reserved_roster_marker(value)
        })
        .map(|(field, value)| {
            (
                field,
                crate::member_comms_id::runtime_alias_str(&value).into_owned(),
            )
        })
}

fn raw_member_tool_arguments(tool_name: &str, args: &Value) -> Vec<(&'static str, String)> {
    let mut candidates = Vec::new();
    match tool_name {
        "delegate"
        | "mob_spawn_member"
        | "mob_retire_member"
        | "spawn_member"
        | "retire_member"
        | "force_cancel_member"
        | "member_status" => {
            candidates.push(("member_id", args.get("member_id").and_then(Value::as_str)));
        }
        "spawn_many_members" => {
            if let Some(specs) = args.get("specs").and_then(Value::as_array) {
                for spec in specs {
                    candidates.push((
                        "specs[].member_id",
                        spec.get("member_id").and_then(Value::as_str),
                    ));
                }
            }
        }
        "mob_wire" | "mob_unwire" => {
            candidates.push(("member_id", args.get("member_id").and_then(Value::as_str)));
            candidates.push((
                "peer.local",
                args.get("peer")
                    .and_then(|peer| peer.get("local"))
                    .and_then(Value::as_str),
            ));
        }
        "wire_members" | "unwire_members" => {
            candidates.push(("member_id", args.get("member_id").and_then(Value::as_str)));
            candidates.push((
                "peer_member_id",
                args.get("peer_member_id").and_then(Value::as_str),
            ));
        }
        _ => return Vec::new(),
    }
    candidates
        .into_iter()
        .filter_map(|(field, value)| value.map(|value| (field, value.to_string())))
        .collect()
}

impl AutoWireParentMobToolDispatcher {
    async fn dispatch_mob_spawn_member(
        &self,
        call: meerkat_core::types::ToolCallView<'_>,
        context: &meerkat_core::ToolDispatchContext,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        let mut args = serde_json::from_str::<Value>(call.args.get()).map_err(|error| {
            meerkat_core::ToolError::invalid_arguments(call.name, error.to_string())
        })?;
        let idle_retire_override = delegate_idle_retire_override_from_args(call.name, &mut args)?;
        let idle_retire_targets = idle_retire_targets_from_spawn_args(&args);
        if let Some(object) = args.as_object_mut() {
            object
                .entry("auto_wire_parent".to_string())
                .or_insert(Value::Bool(true));
        }
        let name = call.name.to_string();
        let args_for_console = args.clone();
        let args = serde_json::value::RawValue::from_string(args.to_string()).map_err(|error| {
            meerkat_core::ToolError::invalid_arguments(call.name, error.to_string())
        })?;
        let call = meerkat_core::types::ToolCallView {
            id: call.id,
            name: call.name,
            args: &args,
        };
        let outcome = self.inner.dispatch_with_context(call, context).await?;
        self.register_idle_retire_override_from_outcome(
            &outcome,
            idle_retire_override,
            &idle_retire_targets,
        )
        .await;
        self.project_spawn_to_console(&name, &args_for_console, &outcome)
            .await;

        Ok(outcome)
    }

    async fn dispatch_delegate(
        &self,
        call: meerkat_core::types::ToolCallView<'_>,
        context: &meerkat_core::ToolDispatchContext,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        let mut args = serde_json::from_str::<Value>(call.args.get()).map_err(|error| {
            meerkat_core::ToolError::invalid_arguments(call.name, error.to_string())
        })?;
        let idle_retire_override = delegate_idle_retire_override_from_args(call.name, &mut args)?;
        let name = call.name.to_string();
        let args_for_console = args.clone();
        let args = serde_json::value::RawValue::from_string(args.to_string()).map_err(|error| {
            meerkat_core::ToolError::invalid_arguments(call.name, error.to_string())
        })?;
        let call = meerkat_core::types::ToolCallView {
            id: call.id,
            name: call.name,
            args: &args,
        };
        let outcome = self.inner.dispatch_with_context(call, context).await?;

        self.register_idle_retire_override_from_outcome(&outcome, idle_retire_override, &[])
            .await;
        self.project_spawn_to_console(&name, &args_for_console, &outcome)
            .await;

        Ok(outcome)
    }

    /// Project a successful spawn into the console, when a console-bearing
    /// runtime installed a sink. Failure-isolated and additive: the spawn
    /// outcome is never altered, and a runtime without a console store
    /// behaves exactly as before.
    async fn project_spawn_to_console(
        &self,
        tool_name: &str,
        args: &Value,
        outcome: &meerkat_core::ToolDispatchOutcome,
    ) {
        if outcome.result.is_error {
            return;
        }
        let sink = self
            .console_spawn_sink
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(sink) = sink else {
            return;
        };
        let seeds = crate::console_spawn::console_spawn_seeds(
            tool_name,
            args,
            &outcome.result.text_content(),
            self.spawner_comms_name.as_deref(),
        );
        for seed in &seeds {
            sink.project_spawned_member(seed).await;
        }
    }

    async fn register_idle_retire_override_from_outcome(
        &self,
        outcome: &meerkat_core::ToolDispatchOutcome,
        idle_retire_override: Option<DelegateIdleRetireOverride>,
        fallback_targets: &[IdleRetireTarget],
    ) {
        if outcome.result.is_error {
            return;
        }
        let Some(override_policy) = idle_retire_override else {
            return;
        };
        for target in
            idle_retire_targets_from_outcome_text(&outcome.result.text_content(), fallback_targets)
        {
            self.implicit_delegate_retirement_overrides
                .set(&target.mob_id, &target.member_id, override_policy)
                .await;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct IdleRetireTarget {
    mob_id: String,
    member_id: String,
}

fn text_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

fn member_identity_field(value: &Value) -> Option<&str> {
    text_field(value, "agent_identity")
        .or_else(|| text_field(value, "member_id"))
        .or_else(|| text_field(value, "identity"))
}

fn target_from_value(value: &Value, default_mob_id: Option<&str>) -> Option<IdleRetireTarget> {
    let mob_id = text_field(value, "mob_id").or(default_mob_id)?;
    let member_id = member_identity_field(value)?;
    Some(IdleRetireTarget {
        mob_id: mob_id.to_string(),
        member_id: member_id.to_string(),
    })
}

fn idle_retire_targets_from_spawn_args(args: &Value) -> Vec<IdleRetireTarget> {
    let default_mob_id = text_field(args, "mob_id");
    let mut targets = BTreeSet::new();
    if let Some(target) = target_from_value(args, default_mob_id) {
        targets.insert(target);
    }
    for key in ["specs", "members"] {
        let Some(values) = args.get(key).and_then(Value::as_array) else {
            continue;
        };
        for value in values {
            if let Some(target) = target_from_value(value, default_mob_id) {
                targets.insert(target);
            }
        }
    }
    targets.into_iter().collect()
}

fn target_from_result_value(
    value: &Value,
    fallback_targets: &[IdleRetireTarget],
) -> Option<IdleRetireTarget> {
    if let Some(target) = target_from_value(value, None) {
        return Some(target);
    }
    let member_id = member_identity_field(value)?;
    let mut matches = fallback_targets
        .iter()
        .filter(|target| target.member_id == member_id);
    let target = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(target.clone())
}

fn collect_idle_retire_result_targets(
    value: &Value,
    fallback_targets: &[IdleRetireTarget],
    targets: &mut BTreeSet<IdleRetireTarget>,
) {
    if let Some(target) = target_from_result_value(value, fallback_targets) {
        targets.insert(target);
    }
    for key in ["members", "specs", "spawned", "results"] {
        let Some(values) = value.get(key).and_then(Value::as_array) else {
            continue;
        };
        for value in values {
            collect_idle_retire_result_targets(value, fallback_targets, targets);
        }
    }
}

fn idle_retire_targets_from_outcome_text(
    text: &str,
    fallback_targets: &[IdleRetireTarget],
) -> Vec<IdleRetireTarget> {
    let Ok(payload) = serde_json::from_str::<Value>(text) else {
        return fallback_targets.to_vec();
    };
    let mut targets = BTreeSet::new();
    collect_idle_retire_result_targets(&payload, fallback_targets, &mut targets);
    if targets.is_empty() {
        targets.extend(fallback_targets.iter().cloned());
    }
    targets.into_iter().collect()
}

struct DefinitionSeededRealmProfileStore {
    inner: Arc<dyn meerkat_mob::RealmProfileStore>,
    profiles: BTreeMap<String, Profile>,
}

impl DefinitionSeededRealmProfileStore {
    fn new(
        definition: &MobDefinition,
        inner: Arc<dyn meerkat_mob::RealmProfileStore>,
    ) -> Option<Self> {
        let profiles = definition
            .profiles
            .iter()
            .filter_map(|(name, binding)| {
                binding
                    .as_inline()
                    .cloned()
                    .map(|profile| (name.to_string(), profile))
            })
            .collect::<BTreeMap<_, _>>();

        (!profiles.is_empty()).then_some(Self { inner, profiles })
    }

    fn stored(&self, name: &str, profile: &Profile) -> meerkat_mob::StoredRealmProfile {
        let now = chrono::Utc::now();
        meerkat_mob::StoredRealmProfile {
            name: name.to_string(),
            profile: profile.clone(),
            revision: 0,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl meerkat_mob::RealmProfileStore for DefinitionSeededRealmProfileStore {
    async fn create(
        &self,
        name: &str,
        profile: &Profile,
    ) -> Result<meerkat_mob::StoredRealmProfile, meerkat_mob::MobStoreError> {
        if self.profiles.contains_key(name) {
            return Err(meerkat_mob::MobStoreError::CasConflict(format!(
                "realm profile already exists: {name}"
            )));
        }
        self.inner.create(name, profile).await
    }

    async fn get(
        &self,
        name: &str,
    ) -> Result<Option<meerkat_mob::StoredRealmProfile>, meerkat_mob::MobStoreError> {
        if let Some(profile) = self.profiles.get(name) {
            return Ok(Some(self.stored(name, profile)));
        }
        self.inner.get(name).await
    }

    async fn list(
        &self,
    ) -> Result<Vec<meerkat_mob::StoredRealmProfile>, meerkat_mob::MobStoreError> {
        let mut merged = self.inner.list().await?;
        merged.retain(|profile| !self.profiles.contains_key(profile.name.as_str()));
        merged.extend(
            self.profiles
                .iter()
                .map(|(name, profile)| self.stored(name, profile)),
        );
        merged.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(merged)
    }

    async fn update(
        &self,
        name: &str,
        profile: &Profile,
        expected_revision: u64,
    ) -> Result<meerkat_mob::StoredRealmProfile, meerkat_mob::MobStoreError> {
        if self.profiles.contains_key(name) {
            return Err(meerkat_mob::MobStoreError::CasConflict(format!(
                "realm profile '{name}' is provided by the mob definition"
            )));
        }
        self.inner.update(name, profile, expected_revision).await
    }

    async fn delete(
        &self,
        name: &str,
        expected_revision: u64,
    ) -> Result<meerkat_mob::StoredRealmProfile, meerkat_mob::MobStoreError> {
        if self.profiles.contains_key(name) {
            return Err(meerkat_mob::MobStoreError::CasConflict(format!(
                "realm profile '{name}' is provided by the mob definition"
            )));
        }
        self.inner.delete(name, expected_revision).await
    }
}

fn delegate_idle_retire_override_from_args(
    tool_name: &str,
    args: &mut Value,
) -> Result<Option<DelegateIdleRetireOverride>, meerkat_core::ToolError> {
    let Some(object) = args.as_object_mut() else {
        return Ok(None);
    };
    let Some(value) = object.remove("idle_retire_secs") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(Some(DelegateIdleRetireOverride::Disabled));
    }
    value
        .as_u64()
        .map(DelegateIdleRetireOverride::Seconds)
        .map(Some)
        .ok_or_else(|| {
            meerkat_core::ToolError::invalid_arguments(
                tool_name,
                "idle_retire_secs must be a non-negative integer or null",
            )
        })
}

fn delegate_tool_def_with_idle_retire_secs(
    tool: &meerkat_core::types::ToolDef,
) -> meerkat_core::types::ToolDef {
    let mut patched = tool.clone();
    if !patched.description.contains("IDLE RETIREMENT:") {
        patched.description.push_str(
            "\n\nIDLE RETIREMENT:\n\
             Omit idle_retire_secs to use the runtime default. Pass an integer \
             number of seconds to override idle auto-retirement for this helper. \
             Pass null to disable auto-retirement for this helper.",
        );
    }
    if let Some(properties) = patched
        .input_schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
    {
        properties
            .entry("idle_retire_secs".to_string())
            .or_insert_with(|| {
                serde_json::json!({
                    "description": "Override idle auto-retirement for this helper. Omit to use the runtime default, use an integer number of seconds to override, or null to disable auto-retirement for this helper.",
                    "anyOf": [
                        {"type": "integer", "minimum": 0},
                        {"type": "null"}
                    ]
                })
            });
    }
    patched
}

fn mob_spawn_tool_def_with_idle_retire_secs(
    tool: &meerkat_core::types::ToolDef,
) -> meerkat_core::types::ToolDef {
    let mut patched = tool.clone();
    if !patched.description.contains("IDLE RETIREMENT:") {
        patched.description.push_str(
            "\n\nIDLE RETIREMENT:\n\
             Omit idle_retire_secs to leave this spawned member out of auto-retirement. \
             Pass an integer number of seconds to retire the member after it has been \
             idle for that long. Pass null to explicitly disable auto-retirement.",
        );
    }
    if let Some(properties) = patched
        .input_schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
    {
        properties
            .entry("idle_retire_secs".to_string())
            .or_insert_with(|| {
                serde_json::json!({
                    "description": "Opt this spawned member into idle auto-retirement. Omit to keep the member indefinitely, use an integer number of seconds to retire after that much idle time, or null to explicitly disable auto-retirement.",
                    "anyOf": [
                        {"type": "integer", "minimum": 0},
                        {"type": "null"}
                    ]
                })
            });
    }
    patched
}

fn install_agent_mob_tools(
    definition: &MobDefinition,
    slot: Arc<std::sync::RwLock<Option<Arc<dyn meerkat_core::service::MobToolsFactory>>>>,
    session_service: Arc<dyn MobSessionService>,
    workgraph_service: Option<meerkat::WorkGraphService>,
    default_llm_client_slot: Option<SharedDefaultLlmClientSlot>,
) -> (
    Arc<meerkat_mob_mcp::MobMcpState>,
    ImplicitDelegateRetirementOverrides,
    SharedDefaultLlmClientSlot,
    SharedConsoleSpawnSinkSlot,
    SharedIdentityRuntimeSlot,
) {
    let default_llm_client_slot = default_llm_client_slot
        .unwrap_or_else(|| Arc::new(std::sync::RwLock::new(None::<Arc<dyn LlmClient>>)));
    let default_llm_client_provider_slot = Arc::clone(&default_llm_client_slot);
    // Forward the workgraph service so agent-spawned child mobs
    // (delegate / mob_spawn_member) inherit apply-time attention overlays.
    let mut state =
        meerkat_mob_mcp::MobMcpState::new(session_service, meerkat_mob::MobControlPrincipal::Owner)
            .with_workgraph_service(workgraph_service);
    if let Some(base_store) = state.realm_profile_store().cloned()
        && let Some(store) = DefinitionSeededRealmProfileStore::new(definition, base_store)
    {
        state = state.with_realm_profile_store(Some(Arc::new(store)));
    }
    state = state
        .with_realm_skill_sources(definition.skills.clone())
        .with_default_llm_client_provider(Some(Arc::new(move || {
            default_llm_client_provider_slot
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })));
    let state = Arc::new(state);
    let implicit_delegate_retirement_overrides = ImplicitDelegateRetirementOverrides::default();
    let console_spawn_sink = new_console_spawn_sink_slot();
    let identity_runtime = Arc::new(std::sync::RwLock::new(None));
    let inner = Arc::new(meerkat_mob_mcp::AgentMobToolSurfaceFactory::new(
        Arc::clone(&state),
    ));
    let factory = Arc::new(AutoWireParentMobToolsFactory {
        inner,
        implicit_delegate_retirement_overrides: implicit_delegate_retirement_overrides.clone(),
        console_spawn_sink: Arc::clone(&console_spawn_sink),
        identity_runtime: Arc::clone(&identity_runtime),
        protected_mob_id: definition.id.to_string(),
    });
    *slot
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(factory);
    (
        state,
        implicit_delegate_retirement_overrides,
        default_llm_client_slot,
        console_spawn_sink,
        identity_runtime,
    )
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct RuntimeTurnTrace {
    pub(crate) session_id: String,
    pub(crate) boundary: String,
    pub(crate) contributing_input_count: usize,
    pub(crate) outcome: String,
}

fn is_replay_unsafe_server_tool_content(name: &str, content: &Value) -> bool {
    name == "web_search_annotations"
        || content
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.starts_with("response."))
}

fn sanitize_llm_request_for_stateless_replay(request: &LlmRequest) -> LlmRequest {
    let mut sanitized = request.clone();
    sanitized.messages = request
        .messages
        .iter()
        .cloned()
        .map(sanitize_message_for_stateless_replay)
        .collect();
    sanitized
}

fn sanitize_create_session_request_llm_override(req: &mut CreateSessionRequest) {
    let Some(build) = req.build.as_mut() else {
        return;
    };
    let Some(client) = build
        .llm_client_override
        .as_ref()
        .and_then(meerkat::decode_llm_client_override_from_service)
    else {
        return;
    };
    build.llm_client_override = Some(meerkat::encode_llm_client_override_for_service(
        ReplaySanitizingLlmClient::wrap(client),
    ));
}

const SHELL_BUILTIN_TOOL_NAMES: [&str; 4] = [
    "shell",
    "shell_job_status",
    "shell_jobs",
    "shell_job_cancel",
];
const COMMS_TOOL_NAMES: [&str; 4] = ["peers", "send_message", "send_request", "send_response"];

fn shell_and_comms_tool_filter() -> meerkat_core::ToolFilter {
    meerkat_core::ToolFilter::Allow(
        SHELL_BUILTIN_TOOL_NAMES
            .iter()
            .chain(COMMS_TOOL_NAMES.iter())
            .map(|name| (*name).to_string())
            .collect(),
    )
}

/// Shell is implemented by Meerkat's native builtin dispatcher, but MobKit
/// profiles treat `tools.shell` as independent from general `tools.builtins`.
/// When a profile asks for shell-only access, force the parent builtin
/// substrate on and install a session-local allow filter so broad builtins
/// remain hidden while shell and comms stay available.
pub fn ensure_shell_tooling_build_substrate(req: &mut CreateSessionRequest) {
    let Some(build) = req.build.as_mut() else {
        return;
    };
    if matches!(
        build.override_shell,
        meerkat_core::ToolCategoryOverride::Enable
    ) && matches!(
        build.override_builtins,
        meerkat_core::ToolCategoryOverride::Disable
    ) {
        build.override_builtins = meerkat_core::ToolCategoryOverride::Enable;
        if build.initial_tool_filter.is_none() {
            build.initial_tool_filter = Some(shell_and_comms_tool_filter());
        }
    }
}

fn sanitize_message_for_stateless_replay(message: Message) -> Message {
    match message {
        Message::BlockAssistant(mut assistant) => {
            assistant.blocks = assistant
                .blocks
                .into_iter()
                .filter_map(|block| match block {
                    // Meerkat 0.7 types the server-tool name as `ServerToolKind`;
                    // the replay-unsafe predicate keys on the provider-native name.
                    AssistantBlock::ServerToolContent { kind, content, .. }
                        if is_replay_unsafe_server_tool_content(kind.provider_name(), &content) =>
                    {
                        None
                    }
                    other => Some(other),
                })
                .collect();
            Message::BlockAssistant(assistant)
        }
        other => other,
    }
}

/// Open the persistent runtime store that holds the authoritative
/// session snapshot used by `load_persisted_session` (resume path) and
/// `load_persisted_session_for_control` (archive/retire path). Lives at
/// `<store_path>/runtime.sqlite` — separate file from the session
/// store so we don't depend on the session_store's concrete type.
///
/// Fail-closed (M4): an open failure is a startup error, never a silent
/// `InMemoryRuntimeStore` twin — in that formerly-degraded mode resume
/// across restart and archive operations fail long after boot. An
/// in-memory runtime store remains constructible only as an explicit
/// declaration (`UnifiedRuntimeBuilder::ephemeral_runtime_store(true)`).
fn build_persistent_runtime_store(
    store_path: &Path,
) -> Result<Arc<dyn meerkat_runtime::RuntimeStore>, RuntimeStoreResolutionError> {
    let runtime_db = store_path.join(crate::storage_layout::RUNTIME_DB_FILE_NAME);
    match meerkat_runtime::store::SqliteRuntimeStore::new(&runtime_db) {
        Ok(store) => Ok(Arc::new(store)),
        Err(err) => Err(RuntimeStoreResolutionError {
            path: runtime_db,
            message: err.to_string(),
        }),
    }
}

/// Monotonic per-runtime write epochs observed at this process's single
/// runtime-store seam. Every session-scoped durable write (session snapshot,
/// machine lifecycle, input state, ops lifecycle) advances the runtime's
/// epoch; [`SessionDocumentReadAbsorber`] keys its cached authoritative reads
/// on the epoch so an unchanged epoch proves the durable authority for that
/// session did not move through this process.
///
/// Correct only while this process is the sole writer of the underlying
/// runtime store (the identity single-embodiment lease guard already enforces
/// one live gateway per store; storage doctor tooling opens stores read-only).
#[derive(Default)]
pub(crate) struct SessionSnapshotWriteEpochs {
    epochs: std::sync::Mutex<BTreeMap<String, u64>>,
}

impl SessionSnapshotWriteEpochs {
    /// Advance before the write is attempted: a failed write may have
    /// partially applied in an unknown store, so over-invalidation is the
    /// safe direction.
    fn advance(&self, runtime_id: &meerkat_runtime::LogicalRuntimeId) {
        let mut epochs = self
            .epochs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *epochs.entry(runtime_id.0.clone()).or_insert(0) += 1;
    }

    fn observe(&self, session_id: &meerkat_core::types::SessionId) -> u64 {
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(session_id);
        self.epochs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&runtime_id.0)
            .copied()
            .unwrap_or(0)
    }
}

/// Public handle to this process's per-session durable write-epoch witness
/// (see [`SessionSnapshotWriteEpochs`]). Obtained from
/// [`epoch_tracking_runtime_store`]; hand it to
/// [`MobBootstrapSpec::with_session_write_epochs`] so the console
/// session-history discovery loop and whole-document read absorption can
/// skip re-reads while a session's epoch is unchanged.
#[derive(Clone)]
pub struct SessionWriteEpochsHandle {
    pub(crate) epochs: Arc<SessionSnapshotWriteEpochs>,
}

/// Wrap a runtime store so every session-scoped durable write advances the
/// returned per-session write-epoch witness.
///
/// Externally-composed runtimes (the gateway binaries roll their own stores
/// and session services and enter through [`MobBootstrapSpec::new`]) MUST
/// wrap BEFORE handing the store to `MeerkatMachine::persistent` /
/// `PersistentSessionService::new`: the witness is sound only if every
/// session-scoped write in this process goes through the returned store.
/// The stock persistent bootstrap constructors do this internally.
pub fn epoch_tracking_runtime_store(
    inner: Arc<dyn meerkat_runtime::RuntimeStore>,
) -> (
    Arc<dyn meerkat_runtime::RuntimeStore>,
    SessionWriteEpochsHandle,
) {
    let epochs = Arc::new(SessionSnapshotWriteEpochs::default());
    let store: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
        SessionStoreBackedRuntimeStore::with_write_epochs(inner, Arc::clone(&epochs)),
    );
    (store, SessionWriteEpochsHandle { epochs })
}

/// [`epoch_tracking_runtime_store`] plus the durable session projection and
/// every-boot runtime-authority re-minting.
///
/// At meerkat 0.8.11 the session service keeps no plain `SessionStore` write
/// path of its own (WholeBlob session authority lives only in the
/// `RuntimeStore`), so an externally-composed runtime that pairs a durable
/// `session_store` with its runtime store MUST wrap through this seam or
/// committed session boundaries never reach that store. The mint arms for
/// EVERY composition carrying a durable session source, durable inner
/// stores included: an absent runtime record over a durable inner store
/// means either a never-persisted session (no durable row - the mint
/// declines and the upstream refusal stands) or a reset/lost runtime store,
/// the sanctioned recovery path that must reseed. Destroyed sessions cannot
/// resurrect through it because identity deletion removes the durable row
/// under the identity fence.
pub fn epoch_tracking_runtime_store_with_durable_projection(
    inner: Arc<dyn meerkat_runtime::RuntimeStore>,
    session_store: Arc<dyn SessionStore>,
) -> (
    Arc<dyn meerkat_runtime::RuntimeStore>,
    SessionWriteEpochsHandle,
) {
    let epochs = Arc::new(SessionSnapshotWriteEpochs::default());
    let store: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
        SessionStoreBackedRuntimeStore::with_write_epochs_and_durable_projection(
            inner,
            Arc::clone(&epochs),
            session_store,
        ),
    );
    (store, SessionWriteEpochsHandle { epochs })
}

/// Absorbs repeated authoritative session-document reads while the session's
/// durable authority is unchanged.
///
/// meerkat-mob 0.8.5's identity reconcile loop re-reads and checkpoint-
/// verifies each durable member's full session document once per scan
/// interval even when nothing changed — on the HomeCore fleet that is one
/// 82 MB deserialize + canonical sha256 per member per second (~0.3 CPU
/// cores per idle member). This absorber serves the previously decoded
/// document (a cheap copy-on-write clone) while the runtime-store write
/// epoch for that session is unchanged.
///
/// Memory constraint: one decoded document per recently-read session stays
/// resident (Session shares its transcript via `Arc`), bounded by the durable
/// fleet's persisted transcript sizes. That residency replaces an unbounded
/// per-second decode+digest burn.
pub(crate) struct SessionDocumentReadAbsorber {
    epochs: Arc<SessionSnapshotWriteEpochs>,
    // Keyed by the session id's canonical string form (SessionId is not Ord).
    cache: std::sync::Mutex<BTreeMap<String, AbsorbedSessionDocument>>,
}

struct AbsorbedSessionDocument {
    epoch: u64,
    session: meerkat_core::session::Session,
}

impl SessionDocumentReadAbsorber {
    pub(crate) fn new(epochs: Arc<SessionSnapshotWriteEpochs>) -> Self {
        Self {
            epochs,
            cache: std::sync::Mutex::new(BTreeMap::new()),
        }
    }

    fn observe_epoch(&self, session_id: &meerkat_core::types::SessionId) -> u64 {
        self.epochs.observe(session_id)
    }

    /// Serve the cached document only when its admission epoch still matches
    /// the current write epoch; stale entries are dropped on lookup.
    fn lookup(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Option<meerkat_core::session::Session> {
        let current = self.epochs.observe(session_id);
        let key = session_id.to_string();
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match cache.get(&key) {
            Some(entry) if entry.epoch == current => Some(entry.session.clone()),
            Some(_) => {
                cache.remove(&key);
                None
            }
            None => None,
        }
    }

    /// Admit under the epoch observed BEFORE the inner load: a write racing
    /// the load bumps the current epoch past `epoch`, so the next lookup
    /// misses and re-reads instead of trusting a possibly-torn read.
    fn admit(
        &self,
        session_id: &meerkat_core::types::SessionId,
        epoch: u64,
        session: &meerkat_core::session::Session,
    ) {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                session_id.to_string(),
                AbsorbedSessionDocument {
                    epoch,
                    session: session.clone(),
                },
            );
    }

    fn evict(&self, session_id: &meerkat_core::types::SessionId) {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&session_id.to_string());
    }
}

/// RuntimeStore forwarding facade for identity-first apps with an external
/// compatibility projection.
///
/// `PersistentSessionService` treats `RuntimeStore` as the authoritative
/// session snapshot source whenever one is installed. The supplied
/// `SessionStore` must therefore remain a compatibility projection rather than
/// being read back through this facade as runtime authority. Store-only
/// recovery remains owned by Meerkat's session-document machine;
/// this facade must not reinterpret the projection as runtime authority.
///
/// When constructed with [`Self::with_write_epochs`], the facade additionally
/// records a per-runtime write epoch for every session-scoped mutating
/// method, feeding [`SessionDocumentReadAbsorber`] invalidation.
struct SessionStoreBackedRuntimeStore {
    inner: Arc<dyn meerkat_runtime::RuntimeStore>,
    write_epochs: Option<Arc<SessionSnapshotWriteEpochs>>,
    /// The injected durable session/continuity store this facade keeps in
    /// sync with the inner runtime store.
    ///
    /// Write side: committed session boundaries project into it via
    /// [`Self::project_committed_session_to_durable`]. Read side: it is the
    /// durable source for every-boot runtime-authority re-minting -
    /// deterministic reconstruction from the same durable facts, fail-closed
    /// on absent facts (ruled in-design by the meerkat lead, 2026-07-31).
    /// The mint arms on EVERY shape carrying this store, durable inner
    /// stores included: an absent runtime record over a durable inner store
    /// is either a never-persisted session (no durable row - the mint
    /// declines and the upstream refusal stands) or a reset/lost runtime
    /// store, the sanctioned recovery path that must reseed from here.
    /// Destroyed sessions cannot resurrect through it: identity deletion
    /// removes this store's row under the identity fence, and resume only
    /// asks for session ids a continuity record binds.
    session_store: Option<Arc<dyn SessionStore>>,
    /// PER-RUNTIME single-flight fences over authority re-minting:
    /// concurrent cold activations of ONE runtime racing the three authority
    /// reads must collapse to ONE committed seed, and a late seed must never
    /// overwrite a boundary a real turn has already advanced (the in-lock
    /// re-read serves the current record instead of seeding). Distinct
    /// runtimes mint independently - a fleet-wide cold boot must not
    /// serialize every activation behind one lock. Weak entries keep one
    /// mutex across concurrent racers without retaining an allocation
    /// forever for every runtime ever observed.
    mint_flights: std::sync::Mutex<
        std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>,
    >,
    /// PER-RUNTIME single-flight fences over durable projection: two
    /// committing verbs racing the rewrite-replay chain walk for ONE runtime
    /// must not interleave their per-commit `save_transcript_rewrite` steps
    /// (each step validates against the durable head its predecessor
    /// installed). Same weak-entry shape as [`Self::mint_flights`]; distinct
    /// runtimes project independently.
    projection_flights: std::sync::Mutex<
        std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>,
    >,
    /// Runtimes whose present committed authority has been checked once this
    /// process against the durable session row (see
    /// [`Self::freshen_stale_runtime_authority_from_durable`]). Staleness is
    /// a boot condition — a runtime store file restored from backup — so one
    /// check per runtime per process suffices, and the set is bounded by the
    /// runtimes this process activates.
    freshened: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl SessionStoreBackedRuntimeStore {
    fn new(
        inner: Arc<dyn meerkat_runtime::RuntimeStore>,
        session_store: Arc<dyn SessionStore>,
    ) -> Self {
        Self {
            inner,
            write_epochs: None,
            session_store: Some(session_store),
            mint_flights: std::sync::Mutex::new(std::collections::HashMap::new()),
            projection_flights: std::sync::Mutex::new(std::collections::HashMap::new()),
            freshened: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    fn with_write_epochs(
        inner: Arc<dyn meerkat_runtime::RuntimeStore>,
        write_epochs: Arc<SessionSnapshotWriteEpochs>,
    ) -> Self {
        Self {
            inner,
            write_epochs: Some(write_epochs),
            session_store: None,
            mint_flights: std::sync::Mutex::new(std::collections::HashMap::new()),
            projection_flights: std::sync::Mutex::new(std::collections::HashMap::new()),
            freshened: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// [`Self::with_write_epochs`] plus the durable session projection, for
    /// the persistent composition where one epoch-observing facade fronts
    /// both the machine and the session service.
    fn with_write_epochs_and_durable_projection(
        inner: Arc<dyn meerkat_runtime::RuntimeStore>,
        write_epochs: Arc<SessionSnapshotWriteEpochs>,
        session_store: Arc<dyn SessionStore>,
    ) -> Self {
        Self {
            inner,
            write_epochs: Some(write_epochs),
            session_store: Some(session_store),
            mint_flights: std::sync::Mutex::new(std::collections::HashMap::new()),
            projection_flights: std::sync::Mutex::new(std::collections::HashMap::new()),
            freshened: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Re-mint store-issued runtime authority from durable session facts for
    /// a runtime the (ephemeral) inner store has no record of.
    ///
    /// The durable session is read through the IMPORTING session-store loader
    /// (a released 0.8.10 envelope imports in this same activation), then
    /// committed into the inner store, whose atomic commit mints the current
    /// store-issued authority - nothing is fabricated facade-side. Returns
    /// false (and mints nothing) when no durable facts exist, keeping the
    /// upstream refusal fail-closed.
    ///
    /// Archived revival stays fail-closed on the durable-inner shapes this
    /// now arms for. With an intact inner store an archived session HAS a
    /// record (meerkat archive retains content and lifecycle authority), so
    /// the mint never fires and revival is lease-gated upstream. After a
    /// store reset, the IDENTITY domain still knows the member is retired
    /// and routes revival through `authorize_revivable_retired_session`,
    /// whose machine-authorized archived-resume lease requires lifecycle
    /// evidence this mint deliberately never synthesizes - the seed is a
    /// session-control snapshot only (no lifecycle, input, receipt, or ops
    /// synthesis), so a reset store cannot silently turn an archived member
    /// live through this path.
    async fn mint_runtime_authority_from_durable(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<bool, meerkat_runtime::store::RuntimeStoreError> {
        let Some(session_store) = self.session_store.as_ref() else {
            return Ok(false);
        };
        // The seed verb is a WholeBlob session-control snapshot; a store
        // declaring any other persistence profile must not be seeded
        // through it.
        if self.inner.session_persistence_profile()
            != meerkat_runtime::store::RuntimeSessionPersistenceProfile::WholeBlobV1
        {
            return Ok(false);
        }
        let raw = runtime_id.0.as_str();
        let candidate = raw.strip_prefix("rt:session:").unwrap_or(raw);
        let Ok(session_id) = meerkat_core::types::SessionId::parse(candidate) else {
            return Ok(false);
        };
        // Per-runtime single-flight: exactly one racer per runtime seeds;
        // the rest converge on the in-lock re-read below. A record that
        // appeared while waiting - including one a real turn has already
        // advanced past the seed - WINS: the current record is served and
        // nothing is overwritten. Distinct runtimes proceed in parallel.
        let flight = self.mint_flight_for(runtime_id);
        let _flight = flight.lock().await;
        if self
            .inner
            .load_whole_blob_store_authority(runtime_id)
            .await?
            .is_some()
        {
            return Ok(true);
        }
        // An absent snapshot WITH a catalog entry is a lifecycle fact the
        // inner store is stating (archived/cleared mid-flow), not a cold or
        // reset store - re-seeding would overwrite that statement (e.g.
        // read an archived session back to life). Only a store that knows
        // NOTHING about the runtime mints.
        if self
            .inner
            .load_runtime_session_catalog_entry(runtime_id)
            .await?
            .is_some()
        {
            return Ok(false);
        }
        let session = session_store.load(&session_id).await.map_err(|e| {
            meerkat_runtime::store::RuntimeStoreError::ReadFailed(format!(
                "durable session read for runtime-authority mint: {e}"
            ))
        })?;
        let Some(session) = session else {
            return Ok(false);
        };
        let bytes = session.to_persisted_bytes().map_err(|e| {
            meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                "durable session encode for runtime-authority mint: {e}"
            ))
        })?;
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .commit_session_snapshot(
                runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(bytes),
                },
            )
            .await;
        self.note_session_scoped_write(runtime_id);
        result?;
        tracing::info!(
            runtime_id = %runtime_id,
            session_id = %session_id,
            "runtime authority re-minted from durable session facts \
             (ephemeral runtime-store activation)"
        );
        Ok(true)
    }

    /// Advisory Form 1 (the 0.8.9 stale-runtime-snapshot failure, recurred
    /// at the 0.8.11 store-owned repin): a runtime store file restored from
    /// backup or rolled back mid-fleet holds committed authority STRICTLY
    /// BEHIND the durable continuity row this facade projects every
    /// committed boundary into. Store-owned reads then serve the stale
    /// snapshot — resume silently drops durably recorded turns — and the
    /// next committed boundary tries to project that regression back over
    /// the newer durable document.
    ///
    /// The write-through projection makes "durable strictly newer than
    /// committed runtime authority" impossible in normal operation: a failed
    /// projection fails its committing verb, so the runtime side may only
    /// run AHEAD by one unacknowledged boundary, never behind. Observing the
    /// inversion therefore proves runtime-store loss, and the durable row —
    /// every byte of it projected from a store-issued committed boundary —
    /// is the recovery source, exactly like the absent-record mint above.
    /// Newness is the monotonic pair (transcript rewrite generation, message
    /// count): rewrites advance the generation, ordinary turns extend the
    /// messages within one, so a compacted-shorter durable document still
    /// orders ahead of the pre-rewrite snapshot it superseded.
    ///
    /// Runs once per runtime per process (staleness is a boot condition, and
    /// the probe costs one durable document read), under the same
    /// single-flight fence the mint uses. Before reseeding, the boundary
    /// save guard runs HERE, against the exact committed snapshot: genuine
    /// divergence (a durable row that orders newer but does not extend the
    /// committed document) is refused typed rather than silently adopted in
    /// either direction. The guard cannot be left to the inner seed verb —
    /// `commit_session_snapshot` treats a session it has no legacy previous
    /// row for as first-save ADOPTION, which is exactly the pick-a-winner
    /// this refusal exists to prevent. A catalog entry carrying a lifecycle
    /// terminal blocks the RESEED direction only (terminal lifecycle facts
    /// outrank content recovery; re-seeding runtime authority is where
    /// resurrection risk lives) - the opposite direction, durable BEHIND
    /// committed (the parent-1 tear), reconciles even under a terminal
    /// because repairing the durable projection of already-committed
    /// authority mints no runtime life.
    async fn freshen_stale_runtime_authority_from_durable(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        let Some(session_store) = self.session_store.as_ref() else {
            return Ok(());
        };
        if self.inner.session_persistence_profile()
            != meerkat_runtime::store::RuntimeSessionPersistenceProfile::WholeBlobV1
        {
            return Ok(());
        }
        if self
            .freshened
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(runtime_id.0.as_str())
        {
            return Ok(());
        }
        let raw = runtime_id.0.as_str();
        let candidate = raw.strip_prefix("rt:session:").unwrap_or(raw);
        let Ok(session_id) = meerkat_core::types::SessionId::parse(candidate) else {
            return Ok(());
        };
        let flight = self.mint_flight_for(runtime_id);
        let _flight = flight.lock().await;
        let mark_fresh = || {
            self.freshened
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(runtime_id.0.clone());
        };
        if self
            .freshened
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(runtime_id.0.as_str())
        {
            return Ok(());
        }
        let lifecycle_terminal = self
            .inner
            .load_runtime_session_catalog_entry(runtime_id)
            .await?
            .is_some_and(|entry| entry.lifecycle_terminal().is_some());
        let Some(committed) = self
            .inner
            .load_committed_whole_blob_snapshot(runtime_id)
            .await?
        else {
            // Nothing committed to be stale; the absent-record mint owns
            // this shape.
            mark_fresh();
            return Ok(());
        };
        let durable = session_store.load(&session_id).await.map_err(|e| {
            meerkat_runtime::store::RuntimeStoreError::ReadFailed(format!(
                "durable session read for runtime-authority freshness probe: {e}"
            ))
        })?;
        let Some(durable) = durable else {
            // Committed authority with NO durable row at all: the FIRST
            // projection failed with its committing verb, and nothing would
            // retry it until some future committing verb - a plain resume
            // must clear the projection debt instead of stranding it. Same
            // reconciliation, same single-flight; the guard's adoption
            // branch owns the first-save shape. Runs under a lifecycle
            // terminal for the same reason as the durable-behind arm.
            self.project_committed_session_to_durable(runtime_id)
                .await?;
            mark_fresh();
            return Ok(());
        };
        let order_of = |session: &meerkat_core::Session| {
            session
                .transcript_rewrite_generation()
                .map(|generation| (generation, session.messages().len()))
                .map_err(|e| {
                    meerkat_runtime::store::RuntimeStoreError::ReadFailed(format!(
                        "transcript rewrite generation for runtime-authority freshness probe: {e}"
                    ))
                })
        };
        let durable_order = order_of(&durable)?;
        let committed_order = order_of(committed.session())?;
        if durable_order == committed_order {
            // Equal (generation, message-count) order is necessary but NOT
            // sufficient for freshness: a session-store restore from a
            // different lineage can coincide on both counts while carrying
            // different content - a FORK, not staleness in either
            // direction, and out-of-band file restores are exactly this
            // probe's threat model, so no in-process ordering argument can
            // rule the shape out. Fork adjudication is not this probe's to
            // make: refuse typed, loudly and repeatably, exactly like the
            // divergent-ahead refusal below. Exact revision equality marks
            // fresh.
            let durable_revision = durable.transcript_revision().map_err(|e| {
                meerkat_runtime::store::RuntimeStoreError::ReadFailed(format!(
                    "durable transcript revision for runtime-authority freshness probe: {e}"
                ))
            })?;
            let committed_revision = committed.session().transcript_revision().map_err(|e| {
                meerkat_runtime::store::RuntimeStoreError::ReadFailed(format!(
                    "committed transcript revision for runtime-authority freshness \
                         probe: {e}"
                ))
            })?;
            if durable_revision != committed_revision {
                return Err(meerkat_runtime::store::RuntimeStoreError::WriteFailed(
                    format!(
                        "durable session row for runtime {runtime_id} matches the committed \
                         runtime authority on (rewrite generation {}, message count {}) but \
                         DIVERGES in content (durable revision {durable_revision}, committed \
                         revision {committed_revision}): a fork between lineages; refusing \
                         to adopt either side",
                        durable_order.0, durable_order.1
                    ),
                ));
            }
            // Exact revision equality still does not prove the durable
            // ENVELOPE is current: a failure after every rewrite save but
            // before the final authoritative projection leaves generation,
            // count, and revision identical while usage/metadata lag. The
            // projection door OWNS the envelope definition, so no
            // field-list comparison can be proved complete against it -
            // compare the full persisted encodings instead. Identical
            // bytes make debt impossible (nothing for the projection to
            // change, no write spent); any difference runs the idempotent
            // reconciliation before mark_fresh, so envelope debt clears on
            // a plain resume instead of stranding.
            let durable_bytes = durable.to_persisted_bytes().map_err(|e| {
                meerkat_runtime::store::RuntimeStoreError::ReadFailed(format!(
                    "durable session encode for envelope-currency probe: {e}"
                ))
            })?;
            let committed_bytes = committed.session().to_persisted_bytes().map_err(|e| {
                meerkat_runtime::store::RuntimeStoreError::ReadFailed(format!(
                    "committed session encode for envelope-currency probe: {e}"
                ))
            })?;
            if durable_bytes != committed_bytes {
                self.project_committed_session_to_durable(runtime_id)
                    .await?;
            }
            mark_fresh();
            return Ok(());
        }
        if durable_order < committed_order {
            // Durable BEHIND committed: the parent-1 tear shape. A projection
            // that failed (or, pre-fix, could not install a rewrite
            // generation) left the durable row behind authority the runtime
            // store already committed - a PLAIN RESUME must converge it, not
            // wait for the next committing verb. The committed->durable
            // reconciliation (rewrite-suffix replay + trailing projection)
            // runs here, under this probe's single-flight, before the
            // runtime is marked fresh. It runs EVEN under a lifecycle
            // terminal: repairing the durable projection of already-committed
            // authority mints no runtime life and resurrects nothing; the
            // terminal gate stays on the reseed direction below, where
            // adopting durable content into the runtime store is exactly the
            // resurrection it exists to prevent. A reconciliation failure
            // fails the probe typed (retryable), never a torn mark-fresh.
            self.project_committed_session_to_durable(runtime_id)
                .await?;
            mark_fresh();
            return Ok(());
        }
        // Durable strictly AHEAD of committed: the reseed direction.
        if lifecycle_terminal {
            // Terminal lifecycle facts outrank content recovery: never
            // re-seed runtime authority for a session the lifecycle domain
            // has closed.
            mark_fresh();
            return Ok(());
        }
        meerkat_core::session_store::run_boundary_snapshot_save_guard(
            &durable,
            Some(committed.session()),
        )
        .map_err(|e| {
            meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                "durable session row for runtime {runtime_id} orders ahead of the committed \
                 runtime authority but does not extend it; refusing to adopt either side: {e}"
            ))
        })?;
        let bytes = durable.to_persisted_bytes().map_err(|e| {
            meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                "durable session encode for runtime-authority freshen: {e}"
            ))
        })?;
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .commit_session_snapshot(
                runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(bytes),
                },
            )
            .await;
        self.note_session_scoped_write(runtime_id);
        result?;
        tracing::warn!(
            runtime_id = %runtime_id,
            session_id = %session_id,
            stale_rewrite_generation = committed_order.0,
            stale_message_count = committed_order.1,
            durable_rewrite_generation = durable_order.0,
            durable_message_count = durable_order.1,
            "stale committed runtime authority re-seeded from the durable session row \
             (runtime store rollback/restore detected)"
        );
        mark_fresh();
        Ok(())
    }

    /// Bump the write epoch. Called BEFORE the inner write (a failed write
    /// may have partially applied — over-invalidate) and AFTER it completes
    /// (a read that overlapped the write may have admitted pre-write bytes
    /// under the during-write epoch; the post-write bump makes the current
    /// epoch strictly greater, so the next lookup misses and re-reads).
    fn note_session_scoped_write(&self, runtime_id: &meerkat_runtime::LogicalRuntimeId) {
        if let Some(epochs) = self.write_epochs.as_ref() {
            epochs.advance(runtime_id);
        }
    }

    /// The single-flight mint fence for ONE runtime. Concurrent racers on
    /// the same runtime share one mutex; dead entries are swept on insert so
    /// the map stays bounded by the number of runtimes currently minting.
    fn mint_flight_for(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Arc<tokio::sync::Mutex<()>> {
        let mut flights = self
            .mint_flights
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = flights
            .get(runtime_id.0.as_str())
            .and_then(std::sync::Weak::upgrade)
        {
            return existing;
        }
        flights.retain(|_, flight| flight.strong_count() > 0);
        let fresh = Arc::new(tokio::sync::Mutex::new(()));
        flights.insert(runtime_id.0.clone(), Arc::downgrade(&fresh));
        fresh
    }

    /// EXACT-PARENT PROJECTION SEAM (task #56, append-before-compact): bring
    /// the durable row to EXACTLY `commit.parent_revision` before replaying
    /// the rewrite commit. The chain walk accepts a durable predecessor that
    /// is a strict APPEND-PREFIX of the commit's parent (the committed turn
    /// appended messages before compacting), but the injected store's
    /// `save_transcript_rewrite` requires the previously persisted head to
    /// equal the commit's parent revision exactly - replaying directly would
    /// conflict at the store.
    ///
    /// `Session::with_validated_transcript_rewrite_parent_projection`
    /// (meerkat 0.8.15) mints the exact proof-carrying parent: the preceding
    /// graph prefix, the exact parent body and timestamps, first occurrence
    /// without an invented graph. Relative to the durable head that parent
    /// is a pure append extension, so the ordinary authoritative-projection
    /// door installs it and the rewrite replay then meets its exact parent.
    async fn project_durable_to_exact_rewrite_parent(
        &self,
        session_store: &Arc<dyn SessionStore>,
        successor: &meerkat_core::Session,
        sealed: &meerkat_core::ValidatedTranscriptHistory,
        commit: &meerkat_core::TranscriptRewriteCommit,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        let parent_session = successor
            .with_validated_transcript_rewrite_parent_projection(sealed, commit)
            .map_err(|e| {
                meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                    "exact rewrite-parent projection at generation {}: {e}",
                    commit.rewrite_generation
                ))
            })?;
        session_store
            .save_authoritative_projection(&parent_session)
            .await
            .map_err(|e| {
                meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                    "exact rewrite-parent save at generation {}: {e}",
                    commit.rewrite_generation
                ))
            })
    }

    /// The single-flight projection fence for ONE runtime (see the field
    /// docs on [`Self::projection_flights`]).
    fn projection_flight_for(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Arc<tokio::sync::Mutex<()>> {
        let mut flights = self
            .projection_flights
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = flights
            .get(runtime_id.0.as_str())
            .and_then(std::sync::Weak::upgrade)
        {
            return existing;
        }
        flights.retain(|_, flight| flight.strong_count() > 0);
        let fresh = Arc::new(tokio::sync::Mutex::new(()));
        flights.insert(runtime_id.0.clone(), Arc::downgrade(&fresh));
        fresh
    }

    /// Write-side complement of
    /// [`Self::mint_runtime_authority_from_durable`]: after a committing
    /// verb succeeds on the (ephemeral) inner store, project the
    /// store-issued committed WholeBlob session into the durable session
    /// store.
    ///
    /// At meerkat 0.8.11 the session service keeps no plain `SessionStore`
    /// write path of its own (`PersistentSessionService::new_with_capacities`
    /// retains only the store's incremental capability; WholeBlob authority
    /// lives only in `RuntimeStore`), and the `RuntimeStore` contract
    /// assigns backing-medium sync to the store implementation ("for stores
    /// that physically share a `SessionStore` table, writes that table in
    /// the same transaction" - `RuntimeStore::atomic_apply`). This facade's
    /// shared medium is the injected store, so it projects after the inner
    /// commit. The committed snapshot is re-read from the inner store so the
    /// projected document is store-issued bytes, never facade-interpreted
    /// input; a projection racing a newer commit converges toward the newer
    /// committed state.
    ///
    /// Fails closed: a projection failure fails the committing verb, so a
    /// reported commit never claims durability the injected store did not
    /// accept. The inner (scratch) store may then run ahead of durable
    /// truth; the next activation's mint re-seeds from the durable row, so
    /// an unacknowledged boundary is lost whole rather than resurrected
    /// torn.
    ///
    /// Supervisor-session scope (deliberate): EVERY runtime's committed
    /// boundary projects through here - the facade cannot (and does not try
    /// to) distinguish the mob supervisor's session from member sessions by
    /// runtime id. Durable admission is the INJECTED STORE's own
    /// discipline: the identity-first continuity adapter parks unregistered
    /// sessions (the supervisor is never identity-registered) in memory, so
    /// supervisor comms traffic does not accumulate durable rows on that
    /// shape; a plain injected store (MemoryStore/SQLite) persists every
    /// session exactly as pre-0.8.11 dual-write did, and cleanup of
    /// abandoned supervisor rows stays the injector's concern.
    async fn project_committed_session_to_durable(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        let Some(session_store) = self.session_store.as_ref() else {
            return Ok(());
        };
        // HeadCanonical media write the session store through its own
        // incremental capability; only the WholeBlob shape needs the
        // facade-owned projection (mirrors the mint's profile gate).
        if self.inner.session_persistence_profile()
            != meerkat_runtime::store::RuntimeSessionPersistenceProfile::WholeBlobV1
        {
            return Ok(());
        }
        // Single-flight per runtime: the rewrite-replay walk below installs
        // each missing commit against the durable head its predecessor
        // step just advanced; two interleaved walks for one runtime would
        // race those validations. The committed snapshot is re-read INSIDE
        // the fence so a projection racing a newer commit converges toward
        // the newer committed state.
        let flight = self.projection_flight_for(runtime_id);
        let _flight = flight.lock().await;
        let Some(snapshot) = self
            .inner
            .load_committed_whole_blob_snapshot(runtime_id)
            .await?
        else {
            // Receipt-only boundary before any committed snapshot exists:
            // nothing durable to project yet.
            return Ok(());
        };
        let successor = snapshot.session();
        // Parent-1 tear (task #56): `save_authoritative_projection` alone
        // cannot INSTALL a new rewrite generation on the durable row - a
        // retire committing a rewrite-advanced WholeBlob over an older
        // durable head left graph-ahead-of-head state that meerkat's
        // rewrite-save invariant then refused on every resume. When the
        // committed successor's PROVED graph extends the durable
        // predecessor, replay each missing rewrite commit through the
        // store's typed rewrite door first; every step is monotonic and
        // validated against the durable head the previous step installed,
        // so a partial failure re-converges on the exact retry. No branch
        // overwrites durable state the successor graph does not prove.
        let durable_predecessor = session_store.load(successor.id()).await.map_err(|e| {
            meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                "durable predecessor read before boundary projection: {e}"
            ))
        })?;
        if let Some(durable_predecessor) = durable_predecessor.as_ref() {
            let sealed = successor
                .validated_transcript_history_state()
                .map_err(|e| {
                    meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                        "committed WholeBlob transcript-history seal: {e}"
                    ))
                })?;
            if let Some(sealed) = sealed.as_ref()
                && sealed.commit_count() != 0
            {
                let missing_commits =
                    meerkat_core::session_store::find_transcript_rewrite_commit_chain_extending_session(
                        sealed,
                        durable_predecessor,
                        sealed.state().head(),
                    )
                    .map_err(|e| {
                        meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                            "rewrite chain walk against durable predecessor: {e}"
                        ))
                    })?;
                // `None` is NOT replayed: the successor graph does not prove
                // an extension of the durable row, so the injected store's
                // own save guard below stays the only authority over that
                // shape (commitless projections, adopted seeds) - never a
                // blind graph-advanced overwrite from this facade.
                if let Some(missing_commits) = missing_commits {
                    // An EMPTY chain means the durable row's CONTENT already
                    // sits at the sealed head (the walk judges by revision):
                    // nothing to replay, only the trailing envelope
                    // projection below. It must never be "corrected" from a
                    // generation read off the durable Session - slim
                    // head-canonical materializations keep retained history
                    // out-of-line and always read generation 0, so a
                    // session-level generation is not a durable oracle here.
                    //
                    // The injected `save_transcript_rewrite` requires the
                    // durable head to equal each commit's parent revision
                    // EXACTLY, while the chain walk also accepts an
                    // append-prefix predecessor (messages appended in the
                    // committed turn before it compacted). Track the durable
                    // head across the walk and route any prefix gap through
                    // the exact-parent projection seam before replaying.
                    let mut durable_head_revision =
                        durable_predecessor.transcript_revision().map_err(|e| {
                            meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                                "durable predecessor revision before rewrite replay: {e}"
                            ))
                        })?;
                    for commit in missing_commits {
                        if durable_head_revision != commit.parent_revision {
                            self.project_durable_to_exact_rewrite_parent(
                                session_store,
                                successor,
                                sealed,
                                commit,
                            )
                            .await?;
                            durable_head_revision.clone_from(&commit.parent_revision);
                        }
                        let projected_history =
                            sealed.project_at_rewrite_commit(commit).map_err(|e| {
                                meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                                    "rewrite replay projection at generation {}: {e}",
                                    commit.rewrite_generation
                                ))
                            })?;
                        let prefix_session = successor
                            .with_validated_transcript_history_projection(projected_history)
                            .map_err(|e| {
                                meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                                    "rewrite replay prefix session at generation {}: {e}",
                                    commit.rewrite_generation
                                ))
                            })?;
                        session_store
                            .save_transcript_rewrite(&prefix_session, commit)
                            .await
                            .map_err(|e| {
                                meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                                    "rewrite replay save at generation {}: {e}",
                                    commit.rewrite_generation
                                ))
                            })?;
                        durable_head_revision = commit.revision.clone();
                    }
                }
            }
        }
        // Trailing appends past the latest audited head plus the envelope
        // (usage, metadata): the durable row is now at the successor's
        // rewrite generation, so this is the ordinary projection shape.
        session_store
            .save_authoritative_projection(successor)
            .await
            .map_err(|e| {
                meerkat_runtime::store::RuntimeStoreError::WriteFailed(format!(
                    "durable session projection after runtime boundary commit: {e}"
                ))
            })
    }
}

#[async_trait]
impl meerkat_runtime::RuntimeStore for SessionStoreBackedRuntimeStore {
    // The complete store-owned session-authority seam is carried by the
    // backend; this decorator forwards the one required accessor and keeps
    // its intentional per-operation overrides below (write-epoch bumps on
    // every session-scoped mutation) observable on the RuntimeStore surface.
    fn session_authority_ops(&self) -> &dyn meerkat_runtime::store::RuntimeSessionAuthorityOps {
        self.inner.session_authority_ops()
    }

    fn session_persistence_profile(
        &self,
    ) -> meerkat_runtime::store::RuntimeSessionPersistenceProfile {
        self.inner.session_persistence_profile()
    }

    fn session_boundary_authority_read_cost(
        &self,
    ) -> meerkat_runtime::store::RuntimeSessionAuthorityReadCost {
        self.inner.session_boundary_authority_read_cost()
    }

    fn auth_authority_key(&self) -> Option<String> {
        self.inner.auth_authority_key()
    }

    fn persist_auth_oauth_flow_snapshot(
        &self,
        snapshot_json: &[u8],
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.inner.persist_auth_oauth_flow_snapshot(snapshot_json)
    }

    fn load_auth_oauth_flow_snapshot(
        &self,
    ) -> Result<Option<Vec<u8>>, meerkat_runtime::store::RuntimeStoreError> {
        self.inner.load_auth_oauth_flow_snapshot()
    }

    fn update_auth_oauth_flow_snapshot(
        &self,
        update: &mut meerkat_runtime::store::AuthOAuthFlowSnapshotUpdate<'_>,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.inner.update_auth_oauth_flow_snapshot(update)
    }

    async fn commit_session_snapshot(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        session_delta: meerkat_runtime::store::SerializedSessionSnapshot,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .commit_session_snapshot(runtime_id, session_delta)
            .await;
        self.note_session_scoped_write(runtime_id);
        result?;
        self.project_committed_session_to_durable(runtime_id).await
    }

    async fn commit_prepared_session_boundary(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        request: meerkat_runtime::store::PreparedRuntimeSessionCommit,
    ) -> Result<
        meerkat_runtime::store::PreparedRuntimeSessionCommitResult,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .commit_prepared_session_boundary(runtime_id, request)
            .await;
        self.note_session_scoped_write(runtime_id);
        let result = result?;
        self.project_committed_session_to_durable(runtime_id)
            .await?;
        Ok(result)
    }

    async fn load_session_boundary_authority(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Option<meerkat_runtime::store::RuntimeSessionAuthority>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.freshen_stale_runtime_authority_from_durable(runtime_id)
            .await?;
        if let Some(authority) = self
            .inner
            .load_session_boundary_authority(runtime_id)
            .await?
        {
            return Ok(Some(authority));
        }
        if !self.mint_runtime_authority_from_durable(runtime_id).await? {
            return Ok(None);
        }
        self.inner.load_session_boundary_authority(runtime_id).await
    }

    async fn load_whole_blob_store_authority(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Option<meerkat_runtime::store::WholeBlobStoreAuthority>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.freshen_stale_runtime_authority_from_durable(runtime_id)
            .await?;
        if let Some(authority) = self
            .inner
            .load_whole_blob_store_authority(runtime_id)
            .await?
        {
            return Ok(Some(authority));
        }
        if !self.mint_runtime_authority_from_durable(runtime_id).await? {
            return Ok(None);
        }
        self.inner.load_whole_blob_store_authority(runtime_id).await
    }

    async fn load_committed_whole_blob_snapshot(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Option<meerkat_runtime::store::CommittedWholeBlobSnapshot>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.freshen_stale_runtime_authority_from_durable(runtime_id)
            .await?;
        if let Some(snapshot) = self
            .inner
            .load_committed_whole_blob_snapshot(runtime_id)
            .await?
        {
            return Ok(Some(snapshot));
        }
        if !self.mint_runtime_authority_from_durable(runtime_id).await? {
            return Ok(None);
        }
        self.inner
            .load_committed_whole_blob_snapshot(runtime_id)
            .await
    }

    async fn commit_prepared_whole_blob_snapshot_cas(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        prepared: meerkat_runtime::store::PreparedWholeBlobSnapshotCas,
    ) -> Result<
        meerkat_runtime::store::WholeBlobSnapshotCasOutcome,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .commit_prepared_whole_blob_snapshot_cas(runtime_id, prepared)
            .await;
        self.note_session_scoped_write(runtime_id);
        let outcome = result?;
        // A Conflict outcome committed nothing; only a committed successor
        // projects.
        if matches!(
            outcome,
            meerkat_runtime::store::WholeBlobSnapshotCasOutcome::Committed(_)
        ) {
            self.project_committed_session_to_durable(runtime_id)
                .await?;
        }
        Ok(outcome)
    }

    async fn commit_prepared_whole_blob_rewrite_boundary(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        boundary: meerkat_runtime::store::PreparedWholeBlobRewriteStoreParts,
    ) -> Result<
        meerkat_runtime::store::WholeBlobStoreAuthority,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .commit_prepared_whole_blob_rewrite_boundary(runtime_id, boundary)
            .await;
        self.note_session_scoped_write(runtime_id);
        let authority = result?;
        self.project_committed_session_to_durable(runtime_id)
            .await?;
        Ok(authority)
    }

    async fn delete_runtime_session_catalog_entry(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .delete_runtime_session_catalog_entry(runtime_id)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn load_runtime_session_catalog_entry(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Option<meerkat_runtime::store::RuntimeSessionCatalogEntry>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_runtime_session_catalog_entry(runtime_id)
            .await
    }

    async fn list_runtime_session_catalog_entries(
        &self,
        filter: meerkat_core::SessionFilter,
    ) -> Result<
        Vec<meerkat_runtime::store::RuntimeSessionCatalogEntry>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .list_runtime_session_catalog_entries(filter)
            .await
    }

    async fn write_prepared_whole_blob_provisional_tail(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        prepared: meerkat_runtime::store::PreparedWholeBlobProvisionalTail,
    ) -> Result<
        meerkat_runtime::store::WholeBlobProvisionalTailAuthority,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .write_prepared_whole_blob_provisional_tail(runtime_id, prepared)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn load_whole_blob_provisional_tail(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Option<meerkat_runtime::store::CommittedWholeBlobProvisionalTail>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_whole_blob_provisional_tail(runtime_id)
            .await
    }

    async fn discard_whole_blob_provisional_tail(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected: &meerkat_runtime::store::WholeBlobProvisionalTailAuthority,
    ) -> Result<bool, meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .discard_whole_blob_provisional_tail(runtime_id, expected)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn write_prepared_head_canonical_provisional_tail(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        prepared: meerkat_runtime::store::PreparedHeadCanonicalProvisionalTail,
    ) -> Result<
        meerkat_runtime::store::HeadCanonicalProvisionalTailAuthority,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .write_prepared_head_canonical_provisional_tail(runtime_id, prepared)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn load_head_canonical_provisional_tail(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Option<meerkat_runtime::store::HeadCanonicalProvisionalTailAuthority>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_head_canonical_provisional_tail(runtime_id)
            .await
    }

    async fn discard_head_canonical_provisional_tail(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected: &meerkat_runtime::store::HeadCanonicalProvisionalTailAuthority,
    ) -> Result<bool, meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .discard_head_canonical_provisional_tail(runtime_id, expected)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn load_durable_tail_recovery_source(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Option<meerkat_runtime::store::PreparedDurableTailRecoverySource>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_durable_tail_recovery_source(runtime_id)
            .await
    }

    async fn load_durable_tail_recovery_receipts(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        run_id: &meerkat_core::lifecycle::RunId,
    ) -> Result<
        Vec<meerkat_runtime::store::PreparedRecoveryReceiptSource>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_durable_tail_recovery_receipts(runtime_id, run_id)
            .await
    }

    async fn load_committed_recovery_boundary(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        candidate_id: &str,
    ) -> Result<
        Option<meerkat_runtime::store::CommittedRecoveryBoundary>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_committed_recovery_boundary(runtime_id, candidate_id)
            .await
    }

    async fn load_runtime_delivery_authority(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Option<meerkat_runtime::store::RuntimeDeliveryAuthorityRecord>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner.load_runtime_delivery_authority(runtime_id).await
    }

    async fn load_runtime_delivery_record(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        delivery_id: &str,
    ) -> Result<
        Option<meerkat_runtime::store::RuntimeDeliveryStoreRecord>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_runtime_delivery_record(runtime_id, delivery_id)
            .await
    }

    async fn compare_and_swap_runtime_delivery_authority(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected_revision: Option<u64>,
        replacement: meerkat_runtime::store::RuntimeDeliveryAuthorityRecord,
        inserted_delivery: Option<meerkat_runtime::store::RuntimeDeliveryStoreRecord>,
    ) -> Result<
        meerkat_runtime::store::RuntimeDeliveryAuthorityCasOutcome,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .compare_and_swap_runtime_delivery_authority(
                runtime_id,
                expected_revision,
                replacement,
                inserted_delivery,
            )
            .await
    }

    async fn list_runtime_delivery_records(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        after_sequence: u64,
        limit: usize,
    ) -> Result<
        Vec<meerkat_runtime::store::RuntimeDeliveryStoreRecord>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .list_runtime_delivery_records(runtime_id, after_sequence, limit)
            .await
    }

    async fn atomic_apply(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        session_delta: Option<meerkat_runtime::store::SerializedSessionSnapshot>,
        receipt: meerkat_core::lifecycle::RunBoundaryReceipt,
        input_updates: Vec<InputStatePersistenceRecord>,
        session_store_key: Option<meerkat_core::types::SessionId>,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        let committed_snapshot = session_delta.is_some();
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .atomic_apply(
                runtime_id,
                session_delta,
                receipt,
                input_updates,
                session_store_key,
            )
            .await;
        self.note_session_scoped_write(runtime_id);
        result?;
        if committed_snapshot {
            self.project_committed_session_to_durable(runtime_id)
                .await?;
        }
        Ok(())
    }

    async fn atomic_apply_with_machine_lifecycle(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        session_delta: meerkat_runtime::store::SerializedSessionSnapshot,
        receipt: meerkat_core::lifecycle::RunBoundaryReceipt,
        machine_lifecycle: MachineLifecycleCommit,
        input_updates: Vec<InputStatePersistenceRecord>,
        session_store_key: meerkat_core::types::SessionId,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .atomic_apply_with_machine_lifecycle(
                runtime_id,
                session_delta,
                receipt,
                machine_lifecycle,
                input_updates,
                session_store_key,
            )
            .await;
        self.note_session_scoped_write(runtime_id);
        result?;
        self.project_committed_session_to_durable(runtime_id).await
    }

    async fn load_input_states(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<Vec<meerkat_runtime::InputStateRow>, meerkat_runtime::store::RuntimeStoreError>
    {
        // Forwards the per-row shape verbatim. meerkat 0.8.8 widened this to
        // yield Decoded/Corrupt witnesses per row so one undecodable row can
        // no longer poison an entire runtime's input-state load; the facade
        // must not collapse that back to a whole-call failure.
        self.inner.load_input_states(runtime_id).await
    }

    async fn load_input_states_strict(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Vec<meerkat_runtime::input_state::StoredInputState>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner.load_input_states_strict(runtime_id).await
    }

    async fn load_input_states_with_versions(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        meerkat_runtime::store::PreparedRecoveryInputSnapshot,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner.load_input_states_with_versions(runtime_id).await
    }

    async fn load_input_state_by_idempotency_key(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        key: &meerkat_runtime::IdempotencyKey,
    ) -> Result<
        Option<meerkat_runtime::store::ExactInputStateObservation>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_input_state_by_idempotency_key(runtime_id, key)
            .await
    }

    async fn load_input_states_by_ids(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        input_ids: &[meerkat_core::lifecycle::InputId],
    ) -> Result<
        Vec<Option<meerkat_runtime::input_state::StoredInputState>>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_input_states_by_ids(runtime_id, input_ids)
            .await
    }

    async fn load_pending_terminal_owner_ids_page(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        after: Option<&meerkat_core::lifecycle::InputId>,
        limit: usize,
    ) -> Result<Vec<meerkat_core::lifecycle::InputId>, meerkat_runtime::store::RuntimeStoreError>
    {
        self.inner
            .load_pending_terminal_owner_ids_page(runtime_id, after, limit)
            .await
    }

    fn input_state_batch_cas_implementation_profile(
        &self,
    ) -> meerkat_runtime::store::InputStateBatchCasImplementationProfile {
        self.inner.input_state_batch_cas_implementation_profile()
    }

    async fn compare_and_swap_recovery_input_states_atomically(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected_revision: meerkat_runtime::store::RecoveryInputSetRevision,
        mutations: &[meerkat_runtime::store::RecoveryInputStateMutation],
    ) -> Result<
        meerkat_runtime::store::InputStateBatchCasOutcome,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .compare_and_swap_recovery_input_states_atomically(
                runtime_id,
                expected_revision,
                mutations,
            )
            .await
    }

    async fn compare_and_swap_recovery_input_states_atomically_with_fence(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected_revision: meerkat_runtime::store::RecoveryInputSetRevision,
        mutations: &[meerkat_runtime::store::RecoveryInputStateMutation],
        write_fence: Arc<dyn meerkat_runtime::store::RuntimeStoreWriteFence>,
    ) -> Result<
        meerkat_runtime::store::FencedInputStateBatchCasOutcome,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .compare_and_swap_recovery_input_states_atomically_with_fence(
                runtime_id,
                expected_revision,
                mutations,
                write_fence,
            )
            .await
    }

    async fn load_committed_boundary_receipts(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        run_id: &meerkat_core::lifecycle::RunId,
    ) -> Result<
        Vec<meerkat_core::lifecycle::RunBoundaryReceipt>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_committed_boundary_receipts(runtime_id, run_id)
            .await
    }

    async fn load_boundary_receipt(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        run_id: &meerkat_core::lifecycle::RunId,
        sequence: u64,
    ) -> Result<
        Option<meerkat_core::lifecycle::RunBoundaryReceipt>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_boundary_receipt(runtime_id, run_id, sequence)
            .await
    }

    async fn load_session_snapshot(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<Option<Arc<Vec<u8>>>, meerkat_runtime::store::RuntimeStoreError> {
        self.inner.load_session_snapshot(runtime_id).await
    }

    async fn clear_session_snapshot(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self.inner.clear_session_snapshot(runtime_id).await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn replace_session_snapshot_if_current(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected_current: &[u8],
        replacement: Vec<u8>,
    ) -> Result<bool, meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .replace_session_snapshot_if_current(runtime_id, expected_current, replacement)
            .await;
        self.note_session_scoped_write(runtime_id);
        let replaced = result?;
        if replaced {
            self.project_committed_session_to_durable(runtime_id)
                .await?;
        }
        Ok(replaced)
    }

    async fn clear_session_snapshot_if_current(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected_current: &[u8],
    ) -> Result<bool, meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .clear_session_snapshot_if_current(runtime_id, expected_current)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn persist_input_state(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        state: &InputStatePersistenceRecord,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self.inner.persist_input_state(runtime_id, state).await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn persist_input_states_atomically(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        states: &[InputStatePersistenceRecord],
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .persist_input_states_atomically(runtime_id, states)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn compare_and_swap_input_states_atomically(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected: &[StoredInputState],
        replacements: &[InputStatePersistenceRecord],
    ) -> Result<
        meerkat_runtime::store::InputStateBatchCasOutcome,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .compare_and_swap_input_states_atomically(runtime_id, expected, replacements)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn compare_and_swap_input_states_atomically_with_fence(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected: &[StoredInputState],
        replacements: &[InputStatePersistenceRecord],
        write_fence: Arc<dyn meerkat_runtime::store::RuntimeStoreWriteFence>,
    ) -> Result<
        meerkat_runtime::store::FencedInputStateBatchCasOutcome,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .compare_and_swap_input_states_atomically_with_fence(
                runtime_id,
                expected,
                replacements,
                write_fence,
            )
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn load_input_state(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        input_id: &meerkat_core::lifecycle::InputId,
    ) -> Result<Option<StoredInputState>, meerkat_runtime::store::RuntimeStoreError> {
        self.inner.load_input_state(runtime_id, input_id).await
    }

    async fn observe_machine_lifecycle(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        meerkat_runtime::store::MachineLifecycleObservation,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner.observe_machine_lifecycle(runtime_id).await
    }

    async fn compare_and_swap_machine_lifecycle(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected: meerkat_runtime::store::MachineLifecycleExpectedVersion,
        replacement: MachineLifecycleCommit,
    ) -> Result<
        meerkat_runtime::store::MachineLifecycleCasOutcome,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .compare_and_swap_machine_lifecycle(runtime_id, expected, replacement)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn compare_and_swap_machine_lifecycle_with_fence(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        expected: meerkat_runtime::store::MachineLifecycleExpectedVersion,
        replacement: MachineLifecycleCommit,
        write_fence: Arc<dyn meerkat_runtime::store::RuntimeStoreWriteFence>,
    ) -> Result<
        meerkat_runtime::store::FencedMachineLifecycleCasOutcome,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .compare_and_swap_machine_lifecycle_with_fence(
                runtime_id,
                expected,
                replacement,
                write_fence,
            )
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn load_machine_lifecycle_record(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<Option<Vec<u8>>, meerkat_runtime::store::RuntimeStoreError> {
        self.inner.load_machine_lifecycle_record(runtime_id).await
    }

    async fn commit_machine_lifecycle(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        commit: MachineLifecycleCommit,
        input_states: &[InputStatePersistenceRecord],
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .commit_machine_lifecycle(runtime_id, commit, input_states)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn commit_unregister_finalization(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        finalization: meerkat_runtime::store::UnregisterFinalizationCommit,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .commit_unregister_finalization(runtime_id, finalization)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    // Defaulted trait methods MUST be delegated too: the defaults answer
    // "unsupported"/"not quarantined", which would mask the inner store's real
    // capabilities through this facade (0.7.29 compaction outbox fails session
    // create closed; a masked quarantine flag would un-quarantine projections).
    fn supports_compaction_projection_outbox(&self) -> bool {
        self.inner.supports_compaction_projection_outbox()
    }

    async fn load_pending_compaction_projections(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Vec<meerkat_core::CompactionProjectionIntent>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner
            .load_pending_compaction_projections(runtime_id)
            .await
    }

    async fn mark_compaction_projection_finalized(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        projection: &meerkat_core::CompactionProjectionId,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .mark_compaction_projection_finalized(runtime_id, projection)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn is_runtime_projection_quarantined(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<bool, meerkat_runtime::store::RuntimeStoreError> {
        self.inner
            .is_runtime_projection_quarantined(runtime_id)
            .await
    }

    async fn delete_ops_lifecycle(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self.inner.delete_ops_lifecycle(runtime_id).await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn initialize_ops_lifecycle_if_absent(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        candidate: &meerkat_runtime::ops_lifecycle::PersistedOpsSnapshot,
    ) -> Result<
        meerkat_runtime::ops_lifecycle::PersistedOpsSnapshot,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.note_session_scoped_write(runtime_id);
        let result = self
            .inner
            .initialize_ops_lifecycle_if_absent(runtime_id, candidate)
            .await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn persist_ops_lifecycle(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
        snapshot: &meerkat_runtime::ops_lifecycle::PersistedOpsSnapshot,
    ) -> Result<(), meerkat_runtime::store::RuntimeStoreError> {
        self.note_session_scoped_write(runtime_id);
        let result = self.inner.persist_ops_lifecycle(runtime_id, snapshot).await;
        self.note_session_scoped_write(runtime_id);
        result
    }

    async fn load_ops_lifecycle(
        &self,
        runtime_id: &meerkat_runtime::LogicalRuntimeId,
    ) -> Result<
        Option<meerkat_runtime::ops_lifecycle::PersistedOpsSnapshot>,
        meerkat_runtime::store::RuntimeStoreError,
    > {
        self.inner.load_ops_lifecycle(runtime_id).await
    }

    async fn load_mob_host_binding(
        &self,
        mob_id: &str,
    ) -> Result<Option<Vec<u8>>, meerkat_runtime::store::RuntimeStoreError> {
        self.inner.load_mob_host_binding(mob_id).await
    }

    async fn list_mob_host_bindings(
        &self,
    ) -> Result<Vec<(String, Vec<u8>)>, meerkat_runtime::store::RuntimeStoreError> {
        self.inner.list_mob_host_bindings().await
    }

    async fn put_mob_host_binding_if_absent(
        &self,
        mob_id: &str,
        record_json: &[u8],
    ) -> Result<bool, meerkat_runtime::store::RuntimeStoreError> {
        self.inner
            .put_mob_host_binding_if_absent(mob_id, record_json)
            .await
    }

    async fn compare_and_put_mob_host_binding(
        &self,
        mob_id: &str,
        expected_json: &[u8],
        next_json: &[u8],
    ) -> Result<bool, meerkat_runtime::store::RuntimeStoreError> {
        self.inner
            .compare_and_put_mob_host_binding(mob_id, expected_json, next_json)
            .await
    }

    async fn delete_mob_host_binding(
        &self,
        mob_id: &str,
        expected_json: &[u8],
    ) -> Result<bool, meerkat_runtime::store::RuntimeStoreError> {
        self.inner
            .delete_mob_host_binding(mob_id, expected_json)
            .await
    }

    async fn load_mob_host_revocation(
        &self,
        mob_id: &str,
    ) -> Result<Option<Vec<u8>>, meerkat_runtime::store::RuntimeStoreError> {
        self.inner.load_mob_host_revocation(mob_id).await
    }

    async fn list_mob_host_revocations(
        &self,
    ) -> Result<Vec<(String, Vec<u8>)>, meerkat_runtime::store::RuntimeStoreError> {
        self.inner.list_mob_host_revocations().await
    }

    async fn revoke_mob_host_binding(
        &self,
        mob_id: &str,
        expected_binding_json: &[u8],
        receipt_json: &[u8],
    ) -> Result<bool, meerkat_runtime::store::RuntimeStoreError> {
        self.inner
            .revoke_mob_host_binding(mob_id, expected_binding_json, receipt_json)
            .await
    }
}

#[cfg(test)]
static RUNTIME_TURN_TRACES: OnceLock<Mutex<Vec<RuntimeTurnTrace>>> = OnceLock::new();

#[cfg(test)]
fn runtime_turn_traces() -> &'static Mutex<Vec<RuntimeTurnTrace>> {
    RUNTIME_TURN_TRACES.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
fn record_runtime_turn_trace(trace: RuntimeTurnTrace) {
    runtime_turn_traces()
        .lock()
        .expect("runtime turn traces mutex")
        .push(trace);
}

#[cfg(test)]
#[allow(dead_code)]
#[allow(clippy::expect_used)]
pub(crate) fn take_runtime_turn_traces() -> Vec<RuntimeTurnTrace> {
    std::mem::take(
        &mut *runtime_turn_traces()
            .lock()
            .expect("runtime turn traces mutex"),
    )
}

#[cfg(not(test))]
#[allow(dead_code)]
fn record_runtime_turn_trace(_trace: ()) {}

fn runtime_turn_diagnostics_enabled() -> bool {
    std::env::var("MOBKIT_TRACE_RUNTIME_TURNS").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Whether the session factory should wire the image-generation substrate for
/// this definition. Meerkat owns the per-profile visibility decision via
/// `profile.tools.image_generation`; MobKit only needs to make the runtime
/// machine available when a profile opts in, or when a realm profile may resolve
/// to an opt-in profile at spawn time.
pub fn mob_definition_may_use_image_generation(definition: &MobDefinition) -> bool {
    definition.profiles.values().any(|binding| {
        binding
            .as_inline()
            .is_none_or(|profile| profile.tools.image_generation)
    })
}

/// Whether the session factory should make the native shell dispatcher
/// available for profiles that opt into `profile.tools.shell`.
pub fn mob_definition_may_use_shell(definition: &MobDefinition) -> bool {
    definition.profiles.values().any(|binding| {
        binding
            .as_inline()
            .is_none_or(|profile| profile.tools.shell)
    })
}

fn normalize_runtime_turn_request(
    mut req: meerkat_core::service::StartTurnRequest,
) -> meerkat_core::service::StartTurnRequest {
    // Queue/Steer and render metadata are runtime-owned semantics. By the
    // time apply_runtime_turn() invokes the session service, the runtime
    // has already chosen the boundary and recorded the metadata it needs.
    // The direct agent/session path is queue-only, so forward a normalized
    // turn request to avoid re-injecting runtime-only semantics.
    req.runtime.handling_mode = meerkat_core::types::HandlingMode::Queue;
    // Meerkat 0.7: render metadata lives only on the typed turn-metadata
    // carrier; strip it there instead of the removed flat field.
    if let Some(metadata) = req.runtime.turn_metadata.as_mut() {
        metadata.render_metadata = None;
    }
    req
}

/// Apply a machine-authorized cooperative cancel as an idempotent quiescence
/// operation. Once the generated runtime machine has admitted the cancel,
/// absence of a live session (or absence of a running turn) proves that there
/// is no lower-plane work left to interrupt. Propagating either observation
/// back as a failed effect wedges the machine in its pre-cancel phase and can
/// make whole-mob shutdown retain otherwise releasable identity authority.
async fn cancel_after_boundary_with_machine_authority_if_live(
    service: &dyn MobSessionService,
    session_id: &meerkat_core::types::SessionId,
    expected_run_id: &meerkat_core::lifecycle::RunId,
    authority: meerkat_runtime::MachineSessionControlAuthority,
) -> Result<(), SessionError> {
    service
        .cancel_after_boundary_with_machine_authority(session_id, expected_run_id, authority)
        .await
        .or_else(|error| match error {
            SessionError::NotFound { .. } | SessionError::NotRunning { .. } => Ok(()),
            error => Err(error),
        })
}

/// Implement all `MobSessionService` super-traits by delegating to `self.inner`,
/// overriding only `create_session` to apply the pre-build hook.
macro_rules! delegate_mob_session_service {
    ($wrapper:ty) => {
        #[async_trait]
        impl meerkat_core::service::SessionService for $wrapper {
            async fn create_session(
                &self,
                req: CreateSessionRequest,
            ) -> Result<meerkat_core::types::RunResult, SessionError> {
                let (req, context) = self.prepare_create_request(req).await?;
                let result = self.inner.create_session(req).await?;
                Ok(self.complete_create(result, context).await)
            }
            async fn start_turn(
                &self,
                id: &meerkat_core::types::SessionId,
                req: meerkat_core::service::StartTurnRequest,
            ) -> Result<meerkat_core::types::RunResult, SessionError> {
                self.inner.start_turn(id, req).await
            }
            async fn reconcile_runtime_compaction_projections(
                &self,
                id: &meerkat_core::types::SessionId,
                intents: Vec<meerkat_core::CompactionProjectionIntent>,
            ) -> Result<(), SessionError> {
                self.inner
                    .reconcile_runtime_compaction_projections(id, intents)
                    .await
            }
            async fn abort_uncommitted_compaction_projections(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner
                    .abort_uncommitted_compaction_projections(id)
                    .await
            }
            async fn abort_rejected_runtime_run_projections(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner.abort_rejected_runtime_run_projections(id).await
            }
            async fn interrupt(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner.interrupt(id).await
            }
            async fn cancel_after_boundary(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner.cancel_after_boundary(id).await
            }
            async fn cancel_after_boundary_for_run(
                &self,
                id: &meerkat_core::types::SessionId,
                expected_run_id: &meerkat_core::lifecycle::RunId,
            ) -> Result<(), SessionError> {
                self.inner
                    .cancel_after_boundary_for_run(id, expected_run_id)
                    .await
            }
            async fn set_session_client(
                &self,
                id: &meerkat_core::types::SessionId,
                client: Arc<dyn meerkat_core::AgentLlmClient>,
            ) -> Result<(), SessionError> {
                self.inner
                    .set_session_client(id, ReplaySanitizingAgentLlmClient::wrap(client))
                    .await
            }
            async fn hot_swap_session_llm_identity(
                &self,
                id: &meerkat_core::types::SessionId,
                client: Arc<dyn meerkat_core::AgentLlmClient>,
                identity: meerkat_core::session::SessionLlmIdentity,
                request_policy: meerkat_core::SessionLlmRequestPolicy,
            ) -> Result<(), SessionError> {
                self.inner
                    .hot_swap_session_llm_identity(
                        id,
                        ReplaySanitizingAgentLlmClient::wrap(client),
                        identity,
                        request_policy,
                    )
                    .await
            }
            async fn update_session_mob_authority_context(
                &self,
                id: &meerkat_core::types::SessionId,
                authority_context: Option<meerkat_core::service::MobToolAuthorityContext>,
            ) -> Result<(), SessionError> {
                self.inner
                    .update_session_mob_authority_context(id, authority_context)
                    .await
            }
            async fn has_live_session(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<bool, SessionError> {
                self.inner.has_live_session(id).await
            }
            async fn set_session_tool_visibility_state(
                &self,
                id: &meerkat_core::types::SessionId,
                state: Option<meerkat_core::SessionToolVisibilityState>,
            ) -> Result<(), SessionError> {
                self.inner
                    .set_session_tool_visibility_state(id, state)
                    .await
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
            async fn archive(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner.archive(id).await
            }
            async fn subscribe_session_events(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<meerkat_core::comms::EventStream, meerkat_core::comms::StreamError> {
                meerkat_core::service::SessionService::subscribe_session_events(
                    self.inner.as_ref(),
                    id,
                )
                .await
            }
            async fn record_live_terminal_error(
                &self,
                id: &meerkat_core::types::SessionId,
                cause: meerkat_core::live_adapter::LiveAdapterErrorCode,
            ) -> Result<(), SessionError> {
                self.inner.record_live_terminal_error(id, cause).await
            }
            async fn record_live_output_audio_degraded(
                &self,
                id: &meerkat_core::types::SessionId,
                dropped: u64,
            ) -> Result<(), SessionError> {
                self.inner
                    .record_live_output_audio_degraded(id, dropped)
                    .await
            }
        }

        #[async_trait]
        impl meerkat_core::service::SessionServiceCommsExt for $wrapper {
            async fn comms_runtime(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Option<Arc<dyn meerkat_core::agent::CommsRuntime>> {
                self.inner.comms_runtime(id).await
            }

            async fn event_injector(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Option<Arc<dyn meerkat_core::EventInjector>> {
                self.inner.event_injector(id).await
            }

            async fn interaction_event_injector(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Option<Arc<dyn meerkat_core::event_injector::SubscribableInjector>> {
                self.inner.interaction_event_injector(id).await
            }
        }

        #[async_trait]
        impl meerkat_core::service::SessionServiceControlExt for $wrapper {
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
            async fn stage_tool_results(
                &self,
                id: &meerkat_core::types::SessionId,
                req: meerkat_core::service::StageToolResultsRequest,
            ) -> Result<meerkat_core::service::StageToolResultsResult, SessionError> {
                self.inner.stage_tool_results(id, req).await
            }
        }

        #[async_trait]
        impl meerkat_core::service::SessionServiceHistoryExt for $wrapper {
            async fn read_history(
                &self,
                id: &meerkat_core::types::SessionId,
                query: meerkat_core::service::SessionHistoryQuery,
            ) -> Result<meerkat_core::service::SessionHistoryPage, SessionError> {
                self.inner.read_history(id, query).await
            }
            async fn read_transcript_revision(
                &self,
                id: &meerkat_core::types::SessionId,
                query: meerkat_core::service::SessionTranscriptRevisionQuery,
            ) -> Result<meerkat_core::service::SessionTranscriptRevisionPage, SessionError> {
                self.inner.read_transcript_revision(id, query).await
            }
            async fn list_transcript_revisions(
                &self,
                id: &meerkat_core::types::SessionId,
                query: meerkat_core::service::SessionTranscriptRevisionListQuery,
            ) -> Result<meerkat_core::service::SessionTranscriptRevisionList, SessionError> {
                self.inner.list_transcript_revisions(id, query).await
            }
        }

        #[async_trait]
        impl MobSessionService for $wrapper {
            async fn load_session_for_resume(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<meerkat_mob::ResumeSessionLoad, SessionError> {
                let load = self.inner.load_session_for_resume(session_id).await?;
                self.overlay_runtime_archived_terminal(load).await
            }
            async fn prepare_session_for_resume(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner.prepare_session_for_resume(session_id).await
            }
            async fn materialize_session_for_resume(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<meerkat_mob::ResumeSessionLoad, SessionError> {
                self.inner.materialize_session_for_resume(session_id).await
            }

            async fn create_session_under_runtime_turn_boundary(
                &self,
                req: meerkat_core::service::CreateSessionRequest,
            ) -> Result<meerkat_core::RunResult, SessionError> {
                let (req, context) = self.prepare_create_request(req).await?;
                let result = self
                    .inner
                    .create_session_under_runtime_turn_boundary(req)
                    .await?;
                Ok(self.complete_create(result, context).await)
            }
            async fn create_session_with_actor_witness_under_runtime_turn_boundary(
                &self,
                req: meerkat_core::service::CreateSessionRequest,
                actor_witness_slot: &meerkat_session::LiveSessionActorWitnessSlot,
            ) -> Result<meerkat_core::RunResult, SessionError> {
                let (req, context) = self.prepare_create_request(req).await?;
                let result = self
                    .inner
                    .create_session_with_actor_witness_under_runtime_turn_boundary(
                        req,
                        actor_witness_slot,
                    )
                    .await?;
                Ok(self.complete_create(result, context).await)
            }
            async fn create_session_with_machine_archived_resume_authority_under_runtime_turn_boundary(
                &self,
                req: meerkat_core::service::CreateSessionRequest,
                authorization: meerkat_runtime::ArchivedSessionActorMaterializationAuthorization,
            ) -> Result<meerkat_core::RunResult, SessionError> {
                let (req, context) = self.prepare_create_request(req).await?;
                let result = self
                    .inner
                    .create_session_with_machine_archived_resume_authority_under_runtime_turn_boundary(
                        req,
                        authorization,
                    )
                    .await?;
                Ok(self.complete_create(result, context).await)
            }
            async fn create_session_with_machine_archived_resume_authority_and_actor_witness_under_runtime_turn_boundary(
                &self,
                req: meerkat_core::service::CreateSessionRequest,
                authorization: meerkat_runtime::ArchivedSessionActorMaterializationAuthorization,
                actor_witness_slot: &meerkat_session::LiveSessionActorWitnessSlot,
            ) -> Result<meerkat_core::RunResult, SessionError> {
                let (req, context) = self.prepare_create_request(req).await?;
                let result = self
                    .inner
                    .create_session_with_machine_archived_resume_authority_and_actor_witness_under_runtime_turn_boundary(
                        req,
                        authorization,
                        actor_witness_slot,
                    )
                    .await?;
                Ok(self.complete_create(result, context).await)
            }
            fn supports_persistent_sessions(&self) -> bool {
                self.inner.supports_persistent_sessions()
            }
            fn supports_runtime_turn_apply(&self) -> bool {
                self.inner.supports_runtime_turn_apply()
            }
            // meerkat 0.7.19: disposal routes on this fact. The trait default
            // is fail-closed (`true`); NOT forwarding it would swallow the
            // inner persistent service's real store read and resurrect the
            // ask-20 stranding for host-owned sessions (the external_tools
            // clobber class: wrappers MUST forward, not default).
            async fn session_known_to_archive_authority(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<bool, SessionError> {
                self.inner.session_known_to_archive_authority(session_id).await
            }
            fn runtime_adapter(&self) -> Option<Arc<meerkat_runtime::MeerkatMachine>> {
                self.runtime_adapter_override
                    .clone()
                    .or_else(|| self.inner.runtime_adapter())
            }
            async fn interrupt_with_machine_authority(
                &self,
                session_id: &meerkat_core::types::SessionId,
                authority: meerkat_runtime::MachineSessionControlAuthority,
            ) -> Result<(), SessionError> {
                self.inner
                    .interrupt_with_machine_authority(session_id, authority)
                    .await
            }
            async fn cancel_after_boundary_with_machine_authority(
                &self,
                session_id: &meerkat_core::types::SessionId,
                expected_run_id: &meerkat_core::lifecycle::RunId,
                authority: meerkat_runtime::MachineSessionControlAuthority,
            ) -> Result<(), SessionError> {
                cancel_after_boundary_with_machine_authority_if_live(
                    self.inner.as_ref(),
                    session_id,
                    expected_run_id,
                    authority,
                )
                .await
            }
            async fn cancel_current_after_boundary_with_machine_authority(
                &self,
                session_id: &meerkat_core::types::SessionId,
                authority: meerkat_runtime::MachineSessionControlAuthority,
            ) -> Result<(), SessionError> {
                self.inner
                    .cancel_current_after_boundary_with_machine_authority(session_id, authority)
                    .await
            }
            async fn live_session_actor_registered(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<bool, SessionError> {
                self.inner.live_session_actor_registered(session_id).await
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
                // Inherent method so absorber-equipped wrappers can serve
                // unchanged documents without an inner read (idle-cadence
                // fix); wrappers without an absorber forward unchanged.
                self.load_persisted_session_absorbed(session_id).await
            }
            // meerkat 0.7.29 revival seam (ask 31): the trait defaults answer
            // `Ok(None)`/`Unsupported`, which masks the inner persistent
            // service's real revival support and leaves Bug I victims
            // unresumable through this wrapper (same forwarding class as
            // `session_known_to_archive_authority` above).
            async fn load_revivable_retired_session(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<Option<meerkat_core::session::Session>, SessionError> {
                self.inner.load_revivable_retired_session(session_id).await
            }
            async fn load_persisted_session_metadata(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<Option<meerkat_core::PersistedSessionMetadataView>, SessionError> {
                self.inner.load_persisted_session_metadata(session_id).await
            }
            async fn authorize_revivable_retired_session(
                &self,
                session_id: &meerkat_core::types::SessionId,
                authority: meerkat_runtime::PreparedArchivedResumeCommitLease,
            ) -> Result<meerkat_runtime::AuthorizedArchivedResumeCommitLease, SessionError> {
                self.inner
                    .authorize_revivable_retired_session(session_id, authority)
                    .await
            }
            async fn create_session_with_machine_archived_resume_authority(
                &self,
                req: meerkat_core::service::CreateSessionRequest,
                authorization: meerkat_runtime::ArchivedSessionActorMaterializationAuthorization,
            ) -> Result<meerkat_core::RunResult, SessionError> {
                let (req, context) = self.prepare_create_request(req).await?;
                let result = self
                    .inner
                    .create_session_with_machine_archived_resume_authority(req, authorization)
                    .await?;
                Ok(self.complete_create(result, context).await)
            }
            async fn subscribe_session_events(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<meerkat_core::comms::EventStream, meerkat_core::comms::StreamError> {
                meerkat_mob::MobSessionService::subscribe_session_events(
                    self.inner.as_ref(),
                    session_id,
                )
                .await
            }
            async fn archive_with_mob_lifecycle_authority(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                match self
                    .inner
                    .archive_with_mob_lifecycle_authority(session_id)
                    .await
                {
                    Err(err)
                        if is_stopped_session_archive_retire_rejection(&err.to_string()) =>
                    {
                        // meerkat 0.7.1: the archive protocol commits the
                        // durable archive document FIRST, then drives the
                        // machine `Retire` realization — which the session
                        // machine of an idle (stopped-between-turns) member
                        // rejects from `Stopped`. meerkat-mob's own archive
                        // helper treats `Stopped` as already-retired; mirror
                        // that tolerance here so member retire/respawn
                        // disposal completes instead of wedging the roster
                        // anchor in `retiring` (any disposal retry on the
                        // retained anchor stalls the mob actor).
                        tracing::warn!(
                            session_id = %session_id,
                            error = %err,
                            "archive: tolerating Retire rejection for stopped idle session; archive document committed"
                        );
                        Ok(())
                    }
                    other => other,
                }
            }
            async fn archive_with_mob_lifecycle_authority_under_runtime_turn_boundary(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner
                    .archive_with_mob_lifecycle_authority_under_runtime_turn_boundary(session_id)
                    .await
            }
            async fn execution_snapshot(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<Option<meerkat_core::agent::AgentExecutionSnapshot>, SessionError> {
                self.inner.execution_snapshot(session_id).await
            }
            async fn tool_scope_snapshot(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<Option<meerkat_core::ToolScopeSnapshot>, SessionError> {
                self.inner.tool_scope_snapshot(session_id).await
            }
            async fn external_tool_surface_snapshot(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<Option<meerkat_core::ExternalToolSurfaceSnapshot>, SessionError> {
                self.inner.external_tool_surface_snapshot(session_id).await
            }
            async fn peer_ingress_runtime_snapshot(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<Option<meerkat_core::PeerIngressRuntimeSnapshot>, SessionError> {
                self.inner.peer_ingress_runtime_snapshot(session_id).await
            }
            async fn apply_runtime_turn(
                &self,
                session_id: &meerkat_core::types::SessionId,
                run_id: meerkat_core::lifecycle::RunId,
                req: meerkat_core::service::StartTurnRequest,
                boundary: meerkat_core::lifecycle::run_primitive::RunApplyBoundary,
                contributing_input_ids: Vec<meerkat_core::lifecycle::InputId>,
            ) -> Result<meerkat_core::lifecycle::core_executor::CoreApplyOutput, SessionError> {
                #[cfg(test)]
                let boundary_name = format!("{boundary:?}");
                #[cfg(test)]
                let contributing_count = contributing_input_ids.len();
                let run_id_for_log = run_id.to_string();
                if runtime_turn_diagnostics_enabled() {
                    tracing::warn!(
                        session_id = %session_id,
                        run_id = %run_id_for_log,
                        boundary = ?boundary,
                        contributing_inputs = contributing_input_ids.len(),
                        "mobkit runtime turn start"
                    );
                }
                let result = self
                    .inner
                    .apply_runtime_turn(
                        session_id,
                        run_id,
                        normalize_runtime_turn_request(req),
                        boundary,
                        contributing_input_ids,
                    )
                    .await;
                #[cfg(test)]
                record_runtime_turn_trace(RuntimeTurnTrace {
                    session_id: session_id.to_string(),
                    boundary: boundary_name,
                    contributing_input_count: contributing_count,
                    outcome: match &result {
                        Ok(_) => "ok".to_string(),
                        Err(error) => format!("err:{error}"),
                    },
                });
                if runtime_turn_diagnostics_enabled() {
                    match &result {
                        Ok(_) => tracing::warn!(
                            session_id = %session_id,
                            run_id = %run_id_for_log,
                            "mobkit runtime turn ok"
                        ),
                        Err(_) => tracing::error!(
                            session_id = %session_id,
                            run_id = %run_id_for_log,
                            "mobkit runtime turn error"
                        ),
                    }
                }
                result
            }
            async fn prepare_transient_turn_context_for_active_turn(
                &self,
                session_id: &meerkat_core::types::SessionId,
                expected_run_id: &meerkat_core::lifecycle::RunId,
                contexts: Vec<meerkat_core::lifecycle::run_primitive::TurnRequestContext>,
            ) -> Result<meerkat_core::CoreBoundaryStageOutput, meerkat_core::CoreBoundaryStageError>
            {
                self.inner
                    .prepare_transient_turn_context_for_active_turn(
                        session_id,
                        expected_run_id,
                        contexts,
                    )
                    .await
            }
            async fn acknowledge_committed_runtime_session_boundary_under_turn_finalization_boundary(
                &self,
                session_id: &meerkat_core::types::SessionId,
                authority: &meerkat_core::CommittedSessionBoundaryAuthority,
            ) -> Result<(), SessionError> {
                match self
                    .inner
                    .acknowledge_committed_runtime_session_boundary_under_turn_finalization_boundary(
                        session_id,
                        authority,
                    )
                    .await
                {
                    // The bounded-bridge shape (an ephemeral session service
                    // paired with a runtime machine via
                    // `runtime_adapter_override`): meerkat-mob's ephemeral
                    // impl refuses on principle - it cannot speak for a
                    // store it does not own - but on THIS composition the
                    // machine IS the store owner and the boundary is
                    // already committed when this acknowledgement runs; the
                    // ephemeral service holds no durable projection whose
                    // fencing it would advance. Refusing here fails every
                    // runtime-backed ephemeral turn AFTER its boundary
                    // committed, so the wrapper completes the
                    // acknowledgement instead. Scope is deliberately
                    // EXACT (lead-approved 2026-07-31): only this one typed
                    // refusal on only this shape - any other error, any
                    // other Unsupported, propagates untouched.
                    Err(SessionError::Unsupported(ref detail))
                        if detail
                            == "ephemeral session service cannot acknowledge store-owned \
                                runtime boundaries"
                            && self.absorbs_unsupported_boundary_acknowledgement() =>
                    {
                        Ok(())
                    }
                    other => other,
                }
            }
            async fn acquire_runtime_turn_finalization_guard(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<
                Box<dyn meerkat_core::lifecycle::CoreExecutorTurnFinalizationGuard>,
                SessionError,
            > {
                self.inner
                    .acquire_runtime_turn_finalization_guard(session_id)
                    .await
            }
            async fn checkpoint_committed_runtime_session_snapshot_under_turn_finalization_boundary(
                &self,
                session_id: &meerkat_core::types::SessionId,
                session_snapshot: Arc<Vec<u8>>,
            ) -> Result<(), SessionError> {
                self.inner
                    .checkpoint_committed_runtime_session_snapshot_under_turn_finalization_boundary(
                        session_id,
                        session_snapshot,
                    )
                    .await
            }
            async fn discard_live_session_after_runtime_stop_terminalized(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner
                    .discard_live_session_after_runtime_stop_terminalized(session_id)
                    .await
            }
            async fn discard_live_session_after_runtime_stop_terminalized_under_turn_finalization_boundary(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner
                    .discard_live_session_after_runtime_stop_terminalized_under_turn_finalization_boundary(
                        session_id,
                    )
                    .await
            }
            async fn publish_interaction_terminals(
                &self,
                session_id: &meerkat_core::types::SessionId,
                events: &[meerkat_core::event::AgentEvent],
            ) -> Result<
                Vec<
                    meerkat_core::lifecycle::core_executor::CoreInteractionTerminalPublicationReceipt,
                >,
                SessionError,
            > {
                self.inner
                    .publish_interaction_terminals(session_id, events)
                    .await
            }
            async fn discard_live_session(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner.discard_live_session(session_id).await
            }
            async fn discard_live_session_under_runtime_turn_boundary(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner
                    .discard_live_session_under_runtime_turn_boundary(session_id)
                    .await
            }
            async fn discard_live_session_actor_under_runtime_turn_boundary(
                &self,
                witness: &meerkat_session::LiveSessionActorWitness,
            ) -> Result<bool, SessionError> {
                self.inner
                    .discard_live_session_actor_under_runtime_turn_boundary(witness)
                    .await
            }
            async fn await_event_projection_drain(
                &self,
                session_id: &meerkat_core::types::SessionId,
            ) -> Result<bool, SessionError> {
                self.inner.await_event_projection_drain(session_id).await
            }
            async fn checkpoint_committed_runtime_session_snapshot(
                &self,
                session_id: &meerkat_core::types::SessionId,
                session_snapshot: Arc<Vec<u8>>,
            ) -> Result<(), SessionError> {
                self.inner
                    .checkpoint_committed_runtime_session_snapshot(session_id, session_snapshot)
                    .await
            }
            async fn cancel_all_checkpointers(&self) {
                self.inner.cancel_all_checkpointers().await;
            }
            async fn rearm_all_checkpointers(&self) {
                self.inner.rearm_all_checkpointers().await;
            }
        }
    };
}

delegate_mob_session_service!(PreBuildMobSessionService);

/// Wraps a `MobSessionService` to fire an `AfterCreateHook` after each
/// successful `create_session`. Unlike `PreBuildMobSessionService`, this
/// captures context **after** the inner service (including any pre-build hooks)
/// has finished, so the context reflects all mutations.
struct AfterCreateMobSessionService {
    inner: Arc<dyn MobSessionService>,
    after_hook: AfterCreateHook,
}

impl AfterCreateMobSessionService {
    fn prepare_create_request(
        &self,
        mut req: CreateSessionRequest,
    ) -> (CreateSessionRequest, SessionCreatedContext) {
        sanitize_create_session_request_llm_override(&mut req);
        ensure_shell_tooling_build_substrate(&mut req);
        let context = SessionCreatedContext {
            model: req.model.clone(),
            labels: req.labels.clone().unwrap_or_default(),
            system_prompt: req.system_prompt.as_set_prompt().map(ToString::to_string),
        };
        (req, context)
    }

    async fn complete_create(
        &self,
        result: meerkat_core::types::RunResult,
        context: SessionCreatedContext,
    ) -> meerkat_core::types::RunResult {
        (self.after_hook)(result.session_id.clone(), context).await;
        result
    }
}

#[async_trait]
impl meerkat_core::service::SessionService for AfterCreateMobSessionService {
    async fn create_session(
        &self,
        req: CreateSessionRequest,
    ) -> Result<meerkat_core::types::RunResult, SessionError> {
        let (req, context) = self.prepare_create_request(req);
        let result = self.inner.create_session(req).await?;
        Ok(self.complete_create(result, context).await)
    }
    async fn start_turn(
        &self,
        id: &meerkat_core::types::SessionId,
        req: meerkat_core::service::StartTurnRequest,
    ) -> Result<meerkat_core::types::RunResult, SessionError> {
        self.inner.start_turn(id, req).await
    }
    async fn reconcile_runtime_compaction_projections(
        &self,
        id: &meerkat_core::types::SessionId,
        intents: Vec<meerkat_core::CompactionProjectionIntent>,
    ) -> Result<(), SessionError> {
        self.inner
            .reconcile_runtime_compaction_projections(id, intents)
            .await
    }
    async fn abort_uncommitted_compaction_projections(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner
            .abort_uncommitted_compaction_projections(id)
            .await
    }
    async fn abort_rejected_runtime_run_projections(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner.abort_rejected_runtime_run_projections(id).await
    }
    async fn interrupt(&self, id: &meerkat_core::types::SessionId) -> Result<(), SessionError> {
        self.inner.interrupt(id).await
    }
    async fn cancel_after_boundary(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner.cancel_after_boundary(id).await
    }
    async fn cancel_after_boundary_for_run(
        &self,
        id: &meerkat_core::types::SessionId,
        expected_run_id: &meerkat_core::lifecycle::RunId,
    ) -> Result<(), SessionError> {
        self.inner
            .cancel_after_boundary_for_run(id, expected_run_id)
            .await
    }
    async fn set_session_client(
        &self,
        id: &meerkat_core::types::SessionId,
        client: Arc<dyn meerkat_core::AgentLlmClient>,
    ) -> Result<(), SessionError> {
        self.inner
            .set_session_client(id, ReplaySanitizingAgentLlmClient::wrap(client))
            .await
    }
    async fn hot_swap_session_llm_identity(
        &self,
        id: &meerkat_core::types::SessionId,
        client: Arc<dyn meerkat_core::AgentLlmClient>,
        identity: meerkat_core::session::SessionLlmIdentity,
        request_policy: meerkat_core::SessionLlmRequestPolicy,
    ) -> Result<(), SessionError> {
        self.inner
            .hot_swap_session_llm_identity(
                id,
                ReplaySanitizingAgentLlmClient::wrap(client),
                identity,
                request_policy,
            )
            .await
    }
    async fn update_session_mob_authority_context(
        &self,
        id: &meerkat_core::types::SessionId,
        authority_context: Option<meerkat_core::service::MobToolAuthorityContext>,
    ) -> Result<(), SessionError> {
        self.inner
            .update_session_mob_authority_context(id, authority_context)
            .await
    }
    async fn has_live_session(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Result<bool, SessionError> {
        self.inner.has_live_session(id).await
    }
    async fn set_session_tool_visibility_state(
        &self,
        id: &meerkat_core::types::SessionId,
        state: Option<meerkat_core::SessionToolVisibilityState>,
    ) -> Result<(), SessionError> {
        self.inner
            .set_session_tool_visibility_state(id, state)
            .await
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
    async fn record_live_terminal_error(
        &self,
        id: &meerkat_core::types::SessionId,
        cause: meerkat_core::live_adapter::LiveAdapterErrorCode,
    ) -> Result<(), SessionError> {
        self.inner.record_live_terminal_error(id, cause).await
    }
    async fn record_live_output_audio_degraded(
        &self,
        id: &meerkat_core::types::SessionId,
        dropped: u64,
    ) -> Result<(), SessionError> {
        self.inner
            .record_live_output_audio_degraded(id, dropped)
            .await
    }
}

#[async_trait]
impl meerkat_core::service::SessionServiceCommsExt for AfterCreateMobSessionService {
    async fn comms_runtime(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Option<Arc<dyn meerkat_core::agent::CommsRuntime>> {
        self.inner.comms_runtime(id).await
    }

    async fn event_injector(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Option<Arc<dyn meerkat_core::EventInjector>> {
        self.inner.event_injector(id).await
    }

    async fn interaction_event_injector(
        &self,
        id: &meerkat_core::types::SessionId,
    ) -> Option<Arc<dyn meerkat_core::event_injector::SubscribableInjector>> {
        self.inner.interaction_event_injector(id).await
    }
}

#[async_trait]
impl meerkat_core::service::SessionServiceControlExt for AfterCreateMobSessionService {
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
    async fn stage_tool_results(
        &self,
        id: &meerkat_core::types::SessionId,
        req: meerkat_core::service::StageToolResultsRequest,
    ) -> Result<meerkat_core::service::StageToolResultsResult, SessionError> {
        self.inner.stage_tool_results(id, req).await
    }
}

#[async_trait]
impl meerkat_core::service::SessionServiceHistoryExt for AfterCreateMobSessionService {
    async fn read_history(
        &self,
        id: &meerkat_core::types::SessionId,
        query: meerkat_core::service::SessionHistoryQuery,
    ) -> Result<meerkat_core::service::SessionHistoryPage, SessionError> {
        self.inner.read_history(id, query).await
    }
    async fn read_transcript_revision(
        &self,
        id: &meerkat_core::types::SessionId,
        query: meerkat_core::service::SessionTranscriptRevisionQuery,
    ) -> Result<meerkat_core::service::SessionTranscriptRevisionPage, SessionError> {
        self.inner.read_transcript_revision(id, query).await
    }
    async fn list_transcript_revisions(
        &self,
        id: &meerkat_core::types::SessionId,
        query: meerkat_core::service::SessionTranscriptRevisionListQuery,
    ) -> Result<meerkat_core::service::SessionTranscriptRevisionList, SessionError> {
        self.inner.list_transcript_revisions(id, query).await
    }
}

#[async_trait]
impl MobSessionService for AfterCreateMobSessionService {
    async fn load_session_for_resume(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_mob::ResumeSessionLoad, SessionError> {
        self.inner.load_session_for_resume(session_id).await
    }
    async fn prepare_session_for_resume(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner.prepare_session_for_resume(session_id).await
    }
    async fn materialize_session_for_resume(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_mob::ResumeSessionLoad, SessionError> {
        self.inner.materialize_session_for_resume(session_id).await
    }

    async fn create_session_under_runtime_turn_boundary(
        &self,
        req: meerkat_core::service::CreateSessionRequest,
    ) -> Result<meerkat_core::RunResult, SessionError> {
        let (req, context) = self.prepare_create_request(req);
        let result = self
            .inner
            .create_session_under_runtime_turn_boundary(req)
            .await?;
        Ok(self.complete_create(result, context).await)
    }

    async fn create_session_with_actor_witness_under_runtime_turn_boundary(
        &self,
        req: meerkat_core::service::CreateSessionRequest,
        actor_witness_slot: &meerkat_session::LiveSessionActorWitnessSlot,
    ) -> Result<meerkat_core::RunResult, SessionError> {
        let (req, context) = self.prepare_create_request(req);
        let result = self
            .inner
            .create_session_with_actor_witness_under_runtime_turn_boundary(req, actor_witness_slot)
            .await?;
        Ok(self.complete_create(result, context).await)
    }

    async fn create_session_with_machine_archived_resume_authority_under_runtime_turn_boundary(
        &self,
        req: meerkat_core::service::CreateSessionRequest,
        authorization: meerkat_runtime::ArchivedSessionActorMaterializationAuthorization,
    ) -> Result<meerkat_core::RunResult, SessionError> {
        let (req, context) = self.prepare_create_request(req);
        let result = self
            .inner
            .create_session_with_machine_archived_resume_authority_under_runtime_turn_boundary(
                req,
                authorization,
            )
            .await?;
        Ok(self.complete_create(result, context).await)
    }

    async fn create_session_with_machine_archived_resume_authority_and_actor_witness_under_runtime_turn_boundary(
        &self,
        req: meerkat_core::service::CreateSessionRequest,
        authorization: meerkat_runtime::ArchivedSessionActorMaterializationAuthorization,
        actor_witness_slot: &meerkat_session::LiveSessionActorWitnessSlot,
    ) -> Result<meerkat_core::RunResult, SessionError> {
        let (req, context) = self.prepare_create_request(req);
        let result = self
            .inner
            .create_session_with_machine_archived_resume_authority_and_actor_witness_under_runtime_turn_boundary(
                req,
                authorization,
                actor_witness_slot,
            )
            .await?;
        Ok(self.complete_create(result, context).await)
    }

    fn supports_persistent_sessions(&self) -> bool {
        self.inner.supports_persistent_sessions()
    }

    fn supports_runtime_turn_apply(&self) -> bool {
        self.inner.supports_runtime_turn_apply()
    }

    // meerkat 0.7.19 disposal-routing seam — forwarded for the same reason
    // as in `delegate_mob_session_service!` above.
    async fn session_known_to_archive_authority(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<bool, SessionError> {
        self.inner
            .session_known_to_archive_authority(session_id)
            .await
    }
    fn runtime_adapter(&self) -> Option<Arc<meerkat_runtime::MeerkatMachine>> {
        self.inner.runtime_adapter()
    }
    async fn interrupt_with_machine_authority(
        &self,
        session_id: &meerkat_core::types::SessionId,
        authority: meerkat_runtime::MachineSessionControlAuthority,
    ) -> Result<(), SessionError> {
        self.inner
            .interrupt_with_machine_authority(session_id, authority)
            .await
    }
    async fn cancel_after_boundary_with_machine_authority(
        &self,
        session_id: &meerkat_core::types::SessionId,
        expected_run_id: &meerkat_core::lifecycle::RunId,
        authority: meerkat_runtime::MachineSessionControlAuthority,
    ) -> Result<(), SessionError> {
        cancel_after_boundary_with_machine_authority_if_live(
            self.inner.as_ref(),
            session_id,
            expected_run_id,
            authority,
        )
        .await
    }
    async fn cancel_current_after_boundary_with_machine_authority(
        &self,
        session_id: &meerkat_core::types::SessionId,
        authority: meerkat_runtime::MachineSessionControlAuthority,
    ) -> Result<(), SessionError> {
        self.inner
            .cancel_current_after_boundary_with_machine_authority(session_id, authority)
            .await
    }
    async fn live_session_actor_registered(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<bool, SessionError> {
        self.inner.live_session_actor_registered(session_id).await
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
    // meerkat 0.7.29 revival seam (ask 31): forward, never default — the
    // trait defaults mask the inner service's revival support (Bug I victims
    // stay unresumable through the wrapper).
    async fn load_revivable_retired_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::session::Session>, SessionError> {
        self.inner.load_revivable_retired_session(session_id).await
    }
    async fn load_persisted_session_metadata(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::PersistedSessionMetadataView>, SessionError> {
        self.inner.load_persisted_session_metadata(session_id).await
    }
    async fn authorize_revivable_retired_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
        authority: meerkat_runtime::PreparedArchivedResumeCommitLease,
    ) -> Result<meerkat_runtime::AuthorizedArchivedResumeCommitLease, SessionError> {
        self.inner
            .authorize_revivable_retired_session(session_id, authority)
            .await
    }
    async fn create_session_with_machine_archived_resume_authority(
        &self,
        req: meerkat_core::service::CreateSessionRequest,
        authorization: meerkat_runtime::ArchivedSessionActorMaterializationAuthorization,
    ) -> Result<meerkat_core::RunResult, SessionError> {
        let (req, context) = self.prepare_create_request(req);
        let result = self
            .inner
            .create_session_with_machine_archived_resume_authority(req, authorization)
            .await?;
        Ok(self.complete_create(result, context).await)
    }
    async fn subscribe_session_events(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::comms::EventStream, meerkat_core::comms::StreamError> {
        meerkat_mob::MobSessionService::subscribe_session_events(self.inner.as_ref(), session_id)
            .await
    }
    async fn archive_with_mob_lifecycle_authority(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner
            .archive_with_mob_lifecycle_authority(session_id)
            .await
    }
    async fn archive_with_mob_lifecycle_authority_under_runtime_turn_boundary(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner
            .archive_with_mob_lifecycle_authority_under_runtime_turn_boundary(session_id)
            .await
    }
    async fn execution_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::agent::AgentExecutionSnapshot>, SessionError> {
        self.inner.execution_snapshot(session_id).await
    }
    async fn tool_scope_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::ToolScopeSnapshot>, SessionError> {
        self.inner.tool_scope_snapshot(session_id).await
    }
    async fn external_tool_surface_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::ExternalToolSurfaceSnapshot>, SessionError> {
        self.inner.external_tool_surface_snapshot(session_id).await
    }
    async fn peer_ingress_runtime_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<meerkat_core::PeerIngressRuntimeSnapshot>, SessionError> {
        self.inner.peer_ingress_runtime_snapshot(session_id).await
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
    async fn prepare_transient_turn_context_for_active_turn(
        &self,
        session_id: &meerkat_core::types::SessionId,
        expected_run_id: &meerkat_core::lifecycle::RunId,
        contexts: Vec<meerkat_core::lifecycle::run_primitive::TurnRequestContext>,
    ) -> Result<meerkat_core::CoreBoundaryStageOutput, meerkat_core::CoreBoundaryStageError> {
        self.inner
            .prepare_transient_turn_context_for_active_turn(session_id, expected_run_id, contexts)
            .await
    }
    async fn acknowledge_committed_runtime_session_boundary_under_turn_finalization_boundary(
        &self,
        session_id: &meerkat_core::types::SessionId,
        authority: &meerkat_core::CommittedSessionBoundaryAuthority,
    ) -> Result<(), SessionError> {
        self.inner
            .acknowledge_committed_runtime_session_boundary_under_turn_finalization_boundary(
                session_id, authority,
            )
            .await
    }
    async fn acquire_runtime_turn_finalization_guard(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Box<dyn meerkat_core::lifecycle::CoreExecutorTurnFinalizationGuard>, SessionError>
    {
        self.inner
            .acquire_runtime_turn_finalization_guard(session_id)
            .await
    }
    async fn checkpoint_committed_runtime_session_snapshot_under_turn_finalization_boundary(
        &self,
        session_id: &meerkat_core::types::SessionId,
        session_snapshot: Arc<Vec<u8>>,
    ) -> Result<(), SessionError> {
        self.inner
            .checkpoint_committed_runtime_session_snapshot_under_turn_finalization_boundary(
                session_id,
                session_snapshot,
            )
            .await
    }
    async fn discard_live_session_after_runtime_stop_terminalized(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner
            .discard_live_session_after_runtime_stop_terminalized(session_id)
            .await
    }
    async fn discard_live_session_after_runtime_stop_terminalized_under_turn_finalization_boundary(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner
            .discard_live_session_after_runtime_stop_terminalized_under_turn_finalization_boundary(
                session_id,
            )
            .await
    }
    async fn publish_interaction_terminals(
        &self,
        session_id: &meerkat_core::types::SessionId,
        events: &[meerkat_core::event::AgentEvent],
    ) -> Result<
        Vec<meerkat_core::lifecycle::core_executor::CoreInteractionTerminalPublicationReceipt>,
        SessionError,
    > {
        self.inner
            .publish_interaction_terminals(session_id, events)
            .await
    }
    async fn discard_live_session(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner.discard_live_session(session_id).await
    }
    async fn discard_live_session_under_runtime_turn_boundary(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<(), SessionError> {
        self.inner
            .discard_live_session_under_runtime_turn_boundary(session_id)
            .await
    }
    async fn discard_live_session_actor_under_runtime_turn_boundary(
        &self,
        witness: &meerkat_session::LiveSessionActorWitness,
    ) -> Result<bool, SessionError> {
        self.inner
            .discard_live_session_actor_under_runtime_turn_boundary(witness)
            .await
    }
    async fn await_event_projection_drain(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<bool, SessionError> {
        self.inner.await_event_projection_drain(session_id).await
    }
    async fn checkpoint_committed_runtime_session_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
        session_snapshot: Arc<Vec<u8>>,
    ) -> Result<(), SessionError> {
        self.inner
            .checkpoint_committed_runtime_session_snapshot(session_id, session_snapshot)
            .await
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
    pub binary_blob_store: Option<Arc<dyn BinaryBlobStore>>,
    pub(crate) agent_mob_mcp_state: Option<Arc<meerkat_mob_mcp::MobMcpState>>,
    pub(crate) implicit_delegate_retirement_overrides: Option<ImplicitDelegateRetirementOverrides>,
    pub(crate) agent_mob_default_llm_client_slot: Option<SharedDefaultLlmClientSlot>,
    pub(crate) console_spawn_sink_slot: Option<SharedConsoleSpawnSinkSlot>,
    pub(crate) identity_runtime_slot: Option<SharedIdentityRuntimeSlot>,
    pub options: MobBootstrapOptions,
    /// Strong runtime authority shared by the session service and every
    /// runtime host installed for it.
    ///
    /// Keeping the adapter here is significant for ephemeral services: their
    /// upstream adapter cache is weak, so an installed host would otherwise be
    /// lost before bootstrap asks the service for its adapter again.
    pub runtime_adapter: Option<Arc<meerkat_runtime::MeerkatMachine>>,
    /// Pre-build customizer applied to every mob member spawn (classic-path
    /// agent memory rides here — see `crate::memory::spawn_customizer`).
    /// Forwarded to `MobBuilder::with_spawn_member_customizer`.
    pub(crate) spawn_member_customizer: Option<Arc<dyn meerkat_mob::SpawnMemberCustomizer>>,
    /// Mob-wide external-tools provider, forwarded to
    /// `MobBuilder::with_default_external_tools_provider`. Called at EVERY
    /// member spawn — including revival — so tools attached here survive
    /// `materialize_revived_member_session`, unlike the per-spawn
    /// `SpawnMemberSpec.external_tools` overlay, which revival drops
    /// (meerkat-studio ask K4/M2). NOTE: a profile's `tools.mcp` allowlist
    /// gates what this provider exposes to that member, and an EMPTY allowlist
    /// means the full surface, not none.
    pub(crate) default_external_tools_provider: Option<meerkat_mob::ExternalToolsProvider>,
    /// Realm-scoped WorkGraph service, forwarded to
    /// `MobBuilder::with_workgraph_service` so every mob-executor turn gets
    /// apply-time attention overlay injection, and to the agent mob-tool
    /// state so child mobs inherit it. Set via
    /// [`with_workgraph_service`](Self::with_workgraph_service).
    pub(crate) workgraph_service: Option<meerkat::WorkGraphService>,
    /// Tool-plane admission slots (one per `install_workgraph_tools` call on
    /// a builder feeding this runtime). `MobRuntime::bootstrap` fills each
    /// with the runtime-wide [`WorkGraphAdmission`] so the agent tool plane's
    /// `workgraph_attention_reassign` runs the same duplicate-binding guard
    /// (and holds the same gate) as the RPC surfaces.
    pub(crate) workgraph_admission_slots: Vec<crate::workgraph_admission::WorkGraphAdmissionSlot>,
    /// Cross-process admission sidecar path — set when the workgraph store is
    /// SQLite-backed (shareable by a gateway + library-mode runtime on one
    /// state dir), `None` for memory-backed runtimes (single-process by
    /// construction).
    pub(crate) workgraph_admission_sidecar: Option<PathBuf>,
    /// Composition-time storage durability resolution (H1/H2), surfaced by
    /// the runtime health surfaces. The stock constructors record it;
    /// externally-composed specs (`MobBootstrapSpec::new` — both gateway
    /// binaries roll their own session services) should set it beside their
    /// own store composition, and `None` renders as an absent declaration.
    pub resolved_storage: Option<ResolvedStorageSummary>,
    /// Per-session durable write epochs (persistent runtime-backed path
    /// only). Cheap unchanged-since-last-look witness for read-side loops
    /// (console session-history discovery); `None` means no such witness and
    /// callers must fall back to reading.
    pub(crate) session_write_epochs: Option<Arc<SessionSnapshotWriteEpochs>>,
    /// Committed-boundary heal authority for the identity-first continuity
    /// repair supervisor (2026-07-29 heal/re-Break incident). The stock
    /// persistent constructors record the concrete meerkat-backed recoverer;
    /// externally-composed specs may leave it `None` (no heal seam — the
    /// repair supervisor falls back to plain reconcile retries).
    pub committed_boundary_recoverer:
        Option<Arc<dyn crate::identity_first::bridge::CommittedBoundaryRecoverer>>,
    /// Late-bound §10.1 dispatch-time taint slot carried by the base
    /// session-service wrapper `Self::new` installs; see
    /// [`Self::dispatch_taint_slot`].
    pub(crate) dispatch_taint_slot: crate::memory::dispatch_taint::DispatchTaintSlot,
    /// Holds the ephemeral temp directory alive for the lifetime of the spec.
    /// Only populated when the builder creates an ephemeral runtime.
    pub(crate) _ephemeral_dir: Option<Arc<tempfile::TempDir>>,
}

impl MobBootstrapSpec {
    pub fn new(
        definition: MobDefinition,
        storage: MobStorage,
        session_service: Arc<dyn MobSessionService>,
    ) -> Self {
        // Every spec construction path funnels through here (the stock
        // constructors call `Self::new` with their wrapped service), so this
        // is the ONE layer that carries the dispatch-time taint slot: each
        // member create passes it exactly once, and later `with_*` re-wraps
        // never double-decorate.
        let dispatch_taint_slot = crate::memory::dispatch_taint::DispatchTaintSlot::default();
        let session_service = Arc::new(PreBuildMobSessionService {
            inner: session_service,
            hook: no_op_pre_build_hook(),
            dispatch_taint: Some(dispatch_taint_slot.clone()),
            after_create_hook: None,
            runtime_adapter_override: None,
            session_read_absorber: None,
            archived_terminal_authority: None,
        }) as Arc<dyn MobSessionService>;
        Self {
            definition,
            storage,
            session_service,
            binary_blob_store: None,
            agent_mob_mcp_state: None,
            implicit_delegate_retirement_overrides: None,
            agent_mob_default_llm_client_slot: None,
            console_spawn_sink_slot: None,
            identity_runtime_slot: None,
            options: MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: None,
            },
            runtime_adapter: None,
            spawn_member_customizer: None,
            default_external_tools_provider: None,
            workgraph_service: None,
            workgraph_admission_slots: Vec::new(),
            workgraph_admission_sidecar: None,
            resolved_storage: None,
            session_write_epochs: None,
            committed_boundary_recoverer: None,
            dispatch_taint_slot,
            _ephemeral_dir: None,
        }
    }

    /// The late-bound §10.1 dispatch-time taint slot every member session
    /// create built from this spec consults (see
    /// `crate::memory::dispatch_taint`). Compositions that assemble the full
    /// agent-memory stack fill the returned slot with the stack's
    /// [`crate::SessionTaintTracker`]; unfilled it costs nothing.
    pub fn dispatch_taint_slot(&self) -> crate::memory::dispatch_taint::DispatchTaintSlot {
        self.dispatch_taint_slot.clone()
    }

    /// Record the composition-time storage durability resolution for a spec
    /// whose stores were composed externally (see
    /// [`resolved_storage`](Self::resolved_storage)).
    #[must_use]
    pub fn with_resolved_storage(mut self, summary: ResolvedStorageSummary) -> Self {
        self.resolved_storage = Some(summary);
        self
    }

    pub fn with_options(mut self, options: MobBootstrapOptions) -> Self {
        self.options = options;
        self
    }

    /// Install the agent-facing mob tool surface (spawn/delegate + the
    /// schedule mob-target authority) for externally-constructed specs.
    ///
    /// The stock `persistent()`/ephemeral constructors do this internally;
    /// specs built via `MobBootstrapSpec::new` (both gateway binaries roll
    /// their own session services) previously skipped it, which left
    /// `agent_mob_mcp_state()` None — members still got mob tools through
    /// meerkat-mob's INTERNAL default state, but mobkit's schedule host had
    /// no mob authority: `spawn_schedule_host` fell back to the Noop mob
    /// host, so agent-authored schedules could neither rewrite to mob-member
    /// targets at authoring nor deliver identity/mob targets at fire time
    /// ("scheduled identity targets are not supported by this session host",
    /// the HomeCore 0.7.26 last-link failure).
    ///
    /// Call AFTER any session-service wrapping (`with_session_runtime_adapter`)
    /// so the installed tools hold the final wrapped service, and AFTER
    /// [`with_workgraph_service`](Self::with_workgraph_service) so child mobs
    /// inherit the workgraph authority. `mob_tools_slot` is the agent factory
    /// builder's `default_mob_tools` slot.
    pub fn with_agent_mob_tools(
        mut self,
        mob_tools_slot: Arc<
            std::sync::RwLock<Option<Arc<dyn meerkat_core::service::MobToolsFactory>>>,
        >,
    ) -> Self {
        let (
            agent_mob_mcp_state,
            implicit_delegate_retirement_overrides,
            agent_mob_default_llm_client_slot,
            console_spawn_sink_slot,
            identity_runtime_slot,
        ) = install_agent_mob_tools(
            &self.definition,
            mob_tools_slot,
            Arc::clone(&self.session_service),
            self.workgraph_service.clone(),
            None,
        );
        self.agent_mob_mcp_state = Some(agent_mob_mcp_state);
        self.implicit_delegate_retirement_overrides = Some(implicit_delegate_retirement_overrides);
        self.agent_mob_default_llm_client_slot = Some(agent_mob_default_llm_client_slot);
        self.console_spawn_sink_slot = Some(console_spawn_sink_slot);
        self.identity_runtime_slot = Some(identity_runtime_slot);
        self
    }

    /// Thread a realm-scoped WorkGraph service into the mob runtime.
    ///
    /// `MobRuntime::bootstrap` forwards it to
    /// `MobBuilder::with_workgraph_service`, which turns on apply-time
    /// attention overlay injection for every mob-executor turn. Call BEFORE
    /// [`with_agent_mob_tools`](Self::with_agent_mob_tools) — the agent mob
    /// state snapshots the service at install time so agent-spawned child
    /// mobs inherit it.
    #[must_use]
    pub fn with_workgraph_service(mut self, service: Option<meerkat::WorkGraphService>) -> Self {
        self.workgraph_service = service;
        self
    }

    /// Register a tool-plane admission slot (returned by
    /// `workgraph_wiring::install_workgraph_tools` /
    /// `attach_workgraph_tools*`) to be filled at bootstrap with the
    /// runtime-wide [`WorkGraphAdmission`](crate::workgraph_admission::WorkGraphAdmission).
    /// Every builder whose members can call `workgraph_attention_reassign`
    /// must have its slot registered here, or those members bypass the
    /// duplicate-binding admission guard the RPC surfaces enforce.
    #[must_use]
    pub fn with_workgraph_admission_slot(
        mut self,
        slot: crate::workgraph_admission::WorkGraphAdmissionSlot,
    ) -> Self {
        self.workgraph_admission_slots.push(slot);
        self
    }

    /// Serialize admissions cross-process through the sidecar lock database
    /// under `state_dir` (see
    /// [`workgraph_admission_sidecar_path`](crate::workgraph_admission::workgraph_admission_sidecar_path)).
    /// Call for SQLite-backed workgraph stores — the store file is shareable
    /// by a gateway and a library-mode runtime on one state dir, and each
    /// process's in-process gate cannot see the other. Memory-backed
    /// runtimes must not set this.
    #[must_use]
    pub fn with_workgraph_admission_sidecar(mut self, state_dir: &Path) -> Self {
        self.workgraph_admission_sidecar =
            Some(crate::workgraph_admission::workgraph_admission_sidecar_path(state_dir));
        self
    }

    /// Install a mob-wide external-tools provider (e.g. MCP-backed callback
    /// tools). Unlike the per-spawn `SpawnMemberSpec.external_tools` overlay —
    /// which member revival silently drops — this provider is consulted on
    /// every spawn AND every revival, so the tools are durable for the
    /// member's whole lifecycle. The profile's `tools.mcp` allowlist gates
    /// what each member sees; an empty allowlist means the full surface.
    pub fn with_default_external_tools_provider(
        mut self,
        provider: meerkat_mob::ExternalToolsProvider,
    ) -> Self {
        self.default_external_tools_provider = Some(provider);
        self
    }

    /// Expose a runtime adapter through the session-service facade.
    ///
    /// Custom embedders that construct their own `MobSessionService` still need
    /// MobKit's session-service surface to report the same runtime authority
    /// that `MobBuilder::with_runtime_adapter(...)` receives. This keeps
    /// autonomous-host comms, runtime inspection, and control paths pointed at
    /// one machine without forcing embedders through the stock factory helpers.
    pub fn with_session_runtime_adapter(
        mut self,
        adapter: Arc<meerkat_runtime::MeerkatMachine>,
    ) -> Self {
        self.session_service = Arc::new(PreBuildMobSessionService {
            inner: self.session_service,
            hook: no_op_pre_build_hook(),
            dispatch_taint: None,
            after_create_hook: None,
            runtime_adapter_override: Some(adapter),
            session_read_absorber: None,
            archived_terminal_authority: None,
        });
        self
    }

    /// Wrap the session service with an after-create hook that fires after
    /// each successful `create_session`. The hook is best-effort: errors are
    /// not propagated. Uses `AfterCreateMobSessionService` which wraps the
    /// inner service without a pre-build hook, so any pre-build mutations
    /// from inner wrappers are fully reflected in the context.
    pub fn with_after_create_hook(mut self, hook: AfterCreateHook) -> Self {
        self.session_service = Arc::new(AfterCreateMobSessionService {
            inner: self.session_service,
            after_hook: hook,
        });
        self
    }

    /// Install the write-epoch witness produced by
    /// [`epoch_tracking_runtime_store`] on an externally-composed spec.
    ///
    /// [`Self::new`] leaves the witness absent, which disables the console
    /// session-history epoch gate and whole-document read absorption — on
    /// gateway compositions that was the 0.8.4 idle driver: the 5s console
    /// discovery loop re-read and re-validated every member's full session
    /// document forever (~0.3 core per idle durable member at production
    /// document sizes). Both gateway binaries compose through [`Self::new`],
    /// so they must wrap their runtime store with
    /// [`epoch_tracking_runtime_store`] and hand the witness here.
    ///
    /// Also wraps the session service with the
    /// [`SessionDocumentReadAbsorber`] so repeated authoritative
    /// whole-document loads are served from the last decoded document while
    /// the session's write epoch is unchanged.
    pub fn with_session_write_epochs(mut self, epochs: &SessionWriteEpochsHandle) -> Self {
        self.session_write_epochs = Some(Arc::clone(&epochs.epochs));
        self.session_service = Arc::new(PreBuildMobSessionService {
            inner: self.session_service,
            hook: no_op_pre_build_hook(),
            dispatch_taint: None,
            after_create_hook: None,
            runtime_adapter_override: None,
            session_read_absorber: Some(Arc::new(SessionDocumentReadAbsorber::new(Arc::clone(
                &epochs.epochs,
            )))),
            archived_terminal_authority: None,
        });
        self
    }

    /// Overlay the RuntimeStore-owned archived terminal onto resume-seam
    /// reads on an externally-composed spec (both gateway binaries roll
    /// their own session services, so the stock persistent constructor's
    /// wiring does not reach them).
    ///
    /// At meerkat 0.8.11 archive never rewrites session bodies; the
    /// absorbing terminal lives in the runtime store's catalog entry or its
    /// Retired/Destroyed lifecycle row. Without this overlay,
    /// `load_session_for_resume` on a runtime-archived session returns
    /// `Revivable` with no archived terminal and hosts rotate identities off
    /// intact preserved transcripts. Hand it the SAME store the machine and
    /// session service share.
    #[must_use]
    pub fn with_runtime_archived_terminal_authority(
        mut self,
        runtime_store: Arc<dyn meerkat_runtime::RuntimeStore>,
    ) -> Self {
        self.session_service = Arc::new(PreBuildMobSessionService {
            inner: self.session_service,
            hook: no_op_pre_build_hook(),
            dispatch_taint: None,
            after_create_hook: None,
            runtime_adapter_override: None,
            session_read_absorber: None,
            archived_terminal_authority: Some(runtime_store),
        });
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
            CapabilityFlags::default(),
            None,
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
        hook: impl Fn(
            &mut CreateSessionRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), SessionError>> + Send + '_>,
        > + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::ephemeral_inner(
            definition,
            storage,
            store_path,
            max_sessions,
            session_store,
            Some(Arc::new(hook)),
            CapabilityFlags::default(),
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ephemeral_inner(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Option<Arc<dyn AgentSessionStore>>,
        hook: Option<PreBuildHook>,
        mut caps: CapabilityFlags,
        after_create_hook: Option<AfterCreateHook>,
        agent_config: Option<Config>,
    ) -> Self {
        caps.image_generation |= mob_definition_may_use_image_generation(&definition);
        let binary_blob_store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
        let runtime_adapter = if caps.image_generation {
            let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
                Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
            Some(Arc::new(meerkat_runtime::MeerkatMachine::persistent(
                runtime_store,
                Arc::clone(&blob_store),
            )))
        } else {
            None
        };
        let mut factory = AgentFactory::new(&store_path)
            .builtins(caps.builtins)
            .shell(caps.shell)
            .mob(caps.mob)
            .comms(caps.comms)
            .memory(caps.memory);
        if let Some(machine) = runtime_adapter.clone() {
            factory = factory.with_image_generation_machine(machine);
        }
        let config = agent_config.unwrap_or_default();
        let mut builder = FactoryAgentBuilder::new(factory, config);
        builder.default_blob_store = Some(blob_store);
        if let Some(store) = session_store {
            builder.default_session_store = Some(store);
        }
        let (session_llm_reconfigure_blueprint, session_llm_default_client_slot) =
            session_llm_reconfigure_blueprint(&builder, &store_path);
        let mob_tools_slot = Arc::clone(&builder.default_mob_tools);
        // Ephemeral specs carry a memory-backed workgraph so profiles with
        // `tools.workgraph = true` build (the factory fails closed on an
        // enabled category with an empty dispatcher slot).
        let (workgraph_service, workgraph_admission_slot) =
            crate::workgraph_wiring::attach_workgraph_tools_ephemeral(
                &builder,
                definition.id.as_str(),
            );
        let concrete_session_service = Arc::new(meerkat_session::EphemeralSessionService::new(
            builder,
            max_sessions,
        ));
        let effective_runtime_adapter = runtime_adapter
            .clone()
            .or_else(|| MobSessionService::runtime_adapter(concrete_session_service.as_ref()));
        let reconfigure_service: Arc<
            dyn meerkat::session_runtime::llm_reconfigure::SessionRuntimeLlmReconfigureService,
        > = concrete_session_service.clone();
        if let Some(effective_runtime_adapter) = effective_runtime_adapter.as_ref() {
            session_llm_reconfigure_blueprint
                .install(effective_runtime_adapter, reconfigure_service);
        } else {
            tracing::error!(
                "ephemeral session service has no runtime adapter; runtime LLM reconfiguration is unavailable"
            );
        }
        let session_service: Arc<dyn MobSessionService> = concrete_session_service;
        let hook = hook.unwrap_or_else(no_op_pre_build_hook);
        let after_create_hook = if let Some(runtime_adapter) = runtime_adapter {
            let user_after_create_hook = after_create_hook.clone();
            Some(Arc::new(
                move |session_id: meerkat_core::types::SessionId, ctx: SessionCreatedContext| {
                    let runtime_adapter = runtime_adapter.clone();
                    let user_after_create_hook = user_after_create_hook.clone();
                    Box::pin(async move {
                        // The after-create hook is fire-and-forget; surface a
                        // failed control-plane registration in logs instead of
                        // silently dropping it (it cannot abort the session).
                        if let Err(error) =
                            runtime_adapter.register_session(session_id.clone()).await
                        {
                            tracing::error!(
                                session_id = %session_id,
                                error = %error,
                                "post-create session runtime registration failed"
                            );
                        }
                        if let Some(user_after_create_hook) = user_after_create_hook {
                            user_after_create_hook(session_id, ctx).await;
                        }
                    })
                        as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
                },
            ) as AfterCreateHook)
        } else {
            after_create_hook
        };
        let session_service = Arc::new(PreBuildMobSessionService {
            inner: session_service,
            hook,
            dispatch_taint: None,
            after_create_hook,
            runtime_adapter_override: effective_runtime_adapter.clone(),
            session_read_absorber: None,
            archived_terminal_authority: None,
        }) as Arc<dyn MobSessionService>;
        let (
            agent_mob_mcp_state,
            implicit_delegate_retirement_overrides,
            agent_mob_default_llm_client_slot,
            console_spawn_sink_slot,
            identity_runtime_slot,
        ) = install_agent_mob_tools(
            &definition,
            mob_tools_slot,
            Arc::clone(&session_service),
            Some(workgraph_service.clone()),
            Some(session_llm_default_client_slot),
        );
        let mut spec = Self::new(definition, storage, session_service);
        spec.agent_mob_mcp_state = Some(agent_mob_mcp_state);
        spec.implicit_delegate_retirement_overrides = Some(implicit_delegate_retirement_overrides);
        spec.agent_mob_default_llm_client_slot = Some(agent_mob_default_llm_client_slot);
        spec.console_spawn_sink_slot = Some(console_spawn_sink_slot);
        spec.identity_runtime_slot = Some(identity_runtime_slot);
        spec.runtime_adapter = effective_runtime_adapter;
        spec.binary_blob_store = Some(binary_blob_store);
        spec.workgraph_service = Some(workgraph_service);
        spec.workgraph_admission_slots
            .push(workgraph_admission_slot);
        // Ephemeral mode: in-memory blobs are the declared choice of the
        // mode itself; the ephemeral session service persists nothing, so
        // the incremental capability is not applicable.
        let mut slots = vec![
            StorageSlotSummary::declared_ephemeral(
                "sessions",
                "EphemeralSessionService",
                "declared by the ephemeral launch mode",
            ),
            blob_slot_summary(BlobDurability::DeclaredEphemeral),
        ];
        slots.extend(scratch_ring_buffer_slots());
        spec.resolved_storage = Some(
            ResolvedStorageSummary::new(BlobDurability::DeclaredEphemeral, None).with_slots(slots),
        );
        spec
    }

    /// Build a persistent session service with a correctly wired `AgentFactory`.
    ///
    /// The `session_store` is used in two places:
    /// 1. As the persistence backend for `PersistentSessionService` (checkpoint/restore).
    /// 2. Adapted via `StoreAdapter` and set on `FactoryAgentBuilder.default_session_store`
    ///    so that agents use it directly instead of falling back to JSONL.
    ///
    /// # Errors
    ///
    /// Fails closed when the local blob directory or the runtime store under
    /// `store_path` cannot be opened — persistent mode never silently falls
    /// back to in-memory stores.
    pub fn persistent(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Arc<dyn SessionStore>,
    ) -> Result<Self, StorageResolutionError> {
        Self::persistent_inner(
            definition,
            storage,
            store_path,
            max_sessions,
            session_store,
            "caller-supplied session store",
            None,
            false,
            false,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        )
    }

    /// Like [`persistent`](Self::persistent), but with a pre-build hook that
    /// is called before each agent is constructed. Use this to inject external
    /// tools, augment system prompts, or set per-agent labels.
    ///
    /// # Errors
    ///
    /// Fails closed when the local blob directory or the runtime store under
    /// `store_path` cannot be opened — persistent mode never silently falls
    /// back to in-memory stores.
    pub fn persistent_with_hook(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Arc<dyn SessionStore>,
        hook: impl Fn(
            &mut CreateSessionRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), SessionError>> + Send + '_>,
        > + Send
        + Sync
        + 'static,
    ) -> Result<Self, StorageResolutionError> {
        Self::persistent_inner(
            definition,
            storage,
            store_path,
            max_sessions,
            session_store,
            "caller-supplied session store",
            None,
            false,
            false,
            None,
            Some(Arc::new(hook)),
            CapabilityFlags::default(),
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn persistent_inner(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Arc<dyn SessionStore>,
        session_store_kind: &str,
        custom_blob_store: Option<BlobStoreInjection>,
        ephemeral_blobs: bool,
        ephemeral_runtime_store: bool,
        schedule_store: Option<Arc<dyn meerkat::ScheduleStore>>,
        hook: Option<PreBuildHook>,
        caps: CapabilityFlags,
        after_create_hook: Option<AfterCreateHook>,
        agent_config: Option<Config>,
    ) -> Result<Self, StorageResolutionError> {
        Self::persistent_inner_with_provider_stores(
            definition,
            storage,
            store_path,
            max_sessions,
            session_store,
            session_store_kind,
            custom_blob_store,
            ephemeral_blobs,
            ephemeral_runtime_store,
            schedule_store,
            hook,
            caps,
            after_create_hook,
            agent_config,
            None,
        )
    }

    /// [`persistent_inner`](Self::persistent_inner) with the composite
    /// storage provider's meerkat-level bundle (M4b): when present, the
    /// runtime and workgraph slots compose over the provider's stores
    /// instead of local SQLite files, so the advertised single bundle is
    /// not silently split across backends.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn persistent_inner_with_provider_stores(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        session_store: Arc<dyn SessionStore>,
        session_store_kind: &str,
        custom_blob_store: Option<BlobStoreInjection>,
        ephemeral_blobs: bool,
        ephemeral_runtime_store: bool,
        schedule_store: Option<Arc<dyn meerkat::ScheduleStore>>,
        hook: Option<PreBuildHook>,
        mut caps: CapabilityFlags,
        after_create_hook: Option<AfterCreateHook>,
        agent_config: Option<Config>,
        provider_meerkat_stores: Option<crate::storage_provider::ProviderMeerkatStores>,
    ) -> Result<Self, StorageResolutionError> {
        caps.image_generation |= mob_definition_may_use_image_generation(&definition);
        // H1 fail-closed blob slot: the slot resolves to a configured
        // backend, an explicitly declared ephemeral choice, or a startup
        // error — never a silent in-memory fallback (the former warn +
        // `ObjectStoreBlobStore::memory()` arm here was the GKE
        // month-of-silent-data-loss hazard).
        let (binary_blob_store, blob_store, blob_durability): (
            Arc<dyn BinaryBlobStore>,
            Arc<dyn meerkat_core::BlobStore>,
            BlobDurability,
        ) = if let Some(injection) = custom_blob_store {
            let (binary_blob_store, blob_store) = injection.into_pair();
            let persistent = binary_blob_store.is_persistent();
            (
                binary_blob_store,
                blob_store,
                BlobDurability::Custom { persistent },
            )
        } else if ephemeral_blobs {
            let binary_blob_store: Arc<dyn BinaryBlobStore> =
                Arc::new(ObjectStoreBlobStore::memory());
            let blob_store: Arc<dyn meerkat_core::BlobStore> =
                Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
            (
                binary_blob_store,
                blob_store,
                BlobDurability::DeclaredEphemeral,
            )
        } else {
            let blob_path = store_path.join(crate::storage_layout::BLOB_ROOT_DIR_NAME);
            let binary_blob_store: Arc<dyn BinaryBlobStore> =
                match ObjectStoreBlobStore::local(blob_path.clone()) {
                    Ok(store) => Arc::new(store),
                    Err(err) => {
                        return Err(BlobStoreResolutionError::OpenFailed {
                            path: blob_path,
                            message: err.to_string(),
                        }
                        .into());
                    }
                };
            let blob_store: Arc<dyn meerkat_core::BlobStore> =
                Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
            (
                binary_blob_store,
                blob_store,
                BlobDurability::PersistentDisk,
            )
        };
        // Persistent mode with a resolved blob store that will not survive a
        // restart is only legal as an explicit declaration — this keeps
        // custom-injected stores honest too.
        if !binary_blob_store.is_persistent() && !ephemeral_blobs {
            return Err(BlobStoreResolutionError::NonPersistentUndeclared.into());
        }
        // H2: duplicate the incremental-capability probe the session service
        // runs privately, so whole-blob degradation is loud and
        // health-visible instead of silent.
        let session_store_incremental = Some(probe_session_store_incremental(
            &session_store,
            session_store_kind,
        ));
        // Use a SQLite-backed runtime store so we get BOTH durability across
        // process restart AND control-op authority (archive/retire). The
        // earlier 0.6.1 wiring used `Some(InMemoryRuntimeStore)`, which was
        // a half-fix: it kept the session-service's runtime_store path on
        // (so `load_authoritative_session` resolved through runtime_store —
        // good for control ops), but the in-memory store died on restart so
        // resume failed. Switching the in-memory store for a persistent one
        // satisfies both. The store lives at `store_path/runtime.sqlite`,
        // sibling to whatever path the caller's `session_store` uses.
        //
        // Fail-closed (M4): an open failure is a startup error; the
        // in-memory form exists only as the explicit
        // `ephemeral_runtime_store` declaration.
        let (runtime_store, runtime_store_slot): (
            Arc<dyn meerkat_runtime::RuntimeStore>,
            StorageSlotSummary,
        ) = if ephemeral_runtime_store {
            (
                Arc::new(meerkat_runtime::InMemoryRuntimeStore::new()),
                StorageSlotSummary::declared_ephemeral(
                    "runtime",
                    "InMemoryRuntimeStore",
                    "explicitly declared: sessions do not survive process restart",
                ),
            )
        } else if let Some(provider) = provider_meerkat_stores.as_ref() {
            // M4b single-bundle: runtime authority rides the composite
            // provider's meerkat-level bundle; the provider-declared
            // resolution flows to the census verbatim.
            (
                Arc::clone(&provider.runtime_store),
                provider.runtime_slot_summary(),
            )
        } else {
            (
                build_persistent_runtime_store(&store_path)?,
                StorageSlotSummary::persistent("runtime", "SqliteRuntimeStore"),
            )
        };
        // One epoch-observing facade fronts BOTH the machine and the session
        // service, so every session-scoped durable write this process performs
        // invalidates the session-document read absorber installed below.
        // The facade also owns the durable session projection (at meerkat
        // 0.8.11 the session service keeps no plain SessionStore write path,
        // so committed boundaries reach the caller's store only through this
        // write-through) and every-boot authority re-minting - durable
        // SQLite runtime stores included, so a reset/lost runtime.sqlite
        // reseeds from the durable session rows instead of refusing resume.
        let session_read_epochs = Arc::new(SessionSnapshotWriteEpochs::default());
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            SessionStoreBackedRuntimeStore::with_write_epochs_and_durable_projection(
                runtime_store,
                Arc::clone(&session_read_epochs),
                session_store.clone(),
            ),
        );
        let runtime_adapter = Arc::new(meerkat_runtime::MeerkatMachine::persistent(
            Arc::clone(&runtime_store),
            Arc::clone(&blob_store),
        ));
        let mut factory = AgentFactory::new(&store_path)
            .builtins(caps.builtins)
            .shell(caps.shell)
            .mob(caps.mob)
            .comms(caps.comms)
            .memory(caps.memory);
        if caps.image_generation {
            factory = factory.with_image_generation_machine(runtime_adapter.clone());
        }
        let config = agent_config.unwrap_or_default();
        let mut builder = FactoryAgentBuilder::new(factory, config);
        builder.default_session_store = Some(Arc::new(StoreAdapter::new(session_store.clone())));
        builder.default_blob_store = Some(blob_store.clone());
        let (job_store, job_store_slot): (Arc<dyn meerkat::DetachedJobStore>, StorageSlotSummary) =
            if let Some(provider) = provider_meerkat_stores.as_ref() {
                (Arc::clone(&provider.job_store), provider.job_slot_summary())
            } else {
                let path = meerkat_store::realm_paths_in(
                    &store_path,
                    crate::storage_provider::MEERKAT_LEVEL_REALM_ID,
                )
                .jobs_sqlite_path;
                let store =
                    meerkat::SqliteDetachedJobStore::open(path.clone()).map_err(|error| {
                        crate::storage_health::JobStoreResolutionError {
                            path,
                            message: error.to_string(),
                        }
                    })?;
                (
                    Arc::new(store),
                    StorageSlotSummary::persistent("jobs", "SqliteDetachedJobStore"),
                )
            };
        builder.default_detached_job_store = Some(job_store);
        let (session_llm_reconfigure_blueprint, session_llm_default_client_slot) =
            session_llm_reconfigure_blueprint(&builder, &store_path);
        let mob_tools_slot = Arc::clone(&builder.default_mob_tools);
        // Injected schedule store (M4 builder seam): attach the agent-facing
        // schedule tools over the caller's store. Library mode wires no
        // firing host — authored schedules become durable rows the embedder's
        // own driver (or a gateway pointed at the same store) fires.
        let schedule_slot = schedule_store.map(|store| {
            let _attached =
                crate::schedule_wiring::attach_schedule_tools_with_store(&builder, store);
            StorageSlotSummary::persistent("schedule", "custom schedule store").with_detail(
                "caller-injected store; durability rides with the injector. Library mode \
                 attaches schedule tools without a firing host",
            )
        });
        // Durable workgraph store: the composite provider's meerkat-level
        // bundle when installed (M4b single-bundle), otherwise local SQLite
        // beside runtime.sqlite (boot-without on open failure — a
        // sanctioned, health-visible degradation).
        let (workgraph_service, workgraph_admission_slot, workgraph_slot) =
            if let Some(provider) = provider_meerkat_stores.as_ref() {
                let service = meerkat::WorkGraphService::with_scope(
                    Arc::clone(&provider.workgraph_store),
                    definition.id.as_str(),
                    meerkat::WorkNamespace::default(),
                );
                let slot = crate::workgraph_wiring::install_workgraph_tools(&builder, &service);
                (Some(service), Some(slot), provider.workgraph_slot_summary())
            } else {
                match crate::workgraph_wiring::attach_workgraph_tools_reporting(
                    &builder,
                    &store_path,
                    definition.id.as_str(),
                ) {
                    Ok((service, slot)) => (
                        Some(service),
                        Some(slot),
                        StorageSlotSummary::persistent("workgraph", "SqliteWorkGraphStore"),
                    ),
                    Err(error) => (
                        None,
                        None,
                        StorageSlotSummary::degraded(
                            "workgraph",
                            format!("workgraph store failed to open; workgraph disabled: {error}"),
                        ),
                    ),
                }
            };
        let archived_terminal_authority = Arc::clone(&runtime_store);
        let concrete_session_service = Arc::new(meerkat_session::PersistentSessionService::new(
            builder,
            max_sessions,
            session_store,
            runtime_store,
            blob_store,
        ));
        let reconfigure_service: Arc<
            dyn meerkat::session_runtime::llm_reconfigure::SessionRuntimeLlmReconfigureService,
        > = concrete_session_service.clone();
        session_llm_reconfigure_blueprint.install(&runtime_adapter, reconfigure_service);
        // Heal seam (2026-07-29 incident): the CONCRETE persistent service is
        // the committed-boundary recoverer; the erased MobSessionService does
        // not carry the heal API, so the typed handle is captured here.
        let committed_boundary_recoverer: Arc<
            dyn crate::identity_first::bridge::CommittedBoundaryRecoverer,
        > = concrete_session_service.clone();
        let session_service: Arc<dyn MobSessionService> = concrete_session_service;
        let hook = hook.unwrap_or_else(no_op_pre_build_hook);
        let session_service = Arc::new(PreBuildMobSessionService {
            inner: session_service,
            hook,
            dispatch_taint: None,
            after_create_hook,
            runtime_adapter_override: None,
            session_read_absorber: Some(Arc::new(SessionDocumentReadAbsorber::new(Arc::clone(
                &session_read_epochs,
            )))),
            archived_terminal_authority: Some(archived_terminal_authority),
        }) as Arc<dyn MobSessionService>;
        let (
            agent_mob_mcp_state,
            implicit_delegate_retirement_overrides,
            agent_mob_default_llm_client_slot,
            console_spawn_sink_slot,
            identity_runtime_slot,
        ) = install_agent_mob_tools(
            &definition,
            mob_tools_slot,
            Arc::clone(&session_service),
            workgraph_service.clone(),
            Some(session_llm_default_client_slot),
        );
        let mut spec = Self::new(definition, storage, session_service);
        spec.committed_boundary_recoverer = Some(committed_boundary_recoverer);
        spec.session_write_epochs = Some(session_read_epochs);
        spec.agent_mob_mcp_state = Some(agent_mob_mcp_state);
        spec.implicit_delegate_retirement_overrides = Some(implicit_delegate_retirement_overrides);
        spec.agent_mob_default_llm_client_slot = Some(agent_mob_default_llm_client_slot);
        spec.console_spawn_sink_slot = Some(console_spawn_sink_slot);
        spec.identity_runtime_slot = Some(identity_runtime_slot);
        spec.runtime_adapter = Some(runtime_adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        spec.workgraph_service = workgraph_service;
        if let Some(slot) = workgraph_admission_slot {
            // SQLite- or provider-backed store: the backend is shareable
            // across processes, so admissions additionally serialize through
            // the sidecar lock (a same-host guard).
            spec.workgraph_admission_slots.push(slot);
            spec.workgraph_admission_sidecar =
                Some(crate::workgraph_admission::workgraph_admission_sidecar_path(&store_path));
        }
        let mut slots = vec![
            StorageSlotSummary::persistent("sessions", session_store_kind).with_detail(
                if session_store_kind == "SqliteSessionStore" {
                    "builder-opened SQLite store under the state directory"
                } else {
                    "caller-injected store; durability rides with the injector"
                },
            ),
            runtime_store_slot,
            blob_slot_summary(blob_durability),
            workgraph_slot,
            job_store_slot,
        ];
        if let Some(slot) = schedule_slot {
            slots.push(slot);
        }
        slots.extend(scratch_ring_buffer_slots());
        spec.resolved_storage = Some(
            ResolvedStorageSummary::new(blob_durability, session_store_incremental)
                .with_slots(slots),
        );
        Ok(spec)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ephemeral_runtime_backed_inner(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        custom_session_store: Option<Arc<dyn SessionStore>>,
        session_store_kind: &str,
        custom_blob_store: Option<BlobStoreInjection>,
        schedule_store: Option<Arc<dyn meerkat::ScheduleStore>>,
        hook: Option<PreBuildHook>,
        caps: CapabilityFlags,
        after_create_hook: Option<AfterCreateHook>,
        agent_config: Option<Config>,
    ) -> Self {
        Self::ephemeral_runtime_backed_with_provider_stores(
            definition,
            storage,
            store_path,
            max_sessions,
            custom_session_store,
            session_store_kind,
            custom_blob_store,
            schedule_store,
            hook,
            caps,
            after_create_hook,
            agent_config,
            None,
        )
    }

    /// [`ephemeral_runtime_backed_inner`](Self::ephemeral_runtime_backed_inner)
    /// with the composite storage provider's meerkat-level bundle (M4b, the
    /// scratch/ob3 shape): when present, runtime and workgraph authority
    /// ride the provider's stores instead of process-local memory.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ephemeral_runtime_backed_with_provider_stores(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        custom_session_store: Option<Arc<dyn SessionStore>>,
        session_store_kind: &str,
        custom_blob_store: Option<BlobStoreInjection>,
        schedule_store: Option<Arc<dyn meerkat::ScheduleStore>>,
        hook: Option<PreBuildHook>,
        mut caps: CapabilityFlags,
        after_create_hook: Option<AfterCreateHook>,
        agent_config: Option<Config>,
        provider_meerkat_stores: Option<crate::storage_provider::ProviderMeerkatStores>,
    ) -> Self {
        caps.image_generation |= mob_definition_may_use_image_generation(&definition);
        let config = agent_config.unwrap_or_default();
        let has_custom_session_store = custom_session_store.is_some();
        let session_store: Arc<dyn SessionStore> = custom_session_store
            .clone()
            .unwrap_or_else(|| Arc::new(meerkat_store::MemoryStore::new()));
        // Ephemeral-by-design mode: in-memory blobs are the declared choice
        // of the mode itself (scratch/temp-dir launches), not an error-path
        // fallback. The declaration still surfaces through
        // `resolved_storage` because this mode also serves the hybrid shape
        // where a caller durably persists sessions via
        // `custom_session_store` yet holds ephemeral blobs.
        let (binary_blob_store, blob_store, blob_durability): (
            Arc<dyn BinaryBlobStore>,
            Arc<dyn meerkat_core::BlobStore>,
            BlobDurability,
        ) = if let Some(injection) = custom_blob_store {
            let (binary_blob_store, blob_store) = injection.into_pair();
            let persistent = binary_blob_store.is_persistent();
            (
                binary_blob_store,
                blob_store,
                BlobDurability::Custom { persistent },
            )
        } else {
            let binary_blob_store: Arc<dyn BinaryBlobStore> =
                Arc::new(ObjectStoreBlobStore::memory());
            let blob_store: Arc<dyn meerkat_core::BlobStore> =
                Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
            (
                binary_blob_store,
                blob_store,
                BlobDurability::DeclaredEphemeral,
            )
        };
        // H2 probe — only meaningful when a custom store backs a persistent
        // session service below; the ephemeral service persists nothing.
        let session_store_incremental = custom_session_store
            .as_ref()
            .map(|store| probe_session_store_incremental(store, session_store_kind));
        // Runtime-backed ephemeral mode keeps the live EphemeralSessionService
        // as the comms authority, but registers each created session with the
        // same in-memory machine used by image generation. Meerkat 0.6.4's
        // persistent runtime-backed create path does not expose member comms
        // handles early enough for mob edge reconciliation; this bounded bridge
        // preserves live comms while avoiding the old "image tool sees the
        // session as destroyed" split-machine bug.
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
            if let Some(provider) = provider_meerkat_stores.as_ref() {
                // M4b single-bundle: runtime authority rides the composite
                // provider's meerkat-level bundle (the remote-authoritative
                // scratch shape), not process-local memory. An injected
                // session store still receives the committed-boundary
                // projection, and a provider store that lost its records
                // reseeds from the durable rows like every other shape.
                if let Some(custom_session_store) = custom_session_store.clone() {
                    Arc::new(SessionStoreBackedRuntimeStore::new(
                        Arc::clone(&provider.runtime_store),
                        custom_session_store,
                    ))
                } else {
                    Arc::clone(&provider.runtime_store)
                }
            } else if let Some(custom_session_store) = custom_session_store.clone() {
                Arc::new(SessionStoreBackedRuntimeStore::new(
                    Arc::new(meerkat_runtime::InMemoryRuntimeStore::new()),
                    custom_session_store,
                ))
            } else {
                Arc::new(meerkat_runtime::InMemoryRuntimeStore::new())
            };
        let runtime_adapter = Arc::new(meerkat_runtime::MeerkatMachine::persistent(
            Arc::clone(&runtime_store),
            Arc::clone(&blob_store),
        ));
        let mut factory = AgentFactory::new(&store_path)
            .builtins(caps.builtins)
            .shell(caps.shell)
            .mob(caps.mob)
            .comms(caps.comms)
            .memory(caps.memory);
        if caps.image_generation {
            factory = factory.with_image_generation_machine(runtime_adapter.clone());
        }
        let mut builder = FactoryAgentBuilder::new(factory, config);
        builder.default_session_store = Some(Arc::new(StoreAdapter::new(session_store.clone())));
        builder.default_blob_store = Some(blob_store.clone());
        if let Some(provider) = provider_meerkat_stores.as_ref() {
            builder.default_detached_job_store = Some(Arc::clone(&provider.job_store));
        }
        let (session_llm_reconfigure_blueprint, session_llm_default_client_slot) =
            session_llm_reconfigure_blueprint(&builder, &store_path);
        let mob_tools_slot = Arc::clone(&builder.default_mob_tools);
        // Injected schedule store (M4 builder seam): the ob3 shape — an
        // otherwise-ephemeral local runtime whose durable schedule rows live
        // in a caller-supplied remote store. Library mode wires no firing
        // host (see `UnifiedRuntimeBuilder::schedule_store`).
        let schedule_slot = schedule_store.map(|store| {
            let _attached =
                crate::schedule_wiring::attach_schedule_tools_with_store(&builder, store);
            StorageSlotSummary::persistent("schedule", "custom schedule store").with_detail(
                "caller-injected store; durability rides with the injector. Library mode \
                 attaches schedule tools without a firing host",
            )
        });
        let (workgraph_service, workgraph_admission_slot) =
            if let Some(provider) = provider_meerkat_stores.as_ref() {
                // M4b single-bundle: the workgraph rides the composite
                // provider's meerkat-level bundle instead of process-local
                // memory.
                let service = meerkat::WorkGraphService::with_scope(
                    Arc::clone(&provider.workgraph_store),
                    definition.id.as_str(),
                    meerkat::WorkNamespace::default(),
                );
                let slot = crate::workgraph_wiring::install_workgraph_tools(&builder, &service);
                (service, slot)
            } else {
                crate::workgraph_wiring::attach_workgraph_tools_ephemeral(
                    &builder,
                    definition.id.as_str(),
                )
            };
        let session_service: Arc<dyn MobSessionService> =
            if let Some(custom_session_store) = custom_session_store {
                let concrete_session_service =
                    Arc::new(meerkat_session::PersistentSessionService::new(
                        builder,
                        max_sessions,
                        custom_session_store,
                        runtime_store.clone(),
                        blob_store,
                    ));
                let reconfigure_service: Arc<
                dyn meerkat::session_runtime::llm_reconfigure::SessionRuntimeLlmReconfigureService,
            > = concrete_session_service.clone();
                session_llm_reconfigure_blueprint.install(&runtime_adapter, reconfigure_service);
                concrete_session_service
            } else {
                let concrete_session_service = Arc::new(
                    meerkat_session::EphemeralSessionService::new(builder, max_sessions),
                );
                let reconfigure_service: Arc<
                dyn meerkat::session_runtime::llm_reconfigure::SessionRuntimeLlmReconfigureService,
            > = concrete_session_service.clone();
                session_llm_reconfigure_blueprint.install(&runtime_adapter, reconfigure_service);
                concrete_session_service
            };
        let hook = hook.unwrap_or_else(no_op_pre_build_hook);
        let runtime_adapter_for_after_create = runtime_adapter.clone();
        let combined_after_create_hook: AfterCreateHook = Arc::new(move |session_id, ctx| {
            let runtime_adapter = runtime_adapter_for_after_create.clone();
            let after_create_hook = after_create_hook.clone();
            Box::pin(async move {
                // The after-create hook is fire-and-forget; surface a failed
                // control-plane registration in logs instead of silently
                // dropping it (it cannot abort the session).
                if let Err(error) = runtime_adapter.register_session(session_id.clone()).await {
                    tracing::error!(
                        session_id = %session_id,
                        error = %error,
                        "post-create session runtime registration failed"
                    );
                }
                if let Some(after_create_hook) = after_create_hook {
                    after_create_hook(session_id, ctx).await;
                }
            })
        });
        let session_service = Arc::new(PreBuildMobSessionService {
            inner: session_service,
            hook,
            dispatch_taint: None,
            after_create_hook: Some(combined_after_create_hook),
            runtime_adapter_override: Some(runtime_adapter.clone()),
            session_read_absorber: None,
            archived_terminal_authority: None,
        }) as Arc<dyn MobSessionService>;
        let (
            agent_mob_mcp_state,
            implicit_delegate_retirement_overrides,
            agent_mob_default_llm_client_slot,
            console_spawn_sink_slot,
            identity_runtime_slot,
        ) = install_agent_mob_tools(
            &definition,
            mob_tools_slot,
            Arc::clone(&session_service),
            Some(workgraph_service.clone()),
            Some(session_llm_default_client_slot),
        );
        let mut spec = Self::new(definition, storage, session_service);
        spec.agent_mob_mcp_state = Some(agent_mob_mcp_state);
        spec.implicit_delegate_retirement_overrides = Some(implicit_delegate_retirement_overrides);
        spec.agent_mob_default_llm_client_slot = Some(agent_mob_default_llm_client_slot);
        spec.console_spawn_sink_slot = Some(console_spawn_sink_slot);
        spec.identity_runtime_slot = Some(identity_runtime_slot);
        spec.runtime_adapter = Some(runtime_adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        spec.workgraph_service = Some(workgraph_service);
        spec.workgraph_admission_slots
            .push(workgraph_admission_slot);
        let mut slots = vec![
            if has_custom_session_store {
                StorageSlotSummary::persistent("sessions", session_store_kind)
                    .with_detail("caller-injected store; durability rides with the injector")
            } else {
                StorageSlotSummary::declared_ephemeral(
                    "sessions",
                    "MemoryStore",
                    "declared by the ephemeral launch mode",
                )
            },
            if let Some(provider) = provider_meerkat_stores.as_ref() {
                provider.runtime_slot_summary()
            } else {
                StorageSlotSummary::declared_ephemeral(
                    "runtime",
                    "InMemoryRuntimeStore",
                    "declared by the ephemeral launch mode",
                )
            },
            blob_slot_summary(blob_durability),
            if let Some(provider) = provider_meerkat_stores.as_ref() {
                provider.workgraph_slot_summary()
            } else {
                StorageSlotSummary::declared_ephemeral(
                    "workgraph",
                    "MemoryWorkGraphStore",
                    "declared by the ephemeral launch mode",
                )
            },
            if let Some(provider) = provider_meerkat_stores.as_ref() {
                provider.job_slot_summary()
            } else {
                StorageSlotSummary::declared_ephemeral(
                    "jobs",
                    "disabled",
                    "semantic detached admission is unavailable in ephemeral launch mode",
                )
            },
        ];
        if let Some(slot) = schedule_slot {
            slots.push(slot);
        }
        slots.extend(scratch_ring_buffer_slots());
        spec.resolved_storage = Some(
            ResolvedStorageSummary::new(blob_durability, session_store_incremental)
                .with_slots(slots),
        );
        spec
    }
}

/// Error returned by mob runtime operations.
#[derive(Debug)]
pub enum MobRuntimeError {
    Mob(MobError),
    InvalidInput(&'static str),
    InvalidConfig(String),
}

impl std::fmt::Display for MobRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(err) => write!(f, "{err}"),
            Self::InvalidInput(message) => write!(f, "{message}"),
            Self::InvalidConfig(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for MobRuntimeError {}

impl From<MobError> for MobRuntimeError {
    fn from(value: MobError) -> Self {
        Self::Mob(value)
    }
}

// Mobkit's `MobMemberSnapshot`, `MobReconcileReport`, `MobReconcileOptions`
// wrapper types were removed as part of the meerkat 0.6 thin-shell cleanup.
// Consumers now use `meerkat_mob::runtime::MobMemberListEntry` and
// `meerkat_mob::runtime::reconcile::{ReconcileReport, ReconcileOptions,
// MemberFilter}` directly.

/// Context delivered to [`SessionHook::after_create`] after a session is
/// successfully created.
#[derive(Clone, Debug)]
pub struct SessionCreatedContext {
    pub model: String,
    pub labels: std::collections::BTreeMap<String, String>,
    pub system_prompt: Option<String>,
}

/// Hook trait for customising session lifecycle.
///
/// - `before_create` — runs before `create_session`. Returning `Err` aborts
///   session creation (both Rust-native and Python/TS boundary).
/// - `after_create` — runs after session creation succeeds. Best-effort: errors
///   are logged at `warn`, not propagated. The session is already live.
#[async_trait]
pub trait SessionHook: Send + Sync {
    /// Called before session creation. Mutate the request to inject tools,
    /// augment prompts, set labels, override model, etc. Return `Err` to
    /// abort session creation.
    async fn before_create(&self, _req: &mut CreateSessionRequest) -> Result<(), SessionError> {
        Ok(())
    }

    /// Called after a session is successfully created. Best-effort — errors
    /// logged, not propagated.
    async fn after_create(
        &self,
        _session_id: &meerkat_core::types::SessionId,
        _ctx: &SessionCreatedContext,
    ) {
    }
}

/// Capability flags controlling which agent capabilities are enabled.
#[derive(Clone, Copy, Debug)]
pub struct CapabilityFlags {
    pub builtins: bool,
    pub shell: bool,
    pub mob: bool,
    pub comms: bool,
    pub memory: bool,
    pub image_generation: bool,
}

impl Default for CapabilityFlags {
    fn default() -> Self {
        Self {
            builtins: true,
            shell: true,
            mob: true,
            comms: true,
            memory: true,
            image_generation: false,
        }
    }
}

/// Backward-compatible alias for [`MobRuntime`].
pub type RealMobRuntime = MobRuntime;

/// "Profile declares it, profile means it": auto-mark every explicitly
/// declared profile field as resume-overridden so a definition edit reaches
/// identities that already hold durable sessions.
///
/// Durable session metadata restores model/provider/provider_params on
/// resume; without a `resume_overrides` entry a profile declaration is inert
/// on every resumed identity. Two production fleets shipped model migrations
/// that silently did nothing (2026-07) — one ran a three-week-old model until
/// a provider byte cap broke the deployment. Declaration is key PRESENCE, not
/// value comparison: `model` is a required profile key (always declared);
/// `provider`/`self_hosted_server_id` and `provider_params` are optional keys
/// whose TOML presence is carried faithfully by their `Option` fields (TOML
/// cannot express an explicit null). Undeclared fields keep durable-wins
/// semantics — with one deliberate exception below.
///
/// **Model and provider are a COHERENT PAIR, never independently masked**
/// (OB3 cutover incident, 2026-07-29): masking the model alone lets the
/// durable provider survive under a profile model it was never registered
/// for, and the resume is REJECTED typed ("model 'claude-fable-5' is
/// registered for provider 'anthropic', not 'openai'"). When the profile
/// declares no provider, the pair is resolved FROM the declared model — the
/// canonical catalog owner, else the definition's `[models.<id>]` entry —
/// and written onto the profile so both fields apply together on resume.
/// When no coherent provider is resolvable (unknown model, or a DERIVED
/// self-hosted/other provider that needs a binding the profile does not
/// declare), neither field is marked: the whole LLM identity stays on
/// durable truth and the resume-divergence INFO line is the tripwire.
///
/// Applies to inline profile bindings only: realm-ref profiles resolve inside
/// meerkat-mob at spawn time and never pass through this seam. NOTE: the
/// bridge resume-divergence tripwire cannot see realm-ref declarations either
/// (the `RealmProfileStore` is not threaded into `MobSessionBridge`), so a
/// realm-profile edit that loses to durable metadata is currently silent
/// (tracked follow-up).
pub fn auto_mark_declared_resume_overrides(definition: &mut MobDefinition) {
    let MobDefinition {
        profiles, models, ..
    } = definition;
    for binding in profiles.values_mut() {
        let Some(profile) = binding.as_inline_mut() else {
            continue;
        };
        let mut declared = Vec::new();
        let coherent_provider = profile
            .provider
            // A self-hosted server binding is only meaningful under the
            // self_hosted provider; adopt that reading rather than leaving
            // an incoherent half-declaration.
            .or_else(|| {
                profile
                    .self_hosted_server_id
                    .as_ref()
                    .map(|_| Provider::SelfHosted)
            })
            .or_else(|| {
                meerkat_models::canonical()
                    .infer_provider(&profile.model)
                    .or_else(|| models.get(&profile.model).map(|entry| entry.provider))
                    // A DERIVED self-hosted provider needs a server binding
                    // the profile does not declare, and Other names no
                    // concrete adapter: neither can be written back as a
                    // coherent pair. (A DECLARED self_hosted/other above is
                    // honored as written.)
                    .filter(|provider| !matches!(provider, Provider::SelfHosted | Provider::Other))
            });
        if let Some(provider) = coherent_provider {
            profile.provider = Some(provider);
            declared.push(meerkat_mob::ResumeOverrideField::Model);
            declared.push(meerkat_mob::ResumeOverrideField::Provider);
        }
        if profile.provider_params.is_some() {
            declared.push(meerkat_mob::ResumeOverrideField::ProviderParams);
        }
        for field in declared {
            if !profile.resume_overrides.contains(&field) {
                profile.resume_overrides.push(field);
            }
        }
    }
}

/// Live mob runtime backed by a `MobHandle`.
#[derive(Clone)]
pub struct MobRuntime {
    handle: MobHandle,
    session_service: Option<Arc<dyn MobSessionService>>,
    agent_mob_mcp_state: Option<Arc<meerkat_mob_mcp::MobMcpState>>,
    implicit_delegate_retirement_overrides: Option<ImplicitDelegateRetirementOverrides>,
    binary_blob_store: Option<Arc<dyn BinaryBlobStore>>,
    baseline_member_specs: Arc<tokio::sync::RwLock<Vec<SpawnMemberSpec>>>,
    /// Slot shared with the agent mob-tool dispatchers. A console-bearing
    /// runtime fills it so agent-tool spawns project into the console.
    console_spawn_sink_slot: Option<SharedConsoleSpawnSinkSlot>,
    identity_runtime_slot: Option<SharedIdentityRuntimeSlot>,
    /// Realm-scoped WorkGraph service carried over from the bootstrap spec
    /// so `UnifiedRuntime` can expose it to the RPC/console surfaces.
    workgraph_service: Option<meerkat::WorkGraphService>,
    /// Admission authority for the workgraph duplicate-binding guards
    /// (goal/create, attention/resume, attention/reassign — RPC arms AND the
    /// agent tool plane's `workgraph_attention_reassign`). ONE per runtime:
    /// every surface built from this runtime must acquire this same
    /// admission, or concurrent creates race past the check and brick the
    /// member with upstream `MultipleActiveBindings`. Non-admitting
    /// consumers (MobBuilder overlays, the schedule host) use the bare
    /// service and are intentionally not serialized here.
    workgraph_admission: Arc<crate::workgraph_admission::WorkGraphAdmission>,
    /// Composition-time storage durability resolution carried over from the
    /// bootstrap spec so the health surfaces can report it.
    resolved_storage: Option<ResolvedStorageSummary>,
    /// Per-session durable write epochs carried from the bootstrap spec
    /// (persistent runtime-backed path only); see
    /// [`Self::session_document_write_epoch`].
    session_write_epochs: Option<Arc<SessionSnapshotWriteEpochs>>,
    /// Committed-boundary heal authority carried from the bootstrap spec so
    /// the identity-first wiring can inject it into the session bridge.
    committed_boundary_recoverer:
        Option<Arc<dyn crate::identity_first::bridge::CommittedBoundaryRecoverer>>,
    /// Keeps the ephemeral temp directory alive for the lifetime of the runtime.
    /// Dropped when the runtime is dropped, cleaning up the temp dir.
    _ephemeral_dir: Option<Arc<tempfile::TempDir>>,
}

impl MobRuntime {
    pub async fn bootstrap(mut spec: MobBootstrapSpec) -> Result<Self, MobRuntimeError> {
        // Every mobkit surface funnels its definition through here, so this
        // is the one ingress where declared-field resume overrides are
        // marked before the definition reaches meerkat-mob.
        auto_mark_declared_resume_overrides(&mut spec.definition);
        let ephemeral_dir = spec._ephemeral_dir.clone();
        let session_service = spec.session_service.clone();
        let binary_blob_store = spec.binary_blob_store.clone();
        let mob_id = spec.definition.id.clone();
        let agent_mob_mcp_state = spec.agent_mob_mcp_state.clone();
        let implicit_delegate_retirement_overrides =
            spec.implicit_delegate_retirement_overrides.clone();
        let console_spawn_sink_slot = spec.console_spawn_sink_slot.clone();
        let identity_runtime_slot = spec.identity_runtime_slot.clone();
        let default_llm_client = spec
            .options
            .default_llm_client
            .clone()
            .map(ReplaySanitizingLlmClient::wrap)
            // Mob-wide default: serves every member on every provider the
            // definition resolves to (see `ProviderAgnosticLlmClient`).
            .map(ProviderAgnosticLlmClient::wrap);
        if let Some(slot) = spec.agent_mob_default_llm_client_slot.as_ref() {
            *slot
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = default_llm_client.clone();
        }
        let effective_runtime_adapter = spec
            .runtime_adapter
            .clone()
            .or_else(|| session_service.runtime_adapter());

        let mut builder = MobBuilder::new(spec.definition, spec.storage);

        // MobActor's autonomous readiness/comms-drain path consults the
        // builder-published runtime adapter directly. For session services
        // that already embed a runtime adapter (definition-based ephemeral
        // and persistent-with-runtime-backed-service), forward that adapter
        // explicitly so autonomous members do not come up session-backed but
        // runtime-unattached.
        if let Some(adapter) = effective_runtime_adapter {
            builder = builder.with_runtime_adapter(adapter);
        }

        builder = builder
            .with_session_service(session_service.clone())
            .allow_ephemeral_sessions(spec.options.allow_ephemeral_sessions)
            .notify_orchestrator_on_resume(spec.options.notify_orchestrator_on_resume);

        if let Some(customizer) = spec.spawn_member_customizer.clone() {
            builder = builder.with_spawn_member_customizer(customizer);
        }

        if let Some(provider) = spec.default_external_tools_provider.clone() {
            builder = builder.with_default_external_tools_provider(Some(provider));
        }

        // Apply-time WorkGraph attention overlays: the provisioner's
        // MobSessionRuntimeExecutor injects the scoped tool overlay before
        // apply_runtime_turn for every mob-executor turn iff the builder
        // carries the service.
        builder = builder.with_workgraph_service(spec.workgraph_service.clone());

        if let Some(client) = default_llm_client {
            builder = builder.with_default_llm_client(client);
        }

        let handle = builder.create().await?;
        if let Some(state) = agent_mob_mcp_state.as_ref() {
            state.mob_insert_handle(mob_id, handle.clone()).await;
        }
        // One admission per runtime; the tool-plane dispatchers were built
        // before the mob (and thus the roster) existed, so their late-bound
        // slots are filled here with the same instance the RPC surfaces use.
        // The session service rides along as the admission's session→member
        // resolution fallback: member sessions carry their identity on
        // `session_metadata.mob_member_binding`, which co-processes sharing
        // the state dir can read even when their roster is blind.
        let workgraph_admission = Arc::new(crate::workgraph_admission::WorkGraphAdmission::new(
            handle.clone(),
            Some(Arc::clone(&session_service)),
            spec.workgraph_admission_sidecar,
        ));
        for slot in &spec.workgraph_admission_slots {
            *slot
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(Arc::clone(&workgraph_admission));
        }
        Ok(Self {
            handle,
            session_service: Some(session_service),
            agent_mob_mcp_state,
            implicit_delegate_retirement_overrides,
            binary_blob_store,
            baseline_member_specs: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            console_spawn_sink_slot,
            identity_runtime_slot,
            workgraph_service: spec.workgraph_service,
            workgraph_admission,
            resolved_storage: spec.resolved_storage,
            session_write_epochs: spec.session_write_epochs,
            committed_boundary_recoverer: spec.committed_boundary_recoverer,
            _ephemeral_dir: ephemeral_dir,
        })
    }

    pub fn from_handle(handle: MobHandle) -> Self {
        let workgraph_admission = Arc::new(crate::workgraph_admission::WorkGraphAdmission::new(
            handle.clone(),
            None,
            None,
        ));
        Self {
            handle,
            session_service: None,
            agent_mob_mcp_state: None,
            implicit_delegate_retirement_overrides: None,
            binary_blob_store: None,
            baseline_member_specs: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            console_spawn_sink_slot: None,
            identity_runtime_slot: None,
            workgraph_service: None,
            workgraph_admission,
            resolved_storage: None,
            session_write_epochs: None,
            committed_boundary_recoverer: None,
            _ephemeral_dir: None,
        }
    }

    pub fn handle(&self) -> MobHandle {
        self.handle.clone()
    }

    pub fn agent_mob_mcp_state(&self) -> Option<Arc<meerkat_mob_mcp::MobMcpState>> {
        self.agent_mob_mcp_state.clone()
    }

    /// The realm-scoped WorkGraph service the runtime was bootstrapped with.
    pub fn workgraph_service(&self) -> Option<meerkat::WorkGraphService> {
        self.workgraph_service.clone()
    }

    /// The runtime-wide admission authority for the workgraph
    /// duplicate-binding guards. Clones share the underlying instance, so
    /// every surface built from (a clone of) this runtime serializes against
    /// the same gate (and, for SQLite-backed stores, the same cross-process
    /// sidecar).
    pub(crate) fn workgraph_admission(
        &self,
    ) -> Arc<crate::workgraph_admission::WorkGraphAdmission> {
        Arc::clone(&self.workgraph_admission)
    }

    /// Install the console sink that agent-tool spawns project into. A no-op
    /// for runtimes built without agent mob tools (no slot to fill).
    pub(crate) fn install_console_spawn_sink(&self, sink: ConsoleSpawnSink) {
        if let Some(slot) = self.console_spawn_sink_slot.as_ref() {
            *slot
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sink);
        }
    }

    /// The installed console spawn sink, if any.
    pub(crate) fn console_spawn_sink(&self) -> Option<ConsoleSpawnSink> {
        self.console_spawn_sink_slot.as_ref().and_then(|slot| {
            slot.read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })
    }

    pub(crate) fn install_identity_runtime_authority(
        &self,
        identity_runtime: Arc<crate::identity_first::IdentityRuntime>,
    ) {
        if let Some(slot) = self.identity_runtime_slot.as_ref() {
            *slot
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(identity_runtime);
        }
    }

    /// Remove the late-bound identity authority after the mob has fully
    /// quiesced. IdentityRuntime owns a MobHandle in its runtime services, so
    /// retaining the reverse Arc here would form a shutdown cycle and keep
    /// persistent controller locks alive after failed construction.
    pub(crate) fn clear_identity_runtime_authority(&self) {
        if let Some(slot) = self.identity_runtime_slot.as_ref() {
            *slot
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }

    /// Console identity metadata registered by agent-tool spawns, keyed by
    /// console identity. Empty when no console sink is installed.
    pub(crate) async fn console_identity_labels(
        &self,
    ) -> BTreeMap<String, BTreeMap<String, String>> {
        match self.console_spawn_sink() {
            Some(sink) => sink.identity_labels_snapshot().await,
            None => BTreeMap::new(),
        }
    }

    pub(crate) fn implicit_delegate_retirement_overrides(
        &self,
    ) -> Option<ImplicitDelegateRetirementOverrides> {
        self.implicit_delegate_retirement_overrides.clone()
    }

    pub async fn set_baseline_member_specs(&self, specs: Vec<SpawnMemberSpec>) {
        *self.baseline_member_specs.write().await = specs;
    }

    pub async fn baseline_member_specs(&self) -> Vec<SpawnMemberSpec> {
        self.baseline_member_specs.read().await.clone()
    }

    /// Current durable write epoch for `session_id`, when this runtime owns
    /// the single-writer epoch seam (persistent runtime-backed path).
    ///
    /// An unchanged value between two observations proves no session-scoped
    /// durable write went through THIS process in between, so read-side
    /// loops (console session-history discovery) can skip whole-document
    /// re-reads. `None` means no witness exists and callers must read.
    pub(crate) fn session_document_write_epoch(&self, session_id_str: &str) -> Option<u64> {
        let epochs = self.session_write_epochs.as_ref()?;
        let session_id = meerkat_core::types::SessionId::parse(session_id_str).ok()?;
        Some(epochs.observe(&session_id))
    }

    pub async fn read_session_history(
        &self,
        session_id_str: &str,
        offset: usize,
        limit: Option<usize>,
    ) -> Result<SessionHistoryPage, MobRuntimeError> {
        if session_id_str.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput(
                "session_id must not be empty",
            ));
        }
        let Some(session_service) = self.session_service.as_ref() else {
            return Err(MobRuntimeError::InvalidInput(
                "session history unavailable for this runtime",
            ));
        };
        let session_id = meerkat_core::types::SessionId::parse(session_id_str)
            .map_err(|_| MobRuntimeError::InvalidInput("invalid session_id format"))?;
        SessionServiceHistoryExt::read_history(
            session_service.as_ref(),
            &session_id,
            SessionHistoryQuery { offset, limit },
        )
        .await
        .map_err(|err| MobRuntimeError::Mob(MobError::Internal(err.to_string())))
    }

    #[allow(dead_code)]
    pub(crate) async fn runtime_state_for_session(
        &self,
        session_id_str: &str,
    ) -> Result<Option<meerkat_runtime::RuntimeState>, MobRuntimeError> {
        if session_id_str.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput(
                "session_id must not be empty",
            ));
        }
        let Some(session_service) = self.session_service.as_ref() else {
            return Ok(None);
        };
        let Some(runtime_adapter) = session_service.runtime_adapter() else {
            return Ok(None);
        };
        let session_id = meerkat_core::types::SessionId::parse(session_id_str)
            .map_err(|_| MobRuntimeError::InvalidInput("invalid session_id format"))?;
        let state = meerkat_runtime::service_ext::SessionServiceRuntimeExt::runtime_state(
            runtime_adapter.as_ref(),
            &session_id,
        )
        .await
        .map_err(|err| MobRuntimeError::Mob(MobError::Internal(err.to_string())))?;
        Ok(Some(state))
    }

    #[allow(dead_code)]
    pub(crate) async fn comms_runtime_for_session(
        &self,
        session_id_str: &str,
    ) -> Result<Option<Arc<dyn CommsRuntime>>, MobRuntimeError> {
        if session_id_str.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput(
                "session_id must not be empty",
            ));
        }
        let Some(session_service) = self.session_service.as_ref() else {
            return Ok(None);
        };
        let session_id = meerkat_core::types::SessionId::parse(session_id_str)
            .map_err(|_| MobRuntimeError::InvalidInput("invalid session_id format"))?;
        Ok(
            meerkat_core::service::SessionServiceCommsExt::comms_runtime(
                session_service.as_ref(),
                &session_id,
            )
            .await,
        )
    }

    #[allow(dead_code)]
    pub(crate) async fn active_input_ids_for_session(
        &self,
        session_id_str: &str,
    ) -> Result<Option<Vec<String>>, MobRuntimeError> {
        if session_id_str.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput(
                "session_id must not be empty",
            ));
        }
        let Some(session_service) = self.session_service.as_ref() else {
            return Ok(None);
        };
        let Some(runtime_adapter) = session_service.runtime_adapter() else {
            return Ok(None);
        };
        let session_id = meerkat_core::types::SessionId::parse(session_id_str)
            .map_err(|_| MobRuntimeError::InvalidInput("invalid session_id format"))?;
        let input_ids = meerkat_runtime::service_ext::SessionServiceRuntimeExt::list_active_inputs(
            runtime_adapter.as_ref(),
            &session_id,
        )
        .await
        .map_err(|err| MobRuntimeError::Mob(MobError::Internal(err.to_string())))?;
        Ok(Some(
            input_ids.into_iter().map(|id| id.to_string()).collect(),
        ))
    }

    #[allow(dead_code)]
    pub(crate) async fn ensure_comms_drain_for_session(
        &self,
        session_id_str: &str,
    ) -> Result<Option<bool>, MobRuntimeError> {
        if session_id_str.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput(
                "session_id must not be empty",
            ));
        }
        let Some(session_service) = self.session_service.as_ref() else {
            return Ok(None);
        };
        let Some(runtime_adapter) = session_service.runtime_adapter() else {
            return Ok(None);
        };
        let session_id = meerkat_core::types::SessionId::parse(session_id_str)
            .map_err(|_| MobRuntimeError::InvalidInput("invalid session_id format"))?;
        let comms_runtime = meerkat_core::service::SessionServiceCommsExt::comms_runtime(
            session_service.as_ref(),
            &session_id,
        )
        .await;
        if let Some(comms) = comms_runtime {
            let _handle = meerkat_runtime::comms_drain::spawn_comms_drain(
                runtime_adapter.clone(),
                session_id,
                comms,
                None,
            );
            Ok(Some(true))
        } else {
            Ok(Some(false))
        }
    }

    /// Access the session service this runtime was bootstrapped with, if any.
    ///
    /// Present for `MobRuntime::bootstrap(...)`-produced runtimes; `None` for
    /// `MobRuntime::from_handle(...)`. HTTP handlers that need to read session
    /// history reach through this accessor.
    pub fn session_service(&self) -> Option<&Arc<dyn MobSessionService>> {
        self.session_service.as_ref()
    }

    /// Access the committed-boundary heal authority recorded at bootstrap,
    /// if any (2026-07-29 incident: injected into the identity-first session
    /// bridge so the continuity repair supervisor heals for real).
    pub fn committed_boundary_recoverer(
        &self,
    ) -> Option<Arc<dyn crate::identity_first::bridge::CommittedBoundaryRecoverer>> {
        self.committed_boundary_recoverer.clone()
    }

    pub fn binary_blob_store(&self) -> Option<Arc<dyn BinaryBlobStore>> {
        self.binary_blob_store.clone()
    }

    /// Composition-time storage durability resolution carried over from the
    /// bootstrap spec. `None` for externally-composed specs that did not
    /// declare it (see [`MobBootstrapSpec::resolved_storage`]).
    pub fn resolved_storage(&self) -> Option<ResolvedStorageSummary> {
        self.resolved_storage.clone()
    }
}

/// Project a meerkat `MobMemberListEntry` into mobkit's HTTP JSON shape.
///
/// Aligns with meerkat 0.6's lightweight-roster design: list entries do
/// not carry a bridge `session_id`. Callers needing the realtime session
/// for a member must use `mobkit/member_status`, which serializes
/// `MobMemberSnapshot.current_session_id` natively.
pub fn member_entry_to_json(entry: &meerkat_mob::runtime::MobMemberListEntry) -> serde_json::Value {
    let mut value = serde_json::to_value(entry).unwrap_or(serde_json::Value::Null);
    // Wire egress speaks the public alias space: roster ids are comms-safe
    // encodings (meerkat 0.7 MemberCommsName forbids `:` in member ids);
    // decode them back to the aliases consoles/SDKs address members by.
    if let Some(object) = value.as_object_mut() {
        if let Some(serde_json::Value::String(id)) = object.get_mut("agent_identity") {
            *id = crate::member_comms_id::runtime_alias_str(id).into_owned();
        }
        if let Some(serde_json::Value::Array(peers)) = object.get_mut("wired_to") {
            for peer in peers {
                if let serde_json::Value::String(peer_id) = peer {
                    *peer_id = crate::member_comms_id::runtime_alias_str(peer_id).into_owned();
                }
            }
        }
        // meerkat 0.7 replaced the roster-owned `state: MemberState` with the
        // machine-projected `status: MobMemberStatus`. MobKit's wire contract
        // (and the published SDKs — Python `MemberSnapshot.from_dict` indexes
        // `data["state"]`) keeps the `state` key, so project `status` back
        // into the console state vocabulary alongside it.
        object.insert(
            "state".to_string(),
            serde_json::Value::String(member_status_state_string(entry.status)),
        );
    }
    value
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResolvedToolsSnapshot {
    pub identity: String,
    pub session_id: String,
    pub tools: Vec<String>,
}

pub async fn resolved_tools_for_session(
    session_service: Option<&Arc<dyn MobSessionService>>,
    identity: &str,
    session_id: meerkat_core::types::SessionId,
) -> Result<ResolvedToolsSnapshot, MobRuntimeError> {
    let Some(session_service) = session_service else {
        return Err(MobRuntimeError::InvalidInput(
            "resolved tools unavailable for this runtime",
        ));
    };
    let scope = session_service
        .tool_scope_snapshot(&session_id)
        .await
        .map_err(|err| MobRuntimeError::Mob(MobError::Internal(err.to_string())))?
        .ok_or(MobRuntimeError::InvalidInput(
            "identity tool scope is unavailable",
        ))?;
    let mut tools = scope
        .visible_names
        .into_iter()
        .map(meerkat_core::types::ToolName::into_string)
        .collect::<Vec<_>>();
    tools.sort();
    Ok(ResolvedToolsSnapshot {
        identity: identity.to_string(),
        session_id: session_id.to_string(),
        tools,
    })
}

pub async fn resolved_tools_for_member(
    handle: &MobHandle,
    session_service: Option<&Arc<dyn MobSessionService>>,
    member_id: &str,
) -> Result<ResolvedToolsSnapshot, MobRuntimeError> {
    if member_id.trim().is_empty() {
        return Err(MobRuntimeError::InvalidInput("identity must not be empty"));
    }
    let mid = crate::member_comms_id::mob_member_id(member_id);
    let status = handle.member_status(&mid).await?;
    let Some(session_id) = status.current_session_id else {
        return Err(MobRuntimeError::InvalidInput(
            "identity has no current session",
        ));
    };
    resolved_tools_for_session(session_service, member_id, session_id).await
}

/// Project a meerkat `AgentEvent` into mobkit's console/SSE/event-log JSON
/// payload shape.
///
/// Every surface that serializes an `AgentEvent` for consoles or SDKs must
/// route through here (HTTP SSE, the unified-runtime event ingest, the
/// identity-first live console projection) so the wire shape stays uniform:
///
/// - tool events mirror `id` into `tool_call_id`;
/// - meerkat 0.7 removed the flat `result: String` from
///   `ToolExecutionCompleted` (typed `content` blocks are the sole owner),
///   while MobKit's wire contract — and the published SDKs, which parse
///   `result` — keep it. Derive it from the text blocks here.
pub fn console_agent_event_payload(event: &meerkat_core::AgentEvent) -> Value {
    use meerkat_core::AgentEvent;
    use meerkat_core::event::agent_event_type;

    let mut payload = serde_json::to_value(event).unwrap_or_else(|_| serde_json::json!({}));
    let record = match payload.as_object_mut() {
        Some(record) => record,
        None => return payload,
    };
    let is_tool_event = matches!(
        agent_event_type(event),
        "tool_call_requested"
            | "tool_result_received"
            | "tool_execution_started"
            | "tool_execution_completed"
            | "tool_execution_timed_out"
    );
    if is_tool_event
        && !record.contains_key("tool_call_id")
        && let Some(id) = record.get("id").cloned()
    {
        record.insert("tool_call_id".to_string(), id);
    }
    if let AgentEvent::ToolExecutionCompleted { content, .. } = event
        && !record.contains_key("result")
    {
        record.insert(
            "result".to_string(),
            Value::String(meerkat_core::types::text_content(content)),
        );
    }
    payload
}

pub fn content_input_has_images(content: &meerkat_core::ContentInput) -> bool {
    match content {
        meerkat_core::ContentInput::Text(_) => false,
        meerkat_core::ContentInput::Blocks(blocks) => blocks
            .iter()
            .any(|block| matches!(block, meerkat_core::ContentBlock::Image { .. })),
    }
}

pub fn model_capabilities_for_model(
    provider: Provider,
    model: &str,
) -> crate::runtime::ConsoleModelCapabilities {
    let image_input = meerkat_models::profile_for(provider, model)
        .map(|profile| profile.vision)
        .unwrap_or(false);
    crate::runtime::ConsoleModelCapabilities { image_input }
}

pub fn model_capabilities_for_profile(
    profile: &Profile,
) -> crate::runtime::ConsoleModelCapabilities {
    let image_input = meerkat_models::infer_provider(&profile.model)
        .and_then(|provider| meerkat_models::profile_for(provider, &profile.model))
        .map(|profile| profile.vision)
        .unwrap_or(false);
    crate::runtime::ConsoleModelCapabilities { image_input }
}

pub fn model_capabilities_for_role(
    definition: &MobDefinition,
    role: &str,
) -> crate::runtime::ConsoleModelCapabilities {
    let profile_name = ProfileName::from(role);
    definition
        .resolve_inline_profile(&profile_name)
        .map(model_capabilities_for_profile)
        .unwrap_or(crate::runtime::ConsoleModelCapabilities { image_input: false })
}

pub fn model_capabilities_for_member_entry(
    definition: &MobDefinition,
    entry: &meerkat_mob::runtime::MobMemberListEntry,
) -> crate::runtime::ConsoleModelCapabilities {
    model_capabilities_for_role(definition, entry.role.as_str())
}

pub async fn model_capabilities_for_member(
    handle: &MobHandle,
    session_service: Option<&Arc<dyn MobSessionService>>,
    member_id: &meerkat_mob::ids::AgentIdentity,
) -> crate::runtime::ConsoleModelCapabilities {
    if let Some(service) = session_service
        && let Some(session_id) = handle.resolve_bridge_session_id(member_id).await
        && let Ok(view) = service.read(&session_id).await
    {
        return model_capabilities_for_model(view.state.provider, &view.state.model);
    }

    // Capability projection is a read-only display hint: a faulted or absent
    // member lookup degrades to "no image input" rather than failing the read.
    handle
        .get_member(member_id)
        .await
        .ok()
        .flatten()
        .map(|member| model_capabilities_for_role(handle.definition(), member.role.as_str()))
        .unwrap_or(crate::runtime::ConsoleModelCapabilities { image_input: false })
}

pub async fn assert_member_accepts_images(
    handle: &MobHandle,
    session_service: Option<&Arc<dyn MobSessionService>>,
    member_id: &str,
    content: &meerkat_core::ContentInput,
) -> Result<(), MobRuntimeError> {
    if !content_input_has_images(content) {
        return Ok(());
    }
    // Wire member ids are public aliases; the roster id is the comms-safe
    // encoding (meerkat 0.7 MemberCommsName).
    let mid = crate::member_comms_id::mob_member_id(member_id);
    let Some(member) = handle
        .get_member(&mid)
        .await
        .map_err(|_| MobRuntimeError::InvalidInput("member lookup failed"))?
    else {
        return Err(MobRuntimeError::InvalidInput("member not found"));
    };
    let caps = model_capabilities_for_member(handle, session_service, &member.agent_identity).await;
    if caps.image_input {
        Ok(())
    } else {
        Err(MobRuntimeError::InvalidInput(
            "target member model cannot accept image input",
        ))
    }
}

/// Send content to a mob member and return the bridge session id that
/// accepted the injection.
///
/// Validates that `member_id` and `content` are non-empty, calls
/// `handle.member(&id).send(...)`, then queries the mob handle for the
/// currently-bound bridge session id. Meerkat 0.6 removed `session_id` from
/// `MemberDeliveryReceipt`; this helper is mobkit's glue for the
/// send-and-learn-what-session-took-it pattern used by HTTP/RPC handlers and
/// the scheduled-dispatch injection path.
pub async fn send_message_on_mob(
    handle: &MobHandle,
    member_id: &str,
    content: impl Into<meerkat_core::ContentInput>,
) -> Result<String, MobRuntimeError> {
    send_message_on_mob_with_mode(
        handle,
        member_id,
        content,
        meerkat_core::types::HandlingMode::Queue,
    )
    .await
}

/// Variant that accepts the console's `Queue`/`Steer` wire contract while
/// delivering through MobKit's direct member-send path.
pub async fn send_message_on_mob_with_mode(
    handle: &MobHandle,
    member_id: &str,
    content: impl Into<meerkat_core::ContentInput>,
    handling_mode: meerkat_core::types::HandlingMode,
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
    // Wire member ids are public aliases; the roster id is the comms-safe
    // encoding (meerkat 0.7 MemberCommsName).
    let mid = crate::member_comms_id::mob_member_id(member_id);
    let _receipt = handle
        .member(&mid)
        .await?
        .send(content, handling_mode)
        .await?;
    if let Some(session_id) = handle.resolve_bridge_session_id(&mid).await {
        return Ok(session_id.to_string());
    }

    let status = handle.member_status(&mid).await?;
    if status.external_member.is_some() {
        return Ok(String::new());
    }

    Err(MobRuntimeError::Mob(MobError::Internal(
        "member has no bridge session after send".to_string(),
    )))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    struct EmptyDispatcher;

    #[async_trait::async_trait]
    impl meerkat_core::AgentToolDispatcher for EmptyDispatcher {
        fn tools(&self) -> Arc<[Arc<meerkat_core::types::ToolDef>]> {
            Vec::<Arc<meerkat_core::types::ToolDef>>::new().into()
        }

        async fn dispatch(
            &self,
            call: meerkat_core::types::ToolCallView<'_>,
        ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
            Err(meerkat_core::ToolError::not_found(call.name))
        }

        fn capabilities(&self) -> meerkat_core::agent::DispatcherCapabilities {
            meerkat_core::agent::DispatcherCapabilities::default()
        }
    }

    fn wrapper_with_overrides(
        overrides: ImplicitDelegateRetirementOverrides,
    ) -> AutoWireParentMobToolDispatcher {
        AutoWireParentMobToolDispatcher {
            inner: Arc::new(EmptyDispatcher),
            implicit_delegate_retirement_overrides: overrides,
            console_spawn_sink: new_console_spawn_sink_slot(),
            identity_runtime: Arc::new(std::sync::RwLock::new(None)),
            protected_mob_id: "test-mob".to_string(),
            spawner_comms_name: None,
        }
    }

    #[test]
    fn raw_mob_tools_detect_public_and_encoded_generated_aliases() {
        let encoded = crate::member_comms_id::mob_member_id_str("rt:worker:0").into_owned();
        assert_eq!(
            reserved_raw_member_tool_argument(
                "mob_spawn_member",
                &serde_json::json!({"member_id": "rt:worker:0"}),
            ),
            Some(("member_id", "rt:worker:0".to_string()))
        );
        assert_eq!(
            reserved_raw_member_tool_argument(
                "mob_wire",
                &serde_json::json!({"member_id": "classic", "peer": {"local": encoded}}),
            ),
            Some(("peer.local", "rt:worker:0".to_string()))
        );
        assert_eq!(
            reserved_raw_member_tool_argument(
                "delegate",
                &serde_json::json!({"member_id": "mk--victim"}),
            ),
            Some(("member_id", "victim".to_string()))
        );
        assert_eq!(
            reserved_raw_member_tool_argument(
                "spawn_many_members",
                &serde_json::json!({
                    "specs": [
                        {"profile": "worker", "member_id": "classic"},
                        {"profile": "worker", "member_id": encoded},
                    ]
                }),
            ),
            Some(("specs[].member_id", "rt:worker:0".to_string()))
        );
    }

    #[tokio::test]
    async fn raw_mob_tool_dispatch_fails_closed_before_lower_plane() {
        use meerkat_core::AgentToolDispatcher;

        let dispatcher = wrapper_with_overrides(ImplicitDelegateRetirementOverrides::default());
        let args = serde_json::value::RawValue::from_string(
            serde_json::json!({"mob_id": "m", "member_id": "rt:worker:0"}).to_string(),
        )
        .expect("raw args");
        let error = dispatcher
            .dispatch(meerkat_core::types::ToolCallView {
                id: "call-reserved",
                name: "mob_retire_member",
                args: &args,
            })
            .await
            .expect_err("reserved alias must not reach the raw dispatcher");

        assert!(error.to_string().contains("reserved rt:* / mk--"));

        let batch_args = serde_json::value::RawValue::from_string(
            serde_json::json!({
                "specs": [{"profile": "worker", "member_id": "rt:worker:0"}]
            })
            .to_string(),
        )
        .expect("raw args");
        let batch_error = dispatcher
            .dispatch(meerkat_core::types::ToolCallView {
                id: "call-reserved-batch",
                name: "spawn_many_members",
                args: &batch_args,
            })
            .await
            .expect_err("reserved batch member must not reach the raw dispatcher");
        assert!(batch_error.to_string().contains("specs[].member_id"));
    }

    #[tokio::test]
    async fn raw_operator_tool_rejects_registered_plain_durable_identity() {
        use meerkat_core::AgentToolDispatcher;

        let dispatcher = wrapper_with_overrides(ImplicitDelegateRetirementOverrides::default());
        let identity_runtime = Arc::new(crate::identity_first::IdentityRuntime::new(
            crate::identity_first::IdentityRuntimeConfig {
                continuity_store: Arc::new(
                    crate::identity_first::LocalContinuityStore::in_memory()
                        .expect("continuity store"),
                ),
                lease_provider: Arc::new(crate::identity_first::LocalLeaseProvider::new()),
                runtime_instance_id: "raw-operator-test".to_string(),
                has_runtime_store: true,
                durability_policy: crate::identity_first::DurabilityPolicy::SyncWriteThrough,
                bridge: None,
                default_timeout: None,
            },
        ));
        let identity =
            crate::identity_first::AgentIdentity::parse("lead").expect("durable identity");
        identity_runtime
            .register(
                crate::identity_first::DurableAgentSpec {
                    identity,
                    profile: meerkat_mob::ProfileName::from("worker"),
                    addressability: crate::identity_first::AgentAddressability::Addressable,
                    display_name: None,
                    labels: BTreeMap::new(),
                    context: None,
                    additional_instructions: Vec::new(),
                    initial_message: None,
                    runtime_mode_override: None,
                    backend: None,
                    binding: None,
                },
                crate::identity_first::IdentityLifecycleState::Dormant,
                None,
                None,
            )
            .await;
        *dispatcher
            .identity_runtime
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(identity_runtime);

        let args = serde_json::value::RawValue::from_string(
            serde_json::json!({"member_id": "lead"}).to_string(),
        )
        .expect("raw args");
        let error = dispatcher
            .dispatch(meerkat_core::types::ToolCallView {
                id: "call-owned",
                name: "force_cancel_member",
                args: &args,
            })
            .await
            .expect_err("registered durable identity must not reach raw force cancel");
        assert!(
            error
                .to_string()
                .contains("owned by the attached IdentityRuntime")
        );
    }

    #[test]
    fn delegate_tool_schema_exposes_idle_retire_secs() {
        let tool = meerkat_core::types::ToolDef::new(
            "delegate",
            "Delegate work",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string"}
                },
                "required": ["task"]
            }),
        );

        let patched = delegate_tool_def_with_idle_retire_secs(&tool);
        let idle_retire_secs = &patched.input_schema["properties"]["idle_retire_secs"];

        assert!(patched.description.contains("IDLE RETIREMENT:"));
        assert_eq!(idle_retire_secs["anyOf"][0]["type"], "integer");
        assert_eq!(idle_retire_secs["anyOf"][0]["minimum"], 0);
        assert_eq!(idle_retire_secs["anyOf"][1]["type"], "null");
    }

    #[test]
    fn mob_spawn_tool_schema_exposes_opt_in_idle_retire_secs() {
        let tool = meerkat_core::types::ToolDef::new(
            "mob_spawn_member",
            "Spawn member",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "profile": {"type": "string"},
                    "member_id": {"type": "string"}
                },
                "required": ["profile", "member_id"]
            }),
        );

        let patched = mob_spawn_tool_def_with_idle_retire_secs(&tool);
        let idle_retire_secs = &patched.input_schema["properties"]["idle_retire_secs"];

        assert!(
            patched
                .description
                .contains("Omit idle_retire_secs to leave this spawned member out")
        );
        assert_eq!(idle_retire_secs["anyOf"][0]["type"], "integer");
        assert_eq!(idle_retire_secs["anyOf"][0]["minimum"], 0);
        assert_eq!(idle_retire_secs["anyOf"][1]["type"], "null");
    }

    #[tokio::test]
    async fn auto_wire_wrapper_preserves_ops_lifecycle_binding() {
        use meerkat_core::AgentToolDispatcher;
        use std::sync::atomic::{AtomicBool, Ordering};

        struct BindAwareDispatcher {
            bound: Arc<AtomicBool>,
        }

        #[async_trait::async_trait]
        impl meerkat_core::AgentToolDispatcher for BindAwareDispatcher {
            fn tools(&self) -> Arc<[Arc<meerkat_core::types::ToolDef>]> {
                Vec::<Arc<meerkat_core::types::ToolDef>>::new().into()
            }

            async fn dispatch(
                &self,
                call: meerkat_core::types::ToolCallView<'_>,
            ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
                Err(meerkat_core::ToolError::not_found(call.name))
            }

            fn capabilities(&self) -> meerkat_core::agent::DispatcherCapabilities {
                meerkat_core::agent::DispatcherCapabilities {
                    ops_lifecycle: true,
                }
            }

            fn bind_ops_lifecycle(
                self: Arc<Self>,
                _registry: Arc<dyn meerkat_core::ops_lifecycle::OpsLifecycleRegistry>,
                _owner_bridge_session_id: meerkat_core::types::SessionId,
            ) -> Result<meerkat_core::agent::BindOutcome, meerkat_core::agent::OpsLifecycleBindError>
            {
                self.bound.store(true, Ordering::SeqCst);
                Ok(meerkat_core::agent::BindOutcome::Bound(self))
            }
        }

        let bound = Arc::new(AtomicBool::new(false));
        let dispatcher = Arc::new(AutoWireParentMobToolDispatcher {
            inner: Arc::new(BindAwareDispatcher {
                bound: Arc::clone(&bound),
            }),
            implicit_delegate_retirement_overrides: ImplicitDelegateRetirementOverrides::default(),
            console_spawn_sink: new_console_spawn_sink_slot(),
            identity_runtime: Arc::new(std::sync::RwLock::new(None)),
            protected_mob_id: "test-mob".to_string(),
            spawner_comms_name: None,
        });

        assert!(dispatcher.capabilities().ops_lifecycle);
        let outcome = dispatcher
            .bind_ops_lifecycle(
                Arc::new(meerkat_runtime::ops_lifecycle::RuntimeOpsLifecycleRegistry::new()),
                meerkat_core::types::SessionId::new(),
            )
            .expect("wrapper should delegate ops lifecycle binding");

        assert!(outcome.was_bound());
        assert!(bound.load(Ordering::SeqCst));
        assert!(outcome.into_dispatcher().capabilities().ops_lifecycle);
    }

    #[tokio::test]
    async fn auto_wire_wrapper_preserves_objective_dispatch_context() {
        struct ContextAwareDispatcher {
            observed_objective: Arc<std::sync::Mutex<Option<String>>>,
        }

        #[async_trait::async_trait]
        impl meerkat_core::AgentToolDispatcher for ContextAwareDispatcher {
            fn tools(&self) -> Arc<[Arc<meerkat_core::types::ToolDef>]> {
                Vec::<Arc<meerkat_core::types::ToolDef>>::new().into()
            }

            async fn dispatch(
                &self,
                call: meerkat_core::types::ToolCallView<'_>,
            ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
                Err(meerkat_core::ToolError::execution_failed(format!(
                    "plain dispatch unexpectedly used for {}",
                    call.name
                )))
            }

            async fn dispatch_with_context(
                &self,
                call: meerkat_core::types::ToolCallView<'_>,
                context: &meerkat_core::ToolDispatchContext,
            ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
                *self
                    .observed_objective
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = context
                    .turn_metadata(meerkat_core::agent::TOOL_DISPATCH_OBJECTIVE_ID_KEY)
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                Ok(meerkat_core::ToolDispatchOutcome::sync_result(
                    meerkat_core::types::ToolResult::new(
                        call.id.to_string(),
                        "{}".to_string(),
                        false,
                    ),
                ))
            }
        }

        let observed_objective = Arc::new(std::sync::Mutex::new(None));
        let dispatcher = AutoWireParentMobToolDispatcher {
            inner: Arc::new(ContextAwareDispatcher {
                observed_objective: Arc::clone(&observed_objective),
            }),
            implicit_delegate_retirement_overrides: ImplicitDelegateRetirementOverrides::default(),
            console_spawn_sink: new_console_spawn_sink_slot(),
            identity_runtime: Arc::new(std::sync::RwLock::new(None)),
            protected_mob_id: "test-mob".to_string(),
            spawner_comms_name: None,
        };
        let objective_id = uuid::Uuid::new_v4().to_string();
        let context =
            meerkat_core::ToolDispatchContext::default().with_turn_metadata(BTreeMap::from([(
                meerkat_core::agent::TOOL_DISPATCH_OBJECTIVE_ID_KEY.to_string(),
                Value::String(objective_id.clone()),
            )]));
        let args = serde_json::value::RawValue::from_string(
            serde_json::json!({"task": "review"}).to_string(),
        )
        .expect("raw delegate args");

        meerkat_core::AgentToolDispatcher::dispatch_with_context(
            &dispatcher,
            meerkat_core::types::ToolCallView {
                id: "call-objective",
                name: "delegate",
                args: &args,
            },
            &context,
        )
        .await
        .expect("context-aware delegate dispatch");

        assert_eq!(
            observed_objective
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_deref(),
            Some(objective_id.as_str())
        );
    }

    #[test]
    fn delegate_idle_retire_secs_arg_is_stripped_and_parsed() {
        let mut args = serde_json::json!({
            "task": "inspect",
            "idle_retire_secs": 42
        });

        let parsed = delegate_idle_retire_override_from_args("delegate", &mut args)
            .expect("valid idle retire arg");

        assert_eq!(parsed, Some(DelegateIdleRetireOverride::Seconds(42)));
        assert!(args.get("idle_retire_secs").is_none());
    }

    #[test]
    fn delegate_idle_retire_secs_null_disables_member_retirement() {
        let mut args = serde_json::json!({
            "task": "inspect",
            "idle_retire_secs": null
        });

        let parsed = delegate_idle_retire_override_from_args("delegate", &mut args)
            .expect("valid idle retire arg");

        assert_eq!(parsed, Some(DelegateIdleRetireOverride::Disabled));
        assert!(args.get("idle_retire_secs").is_none());
    }

    #[test]
    fn delegate_idle_retire_secs_omitted_inherits_runtime_default() {
        let mut args = serde_json::json!({"task": "inspect"});

        let parsed = delegate_idle_retire_override_from_args("delegate", &mut args)
            .expect("omitted idle retire arg");

        assert_eq!(parsed, None);
        assert_eq!(args, serde_json::json!({"task": "inspect"}));
    }

    #[test]
    fn delegate_idle_retire_secs_rejects_negative_or_fractional_values() {
        let mut negative = serde_json::json!({"task": "inspect", "idle_retire_secs": -1});
        let mut fractional = serde_json::json!({"task": "inspect", "idle_retire_secs": 1.5});

        assert!(delegate_idle_retire_override_from_args("delegate", &mut negative).is_err());
        assert!(delegate_idle_retire_override_from_args("delegate", &mut fractional).is_err());
    }

    #[test]
    fn mob_spawn_idle_retire_targets_use_args_when_result_omits_mob_id() {
        let args = serde_json::json!({
            "mob_id": "ob3",
            "profile": "review-worker",
            "member_id": "review-worker-vibe-forward",
        });
        let fallback_targets = idle_retire_targets_from_spawn_args(&args);

        assert_eq!(
            fallback_targets,
            vec![IdleRetireTarget {
                mob_id: "ob3".to_string(),
                member_id: "review-worker-vibe-forward".to_string(),
            }]
        );
        assert_eq!(
            idle_retire_targets_from_outcome_text(
                r#"{"agent_identity":"review-worker-vibe-forward","member_ref":"opaque"}"#,
                &fallback_targets,
            ),
            fallback_targets
        );
    }

    #[test]
    fn mob_spawn_idle_retire_targets_support_canonical_specs_shape() {
        let args = serde_json::json!({
            "mob_id": "ob3",
            "specs": [
                {"profile": "person-worker", "agent_identity": "person-worker-a"},
                {"profile": "person-worker", "member_id": "person-worker-b", "mob_id": "other"}
            ]
        });
        let fallback_targets = idle_retire_targets_from_spawn_args(&args);

        assert_eq!(
            fallback_targets,
            vec![
                IdleRetireTarget {
                    mob_id: "ob3".to_string(),
                    member_id: "person-worker-a".to_string(),
                },
                IdleRetireTarget {
                    mob_id: "other".to_string(),
                    member_id: "person-worker-b".to_string(),
                },
            ]
        );
        assert_eq!(
            idle_retire_targets_from_outcome_text(
                r#"{"members":[{"agent_identity":"person-worker-a"},{"agent_identity":"person-worker-b","mob_id":"other"}]}"#,
                &fallback_targets,
            ),
            fallback_targets
        );
    }

    #[tokio::test]
    async fn implicit_delegate_retirement_overrides_round_trip_per_member() {
        let overrides = ImplicitDelegateRetirementOverrides::default();

        overrides
            .set("mob-a", "worker-1", DelegateIdleRetireOverride::Seconds(12))
            .await;
        overrides
            .set("mob-a", "worker-2", DelegateIdleRetireOverride::Disabled)
            .await;

        assert_eq!(
            overrides.get("mob-a", "worker-1").await,
            Some(DelegateIdleRetireOverride::Seconds(12))
        );
        assert_eq!(
            overrides.get("mob-a", "worker-2").await,
            Some(DelegateIdleRetireOverride::Disabled)
        );
        assert_eq!(overrides.get("mob-a", "worker-3").await, None);
    }

    #[tokio::test]
    async fn mob_spawn_idle_retire_registration_uses_spawn_args_when_result_omits_mob_id() {
        let overrides = ImplicitDelegateRetirementOverrides::default();
        let dispatcher = wrapper_with_overrides(overrides.clone());
        let fallback_targets = idle_retire_targets_from_spawn_args(&serde_json::json!({
            "mob_id": "ob3",
            "member_id": "review-worker-vibe-forward",
        }));
        let outcome =
            meerkat_core::ToolDispatchOutcome::sync_result(meerkat_core::types::ToolResult::new(
                "spawn-1".to_string(),
                r#"{"agent_identity":"review-worker-vibe-forward","member_ref":"opaque"}"#
                    .to_string(),
                false,
            ));

        dispatcher
            .register_idle_retire_override_from_outcome(
                &outcome,
                Some(DelegateIdleRetireOverride::Seconds(900)),
                &fallback_targets,
            )
            .await;

        assert_eq!(
            overrides.get("ob3", "review-worker-vibe-forward").await,
            Some(DelegateIdleRetireOverride::Seconds(900))
        );
    }

    #[test]
    fn image_generation_substrate_defaults_off_for_inline_profiles() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "test"

[profiles.worker]
model = "gpt-5.5"

[profiles.worker.tools]
builtins = true
"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));

        assert!(
            !mob_definition_may_use_image_generation(&definition),
            "inline profiles should not wire the image substrate unless a profile opts in"
        );
    }

    #[test]
    fn image_generation_substrate_follows_profile_tool_config() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "test"

[profiles.commander]
model = "gpt-5.5"

[profiles.commander.tools]
builtins = true
image_generation = true

[profiles.investigator]
model = "gpt-5.5"

[profiles.investigator.tools]
builtins = true
image_generation = false
"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));

        let commander = definition.profiles["commander"].as_inline().unwrap();
        let investigator = definition.profiles["investigator"].as_inline().unwrap();
        assert!(commander.tools.image_generation);
        assert!(!investigator.tools.image_generation);
        assert!(
            mob_definition_may_use_image_generation(&definition),
            "one opt-in profile is enough to wire substrate; Meerkat gates visibility per profile"
        );
    }

    #[test]
    fn shell_substrate_defaults_off_for_inline_profiles() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "test"

[profiles.worker]
model = "gpt-5.5"

[profiles.worker.tools]
builtins = true
"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));

        assert!(
            !mob_definition_may_use_shell(&definition),
            "inline profiles should not wire the shell substrate unless a profile opts in"
        );
    }

    #[test]
    fn shell_substrate_follows_profile_tool_config() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "test"

[profiles.domain]
model = "gpt-5.5"

[profiles.domain.tools]
builtins = true
shell = false

[profiles.security]
model = "gpt-5.5"

[profiles.security.tools]
builtins = true
shell = true
"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));

        let domain = definition.profiles["domain"].as_inline().unwrap();
        let security = definition.profiles["security"].as_inline().unwrap();
        assert!(!domain.tools.shell);
        assert!(security.tools.shell);
        assert!(
            mob_definition_may_use_shell(&definition),
            "one opt-in profile is enough to wire substrate; Meerkat gates visibility per profile"
        );
    }

    #[test]
    fn shell_tooling_forces_builtin_substrate_without_exposing_broad_builtins() {
        let mut req = CreateSessionRequest {
            model: "gpt-5.5".to_string(),
            prompt: meerkat_core::ContentInput::Text("test".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: Some(meerkat_core::service::SessionBuildOptions {
                override_builtins: meerkat_core::ToolCategoryOverride::Disable,
                override_shell: meerkat_core::ToolCategoryOverride::Enable,
                ..Default::default()
            }),
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            injected_context: Vec::new(),
        };

        ensure_shell_tooling_build_substrate(&mut req);

        let build = req.build.expect("build options");
        assert_eq!(
            build.override_shell,
            meerkat_core::ToolCategoryOverride::Enable
        );
        assert_eq!(
            build.override_builtins,
            meerkat_core::ToolCategoryOverride::Enable,
            "shell-only profiles must still enable Meerkat's builtin substrate"
        );
        let allow = match build.initial_tool_filter.expect("shell visibility filter") {
            meerkat_core::ToolFilter::Allow(allow) => allow,
            other => panic!("expected shell/comms allow filter, got {other:?}"),
        };
        for tool in SHELL_BUILTIN_TOOL_NAMES
            .iter()
            .chain(COMMS_TOOL_NAMES.iter())
        {
            assert!(allow.contains(tool), "missing expected tool {tool}");
        }
        for broad_builtin in ["task_list", "task_create", "apply_patch", "browse_skills"] {
            assert!(
                !allow.contains(broad_builtin),
                "shell-only filter must not expose broad builtin {broad_builtin}",
            );
        }
    }

    #[test]
    fn non_shell_profiles_keep_builtin_override_unchanged() {
        let mut req = CreateSessionRequest {
            model: "gpt-5.5".to_string(),
            prompt: meerkat_core::ContentInput::Text("test".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: Some(meerkat_core::service::SessionBuildOptions {
                override_builtins: meerkat_core::ToolCategoryOverride::Disable,
                override_shell: meerkat_core::ToolCategoryOverride::Disable,
                ..Default::default()
            }),
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            injected_context: Vec::new(),
        };

        ensure_shell_tooling_build_substrate(&mut req);

        let build = req.build.expect("build options");
        assert_eq!(
            build.override_builtins,
            meerkat_core::ToolCategoryOverride::Disable
        );
        assert_eq!(
            build.override_shell,
            meerkat_core::ToolCategoryOverride::Disable
        );
    }

    #[test]
    fn image_generation_profiles_can_disable_builtins_with_meerkat_062() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "test"

[profiles.commander]
model = "gpt-5.5"

[profiles.commander.tools]
builtins = false
image_generation = true
"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));

        let commander = definition.profiles["commander"].as_inline().unwrap();
        assert!(!commander.tools.builtins);
        assert!(commander.tools.image_generation);
        assert!(
            mob_definition_may_use_image_generation(&definition),
            "image generation now has its own Meerkat tool gate"
        );
    }

    #[test]
    fn image_generation_substrate_is_conservative_for_realm_profile_refs() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "test"

[profiles.worker]
realm_profile = "worker-v2"
"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));

        assert!(
            mob_definition_may_use_image_generation(&definition),
            "realm profiles resolve at spawn time, so MobKit wires substrate and lets Meerkat enforce profile policy"
        );
    }

    #[test]
    fn sanitize_llm_request_drops_replay_unsafe_server_tool_blocks() {
        let request = meerkat_client::LlmRequest::new(
            "gpt-5.5",
            vec![meerkat_core::Message::BlockAssistant(
                meerkat_core::BlockAssistantMessage::new(
                    vec![
                        meerkat_core::AssistantBlock::Text {
                            text: "done".to_string(),
                            meta: None,
                        },
                        meerkat_core::AssistantBlock::ServerToolContent {
                            id: Some("ws-stream".to_string()),
                            kind: meerkat_core::ServerToolKind::WebSearch,
                            content: serde_json::json!({
                                "type": "response.web_search_call.searching",
                                "item_id": "ws_123"
                            }),
                            meta: None,
                        },
                        meerkat_core::AssistantBlock::ServerToolContent {
                            id: Some("ws_123".to_string()),
                            kind: meerkat_core::ServerToolKind::ProviderNative {
                                name: "web_search_call".to_string(),
                            },
                            content: serde_json::json!({
                                "type": "web_search_call",
                                "id": "ws_123",
                                "status": "completed"
                            }),
                            meta: None,
                        },
                        meerkat_core::AssistantBlock::ServerToolContent {
                            id: None,
                            kind: meerkat_core::ServerToolKind::ProviderNative {
                                name: "web_search_annotations".to_string(),
                            },
                            content: serde_json::json!({
                                "type": "message_annotations",
                                "annotations": []
                            }),
                            meta: None,
                        },
                    ],
                    meerkat_core::StopReason::EndTurn,
                ),
            )],
        );

        let sanitized = sanitize_llm_request_for_stateless_replay(&request);
        let meerkat_core::Message::BlockAssistant(assistant) = &sanitized.messages[0] else {
            panic!("expected block assistant");
        };

        assert_eq!(assistant.blocks.len(), 2);
        assert!(matches!(
            assistant.blocks[0],
            meerkat_core::AssistantBlock::Text { .. }
        ));
        assert!(matches!(
            assistant.blocks[1],
            meerkat_core::AssistantBlock::ServerToolContent { ref kind, .. }
                if kind.provider_name() == "web_search_call"
        ));
    }

    #[test]
    fn sanitize_llm_request_preserves_generated_images_for_meerkat_062() {
        let request = meerkat_client::LlmRequest::new(
            "gpt-5.5",
            vec![meerkat_core::Message::BlockAssistant(
                meerkat_core::BlockAssistantMessage::new(
                    vec![
                        meerkat_core::AssistantBlock::Text {
                            text: "visible".to_string(),
                            meta: None,
                        },
                        generated_image_block_for_test(),
                    ],
                    meerkat_core::StopReason::EndTurn,
                ),
            )],
        );

        let sanitized = sanitize_llm_request_for_stateless_replay(&request);

        let meerkat_core::Message::BlockAssistant(original_assistant) = &request.messages[0] else {
            panic!("expected original block assistant");
        };
        assert!(
            original_assistant
                .blocks
                .iter()
                .any(|block| matches!(block, meerkat_core::AssistantBlock::Image { .. })),
            "request-view sanitization must not rewrite canonical caller-owned messages"
        );

        let meerkat_core::Message::BlockAssistant(sanitized_assistant) = &sanitized.messages[0]
        else {
            panic!("expected sanitized block assistant");
        };
        assert!(
            sanitized_assistant
                .blocks
                .iter()
                .any(|block| matches!(block, meerkat_core::AssistantBlock::Image { .. })),
            "Meerkat 0.6.2 owns provider replay projection for generated images"
        );
    }

    #[derive(Default)]
    struct CapturingLlmClient {
        projected_messages: std::sync::Mutex<Vec<meerkat_core::Message>>,
    }

    #[async_trait]
    impl LlmClient for CapturingLlmClient {
        fn project_replay_messages(
            &self,
            messages: &[meerkat_core::Message],
        ) -> Result<Vec<meerkat_core::Message>, meerkat_client::LlmError> {
            *self
                .projected_messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = messages.to_vec();
            Ok(messages.to_vec())
        }

        fn stream<'a>(&'a self, _request: &'a LlmRequest) -> LlmStream<'a> {
            Box::pin(futures::stream::iter([Ok(
                meerkat_client::LlmEvent::Done {
                    outcome: meerkat_client::LlmDoneOutcome::Success {
                        stop_reason: meerkat_core::StopReason::EndTurn,
                    },
                },
            )]))
        }

        fn provider(&self) -> meerkat_core::Provider {
            meerkat_core::Provider::OpenAI
        }

        async fn health_check(&self) -> Result<(), meerkat_client::LlmError> {
            Ok(())
        }
    }

    #[test]
    fn replay_sanitizing_llm_client_delegates_provider_projection() {
        let capture = Arc::new(CapturingLlmClient::default());
        let inner: Arc<dyn LlmClient> = capture.clone();
        let wrapped = ReplaySanitizingLlmClient::new(inner);
        let messages = vec![meerkat_core::Message::BlockAssistant(
            meerkat_core::BlockAssistantMessage::new(
                vec![
                    meerkat_core::AssistantBlock::Text {
                        text: "visible".to_string(),
                        meta: None,
                    },
                    meerkat_core::AssistantBlock::ServerToolContent {
                        id: Some("ws-stream".to_string()),
                        kind: meerkat_core::ServerToolKind::WebSearch,
                        content: serde_json::json!({
                            "type": "response.web_search_call.searching",
                            "item_id": "ws_123"
                        }),
                        meta: None,
                    },
                ],
                meerkat_core::StopReason::EndTurn,
            ),
        )];

        let projected = wrapped
            .project_replay_messages(&messages)
            .expect("wrapped client should delegate provider projection");

        let seen = capture
            .projected_messages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let meerkat_core::Message::BlockAssistant(assistant) = &seen[0] else {
            panic!("expected block assistant");
        };
        assert_eq!(
            assistant.blocks.len(),
            1,
            "MobKit sanitization must happen before Meerkat provider projection"
        );
        assert!(matches!(
            assistant.blocks[0],
            meerkat_core::AssistantBlock::Text { .. }
        ));
        assert_eq!(
            serde_json::to_value(&projected).expect("projected messages serialize"),
            serde_json::to_value(&seen).expect("seen messages serialize")
        );
    }

    #[derive(Default)]
    struct CapturingAgentLlmClient {
        seen_messages: std::sync::Mutex<Vec<meerkat_core::Message>>,
        fallback_prepare_calls: std::sync::atomic::AtomicUsize,
        fallback_commit_calls: std::sync::atomic::AtomicUsize,
        fallback_schema_calls: std::sync::atomic::AtomicUsize,
        stream_observation_starts: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl meerkat_core::AgentLlmClient for CapturingAgentLlmClient {
        async fn stream_response(
            &self,
            messages: &[meerkat_core::Message],
            _tools: &[Arc<meerkat_core::ToolDef>],
            _max_tokens: u32,
            _temperature: Option<f32>,
            _provider_params: Option<
                &meerkat_core::lifecycle::run_primitive::ProviderParamsOverride,
            >,
        ) -> Result<meerkat_core::agent::LlmStreamResult, meerkat_core::AgentError> {
            *self
                .seen_messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = messages.to_vec();
            Ok(meerkat_core::agent::LlmStreamResult::new(
                Vec::new(),
                meerkat_core::StopReason::EndTurn,
                meerkat_core::Usage::default(),
            ))
        }

        fn provider(&self) -> meerkat_core::Provider {
            meerkat_core::Provider::OpenAI
        }

        fn model(&self) -> &'static str {
            "gpt-5.5"
        }

        fn prepare_model_fallback(
            &self,
            _failure: &meerkat_core::AgentError,
        ) -> Option<meerkat_core::agent::AgentLlmFallbackSwitch> {
            self.fallback_prepare_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            None
        }

        fn commit_model_fallback(
            &self,
            _previous_identity: &meerkat_core::SessionLlmIdentity,
            _target_identity: &meerkat_core::SessionLlmIdentity,
        ) -> Result<(), meerkat_core::AgentError> {
            self.fallback_commit_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        fn active_model_fallback_identity(&self) -> Option<meerkat_core::SessionLlmIdentity> {
            Some(test_session_llm_identity("fallback-model"))
        }

        fn compile_model_fallback_schema(
            &self,
            _target_identity: &meerkat_core::SessionLlmIdentity,
            _output_schema: &meerkat_core::OutputSchema,
        ) -> Result<meerkat_core::schema::CompiledSchema, meerkat_core::AgentError> {
            self.fallback_schema_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(meerkat_core::AgentError::ConfigError(
                "fallback schema probe".to_string(),
            ))
        }

        fn begin_stream_output_observation(&self) {
            self.stream_observation_starts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        fn stream_output_observed(&self) -> bool {
            true
        }
    }

    fn test_session_llm_identity(model: &str) -> meerkat_core::SessionLlmIdentity {
        meerkat_core::SessionLlmIdentity {
            model: model.to_string(),
            provider: meerkat_core::Provider::OpenAI,
            self_hosted_server_id: None,
            provider_params: None,
            auth_binding: None,
        }
    }

    #[test]
    fn sanitize_agent_llm_client_forwards_fallback_and_stream_observation_state() {
        let capture = Arc::new(CapturingAgentLlmClient::default());
        let inner: Arc<dyn meerkat_core::AgentLlmClient> = capture.clone();
        let wrapped = ReplaySanitizingAgentLlmClient::new(inner);
        let previous = test_session_llm_identity("primary-model");
        let target = test_session_llm_identity("fallback-model");

        assert!(
            meerkat_core::AgentLlmClient::prepare_model_fallback(
                &wrapped,
                &meerkat_core::AgentError::ConfigError("probe".to_string()),
            )
            .is_none()
        );
        meerkat_core::AgentLlmClient::commit_model_fallback(&wrapped, &previous, &target)
            .expect("fallback activation should forward");
        assert_eq!(
            meerkat_core::AgentLlmClient::active_model_fallback_identity(&wrapped)
                .expect("active fallback identity should forward")
                .model,
            "fallback-model"
        );
        let schema = meerkat_core::OutputSchema::new(serde_json::json!({"type": "object"}))
            .expect("valid test schema");
        let error =
            meerkat_core::AgentLlmClient::compile_model_fallback_schema(&wrapped, &target, &schema)
                .expect_err("fallback schema probe error should forward");
        assert!(error.to_string().contains("fallback schema probe"));
        meerkat_core::AgentLlmClient::begin_stream_output_observation(&wrapped);
        assert!(meerkat_core::AgentLlmClient::stream_output_observed(
            &wrapped
        ));

        assert_eq!(
            capture
                .fallback_prepare_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            capture
                .fallback_commit_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            capture
                .fallback_schema_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            capture
                .stream_observation_starts
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn sanitize_agent_llm_client_drops_replay_unsafe_server_tool_blocks() {
        let capture = Arc::new(CapturingAgentLlmClient::default());
        let inner: Arc<dyn meerkat_core::AgentLlmClient> = capture.clone();
        let wrapped = ReplaySanitizingAgentLlmClient::wrap(inner);
        let messages = vec![meerkat_core::Message::BlockAssistant(
            meerkat_core::BlockAssistantMessage::new(
                vec![
                    meerkat_core::AssistantBlock::Text {
                        text: "visible".to_string(),
                        meta: None,
                    },
                    meerkat_core::AssistantBlock::ServerToolContent {
                        id: Some("ws-stream".to_string()),
                        kind: meerkat_core::ServerToolKind::WebSearch,
                        content: serde_json::json!({
                            "type": "response.web_search_call.searching",
                            "item_id": "ws_123"
                        }),
                        meta: None,
                    },
                    meerkat_core::AssistantBlock::ServerToolContent {
                        id: Some("ok".to_string()),
                        kind: meerkat_core::ServerToolKind::ProviderNative {
                            name: "web_search_call".to_string(),
                        },
                        content: serde_json::json!({
                            "type": "web_search_call",
                            "id": "ws_123",
                            "status": "completed"
                        }),
                        meta: None,
                    },
                ],
                meerkat_core::StopReason::EndTurn,
            ),
        )];
        let tools: Vec<Arc<meerkat_core::ToolDef>> = Vec::new();

        wrapped
            .stream_response(&messages, &tools, 512, None, None)
            .await
            .expect("wrapped client should delegate");

        let seen = capture
            .seen_messages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let meerkat_core::Message::BlockAssistant(assistant) = &seen[0] else {
            panic!("expected block assistant");
        };
        assert_eq!(assistant.blocks.len(), 2);
        assert!(matches!(
            assistant.blocks[0],
            meerkat_core::AssistantBlock::Text { .. }
        ));
        assert!(matches!(
            assistant.blocks[1],
            meerkat_core::AssistantBlock::ServerToolContent { ref kind, .. }
                if kind.provider_name() == "web_search_call"
        ));
    }

    fn generated_image_block_for_test() -> meerkat_core::AssistantBlock {
        serde_json::from_value(serde_json::json!({
            "block_type": "image",
            "data": {
                "image_id": "00000000-0000-0000-0000-000000000051",
                "blob_ref": {
                    "blob_id": "sha256:test-generated-image",
                    "media_type": "image/png"
                },
                "media_type": "image/png",
                "width": 1024,
                "height": 1024,
                "revised_prompt": { "disposition": "not_requested" },
                "meta": { "provider": "not_emitted" }
            }
        }))
        .expect("test image block should deserialize")
    }

    #[test]
    fn sanitize_message_preserves_assistant_image_blocks() {
        let message =
            meerkat_core::Message::BlockAssistant(meerkat_core::BlockAssistantMessage::new(
                vec![
                    meerkat_core::AssistantBlock::Text {
                        text: "Here is the image.".to_string(),
                        meta: None,
                    },
                    generated_image_block_for_test(),
                ],
                meerkat_core::StopReason::EndTurn,
            ));

        let sanitized = sanitize_message_for_stateless_replay(message);
        let meerkat_core::Message::BlockAssistant(assistant) = sanitized else {
            panic!("expected block assistant");
        };

        assert_eq!(assistant.blocks.len(), 2);
        assert!(matches!(
            assistant.blocks[0],
            meerkat_core::AssistantBlock::Text { .. }
        ));
        assert!(
            matches!(
                assistant.blocks[1],
                meerkat_core::AssistantBlock::Image { .. }
            ),
            "generated image blocks should reach Meerkat's provider projection"
        );
    }

    /// Verify that persistent_with_hook wraps the session service with
    /// PreBuildMobSessionService (hook is Some).
    #[test]
    fn persistent_with_hook_wraps_session_service() {
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

        let hook_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hook_called_clone = hook_called.clone();

        let spec = MobBootstrapSpec::persistent_with_hook(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path.clone(),
            4,
            session_store,
            move |_req: &mut CreateSessionRequest| {
                hook_called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                Box::pin(async { Ok(()) })
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));

        // The session service is wired with a SqliteRuntimeStore so that
        // both `load_persisted_session` (resume) and
        // `load_persisted_session_for_control` (archive/retire) succeed
        // across process restart. spec.runtime_adapter is also set
        // explicitly so the bootstrap path uses the same store. See
        // `persistent_bootstrap_uses_sqlite_runtime_store` for the full
        // regression coverage.
        assert!(
            spec.runtime_adapter.is_some(),
            "persistent_with_hook must provide a runtime adapter via spec.runtime_adapter"
        );
        assert!(
            spec.session_service.runtime_adapter().is_some(),
            "session service must own a runtime_store so archive/retire don't \
             hit the store-only-projection rejection in meerkat-session"
        );
        assert!(
            store_path.join("runtime.sqlite").exists(),
            "persistent_inner must open a SqliteRuntimeStore at <store_path>/runtime.sqlite"
        );

        // The hook isn't called until create_session — verify the wrapper exists
        // by checking the service is not the raw PersistentSessionService (it
        // wraps it). We can't call create_session without a full LLM stack, but
        // we can verify the hook_called flag is false (not prematurely invoked).
        assert!(
            !hook_called.load(std::sync::atomic::Ordering::Relaxed),
            "hook must not be called before create_session"
        );
    }

    /// Verify that ephemeral_with_hook accepts and stores a hook.
    #[test]
    fn ephemeral_with_hook_creates_spec() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let Ok(definition) = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"test\"\n") else {
            panic!("failed to parse minimal mob definition");
        };

        let hook_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hook_called_clone = hook_called.clone();

        let spec = MobBootstrapSpec::ephemeral_with_hook(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            None,
            move |_req: &mut CreateSessionRequest| {
                hook_called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                Box::pin(async { Ok(()) })
            },
        );

        let runtime_adapter = spec
            .runtime_adapter
            .as_ref()
            .unwrap_or_else(|| panic!("ephemeral_with_hook must retain its runtime adapter"));
        assert!(
            runtime_adapter.has_session_llm_reconfigure_host(),
            "ephemeral_with_hook must install the live LLM reconfiguration host"
        );

        // Hook not yet called.
        assert!(
            !hook_called.load(std::sync::atomic::Ordering::Relaxed),
            "hook must not be called before create_session"
        );
    }

    /// Verify that PreBuildMobSessionService applies the hook to the request
    /// in create_session. The hook mutates the model and adds labels; we
    /// verify by capturing the state inside the hook itself.
    #[tokio::test]
    async fn pre_build_hook_mutates_create_session_request() {
        use std::sync::Mutex;

        let captured = Arc::new(Mutex::new(None::<(String, Option<String>)>));
        let captured_clone = captured.clone();

        // Build a minimal ephemeral service as the inner.
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let factory = AgentFactory::new(dir.path()).builtins(true);
        let config = Config::default();
        let builder = FactoryAgentBuilder::new(factory, config);
        let inner: Arc<dyn MobSessionService> =
            Arc::new(meerkat_session::EphemeralSessionService::new(builder, 4));

        // Hook that mutates and captures the post-mutation state.
        let hook: PreBuildHook = Arc::new(move |req: &mut CreateSessionRequest| {
            req.model = "hooked-model".to_string();
            req.system_prompt =
                meerkat_core::config::SystemPromptOverride::Set("injected-prompt".to_string());
            let labels = req.labels.get_or_insert_with(Default::default);
            labels.insert("hook_label".to_string(), "hook_value".to_string());
            // Capture to prove the hook ran and mutated the request.
            let mut lock = captured_clone
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *lock = Some((
                req.model.clone(),
                req.system_prompt.as_set_prompt().map(ToString::to_string),
            ));
            Box::pin(async { Ok(()) })
        });
        let wrapped = PreBuildMobSessionService {
            inner,
            hook,
            dispatch_taint: None,
            after_create_hook: None,
            runtime_adapter_override: None,
            session_read_absorber: None,
            archived_terminal_authority: None,
        };

        let req = CreateSessionRequest {
            model: "original-model".to_string(),
            prompt: meerkat_core::ContentInput::Text("test".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            injected_context: Vec::new(),
        };

        // create_session will fail (no LLM) but the hook runs first.
        let _ = meerkat_core::service::SessionService::create_session(&wrapped, req).await;

        let (model, prompt) = captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("hook must have been called");
        assert_eq!(model, "hooked-model", "hook must mutate the model");
        assert_eq!(
            prompt.as_deref(),
            Some("injected-prompt"),
            "hook must set the system prompt"
        );
    }

    /// Regression for meerkat 0.7.2 fix #1: a custom `Config.retry` must reach
    /// the agent's effective `RetryPolicy` through MobKit's session-service build
    /// path, instead of silently falling back to the 3-retry / 30s default.
    ///
    /// We build through the same `FactoryAgentBuilder` MobKit's session service
    /// uses (`build_*_session_service` -> `FactoryAgentBuilder::new(factory,
    /// config)`), then read the effective policy back through the public
    /// `Agent::retry_policy()` accessor. The stub `LlmClient` exists only so the
    /// offline `build_agent` succeeds; no turn is run and no provider behavior is
    /// faked. The assertion targets the *plumbed config value*, not retry timing.
    #[tokio::test]
    async fn config_retry_reaches_agent_effective_retry_policy() {
        use meerkat_session::SessionAgentBuilder as _;

        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let factory = AgentFactory::new(dir.path()).builtins(true);

        // Non-default retry config: max_retries away from the canonical 3.
        let mut config = Config::default();
        assert_ne!(
            config.retry.max_retries, 11,
            "test sentinel must differ from default"
        );
        config.retry.max_retries = 11;
        config.retry.initial_delay = std::time::Duration::from_millis(125);
        config.retry.max_delay = std::time::Duration::from_secs(7);
        config.retry.multiplier = 3.5;

        let mut builder = FactoryAgentBuilder::new(factory, config);
        // Build-only stub so the offline build_agent succeeds; never run.
        builder.default_llm_client = Some(Arc::new(CapturingLlmClient::default()));

        let req = CreateSessionRequest {
            model: "mock-model".to_string(),
            prompt: meerkat_core::ContentInput::Text("noop".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::Discard,
            injected_context: Vec::new(),
        };

        let (event_tx, _event_rx) = tokio::sync::mpsc::channel(8);
        let factory_agent = builder
            .build_agent(&req, event_tx)
            .await
            .unwrap_or_else(|e| panic!("build_agent should succeed offline: {e}"));

        let effective = factory_agent.agent().retry_policy();
        assert_eq!(
            effective.max_retries, 11,
            "Config.retry.max_retries must be plumbed into the agent's effective \
             RetryPolicy, not the default 3"
        );
        assert_eq!(
            effective.initial_delay,
            std::time::Duration::from_millis(125),
            "Config.retry.initial_delay must be plumbed, not the 500ms default"
        );
        assert_eq!(
            effective.max_delay,
            std::time::Duration::from_secs(7),
            "Config.retry.max_delay must be plumbed, not the 30s default"
        );
        assert!(
            (effective.multiplier - 3.5).abs() < f64::EPSILON,
            "Config.retry.multiplier must be plumbed, not the 2.0 default"
        );
    }

    /// Inner-service stand-in for the absorber seam: `load_persisted_session`
    /// returns one fixed document and counts reads; everything else is inert.
    struct AbsorberInnerProbe {
        session: meerkat_core::session::Session,
        loads: std::sync::atomic::AtomicU64,
    }

    #[async_trait]
    impl meerkat_core::service::SessionService for AbsorberInnerProbe {
        async fn create_session(
            &self,
            _req: CreateSessionRequest,
        ) -> Result<meerkat_core::types::RunResult, SessionError> {
            Err(SessionError::Unsupported("create_session".to_string()))
        }

        async fn start_turn(
            &self,
            _id: &meerkat_core::types::SessionId,
            _req: meerkat_core::service::StartTurnRequest,
        ) -> Result<meerkat_core::types::RunResult, SessionError> {
            Err(SessionError::Unsupported("start_turn".to_string()))
        }

        async fn interrupt(
            &self,
            _id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            Ok(())
        }

        async fn read(
            &self,
            id: &meerkat_core::types::SessionId,
        ) -> Result<meerkat_core::service::SessionView, SessionError> {
            Err(SessionError::NotFound { id: id.clone() })
        }

        async fn list(
            &self,
            _query: meerkat_core::service::SessionQuery,
        ) -> Result<Vec<meerkat_core::service::SessionSummary>, SessionError> {
            Ok(Vec::new())
        }

        async fn archive(&self, _id: &meerkat_core::types::SessionId) -> Result<(), SessionError> {
            Ok(())
        }
    }

    #[async_trait]
    impl meerkat_core::service::SessionServiceCommsExt for AbsorberInnerProbe {}

    #[async_trait]
    impl meerkat_core::service::SessionServiceControlExt for AbsorberInnerProbe {
        async fn append_system_context(
            &self,
            _id: &meerkat_core::types::SessionId,
            _req: meerkat_core::service::AppendSystemContextRequest,
        ) -> Result<
            meerkat_core::service::AppendSystemContextResult,
            meerkat_core::service::SessionControlError,
        > {
            Err(SessionError::Unsupported("append_system_context".to_string()).into())
        }

        async fn stage_tool_results(
            &self,
            _id: &meerkat_core::types::SessionId,
            _req: meerkat_core::service::StageToolResultsRequest,
        ) -> Result<meerkat_core::service::StageToolResultsResult, SessionError> {
            Err(SessionError::Unsupported("stage_tool_results".to_string()))
        }
    }

    #[async_trait]
    impl meerkat_core::service::SessionServiceHistoryExt for AbsorberInnerProbe {
        async fn read_history(
            &self,
            id: &meerkat_core::types::SessionId,
            _query: meerkat_core::service::SessionHistoryQuery,
        ) -> Result<meerkat_core::service::SessionHistoryPage, SessionError> {
            Err(SessionError::NotFound { id: id.clone() })
        }
    }

    #[async_trait]
    impl MobSessionService for AbsorberInnerProbe {
        async fn prepare_session_for_resume(
            &self,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            Ok(())
        }
        async fn acknowledge_committed_runtime_session_boundary_under_turn_finalization_boundary(
            &self,
            _session_id: &meerkat_core::types::SessionId,
            _authority: &meerkat_core::CommittedSessionBoundaryAuthority,
        ) -> Result<(), SessionError> {
            Err(SessionError::Unsupported(
                "test double does not acknowledge store-owned runtime boundaries".to_string(),
            ))
        }
        async fn load_session_for_resume(
            &self,
            session_id: &meerkat_core::types::SessionId,
        ) -> Result<meerkat_mob::ResumeSessionLoad, SessionError> {
            // Truthful derived answer: this double's resume visibility IS its
            // typed reads' visibility (the meerkat-mob test-double idiom).
            if let Some(session) = self.load_persisted_session(session_id).await? {
                return Ok(meerkat_mob::ResumeSessionLoad::Active(Box::new(session)));
            }
            if let Some(session) = self.load_revivable_retired_session(session_id).await? {
                return Ok(meerkat_mob::ResumeSessionLoad::Revivable(Box::new(session)));
            }
            Ok(meerkat_mob::ResumeSessionLoad::Absent)
        }

        async fn create_session_under_runtime_turn_boundary(
            &self,
            req: CreateSessionRequest,
        ) -> Result<meerkat_core::types::RunResult, SessionError> {
            meerkat_core::SessionService::create_session(self, req).await
        }

        async fn archive_with_mob_lifecycle_authority_under_runtime_turn_boundary(
            &self,
            session_id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            meerkat_core::SessionService::archive(self, session_id).await
        }

        async fn discard_live_session_under_runtime_turn_boundary(
            &self,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            Ok(())
        }

        async fn load_persisted_session(
            &self,
            session_id: &meerkat_core::types::SessionId,
        ) -> Result<Option<meerkat_core::session::Session>, SessionError> {
            self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if session_id == self.session.id() {
                Ok(Some(self.session.clone()))
            } else {
                Ok(None)
            }
        }
    }

    /// The idle-cadence structural gate at the mobkit seam: repeated
    /// authoritative reads of an UNCHANGED session document must reach the
    /// inner (PersistentSessionService-shaped) service exactly once, and a
    /// session-scoped durable write through the runtime-store facade must
    /// invalidate the absorbed copy.
    #[tokio::test]
    async fn session_read_absorber_serves_unchanged_documents_until_a_write_epoch_advances() {
        let session =
            meerkat_core::session::Session::with_id(meerkat_core::types::SessionId::new());
        let session_id = session.id().clone();
        let probe = Arc::new(AbsorberInnerProbe {
            session,
            loads: std::sync::atomic::AtomicU64::new(0),
        });
        let epochs = Arc::new(SessionSnapshotWriteEpochs::default());
        let wrapped = PreBuildMobSessionService {
            inner: probe.clone(),
            hook: no_op_pre_build_hook(),
            dispatch_taint: None,
            after_create_hook: None,
            runtime_adapter_override: None,
            session_read_absorber: Some(Arc::new(SessionDocumentReadAbsorber::new(Arc::clone(
                &epochs,
            )))),
            archived_terminal_authority: None,
        };

        // A converged idle window issues many reads; only the first may reach
        // the inner service.
        for _ in 0..5 {
            let loaded = MobSessionService::load_persisted_session(&wrapped, &session_id)
                .await
                .expect("absorbed load");
            assert_eq!(loaded.expect("absorbed document present").id(), &session_id);
        }
        assert_eq!(
            probe.loads.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "unchanged-session reads must be absorbed after the first inner load"
        );

        // Any session-scoped durable write through the epoch-observing
        // runtime-store facade invalidates the absorbed copy.
        let facade = SessionStoreBackedRuntimeStore::with_write_epochs(
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new()),
            Arc::clone(&epochs),
        );
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(&session_id);
        meerkat_runtime::RuntimeStore::clear_session_snapshot(&facade, &runtime_id)
            .await
            .expect("facade-observed session-scoped write");

        let reloaded = MobSessionService::load_persisted_session(&wrapped, &session_id)
            .await
            .expect("post-write load");
        assert!(reloaded.is_some());
        assert_eq!(
            probe.loads.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "a session-scoped write must force the next read back to the inner service"
        );

        // Absence evicts: a missing document must never be served from cache.
        let other_id = meerkat_core::types::SessionId::new();
        for _ in 0..2 {
            assert!(
                MobSessionService::load_persisted_session(&wrapped, &other_id)
                    .await
                    .expect("absent load")
                    .is_none()
            );
        }
        assert_eq!(
            probe.loads.load(std::sync::atomic::Ordering::SeqCst),
            4,
            "absent documents are not cached"
        );
    }

    #[derive(Default)]
    struct ForwardingProbe {
        calls: Mutex<Vec<&'static str>>,
        cancel_outcome: std::sync::atomic::AtomicU8,
    }

    impl ForwardingProbe {
        fn record(&self, call: &'static str) {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(call);
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        fn set_cancel_outcome(&self, outcome: u8) {
            self.cancel_outcome
                .store(outcome, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[async_trait]
    impl meerkat_core::service::SessionService for ForwardingProbe {
        async fn create_session(
            &self,
            _req: CreateSessionRequest,
        ) -> Result<meerkat_core::types::RunResult, SessionError> {
            Err(SessionError::Unsupported("create_session".to_string()))
        }

        async fn start_turn(
            &self,
            _id: &meerkat_core::types::SessionId,
            _req: meerkat_core::service::StartTurnRequest,
        ) -> Result<meerkat_core::types::RunResult, SessionError> {
            Err(SessionError::Unsupported("start_turn".to_string()))
        }

        async fn reconcile_runtime_compaction_projections(
            &self,
            _id: &meerkat_core::types::SessionId,
            _intents: Vec<meerkat_core::CompactionProjectionIntent>,
        ) -> Result<(), SessionError> {
            self.record("reconcile_runtime_compaction_projections");
            Ok(())
        }

        async fn abort_uncommitted_compaction_projections(
            &self,
            _id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            self.record("abort_uncommitted_compaction_projections");
            Ok(())
        }

        async fn abort_rejected_runtime_run_projections(
            &self,
            _id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            self.record("abort_rejected_runtime_run_projections");
            Ok(())
        }

        async fn interrupt(
            &self,
            _id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            self.record("interrupt");
            Ok(())
        }

        async fn read(
            &self,
            id: &meerkat_core::types::SessionId,
        ) -> Result<meerkat_core::service::SessionView, SessionError> {
            Err(SessionError::NotFound { id: id.clone() })
        }

        async fn list(
            &self,
            _query: meerkat_core::service::SessionQuery,
        ) -> Result<Vec<meerkat_core::service::SessionSummary>, SessionError> {
            Ok(Vec::new())
        }

        async fn archive(&self, _id: &meerkat_core::types::SessionId) -> Result<(), SessionError> {
            self.record("archive");
            Ok(())
        }

        async fn record_live_terminal_error(
            &self,
            _id: &meerkat_core::types::SessionId,
            _cause: meerkat_core::live_adapter::LiveAdapterErrorCode,
        ) -> Result<(), SessionError> {
            self.record("record_live_terminal_error");
            Ok(())
        }

        async fn record_live_output_audio_degraded(
            &self,
            _id: &meerkat_core::types::SessionId,
            _dropped: u64,
        ) -> Result<(), SessionError> {
            self.record("record_live_output_audio_degraded");
            Ok(())
        }
    }

    #[async_trait]
    impl meerkat_core::service::SessionServiceCommsExt for ForwardingProbe {}

    #[async_trait]
    impl meerkat_core::service::SessionServiceControlExt for ForwardingProbe {
        async fn append_system_context(
            &self,
            _id: &meerkat_core::types::SessionId,
            _req: meerkat_core::service::AppendSystemContextRequest,
        ) -> Result<
            meerkat_core::service::AppendSystemContextResult,
            meerkat_core::service::SessionControlError,
        > {
            self.record("append_system_context");
            Ok(meerkat_core::service::AppendSystemContextResult {
                status: meerkat_core::service::AppendSystemContextStatus::Applied,
            })
        }

        async fn stage_tool_results(
            &self,
            _id: &meerkat_core::types::SessionId,
            _req: meerkat_core::service::StageToolResultsRequest,
        ) -> Result<meerkat_core::service::StageToolResultsResult, SessionError> {
            self.record("stage_tool_results");
            Ok(meerkat_core::service::StageToolResultsResult {
                accepted_result_count: 7,
                // meerkat 0.8.8 added the durable-ingress disposition. This
                // probe records the forwarded call; `Staged` is the ordinary
                // accepted outcome.
                disposition: meerkat_core::service::StageToolResultsDisposition::Staged,
            })
        }
    }

    #[async_trait]
    impl meerkat_core::service::SessionServiceHistoryExt for ForwardingProbe {
        async fn read_history(
            &self,
            id: &meerkat_core::types::SessionId,
            _query: meerkat_core::service::SessionHistoryQuery,
        ) -> Result<meerkat_core::service::SessionHistoryPage, SessionError> {
            Err(SessionError::NotFound { id: id.clone() })
        }

        async fn read_transcript_revision(
            &self,
            id: &meerkat_core::types::SessionId,
            _query: meerkat_core::service::SessionTranscriptRevisionQuery,
        ) -> Result<meerkat_core::service::SessionTranscriptRevisionPage, SessionError> {
            self.record("read_transcript_revision");
            Err(SessionError::NotFound { id: id.clone() })
        }

        async fn list_transcript_revisions(
            &self,
            id: &meerkat_core::types::SessionId,
            _query: meerkat_core::service::SessionTranscriptRevisionListQuery,
        ) -> Result<meerkat_core::service::SessionTranscriptRevisionList, SessionError> {
            self.record("list_transcript_revisions");
            Err(SessionError::NotFound { id: id.clone() })
        }
    }

    #[async_trait]
    impl MobSessionService for ForwardingProbe {
        async fn prepare_session_for_resume(
            &self,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            Ok(())
        }
        async fn acknowledge_committed_runtime_session_boundary_under_turn_finalization_boundary(
            &self,
            _session_id: &meerkat_core::types::SessionId,
            _authority: &meerkat_core::CommittedSessionBoundaryAuthority,
        ) -> Result<(), SessionError> {
            Err(SessionError::Unsupported(
                "test double does not acknowledge store-owned runtime boundaries".to_string(),
            ))
        }
        async fn load_session_for_resume(
            &self,
            session_id: &meerkat_core::types::SessionId,
        ) -> Result<meerkat_mob::ResumeSessionLoad, SessionError> {
            // Truthful derived answer: this double's resume visibility IS its
            // typed reads' visibility (the meerkat-mob test-double idiom).
            if let Some(session) = self.load_persisted_session(session_id).await? {
                return Ok(meerkat_mob::ResumeSessionLoad::Active(Box::new(session)));
            }
            if let Some(session) = self.load_revivable_retired_session(session_id).await? {
                return Ok(meerkat_mob::ResumeSessionLoad::Revivable(Box::new(session)));
            }
            Ok(meerkat_mob::ResumeSessionLoad::Absent)
        }

        async fn create_session_under_runtime_turn_boundary(
            &self,
            req: CreateSessionRequest,
        ) -> Result<meerkat_core::types::RunResult, SessionError> {
            meerkat_core::SessionService::create_session(self, req).await
        }

        fn supports_persistent_sessions(&self) -> bool {
            true
        }

        fn runtime_adapter(&self) -> Option<Arc<meerkat_runtime::MeerkatMachine>> {
            Some(Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()))
        }

        async fn cancel_after_boundary_with_machine_authority(
            &self,
            session_id: &meerkat_core::types::SessionId,
            _expected_run_id: &meerkat_core::lifecycle::RunId,
            _authority: meerkat_runtime::MachineSessionControlAuthority,
        ) -> Result<(), SessionError> {
            self.record("cancel_after_boundary_with_machine_authority");
            match self
                .cancel_outcome
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                0 => Err(SessionError::NotFound {
                    id: session_id.clone(),
                }),
                1 => Err(SessionError::NotRunning {
                    id: session_id.clone(),
                }),
                2 => Err(SessionError::Unsupported(
                    "synthetic cancel rejection".to_string(),
                )),
                _ => Ok(()),
            }
        }

        async fn archive_with_mob_lifecycle_authority(
            &self,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            self.record("archive_with_mob_lifecycle_authority");
            Ok(())
        }

        async fn archive_with_mob_lifecycle_authority_under_runtime_turn_boundary(
            &self,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            self.record("archive_with_mob_lifecycle_authority_under_runtime_turn_boundary");
            Ok(())
        }

        async fn discard_live_session_under_runtime_turn_boundary(
            &self,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<(), SessionError> {
            self.record("discard_live_session_under_runtime_turn_boundary");
            Ok(())
        }

        async fn session_known_to_archive_authority(
            &self,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<bool, SessionError> {
            self.record("session_known_to_archive_authority");
            Ok(true)
        }

        async fn prepare_transient_turn_context_for_active_turn(
            &self,
            _session_id: &meerkat_core::types::SessionId,
            _expected_run_id: &meerkat_core::lifecycle::RunId,
            _contexts: Vec<meerkat_core::lifecycle::run_primitive::TurnRequestContext>,
        ) -> Result<meerkat_core::CoreBoundaryStageOutput, meerkat_core::CoreBoundaryStageError>
        {
            self.record("prepare_transient_turn_context_for_active_turn");
            Err(meerkat_core::CoreBoundaryStageError::unavailable(
                "probe has no boundary authority",
            ))
        }
    }

    #[tokio::test]
    async fn pre_build_wrapper_forwards_mob_authority_and_control_extensions() {
        let probe = Arc::new(ForwardingProbe::default());
        let inner: Arc<dyn MobSessionService> = probe.clone();
        let wrapped = PreBuildMobSessionService {
            inner,
            hook: no_op_pre_build_hook(),
            dispatch_taint: None,
            after_create_hook: None,
            runtime_adapter_override: Some(Arc::new(meerkat_runtime::MeerkatMachine::ephemeral())),
            session_read_absorber: None,
            archived_terminal_authority: None,
        };
        let session_id = meerkat_core::types::SessionId::new();
        let run_id = meerkat_core::lifecycle::RunId::new();

        MobSessionService::cancel_after_boundary_with_machine_authority(
            &wrapped,
            &session_id,
            &run_id,
            wrapped
                .runtime_adapter()
                .expect("wrapper should expose runtime adapter")
                .session_control_authority(),
        )
        .await
        .expect("machine-authorized cancel should treat a missing live session as quiesced");

        MobSessionService::archive_with_mob_lifecycle_authority(&wrapped, &session_id)
            .await
            .expect("archive_with_mob_lifecycle_authority should forward to inner service");
        meerkat_core::service::SessionService::reconcile_runtime_compaction_projections(
            &wrapped,
            &session_id,
            Vec::new(),
        )
        .await
        .expect("runtime compaction reconciliation should forward to inner service");
        meerkat_core::service::SessionService::abort_uncommitted_compaction_projections(
            &wrapped,
            &session_id,
        )
        .await
        .expect("runtime compaction abort should forward to inner service");
        meerkat_core::service::SessionService::abort_rejected_runtime_run_projections(
            &wrapped,
            &session_id,
        )
        .await
        .expect("rejected runtime-run cleanup should forward to inner service");
        meerkat_core::service::SessionService::record_live_terminal_error(
            &wrapped,
            &session_id,
            meerkat_core::live_adapter::LiveAdapterErrorCode::ConnectionLost,
        )
        .await
        .expect("live terminal errors should forward to inner service");
        meerkat_core::service::SessionService::record_live_output_audio_degraded(
            &wrapped,
            &session_id,
            3,
        )
        .await
        .expect("live output degradation should forward to inner service");
        let staged = meerkat_core::service::SessionServiceControlExt::stage_tool_results(
            &wrapped,
            &session_id,
            meerkat_core::service::StageToolResultsRequest {
                results: Vec::new(),
            },
        )
        .await
        .expect("stage_tool_results should forward to inner service");
        let _ = meerkat_core::service::SessionServiceHistoryExt::read_transcript_revision(
            &wrapped,
            &session_id,
            meerkat_core::service::SessionTranscriptRevisionQuery {
                revision: "rev-1".to_string(),
                offset: 0,
                limit: None,
            },
        )
        .await;
        let _ = meerkat_core::service::SessionServiceHistoryExt::list_transcript_revisions(
            &wrapped,
            &session_id,
            meerkat_core::service::SessionTranscriptRevisionListQuery::default(),
        )
        .await;

        assert_eq!(staged.accepted_result_count, 7);
        let preparation_error = wrapped
            .prepare_transient_turn_context_for_active_turn(
                &session_id,
                &meerkat_core::lifecycle::RunId::new(),
                vec![
                    meerkat_core::lifecycle::run_primitive::TurnRequestContext::new("steer")
                        .expect("non-empty transient turn context"),
                ],
            )
            .await
            .expect_err("probe preparation error should forward unchanged");
        assert!(preparation_error.is_unavailable());
        // meerkat 0.7.19 disposal-routing seam: the trait default is
        // fail-closed `true`, so a wrapper that fails to forward this
        // silently resurrects the ask-20 stranding for host-owned sessions.
        let known = wrapped
            .session_known_to_archive_authority(&session_id)
            .await
            .expect("archive-authority probe should forward");
        assert!(known, "probe answers true");
        assert_eq!(
            probe.calls(),
            vec![
                "cancel_after_boundary_with_machine_authority",
                "archive_with_mob_lifecycle_authority",
                "reconcile_runtime_compaction_projections",
                "abort_uncommitted_compaction_projections",
                "abort_rejected_runtime_run_projections",
                "record_live_terminal_error",
                "record_live_output_audio_degraded",
                "stage_tool_results",
                "read_transcript_revision",
                "list_transcript_revisions",
                "prepare_transient_turn_context_for_active_turn",
                "session_known_to_archive_authority",
            ]
        );
    }

    #[tokio::test]
    async fn machine_authorized_boundary_cancel_only_normalizes_quiesced_liveness() {
        let probe = Arc::new(ForwardingProbe::default());
        let wrapped = PreBuildMobSessionService {
            inner: probe.clone(),
            hook: no_op_pre_build_hook(),
            dispatch_taint: None,
            after_create_hook: None,
            runtime_adapter_override: Some(Arc::new(meerkat_runtime::MeerkatMachine::ephemeral())),
            session_read_absorber: None,
            archived_terminal_authority: None,
        };
        let session_id = meerkat_core::types::SessionId::new();
        let run_id = meerkat_core::lifecycle::RunId::new();

        for quiesced_outcome in [0, 1] {
            probe.set_cancel_outcome(quiesced_outcome);
            MobSessionService::cancel_after_boundary_with_machine_authority(
                &wrapped,
                &session_id,
                &run_id,
                wrapped
                    .runtime_adapter()
                    .expect("wrapper should expose runtime adapter")
                    .session_control_authority(),
            )
            .await
            .expect("NotFound and NotRunning both prove lower-plane quiescence");
        }

        probe.set_cancel_outcome(2);
        let error = MobSessionService::cancel_after_boundary_with_machine_authority(
            &wrapped,
            &session_id,
            &run_id,
            wrapped
                .runtime_adapter()
                .expect("wrapper should expose runtime adapter")
                .session_control_authority(),
        )
        .await
        .expect_err("non-liveness cancellation failures must remain fatal");
        assert!(
            matches!(error, SessionError::Unsupported(ref detail) if detail == "synthetic cancel rejection"),
            "unexpected fail-closed cancellation error: {error}"
        );
    }

    /// Cold-activation authority mint (OB3 ephemeral runtime store, ruled
    /// in-design 2026-07-31): racers over the authority reads on a COLD
    /// store collapse to one committed seed under the single-flight fence
    /// and both observe the same store-issued authority; the seed is the
    /// current committed record the next boundary CAS chains off.
    #[tokio::test]
    async fn cold_activation_mints_single_runtime_authority_from_durable_session() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let mut session = meerkat_core::Session::new();
        session.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("durable turn"),
        ));
        session_store
            .save(&session)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> =
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
        let store = Arc::new(SessionStoreBackedRuntimeStore::new(inner, session_store));
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(session.id());

        let authority_read = {
            let store = Arc::clone(&store);
            let runtime_id = runtime_id.clone();
            async move {
                meerkat_runtime::RuntimeStore::load_whole_blob_store_authority(&*store, &runtime_id)
                    .await
            }
        };
        let snapshot_read = {
            let store = Arc::clone(&store);
            let runtime_id = runtime_id.clone();
            async move {
                meerkat_runtime::RuntimeStore::load_committed_whole_blob_snapshot(
                    &*store,
                    &runtime_id,
                )
                .await
            }
        };
        let (authority, snapshot) = tokio::join!(authority_read, snapshot_read);
        let authority = authority
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("racer A must observe the minted authority"));
        let snapshot = snapshot
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("racer B must observe the minted snapshot"));
        assert_eq!(
            &authority,
            snapshot.authority(),
            "both racers must converge on ONE store-issued authority"
        );
    }

    /// Write-side complement of the mint regressions (release-ladder
    /// acceptance): a committed inner boundary whose DURABLE PROJECTION
    /// write fails is never reported durable - the committing verb errors
    /// and the injected store still holds the previous boundary - and a
    /// RETRY of the same prepared boundary converges from the
    /// already-current inner successor (idempotent CAS observation) instead
    /// of wedging on a conflict.
    #[tokio::test]
    async fn failed_durable_projection_is_never_reported_durable_and_retry_converges() {
        struct FailingProjectionStore {
            inner: meerkat_store::MemoryStore,
            fail: std::sync::atomic::AtomicBool,
        }

        impl FailingProjectionStore {
            fn outage(&self) -> Option<meerkat_store::SessionStoreError> {
                self.fail
                    .load(std::sync::atomic::Ordering::SeqCst)
                    .then(|| {
                        meerkat_store::SessionStoreError::Internal(
                            "injected projection outage".to_string(),
                        )
                    })
            }
        }

        #[async_trait]
        impl SessionStore for FailingProjectionStore {
            async fn save(
                &self,
                session: &meerkat_core::Session,
            ) -> Result<(), meerkat_store::SessionStoreError> {
                if let Some(outage) = self.outage() {
                    return Err(outage);
                }
                self.inner.save(session).await
            }

            async fn save_authoritative_projection(
                &self,
                session: &meerkat_core::Session,
            ) -> Result<(), meerkat_store::SessionStoreError> {
                if let Some(outage) = self.outage() {
                    return Err(outage);
                }
                self.inner.save_authoritative_projection(session).await
            }

            async fn load(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<Option<meerkat_core::Session>, meerkat_store::SessionStoreError>
            {
                self.inner.load(id).await
            }

            async fn list(
                &self,
                filter: meerkat_store::SessionFilter,
            ) -> Result<Vec<meerkat_core::SessionMeta>, meerkat_store::SessionStoreError>
            {
                self.inner.list(filter).await
            }

            async fn delete(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<(), meerkat_store::SessionStoreError> {
                self.inner.delete(id).await
            }

            async fn delete_if_current_revision(
                &self,
                id: &meerkat_core::types::SessionId,
                expected_current_revision: &str,
            ) -> Result<bool, meerkat_store::SessionStoreError> {
                self.inner
                    .delete_if_current_revision(id, expected_current_revision)
                    .await
            }
        }

        let failing = Arc::new(FailingProjectionStore {
            inner: meerkat_store::MemoryStore::new(),
            fail: std::sync::atomic::AtomicBool::new(false),
        });
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> =
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
        let store = SessionStoreBackedRuntimeStore::new(
            inner,
            Arc::clone(&failing) as Arc<dyn SessionStore>,
        );
        let mut session = meerkat_core::Session::new();
        session.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("first turn"),
        ));
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(session.id());

        // Baseline committed boundary with a healthy projection.
        let bytes = session
            .to_persisted_bytes()
            .unwrap_or_else(|error| panic!("{error}"));
        meerkat_runtime::RuntimeStore::commit_session_snapshot(
            &store,
            &runtime_id,
            meerkat_runtime::store::SerializedSessionSnapshot {
                session_snapshot: Arc::new(bytes),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            failing
                .inner
                .load(session.id())
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .is_some(),
            "the baseline boundary must project durably"
        );

        // Successor boundary: the inner CAS commits, the projection FAILS.
        let expected =
            meerkat_runtime::RuntimeStore::load_whole_blob_store_authority(&store, &runtime_id)
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .unwrap_or_else(|| panic!("baseline boundary must issue authority"));
        let mut successor = session.clone();
        successor.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("second turn"),
        ));
        let prepared = meerkat_runtime::store::PreparedWholeBlobSnapshotCas::prepare(
            expected,
            meerkat_core::lifecycle::core_executor::BoundSessionCommit::sealed(Arc::new(
                successor.clone(),
            ))
            .unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        failing
            .fail
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let error = meerkat_runtime::RuntimeStore::commit_prepared_whole_blob_snapshot_cas(
            &store,
            &runtime_id,
            prepared.clone(),
        )
        .await
        .expect_err("a failed durable projection must fail the committing verb");
        assert!(
            error.to_string().contains("durable session projection"),
            "the failure must name the projection write: {error}"
        );
        let after_failure = failing
            .inner
            .load(session.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the previous boundary must survive the outage"));
        assert_eq!(
            after_failure.messages().len(),
            session.messages().len(),
            "a boundary whose projection failed must never be reported durable"
        );

        // Retry convergence from the ALREADY-CURRENT inner successor: the
        // store-level CAS rightly conflicts (the predecessor token moved),
        // and the caller-side proof meerkat's service uses for exactly this
        // shape - `PreparedWholeBlobSnapshotCas::accepts_committed_authority`
        // - accepts the observed authority as the committed candidate. No
        // wedge, no second physical write.
        failing
            .fail
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let observed =
            meerkat_runtime::RuntimeStore::load_whole_blob_store_authority(&store, &runtime_id)
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .unwrap_or_else(|| panic!("the inner successor must be current"));
        assert!(
            prepared.accepts_committed_authority(&observed),
            "the already-current inner successor must prove the retried candidate committed"
        );

        // And the projection self-heals: the NEXT boundary, prepared against
        // the CURRENT successor, commits through the facade and writes the
        // full document through (whole-blob projection carries the whole
        // session, so one successful commit converges durable truth).
        let mut third = successor.clone();
        third.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("third turn"),
        ));
        let prepared_third = meerkat_runtime::store::PreparedWholeBlobSnapshotCas::prepare(
            observed,
            meerkat_core::lifecycle::core_executor::BoundSessionCommit::sealed(Arc::new(
                third.clone(),
            ))
            .unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let outcome = meerkat_runtime::RuntimeStore::commit_prepared_whole_blob_snapshot_cas(
            &store,
            &runtime_id,
            prepared_third,
        )
        .await
        .unwrap_or_else(|error| panic!("the healed boundary must commit, not wedge: {error}"));
        assert!(
            matches!(
                outcome,
                meerkat_runtime::store::WholeBlobSnapshotCasOutcome::Committed(_)
            ),
            "the next boundary against the current successor must commit"
        );
        let converged = failing
            .inner
            .load(session.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the healed boundary must project durably"));
        assert_eq!(
            converged.messages().len(),
            third.messages().len(),
            "the durable projection must converge on the latest committed document"
        );
    }

    /// Task #56 (parent-1 launch blocker): a committed WholeBlob boundary
    /// carrying a NEW rewrite generation over an older durable row must
    /// project through the store's typed rewrite door, installing the
    /// missing commit on the durable row - not tear it into
    /// graph-ahead-of-head state that the rewrite-save invariant then
    /// refuses on every resume. Durable HeadCanonical gen0 + committed
    /// WholeBlob gen1 -> the projection proves gen1 head, session document,
    /// and graph on the durable row.
    #[tokio::test]
    async fn rewrite_advanced_boundary_projects_missing_commit_into_durable_row() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new(dir.path().join("runtime.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        // Durable predecessor at generation 0.
        let mut gen0 = meerkat_core::Session::new();
        gen0.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("original opening"),
        ));
        session_store
            .save(&gen0)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(gen0.id());
        inner
            .commit_session_snapshot(
                &runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(
                        gen0.to_persisted_bytes()
                            .unwrap_or_else(|error| panic!("{error}")),
                    ),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        // The committed successor: one typed rewrite (generation 1) plus a
        // trailing plain append past the audited head.
        let parent_revision = gen0
            .transcript_revision()
            .unwrap_or_else(|error| panic!("{error}"));
        let mut successor = gen0.clone();
        successor
            .commit_transcript_rewrite(
                meerkat_core::TranscriptRewriteSelection::MessageRange { start: 0, end: 1 },
                vec![meerkat_core::Message::User(
                    meerkat_core::types::UserMessage::text("rewritten opening"),
                )],
                meerkat_core::TranscriptRewriteReason::new("wedged-turn retire"),
                Some("task-56-regression".to_string()),
                Some(parent_revision),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        successor.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("post-rewrite turn"),
        ));

        // Commit the rewrite-advanced boundary THROUGH THE FACADE - the
        // projection under test runs as part of this committing verb.
        let store = Arc::new(SessionStoreBackedRuntimeStore::new(
            Arc::clone(&inner),
            Arc::clone(&session_store),
        ));
        let authority =
            meerkat_runtime::RuntimeStore::load_whole_blob_store_authority(&*store, &runtime_id)
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .unwrap_or_else(|| panic!("the seeded runtime authority must exist"));
        let prepared = meerkat_runtime::store::PreparedWholeBlobSnapshotCas::prepare(
            authority,
            meerkat_core::lifecycle::core_executor::BoundSessionCommit::sealed(Arc::new(
                successor.clone(),
            ))
            .unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let outcome = meerkat_runtime::RuntimeStore::commit_prepared_whole_blob_snapshot_cas(
            &*store,
            &runtime_id,
            prepared,
        )
        .await
        .unwrap_or_else(|error| {
            panic!("a rewrite-advanced boundary must project, not tear: {error}")
        });
        assert!(
            matches!(
                outcome,
                meerkat_runtime::store::WholeBlobSnapshotCasOutcome::Committed(_)
            ),
            "the boundary must commit"
        );

        // The durable row now proves the gen1 head, document, and graph.
        let durable = session_store
            .load(gen0.id())
            .await
            .unwrap_or_else(|error| panic!("the projected row must load cleanly: {error}"))
            .unwrap_or_else(|| panic!("the durable row must exist"));
        assert_eq!(
            durable
                .transcript_rewrite_generation()
                .unwrap_or_else(|error| panic!("{error}")),
            1,
            "the durable row must carry the installed rewrite generation"
        );
        assert_eq!(
            durable.messages().len(),
            successor.messages().len(),
            "the durable document must match the committed successor"
        );
        assert_eq!(
            durable
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            successor
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            "the durable live revision must match the committed successor"
        );
        let graph = durable
            .validated_transcript_history_state()
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the durable row must carry the proved graph"));
        assert_eq!(
            graph.commit_count(),
            1,
            "the durable graph must retain the installed rewrite commit"
        );
    }

    /// Task #56, freshness half (parent-1's ACTUAL recovery path): the tear
    /// already exists on disk - durable gen0, committed gen1 - and the next
    /// thing that happens is a plain RESUME, not a new committing verb. The
    /// freshness probe must distinguish durable-behind from fresh, run the
    /// committed-to-durable rewrite reconciliation, and converge the durable
    /// row to gen1 with NO new write through the facade.
    #[tokio::test]
    async fn parent_1_torn_durable_row_heals_on_plain_freshness_pass() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new(dir.path().join("runtime.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let mut gen0 = meerkat_core::Session::new();
        gen0.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("original opening"),
        ));
        session_store
            .save(&gen0)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(gen0.id());
        // The tear: the runtime store holds a committed rewrite-advanced
        // boundary the durable row never received.
        let parent_revision = gen0
            .transcript_revision()
            .unwrap_or_else(|error| panic!("{error}"));
        let mut successor = gen0.clone();
        successor
            .commit_transcript_rewrite(
                meerkat_core::TranscriptRewriteSelection::MessageRange { start: 0, end: 1 },
                vec![meerkat_core::Message::User(
                    meerkat_core::types::UserMessage::text("rewritten opening"),
                )],
                meerkat_core::TranscriptRewriteReason::new("wedged-turn retire"),
                Some("task-56-regression".to_string()),
                Some(parent_revision),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        inner
            .commit_session_snapshot(
                &runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(
                        successor
                            .to_persisted_bytes()
                            .unwrap_or_else(|error| panic!("{error}")),
                    ),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        // A PLAIN READ through the facade (the resume path's authority
        // load) - no committing verb anywhere.
        let store = Arc::new(SessionStoreBackedRuntimeStore::new(
            Arc::clone(&inner),
            Arc::clone(&session_store),
        ));
        let snapshot =
            meerkat_runtime::RuntimeStore::load_committed_whole_blob_snapshot(&*store, &runtime_id)
                .await
                .unwrap_or_else(|error| {
                    panic!("the freshness pass must reconcile, not fail: {error}")
                })
                .unwrap_or_else(|| panic!("the committed snapshot must remain readable"));
        assert_eq!(
            snapshot
                .session()
                .transcript_rewrite_generation()
                .unwrap_or_else(|error| panic!("{error}")),
            1,
            "the committed authority is the gen1 successor"
        );

        // The durable row converged to gen1 on the read alone.
        let healed = session_store
            .load(gen0.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the durable row must exist"));
        assert_eq!(
            healed
                .transcript_rewrite_generation()
                .unwrap_or_else(|error| panic!("{error}")),
            1,
            "a plain freshness pass must heal the torn durable row"
        );
        assert_eq!(
            healed
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            successor
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            "the healed row must match the committed successor exactly"
        );
    }

    /// Task #56, append-before-compact seam: durable synced at the gen1
    /// audited head;
    /// the committed turn appended C and rewrote to gen2 (parent gen1-head +
    /// C). The reconciler must FIRST project the gen1-head -> gen1-head + C
    /// append onto the durable row (exact parent revision), THEN replay the
    /// gen2 commit, and converge end to end with exact digests.
    #[tokio::test]
    async fn append_before_compact_projects_append_then_replays_rewrite() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> =
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
        let mut gen1 = meerkat_core::Session::new();
        gen1.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("original opening"),
        ));
        let parent_revision = gen1
            .transcript_revision()
            .unwrap_or_else(|error| panic!("{error}"));
        gen1.commit_transcript_rewrite(
            meerkat_core::TranscriptRewriteSelection::MessageRange { start: 0, end: 1 },
            vec![meerkat_core::Message::User(
                meerkat_core::types::UserMessage::text("gen1 head"),
            )],
            meerkat_core::TranscriptRewriteReason::new("first compaction"),
            Some("task-56-regression".to_string()),
            Some(parent_revision),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        session_store
            .save_authoritative_projection(&gen1)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(gen1.id());
        let mut successor = gen1.clone();
        successor.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("appended C"),
        ));
        let parent_revision = successor
            .transcript_revision()
            .unwrap_or_else(|error| panic!("{error}"));
        successor
            .commit_transcript_rewrite(
                meerkat_core::TranscriptRewriteSelection::MessageRange { start: 0, end: 2 },
                vec![meerkat_core::Message::User(
                    meerkat_core::types::UserMessage::text("gen2 head"),
                )],
                meerkat_core::TranscriptRewriteReason::new("append-before-compact"),
                Some("task-56-regression".to_string()),
                Some(parent_revision),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        inner
            .commit_session_snapshot(
                &runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(
                        successor
                            .to_persisted_bytes()
                            .unwrap_or_else(|error| panic!("{error}")),
                    ),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let store = Arc::new(SessionStoreBackedRuntimeStore::new(
            Arc::clone(&inner),
            Arc::clone(&session_store),
        ));
        store
            .project_committed_session_to_durable(&runtime_id)
            .await
            .unwrap_or_else(|error| {
                panic!("append-then-rewrite reconciliation must converge: {error}")
            });
        let converged = session_store
            .load(gen1.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the durable row must exist"));
        assert_eq!(
            converged
                .transcript_rewrite_generation()
                .unwrap_or_else(|error| panic!("{error}")),
            2,
            "the gen2 rewrite must be installed after the append projection"
        );
        assert_eq!(
            converged
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            successor
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            "the durable row must converge on the exact committed digests"
        );
    }

    /// Task #56, gap 3 (stranded first projection): committed WholeBlob
    /// authority exists - including a rewrite generation - but the durable
    /// store has NO row for the session (the first projection failed with
    /// its committing verb). A plain freshness/resume pass must create the
    /// durable projection and converge, not mark the runtime fresh over the
    /// projection debt and strand it until some future committing verb.
    #[tokio::test]
    async fn cold_activation_with_no_durable_row_projects_committed_authority() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new(dir.path().join("runtime.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        // Committed authority carrying a rewrite; the durable store is left
        // completely empty for this session.
        let mut successor = meerkat_core::Session::new();
        successor.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("original opening"),
        ));
        let parent_revision = successor
            .transcript_revision()
            .unwrap_or_else(|error| panic!("{error}"));
        successor
            .commit_transcript_rewrite(
                meerkat_core::TranscriptRewriteSelection::MessageRange { start: 0, end: 1 },
                vec![meerkat_core::Message::User(
                    meerkat_core::types::UserMessage::text("rewritten opening"),
                )],
                meerkat_core::TranscriptRewriteReason::new("first boundary"),
                Some("task-56-regression".to_string()),
                Some(parent_revision),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(successor.id());
        inner
            .commit_session_snapshot(
                &runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(
                        successor
                            .to_persisted_bytes()
                            .unwrap_or_else(|error| panic!("{error}")),
                    ),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        // A plain read through the facade - no committing verb anywhere.
        let store = Arc::new(SessionStoreBackedRuntimeStore::new(
            Arc::clone(&inner),
            Arc::clone(&session_store),
        ));
        meerkat_runtime::RuntimeStore::load_committed_whole_blob_snapshot(&*store, &runtime_id)
            .await
            .unwrap_or_else(|error| panic!("the freshness pass must project, not fail: {error}"))
            .unwrap_or_else(|| panic!("the committed snapshot must remain readable"));

        let projected = session_store
            .load(successor.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| {
                panic!("the plain freshness pass must create the missing durable row")
            });
        assert_eq!(
            projected
                .transcript_rewrite_generation()
                .unwrap_or_else(|error| panic!("{error}")),
            1,
            "the projected row must carry the committed rewrite generation"
        );
        assert_eq!(
            projected
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            successor
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            "the projected row must match the committed authority exactly"
        );
    }

    /// Task #56 corpus finding (HomeCore parent-1, real bytes): the member
    /// is PARKED and its session explicitly UNREGISTERED from
    /// identity-runtime state while the durable row sits torn behind
    /// committed runtime authority. The tear reconciliation is a
    /// durable-store repair, not a live-session operation - it must not
    /// depend on registration, or repair and registration deadlock (the
    /// member cannot register until its row resumes; the row cannot be
    /// repaired until the member registers). A plain freshness/boot pass
    /// must converge the row through the projection doors' parked-repair
    /// admission.
    #[tokio::test]
    async fn parked_unregistered_torn_head_heals_on_plain_freshness_pass() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let continuity: Arc<dyn crate::identity_first::ContinuityStore> = Arc::new(
            crate::identity_first::LocalContinuityStore::open(dir.path().join("continuity.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let adapter = Arc::new(crate::identity_first::ContinuitySessionStoreAdapter::new(
            Arc::clone(&continuity),
        ));
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new(dir.path().join("runtime.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );

        // A REGISTERED write lands the gen0 durable row, exactly as the
        // member's live turns did before the incident.
        let mut gen0 = meerkat_core::Session::new();
        gen0.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("original opening"),
        ));
        let identity = crate::identity_first::AgentIdentity::parse("domain:parked")
            .unwrap_or_else(|error| panic!("{error}"));
        // The durable continuity record binding the identity to this session
        // - in the field this is what restore resolves, and what the parked
        // repair hydrates its write authority from.
        crate::identity_first::ContinuityStore::upsert_continuity_record(
            continuity.as_ref(),
            &crate::identity_first::ContinuityRecord {
                identity: identity.clone(),
                agent_runtime_id: crate::identity_first::AgentRuntimeId::parse(
                    "rt:domain:parked:0",
                )
                .unwrap_or_else(|error| panic!("{error}")),
                session_id: gen0.id().clone(),
                generation: crate::identity_first::ContinuityGeneration::new(0),
                checkpoint_version: crate::identity_first::CheckpointVersion::new(0),
            },
            crate::identity_first::FencingToken::new(1),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        adapter
            .register_session(
                gen0.id(),
                crate::identity_first::SessionRuntimeState {
                    identity,
                    generation: crate::identity_first::ContinuityGeneration::new(0),
                    fencing_token: crate::identity_first::FencingToken::new(1),
                    checkpoint_version: crate::identity_first::CheckpointVersion::new(0),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        meerkat::SessionStore::save(adapter.as_ref(), &gen0)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        // The PARK: explicit unregistration from identity-runtime state.
        adapter
            .unregister_session(gen0.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        // The tear: committed runtime authority advanced one rewrite
        // generation past the durable row.
        let parent_revision = gen0
            .transcript_revision()
            .unwrap_or_else(|error| panic!("{error}"));
        let mut successor = gen0.clone();
        successor
            .commit_transcript_rewrite(
                meerkat_core::TranscriptRewriteSelection::MessageRange { start: 0, end: 1 },
                vec![meerkat_core::Message::User(
                    meerkat_core::types::UserMessage::text("rewritten opening"),
                )],
                meerkat_core::TranscriptRewriteReason::new("wedged-turn retire"),
                Some("task-56-corpus-regression".to_string()),
                Some(parent_revision),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(gen0.id());
        inner
            .commit_session_snapshot(
                &runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(
                        successor
                            .to_persisted_bytes()
                            .unwrap_or_else(|error| panic!("{error}")),
                    ),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        // A plain read through the facade with the CONTINUITY ADAPTER as
        // the injected store - the field composition, unregistered state
        // and all. No committing verb, no registration.
        let store = Arc::new(SessionStoreBackedRuntimeStore::new(
            Arc::clone(&inner),
            Arc::clone(&adapter) as Arc<dyn SessionStore>,
        ));
        meerkat_runtime::RuntimeStore::load_committed_whole_blob_snapshot(&*store, &runtime_id)
            .await
            .unwrap_or_else(|error| {
                panic!("the parked repair must converge, not deadlock on registration: {error}")
            })
            .unwrap_or_else(|| panic!("the committed snapshot must remain readable"));

        // The durable AUTHORITY is the continuity head row. Slim
        // head-canonical materializations keep retained history out-of-line
        // (a loaded Session always reads rewrite generation 0 by design), so
        // the heal is proven where meerkat's resume invariant reads it: the
        // head row's adopted rewrite count and revision.
        let channel = continuity
            .as_incremental_sessions()
            .unwrap_or_else(|| panic!("the local continuity store provides the delta channel"));
        let healed_head = channel
            .load_canonical_head(gen0.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the healed head row must exist"));
        assert_eq!(
            healed_head.rewrite_count, 1,
            "the parked member's torn durable head must carry the committed rewrite"
        );
        let successor_revision = successor
            .transcript_revision()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            healed_head.head_revision, successor_revision,
            "the healed head row must sit at the committed successor's revision"
        );
        let healed = meerkat::SessionStore::load(adapter.as_ref(), gen0.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the durable row must exist"));
        assert_eq!(
            healed
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            successor_revision,
            "the healed row must match the committed successor exactly"
        );

        // Second boot: the member restores REGISTERED (the mob boot path
        // registers rostered members from the continuity record before any
        // read). A fresh facade's freshness pass over the already-healed row
        // must converge idempotently: no typed refusal, no re-replay, the
        // head row still at the committed rewrite and revision (envelope
        // updates aside).
        let (record, fencing_token, fence_current) =
            crate::identity_first::ContinuityStore::resolve_record_by_session(
                continuity.as_ref(),
                gen0.id(),
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the continuity record must still bind the session"));
        adapter
            .register_session(
                gen0.id(),
                crate::identity_first::SessionRuntimeState {
                    identity: record.identity.clone(),
                    generation: record.generation,
                    fencing_token,
                    checkpoint_version: fence_current,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let second_boot = Arc::new(SessionStoreBackedRuntimeStore::new(
            Arc::clone(&inner),
            Arc::clone(&adapter) as Arc<dyn SessionStore>,
        ));
        meerkat_runtime::RuntimeStore::load_committed_whole_blob_snapshot(
            &*second_boot,
            &runtime_id,
        )
        .await
        .unwrap_or_else(|error| {
            panic!("the healed row must stay resumable on the next boot: {error}")
        })
        .unwrap_or_else(|| panic!("the committed snapshot must remain readable on the next boot"));
        let head_after_second_boot = channel
            .load_canonical_head(gen0.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the head row must survive the second boot"));
        assert_eq!(
            head_after_second_boot.rewrite_count, 1,
            "the second boot must not re-replay or regress the healed rewrite"
        );
        assert_eq!(
            head_after_second_boot.head_revision, successor_revision,
            "the second boot must leave the healed head at the committed revision"
        );
    }

    /// Task #56, equal-order fork disposition: equal (rewrite generation,
    /// message count) order between the durable row and the committed
    /// authority is necessary but not sufficient for freshness - a
    /// session-store restore from a different lineage can coincide on both
    /// counts with different content. That is a FORK: the probe must refuse
    /// typed, loudly and repeatably, and adopt neither side - never mark
    /// fresh over silent divergence.
    #[tokio::test]
    async fn freshen_refuses_equal_order_divergent_durable_row_typed() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new(dir.path().join("runtime.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        // One session identity, two single-message documents from different
        // lineages: generation 0 and message count 1 on BOTH sides, content
        // divergent.
        let seed = meerkat_core::Session::new();
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(seed.id());
        let mut committed = seed.clone();
        committed.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("committed turn"),
        ));
        let mut divergent = seed;
        divergent.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("divergent turn"),
        ));
        inner
            .commit_session_snapshot(
                &runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(
                        committed
                            .to_persisted_bytes()
                            .unwrap_or_else(|error| panic!("{error}")),
                    ),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        session_store
            .save(&divergent)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let store = Arc::new(SessionStoreBackedRuntimeStore::new(
            Arc::clone(&inner),
            Arc::clone(&session_store),
        ));
        for attempt in ["first read", "retry"] {
            let refused = meerkat_runtime::RuntimeStore::load_committed_whole_blob_snapshot(
                &*store,
                &runtime_id,
            )
            .await
            .expect_err("an equal-order divergent durable row must refuse typed, not adopt");
            assert!(
                refused.to_string().contains("DIVERGES in content"),
                "the {attempt} refusal must name the fork: {refused}"
            );
        }
        // Neither side was rewritten by the refused probe.
        let retained = inner
            .load_committed_whole_blob_snapshot(&runtime_id)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the committed runtime authority must survive"));
        assert_eq!(
            retained
                .session()
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            committed
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            "the committed runtime document must be untouched"
        );
        let durable = session_store
            .load(committed.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the durable row must survive"));
        assert_eq!(
            durable
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            divergent
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            "the durable document must be untouched"
        );
    }

    /// Task #56, envelope-debt case: generation, count, AND revision all
    /// equal, but the durable ENVELOPE lags (a failure after every rewrite
    /// save, before the final authoritative projection). The equal arm must
    /// detect the debt (persisted-encoding comparison) and complete the
    /// projection on a plain freshness pass instead of marking fresh over
    /// it.
    #[tokio::test]
    async fn equal_revision_envelope_debt_clears_on_plain_freshness_pass() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new(dir.path().join("runtime.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let mut session = meerkat_core::Session::new();
        session.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("original opening"),
        ));
        // Durable row WITHOUT the envelope update.
        session_store
            .save(&session)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        // Committed authority: identical transcript, updated envelope.
        let mut committed = session.clone();
        committed.set_metadata(
            "mobkit:task56:envelope-probe",
            serde_json::Value::String("current".to_string()),
        );
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(session.id());
        inner
            .commit_session_snapshot(
                &runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(
                        committed
                            .to_persisted_bytes()
                            .unwrap_or_else(|error| panic!("{error}")),
                    ),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        // A plain read through the facade - no committing verb anywhere.
        let store = Arc::new(SessionStoreBackedRuntimeStore::new(
            Arc::clone(&inner),
            Arc::clone(&session_store),
        ));
        meerkat_runtime::RuntimeStore::load_committed_whole_blob_snapshot(&*store, &runtime_id)
            .await
            .unwrap_or_else(|error| {
                panic!("the freshness pass must complete the envelope, not fail: {error}")
            })
            .unwrap_or_else(|| panic!("the committed snapshot must remain readable"));

        let healed = session_store
            .load(session.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the durable row must exist"));
        assert_eq!(
            healed
                .metadata()
                .get("mobkit:task56:envelope-probe")
                .and_then(|value| value.as_str()),
            Some("current"),
            "a plain freshness pass must complete the lagging envelope"
        );
        assert_eq!(
            healed
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            committed
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            "content stays identical - only the envelope was owed"
        );
    }

    /// Task #56, retry half: a partial failure MID-CHAIN (first missing
    /// commit installed, second refused by an injected outage) fails the
    /// committing verb, keeps the monotonic progress it made, and the exact
    /// retried step converges - installing ONLY the remaining commit, never
    /// re-writing the one already durable.
    #[tokio::test]
    async fn rewrite_replay_partial_failure_keeps_progress_and_exact_retry_converges() {
        struct FailNthRewriteStore {
            inner: Arc<dyn SessionStore>,
            rewrite_calls: std::sync::atomic::AtomicUsize,
            fail_on_call: std::sync::atomic::AtomicUsize,
        }

        #[async_trait]
        impl SessionStore for FailNthRewriteStore {
            async fn save(
                &self,
                session: &meerkat_core::Session,
            ) -> Result<(), meerkat_store::SessionStoreError> {
                self.inner.save(session).await
            }

            async fn save_transcript_rewrite(
                &self,
                session: &meerkat_core::Session,
                commit: &meerkat_core::TranscriptRewriteCommit,
            ) -> Result<(), meerkat_store::SessionStoreError> {
                let call = self
                    .rewrite_calls
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;
                if call == self.fail_on_call.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(meerkat_store::SessionStoreError::Internal(
                        "injected mid-chain outage".to_string(),
                    ));
                }
                self.inner.save_transcript_rewrite(session, commit).await
            }

            async fn save_authoritative_projection(
                &self,
                session: &meerkat_core::Session,
            ) -> Result<(), meerkat_store::SessionStoreError> {
                self.inner.save_authoritative_projection(session).await
            }

            async fn load(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<Option<meerkat_core::Session>, meerkat_store::SessionStoreError>
            {
                self.inner.load(id).await
            }

            async fn list(
                &self,
                filter: meerkat_store::SessionFilter,
            ) -> Result<Vec<meerkat_core::SessionMeta>, meerkat_store::SessionStoreError>
            {
                self.inner.list(filter).await
            }

            async fn delete(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<(), meerkat_store::SessionStoreError> {
                self.inner.delete(id).await
            }

            async fn delete_if_current_revision(
                &self,
                id: &meerkat_core::types::SessionId,
                expected_current_revision: &str,
            ) -> Result<bool, meerkat_store::SessionStoreError> {
                self.inner
                    .delete_if_current_revision(id, expected_current_revision)
                    .await
            }
        }

        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let failing = Arc::new(FailNthRewriteStore {
            inner: Arc::new(
                meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                    .unwrap_or_else(|error| panic!("{error}")),
            ),
            rewrite_calls: std::sync::atomic::AtomicUsize::new(0),
            fail_on_call: std::sync::atomic::AtomicUsize::new(0),
        });
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> =
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());

        // Durable predecessor at generation 0; committed successor two
        // rewrite generations ahead.
        let mut gen0 = meerkat_core::Session::new();
        gen0.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("original opening"),
        ));
        failing
            .save(&gen0)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(gen0.id());
        inner
            .commit_session_snapshot(
                &runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(
                        gen0.to_persisted_bytes()
                            .unwrap_or_else(|error| panic!("{error}")),
                    ),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let mut successor = gen0.clone();
        for (generation, replacement) in [(1u64, "first rewrite"), (2, "second rewrite")] {
            let parent_revision = successor
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}"));
            let commit = successor
                .commit_transcript_rewrite(
                    meerkat_core::TranscriptRewriteSelection::MessageRange { start: 0, end: 1 },
                    vec![meerkat_core::Message::User(
                        meerkat_core::types::UserMessage::text(replacement),
                    )],
                    meerkat_core::TranscriptRewriteReason::new("task-56 chain"),
                    Some("task-56-regression".to_string()),
                    Some(parent_revision),
                )
                .unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(commit.rewrite_generation, generation);
        }

        let store = Arc::new(SessionStoreBackedRuntimeStore::new(
            Arc::clone(&inner),
            Arc::clone(&failing) as Arc<dyn SessionStore>,
        ));
        // Outage on the SECOND missing commit: mid-chain.
        failing
            .fail_on_call
            .store(2, std::sync::atomic::Ordering::SeqCst);
        let authority =
            meerkat_runtime::RuntimeStore::load_whole_blob_store_authority(&*store, &runtime_id)
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .unwrap_or_else(|| panic!("the seeded runtime authority must exist"));
        let prepared = meerkat_runtime::store::PreparedWholeBlobSnapshotCas::prepare(
            authority,
            meerkat_core::lifecycle::core_executor::BoundSessionCommit::sealed(Arc::new(
                successor.clone(),
            ))
            .unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let error = meerkat_runtime::RuntimeStore::commit_prepared_whole_blob_snapshot_cas(
            &*store,
            &runtime_id,
            prepared,
        )
        .await
        .expect_err("a mid-chain projection outage must fail the committing verb");
        assert!(
            error.to_string().contains("generation 2"),
            "the failure must name the refused step: {error}"
        );
        assert_eq!(
            failing
                .rewrite_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            2,
            "generation 1 installed, generation 2 attempted and refused"
        );
        let after_failure = failing
            .load(gen0.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the durable row must survive the outage"));
        assert_eq!(
            after_failure
                .transcript_rewrite_generation()
                .unwrap_or_else(|error| panic!("{error}")),
            1,
            "the monotonic progress before the outage must stand"
        );

        // The exact retried step converges: only the REMAINING commit is
        // installed (call 3 is generation 2 again; generation 1 is not
        // re-written), and the durable row reaches the committed successor.
        failing
            .fail_on_call
            .store(0, std::sync::atomic::Ordering::SeqCst);
        store
            .project_committed_session_to_durable(&runtime_id)
            .await
            .unwrap_or_else(|error| panic!("the exact retry must converge: {error}"));
        assert_eq!(
            failing
                .rewrite_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            3,
            "the retry must install only the remaining commit"
        );
        let converged = failing
            .load(gen0.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the durable row must exist after retry"));
        assert_eq!(
            converged
                .transcript_rewrite_generation()
                .unwrap_or_else(|error| panic!("{error}")),
            2,
            "the retried projection must install the remaining generation"
        );
        assert_eq!(
            converged
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            successor
                .transcript_revision()
                .unwrap_or_else(|error| panic!("{error}")),
            "the durable row must converge on the committed successor"
        );
    }

    /// Direction pin for the staleness freshen (advisory Form 1, third
    /// leg). Durable strictly newer reseeds (the stale-runtime-snapshot
    /// lanes) and a runtime legitimately ahead stays untouched (the
    /// projection-failure regression above); this test pins the remaining
    /// direction: GENUINE DIVERGENCE — a durable row that orders newer but
    /// does not extend the committed snapshot — surfaces the inner store's
    /// typed boundary-guard refusal, loudly and repeatably, never a silent
    /// pick-a-winner adoption in either direction.
    #[tokio::test]
    async fn freshen_refuses_divergent_durable_row_typed_instead_of_adopting() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        // The boundary save guard lives in the store; the SQLite runtime
        // store enforces it on every snapshot commit (InMemory does not),
        // so the refusal under test is the real store-issued one.
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new(dir.path().join("runtime.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        // One session identity, two documents that share no common tail:
        // the committed runtime authority holds [committed turn], the
        // durable row holds [divergent turn, divergent follow-up] — newer
        // by the (rewrite generation, message count) order, but a fork.
        let seed = meerkat_core::Session::new();
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(seed.id());
        let mut committed = seed.clone();
        committed.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("committed turn"),
        ));
        let mut divergent = seed;
        divergent.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("divergent turn"),
        ));
        divergent.push(meerkat_core::Message::User(
            meerkat_core::types::UserMessage::text("divergent follow-up"),
        ));
        inner
            .commit_session_snapshot(
                &runtime_id,
                meerkat_runtime::store::SerializedSessionSnapshot {
                    session_snapshot: Arc::new(
                        committed
                            .to_persisted_bytes()
                            .unwrap_or_else(|error| panic!("{error}")),
                    ),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        session_store
            .save(&divergent)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let store = Arc::new(SessionStoreBackedRuntimeStore::new(
            Arc::clone(&inner),
            Arc::clone(&session_store),
        ));
        for attempt in ["first read", "retry"] {
            let refused = meerkat_runtime::RuntimeStore::load_committed_whole_blob_snapshot(
                &*store,
                &runtime_id,
            )
            .await
            .expect_err("a divergent durable row must refuse the freshen typed, not adopt");
            assert!(
                refused.to_string().contains("not a continuation"),
                "the {attempt} refusal must be the boundary guard's continuity violation: {refused}"
            );
        }
        // Neither side was silently rewritten by the refused freshen.
        let retained = inner
            .load_committed_whole_blob_snapshot(&runtime_id)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the committed runtime authority must survive"));
        assert_eq!(
            retained.session().messages().len(),
            1,
            "the committed runtime document must be untouched"
        );
        let durable = session_store
            .load(committed.id())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("the durable row must survive"));
        assert_eq!(
            durable.messages().len(),
            2,
            "the durable document must be untouched"
        );
    }

    #[tokio::test]
    async fn session_store_backed_runtime_store_forwards_defaulted_authority_seams() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let inner: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new(dir.path().join("runtime.db"))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let store = SessionStoreBackedRuntimeStore::new(inner, session_store);
        let runtime_id = meerkat_runtime::LogicalRuntimeId::new("runtime-store-facade-proof");
        let registry = meerkat_runtime::ops_lifecycle::RuntimeOpsLifecycleRegistry::new();
        let candidate = registry
            .capture_persistence_snapshot(
                meerkat_core::RuntimeEpochId::new(),
                &meerkat_core::EpochCursorState::new(),
            )
            .unwrap_or_else(|error| panic!("{error}"));

        let initialized = meerkat_runtime::RuntimeStore::initialize_ops_lifecycle_if_absent(
            &store,
            &runtime_id,
            &candidate,
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(initialized.epoch_id, candidate.epoch_id);

        assert!(
            meerkat_runtime::RuntimeStore::put_mob_host_binding_if_absent(
                &store,
                "mob-a",
                b"binding-v1",
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"))
        );
        assert!(
            meerkat_runtime::RuntimeStore::compare_and_put_mob_host_binding(
                &store,
                "mob-a",
                b"binding-v1",
                b"binding-v2",
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"))
        );
        assert_eq!(
            meerkat_runtime::RuntimeStore::load_mob_host_binding(&store, "mob-a")
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            Some(b"binding-v2".to_vec())
        );
        assert!(
            meerkat_runtime::RuntimeStore::delete_mob_host_binding(&store, "mob-a", b"binding-v2",)
                .await
                .unwrap_or_else(|error| panic!("{error}"))
        );

        assert!(
            meerkat_runtime::RuntimeStore::put_mob_host_binding_if_absent(
                &store, "mob-b", b"binding",
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"))
        );
        assert!(
            meerkat_runtime::RuntimeStore::revoke_mob_host_binding(
                &store, "mob-b", b"binding", b"receipt",
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"))
        );
        assert_eq!(
            meerkat_runtime::RuntimeStore::load_mob_host_revocation(&store, "mob-b")
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            Some(b"receipt".to_vec())
        );
        assert_eq!(
            meerkat_runtime::RuntimeStore::list_mob_host_revocations(&store)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            vec![("mob-b".to_string(), b"receipt".to_vec())]
        );
        assert!(
            meerkat_runtime::RuntimeStore::list_mob_host_bindings(&store)
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .is_empty()
        );
        assert_eq!(
            meerkat_runtime::RuntimeStore::list_mob_host_revocations(&store)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            vec![("mob-b".to_string(), b"receipt".to_vec())]
        );
    }

    /// Regression for two compounding bugs in the persistent wiring:
    ///
    /// 1. **0.6.0**: `persistent_inner` handed the
    ///    `PersistentSessionService` an `InMemoryRuntimeStore`. With the
    ///    runtime_store path active the `StoreCheckpointer` was disabled
    ///    (it's gated on `runtime_store.is_none()`), and the in-memory
    ///    store didn't survive process restart. Resume raised "missing
    ///    durable session snapshot for '<sid>'".
    ///
    /// 2. **0.6.1**: switching the session service to `runtime_store=None`
    ///    re-enabled the checkpointer (fixing #1) but broke archive/retire,
    ///    because `load_persisted_session_for_control` rejects mutations
    ///    when runtime_store is None and the session exists in the store
    ///    (the "store-only compatibility projection" error from
    ///    meerkat-session/src/persistent.rs:786).
    ///
    /// The 0.6.3 fix uses a **persistent** SqliteRuntimeStore — durable
    /// across restart AND control-op authoritative — at
    /// `<store_path>/runtime.sqlite`.
    #[test]
    fn persistent_bootstrap_uses_sqlite_runtime_store() {
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
            store_path.clone(),
            4,
            session_store,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            spec.runtime_adapter.is_some(),
            "persistent bootstrap must provide its own runtime adapter via spec.runtime_adapter"
        );
        assert!(
            spec.session_service.runtime_adapter().is_some(),
            "session service must own a runtime_store so archive/retire don't \
             hit the store-only-projection rejection"
        );
        assert!(
            store_path.join("runtime.sqlite").exists(),
            "persistent_inner must open a SqliteRuntimeStore at <store_path>/runtime.sqlite"
        );
    }

    /// H1: a persistent-mode blob-dir open failure is a startup error —
    /// never the former silent in-memory fallback (the GKE hazard).
    #[test]
    fn persistent_spec_fails_closed_when_blob_dir_unopenable() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        // A regular FILE at <store_path>/blobs makes the blob-dir open fail.
        std::fs::write(store_path.join("blobs"), b"not a directory")
            .unwrap_or_else(|e| panic!("{e}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(store_path.join("sessions.db"))
                .unwrap_or_else(|e| panic!("{e}")),
        );
        let definition = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"test\"\n")
            .unwrap_or_else(|e| panic!("{e}"));

        match MobBootstrapSpec::persistent(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path.clone(),
            4,
            session_store,
        ) {
            Err(StorageResolutionError::Blob(BlobStoreResolutionError::OpenFailed {
                path,
                ..
            })) => {
                assert_eq!(path, store_path.join("blobs"));
            }
            Err(other) => panic!("expected OpenFailed, got: {other}"),
            Ok(_) => {
                panic!("blob-dir open failure must fail closed, not fall back to in-memory blobs")
            }
        }
    }

    /// M4: a persistent-mode runtime-store open failure is a startup error —
    /// never the former silent `InMemoryRuntimeStore` twin. The in-memory
    /// form composes only as the explicit declaration and is census-visible.
    #[test]
    fn persistent_spec_fails_closed_when_runtime_store_unopenable() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        // A DIRECTORY at <store_path>/runtime.sqlite makes the SQLite open fail.
        std::fs::create_dir_all(store_path.join(crate::storage_layout::RUNTIME_DB_FILE_NAME))
            .unwrap_or_else(|e| panic!("{e}"));
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(store_path.join("sessions.db"))
                .unwrap_or_else(|e| panic!("{e}")),
        );
        let definition = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"test\"\n")
            .unwrap_or_else(|e| panic!("{e}"));

        match MobBootstrapSpec::persistent(
            definition.clone(),
            meerkat_mob::MobStorage::in_memory(),
            store_path.clone(),
            4,
            session_store.clone(),
        ) {
            Err(StorageResolutionError::RuntimeStore(error)) => {
                assert_eq!(
                    error.path,
                    store_path.join(crate::storage_layout::RUNTIME_DB_FILE_NAME)
                );
                assert!(
                    error.to_string().contains("ephemeral_runtime_store"),
                    "the error must name the declaration remediation: {error}"
                );
            }
            Err(other) => panic!("expected a runtime-store resolution error, got: {other}"),
            Ok(_) => panic!(
                "runtime-store open failure must fail closed, not fall back to \
                 InMemoryRuntimeStore"
            ),
        }

        // The explicit declaration composes over the same broken file and is
        // recorded in the per-slot census.
        let spec = MobBootstrapSpec::persistent_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            session_store,
            "SqliteSessionStore",
            None,
            false,
            true,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        )
        .unwrap_or_else(|e| panic!("declared ephemeral runtime store must compose: {e}"));
        let summary = spec.resolved_storage.unwrap_or_else(|| panic!("summary"));
        let runtime_slot = summary
            .slots
            .iter()
            .find(|slot| slot.declaration.domain == "runtime")
            .unwrap_or_else(|| panic!("runtime slot recorded"));
        assert_eq!(
            runtime_slot.declaration.resolution,
            meerkat_core::DurabilityResolution::DeclaredEphemeral
        );
        assert_eq!(runtime_slot.backend, "InMemoryRuntimeStore");
    }

    /// H1: the happy persistent path reports disk-backed blobs and the
    /// incremental session capability (H2) on the resolved summary.
    #[test]
    fn persistent_spec_reports_persistent_disk_and_incremental_sessions() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(store_path.join("sessions.db"))
                .unwrap_or_else(|e| panic!("{e}")),
        );
        let definition = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"test\"\n")
            .unwrap_or_else(|e| panic!("{e}"));

        let spec = MobBootstrapSpec::persistent(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            session_store,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let summary = spec.resolved_storage.unwrap_or_else(|| panic!("summary"));
        assert_eq!(summary.blob_durability, BlobDurability::PersistentDisk);
        assert_eq!(summary.session_store_incremental, Some(true));
        // The M4 per-slot census: durable slots persistent, the ring buffers
        // classified Scratch explicitly.
        for (domain, backend) in [
            ("runtime", "SqliteRuntimeStore"),
            ("workgraph", "SqliteWorkGraphStore"),
        ] {
            let slot = summary
                .slots
                .iter()
                .find(|slot| slot.declaration.domain == domain)
                .unwrap_or_else(|| panic!("{domain} slot recorded"));
            assert_eq!(slot.backend, backend);
            assert_eq!(
                slot.declaration.resolution,
                meerkat_core::DurabilityResolution::Persistent
            );
        }
        for domain in ["gating_audit", "delivery_history", "routing_resolutions"] {
            let slot = summary
                .slots
                .iter()
                .find(|slot| slot.declaration.domain == domain)
                .unwrap_or_else(|| panic!("{domain} ring buffer classified"));
            assert_eq!(
                slot.declaration.class,
                meerkat_core::DurabilityClass::Scratch
            );
        }
    }

    /// H1: persistent mode with a custom blob store reporting
    /// `!is_persistent()` is a startup error without the explicit
    /// declaration, and accepted (reported as non-persistent custom) with it.
    #[test]
    fn persistent_spec_gates_non_persistent_custom_blob_store_on_declaration() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(store_path.join("sessions.db"))
                .unwrap_or_else(|e| panic!("{e}")),
        );
        let memory_blobs: Arc<dyn meerkat_core::BlobStore> = Arc::new(Base64BlobStoreAdapter::new(
            Arc::new(ObjectStoreBlobStore::memory()),
        ));
        let definition = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"test\"\n")
            .unwrap_or_else(|e| panic!("{e}"));

        match MobBootstrapSpec::persistent_inner(
            definition.clone(),
            meerkat_mob::MobStorage::in_memory(),
            store_path.clone(),
            4,
            session_store.clone(),
            "SqliteSessionStore",
            Some(BlobStoreInjection::Core(memory_blobs.clone())),
            false,
            false,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        ) {
            Err(StorageResolutionError::Blob(
                BlobStoreResolutionError::NonPersistentUndeclared,
            )) => {}
            Err(other) => panic!("expected NonPersistentUndeclared, got: {other}"),
            Ok(_) => panic!("undeclared non-persistent custom blob store must fail composition"),
        }

        let spec = MobBootstrapSpec::persistent_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            session_store,
            "SqliteSessionStore",
            Some(BlobStoreInjection::Core(memory_blobs)),
            true,
            false,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        )
        .unwrap_or_else(|e| panic!("declared ephemeral blobs must compose: {e}"));
        let summary = spec.resolved_storage.unwrap_or_else(|| panic!("summary"));
        assert_eq!(
            summary.blob_durability,
            BlobDurability::Custom { persistent: false }
        );
        assert_eq!(summary.session_store_incremental, Some(true));
    }

    /// H1: `ephemeral_blobs` without a custom store is a declared in-memory
    /// choice — no `blobs/` directory materializes on disk.
    #[test]
    fn persistent_spec_declared_ephemeral_blobs_skips_disk() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let session_store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(store_path.join("sessions.db"))
                .unwrap_or_else(|e| panic!("{e}")),
        );
        let definition = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"test\"\n")
            .unwrap_or_else(|e| panic!("{e}"));

        let spec = MobBootstrapSpec::persistent_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path.clone(),
            4,
            session_store,
            "SqliteSessionStore",
            None,
            true,
            false,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            spec.resolved_storage.map(|summary| summary.blob_durability),
            Some(BlobDurability::DeclaredEphemeral)
        );
        assert!(
            !store_path.join("blobs").exists(),
            "declared-ephemeral blobs must not touch the disk blob root"
        );
    }

    /// H1: the ephemeral-by-design mode records its declaration; H2 is not
    /// applicable without a persistent session service.
    #[test]
    fn ephemeral_runtime_backed_spec_reports_declared_ephemeral() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let definition = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"test\"\n")
            .unwrap_or_else(|e| panic!("{e}"));

        let spec = MobBootstrapSpec::ephemeral_runtime_backed_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            dir.path().to_path_buf(),
            4,
            None,
            "test session store",
            None,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        );
        let summary = spec.resolved_storage.unwrap_or_else(|| panic!("summary"));
        assert_eq!(summary.blob_durability, BlobDurability::DeclaredEphemeral);
        assert_eq!(summary.session_store_incremental, None);
        // Every by-mode in-memory slot is a DECLARED choice in the census.
        for domain in ["sessions", "runtime", "blobs", "workgraph"] {
            let slot = summary
                .slots
                .iter()
                .find(|slot| slot.declaration.domain == domain)
                .unwrap_or_else(|| panic!("{domain} slot recorded"));
            assert_eq!(
                slot.declaration.resolution,
                meerkat_core::DurabilityResolution::DeclaredEphemeral,
                "{domain} must be a declared ephemeral choice"
            );
        }
    }

    /// Ephemeral counterpart: runtime-backed ephemeral builds must use a
    /// single in-memory machine authority for session service, comms, and
    /// image-generation tooling.
    #[test]
    fn ephemeral_runtime_backed_uses_session_service_runtime_adapter() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let Ok(definition) = meerkat_mob::MobDefinition::from_toml("[mob]\nid = \"test\"\n") else {
            panic!("failed to parse minimal mob definition");
        };
        let spec = MobBootstrapSpec::ephemeral_runtime_backed_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            None,
            "test session store",
            None,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        );
        assert!(
            spec.runtime_adapter.is_some(),
            "ephemeral_runtime_backed_inner must expose the shared runtime authority"
        );
        assert!(
            spec.session_service.runtime_adapter().is_some(),
            "session service must still expose a runtime adapter so autonomous-host comms can wire"
        );
    }

    #[tokio::test]
    async fn agent_mob_tools_expose_definition_profiles_as_realm_profiles() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let Ok(definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"test\"\n\n[profiles.investigation-worker]\nmodel = \"gpt-5.5\"\n[profiles.investigation-worker.tools]\ncomms = true\nmob = true\n\n[profiles.person-worker]\nmodel = \"gpt-5.5\"\n[profiles.person-worker.tools]\ncomms = true\n",
        ) else {
            panic!("failed to parse mob definition with worker profiles");
        };

        let spec = MobBootstrapSpec::ephemeral_runtime_backed_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            None,
            "test session store",
            None,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        );
        let state = spec
            .agent_mob_mcp_state
            .expect("agent mob MCP state should be installed");

        let profiles = state
            .realm_profile_list()
            .await
            .expect("definition profiles should list through agent mob tools");
        let names = profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>();
        assert!(
            names.contains(&"investigation-worker"),
            "definition profiles must be visible to mob_profile_list so agents can create mobs that reference them"
        );
        assert!(names.contains(&"person-worker"));

        let worker = state
            .realm_profile_get("investigation-worker")
            .await
            .expect("definition profile lookup should succeed")
            .expect("definition profile should exist");
        assert_eq!(worker.profile.model, "gpt-5.5");
        assert_eq!(
            worker.revision, 0,
            "definition-backed profiles are immutable runtime seeds, not persisted realm revisions"
        );
    }

    #[tokio::test]
    async fn agent_created_mobs_can_spawn_definition_seeded_realm_profiles() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let Ok(parent_definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"parent\"\n\n[profiles.investigation-worker]\nmodel = \"gpt-5.5\"\n[profiles.investigation-worker.tools]\ncomms = true\nmob = true\n",
        ) else {
            panic!("failed to parse parent mob definition");
        };

        let mut spec = MobBootstrapSpec::ephemeral_runtime_backed_inner(
            parent_definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            None,
            "test session store",
            None,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        );
        spec.options.default_llm_client = Some(Arc::new(meerkat_client::TestClient::default()));
        let runtime = MobRuntime::bootstrap(spec)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let state = runtime
            .agent_mob_mcp_state
            .clone()
            .expect("agent mob MCP state should be installed");
        let Ok(child_definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"child\"\n\n[profiles.investigation-worker]\nrealm_profile = \"investigation-worker\"\n",
        ) else {
            panic!("failed to parse child mob definition");
        };

        let mob_id = Box::pin(state.mob_create_definition(child_definition))
            .await
            .expect("child mob should be created");
        Box::pin(state.mob_spawn_spec(
            &mob_id,
            SpawnMemberSpec::new(
                ProfileName::from("investigation-worker"),
                // meerkat 0.7: MemberCommsName is fail-closed; raw mob
                // member ids must be identifier-safe (no ":").
                meerkat_mob::AgentIdentity::from("investigation-worker-one"),
            ),
        ))
        .await
        .expect("created mob should resolve definition-seeded realm profile at spawn time");
    }

    /// EXPLICIT ADAPTER CONTRACT (meerkat 0.8.11; formerly the service-owned
    /// dual-write expectation): the session service no longer writes the
    /// SessionStore itself - a created session reaches the injected store
    /// only through `SessionStoreBackedRuntimeStore`'s committed-boundary
    /// write-through, which is external-authoritative (an external write
    /// failure fails the committing verb). This test pins that first-turn
    /// half of the round trip.
    #[tokio::test]
    async fn ephemeral_runtime_backed_custom_session_store_persists_created_session() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let Ok(definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"test\"\n\n[profiles.worker]\nmodel = \"gpt-5.5\"\n[profiles.worker.tools]\ncomms = true\n",
        ) else {
            panic!("failed to parse minimal mob definition");
        };
        let custom_store: Arc<dyn SessionStore> = Arc::new(meerkat_store::MemoryStore::new());
        let mut spec = MobBootstrapSpec::ephemeral_runtime_backed_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            Some(custom_store.clone()),
            "test session store",
            None,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        );
        spec.options.default_llm_client = Some(Arc::new(meerkat_client::TestClient::default()));

        let runtime = MobRuntime::bootstrap(spec)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        // meerkat 0.7: MemberCommsName is fail-closed; raw mob member ids
        // must be identifier-safe (no ":").
        let mid = meerkat_mob::ids::AgentIdentity::from("worker-one");
        Box::pin(runtime.handle.spawn_spec(SpawnMemberSpec::new(
            ProfileName::from("worker"),
            mid.clone(),
        )))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
        let session_id = runtime
            .handle
            .resolve_bridge_session_id(&mid)
            .await
            .unwrap_or_else(|| panic!("spawned worker has no bridge session id"));

        let stored = custom_store
            .load(&session_id)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            stored.is_some(),
            "ephemeral runtime-backed builds with a custom store must persist through that store"
        );
    }

    /// EXPLICIT ADAPTER CONTRACT (meerkat 0.8.11), the kill/restart half of
    /// the round trip: the external durable row written by the facade's
    /// committed-boundary write-through is the authoritative predecessor the
    /// next boot imports - a cold (empty) inner runtime store re-mints
    /// store-issued authority from it and resume serves the exact projected
    /// transcript.
    #[tokio::test]
    async fn ephemeral_runtime_backed_custom_session_store_resumes_after_runtime_restart() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let definition_toml = "[mob]\nid = \"test\"\n\n[profiles.worker]\nmodel = \"gpt-5.5\"\n[profiles.worker.tools]\ncomms = true\n";
        let Ok(definition) = meerkat_mob::MobDefinition::from_toml(definition_toml) else {
            panic!("failed to parse minimal mob definition");
        };
        let custom_store: Arc<dyn SessionStore> = Arc::new(meerkat_store::MemoryStore::new());
        let mut spec = MobBootstrapSpec::ephemeral_runtime_backed_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path.clone(),
            4,
            Some(custom_store.clone()),
            "test session store",
            None,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        );
        spec.options.default_llm_client = Some(Arc::new(meerkat_client::TestClient::default()));

        let runtime = MobRuntime::bootstrap(spec)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        // meerkat 0.7: MemberCommsName is fail-closed; raw mob member ids
        // must be identifier-safe (no ":").
        let mid = meerkat_mob::ids::AgentIdentity::from("worker-one");
        Box::pin(runtime.handle.spawn_spec(SpawnMemberSpec::new(
            ProfileName::from("worker"),
            mid.clone(),
        )))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
        let session_id = runtime
            .handle
            .resolve_bridge_session_id(&mid)
            .await
            .unwrap_or_else(|| panic!("spawned worker has no bridge session id"));
        // Dropping `MobRuntime` is not a process boundary: the actor and its
        // checkpointer own independent handles and may still append to the
        // custom store. Starting the replacement at that point creates two
        // live writers, not a restart. The public shutdown boundary joins
        // those volatile producers before the replacement reads durable state.
        runtime
            .handle
            .shutdown()
            .await
            .unwrap_or_else(|e| panic!("failed to quiesce pre-restart runtime: {e}"));
        let before_restart = custom_store
            .load(&session_id)
            .await
            .unwrap_or_else(|e| panic!("failed to load pre-restart session: {e}"))
            .unwrap_or_else(|| panic!("spawned worker was not projected before restart"));
        let before_restart_revision =
            meerkat_core::transcript_messages_digest(before_restart.messages())
                .unwrap_or_else(|e| panic!("failed to digest pre-restart transcript: {e}"));
        let before_restart_message_count = before_restart.messages().len();
        assert!(
            before_restart_message_count > 1,
            "restart fixture must hold a nontrivial transcript before resume"
        );
        drop(runtime);

        let Ok(definition) = meerkat_mob::MobDefinition::from_toml(definition_toml) else {
            panic!("failed to parse minimal mob definition");
        };
        let mut restarted_spec = MobBootstrapSpec::ephemeral_runtime_backed_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            Some(custom_store.clone()),
            "test session store",
            None,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        );
        restarted_spec.options.default_llm_client =
            Some(Arc::new(meerkat_client::TestClient::default()));

        let restarted = MobRuntime::bootstrap(restarted_spec)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let mut resume_spec = SpawnMemberSpec::new(ProfileName::from("worker"), mid.clone());
        resume_spec.launch_mode = meerkat_mob::MemberLaunchMode::Resume {
            bridge_session_id: session_id.clone(),
        };
        Box::pin(restarted.handle.spawn_spec(resume_spec))
            .await
            .unwrap_or_else(|e| panic!("resume should load the external session snapshot: {e}"));

        let resumed_session_id = restarted
            .handle
            .resolve_bridge_session_id(&mid)
            .await
            .unwrap_or_else(|| panic!("resumed worker has no bridge session id"));
        assert_eq!(resumed_session_id, session_id);
        let after_restart = custom_store
            .load(&session_id)
            .await
            .unwrap_or_else(|e| panic!("failed to load resumed session: {e}"))
            .unwrap_or_else(|| panic!("resumed worker lost its durable projection"));
        assert_eq!(
            after_restart.messages().len(),
            before_restart_message_count,
            "turnless resume must not shrink the durable transcript"
        );
        assert_eq!(
            meerkat_core::transcript_messages_digest(after_restart.messages())
                .unwrap_or_else(|e| panic!("failed to digest resumed transcript: {e}")),
            before_restart_revision,
            "turnless resume must preserve the exact durable transcript"
        );
        restarted
            .handle
            .shutdown()
            .await
            .unwrap_or_else(|e| panic!("failed to quiesce resumed runtime: {e}"));
    }

    /// Regression: public ephemeral builds without image generation retain the
    /// exact runtime authority carrying their live LLM reconfiguration host.
    #[test]
    fn ephemeral_bootstrap_without_image_generation_retains_runtime_host() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let Ok(definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"test\"\n\n[profiles.worker]\nmodel = \"gpt-5.5\"\nruntime_mode = \"autonomous_host\"\n[profiles.worker.tools]\ncomms = true\n",
        ) else {
            panic!("failed to parse definition");
        };
        let spec = MobBootstrapSpec::ephemeral_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        );
        let spec_adapter = spec
            .runtime_adapter
            .as_ref()
            .unwrap_or_else(|| panic!("public ephemeral builds must retain their runtime adapter"));
        let service_adapter = spec
            .session_service
            .runtime_adapter()
            .unwrap_or_else(|| panic!("session service must expose the retained runtime adapter"));
        assert!(
            Arc::ptr_eq(spec_adapter, &service_adapter),
            "the spec and session service must share one exact runtime authority"
        );
        assert!(
            spec_adapter.has_session_llm_reconfigure_host(),
            "the retained runtime authority must carry the live LLM reconfiguration host"
        );
    }

    /// "Profile declares it, profile means it" (2026-07 resume-inertness
    /// traps): every explicitly declared profile field is auto-marked
    /// resume-overridden so profile edits reach resumed durable identities.
    #[test]
    fn auto_mark_marks_declared_profile_fields() {
        let Ok(mut definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"auto-mark\"\n\n[profiles.worker]\nmodel = \"claude-opus-4-8\"\nprovider = \"anthropic\"\n[profiles.worker.provider_params]\nthinking_budget_tokens = 1024\n",
        ) else {
            panic!("failed to parse definition");
        };
        auto_mark_declared_resume_overrides(&mut definition);
        let profile = definition
            .profiles
            .get(&ProfileName::from("worker"))
            .and_then(|binding| binding.as_inline())
            .unwrap_or_else(|| panic!("worker profile must be inline"));
        for field in [
            meerkat_mob::ResumeOverrideField::Model,
            meerkat_mob::ResumeOverrideField::Provider,
            meerkat_mob::ResumeOverrideField::ProviderParams,
        ] {
            assert!(
                profile.resume_overrides.contains(&field),
                "declared field {field:?} must be auto-marked resume-overridden"
            );
        }
    }

    /// Model + provider are a coherent pair (OB3 cutover incident): a
    /// declared model with no declared provider derives the provider from
    /// the canonical catalog and applies BOTH — masking the model alone
    /// would let the durable provider survive under a model it was never
    /// registered for, and the resume would be rejected typed. Undeclared
    /// provider_params keep durable-wins.
    #[test]
    fn auto_mark_derives_provider_from_catalog_for_declared_model() {
        let Ok(mut definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"auto-mark\"\n\n[profiles.worker]\nmodel = \"gpt-5.5\"\n",
        ) else {
            panic!("failed to parse definition");
        };
        auto_mark_declared_resume_overrides(&mut definition);
        let profile = definition
            .profiles
            .get(&ProfileName::from("worker"))
            .and_then(|binding| binding.as_inline())
            .unwrap_or_else(|| panic!("worker profile must be inline"));
        assert_eq!(
            profile.provider,
            Some(Provider::OpenAI),
            "the pair's provider must be derived from the catalog and written onto the profile"
        );
        assert!(
            profile
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::Model)
                && profile
                    .resume_overrides
                    .contains(&meerkat_mob::ResumeOverrideField::Provider),
            "model and provider must be masked together, never independently"
        );
        assert!(
            !profile
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::ProviderParams),
            "undeclared provider_params must keep durable-wins"
        );
    }

    /// The OB3 incident shape must be impossible: an explicit
    /// `resume_overrides = ["model", "provider"]` with NO provider key used
    /// to apply the profile model while the durable provider survived
    /// (profile provider was None → nothing to apply), minting invalid
    /// pairs like (claude-fable-5, openai). The auto-mark now writes the
    /// catalog-derived provider onto the profile so the pair applies
    /// atomically.
    #[test]
    fn auto_mark_completes_the_pair_for_explicit_model_provider_mask() {
        let Ok(mut definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"auto-mark\"\n\n[profiles.worker]\nmodel = \"claude-opus-4-8\"\nresume_overrides = [\"model\", \"provider\"]\n",
        ) else {
            panic!("failed to parse definition");
        };
        auto_mark_declared_resume_overrides(&mut definition);
        let profile = definition
            .profiles
            .get(&ProfileName::from("worker"))
            .and_then(|binding| binding.as_inline())
            .unwrap_or_else(|| panic!("worker profile must be inline"));
        assert_eq!(
            profile.provider,
            Some(Provider::Anthropic),
            "an explicit model+provider mask with no provider key must gain the catalog \
             provider, or the mask applies the model against the durable provider"
        );
        assert_eq!(
            profile
                .resume_overrides
                .iter()
                .filter(|field| **field == meerkat_mob::ResumeOverrideField::Provider)
                .count(),
            1,
            "the explicit mask entry must not be duplicated"
        );
    }

    /// A catalog-unknown model falls back to the definition's
    /// `[models.<id>]` entry for the pair's provider; with no entry at all,
    /// NEITHER field is marked (no coherent pair exists — the divergence
    /// line is the tripwire) and nothing panics.
    #[test]
    fn auto_mark_unknown_model_uses_config_entry_or_stays_durable_wins() {
        let Ok(mut definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"auto-mark\"\n\n[models.house-llm]\nprovider = \"openai\"\n\n[profiles.custom]\nmodel = \"house-llm\"\n\n[profiles.orphan]\nmodel = \"nobody-knows-this-model\"\n[profiles.orphan.provider_params]\nthinking_budget_tokens = 64\n",
        ) else {
            panic!("failed to parse definition");
        };
        auto_mark_declared_resume_overrides(&mut definition);
        let custom = definition
            .profiles
            .get(&ProfileName::from("custom"))
            .and_then(|binding| binding.as_inline())
            .unwrap_or_else(|| panic!("custom profile must be inline"));
        assert_eq!(
            custom.provider,
            Some(Provider::OpenAI),
            "a [models.<id>] entry owns the pair's provider for uncatalogued models"
        );
        assert!(
            custom
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::Model)
                && custom
                    .resume_overrides
                    .contains(&meerkat_mob::ResumeOverrideField::Provider),
            "config-entry models mask the pair together"
        );
        let orphan = definition
            .profiles
            .get(&ProfileName::from("orphan"))
            .and_then(|binding| binding.as_inline())
            .unwrap_or_else(|| panic!("orphan profile must be inline"));
        assert_eq!(
            orphan.provider, None,
            "no coherent provider source: the profile must not gain one"
        );
        assert!(
            !orphan
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::Model)
                && !orphan
                    .resume_overrides
                    .contains(&meerkat_mob::ResumeOverrideField::Provider),
            "without a coherent pair neither field is masked — durable truth wins whole"
        );
        assert!(
            orphan
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::ProviderParams),
            "provider_params declaration is independent of the LLM-identity pair"
        );
    }

    /// Both declared: the pair is masked exactly as written — including a
    /// pair the catalog would contradict. Auto-mark must not silently
    /// "repair" an explicit declaration; the build-time registry rejection
    /// then names the user's own (model, provider), not a minted one.
    #[test]
    fn auto_mark_honors_a_declared_pair_as_written() {
        let Ok(mut definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"auto-mark\"\n\n[profiles.worker]\nmodel = \"claude-opus-4-8\"\nprovider = \"openai\"\n",
        ) else {
            panic!("failed to parse definition");
        };
        auto_mark_declared_resume_overrides(&mut definition);
        let profile = definition
            .profiles
            .get(&ProfileName::from("worker"))
            .and_then(|binding| binding.as_inline())
            .unwrap_or_else(|| panic!("worker profile must be inline"));
        assert_eq!(
            profile.provider,
            Some(Provider::OpenAI),
            "a declared provider is honored as written, never catalog-corrected"
        );
        assert!(
            profile
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::Model)
                && profile
                    .resume_overrides
                    .contains(&meerkat_mob::ResumeOverrideField::Provider),
            "a declared pair masks together"
        );
    }

    /// A `self_hosted_server_id` binding is only meaningful under the
    /// self_hosted provider: the pair adopts that reading and masks
    /// together, instead of leaving a masked-but-absent provider whose
    /// resume application falls back to the durable one under the declared
    /// model.
    #[test]
    fn auto_mark_adopts_self_hosted_for_server_binding() {
        let Ok(mut definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"auto-mark\"\n\n[profiles.worker]\nmodel = \"house-llm\"\nself_hosted_server_id = \"srv-1\"\n",
        ) else {
            panic!("failed to parse definition");
        };
        auto_mark_declared_resume_overrides(&mut definition);
        let profile = definition
            .profiles
            .get(&ProfileName::from("worker"))
            .and_then(|binding| binding.as_inline())
            .unwrap_or_else(|| panic!("worker profile must be inline"));
        assert_eq!(
            profile.provider,
            Some(Provider::SelfHosted),
            "a server binding pins the pair to self_hosted"
        );
        assert!(
            profile
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::Model)
                && profile
                    .resume_overrides
                    .contains(&meerkat_mob::ResumeOverrideField::Provider),
            "the self-hosted pair masks together"
        );
    }

    /// An explicit `resume_overrides = ["provider"]` with no provider key is
    /// the mirror of the OB3 shape: a provider-only mask over a None profile
    /// provider applies NOTHING (resume falls back to durable), while the
    /// declared model stays unmasked. Auto-mark completes it to the full
    /// pair — provider derived from the catalog, model added to the mask —
    /// so a provider-only application is structurally impossible.
    #[test]
    fn auto_mark_completes_explicit_provider_only_mask_to_the_pair() {
        let Ok(mut definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"auto-mark\"\n\n[profiles.worker]\nmodel = \"gpt-5.5\"\nresume_overrides = [\"provider\"]\n",
        ) else {
            panic!("failed to parse definition");
        };
        auto_mark_declared_resume_overrides(&mut definition);
        let profile = definition
            .profiles
            .get(&ProfileName::from("worker"))
            .and_then(|binding| binding.as_inline())
            .unwrap_or_else(|| panic!("worker profile must be inline"));
        assert_eq!(
            profile.provider,
            Some(Provider::OpenAI),
            "the masked provider must exist on the profile, derived from the declared model"
        );
        assert!(
            profile
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::Model),
            "the model joins the explicit provider mask: the pair is never split"
        );
        assert_eq!(
            profile
                .resume_overrides
                .iter()
                .filter(|field| **field == meerkat_mob::ResumeOverrideField::Provider)
                .count(),
            1,
            "the explicit provider entry must not be duplicated"
        );
    }

    /// An explicit `resume_overrides` list is preserved, declared fields are
    /// added without duplicates, and the pass is idempotent across boots.
    #[test]
    fn auto_mark_mixed_preserves_explicit_list_without_duplicates() {
        let Ok(mut definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"auto-mark\"\n\n[profiles.worker]\nmodel = \"gpt-5.5\"\nprovider = \"openai\"\nresume_overrides = [\"model\"]\n",
        ) else {
            panic!("failed to parse definition");
        };
        auto_mark_declared_resume_overrides(&mut definition);
        auto_mark_declared_resume_overrides(&mut definition);
        let profile = definition
            .profiles
            .get(&ProfileName::from("worker"))
            .and_then(|binding| binding.as_inline())
            .unwrap_or_else(|| panic!("worker profile must be inline"));
        let model_entries = profile
            .resume_overrides
            .iter()
            .filter(|field| **field == meerkat_mob::ResumeOverrideField::Model)
            .count();
        assert_eq!(
            model_entries, 1,
            "an explicitly listed field must not be duplicated"
        );
        assert!(
            profile
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::Provider),
            "declared provider must be added alongside the explicit list"
        );
        assert!(
            !profile
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::ProviderParams),
            "undeclared provider_params must stay durable-wins"
        );
    }

    /// The auto-mark runs at the single bootstrap ingress: the definition the
    /// runtime installs (the one resumes resolve profiles from) carries the
    /// declared-field masks without the host writing `resume_overrides`.
    #[tokio::test]
    async fn bootstrap_auto_marks_declared_resume_overrides() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let Ok(definition) = meerkat_mob::MobDefinition::from_toml(
            "[mob]\nid = \"auto-mark\"\n\n[profiles.worker]\nmodel = \"claude-opus-4-8\"\nprovider = \"anthropic\"\n",
        ) else {
            panic!("failed to parse definition");
        };
        let spec = MobBootstrapSpec::ephemeral_inner(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            None,
            None,
            CapabilityFlags::default(),
            None,
            None,
        );
        let runtime = MobRuntime::bootstrap(spec)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let profile = runtime
            .handle
            .definition()
            .profiles
            .get(&ProfileName::from("worker"))
            .and_then(|binding| binding.as_inline())
            .unwrap_or_else(|| panic!("worker profile must be inline"));
        assert!(
            profile
                .resume_overrides
                .contains(&meerkat_mob::ResumeOverrideField::Model)
                && profile
                    .resume_overrides
                    .contains(&meerkat_mob::ResumeOverrideField::Provider),
            "bootstrap must install the definition with declared fields auto-marked"
        );
        runtime
            .handle
            .shutdown()
            .await
            .unwrap_or_else(|e| panic!("failed to quiesce runtime: {e}"));
    }

    /// Regression: public ephemeral image-generation builds must expose the same
    /// runtime adapter through the spec and the session service. The generated
    /// image tool consults runtime session/image-operation state by session id,
    /// so a fresh, tool-only MeerkatMachine cannot be used here.
    #[test]
    fn ephemeral_bootstrap_with_image_generation_shares_runtime_adapter() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store_path = dir.path().to_path_buf();
        let Ok(definition) = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "test"

[profiles.commander]
model = "gpt-5.5"

[profiles.commander.tools]
builtins = true
image_generation = true
"#,
        ) else {
            panic!("failed to parse image-generation definition");
        };
        let spec = MobBootstrapSpec::ephemeral(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            store_path,
            4,
            None,
        );
        let spec_adapter = spec
            .runtime_adapter
            .as_ref()
            .expect("image-generation ephemeral builds must expose a runtime adapter");
        let service_adapter = spec
            .session_service
            .runtime_adapter()
            .expect("session service must expose the same runtime adapter");
        assert!(
            spec_adapter.shares_runtime_persistence_with(&service_adapter),
            "image-generation tool state and session state must share one runtime authority"
        );
    }

    /// Runtime-owned handling/routing semantics must be stripped before a
    /// runtime-applied turn reaches the direct session-service path.
    #[test]
    fn normalize_runtime_turn_request_strips_runtime_owned_semantics() {
        let req = meerkat_core::service::StartTurnRequest {
            prompt: meerkat_core::ContentInput::Text("checkpoint".to_string()),
            injected_context: Vec::new(),
            system_prompt: Some("system".to_string()),
            event_tx: None,
            runtime: meerkat_core::service::StartTurnRuntimeSemantics {
                input_identity: None,
                handling_mode: meerkat_core::types::HandlingMode::Steer,
                turn_tool_overlay: None,
                typed_turn_appends: Vec::new(),
                // Render metadata now lives only on the typed turn-metadata
                // carrier (meerkat 0.7).
                turn_metadata: Some(
                    meerkat_core::lifecycle::run_primitive::RuntimeTurnMetadata {
                        render_metadata: Some(meerkat_core::types::RenderMetadata {
                            class: meerkat_core::types::RenderClass::OpsProgress,
                            salience: meerkat_core::types::RenderSalience::Urgent,
                        }),
                        ..Default::default()
                    },
                ),
            },
        };

        let expected_prompt = req.prompt.clone();
        let expected_system_prompt = req.system_prompt.clone();

        let normalized = normalize_runtime_turn_request(req);

        assert_eq!(
            normalized.runtime.handling_mode,
            meerkat_core::types::HandlingMode::Queue,
            "runtime-applied turns must downgrade Steer before reaching direct session services"
        );
        assert!(
            normalized
                .runtime
                .turn_metadata
                .as_ref()
                .is_none_or(|metadata| metadata.render_metadata.is_none()),
            "runtime-owned render metadata must not be forwarded through the direct agent path"
        );
        assert_eq!(normalized.prompt, expected_prompt);
        assert_eq!(normalized.system_prompt, expected_system_prompt);
    }

    /// SessionCreatedContext must carry model, labels, and optional system_prompt.
    #[test]
    fn session_created_context_fields() {
        let ctx = SessionCreatedContext {
            model: "claude-sonnet-4-5".to_string(),
            labels: std::collections::BTreeMap::from([(
                "agent_type".to_string(),
                "lead".to_string(),
            )]),
            system_prompt: Some("You are a lead agent.".to_string()),
        };
        assert_eq!(ctx.model, "claude-sonnet-4-5");
        assert_eq!(ctx.labels["agent_type"], "lead");
        assert_eq!(ctx.system_prompt.as_deref(), Some("You are a lead agent."));
    }

    /// SessionHook default implementations are no-ops — calling them must not panic.
    #[tokio::test]
    async fn session_hook_default_impls_are_noop() {
        struct EmptyHook;
        #[async_trait]
        impl SessionHook for EmptyHook {}

        let hook = EmptyHook;
        let mut req = CreateSessionRequest {
            model: "test".to_string(),
            prompt: meerkat_core::ContentInput::Text("test".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            injected_context: Vec::new(),
        };
        // before_create must succeed with default impl.
        hook.before_create(&mut req).await.unwrap();
        // after_create must not panic.
        let ctx = SessionCreatedContext {
            model: "test".to_string(),
            labels: Default::default(),
            system_prompt: None,
        };
        hook.after_create(&meerkat_core::types::SessionId::new(), &ctx)
            .await;
    }

    /// before_create returning Err must abort (the caller decides how).
    #[tokio::test]
    async fn session_hook_before_create_can_abort() {
        struct AbortHook;
        #[async_trait]
        impl SessionHook for AbortHook {
            async fn before_create(
                &self,
                _req: &mut CreateSessionRequest,
            ) -> Result<(), SessionError> {
                Err(SessionError::Unsupported("hook abort".into()))
            }
        }

        let hook = AbortHook;
        let mut req = CreateSessionRequest {
            model: "test".to_string(),
            prompt: meerkat_core::ContentInput::Text("test".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            injected_context: Vec::new(),
        };
        let result = hook.before_create(&mut req).await;
        assert!(result.is_err());
    }

    /// before_create mutations must be visible in the request.
    #[tokio::test]
    async fn session_hook_before_create_mutates_request() {
        struct MutatingHook;
        #[async_trait]
        impl SessionHook for MutatingHook {
            async fn before_create(
                &self,
                req: &mut CreateSessionRequest,
            ) -> Result<(), SessionError> {
                req.model = "hook-overridden".to_string();
                req.system_prompt =
                    meerkat_core::config::SystemPromptOverride::Set("injected by hook".to_string());
                Ok(())
            }
        }

        let hook = MutatingHook;
        let mut req = CreateSessionRequest {
            model: "original".to_string(),
            prompt: meerkat_core::ContentInput::Text("test".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            injected_context: Vec::new(),
        };
        hook.before_create(&mut req).await.unwrap();
        assert_eq!(req.model, "hook-overridden");
        assert_eq!(req.system_prompt.as_set_prompt(), Some("injected by hook"));
    }

    #[test]
    fn recoverable_lifecycle_cleanup_accepts_ambiguous_member_cleanup() {
        let error = "previous member cleanup ambiguous for member rt:deep-investigator:singleton:0";

        assert!(is_previous_member_cleanup_ambiguous_error(error));
        assert!(is_recoverable_lifecycle_cleanup_error(error));
    }

    #[test]
    fn topology_restore_failed_peer_ids_extracts_tolerated_peers() {
        let identity = meerkat_mob::AgentIdentity::from("rt:review:singleton:0");
        let receipt = meerkat_mob::MemberRespawnReceipt::new(
            identity.clone(),
            meerkat_mob::AgentRuntimeId::new(identity, meerkat_mob::ids::Generation::INITIAL),
            meerkat_mob::FenceToken::new(1),
            meerkat_mob::FenceToken::new(2),
        );
        let err = meerkat_mob::MobRespawnError::TopologyRestoreFailed {
            receipt,
            failed_peer_ids: vec![meerkat_mob::RespawnTopologyPeerId::from(
                "initiative:broken",
            )],
        };

        assert_eq!(
            topology_restore_failed_peer_ids(&err),
            Some(vec!["initiative:broken".to_string()])
        );
        assert_eq!(
            topology_restore_failed_peer_ids(&meerkat_mob::MobRespawnError::NoRuntimeControl {
                identity: meerkat_mob::AgentIdentity::from("rt:review:singleton:0"),
            }),
            None
        );
    }

    #[test]
    fn topology_restore_warning_json_surfaces_isolated_peers() {
        let failed_peer_ids = vec!["initiative:broken".to_string(), "helper:cold".to_string()];

        assert_eq!(
            topology_restore_warning_json(&failed_peer_ids),
            serde_json::json!({
                "kind": "topology_restore_degraded",
                "failed_peer_ids": ["initiative:broken", "helper:cold"],
            })
        );
    }

    #[test]
    fn recoverable_lifecycle_cleanup_preserves_archive_cancel_race() {
        let error = "internal error: disposal completed but ArchiveSession failed: \
            session error: agent error: Internal error: runtime cancel-before-retire failed \
            for 019e3c52-0f1b-73d3-a5c7-4b21c2bbf131: Runtime not ready: running";

        assert!(is_recoverable_lifecycle_cleanup_error(error));
    }

    #[test]
    fn recoverable_lifecycle_cleanup_rejects_unrelated_errors() {
        assert!(!is_recoverable_lifecycle_cleanup_error(
            "actor task dropped"
        ));
        assert!(!is_recoverable_lifecycle_cleanup_error(
            "model provider returned rate limit"
        ));
    }

    /// Regression (meerkat 0.7.1): idle members sit in machine state
    /// `Stopped`, whose DSL authority rejects the archive step's final
    /// `Retire` input. Disposal completed, so retire/respawn must treat the
    /// failed bookkeeping transition as success instead of surfacing -32000.
    #[test]
    fn recoverable_lifecycle_cleanup_accepts_stopped_guard_archive_retire() {
        let error = "internal error: disposal completed but ArchiveSession failed: \
            session error: agent error: Internal error: machine archive retire failed \
            after registration: Internal error: DSL authority (Retire): guard rejected \
            transition from Stopped for input::Retire";

        assert!(is_recoverable_lifecycle_cleanup_error(error));
    }

    /// Regression (meerkat 0.7.1): retire now performs a final fenced
    /// continuity save. Identity-first reset/delete advance or remove the
    /// mobkit-owned continuity record before retiring the old generation, so
    /// that save fails fail-closed with "record not found" / "stale fencing
    /// token" — both must be recoverable cleanup, not reset/delete failures.
    #[test]
    fn recoverable_lifecycle_cleanup_accepts_stale_continuity_save_on_retire() {
        let record_gone = "internal error: disposal completed but ArchiveSession failed: \
            session error: agent error: Internal error: continuity save: \
            continuity record not found for identity:luka";
        let stale_fence = "internal error: disposal completed but ArchiveSession failed: \
            session error: agent error: Internal error: continuity save: \
            stale fencing token for identity:luka: presented 1, current 6";

        assert!(is_recoverable_lifecycle_cleanup_error(record_gone));
        assert!(is_recoverable_lifecycle_cleanup_error(stale_fence));
    }

    /// Regression (meerkat 0.7.1): an idle member's session machine sits in
    /// `Stopped`; the archive protocol commits the durable document first,
    /// then fails its runtime `Retire` realization on the `Stopped` guard.
    /// The session-service wrapper tolerates exactly this signature so
    /// member retire/respawn disposal completes (meerkat-mob's own archive
    /// helper treats `Stopped` as already-retired).
    #[test]
    fn stopped_session_archive_retire_rejection_matches_only_stopped_guard() {
        assert!(is_stopped_session_archive_retire_rejection(
            "session error: agent error: Internal error: machine archive retire failed \
             after registration: Internal error: DSL authority (Retire): guard rejected \
             transition from Stopped for input::Retire"
        ));
        // The pre-registration variant carries the same meaning.
        assert!(is_stopped_session_archive_retire_rejection(
            "agent error: Internal error: machine archive retire failed: Internal error: \
             DSL authority (Retire): guard rejected transition from Stopped for input::Retire"
        ));
        // Other guard rejections and other retire failures stay fail-closed.
        assert!(!is_stopped_session_archive_retire_rejection(
            "machine archive retire failed after registration: Internal error: \
             DSL authority (Retire): guard rejected transition from Running for input::Retire"
        ));
        assert!(!is_stopped_session_archive_retire_rejection(
            "guard rejected transition from Stopped for input::Retire"
        ));
        assert!(!is_stopped_session_archive_retire_rejection(
            "machine archive retire failed: store unavailable"
        ));
    }

    /// The new arms must stay scoped to completed disposals: the same inner
    /// failures without the "disposal completed" prefix (e.g. a continuity
    /// save failing mid-delivery) are real errors.
    #[test]
    fn recoverable_lifecycle_cleanup_requires_completed_disposal() {
        assert!(!is_recoverable_lifecycle_cleanup_error(
            "continuity save: continuity record not found for identity:luka"
        ));
        assert!(!is_recoverable_lifecycle_cleanup_error(
            "DSL authority (Retire): guard rejected transition from Stopped for input::Retire"
        ));
        assert!(!is_recoverable_lifecycle_cleanup_error(
            "disposal aborted at ArchiveSession: continuity save: stale fencing token"
        ));
    }

    /// Regression: `reset()` / `delete_identity()` for a SESSION-OWNED roster
    /// identity failed because meerkat-mob escalates the archive miss to a fatal
    /// "disposal completed but ArchiveSession failed: ... NotFound for
    /// registered runtime session". The exact production strings (from the
    /// field report) must classify as recoverable for the identity-first bridge
    /// retire path.
    #[test]
    fn session_owned_retire_cleanup_accepts_archive_not_found_for_registered_runtime_session() {
        // Exact shape the classifier receives at the bridge layer:
        // `handle.retire(..).err().to_string()` (the `MobError` Display chain),
        // WITHOUT any later `session bridge mob error:` wrapping.
        let reset_error = "internal error: disposal completed but ArchiveSession failed: \
            session error: agent error: Internal error: mob archive authority returned \
            NotFound for registered runtime session 019ee136-33bc-7bc3-80f9-2aac38736291";
        let delete_error = "internal error: disposal completed but ArchiveSession failed: \
            session error: agent error: Internal error: mob archive authority returned \
            NotFound for registered runtime session 019ee136-340d-7203-a089-c3357835c824";

        assert!(is_recoverable_session_owned_retire_cleanup_error(
            reset_error
        ));
        assert!(is_recoverable_session_owned_retire_cleanup_error(
            delete_error
        ));
    }

    /// The mob-MEMBER orphan gate must stay fail-closed on the same string — a
    /// spawned member whose archive authority lost its record is a real orphan.
    /// This is the deliberate separation: only the identity-first session-owned
    /// retire path tolerates it.
    #[test]
    fn mob_member_lifecycle_gate_still_rejects_archive_not_found() {
        let error = "internal error: disposal completed but ArchiveSession failed: \
            session error: agent error: Internal error: mob archive authority returned \
            NotFound for registered runtime session 019ee136-33bc-7bc3-80f9-2aac38736291";

        assert!(!is_recoverable_lifecycle_cleanup_error(error));
        // Belt-and-suspenders: the helper isolates the new arm.
        assert!(is_session_owned_archive_absent_cleanup_error(error));
    }

    /// The session-owned gate is a strict superset of the shared one.
    #[test]
    fn session_owned_retire_cleanup_still_accepts_shared_recoverable_cases() {
        let cancel_race = "internal error: disposal completed but ArchiveSession failed: \
            session error: agent error: Internal error: runtime cancel-before-retire failed \
            for 019e3c52-0f1b-73d3-a5c7-4b21c2bbf131: Runtime not ready: running";

        assert!(is_recoverable_session_owned_retire_cleanup_error(
            cancel_race
        ));
    }

    /// Stay scoped to COMPLETED disposals: an aborted disposal (session never
    /// tore down) or a bare NotFound without the disposal prefix is a real error.
    #[test]
    fn session_owned_retire_cleanup_requires_completed_disposal() {
        assert!(!is_recoverable_session_owned_retire_cleanup_error(
            "disposal aborted at ArchiveSession: mob archive authority returned NotFound \
             for registered runtime session 019ee136-33bc-7bc3-80f9-2aac38736291"
        ));
        assert!(!is_recoverable_session_owned_retire_cleanup_error(
            "mob archive authority returned NotFound for registered runtime session \
             019ee136-33bc-7bc3-80f9-2aac38736291"
        ));
        assert!(!is_recoverable_session_owned_retire_cleanup_error(
            "model provider returned rate limit"
        ));
    }

    mod console_spawn_projection {
        use super::*;
        use crate::console_spawn::new_console_spawn_sink_slot;
        use crate::unified_runtime::ConsoleEventStore;
        use meerkat_core::AgentToolDispatcher;

        /// Inner dispatcher standing in for the meerkat-mob-mcp tool surface:
        /// returns a canned payload for any call.
        struct CannedDispatcher {
            payload: Value,
            is_error: bool,
        }

        #[async_trait::async_trait]
        impl meerkat_core::AgentToolDispatcher for CannedDispatcher {
            fn tools(&self) -> Arc<[Arc<meerkat_core::types::ToolDef>]> {
                Vec::<Arc<meerkat_core::types::ToolDef>>::new().into()
            }

            async fn dispatch(
                &self,
                call: meerkat_core::types::ToolCallView<'_>,
            ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
                Ok(meerkat_core::ToolDispatchOutcome::sync_result(
                    meerkat_core::types::ToolResult::new(
                        call.id.to_string(),
                        self.payload.to_string(),
                        self.is_error,
                    ),
                ))
            }

            fn capabilities(&self) -> meerkat_core::agent::DispatcherCapabilities {
                meerkat_core::agent::DispatcherCapabilities::default()
            }
        }

        fn console_wrapper(
            payload: Value,
            is_error: bool,
            store: Option<&ConsoleEventStore>,
        ) -> AutoWireParentMobToolDispatcher {
            let console_spawn_sink = new_console_spawn_sink_slot();
            if let Some(store) = store {
                *console_spawn_sink
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(ConsoleSpawnSink::new(store.clone()));
            }
            AutoWireParentMobToolDispatcher {
                inner: Arc::new(CannedDispatcher { payload, is_error }),
                implicit_delegate_retirement_overrides:
                    ImplicitDelegateRetirementOverrides::default(),
                console_spawn_sink,
                identity_runtime: Arc::new(std::sync::RwLock::new(None)),
                protected_mob_id: "test-mob".to_string(),
                spawner_comms_name: Some("ob3/orchestrator/ops-lead".to_string()),
            }
        }

        async fn dispatch_tool(
            dispatcher: &AutoWireParentMobToolDispatcher,
            name: &str,
            args: Value,
        ) -> meerkat_core::ToolDispatchOutcome {
            let raw = serde_json::value::RawValue::from_string(args.to_string()).expect("raw args");
            dispatcher
                .dispatch(meerkat_core::types::ToolCallView {
                    id: "call-1",
                    name,
                    args: &raw,
                })
                .await
                .expect("dispatch succeeds")
        }

        async fn kickoff_events(
            store: &ConsoleEventStore,
        ) -> Vec<crate::console_contracts::ConsoleIdentityEventEnvelope> {
            store
                .replay_all(None)
                .await
                .expect("replay")
                .into_iter()
                .filter(|event| event.event_type == "user_input")
                .collect()
        }

        #[tokio::test]
        async fn mob_spawn_member_projects_kickoff_into_console() {
            let store = ConsoleEventStore::new();
            let dispatcher = console_wrapper(
                serde_json::json!({
                    "agent_identity": "worker-3",
                    "member_ref": "opaque-ref"
                }),
                false,
                Some(&store),
            );

            let outcome = dispatch_tool(
                &dispatcher,
                "mob_spawn_member",
                serde_json::json!({
                    "mob_id": "ob3",
                    "profile": "person-worker",
                    "member_id": "worker-3",
                    "initial_message": "Find the person",
                    "labels": { "group": "workers" }
                }),
            )
            .await;
            assert!(!outcome.result.is_error);

            let kickoffs = kickoff_events(&store).await;
            assert_eq!(kickoffs.len(), 1, "spawn must project one kickoff");
            let kickoff = &kickoffs[0];
            assert_eq!(kickoff.identity, "worker-3");
            assert!(kickoff.event_id.starts_with("spawn-kickoff:ob3:worker-3:"));
            assert_eq!(kickoff.data["content"][0]["text"], "Find the person");
            assert_eq!(kickoff.data["via_tool"], "mob_spawn_member");
            assert_eq!(kickoff.data["parent_identity"], "ops-lead");

            let labels = store
                .identity_labels("worker-3")
                .await
                .expect("spawn registers console identity labels");
            assert_eq!(labels.get("group").map(String::as_str), Some("workers"));
            assert_eq!(
                labels.get("spawned_by").map(String::as_str),
                Some("ops-lead")
            );
        }

        #[tokio::test]
        async fn repeated_spawn_dispatch_keeps_one_kickoff() {
            let store = ConsoleEventStore::new();
            let dispatcher = console_wrapper(
                serde_json::json!({ "agent_identity": "worker-3" }),
                false,
                Some(&store),
            );
            let args = serde_json::json!({
                "mob_id": "ob3",
                "profile": "person-worker",
                "member_id": "worker-3",
                "initial_message": "Find the person"
            });

            dispatch_tool(&dispatcher, "mob_spawn_member", args.clone()).await;
            dispatch_tool(&dispatcher, "mob_spawn_member", args).await;

            assert_eq!(
                kickoff_events(&store).await.len(),
                1,
                "retry/double-spawn must not duplicate the kickoff frame"
            );
        }

        #[tokio::test]
        async fn delegate_projects_task_kickoff_for_generated_helper() {
            let store = ConsoleEventStore::new();
            let dispatcher = console_wrapper(
                serde_json::json!({
                    "agent_identity": "helper-3f2a",
                    "member_ref": "opaque",
                    "mob_id": "implicit-1",
                    "wired": true
                }),
                false,
                Some(&store),
            );

            dispatch_tool(
                &dispatcher,
                "delegate",
                serde_json::json!({ "task": "Review the diff" }),
            )
            .await;

            let kickoffs = kickoff_events(&store).await;
            assert_eq!(kickoffs.len(), 1);
            assert_eq!(kickoffs[0].identity, "helper-3f2a");
            assert_eq!(kickoffs[0].data["content"][0]["text"], "Review the diff");
            assert_eq!(kickoffs[0].data["via_tool"], "delegate");
        }

        #[tokio::test]
        async fn spawn_without_console_sink_leaves_outcome_unchanged() {
            let dispatcher = console_wrapper(
                serde_json::json!({ "agent_identity": "worker-3" }),
                false,
                None,
            );

            let outcome = dispatch_tool(
                &dispatcher,
                "mob_spawn_member",
                serde_json::json!({
                    "mob_id": "ob3",
                    "profile": "person-worker",
                    "member_id": "worker-3",
                    "initial_message": "Find the person"
                }),
            )
            .await;

            assert!(!outcome.result.is_error);
            assert!(
                outcome
                    .result
                    .text_content()
                    .contains("\"agent_identity\":\"worker-3\""),
                "no console store → spawn outcome passes through untouched"
            );
        }

        #[tokio::test]
        async fn failed_spawn_projects_nothing() {
            let store = ConsoleEventStore::new();
            let dispatcher = console_wrapper(
                serde_json::json!({ "error": "spawn rejected" }),
                true,
                Some(&store),
            );

            dispatch_tool(
                &dispatcher,
                "mob_spawn_member",
                serde_json::json!({
                    "mob_id": "ob3",
                    "profile": "person-worker",
                    "member_id": "worker-3",
                    "initial_message": "Find the person"
                }),
            )
            .await;

            assert!(
                kickoff_events(&store).await.is_empty(),
                "failed spawns must not seed console chats"
            );
            assert!(store.identity_labels("worker-3").await.is_none());
        }
    }
}
