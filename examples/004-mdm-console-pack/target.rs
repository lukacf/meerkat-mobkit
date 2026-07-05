use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use meerkat::{AgentFactory, FactoryAgentBuilder, PersistenceBundle, PersistentSessionService};
use meerkat_core::lifecycle::RunId;
use meerkat_core::lifecycle::core_executor::{
    CoreApplyOutput, CoreExecutor, CoreExecutorBoundaryHandle, CoreExecutorError,
    CoreExecutorInterruptHandle,
};
use meerkat_core::lifecycle::run_primitive::{
    ConversationContextAppend, RunApplyBoundary, RunPrimitive,
};
use meerkat_core::service::{
    CreateSessionRequest, InitialTurnPolicy, SessionBuildOptions, SessionError, SessionService,
    StartTurnRequest, StartTurnRuntimeSemantics,
};
use meerkat_core::types::{ContentInput, HandlingMode, SessionId};
use meerkat_core::{Config, PendingSystemContextAppend, Session};
use meerkat_mob::MobSessionService;
use meerkat_mob_mcp::{AgentMobToolSurfaceFactory, MobMcpState};
use meerkat_runtime::MeerkatMachine;
use meerkat_store::{JsonlStore, MemoryBlobStore, SessionFilter, SessionStore};
use serde::Serialize;

const SYSTEM_PROMPT: &str = "\
You are a remote managed target named '{name}' in a MobKit MDM mob.
You run on the target host and may use unrestricted shell tools on this host.
When hive or the operator asks for hardware, OS, process, file, network, or
system state, inspect the real local machine before answering.
Your current session_id is '{session_id}'.";

#[derive(Debug, Clone)]
struct Args {
    id: String,
    name: String,
    listen: String,
    advertise: Option<String>,
    data_dir: PathBuf,
    binding_out: Option<PathBuf>,
    model: String,
    provider: String,
    site: String,
    platform: String,
}

#[derive(Serialize)]
struct BindingFile {
    id: String,
    name: String,
    site: String,
    platform: String,
    address: String,
    public_key: String,
    bootstrap_token: String,
    labels: BTreeMap<String, String>,
}

struct TargetRuntimeSurface {
    service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
    runtime_adapter: Arc<MeerkatMachine>,
    jsonl_store: Arc<JsonlStore>,
    mob_state: Arc<MobMcpState>,
    _factory: Arc<AgentFactory>,
    _config: Config,
}

struct TargetCoreExecutor {
    service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
    mob_state: Arc<MobMcpState>,
    session_id: SessionId,
}

impl TargetCoreExecutor {
    fn new(
        service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
        mob_state: Arc<MobMcpState>,
        session_id: SessionId,
    ) -> Self {
        Self {
            service,
            mob_state,
            session_id,
        }
    }
}

struct TargetCoreBoundaryHandle {
    service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
    session_id: SessionId,
}

#[async_trait::async_trait]
impl CoreExecutorBoundaryHandle for TargetCoreBoundaryHandle {
    async fn cancel_after_boundary(&self, _reason: String) -> Result<(), CoreExecutorError> {
        self.service
            .cancel_after_boundary(&self.session_id)
            .await
            .or_else(|error| match error {
                SessionError::NotRunning { .. } => Ok(()),
                error => Err(error),
            })
            .map_err(|error| CoreExecutorError::control_failed_runtime(error.to_string()))
    }
}

struct TargetCoreInterruptHandle {
    service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
    session_id: SessionId,
}

#[async_trait::async_trait]
impl CoreExecutorInterruptHandle for TargetCoreInterruptHandle {
    async fn hard_cancel_current_run(&self, _reason: String) -> Result<(), CoreExecutorError> {
        self.service
            .interrupt(&self.session_id)
            .await
            .or_else(|error| match error {
                SessionError::NotRunning { .. } => Ok(()),
                error => Err(error),
            })
            .map_err(|error| CoreExecutorError::control_failed_runtime(error.to_string()))
    }
}

#[async_trait::async_trait]
impl CoreExecutor for TargetCoreExecutor {
    fn boundary_handle(&self) -> Option<Arc<dyn CoreExecutorBoundaryHandle>> {
        Some(Arc::new(TargetCoreBoundaryHandle {
            service: Arc::clone(&self.service),
            session_id: self.session_id.clone(),
        }))
    }

    fn interrupt_handle(&self) -> Option<Arc<dyn CoreExecutorInterruptHandle>> {
        Some(Arc::new(TargetCoreInterruptHandle {
            service: Arc::clone(&self.service),
            session_id: self.session_id.clone(),
        }))
    }

    async fn apply(
        &mut self,
        run_id: RunId,
        primitive: RunPrimitive,
    ) -> Result<CoreApplyOutput, CoreExecutorError> {
        if let Some(reason) = primitive.peer_response_terminal_apply_intent_violation() {
            return Err(CoreExecutorError::apply_failed_primitive_rejected(
                reason.to_string(),
            ));
        }
        let metadata = primitive.turn_metadata();
        let pre_turn_context_appends = match &primitive {
            RunPrimitive::StagedInput(staged)
                if primitive.is_peer_response_terminal_context_and_run() =>
            {
                pending_system_context_appends(&staged.context_appends)
            }
            _ => Vec::new(),
        };
        let prompt = primitive.extract_content_input();
        let prompt_preview = match &prompt {
            ContentInput::Text(text) => text.replace('\n', " "),
            ContentInput::Blocks(blocks) => format!("{} content blocks", blocks.len()),
        };
        eprintln!(
            "[mdm-target] peer turn accepted: {}",
            prompt_preview.chars().take(160).collect::<String>()
        );
        let req = StartTurnRequest {
            prompt,
            injected_context: Vec::new(),
            system_prompt: None,
            event_tx: None,
            // meerkat 0.7: render_metadata/skill_references live on the
            // RuntimeTurnMetadata carrier, not as flat semantics arguments.
            runtime: StartTurnRuntimeSemantics::new(
                metadata
                    .and_then(|meta| meta.handling_mode)
                    .unwrap_or(HandlingMode::Queue),
                metadata.and_then(|meta| meta.flow_tool_overlay.clone()),
                pre_turn_context_appends,
                metadata.cloned(),
            ),
        };
        let boundary = match &primitive {
            RunPrimitive::StagedInput(staged) => staged.boundary,
            _ => RunApplyBoundary::Immediate,
        };
        let input_ids = primitive.contributing_input_ids().to_vec();
        self.service
            .apply_runtime_turn(&self.session_id, run_id, req, boundary, input_ids)
            .await
            .map_err(CoreExecutorError::apply_failed_from_session_error)
    }

    async fn cancel_after_boundary(&mut self, reason: String) -> Result<(), CoreExecutorError> {
        TargetCoreBoundaryHandle {
            service: Arc::clone(&self.service),
            session_id: self.session_id.clone(),
        }
        .cancel_after_boundary(reason)
        .await
    }

    async fn stop_runtime_executor(&mut self, _reason: String) -> Result<(), CoreExecutorError> {
        let discard_result = self.service.discard_live_session(&self.session_id).await;
        if let Err(error) = Box::pin(
            self.mob_state
                .destroy_bridge_session_mobs(&self.session_id.to_string()),
        )
        .await
        {
            eprintln!(
                "[mdm-target] warning: cleanup mobs for session {}: {error}",
                self.session_id
            );
        }
        match discard_result {
            Ok(()) | Err(SessionError::NotFound { .. }) => Ok(()),
            Err(error) => Err(CoreExecutorError::control_failed_runtime(error.to_string())),
        }
    }
}

fn pending_system_context_appends(
    appends: &[ConversationContextAppend],
) -> Vec<PendingSystemContextAppend> {
    let accepted_at = meerkat_core::time_compat::SystemTime::now();
    appends
        .iter()
        .map(|append| PendingSystemContextAppend {
            // meerkat 0.7: the append carries typed renderable content; the
            // one lowering to prompt text happens at the render seam.
            content: append.content.clone(),
            source: Some(append.key.clone()),
            idempotency_key: Some(append.key.clone()),
            source_kind: meerkat_core::session::SystemContextSource::default(),
            peer_response_terminal: None,
            accepted_at,
        })
        .collect()
}

fn parse_args() -> anyhow::Result<Args> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| {
        raw.iter()
            .position(|arg| arg == flag)
            .and_then(|index| raw.get(index + 1).cloned())
    };
    if raw.iter().any(|arg| arg == "--help" || arg == "-h") {
        eprintln!(
            "Usage: mdm_mob_target --id ID --listen HOST:PORT [--advertise tcp://HOST:PORT] [--name NAME] [--binding-out PATH]"
        );
        std::process::exit(0);
    }
    let id = value("--id").context("--id ID is required")?;
    let name = value("--name").unwrap_or_else(|| id.clone());
    let listen = value("--listen").unwrap_or_else(|| "127.0.0.1:5791".to_string());
    let data_dir = value("--data-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".state/targets").join(&id));
    Ok(Args {
        id,
        name,
        listen,
        advertise: value("--advertise"),
        data_dir,
        binding_out: value("--binding-out").map(PathBuf::from),
        model: value("--model").unwrap_or_else(|| "gpt-5.5".to_string()),
        provider: value("--provider").unwrap_or_else(|| "openai".to_string()),
        site: value("--site").unwrap_or_else(|| "local".to_string()),
        platform: value("--platform").unwrap_or_else(|| std::env::consts::OS.to_string()),
    })
}

fn advertised_address(args: &Args) -> anyhow::Result<String> {
    args.listen
        .parse::<SocketAddr>()
        .with_context(|| format!("invalid --listen address '{}'", args.listen))?;
    if let Some(address) = args.advertise.as_ref() {
        meerkat_core::comms::PeerAddress::parse(address)
            .map_err(|error| anyhow::anyhow!("invalid --advertise address '{address}': {error}"))?;
        return Ok(address.clone());
    }
    Ok(format!("tcp://{}", args.listen))
}

async fn create_comms_runtime(args: &Args) -> anyhow::Result<Arc<meerkat_comms::CommsRuntime>> {
    let config = meerkat_comms::ResolvedCommsConfig {
        enabled: true,
        name: args.id.clone(),
        inproc_namespace: None,
        listen_tcp: Some(args.listen.parse::<SocketAddr>()?),
        listen_uds: None,
        advertise_address: Some(advertised_address(args)?),
        event_listen_tcp: None,
        #[cfg(unix)]
        event_listen_uds: None,
        identity_dir: args.data_dir.join("identity"),
        trusted_peers_path: args.data_dir.join("trusted_peers.json"),
        comms_config: Default::default(),
        auth: Default::default(),
        require_peer_auth: true,
        allow_external_unauthenticated: false,
        pairing_password: None,
    };
    let mut runtime =
        meerkat_comms::CommsRuntime::new_with_silent_intents(config, Arc::new(HashSet::new()))
            .await
            .map_err(|error| anyhow::anyhow!("comms runtime: {error}"))?;
    runtime.set_blob_store(Arc::new(MemoryBlobStore::new()));
    runtime
        .start_listeners()
        .await
        .map_err(|error| anyhow::anyhow!("start comms listener: {error}"))?;
    Ok(Arc::new(runtime))
}

async fn build_target_runtime_surface(
    session_dir: &Path,
    comms_runtime: Arc<meerkat_comms::CommsRuntime>,
) -> anyhow::Result<TargetRuntimeSurface> {
    let factory = AgentFactory::new(session_dir)
        .shell(true)
        .builtins(true)
        .comms(true)
        .mob(true)
        .with_comms_runtime(comms_runtime);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let config = Config::load_from(session_dir, home.as_deref())
        .await
        .unwrap_or_default();
    let shared_factory = Arc::new(factory.clone());
    let shared_config = config.clone();
    let builder = FactoryAgentBuilder::new(factory, config);
    let mob_tools_slot = Arc::clone(&builder.default_mob_tools);

    let jsonl_store = Arc::new(JsonlStore::new(session_dir.to_path_buf()));
    jsonl_store.init().await?;
    let persistence = PersistenceBundle::new(
        Arc::clone(&jsonl_store) as Arc<dyn SessionStore>,
        Arc::new(meerkat_runtime::InMemoryRuntimeStore::new()),
        Arc::new(MemoryBlobStore::new()),
    );
    let runtime_adapter = persistence.runtime_adapter();
    let (session_store, runtime_store, blob_store) = persistence.into_parts();
    let service = Arc::new(PersistentSessionService::new(
        builder,
        10,
        session_store,
        runtime_store,
        blob_store,
    ));
    let mob_state = Arc::new(MobMcpState::new_with_runtime_adapter(
        service.clone(),
        Some(runtime_adapter.clone()),
    ));
    *mob_tools_slot
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(
        AgentMobToolSurfaceFactory::new(Arc::clone(&mob_state)),
    ));
    Ok(TargetRuntimeSurface {
        service,
        runtime_adapter,
        jsonl_store,
        mob_state,
        _factory: shared_factory,
        _config: shared_config,
    })
}

async fn create_or_resume_session(
    args: &Args,
    surface: &TargetRuntimeSurface,
    comms_runtime: Arc<meerkat_comms::CommsRuntime>,
) -> anyhow::Result<SessionId> {
    if let Ok(mut sessions) = surface.jsonl_store.list(SessionFilter::default()).await {
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
        if let Some(latest) = sessions.first() {
            match setup_session(
                args,
                surface,
                comms_runtime.clone(),
                Some(latest.id.clone()),
            )
            .await
            {
                Ok(session_id) => return Ok(session_id),
                Err(error) => {
                    eprintln!("[mdm-target] resume failed: {error}; starting fresh session");
                }
            }
        }
    }
    setup_session(args, surface, comms_runtime, None).await
}

async fn setup_session(
    args: &Args,
    surface: &TargetRuntimeSurface,
    comms_runtime: Arc<meerkat_comms::CommsRuntime>,
    resume_id: Option<SessionId>,
) -> anyhow::Result<SessionId> {
    let resume_session = match &resume_id {
        Some(id) => surface
            .service
            .load_persisted_session(id)
            .await
            .map_err(|error| anyhow::anyhow!("load session {id}: {error}"))?,
        None => Some(Session::new()),
    };
    let prepared_session =
        resume_session.ok_or_else(|| anyhow::anyhow!("target session not found"))?;
    let prepared_session_id = prepared_session.id().clone();
    let bindings = surface
        .runtime_adapter
        .prepare_bindings(prepared_session_id.clone())
        .await
        .map_err(|error| anyhow::anyhow!("runtime bindings: {error}"))?;
    let system_prompt = SYSTEM_PROMPT
        .replace("{name}", &args.name)
        .replace("{session_id}", &prepared_session_id.to_string());
    let build_opts = SessionBuildOptions {
        provider: Some(meerkat_core::Provider::from_name(&args.provider)),
        override_builtins: meerkat_core::ToolCategoryOverride::Enable,
        override_shell: meerkat_core::ToolCategoryOverride::Enable,
        override_mob: meerkat_core::ToolCategoryOverride::Enable,
        resume_session: Some(prepared_session),
        runtime_build_mode: meerkat_core::RuntimeBuildMode::SessionOwned(bindings),
        ..Default::default()
    };
    let result = surface
        .service
        .create_session(CreateSessionRequest {
            model: args.model.clone(),
            prompt: ContentInput::Text(String::new()),
            // meerkat 0.7: render_metadata/skill_references moved to
            // SessionBuildOptions.initial_turn_metadata; system_prompt is the
            // typed tri-state SystemPromptOverride.
            system_prompt: meerkat_core::config::SystemPromptOverride::Set(system_prompt),
            max_tokens: None,
            event_tx: None,
            initial_turn: InitialTurnPolicy::Defer,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::Discard,
            build: Some(build_opts),
            labels: None,
            injected_context: Vec::new(),
        })
        .await
        .map_err(|error| anyhow::anyhow!("create session: {error}"))?;
    let session_id = result.session_id;
    let executor = Box::new(TargetCoreExecutor::new(
        surface.service.clone(),
        surface.mob_state.clone(),
        session_id.clone(),
    ));
    // meerkat 0.7: ensure_session_with_executor returns a Result.
    surface
        .runtime_adapter
        .ensure_session_with_executor(session_id.clone(), executor)
        .await
        .map_err(|error| anyhow::anyhow!("ensure session executor: {error}"))?;
    // meerkat 0.7: update_peer_ingress_context returns a Result; surface
    // failures instead of formatting the Result.
    let peer_ingress_spawned = surface
        .runtime_adapter
        .update_peer_ingress_context(
            &session_id,
            true,
            Some(comms_runtime as Arc<dyn meerkat_core::agent::CommsRuntime>),
        )
        .await
        .map_err(|error| anyhow::anyhow!("enable peer ingress: {error}"))?;
    eprintln!("[mdm-target] peer ingress enabled for {session_id}; spawned={peer_ingress_spawned}");
    Ok(session_id)
}

async fn write_binding(
    args: &Args,
    comms_runtime: &meerkat_comms::CommsRuntime,
) -> anyhow::Result<BindingFile> {
    let binding = BindingFile {
        id: args.id.clone(),
        name: args.name.clone(),
        site: args.site.clone(),
        platform: args.platform.clone(),
        address: advertised_address(args)?,
        public_key: comms_runtime.public_key().to_pubkey_string(),
        bootstrap_token: comms_runtime.bridge_bootstrap_token().to_string(),
        labels: BTreeMap::from([
            ("target_runtime".to_string(), "mdm_mob_target".to_string()),
            ("shell".to_string(), "unrestricted".to_string()),
        ]),
    };
    if let Some(path) = args.binding_out.as_ref() {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let bytes = serde_json::to_vec_pretty(&binding)?;
        tokio::fs::write(path, bytes).await?;
    }
    Ok(binding)
}

// Bootstrap-only expects, before any runtime exists to report errors through
// (the sibling example packs carry the same allowance file-wide).
#[allow(clippy::expect_used)]
fn main() {
    // Meerkat 0.7's generated machine-authority apply path allocates very
    // large stack frames in debug builds (see .cargo/config.toml). The
    // workspace-level `[env] RUST_MIN_STACK` only covers `cargo run`/test
    // flows, not direct execution of the prebuilt binary (local-target.sh
    // and real-target-smoke.sh launch the built example directly), so the
    // example sizes its own threads explicitly: the root future runs on a
    // dedicated 32 MiB thread and tokio workers get 32 MiB stacks (mirrors
    // mobkit_gateway/rpc_gateway's explicit worker sizing).
    const STACK_SIZE: usize = 32 * 1024 * 1024;
    std::thread::Builder::new()
        .name("mdm-target-runtime".into())
        .stack_size(STACK_SIZE)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(STACK_SIZE)
                .build()
                .expect("build tokio runtime");
            if let Err(error) = runtime.block_on(run()) {
                eprintln!("mdm target failed: {error:#}");
                std::process::exit(1);
            }
        })
        .expect("spawn runtime thread")
        .join()
        .expect("runtime thread panicked");
}

async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".into()))
        .init();
    let args = parse_args()?;
    tokio::fs::create_dir_all(&args.data_dir).await?;
    let comms_runtime = create_comms_runtime(&args).await?;
    let session_dir = args.data_dir.join("sessions");
    tokio::fs::create_dir_all(&session_dir).await?;
    let surface = build_target_runtime_surface(&session_dir, comms_runtime.clone()).await?;
    let session_id = create_or_resume_session(&args, &surface, comms_runtime.clone()).await?;
    let binding = write_binding(&args, &comms_runtime).await?;

    println!("{}", serde_json::to_string_pretty(&binding)?);
    eprintln!(
        "[mdm-target] {} ready; session={session_id}; comms={}",
        args.id, binding.address
    );
    std::future::pending::<()>().await;
    Ok(())
}
