use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use meerkat::{AgentFactory, Config, FactoryAgentBuilder, PersistentSessionService};
use meerkat_mob::ids::AgentIdentity;
use meerkat_mob::{MobDefinition, MobStorage, ProfileName, SpawnMemberSpec};
use meerkat_mobkit::contact_directory::ContactDirectory;
use meerkat_mobkit::{
    AuthPolicy, Base64BlobStoreAdapter, BigQueryNaming, BinaryBlobStore, ConsolePolicy,
    ConsoleUiConfig, ConventionalPaths, GatewayPeerKeys, MOBKIT_CONTRACT_VERSION,
    MobBootstrapOptions, MobBootstrapSpec, ObjectStoreBlobStore, ReleaseMetadata,
    RuntimeDecisionState, RuntimeOpsPolicy, TrustedOidcRuntimeConfig, UnifiedRuntime,
    load_console_ui_config_from_path_for_realm,
    mob_handle_runtime::mob_definition_may_use_image_generation,
};
use meerkat_store::SqliteSessionStore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, BufReader};

const FALLBACK_TEMPLATE_VERSION: &str = "tux-fallback-v2";

/// Durable schedule service + the concrete persistent session service it shares
/// with the firing host (the `dyn MobSessionService` returned for the spec can't
/// drive the runtime-backed schedule host, which needs the concrete type).
type ScheduleHostInputs = (
    meerkat::ScheduleService,
    meerkat_mobkit::schedule_wiring::ScheduleMobTargetRegistry,
    Arc<PersistentSessionService<FactoryAgentBuilder>>,
    PathBuf,
);
/// WorkGraph wiring from a durable attach: the realm-scoped service, the
/// tool-plane admission slot the bootstrap spec must register, and the state
/// dir for the cross-process admission sidecar (the store file is shareable
/// across processes).
type WorkGraphParts = (
    meerkat::WorkGraphService,
    meerkat_mobkit::workgraph_admission::WorkGraphAdmissionSlot,
    PathBuf,
);
type PersistentSessionServiceParts = (
    Arc<dyn meerkat_mob::MobSessionService>,
    Arc<meerkat_runtime::MeerkatMachine>,
    Arc<dyn BinaryBlobStore>,
    Option<ScheduleHostInputs>,
    Option<WorkGraphParts>,
);

#[derive(Debug, Deserialize)]
struct InitParams {
    workspace_root: Option<PathBuf>,
    project_root: Option<PathBuf>,
    context_root: Option<PathBuf>,
    runtime_root: Option<PathBuf>,
    store_path: Option<PathBuf>,
    persistent_sessions: Option<bool>,
    realm: Option<String>,
    isolated: Option<bool>,
    surface: Option<String>,
    runtime_profile: Option<String>,
    console_read_only: Option<bool>,
    /// Run this gateway identity-first (meerkat-studio ask K0): durable
    /// identities with continuity records, lease-fenced embodiment, and the
    /// tolerant identity lifecycle paths — instead of the session-owned
    /// mob-member surface (where the ask-20 retire/respawn class lives).
    identity_first: Option<bool>,
    /// Desired identity roster for `identity_first: true` — restored at boot
    /// and reconciled thereafter. `mobkit/ensure_member` adds to it at
    /// runtime.
    identity_roster: Option<Vec<meerkat_mobkit::identity_first::DurableAgentSpec>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct RuntimeRegistry {
    entries: Vec<RuntimeRegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeRegistryEntry {
    key: String,
    runtime_id: String,
    http_base_url: String,
    pid: u32,
    updated_at_ms: u64,
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn short_hash(value: &str) -> String {
    value.chars().take(8).collect()
}

fn state_dir() -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("XDG_STATE_HOME")
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path).join("meerkat-mobkit"));
    }

    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("meerkat-mobkit"))
}

fn registry_path() -> anyhow::Result<PathBuf> {
    Ok(state_dir()?.join("tux-runtimes.json"))
}

fn load_registry(path: &Path) -> RuntimeRegistry {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_registry(path: &Path, registry: &RuntimeRegistry) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(registry)?;
    fs::write(path, text)?;
    Ok(())
}

async fn url_is_alive(url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };

    client
        .get(format!("{}/healthz", url.trim_end_matches('/')))
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

fn conventional_paths(workspace_root: &Path) -> ConventionalPaths {
    ConventionalPaths::discover(
        workspace_root.join("config"),
        workspace_root.join("deployment"),
    )
}

fn collect_recursive_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_recursive_files(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn config_fingerprint(
    workspace_root: &Path,
    realm: Option<&str>,
    isolated: bool,
    runtime_profile: &str,
    persistent_sessions: bool,
    console_read_only: bool,
    runtime_root: &Path,
    store_path: &Path,
    project_root: &Path,
    context_root: Option<&Path>,
    paths: &ConventionalPaths,
) -> anyhow::Result<String> {
    let mut hasher = Sha256::new();
    let realpath = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    hasher.update(realpath.to_string_lossy().as_bytes());
    hasher.update(b"\n");
    hasher.update(realm.unwrap_or("").as_bytes());
    hasher.update(b"\n");
    hasher.update(if isolated { b"1" } else { b"0" });
    hasher.update(b"\n");
    hasher.update(runtime_profile.as_bytes());
    hasher.update(b"\n");
    hasher.update(if persistent_sessions { b"1" } else { b"0" });
    hasher.update(b"\n");
    hasher.update(if console_read_only { b"1" } else { b"0" });
    hasher.update(b"\n");
    hasher.update(runtime_root.to_string_lossy().as_bytes());
    hasher.update(b"\n");
    hasher.update(store_path.to_string_lossy().as_bytes());
    hasher.update(b"\n");
    hasher.update(project_root.to_string_lossy().as_bytes());
    hasher.update(b"\n");
    if let Some(ctx) = context_root {
        hasher.update(ctx.to_string_lossy().as_bytes());
    }
    hasher.update(b"\n");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());

    let definition_json = workspace_root.join("definition.json");
    if paths.mob_toml.is_some() {
        hasher.update(b"\nworkspace-config");
    } else if definition_json.exists() {
        hasher.update(b"\ndefinition-json");
    } else {
        // Version the generated fallback runtime separately so local TUX
        // launches do not resume older minimal runtimes after capability
        // changes such as new profiles, tools, or wiring defaults.
        hasher.update(b"\nfallback-template:");
        hasher.update(FALLBACK_TEMPLATE_VERSION.as_bytes());
    }

    let mut files = Vec::new();
    if let Some(path) = &paths.mob_toml {
        files.push(path.clone());
    }
    if let Some(path) = &paths.gating_toml {
        files.push(path.clone());
    }
    if let Some(path) = &paths.console_toml {
        files.push(path.clone());
    }
    if let Some(path) = &paths.routing_toml {
        files.push(path.clone());
    }
    files.extend(paths.schedule_files.clone());
    if definition_json.exists() {
        files.push(definition_json);
    }
    let manifest_toml = workspace_root.join("manifest.toml");
    if manifest_toml.exists() {
        files.push(manifest_toml);
    }
    // Scan workspace root and any override roots for config files
    let mut scan_roots = vec![workspace_root.to_path_buf()];
    if project_root != workspace_root {
        scan_roots.push(project_root.to_path_buf());
    }
    if let Some(ctx) = context_root
        && ctx != workspace_root
    {
        scan_roots.push(ctx.to_path_buf());
    }
    for root in &scan_roots {
        for extra_dir in ["skills", "hooks", "mcp", "config"] {
            collect_recursive_files(&root.join(extra_dir), &mut files);
        }
    }
    files.sort();

    for path in files {
        hasher.update(b"\nfile:");
        hasher.update(path.to_string_lossy().as_bytes());
        if let Ok(bytes) = fs::read(&path) {
            hasher.update(b"\n");
            hasher.update(bytes);
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn minimal_definition(runtime_id: &str) -> anyhow::Result<MobDefinition> {
    MobDefinition::from_toml(&format!(
        r#"
[mob]
id = "{runtime_id}"
orchestrator = "alpha"

[profiles.alpha]
model = "gpt-5.5"
skills = ["alpha-role"]
peer_description = "Runtime guide -- expands this runtime into a small mob and coordinates peers"
external_addressable = true

[profiles.alpha.tools]
builtins = true
comms = true
mob = true
mob_tasks = true

[profiles.worker]
model = "gpt-5.5"
skills = ["worker-role"]
peer_description = "General-purpose peer meerkat"
external_addressable = true

[profiles.worker.tools]
builtins = true
comms = true
mob_tasks = true

[wiring]
auto_wire_orchestrator = true

[skills.alpha-role]
source = "inline"
content = """
## Role
You are Alpha, the runtime guide for a lightweight Meerkat workspace.

## What You Can Do
- Answer directly when the job is simple.
- Grow the runtime into a small mob when parallel work helps.
- Spawn classic sub-agents for delegated background work.
- Spawn peer meerkats when a longer-lived collaborator should appear in the runtime.

## Preferred Growth Pattern
- For quick delegated work, use sub-agent tools.
- For visible collaborators inside this runtime, use mob tools to spawn worker peers.
- When you spawn worker peers, they should appear in the shared runtime UI.

## Coordination
- Use mob tools to spawn, list, wire, and retire meerkats.
- Use peers() and send() when peers are available.
- If asked to create a small team, prefer spawning `worker` peers unless the user clearly asks for classic sub-agents.

## Communication Style
Be explicit about whether you used a sub-agent or spawned a peer meerkat.
"""

[skills.worker-role]
source = "inline"
content = """
You are a general-purpose worker meerkat inside a lightweight runtime.
Complete assigned tasks concisely and report status back to Alpha.
If peer messaging is available, use it to report completion or blockers.
"""
"#
    ))
    .map_err(|error| anyhow!("invalid fallback mob definition: {error}"))
}

fn load_definition(
    workspace_root: &Path,
    fingerprint: &str,
    paths: &ConventionalPaths,
) -> anyhow::Result<(MobDefinition, bool)> {
    if let Some(path) = &paths.mob_toml {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let definition = MobDefinition::from_toml(&text)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        return Ok((definition, true));
    }

    let definition_json_path = workspace_root.join("definition.json");
    if definition_json_path.exists() {
        let text = fs::read_to_string(&definition_json_path)
            .with_context(|| format!("failed to read {}", definition_json_path.display()))?;
        let definition = serde_json::from_str::<MobDefinition>(&text)
            .with_context(|| format!("failed to parse {}", definition_json_path.display()))?;
        return Ok((definition, true));
    }

    let runtime_id = format!("tux-{}", short_hash(fingerprint));
    Ok((minimal_definition(&runtime_id)?, false))
}

fn resolve_store_dir(store_path: &Path) -> (PathBuf, PathBuf) {
    let store_dir = if store_path.extension().is_some() {
        store_path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    } else {
        store_path.to_path_buf()
    };
    let sqlite_path = if store_path.extension().is_some() {
        store_path.to_path_buf()
    } else {
        store_dir.join("sessions.sqlite")
    };
    (store_dir, sqlite_path)
}

/// Returns (session_service, runtime_adapter, binary_blob_store).
///
/// The runtime adapter is supplied separately from the session service so
/// the session service's `runtime_store` stays `None` — keeping the
/// StoreCheckpointer enabled.  The adapter is wired into MobBuilder
/// directly via `with_runtime_adapter()`.
fn build_persistent_session_service(
    store_path: &Path,
    runtime_root: PathBuf,
    project_root: PathBuf,
    context_root: Option<PathBuf>,
    image_generation: bool,
    realm_id: &str,
) -> anyhow::Result<PersistentSessionServiceParts> {
    let (store_dir, sqlite_path) = resolve_store_dir(store_path);
    fs::create_dir_all(&store_dir)
        .with_context(|| format!("failed to create {}", store_dir.display()))?;
    let session_store = Arc::new(
        SqliteSessionStore::open(sqlite_path.clone())
            .with_context(|| format!("failed to open {}", sqlite_path.display()))?,
    );

    let binary_blob_store: Arc<dyn BinaryBlobStore> =
        Arc::new(ObjectStoreBlobStore::local(store_dir.join("blobs"))?);
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
    // Persistent runtime store at <store_dir>/runtime.sqlite — same path
    // chosen by `MobBootstrapSpec::persistent_inner`, so a gateway and a
    // library-mode runtime pointed at the same dir share state.
    let runtime_db_path = sqlite_path
        .parent()
        .map(|p| p.join("runtime.sqlite"))
        .unwrap_or_else(|| std::path::PathBuf::from("runtime.sqlite"));
    let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
        match meerkat_runtime::store::SqliteRuntimeStore::new(&runtime_db_path) {
            Ok(store) => Arc::new(store),
            Err(err) => {
                tracing::warn!(
                    path = %runtime_db_path.display(),
                    error = %err,
                    "failed to open SqliteRuntimeStore; falling back to InMemoryRuntimeStore. \
                     Sessions will not survive process restart and archive operations may fail.",
                );
                Arc::new(meerkat_runtime::InMemoryRuntimeStore::new())
            }
        };
    let adapter = Arc::new(meerkat_runtime::MeerkatMachine::persistent(
        Arc::clone(&runtime_store),
        Arc::clone(&blob_store),
    ));
    let mut factory = AgentFactory::new(store_dir)
        .session_store(session_store.clone())
        .runtime_root(runtime_root)
        .project_root(project_root)
        .builtins(true)
        .shell(true)
        .mob(true)
        .comms(true)
        .memory(true);
    if image_generation {
        factory = factory.with_image_generation_machine(adapter.clone());
    }
    if let Some(context_root) = context_root {
        factory = factory.context_root(context_root);
    }

    let config = Config::default();
    let mut builder = FactoryAgentBuilder::new(factory, config);
    builder.default_blob_store = Some(blob_store.clone());
    // Attach meerkat's per-session schedule tools so members whose profile sets
    // tools.schedule=true get the meerkat_schedule_* surface; the returned
    // service backs the firing host spawned once the runtime has booted.
    let schedule_tools =
        meerkat_mobkit::schedule_wiring::attach_schedule_tools_with_identity_targets(
            &builder,
            runtime_db_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        );
    // WorkGraph: durable store beside the schedule store, realm scoped to
    // the mob definition id (member tools + overlays + console RPCs). The
    // state dir travels along so the bootstrap spec can place the
    // cross-process admission sidecar beside the store.
    let workgraph_state_dir = runtime_db_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let workgraph = meerkat_mobkit::workgraph_wiring::attach_workgraph_tools(
        &builder,
        &workgraph_state_dir,
        realm_id,
    )
    .map(|(service, admission_slot)| (service, admission_slot, workgraph_state_dir));
    let service = Arc::new(PersistentSessionService::new(
        builder,
        64,
        session_store,
        Arc::clone(&runtime_store),
        blob_store,
    ));
    let schedule_store_dir = runtime_db_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let schedule_host_inputs = schedule_tools.map(|tools| {
        (
            tools.service,
            tools.mob_target_registry,
            Arc::clone(&service),
            schedule_store_dir.join(meerkat_mobkit::schedule_wiring::SCHEDULE_STORE_FILE),
        )
    });
    Ok((
        service,
        adapter,
        binary_blob_store,
        schedule_host_inputs,
        workgraph,
    ))
}

fn runtime_decision_state(
    runtime_id: &str,
    console_ui: ConsoleUiConfig,
    console_read_only: bool,
) -> RuntimeDecisionState {
    RuntimeDecisionState {
        bigquery: BigQueryNaming {
            dataset: "tux_local".to_string(),
            table: "runtime_events".to_string(),
        },
        modules: Vec::new(),
        auth: AuthPolicy::default(),
        trusted_oidc: TrustedOidcRuntimeConfig {
            discovery_json: r#"{"issuer":"https://noop.example.com","authorization_endpoint":"https://noop.example.com/auth","token_endpoint":"https://noop.example.com/token","jwks_uri":"https://noop.example.com/.well-known/jwks.json","response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"]}"#.to_string(),
            jwks_json: r#"{"keys":[]}"#.to_string(),
            audience: runtime_id.to_string(),
        },
        console: ConsolePolicy {
            require_app_auth: false,
            read_only: console_read_only,
            fetch_timeout_ms: None,
            ui: console_ui,
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata: ReleaseMetadata {
            targets: vec!["local".to_string()],
            support_matrix: "tux".to_string(),
        },
    }
}

fn print_json_line(value: &Value) {
    let line = serde_json::to_string(value)
        .unwrap_or_else(|_| r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialization failed"}}"#.to_string());
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

fn parse_init_request(line: &str) -> anyhow::Result<(Value, InitParams)> {
    let raw: Value = serde_json::from_str(line).context("failed to parse init request")?;
    let method = raw
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if method != "mobkit/init" {
        return Err(anyhow!("expected mobkit/init, got {method}"));
    }
    let params = raw.get("params").cloned().unwrap_or_else(|| json!({}));
    let parsed: InitParams = serde_json::from_value(params).context("invalid init params")?;
    Ok((raw.get("id").cloned().unwrap_or(Value::Null), parsed))
}

fn env_bool(name: &str) -> anyhow::Result<Option<bool>> {
    let Ok(value) = std::env::var(name) else {
        return Ok(None);
    };
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" => Ok(None),
        "1" | "true" | "yes" | "on" => Ok(Some(true)),
        "0" | "false" | "no" | "off" => Ok(Some(false)),
        _ => Err(anyhow!("{name} must be a boolean value")),
    }
}

fn init_response(
    request_id: Value,
    runtime_id: &str,
    http_base_url: &str,
    launch_state: &str,
) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "contract_version": MOBKIT_CONTRACT_VERSION,
            "runtime_id": runtime_id,
            "http_base_url": http_base_url,
            "launch_state": launch_state,
        }
    })
}

fn init_error(request_id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {
            "code": code,
            "message": message,
        }
    })
}

fn main() {
    if std::env::args()
        .skip(1)
        .any(|a| a == "--version" || a == "-V")
    {
        println!(
            "mobkit_gateway {} (meerkat-mobkit console/HTTP gateway)",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }
    // Install the tracing subscriber FIRST (mirrors rpc_gateway). Without it
    // every tracing event in the process is silently dropped: runtime
    // failures, console internal-error logs, and the schedule claim
    // watchdog's stall diagnosis all vanish — meerkat-studio root-caused
    // their opaque K1/K2 failures to exactly this missing init on the child
    // gateways their app spawns. Stderr, never stdout: stdout carries the
    // init JSON handshake.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "mobkit_gateway starting (console/HTTP gateway)"
    );
    // Meerkat 0.7's generated machine-authority apply path needs deep worker
    // stacks (mirrors meerkat-rpc's explicit 16 MiB tokio worker sizing).
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let response = init_error(Value::Null, -32603, error.to_string());
            print_json_line(&response);
            std::process::exit(1);
        }
    };
    if let Err(error) = runtime.block_on(Box::pin(run())) {
        let response = init_error(Value::Null, -32603, error.to_string());
        print_json_line(&response);
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut init_line = String::new();
    if reader.read_line(&mut init_line).await? == 0 {
        return Err(anyhow!("stdin closed before init request"));
    }

    let (request_id, params) = match parse_init_request(init_line.trim()) {
        Ok(value) => value,
        Err(error) => {
            print_json_line(&init_error(Value::Null, -32602, error.to_string()));
            return Err(error);
        }
    };

    let workspace_root = params
        .workspace_root
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
    let project_root = params
        .project_root
        .unwrap_or_else(|| workspace_root.clone());
    let project_root = project_root.canonicalize().unwrap_or(project_root);
    let context_root = params
        .context_root
        .map(|path| path.canonicalize().unwrap_or(path))
        .or_else(|| Some(project_root.clone()));
    let runtime_root = params
        .runtime_root
        .unwrap_or_else(|| workspace_root.clone());
    let runtime_root = runtime_root.canonicalize().unwrap_or(runtime_root);
    let store_path = params
        .store_path
        .unwrap_or_else(|| runtime_root.join("state"));
    let store_path = store_path.canonicalize().unwrap_or(store_path);
    let persistent_sessions = params.persistent_sessions.unwrap_or(false);
    // Doctrine default: the identity substrate is ON. `identity_first: false`
    // remains as a one-release opt-out for deployments that need the pure
    // mob-plane console back (changelog-flagged).
    let identity_first = params.identity_first.unwrap_or(true);
    let identity_roster_seed = params.identity_roster.clone().unwrap_or_default();
    let realm = params.realm.as_deref();
    let isolated = params.isolated.unwrap_or(false);
    let _surface = params.surface.unwrap_or_else(|| "tux".to_string());
    let runtime_profile = params
        .runtime_profile
        .unwrap_or_else(|| "tux-auto".to_string());
    let console_read_only = match params.console_read_only {
        Some(value) => value,
        None => env_bool("MOBKIT_CONSOLE_READ_ONLY")?.unwrap_or(false),
    };

    let paths = conventional_paths(&workspace_root);
    let key = config_fingerprint(
        &workspace_root,
        realm,
        isolated,
        &runtime_profile,
        persistent_sessions,
        console_read_only,
        &runtime_root,
        &store_path,
        &project_root,
        context_root.as_deref(),
        &paths,
    )?;
    let registry_file = registry_path()?;
    let mut registry = load_registry(&registry_file);

    let mut live_entries = Vec::new();
    let mut resumed_entry = None;
    for entry in registry.entries.drain(..) {
        if url_is_alive(&entry.http_base_url).await {
            if entry.key == key {
                resumed_entry = Some(entry.clone());
            }
            live_entries.push(entry);
        }
    }
    registry.entries = live_entries;
    save_registry(&registry_file, &registry)?;

    if let Some(entry) = resumed_entry {
        print_json_line(&init_response(
            request_id,
            &entry.runtime_id,
            &entry.http_base_url,
            "resumed",
        ));
        return Ok(());
    }

    std::env::set_current_dir(&workspace_root).ok();
    let (definition, used_workspace_config) = load_definition(&workspace_root, &key, &paths)?;
    let console_ui = match &paths.console_toml {
        Some(path) => load_console_ui_config_from_path_for_realm(path, realm)
            .with_context(|| format!("failed to load {}", path.display()))?,
        None => ConsoleUiConfig::default(),
    };
    let runtime_id = definition.id.to_string();
    let image_generation = mob_definition_may_use_image_generation(&definition);

    let (session_spec, schedule_host_inputs, workgraph_service) = if persistent_sessions {
        let (service, adapter, binary_blob_store, schedule_host_inputs, workgraph) =
            build_persistent_session_service(
                &store_path,
                runtime_root.clone(),
                project_root.clone(),
                context_root.clone(),
                image_generation,
                &runtime_id,
            )?;
        // Pair the schedule wiring with a clone of the runtime adapter so the
        // firing host can be spawned once the runtime has booted (below).
        let schedule_host_inputs = schedule_host_inputs
            .map(|(sched, registry, svc, path)| (sched, registry, svc, path, adapter.clone()));
        let workgraph_service = workgraph.as_ref().map(|(service, _, _)| service.clone());
        // The explicit runtime adapter must share the session service's runtime
        // persistence authority or meerkat 0.7 fails the bootstrap closed.
        // `with_session_runtime_adapter` wires the SAME adapter into the session
        // service (mirrors rpc_gateway.rs). This branch already shared via the
        // runtime_store handed to PersistentSessionService, so this is defensive;
        // the default ephemeral branch below is the one that was actually broken.
        let mut spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), service)
            .with_session_runtime_adapter(adapter.clone())
            .with_workgraph_service(workgraph_service.clone());
        if let Some((_, admission_slot, state_dir)) = &workgraph {
            // Durable (cross-process shareable) store: register the tool-plane
            // admission slot and the sidecar lock beside the store.
            spec = spec
                .with_workgraph_admission_slot(admission_slot.clone())
                .with_workgraph_admission_sidecar(state_dir);
        }
        spec.runtime_adapter = Some(adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        (spec, schedule_host_inputs, workgraph_service)
    } else {
        // Build the ephemeral path manually to thread project/context roots
        // into AgentFactory (MobBootstrapSpec::ephemeral doesn't accept them).
        let binary_blob_store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
        let adapter = Arc::new(meerkat_runtime::MeerkatMachine::persistent(
            Arc::clone(&runtime_store),
            Arc::clone(&blob_store),
        ));
        let mut factory = AgentFactory::new(&runtime_root)
            .runtime_root(runtime_root.clone())
            .project_root(project_root.clone())
            .builtins(true)
            .shell(true)
            .mob(true)
            .comms(true)
            .memory(true);
        if image_generation {
            factory = factory.with_image_generation_machine(adapter.clone());
        }
        if let Some(ref ctx) = context_root {
            factory = factory.context_root(ctx.clone());
        }
        let config = Config::default();
        let mut builder = FactoryAgentBuilder::new(factory, config);
        builder.default_blob_store = Some(blob_store);
        // The default TUX launch is ephemeral: a memory-backed workgraph
        // keeps the feature available (tools stay profile-gated). Memory
        // store = single process, so no admission sidecar.
        let (ephemeral_workgraph, workgraph_admission_slot) =
            meerkat_mobkit::workgraph_wiring::attach_workgraph_tools_ephemeral(
                &builder,
                &runtime_id,
            );
        let workgraph_service = Some(ephemeral_workgraph);
        let session_service = Arc::new(meerkat_session::EphemeralSessionService::new(builder, 64));
        // THE FIX: share the explicit adapter's persistence authority with the
        // session service. Without this, EphemeralSessionService keeps its own
        // (store-less) adapter, meerkat 0.7's canonical_runtime_adapter check sees
        // a mismatch and fails closed with "failed to bootstrap local runtime" —
        // which is what broke every shipped 0.7.x mobkit_gateway binary on the
        // first mobkit/init (persistent_sessions defaults off, so this is the path
        // every launch hits). rpc_gateway.rs already had this call.
        let mut spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
            .with_session_runtime_adapter(adapter.clone())
            .with_workgraph_service(workgraph_service.clone())
            .with_workgraph_admission_slot(workgraph_admission_slot);
        spec.runtime_adapter = Some(adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        // Ephemeral sessions have no persistent service; the runtime-backed
        // schedule firing host (and thus schedule tools) is persistent-only.
        (spec, None, workgraph_service)
    };
    let mob_spec = session_spec.with_options(MobBootstrapOptions {
        allow_ephemeral_sessions: !persistent_sessions,
        notify_orchestrator_on_resume: true,
        default_llm_client: None,
    });

    let mut runtime = Box::pin(UnifiedRuntime::bootstrap(
        mob_spec,
        meerkat_mobkit::MobKitConfig {
            modules: Vec::new(),
            discovery: meerkat_mobkit::DiscoverySpec {
                namespace: format!("tux.{}", short_hash(&key)),
                modules: Vec::new(),
            },
            pre_spawn: Vec::new(),
        },
        Duration::from_secs(30),
    ))
    .await
    .context("failed to bootstrap local runtime")?;

    // Run the schedule driver so members' authored schedules actually fire: at
    // due time it materializes a session and runs the prompt as a real agent
    // turn (session targets via the runtime-backed host, mob targets via the
    // mob runtime). Held for the gateway's lifetime — dropping the handle shuts
    // the host down. Persistent sessions only.
    let (_schedule_host, _schedule_watchdog) = if let Some((
        schedule_service,
        mob_target_registry,
        service,
        schedule_store_path,
        adapter,
    )) = schedule_host_inputs
    {
        let mob_state = runtime.mob_runtime().agent_mob_mcp_state();
        mob_target_registry.set_mob_state(mob_state.clone());
        match meerkat_mobkit::schedule_wiring::repair_resumable_session_targets_to_mob_members(
            &schedule_service,
            &mob_target_registry,
        )
        .await
        {
            Ok(repaired) if repaired > 0 => {
                tracing::info!(
                    repaired,
                    "repaired persisted resumable-session schedules to identity mob targets"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "failed to repair persisted resumable-session schedules to identity mob targets",
                );
            }
        }
        // Same silent-stall guard as rpc_gateway: the upstream driver
        // discards tick errors, so stalls only become visible here.
        let watchdog = meerkat_mobkit::schedule_wiring::spawn_schedule_claim_watchdog(
            schedule_service.clone(),
            schedule_store_path,
            Default::default(),
        );
        (
            meerkat_mobkit::schedule_wiring::spawn_schedule_host(
                service,
                adapter,
                schedule_service,
                mob_state,
                runtime.mob_handle(),
                None,
                workgraph_service.clone(),
                runtime_id.clone(),
            ),
            Some(watchdog),
        )
    } else {
        (None, None)
    };

    // Load contacts.toml if present. This enables mobkit/cross_mob/directory
    // (lookup of known mob addresses) without requiring peer mob handles.
    // High-level wire/unwire/send still need peer handles and are gated
    // separately by has_peer_mob_handles().
    if let Some(ref contacts_path) = paths.contacts_toml {
        let contacts_text = fs::read_to_string(contacts_path)
            .with_context(|| format!("failed to read {}", contacts_path.display()))?;
        let directory = ContactDirectory::from_toml(&contacts_text)
            .with_context(|| format!("failed to parse {}", contacts_path.display()))?;
        runtime.set_contact_directory(directory);
    }

    // Opt-in ABAC: a `config/access.toml` wires the access controller into
    // every console and SSE surface. Console admin edits persist back to
    // the same file, so the deployment's access policy survives restarts.
    if let Some(ref access_path) = paths.access_toml {
        let controller = meerkat_mobkit::AccessController::load_or_default(access_path)
            .map_err(|error| anyhow!("failed to load {}: {error}", access_path.display()))?;
        runtime.set_access_controller(controller);
    }

    // Load (or mint and persist) the gateway's Ed25519 signing keypair.
    // Stored under the same state directory the registry lives in, which
    // already survives across runs. Cross-process peers fetch the
    // resulting pubkey via `mobkit/peer_pubkey`; inproc-only deployments
    // never use it but it's cheap to keep one ready.
    let gateway_state_dir = state_dir().context("resolve gateway state directory")?;
    let peer_keys = GatewayPeerKeys::load_or_create(&gateway_state_dir).with_context(|| {
        format!(
            "failed to load or mint gateway peer key under {}",
            gateway_state_dir.display()
        )
    })?;
    runtime.set_gateway_peer_keys(peer_keys);

    if !used_workspace_config {
        let mut labels = BTreeMap::new();
        labels.insert("surface".to_string(), "tux".to_string());
        labels.insert("ui".to_string(), "meerkat-tux".to_string());
        if let Some(realm) = realm {
            labels.insert("realm".to_string(), realm.to_string());
        }
        runtime
            .mob_handle()
            .ensure_member(
                SpawnMemberSpec::new(ProfileName::from("alpha"), AgentIdentity::from("alpha"))
                    .with_labels(labels),
            )
            .await
            .map_err(|error| anyhow!("failed to spawn fallback alpha meerkat: {error}"))?;
    }

    // Identity-first mode (meerkat-studio ask K0): give this gateway the
    // durable-identity substrate — continuity records, lease-fenced
    // embodiment, resume-first restore with the Broken-identity repair task,
    // and the tolerant identity lifecycle paths. Default providers are
    // constructed from the existing store paths; the roster is seeded from
    // init params and extended at runtime by `mobkit/ensure_member`.
    let _identity_roster_provider: Option<
        Arc<meerkat_mobkit::identity_first::MutableRosterProvider>,
    > = if identity_first {
        use meerkat_mobkit::identity_first::{
            AgentRuntimeServices, DurabilityPolicy, IdentityFirstRuntimeContext, IdentityRuntime,
            IdentityRuntimeConfig, MobSessionBridge, MutableRosterProvider, restore_flow,
        };

        let (store_dir, _) = resolve_store_dir(&store_path);
        fs::create_dir_all(&store_dir)
            .with_context(|| format!("failed to create {}", store_dir.display()))?;
        let continuity_db = store_dir.join("continuity.db");
        let substrate = meerkat_mobkit::gateway_wiring::open_identity_substrate(&continuity_db)
            .map_err(|e| anyhow!("{e}"))?;

        let mob_handle = runtime.mob_handle();
        let bridge: Arc<dyn meerkat_mobkit::identity_first::SessionBridge> =
            if let Some(session_service) = runtime.mob_runtime().session_service().cloned() {
                Arc::new(MobSessionBridge::with_session_service(
                    mob_handle.clone(),
                    session_service,
                ))
            } else {
                Arc::new(MobSessionBridge::new(mob_handle.clone()))
            };

        let irt = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: substrate.continuity_store,
            lease_provider: substrate.lease_provider,
            runtime_instance_id: format!("mobkit-gateway-{}", std::process::id()),
            has_runtime_store: persistent_sessions,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge),
            default_timeout: None,
        })
        .with_runtime_services(AgentRuntimeServices::new(mob_handle.clone()));

        let roster = Arc::new(MutableRosterProvider::new(identity_roster_seed));
        let mob_definition = mob_handle.definition().clone();
        let irt = Arc::new(irt);
        restore_flow(&irt, &roster.snapshot(), None, None)
            .await
            .context("identity-first restore_flow failed")?;
        // Attaching the context wires the console's identity RPC surface and
        // spawns the Broken-identity repair task; the roster slot lets
        // `mobkit/ensure_member` extend the desired roster at runtime.
        runtime.set_console_identity_roster(roster.clone());
        runtime.attach_identity_first_context(Arc::new(IdentityFirstRuntimeContext::new(
            irt,
            roster.clone(),
            None,
            None,
            Some(mob_definition),
        )));
        tracing::info!(
            roster = roster.snapshot().len(),
            continuity_db = %continuity_db.display(),
            "identity-first gateway mode active"
        );
        Some(roster)
    } else {
        None
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind gateway listener")?;
    let http_base_url = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().context("missing local addr")?.port()
    );

    registry.entries.retain(|entry| entry.key != key);
    registry.entries.push(RuntimeRegistryEntry {
        key: key.clone(),
        runtime_id: runtime_id.clone(),
        http_base_url: http_base_url.clone(),
        pid: std::process::id(),
        updated_at_ms: current_time_ms(),
    });
    save_registry(&registry_file, &registry)?;

    print_json_line(&init_response(
        request_id,
        &runtime_id,
        &http_base_url,
        "created",
    ));

    let decisions = runtime_decision_state(&runtime_id, console_ui, console_read_only);
    let app = runtime.build_reference_app_router(decisions);

    // `mobkit_gateway` serves the console/admin API over HTTP. It is NOT the
    // SDK's stdin JSON-RPC gateway — that is the separate `rpc_gateway` binary.
    // The SDK's PersistentTransport drives a gateway by sending JSON-RPC lines
    // (init, then reconcile_identity, send, ...) over stdin; we already consumed
    // the single init line above. If more JSON-RPC arrives on stdin, this binary
    // was misconfigured as the SDK gateway (a common mix-up, since the SDK env
    // var is named MOBKIT_RPC_GATEWAY_BIN and the release ships this binary).
    // Answer each such line with a clear JSON-RPC error instead of silently
    // ignoring it, so the SDK surfaces an actionable failure immediately rather
    // than blocking forever waiting for a response that never comes.
    let stdin_guard = async move {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break, // launching parent closed stdin
                Ok(_) => {}
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let id = serde_json::from_str::<Value>(trimmed)
                .ok()
                .and_then(|value| value.get("id").cloned())
                .unwrap_or(Value::Null);
            print_json_line(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": "mobkit_gateway serves the console/admin API over HTTP and does not handle SDK stdin JSON-RPC after init. Point your SDK gateway path (MOBKIT_RPC_GATEWAY_BIN) at the 'rpc_gateway' binary instead."
                }
            }));
        }
        // stdin ended: keep serving HTTP until ctrl-c rather than tearing down
        // the console out from under any live HTTP clients.
        std::future::pending::<()>().await;
    };

    tokio::select! {
        result = axum::serve(listener, app).with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        }) => {
            result.context("gateway HTTP server failed")?;
        }
        () = stdin_guard => {}
    }

    let mut registry = load_registry(&registry_file);
    registry.entries.retain(|entry| entry.key != key);
    save_registry(&registry_file, &registry)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_params_parse_console_read_only() -> anyhow::Result<()> {
        let line = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "mobkit/init",
            "params": {
                "console_read_only": true
            }
        })
        .to_string();

        let (_id, params) = parse_init_request(&line)?;

        assert_eq!(params.console_read_only, Some(true));
        Ok(())
    }

    #[test]
    fn runtime_decision_state_projects_console_read_only() {
        let state = runtime_decision_state("test-runtime", ConsoleUiConfig::default(), true);

        assert!(state.console.read_only);
    }

    #[test]
    fn config_fingerprint_changes_with_console_read_only() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let paths = conventional_paths(temp.path());

        let writable = config_fingerprint(
            temp.path(),
            None,
            false,
            "tux-auto",
            false,
            false,
            temp.path(),
            temp.path(),
            temp.path(),
            None,
            &paths,
        )?;
        let read_only = config_fingerprint(
            temp.path(),
            None,
            false,
            "tux-auto",
            false,
            true,
            temp.path(),
            temp.path(),
            temp.path(),
            None,
            &paths,
        )?;

        assert_ne!(writable, read_only);
        Ok(())
    }
}
