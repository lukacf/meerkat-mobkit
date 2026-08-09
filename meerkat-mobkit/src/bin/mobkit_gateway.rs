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
use meerkat_mobkit::runtime::cross_mob_control::{
    ControlAuthorizer, ControlGrantTable, ControlListenAddr,
};
use meerkat_mobkit::{
    AuthPolicy, Base64BlobStoreAdapter, BigQueryNaming, BinaryBlobStore, ConsolePolicy,
    ConsoleUiConfig, ConventionalPaths, GatewayPeerKeys, MOBKIT_CONTRACT_VERSION,
    MobBootstrapOptions, MobBootstrapSpec, MobKitStorageLayout, ObjectStoreBlobStore,
    ReleaseMetadata, RuntimeDecisionState, RuntimeOpsPolicy, TrustedOidcRuntimeConfig,
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
    meerkat_mobkit::schedule_wiring::ScheduleFiringHostBinding,
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

/// This binary's own tracing target. Feeds
/// `gateway_composition::default_tracing_filter`, which builds the same
/// filter string this binary used to carry as its own constant: this crate's
/// own targets at INFO, dependencies at WARN. The one-time head-canonical
/// conversion and the storage maintenance verbs report progress at INFO from
/// `meerkat_mobkit`; a blanket "warn" default hid them.
const GATEWAY_TRACING_TARGET: &str = "mobkit_gateway";
type PersistentSessionServiceParts = (
    Arc<dyn meerkat_mob::MobSessionService>,
    Arc<meerkat_runtime::MeerkatMachine>,
    Arc<dyn BinaryBlobStore>,
    Option<ScheduleHostInputs>,
    Option<WorkGraphParts>,
    meerkat_mobkit::storage_health::ResolvedStorageSummary,
    meerkat_mobkit::mob_handle_runtime::SessionWriteEpochsHandle,
    Arc<dyn meerkat_mobkit::identity_first::CommittedBoundaryRecoverer>,
    Arc<dyn meerkat_runtime::RuntimeStore>,
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
    /// Host-level session-compaction policy for every agent this gateway
    /// builds (`{"auto_compact_threshold": 120000, ...}`). Absent, the
    /// gateway inherits meerkat's model-aware default —
    /// `context_window * 4 / 5`, i.e. `840_000` tokens on a million-token
    /// model, which in practice never fires. Validated at init through
    /// [`meerkat_mobkit::parse_compaction_policy`].
    compaction: Option<Value>,
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
    /// Dialable cross-mob control-listener address of the live gateway, so
    /// a resumed launch can still report it. Serde-defaulted: registries
    /// written by older gateways simply resume with no control address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    control_listen_address: Option<String>,
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
    compaction: Option<&meerkat_core::config::CompactionRuntimeConfig>,
    control_listen: Option<&ControlListenAddr>,
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
    // The compaction policy is baked into every agent this runtime builds, so
    // a launch that changes it must NOT resume a runtime built with the old
    // one — that would be the dead-knob failure with extra steps.
    if let Some(policy) = compaction {
        hasher.update(policy.auto_compact_threshold.to_le_bytes());
        hasher.update(if policy.auto_compact_threshold_explicit {
            b"1"
        } else {
            b"0"
        });
        hasher.update(policy.recent_turn_budget.to_le_bytes());
        hasher.update(policy.max_summary_tokens.to_le_bytes());
        hasher.update(policy.min_turns_between_compactions.to_le_bytes());
    }
    hasher.update(b"\n");
    // The control listener binds at launch; resuming a live gateway that was
    // launched without (or with a different) --control-listen would silently
    // drop the flag, so the address participates in the resume key.
    if let Some(addr) = control_listen {
        hasher.update(addr.to_string().as_bytes());
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

/// Returns (session_service, runtime_adapter, binary_blob_store).
///
/// The runtime adapter is supplied separately from the session service so
/// the session service's `runtime_store` stays `None` — keeping the
/// StoreCheckpointer enabled.  The adapter is wired into MobBuilder
/// directly via `with_runtime_adapter()`.
fn build_persistent_session_service(
    layout: &MobKitStorageLayout,
    runtime_root: PathBuf,
    project_root: PathBuf,
    context_root: Option<PathBuf>,
    image_generation: bool,
    realm_id: &str,
    compaction: Option<&meerkat_core::config::CompactionRuntimeConfig>,
) -> anyhow::Result<PersistentSessionServiceParts> {
    let store_dir = layout.state_dir().to_path_buf();
    fs::create_dir_all(&store_dir)
        .with_context(|| format!("failed to create {}", store_dir.display()))?;
    let sqlite_path = layout.session_db().map_err(|e| anyhow!("{e}"))?.path;
    let session_store = Arc::new(
        SqliteSessionStore::open(sqlite_path.clone())
            .with_context(|| format!("failed to open {}", sqlite_path.display()))?,
    );
    // H2: probe the incremental capability on the same store the session
    // service receives below, so whole-blob degradation is loud and
    // health-visible.
    let session_store_incremental = meerkat_mobkit::storage_health::probe_session_store_incremental(
        &(session_store.clone() as Arc<dyn meerkat::SessionStore>),
        "SqliteSessionStore",
    );

    let binary_blob_store: Arc<dyn BinaryBlobStore> =
        Arc::new(ObjectStoreBlobStore::local(layout.blob_root())?);
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
    // Persistent runtime store — same path chosen by
    // `MobBootstrapSpec::persistent_inner`, so a gateway and a library-mode
    // runtime pointed at the same dir share state.
    //
    // Fail-closed (M4): an open failure is a startup error — the former
    // silent `InMemoryRuntimeStore` fallback left resume and archive broken
    // long after boot. This surface has no ephemeral-runtime-store
    // declaration; its ephemeral launch mode (persistent_sessions = false)
    // is the declared in-memory path.
    let runtime_db_path = layout.runtime_db();
    let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
        meerkat_runtime::store::SqliteRuntimeStore::new(&runtime_db_path).map_err(|err| {
            anyhow!(
                "{}",
                meerkat_mobkit::storage_health::RuntimeStoreResolutionError {
                    path: runtime_db_path.clone(),
                    message: err.to_string(),
                }
            )
        })?,
    );
    // Wrap in the write-epoch facade BEFORE the machine and session service
    // capture the store; the witness threads to the bootstrap spec so the
    // console session-history epoch gate works on this externally-composed
    // path (mirrors rpc_gateway.rs — without it the 5s discovery loop
    // re-reads whole session documents forever). The facade also owns the
    // durable session projection at meerkat 0.8.11 (the session service no
    // longer writes the SessionStore itself) and every-boot authority
    // re-minting - a reset/lost runtime.sqlite reseeds from the durable
    // session rows instead of refusing resume.
    let (runtime_store, session_write_epochs) =
        meerkat_mobkit::mob_handle_runtime::epoch_tracking_runtime_store_with_durable_projection(
            runtime_store,
            session_store.clone() as Arc<dyn meerkat::SessionStore>,
        );
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

    // The compaction slot of this config is what meerkat's
    // `AgentFactory::build_agent` turns into the session compactor; an
    // absent declaration inherits the model-aware `context_window * 4 / 5`
    // trigger.
    let mut config = Config::default();
    if let Some(policy) = compaction {
        meerkat_mobkit::apply_compaction_policy(&mut config, policy).map_err(|e| anyhow!("{e}"))?;
    }
    let mut builder = FactoryAgentBuilder::new(factory, config);
    builder.default_blob_store = Some(blob_store.clone());
    // Attach meerkat's per-session schedule tools so members whose profile sets
    // tools.schedule=true get the meerkat_schedule_* surface; the returned
    // service backs the firing host spawned once the runtime has booted.
    let (schedule_tools, schedule_slot) =
        match meerkat_mobkit::schedule_wiring::attach_schedule_tools_with_identity_targets_reporting(
            &builder,
            layout.state_dir(),
        ) {
            Ok(tools) => (
                Some(tools),
                meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                    "schedule",
                    "SqliteScheduleStore",
                ),
            ),
            Err(error) => (
                None,
                meerkat_mobkit::storage_health::StorageSlotSummary::degraded(
                    "schedule",
                    format!("schedule store failed to open; schedule tools disabled: {error}"),
                ),
            ),
        };
    // WorkGraph: durable store beside the schedule store, realm scoped to
    // the mob definition id (member tools + overlays + console RPCs). The
    // state dir travels along so the bootstrap spec can place the
    // cross-process admission sidecar beside the store.
    let workgraph_state_dir = layout.state_dir().to_path_buf();
    let (workgraph, workgraph_slot) =
        match meerkat_mobkit::workgraph_wiring::attach_workgraph_tools_reporting(
            &builder,
            &workgraph_state_dir,
            realm_id,
        ) {
            Ok((service, admission_slot)) => (
                Some((service, admission_slot, workgraph_state_dir)),
                meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                    "workgraph",
                    "SqliteWorkGraphStore",
                ),
            ),
            Err(error) => (
                None,
                meerkat_mobkit::storage_health::StorageSlotSummary::degraded(
                    "workgraph",
                    format!("workgraph store failed to open; workgraph disabled: {error}"),
                ),
            ),
        };
    let service = Arc::new(PersistentSessionService::new(
        builder,
        64,
        session_store,
        Arc::clone(&runtime_store),
        blob_store,
    ));
    let schedule_host_inputs = schedule_tools.map(|tools| {
        (
            tools.service,
            tools.mob_target_registry,
            Arc::clone(&service),
            layout.schedule_db(),
            tools.firing_host_binding,
        )
    });
    // Blob slot resolved fail-closed above (local disk under
    // <store_dir>/blobs). The console timeline and metadata cursor of this
    // surface are in-memory by contract (`UnifiedRuntime::bootstrap` keeps
    // its defaults) — a declared default, documented and health-visible
    // (M4), not an error.
    let mut slots = vec![
        meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
            "sessions",
            "SqliteSessionStore",
        ),
        meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
            "runtime",
            "SqliteRuntimeStore",
        ),
        meerkat_mobkit::storage_health::blob_slot_summary(
            meerkat_mobkit::storage_health::BlobDurability::PersistentDisk,
        ),
        meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
            "console",
            "InMemoryConsoleLogStore",
            "declared default of this surface (UnifiedRuntime::bootstrap keeps in-memory \
             console/metadata)",
        ),
        meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
            "metadata",
            "InMemoryMetadataStore",
            "declared default of this surface (UnifiedRuntime::bootstrap keeps in-memory \
             console/metadata)",
        ),
        schedule_slot,
        workgraph_slot,
    ];
    slots.extend(meerkat_mobkit::storage_health::scratch_ring_buffer_slots());
    // Heal seam (2026-07-29 incident): the CONCRETE persistent service is the
    // committed-boundary recoverer; cast here, before the tuple erases it to
    // `dyn MobSessionService` (mirrors rpc_gateway.rs).
    let committed_boundary_recoverer: Arc<
        dyn meerkat_mobkit::identity_first::CommittedBoundaryRecoverer,
    > = service.clone();
    Ok((
        service,
        adapter,
        binary_blob_store,
        schedule_host_inputs,
        workgraph,
        meerkat_mobkit::storage_health::ResolvedStorageSummary::new(
            meerkat_mobkit::storage_health::BlobDurability::PersistentDisk,
            Some(session_store_incremental),
        )
        .with_slots(slots),
        session_write_epochs,
        committed_boundary_recoverer,
        runtime_store,
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
    control_listen_address: Option<&str>,
) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "contract_version": MOBKIT_CONTRACT_VERSION,
            "runtime_id": runtime_id,
            "http_base_url": http_base_url,
            "launch_state": launch_state,
            // Dialable address of the cross-mob control listener when the
            // gateway was launched with --control-listen (real bound port
            // for tcp://host:0); null otherwise. Peers put this address in
            // their contact directories. Wire-additive optional field.
            "control_listen_address": control_listen_address,
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

const STORAGE_MIGRATE_USAGE: &str = "usage: mobkit_gateway storage-migrate --state-dir <dir> \
     [--apply] [--adopt <path>] [--json]\n\
     Fenced offline migration of one MobKit state directory \
     (storage-unification M6): ledger baseline, legacy-spelling renames, \
     twin reconciliation, leftover census.\n\
     Dry-run by default; --apply mutates under the exclusive maintenance \
     fence. --adopt <path> resolves a divergent file-name twin by adopting \
     that copy and archiving the rest read-only (requires --apply).\n\
     --acknowledge-skipped <SESSION_ID> (repeatable) authorises the ledger \
     bump to proceed despite a blob row that cannot be parsed as a session. \
     Acknowledgement is BY ROW ID, never by count: a row you did not name \
     still blocks, so a later run cannot silently authorise a different set \
     than the one you read.\n\
     Exit codes: 0 clean, 1 refusals or fence/store failure, 2 usage error.";

/// Maintenance verb: `mobkit_gateway storage-migrate`. Runs the five-case
/// M6 migration pass and prints the report. Like the H3 verb, it is a
/// standalone argv verb bypassing the stdin init handshake — migration is
/// never an eager side effect of ordinary gateway startup.
fn run_storage_migrate(args: &[String]) -> i32 {
    let mut state_dir: Option<PathBuf> = None;
    let mut adopt: Option<PathBuf> = None;
    let mut apply = false;
    let mut json = false;
    let mut acknowledged_rows: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--state-dir" => match iter.next() {
                Some(value) => state_dir = Some(PathBuf::from(value)),
                None => {
                    eprintln!("--state-dir requires a directory\n{STORAGE_MIGRATE_USAGE}");
                    return 2;
                }
            },
            "--adopt" => match iter.next() {
                Some(value) => adopt = Some(PathBuf::from(value)),
                None => {
                    eprintln!("--adopt requires a path\n{STORAGE_MIGRATE_USAGE}");
                    return 2;
                }
            },
            "--acknowledge-skipped" => match iter.next() {
                Some(value) => {
                    acknowledged_rows.insert(value.clone());
                }
                None => {
                    eprintln!(
                        "--acknowledge-skipped requires a session id\n{STORAGE_MIGRATE_USAGE}"
                    );
                    return 2;
                }
            },
            "--apply" => apply = true,
            "--json" => json = true,
            other => {
                eprintln!("unknown argument {other:?}\n{STORAGE_MIGRATE_USAGE}");
                return 2;
            }
        }
    }
    let Some(state_dir) = state_dir else {
        eprintln!("--state-dir is required\n{STORAGE_MIGRATE_USAGE}");
        return 2;
    };
    if adopt.is_some() && !apply {
        eprintln!("--adopt requires --apply\n{STORAGE_MIGRATE_USAGE}");
        return 2;
    }
    let mode = if apply {
        meerkat_mobkit::MigrateMode::Apply
    } else {
        meerkat_mobkit::MigrateMode::DryRun
    };
    let report = meerkat_mobkit::migrate_state_dir_acknowledging_skipped(
        &state_dir,
        mode,
        adopt.as_deref(),
        &acknowledged_rows,
    );
    if json {
        match serde_json::to_string_pretty(&report) {
            Ok(text) => println!("{text}"),
            Err(error) => {
                eprintln!("failed to serialize migrate report: {error}");
                return 1;
            }
        }
    } else {
        print_migrate_report_text(&report);
    }
    i32::from(report.has_errors())
}

fn print_migrate_report_text(report: &meerkat_mobkit::MobKitMigrateReport) {
    let mode = match report.mode {
        meerkat_mobkit::MigrateMode::Apply => "apply",
        _ => "dry-run",
    };
    println!(
        "Storage migrate ({mode}) over {} ({} database(s) fenced):",
        report.state_dir.display(),
        report.fenced_databases.len()
    );
    if let Some(backfill) = &report.head_canonical_backfill {
        // The head-canonical crossing is the one irreversible thing this verb
        // does, so its outcome is printed before anything else and states
        // explicitly whether the ledger advanced.
        println!(
            "head-canonical backfill: {} converted of {} pending",
            backfill.converted.len(),
            backfill.examined
        );
        if !backfill.skipped_unparseable.is_empty() {
            println!(
                "  skipped {} malformed blob row(s) (never convertible; not blocking)",
                backfill.skipped_unparseable.len()
            );
        }
        for (session_id, failure) in &backfill.failures {
            if session_id.is_empty() {
                println!("  REFUSED: {failure}");
            } else {
                println!("  FAILED {session_id}: {failure}");
            }
        }
        if backfill.ledger_stamped {
            println!(
                "  ledger STAMPED v2 - the corpus is wholly head-canonical; \
                 rollback to a pre-head-canonical release is no longer possible"
            );
        } else if backfill.applied {
            println!(
                "  ledger LEFT AT v1 - the crossing is incomplete, so rollback \
                 remains available; re-run to resume"
            );
        }
    }
    for twin in &report.twins {
        println!("twin [{}]:", twin.slot);
        for path in &twin.paths {
            println!("  copy: {}", path.display());
        }
        println!(
            "  rows: {} equal across all copies; byte-identical: {}",
            twin.rows_equal, twin.byte_identical
        );
        for row in &twin.rows {
            let status = match &row.status {
                meerkat_mobkit::DivergenceStatus::Equal => "equal".to_string(),
                meerkat_mobkit::DivergenceStatus::Divergent => "divergent".to_string(),
                meerkat_mobkit::DivergenceStatus::OnlyIn { location } => {
                    format!("only in {}", location.display())
                }
                _ => "unknown".to_string(),
            };
            println!("    {}: {status}", row.key);
        }
        match &twin.resolution {
            meerkat_mobkit::TwinResolution::Refused { reason } => {
                println!("  resolution: REFUSED — {reason}");
            }
            meerkat_mobkit::TwinResolution::Deduped { kept, archived } => {
                println!("  resolution: deduped, kept {}", kept.display());
                for archive in archived {
                    println!("    archived read-only: {}", archive.display());
                }
            }
            meerkat_mobkit::TwinResolution::Adopted { adopted, archived } => {
                println!("  resolution: adopted {}", adopted.display());
                for archive in archived {
                    println!("    archived read-only: {}", archive.display());
                }
            }
            _ => println!("  resolution: unknown"),
        }
        for note in &twin.notes {
            println!("  note: {note}");
        }
        for error in &twin.errors {
            println!("  error: {error}");
        }
    }
    for rename in &report.renames {
        let action = match rename.action {
            meerkat_mobkit::RenameAction::WouldRename => "would rename",
            meerkat_mobkit::RenameAction::Renamed => "renamed",
            meerkat_mobkit::RenameAction::Refused => "REFUSED",
            _ => "unknown",
        };
        println!(
            "rename [{}]: {} -> {} ({action}, {} sibling(s), wal checkpointed: {})",
            rename.slot,
            rename.from.display(),
            rename.to.display(),
            rename.siblings.len(),
            rename.wal_checkpointed
        );
    }
    for entry in &report.ledger {
        let action = match entry.action {
            meerkat_mobkit::LedgerBaselineAction::WouldStamp => "would-stamp",
            meerkat_mobkit::LedgerBaselineAction::Recorded => "recorded",
            meerkat_mobkit::LedgerBaselineAction::Stamped => "stamped",
            meerkat_mobkit::LedgerBaselineAction::AlreadyCurrent => "already-current",
            meerkat_mobkit::LedgerBaselineAction::ReportOnly => "report-only",
            meerkat_mobkit::LedgerBaselineAction::Exempt => "exempt",
            _ => "unknown",
        };
        let describe =
            |version: Option<i64>| version.map_or_else(|| "none".to_string(), |v| v.to_string());
        println!(
            "ledger {} [{}]: {} -> {} ({action})",
            entry.database.display(),
            entry.domain,
            describe(entry.before),
            describe(entry.after)
        );
    }
    for finding in &report.findings {
        let path = finding
            .path
            .as_ref()
            .map(|path| format!(" at {}", path.display()))
            .unwrap_or_default();
        println!("leftover [{}] {}{path}", finding.code, finding.message);
    }
    for note in &report.notes {
        println!("note: {note}");
    }
    for error in &report.errors {
        println!("error: {error}");
    }
    println!("storage migrate: {} error(s)", report.errors.len());
}

const STORAGE_PRUNE_USAGE: &str = "usage: mobkit_gateway storage-prune --state-dir <dir> \
     [--apply] [--older-than-days N] [--json]\n\
     Lifecycle of registered maintenance artifacts (`*.pre-*` backups, \
     `*.corrupt-*` quarantines) under one MobKit state directory. Never \
     touches anything outside those naming patterns.\n\
     Dry-run by default; --apply deletes artifacts at least \
     --older-than-days old (default 30; 0 = all).\n\
     Exit codes: 0 clean, 1 delete failures, 2 usage error.";

/// Maintenance verb: `mobkit_gateway storage-prune`.
fn run_storage_prune(args: &[String]) -> i32 {
    let mut state_dir: Option<PathBuf> = None;
    let mut older_than_days: u64 = 30;
    let mut apply = false;
    let mut json = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--state-dir" => match iter.next() {
                Some(value) => state_dir = Some(PathBuf::from(value)),
                None => {
                    eprintln!("--state-dir requires a directory\n{STORAGE_PRUNE_USAGE}");
                    return 2;
                }
            },
            "--older-than-days" => match iter.next().map(|value| value.parse::<u64>()) {
                Some(Ok(days)) => older_than_days = days,
                _ => {
                    eprintln!("--older-than-days requires a number\n{STORAGE_PRUNE_USAGE}");
                    return 2;
                }
            },
            "--apply" => apply = true,
            "--json" => json = true,
            other => {
                eprintln!("unknown argument {other:?}\n{STORAGE_PRUNE_USAGE}");
                return 2;
            }
        }
    }
    let Some(state_dir) = state_dir else {
        eprintln!("--state-dir is required\n{STORAGE_PRUNE_USAGE}");
        return 2;
    };
    let mode = if apply {
        meerkat_mobkit::MigrateMode::Apply
    } else {
        meerkat_mobkit::MigrateMode::DryRun
    };
    let report = meerkat_mobkit::prune_state_dir(&state_dir, older_than_days, mode);
    if json {
        match serde_json::to_string_pretty(&report) {
            Ok(text) => println!("{text}"),
            Err(error) => {
                eprintln!("failed to serialize prune report: {error}");
                return 1;
            }
        }
    } else {
        let mode = if apply { "apply" } else { "dry-run" };
        println!(
            "Storage prune ({mode}, older than {} day(s)) over {}:",
            report.older_than_days,
            report.state_dir.display()
        );
        if report.artifacts.is_empty() {
            println!("No registered maintenance artifacts found.");
        }
        for artifact in &report.artifacts {
            let action = match artifact.action {
                meerkat_mobkit::PruneAction::WouldDelete => "would delete",
                meerkat_mobkit::PruneAction::Deleted => "deleted",
                meerkat_mobkit::PruneAction::Kept => "kept (younger than threshold)",
                meerkat_mobkit::PruneAction::DeleteFailed => "DELETE FAILED",
                _ => "unknown",
            };
            println!(
                "  {}  {} bytes, {} day(s) old — {action}",
                artifact.path.display(),
                artifact.bytes,
                artifact.age_days
            );
        }
        for error in &report.errors {
            println!("error: {error}");
        }
        println!("storage prune: {} error(s)", report.errors.len());
    }
    i32::from(report.has_errors())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!(
            "mobkit_gateway {} (meerkat-mobkit console/HTTP gateway)",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }
    // Install the tracing subscriber FIRST — before the maintenance verbs,
    // not just before the gateway boot. The storage verbs drive the same
    // migration code whose progress reports through tracing; dispatching
    // them ahead of this init dropped every progress line, and a 2026-07
    // production deploy was aborted because a supervisor read the
    // silent-but-working migration as a hang. Also (meerkat-studio K1/K2):
    // without any init, runtime failures, console internal-error logs, and
    // the schedule claim watchdog's stall diagnosis all vanish on the child
    // gateways their app spawns. Stderr, never stdout: stdout carries the
    // init JSON handshake and the verbs' report output.
    //
    // Default: this crate's own targets at INFO, dependencies at WARN;
    // RUST_LOG overrides everything. Shared with rpc_gateway so the two
    // gateways cannot drift on observability posture.
    meerkat_mobkit::gateway_composition::init_gateway_tracing(GATEWAY_TRACING_TARGET);
    if args.first().map(String::as_str) == Some("storage-migrate") {
        std::process::exit(run_storage_migrate(&args[1..]));
    }
    if args.first().map(String::as_str) == Some("storage-prune") {
        std::process::exit(run_storage_prune(&args[1..]));
    }
    // --control-listen <tcp://host:port | uds:///path>: bind the cross-mob
    // control listener so remote gateways can wire/unwire/inject/lookup
    // members of this runtime. Validated here so a typo is a launch error,
    // not a silently ignored flag.
    let control_listen = match meerkat_mobkit::gateway_composition::parse_control_listen_arg(&args)
    {
        Ok(value) => value,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "mobkit_gateway starting (console/HTTP gateway)"
    );
    // Deep worker stacks for the generated machine-authority apply path; the
    // builder is shared with rpc_gateway, the failure reporting is not (this
    // binary's parent reads a JSON-RPC handshake on stdout).
    let runtime = match meerkat_mobkit::gateway_composition::gateway_tokio_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            let response = init_error(Value::Null, -32603, error.to_string());
            print_json_line(&response);
            std::process::exit(1);
        }
    };
    if let Err(error) = runtime.block_on(Box::pin(run(control_listen))) {
        let response = init_error(Value::Null, -32603, error.to_string());
        print_json_line(&response);
        std::process::exit(1);
    }
}

async fn run(control_listen: Option<ControlListenAddr>) -> anyhow::Result<()> {
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
    // The path authority for this boot: the state dir (a `store_path` with a
    // file extension is the explicit session-DB override escape hatch) plus
    // the XDG gateway home (runtime registry + peer key).
    let gateway_home = meerkat_mobkit::storage_layout::default_gateway_home()
        .context("resolve gateway state directory")?;
    let layout = MobKitStorageLayout::standalone_from_store_path(&store_path, gateway_home.clone());
    let persistent_sessions = params.persistent_sessions.unwrap_or(false);
    // Host-level compaction policy: validated once here so a typo or a zero
    // threshold is an init error, not a dead knob or a compaction storm.
    let compaction_policy = params
        .compaction
        .as_ref()
        .map(|value| {
            meerkat_mobkit::parse_compaction_policy(value)
                .map_err(|error| anyhow!("init params: compaction {error}"))
        })
        .transpose()?;
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
        compaction_policy.as_ref(),
        control_listen.as_ref(),
        &paths,
    )?;
    let registry_file = layout
        .registry_file()
        .ok_or_else(|| anyhow!("gateway storage layout carries no gateway home"))?;
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
            entry.control_listen_address.as_deref(),
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
        let (
            service,
            adapter,
            binary_blob_store,
            schedule_host_inputs,
            workgraph,
            resolved_storage,
            session_write_epochs,
            committed_boundary_recoverer,
            runtime_store,
        ) = build_persistent_session_service(
            &layout,
            runtime_root.clone(),
            project_root.clone(),
            context_root.clone(),
            image_generation,
            &runtime_id,
            compaction_policy.as_ref(),
        )?;
        // Pair the schedule wiring with a clone of the runtime adapter so the
        // firing host can be spawned once the runtime has booted (below).
        let schedule_host_inputs =
            schedule_host_inputs.map(|(sched, registry, svc, path, binding)| {
                (sched, registry, svc, path, binding, adapter.clone())
            });
        let workgraph_service = workgraph.as_ref().map(|(service, _, _)| service.clone());
        // The explicit runtime adapter must share the session service's runtime
        // persistence authority or meerkat 0.7 fails the bootstrap closed.
        // `with_session_runtime_adapter` wires the SAME adapter into the session
        // service (mirrors rpc_gateway.rs). This branch already shared via the
        // runtime_store handed to PersistentSessionService, so this is defensive;
        // the default ephemeral branch below is the one that was actually broken.
        let mut spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), service)
            .with_session_write_epochs(&session_write_epochs)
            // Resume-seam reads must carry the runtime store's archived
            // terminal (at 0.8.11 archive stamps the catalog/lifecycle row,
            // never the session body).
            .with_runtime_archived_terminal_authority(runtime_store)
            .with_session_runtime_adapter(adapter.clone())
            .with_workgraph_service(workgraph_service.clone());
        spec.committed_boundary_recoverer = Some(committed_boundary_recoverer);
        if let Some((_, admission_slot, state_dir)) = &workgraph {
            // Durable (cross-process shareable) store: register the tool-plane
            // admission slot and the sidecar lock beside the store.
            spec = spec
                .with_workgraph_admission_slot(admission_slot.clone())
                .with_workgraph_admission_sidecar(state_dir);
        }
        spec.runtime_adapter = Some(adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        spec.resolved_storage = Some(resolved_storage);
        (spec, schedule_host_inputs, workgraph_service)
    } else {
        // Build the ephemeral path manually to thread project/context roots
        // into AgentFactory (MobBootstrapSpec::ephemeral doesn't accept them).
        let binary_blob_store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
        // Same write-epoch facade as the persistent path: the console
        // discovery gate is composition-independent, and the witness is
        // sound for any store as long as every write goes through it.
        let (runtime_store, session_write_epochs) =
            meerkat_mobkit::mob_handle_runtime::epoch_tracking_runtime_store(runtime_store);
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
        // Same seam as the persistent arm: this config's compaction slot is
        // the session compactor meerkat installs for every ephemeral member.
        let mut config = Config::default();
        if let Some(policy) = compaction_policy.as_ref() {
            meerkat_mobkit::apply_compaction_policy(&mut config, policy)
                .map_err(|e| anyhow!("{e}"))?;
        }
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
            .with_session_write_epochs(&session_write_epochs)
            .with_session_runtime_adapter(adapter.clone())
            .with_workgraph_service(workgraph_service.clone())
            .with_workgraph_admission_slot(workgraph_admission_slot);
        spec.runtime_adapter = Some(adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        // In-memory blobs are the declared choice of the default ephemeral
        // launch; no persistent session service, so no H2 flag.
        let mut slots = vec![
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "sessions",
                "EphemeralSessionService",
                "declared by the default ephemeral launch (persistent_sessions = false)",
            ),
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "runtime",
                "InMemoryRuntimeStore",
                "declared by the default ephemeral launch",
            ),
            meerkat_mobkit::storage_health::blob_slot_summary(
                meerkat_mobkit::storage_health::BlobDurability::DeclaredEphemeral,
            ),
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "console",
                "InMemoryConsoleLogStore",
                "declared default of this surface",
            ),
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "metadata",
                "InMemoryMetadataStore",
                "declared default of this surface",
            ),
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "workgraph",
                "MemoryWorkGraphStore",
                "declared by the default ephemeral launch",
            ),
        ];
        slots.extend(meerkat_mobkit::storage_health::scratch_ring_buffer_slots());
        spec.resolved_storage = Some(
            meerkat_mobkit::storage_health::ResolvedStorageSummary::new(
                meerkat_mobkit::storage_health::BlobDurability::DeclaredEphemeral,
                None,
            )
            .with_slots(slots),
        );
        // Ephemeral sessions have no persistent service; the runtime-backed
        // schedule firing host (and thus schedule tools) is persistent-only.
        (spec, None, workgraph_service)
    };
    let mob_spec = session_spec.with_options(MobBootstrapOptions {
        allow_ephemeral_sessions: !persistent_sessions,
        notify_orchestrator_on_resume: true,
        default_llm_client: None,
    });

    let bootstrap_plan =
        meerkat_mobkit::gateway_composition::GatewayRuntimeBootstrapPlan::console_http(
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
        );
    let mut composition = meerkat_mobkit::gateway_composition::GatewayComposition::prepare(
        meerkat_mobkit::gateway_composition::GatewayCompatibilityProfile::ConsoleHttp,
        bootstrap_plan,
    )
    .bootstrap()
    .await
    .context("failed to bootstrap local runtime")?;
    let runtime = composition.runtime_mut();

    // Load contacts.toml if present. This enables mobkit/cross_mob/directory
    // (lookup of known mob addresses) without requiring peer mob handles.
    // High-level wire/unwire/send still need peer handles and are gated
    // separately by has_peer_mob_handles().
    let mut control_grants = ControlGrantTable::new();
    if let Some(ref contacts_path) = paths.contacts_toml {
        let contacts_text = fs::read_to_string(contacts_path)
            .with_context(|| format!("failed to read {}", contacts_path.display()))?;
        let directory = ContactDirectory::from_toml(&contacts_text)
            .with_context(|| format!("failed to parse {}", contacts_path.display()))?;
        if let Some(configured) =
            ControlGrantTable::from_toml(&contacts_text).with_context(|| {
                format!(
                    "failed to parse control grants in {}",
                    contacts_path.display()
                )
            })?
        {
            control_grants = configured;
        }
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
    let peer_keys = GatewayPeerKeys::load_or_create(&gateway_home).with_context(|| {
        format!(
            "failed to load or mint gateway peer key under {}",
            gateway_home.display()
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

        let store_dir = layout.state_dir();
        fs::create_dir_all(store_dir)
            .with_context(|| format!("failed to create {}", store_dir.display()))?;
        let continuity_db = layout.continuity_db().map_err(|e| anyhow!("{e}"))?.path;
        let substrate = meerkat_mobkit::gateway_wiring::open_identity_substrate(&continuity_db)
            .await
            .map_err(|e| anyhow!("{e}"))?;

        let mob_handle = runtime.mob_handle();
        let mut bridge =
            if let Some(session_service) = runtime.mob_runtime().session_service().cloned() {
                MobSessionBridge::with_session_service(mob_handle.clone(), session_service)
            } else {
                MobSessionBridge::new(mob_handle.clone())
            };
        // Heal seam (2026-07-29 incident): the continuity repair supervisor
        // asks this recoverer to commit the durable head before declaring an
        // identity healed; without it, heal is a cosmetic entry reset that
        // the next materialization re-Breaks.
        if let Some(recoverer) = runtime.mob_runtime().committed_boundary_recoverer() {
            bridge = bridge.with_committed_boundary_recoverer(recoverer);
        }
        let bridge: Arc<dyn meerkat_mobkit::identity_first::SessionBridge> = Arc::new(bridge);

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

    // Bind the cross-mob control listener after identity-first attachment so
    // its startup log reflects the final authority posture (the handler
    // re-reads the identity slot per request either way).
    if let Some(addr) = control_listen.as_ref() {
        let authorizer = Arc::new(ControlAuthorizer::with_grants_for_audience(
            control_grants,
            runtime.mob_id(),
        ));
        let advertised = runtime
            .start_control_listener_with_authorizer(addr, authorizer)
            .await
            .map_err(|error| anyhow!("--control-listen {addr}: {error}"))?;
        tracing::info!(%advertised, "cross-mob control listener bound");
    }

    // Start schedule delivery only after identity-first attachment. The host
    // owns a snapshot of the authority Arc, so starting it before this point
    // would permanently reject every generated `rt:*` target.
    let (_schedule_host, _schedule_watchdog) = if let Some((
        schedule_service,
        mob_target_registry,
        service,
        schedule_store_path,
        schedule_firing_host_binding,
        adapter,
    )) = schedule_host_inputs
    {
        let mob_state = meerkat_mobkit::gateway_composition::adopt_schedule_mob_targets(
            runtime,
            &schedule_service,
            &mob_target_registry,
        )
        .await;
        // Shared with rpc_gateway: the watchdog liveness contract log, the
        // host spawn, the firing-intent gate bind and the boot probe were
        // byte-identical in both binaries, every log string included. This
        // gateway declares no host runnables (it has no memory steward and no
        // SDK callback plane), which is now an explicit `None` FIELD rather
        // than a hard-coded argument nobody could see was a divergence.
        let (schedule_host, watchdog) =
            meerkat_mobkit::gateway_composition::spawn_gateway_schedule_host(
                runtime,
                mob_state,
                meerkat_mobkit::gateway_composition::GatewayScheduleHostInputs {
                    schedule_service,
                    session_service: service,
                    runtime_adapter: adapter,
                    schedule_store_path,
                    firing_host_binding: schedule_firing_host_binding,
                    runnable_host: None,
                    workgraph_service: workgraph_service.clone(),
                    owner_id: runtime_id.clone(),
                },
            )
            .await;
        (schedule_host, Some(watchdog))
    } else {
        (None, None)
    };

    let composition = composition.activate();
    let runtime = composition.runtime();
    let http_binding = meerkat_mobkit::gateway_composition::GatewayHttpBinding::bind_loopback()
        .await
        .context("failed to bind gateway listener")?;
    let http_base_url = http_binding.http_base_url();

    let control_listen_address = runtime.control_listener_advertised_address();
    registry.entries.retain(|entry| entry.key != key);
    registry.entries.push(RuntimeRegistryEntry {
        key: key.clone(),
        runtime_id: runtime_id.clone(),
        http_base_url: http_base_url.clone(),
        pid: std::process::id(),
        updated_at_ms: current_time_ms(),
        control_listen_address: control_listen_address.clone(),
    });
    save_registry(&registry_file, &registry)?;

    print_json_line(&init_response(
        request_id,
        &runtime_id,
        &http_base_url,
        "created",
        control_listen_address.as_deref(),
    ));

    let decisions = runtime_decision_state(&runtime_id, console_ui, console_read_only);
    let app = runtime.build_reference_app_router(decisions);
    let mut http_server = http_binding.serve(app);

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
        // SIGINT *and* SIGTERM: a container stop sends SIGTERM, and waiting
        // on ctrl_c alone meant the graceful path never ran on an ordinary
        // deploy. See `meerkat_mobkit::shutdown_signal` for why that became
        // load-bearing at 0.8.22 (unreleased schedule executor lease).
        result = http_server.wait() => {
            match result {
                meerkat_mobkit::gateway_composition::GatewayHttpDrainOutcome::Completed(result) => {
                    result.context("gateway HTTP server failed")?;
                }
                meerkat_mobkit::gateway_composition::GatewayHttpDrainOutcome::TimedOut => {
                    return Err(anyhow!("gateway HTTP server wait timed out"));
                }
                meerkat_mobkit::gateway_composition::GatewayHttpDrainOutcome::JoinFailed(error) => {
                    return Err(anyhow!("gateway HTTP server task failed: {error}"));
                }
            }
        }
        () = stdin_guard => {}
        () = meerkat_mobkit::shutdown_signal::shutdown_signal() => {}
    }

    let shutdown = composition
        .shutdown(
            http_server,
            || async {},
            || async {
                let mut registry = load_registry(&registry_file);
                registry.entries.retain(|entry| entry.key != key);
                save_registry(&registry_file, &registry)
            },
        )
        .await;
    shutdown.cleanup?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default tracing filter (RUST_LOG unset) must surface this crate's
    /// own INFO lines — storage-verb/migration progress was invisible at the
    /// old blanket "warn" default — while keeping dependencies at WARN.
    #[test]
    fn default_tracing_filter_surfaces_own_info_keeps_deps_at_warn() -> anyhow::Result<()> {
        use tracing_subscriber::layer::SubscriberExt;
        let filter = tracing_subscriber::EnvFilter::try_new(
            meerkat_mobkit::gateway_composition::default_tracing_filter(GATEWAY_TRACING_TARGET),
        )?;
        let subscriber = tracing_subscriber::registry().with(filter);
        tracing::subscriber::with_default(subscriber, || {
            assert!(
                tracing::enabled!(
                    target: "meerkat_mobkit::identity_first::local_store",
                    tracing::Level::INFO
                ),
                "the crate's own INFO lines (conversion progress) must pass the default filter"
            );
            assert!(
                tracing::enabled!(target: "mobkit_gateway", tracing::Level::INFO),
                "the gateway binary's own INFO lines must pass the default filter"
            );
            assert!(
                !tracing::enabled!(target: "meerkat_runtime::ops_lifecycle", tracing::Level::INFO),
                "dependency INFO noise must stay filtered"
            );
            assert!(
                tracing::enabled!(target: "meerkat_runtime::ops_lifecycle", tracing::Level::WARN),
                "dependency warnings must still pass"
            );
        });
        Ok(())
    }

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
            None,
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
            None,
            None,
            &paths,
        )?;

        assert_ne!(writable, read_only);
        Ok(())
    }

    /// A launch that changes the compaction policy must not resume a runtime
    /// built with the previous one: the policy is baked into every agent the
    /// runtime constructs, so reuse would silently keep the old trigger.
    #[test]
    fn config_fingerprint_changes_with_compaction_policy() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let paths = conventional_paths(temp.path());
        let pinned = meerkat_mobkit::parse_compaction_policy(&json!({
            "auto_compact_threshold": 120_000
        }))
        .map_err(|e| anyhow!("{e}"))?;
        let lower = meerkat_mobkit::parse_compaction_policy(&json!({
            "auto_compact_threshold": 60_000
        }))
        .map_err(|e| anyhow!("{e}"))?;

        let inherited = config_fingerprint(
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
            None,
            None,
            &paths,
        )?;
        let pinned_key = config_fingerprint(
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
            Some(&pinned),
            None,
            &paths,
        )?;
        let lower_key = config_fingerprint(
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
            Some(&lower),
            None,
            &paths,
        )?;

        assert_ne!(
            inherited, pinned_key,
            "pinning a threshold must not resume an inheriting runtime"
        );
        assert_ne!(
            pinned_key, lower_key,
            "two different thresholds must not share a runtime"
        );
        Ok(())
    }
}
