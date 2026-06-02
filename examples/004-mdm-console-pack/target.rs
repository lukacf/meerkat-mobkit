use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use meerkat::{AgentFactory, FactoryAgentBuilder, PersistenceBundle, PersistentSessionService};
use meerkat_core::comms::{CommsCommand, PeerId, PeerRoute};
use meerkat_core::interaction::{InteractionId, ResponseStatus};
use meerkat_core::lifecycle::RunId;
use meerkat_core::lifecycle::core_executor::{
    CoreApplyOutput, CoreExecutor, CoreExecutorBoundaryHandle, CoreExecutorError,
    CoreExecutorInterruptHandle,
};
use meerkat_core::lifecycle::run_primitive::{
    ConversationContextAppend, CoreRenderable, RunApplyBoundary, RunPrimitive,
};
use meerkat_core::lifecycle::run_receipt::RunBoundaryReceipt;
use meerkat_core::service::{
    CreateSessionRequest, InitialTurnPolicy, SessionBuildOptions, SessionError, SessionService,
    StartTurnRequest, StartTurnRuntimeSemantics,
};
use meerkat_core::types::{ContentInput, HandlingMode, RunResult, SessionId, Usage};
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
    control_listen: String,
    control_advertise: Option<String>,
    data_dir: PathBuf,
    binding_out: Option<PathBuf>,
    model: String,
    provider: String,
    site: String,
    platform: String,
}

#[derive(Serialize)]
struct BindingIdentity {
    kind: &'static str,
    public_key: String,
}

#[derive(Serialize)]
struct BindingFile {
    kind: &'static str,
    id: String,
    name: String,
    site: String,
    platform: String,
    address: String,
    control_url: String,
    peer_id: String,
    public_key: String,
    identity: BindingIdentity,
    bootstrap_token: String,
    labels: BTreeMap<String, String>,
}

#[derive(Clone)]
struct ControlState {
    target_id: String,
    target_name: String,
    session_id: SessionId,
}

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
    target_id: String,
    target_name: String,
    session_id: String,
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
    comms_runtime: Arc<meerkat_comms::CommsRuntime>,
    session_id: SessionId,
}

impl TargetCoreExecutor {
    fn new(
        service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
        mob_state: Arc<MobMcpState>,
        comms_runtime: Arc<meerkat_comms::CommsRuntime>,
        session_id: SessionId,
    ) -> Self {
        Self {
            service,
            mob_state,
            comms_runtime,
            session_id,
        }
    }

    fn log_trusted_routes(&self) {
        let trusted = self.comms_runtime.trusted_peers_shared();
        let guard = trusted.read();
        let routes = guard
            .peers
            .iter()
            .map(|peer| format!("{}|{}|{}", peer.pubkey.to_peer_id(), peer.name, peer.addr))
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("[mdm-target] trusted routes: [{routes}]");
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
        let prompt = primitive.model_projection_content_input();
        let prompt_preview = match &prompt {
            ContentInput::Text(text) => text.replace('\n', " "),
            ContentInput::Blocks(blocks) => format!("{} content blocks", blocks.len()),
        };
        eprintln!(
            "[mdm-target] peer turn accepted: {}",
            prompt_preview.chars().take(160).collect::<String>()
        );
        self.log_trusted_routes();
        if let ContentInput::Text(text) = &prompt
            && should_answer_with_shell_report(text)
        {
            let report = shell_inspection_report(text)
                .await
                .map_err(|error| CoreExecutorError::apply_failed_runtime_turn(error.to_string()))?;
            eprintln!("[mdm-target] peer shell inspection completed");
            if text.contains("Peer request") {
                match parse_peer_request_route(text) {
                    Some(request) => {
                        self.send_peer_response(&request, &report)
                            .await
                            .map_err(|error| {
                                CoreExecutorError::apply_failed_runtime_turn(error.to_string())
                            })?;
                    }
                    None => {
                        eprintln!(
                            "[mdm-target] peer request detected but response route parse failed"
                        );
                    }
                }
            }
            return Ok(CoreApplyOutput::with_run_result(
                RunBoundaryReceipt {
                    run_id,
                    boundary: RunApplyBoundary::RunStart,
                    contributing_input_ids: primitive.contributing_input_ids().to_vec(),
                    conversation_digest: None,
                    message_count: 0,
                    sequence: 0,
                },
                None,
                RunResult {
                    text: report,
                    session_id: self.session_id.clone(),
                    usage: Usage::default(),
                    turns: 0,
                    tool_calls: 1,
                    terminal_cause_kind: None,
                    structured_output: None,
                    extraction_error: None,
                    schema_warnings: None,
                    skill_diagnostics: None,
                },
            ));
        }
        let req = StartTurnRequest {
            prompt,
            system_prompt: None,
            event_tx: None,
            runtime: StartTurnRuntimeSemantics::new(
                metadata.and_then(|meta| meta.render_metadata.clone()),
                metadata
                    .and_then(|meta| meta.handling_mode)
                    .unwrap_or(HandlingMode::Queue),
                metadata.and_then(|meta| meta.skill_references.clone()),
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
        if let Err(error) = self
            .mob_state
            .destroy_bridge_session_mobs(&self.session_id.to_string())
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

struct PeerRequestRoute {
    peer_id: PeerId,
    request_id: InteractionId,
}

fn take_uuid_after(text: &str, marker: &str) -> Option<uuid::Uuid> {
    let rest = text.split_once(marker)?.1.trim_start();
    let candidate = rest
        .chars()
        .take_while(|ch| ch.is_ascii_hexdigit() || *ch == '-')
        .collect::<String>();
    uuid::Uuid::parse_str(&candidate).ok()
}

fn parse_peer_request_route(text: &str) -> Option<PeerRequestRoute> {
    if !text.contains("Peer request") {
        return None;
    }
    let peer_id = PeerId::from_uuid(take_uuid_after(text, "peer_id ")?);
    let request_id = InteractionId(take_uuid_after(text, "(id: ")?);
    Some(PeerRequestRoute {
        peer_id,
        request_id,
    })
}

impl TargetCoreExecutor {
    async fn send_peer_response(
        &self,
        request: &PeerRequestRoute,
        report: &str,
    ) -> anyhow::Result<()> {
        let receipt = meerkat_core::agent::CommsRuntime::send(
            self.comms_runtime.as_ref(),
            CommsCommand::PeerResponse {
                to: PeerRoute::new(request.peer_id),
                in_reply_to: request.request_id,
                status: ResponseStatus::Completed,
                result: serde_json::json!({
                    "kind": "mdm_hardware_report",
                    "text": report,
                }),
                blocks: None,
                handling_mode: Some(HandlingMode::Steer),
            },
        )
        .await
        .context("send correlated peer response")?;
        eprintln!("[mdm-target] peer response sent: {receipt:?}");
        Ok(())
    }
}

fn render_runtime_context_append_text(content: &CoreRenderable) -> String {
    match content {
        CoreRenderable::Text { text } => text.clone(),
        CoreRenderable::Blocks { blocks } => meerkat_core::types::text_content(blocks),
        CoreRenderable::Json { value } => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
        CoreRenderable::Reference { uri, label } => match label {
            Some(label) if !label.trim().is_empty() => format!("[Reference] {label} ({uri})"),
            _ => format!("[Reference] {uri}"),
        },
        _ => String::new(),
    }
}

fn pending_system_context_appends(
    appends: &[ConversationContextAppend],
) -> Vec<PendingSystemContextAppend> {
    let accepted_at = meerkat_core::time_compat::SystemTime::now();
    appends
        .iter()
        .map(|append| PendingSystemContextAppend {
            text: render_runtime_context_append_text(&append.content),
            source: Some(append.key.clone()),
            idempotency_key: Some(append.key.clone()),
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
        control_listen: value("--control-listen").unwrap_or_else(|| {
            derive_control_listen(
                &value("--listen").unwrap_or_else(|| "127.0.0.1:5791".to_string()),
            )
            .unwrap_or_else(|| "127.0.0.1:6791".to_string())
        }),
        control_advertise: value("--control-advertise"),
        data_dir,
        binding_out: value("--binding-out").map(PathBuf::from),
        model: value("--model").unwrap_or_else(|| "gpt-5.5".to_string()),
        provider: value("--provider").unwrap_or_else(|| "openai".to_string()),
        site: value("--site").unwrap_or_else(|| "local".to_string()),
        platform: value("--platform").unwrap_or_else(|| std::env::consts::OS.to_string()),
    })
}

fn derive_control_listen(listen: &str) -> Option<String> {
    let mut addr = listen.parse::<SocketAddr>().ok()?;
    addr.set_port(addr.port().saturating_add(1000));
    Some(addr.to_string())
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

fn advertised_control_url(args: &Args) -> anyhow::Result<String> {
    args.control_listen
        .parse::<SocketAddr>()
        .with_context(|| format!("invalid --control-listen address '{}'", args.control_listen))?;
    if let Some(address) = args.control_advertise.as_ref() {
        if !address.starts_with("http://") && !address.starts_with("https://") {
            anyhow::bail!("invalid --control-advertise URL '{address}': expected http(s) URL");
        }
        return Ok(address.clone());
    }
    Ok(format!("http://{}", args.control_listen))
}

async fn control_health(State(state): State<ControlState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        target_id: state.target_id,
        target_name: state.target_name,
        session_id: state.session_id.to_string(),
    })
}

fn should_answer_with_shell_report(prompt: &str) -> bool {
    let prompt = prompt.to_ascii_lowercase();
    [
        "hardware", "machine", "host", "hostname", "os", "kernel", "cpu", "memory", "shell", "user",
    ]
    .iter()
    .any(|needle| prompt.contains(needle))
}

async fn run_shell(command: &str) -> anyhow::Result<String> {
    let output = tokio::process::Command::new("/bin/sh")
        .arg("-lc")
        .arg(command)
        .output()
        .await
        .with_context(|| format!("run shell command: {command}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        Ok(format!(
            "exit={}; stdout={}; stderr={}",
            output.status, stdout, stderr
        ))
    }
}

async fn shell_inspection_report(prompt: &str) -> anyhow::Result<String> {
    let hostname = run_shell("hostname").await?;
    let user = run_shell("whoami").await?;
    let os = run_shell("uname -a").await?;
    let cpu = run_shell("sysctl -n machdep.cpu.brand_string 2>/dev/null || lscpu 2>/dev/null | sed -n '1,12p' || cat /proc/cpuinfo 2>/dev/null | sed -n '1,12p'").await?;
    let memory = run_shell("sysctl -n hw.memsize 2>/dev/null | awk '{printf \"%.2f GiB\", $1/1024/1024/1024}' || free -h 2>/dev/null || vm_stat 2>/dev/null | sed -n '1,8p'").await?;
    let shell = run_shell("command -v sh; command -v bash || true; command -v zsh || true").await?;
    Ok(format!(
        "Target host inspection for request: {prompt}\n\nhostname: {hostname}\nuser: {user}\nos_kernel: {os}\ncpu: {cpu}\nmemory: {memory}\nshell_tools_available:\n{shell}"
    ))
}

async fn spawn_control_server(args: &Args, state: ControlState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(&args.control_listen)
        .await
        .with_context(|| format!("bind control listener {}", args.control_listen))?;
    let app = Router::new()
        .route("/mdm/health", get(control_health))
        .with_state(state);
    let control_listen = args.control_listen.clone();
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("[mdm-target] control server {control_listen} failed: {error}");
        }
    });
    Ok(())
}

async fn create_comms_runtime(args: &Args) -> anyhow::Result<Arc<meerkat_comms::CommsRuntime>> {
    let config = meerkat_comms::ResolvedCommsConfig {
        enabled: true,
        name: args.id.clone(),
        inproc_namespace: None,
        listen_tcp: Some(args.listen.parse::<SocketAddr>()?),
        listen_uds: None,
        event_listen_tcp: None,
        #[cfg(unix)]
        event_listen_uds: None,
        identity_dir: args.data_dir.join("identity"),
        trusted_peers_path: args.data_dir.join("trusted_peers.json"),
        comms_config: Default::default(),
        auth: Default::default(),
        require_peer_auth: true,
        allow_external_unauthenticated: false,
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
        None,
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
            render_metadata: None,
            system_prompt: Some(system_prompt),
            max_tokens: None,
            event_tx: None,
            skill_references: None,
            initial_turn: InitialTurnPolicy::Defer,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::Discard,
            build: Some(build_opts),
            labels: None,
        })
        .await
        .map_err(|error| anyhow::anyhow!("create session: {error}"))?;
    let session_id = result.session_id;
    let executor = Box::new(TargetCoreExecutor::new(
        surface.service.clone(),
        surface.mob_state.clone(),
        comms_runtime.clone(),
        session_id.clone(),
    ));
    surface
        .runtime_adapter
        .ensure_session_with_executor(session_id.clone(), executor)
        .await;
    let peer_ingress_spawned = surface
        .runtime_adapter
        .update_peer_ingress_context(
            &session_id,
            true,
            Some(comms_runtime as Arc<dyn meerkat_core::agent::CommsRuntime>),
        )
        .await;
    eprintln!("[mdm-target] peer ingress enabled for {session_id}; spawned={peer_ingress_spawned}");
    Ok(session_id)
}

async fn write_binding(
    args: &Args,
    comms_runtime: &meerkat_comms::CommsRuntime,
) -> anyhow::Result<BindingFile> {
    let public_key = comms_runtime.public_key().to_pubkey_string();
    let binding = BindingFile {
        kind: "external",
        id: args.id.clone(),
        name: args.name.clone(),
        site: args.site.clone(),
        platform: args.platform.clone(),
        address: advertised_address(args)?,
        control_url: advertised_control_url(args)?,
        peer_id: comms_runtime.public_key().to_peer_id().to_string(),
        public_key: public_key.clone(),
        identity: BindingIdentity {
            kind: "ed25519_public_key",
            public_key,
        },
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    spawn_control_server(
        &args,
        ControlState {
            target_id: args.id.clone(),
            target_name: args.name.clone(),
            session_id: session_id.clone(),
        },
    )
    .await?;

    println!("{}", serde_json::to_string_pretty(&binding)?);
    eprintln!(
        "[mdm-target] {} ready; session={session_id}; comms={}; control={}",
        args.id, binding.address, binding.control_url
    );
    std::future::pending::<()>().await;
    Ok(())
}
