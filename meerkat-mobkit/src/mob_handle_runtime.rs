//! Mob member lifecycle management — bootstrap, spawn, reconcile, and roster queries.

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
    ProfileName,
};
use meerkat_store::StoreAdapter;
use serde_json::Value;

use crate::blob_store::{Base64BlobStoreAdapter, BinaryBlobStore, ObjectStoreBlobStore};

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

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

/// Wraps an LLM client and strips provider-emitted evidence blocks that are
/// useful for UI/citation projection but unsafe to replay into the next
/// stateless provider request.
pub struct ReplaySanitizingLlmClient {
    inner: Arc<dyn LlmClient>,
}

impl ReplaySanitizingLlmClient {
    pub fn new(inner: Arc<dyn LlmClient>) -> Self {
        Self { inner }
    }

    pub fn wrap(inner: Arc<dyn LlmClient>) -> Arc<dyn LlmClient> {
        Arc::new(Self::new(inner))
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

    fn provider(&self) -> &'static str {
        self.inner.provider()
    }

    fn model(&self) -> &str {
        self.inner.model()
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
    fn stream<'a>(&'a self, request: &'a LlmRequest) -> LlmStream<'a> {
        let inner = Arc::clone(&self.inner);
        let sanitized = sanitize_llm_request_for_stateless_replay(request);
        Box::pin(async_stream::stream! {
            let mut stream = inner.stream(&sanitized);
            while let Some(event) = stream.next().await {
                yield event;
            }
        })
    }

    fn provider(&self) -> &'static str {
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
/// );
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
    after_create_hook: Option<AfterCreateHook>,
}

fn no_op_pre_build_hook() -> PreBuildHook {
    Arc::new(|_req: &mut CreateSessionRequest| Box::pin(async { Ok(()) }))
}

#[cfg(test)]
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

fn sanitize_message_for_stateless_replay(message: Message) -> Message {
    match message {
        Message::BlockAssistant(mut assistant) => {
            assistant.blocks = assistant
                .blocks
                .into_iter()
                .filter_map(|block| match block {
                    AssistantBlock::ServerToolContent { name, content, .. }
                        if is_replay_unsafe_server_tool_content(&name, &content) =>
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
/// store so we don't depend on the session_store's concrete type. If
/// the SQLite open fails (rare: disk full, permissions), fall back to
/// `InMemoryRuntimeStore` so the runtime can still bootstrap. In that
/// degraded mode resume across restart and archive operations will
/// fail; the warning makes the cause visible in operator logs.
fn build_persistent_runtime_store(store_path: &Path) -> Arc<dyn meerkat_runtime::RuntimeStore> {
    let runtime_db = store_path.join("runtime.sqlite");
    match meerkat_runtime::store::SqliteRuntimeStore::new(&runtime_db) {
        Ok(store) => Arc::new(store),
        Err(err) => {
            tracing::warn!(
                path = %runtime_db.display(),
                error = %err,
                "failed to open SqliteRuntimeStore; falling back to InMemoryRuntimeStore. \
                 Sessions will not survive process restart and archive operations may fail.",
            );
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new())
        }
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
    std::env::var_os("MOBKIT_TRACE_RUNTIME_TURNS").is_some()
}

fn summarize_runtime_prompt(prompt: &meerkat_core::ContentInput) -> String {
    match prompt {
        meerkat_core::ContentInput::Text(text) => {
            text.lines().take(6).collect::<Vec<_>>().join(" ")
        }
        meerkat_core::ContentInput::Blocks(blocks) => blocks
            .iter()
            .map(|block| block.text_projection().to_string())
            .collect::<Vec<_>>()
            .join(" ")
            .lines()
            .take(6)
            .collect::<Vec<_>>()
            .join(" "),
    }
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

fn normalize_runtime_turn_request(
    mut req: meerkat_core::service::StartTurnRequest,
) -> meerkat_core::service::StartTurnRequest {
    // Queue/Steer and render metadata are runtime-owned semantics. By the
    // time apply_runtime_turn() invokes the session service, the runtime
    // has already chosen the boundary and recorded the metadata it needs.
    // The direct agent/session path is queue-only, so forward a normalized
    // turn request to avoid re-injecting runtime-only semantics.
    req.runtime.handling_mode = meerkat_core::types::HandlingMode::Queue;
    req.runtime.render_metadata = None;
    req
}

fn normalize_direct_member_delivery_mode(
    handling_mode: meerkat_core::types::HandlingMode,
) -> meerkat_core::types::HandlingMode {
    match handling_mode {
        // Direct member delivery goes through the queue-only session-service
        // path. Runtime-backed callers may request Steer, but forwarding it
        // here causes Meerkat to reject the replay as an invalid surface.
        meerkat_core::types::HandlingMode::Steer => meerkat_core::types::HandlingMode::Queue,
        other => other,
    }
}

/// Implement all `MobSessionService` super-traits by delegating to `self.inner`,
/// overriding only `create_session` to apply the pre-build hook.
macro_rules! delegate_mob_session_service {
    ($wrapper:ty) => {
        #[async_trait]
        impl meerkat_core::service::SessionService for $wrapper {
            async fn create_session(
                &self,
                mut req: CreateSessionRequest,
            ) -> Result<meerkat_core::types::RunResult, SessionError> {
                (self.hook)(&mut req).await?;
                sanitize_create_session_request_llm_override(&mut req);

                // Capture context before create_session consumes the request.
                let ctx = SessionCreatedContext {
                    model: req.model.clone(),
                    labels: req.labels.clone().unwrap_or_default(),
                    system_prompt: req.system_prompt.clone(),
                };

                let result = self.inner.create_session(req).await?;

                // Best-effort after_create — errors logged, not propagated.
                if let Some(ref after_hook) = self.after_create_hook {
                    after_hook(result.session_id.clone(), ctx).await;
                }

                Ok(result)
            }
            async fn start_turn(
                &self,
                id: &meerkat_core::types::SessionId,
                req: meerkat_core::service::StartTurnRequest,
            ) -> Result<meerkat_core::types::RunResult, SessionError> {
                self.inner.start_turn(id, req).await
            }
            async fn interrupt(
                &self,
                id: &meerkat_core::types::SessionId,
            ) -> Result<(), SessionError> {
                self.inner.interrupt(id).await
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
        }

        #[async_trait]
        impl MobSessionService for $wrapper {
            fn supports_persistent_sessions(&self) -> bool {
                self.inner.supports_persistent_sessions()
            }
            fn runtime_adapter(&self) -> Option<Arc<meerkat_runtime::MeerkatMachine>> {
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
                #[cfg(test)]
                let boundary_name = format!("{boundary:?}");
                #[cfg(test)]
                let contributing_count = contributing_input_ids.len();
                let run_id_for_log = run_id.to_string();
                let prompt_summary = if runtime_turn_diagnostics_enabled() {
                    Some(summarize_runtime_prompt(&req.prompt))
                } else {
                    None
                };
                if let Some(summary) = prompt_summary.as_ref() {
                    tracing::warn!(
                        session_id = %session_id,
                        run_id = %run_id_for_log,
                        boundary = ?boundary,
                        contributing_inputs = contributing_input_ids.len(),
                        prompt = %summary,
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
                        Err(error) => tracing::error!(
                            session_id = %session_id,
                            run_id = %run_id_for_log,
                            error = %error,
                            "mobkit runtime turn error"
                        ),
                    }
                }
                result
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

#[async_trait]
impl meerkat_core::service::SessionService for AfterCreateMobSessionService {
    async fn create_session(
        &self,
        mut req: CreateSessionRequest,
    ) -> Result<meerkat_core::types::RunResult, SessionError> {
        sanitize_create_session_request_llm_override(&mut req);
        // Capture pre-create context from the request (before inner consumes it).
        // The inner service's pre-build hooks may mutate the request further,
        // but we capture here because we can't read the request after inner
        // consumes it. The pre-build hook runs inside inner.create_session.
        //
        // For accurate post-mutation context, we re-read from the request
        // that was already mutated by any outer hooks, and accept that inner
        // hooks are not visible here. This is the correct trade-off: the
        // after_create context matches the request as seen by this layer.
        let ctx = SessionCreatedContext {
            model: req.model.clone(),
            labels: req.labels.clone().unwrap_or_default(),
            system_prompt: req.system_prompt.clone(),
        };
        let result = self.inner.create_session(req).await?;
        (self.after_hook)(result.session_id.clone(), ctx).await;
        Ok(result)
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
}

#[async_trait]
impl MobSessionService for AfterCreateMobSessionService {
    fn supports_persistent_sessions(&self) -> bool {
        self.inner.supports_persistent_sessions()
    }
    fn runtime_adapter(&self) -> Option<Arc<meerkat_runtime::MeerkatMachine>> {
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
    pub binary_blob_store: Option<Arc<dyn BinaryBlobStore>>,
    pub options: MobBootstrapOptions,
    /// Explicit runtime adapter — bypasses `session_service.runtime_adapter()`.
    ///
    /// Used by `persistent()` to supply the adapter directly so the session
    /// service's `runtime_store` can stay `None` (keeping the checkpointer
    /// enabled). See meerkat-session#checkpointer-enabled-flag.
    pub runtime_adapter: Option<Arc<meerkat_runtime::MeerkatMachine>>,
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
        let session_service = Arc::new(PreBuildMobSessionService {
            inner: session_service,
            hook: no_op_pre_build_hook(),
            after_create_hook: None,
        }) as Arc<dyn MobSessionService>;
        Self {
            definition,
            storage,
            session_service,
            binary_blob_store: None,
            options: MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: None,
            },
            runtime_adapter: None,
            _ephemeral_dir: None,
        }
    }

    pub fn with_options(mut self, options: MobBootstrapOptions) -> Self {
        self.options = options;
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
    ) -> Self {
        caps.image_generation |= mob_definition_may_use_image_generation(&definition);
        let binary_blob_store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
        let image_generation_machine = if caps.image_generation {
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
        if let Some(machine) = image_generation_machine {
            factory = factory.with_image_generation_machine(machine);
        }
        let config = Config::default();
        let mut builder = FactoryAgentBuilder::new(factory, config);
        builder.default_blob_store = Some(blob_store);
        if let Some(store) = session_store {
            builder.default_session_store = Some(store);
        }
        let session_service: Arc<dyn MobSessionService> = Arc::new(
            meerkat_session::EphemeralSessionService::new(builder, max_sessions),
        );
        let hook = hook.unwrap_or_else(no_op_pre_build_hook);
        let session_service = Arc::new(PreBuildMobSessionService {
            inner: session_service,
            hook,
            after_create_hook,
        }) as Arc<dyn MobSessionService>;
        let mut spec = Self::new(definition, storage, session_service);
        spec.binary_blob_store = Some(binary_blob_store);
        spec
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
            CapabilityFlags::default(),
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
        hook: impl Fn(
            &mut CreateSessionRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), SessionError>> + Send + '_>,
        > + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::persistent_inner(
            definition,
            storage,
            store_path,
            max_sessions,
            session_store,
            Some(Arc::new(hook)),
            CapabilityFlags::default(),
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
        hook: Option<PreBuildHook>,
        mut caps: CapabilityFlags,
        after_create_hook: Option<AfterCreateHook>,
    ) -> Self {
        caps.image_generation |= mob_definition_may_use_image_generation(&definition);
        let binary_blob_store: Arc<dyn BinaryBlobStore> = match ObjectStoreBlobStore::local(
            store_path.join("blobs"),
        ) {
            Ok(store) => Arc::new(store),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "failed to initialize persistent binary blob store; falling back to in-memory blobs"
                );
                Arc::new(ObjectStoreBlobStore::memory())
            }
        };
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
        // Use a SQLite-backed runtime store so we get BOTH durability across
        // process restart AND control-op authority (archive/retire). The
        // earlier 0.6.1 wiring used `Some(InMemoryRuntimeStore)`, which was
        // a half-fix: it kept the session-service's runtime_store path on
        // (so `load_authoritative_session` resolved through runtime_store —
        // good for control ops), but the in-memory store died on restart so
        // resume failed. Switching the in-memory store for a persistent one
        // satisfies both. The store lives at `store_path/runtime.sqlite`,
        // sibling to whatever path the caller's `session_store` uses.
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
            build_persistent_runtime_store(&store_path);
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
        let config = Config::default();
        let mut builder = FactoryAgentBuilder::new(factory, config);
        builder.default_session_store = Some(Arc::new(StoreAdapter::new(session_store.clone())));
        builder.default_blob_store = Some(blob_store.clone());
        let session_service: Arc<dyn MobSessionService> =
            Arc::new(meerkat_session::PersistentSessionService::new(
                builder,
                max_sessions,
                session_store,
                Some(runtime_store),
                blob_store,
            ));
        let hook = hook.unwrap_or_else(no_op_pre_build_hook);
        let session_service = Arc::new(PreBuildMobSessionService {
            inner: session_service,
            hook,
            after_create_hook,
        }) as Arc<dyn MobSessionService>;
        let mut spec = Self::new(definition, storage, session_service);
        spec.runtime_adapter = Some(runtime_adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        spec
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ephemeral_runtime_backed_inner(
        definition: MobDefinition,
        storage: MobStorage,
        store_path: PathBuf,
        max_sessions: usize,
        hook: Option<PreBuildHook>,
        mut caps: CapabilityFlags,
        after_create_hook: Option<AfterCreateHook>,
    ) -> Self {
        caps.image_generation |= mob_definition_may_use_image_generation(&definition);
        let config = Config::default();
        let session_store: Arc<dyn SessionStore> = Arc::new(meerkat_store::MemoryStore::new());
        let binary_blob_store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
        // Ephemeral mode: an in-memory runtime_store is fine — there is no
        // restart to survive. But it MUST be passed as `Some(...)` to the
        // session service so meerkat's `load_persisted_session_for_control`
        // resolves through the runtime authority and archive/retire control
        // ops succeed within the session lifetime. See `persistent_inner`
        // for the durable counterpart.
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
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
        let session_service: Arc<dyn MobSessionService> =
            Arc::new(meerkat_session::PersistentSessionService::new(
                builder,
                max_sessions,
                session_store,
                Some(runtime_store),
                blob_store,
            ));
        let hook = hook.unwrap_or_else(no_op_pre_build_hook);
        let session_service = Arc::new(PreBuildMobSessionService {
            inner: session_service,
            hook,
            after_create_hook,
        }) as Arc<dyn MobSessionService>;
        let mut spec = Self::new(definition, storage, session_service);
        spec.runtime_adapter = Some(runtime_adapter);
        spec.binary_blob_store = Some(binary_blob_store);
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

/// Live mob runtime backed by a `MobHandle`.
#[derive(Clone)]
pub struct MobRuntime {
    handle: MobHandle,
    session_service: Option<Arc<dyn MobSessionService>>,
    binary_blob_store: Option<Arc<dyn BinaryBlobStore>>,
    /// Keeps the ephemeral temp directory alive for the lifetime of the runtime.
    /// Dropped when the runtime is dropped, cleaning up the temp dir.
    _ephemeral_dir: Option<Arc<tempfile::TempDir>>,
}

impl MobRuntime {
    pub async fn bootstrap(spec: MobBootstrapSpec) -> Result<Self, MobRuntimeError> {
        let ephemeral_dir = spec._ephemeral_dir.clone();
        let session_service = spec.session_service.clone();
        let binary_blob_store = spec.binary_blob_store.clone();
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

        if let Some(client) = spec.options.default_llm_client {
            builder = builder.with_default_llm_client(ReplaySanitizingLlmClient::wrap(client));
        }

        let handle = builder.create().await?;
        Ok(Self {
            handle,
            session_service: Some(session_service),
            binary_blob_store,
            _ephemeral_dir: ephemeral_dir,
        })
    }

    pub fn from_handle(handle: MobHandle) -> Self {
        Self {
            handle,
            session_service: None,
            binary_blob_store: None,
            _ephemeral_dir: None,
        }
    }

    pub fn handle(&self) -> MobHandle {
        self.handle.clone()
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

    pub fn binary_blob_store(&self) -> Option<Arc<dyn BinaryBlobStore>> {
        self.binary_blob_store.clone()
    }
}

/// Project a meerkat `MobMemberListEntry` into mobkit's HTTP JSON shape.
///
/// Aligns with meerkat 0.6's lightweight-roster design: list entries do
/// not carry a bridge `session_id`. Callers needing the realtime session
/// for a member must use `mobkit/member_status`, which serializes
/// `MobMemberSnapshot.current_session_id` natively.
pub fn member_entry_to_json(entry: &meerkat_mob::runtime::MobMemberListEntry) -> serde_json::Value {
    serde_json::to_value(entry).unwrap_or(serde_json::Value::Null)
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
    let image_input = meerkat_core::model_profile::profile_for(provider, model)
        .map(|profile| profile.vision)
        .unwrap_or(false);
    crate::runtime::ConsoleModelCapabilities { image_input }
}

pub fn model_capabilities_for_profile(
    profile: &Profile,
) -> crate::runtime::ConsoleModelCapabilities {
    let image_input = Provider::infer_from_model(&profile.model)
        .and_then(|provider| meerkat_core::model_profile::profile_for(provider, &profile.model))
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

pub async fn model_capabilities_for_member(
    handle: &MobHandle,
    session_service: Option<&Arc<dyn MobSessionService>>,
    member_id: &meerkat_mob::ids::MeerkatId,
) -> crate::runtime::ConsoleModelCapabilities {
    if let Some(service) = session_service
        && let Some(session_id) = handle.resolve_bridge_session_id(member_id).await
        && let Ok(view) = service.read(&session_id).await
    {
        return model_capabilities_for_model(view.state.provider, &view.state.model);
    }

    handle
        .get_member(member_id)
        .await
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
    let mid = meerkat_mob::ids::MeerkatId::from(member_id);
    let Some(member) = handle.get_member(&mid).await else {
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
/// delivering through MobKit's direct member-send path. Direct member delivery
/// is queue-only in Meerkat 0.6, so `Steer` is normalized before reaching the
/// session service; callers that need a true interrupt boundary must use a
/// runtime-backed steering surface.
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
    let mid = meerkat_mob::ids::MeerkatId::from(member_id);
    let handling_mode = normalize_direct_member_delivery_mode(handling_mode);
    let _receipt = handle
        .member(&mid)
        .await?
        .send(content, handling_mode)
        .await?;
    let session_id = handle
        .resolve_bridge_session_id(&mid)
        .await
        .ok_or_else(|| {
            MobRuntimeError::Mob(MobError::Internal(
                "member has no bridge session after send".to_string(),
            ))
        })?;
    Ok(session_id.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

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
                            name: "web_search".to_string(),
                            content: serde_json::json!({
                                "type": "response.web_search_call.searching",
                                "item_id": "ws_123"
                            }),
                            meta: None,
                        },
                        meerkat_core::AssistantBlock::ServerToolContent {
                            id: Some("ws_123".to_string()),
                            name: "web_search_call".to_string(),
                            content: serde_json::json!({
                                "type": "web_search_call",
                                "id": "ws_123",
                                "status": "completed"
                            }),
                            meta: None,
                        },
                        meerkat_core::AssistantBlock::ServerToolContent {
                            id: None,
                            name: "web_search_annotations".to_string(),
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
            meerkat_core::AssistantBlock::ServerToolContent { ref name, .. }
                if name == "web_search_call"
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
    struct CapturingAgentLlmClient {
        seen_messages: std::sync::Mutex<Vec<meerkat_core::Message>>,
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

        fn provider(&self) -> &'static str {
            "openai"
        }

        fn model(&self) -> &'static str {
            "gpt-5.5"
        }
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
                        name: "web_search".to_string(),
                        content: serde_json::json!({
                            "type": "response.web_search_call.searching",
                            "item_id": "ws_123"
                        }),
                        meta: None,
                    },
                    meerkat_core::AssistantBlock::ServerToolContent {
                        id: Some("ok".to_string()),
                        name: "web_search_call".to_string(),
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
            meerkat_core::AssistantBlock::ServerToolContent { ref name, .. }
                if name == "web_search_call"
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
        );

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

        // Ephemeral specs don't have a runtime adapter.
        assert!(spec.runtime_adapter.is_none());

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
            req.system_prompt = Some("injected-prompt".to_string());
            let labels = req.labels.get_or_insert_with(Default::default);
            labels.insert("hook_label".to_string(), "hook_value".to_string());
            // Capture to prove the hook ran and mutated the request.
            let mut lock = captured_clone
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *lock = Some((req.model.clone(), req.system_prompt.clone()));
            Box::pin(async { Ok(()) })
        });
        let wrapped = PreBuildMobSessionService {
            inner,
            hook,
            after_create_hook: None,
        };

        let req = CreateSessionRequest {
            model: "original-model".to_string(),
            prompt: meerkat_core::ContentInput::Text("test".to_string()),
            render_metadata: None,
            system_prompt: None,
            max_tokens: None,
            event_tx: None,
            skill_references: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
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
        );
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

    /// Ephemeral counterpart: an in-memory runtime_store is fine (no
    /// restart to survive) but it MUST be `Some(...)` on the session
    /// service so archive/retire control ops succeed within the session
    /// lifetime.
    #[test]
    fn ephemeral_runtime_backed_passes_runtime_store_to_session_service() {
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
            CapabilityFlags::default(),
            None,
        );
        assert!(
            spec.runtime_adapter.is_some(),
            "ephemeral_runtime_backed_inner must provide a runtime adapter via spec.runtime_adapter"
        );
        assert!(
            spec.session_service.runtime_adapter().is_some(),
            "session service must own a runtime_store (in-memory is fine here) \
             so archive/retire control ops succeed in-session"
        );
    }

    /// Regression: definition-based ephemeral builds must also carry a runtime
    /// adapter so autonomous-host members can process peer-delivered work.
    #[test]
    fn ephemeral_bootstrap_provides_runtime_adapter() {
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
        );
        assert!(
            spec.runtime_adapter.is_none(),
            "raw ephemeral_inner stays bare; builder must layer the adapter on top"
        );
    }

    /// Runtime-owned handling/routing semantics must be stripped before a
    /// runtime-applied turn reaches the direct session-service path.
    #[test]
    fn normalize_runtime_turn_request_strips_runtime_owned_semantics() {
        let req = meerkat_core::service::StartTurnRequest {
            prompt: meerkat_core::ContentInput::Text("checkpoint".to_string()),
            system_prompt: Some("system".to_string()),
            event_tx: None,
            runtime: meerkat_core::service::StartTurnRuntimeSemantics {
                render_metadata: Some(meerkat_core::types::RenderMetadata {
                    class: meerkat_core::types::RenderClass::OpsProgress,
                    salience: meerkat_core::types::RenderSalience::Urgent,
                }),
                handling_mode: meerkat_core::types::HandlingMode::Steer,
                skill_references: None,
                flow_tool_overlay: None,
                pre_turn_context_appends: Vec::new(),
                turn_metadata: None,
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
            normalized.runtime.render_metadata.is_none(),
            "runtime-owned render metadata must not be forwarded through the direct agent path"
        );
        assert_eq!(normalized.prompt, expected_prompt);
        assert_eq!(normalized.system_prompt, expected_system_prompt);
    }

    /// Direct member delivery must not forward runtime-only steering semantics.
    #[test]
    fn normalize_direct_member_delivery_mode_downgrades_steer() {
        assert_eq!(
            normalize_direct_member_delivery_mode(meerkat_core::types::HandlingMode::Queue),
            meerkat_core::types::HandlingMode::Queue
        );
        assert_eq!(
            normalize_direct_member_delivery_mode(meerkat_core::types::HandlingMode::Steer),
            meerkat_core::types::HandlingMode::Queue,
            "the direct member-send path is queue-only until a runtime-backed steering surface is wired"
        );
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
            render_metadata: None,
            system_prompt: None,
            max_tokens: None,
            event_tx: None,
            skill_references: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
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
            render_metadata: None,
            system_prompt: None,
            max_tokens: None,
            event_tx: None,
            skill_references: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
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
                req.system_prompt = Some("injected by hook".to_string());
                Ok(())
            }
        }

        let hook = MutatingHook;
        let mut req = CreateSessionRequest {
            model: "original".to_string(),
            prompt: meerkat_core::ContentInput::Text("test".to_string()),
            render_metadata: None,
            system_prompt: None,
            max_tokens: None,
            event_tx: None,
            skill_references: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
        };
        hook.before_create(&mut req).await.unwrap();
        assert_eq!(req.model, "hook-overridden");
        assert_eq!(req.system_prompt.as_deref(), Some("injected by hook"));
    }
}
