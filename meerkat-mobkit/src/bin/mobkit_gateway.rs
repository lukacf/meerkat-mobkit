use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use meerkat_mob::{MeerkatId, MobDefinition, MobStorage, ProfileName, SpawnMemberSpec};
use meerkat_mobkit::{
    AuthPolicy, BigQueryNaming, ConsolePolicy, ConventionalPaths, MOBKIT_CONTRACT_VERSION,
    MobBootstrapOptions, MobBootstrapSpec, ReleaseMetadata, RuntimeDecisionState, RuntimeOpsPolicy,
    TrustedOidcRuntimeConfig, UnifiedRuntime,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, BufReader};

const FALLBACK_TEMPLATE_VERSION: &str = "tux-fallback-v2";

#[derive(Debug, Deserialize)]
struct InitParams {
    workspace_root: Option<PathBuf>,
    realm: Option<String>,
    isolated: Option<bool>,
    surface: Option<String>,
    runtime_profile: Option<String>,
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

fn config_fingerprint(
    workspace_root: &Path,
    realm: Option<&str>,
    isolated: bool,
    runtime_profile: &str,
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
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());

    if paths.mob_toml.is_some() {
        hasher.update(b"\nworkspace-config");
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
    if let Some(path) = &paths.routing_toml {
        files.push(path.clone());
    }
    files.extend(paths.schedule_files.clone());
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
model = "gpt-5.2"
skills = ["alpha-role"]
peer_description = "Runtime guide -- expands this runtime into a small mob and coordinates peers"
external_addressable = true

[profiles.alpha.tools]
builtins = true
comms = true
mob = true
mob_tasks = true

[profiles.worker]
model = "gpt-5.2"
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
    _workspace_root: &Path,
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

    let runtime_id = format!("tux-{}", short_hash(fingerprint));
    Ok((minimal_definition(&runtime_id)?, false))
}

fn runtime_decision_state(runtime_id: &str) -> RuntimeDecisionState {
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

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
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
    let realm = params.realm.as_deref();
    let isolated = params.isolated.unwrap_or(false);
    let _surface = params.surface.unwrap_or_else(|| "tux".to_string());
    let runtime_profile = params
        .runtime_profile
        .unwrap_or_else(|| "tux-auto".to_string());

    let paths = conventional_paths(&workspace_root);
    let key = config_fingerprint(&workspace_root, realm, isolated, &runtime_profile, &paths)?;
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
    let runtime_id = definition.id.to_string();

    let mob_spec = MobBootstrapSpec::ephemeral(
        definition,
        MobStorage::in_memory(),
        workspace_root.clone(),
        64,
        None,
    )
    .with_options(MobBootstrapOptions {
        allow_ephemeral_sessions: true,
        notify_orchestrator_on_resume: true,
        default_llm_client: None,
    });

    let runtime = UnifiedRuntime::bootstrap(
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
    )
    .await
    .context("failed to bootstrap local runtime")?;

    if !used_workspace_config {
        let mut labels = BTreeMap::new();
        labels.insert("surface".to_string(), "tux".to_string());
        labels.insert("ui".to_string(), "meerkat-tux".to_string());
        if let Some(realm) = realm {
            labels.insert("realm".to_string(), realm.to_string());
        }
        runtime
            .ensure_member(
                SpawnMemberSpec::new(ProfileName::from("alpha"), MeerkatId::from("alpha"))
                    .with_labels(labels),
            )
            .await
            .map_err(|error| anyhow!("failed to spawn fallback alpha meerkat: {error}"))?;
    }

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

    let decisions = runtime_decision_state(&runtime_id);
    axum::serve(listener, runtime.build_reference_app_router(decisions))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("gateway HTTP server failed")?;

    let mut registry = load_registry(&registry_file);
    registry.entries.retain(|entry| entry.key != key);
    save_registry(&registry_file, &registry)?;
    Ok(())
}
