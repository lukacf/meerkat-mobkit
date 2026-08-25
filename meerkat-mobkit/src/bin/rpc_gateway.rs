#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::collapsible_if,
    clippy::redundant_clone,
    clippy::needless_raw_string_hashes,
    clippy::single_match,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_pattern_matching,
    clippy::ignored_unit_patterns,
    clippy::clone_on_copy,
    clippy::manual_assert,
    clippy::unwrap_in_result,
    clippy::useless_vec
)]
//! Phase 0b binary — JSON-RPC gateway bridging SDK clients to the unified runtime.

/// This binary's own tracing target. Feeds
/// `gateway_composition::default_tracing_filter`, which builds the same
/// filter string this binary used to carry as its own constant: this crate's
/// own targets at INFO, dependencies at WARN. Operationally significant boot
/// phases (the one-time head-canonical conversion, continuity repair) report
/// at INFO from `meerkat_mobkit`; the old blanket "warn" default hid them,
/// and a 2026-07 production deploy was aborted when a supervisor read a
/// silent-but-working migration as a hang.
const GATEWAY_TRACING_TARGET: &str = "rpc_gateway";

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine;
use meerkat_mobkit::contact_directory::ContactDirectory;
use meerkat_mobkit::runtime::cross_mob_control::{
    ControlAuthorizer, ControlGrantTable, ControlListenAddr,
};
use meerkat_mobkit::unified_runtime::EventLogError;
use meerkat_mobkit::unified_runtime::types::IdentityAuthorityReleaseOutcome;
use meerkat_mobkit::unified_runtime::types::RetiredSupervisorCleanupOutcome;
use meerkat_mobkit::{
    AuthPolicy, AuthProvider, Base64BlobStoreAdapter, BigQueryNaming, BinaryBlobStore,
    ConsolePolicy, ConsoleUiConfig, DiscoverySpec, EventLogConfig, EventLogStore, EventQuery,
    InMemoryMetadataStore, LocalJsonMemoryBackendConfig, MOBKIT_CONTRACT_VERSION,
    MemoryBackendConfig, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig, ModuleConfig,
    ObjectStoreBlobStore, PersistedEvent, PersistentMetadataStore, PreSpawnData, ReleaseMetadata,
    RestartPolicy, RuntimeDecisionState, RuntimeOpsPolicy, RuntimeOptions, RuntimeRoute,
    STORAGE_RESOLUTION_CODE, SqliteConsoleLogStore, SqliteMetadataStore, TrustedOidcRuntimeConfig,
    UnifiedRuntime, UnifiedRuntimeShutdownReport, handle_mobkit_rpc_json,
    load_console_ui_config_from_path_for_realm,
    mob_handle_runtime::{mob_definition_may_use_image_generation, mob_definition_may_use_shell},
    start_mobkit_runtime,
};
use sha2::{Digest, Sha256};

use async_trait::async_trait;
use meerkat::{
    AgentEvent, AgentFactory, Config, CreateSessionRequest, EphemeralSessionService, FactoryAgent,
    FactoryAgentBuilder, SessionAgentBuilder, SessionError,
};
use meerkat_core::ContentBlock;
use meerkat_core::error::{AgentError, ToolError};
use meerkat_core::ops::ToolDispatchOutcome;
use meerkat_core::types::{ToolCallView, ToolDef, ToolResult};
use meerkat_core::{
    AgentToolDispatcher, ToolCatalogCapabilities, ToolCatalogEntry, ToolDeadlineContributor,
    ToolDeadlineOwner, ToolExecutionContract, ToolExecutionMode,
};
use meerkat_mob::{MobDefinition, MobStorage};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc, oneshot};

#[derive(Clone, Default)]
struct GatewayGatingConfig {
    action_risk_tiers: HashMap<String, String>,
}

struct GatewayRuntimeOptions {
    runtime_options: RuntimeOptions,
    /// Presence is identity-first intent. `None` preserves the classic gateway
    /// when no roster is configured; a roster still defaults to eager restore.
    identity_bootstrap_mode: Option<meerkat_mobkit::IdentityBootstrapMode>,
    max_sessions: usize,
    routing_routes: Vec<RuntimeRoute>,
    gating: GatewayGatingConfig,
    event_log: Option<EventLogConfig>,
    decisions: Option<RuntimeDecisionState>,
    console_ui: ConsoleUiConfig,
    console_require_app_auth: Option<bool>,
    console_read_only: Option<bool>,
    console_fetch_timeout_ms: Option<u64>,
    access: Option<meerkat_mobkit::AccessController>,
    demo_llm: bool,
    /// Bind each locally hosted member to a signed loopback TCP endpoint so
    /// peer-only external members in another process can return traffic.
    member_comms_address: Option<String>,
    /// `runtime_options.contacts_toml`: the cross-mob contact directory as
    /// inline TOML (same format as `config/contacts.toml` on
    /// mobkit_gateway). Inline because SDK-driven gateways receive all
    /// launch config through init params, and cross-process tests must
    /// write a peer's bound address into the directory at spawn time.
    contacts: Option<ContactDirectory>,
    /// Scoped caller grants for the cross-mob control listener. The empty
    /// default is deny-all, never an implicit open listener.
    control_grants: ControlGrantTable,
    agent_memory: Option<GatewayAgentMemoryOptions>,
    /// WorkGraph service construction switch (default on). `false` disables
    /// the store, member tools, overlays, and the mobkit/workgraph/* RPCs.
    workgraph: GatewayWorkgraphOption,
    /// Live (realtime) transport opt-in (default off). Persistent mode only.
    live: GatewayLiveOption,
    /// Strict experimental GPT Live registration. Independent from the
    /// ordinary WebSocket live option so opting in does not mount an HTTP
    /// route. Every authority-bearing value is explicit and absence keeps
    /// capability projection fail-closed.
    #[cfg(feature = "experimental-gpt-live")]
    experimental_live: Option<GatewayExperimentalLiveOption>,
    /// SDK-registered deterministic schedule targets
    /// (`runtime_options.host_runnables`): each name registers a schedule
    /// host runnable whose fire forwards over the callback bridge as
    /// `callback/schedule_fire`.
    host_runnables: Vec<meerkat::HostRunnableName>,
    /// `runtime_options.runtime_store = {"storage": "memory"}`: the explicit
    /// declaration that the runtime store is in-memory on a persistent
    /// launch (M4). Without it, a failed `runtime.sqlite` open is a startup
    /// error — the former silent `InMemoryRuntimeStore` fallback is gone.
    runtime_store_ephemeral: bool,
    /// Declared in-memory mob storage. Persistent SQLite is the default on a
    /// persistent_state launch, so this exists for launches that would rather
    /// keep an editable mob_config than durable mob state: a persistent mob
    /// storage pins the definition, because Meerkat refuses a definition that
    /// disagrees with the persisted spec store.
    mob_storage_ephemeral: bool,
    /// `runtime_options.declare_spec_update = {"expected_revision": N}`: an
    /// explicit operator declaration that the persisted mob spec now matches the
    /// definition this launch supplies.
    ///
    /// The door through the pinned-`mob_config` refusal. Present only on the
    /// activation that intends to move the pin - it is a declared transition, not
    /// a mode. Compare-and-swapped on `expected_revision`, so an activation that
    /// names a revision the store has moved past is refused rather than
    /// overwriting a spec the operator never saw.
    declare_spec_update: Option<u64>,
    /// `runtime_options.mob_composition = {"authority": "candidate"}`: this
    /// launch does NOT speak for the durable composition, so it neither creates
    /// the composition pin nor is refused by one.
    ///
    /// For candidate/certification boots in a candidate-then-promote pipeline
    /// against one state directory. Without it such a boot pins its own
    /// deliberately-restricted composition and the promoted boot is refused -
    /// which cost a live household 929 supervisor respawns and a rollback.
    composition_authority: meerkat_mobkit::mob_composition_manifest::CompositionAuthority,
    /// `runtime_options.compaction = {"auto_compact_threshold": 120000, ...}`:
    /// the host-level session-compaction policy for every agent this gateway
    /// builds. Absent, the gateway inherits meerkat's model-aware default
    /// (`context_window * 4 / 5` — `840_000` tokens on a million-token model,
    /// i.e. effectively "never compact"). See
    /// [`meerkat_mobkit::compaction_policy`].
    compaction: Option<meerkat_core::config::CompactionRuntimeConfig>,
}

/// `runtime_options.live` wire forms: `true` mounts the live WebSocket
/// transport on the gateway's HTTP listener with bootstrap URLs derived from
/// the loopback base; the object form
/// `{"public_base_url": "ws://192.168.0.123:8080", "seed_max_chars": 200000}`
/// additionally rewrites the minted bootstrap URLs for clients that reach
/// the gateway through a proxy or a LAN address (the token/channel query
/// parameters are appended to this base) and/or clamps the projected seed
/// transcript at open time (upstream ask 30 stopgap; see
/// `live_wiring::clamp_seed_messages_oldest_first`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum GatewayLiveOption {
    #[default]
    Disabled,
    Enabled {
        public_base_url: Option<String>,
        seed_max_chars: Option<usize>,
    },
}

#[cfg(feature = "experimental-gpt-live")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct GatewayExperimentalLiveOption {
    principal: String,
    realm: meerkat_core::RealmId,
    factory: meerkat::ExperimentalLiveFactoryIdentity,
    qualification: meerkat::ExperimentalLiveGate0QualificationVersion,
    binding: meerkat_core::AuthBindingRef,
    voice: String,
    instructions: Option<String>,
}

/// `runtime_options.workgraph` wire forms. Booleans keep the original
/// semantics (on with defaulted store placement / off). A string is an
/// explicit DIRECTORY for the durable store — `workgraph.sqlite3` is created
/// inside it. The explicit form exists for identity-first launches that
/// persist through an SDK-hosted continuity store (`has_continuity_store`
/// without `persistent_state`): those have no state dir to default to, so a
/// bare `true` silently rides a memory store.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum GatewayWorkgraphOption {
    #[default]
    Enabled,
    Disabled,
    DurableDir(std::path::PathBuf),
}

struct GatewayAgentMemoryOptions {
    config: meerkat_mobkit::AgentMemoryConfig,
    path: std::path::PathBuf,
    store: GatewayAgentMemoryStoreKind,
    /// §8.4 distiller block from `agent_memory.distiller`. Disabled by
    /// default (flipping it is a calibration decision, §11).
    distiller: meerkat_mobkit::memory::distiller::DistillerConfig,
    /// §8.5 steward block from `agent_memory.steward`. Disabled by
    /// default; enablement is the application's call (mechanism from
    /// MobKit, policy from the app).
    steward: meerkat_mobkit::memory::steward::StewardConfig,
}

/// Which bundled store backs agent memory. SQLite is the default now that
/// the P1 recall coordinator and injection ledger ride on it
/// (docs/design/agent-memory-architecture.md §15); a realm's existing
/// markdown files are auto-imported when that realm is first accessed.
///
/// Markdown is still a RECOGNIZED value - it parses, it censuses, and the
/// per-knob `requires store='sqlite'` refusals below still name it - but it
/// is no longer a live execution backend: selecting it now fails init with
/// [`AgentMemoryStoreMigration`]. The markdown READER stays fully intact for
/// the deliberate one-shot import (the SQLite store migrates a realm's
/// un-imported `.md` files when that realm's connection is first opened),
/// which is the supported path off markdown and the thing the migration
/// error points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum GatewayAgentMemoryStoreKind {
    Markdown,
    #[default]
    Sqlite,
}

/// A typed refusal for an agent-memory store kind that is no longer a live
/// execution backend. Typed rather than a bare string so the refusal has one
/// definition, one code, and one message that tests can pin - a migration
/// verdict, not an incidental open failure.
///
/// It rides the existing `STORAGE_RESOLUTION_CODE` because that is exactly
/// what it is from a client's point of view: this durable storage slot
/// cannot be resolved as configured. No new wire code is minted, so SDKs
/// that already branch on -32014 keep working unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentMemoryStoreMigration {
    /// `agent_memory.store = "markdown"` selected as the LIVE backend.
    MarkdownIsImportOnly,
}

/// Typed invalid-params refusal for public gateway activation of a parked
/// capability. Keeping this separate from generic parse strings prevents a
/// future composition rewrite from accidentally turning a compatibility key
/// back into an executable feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParkedGatewayCapability {
    Hygienist,
}

impl ParkedGatewayCapability {
    const fn code(self) -> i64 {
        -32602
    }

    fn message(self) -> String {
        match self {
            Self::Hygienist => "runtime_options.agent_memory.hygienist is PARKED and cannot be \
                 enabled through the public gateway: remove the key, set it to false, or use \
                 {enabled:false} while migrating existing configuration. The internal engine is \
                 retained for future validation, but this release has no supported provider \
                 proof for transcript curation."
                .to_string(),
        }
    }
}

impl AgentMemoryStoreMigration {
    const fn code(self) -> i64 {
        STORAGE_RESOLUTION_CODE
    }

    fn message(self) -> String {
        match self {
            Self::MarkdownIsImportOnly => "runtime_options.agent_memory.store='markdown' is \
                 retired as a live store and cannot back a running gateway: it has no manifest, \
                 no injection ledger, no taint firewall and no judgment plane, so a markdown \
                 deployment silently loses every §7-§10 guarantee the sqlite store provides. \
                 Migration is one step and lossless: set store='sqlite' (or drop the key - \
                 sqlite is the default) against the SAME agent-memory directory. \
                 The SQLite store imports each realm's un-imported .md files when that realm's \
                 connection is first opened, preserving memory ids, tags and timestamps, and \
                 renames each source file to .md.imported rather than deleting it."
                .to_string(),
        }
    }
}

/// §7.2: `operator_scope = "provisional"` composes operator-scope recall
/// only when an `OperatorResolver` is installed. The shipped SDK gateway
/// installs the console-principal resolver when provisional scope is enabled;
/// this predicate remains a fail-loud guard for any composition path that
/// omits it.
fn operator_scope_recall_inert(
    agent_memory: Option<&GatewayAgentMemoryOptions>,
    resolver_installed: bool,
) -> bool {
    !resolver_installed
        && agent_memory.is_some_and(|memory| {
            memory.config.operator_scope == meerkat_mobkit::AgentMemoryOperatorScope::Provisional
        })
}

impl Default for GatewayRuntimeOptions {
    fn default() -> Self {
        Self {
            runtime_options: RuntimeOptions::default(),
            identity_bootstrap_mode: None,
            max_sessions: 16,
            routing_routes: Vec::new(),
            gating: GatewayGatingConfig::default(),
            event_log: None,
            decisions: None,
            console_ui: ConsoleUiConfig::default(),
            console_require_app_auth: None,
            console_read_only: None,
            console_fetch_timeout_ms: None,
            access: None,
            demo_llm: false,
            member_comms_address: None,
            contacts: None,
            control_grants: ControlGrantTable::new(),
            agent_memory: None,
            workgraph: GatewayWorkgraphOption::Enabled,
            live: GatewayLiveOption::Disabled,
            #[cfg(feature = "experimental-gpt-live")]
            experimental_live: None,
            host_runnables: Vec::new(),
            runtime_store_ephemeral: false,
            mob_storage_ephemeral: false,
            declare_spec_update: None,
            composition_authority:
                meerkat_mobkit::mob_composition_manifest::CompositionAuthority::default(),
            compaction: None,
        }
    }
}

/// The `meerkat::Config` every gateway-built agent is constructed from.
///
/// This is where the gateway's session-compaction policy becomes real:
/// meerkat's `AgentFactory::build_agent` builds its `DefaultCompactor` from
/// `config.compaction`, so an un-declared policy here is what leaves the
/// gateway on meerkat's model-aware `context_window * 4 / 5` trigger.
fn gateway_agent_config(options: &GatewayRuntimeOptions) -> Config {
    let mut config = Config::default();
    if let Some(address) = options.member_comms_address.as_ref() {
        config.comms.mode = meerkat_core::CommsRuntimeMode::Tcp;
        config.comms.address = Some(address.clone());
    }
    if let Some(compaction) = options.compaction.as_ref() {
        // `parse_gateway_runtime_options` is the only producer of this slot
        // and validates before storing, so the error arm is unreachable from
        // a launched gateway. It is still handled rather than unwrapped: the
        // one rejected shape is a zero threshold, and refusing to install it
        // (falling back to the inherited policy) is strictly safer than the
        // compaction storm installing it would cause.
        if let Err(error) = meerkat_mobkit::apply_compaction_policy(&mut config, compaction) {
            tracing::error!(
                %error,
                "refusing an invalid runtime_options.compaction declaration; \
                 inheriting meerkat's default compaction policy instead"
            );
        }
    }
    config
}

#[derive(Default)]
struct InMemoryEventLogStore {
    events: std::sync::Mutex<Vec<PersistedEvent>>,
}

impl EventLogStore for InMemoryEventLogStore {
    fn append_batch(
        &self,
        events: Vec<PersistedEvent>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), EventLogError>> + Send + '_>> {
        Box::pin(async move {
            self.events.lock().unwrap().extend(events);
            Ok(())
        })
    }

    fn query(
        &self,
        query: EventQuery,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<PersistedEvent>, EventLogError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            let mut events = self.events.lock().unwrap().clone();
            events.retain(|event| {
                query
                    .since_ms
                    .is_none_or(|since| event.timestamp_ms >= since)
                    && query
                        .until_ms
                        .is_none_or(|until| event.timestamp_ms < until)
                    && query
                        .after_seq
                        .is_none_or(|after_seq| event.seq > after_seq)
                    && query
                        .member_id
                        .as_ref()
                        .is_none_or(|member_id| event.member_id.as_ref() == Some(member_id))
                    && (query.event_types.is_empty()
                        || query.event_types.iter().any(|event_type| {
                            matches!(
                                &event.event,
                                meerkat_mobkit::UnifiedEvent::Module(module)
                                    if &module.event_type == event_type
                            )
                        }))
            });
            events.sort_by_key(|event| event.seq);
            if let Some(limit) = query.limit {
                events.truncate(limit);
            }
            Ok(events)
        })
    }
}

fn minimal_decision_state() -> RuntimeDecisionState {
    RuntimeDecisionState {
        bigquery: BigQueryNaming {
            dataset: "default_dataset".to_string(),
            table: "default_table".to_string(),
        },
        modules: vec![],
        auth: AuthPolicy::default(),
        trusted_oidc: TrustedOidcRuntimeConfig {
            discovery_json: r#"{"issuer":"https://noop.example.com","authorization_endpoint":"https://noop.example.com/auth","token_endpoint":"https://noop.example.com/token","jwks_uri":"https://noop.example.com/.well-known/jwks.json","response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"]}"#.to_string(),
            jwks_json: r#"{"keys":[]}"#.to_string(),
            audience: "persistent-gateway".to_string(),
        },
        console: ConsolePolicy::default(),
        ops: RuntimeOpsPolicy::default(),
        release_metadata: ReleaseMetadata {
            targets: vec![
                "crates.io".to_string(),
                "npm".to_string(),
                "pypi".to_string(),
                "github-releases".to_string(),
            ],
            support_matrix: "lts".to_string(),
        },
    }
}

fn shell_module(id: &str, script: &str) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        restart_policy: RestartPolicy::Never,
    }
}

const MODULE_BOUNDARY_ENV_KEY: &str = "MOBKIT_MODULE_BOUNDARY";
const MODULE_BOUNDARY_MCP: &str = "mcp";

#[derive(Debug, Deserialize)]
struct GatewayModuleConfig {
    id: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "gateway_restart_policy_never")]
    restart_policy: RestartPolicy,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    boundary: Option<String>,
}

fn gateway_restart_policy_never() -> RestartPolicy {
    RestartPolicy::Never
}

impl GatewayModuleConfig {
    fn into_module_and_pre_spawn(self) -> (ModuleConfig, Option<PreSpawnData>) {
        let GatewayModuleConfig {
            id,
            command,
            args,
            restart_policy,
            mut env,
            boundary,
        } = self;
        if boundary
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(MODULE_BOUNDARY_MCP))
        {
            env.insert(
                MODULE_BOUNDARY_ENV_KEY.to_string(),
                MODULE_BOUNDARY_MCP.to_string(),
            );
        }
        let pre_spawn = if env.is_empty() {
            None
        } else {
            Some(PreSpawnData {
                module_id: id.clone(),
                env: env.into_iter().collect(),
            })
        };
        (
            ModuleConfig {
                id,
                command,
                args,
                restart_policy,
            },
            pre_spawn,
        )
    }
}

fn parse_gateway_modules(params: &Value) -> (Vec<ModuleConfig>, Vec<PreSpawnData>) {
    let gateway_modules: Vec<GatewayModuleConfig> = params
        .get("modules")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();

    let mut modules = Vec::with_capacity(gateway_modules.len());
    let mut pre_spawn = Vec::new();
    for gateway_module in gateway_modules {
        let (module, maybe_pre_spawn) = gateway_module.into_module_and_pre_spawn();
        modules.push(module);
        if let Some(pre_spawn_data) = maybe_pre_spawn {
            pre_spawn.push(pre_spawn_data);
        }
    }
    (modules, pre_spawn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use meerkat_mobkit::mob_handle_runtime::MobRuntimeError;
    use meerkat_mobkit::unified_runtime::types::IdentityAuthorityReleaseOutcome;
    use meerkat_mobkit::unified_runtime::types::RetiredSupervisorCleanupOutcome;
    use meerkat_mobkit::{RuntimeShutdownReport, ShutdownDrainReport};

    /// The compiled `action_risk_tiers` table must BIND a caller, not merely
    /// default for one. Before 0.8.19 the gateway filled the tier only when the
    /// caller omitted it, so any client could claim `r0` for an `r3` action and
    /// `evaluate_gating_action` would honour the claim - making the policy table
    /// advisory against exactly the caller it exists to constrain.
    ///
    /// Reverting the fix (filling only under `!params.contains_key("risk_tier")`)
    /// turns this red: the assertion below would see the caller's `r0` survive.
    #[test]
    fn configured_risk_tier_overrides_a_caller_supplied_tier() {
        let mut action_risk_tiers = HashMap::new();
        action_risk_tiers.insert("delete_household_data".to_string(), "r3".to_string());
        let gating = GatewayGatingConfig { action_risk_tiers };

        let claimed_low = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "mobkit/gating/evaluate",
            "params": {"action": "delete_household_data", "risk_tier": "r0"}
        })
        .to_string();

        let applied = apply_gateway_runtime_config_to_request(&claimed_low, &gating);
        let parsed: Value = serde_json::from_str(&applied).expect("rewritten request is json");
        assert_eq!(
            parsed["params"]["risk_tier"], "r3",
            "configured policy must win over a caller-supplied tier"
        );

        // The absent-tier path must keep working, and an action with no policy
        // entry must keep the caller's value rather than being silently dropped.
        let omitted = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "mobkit/gating/evaluate",
            "params": {"action": "delete_household_data"}
        })
        .to_string();
        let applied = apply_gateway_runtime_config_to_request(&omitted, &gating);
        let parsed: Value = serde_json::from_str(&applied).expect("rewritten request is json");
        assert_eq!(parsed["params"]["risk_tier"], "r3");

        let unknown_action = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "mobkit/gating/evaluate",
            "params": {"action": "not_in_policy", "risk_tier": "r1"}
        })
        .to_string();
        let applied = apply_gateway_runtime_config_to_request(&unknown_action, &gating);
        let parsed: Value = serde_json::from_str(&applied).expect("rewritten request is json");
        assert_eq!(
            parsed["params"]["risk_tier"], "r1",
            "an action absent from the policy table keeps the caller's tier"
        );
    }

    /// The default tracing filter (RUST_LOG unset) must surface this crate's
    /// own INFO lines — the 0.8.8 conversion-progress observability was
    /// invisible at the old blanket "warn" default — while keeping
    /// dependencies at WARN.
    #[test]
    fn default_tracing_filter_surfaces_own_info_keeps_deps_at_warn() {
        use tracing_subscriber::layer::SubscriberExt;
        let filter = tracing_subscriber::EnvFilter::try_new(
            meerkat_mobkit::gateway_composition::default_tracing_filter(GATEWAY_TRACING_TARGET),
        )
        .expect("default filter must parse");
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
                tracing::enabled!(target: "rpc_gateway", tracing::Level::INFO),
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
    }

    #[test]
    fn gateway_module_boundary_becomes_pre_spawn_data() {
        let params = json!({
            "modules": [{
                "id": "router",
                "command": "python3",
                "args": ["router.py"],
                "restart_policy": "on_failure",
                "boundary": "mcp",
                "env": {
                    "ROUTER_FIXTURE": "homecore"
                }
            }]
        });

        let (modules, pre_spawn) = parse_gateway_modules(&params);

        assert_eq!(
            modules,
            vec![ModuleConfig {
                id: "router".to_string(),
                command: "python3".to_string(),
                args: vec!["router.py".to_string()],
                restart_policy: RestartPolicy::OnFailure,
            }]
        );
        assert_eq!(
            pre_spawn,
            vec![PreSpawnData {
                module_id: "router".to_string(),
                env: vec![
                    ("MOBKIT_MODULE_BOUNDARY".to_string(), "mcp".to_string()),
                    ("ROUTER_FIXTURE".to_string(), "homecore".to_string()),
                ],
            }]
        );
    }

    #[test]
    fn gateway_module_without_env_does_not_create_pre_spawn_data() {
        let params = json!({
            "modules": [{
                "id": "delivery",
                "command": "python3"
            }]
        });

        let (modules, pre_spawn) = parse_gateway_modules(&params);

        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].id, "delivery");
        assert_eq!(modules[0].args, Vec::<String>::new());
        assert_eq!(modules[0].restart_policy, RestartPolicy::Never);
        assert!(pre_spawn.is_empty());
    }

    #[test]
    fn gateway_runtime_options_parse_host_runnables() {
        let params = json!({
            "runtime_options": {
                "host_runnables": ["digest", "backup.rotate"]
            }
        });
        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");
        assert_eq!(
            options
                .host_runnables
                .iter()
                .map(meerkat::HostRunnableName::as_str)
                .collect::<Vec<_>>(),
            vec!["digest", "backup.rotate"]
        );
    }

    #[test]
    fn gateway_runtime_options_reject_invalid_host_runnables() {
        for (runtime_options, needle) in [
            (json!({"host_runnables": "digest"}), "must be an array"),
            (json!({"host_runnables": [7]}), "must be strings"),
            (json!({"host_runnables": ["  "]}), "is invalid"),
            (
                json!({"host_runnables": ["digest", "digest"]}),
                "duplicated",
            ),
            (
                json!({"host_runnables": [
                    meerkat_mobkit::schedule_wiring::STEWARD_DREAM_RUNNABLE
                ]}),
                "reserved",
            ),
        ] {
            let params = json!({ "runtime_options": runtime_options });
            let err = match parse_gateway_runtime_options(&params, None) {
                Err(err) => err,
                Ok(_) => panic!("expected rejection for {params}"),
            };
            assert!(err.contains(needle), "{err} should mention '{needle}'");
        }
    }

    /// Every documented runtime_options door must survive the UNKNOWN-FIELD
    /// ALLOWLIST, not merely have a handler.
    ///
    /// Both of these shipped in 0.8.24 with a working handler, a documented
    /// struct field, and NO allowlist entry, so the unknown-field rejection ran
    /// first and the handlers were dead code in the published binary. HomeCore
    /// found it by sending the documented line to the released artifact and
    /// getting `unsupported runtime_options fields: mob_composition` - one step
    /// EARLIER than the refusal the flag existed to avoid.
    ///
    /// The persistence suite stayed green throughout because it constructs the
    /// authority through the builder, never through this parse path. A test that
    /// does not cross the door cannot prove the door opens.
    ///
    /// Asserts the PARSED VALUE, not the absence of an error: an allowlist entry
    /// with a broken handler would pass an is_ok() check.
    #[test]
    fn documented_runtime_options_doors_survive_the_unknown_field_allowlist() {
        let candidate = parse_gateway_runtime_options(
            &json!({ "runtime_options": { "mob_composition": { "authority": "candidate" } } }),
            None,
        )
        .expect("mob_composition must pass the allowlist");
        assert_eq!(
            candidate.composition_authority,
            meerkat_mobkit::mob_composition_manifest::CompositionAuthority::NonAuthoritative,
            "the candidate declaration must reach the parsed options, not just avoid rejection"
        );

        let authoritative = parse_gateway_runtime_options(
            &json!({ "runtime_options": { "mob_composition": { "authority": "authoritative" } } }),
            None,
        )
        .expect("explicit authoritative must parse");
        assert_eq!(
            authoritative.composition_authority,
            meerkat_mobkit::mob_composition_manifest::CompositionAuthority::Authoritative
        );

        let declared = parse_gateway_runtime_options(
            &json!({ "runtime_options": { "declare_spec_update": { "expected_revision": 7 } } }),
            None,
        )
        .expect("declare_spec_update must pass the allowlist");
        assert_eq!(
            declared.declare_spec_update,
            Some(7),
            "the declared revision must reach the parsed options"
        );

        // Default stays authoritative and undeclared, so a launch that says
        // nothing keeps the protection rather than silently losing it.
        let quiet = parse_gateway_runtime_options(&json!({ "runtime_options": {} }), None)
            .expect("empty options");
        assert_eq!(
            quiet.composition_authority,
            meerkat_mobkit::mob_composition_manifest::CompositionAuthority::Authoritative
        );
        assert_eq!(quiet.declare_spec_update, None);
    }

    #[test]
    fn gateway_runtime_options_control_grants_are_closed_and_scoped() {
        let defaulted = parse_gateway_runtime_options(&json!({ "runtime_options": {} }), None)
            .expect("runtime options");
        assert!(
            defaulted.control_grants.is_empty(),
            "an omitted grant declaration must be deny-all"
        );

        let caller = meerkat_mobkit::GatewayPeerKeys::ephemeral();
        let params = json!({
            "runtime_options": {
                "control_grants_toml": format!(
                    "[control_grants.desktop]\npubkey = \"{}\"\nverbs = [\"lookup_member\", \"inject\"]\nmembers = [\"worker-1\"]\n",
                    caller.pubkey_b64()
                )
            }
        });
        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");
        let grant = options
            .control_grants
            .get(&caller.pubkey_bytes())
            .expect("desktop grant");
        assert!(
            grant
                .verbs()
                .contains(&meerkat_mobkit::runtime::cross_mob_control::ControlVerb::LookupMember)
        );
        assert!(
            grant
                .verbs()
                .contains(&meerkat_mobkit::runtime::cross_mob_control::ControlVerb::Inject)
        );
        assert_eq!(
            grant.members(),
            &meerkat_mobkit::runtime::cross_mob_control::ControlMemberScope::members(["worker-1"])
        );

        let missing_section = json!({
            "runtime_options": {
                "control_grants_toml": "[mobs]\nremote = \"inproc\"\n"
            }
        });
        let error = parse_gateway_runtime_options(&missing_section, None)
            .err()
            .expect("an explicit grant document without the section must fail");
        assert!(error.contains("must contain a [control_grants] section"));
    }

    /// The runtime_options allowlist stays closed: unknown keys are a hard
    /// init error naming the offender (M4 added `runtime_store`; anything
    /// else still rejects).
    #[test]
    fn gateway_runtime_options_reject_unknown_fields() {
        let params = json!({ "runtime_options": { "blob_storage": {} } });
        let err = match parse_gateway_runtime_options(&params, None) {
            Err(err) => err,
            Ok(_) => panic!("unknown runtime_options keys must be rejected"),
        };
        assert!(
            err.contains("unsupported runtime_options fields: blob_storage"),
            "{err}"
        );
    }

    /// `runtime_options.compaction` is the gateway's host-level compaction
    /// policy. Declaring a threshold must reach the `meerkat::Config` every
    /// agent is built from AND pin it — an un-pinned threshold is silently
    /// rescaled by meerkat to `context_window * 4 / 5` (840_000 tokens on a
    /// million-token model), which is the "compaction never fires" production
    /// failure this knob exists to prevent.
    #[test]
    fn gateway_runtime_options_parse_compaction_policy() {
        let defaulted = parse_gateway_runtime_options(&json!({ "runtime_options": {} }), None)
            .expect("runtime options");
        assert!(
            defaulted.compaction.is_none(),
            "an undeclared policy must inherit, not invent a threshold"
        );
        assert!(
            !gateway_agent_config(&defaulted)
                .compaction
                .auto_compact_threshold_explicit,
            "the inheriting form must stay un-pinned"
        );

        let params = json!({
            "runtime_options": {
                "compaction": {
                    "auto_compact_threshold": 120_000,
                    "recent_turn_budget": 6,
                }
            }
        });
        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");
        let config = gateway_agent_config(&options);
        assert_eq!(config.compaction.auto_compact_threshold, 120_000);
        assert!(
            config.compaction.auto_compact_threshold_explicit,
            "a declared gateway threshold must be pinned against model-aware scaling"
        );
        assert_eq!(config.compaction.recent_turn_budget, 6);
    }

    /// The compaction declaration is key-closed and validated at ingress: a
    /// typo or a zero threshold is a startup error, never a dead knob.
    #[test]
    fn gateway_runtime_options_reject_invalid_compaction_policy() {
        for (compaction, needle) in [
            (json!({"auto_compact_treshold": 100}), "unsupported fields"),
            (json!({"auto_compact_threshold": 0}), "greater than 0"),
            (json!({"auto_compact_threshold": "lots"}), "is invalid"),
            (json!(120_000), "must be a JSON object"),
        ] {
            let params = json!({ "runtime_options": { "compaction": compaction } });
            let err = match parse_gateway_runtime_options(&params, None) {
                Err(err) => err,
                Ok(_) => panic!("expected rejection for {params}"),
            };
            assert!(
                err.contains("runtime_options.compaction"),
                "{err} should name the offending path"
            );
            assert!(err.contains(needle), "{err} should mention '{needle}'");
        }
    }

    /// M4: `runtime_store` accepts only the explicit in-memory declaration;
    /// persistent SQLite is the default and any other spelling rejects.
    #[test]
    fn gateway_runtime_options_parse_runtime_store_declaration() {
        let params = json!({ "runtime_options": { "runtime_store": { "storage": "memory" } } });
        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");
        assert!(options.runtime_store_ephemeral);

        let defaulted = parse_gateway_runtime_options(&json!({ "runtime_options": {} }), None)
            .expect("runtime options");
        assert!(!defaulted.runtime_store_ephemeral);

        for (value, needle) in [
            (json!({"storage": "sqlite"}), "unsupported"),
            (json!({"storage": 7}), "must be 'memory'"),
            (json!("memory"), "must be a JSON object"),
        ] {
            let params = json!({ "runtime_options": { "runtime_store": value } });
            let err = match parse_gateway_runtime_options(&params, None) {
                Err(err) => err,
                Ok(_) => panic!("expected rejection for {params}"),
            };
            assert!(err.contains(needle), "{err} should mention '{needle}'");
        }
    }

    /// M4: `event_log.storage` gains the explicit 'null' declaration; the
    /// pre-existing 'memory' wire form keeps working; everything else
    /// rejects.
    #[test]
    fn gateway_runtime_options_event_log_storage_declarations() {
        for storage in ["memory", "in_memory", "null"] {
            let params = json!({ "runtime_options": { "event_log": { "storage": storage } } });
            let options = parse_gateway_runtime_options(&params, None)
                .unwrap_or_else(|err| panic!("storage '{storage}' must parse: {err}"));
            assert!(options.event_log.is_some());
        }
        let err = match parse_gateway_runtime_options(
            &json!({ "runtime_options": { "event_log": { "storage": "sqlite" } } }),
            None,
        ) {
            Err(err) => err,
            Ok(_) => panic!("undeclared event_log storage must be rejected"),
        };
        assert!(
            err.contains("unsupported runtime_options.event_log.storage"),
            "{err}"
        );
    }

    #[test]
    fn gateway_runtime_options_parse_live_seed_max_chars() {
        let params = json!({
            "runtime_options": {
                "live": { "seed_max_chars": 200000 }
            }
        });
        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");
        assert_eq!(
            options.live,
            GatewayLiveOption::Enabled {
                public_base_url: None,
                seed_max_chars: Some(200_000),
            }
        );

        let err = match parse_gateway_runtime_options(
            &json!({"runtime_options": {"live": {"seed_max_chars": 0}}}),
            None,
        ) {
            Err(err) => err,
            Ok(_) => panic!("zero seed_max_chars must be rejected"),
        };
        assert!(err.contains("seed_max_chars"), "{err}");
    }

    fn test_callback_bridge() -> StdioCallbackBridge {
        let (stdout_tx, _stdout_rx) = mpsc::channel::<GatewayStdoutLine>(4);
        StdioCallbackBridge::new(stdout_tx)
    }

    #[cfg(feature = "experimental-gpt-live")]
    fn stale_public_observation_binding() -> (
        Arc<meerkat_runtime::MeerkatMachine>,
        meerkat_live::ProviderWebrtcBinding,
    ) {
        (
            Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()),
            meerkat_live::ProviderWebrtcBinding::new(
                meerkat_live::LiveChannelId::new("stale-output-channel"),
                meerkat_core::SessionId::new(),
                meerkat_live::LiveRuntimeBindingGeneration::new(1),
                meerkat_live::LiveRuntimeBindingFence::new(1),
            ),
        )
    }

    #[cfg(feature = "experimental-gpt-live")]
    struct PublicationTestSideband;

    #[cfg(feature = "experimental-gpt-live")]
    #[async_trait]
    impl meerkat_live::ProviderWebrtcSidebandSession for PublicationTestSideband {
        async fn send_command(
            &self,
            _command: meerkat_live::LiveSidebandCommand,
        ) -> Result<
            meerkat_live::LiveSidebandCommandDelivery,
            meerkat_live::ProviderWebrtcBrokerError,
        > {
            Err(meerkat_live::ProviderWebrtcBrokerError::Unavailable)
        }

        async fn next_observation(
            &self,
        ) -> Result<
            Option<meerkat_live::LiveSidebandObservation>,
            meerkat_live::ProviderWebrtcBrokerError,
        > {
            Ok(None)
        }

        async fn close(&self) -> Result<(), meerkat_live::ProviderWebrtcBrokerError> {
            Ok(())
        }
    }

    #[cfg(feature = "experimental-gpt-live")]
    struct PublicationTestAnswerTransport;

    #[cfg(feature = "experimental-gpt-live")]
    #[async_trait]
    impl meerkat_live::LiveWebrtcAnswerTransport for PublicationTestAnswerTransport {
        async fn answer_admitted_offer(
            &self,
            offer: meerkat_live::LiveWebrtcAdmittedOffer,
        ) -> Result<meerkat_live::LiveWebrtcAnswerAccepted, meerkat_live::LiveWebrtcError> {
            let answer = offer.into_provider_offer()?.into_seeded_answer(
                "publication-answer".to_string(),
                Arc::new(PublicationTestSideband),
                0,
            );
            let (answer_sdp, _sideband, bound_ready) = answer.into_parts();
            Ok(meerkat_live::LiveWebrtcAnswerAccepted {
                answer_sdp,
                answer_observation_sequence: 1,
                bound_ready: Some(bound_ready),
            })
        }

        async fn reject_answer(
            &self,
            _binding: &meerkat_live::LiveWebrtcBindingRequest,
            _answer_observation_sequence: u64,
        ) -> Result<(), meerkat_live::LiveWebrtcError> {
            Ok(())
        }

        async fn accept_answer(
            &self,
            _binding: &meerkat_live::LiveWebrtcBindingRequest,
            _answer_observation_sequence: u64,
        ) {
        }

        async fn wait_for_construction_cleanup(
            &self,
            _binding: &meerkat_live::LiveWebrtcBindingRequest,
        ) -> Result<(), meerkat_live::LiveWebrtcError> {
            Ok(())
        }

        async fn close_binding(
            &self,
            _binding: &meerkat_live::LiveWebrtcBindingRequest,
        ) -> Result<(), meerkat_live::LiveWebrtcError> {
            Ok(())
        }
    }

    #[cfg(feature = "experimental-gpt-live")]
    struct PublicationTestBoundReadyBinder;

    #[cfg(feature = "experimental-gpt-live")]
    struct PublicationTestBoundReadyCustody {
        authority:
            Option<meerkat_runtime::meerkat_machine::LiveWebrtcAnswerExecutionBindingAuthority>,
    }

    #[cfg(feature = "experimental-gpt-live")]
    #[async_trait]
    impl meerkat::surface::LiveWebrtcBoundReadyCustody for PublicationTestBoundReadyCustody {
        async fn commit(mut self: Box<Self>) {
            if let Some(authority) = self.authority.take() {
                let _ = authority.commit();
            }
        }

        async fn rollback(mut self: Box<Self>) -> Result<(), String> {
            let _rollback = self
                .authority
                .take()
                .map(|authority| authority.into_rollback());
            Ok(())
        }
    }

    #[cfg(feature = "experimental-gpt-live")]
    #[async_trait]
    impl meerkat::surface::LiveWebrtcBoundReadyBinder for PublicationTestBoundReadyBinder {
        async fn bind_answer_ready(
            &self,
            runtime: Arc<meerkat_runtime::MeerkatMachine>,
            binding: &meerkat_live::LiveWebrtcBindingRequest,
            receipt: meerkat_live::ProviderWebrtcBoundReadyReceipt,
            answer_observation_sequence: u64,
        ) -> Result<
            Box<dyn meerkat::surface::LiveWebrtcBoundReadyCustody>,
            meerkat::surface::LiveWebrtcBoundReadyBindFailure,
        > {
            let runtime_binding = binding
                .runtime_binding
                .expect("test answer carries runtime binding");
            let provider_binding = meerkat_live::ProviderWebrtcBinding::new(
                binding.channel_id.clone(),
                binding.session_id.clone(),
                meerkat_live::LiveRuntimeBindingGeneration::new(runtime_binding.generation),
                meerkat_live::LiveRuntimeBindingFence::new(runtime_binding.fence),
            );
            let authority = runtime
                .accept_live_webrtc_answer_and_bind_execution(
                    &provider_binding,
                    &receipt,
                    answer_observation_sequence,
                )
                .await
                .expect("test answer binds through generated authority");
            Ok(Box::new(PublicationTestBoundReadyCustody {
                authority: Some(authority),
            }))
        }
    }

    #[cfg(feature = "experimental-gpt-live")]
    async fn current_public_observation_binding() -> (
        Arc<meerkat_runtime::MeerkatMachine>,
        meerkat_live::ProviderWebrtcBinding,
    ) {
        let machine = Arc::new(meerkat_runtime::MeerkatMachine::ephemeral());
        let session_id = meerkat_core::SessionId::new();
        machine
            .prepare_bindings(session_id.clone())
            .await
            .expect("prepare exact live binding");
        let channel_id = meerkat_live::LiveChannelId::new("current-output-channel");
        let identity = meerkat_core::SessionLlmIdentity {
            model: "gpt-realtime-2".to_string(),
            provider: meerkat_core::Provider::OpenAI,
            self_hosted_server_id: None,
            provider_params: None,
            auth_binding: None,
        };
        machine
            .resolve_live_open_admission(&session_id, &channel_id, &identity)
            .await
            .expect("admit exact live binding");
        machine
            .stage_experimental_live_execution(&session_id, &channel_id, 0)
            .await
            .expect("stage exact live binding");
        let token = "publication-test-token";
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_millis() as u64;
        machine
            .record_live_webrtc_token_issued(&session_id, &channel_id, token, now, 60_000)
            .await
            .expect("issue exact signaling token");
        let answer = meerkat::surface::coordinate_live_webrtc_answer(
            Arc::clone(&machine),
            Arc::new(PublicationTestAnswerTransport),
            Some(Arc::new(PublicationTestBoundReadyBinder)),
            channel_id.clone(),
            token.to_string(),
            "publication-offer".to_string(),
        )
        .await
        .expect("bind exact answer through production coordinator");
        answer
            .delivery_custody
            .delivered()
            .await
            .expect("commit exact answer publication");
        let runtime_binding = machine
            .live_delegation_runtime_binding(&session_id, &channel_id)
            .await
            .expect("project exact live runtime binding");
        let provider_binding = meerkat_live::ProviderWebrtcBinding::new(
            channel_id,
            session_id,
            meerkat_live::LiveRuntimeBindingGeneration::new(runtime_binding.generation()),
            meerkat_live::LiveRuntimeBindingFence::new(runtime_binding.fence_token()),
        );
        (machine, provider_binding)
    }

    #[cfg(feature = "experimental-gpt-live")]
    #[tokio::test]
    async fn live_public_observation_ack_waits_for_outer_writer_settlement() {
        let (machine, binding) = stale_public_observation_binding();
        let (_line, delivered) = GatewayStdoutLine::public_observation(
            Arc::clone(&machine),
            binding.clone(),
            "{\"event\":true}".to_string(),
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), delivered)
                .await
                .is_err(),
            "enqueue alone must not attest outer publication"
        );

        let (mut line, delivered) =
            GatewayStdoutLine::public_observation(machine, binding, "{\"event\":true}".to_string());
        line.settle_delivery(true).await;
        assert!(delivered.await.expect("writer settlement"));
    }

    #[cfg(feature = "experimental-gpt-live")]
    #[tokio::test]
    async fn dropped_live_public_observation_rejects_delivery() {
        let (machine, binding) = stale_public_observation_binding();
        let (line, delivered) =
            GatewayStdoutLine::public_observation(machine, binding, "{\"event\":true}".to_string());
        drop(line);
        assert!(!delivered.await.expect("drop settlement"));
    }

    #[cfg(feature = "experimental-gpt-live")]
    #[tokio::test]
    async fn queued_stale_live_public_observation_is_rejected_before_write() {
        let (machine, binding) = stale_public_observation_binding();
        let (line, _delivered) =
            GatewayStdoutLine::public_observation(machine, binding, "{\"event\":true}".to_string());
        assert!(
            line.acquire_public_observation_custody().await.is_err(),
            "a queued stale generation/fence must be fenced before stdout"
        );
    }

    #[cfg(feature = "experimental-gpt-live")]
    #[tokio::test]
    async fn live_public_observation_requires_writer_and_sdk_queue_ack() {
        let (machine, binding) = current_public_observation_binding().await;
        let (stdout_tx, mut stdout_rx) = mpsc::channel::<GatewayStdoutLine>(1);
        let bridge = StdioCallbackBridge::new(stdout_tx);
        let publish = tokio::spawn({
            let bridge = bridge.clone();
            async move {
                bridge
                    .call_live_public_observation(
                        machine,
                        binding,
                        json!({
                            "channel_id": "current-output-channel",
                            "output_id": "opaque-output-1",
                            "content_index": 0,
                        }),
                    )
                    .await
            }
        });

        let mut line = stdout_rx.recv().await.expect("queued callback request");
        let request: Value = serde_json::from_str(&line).expect("callback request JSON");
        assert_eq!(
            request.get("method"),
            Some(&json!("mobkit/live/assistant_output_available"))
        );
        let custody = line
            .acquire_public_observation_custody()
            .await
            .expect("current binding")
            .expect("live line requires custody");
        line.settle_delivery(true).await;
        bridge
            .route_callback_response(json!({
                "jsonrpc": "2.0",
                "id": request.get("id").expect("callback id"),
                "result": {"accepted": true},
            }))
            .await;
        drop(custody);

        assert_eq!(
            publish
                .await
                .expect("publisher task")
                .expect("accepted callback"),
            json!({"accepted": true})
        );
    }

    /// Fix 3: `register_tool(..., input_schema=...)` schemas cross the
    /// `callback/build_agent` wire and land on the dispatcher's `ToolDef`s
    /// (which is what `live_visible_tool_defs` and normal turns both read).
    #[test]
    fn callback_tool_dispatcher_defs_carry_wire_schemas() {
        let schema = json!({
            "type": "object",
            "properties": { "city": { "type": "string" } },
            "required": ["city"]
        });
        let specs = vec![
            CallbackToolSpec::parse(&json!("legacy_name")).expect("legacy string spec"),
            CallbackToolSpec::parse(&json!({
                "name": "weather",
                "description": "Look up the weather",
                "input_schema": schema,
                "execution": {
                    "mode": "detached",
                    "runner": {"name": "weather.scan", "version": "1"},
                    "restart_class": "non_resumable",
                    "idempotency_scope": "interaction_and_arguments",
                    "submission_timeout_ms": 30000,
                    "credential_scopes": ["weather.read"]
                }
            }))
            .expect("object spec"),
        ];
        let dispatcher =
            CallbackToolDispatcher::new(test_callback_bridge(), "build-1".to_string(), specs, None);
        let defs = AgentToolDispatcher::tools(&dispatcher);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].name.as_ref(), "legacy_name");
        assert_eq!(defs[0].description, "Python callback tool");
        assert_eq!(defs[0].input_schema, json!({"type": "object"}));
        assert_eq!(defs[1].name.as_ref(), "weather");
        assert_eq!(defs[1].description, "Look up the weather");
        assert_eq!(defs[1].input_schema, schema);
        let catalog = AgentToolDispatcher::tool_catalog(&dispatcher);
        assert_eq!(
            catalog[1].execution.default_mode(),
            meerkat_core::ToolExecutionMode::Detached
        );
        let detached = catalog[1]
            .execution
            .detached_policy()
            .expect("detached policy");
        assert_eq!(detached.runner().name(), "weather.scan");
        assert_eq!(detached.runner().version(), "1");
        assert_eq!(
            detached.restart_class(),
            meerkat_core::RestartClass::NonResumable
        );
        assert_eq!(
            detached.idempotency_scope(),
            meerkat_core::IdempotencyScope::InteractionAndArguments
        );
        assert_eq!(
            detached.credential_scopes(),
            &std::collections::BTreeSet::from(["weather.read".to_string()])
        );
    }

    #[test]
    fn callback_tool_spec_rejects_malformed_wire_entries() {
        for value in [
            json!(7),
            json!({"description": "no name"}),
            json!({"name": ""}),
            json!({"name": "x", "input_schema": "not-an-object"}),
            json!({"name": "x", "description": 3}),
            json!({"name": "x", "execution": {"mode": "detached"}}),
        ] {
            assert!(
                CallbackToolSpec::parse(&value).is_err(),
                "expected rejection for {value}"
            );
        }
    }

    #[test]
    fn callback_event_delivery_builds_stable_runtime_owned_ingress() {
        let job_id = meerkat::JobId::new("019f74fb-1907-7b21-932d-ab22c4d1f500").expect("job id");
        let session_id = meerkat_core::SessionId::parse("019f74fb-1907-7b21-932d-ab22c4d1f501")
            .expect("session id");
        let subscription = meerkat::JobSubscription::new(
            meerkat::JobSubscriptionId::new("review-agent").expect("subscription"),
            session_id,
            meerkat::JobDeliveryKind::Event {
                handling_mode: meerkat_core::HandlingMode::Queue,
            },
        );
        let lineage =
            meerkat::InteractionLineageId::from_string("019f74fb-1907-7b21-932d-ab22c4d1f502")
                .expect("lineage");
        let content = meerkat::JobDeliveryContent::Terminal(meerkat::JobTerminalResult::WorkerLost);

        let input = callback_job_event_input(
            &job_id,
            7,
            &subscription,
            &lineage,
            meerkat_core::HandlingMode::Queue,
            &content,
        );
        let meerkat_runtime::Input::ExternalEvent(event) = input else {
            panic!("job event delivery must use canonical external-event ingress");
        };
        assert_eq!(event.event_type, "job.terminal");
        assert_eq!(event.handling_mode, meerkat_core::HandlingMode::Queue);
        assert_eq!(
            event
                .header
                .idempotency_key
                .as_ref()
                .map(ToString::to_string),
            Some(format!("job:{job_id}:7:review-agent"))
        );
        assert_eq!(
            event.payload,
            json!({
                "job_id": job_id.to_string(),
                "delivery_sequence": 7,
                "content": {
                    "kind": "terminal",
                    "result": "WorkerLost"
                }
            })
        );
        assert_eq!(
            event.header.correlation_id.map(|id| id.to_string()),
            Some(lineage.as_str().to_string())
        );
    }

    #[test]
    fn callback_runner_ownership_requires_exact_tool_runner_and_version() {
        let binary: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let blobs: Arc<dyn meerkat_core::BlobStore> = Arc::new(Base64BlobStoreAdapter::new(binary));
        let runtime = DetachedCallbackJobRuntime::new(
            meerkat_mobkit::storage_provider::MEERKAT_LEVEL_REALM_ID,
            Arc::new(meerkat::MemoryDetachedJobStore::new()),
            blobs,
        );
        let dispatcher = CallbackToolDispatcher::new(
            test_callback_bridge(),
            "build-1".to_string(),
            vec![
                CallbackToolSpec::parse(&json!({
                    "name": "security_scan",
                    "execution": {
                        "mode": "detached",
                        "runner": {"name": "homecore.security_scan", "version": "1"},
                        "restart_class": "adoptable",
                        "idempotency_scope": "interaction_and_arguments",
                        "submission_timeout_ms": 30000
                    }
                }))
                .expect("tool spec"),
            ],
            Some(runtime.clone()),
        );
        assert!(
            dispatcher.reconcile_registered_catalog,
            "the first exact callback owner must trigger recovery"
        );
        let duplicate = CallbackToolDispatcher::new(
            test_callback_bridge(),
            "build-2".to_string(),
            vec![
                CallbackToolSpec::parse(&json!({
                    "name": "security_scan",
                    "execution": {
                        "mode": "detached",
                        "runner": {"name": "homecore.security_scan", "version": "1"},
                        "restart_class": "adoptable",
                        "idempotency_scope": "interaction_and_arguments",
                        "submission_timeout_ms": 30000
                    }
                }))
                .expect("tool spec"),
            ],
            Some(runtime.clone()),
        );
        assert!(
            !duplicate.reconcile_registered_catalog,
            "rebuilding an already-known exact owner must not start a duplicate recovery census"
        );
        let later_runner = CallbackToolDispatcher::new(
            test_callback_bridge(),
            "build-3".to_string(),
            vec![
                CallbackToolSpec::parse(&json!({
                    "name": "report_export",
                    "execution": {
                        "mode": "detached",
                        "runner": {"name": "homecore.report_export", "version": "1"},
                        "restart_class": "adoptable",
                        "idempotency_scope": "interaction_and_arguments",
                        "submission_timeout_ms": 30000
                    }
                }))
                .expect("tool spec"),
            ],
            Some(runtime.clone()),
        );
        assert!(
            later_runner.reconcile_registered_catalog,
            "a later exact owner must get its own recovery census"
        );
        let make_spec = |tool: &str, runner: &str, version: &str| {
            meerkat::JobSpec::new(
                meerkat_mobkit::storage_provider::MEERKAT_LEVEL_REALM_ID,
                meerkat_core::SessionId::parse("019f74fb-1907-7b21-932d-ab22c4d1f503")
                    .expect("session"),
                meerkat::ExecutionIntentId::from_string(format!("intent:{tool}")).expect("intent"),
                meerkat::InteractionLineageId::from_string(format!("lineage:{tool}"))
                    .expect("lineage"),
                meerkat::ToolIdentity::new(tool, version).expect("tool"),
                meerkat::RunnerIdentity::new(runner, version).expect("runner"),
                meerkat::RestartClass::Adoptable,
                meerkat::CanonicalArgumentsHash::new(format!("sha256:{}", "a".repeat(64)))
                    .expect("hash"),
                meerkat::JobSubmissionKey::new(format!("submission:{tool}:{runner}:{version}"))
                    .expect("submission"),
            )
        };
        assert!(runtime.owns_callback_job(&make_spec(
            "security_scan",
            "homecore.security_scan",
            "1"
        )));
        assert!(!runtime.owns_callback_job(&make_spec(
            "other_tool",
            "homecore.security_scan",
            "1"
        )));
        assert!(!runtime.owns_callback_job(&make_spec(
            "security_scan",
            "homecore.security_scan",
            "2"
        )));
    }

    #[tokio::test]
    async fn callback_reconcile_rehydrates_exact_committed_authority_without_advancing_fence() {
        let store = Arc::new(meerkat::MemoryDetachedJobStore::new());
        let service = meerkat::DetachedJobService::new(store.clone());
        let session_id = meerkat_core::SessionId::parse("019f74fb-1907-7b21-932d-ab22c4d1f532")
            .expect("session id");
        let spec = meerkat::JobSpec::new(
            meerkat_mobkit::storage_provider::MEERKAT_LEVEL_REALM_ID,
            session_id,
            meerkat::ExecutionIntentId::from_string("intent:reconcile").expect("intent"),
            meerkat::InteractionLineageId::from_string("lineage:reconcile").expect("lineage"),
            meerkat::ToolIdentity::new("security_scan", "1").expect("tool"),
            meerkat::RunnerIdentity::new("homecore.security_scan", "1").expect("runner"),
            meerkat::RestartClass::Adoptable,
            meerkat::CanonicalArgumentsHash::new(format!("sha256:{}", "a".repeat(64)))
                .expect("arguments hash"),
            meerkat::JobSubmissionKey::new("callback:reconcile").expect("submission key"),
        );
        let receipt = service.submit(spec).await.expect("submit");
        let claim = service
            .claim_attempt(
                &receipt.job_id,
                meerkat::AttemptClaim::new(
                    meerkat::WorkerId::new("worker-1").expect("worker"),
                    100,
                    u64::MAX,
                    meerkat::RunnerHandleRef::new("external:scan-1").expect("handle"),
                ),
            )
            .await
            .expect("claim");

        let (stdout_tx, mut stdout_rx) = mpsc::channel::<GatewayStdoutLine>(4);
        let bridge = StdioCallbackBridge::new(stdout_tx);
        let binary: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let blobs: Arc<dyn meerkat_core::BlobStore> = Arc::new(Base64BlobStoreAdapter::new(binary));
        let dispatcher = CallbackToolDispatcher::new(
            bridge.clone(),
            "build-1".to_string(),
            vec![
                CallbackToolSpec::parse(&json!({
                    "name": "security_scan",
                    "execution": {
                        "mode": "detached",
                        "runner": {"name": "homecore.security_scan", "version": "1"},
                        "restart_class": "adoptable",
                        "idempotency_scope": "interaction_and_arguments",
                        "submission_timeout_ms": 30000
                    }
                }))
                .expect("tool spec"),
            ],
            Some(DetachedCallbackJobRuntime::new(
                meerkat_mobkit::storage_provider::MEERKAT_LEVEL_REALM_ID,
                store.clone(),
                blobs,
            )),
        );
        let reconcile = tokio::spawn(async move { dispatcher.reconcile_detached_jobs().await });
        let request_line = stdout_rx.recv().await.expect("reconcile callback");
        let request: Value = serde_json::from_str(&request_line).expect("callback json");
        assert_eq!(
            request.pointer("/params/attempts/0/authority/fence"),
            Some(&json!(claim.fence.get()))
        );
        assert_eq!(
            request.pointer("/params/attempts/0/authority/attempt_id"),
            Some(&json!(claim.attempt_id.to_string()))
        );
        assert_eq!(
            request.pointer("/params/attempts/0/runner_handle"),
            Some(&json!("external:scan-1"))
        );
        bridge
            .route_callback_response(json!({
                "jsonrpc": "2.0",
                "id": request["id"].clone(),
                "result": {
                    "live_attempts": [{
                        "job_id": receipt.job_id.to_string(),
                        "attempt_id": claim.attempt_id.to_string(),
                        "fence": claim.fence.get()
                    }]
                }
            }))
            .await;
        reconcile
            .await
            .expect("reconcile task")
            .expect("reconcile succeeds");

        let reopened = meerkat::DetachedJobStore::get(&*store, &receipt.job_id)
            .await
            .expect("read")
            .expect("job");
        assert_eq!(reopened.machine_state.attempt_count, 1);
        assert_eq!(reopened.machine_state.current_fence, claim.fence.get());
        assert_eq!(
            reopened.machine_state.current_attempt_id.as_deref(),
            Some(claim.attempt_id.as_str())
        );
    }

    #[test]
    fn gateway_runtime_options_parse_implicit_delegate_retirement() {
        let params = json!({
            "runtime_options": {
                "implicit_delegate_idle_retire_secs": 42,
                "implicit_delegate_idle_sweep_interval_ms": 2500
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        assert_eq!(
            options.runtime_options.implicit_delegate_idle_retire_secs,
            Some(42)
        );
        assert_eq!(
            options
                .runtime_options
                .implicit_delegate_idle_sweep_interval_ms,
            2500
        );
    }

    #[test]
    fn gateway_runtime_options_null_disables_implicit_delegate_retirement() {
        let params = json!({
            "runtime_options": {
                "implicit_delegate_idle_retire_secs": null
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        assert_eq!(
            options.runtime_options.implicit_delegate_idle_retire_secs,
            None
        );
    }

    #[test]
    fn gateway_runtime_options_parse_console_config_path() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("console.toml");
        std::fs::write(
            &path,
            r#"
[sidebar]
visible_controls = ["topology", "roster"]

[agent_list]
group_by = ["labels.console_group", "group"]
"#,
        )
        .expect("write console config");
        let params = json!({
            "runtime_options": {
                "console_config_path": path.to_string_lossy()
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        assert_eq!(
            options.console_ui.sidebar.visible_controls,
            Some(vec!["topology".to_string(), "roster".to_string()])
        );
        assert_eq!(
            options.console_ui.agent_list.group_by,
            vec!["labels.console_group".to_string(), "group".to_string()]
        );
    }

    #[test]
    fn gateway_runtime_options_parse_access_config_path() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("access.toml");
        std::fs::write(
            &path,
            r#"
enabled = true
admins = ["root@example.test"]

[[rules]]
id = "everyone-views"
actions = ["agent.view"]
"#,
        )
        .expect("write access config");
        let params = json!({
            "runtime_options": {
                "access_config_path": path.to_string_lossy()
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        let access = options.access.expect("access controller");
        assert!(access.enabled());
        let (config, _) = access.snapshot();
        assert_eq!(config.admins, vec!["root@example.test".to_string()]);
        assert_eq!(config.rules.len(), 1);
    }

    #[test]
    fn gateway_runtime_options_reject_invalid_access_config() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("access.toml");
        // Enabled without admins would lock everyone out — must fail closed.
        std::fs::write(&path, "enabled = true").expect("write access config");
        let params = json!({
            "runtime_options": {
                "access_config_path": path.to_string_lossy()
            }
        });

        let err = match parse_gateway_runtime_options(&params, None) {
            Ok(_) => panic!("invalid access config must be rejected"),
            Err(err) => err,
        };
        assert!(err.contains("access_config_path"), "{err}");
    }

    #[test]
    fn gateway_runtime_options_can_disable_console_auth_for_local_console() {
        let params = json!({
            "runtime_options": {
                "console_require_app_auth": false
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        assert!(
            !options
                .decisions
                .expect("decisions")
                .console
                .require_app_auth
        );
    }

    #[test]
    fn gateway_runtime_options_can_enable_read_only_console() {
        let params = json!({
            "runtime_options": {
                "console_read_only": true
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        assert!(options.decisions.expect("decisions").console.read_only);
    }

    #[test]
    fn gateway_runtime_options_parse_console_fetch_timeout_ms() {
        let params = json!({
            "runtime_options": {
                "console_fetch_timeout_ms": 120_000
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        assert_eq!(
            options
                .decisions
                .expect("decisions")
                .console
                .fetch_timeout_ms,
            Some(120_000)
        );
    }

    #[test]
    fn gateway_runtime_options_reject_zero_console_fetch_timeout_ms() {
        let params = json!({
            "runtime_options": {
                "console_fetch_timeout_ms": 0
            }
        });

        let err = match parse_gateway_runtime_options(&params, None) {
            Ok(_) => panic!("zero should fail"),
            Err(err) => err,
        };

        assert!(err.contains("runtime_options.console_fetch_timeout_ms"));
    }

    #[test]
    fn gateway_runtime_options_parse_demo_llm() {
        let params = json!({
            "runtime_options": {
                "demo_llm": true
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        assert!(options.demo_llm);
    }

    #[test]
    fn gateway_runtime_options_parse_member_comms_address() {
        let params = json!({
            "runtime_options": {
                "member_comms_address": "127.0.0.1:0"
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        assert_eq!(options.member_comms_address.as_deref(), Some("127.0.0.1:0"));
        let config = gateway_agent_config(&options);
        assert_eq!(config.comms.mode, meerkat_core::CommsRuntimeMode::Tcp);
        assert_eq!(config.comms.address.as_deref(), Some("127.0.0.1:0"));
    }

    #[test]
    fn gateway_runtime_options_parse_max_sessions() {
        let params = json!({
            "runtime_options": {
                "max_sessions": 320
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        assert_eq!(options.max_sessions, 320);
    }

    /// Task #62 (HomeCore field ask): the build callback must carry the
    /// mint-vs-resume signal so a host can append standing instructions on
    /// MINT and inherit on RESUME. Measured field gap: both boots printed
    /// session_id=None resume_session_id=None, so every host composing
    /// instructions in build_agent silently accreted one durable System row
    /// per boot.
    #[test]
    fn callback_build_agent_options_carry_resume_session_identity() {
        let resumed_session = meerkat_core::Session::new();
        let resumed_id = resumed_session.id().to_string();
        let build = meerkat_core::service::SessionBuildOptions {
            resume_session: Some(resumed_session),
            ..Default::default()
        };
        let req = CreateSessionRequest {
            model: "m".to_string(),
            prompt: meerkat_core::ContentInput::Text("noop".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: Some(build),
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            injected_context: Vec::new(),
        };

        let options = callback_build_agent_options(&req, "build-test");
        assert_eq!(options["resume_session_id"], resumed_id.as_str());
        assert_eq!(
            options["session_id"],
            resumed_id.as_str(),
            "session identity falls back to the resumed session when labels lack it"
        );

        let mint_req = CreateSessionRequest {
            model: "m".to_string(),
            prompt: meerkat_core::ContentInput::Text("noop".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: None,
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            injected_context: Vec::new(),
        };
        let mint_options = callback_build_agent_options(&mint_req, "build-test");
        assert!(
            mint_options["resume_session_id"].is_null(),
            "a mint build carries no resume identity"
        );
    }

    #[test]
    fn callback_build_agent_options_include_profile_name_from_spawn_labels() {
        let build = meerkat_core::service::SessionBuildOptions {
            peer_meta: Some(
                meerkat_core::PeerMeta::default()
                    .with_label("role", "security")
                    .with_label("agent_identity", "domain:security"),
            ),
            ..Default::default()
        };
        let req = CreateSessionRequest {
            model: "security-model".to_string(),
            prompt: meerkat_core::ContentInput::Text("noop".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: Some(build),
            labels: None,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            injected_context: Vec::new(),
        };

        let options = callback_build_agent_options(&req, "build-test");

        assert_eq!(options["scope_id"], "build-test");
        assert_eq!(
            options["profile_name"], "security",
            "SDK build_agent must see the adopted roster profile instead of \
             falling back to checkpoint-local defaults"
        );
        assert_eq!(options["labels"]["agent_identity"], "domain:security");
    }

    #[test]
    fn callback_build_agent_options_merge_request_and_spawn_labels() {
        let build = meerkat_core::service::SessionBuildOptions {
            peer_meta: Some(
                meerkat_core::PeerMeta::default()
                    .with_label("role", "security")
                    .with_label("agent_identity", "domain:security"),
            ),
            ..Default::default()
        };
        let mut request_labels = BTreeMap::new();
        request_labels.insert("session_id".to_string(), "session-123".to_string());
        let req = CreateSessionRequest {
            model: "security-model".to_string(),
            prompt: meerkat_core::ContentInput::Text("noop".to_string()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            build: Some(build),
            labels: Some(request_labels),
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::default(),
            injected_context: Vec::new(),
        };

        let options = callback_build_agent_options(&req, "build-test");

        assert_eq!(options["profile_name"], "security");
        assert_eq!(options["session_id"], "session-123");
        assert_eq!(options["labels"]["session_id"], "session-123");
        assert_eq!(options["labels"]["agent_identity"], "domain:security");
    }

    #[test]
    fn gateway_runtime_options_parse_agent_memory_defaults() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": {
                "agent_memory": true
            }
        });

        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("runtime options");
        let agent_memory = options.agent_memory.expect("agent memory options");

        assert_eq!(agent_memory.config.realm, "default");
        assert_eq!(
            agent_memory.config.selection,
            meerkat_mobkit::AgentMemorySelection::Contextual
        );
        assert_eq!(agent_memory.config.recall_timeout_ms, 500);
        assert_eq!(
            agent_memory.config.recall_failure_policy,
            meerkat_mobkit::AgentMemoryRecallFailurePolicy::Skip
        );
        assert!(agent_memory.config.defang_inbound);
        assert_eq!(agent_memory.path, tmp.path().join("agent-memory"));
    }

    #[test]
    fn agent_memory_census_slot_covers_both_store_kinds() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({ "runtime_options": { "agent_memory": true } });
        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("runtime options");
        let slot = agent_memory_census_slot(&options.agent_memory.expect("agent memory options"));
        assert_eq!(slot.declaration.domain, "agent-memory");
        assert_eq!(
            slot.declaration.resolution,
            meerkat_core::DurabilityResolution::Persistent
        );
        assert_eq!(slot.backend, "SqliteAgentMemoryStore");
        assert!(!slot.degraded);

        let params = json!({ "runtime_options": { "agent_memory": { "store": "markdown" } } });
        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("runtime options");
        let slot = agent_memory_census_slot(&options.agent_memory.expect("agent memory options"));
        assert_eq!(slot.declaration.domain, "agent-memory");
        assert_eq!(slot.backend, "MarkdownImportOnly");
    }

    /// The live markdown backend is retired, but the refusal has to be a
    /// MIGRATION verdict, not a shrug: it must name the target store and the
    /// lossless import that gets a deployment there. Pinned so the message
    /// cannot decay into "unsupported store".
    #[test]
    fn markdown_live_store_refusal_names_the_migration() {
        let migration = AgentMemoryStoreMigration::MarkdownIsImportOnly;
        assert_eq!(migration.code(), STORAGE_RESOLUTION_CODE);
        let message = migration.message();
        assert!(message.contains("store='sqlite'"), "{message}");
        assert!(message.contains("SAME agent-memory directory"), "{message}");
        assert!(message.contains(".md.imported"), "{message}");
    }

    /// The store kind still PARSES as markdown; only the live construction
    /// path refuses. Keeping the parse arm is what makes the refusal say
    /// "migrate" instead of "unknown value 'markdown'".
    #[test]
    fn markdown_store_kind_still_parses() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({ "runtime_options": { "agent_memory": { "store": "markdown" } } });
        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("runtime options");
        assert_eq!(
            options.agent_memory.expect("agent memory options").store,
            GatewayAgentMemoryStoreKind::Markdown
        );
    }

    #[test]
    fn gateway_runtime_options_parse_agent_memory_defang_inbound() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": {
                "agent_memory": { "defang_inbound": false }
            }
        });

        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("runtime options");
        let agent_memory = options.agent_memory.expect("agent memory options");
        assert!(!agent_memory.config.defang_inbound);

        let params = json!({
            "runtime_options": {
                "agent_memory": { "defang_inbound": "yes" }
            }
        });
        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("non-boolean defang_inbound should fail loudly"),
            Err(err) => err,
        };
        assert!(err.contains("defang_inbound"), "{err}");
    }

    #[test]
    fn gateway_runtime_options_parse_agent_memory_recall_policy() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": {
                "agent_memory": {
                    "recall_timeout_ms": 1200,
                    "recall_failure_policy": "fail"
                }
            }
        });

        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("runtime options");
        let agent_memory = options.agent_memory.expect("agent memory options");

        assert_eq!(agent_memory.config.recall_timeout_ms, 1200);
        assert_eq!(
            agent_memory.config.recall_failure_policy,
            meerkat_mobkit::AgentMemoryRecallFailurePolicy::Fail
        );
    }

    #[test]
    fn gateway_runtime_options_reject_non_boolean_agent_memory_enabled() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": {
                "agent_memory": {
                    "enabled": "false"
                }
            }
        });

        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("non-boolean agent memory enabled should fail"),
            Err(err) => err,
        };

        assert!(err.contains("enabled"), "{err}");
    }

    #[test]
    fn gateway_runtime_options_reject_agent_memory_without_state_path() {
        let params = json!({
            "runtime_options": {
                "agent_memory": true
            }
        });

        let err = match parse_gateway_runtime_options(&params, None) {
            Ok(_) => panic!("agent memory without path should fail"),
            Err(err) => err,
        };

        assert!(err.contains("agent_memory"), "{err}");
    }

    #[test]
    fn gateway_runtime_options_reject_agent_memory_path_override() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": {
                "agent_memory": {
                    "path": "/tmp/other-agent-memory"
                }
            }
        });

        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("agent memory path override should fail"),
            Err(err) => err,
        };

        assert!(err.contains("path"), "{err}");
    }

    #[test]
    fn gateway_runtime_options_agent_memory_store_defaults_to_sqlite() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": {
                "agent_memory": true
            }
        });

        let options = parse_gateway_runtime_options(&params, Some(tmp.path()))
            .expect("boolean agent memory config should parse");
        let agent_memory = options.agent_memory.expect("agent memory options");
        assert_eq!(agent_memory.store, GatewayAgentMemoryStoreKind::Sqlite);

        let params = json!({
            "runtime_options": {
                "agent_memory": { "enabled": true }
            }
        });
        let options = parse_gateway_runtime_options(&params, Some(tmp.path()))
            .expect("object agent memory config should parse");
        let agent_memory = options.agent_memory.expect("agent memory options");
        assert_eq!(agent_memory.store, GatewayAgentMemoryStoreKind::Sqlite);
    }

    #[test]
    fn gateway_runtime_options_agent_memory_store_accepts_markdown() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": {
                "agent_memory": { "store": "markdown" }
            }
        });

        let options = parse_gateway_runtime_options(&params, Some(tmp.path()))
            .expect("markdown store config should parse");
        let agent_memory = options.agent_memory.expect("agent memory options");
        assert_eq!(agent_memory.store, GatewayAgentMemoryStoreKind::Markdown);
        assert_eq!(agent_memory.path, tmp.path().join("agent-memory"));
    }

    #[test]
    fn gateway_runtime_options_agent_memory_store_rejects_unknown_backend() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": {
                "agent_memory": { "store": "postgres" }
            }
        });

        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("unknown store backend should fail loudly"),
            Err(err) => err,
        };

        assert!(err.contains("'markdown' or 'sqlite'"), "{err}");
    }

    #[test]
    fn gateway_runtime_options_agent_memory_budgeted_injection_requires_sqlite() {
        // The §9.1 compaction budget-reset sink is wired only in the sqlite
        // arm; markdown + budgeted would silently stop injecting once the
        // session budget is spent — the config must fail loudly instead.
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": {
                "agent_memory": { "store": "markdown", "per_turn_injection": "budgeted" }
            }
        });
        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("markdown + budgeted injection should fail loudly"),
            Err(err) => err,
        };
        assert!(
            err.contains("per_turn_injection='budgeted' requires store='sqlite'"),
            "{err}"
        );

        // Both knobs stay valid apart: sqlite + budgeted, markdown + off.
        let params = json!({
            "runtime_options": {
                "agent_memory": { "per_turn_injection": "budgeted" }
            }
        });
        let options = parse_gateway_runtime_options(&params, Some(tmp.path()))
            .expect("sqlite + budgeted should parse");
        assert_eq!(
            options
                .agent_memory
                .expect("agent memory")
                .config
                .per_turn_injection,
            meerkat_mobkit::AgentMemoryPerTurnInjection::Budgeted
        );
        let params = json!({
            "runtime_options": {
                "agent_memory": { "store": "markdown", "per_turn_injection": "off" }
            }
        });
        parse_gateway_runtime_options(&params, Some(tmp.path()))
            .expect("markdown + off should parse");
    }

    #[test]
    fn operator_scope_warning_fires_exactly_when_provisional_without_resolver() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let parse = |scope: &str| {
            let params = json!({
                "runtime_options": {
                    "agent_memory": { "operator_scope": scope }
                }
            });
            parse_gateway_runtime_options(&params, Some(tmp.path()))
                .expect("parse")
                .agent_memory
                .expect("agent memory options")
        };
        let provisional = parse("provisional");
        let off = parse("off");

        assert!(operator_scope_recall_inert(Some(&provisional), false));
        assert!(!operator_scope_recall_inert(Some(&provisional), true));
        assert!(!operator_scope_recall_inert(Some(&off), false));
        assert!(!operator_scope_recall_inert(Some(&off), true));
        assert!(!operator_scope_recall_inert(None, false));
        assert!(!operator_scope_recall_inert(None, true));
    }

    #[test]
    fn gateway_runtime_options_agent_memory_distiller_parse_matrix() {
        let tmp = tempfile::tempdir().expect("temp dir");

        // Default: disabled (flipping the default is a calibration decision).
        let params = json!({ "runtime_options": { "agent_memory": true } });
        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("runtime options");
        let agent_memory = options.agent_memory.expect("agent memory options");
        assert!(!agent_memory.distiller.enabled);
        assert_eq!(agent_memory.distiller.runs_per_hour, 12);
        assert_eq!(agent_memory.distiller.min_interactions, 3);
        assert_eq!(agent_memory.distiller.model, None);

        // Full object block.
        let params = json!({
            "runtime_options": {
                "agent_memory": {
                    "distiller": {
                        "enabled": true,
                        "runs_per_hour": 6,
                        "min_interactions": 5,
                        "model": "claude-haiku-4-5"
                    }
                }
            }
        });
        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("runtime options");
        let distiller = options.agent_memory.expect("agent memory").distiller;
        assert!(distiller.enabled);
        assert_eq!(distiller.runs_per_hour, 6);
        assert_eq!(distiller.min_interactions, 5);
        assert_eq!(distiller.model.as_deref(), Some("claude-haiku-4-5"));

        // Boolean shorthand + object-without-enabled is an explicit opt-in.
        let params = json!({
            "runtime_options": { "agent_memory": { "distiller": true } }
        });
        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("runtime options");
        assert!(
            options
                .agent_memory
                .expect("agent memory")
                .distiller
                .enabled
        );
        let params = json!({
            "runtime_options": { "agent_memory": { "distiller": { "runs_per_hour": 2 } } }
        });
        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("runtime options");
        assert!(
            options
                .agent_memory
                .expect("agent memory")
                .distiller
                .enabled
        );

        // Fail-loud: unknown fields, bad types, out-of-range values.
        for (params, needle) in [
            (
                json!({ "runtime_options": { "agent_memory": { "distiller": { "cadence": 5 } } } }),
                "unsupported runtime_options.agent_memory.distiller fields",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "distiller": "on" } } }),
                "must be a boolean or object",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "distiller": { "runs_per_hour": 0 } } } }),
                "runs_per_hour must be between 1 and 240",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "distiller": { "min_interactions": 0 } } } }),
                "min_interactions must be between 1 and 100",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "distiller": { "model": "" } } } }),
                "model must be a non-empty string",
            ),
        ] {
            let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
                Ok(_) => panic!("expected fail-loud parse for {params}"),
                Err(err) => err,
            };
            assert!(err.contains(needle), "{err}");
        }

        // The markdown store has no manifest/tombstone/authored-write seams.
        let params = json!({
            "runtime_options": {
                "agent_memory": { "store": "markdown", "distiller": { "enabled": true } }
            }
        });
        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("markdown + distiller should fail loudly"),
            Err(err) => err,
        };
        assert!(err.contains("distiller requires store='sqlite'"), "{err}");
    }

    #[test]
    fn gateway_runtime_options_agent_memory_steward_parse_matrix() {
        let tmp = tempfile::tempdir().expect("tempdir");

        // Defaults: disabled, */6h cadence, per-realm, 4 runs/day.
        let params = json!({ "runtime_options": { "agent_memory": true } });
        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("defaults parse");
        let steward = options.agent_memory.expect("agent memory").steward;
        assert!(!steward.enabled);
        assert_eq!(steward.cadence, "*/6h");
        assert!(!steward.per_mob);
        assert_eq!(steward.runs_per_day, 4);
        assert_eq!(steward.min_signals, 3);
        assert_eq!(steward.model, None);

        // Full object form.
        let params = json!({
            "runtime_options": {
                "agent_memory": {
                    "steward": {
                        "enabled": true,
                        "cadence": "*/30m",
                        "model": "claude-sonnet-4-6",
                        "per_mob": true,
                        "runs_per_day": 8,
                        "min_signals": 5
                    }
                }
            }
        });
        let steward = parse_gateway_runtime_options(&params, Some(tmp.path()))
            .expect("object form parses")
            .agent_memory
            .expect("agent memory")
            .steward;
        assert!(steward.enabled);
        assert_eq!(steward.cadence, "*/30m");
        assert_eq!(steward.model.as_deref(), Some("claude-sonnet-4-6"));
        assert!(steward.per_mob);
        assert_eq!(steward.runs_per_day, 8);
        assert_eq!(steward.min_signals, 5);

        // Bare true / bare object are opt-ins.
        let params = json!({
            "runtime_options": { "agent_memory": { "steward": true } }
        });
        assert!(
            parse_gateway_runtime_options(&params, Some(tmp.path()))
                .expect("bool form parses")
                .agent_memory
                .expect("agent memory")
                .steward
                .enabled
        );
        let params = json!({
            "runtime_options": { "agent_memory": { "steward": { "runs_per_day": 2 } } }
        });
        assert!(
            parse_gateway_runtime_options(&params, Some(tmp.path()))
                .expect("object without enabled parses")
                .agent_memory
                .expect("agent memory")
                .steward
                .enabled
        );

        // Fail-loud matrix.
        for (params, needle) in [
            (
                json!({ "runtime_options": { "agent_memory": { "steward": { "tempo": 5 } } } }),
                "unsupported runtime_options.agent_memory.steward fields",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "steward": "on" } } }),
                "must be a boolean or object",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "steward": { "cadence": "0 9 * * *" } } } }),
                "not an interval marker",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "steward": { "cadence": "" } } } }),
                "cadence must be a non-empty string",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "steward": { "runs_per_day": 0 } } } }),
                "runs_per_day must be between 1 and 96",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "steward": { "min_signals": 0 } } } }),
                "min_signals must be between 1 and 1000",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "steward": { "model": "" } } } }),
                "model must be a non-empty string",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "steward": { "per_mob": "yes" } } } }),
                "per_mob must be a boolean",
            ),
        ] {
            let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
                Ok(_) => panic!("expected fail-loud parse for {params}"),
                Err(err) => err,
            };
            assert!(err.contains(needle), "{err}");
        }

        // Staging/proposals/harvest tables are sqlite-store machinery.
        let params = json!({
            "runtime_options": {
                "agent_memory": { "store": "markdown", "steward": { "enabled": true } }
            }
        });
        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("markdown + steward should fail loudly"),
            Err(err) => err,
        };
        assert!(err.contains("steward requires store='sqlite'"), "{err}");
    }

    #[test]
    fn gateway_distiller_max_output_tokens_reaches_effective_memory_profile() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let parse = |distiller| {
            parse_gateway_runtime_options(
                &json!({
                    "runtime_options": { "agent_memory": { "distiller": distiller } }
                }),
                Some(tmp.path()),
            )
            .expect("gateway host JSON parses")
            .agent_memory
            .expect("agent memory")
            .distiller
        };

        let default_config = parse(json!(true));
        let default_profile =
            meerkat_mobkit::memory_wiring::effective_distiller_profile(&default_config)
                .expect("memory wiring resolves default profile");
        assert_eq!(
            default_profile.params.max_output_tokens,
            meerkat_mobkit::memory::distiller::DistillerProfile::embedded_default()
                .params
                .max_output_tokens
        );

        let override_config = parse(json!({ "enabled": true, "max_output_tokens": 32_768 }));
        let override_profile =
            meerkat_mobkit::memory_wiring::effective_distiller_profile(&override_config)
                .expect("memory wiring applies host override");
        assert_eq!(override_profile.params.max_output_tokens, 32_768);

        let zero_result = parse_gateway_runtime_options(
            &json!({
                "runtime_options": {
                    "agent_memory": { "distiller": { "max_output_tokens": 0 } }
                }
            }),
            Some(tmp.path()),
        );
        let err = match zero_result {
            Ok(_) => panic!("zero output budget must fail at the host boundary"),
            Err(err) => err,
        };
        assert!(err.contains("must be a positive integer"), "{err}");
    }

    #[test]
    fn gateway_steward_max_output_tokens_reaches_effective_memory_profile() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let parse = |steward| {
            parse_gateway_runtime_options(
                &json!({
                    "runtime_options": { "agent_memory": { "steward": steward } }
                }),
                Some(tmp.path()),
            )
            .expect("gateway host JSON parses")
            .agent_memory
            .expect("agent memory")
            .steward
        };

        let default_config = parse(json!(true));
        let default_profile =
            meerkat_mobkit::memory_wiring::effective_steward_profile(&default_config)
                .expect("memory wiring resolves default profile");
        assert_eq!(
            default_profile.params.max_output_tokens,
            meerkat_mobkit::memory::steward::StewardProfile::embedded_default()
                .params
                .max_output_tokens
        );

        let override_config = parse(json!({ "enabled": true, "max_output_tokens": 65_536 }));
        let override_profile =
            meerkat_mobkit::memory_wiring::effective_steward_profile(&override_config)
                .expect("memory wiring applies host override");
        assert_eq!(override_profile.params.max_output_tokens, 65_536);

        let zero_result = parse_gateway_runtime_options(
            &json!({
                "runtime_options": {
                    "agent_memory": { "steward": { "max_output_tokens": 0 } }
                }
            }),
            Some(tmp.path()),
        );
        let err = match zero_result {
            Ok(_) => panic!("zero output budget must fail at the host boundary"),
            Err(err) => err,
        };
        assert!(err.contains("must be a positive integer"), "{err}");
    }

    #[test]
    fn gateway_hygienist_max_output_tokens_cannot_reach_memory_wiring() {
        let tmp = tempfile::tempdir().expect("tempdir");

        // The retained internal engine still has a bounded default, but the
        // public host has no Hygienist entry in MemoryEnginesConfig.
        assert!(
            meerkat_mobkit::memory::hygienist::HygienistProfile::embedded_default()
                .params
                .max_output_tokens
                > 0
        );

        // Omission is the supported posture.
        let params = json!({ "runtime_options": { "agent_memory": true } });
        parse_gateway_runtime_options(&params, Some(tmp.path())).expect("omission parses");

        // Disabled legacy forms remain accepted, including dormant tuning
        // fields that no longer reach a gateway engine.
        for value in [
            json!(false),
            json!({"enabled": false}),
            json!({
                "enabled": false,
                "runs_per_day": 4,
                "model": "legacy-model",
                "max_output_tokens": 8192
            }),
        ] {
            let params = json!({
                "runtime_options": { "agent_memory": { "hygienist": value } }
            });
            parse_gateway_runtime_options(&params, Some(tmp.path()))
                .expect("disabled compatibility form parses");
        }

        // Every activation-shaped value reaches the same typed refusal. This
        // matrix is mutation-sensitive: changing true to false makes it green.
        for value in [
            json!(true),
            json!({}),
            json!({"enabled": true}),
            json!({"enabled": true, "max_output_tokens": 32_768}),
            json!({"enabled": true, "max_output_tokens": 0}),
            json!({"runs_per_day": 1}),
            json!("on"),
        ] {
            let params = json!({
                "runtime_options": { "agent_memory": { "hygienist": value } }
            });
            let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
                Ok(_) => panic!("parked Hygienist activation must be refused for {params}"),
                Err(err) => err,
            };
            assert!(err.contains("hygienist is PARKED"), "{err}");
            assert!(err.contains("cannot be enabled"), "{err}");
        }

        let refusal = parse_gateway_hygienist_compatibility(&json!(true))
            .expect_err("true is a parked activation request");
        assert_eq!(refusal, ParkedGatewayCapability::Hygienist);
        assert_eq!(refusal.code(), -32602);
    }

    #[test]
    fn gateway_runtime_options_agent_memory_operator_scope_parse_matrix() {
        let tmp = tempfile::tempdir().expect("tempdir");

        // Default: off.
        let params = json!({ "runtime_options": { "agent_memory": true } });
        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("defaults parse");
        assert_eq!(
            options
                .agent_memory
                .expect("agent memory")
                .config
                .operator_scope,
            meerkat_mobkit::AgentMemoryOperatorScope::Off
        );

        // Explicit values.
        for (value, expected) in [
            ("off", meerkat_mobkit::AgentMemoryOperatorScope::Off),
            (
                "provisional",
                meerkat_mobkit::AgentMemoryOperatorScope::Provisional,
            ),
        ] {
            let params = json!({
                "runtime_options": { "agent_memory": { "operator_scope": value } }
            });
            let options = parse_gateway_runtime_options(&params, Some(tmp.path()))
                .expect("operator_scope parses");
            assert_eq!(
                options
                    .agent_memory
                    .expect("agent memory")
                    .config
                    .operator_scope,
                expected
            );
        }

        // Fail-loud on unknown values and wrong types.
        for params in [
            json!({ "runtime_options": { "agent_memory": { "operator_scope": "final" } } }),
            json!({ "runtime_options": { "agent_memory": { "operator_scope": true } } }),
        ] {
            let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
                Ok(_) => panic!("expected fail-loud parse for {params}"),
                Err(err) => err,
            };
            assert!(err.contains("operator_scope must be 'off' or"), "{err}");
        }

        // Operator composition/routing is sqlite-store machinery.
        let params = json!({
            "runtime_options": {
                "agent_memory": { "store": "markdown", "operator_scope": "provisional" }
            }
        });
        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("markdown + operator_scope should fail loudly"),
            Err(err) => err,
        };
        assert!(
            err.contains("operator_scope requires store='sqlite'"),
            "{err}"
        );
    }

    // §7.2 mob-scope binding: the exact resolver the gateway installs on the
    // customizer and injector. The hosting mob composes only for the
    // configured realm — a foreign realm gets no mob scope.
    #[test]
    fn gateway_mob_binding_resolves_hosting_mob_only_for_matching_realm() {
        use meerkat_mobkit::memory::coordinator::{MobScopeResolver, StaticMobBinding};

        let binding = StaticMobBinding {
            realm: "default".to_string(),
            mob: "mob:example".to_string(),
        };
        assert_eq!(
            binding.active_mobs("default", "identity:frontdesk"),
            vec!["mob:example".to_string()],
            "matching realm must compose the hosting mob"
        );
        assert!(
            binding
                .active_mobs("other-realm", "identity:frontdesk")
                .is_empty(),
            "foreign realm must compose no mob scope"
        );
    }

    // §9.1 always-on compaction reset: the sink the gateway registers
    // unconditionally next to the taint tracker. CompactionCompleted with
    // session attribution fires the reset callback with that session key
    // (the gateway binds it to AgentMemoryRuntimeInjector::
    // on_session_compacted); other events and unattributed sources do not.
    #[test]
    fn compaction_reset_sink_fires_on_attributed_compaction_only() {
        use meerkat_core::event::AgentEvent;

        let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = meerkat_mobkit::CompactionResetSink::new({
            let seen = seen.clone();
            Arc::new(move |session: &str| {
                seen.lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .push(session.to_string());
            })
        });

        let session = meerkat_core::types::SessionId::new();
        let envelope = |source, payload| meerkat_core::event::EventEnvelope {
            event_id: Default::default(),
            source,
            seq: 0,
            mob_id: None,
            timestamp_ms: 0,
            payload,
        };
        let compaction = || AgentEvent::CompactionCompleted {
            summary_tokens: 10,
            messages_before: 20,
            messages_after: 2,
        };

        use meerkat_mobkit::MemberAgentEventSink as _;
        // Attributed compaction fires with the envelope's session key.
        sink.observe(
            "identity:a",
            &envelope(
                meerkat_core::event::EventSourceIdentity::Session {
                    session_id: session.clone(),
                },
                compaction(),
            ),
        );
        assert_eq!(
            seen.lock().unwrap_or_else(|err| err.into_inner()).clone(),
            vec![session.to_string()],
            "reset must fire with the compacted session's key"
        );

        // A non-compaction event on the same session never fires it.
        sink.observe(
            "identity:a",
            &envelope(
                meerkat_core::event::EventSourceIdentity::Session {
                    session_id: session.clone(),
                },
                AgentEvent::RunCompleted {
                    session_id: session.clone(),
                    result: "done".to_string(),
                    structured_output: None,
                    extraction_required: false,
                    usage: Default::default(),
                    terminal_cause_kind: None,
                },
            ),
        );
        // Compaction without session attribution has no key to reset.
        sink.observe(
            "identity:a",
            &envelope(
                meerkat_core::event::EventSourceIdentity::Callback,
                compaction(),
            ),
        );
        assert_eq!(
            seen.lock().unwrap_or_else(|err| err.into_inner()).len(),
            1,
            "only attributed compaction events reset"
        );
    }

    #[test]
    fn gateway_runtime_options_parse_agent_memory_taint_knobs() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": {
                "agent_memory": {
                    "llm_writes": "quarantined",
                    "recorder_tool": false,
                    "content_trust": {
                        "trusted_mcp_servers": ["knowledge_graph"],
                        "untrusted_tools": ["scrape_page"],
                        "trusted_tools": ["safe_calc"],
                    }
                }
            }
        });

        let options = parse_gateway_runtime_options(&params, Some(tmp.path()))
            .expect("taint knobs should parse");
        let agent_memory = options.agent_memory.expect("agent memory options");
        assert_eq!(
            agent_memory.config.llm_writes,
            meerkat_mobkit::AgentMemoryLlmWrites::Quarantined
        );
        assert!(!agent_memory.config.recorder_tool);
        assert_eq!(
            agent_memory.config.content_trust.trusted_mcp_servers,
            vec!["knowledge_graph"]
        );
        assert_eq!(
            agent_memory.config.content_trust.untrusted_tools,
            vec!["scrape_page"]
        );

        // Defaults: observed writes, recorder on, empty trust lists.
        let params = json!({
            "runtime_options": { "agent_memory": true }
        });
        let options = parse_gateway_runtime_options(&params, Some(tmp.path()))
            .expect("boolean agent memory config should parse");
        let agent_memory = options.agent_memory.expect("agent memory options");
        assert_eq!(
            agent_memory.config.llm_writes,
            meerkat_mobkit::AgentMemoryLlmWrites::Observed
        );
        assert!(agent_memory.config.recorder_tool);
        assert_eq!(
            agent_memory.config.content_trust,
            meerkat_mobkit::ContentTrustConfig::default()
        );
    }

    #[test]
    fn gateway_runtime_options_reject_bad_taint_knobs() {
        let tmp = tempfile::tempdir().expect("temp dir");
        for (block, needle) in [
            (
                json!({ "llm_writes": "yolo" }),
                "'observed' or 'quarantined'",
            ),
            (json!({ "llm_writes": true }), "'observed' or 'quarantined'"),
            (json!({ "recorder_tool": "yes" }), "must be a boolean"),
            (
                json!({ "content_trust": { "servers": [] } }),
                "unsupported content_trust fields",
            ),
            (
                json!({ "content_trust": { "trusted_mcp_servers": "kg" } }),
                "must be an array",
            ),
            (
                json!({ "store": "markdown", "llm_writes": "quarantined" }),
                "require store='sqlite'",
            ),
            (
                json!({ "store": "markdown", "content_trust": {} }),
                "require store='sqlite'",
            ),
        ] {
            let params = json!({ "runtime_options": { "agent_memory": block } });
            let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
                Ok(_) => panic!("config must fail loudly: {params}"),
                Err(err) => err,
            };
            assert!(err.contains(needle), "{err} (wanted '{needle}')");
        }
    }

    /// The §8.3 selector knob is retired. `off`/empty is still ACCEPTED,
    /// because that config asked for exactly today's behaviour and refusing
    /// it would brick an init for no gain. Every OTHER value is REFUSED,
    /// typed, at init: it asked for a stage that no longer exists, and
    /// accepting it would hand that caller something different from what it
    /// configured while the only notice went to a gateway log the consumer
    /// may not read. Silently-inert configuration is the exact class this
    /// release program exists to remove.
    #[test]
    fn retired_agent_memory_selector_accepts_off_and_refuses_the_rest() {
        let tmp = tempfile::tempdir().expect("temp dir");
        for value in ["off", ""] {
            let params = json!({
                "runtime_options": { "agent_memory": { "selector": value } }
            });
            parse_gateway_runtime_options(&params, Some(tmp.path())).unwrap_or_else(|err| {
                panic!("selector '{value}' states today's behaviour and must parse: {err}")
            });
        }
        for value in ["default", "profile:/etc/mobkit/selector.toml", "on"] {
            let params = json!({
                "runtime_options": { "agent_memory": { "selector": value } }
            });
            let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
                Ok(_) => {
                    panic!("retired selector '{value}' must be refused, not silently ignored")
                }
                Err(err) => err,
            };
            assert!(
                err.contains("RETIRED") && err.contains(value),
                "refusal must name the retirement and the offending value, got: {err}"
            );
        }
        // Retired EVERYWHERE, so the store kind no longer changes the answer.
        // The old refusal here was `selector requires store='sqlite'` - a
        // COUPLING complaint. That coupling is gone, but the value is still
        // refused, now for the retirement itself. Pinning the reason (not
        // merely "some error") is the point: a store-kind message reappearing
        // here would mean the coupling rule outlived the knob it guarded.
        let params = json!({
            "runtime_options": {
                "agent_memory": { "store": "markdown", "selector": "default" }
            }
        });
        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("a retired selector must be refused whatever the store kind"),
            Err(err) => err,
        };
        assert!(
            err.contains("RETIRED"),
            "the refusal must cite the retirement, not the old store-kind coupling, got: {err}"
        );
    }

    /// Type validation does NOT loosen: the value is still a string or a
    /// loud parse refusal, so a structurally wrong config is never silently
    /// swallowed by the accept-and-ignore path.
    #[test]
    fn retired_agent_memory_selector_key_still_rejects_non_strings() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let params = json!({
            "runtime_options": { "agent_memory": { "selector": true } }
        });
        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("a non-string selector value must fail loudly"),
            Err(err) => err,
        };
        assert!(err.contains("must be a string"), "{err}");
    }

    #[test]
    fn gateway_runtime_options_reject_zero_max_sessions() {
        let params = json!({
            "runtime_options": {
                "max_sessions": 0
            }
        });

        let err = match parse_gateway_runtime_options(&params, None) {
            Ok(_) => panic!("zero should fail"),
            Err(err) => err,
        };

        assert!(err.contains("runtime_options.max_sessions"));
    }

    #[test]
    fn gateway_runtime_options_workgraph_parses_bool_and_path_forms() {
        let defaults = parse_gateway_runtime_options(&json!({}), None).expect("defaults parse");
        assert_eq!(
            defaults.workgraph,
            GatewayWorkgraphOption::Enabled,
            "workgraph defaults on"
        );

        let enabled = parse_gateway_runtime_options(
            &json!({ "runtime_options": { "workgraph": true } }),
            None,
        )
        .expect("explicit true parses");
        assert_eq!(enabled.workgraph, GatewayWorkgraphOption::Enabled);

        let disabled = parse_gateway_runtime_options(
            &json!({ "runtime_options": { "workgraph": false } }),
            None,
        )
        .expect("explicit false parses");
        assert_eq!(disabled.workgraph, GatewayWorkgraphOption::Disabled);

        // A string is an explicit durable store directory — the escape hatch
        // for SDK-hosted-continuity launches that have no persistent_state.
        let durable = parse_gateway_runtime_options(
            &json!({ "runtime_options": { "workgraph": "/var/lib/mobkit/workgraph" } }),
            None,
        )
        .expect("string path parses");
        assert_eq!(
            durable.workgraph,
            GatewayWorkgraphOption::DurableDir("/var/lib/mobkit/workgraph".into())
        );

        for invalid in [json!(42), json!(""), json!("   "), json!({ "dir": "x" })] {
            let err = match parse_gateway_runtime_options(
                &json!({ "runtime_options": { "workgraph": invalid } }),
                None,
            ) {
                Ok(_) => panic!("invalid workgraph value should fail: {invalid}"),
                Err(err) => err,
            };
            assert!(err.contains("runtime_options.workgraph"), "{err}");
        }
    }

    #[test]
    fn gateway_identity_bootstrap_mode_parse_matrix_is_strict() {
        use meerkat_mobkit::IdentityBootstrapMode;

        let defaults = parse_gateway_runtime_options(&json!({}), None).expect("defaults");
        assert_eq!(defaults.identity_bootstrap_mode, None);

        for (wire, expected) in [
            (
                json!({"mode": "eager_materialize"}),
                IdentityBootstrapMode::EagerMaterialize,
            ),
            (
                json!({"mode": "lazy_materialize"}),
                IdentityBootstrapMode::LazyMaterialize,
            ),
            (
                json!({"mode": "lazy_with_background_warm", "concurrency": 2}),
                IdentityBootstrapMode::LazyWithBackgroundWarm { concurrency: 2 },
            ),
        ] {
            let options = parse_gateway_runtime_options(
                &json!({"runtime_options": {"identity_bootstrap_mode": wire}}),
                None,
            )
            .expect("valid bootstrap mode");
            assert_eq!(options.identity_bootstrap_mode, Some(expected));
        }

        for invalid in [
            json!("lazy_materialize"),
            json!({}),
            json!({"mode": "unknown"}),
            json!({"mode": "lazy_materialize", "concurrency": 2}),
            json!({"mode": "lazy_with_background_warm"}),
            json!({"mode": "lazy_with_background_warm", "concurrency": 0}),
            json!({"mode": "lazy_with_background_warm", "concurrency": 17}),
            json!({"mode": "eager_materialize", "extra": true}),
        ] {
            let error = parse_gateway_runtime_options(
                &json!({"runtime_options": {"identity_bootstrap_mode": invalid}}),
                None,
            )
            .err()
            .expect("invalid bootstrap mode must fail");
            assert!(error.contains("identity_bootstrap_mode"), "{error}");
        }
    }

    #[cfg(feature = "experimental-gpt-live")]
    #[test]
    fn gateway_experimental_live_registration_is_explicit_and_strict() {
        let defaults = parse_gateway_runtime_options(&json!({}), None).expect("defaults");
        assert!(defaults.experimental_live.is_none());

        let options = parse_gateway_runtime_options(
            &json!({
                "runtime_options": {
                    "experimental_live": {
                        "principal": "user:luka",
                        "realm": "family",
                        "factory_kind": "private-live",
                        "factory_version": "v1",
                        "gate0_qualification": "gate0-v1",
                        "auth_binding": {
                            "realm": "family",
                            "binding": "chatgpt-oauth",
                            "profile": "luka"
                        },
                        "voice": "marin",
                        "instructions": "Use the canonical session context."
                    }
                }
            }),
            None,
        )
        .expect("explicit registration parses");
        let experimental = options.experimental_live.expect("registration");
        assert_eq!(experimental.principal, "user:luka");
        assert_eq!(experimental.realm.as_str(), "family");
        assert_eq!(experimental.binding.realm.as_str(), "family");
        assert_eq!(experimental.binding.binding.as_str(), "chatgpt-oauth");
        assert_eq!(experimental.voice, "marin");
        assert_eq!(
            experimental.instructions.as_deref(),
            Some("Use the canonical session context.")
        );
        assert_eq!(options.live, GatewayLiveOption::Disabled);

        for invalid in [
            json!({
                "realm": "family",
                "factory_kind": "private-live",
                "factory_version": "v1",
                "gate0_qualification": "gate0-v1",
                "auth_binding": {"realm": "family", "binding": "chatgpt-oauth"},
                "voice": "marin"
            }),
            json!({
                "principal": "user:luka",
                "realm": "family",
                "factory_kind": "private-live",
                "factory_version": "v1",
                "gate0_qualification": "gate0-v1",
                "auth_binding": {"realm": "other", "binding": "chatgpt-oauth"},
                "voice": "marin"
            }),
            json!({
                "principal": "user:luka",
                "realm": "family",
                "factory_kind": "private-live",
                "factory_version": "v1",
                "gate0_qualification": "gate0-v1",
                "auth_binding": {"realm": "family", "binding": "chatgpt-oauth"},
                "voice": "marin",
                "ambient_default": true
            }),
        ] {
            let error = parse_gateway_runtime_options(
                &json!({"runtime_options": {"experimental_live": invalid}}),
                None,
            )
            .err()
            .expect("invalid registration must fail");
            assert!(error.contains("experimental_live"), "{error}");
        }
    }

    #[tokio::test]
    async fn callback_close_wakes_pending_call_and_rejects_late_admission() {
        let (stdout_tx, mut stdout_rx) = mpsc::channel(4);
        let bridge = StdioCallbackBridge::new(stdout_tx);
        let pending = tokio::spawn({
            let bridge = bridge.clone();
            async move { bridge.call("callback/build_agent", json!({})).await }
        });

        tokio::time::timeout(Duration::from_secs(1), stdout_rx.recv())
            .await
            .expect("pending callback should be written")
            .expect("stdout callback channel should remain open");
        bridge.close().await;

        let error = tokio::time::timeout(Duration::from_secs(1), pending)
            .await
            .expect("close should wake the pending callback")
            .expect("callback task should not panic")
            .expect_err("closed callback must fail");
        assert!(error.contains("channel dropped"), "{error}");

        let late_error = bridge
            .call("callback/build_agent", json!({}))
            .await
            .expect_err("close must reject callbacks admitted after EOF");
        assert!(late_error.contains("transport closed"), "{late_error}");
        assert!(bridge.state.lock().await.pending.is_empty());
    }

    #[tokio::test]
    async fn callback_builder_delegates_absent_session_compaction_reconciliation() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let inner = FactoryAgentBuilder::new(AgentFactory::new(tmp.path()), Config::default());
        let (stdout_tx, _stdout_rx) = mpsc::channel(1);
        let builder = StdioCallbackAgentBuilder {
            inner,
            bridge: StdioCallbackBridge::new(stdout_tx),
            has_session_builder: false,
            session_store: None,
            detached_jobs: None,
        };
        let session_id = meerkat_core::SessionId::parse("019f74fb-1907-7b21-932d-ab22c4d1f532")
            .expect("valid session id");

        builder
            .abort_absent_session_compaction_stages(&session_id)
            .await
            .expect("callback wrapper must preserve the inner factory's durable-memory seam");
    }

    /// `cleanup_completed` is a four-way conjunction, and the response used to
    /// carry only its result, so a blocked shutdown named no phase. These pin
    /// that each blocking phase is now distinguishable ON THE WIRE, which is
    /// what four undiagnosed CI occurrences actually needed.
    #[test]
    fn a_blocked_shutdown_names_which_phase_blocked() {
        fn clean_report() -> UnifiedRuntimeShutdownReport {
            UnifiedRuntimeShutdownReport {
                drain: ShutdownDrainReport {
                    drained_count: 1,
                    timed_out: false,
                    drain_duration_ms: 2,
                },
                module_shutdown: RuntimeShutdownReport {
                    terminated_modules: vec!["router".to_string()],
                    orphan_processes: 0,
                },
                mob_stop: Ok(()),
                identity_authority_release: IdentityAuthorityReleaseOutcome::Released {
                    grant_count: 1,
                },
                retired_supervisor_cleanup: RetiredSupervisorCleanupOutcome::NothingPending,
            }
        }
        fn diagnostics(report: &UnifiedRuntimeShutdownReport) -> Value {
            let response = gateway_shutdown_response(json!("shutdown"), Some(report));
            assert_eq!(response["result"]["shutdown"], false);
            assert_eq!(response["result"]["runtime_cleanup_completed"], false);
            response["result"]["runtime_cleanup_diagnostics"].clone()
        }

        // 1. drain timed out
        let mut report = clean_report();
        report.drain.timed_out = true;
        report.drain.drain_duration_ms = 4321;
        let blocked = diagnostics(&report);
        assert_eq!(blocked["drain"]["timed_out"], true);
        assert_eq!(blocked["drain"]["drain_duration_ms"], 4321);
        assert_eq!(blocked["mob_stop"], "ok");

        // 2. mob stop failed
        let mut report = clean_report();
        report.mob_stop =
            Err(meerkat_mobkit::mob_handle_runtime::MobRuntimeError::InvalidInput("stop refused"));
        let blocked = diagnostics(&report);
        assert_eq!(blocked["mob_stop"], "failed");
        assert_eq!(blocked["drain"]["timed_out"], false);

        // 3. identity authority not released
        let mut report = clean_report();
        report.identity_authority_release = IdentityAuthorityReleaseOutcome::SkippedMobStopFailed;
        let blocked = diagnostics(&report);
        assert_eq!(
            blocked["identity_authority_release"]["outcome"],
            "skipped_mob_stop_failed"
        );

        // 4. orphan processes remain
        let mut report = clean_report();
        report.module_shutdown.orphan_processes = 3;
        let blocked = diagnostics(&report);
        assert_eq!(blocked["module_shutdown"]["orphan_processes"], 3);

        // 5. a retired supervisor cleanup was still running when its join budget
        // expired. `diagnostics()` asserts shutdown=false and
        // runtime_cleanup_completed=false, so this also pins that this phase
        // ALONE is enough to withhold the attestation. It has to be: the
        // diagnostics block is attached only when cleanup did not complete, so
        // if this outcome did not gate, it could never be reported in the case
        // it exists for.
        let mut report = clean_report();
        report.retired_supervisor_cleanup = RetiredSupervisorCleanupOutcome::Incomplete {
            joined: 2,
            join_failed: 0,
            pending: 1,
        };
        let blocked = diagnostics(&report);
        assert_eq!(
            blocked["retired_supervisor_cleanup"]["outcome"],
            "incomplete"
        );
        assert_eq!(blocked["retired_supervisor_cleanup"]["pending"], 1);
        assert_eq!(blocked["retired_supervisor_cleanup"]["join_failed"], 0);

        // 6. a retired cleanup did not return normally. Distinct from case 5 on
        // the wire even though both are Incomplete: `join_failed` means a
        // cleanup whose release boundary cannot be attested, `pending` means a
        // task that may still be holding the authority.
        let mut report = clean_report();
        report.retired_supervisor_cleanup = RetiredSupervisorCleanupOutcome::Incomplete {
            joined: 0,
            join_failed: 1,
            pending: 0,
        };
        let blocked = diagnostics(&report);
        assert_eq!(
            blocked["retired_supervisor_cleanup"]["outcome"],
            "incomplete"
        );
        assert_eq!(blocked["retired_supervisor_cleanup"]["join_failed"], 1);
        assert_eq!(blocked["retired_supervisor_cleanup"]["pending"], 0);

        // A fully joined cleanup is NOT a blocking phase, so it must not be able
        // to withhold the attestation on its own.
        let mut report = clean_report();
        report.retired_supervisor_cleanup = RetiredSupervisorCleanupOutcome::Joined {
            lease_renewal: 1,
            continuity_repair: 2,
        };
        assert!(
            report.cleanup_completed(),
            "joining every retired cleanup is a completed phase, not a blocked one"
        );

        // Absent report is a DISTINCT case from any phase failing: nothing ran.
        let response = gateway_shutdown_response(json!("shutdown"), None);
        assert_eq!(response["result"]["shutdown"], false);
        assert_eq!(
            response["result"]["runtime_cleanup_diagnostics"]["runtime_shutdown_report"],
            "absent"
        );
    }

    /// The success response must stay byte-identical to what existing SDK
    /// checks already validate: no diagnostics key at all.
    #[test]
    fn a_successful_shutdown_response_gains_no_new_keys() {
        let report = UnifiedRuntimeShutdownReport {
            drain: ShutdownDrainReport {
                drained_count: 1,
                timed_out: false,
                drain_duration_ms: 2,
            },
            module_shutdown: RuntimeShutdownReport {
                terminated_modules: vec!["router".to_string()],
                orphan_processes: 0,
            },
            mob_stop: Ok(()),
            identity_authority_release: IdentityAuthorityReleaseOutcome::NotConfigured,
            retired_supervisor_cleanup: RetiredSupervisorCleanupOutcome::NothingPending,
        };
        let response = gateway_shutdown_response(json!("shutdown"), Some(&report));
        assert_eq!(response["result"]["shutdown"], true);
        assert_eq!(response["result"]["runtime_cleanup_completed"], true);
        // Compare the key SET, not its order. JSON object order is not the
        // contract, and serde_json's map ordering varies with the
        // preserve_order feature, so an ordered assertion here would pin an
        // incidental property and fail for a reason that is not a defect.
        let keys: std::collections::BTreeSet<&str> = response["result"]
            .as_object()
            .expect("result is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from(["shutdown", "runtime_cleanup_completed"]),
            "a successful shutdown must gain no diagnostics key"
        );
    }

    #[test]
    fn advertised_shutdown_horizon_covers_every_bounded_gateway_phase() {
        fn completed_report() -> UnifiedRuntimeShutdownReport {
            UnifiedRuntimeShutdownReport {
                drain: ShutdownDrainReport {
                    drained_count: 1,
                    timed_out: false,
                    drain_duration_ms: 2,
                },
                module_shutdown: RuntimeShutdownReport {
                    terminated_modules: vec!["router".to_string()],
                    orphan_processes: 0,
                },
                mob_stop: Ok(()),
                identity_authority_release: IdentityAuthorityReleaseOutcome::Released {
                    grant_count: 1,
                },
                retired_supervisor_cleanup: RetiredSupervisorCleanupOutcome::NothingPending,
            }
        }

        fn assert_not_attested(report: Option<&UnifiedRuntimeShutdownReport>) {
            let response = gateway_shutdown_response(json!("shutdown"), report);
            assert_eq!(response["result"]["shutdown"], false);
            assert_eq!(response["result"]["runtime_cleanup_completed"], false);
        }

        assert_eq!(PROVIDER_CALLBACK_TIMEOUT, Duration::from_secs(130));
        let mob_quiesce_window = Duration::from_secs(10);
        let scheduler_overhead = Duration::from_secs(10);
        assert_eq!(
            meerkat_mobkit::gateway_composition::GATEWAY_RUNTIME_SHUTDOWN_TIMEOUT,
            PROVIDER_CALLBACK_TIMEOUT
                + PROVIDER_CALLBACK_TIMEOUT
                + GATEWAY_RUNTIME_EVENT_DRAIN_TIMEOUT
                + mob_quiesce_window
                + scheduler_overhead
                // A bounded phase inside runtime.shutdown(). It was added
                // without this term and the gate did not notice, because the
                // sum is a hand-written enumeration of phases rather than
                // anything derived from them. Whoever adds the next phase pays
                // the same tax: state it here, or silently overrun the horizon.
                + meerkat_mobkit::unified_runtime::lifecycle::RETIRED_SUPERVISOR_JOIN_BUDGET,
            "runtime budget must exactly cover both provider callbacks, event drain, mob quiesce, scheduler overhead, and the retired-supervisor join"
        );
        let gateway_phase_budget = GATEWAY_RPC_DRAIN_TIMEOUT
            + meerkat_mobkit::gateway_composition::GATEWAY_HTTP_DRAIN_TIMEOUT
            + meerkat_mobkit::gateway_composition::GATEWAY_RUNTIME_SHUTDOWN_TIMEOUT
            + GATEWAY_STDOUT_DRAIN_TIMEOUT;
        assert_eq!(gateway_phase_budget, Duration::from_secs(327));
        assert_eq!(
            Duration::from_millis(GATEWAY_SHUTDOWN_HORIZON_MS),
            Duration::from_secs(337)
        );
        assert_eq!(
            Duration::from_millis(GATEWAY_SHUTDOWN_HORIZON_MS).saturating_sub(gateway_phase_budget),
            Duration::from_secs(10),
            "advertised horizon must leave response/reaping margin"
        );

        assert_not_attested(None);

        let successful_report = completed_report();
        let completed = gateway_shutdown_response(json!("shutdown"), Some(&successful_report));
        assert_eq!(completed["result"]["shutdown"], true);
        assert_eq!(completed["result"]["runtime_cleanup_completed"], true);

        let mut report = completed_report();
        report.drain.timed_out = true;
        assert_not_attested(Some(&report));

        let mut report = completed_report();
        report.mob_stop = Err(MobRuntimeError::InvalidConfig(
            "mob stop failed".to_string(),
        ));
        report.identity_authority_release = IdentityAuthorityReleaseOutcome::SkippedMobStopFailed;
        assert_not_attested(Some(&report));

        let mut report = completed_report();
        report.identity_authority_release =
            IdentityAuthorityReleaseOutcome::SkippedResetCleanupFailed {
                error: "superseded generation cleanup remains pending".to_string(),
            };
        assert_not_attested(Some(&report));

        let mut report = completed_report();
        report.identity_authority_release = IdentityAuthorityReleaseOutcome::Failed {
            error: "provider release failed".to_string(),
        };
        assert_not_attested(Some(&report));

        let mut report = completed_report();
        report.module_shutdown.orphan_processes = 1;
        assert_not_attested(Some(&report));
    }

    #[test]
    fn gateway_explicit_identity_bootstrap_mode_always_requires_roster() {
        use meerkat_mobkit::IdentityBootstrapMode;

        validate_gateway_identity_bootstrap_intent(None, false)
            .expect("omitted mode preserves the classic gateway");
        validate_gateway_identity_bootstrap_intent(None, true)
            .expect("a roster may use the eager compatibility default");

        for mode in [
            IdentityBootstrapMode::EagerMaterialize,
            IdentityBootstrapMode::LazyMaterialize,
            IdentityBootstrapMode::LazyWithBackgroundWarm { concurrency: 2 },
        ] {
            let error = validate_gateway_identity_bootstrap_intent(Some(&mode), false)
                .expect_err("every explicit identity mode requires a roster");
            assert!(error.contains("roster provider"), "{error}");
            validate_gateway_identity_bootstrap_intent(Some(&mode), true)
                .expect("an explicit mode with a roster is valid");
        }
    }
}

fn parse_gateway_identity_bootstrap_mode(
    value: &Value,
) -> Result<meerkat_mobkit::IdentityBootstrapMode, String> {
    use meerkat_mobkit::identity_first::MAX_IDENTITY_BACKGROUND_WARM_CONCURRENCY;

    let object = value
        .as_object()
        .ok_or_else(|| "runtime_options.identity_bootstrap_mode must be an object".to_string())?;
    let unsupported = object
        .keys()
        .filter(|key| !matches!(key.as_str(), "mode" | "concurrency"))
        .cloned()
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(format!(
            "unsupported runtime_options.identity_bootstrap_mode fields: {}",
            unsupported.join(", ")
        ));
    }
    let mode = object.get("mode").and_then(Value::as_str).ok_or_else(|| {
        "runtime_options.identity_bootstrap_mode.mode must be a string".to_string()
    })?;
    let parsed = match mode {
        "eager_materialize" => {
            if object.contains_key("concurrency") {
                return Err(
                    "runtime_options.identity_bootstrap_mode.concurrency is only valid for lazy_with_background_warm"
                        .to_string(),
                );
            }
            meerkat_mobkit::IdentityBootstrapMode::EagerMaterialize
        }
        "lazy_materialize" => {
            if object.contains_key("concurrency") {
                return Err(
                    "runtime_options.identity_bootstrap_mode.concurrency is only valid for lazy_with_background_warm"
                        .to_string(),
                );
            }
            meerkat_mobkit::IdentityBootstrapMode::LazyMaterialize
        }
        "lazy_with_background_warm" => {
            let concurrency = object
                .get("concurrency")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    "runtime_options.identity_bootstrap_mode.concurrency must be a positive integer"
                        .to_string()
                })?;
            let concurrency = usize::try_from(concurrency).map_err(|_| {
                "runtime_options.identity_bootstrap_mode.concurrency is too large".to_string()
            })?;
            if !(1..=MAX_IDENTITY_BACKGROUND_WARM_CONCURRENCY).contains(&concurrency) {
                return Err(format!(
                    "runtime_options.identity_bootstrap_mode.concurrency must be between 1 and {MAX_IDENTITY_BACKGROUND_WARM_CONCURRENCY}"
                ));
            }
            meerkat_mobkit::IdentityBootstrapMode::LazyWithBackgroundWarm { concurrency }
        }
        _ => {
            return Err(format!(
                "runtime_options.identity_bootstrap_mode.mode '{mode}' is unsupported"
            ));
        }
    };
    Ok(parsed)
}

fn validate_gateway_identity_bootstrap_intent(
    configured_mode: Option<&meerkat_mobkit::IdentityBootstrapMode>,
    has_roster_provider: bool,
) -> Result<(), String> {
    if configured_mode.is_some() && !has_roster_provider {
        return Err(
            "runtime_options.identity_bootstrap_mode requires an identity-first roster provider"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(feature = "experimental-gpt-live")]
fn parse_gateway_experimental_live_option(
    value: &Value,
) -> Result<GatewayExperimentalLiveOption, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "runtime_options.experimental_live must be an object".to_string())?;
    let supported = [
        "principal",
        "realm",
        "factory_kind",
        "factory_version",
        "gate0_qualification",
        "auth_binding",
        "voice",
        "instructions",
    ];
    let unsupported = object
        .keys()
        .filter(|key| !supported.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(format!(
            "unsupported runtime_options.experimental_live fields: {}",
            unsupported.join(", ")
        ));
    }
    let required_string = |name: &str| -> Result<String, String> {
        object
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                format!("runtime_options.experimental_live.{name} must be a non-empty string")
            })
    };
    let principal = required_string("principal")?;
    let realm_text = required_string("realm")?;
    let realm = meerkat_core::RealmId::parse(&realm_text)
        .map_err(|error| format!("runtime_options.experimental_live.realm is invalid: {error}"))?;
    let factory = meerkat::ExperimentalLiveFactoryIdentity::parse(
        required_string("factory_kind")?,
        required_string("factory_version")?,
    )
    .map_err(|error| {
        format!("runtime_options.experimental_live factory identity is invalid: {error}")
    })?;
    let qualification = meerkat::ExperimentalLiveGate0QualificationVersion::parse(required_string(
        "gate0_qualification",
    )?)
    .map_err(|error| {
        format!("runtime_options.experimental_live Gate0 qualification is invalid: {error}")
    })?;
    let binding_object = object
        .get("auth_binding")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            "runtime_options.experimental_live.auth_binding must be an object".to_string()
        })?;
    let binding_supported = ["realm", "binding", "profile"];
    let binding_unsupported = binding_object
        .keys()
        .filter(|key| !binding_supported.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !binding_unsupported.is_empty() {
        return Err(format!(
            "unsupported runtime_options.experimental_live.auth_binding fields: {}",
            binding_unsupported.join(", ")
        ));
    }
    let binding_string = |name: &str| -> Result<String, String> {
        binding_object
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                format!(
                    "runtime_options.experimental_live.auth_binding.{name} must be a non-empty string"
                )
            })
    };
    let binding_realm =
        meerkat_core::RealmId::parse(binding_string("realm")?).map_err(|error| {
            format!("runtime_options.experimental_live.auth_binding.realm is invalid: {error}")
        })?;
    if binding_realm != realm {
        return Err(
            "runtime_options.experimental_live.auth_binding.realm must equal experimental_live.realm"
                .to_string(),
        );
    }
    let binding = meerkat_core::BindingId::parse(binding_string("binding")?).map_err(|error| {
        format!("runtime_options.experimental_live.auth_binding.binding is invalid: {error}")
    })?;
    let profile = match binding_object.get("profile") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let profile = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "runtime_options.experimental_live.auth_binding.profile must be a non-empty string or null"
                        .to_string()
                })?;
            Some(meerkat_core::ProfileId::parse(profile).map_err(|error| {
                format!(
                    "runtime_options.experimental_live.auth_binding.profile is invalid: {error}"
                )
            })?)
        }
    };
    let instructions = match object.get("instructions") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    "runtime_options.experimental_live.instructions must be a non-empty string or null"
                        .to_string()
                })?,
        ),
    };
    Ok(GatewayExperimentalLiveOption {
        principal,
        realm,
        factory,
        qualification,
        binding: meerkat_core::AuthBindingRef {
            realm: binding_realm,
            binding,
            profile,
            origin: meerkat_core::BindingOrigin::Configured,
        },
        voice: required_string("voice")?,
        instructions,
    })
}

fn parse_gateway_runtime_options(
    params: &Value,
    persistent_state: Option<&std::path::Path>,
) -> Result<GatewayRuntimeOptions, String> {
    let Some(runtime_options) = params.get("runtime_options") else {
        return Ok(GatewayRuntimeOptions::default());
    };
    let runtime_options = runtime_options
        .as_object()
        .ok_or_else(|| "runtime_options must be a JSON object".to_string())?;
    let supported = [
        "memory_config",
        "identity_bootstrap_mode",
        "routing_config_path",
        "gating_config_path",
        "auth_config",
        "access_config_path",
        "console_config_path",
        "console_require_app_auth",
        "console_read_only",
        "console_fetch_timeout_ms",
        "demo_llm",
        "member_comms_address",
        "contacts_toml",
        "control_grants_toml",
        "max_sessions",
        "event_log",
        "agent_memory",
        "implicit_delegate_idle_retire_secs",
        "implicit_delegate_idle_sweep_interval_ms",
        "workgraph",
        "live",
        #[cfg(feature = "experimental-gpt-live")]
        "experimental_live",
        "host_runnables",
        "runtime_store",
        "mob_storage",
        "mob_composition",
        "declare_spec_update",
        "compaction",
    ];
    let unsupported = runtime_options
        .keys()
        .filter(|key| !supported.contains(&key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(format!(
            "unsupported runtime_options fields: {}",
            unsupported.join(", ")
        ));
    }

    let mut parsed = GatewayRuntimeOptions::default();
    if let Some(value) = runtime_options.get("identity_bootstrap_mode") {
        parsed.identity_bootstrap_mode = Some(parse_gateway_identity_bootstrap_mode(value)?);
    }
    if let Some(memory_config) = runtime_options.get("memory_config") {
        parsed.runtime_options.memory_backend = Some(parse_gateway_memory_config(
            memory_config,
            persistent_state,
        )?);
    }
    if let Some(agent_memory) = runtime_options.get("agent_memory") {
        parsed.agent_memory = parse_gateway_agent_memory_config(agent_memory, persistent_state)?;
    }
    if let Some(path) = runtime_options
        .get("routing_config_path")
        .and_then(Value::as_str)
    {
        parsed.routing_routes = parse_gateway_routing_config_path(path)?;
    }
    if let Some(path) = runtime_options
        .get("gating_config_path")
        .and_then(Value::as_str)
    {
        parsed.gating = parse_gateway_gating_config_path(path)?;
    }
    if let Some(auth_config) = runtime_options.get("auth_config") {
        parsed.decisions = Some(parse_gateway_auth_config(auth_config)?);
    }
    if let Some(path) = runtime_options
        .get("console_config_path")
        .and_then(Value::as_str)
    {
        parsed.console_ui = load_console_ui_config_from_path_for_realm(path, None)
            .map_err(|err| format!("runtime_options.console_config_path is invalid: {err}"))?;
    }
    if let Some(path) = runtime_options
        .get("access_config_path")
        .and_then(Value::as_str)
    {
        parsed.access = Some(
            meerkat_mobkit::AccessController::load_or_default(path)
                .map_err(|err| format!("runtime_options.access_config_path is invalid: {err}"))?,
        );
    }
    if let Some(value) = runtime_options.get("console_require_app_auth") {
        parsed.console_require_app_auth = Some(value.as_bool().ok_or_else(|| {
            "runtime_options.console_require_app_auth must be a boolean".to_string()
        })?);
    }
    if let Some(value) = runtime_options.get("console_read_only") {
        parsed.console_read_only =
            Some(value.as_bool().ok_or_else(|| {
                "runtime_options.console_read_only must be a boolean".to_string()
            })?);
    }
    if let Some(value) = runtime_options.get("console_fetch_timeout_ms") {
        if !value.is_null() {
            let timeout_ms = value.as_u64().ok_or_else(|| {
                "runtime_options.console_fetch_timeout_ms must be a positive integer or null"
                    .to_string()
            })?;
            if timeout_ms == 0 {
                return Err(
                    "runtime_options.console_fetch_timeout_ms must be greater than zero"
                        .to_string(),
                );
            }
            parsed.console_fetch_timeout_ms = Some(timeout_ms);
        }
    }
    if let Some(value) = runtime_options.get("workgraph") {
        parsed.workgraph = match value {
            Value::Bool(true) => GatewayWorkgraphOption::Enabled,
            Value::Bool(false) => GatewayWorkgraphOption::Disabled,
            Value::String(path) if !path.trim().is_empty() => {
                GatewayWorkgraphOption::DurableDir(std::path::PathBuf::from(path))
            }
            _ => {
                return Err(
                    "runtime_options.workgraph must be a boolean or a non-empty string \
                     (the directory for the durable workgraph store)"
                        .to_string(),
                );
            }
        };
    }
    if let Some(value) = runtime_options.get("live") {
        parsed.live = match value {
            Value::Bool(true) => GatewayLiveOption::Enabled {
                public_base_url: None,
                seed_max_chars: None,
            },
            Value::Bool(false) => GatewayLiveOption::Disabled,
            Value::Object(map) => {
                let enabled = map.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                let seed_max_chars = match map.get("seed_max_chars") {
                    None | Some(Value::Null) => None,
                    Some(value) => {
                        let chars = value.as_u64().filter(|chars| *chars > 0).ok_or_else(|| {
                            "runtime_options.live.seed_max_chars must be a positive integer"
                                .to_string()
                        })?;
                        Some(usize::try_from(chars).map_err(|_| {
                            "runtime_options.live.seed_max_chars is too large".to_string()
                        })?)
                    }
                };
                if enabled {
                    GatewayLiveOption::Enabled {
                        public_base_url: map
                            .get("public_base_url")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        seed_max_chars,
                    }
                } else {
                    GatewayLiveOption::Disabled
                }
            }
            _ => {
                return Err(
                    "runtime_options.live must be a boolean or an object                      ({enabled?, public_base_url?, seed_max_chars?})"
                        .to_string(),
                );
            }
        };
    }
    #[cfg(feature = "experimental-gpt-live")]
    if let Some(value) = runtime_options.get("experimental_live") {
        parsed.experimental_live = Some(parse_gateway_experimental_live_option(value)?);
    }
    if let Some(value) = runtime_options.get("host_runnables") {
        let entries = value.as_array().ok_or_else(|| {
            "runtime_options.host_runnables must be an array of runnable names".to_string()
        })?;
        let mut names = Vec::with_capacity(entries.len());
        for entry in entries {
            let text = entry.as_str().ok_or_else(|| {
                "runtime_options.host_runnables entries must be strings".to_string()
            })?;
            let name = meerkat::HostRunnableName::parse(text).map_err(|err| {
                format!("runtime_options.host_runnables entry '{text}' is invalid: {err}")
            })?;
            if name.as_str() == meerkat_mobkit::schedule_wiring::STEWARD_DREAM_RUNNABLE {
                return Err(format!(
                    "runtime_options.host_runnables entry '{text}' collides with the \
                     reserved steward dream runnable"
                ));
            }
            if names.contains(&name) {
                return Err(format!(
                    "runtime_options.host_runnables entry '{text}' is duplicated"
                ));
            }
            names.push(name);
        }
        parsed.host_runnables = names;
    }
    if let Some(value) = runtime_options.get("demo_llm") {
        parsed.demo_llm = value
            .as_bool()
            .ok_or_else(|| "runtime_options.demo_llm must be a boolean".to_string())?;
    }
    if let Some(value) = runtime_options.get("member_comms_address") {
        let address = value.as_str().ok_or_else(|| {
            "runtime_options.member_comms_address must be a socket address string".to_string()
        })?;
        let socket = address
            .parse::<std::net::SocketAddr>()
            .map_err(|error| format!("runtime_options.member_comms_address is invalid: {error}"))?;
        if socket.ip().is_unspecified() {
            return Err(
                "runtime_options.member_comms_address must name a concrete interface; wildcard binds cannot be advertised to external peers"
                    .to_string(),
            );
        }
        parsed.member_comms_address = Some(address.to_string());
    }
    if let Some(value) = runtime_options.get("contacts_toml") {
        let text = value
            .as_str()
            .ok_or_else(|| "runtime_options.contacts_toml must be a TOML string".to_string())?;
        parsed.contacts = Some(
            ContactDirectory::from_toml(text)
                .map_err(|error| format!("runtime_options.contacts_toml is invalid: {error}"))?,
        );
    }
    if let Some(value) = runtime_options.get("control_grants_toml") {
        let text = value.as_str().ok_or_else(|| {
            "runtime_options.control_grants_toml must be a TOML string".to_string()
        })?;
        parsed.control_grants = ControlGrantTable::from_toml(text)
            .map_err(|error| format!("runtime_options.control_grants_toml is invalid: {error}"))?
            .ok_or_else(|| {
                "runtime_options.control_grants_toml must contain a [control_grants] section"
                    .to_string()
            })?;
    }
    if let Some(value) = runtime_options.get("max_sessions") {
        let max_sessions = value
            .as_u64()
            .ok_or_else(|| "runtime_options.max_sessions must be a positive integer".to_string())?;
        if max_sessions == 0 {
            return Err("runtime_options.max_sessions must be greater than zero".to_string());
        }
        parsed.max_sessions = usize::try_from(max_sessions)
            .map_err(|_| "runtime_options.max_sessions is too large".to_string())?;
    }
    if let Some(event_log) = runtime_options.get("event_log") {
        parsed.event_log = Some(parse_gateway_event_log_config(event_log)?);
    }
    if let Some(runtime_store) = runtime_options.get("runtime_store") {
        parsed.runtime_store_ephemeral = parse_gateway_runtime_store_config(runtime_store)?;
    }
    if let Some(mob_storage) = runtime_options.get("mob_storage") {
        parsed.mob_storage_ephemeral = parse_gateway_mob_storage_config(mob_storage)?;
    }
    if let Some(composition) = runtime_options.get("mob_composition") {
        parsed.composition_authority = parse_gateway_composition_authority(composition)?;
    }
    if let Some(declare) = runtime_options.get("declare_spec_update") {
        parsed.declare_spec_update = Some(parse_gateway_declare_spec_update(declare)?);
    }
    if let Some(compaction) = runtime_options.get("compaction") {
        parsed.compaction = Some(
            meerkat_mobkit::parse_compaction_policy(compaction)
                .map_err(|error| format!("runtime_options.compaction {error}"))?,
        );
    }
    if let Some(value) = runtime_options.get("implicit_delegate_idle_retire_secs") {
        parsed.runtime_options.implicit_delegate_idle_retire_secs = if value.is_null() {
            None
        } else {
            Some(value.as_u64().ok_or_else(|| {
                "runtime_options.implicit_delegate_idle_retire_secs must be a non-negative integer or null".to_string()
            })?)
        };
    }
    if let Some(value) = runtime_options.get("implicit_delegate_idle_sweep_interval_ms") {
        let interval = value.as_u64().ok_or_else(|| {
            "runtime_options.implicit_delegate_idle_sweep_interval_ms must be a positive integer"
                .to_string()
        })?;
        if interval == 0 {
            return Err(
                "runtime_options.implicit_delegate_idle_sweep_interval_ms must be greater than zero"
                    .to_string(),
            );
        }
        parsed
            .runtime_options
            .implicit_delegate_idle_sweep_interval_ms = interval;
    }
    if let Some(require_app_auth) = parsed.console_require_app_auth {
        parsed
            .decisions
            .get_or_insert_with(minimal_decision_state)
            .console
            .require_app_auth = require_app_auth;
    }
    if let Some(read_only) = parsed.console_read_only {
        parsed
            .decisions
            .get_or_insert_with(minimal_decision_state)
            .console
            .read_only = read_only;
    }
    if let Some(fetch_timeout_ms) = parsed.console_fetch_timeout_ms {
        parsed
            .decisions
            .get_or_insert_with(minimal_decision_state)
            .console
            .fetch_timeout_ms = Some(fetch_timeout_ms);
    }
    if let Some(decisions) = parsed.decisions.as_mut() {
        decisions.console.ui = parsed.console_ui.clone();
    }
    Ok(parsed)
}

fn read_gateway_config_file(path: &str, option_name: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read runtime_options.{option_name} '{path}': {err}"))?;
    if path.ends_with(".json") {
        return serde_json::from_str(&text)
            .map_err(|err| format!("invalid JSON in runtime_options.{option_name}: {err}"));
    }
    let toml_value: toml::Value = toml::from_str(&text)
        .map_err(|err| format!("invalid TOML in runtime_options.{option_name}: {err}"))?;
    serde_json::to_value(toml_value)
        .map_err(|err| format!("failed to normalize runtime_options.{option_name}: {err}"))
}

fn parse_gateway_routing_config_path(path: &str) -> Result<Vec<RuntimeRoute>, String> {
    let value = read_gateway_config_file(path, "routing_config_path")?;
    let routes_value = value
        .get("routes")
        .cloned()
        .unwrap_or_else(|| value.clone());
    serde_json::from_value(routes_value)
        .map_err(|err| format!("runtime_options.routing_config_path routes are invalid: {err}"))
}

fn parse_gateway_gating_config_path(path: &str) -> Result<GatewayGatingConfig, String> {
    let value = read_gateway_config_file(path, "gating_config_path")?;
    let actions = value
        .get("actions")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            "runtime_options.gating_config_path must define an actions object".to_string()
        })?;
    let mut action_risk_tiers = HashMap::new();
    for (action, config) in actions {
        let risk_tier = config.as_str().or_else(|| {
            config
                .as_object()
                .and_then(|object| object.get("risk_tier"))
                .and_then(Value::as_str)
        });
        let risk_tier = risk_tier.ok_or_else(|| {
            format!("runtime_options.gating_config_path action '{action}' must define risk_tier")
        })?;
        let normalized = risk_tier.trim().to_ascii_lowercase();
        if !matches!(normalized.as_str(), "r0" | "r1" | "r2" | "r3") {
            return Err(format!(
                "runtime_options.gating_config_path action '{action}' has unsupported risk_tier '{risk_tier}'"
            ));
        }
        action_risk_tiers.insert(action.trim().to_string(), normalized);
    }
    Ok(GatewayGatingConfig { action_risk_tiers })
}

fn parse_gateway_event_log_config(value: &Value) -> Result<EventLogConfig, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "runtime_options.event_log must be a JSON object".to_string())?;
    let storage = object
        .get("storage")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            "runtime_options.event_log.storage must be 'memory' or 'null'".to_string()
        })?;
    // Declared ephemeral choices only: 'memory'/'in_memory' keeps a bounded
    // queryable in-process store, 'null' (M4) explicitly declares dropped
    // events. Both are configurations, never fallbacks — the silent case is
    // the absence of the event_log key (no ingestion at all).
    let store: Box<dyn EventLogStore> = match storage {
        "memory" | "in_memory" => Box::new(InMemoryEventLogStore::default()),
        "null" => Box::new(meerkat_mobkit::unified_runtime::NullEventLogStore),
        other => {
            return Err(format!(
                "unsupported runtime_options.event_log.storage '{other}'"
            ));
        }
    };
    let batch_size = object
        .get("batch_size")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(64);
    // `tokio::time::interval` requires a non-zero period; a zero flush
    // interval would kill the ingestion task, so it is invalid params here
    // rather than a silently clamped value.
    let flush_interval_ms = match object.get("flush_interval_ms") {
        Some(value) => {
            let flush_interval_ms = value.as_u64().ok_or_else(|| {
                "runtime_options.event_log.flush_interval_ms must be a positive integer".to_string()
            })?;
            if flush_interval_ms == 0 {
                return Err(
                    "runtime_options.event_log.flush_interval_ms must be greater than zero"
                        .to_string(),
                );
            }
            flush_interval_ms
        }
        None => 1_000,
    };
    Ok(EventLogConfig {
        store,
        filter: None,
        batch_size,
        flush_interval: Duration::from_millis(flush_interval_ms),
    })
}

/// The event-log slot census entry: an explicitly declared in-process store
/// when `runtime_options.event_log` was configured, otherwise the
/// health-visible record that operational events are not ingested at all —
/// the formerly silent case (M4).
fn gateway_event_log_slot(
    options: &GatewayRuntimeOptions,
) -> meerkat_mobkit::storage_health::StorageSlotSummary {
    if options.event_log.is_some() {
        meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
            "event_log",
            "in-process store",
            "declared via runtime_options.event_log ('memory' retains a bounded queryable \
             buffer; 'null' drops events explicitly)",
        )
    } else {
        meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
            "event_log",
            "not configured",
            "operational events are not ingested; set runtime_options.event_log to declare a \
             store explicitly",
        )
    }
}

/// The agent-memory slot for the composition-time durability census.
/// Configured SQLite composes a disk-backed store under the persistent state
/// dir. Markdown remains recognized only so initialization can return its
/// typed migration refusal; it never becomes a running storage backend.
fn agent_memory_census_slot(
    agent_memory: &GatewayAgentMemoryOptions,
) -> meerkat_mobkit::storage_health::StorageSlotSummary {
    match agent_memory.store {
        GatewayAgentMemoryStoreKind::Sqlite => {
            meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                "agent-memory",
                "SqliteAgentMemoryStore",
            )
        }
        GatewayAgentMemoryStoreKind::Markdown => {
            meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                "agent-memory",
                "MarkdownImportOnly",
            )
        }
    }
}

/// Parse `runtime_options.runtime_store`. The single accepted form is the
/// explicit ephemeral declaration `{"storage": "memory"}` (persistent SQLite
/// is the default and needs no key); everything else is a typed init error.
fn parse_gateway_runtime_store_config(value: &Value) -> Result<bool, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "runtime_options.runtime_store must be a JSON object".to_string())?;
    let storage = object
        .get("storage")
        .and_then(Value::as_str)
        .ok_or_else(|| "runtime_options.runtime_store.storage must be 'memory'".to_string())?;
    if !matches!(storage, "memory" | "in_memory") {
        return Err(format!(
            "unsupported runtime_options.runtime_store.storage '{storage}' (persistent SQLite \
             is the default; the only declaration is 'memory')"
        ));
    }
    Ok(true)
}

fn parse_gateway_composition_authority(
    value: &Value,
) -> Result<meerkat_mobkit::mob_composition_manifest::CompositionAuthority, String> {
    let object = value.as_object().ok_or_else(|| {
        "runtime_options.mob_composition must be a JSON object with an authority".to_string()
    })?;
    let authority = object
        .get("authority")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            "runtime_options.mob_composition.authority must be 'candidate' or 'authoritative'"
                .to_string()
        })?;
    match authority {
        "candidate" | "non_authoritative" => {
            Ok(meerkat_mobkit::mob_composition_manifest::CompositionAuthority::NonAuthoritative)
        }
        "authoritative" => {
            Ok(meerkat_mobkit::mob_composition_manifest::CompositionAuthority::Authoritative)
        }
        other => Err(format!(
            "unsupported runtime_options.mob_composition.authority '{other}' (use 'candidate' for \
             a certification boot that must not pin its own composition, or 'authoritative' - the \
             default - for the launch that speaks for the durable composition)"
        )),
    }
}

fn parse_gateway_declare_spec_update(value: &Value) -> Result<u64, String> {
    let object = value.as_object().ok_or_else(|| {
        "runtime_options.declare_spec_update must be a JSON object with an expected_revision"
            .to_string()
    })?;
    // Required, not defaulted. A declaration without the revision it was made
    // against is not a declaration - it is "accept whatever is there", which is
    // indistinguishable from having no pin.
    let revision = object
        .get("expected_revision")
        .ok_or_else(|| {
            "runtime_options.declare_spec_update.expected_revision is required: it is the \
             revision the divergence was observed at, and declaring without it would accept a \
             spec the operator has not seen"
                .to_string()
        })?
        .as_u64()
        .ok_or_else(|| {
            "runtime_options.declare_spec_update.expected_revision must be a non-negative integer"
                .to_string()
        })?;
    Ok(revision)
}

fn parse_gateway_mob_storage_config(value: &Value) -> Result<bool, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "runtime_options.mob_storage must be a JSON object".to_string())?;
    let storage = object
        .get("storage")
        .and_then(Value::as_str)
        .ok_or_else(|| "runtime_options.mob_storage.storage must be 'memory'".to_string())?;
    if !matches!(storage, "memory" | "in_memory") {
        return Err(format!(
            "unsupported runtime_options.mob_storage.storage '{storage}' (persistent SQLite \
             is the default on a persistent_state launch; the only declaration is 'memory', \
             which trades durable adopted identity declarations for an editable mob_config)"
        ));
    }
    Ok(true)
}

fn parse_gateway_auth_config(value: &Value) -> Result<RuntimeDecisionState, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "runtime_options.auth_config must be a JSON object".to_string())?;
    let provider = object
        .get("provider")
        .and_then(Value::as_str)
        .or_else(|| {
            if object.contains_key("sharedSecret") || object.contains_key("shared_secret") {
                Some("jwt")
            } else {
                None
            }
        })
        .ok_or_else(|| "runtime_options.auth_config.provider is required".to_string())?;
    if provider != "jwt" {
        return Err(format!(
            "unsupported runtime_options.auth_config.provider '{provider}'"
        ));
    }
    let shared_secret = object
        .get("shared_secret")
        .or_else(|| object.get("sharedSecret"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "runtime_options.auth_config.shared_secret must be a non-empty string".to_string()
        })?;
    let issuer = object
        .get("issuer")
        .and_then(Value::as_str)
        .unwrap_or("http://127.0.0.1/mobkit-gateway");
    let audience = object
        .get("audience")
        .and_then(Value::as_str)
        .unwrap_or("persistent-gateway");
    let email_allowlist = object
        .get("email_allowlist")
        .or_else(|| object.get("emailAllowlist"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(shared_secret.as_bytes());
    let discovery_json = serde_json::to_string(&json!({
        "issuer": issuer,
        "jwks_uri": "http://127.0.0.1/mobkit-gateway/jwks.json"
    }))
    .map_err(|err| format!("failed to build trusted OIDC discovery: {err}"))?;
    let jwks_json = serde_json::to_string(&json!({
        "keys": [{
            "kty": "oct",
            "alg": "HS256",
            "k": key
        }]
    }))
    .map_err(|err| format!("failed to build trusted JWKS: {err}"))?;
    Ok(RuntimeDecisionState {
        bigquery: BigQueryNaming {
            dataset: "default_dataset".to_string(),
            table: "default_table".to_string(),
        },
        modules: vec![],
        auth: AuthPolicy {
            default_provider: AuthProvider::GenericOidc,
            email_allowlist,
        },
        trusted_oidc: TrustedOidcRuntimeConfig {
            discovery_json,
            jwks_json,
            audience: audience.to_string(),
        },
        console: ConsolePolicy {
            require_app_auth: true,
            ..ConsolePolicy::default()
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata: ReleaseMetadata {
            targets: vec![
                "crates.io".to_string(),
                "npm".to_string(),
                "pypi".to_string(),
                "github-releases".to_string(),
            ],
            support_matrix: "lts".to_string(),
        },
    })
}

fn apply_gateway_runtime_config_to_request(
    request_line: &str,
    gating: &GatewayGatingConfig,
) -> String {
    let Ok(mut request) = serde_json::from_str::<Value>(request_line) else {
        return request_line.to_string();
    };
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    match method {
        "mobkit/gating/evaluate" => {
            let params = request.get_mut("params").and_then(Value::as_object_mut);
            if let Some(params) = params
                && let Some(action) = params
                    .get("action")
                    .and_then(Value::as_str)
                    .map(|action| action.trim().to_string())
                && let Some(risk_tier) = gating.action_risk_tiers.get(action.as_str())
            {
                // The configured table WINS over a caller-supplied tier. Filling
                // only when absent made `action_risk_tiers` a default wearing a
                // policy's name: a caller could claim `r0` for an `r3` action and
                // `evaluate_gating_action` would honour it, so the compiled table
                // was advisory against exactly the caller it needs to bind.
                if let Some(claimed) = params.get("risk_tier").and_then(Value::as_str)
                    && claimed != risk_tier
                {
                    // Never drop the claim silently. A caller that keeps sending
                    // an overridden tier must be able to learn that it is being
                    // overridden, or it will keep sending it forever.
                    tracing::warn!(
                        action = %action,
                        caller_risk_tier = %claimed,
                        policy_risk_tier = %risk_tier,
                        "gating risk_tier supplied by caller overridden by configured policy"
                    );
                }
                params.insert("risk_tier".to_string(), Value::String(risk_tier.clone()));
            }
        }
        _ => {}
    }
    serde_json::to_string(&request).unwrap_or_else(|_| request_line.to_string())
}

// Legacy state file name written by the misleadingly-named "elephant" backend
// (which always persisted local JSON). Kept so existing deployments keep their
// ledger across the rename.
const LEGACY_MEMORY_LEDGER_STATE_FILE: &str = "elephant-memory-state.json";
const MEMORY_LEDGER_STATE_FILE: &str = "memory-ledger-state.json";

fn parse_gateway_memory_config(
    memory_config: &Value,
    persistent_state: Option<&std::path::Path>,
) -> Result<MemoryBackendConfig, String> {
    let object = memory_config
        .as_object()
        .ok_or_else(|| "runtime_options.memory_config must be a JSON object".to_string())?;
    let backend = object.get("backend").and_then(Value::as_str).ok_or_else(|| {
        "runtime_options.memory_config.backend must be 'local_json' (or the deprecated 'elephant')"
            .to_string()
    })?;
    let persistent_state = persistent_state.ok_or_else(|| {
        "runtime_options.memory_config requires persistent_state so the memory ledger has a stable path"
            .to_string()
    })?;
    match backend {
        "local_json" => {
            let health_check_endpoint = match object.get("health_check_endpoint") {
                None => None,
                Some(value) => Some(
                    value
                        .as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| {
                            "runtime_options.memory_config.health_check_endpoint must be a non-empty string when provided"
                                .to_string()
                        })?
                        .to_string(),
                ),
            };
            let unsupported = object
                .keys()
                .filter(|key| key.as_str() != "backend" && key.as_str() != "health_check_endpoint")
                .map(String::as_str)
                .collect::<Vec<_>>();
            if !unsupported.is_empty() {
                return Err(format!(
                    "unsupported runtime_options.memory_config fields: {}",
                    unsupported.join(", ")
                ));
            }
            // Adopt the legacy ledger file when it is the only one present so
            // switching backend shape does not lose persisted state.
            let new_path = persistent_state.join(MEMORY_LEDGER_STATE_FILE);
            let legacy_path = persistent_state.join(LEGACY_MEMORY_LEDGER_STATE_FILE);
            let state_path = if !new_path.exists() && legacy_path.exists() {
                legacy_path
            } else {
                new_path
            };
            Ok(MemoryBackendConfig::LocalJson(
                LocalJsonMemoryBackendConfig {
                    state_path: state_path.to_string_lossy().to_string(),
                    health_check_endpoint,
                },
            ))
        }
        "elephant" => {
            let endpoint = object
                .get("endpoint")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "runtime_options.memory_config.endpoint must be a non-empty string".to_string()
                })?;
            let unsupported = object
                .keys()
                .filter(|key| key.as_str() != "backend" && key.as_str() != "endpoint")
                .map(String::as_str)
                .collect::<Vec<_>>();
            if !unsupported.is_empty() {
                return Err(format!(
                    "unsupported runtime_options.memory_config fields: {}",
                    unsupported.join(", ")
                ));
            }
            eprintln!(
                "[mobkit-gateway] runtime_options.memory_config.backend 'elephant' is deprecated: \
                 it only health-checks the endpoint and persists the ledger as local JSON; use \
                 backend 'local_json' with an optional health_check_endpoint"
            );
            let state_path = persistent_state.join(LEGACY_MEMORY_LEDGER_STATE_FILE);
            Ok(MemoryBackendConfig::LocalJson(
                LocalJsonMemoryBackendConfig {
                    state_path: state_path.to_string_lossy().to_string(),
                    health_check_endpoint: Some(endpoint.to_string()),
                },
            ))
        }
        other => Err(format!(
            "unsupported runtime_options.memory_config.backend '{other}'"
        )),
    }
}

/// Resolve the agent-memory root through the storage layout: canonical
/// `agent-memory/`, with a legacy `agent-memory-sqlite/` corpus honored
/// where it lies (both dirs present is a twins refusal).
fn resolve_agent_memory_root(
    persistent_state: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    meerkat_mobkit::MobKitStorageLayout::with_injected_roots(persistent_state.to_path_buf(), None)
        .agent_memory_root()
        .map(|resolved| resolved.path)
        .map_err(|e| e.to_string())
}

fn parse_gateway_agent_memory_config(
    agent_memory: &Value,
    persistent_state: Option<&std::path::Path>,
) -> Result<Option<GatewayAgentMemoryOptions>, String> {
    if let Some(enabled) = agent_memory.as_bool() {
        if !enabled {
            return Ok(None);
        }
        let path = resolve_agent_memory_root(persistent_state.ok_or_else(|| {
            "runtime_options.agent_memory=true requires persistent_state".to_string()
        })?)?;
        return Ok(Some(GatewayAgentMemoryOptions {
            config: meerkat_mobkit::AgentMemoryConfig::default(),
            path,
            store: GatewayAgentMemoryStoreKind::default(),
            distiller: meerkat_mobkit::memory::distiller::DistillerConfig::default(),
            steward: meerkat_mobkit::memory::steward::StewardConfig::default(),
        }));
    }

    let object = agent_memory
        .as_object()
        .ok_or_else(|| "runtime_options.agent_memory must be a boolean or object".to_string())?;
    let supported = [
        "enabled",
        "realm",
        "selection",
        "max_entries",
        "recall_timeout_ms",
        "recall_failure_policy",
        "instruction_header",
        "per_turn_injection",
        "defang_inbound",
        "store",
        "llm_writes",
        "recorder_tool",
        "content_trust",
        "selector",
        "distiller",
        "steward",
        "operator_scope",
        "hygienist",
    ];
    let unsupported = object
        .keys()
        .filter(|key| !supported.contains(&key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(format!(
            "unsupported runtime_options.agent_memory fields: {}",
            unsupported.join(", ")
        ));
    }
    if let Some(enabled) = object.get("enabled") {
        let enabled = enabled
            .as_bool()
            .ok_or_else(|| "runtime_options.agent_memory.enabled must be a boolean".to_string())?;
        if !enabled {
            return Ok(None);
        }
    }

    let realm = object
        .get("realm")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("default")
        .to_string();
    let selection = match object
        .get("selection")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("contextual")
    {
        "always" => meerkat_mobkit::AgentMemorySelection::Always,
        "contextual" => meerkat_mobkit::AgentMemorySelection::Contextual,
        other => {
            return Err(format!(
                "runtime_options.agent_memory.selection must be 'always' or 'contextual' (got '{other}')"
            ));
        }
    };
    let max_entries = match object.get("max_entries") {
        None => 8,
        Some(value) => {
            let Some(value) = value.as_u64() else {
                return Err(
                    "runtime_options.agent_memory.max_entries must be a positive integer"
                        .to_string(),
                );
            };
            if value == 0 || value > 64 {
                return Err(
                    "runtime_options.agent_memory.max_entries must be between 1 and 64".to_string(),
                );
            }
            value as usize
        }
    };
    let recall_timeout_ms = match object.get("recall_timeout_ms") {
        None => 500,
        Some(value) => {
            let Some(value) = value.as_u64() else {
                return Err(
                    "runtime_options.agent_memory.recall_timeout_ms must be a positive integer"
                        .to_string(),
                );
            };
            if value == 0 || value > 30_000 {
                return Err(
                    "runtime_options.agent_memory.recall_timeout_ms must be between 1 and 30000"
                        .to_string(),
                );
            }
            value
        }
    };
    let recall_failure_policy = match object
        .get("recall_failure_policy")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("skip")
    {
        "skip" => meerkat_mobkit::AgentMemoryRecallFailurePolicy::Skip,
        "fail" => meerkat_mobkit::AgentMemoryRecallFailurePolicy::Fail,
        other => {
            return Err(format!(
                "runtime_options.agent_memory.recall_failure_policy must be 'skip' or 'fail' (got '{other}')"
            ));
        }
    };
    let instruction_header = match object.get("instruction_header") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "runtime_options.agent_memory.instruction_header must be a non-empty string"
                        .to_string()
                })?
                .to_string(),
        ),
    };
    let per_turn_injection = match object
        .get("per_turn_injection")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("off")
    {
        "off" => meerkat_mobkit::AgentMemoryPerTurnInjection::Off,
        "budgeted" => meerkat_mobkit::AgentMemoryPerTurnInjection::Budgeted,
        other => {
            return Err(format!(
                "runtime_options.agent_memory.per_turn_injection must be 'off' or 'budgeted' (got '{other}')"
            ));
        }
    };
    let defang_inbound = match object.get("defang_inbound") {
        None => true,
        Some(value) => value.as_bool().ok_or_else(|| {
            "runtime_options.agent_memory.defang_inbound must be a boolean".to_string()
        })?,
    };
    let store = match object
        .get("store")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("sqlite")
    {
        "markdown" => GatewayAgentMemoryStoreKind::Markdown,
        "sqlite" => GatewayAgentMemoryStoreKind::Sqlite,
        other => {
            return Err(format!(
                "runtime_options.agent_memory.store must be 'markdown' or 'sqlite' (got '{other}')"
            ));
        }
    };
    let llm_writes = match object.get("llm_writes") {
        None => meerkat_mobkit::AgentMemoryLlmWrites::Observed,
        Some(value) => match value.as_str().map(str::trim) {
            Some("observed") => meerkat_mobkit::AgentMemoryLlmWrites::Observed,
            Some("quarantined") => meerkat_mobkit::AgentMemoryLlmWrites::Quarantined,
            _ => {
                return Err(format!(
                    "runtime_options.agent_memory.llm_writes must be 'observed' or 'quarantined' \
                     (got '{value}')"
                ));
            }
        },
    };
    let recorder_tool = match object.get("recorder_tool") {
        None => true,
        Some(value) => value.as_bool().ok_or_else(|| {
            "runtime_options.agent_memory.recorder_tool must be a boolean".to_string()
        })?,
    };
    let content_trust = match object.get("content_trust") {
        None => meerkat_mobkit::ContentTrustConfig::default(),
        Some(value) => meerkat_mobkit::ContentTrustConfig::from_json_value(value)
            .map_err(|err| format!("runtime_options.agent_memory.{err}"))?,
    };
    // §7.2 operator-scope activation. PROVISIONAL keying (§16 Q1): the
    // value name says so on purpose — deployments opting in accept that the
    // keying (console auth principal) may change when the question closes.
    let operator_scope = match object.get("operator_scope") {
        None => meerkat_mobkit::AgentMemoryOperatorScope::Off,
        Some(value) => match value.as_str().map(str::trim) {
            Some("off") => meerkat_mobkit::AgentMemoryOperatorScope::Off,
            Some("provisional") => meerkat_mobkit::AgentMemoryOperatorScope::Provisional,
            _ => {
                return Err(format!(
                    "runtime_options.agent_memory.operator_scope must be 'off' or \
                     'provisional' (got '{value}')"
                ));
            }
        },
    };
    // §8.3 selector switch: RETIRED. The LLM Selector stage shipped behind
    // this knob and was never activated (default off, in config and in the
    // MOBKIT_AGENT_MEMORY_SELECTOR env fallback alike), so it is gone and
    // recall is the deterministic lexical path on every turn.
    //
    // The KEY is still ACCEPTED, not rejected: `runtime_options` rejects
    // unknown keys fail-loud, so dropping it from the supported set would
    // brick init for any deployment that pinned `selector = "off"` - a
    // config that already asked for exactly today's behaviour.
    //
    // The VALUE is a separate question, and the answer is NOT
    // warn-and-ignore: only off/empty is accepted, and every other value is
    // REFUSED at init just below, with the reasoning stated there. Both
    // halves are pinned by the unit test
    // `retired_agent_memory_selector_accepts_off_and_refuses_the_rest`.
    // Two SDK docstrings still describe the old accept-and-warn shape and
    // are wrong about this gateway - `sdk/python/meerkat_mobkit/builder.py`
    // and `sdk/typescript/src/builder.ts`. The code here is the authority.
    if let Some(value) = object.get("selector") {
        let value = value.as_str().map(str::trim).ok_or_else(|| {
            "runtime_options.agent_memory.selector must be a string \
             (the option is retired; only 'off' remains meaningful)"
                .to_string()
        })?;
        // `off`/empty asked for exactly today's behaviour, so it is accepted
        // silently and nothing is lost. ANY OTHER VALUE ASKED FOR A STAGE THAT
        // NO LONGER EXISTS, and accepting it would silently give that caller
        // something different from what it configured - the declared-but-inert
        // failure this release program exists to remove. A warning is not
        // enough: it goes to the GATEWAY log, which the configuring consumer
        // may not be reading. A downstream fleet spent thirteen days blind to
        // a real failure for exactly that reason. So this refuses, typed, at
        // init, and names the migration.
        if !matches!(value, "" | "off") {
            return Err(format!(
                "runtime_options.agent_memory.selector = '{value}' is RETIRED and cannot be \
                 honoured: the §8.3 LLM Selector stage was removed unactivated, so recall is \
                 now the deterministic lexical path on every turn. Remove the key, or set it \
                 to 'off' to state that intent explicitly. Refusing rather than ignoring, so \
                 you learn this here instead of discovering later that a configured stage \
                 never ran."
            ));
        }
    }
    // §8.4 distiller block: fail-loud parse; enabled defaults false.
    let distiller = match object.get("distiller") {
        None => meerkat_mobkit::memory::distiller::DistillerConfig::default(),
        Some(value) => parse_gateway_distiller_config(value)?,
    };
    // §8.5 steward block: fail-loud parse; enabled defaults false.
    let steward = match object.get("steward") {
        None => meerkat_mobkit::memory::steward::StewardConfig::default(),
        Some(value) => parse_gateway_steward_config(value)?,
    };
    // §8.6 Hygienist is parked. Keep only the disabled compatibility forms;
    // any activation intent is a typed invalid-params refusal.
    if let Some(value) = object.get("hygienist") {
        parse_gateway_hygienist_compatibility(value).map_err(|error| {
            debug_assert_eq!(error.code(), -32602);
            error.message()
        })?;
    }
    // The write gate and taint tracker are store-seam machinery; only the
    // sqlite store has the seam. Accepting these knobs with the markdown
    // store would silently enforce nothing — fail loud instead.
    if store == GatewayAgentMemoryStoreKind::Markdown
        && (llm_writes != meerkat_mobkit::AgentMemoryLlmWrites::Observed
            || object.contains_key("content_trust"))
    {
        return Err(
            "runtime_options.agent_memory.llm_writes/content_trust require store='sqlite'"
                .to_string(),
        );
    }
    // And for the distiller: manifests, tombstones, and the authored-write
    // seam all live on the sqlite store.
    if store == GatewayAgentMemoryStoreKind::Markdown && distiller.enabled {
        return Err("runtime_options.agent_memory.distiller requires store='sqlite'".to_string());
    }
    // And for the steward: staging, proposals, quarantine review, and the
    // pending-harvest/promotion tables are all sqlite-store machinery.
    if store == GatewayAgentMemoryStoreKind::Markdown && steward.enabled {
        return Err("runtime_options.agent_memory.steward requires store='sqlite'".to_string());
    }
    // Operator scope composes through manifests and steward routing — both
    // sqlite-store machinery.
    if store == GatewayAgentMemoryStoreKind::Markdown
        && operator_scope != meerkat_mobkit::AgentMemoryOperatorScope::Off
    {
        return Err(
            "runtime_options.agent_memory.operator_scope requires store='sqlite'".to_string(),
        );
    }
    // And for budgeted per-turn injection: the §9.1 compaction reset sink
    // (which restores the session injection budget and dedup set when a
    // session compacts) is wired only in the sqlite arm, so a markdown
    // deployment would silently stop injecting once a session's cumulative
    // budget is spent — fail loud instead.
    if store == GatewayAgentMemoryStoreKind::Markdown
        && per_turn_injection == meerkat_mobkit::AgentMemoryPerTurnInjection::Budgeted
    {
        return Err(
            "runtime_options.agent_memory.per_turn_injection='budgeted' requires store='sqlite'"
                .to_string(),
        );
    }
    let path =
        resolve_agent_memory_root(persistent_state.ok_or_else(|| {
            "runtime_options.agent_memory requires persistent_state".to_string()
        })?)?;

    Ok(Some(GatewayAgentMemoryOptions {
        config: meerkat_mobkit::AgentMemoryConfig {
            realm,
            selection,
            max_entries,
            recall_timeout_ms,
            recall_failure_policy,
            instruction_header,
            per_turn_injection,
            defang_inbound,
            llm_writes,
            recorder_tool,
            content_trust,
            operator_scope,
        },
        path,
        store,
        distiller,
        steward,
    }))
}

/// Fail-loud parse of `runtime_options.agent_memory.distiller` (§8.4):
/// `{enabled, runs_per_hour, min_interactions, model}`; unknown fields and
/// wrong types are errors, never silently ignored.
fn parse_gateway_distiller_config(
    value: &Value,
) -> Result<meerkat_mobkit::memory::distiller::DistillerConfig, String> {
    let mut config = meerkat_mobkit::memory::distiller::DistillerConfig::default();
    if let Some(enabled) = value.as_bool() {
        config.enabled = enabled;
        return Ok(config);
    }
    let object = value.as_object().ok_or_else(|| {
        "runtime_options.agent_memory.distiller must be a boolean or object".to_string()
    })?;
    let supported = [
        "enabled",
        "runs_per_hour",
        "min_interactions",
        "model",
        "max_output_tokens",
    ];
    let unsupported = object
        .keys()
        .filter(|key| !supported.contains(&key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(format!(
            "unsupported runtime_options.agent_memory.distiller fields: {}",
            unsupported.join(", ")
        ));
    }
    if let Some(enabled) = object.get("enabled") {
        config.enabled = enabled.as_bool().ok_or_else(|| {
            "runtime_options.agent_memory.distiller.enabled must be a boolean".to_string()
        })?;
    } else {
        // An object block without `enabled` is an explicit opt-in.
        config.enabled = true;
    }
    if let Some(value) = object.get("runs_per_hour") {
        let runs = value.as_u64().ok_or_else(|| {
            "runtime_options.agent_memory.distiller.runs_per_hour must be a positive integer"
                .to_string()
        })?;
        if runs == 0 || runs > 240 {
            return Err(
                "runtime_options.agent_memory.distiller.runs_per_hour must be between 1 and 240"
                    .to_string(),
            );
        }
        config.runs_per_hour = runs as u32;
    }
    if let Some(value) = object.get("min_interactions") {
        let min = value.as_u64().ok_or_else(|| {
            "runtime_options.agent_memory.distiller.min_interactions must be a positive integer"
                .to_string()
        })?;
        if min == 0 || min > 100 {
            return Err(
                "runtime_options.agent_memory.distiller.min_interactions must be between 1 and 100"
                    .to_string(),
            );
        }
        config.min_interactions = min as u32;
    }
    if let Some(value) = object.get("model") {
        let model = value
            .as_str()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .ok_or_else(|| {
                "runtime_options.agent_memory.distiller.model must be a non-empty string"
                    .to_string()
            })?;
        config.model = Some(model.to_string());
    }
    // See the steward parser: this ceiling is exposed because a hard-wired one
    // failed invisibly in the field. Upper bound is the profile method's job.
    if let Some(value) = object.get("max_output_tokens") {
        let max = value.as_u64().ok_or_else(|| {
            "runtime_options.agent_memory.distiller.max_output_tokens must be a positive integer"
                .to_string()
        })?;
        if max == 0 || max > u64::from(u32::MAX) {
            return Err(
                "runtime_options.agent_memory.distiller.max_output_tokens must be a positive integer"
                    .to_string(),
            );
        }
        config.max_output_tokens = Some(max as u32);
    }
    Ok(config)
}

/// Fail-loud parse of `runtime_options.agent_memory.steward` (§8.5):
/// `{enabled, cadence, model, per_mob, runs_per_day, min_signals}`;
/// unknown fields and wrong types are errors, never silently ignored. The
/// cadence uses the scheduling subsystem's interval-marker grammar.
fn parse_gateway_steward_config(
    value: &Value,
) -> Result<meerkat_mobkit::memory::steward::StewardConfig, String> {
    let mut config = meerkat_mobkit::memory::steward::StewardConfig::default();
    if let Some(enabled) = value.as_bool() {
        config.enabled = enabled;
        return Ok(config);
    }
    let object = value.as_object().ok_or_else(|| {
        "runtime_options.agent_memory.steward must be a boolean or object".to_string()
    })?;
    let supported = [
        "enabled",
        "cadence",
        "model",
        "per_mob",
        "runs_per_day",
        "min_signals",
        "max_output_tokens",
    ];
    let unsupported = object
        .keys()
        .filter(|key| !supported.contains(&key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(format!(
            "unsupported runtime_options.agent_memory.steward fields: {}",
            unsupported.join(", ")
        ));
    }
    if let Some(enabled) = object.get("enabled") {
        config.enabled = enabled.as_bool().ok_or_else(|| {
            "runtime_options.agent_memory.steward.enabled must be a boolean".to_string()
        })?;
    } else {
        // An object block without `enabled` is an explicit opt-in.
        config.enabled = true;
    }
    if let Some(value) = object.get("cadence") {
        let cadence = value
            .as_str()
            .map(str::trim)
            .filter(|cadence| !cadence.is_empty())
            .ok_or_else(|| {
                "runtime_options.agent_memory.steward.cadence must be a non-empty string"
                    .to_string()
            })?;
        meerkat_mobkit::memory::steward::StewardConfig::parse_cadence(cadence)
            .map_err(|err| format!("runtime_options.agent_memory.steward.cadence: {err}"))?;
        config.cadence = cadence.to_string();
    }
    if let Some(value) = object.get("model") {
        let model = value
            .as_str()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .ok_or_else(|| {
                "runtime_options.agent_memory.steward.model must be a non-empty string".to_string()
            })?;
        config.model = Some(model.to_string());
    }
    if let Some(value) = object.get("per_mob") {
        config.per_mob = value.as_bool().ok_or_else(|| {
            "runtime_options.agent_memory.steward.per_mob must be a boolean".to_string()
        })?;
    }
    if let Some(value) = object.get("runs_per_day") {
        let runs = value.as_u64().ok_or_else(|| {
            "runtime_options.agent_memory.steward.runs_per_day must be a positive integer"
                .to_string()
        })?;
        if runs == 0 || runs > 96 {
            return Err(
                "runtime_options.agent_memory.steward.runs_per_day must be between 1 and 96"
                    .to_string(),
            );
        }
        config.runs_per_day = runs as u32;
    }
    // Exposed because a fleet ran this steward against a
    // hard-wired ceiling and committed zero ops for four days with no reachable
    // way to raise it. The upper bound is deliberately NOT enforced here: the
    // profile method owns validation, and a ceiling the provider rejects is a
    // loud 400, whereas one that is too low fails invisibly.
    if let Some(value) = object.get("max_output_tokens") {
        let max = value.as_u64().ok_or_else(|| {
            "runtime_options.agent_memory.steward.max_output_tokens must be a positive integer"
                .to_string()
        })?;
        if max == 0 || max > u64::from(u32::MAX) {
            return Err(
                "runtime_options.agent_memory.steward.max_output_tokens must be a positive integer"
                    .to_string(),
            );
        }
        config.max_output_tokens = Some(max as u32);
    }
    if let Some(value) = object.get("min_signals") {
        let min = value.as_u64().ok_or_else(|| {
            "runtime_options.agent_memory.steward.min_signals must be a positive integer"
                .to_string()
        })?;
        if min == 0 || min > 1000 {
            return Err(
                "runtime_options.agent_memory.steward.min_signals must be between 1 and 1000"
                    .to_string(),
            );
        }
        config.min_signals = min as u32;
    }
    Ok(config)
}

/// Compatibility parser for a parked public surface. Missing is handled by
/// the caller; `false` and `{enabled:false}` remain accepted so upgrades do
/// not brick deployments that already stated the disabled posture. Every
/// activation-shaped value returns the typed parked-capability verdict.
fn parse_gateway_hygienist_compatibility(value: &Value) -> Result<(), ParkedGatewayCapability> {
    if value.as_bool() == Some(false) {
        return Ok(());
    }
    if value
        .as_object()
        .and_then(|object| object.get("enabled"))
        .and_then(Value::as_bool)
        == Some(false)
    {
        return Ok(());
    }
    Err(ParkedGatewayCapability::Hygienist)
}

/// Environment variable carrying the one request the no-argument mode
/// answers. Set by the SDKs' per-call transports and by nothing else in this
/// tree: `create_gateway_sync_transport` / `create_gateway_async_transport`
/// (Python, private) and `createGatewaySyncTransport` /
/// `createGatewayAsyncTransport` (TypeScript, exported).
const SINGLE_SHOT_REQUEST_ENV: &str = "MOBKIT_RPC_REQUEST";

/// Usage for a genuinely bare invocation: no `--persistent`, and no
/// single-shot request in the environment either. That combination used to
/// land on an `.expect()` panic whose message named an environment variable
/// and never mentioned `--persistent`, which is the mode an operator running
/// `./rpc_gateway` by hand actually wants.
const BARE_INVOCATION_USAGE: &str = r#"rpc_gateway: no mode selected.

usage:
  rpc_gateway --persistent [--control-listen <tcp://host:port | uds:///path>]
      The long-lived gateway. Reads one `mobkit/init` request on stdin, then
      serves JSON-RPC over stdin/stdout until the host closes it. Every SDK
      persistent transport and every deployment uses this mode.

  MOBKIT_RPC_REQUEST='<one JSON-RPC request>' rpc_gateway
      Single-shot module-plane probe: answers exactly one request against a
      fixed built-in demo module and exits. It hosts no mob, no session and
      no agent. The SDK per-call transports use this mode.

  rpc_gateway --version"#;

/// Single-shot mode: reads one request from the environment, answers it
/// against a fixed built-in demo module, and exits.
///
/// RETAINED DELIBERATELY. The simplification program lists this mode for
/// deletion, but the TypeScript SDK still EXPORTS the constructors that spawn
/// this binary with no arguments - `MobkitTypedClient` and
/// `MobkitAsyncClient.fromGatewayBin`, plus the two transport factories,
/// under an explicit "Low-level clients (backward compat)" heading in
/// `sdk/typescript/src/index.ts`. Deleting the mode here without deleting
/// those exports turns every downstream caller into a runtime failure with no
/// compile-time signal anywhere. The Python twins are already private
/// (`meerkat_mobkit._client`, pinned off the public surface by
/// `TestLegacySymbolsRemoved`), but `sdk/python/scripts/productization.py`
/// still drives them. The cut is one atomic change across this file,
/// `handle_mobkit_rpc_json` in `src/rpc.rs`, and the SDK factories, and it is
/// gated on the downstream census.
///
/// `args` is argv without the program name, used only to name unrecognized
/// flags in the usage error.
fn run_single_shot(args: &[String]) {
    let request = match std::env::var(SINGLE_SHOT_REQUEST_ENV) {
        Ok(request) => request,
        Err(std::env::VarError::NotPresent) => {
            if !args.is_empty() {
                eprintln!("rpc_gateway: unrecognized arguments: {}", args.join(" "));
            }
            eprintln!("{BARE_INVOCATION_USAGE}");
            std::process::exit(2);
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            eprintln!(
                "rpc_gateway: {SINGLE_SHOT_REQUEST_ENV} is set but is not valid UTF-8; it must \
                 be one JSON-RPC request object"
            );
            std::process::exit(2);
        }
    };

    let config = MobKitConfig {
        modules: vec![shell_module(
            "routing",
            r#"printf '%s\n' '{"event_id":"evt-routing","source":"module","timestamp_ms":101,"event":{"kind":"module","module":"routing","event_type":"ready","payload":{"family":"routing","health":{"state":"healthy"},"tools":{"list_method":"routing/tools.list","representative_call":{"method":"routing/tool.call","params_schema":{"tool":"string","input":"json"}}}}}}'"#,
        )],
        discovery: DiscoverySpec {
            namespace: "mobkit-rpc".to_string(),
            modules: vec!["routing".to_string()],
        },
        pre_spawn: vec![],
    };

    let mut runtime =
        start_mobkit_runtime(config, vec![], Duration::from_secs(1)).expect("runtime starts");
    let response = handle_mobkit_rpc_json(&mut runtime, &request, Duration::from_secs(1));
    print!("{response}");
    let _ = runtime.shutdown();
}

// ---------------------------------------------------------------------------
// StdioCallbackAgentBuilder — wraps FactoryAgentBuilder, sends callback/build_agent
// to Python over stdout before building the agent.
// ---------------------------------------------------------------------------

struct GatewayStdoutLine {
    response: meerkat_mobkit::SerializedRpcResponseDelivery,
    #[cfg(feature = "experimental-gpt-live")]
    public_observation_delivery: Option<oneshot::Sender<bool>>,
    #[cfg(feature = "experimental-gpt-live")]
    public_observation_binding: Option<LivePublicObservationBinding>,
}

#[cfg(feature = "experimental-gpt-live")]
struct LivePublicObservationBinding {
    machine: Arc<meerkat_runtime::MeerkatMachine>,
    binding: meerkat_live::ProviderWebrtcBinding,
}

impl GatewayStdoutLine {
    fn plain(line: String) -> Self {
        Self {
            response: meerkat_mobkit::SerializedRpcResponseDelivery::plain(line),
            #[cfg(feature = "experimental-gpt-live")]
            public_observation_delivery: None,
            #[cfg(feature = "experimental-gpt-live")]
            public_observation_binding: None,
        }
    }

    fn delivery(response: meerkat_mobkit::SerializedRpcResponseDelivery) -> Self {
        Self {
            response,
            #[cfg(feature = "experimental-gpt-live")]
            public_observation_delivery: None,
            #[cfg(feature = "experimental-gpt-live")]
            public_observation_binding: None,
        }
    }

    #[cfg(feature = "experimental-gpt-live")]
    fn public_observation(
        machine: Arc<meerkat_runtime::MeerkatMachine>,
        binding: meerkat_live::ProviderWebrtcBinding,
        line: String,
    ) -> (Self, oneshot::Receiver<bool>) {
        let (delivery, delivered) = oneshot::channel();
        (
            Self {
                response: meerkat_mobkit::SerializedRpcResponseDelivery::plain(line),
                public_observation_delivery: Some(delivery),
                public_observation_binding: Some(LivePublicObservationBinding { machine, binding }),
            },
            delivered,
        )
    }

    #[cfg(feature = "experimental-gpt-live")]
    async fn acquire_public_observation_custody(
        &self,
    ) -> Result<Option<meerkat_runtime::meerkat_machine::LiveBindingPublicationCustody>, ()> {
        let Some(publication) = self.public_observation_binding.as_ref() else {
            return Ok(None);
        };
        match publication
            .machine
            .acquire_live_binding_publication_custody(&publication.binding)
            .await
            .map_err(|_| ())?
        {
            meerkat_runtime::meerkat_machine::LiveBindingPublicationAdmission::Current(custody) => {
                Ok(Some(custody))
            }
            meerkat_runtime::meerkat_machine::LiveBindingPublicationAdmission::Stale => Err(()),
        }
    }

    async fn settle_delivery(&mut self, delivered: bool) {
        let _ = self.response.settle_delivery(delivered).await;
        #[cfg(feature = "experimental-gpt-live")]
        if let Some(public_observation_delivery) = self.public_observation_delivery.take() {
            let _ = public_observation_delivery.send(delivered);
        }
    }
}

impl Drop for GatewayStdoutLine {
    fn drop(&mut self) {
        #[cfg(feature = "experimental-gpt-live")]
        if let Some(public_observation_delivery) = self.public_observation_delivery.take() {
            let _ = public_observation_delivery.send(false);
        }
    }
}

impl std::ops::Deref for GatewayStdoutLine {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.response.response.as_str()
    }
}

#[cfg(feature = "experimental-gpt-live")]
struct GatewayExperimentalLiveSessionBindingAuthority {
    handle: meerkat_mob::MobHandle,
    machine: Arc<meerkat_runtime::MeerkatMachine>,
    access: meerkat_mobkit::AccessController,
    principal: String,
    allowed_binding: meerkat_core::AuthBindingRef,
}

#[cfg(feature = "experimental-gpt-live")]
#[async_trait]
impl meerkat::experimental_gpt_live::ExperimentalLiveSessionBindingAuthority
    for GatewayExperimentalLiveSessionBindingAuthority
{
    async fn authorize_binding_use(
        &self,
        canonical_session_id: &meerkat_core::SessionId,
        selected_binding: &meerkat_core::AuthBindingRef,
    ) -> Result<
        meerkat::experimental_gpt_live::ExperimentalLiveSessionBindingAuthorization,
        meerkat::experimental_gpt_live::ExperimentalLiveOpenAuthorityError,
    > {
        use meerkat::experimental_gpt_live::ExperimentalLiveOpenAuthorityError;

        if selected_binding != &self.allowed_binding {
            return Err(ExperimentalLiveOpenAuthorityError::BindingUseDenied);
        }
        let mut owners = Vec::new();
        for member in self.handle.list_members().await {
            if self
                .handle
                .resolve_bridge_session_id(&member.agent_identity)
                .await
                .as_ref()
                == Some(canonical_session_id)
            {
                owners.push(member);
            }
        }
        if owners.len() != 1 {
            return Err(ExperimentalLiveOpenAuthorityError::DurableTargetUnavailable);
        }
        let owner = owners
            .pop()
            .ok_or(ExperimentalLiveOpenAuthorityError::DurableTargetUnavailable)?;
        let durable_identity = owner.agent_identity.as_str().to_string();
        let access = self.access.view_for_subject(Some(&self.principal));
        if !access.allows_agent(meerkat_mobkit::access::ACTION_AGENT_SEND, &durable_identity) {
            return Err(ExperimentalLiveOpenAuthorityError::AccessDenied);
        }

        let principal = meerkat_core::PrincipalRef::new(
            meerkat_core::PrincipalKind::Human,
            self.principal.clone(),
        )
        .map_err(|_| ExperimentalLiveOpenAuthorityError::BindingUseDenied)?;
        let target = meerkat_core::PrincipalRef::new(
            meerkat_core::PrincipalKind::PersonalAgent,
            durable_identity,
        )
        .map_err(|_| ExperimentalLiveOpenAuthorityError::BindingUseDenied)?;
        let request = meerkat_core::AuthBindingUseRequest::new(
            principal.clone(),
            target.clone(),
            selected_binding.clone(),
        );
        let grant = meerkat_core::AuthGrant {
            principal: principal.clone(),
            scope: meerkat_core::GrantScope::AuthBinding {
                realm_id: selected_binding.realm.clone(),
                binding_id: selected_binding.binding.clone(),
                profile_id: selected_binding.profile.clone(),
            },
            actions: std::collections::BTreeSet::from([meerkat_core::GrantAction::UseAuthBinding]),
            acting_on_behalf_of: Some(meerkat_core::ActingOnBehalfOf::new(principal, target)),
        };
        let binding_use = meerkat_core::authorize_explicit_auth_binding_use(&request, &[grant])
            .into_result()
            .map_err(|_| ExperimentalLiveOpenAuthorityError::BindingUseDenied)?;
        Ok(
            meerkat::experimental_gpt_live::ExperimentalLiveSessionBindingAuthorization::from_machine_authority(
                binding_use,
                self.machine.generated_auth_lease_handle(),
            ),
        )
    }
}

#[cfg(feature = "experimental-gpt-live")]
#[derive(Clone)]
struct StdioExperimentalLivePublicObservationPublisher {
    machine: Arc<meerkat_runtime::MeerkatMachine>,
    callback_bridge: StdioCallbackBridge,
}

#[cfg(feature = "experimental-gpt-live")]
impl StdioExperimentalLivePublicObservationPublisher {
    fn new(
        machine: Arc<meerkat_runtime::MeerkatMachine>,
        callback_bridge: StdioCallbackBridge,
    ) -> Self {
        Self {
            machine,
            callback_bridge,
        }
    }
}

#[cfg(feature = "experimental-gpt-live")]
#[async_trait]
impl meerkat::experimental_gpt_live::ExperimentalLivePublicObservationPublisher
    for StdioExperimentalLivePublicObservationPublisher
{
    async fn publish(
        &self,
        observation: meerkat::experimental_gpt_live::ExperimentalLivePublicObservation,
    ) -> Result<(), meerkat::experimental_gpt_live::ExperimentalLivePublicObservationDeliveryError>
    {
        use meerkat::experimental_gpt_live::ExperimentalLivePublicObservationDeliveryError;

        if observation.binding().channel_id() != &observation.output().channel_id {
            return Err(ExperimentalLivePublicObservationDeliveryError::Rejected);
        }
        let result = self
            .callback_bridge
            .call_live_public_observation(
                Arc::clone(&self.machine),
                observation.binding().clone(),
                observation.output(),
            )
            .await
            .map_err(|error| {
                if error == "callback transport closed" || error == "stdout channel closed" {
                    ExperimentalLivePublicObservationDeliveryError::Closed
                } else {
                    ExperimentalLivePublicObservationDeliveryError::Rejected
                }
            })?;
        if result.get("accepted").and_then(Value::as_bool) == Some(true) {
            Ok(())
        } else {
            Err(ExperimentalLivePublicObservationDeliveryError::Rejected)
        }
    }
}

impl std::fmt::Display for GatewayStdoutLine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self)
    }
}

async fn write_gateway_stdout_line(line: &mut GatewayStdoutLine) -> bool {
    #[cfg(feature = "experimental-gpt-live")]
    let _public_observation_custody = match line.acquire_public_observation_custody().await {
        Ok(custody) => custody,
        Err(()) => return false,
    };
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{line}")
        .and_then(|()| stdout.flush())
        .is_ok()
}

/// Shared handle for sending lines to stdout and receiving callback responses.
#[derive(Clone)]
struct StdioCallbackBridge {
    /// Send a line to stdout (the stdout writer task reads from this).
    stdout_tx: mpsc::Sender<GatewayStdoutLine>,
    /// Callback admission and pending responses share one lock so EOF cannot
    /// race a late insertion after pending callers have been cancelled.
    state: Arc<Mutex<StdioCallbackState>>,
    /// Counter for generating unique callback IDs.
    counter: Arc<std::sync::atomic::AtomicU64>,
}

#[derive(Default)]
struct StdioCallbackState {
    closed: bool,
    pending: HashMap<String, oneshot::Sender<Value>>,
}

const GATEWAY_SHUTDOWN_METHOD: &str = "mobkit/shutdown";

// Shutdown is negotiated with SDK hosts because the callback bridge must stay
// open while identity-owned cleanup runs. The runtime budget deliberately
// covers two complete callback windows: one for an already-admitted identity
// operation which shutdown must join, and one for the final batched lease
// release. The 312-second runtime budget is exactly two 130-second provider
// callback windows, the runtime's 30-second event drain, its 10-second mob
// quiesce window, 10 seconds of scheduler overhead, and the 2-second
// retired-supervisor join.
const PROVIDER_CALLBACK_TIMEOUT: Duration = Duration::from_secs(130);
const GATEWAY_RPC_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const GATEWAY_RUNTIME_EVENT_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
const GATEWAY_STDOUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

// The bounded gateway phases total at most 327 seconds (5 + 5 + 312 + 5).
// Advertise another 10 seconds for response delivery and process reaping so
// an SDK never races the gateway's own deadline and preempts a valid callback.
const GATEWAY_SHUTDOWN_HORIZON_MS: u64 = 337_000;

#[derive(Debug)]
struct GatewayShutdownRequest {
    response_id: Value,
}

fn gateway_shutdown_request(message: &Value) -> Option<GatewayShutdownRequest> {
    if message.get("method").and_then(Value::as_str) != Some(GATEWAY_SHUTDOWN_METHOD) {
        return None;
    }
    // The private SDK handshake is deliberately request/response, not a
    // notification: the host must keep stdin open until runtime cleanup
    // (including provider callbacks) is complete.
    Some(GatewayShutdownRequest {
        response_id: message.get("id")?.clone(),
    })
}

/// Name WHICH cleanup phase blocked, for a failed shutdown only.
///
/// `cleanup_completed` is a four-way conjunction (drain not timed out, mob stop
/// ok, identity authority released or unconfigured, zero orphan processes), and
/// the wire response used to carry only its result. So a caller learned that
/// cleanup failed and nothing about which of four unrelated phases failed,
/// which left a recurring CI shutdown cluster undiagnosed across four
/// occurrences: every test faithfully printed a response from which the cause
/// had already been erased.
///
/// Typed rather than an encoded string list, so counts stay counts and a new
/// phase can be added without re-parsing prose.
fn gateway_shutdown_diagnostics(runtime_shutdown: Option<&UnifiedRuntimeShutdownReport>) -> Value {
    let Some(report) = runtime_shutdown else {
        // Distinct from any phase failing: there was no report at all, so
        // nothing ran to completion or otherwise.
        return json!({ "runtime_shutdown_report": "absent" });
    };
    json!({
        "runtime_shutdown_report": "present",
        "drain": {
            "timed_out": report.drain.timed_out,
            "drained_count": report.drain.drained_count,
            "drain_duration_ms": report.drain.drain_duration_ms
        },
        // Typed phase status and numeric facts ONLY. No error Display and no
        // provider message: those are unstable across versions and can carry
        // paths or credentials, and this response crosses the SDK wire. The
        // point is to name WHICH phase blocked, which the operator can then
        // pair with the gateway's own logs.
        "mob_stop": match &report.mob_stop {
            Ok(()) => "ok",
            Err(_) => "failed",
        },
        "identity_authority_release": match &report.identity_authority_release {
            IdentityAuthorityReleaseOutcome::NotConfigured => {
                json!({ "outcome": "not_configured" })
            }
            IdentityAuthorityReleaseOutcome::Released { grant_count } => {
                json!({ "outcome": "released", "grant_count": grant_count })
            }
            IdentityAuthorityReleaseOutcome::Failed { .. } => {
                json!({ "outcome": "failed" })
            }
            IdentityAuthorityReleaseOutcome::SkippedResetCleanupFailed { .. } => {
                json!({ "outcome": "skipped_reset_cleanup_failed" })
            }
            IdentityAuthorityReleaseOutcome::SkippedMobStopFailed => {
                json!({ "outcome": "skipped_mob_stop_failed" })
            }
        },
        "module_shutdown": { "orphan_processes": report.module_shutdown.orphan_processes },
        // This phase gates cleanup_completed(), so it CAN be the sole reason
        // this response exists. That is why it must appear here: diagnostics
        // attach only when cleanup did not complete, so a gating phase absent
        // from them would be unreportable in exactly its own failure case.
        "retired_supervisor_cleanup": match &report.retired_supervisor_cleanup {
            RetiredSupervisorCleanupOutcome::NothingPending => {
                json!({ "outcome": "nothing_pending" })
            }
            RetiredSupervisorCleanupOutcome::Joined {
                lease_renewal,
                continuity_repair,
            } => json!({
                "outcome": "joined",
                "lease_renewal": lease_renewal,
                "continuity_repair": continuity_repair
            }),
            RetiredSupervisorCleanupOutcome::Incomplete {
                joined,
                join_failed,
                pending,
            } => json!({
                "outcome": "incomplete",
                "joined": joined,
                "join_failed": join_failed,
                "pending": pending
            }),
            _ => json!({ "outcome": "unclassified" }),
        }
    })
}

fn gateway_shutdown_response(
    response_id: Value,
    runtime_shutdown: Option<&UnifiedRuntimeShutdownReport>,
) -> Value {
    let runtime_cleanup_completed =
        runtime_shutdown.is_some_and(UnifiedRuntimeShutdownReport::cleanup_completed);
    let mut result = json!({
        // Future completion alone is not successful authority cleanup.
        // SDKs validate these fields and surface failure only after
        // reaping the gateway.
        "shutdown": runtime_cleanup_completed,
        "runtime_cleanup_completed": runtime_cleanup_completed
    });
    if !runtime_cleanup_completed {
        // Failure only. A successful shutdown keeps the response byte-identical
        // to what every existing SDK check already validates.
        result["runtime_cleanup_diagnostics"] = gateway_shutdown_diagnostics(runtime_shutdown);
    }
    json!({
        "jsonrpc": "2.0",
        "id": response_id,
        "result": result
    })
}

impl StdioCallbackBridge {
    fn new(stdout_tx: mpsc::Sender<GatewayStdoutLine>) -> Self {
        Self {
            stdout_tx,
            state: Arc::new(Mutex::new(StdioCallbackState::default())),
            counter: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Send a fire-and-forget notification to Python (no response expected).
    /// Uses `try_send` — may drop under backpressure.
    fn notify(&self, method: &str, params: Value) {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        if let Ok(line) = serde_json::to_string(&notification) {
            let _ = self.stdout_tx.try_send(GatewayStdoutLine::plain(line));
        }
    }

    /// Send a notification with reliable delivery (async, waits for channel space).
    /// Use for callbacks where delivery must not be silently lost.
    async fn notify_reliable(&self, method: &str, params: Value) {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        if let Ok(line) = serde_json::to_string(&notification) {
            if let Err(e) = self.stdout_tx.send(GatewayStdoutLine::plain(line)).await {
                eprintln!("[mobkit-gateway] failed to deliver {method}: {e}");
            }
        }
    }

    /// Send a callback request to Python and wait for the response.
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id_str = format!("cb-{id}");

        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            if state.closed {
                return Err("callback transport closed".to_string());
            }
            state.pending.insert(id_str.clone(), tx);
        }

        let request = json!({
            "jsonrpc": "2.0",
            "id": id_str,
            "method": method,
            "params": params,
        });
        let line = match serde_json::to_string(&request) {
            Ok(l) => l,
            Err(e) => {
                self.state.lock().await.pending.remove(&id_str);
                return Err(e.to_string());
            }
        };
        if let Err(_) = self.stdout_tx.send(GatewayStdoutLine::plain(line)).await {
            self.state.lock().await.pending.remove(&id_str);
            return Err("stdout channel closed".to_string());
        }

        // Wait for Python to respond (routed by the stdin multiplexer)
        match tokio::time::timeout(PROVIDER_CALLBACK_TIMEOUT, rx).await {
            Ok(Ok(value)) => {
                if let Some(error) = value.get("error") {
                    Err(format!(
                        "callback error: {}",
                        error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown")
                    ))
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                }
            }
            Ok(Err(_)) => Err("callback response channel dropped".to_string()),
            Err(_) => {
                self.state.lock().await.pending.remove(&id_str);
                Err(format!(
                    "callback timed out after {}s",
                    PROVIDER_CALLBACK_TIMEOUT.as_secs()
                ))
            }
        }
    }

    /// Publish one exact live output as an acknowledged SDK callback.
    ///
    /// The writer acquires opaque machine custody immediately before writing
    /// and holds it through flush. The callback must then explicitly accept
    /// the bounded SDK queue entry. A stale binding, writer failure, missing
    /// consumer, or queue overflow is returned to the provider pump as a
    /// delivery failure instead of silently losing playback authority.
    #[cfg(feature = "experimental-gpt-live")]
    async fn call_live_public_observation(
        &self,
        machine: Arc<meerkat_runtime::MeerkatMachine>,
        binding: meerkat_live::ProviderWebrtcBinding,
        params: impl serde::Serialize,
    ) -> Result<Value, String> {
        let id = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id_str = format!("cb-{id}");

        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            if state.closed {
                return Err("callback transport closed".to_string());
            }
            state.pending.insert(id_str.clone(), tx);
        }

        let request = json!({
            "jsonrpc": "2.0",
            "id": id_str,
            "method": "mobkit/live/assistant_output_available",
            "params": params,
        });
        let line = match serde_json::to_string(&request) {
            Ok(line) => line,
            Err(error) => {
                self.state.lock().await.pending.remove(&id_str);
                return Err(error.to_string());
            }
        };
        let (line, written) = GatewayStdoutLine::public_observation(machine, binding, line);
        if self.stdout_tx.send(line).await.is_err() {
            self.state.lock().await.pending.remove(&id_str);
            return Err("stdout channel closed".to_string());
        }
        match written.await {
            Ok(true) => {}
            Ok(false) => {
                self.state.lock().await.pending.remove(&id_str);
                return Err("live output publication rejected before write".to_string());
            }
            Err(_) => {
                self.state.lock().await.pending.remove(&id_str);
                return Err("stdout channel closed".to_string());
            }
        }

        match tokio::time::timeout(PROVIDER_CALLBACK_TIMEOUT, rx).await {
            Ok(Ok(value)) => {
                if let Some(error) = value.get("error") {
                    Err(format!(
                        "callback error: {}",
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                    ))
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                }
            }
            Ok(Err(_)) => Err("callback response channel dropped".to_string()),
            Err(_) => {
                self.state.lock().await.pending.remove(&id_str);
                Err(format!(
                    "callback timed out after {}s",
                    PROVIDER_CALLBACK_TIMEOUT.as_secs()
                ))
            }
        }
    }

    /// Route an incoming callback response (has "id" starting with "cb-").
    async fn route_callback_response(&self, msg: Value) {
        let id = msg
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(tx) = self.state.lock().await.pending.remove(&id) {
            let _ = tx.send(msg);
        }
    }

    /// Close callback admission and wake every pending caller. The pending
    /// senders are dropped after releasing the lock, resolving their oneshot
    /// receivers immediately without holding callback state across wakeups.
    async fn close(&self) {
        let pending = {
            let mut state = self.state.lock().await;
            state.closed = true;
            std::mem::take(&mut state.pending)
        };
        drop(pending);
    }
}

#[async_trait]
impl meerkat_mobkit::identity_first::gateway_bridges::CallbackBridge for StdioCallbackBridge {
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.call(method, params).await
    }
}

/// One SDK-registered callback tool as it crosses the `callback/build_agent`
/// response wire: a bare name string (legacy shape) or
/// `{name, description?, input_schema?}` so the agent — and the live
/// projection's `live_visible_tool_defs` — see the real argument schema
/// instead of the permissive `{"type": "object"}` placeholder.
struct CallbackToolSpec {
    name: String,
    description: Option<String>,
    input_schema: Option<Value>,
    execution: ToolExecutionContract,
}

#[derive(Clone)]
struct DetachedCallbackJobRuntime {
    realm_id: String,
    store: Arc<dyn meerkat::DetachedJobStore>,
    service: meerkat::DetachedJobService,
    blob_store: Arc<dyn meerkat_core::BlobStore>,
    runtime_inbox: Option<meerkat_runtime::RuntimeDeliveryInbox>,
    delivery_service: Arc<std::sync::RwLock<Option<Arc<dyn meerkat_mob::MobSessionService>>>>,
    delivery_driver_started: Arc<AtomicBool>,
    monitor_recovery_completed: Arc<AtomicBool>,
    monitor_shell_config: Option<meerkat_tools::builtin::shell::ShellConfig>,
    monitor_managers: Arc<Mutex<HashMap<String, Arc<meerkat_tools::builtin::shell::JobManager>>>>,
    callback_runners: Arc<std::sync::RwLock<BTreeSet<(String, String, String)>>>,
}

impl DetachedCallbackJobRuntime {
    fn new(
        realm_id: impl Into<String>,
        store: Arc<dyn meerkat::DetachedJobStore>,
        blob_store: Arc<dyn meerkat_core::BlobStore>,
    ) -> Self {
        Self {
            realm_id: realm_id.into(),
            service: meerkat::DetachedJobService::new(Arc::clone(&store)),
            store,
            blob_store,
            runtime_inbox: None,
            delivery_service: Arc::new(std::sync::RwLock::new(None)),
            delivery_driver_started: Arc::new(AtomicBool::new(false)),
            monitor_recovery_completed: Arc::new(AtomicBool::new(false)),
            monitor_shell_config: None,
            monitor_managers: Arc::new(Mutex::new(HashMap::new())),
            callback_runners: Arc::new(std::sync::RwLock::new(BTreeSet::new())),
        }
    }

    fn with_runtime_delivery_store(
        mut self,
        runtime_store: Arc<dyn meerkat_runtime::RuntimeStore>,
    ) -> Self {
        self.runtime_inbox = Some(meerkat_runtime::RuntimeDeliveryInbox::new(runtime_store));
        self
    }

    fn with_monitor_shell(mut self, project_root: PathBuf, enabled: bool) -> Self {
        if enabled {
            self.monitor_shell_config =
                Some(meerkat_tools::builtin::shell::ShellConfig::with_project_root(project_root));
        }
        self
    }

    fn shell_delivery_projector(
        &self,
    ) -> Result<Arc<dyn meerkat_tools::builtin::shell::ShellJobDeliveryProjector>, String> {
        let runtime_inbox = self.runtime_inbox.clone().ok_or_else(|| {
            "durable monitor execution requires the persistent runtime inbox".to_string()
        })?;
        Ok(Arc::new(meerkat::JobOutboxProjector::new_for_realm(
            Arc::clone(&self.store),
            runtime_inbox,
            self.realm_id.clone(),
        )))
    }

    async fn monitor_manager(
        &self,
        session_id: &meerkat_core::SessionId,
    ) -> Result<Arc<meerkat_tools::builtin::shell::JobManager>, String> {
        let key = session_id.to_string();
        let mut managers = self.monitor_managers.lock().await;
        if let Some(manager) = managers.get(&key) {
            return Ok(Arc::clone(manager));
        }
        let config = self.monitor_shell_config.clone().ok_or_else(|| {
            "monitors/start requires shell tooling to be enabled for this MobKit runtime"
                .to_string()
        })?;
        let durable = meerkat_tools::builtin::shell::DurableShellJobRuntime::new(
            self.realm_id.clone(),
            session_id.clone(),
            Arc::clone(&self.store),
            Arc::clone(&self.blob_store),
            self.shell_delivery_projector()?,
        )
        .map_err(|error| error.to_string())?;
        let manager = Arc::new(
            meerkat_tools::builtin::shell::JobManager::new(config)
                .with_durable_job_runtime(durable)
                .bind_canonical_async_ops(
                    session_id.clone(),
                    Arc::new(meerkat_runtime::RuntimeOpsLifecycleRegistry::new()),
                ),
        );
        managers.insert(key, Arc::clone(&manager));
        Ok(manager)
    }

    fn register_callback_catalog(&self, catalog: &[ToolCatalogEntry]) -> bool {
        let mut runners = self
            .callback_runners
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut registered_new_runner = false;
        for entry in catalog {
            if let Some(policy) = entry.execution.detached_policy() {
                registered_new_runner |= runners.insert((
                    entry.tool.name.to_string(),
                    policy.runner().name().to_string(),
                    policy.runner().version().to_string(),
                ));
            }
        }
        registered_new_runner
    }

    fn owns_callback_job(&self, spec: &meerkat::JobSpec) -> bool {
        self.callback_runners
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&(
                spec.tool.name().to_string(),
                spec.runner.name().to_string(),
                spec.runner.version().to_string(),
            ))
    }

    async fn recover_monitor_jobs(&self) -> Result<(), String> {
        if self.monitor_shell_config.is_none()
            || self.monitor_recovery_completed.load(Ordering::Acquire)
        {
            return Ok(());
        }
        let sessions = self
            .store
            .list_all(usize::MAX)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|job| {
                job.spec.realm_id == self.realm_id
                    && matches!(
                        job.spec.runner.name(),
                        "meerkat.shell" | "meerkat.monitor_script"
                    )
            })
            .map(|job| {
                let session_id = job.spec.origin_session_id;
                (session_id.to_string(), session_id)
            })
            .collect::<BTreeMap<_, _>>();
        for session_id in sessions.into_values() {
            self.monitor_manager(&session_id)
                .await?
                .list_jobs()
                .await
                .map_err(|error| error.to_string())?;
        }
        self.monitor_recovery_completed
            .store(true, Ordering::Release);
        Ok(())
    }

    fn attach_delivery_service(&self, service: Arc<dyn meerkat_mob::MobSessionService>) {
        *self
            .delivery_service
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(service);
    }

    fn arm_delivery_driver(&self, unified_runtime: Arc<UnifiedRuntime>) {
        if self
            .delivery_driver_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let runtime = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = runtime.recover_monitor_jobs().await {
                    tracing::warn!(%error, "durable monitor recovery failed");
                }
                if let Err(error) = runtime.drain_deliveries().await {
                    tracing::warn!(%error, "durable callback delivery drain failed");
                }
                match runtime.health_projection().await {
                    Ok(projection) => unified_runtime.set_job_health_projection(Some(projection)),
                    Err(error) => {
                        tracing::warn!(%error, "durable callback health projection failed");
                        unified_runtime.set_job_health_projection(Some(json!({
                            "status": "degraded",
                            "detached_jobs": {
                                "status": "degraded",
                                "reason": "job_health_projection_failed"
                            }
                        })));
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    async fn drain_deliveries(&self) -> Result<(), String> {
        let Some(runtime_inbox) = self.runtime_inbox.clone() else {
            return Ok(());
        };
        let delivery_service = self
            .delivery_service
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(delivery_service) = delivery_service else {
            return Ok(());
        };
        let projector = meerkat::JobOutboxProjector::new_for_realm(
            Arc::clone(&self.store),
            runtime_inbox.clone(),
            self.realm_id.clone(),
        );
        projector
            .project_pending(256)
            .await
            .map_err(|error| error.to_string())?;

        let sink: Arc<dyn meerkat::JobDeliverySink> =
            Arc::new(CallbackJobDeliverySink { delivery_service });
        let applier = meerkat::JobRuntimeDeliveryApplier::new(runtime_inbox, sink);
        for session_id in self.delivery_origin_sessions().await? {
            applier
                .apply_pending(
                    &meerkat_runtime::LogicalRuntimeId::for_session(&session_id),
                    256,
                )
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    async fn delivery_origin_sessions(&self) -> Result<Vec<meerkat_core::SessionId>, String> {
        Ok(self
            .store
            .list_all(usize::MAX)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|job| job.spec.realm_id == self.realm_id)
            .map(|job| {
                let origin = job.spec.origin_session_id;
                (origin.to_string(), origin)
            })
            .collect::<BTreeMap<_, _>>()
            .into_values()
            .collect())
    }

    /// The exact host-store total behind `JobHealthSummary.runtime_inbox_backlog`.
    ///
    /// Deliberately NOT derived by walking job rows for origin sessions. The
    /// durable runtime store is one file per realm ROOT and serves every
    /// session this host built - including members built under `mob.<mob_id>`,
    /// and runtime ids carry no realm - so a job-derived sum silently misses
    /// any runtime whose job aged out or sits outside the logical realm, and
    /// publishes a false zero exactly when a backlog is what the operator is
    /// asking about. Over-inclusion is the safe direction here: every counted
    /// row is a real undrained delivery in this host.
    async fn runtime_inbox_backlog_count(&self) -> Result<u64, String> {
        let Some(runtime_inbox) = self.runtime_inbox.clone() else {
            return Ok(0);
        };
        runtime_inbox
            .pending_delivery_total()
            .await
            .map_err(|error| error.to_string())
    }

    async fn health_projection(&self) -> Result<Value, String> {
        let now_ms = callback_unix_time_ms()?;
        let health = self
            .service
            .health_snapshot_for_realm(&self.realm_id, now_ms, usize::MAX)
            .await
            .map_err(|error| error.to_string())?;
        // NOT summed with `health.pending_outbox_jobs`. The two name different
        // wedges: a job whose delivery was never handed to a runtime, versus a
        // delivery a runtime accepted and never drained. 0.8.23 published their
        // sum under one name, which is a number that means neither.
        let runtime_inbox_backlog = self.runtime_inbox_backlog_count().await?;
        // Fold mobkit's own runtime dimension into meerkat's census rung rather
        // than collapsing to a boolean. `Unreadable` outranks `Degraded`: a
        // census that could not be read may be hiding the fault, so the
        // operator is told to look rather than told a rung.
        let reading = health.reading().worst(if runtime_inbox_backlog > 0 {
            meerkat::JobHealthReading::Degraded
        } else {
            meerkat::JobHealthReading::Ok
        });
        let status = match reading {
            meerkat::JobHealthReading::Ok => "ok",
            meerkat::JobHealthReading::Degraded => "degraded",
            meerkat::JobHealthReading::Unreadable => "unreadable",
        };
        let mut by_session = serde_json::Map::new();
        for job in self
            .store
            .list_all(usize::MAX)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|job| job.spec.realm_id == self.realm_id)
        {
            let key = job.spec.origin_session_id.to_string();
            let entry = by_session.entry(key).or_insert_with(|| {
                json!({
                    "active": 0_u64,
                    "awaiting_detached": false,
                    "queued": 0_u64,
                    "running": 0_u64,
                    "needs_attention": 0_u64
                })
            });
            let active = matches!(
                job.machine_state.lifecycle_phase,
                meerkat::JobPhase::Unsubmitted
                    | meerkat::JobPhase::Queued
                    | meerkat::JobPhase::Claimed
                    | meerkat::JobPhase::Running
                    | meerkat::JobPhase::WaitingExternal
                    | meerkat::JobPhase::LossObserved
                    | meerkat::JobPhase::RetryScheduled
            );
            if active {
                entry["active"] = json!(
                    entry["active"]
                        .as_u64()
                        .unwrap_or_default()
                        .saturating_add(1)
                );
                entry["awaiting_detached"] = Value::Bool(true);
            }
            let field = match job.machine_state.lifecycle_phase {
                meerkat::JobPhase::Queued | meerkat::JobPhase::RetryScheduled => Some("queued"),
                meerkat::JobPhase::Running | meerkat::JobPhase::WaitingExternal => Some("running"),
                meerkat::JobPhase::NeedsAttention => Some("needs_attention"),
                _ => None,
            };
            if let Some(field) = field {
                entry[field] = json!(entry[field].as_u64().unwrap_or_default().saturating_add(1));
            }
        }
        Ok(json!({
            "status": status,
            "monitors_available": self.monitor_shell_config.is_some(),
            "detached_jobs": {
                "status": status,
                "queued": health.queued,
                "running": health.running,
                "awaiting_members": health.awaiting_members,
                "stale_leases": health.stale_leases,
                "needs_attention": health.needs_attention,
                "pending_outbox_jobs": health.pending_outbox_jobs,
                "runtime_inbox_backlog": runtime_inbox_backlog
            },
            "by_session": by_session
        }))
    }
}

struct CallbackJobDeliverySink {
    delivery_service: Arc<dyn meerkat_mob::MobSessionService>,
}

#[async_trait]
impl meerkat::JobDeliverySink for CallbackJobDeliverySink {
    async fn apply(&self, application: meerkat::JobDeliveryApplication) -> Result<(), String> {
        match application {
            meerkat::JobDeliveryApplication::Record { .. } => Ok(()),
            meerkat::JobDeliveryApplication::Notification {
                job_id,
                delivery_sequence,
                subscription,
                content,
            } => {
                let mut request = meerkat_core::service::AppendSystemContextRequest::from_text(
                    callback_job_delivery_text(&job_id, &content),
                );
                request.source = Some(format!("detached_job:{job_id}"));
                request.idempotency_key = Some(format!(
                    "job:{job_id}:{delivery_sequence}:{}",
                    subscription.subscription_id()
                ));
                self.delivery_service
                    .append_system_context(subscription.session_id(), request)
                    .await
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
            meerkat::JobDeliveryApplication::Event {
                job_id,
                delivery_sequence,
                subscription,
                interaction_lineage_id,
                handling_mode,
                content,
            } => {
                let runtime_adapter = self
                    .delivery_service
                    .runtime_adapter()
                    .ok_or_else(|| {
                        format!(
                            "session {} has no runtime-owned durable event ingress for detached job {job_id}",
                            subscription.session_id()
                        )
                    })?;
                let input = callback_job_event_input(
                    &job_id,
                    delivery_sequence,
                    &subscription,
                    &interaction_lineage_id,
                    handling_mode,
                    &content,
                );
                meerkat_runtime::SessionServiceRuntimeExt::accept_input(
                    runtime_adapter.as_ref(),
                    subscription.session_id(),
                    input,
                )
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
            }
        }
    }
}

fn callback_job_event_input(
    job_id: &meerkat::JobId,
    delivery_sequence: u64,
    subscription: &meerkat::JobSubscription,
    interaction_lineage_id: &meerkat::InteractionLineageId,
    handling_mode: meerkat_core::HandlingMode,
    content: &meerkat::JobDeliveryContent,
) -> meerkat_runtime::Input {
    let event_type = match content {
        meerkat::JobDeliveryContent::Notification(_) => "job.notification",
        meerkat::JobDeliveryContent::Terminal(_) => "job.terminal",
    };
    let content_value = match content {
        meerkat::JobDeliveryContent::Notification(notification) => json!({
            "kind": "notification",
            "notification": notification,
        }),
        meerkat::JobDeliveryContent::Terminal(result) => json!({
            "kind": "terminal",
            "result": result,
        }),
    };
    let correlation_id = uuid::Uuid::parse_str(interaction_lineage_id.as_str())
        .ok()
        .map(meerkat_runtime::CorrelationId::from_uuid);
    let idempotency_key = format!(
        "job:{job_id}:{delivery_sequence}:{}",
        subscription.subscription_id()
    );
    meerkat_runtime::Input::ExternalEvent(meerkat_runtime::ExternalEventInput {
        objective_id: None,
        header: meerkat_runtime::InputHeader {
            id: meerkat_core::lifecycle::InputId::new(),
            timestamp: chrono::Utc::now(),
            source: meerkat_runtime::InputOrigin::External {
                source_name: event_type.to_string(),
            },
            durability: meerkat_runtime::InputDurability::Durable,
            visibility: meerkat_runtime::InputVisibility::default(),
            idempotency_key: Some(meerkat_runtime::IdempotencyKey::new(idempotency_key)),
            supersession_key: None,
            correlation_id,
        },
        event_type: event_type.to_string(),
        payload: json!({
            "job_id": job_id.to_string(),
            "delivery_sequence": delivery_sequence,
            "content": content_value,
        }),
        blocks: None,
        handling_mode,
        render_metadata: None,
    })
}

fn callback_job_delivery_text(
    job_id: &meerkat::JobId,
    content: &meerkat::JobDeliveryContent,
) -> String {
    match content {
        meerkat::JobDeliveryContent::Notification(notification) => format!(
            "Detached job {job_id}: {}\n\n{}",
            notification.title(),
            notification.body()
        ),
        meerkat::JobDeliveryContent::Terminal(result) => {
            format!("Detached job {job_id} reached terminal state: {result:?}")
        }
    }
}

impl CallbackToolSpec {
    fn parse(value: &Value) -> Result<Self, String> {
        if let Some(name) = value.as_str() {
            return Ok(Self {
                name: name.to_string(),
                description: None,
                input_schema: None,
                execution: ToolExecutionContract::default(),
            });
        }
        let Some(object) = value.as_object() else {
            return Err(format!(
                "tools entries must be strings or {{name, description?, input_schema?}} \
                 objects, got: {value}"
            ));
        };
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("tool object requires a non-empty string name, got: {value}"))?
            .to_string();
        let description = match object.get("description") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.clone()),
            Some(other) => {
                return Err(format!(
                    "tool '{name}' description must be a string, got: {other}"
                ));
            }
        };
        let input_schema = match object.get("input_schema") {
            None | Some(Value::Null) => None,
            Some(schema @ Value::Object(_)) => Some(schema.clone()),
            Some(other) => {
                return Err(format!(
                    "tool '{name}' input_schema must be a JSON object, got: {other}"
                ));
            }
        };
        let execution = object
            .get("execution")
            .cloned()
            .map(serde_json::from_value::<meerkat_contracts::CallbackToolExecution>)
            .transpose()
            .map_err(|error| format!("tool '{name}' execution is invalid: {error}"))?
            .map_or_else(
                || Ok(ToolExecutionContract::default()),
                callback_execution_contract,
            )?;
        Ok(Self {
            name,
            description,
            input_schema,
            execution,
        })
    }
}

fn callback_execution_contract(
    execution: meerkat_contracts::CallbackToolExecution,
) -> Result<ToolExecutionContract, String> {
    match execution {
        meerkat_contracts::CallbackToolExecution::Fast => Ok(ToolExecutionContract::default()),
        meerkat_contracts::CallbackToolExecution::Detached {
            runner,
            restart_class,
            idempotency_scope,
            submission_timeout_ms,
            credential_scopes,
        } => {
            let runner = meerkat_core::RunnerIdentity::new(runner.name, runner.version)
                .map_err(|error| error.to_string())?;
            let restart_class = match restart_class {
                meerkat_contracts::JobRestartClass::Adoptable => {
                    meerkat_core::RestartClass::Adoptable
                }
                meerkat_contracts::JobRestartClass::CheckpointResumable => {
                    meerkat_core::RestartClass::CheckpointResumable
                }
                meerkat_contracts::JobRestartClass::Replayable => {
                    meerkat_core::RestartClass::Replayable
                }
                meerkat_contracts::JobRestartClass::NonResumable => {
                    meerkat_core::RestartClass::NonResumable
                }
            };
            let idempotency_scope = match idempotency_scope {
                meerkat_contracts::JobIdempotencyScope::ToolCall => {
                    meerkat_core::IdempotencyScope::ToolCall
                }
                meerkat_contracts::JobIdempotencyScope::InteractionAndArguments => {
                    meerkat_core::IdempotencyScope::InteractionAndArguments
                }
                meerkat_contracts::JobIdempotencyScope::HostSemanticKey => {
                    meerkat_core::IdempotencyScope::HostSemanticKey
                }
            };
            let policy = meerkat_core::DetachedToolExecutionPolicy::new(
                runner,
                restart_class,
                idempotency_scope,
                Duration::from_millis(submission_timeout_ms),
            )
            .map_err(|error| error.to_string())?
            .with_credential_scopes(credential_scopes);
            ToolExecutionContract::new(
                std::collections::BTreeSet::from([ToolExecutionMode::Detached]),
                ToolExecutionMode::Detached,
                None,
                Some(policy),
            )
            .map_err(|error| error.to_string())
        }
    }
}

/// Tool dispatcher that routes tool calls to Python via the callback bridge.
///
/// Created from the tool specs the SDK returned from `callback/build_agent`
/// (`add_tools()` names or `register_tool()` name + description + schema).
/// When the agent calls a tool, `dispatch()` sends `callback/call_tool` to
/// Python and returns the result.
#[derive(Clone)]
struct CallbackToolDispatcher {
    bridge: StdioCallbackBridge,
    scope_id: String,
    tool_defs: Arc<[Arc<ToolDef>]>,
    tool_catalog: Arc<[ToolCatalogEntry]>,
    detached_jobs: Option<DetachedCallbackJobRuntime>,
    reconcile_registered_catalog: bool,
}

impl CallbackToolDispatcher {
    fn new(
        bridge: StdioCallbackBridge,
        scope_id: String,
        tools: Vec<CallbackToolSpec>,
        detached_jobs: Option<DetachedCallbackJobRuntime>,
    ) -> Self {
        let entries: Vec<(Arc<ToolDef>, ToolExecutionContract)> = tools
            .into_iter()
            .map(|tool| {
                (
                    Arc::new(ToolDef {
                        name: tool.name.into(),
                        description: tool
                            .description
                            .unwrap_or_else(|| "Python callback tool".to_string()),
                        input_schema: tool
                            .input_schema
                            .unwrap_or_else(|| json!({"type": "object"})),
                        provenance: None,
                    }),
                    tool.execution,
                )
            })
            .collect();
        let tool_defs = entries
            .iter()
            .map(|(tool, _)| Arc::clone(tool))
            .collect::<Vec<_>>();
        let tool_catalog = entries
            .into_iter()
            .map(|(tool, execution)| {
                ToolCatalogEntry::session_inline(tool, true).with_execution_contract(execution)
            })
            .collect::<Vec<_>>();
        let reconcile_registered_catalog = detached_jobs
            .as_ref()
            .is_some_and(|runtime| runtime.register_callback_catalog(&tool_catalog));
        Self {
            bridge,
            scope_id,
            tool_defs: tool_defs.into(),
            tool_catalog: tool_catalog.into(),
            detached_jobs,
            reconcile_registered_catalog,
        }
    }

    async fn submit_detached(
        &self,
        call: ToolCallView<'_>,
        context: &meerkat_core::ToolDispatchContext,
        plan: &meerkat_core::ResolvedToolExecutionPlan,
    ) -> Result<ToolDispatchOutcome, ToolError> {
        let runtime = self.detached_jobs.clone().ok_or_else(|| {
            ToolError::unavailable(
                call.name,
                meerkat_core::ToolUnavailableReason::ExecutionModeOwnerUnavailable,
            )
        })?;
        let origin_session_id = context.origin_session_id().cloned().ok_or_else(|| {
            ToolError::execution_failed(
                "detached callback dispatch requires runtime-owned session identity".to_string(),
            )
        })?;
        let interaction_lineage = context.interaction_lineage_id().ok_or_else(|| {
            ToolError::execution_failed(
                "detached callback dispatch requires runtime-owned interaction lineage".to_string(),
            )
        })?;
        let arguments_sha256 = plan.canonical_arguments_sha256().ok_or_else(|| {
            ToolError::execution_failed(
                "detached callback dispatch requires a root-fenced canonical argument digest"
                    .to_string(),
            )
        })?;
        let policy = match plan.kind() {
            meerkat_core::ResolvedExecutionKind::Detached(policy) => policy,
            _ => {
                return Err(ToolError::execution_failed(
                    "detached callback owner received a non-detached execution plan".to_string(),
                ));
            }
        };
        let arguments: Value = serde_json::from_str(call.args.get())
            .map_err(|error| ToolError::invalid_arguments(call.name, error.to_string()))?;
        let arguments_hash = callback_sha256(arguments_sha256);
        let lineage = interaction_lineage.to_string();
        let submission_key = callback_submission_key(
            &runtime.realm_id,
            &origin_session_id,
            &lineage,
            call,
            policy,
            &arguments_hash,
            &arguments,
        )?;
        let specification = runtime
            .blob_store
            .put_artifact(
                "application/vnd.meerkat.callback-arguments+json",
                call.args.get(),
            )
            .await
            .map_err(|error| {
                ToolError::execution_failed(format!(
                    "failed to persist detached callback specification: {error}"
                ))
            })?;
        let spec = meerkat::JobSpec::new(
            runtime.realm_id.clone(),
            origin_session_id,
            meerkat::ExecutionIntentId::from_string(format!(
                "intent:{lineage}:{}:{arguments_hash}",
                call.name
            ))
            .map_err(|error| ToolError::execution_failed(error.to_string()))?,
            meerkat::InteractionLineageId::from_string(lineage)
                .map_err(|error| ToolError::execution_failed(error.to_string()))?,
            meerkat::ToolIdentity::new(call.name, policy.runner().version())
                .map_err(|error| ToolError::execution_failed(error.to_string()))?,
            meerkat::RunnerIdentity::new(policy.runner().name(), policy.runner().version())
                .map_err(|error| ToolError::execution_failed(error.to_string()))?,
            callback_job_restart_class(policy.restart_class()),
            meerkat::CanonicalArgumentsHash::new(arguments_hash)
                .map_err(|error| ToolError::execution_failed(error.to_string()))?,
            meerkat::JobSubmissionKey::new(submission_key)
                .map_err(|error| ToolError::execution_failed(error.to_string()))?,
        )
        .with_runner_specification_ref(
            meerkat::RunnerSpecificationRef::new(specification.blob_id.to_string())
                .map_err(|error| ToolError::execution_failed(error.to_string()))?,
        )
        .with_credential_context_refs(match plan.credential_context_refs() {
            meerkat_core::ToolExecutionApplicability::Applicable(references) => references.clone(),
            meerkat_core::ToolExecutionApplicability::NotApplicable => Vec::new(),
        });
        let receipt = runtime
            .service
            .submit(spec)
            .await
            .map_err(|error| ToolError::execution_failed(error.to_string()))?;
        let projected = meerkat::project_job_receipt(receipt.clone());
        let bridge = self.bridge.clone();
        tokio::spawn(async move {
            if let Err(error) =
                start_detached_callback_attempt(runtime, bridge, receipt.job_id).await
            {
                tracing::warn!(%error, "detached callback attempt start did not complete");
            }
        });
        let content = serde_json::to_string(&projected).map_err(|error| {
            ToolError::execution_failed(format!("failed to encode detached job receipt: {error}"))
        })?;
        Ok(ToolResult::new(call.id.to_string(), content, false).into())
    }

    fn owns_detached_callback_spec(&self, spec: &meerkat::JobSpec) -> bool {
        self.tool_catalog.iter().any(|entry| {
            entry.tool.name == spec.tool.name()
                && entry.execution.detached_policy().is_some_and(|policy| {
                    policy.runner().name() == spec.runner.name()
                        && policy.runner().version() == spec.runner.version()
                })
        })
    }

    /// Rehydrate exact committed authority. Offering an attempt to the host
    /// never claims, retries, or advances its fence.
    async fn reconcile_detached_jobs(&self) -> Result<(), String> {
        let Some(runtime) = self.detached_jobs.clone() else {
            return Ok(());
        };
        let jobs = runtime.store.list_all(usize::MAX).await.map_err(|error| {
            format!("failed to enumerate detached callbacks for reconciliation: {error}")
        })?;
        let mut attempts = Vec::new();
        for job in jobs.into_iter().filter(|job| {
            job.spec.realm_id == runtime.realm_id && self.owns_detached_callback_spec(&job.spec)
        }) {
            if matches!(
                job.machine_state.lifecycle_phase,
                meerkat::JobPhase::Running | meerkat::JobPhase::WaitingExternal
            ) {
                let attempt_id =
                    job.machine_state
                        .current_attempt_id
                        .as_deref()
                        .ok_or_else(|| {
                            format!(
                                "active detached callback {} has no committed attempt id",
                                job.job_id
                            )
                        })?;
                let runner_handle =
                    job.machine_state.runner_handle.as_deref().ok_or_else(|| {
                        format!(
                            "active detached callback {} has no committed runner handle",
                            job.job_id
                        )
                    })?;
                let lease_expires_at_ms =
                    job.machine_state.lease_expires_at_ms.ok_or_else(|| {
                        format!(
                            "active detached callback {} has no committed lease",
                            job.job_id
                        )
                    })?;
                attempts.push(meerkat_contracts::CallbackJobReconcileAttempt {
                    authority: meerkat_contracts::JobAttemptAuthority {
                        job_id: job.job_id.to_string(),
                        attempt_id: attempt_id.to_string(),
                        fence: job.machine_state.current_fence,
                    },
                    runner: meerkat_contracts::JobRunner {
                        name: job.spec.runner.name().to_string(),
                        version: job.spec.runner.version().to_string(),
                    },
                    restart_class: callback_wire_restart_class(job.spec.restart_class),
                    runner_handle: runner_handle.to_string(),
                    checkpoint_ref: job
                        .machine_state
                        .checkpoint_ref
                        .as_ref()
                        .map(ToString::to_string),
                    lease_expires_at_ms,
                });
            }
        }
        if !attempts.is_empty() {
            let offered_attempts = attempts.clone();
            let offered = attempts
                .iter()
                .map(|attempt| attempt.authority.clone())
                .collect::<Vec<_>>();
            let result: meerkat_contracts::CallbackJobReconcileResult = serde_json::from_value(
                self.bridge
                    .call(
                        "callback/job/reconcile",
                        serde_json::to_value(meerkat_contracts::CallbackJobReconcileParams {
                            attempts,
                        })
                        .map_err(|error| error.to_string())?,
                    )
                    .await?,
            )
            .map_err(|error| error.to_string())?;
            if result
                .live_attempts
                .iter()
                .any(|authority| !offered.contains(authority))
            {
                return Err(
                    "callback/job/reconcile returned authority that was not offered".to_string(),
                );
            }
            let now_ms = callback_unix_time_ms()?;
            for attempt in offered_attempts.iter().filter(|attempt| {
                attempt.lease_expires_at_ms <= now_ms
                    && !result.live_attempts.contains(&attempt.authority)
            }) {
                let job_id =
                    meerkat::JobId::new(&attempt.authority.job_id).map_err(|e| e.to_string())?;
                let write = meerkat::AttemptWriteAuthority {
                    attempt_id: meerkat::AttemptId::new(&attempt.authority.attempt_id)
                        .map_err(|e| e.to_string())?,
                    fence: meerkat::FenceToken::new(attempt.authority.fence),
                };
                match runtime
                    .service
                    .observe_lease_expired(&job_id, write, now_ms)
                    .await
                {
                    Ok(_) => {}
                    Err(
                        meerkat::DetachedJobError::StaleRevision { .. }
                        | meerkat::DetachedJobError::InvalidTransition { .. },
                    ) => continue,
                    Err(error) => return Err(error.to_string()),
                }
                match attempt.restart_class {
                    meerkat_contracts::JobRestartClass::NonResumable => {
                        runtime
                            .service
                            .classify_worker_loss(&job_id, now_ms)
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                    meerkat_contracts::JobRestartClass::CheckpointResumable
                        if attempt.checkpoint_ref.is_none() =>
                    {
                        runtime
                            .service
                            .mark_needs_attention(
                                &job_id,
                                now_ms,
                                meerkat::JobFailureCode::new(
                                    "checkpoint_resume_missing_checkpoint",
                                )
                                .map_err(|error| error.to_string())?,
                            )
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                    meerkat_contracts::JobRestartClass::Adoptable
                    | meerkat_contracts::JobRestartClass::CheckpointResumable
                    | meerkat_contracts::JobRestartClass::Replayable => {
                        runtime
                            .service
                            .schedule_retry(&job_id, now_ms)
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                }
            }
            for attempt in offered_attempts.iter().filter(|attempt| {
                attempt.lease_expires_at_ms > now_ms
                    || result.live_attempts.contains(&attempt.authority)
            }) {
                let runtime = runtime.clone();
                let bridge = self.bridge.clone();
                let authority = attempt.authority.clone();
                spawn_callback_lease_tracker(runtime, bridge, authority);
            }
        }
        self.start_due_detached_jobs().await
    }

    async fn start_due_detached_jobs(&self) -> Result<(), String> {
        let Some(runtime) = self.detached_jobs.clone() else {
            return Ok(());
        };
        let now_ms = callback_unix_time_ms()?;
        let jobs = runtime.store.list_all(usize::MAX).await.map_err(|error| {
            format!("failed to enumerate detached callbacks for runnable work: {error}")
        })?;
        for job in jobs.into_iter().filter(|job| {
            job.spec.realm_id == runtime.realm_id
                && self.owns_detached_callback_spec(&job.spec)
                && (job.machine_state.lifecycle_phase == meerkat::JobPhase::Queued
                    || job.machine_state.lifecycle_phase == meerkat::JobPhase::RetryScheduled)
        }) {
            let runtime = runtime.clone();
            let bridge = self.bridge.clone();
            let delay_ms = job
                .machine_state
                .retry_due_at_ms
                .unwrap_or(now_ms)
                .saturating_sub(now_ms);
            tokio::spawn(async move {
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                if let Err(error) =
                    start_detached_callback_attempt(runtime, bridge, job.job_id).await
                {
                    tracing::warn!(%error, "runnable detached callback start did not complete");
                }
            });
        }
        Ok(())
    }
}

fn spawn_callback_lease_tracker(
    runtime: DetachedCallbackJobRuntime,
    bridge: StdioCallbackBridge,
    authority: meerkat_contracts::JobAttemptAuthority,
) {
    tokio::spawn(async move {
        if let Err(error) = reconcile_missing_callback_after_lease(runtime, bridge, authority).await
        {
            tracing::warn!(%error, "detached callback lease tracking failed");
        }
    });
}

async fn reconcile_missing_callback_after_lease(
    runtime: DetachedCallbackJobRuntime,
    bridge: StdioCallbackBridge,
    authority: meerkat_contracts::JobAttemptAuthority,
) -> Result<(), String> {
    let job_id = meerkat::JobId::new(&authority.job_id).map_err(|error| error.to_string())?;
    loop {
        let stored = runtime
            .store
            .get(&job_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("detached callback {job_id} disappeared before lease expiry"))?;
        let exact_attempt = stored.machine_state.current_attempt_id.as_deref()
            == Some(authority.attempt_id.as_str())
            && stored.machine_state.current_fence == authority.fence
            && matches!(
                stored.machine_state.lifecycle_phase,
                meerkat::JobPhase::Running | meerkat::JobPhase::WaitingExternal
            );
        if !exact_attempt {
            return Ok(());
        }
        let lease_expires_at_ms = stored
            .machine_state
            .lease_expires_at_ms
            .ok_or_else(|| format!("active detached callback {job_id} has no committed lease"))?;
        let now_ms = callback_unix_time_ms()?;
        if lease_expires_at_ms > now_ms {
            tokio::time::sleep(Duration::from_millis(
                lease_expires_at_ms.saturating_sub(now_ms).saturating_add(1),
            ))
            .await;
            // Heartbeats may have extended the exact committed lease and
            // checkpoint while this task slept. Rehydrate again before
            // offering recovery so reopen never regresses durable authority.
            continue;
        }

        let offered = meerkat_contracts::CallbackJobReconcileAttempt {
            authority: authority.clone(),
            runner: meerkat_contracts::JobRunner {
                name: stored.spec.runner.name().to_string(),
                version: stored.spec.runner.version().to_string(),
            },
            restart_class: callback_wire_restart_class(stored.spec.restart_class),
            runner_handle: stored
                .machine_state
                .runner_handle
                .as_ref()
                .ok_or_else(|| {
                    format!("active detached callback {job_id} has no committed runner handle")
                })?
                .clone(),
            checkpoint_ref: stored
                .machine_state
                .checkpoint_ref
                .as_ref()
                .map(ToString::to_string),
            lease_expires_at_ms,
        };
        let _live_attempts = match bridge
            .call(
                "callback/job/reconcile",
                serde_json::to_value(meerkat_contracts::CallbackJobReconcileParams {
                    attempts: vec![offered],
                })
                .map_err(|error| error.to_string())?,
            )
            .await
        {
            Ok(value) => {
                let result: meerkat_contracts::CallbackJobReconcileResult =
                    serde_json::from_value(value).map_err(|error| error.to_string())?;
                if result
                    .live_attempts
                    .iter()
                    .any(|candidate| candidate != &authority)
                {
                    return Err(
                        "callback/job/reconcile returned authority that was not offered"
                            .to_string(),
                    );
                }
                result.live_attempts
            }
            // Once the committed lease has elapsed, an unavailable host is a
            // missing worker observation. The generated machine still decides
            // whether this becomes loss, retry, or needs-attention.
            Err(error) => {
                tracing::warn!(%error, %job_id, "host unavailable at detached callback lease boundary");
                Vec::new()
            }
        };
        let current = runtime
            .store
            .get(&job_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("detached callback {job_id} disappeared during reconcile"))?;
        let still_exact = current.machine_state.current_attempt_id.as_deref()
            == Some(authority.attempt_id.as_str())
            && current.machine_state.current_fence == authority.fence
            && matches!(
                current.machine_state.lifecycle_phase,
                meerkat::JobPhase::Running | meerkat::JobPhase::WaitingExternal
            );
        if !still_exact {
            return Ok(());
        }
        let current_lease = current
            .machine_state
            .lease_expires_at_ms
            .ok_or_else(|| format!("active detached callback {job_id} has no committed lease"))?;
        let now_ms = callback_unix_time_ms()?;
        if current_lease > now_ms {
            continue;
        }
        let write = meerkat::AttemptWriteAuthority {
            attempt_id: meerkat::AttemptId::new(&authority.attempt_id)
                .map_err(|error| error.to_string())?,
            fence: meerkat::FenceToken::new(authority.fence),
        };
        match runtime
            .service
            .observe_lease_expired(&job_id, write, now_ms)
            .await
        {
            Ok(_) => {}
            Err(
                meerkat::DetachedJobError::StaleRevision { .. }
                | meerkat::DetachedJobError::InvalidTransition { .. },
            ) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        }
        let retry = match current.spec.restart_class {
            meerkat::RestartClass::NonResumable => {
                runtime
                    .service
                    .classify_worker_loss(&job_id, now_ms)
                    .await
                    .map_err(|error| error.to_string())?;
                false
            }
            meerkat::RestartClass::CheckpointResumable
                if current.machine_state.checkpoint_ref.is_none() =>
            {
                runtime
                    .service
                    .mark_needs_attention(
                        &job_id,
                        now_ms,
                        meerkat::JobFailureCode::new("checkpoint_resume_missing_checkpoint")
                            .map_err(|error| error.to_string())?,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                false
            }
            meerkat::RestartClass::Adoptable
            | meerkat::RestartClass::CheckpointResumable
            | meerkat::RestartClass::Replayable => {
                runtime
                    .service
                    .schedule_retry(&job_id, now_ms)
                    .await
                    .map_err(|error| error.to_string())?;
                true
            }
        };
        if retry {
            start_detached_callback_attempt(runtime, bridge, job_id).await?;
        }
        return Ok(());
    }
}

#[async_trait]
impl AgentToolDispatcher for CallbackToolDispatcher {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        Arc::clone(&self.tool_defs)
    }

    fn tool_catalog_capabilities(&self) -> ToolCatalogCapabilities {
        ToolCatalogCapabilities {
            exact_catalog: true,
            may_require_catalog_control_plane: false,
        }
    }

    fn tool_catalog(&self) -> Arc<[ToolCatalogEntry]> {
        Arc::clone(&self.tool_catalog)
    }

    fn resolve_execution_plan(
        &self,
        call: ToolCallView<'_>,
        _dispatch_context: &meerkat_core::ToolDispatchContext,
        resolution_context: &meerkat_core::ToolExecutionResolutionContext,
    ) -> Result<meerkat_core::ResolvedToolExecutionPlan, meerkat_core::ToolExecutionResolutionError>
    {
        let entry = self
            .tool_catalog
            .iter()
            .find(|entry| entry.tool.name == call.name)
            .ok_or_else(|| meerkat_core::ToolExecutionResolutionError::NotFound {
                tool_name: call.name.to_string(),
            })?;
        let resolution_context =
            resolution_context.with_deadline(ToolDeadlineContributor::finite(
                ToolDeadlineOwner::ToolInternal,
                Duration::from_mins(2),
            ))?;
        entry
            .execution
            .resolve_default(resolution_context.deadlines().clone())
            .map_err(Into::into)
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        let args: Value =
            serde_json::from_str(call.args.get()).map_err(|e| ToolError::InvalidArguments {
                name: call.name.to_string(),
                reason: e.to_string(),
            })?;
        let params = json!({
            "scope_id": self.scope_id,
            "tool": call.name,
            "arguments": args,
        });
        match self.bridge.call("callback/call_tool", params).await {
            Ok(result) => Ok(ToolResult {
                tool_use_id: call.id.to_string(),
                content:
                    meerkat_mobkit::identity_first::gateway_bridges::callback_result_to_content(
                        &result,
                    ),
                is_error: false,
            }
            .into()),
            Err(err) => Ok(ToolResult {
                tool_use_id: call.id.to_string(),
                content: vec![ContentBlock::Text {
                    text: format!("Tool execution failed: {err}"),
                }],
                is_error: true,
            }
            .into()),
        }
    }

    async fn dispatch_resolved_with_context(
        &self,
        call: ToolCallView<'_>,
        context: &meerkat_core::ToolDispatchContext,
        plan: &meerkat_core::ResolvedToolExecutionPlan,
    ) -> Result<ToolDispatchOutcome, ToolError> {
        match plan.mode() {
            ToolExecutionMode::Fast => self.dispatch(call).await,
            ToolExecutionMode::Detached => self.submit_detached(call, context, plan).await,
            ToolExecutionMode::Streaming => Err(ToolError::unavailable(
                call.name,
                meerkat_core::ToolUnavailableReason::ExecutionModeOwnerUnavailable,
            )),
        }
    }
}

fn callback_sha256(digest: [u8; 32]) -> String {
    let mut encoded = String::with_capacity("sha256:".len() + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn callback_submission_key(
    realm_id: &str,
    origin_session_id: &meerkat_core::SessionId,
    interaction_lineage: &str,
    call: ToolCallView<'_>,
    policy: &meerkat_core::DetachedToolExecutionPolicy,
    arguments_hash: &str,
    arguments: &Value,
) -> Result<String, ToolError> {
    let scope = match policy.idempotency_scope() {
        meerkat_core::IdempotencyScope::ToolCall => format!("tool-call:{}", call.id),
        meerkat_core::IdempotencyScope::InteractionAndArguments => {
            format!("interaction:{interaction_lineage}:arguments:{arguments_hash}")
        }
        meerkat_core::IdempotencyScope::HostSemanticKey => {
            let semantic_key = arguments
                .get("idempotency_key")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    ToolError::invalid_arguments(
                        call.name,
                        "host_semantic_key execution requires a non-empty string idempotency_key",
                    )
                })?;
            format!(
                "host-semantic:sha256:{:x}",
                Sha256::digest(semantic_key.as_bytes())
            )
        }
    };
    Ok(format!(
        "callback:{realm_id}:{origin_session_id}:{}:{}:{}:{scope}",
        call.name,
        policy.runner().name(),
        policy.runner().version(),
    ))
}

fn callback_job_restart_class(class: meerkat_core::RestartClass) -> meerkat::RestartClass {
    match class {
        meerkat_core::RestartClass::Adoptable => meerkat::RestartClass::Adoptable,
        meerkat_core::RestartClass::CheckpointResumable => {
            meerkat::RestartClass::CheckpointResumable
        }
        meerkat_core::RestartClass::Replayable => meerkat::RestartClass::Replayable,
        meerkat_core::RestartClass::NonResumable => meerkat::RestartClass::NonResumable,
    }
}

fn callback_wire_restart_class(class: meerkat::RestartClass) -> meerkat_contracts::JobRestartClass {
    match class {
        meerkat::RestartClass::Adoptable => meerkat_contracts::JobRestartClass::Adoptable,
        meerkat::RestartClass::CheckpointResumable => {
            meerkat_contracts::JobRestartClass::CheckpointResumable
        }
        meerkat::RestartClass::Replayable => meerkat_contracts::JobRestartClass::Replayable,
        meerkat::RestartClass::NonResumable => meerkat_contracts::JobRestartClass::NonResumable,
    }
}

async fn start_detached_callback_attempt(
    runtime: DetachedCallbackJobRuntime,
    bridge: StdioCallbackBridge,
    job_id: meerkat::JobId,
) -> Result<(), String> {
    let stored = runtime
        .store
        .get(&job_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("detached callback job {job_id} disappeared after submission"))?;
    let claimed_at_ms = callback_unix_time_ms()?;
    let runnable = stored.machine_state.lifecycle_phase == meerkat::JobPhase::Queued
        || (stored.machine_state.lifecycle_phase == meerkat::JobPhase::RetryScheduled
            && stored
                .machine_state
                .retry_due_at_ms
                .is_some_and(|due_at_ms| claimed_at_ms >= due_at_ms));
    if !runnable {
        return Ok(());
    }
    let lease_expires_at_ms = claimed_at_ms
        .checked_add(120_000)
        .ok_or_else(|| "detached callback lease timestamp overflowed".to_string())?;
    let runner_handle = format!(
        "callback:{job_id}:attempt:{}",
        stored.machine_state.attempt_count.saturating_add(1)
    );
    let claim = match runtime
        .service
        .claim_attempt(
            &job_id,
            meerkat::AttemptClaim::new(
                meerkat::WorkerId::new(format!("mobkit-callback:{}", std::process::id()))
                    .map_err(|error| error.to_string())?,
                claimed_at_ms,
                lease_expires_at_ms,
                meerkat::RunnerHandleRef::new(runner_handle.clone())
                    .map_err(|error| error.to_string())?,
            ),
        )
        .await
    {
        Ok(claim) => claim,
        Err(
            meerkat::DetachedJobError::StaleRevision { .. }
            | meerkat::DetachedJobError::InvalidTransition { .. },
        ) => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let specification_ref = stored
        .spec
        .runner_specification_ref
        .as_ref()
        .ok_or_else(|| format!("detached callback job {job_id} has no runner specification"))?;
    let specification = runtime
        .blob_store
        .get(&meerkat_core::BlobId::new(specification_ref.as_str()))
        .await
        .map_err(|error| error.to_string())?;
    let arguments: Value =
        serde_json::from_str(&specification.data).map_err(|error| error.to_string())?;
    let credential_scopes = stored
        .spec
        .credential_context_refs
        .iter()
        .flat_map(|reference| match reference {
            meerkat_core::ToolCredentialContextRef::OwningProfile { required_scopes }
            | meerkat_core::ToolCredentialContextRef::AuthBinding {
                required_scopes, ..
            } => required_scopes.iter().cloned().collect::<Vec<_>>(),
        })
        .collect();
    let params = meerkat_contracts::CallbackJobStartParams {
        authority: meerkat_contracts::JobAttemptAuthority {
            job_id: job_id.to_string(),
            attempt_id: claim.attempt_id.to_string(),
            fence: claim.fence.get(),
        },
        runner: meerkat_contracts::JobRunner {
            name: stored.spec.runner.name().to_string(),
            version: stored.spec.runner.version().to_string(),
        },
        restart_class: callback_wire_restart_class(stored.spec.restart_class),
        runner_handle: runner_handle.clone(),
        runner_specification_ref: Some(specification_ref.to_string()),
        arguments,
        credential_scopes,
        resume_checkpoint: claim.resume_checkpoint.as_ref().map(ToString::to_string),
    };
    spawn_callback_lease_tracker(runtime.clone(), bridge.clone(), params.authority.clone());
    let result: meerkat_contracts::CallbackJobStartResult = serde_json::from_value(
        bridge
            .call(
                "callback/job/start",
                serde_json::to_value(params).map_err(|error| error.to_string())?,
            )
            .await?,
    )
    .map_err(|error| error.to_string())?;
    if !result.accepted || result.runner_handle != runner_handle {
        runtime
            .service
            .mark_needs_attention(
                &job_id,
                callback_unix_time_ms().unwrap_or(claimed_at_ms),
                meerkat::JobFailureCode::new(if result.accepted {
                    "callback_runner_handle_mismatch"
                } else {
                    "callback_start_rejected"
                })
                .map_err(|error| error.to_string())?,
            )
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn callback_unix_time_ms() -> Result<u64, String> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    u64::try_from(millis).map_err(|_| "wall clock exceeds u64 milliseconds".to_string())
}

async fn callback_job_description(
    runtime: &DetachedCallbackJobRuntime,
    job_id: &meerkat::JobId,
) -> Result<meerkat::JobDescription, String> {
    runtime
        .service
        .describe_for_realm(&runtime.realm_id, job_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("detached job {job_id} does not exist in this realm"))
}

fn callback_write_authority(
    authority: meerkat_contracts::JobAttemptAuthority,
) -> Result<(meerkat::JobId, meerkat::AttemptWriteAuthority), String> {
    Ok((
        meerkat::JobId::new(authority.job_id).map_err(|error| error.to_string())?,
        meerkat::AttemptWriteAuthority {
            attempt_id: meerkat::AttemptId::new(authority.attempt_id)
                .map_err(|error| error.to_string())?,
            fence: meerkat::FenceToken::new(authority.fence),
        },
    ))
}

fn callback_job_rpc_response(id: Value, result: Result<Value, String>) -> Result<String, String> {
    serde_json::to_string(&match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(message) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": message}})
        }
    })
    .map_err(|error| error.to_string())
}

/// Handle the canonical Meerkat job surface over MobKit's inherited job
/// store. Returns `None` for methods owned by the ordinary MobKit router.
async fn handle_callback_job_rpc(
    request_line: &str,
    runtime: Option<&DetachedCallbackJobRuntime>,
    bridge: &StdioCallbackBridge,
) -> Option<String> {
    const METHODS: &[&str] = &[
        "jobs/get",
        "jobs/list",
        "jobs/cancel",
        "jobs/progress",
        "jobs/result",
        "jobs/artifacts",
        "jobs/retry",
        "jobs/health",
        "jobs/subscribe",
        "jobs/unsubscribe",
        "monitors/start",
        "mobkit/jobs/heartbeat",
        "mobkit/jobs/progress",
        "mobkit/jobs/checkpoint",
        "mobkit/jobs/complete",
        "mobkit/jobs/fail",
        "mobkit/jobs/cancel_ack",
    ];
    let request: Value = serde_json::from_str(request_line).ok()?;
    let method = request.get("method").and_then(Value::as_str)?;
    if !METHODS.contains(&method) {
        return None;
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let Some(runtime) = runtime else {
        return Some(
            callback_job_rpc_response(
                id,
                Err(
                    "durable jobs require persistent_state; semantic detached admission is disabled"
                        .to_string(),
                ),
            )
            .unwrap_or_default(),
        );
    };
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let result: Result<Value, String> = async {
        match method {
            "jobs/get" => {
                let params: meerkat_contracts::JobsGetParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let job_id =
                    meerkat::JobId::new(params.job_id).map_err(|error| error.to_string())?;
                let job = callback_job_description(runtime, &job_id).await?;
                serde_json::to_value(meerkat_contracts::JobsGetResult {
                    job: meerkat::project_job_description(job),
                })
                .map_err(|error| error.to_string())
            }
            "jobs/list" => {
                let params: meerkat_contracts::JobsListParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let session = params
                    .session_id
                    .ok_or_else(|| "jobs/list requires session_id".to_string())
                    .and_then(|raw| {
                        meerkat_core::SessionId::parse(&raw).map_err(|error| error.to_string())
                    })?;
                let limit =
                    usize::try_from(params.limit.unwrap_or(100).min(1_000)).unwrap_or(1_000);
                let jobs = runtime
                    .service
                    .list_descriptions_for_origin(&runtime.realm_id, &session, limit)
                    .await
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .map(meerkat::project_job_description)
                    .collect();
                serde_json::to_value(meerkat_contracts::JobsListResult { jobs })
                    .map_err(|error| error.to_string())
            }
            "jobs/cancel" => {
                let params: meerkat_contracts::JobsCancelParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let job_id =
                    meerkat::JobId::new(params.job_id).map_err(|error| error.to_string())?;
                let stored = runtime
                    .store
                    .get(&job_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .filter(|job| job.spec.realm_id == runtime.realm_id)
                    .ok_or_else(|| format!("detached job {job_id} does not exist in this realm"))?;
                if matches!(
                    stored.spec.runner.name(),
                    "meerkat.shell" | "meerkat.monitor_script"
                ) {
                    let manager = runtime
                        .monitor_manager(&stored.spec.origin_session_id)
                        .await?;
                    manager
                        .cancel_job(&meerkat_tools::builtin::shell::JobId::from_string(
                            job_id.to_string(),
                        ))
                        .await
                        .map_err(|error| error.to_string())?;
                } else {
                    let snapshot = runtime
                        .service
                        .request_cancel(&job_id)
                        .await
                        .map_err(|error| error.to_string())?;
                    if runtime.owns_callback_job(&stored.spec)
                        && let Some(attempt_id) = snapshot.current_attempt_id.as_ref()
                        && matches!(
                            snapshot.phase,
                            meerkat::JobPhase::Running | meerkat::JobPhase::WaitingExternal
                        )
                    {
                        let cancel: meerkat_contracts::CallbackJobCancelResult =
                            serde_json::from_value(
                                bridge
                                    .call(
                                        "callback/job/cancel",
                                        serde_json::to_value(
                                            meerkat_contracts::CallbackJobCancelParams {
                                                authority: meerkat_contracts::JobAttemptAuthority {
                                                    job_id: job_id.to_string(),
                                                    attempt_id: attempt_id.to_string(),
                                                    fence: snapshot.current_fence.get(),
                                                },
                                            },
                                        )
                                        .map_err(|error| error.to_string())?,
                                    )
                                    .await?,
                            )
                            .map_err(|error| error.to_string())?;
                        if !cancel.accepted {
                            return Err(
                                "callback/job/cancel rejected the committed active attempt"
                                    .to_string(),
                            );
                        }
                    }
                }
                let job = callback_job_description(runtime, &job_id).await?;
                serde_json::to_value(meerkat_contracts::JobsCancelResult {
                    job: meerkat::project_job_description(job),
                })
                .map_err(|error| error.to_string())
            }
            "jobs/progress" => {
                let params: meerkat_contracts::JobsProgressParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let job_id =
                    meerkat::JobId::new(params.job_id).map_err(|error| error.to_string())?;
                let job = meerkat::project_job_description(
                    callback_job_description(runtime, &job_id).await?,
                );
                serde_json::to_value(meerkat_contracts::JobsProgressResult {
                    job_id: job.job_id,
                    phase: job.phase,
                    progress: job.progress,
                })
                .map_err(|error| error.to_string())
            }
            "jobs/result" => {
                let params: meerkat_contracts::JobsResultParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let job_id =
                    meerkat::JobId::new(params.job_id).map_err(|error| error.to_string())?;
                let job = meerkat::project_job_description(
                    callback_job_description(runtime, &job_id).await?,
                );
                serde_json::to_value(meerkat_contracts::JobsResultResult {
                    job_id: job.job_id,
                    phase: job.phase,
                    result: job.terminal_result,
                })
                .map_err(|error| error.to_string())
            }
            "jobs/artifacts" => {
                let params: meerkat_contracts::JobsArtifactsParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let job_id =
                    meerkat::JobId::new(params.job_id).map_err(|error| error.to_string())?;
                let job = callback_job_description(runtime, &job_id).await?;
                let reference = match job.terminal_result {
                    Some(
                        meerkat::JobTerminalResult::Succeeded {
                            result_ref: Some(reference),
                        }
                        | meerkat::JobTerminalResult::Failed {
                            detail_ref: Some(reference),
                            ..
                        },
                    ) => Some(reference.to_string()),
                    _ => None,
                };
                serde_json::to_value(meerkat_contracts::JobsArtifactsResult {
                    job_id: job_id.to_string(),
                    artifacts: reference
                        .into_iter()
                        .map(|reference| meerkat_contracts::JobArtifactRef { reference })
                        .collect(),
                })
                .map_err(|error| error.to_string())
            }
            "jobs/retry" => {
                let params: meerkat_contracts::JobsRetryParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let job_id =
                    meerkat::JobId::new(params.job_id).map_err(|error| error.to_string())?;
                let stored = runtime
                    .store
                    .get(&job_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .filter(|job| job.spec.realm_id == runtime.realm_id)
                    .ok_or_else(|| format!("detached job {job_id} does not exist in this realm"))?;
                let shell_owned = matches!(
                    stored.spec.runner.name(),
                    "meerkat.shell" | "meerkat.monitor_script"
                );
                runtime
                    .service
                    .schedule_retry(&job_id, params.retry_due_at_ms)
                    .await
                    .map_err(|error| error.to_string())?;
                let now = callback_unix_time_ms()?;
                let delay_ms = params.retry_due_at_ms.saturating_sub(now);
                if shell_owned {
                    let manager = runtime
                        .monitor_manager(&stored.spec.origin_session_id)
                        .await?;
                    let public_job_id =
                        meerkat_tools::builtin::shell::JobId::from_string(job_id.to_string());
                    tokio::spawn(async move {
                        if delay_ms > 0 {
                            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        }
                        if let Err(error) = manager.get_status(&public_job_id).await {
                            tracing::warn!(%error, "durable shell/monitor retry start failed");
                        }
                    });
                } else if runtime.owns_callback_job(&stored.spec) {
                    let runtime = (*runtime).clone();
                    let bridge = bridge.clone();
                    let retry_job_id = job_id.clone();
                    tokio::spawn(async move {
                        if delay_ms > 0 {
                            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        }
                        if let Err(error) =
                            start_detached_callback_attempt(runtime, bridge, retry_job_id).await
                        {
                            tracing::warn!(%error, "detached callback retry start failed");
                        }
                    });
                } else {
                    // Retry scheduling is machine authority. An unregistered
                    // runner may be claimed by another host; this gateway
                    // simply refrains from impersonating that execution owner.
                }
                let job = callback_job_description(runtime, &job_id).await?;
                serde_json::to_value(meerkat_contracts::JobsRetryResult {
                    job: meerkat::project_job_description(job),
                })
                .map_err(|error| error.to_string())
            }
            "jobs/health" => {
                let health = runtime
                    .service
                    .health_snapshot_for_realm(
                        &runtime.realm_id,
                        callback_unix_time_ms()?,
                        usize::MAX,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                // Separate wedges, deliberately not summed - see the health
                // projection above.
                let runtime_inbox_backlog = runtime.runtime_inbox_backlog_count().await?;
                // Carry meerkat's third rung through to the wire. Folding
                // `Unreadable` into either `Ok` or `Degraded` would publish a
                // verdict the census did not establish.
                let reading = health.reading().worst(if runtime_inbox_backlog > 0 {
                    meerkat::JobHealthReading::Degraded
                } else {
                    meerkat::JobHealthReading::Ok
                });
                serde_json::to_value(meerkat_contracts::JobsHealthResult {
                    detached_jobs: meerkat_contracts::JobHealthSummary {
                        status: match reading {
                            meerkat::JobHealthReading::Ok => meerkat_contracts::JobHealthStatus::Ok,
                            meerkat::JobHealthReading::Degraded => {
                                meerkat_contracts::JobHealthStatus::Degraded
                            }
                            meerkat::JobHealthReading::Unreadable => {
                                meerkat_contracts::JobHealthStatus::Unreadable
                            }
                        },
                        queued: health.queued,
                        running: health.running,
                        awaiting_members: health.awaiting_members,
                        stale_leases: health.stale_leases,
                        needs_attention: health.needs_attention,
                        pending_outbox_jobs: health.pending_outbox_jobs,
                        runtime_inbox_backlog,
                        coverage: match health.coverage {
                            meerkat::JobHealthCoverage::Complete => {
                                meerkat_contracts::JobHealthCoverage::Complete
                            }
                            meerkat::JobHealthCoverage::Truncated { scanned, limit } => {
                                meerkat_contracts::JobHealthCoverage::Truncated { scanned, limit }
                            }
                        },
                    },
                })
                .map_err(|error| error.to_string())
            }
            "jobs/subscribe" => {
                let params: meerkat_contracts::JobsSubscribeParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let job_id =
                    meerkat::JobId::new(params.job_id).map_err(|error| error.to_string())?;
                callback_job_description(runtime, &job_id).await?;
                let delivery = match params.delivery {
                    meerkat_contracts::JobDeliveryKind::Record => meerkat::JobDeliveryKind::Record,
                    meerkat_contracts::JobDeliveryKind::Notification => {
                        meerkat::JobDeliveryKind::Notification
                    }
                    meerkat_contracts::JobDeliveryKind::Event { handling_mode } => {
                        meerkat::JobDeliveryKind::Event {
                            handling_mode: handling_mode.into(),
                        }
                    }
                };
                runtime
                    .service
                    .subscribe(
                        &job_id,
                        meerkat::JobSubscription::new(
                            meerkat::JobSubscriptionId::new(params.subscription_id)
                                .map_err(|error| error.to_string())?,
                            meerkat_core::SessionId::parse(&params.session_id)
                                .map_err(|error| error.to_string())?,
                            delivery,
                        ),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                let job = callback_job_description(runtime, &job_id).await?;
                serde_json::to_value(meerkat_contracts::JobsSubscribeResult {
                    job: meerkat::project_job_description(job),
                })
                .map_err(|error| error.to_string())
            }
            "jobs/unsubscribe" => {
                let params: meerkat_contracts::JobsUnsubscribeParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let job_id =
                    meerkat::JobId::new(params.job_id).map_err(|error| error.to_string())?;
                callback_job_description(runtime, &job_id).await?;
                runtime
                    .service
                    .unsubscribe(
                        &job_id,
                        &meerkat::JobSubscriptionId::new(params.subscription_id)
                            .map_err(|error| error.to_string())?,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                let job = callback_job_description(runtime, &job_id).await?;
                serde_json::to_value(meerkat_contracts::JobsUnsubscribeResult {
                    job: meerkat::project_job_description(job),
                })
                .map_err(|error| error.to_string())
            }
            "monitors/start" => {
                let params: meerkat_contracts::MonitorsStartParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let session_id = meerkat_core::SessionId::parse(&params.session_id)
                    .map_err(|error| error.to_string())?;
                let mut limits = meerkat_tools::builtin::shell::MonitorProtocolLimits::default();
                if let Some(value) = params.max_line_bytes {
                    limits.max_line_bytes = usize::try_from(value)
                        .map_err(|_| "max_line_bytes exceeds this host's size limit".to_string())?;
                }
                if let Some(value) = params.max_notifications_per_window {
                    limits.max_notifications_per_window = usize::try_from(value).map_err(|_| {
                        "max_notifications_per_window exceeds this host's size limit".to_string()
                    })?;
                }
                if let Some(value) = params.notification_window_ms {
                    limits.notification_window_ms = value;
                }
                if let Some(value) = params.max_retained_diagnostic_bytes {
                    limits.max_retained_diagnostic_bytes =
                        usize::try_from(value).map_err(|_| {
                            "max_retained_diagnostic_bytes exceeds this host's size limit"
                                .to_string()
                        })?;
                }
                let protocol = match params.protocol {
                    meerkat_contracts::MonitorOutputProtocol::FramedJsonl => {
                        meerkat_tools::builtin::shell::MonitorOutputProtocol::FramedJsonl
                    }
                    meerkat_contracts::MonitorOutputProtocol::Lines => {
                        meerkat_tools::builtin::shell::MonitorOutputProtocol::Lines
                    }
                };
                let restart_class = match params.restart_class {
                    meerkat_contracts::JobRestartClass::Adoptable => {
                        meerkat::RestartClass::Adoptable
                    }
                    meerkat_contracts::JobRestartClass::CheckpointResumable => {
                        meerkat::RestartClass::CheckpointResumable
                    }
                    meerkat_contracts::JobRestartClass::Replayable => {
                        meerkat::RestartClass::Replayable
                    }
                    meerkat_contracts::JobRestartClass::NonResumable => {
                        meerkat::RestartClass::NonResumable
                    }
                };
                let delivery = match params.delivery {
                    meerkat_contracts::JobDeliveryKind::Record => meerkat::JobDeliveryKind::Record,
                    meerkat_contracts::JobDeliveryKind::Notification => {
                        meerkat::JobDeliveryKind::Notification
                    }
                    meerkat_contracts::JobDeliveryKind::Event { handling_mode } => {
                        meerkat::JobDeliveryKind::Event {
                            handling_mode: handling_mode.into(),
                        }
                    }
                };
                let manager = runtime.monitor_manager(&session_id).await?;
                let job_id = manager
                    .spawn_monitor_for_call(
                        &params.command,
                        params.working_dir.as_deref().map(std::path::Path::new),
                        params.timeout_secs,
                        &params.submission_key,
                        meerkat_tools::builtin::shell::MonitorStartOptions {
                            protocol,
                            restart_class,
                            limits,
                            delivery,
                        },
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                let job_id =
                    meerkat::JobId::new(job_id.to_string()).map_err(|error| error.to_string())?;
                let job = callback_job_description(runtime, &job_id).await?;
                serde_json::to_value(meerkat_contracts::MonitorsStartResult {
                    job: meerkat::project_job_description(job),
                })
                .map_err(|error| error.to_string())
            }
            "mobkit/jobs/heartbeat" => {
                let params: meerkat_contracts::MobkitJobHeartbeatParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let (job_id, write) = callback_write_authority(params.authority)?;
                callback_job_description(runtime, &job_id).await?;
                runtime
                    .service
                    .renew_lease(
                        &job_id,
                        write,
                        params.heartbeat_at_ms,
                        params.lease_expires_at_ms,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                callback_mutation_projection(runtime, &job_id).await
            }
            "mobkit/jobs/progress" => {
                let params: meerkat_contracts::MobkitJobProgressParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let (job_id, write) = callback_write_authority(params.authority)?;
                callback_job_description(runtime, &job_id).await?;
                runtime
                    .service
                    .report_progress(
                        &job_id,
                        write,
                        meerkat::JobProgress::new(params.cursor, params.detail)
                            .map_err(|error| error.to_string())?,
                        params.observed_at_ms,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                callback_mutation_projection(runtime, &job_id).await
            }
            "mobkit/jobs/checkpoint" => {
                let params: meerkat_contracts::MobkitJobCheckpointParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let (job_id, write) = callback_write_authority(params.authority)?;
                callback_job_description(runtime, &job_id).await?;
                runtime
                    .service
                    .record_checkpoint(
                        &job_id,
                        write,
                        meerkat::CheckpointRef::new(params.checkpoint_ref)
                            .map_err(|error| error.to_string())?,
                        params.observed_at_ms,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                callback_mutation_projection(runtime, &job_id).await
            }
            "mobkit/jobs/complete" => {
                let params: meerkat_contracts::MobkitJobCompleteParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let (job_id, write) = callback_write_authority(params.authority)?;
                callback_job_description(runtime, &job_id).await?;
                runtime
                    .service
                    .complete_attempt(
                        &job_id,
                        write,
                        params.completed_at_ms,
                        params
                            .result_ref
                            .map(meerkat::JobResultRef::new)
                            .transpose()
                            .map_err(|error| error.to_string())?,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                callback_mutation_projection(runtime, &job_id).await
            }
            "mobkit/jobs/fail" => {
                let params: meerkat_contracts::MobkitJobFailParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let (job_id, write) = callback_write_authority(params.authority)?;
                callback_job_description(runtime, &job_id).await?;
                runtime
                    .service
                    .fail_attempt(
                        &job_id,
                        write,
                        params.failed_at_ms,
                        meerkat::JobFailureCode::new(params.code)
                            .map_err(|error| error.to_string())?,
                        params
                            .detail_ref
                            .map(meerkat::JobResultRef::new)
                            .transpose()
                            .map_err(|error| error.to_string())?,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                callback_mutation_projection(runtime, &job_id).await
            }
            "mobkit/jobs/cancel_ack" => {
                let params: meerkat_contracts::MobkitJobCancelAckParams =
                    serde_json::from_value(params).map_err(|error| error.to_string())?;
                let (job_id, write) = callback_write_authority(params.authority)?;
                callback_job_description(runtime, &job_id).await?;
                runtime
                    .service
                    .acknowledge_cancel(&job_id, write, params.acknowledged_at_ms)
                    .await
                    .map_err(|error| error.to_string())?;
                callback_mutation_projection(runtime, &job_id).await
            }
            _ => unreachable!("method allowlist and match must stay in sync"),
        }
    }
    .await;
    Some(callback_job_rpc_response(id, result).unwrap_or_default())
}

async fn callback_mutation_projection(
    runtime: &DetachedCallbackJobRuntime,
    job_id: &meerkat::JobId,
) -> Result<Value, String> {
    let job = callback_job_description(runtime, job_id).await?;
    serde_json::to_value(meerkat_contracts::MobkitJobMutationResult {
        job: meerkat::project_job_description(job),
    })
    .map_err(|error| error.to_string())
}

/// Wraps FactoryAgentBuilder — sends callback/build_agent to Python before building.
struct StdioCallbackAgentBuilder {
    inner: FactoryAgentBuilder,
    bridge: StdioCallbackBridge,
    has_session_builder: bool,
    /// Session store for loading sessions by ID when the Python builder
    /// sets `resume_session_id`. Only populated in persistent mode.
    session_store: Option<Arc<dyn meerkat::SessionStore>>,
    detached_jobs: Option<DetachedCallbackJobRuntime>,
}

fn callback_build_agent_options(req: &CreateSessionRequest, scope_id: &str) -> Value {
    let request_labels = req.labels.as_ref();
    let build_labels = req
        .build
        .as_ref()
        .and_then(|build| build.peer_meta.as_ref())
        .map(|meta| &meta.labels);
    let mut labels = BTreeMap::new();
    if let Some(build_labels) = build_labels {
        labels.extend(
            build_labels
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    if let Some(request_labels) = request_labels {
        labels.extend(
            request_labels
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    let labels = (!labels.is_empty()).then_some(labels);
    let profile_name = build_labels
        .and_then(|labels| labels.get("profile_name").or_else(|| labels.get("role")))
        .or_else(|| {
            request_labels
                .and_then(|labels| labels.get("profile_name").or_else(|| labels.get("role")))
        });
    // Mint-vs-resume signal (task #62, HomeCore field ask 2026-08-06): a
    // spawn-level resume is known here via `build.resume_session`, so the
    // host can honor the System-message contract (append standing
    // instructions on MINT, inherit on RESUME) instead of inferring
    // platform state from its own continuity store. `session_id` keeps its
    // label-sourced semantics but falls back to the resumed session's id,
    // which is the identity the older restore-path complaint was missing.
    let resume_session_id = req
        .build
        .as_ref()
        .and_then(|b| b.resume_session.as_ref())
        .map(|s| s.id().to_string());
    json!({
        "scope_id": scope_id,
        "session_id": labels
            .as_ref()
            .and_then(|l| l.get("session_id").cloned())
            .or_else(|| resume_session_id.clone()),
        "profile_name": profile_name,
        "model": &req.model,
        "prompt": &req.prompt,
        "labels": &labels,
        "app_context": req.build.as_ref()
            .and_then(|b| b.app_context.as_ref()),
        "resume_session_id": resume_session_id,
    })
}

#[async_trait]
impl SessionAgentBuilder for StdioCallbackAgentBuilder {
    type Agent = FactoryAgent;

    async fn abort_absent_session_compaction_stages(
        &self,
        session_id: &meerkat_core::SessionId,
    ) -> Result<(), SessionError> {
        // This wrapper only adds SDK callbacks around agent construction. The
        // inner factory still owns the canonical durable-memory backend and
        // therefore the pre-materialization compaction reconciliation seam.
        // Falling back to the trait default here fails every retire/respawn of
        // an already disposed session even though the factory can reconcile it.
        self.inner
            .abort_absent_session_compaction_stages(session_id)
            .await
    }

    async fn build_agent(
        &self,
        req: &CreateSessionRequest,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Self::Agent, SessionError> {
        if !self.has_session_builder {
            let normalized_req = CreateSessionRequest {
                model: req.model.clone(),
                prompt: req.prompt.clone(),
                system_prompt: req.system_prompt.clone(),
                max_tokens: req.max_tokens,
                event_tx: req.event_tx.clone(),
                initial_turn: req.initial_turn.clone(),
                build: req.build.clone(),
                labels: req.labels.clone(),
                deferred_prompt_policy: req.deferred_prompt_policy,
                injected_context: req.injected_context.clone(),
            };
            return self.inner.build_agent(&normalized_req, event_tx).await;
        }

        // Generate a unique scope ID for this build — used to isolate tool
        // handlers between concurrent sessions on the Python side.
        let scope_id = format!(
            "build-{}",
            self.bridge
                .counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );

        // Send callback to Python with full session context.
        // app_context flows from SpawnMemberSpec.context → build.app_context.
        let options = callback_build_agent_options(req, &scope_id);
        let params = json!({ "options": options });
        let callback_result = self.bridge.call("callback/build_agent", params).await;

        match callback_result {
            Ok(result) => {
                // Apply Python-returned options to a cloned request
                let mut modified_req = CreateSessionRequest {
                    model: req.model.clone(),
                    prompt: req.prompt.clone(),
                    system_prompt: req.system_prompt.clone(),
                    max_tokens: req.max_tokens,
                    event_tx: req.event_tx.clone(),
                    initial_turn: req.initial_turn.clone(),
                    build: req.build.clone(),
                    labels: req.labels.clone(),
                    deferred_prompt_policy: req.deferred_prompt_policy,
                    injected_context: req.injected_context.clone(),
                };
                // Resume-ness decides the instruction fold below, so resolve
                // it FIRST: a resume can arrive spawn-level
                // (build.resume_session already loaded) or be requested by
                // the Python response (resume_session_id, applied further
                // down) - both shapes must suppress the fold.
                let resumed = modified_req
                    .build
                    .as_ref()
                    .and_then(|b| b.resume_session.as_ref())
                    .is_some()
                    || result
                        .get("resume_session_id")
                        .and_then(|v| v.as_str())
                        .is_some();
                // Apply additional_instructions through meerkat's NATIVE
                // standing-instructions carrier
                // (SessionBuildOptions.additional_instructions) - on MINT
                // builds only. The old translation folded them into an
                // explicit SystemPromptOverride::Set, a transcript-authoring
                // ruling this layer does not own: an explicit Set at resume
                // IS new transcript intent by contract, so the runtime
                // recorded one assembled System row per boot (the HomeCore
                // accretion: parent-1 reached 1,294,962 tokens against a
                // 922,000 ceiling). Standing instructions are per-session
                // build state baked at mint; a deliberate mid-life change
                // must use the typed transcript admission.
                if let Some(instructions) = result.get("additional_instructions") {
                    if let Some(arr) = instructions.as_array() {
                        let combined: Vec<String> = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(ToString::to_string))
                            .collect();
                        if !combined.is_empty() {
                            if resumed {
                                tracing::warn!(
                                    "callback/build_agent: additional_instructions ignored on a \
                                     RESUMED build (resume inherits persisted prompt state; \
                                     append deliberate System content through the typed \
                                     admission instead)"
                                );
                            } else {
                                let build = modified_req.build.get_or_insert_with(|| {
                                    meerkat_core::service::SessionBuildOptions::default()
                                });
                                // Merge, preserving instructions another
                                // customizer already installed.
                                build
                                    .additional_instructions
                                    .get_or_insert_with(Vec::new)
                                    .extend(combined);
                            }
                        }
                    }
                }
                // Apply labels
                if let Some(labels) = result.get("labels").and_then(|v| v.as_object()) {
                    let label_map = modified_req.labels.get_or_insert_with(Default::default);
                    for (k, v) in labels {
                        if let Some(s) = v.as_str() {
                            label_map.insert(k.clone(), s.to_string());
                        }
                    }
                }
                // Apply resume_session_id: Python builder can request resuming
                // an existing session. The gateway loads the session from the
                // store and sets it on build.resume_session.
                if let Some(resume_id) = result.get("resume_session_id").and_then(|v| v.as_str()) {
                    if let Some(ref store) = self.session_store {
                        let sid =
                            meerkat_core::types::SessionId::parse(resume_id).map_err(|_| {
                                SessionError::Agent(agent_tool_error(format!(
                                    "callback/build_agent: invalid resume_session_id: {resume_id}"
                                )))
                            })?;
                        // Validate against any spawn-level resume already set.
                        if let Some(existing) = modified_req
                            .build
                            .as_ref()
                            .and_then(|b| b.resume_session.as_ref())
                        {
                            if existing.id() != &sid {
                                return Err(SessionError::Agent(agent_tool_error(format!(
                                    "callback/build_agent: resume_session_id conflict: \
                                     spawn set {} but hook set {resume_id}",
                                    existing.id()
                                ))));
                            }
                            // Same ID — already loaded, skip.
                        } else {
                            let session = store.load(&sid).await.map_err(|e| {
                                SessionError::Agent(agent_tool_error(format!(
                                    "callback/build_agent: failed to load resume session {resume_id}: {e}"
                                )))
                            })?;
                            let session = session.ok_or_else(|| {
                                SessionError::Agent(agent_tool_error(format!(
                                    "callback/build_agent: resume session not found: {resume_id}"
                                )))
                            })?;
                            let build = modified_req.build.get_or_insert_with(|| {
                                meerkat_core::service::SessionBuildOptions::default()
                            });
                            build.resume_session = Some(session);
                        }
                    } else {
                        return Err(SessionError::Agent(agent_tool_error(
                            "callback/build_agent: resume_session_id requires persistent mode \
                             (no session store available in ephemeral mode)"
                                .to_string(),
                        )));
                    }
                }
                // Callback tools: Python SDK provides tool names via add_tools()
                // or register_tool(). Create a CallbackToolDispatcher that routes
                // tool calls back to Python via callback/call_tool.
                if let Some(tools) = result.get("tools") {
                    match tools.as_array() {
                        Some(arr) => {
                            let mut tool_specs = Vec::with_capacity(arr.len());
                            for v in arr {
                                let spec = CallbackToolSpec::parse(v).map_err(|reason| {
                                    SessionError::Agent(agent_tool_error(format!(
                                        "callback/build_agent: {reason}"
                                    )))
                                })?;
                                tool_specs.push(spec);
                            }
                            if !tool_specs.is_empty() {
                                let dispatcher = CallbackToolDispatcher::new(
                                    self.bridge.clone(),
                                    scope_id.clone(),
                                    tool_specs,
                                    self.detached_jobs.clone(),
                                );
                                if dispatcher.reconcile_registered_catalog {
                                    let reconciliation = dispatcher.clone();
                                    tokio::spawn(async move {
                                        if let Err(error) =
                                            reconciliation.reconcile_detached_jobs().await
                                        {
                                            tracing::warn!(
                                                %error,
                                                "detached callback reconciliation failed"
                                            );
                                        }
                                    });
                                }
                                let build = modified_req.build.get_or_insert_with(|| {
                                    meerkat_core::service::SessionBuildOptions::default()
                                });
                                // COMPOSE over whatever an earlier installer
                                // put in the slot (HomeCore Bug D: assigning
                                // wholesale silently discarded the agent-memory
                                // recorder's `memory` tool for every
                                // callback-built agent). Python-registered
                                // tools win name collisions; everything else
                                // falls through to the pre-installed
                                // dispatcher.
                                let pre_installed = build.external_tools.take();
                                build.external_tools = Some(
                                    meerkat_mobkit::tool_compose::ComposedExternalTools::over(
                                        Arc::new(dispatcher),
                                        pre_installed,
                                    ),
                                );
                            }
                        }
                        None => {
                            return Err(SessionError::Agent(agent_tool_error(format!(
                                "callback/build_agent: tools must be a JSON array, got: {tools}"
                            ))));
                        }
                    }
                }
                self.inner.build_agent(&modified_req, event_tx).await
            }
            Err(err) => {
                // Propagate the error — before_create failure aborts session creation.
                // This is an intentional breaking change from v0.5.x where failures
                // were silently swallowed with a fallback to default build.
                Err(SessionError::Agent(agent_tool_error(format!(
                    "callback/build_agent failed: {err}"
                ))))
            }
        }
    }
}

/// Meerkat 0.7 replaced `agent_tool_error(String)` with the typed
/// `AgentError::Tool { error: ToolError }` carrier.
fn agent_tool_error(message: String) -> AgentError {
    AgentError::Tool {
        error: ToolError::execution_failed(message),
    }
}

/// Persistent mode: reads JSON-RPC over stdin, bootstraps unified runtime, serves HTTP.
fn run_persistent(control_listen: Option<ControlListenAddr>) {
    // Deep worker stacks for the generated machine-authority apply path; the
    // builder is shared with mobkit_gateway, the failure reporting is not
    // (this binary has no init handshake to answer at this point).
    match meerkat_mobkit::gateway_composition::gateway_tokio_runtime() {
        Ok(runtime) => runtime.block_on(run_persistent_inner(control_listen)),
        Err(error) => {
            eprintln!("failed to build tokio runtime: {error}");
            std::process::exit(1);
        }
    }
}

async fn run_persistent_inner(control_listen: Option<ControlListenAddr>) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    // Initialize tracing subscriber so meerkat-mob/meerkat-runtime errors
    // are visible on stderr. Without this, all tracing events are silently
    // dropped and runtime failures (agent build, LLM calls, comms drain)
    // are invisible.
    //
    // Default: this crate's own targets at INFO, dependencies at WARN.
    // Operationally significant boot phases (the one-time head-canonical
    // conversion, continuity repair) report at INFO from meerkat_mobkit; at
    // the old blanket "warn" default they were invisible, and a 2026-07
    // production deploy was aborted because a supervisor read a
    // silent-but-working migration as a hang. RUST_LOG still overrides
    // everything. Shared with mobkit_gateway so the two gateways cannot
    // drift on observability posture.
    meerkat_mobkit::gateway_composition::init_gateway_tracing(GATEWAY_TRACING_TARGET);

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);

    // 1. Read first line — must be mobkit/init
    let mut init_line = String::new();
    if reader.read_line(&mut init_line).await.unwrap_or(0) == 0 {
        eprintln!("rpc_gateway: stdin closed before init request");
        std::process::exit(1);
    }

    let init_raw: Value = match serde_json::from_str(init_line.trim()) {
        Ok(v) => v,
        Err(e) => {
            let error_response = json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": format!("Parse error: {e}") }
            });
            println!(
                "{}",
                serde_json::to_string(&error_response)
                    .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
            );
            std::process::exit(1);
        }
    };

    let request_id = init_raw.get("id").cloned().unwrap_or(Value::Null);
    let method = init_raw
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    if method != "mobkit/init" {
        let error_response = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": { "code": -32600, "message": format!("Expected mobkit/init, got {method}") }
        });
        println!(
            "{}",
            serde_json::to_string(&error_response)
                .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
        );
        std::process::exit(1);
    }

    let params = init_raw.get("params").cloned().unwrap_or_else(|| json!({}));

    // 2. Parse init params
    let mob_config_param = params.get("mob_config").and_then(|v| v.as_str());
    let is_workspace_config = mob_config_param.is_some();
    let mob_config_toml = mob_config_param.unwrap_or(
        r#"
[mob]
id = "persistent-gateway"

[profiles.default]
model = "gpt-5.5"
external_addressable = true
"#,
    );

    let definition = MobDefinition::from_toml(mob_config_toml).unwrap_or_else(|e| {
        let error_response = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": { "code": -32602, "message": format!("Invalid mob_config TOML: {e}") }
        });
        println!(
            "{}",
            serde_json::to_string(&error_response)
                .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
        );
        std::process::exit(1);
    });
    let image_generation = mob_definition_may_use_image_generation(&definition);
    let shell = mob_definition_may_use_shell(&definition);

    // Validate profile model names against the catalog.
    // A wrong model name (e.g., "claude-sonnet-4-5-20250514" instead of "claude-sonnet-4-5")
    // silently fails at LLM call time with no observable error. Catch it here.
    {
        let catalog = meerkat_models::catalog::catalog();
        let known_models: std::collections::HashSet<&str> =
            catalog.iter().map(|entry| entry.id).collect();

        for (profile_name, binding) in &definition.profiles {
            let Some(profile) = binding.as_inline() else {
                continue; // Realm refs are resolved at runtime, not validated here.
            };
            if !known_models.contains(profile.model.as_str()) {
                let model = &profile.model;
                // Find similar model names for the error hint
                let prefix = model.split('-').take(3).collect::<Vec<_>>().join("-");
                let mut suggestions: Vec<&str> = known_models
                    .iter()
                    .filter(|m| {
                        m.starts_with(&prefix)
                            || model
                                .starts_with(&m.split('-').take(3).collect::<Vec<_>>().join("-"))
                    })
                    .copied()
                    .collect();
                suggestions.sort_unstable();
                suggestions.truncate(5);
                let hint = if suggestions.is_empty() {
                    String::new()
                } else {
                    format!(". Did you mean one of: {}?", suggestions.join(", "))
                };
                fail_init(
                    &request_id,
                    -32602,
                    format!("Profile '{profile_name}' uses unknown model '{model}'{hint}"),
                );
            }
        }
    }

    let (modules, pre_spawn) = parse_gateway_modules(&params);

    let discovery_modules: Vec<String> = modules.iter().map(|m| m.id.clone()).collect();
    let module_config = MobKitConfig {
        modules,
        discovery: DiscoverySpec {
            namespace: "persistent-gateway".to_string(),
            modules: discovery_modules,
        },
        pre_spawn,
    };

    let has_session_builder = params
        .get("has_session_builder")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let persistent_state = params
        .get("persistent_state")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);

    let has_roster_provider = params
        .get("has_roster_provider")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let has_topology_provider = params
        .get("has_topology_provider")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let has_agent_customizer = params
        .get("has_agent_customizer")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let has_continuity_store = params
        .get("has_continuity_store")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let has_lease_provider = params
        .get("has_lease_provider")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let scratch_dir = params
        .get("scratch_dir")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);

    // The path authority for this boot. Without persistent_state the layout
    // is an explicitly declared-ephemeral scratch root (per-process, under
    // the OS temp dir) — recorded in the layout summary, never a silent
    // call-site fallback.
    let storage_layout = match persistent_state {
        Some(ref state_path) => {
            meerkat_mobkit::MobKitStorageLayout::with_injected_roots(state_path.clone(), None)
        }
        None => meerkat_mobkit::MobKitStorageLayout::declared_ephemeral(
            meerkat_mobkit::storage_layout::default_ephemeral_scratch_root(),
        ),
    };
    let callback_job_store: Option<Arc<dyn meerkat::DetachedJobStore>> =
        persistent_state.as_ref().map(|state_path| {
            let path = meerkat_store::realm_paths_in(
                state_path,
                meerkat_mobkit::storage_provider::MEERKAT_LEVEL_REALM_ID,
            )
            .jobs_sqlite_path;
            if let Some(parent) = path.parent()
                && let Err(error) = std::fs::create_dir_all(parent)
            {
                fail_init(
                    &request_id,
                    STORAGE_RESOLUTION_CODE,
                    format!(
                        "failed to create detached job store directory {}: {error}",
                        parent.display()
                    ),
                );
            }
            match meerkat::SqliteDetachedJobStore::open(path.clone()) {
                Ok(store) => Arc::new(store) as Arc<dyn meerkat::DetachedJobStore>,
                Err(error) => fail_init(
                    &request_id,
                    STORAGE_RESOLUTION_CODE,
                    meerkat_mobkit::storage_health::JobStoreResolutionError {
                        path,
                        message: error.to_string(),
                    }
                    .to_string(),
                ),
            }
        });

    // 3. Set up stdout writer channel for multiplexed output
    let (stdout_tx, mut stdout_rx) = mpsc::channel::<GatewayStdoutLine>(64);
    let (stdout_shutdown_tx, mut stdout_shutdown_rx) = oneshot::channel::<()>();
    let mut stdout_writer = tokio::spawn(async move {
        loop {
            let line = tokio::select! {
                shutdown = &mut stdout_shutdown_rx => {
                    let _ = shutdown;
                    while let Ok(mut line) = stdout_rx.try_recv() {
                        let delivered = write_gateway_stdout_line(&mut line).await;
                        line.settle_delivery(delivered).await;
                    }
                    break;
                }
                line = stdout_rx.recv() => match line {
                    Some(line) => line,
                    None => break,
                },
            };
            let mut line = line;
            let delivered = write_gateway_stdout_line(&mut line).await;
            line.settle_delivery(delivered).await;
        }
    });

    // 4. Build callback bridge and start stdin multiplexer BEFORE bootstrap.
    // This ensures callback responses (e.g. callback/build_agent during discovery
    // spawn) are routed even while UnifiedRuntime::bootstrap is running.
    let bridge = StdioCallbackBridge::new(stdout_tx.clone());
    let (rpc_tx, mut rpc_rx) = mpsc::channel::<String>(64);
    let shutdown_requested = Arc::new(AtomicBool::new(false));

    let stdin_reader = tokio::spawn({
        let bridge = bridge.clone();
        let rpc_tx = rpc_tx.clone();
        let stdout_tx = stdout_tx.clone();
        let shutdown_requested = shutdown_requested.clone();
        async move {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {}
                    Err(_) => break,
                }
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let msg: Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                // Callback responses: "id" starts with "cb-" and no "method"
                let is_callback_response = msg
                    .get("id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id.starts_with("cb-"))
                    && msg.get("method").is_none();

                if is_callback_response {
                    bridge.route_callback_response(msg).await;
                } else if gateway_shutdown_request(&msg).is_some() {
                    // Stop new RPC admission as soon as the handshake enters
                    // the reader, but keep routing callback responses. The
                    // dispatch loop will retain the bridge through
                    // `runtime.shutdown()` and answer this request only after
                    // provider-backed cleanup has completed.
                    if shutdown_requested.swap(true, Ordering::AcqRel) {
                        if let Some(id) = msg.get("id") {
                            let response = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": {
                                    "code": -32098,
                                    "message": "gateway shutdown already in progress"
                                }
                            });
                            if let Ok(line) = serde_json::to_string(&response) {
                                let _ = stdout_tx.send(GatewayStdoutLine::plain(line)).await;
                            }
                        }
                    } else if rpc_tx.send(trimmed.to_string()).await.is_err() {
                        break;
                    }
                } else if shutdown_requested.load(Ordering::Acquire) {
                    // Requests admitted after shutdown cannot be serviced and
                    // must not be left pending in an SDK. Callback responses
                    // were routed above and remain admitted.
                    if let Some(id) = msg.get("id") {
                        let response = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32098,
                                "message": "gateway shutdown in progress"
                            }
                        });
                        if let Ok(line) = serde_json::to_string(&response) {
                            let _ = stdout_tx.send(GatewayStdoutLine::plain(line)).await;
                        }
                    }
                } else {
                    // Queue RPC request for the dispatch loop
                    if rpc_tx.send(trimmed.to_string()).await.is_err() {
                        break;
                    }
                }
            }
            bridge.close().await;
        }
    });
    // The reader task owns the sole producer. Dropping this setup copy is
    // what lets EOF close `rpc_rx`; retaining it would make the dispatch loop
    // wait forever and skip graceful runtime shutdown entirely.
    drop(rpc_tx);

    /// Helper: send a JSON-RPC error response for the init request and exit.
    /// Storage-refusal sites pass [`STORAGE_RESOLUTION_CODE`] so SDKs reify
    /// the fail-closed durability errors; everything else keeps the standard
    /// JSON-RPC codes.
    fn fail_init(request_id: &Value, code: i64, message: String) -> ! {
        let error_response = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": { "code": code, "message": message }
        });
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(
            stdout,
            "{}",
            serde_json::to_string(&error_response)
                .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
        );
        let _ = stdout.flush();
        std::process::exit(1);
    }

    if (has_continuity_store || has_lease_provider || scratch_dir.is_some())
        && !(has_continuity_store && has_lease_provider && scratch_dir.is_some())
    {
        let mut missing = Vec::new();
        if !has_continuity_store {
            missing.push("continuity_store");
        }
        if !has_lease_provider {
            missing.push("lease_provider");
        }
        if scratch_dir.is_none() {
            missing.push("scratch_dir");
        }
        fail_init(
            &request_id,
            -32602,
            format!(
                "external-authoritative path requires continuity_store + lease_provider + scratch_dir; missing: {}",
                missing.join(", ")
            ),
        );
    }

    let mut gateway_options = parse_gateway_runtime_options(&params, persistent_state.as_deref())
        .unwrap_or_else(|e| {
            fail_init(&request_id, -32602, e);
        });
    validate_gateway_identity_bootstrap_intent(
        gateway_options.identity_bootstrap_mode.as_ref(),
        has_roster_provider,
    )
    .unwrap_or_else(|error| fail_init(&request_id, -32602, error));
    if gateway_options.agent_memory.is_some() && !has_roster_provider {
        fail_init(
            &request_id,
            -32602,
            "runtime_options.agent_memory requires an identity-first roster provider".to_string(),
        );
    }
    // Member role migrations this boot is authorized to perform, e.g.
    // `[{"identity": "domain:home-automation", "from_role": "domain"}]`.
    //
    // A malformed declaration refuses the boot rather than arming nothing. An
    // operator who wrote a declaration expects it to be in force, and silently
    // dropping it would resurface as the original
    // `MemberRoleMigrationRequired` refusal with nothing pointing at the
    // payload as the cause.
    let role_migration_declarations: Vec<meerkat_mobkit::identity_first::RoleMigrationDeclaration> =
        params
            .get("role_migrations")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .unwrap_or_else(|error| {
                fail_init(
                    &request_id,
                    -32602,
                    format!("role_migrations is malformed: {error}"),
                )
            })
            .unwrap_or_default();
    if let Some((identity, first, second)) =
        meerkat_mobkit::identity_first::conflicting_role_migration_declaration(
            &role_migration_declarations,
        )
    {
        fail_init(
            &request_id,
            -32602,
            format!(
                "role_migrations declares '{identity}' twice with different predecessor roles \
                 ('{first}' and '{second}'); migration authority cannot be resolved by order"
            ),
        );
    }
    // Compiled application tool policies this boot serves, each the EXACT
    // canonical JSON bytes of one compiled policy:
    // `["{\"schema_version\":1,...}\n", ...]`.
    //
    // They arrive as strings rather than nested objects on purpose. Parsing
    // verifies the digest against the bytes it was given, so re-serialising a
    // nested object would check a digest against bytes MobKit just
    // manufactured instead of the ones the operator compiled. A malformed or
    // digest-mismatched payload refuses the boot, for the same reason
    // `role_migrations` does: an operator who supplied a policy expects it in
    // force, and arming nothing would resurface later as an unexplained
    // access denial.
    let compiled_tool_policies =
        meerkat_mobkit::member_tool_policy::compiled_policy_payloads_from_init_params(&params)
            .unwrap_or_else(|error| fail_init(&request_id, -32602, format!("{error}")));
    let tool_consequence_policy_registry = if compiled_tool_policies.is_empty() {
        None
    } else {
        // One provider per provider id CARRIED by the artifacts. The gateway
        // deliberately names none: see providers_from_canonical_payloads.
        let providers = meerkat_mobkit::member_tool_policy::providers_from_canonical_payloads(
            &compiled_tool_policies,
        )
        .unwrap_or_else(|error| {
            fail_init(
                &request_id,
                -32602,
                format!("application_tool_policies entry rejected: {error}"),
            )
        });
        let providers: Vec<std::sync::Arc<dyn meerkat_core::ToolConsequenceNarrowingPolicy>> =
            providers
                .into_iter()
                .map(|provider| {
                    provider as std::sync::Arc<dyn meerkat_core::ToolConsequenceNarrowingPolicy>
                })
                .collect();
        let registry = meerkat_core::ToolConsequencePolicyRegistry::new(
            providers,
            meerkat_core::PolicyEvaluationSupervisorConfig::default(),
            None,
        )
        .unwrap_or_else(|error| {
            fail_init(
                &request_id,
                -32603,
                format!("application tool policy registry could not be built: {error}"),
            )
        });
        Some(std::sync::Arc::new(registry))
    };
    let default_llm_client: Option<Arc<dyn meerkat_client::LlmClient>> =
        match gateway_options.demo_llm {
            true => {
                let client: Arc<dyn meerkat_client::LlmClient> =
                    Arc::new(meerkat_client::TestClient::default());
                Some(client)
            }
            false => None,
        };
    // The bundled local lease provider's monotonic fencing counter must resume
    // ABOVE the persisted high-water on restart. Otherwise it resets to 1 and
    // restore presents a stale token that the store's compare-and-set rejects
    // ("stale fencing token: presented 1, current N"), aborting boot on every
    // restart with existing history. Capture the local store's high-water here
    // and seed the lease provider with it below.
    let mut local_default_lease_provider: Option<
        Arc<dyn meerkat_mobkit::identity_first::contracts::LeaseProvider>,
    > = None;
    let identity_continuity_store: Option<
        Arc<dyn meerkat_mobkit::identity_first::ContinuityStore>,
    > = if has_roster_provider {
        Some(if has_continuity_store {
            Arc::new(meerkat_mobkit::identity_first::GatewayContinuityStore::new(
                bridge.clone(),
            ))
        } else {
            let db_path = match storage_layout.continuity_db() {
                Ok(resolved) => resolved.path,
                Err(e) => fail_init(&request_id, STORAGE_RESOLUTION_CODE, e.to_string()),
            };
            if storage_layout.is_declared_ephemeral()
                && let Err(e) = std::fs::create_dir_all(storage_layout.state_dir())
            {
                fail_init(
                    &request_id,
                    STORAGE_RESOLUTION_CODE,
                    format!("failed to create the declared-ephemeral scratch root: {e}"),
                );
            }
            let substrate = meerkat_mobkit::gateway_wiring::open_identity_substrate(&db_path)
                .await
                .unwrap_or_else(|e| fail_init(&request_id, STORAGE_RESOLUTION_CODE, e));
            local_default_lease_provider = Some(substrate.lease_provider);
            substrate.continuity_store
        })
    } else {
        None
    };
    let identity_lease_provider: Option<
        Arc<dyn meerkat_mobkit::identity_first::contracts::LeaseProvider>,
    > = if has_roster_provider {
        Some(if has_lease_provider {
            Arc::new(meerkat_mobkit::identity_first::GatewayLeaseProvider::new(
                bridge.clone(),
            )) as Arc<dyn meerkat_mobkit::identity_first::contracts::LeaseProvider>
        } else {
            // From the shared substrate: fencing counter resumes above the
            // persisted high-water (see gateway_wiring::open_identity_substrate).
            local_default_lease_provider
                .clone()
                .expect("local substrate initialized with the continuity store")
        })
    } else {
        None
    };
    let identity_session_store_adapter = identity_continuity_store.as_ref().map(|store| {
        Arc::new(meerkat_mobkit::identity_first::ContinuitySessionStoreAdapter::new(store.clone()))
    });

    // 5. Build session service with callback bridge.
    // Stable owner id for the schedule driver, captured before `definition` is
    // moved into the bootstrap spec.
    let schedule_owner_id = definition.id.to_string();
    // Operator-verb seam: the CONCRETE persistent session service the mob
    // bootstraps with, reached through the transcript extension traits. The
    // erased Arc<dyn MobSessionService> cannot recover this (no trait
    // upcasting), and it must be THIS instance - a second service over the
    // same stores has its own live registry, so its Busy check would not see
    // this gateway's running members. Ephemeral lanes stay None.
    let mut gateway_transcript_edit_service: Option<
        Arc<dyn meerkat_mobkit::memory::hygienist::TranscriptEditSessionService>,
    > = None;
    let (
        mob_spec,
        _temp_dir,
        schedule_host_inputs,
        workgraph_service,
        live_inputs,
        gateway_detached_jobs,
    ) = if let Some(ref state_path) = persistent_state {
        if let Err(e) = std::fs::create_dir_all(state_path) {
            fail_init(
                &request_id,
                STORAGE_RESOLUTION_CODE,
                format!("failed to create persistent state directory: {e}"),
            );
        }
        let sqlite_path = match storage_layout.session_db() {
            Ok(resolved) => resolved.path,
            Err(e) => fail_init(&request_id, STORAGE_RESOLUTION_CODE, e.to_string()),
        };
        let session_store_kind = if identity_session_store_adapter.is_some() {
            "ContinuitySessionStoreAdapter"
        } else {
            "SqliteSessionStore"
        };
        let session_store: Arc<dyn meerkat::SessionStore> =
            if let Some(adapter) = identity_session_store_adapter.clone() {
                adapter
            } else {
                match meerkat_store::SqliteSessionStore::open(sqlite_path) {
                    Ok(s) => Arc::new(s),
                    Err(e) => fail_init(
                        &request_id,
                        STORAGE_RESOLUTION_CODE,
                        format!("failed to open SQLite session store: {e}"),
                    ),
                }
            };
        // H2: probe the incremental capability on the same Arc the session
        // service receives below — identity-first launches ride the
        // continuity adapter, which persists whole-blob only.
        let session_store_incremental =
            meerkat_mobkit::storage_health::probe_session_store_incremental(
                &session_store,
                session_store_kind,
            );
        // This is the persistent launch branch: the runtime store 25 lines
        // below is fail-closed persistent for exactly this reason ("must be
        // persistent so resume works across gateway restart"), and mob storage
        // was never given the same treatment. In-memory here means every
        // adopted identity declaration is gone on restart while sessions
        // survive.
        let (mob_storage, mob_storage_provenance) = if gateway_options.mob_storage_ephemeral {
            // Declared, not silent. This launch keeps mob state in memory even
            // though it has a persistent state dir, which is the only way to
            // keep editing mob_config across restarts: a persistent mob
            // storage pins the definition.
            if gateway_options.declare_spec_update.is_some() {
                // Nothing is pinned on in-memory mob storage, so there is no
                // spec to move. Silently ignoring the declaration would let an
                // activation log "spec updated" against a store that has none.
                fail_init(
                    &request_id,
                    STORAGE_RESOLUTION_CODE,
                    "runtime_options.declare_spec_update was supplied alongside an in-memory \
                     mob_storage declaration; in-memory mob storage pins no definition, so \
                     there is no persisted spec to declare an update against"
                        .to_string(),
                );
            }
            (
                MobStorage::in_memory(),
                meerkat_mobkit::mob_composition_manifest::MobStorageProvenance::declared_ephemeral(
                ),
            )
        } else {
            let pair = match meerkat_mobkit::mob_composition_manifest::persistent_mob_storage(
                storage_layout.event_log_db(),
            ) {
                Ok(pair) => pair,
                Err(err) => fail_init(
                    &request_id,
                    STORAGE_RESOLUTION_CODE,
                    format!(
                        "failed to open the persistent mob storage at {}: {err} (adopted \
                         identity declarations and mob events would not survive gateway \
                         restart)",
                        storage_layout.event_log_db().display()
                    ),
                ),
            };
            // The door through the pinned-mob_config refusal, taken only when
            // THIS activation declares it. Runs before the mob is built, so the
            // upstream definition/spec-store sync sees the declared spec rather
            // than refusing and leaving the operator to discover the door from
            // an error message.
            if let Some(expected_revision) = gateway_options.declare_spec_update {
                match meerkat_mobkit::spec_update_ceremony::declare_spec_update(
                    &storage_layout.event_log_db(),
                    definition.id.as_str(),
                    &definition,
                    expected_revision,
                )
                .await
                {
                    Ok(receipt) => {
                        // Evidence, at warn: moving a pinned spec is a declared
                        // operator transition and should be legible in an
                        // activation log without raising the log level to find it.
                        tracing::warn!(
                            mob_id = %receipt.mob_id,
                            previous_revision = receipt.previous_revision,
                            committed_revision = receipt.committed_revision,
                            declared_fields = ?receipt.declared_fields,
                            "operator-declared mob spec update committed"
                        );
                    }
                    Err(err) => fail_init(
                        &request_id,
                        STORAGE_RESOLUTION_CODE,
                        format!("declared mob spec update refused: {err}"),
                    ),
                }
            }
            pair
        };
        let binary_blob_store: Arc<dyn BinaryBlobStore> =
            match ObjectStoreBlobStore::local(storage_layout.blob_root()) {
                Ok(store) => Arc::new(store),
                Err(e) => fail_init(
                    &request_id,
                    STORAGE_RESOLUTION_CODE,
                    format!("failed to open binary blob store: {e}"),
                ),
            };
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
        // Persistent runtime store — must be Some() on the session service
        // so archive/retire can mutate the authoritative session, and must
        // be persistent so resume works across gateway restart.
        //
        // Fail-closed (M4): an open failure is an init error; the in-memory
        // form exists only as the explicit
        // `runtime_options.runtime_store = {"storage": "memory"}` declaration
        // (the former silent fallback left resume and archive broken long
        // after boot).
        let runtime_db_path = storage_layout.runtime_db();
        let (runtime_store, runtime_store_slot): (
            Arc<dyn meerkat_runtime::RuntimeStore>,
            meerkat_mobkit::storage_health::StorageSlotSummary,
        ) = if gateway_options.runtime_store_ephemeral {
            (
                Arc::new(meerkat_runtime::InMemoryRuntimeStore::new()),
                meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                    "runtime",
                    "InMemoryRuntimeStore",
                    "explicitly declared via runtime_options.runtime_store: sessions do not \
                     survive gateway restart",
                ),
            )
        } else {
            match meerkat_runtime::store::SqliteRuntimeStore::new(&runtime_db_path) {
                Ok(store) => (
                    Arc::new(store) as Arc<dyn meerkat_runtime::RuntimeStore>,
                    meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                        "runtime",
                        "SqliteRuntimeStore",
                    ),
                ),
                Err(err) => fail_init(
                    &request_id,
                    STORAGE_RESOLUTION_CODE,
                    meerkat_mobkit::storage_health::RuntimeStoreResolutionError {
                        path: runtime_db_path.clone(),
                        message: err.to_string(),
                    }
                    .to_string(),
                ),
            }
        };
        // Wrap the runtime store in the write-epoch facade BEFORE the machine
        // and session service capture it, and thread the witness into the
        // bootstrap spec below. Without it `MobBootstrapSpec::new` leaves the
        // witness absent and the console session-history epoch gate is
        // disabled — the 5s discovery loop then re-reads and re-validates
        // every member's whole session document forever (~0.3 core per idle
        // durable member at production document sizes; the 0.8.4 idle
        // driver on this externally-composed path).
        //
        // The facade also owns the durable session projection at meerkat
        // 0.8.11 (the session service no longer writes the SessionStore
        // itself) and every-boot runtime-authority re-minting from that
        // durable store - a reset/lost runtime store reseeds instead of
        // refusing resume.
        let (runtime_store, session_write_epochs) =
            meerkat_mobkit::mob_handle_runtime::epoch_tracking_runtime_store_with_durable_projection(
                runtime_store,
                session_store.clone(),
            );
        let adapter = Arc::new(meerkat_runtime::MeerkatMachine::persistent(
            Arc::clone(&runtime_store),
            Arc::clone(&blob_store),
        ));
        // Match the ephemeral path's capability mask — only comms is enabled
        // by default. Apps control additional capabilities via their mob
        // definition profiles, not the gateway factory.
        let mut factory = AgentFactory::new(state_path)
            .builtins(false)
            .shell(shell)
            .comms(true);
        if image_generation {
            factory = factory.with_image_generation_machine(adapter.clone());
        }
        // Live (realtime) needs the SAME factory shape for its per-open
        // credential-resolving realtime session factory; clone before
        // the builder consumes it.
        #[cfg(feature = "experimental-gpt-live")]
        let live_agent_factory = if let Some(experimental) =
            gateway_options.experimental_live.as_ref()
        {
            let operator = meerkat::ExperimentalLiveOperatorConfig::new(
                experimental.factory.clone(),
                experimental.qualification.clone(),
            )
            .with_gpt_live_function_bridge_profile()
            .unwrap_or_else(|error| {
                fail_init(
                    &request_id,
                    -32602,
                    format!("runtime_options.experimental_live execution profile failed: {error}"),
                )
            });
            factory
                .clone()
                .with_experimental_live_admission(operator, [experimental.realm.clone()])
        } else {
            factory.clone()
        };
        #[cfg(not(feature = "experimental-gpt-live"))]
        let live_agent_factory = factory.clone();
        let live_machine = Arc::clone(&adapter);
        let mut inner_builder =
            FactoryAgentBuilder::new(factory, gateway_agent_config(&gateway_options));
        inner_builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
            session_store.clone(),
        )));
        inner_builder.default_blob_store = Some(blob_store.clone());
        inner_builder.default_detached_job_store = callback_job_store.clone();
        if let Some(job_store) = callback_job_store.as_ref() {
            // meerkat 0.8.22 (F4): the projector slot is a Clone value, not a
            // shared Arc - per-session builds rebind its realm authority via
            // bound_to_realm so mob members project under mob.<mob_id>, never
            // a service-lifetime realm frozen at gateway construction.
            inner_builder.default_shell_job_delivery_projector =
                Some(meerkat::JobOutboxProjector::new_for_realm(
                    Arc::clone(job_store),
                    meerkat_runtime::RuntimeDeliveryInbox::new(Arc::clone(&runtime_store)),
                    meerkat_mobkit::storage_provider::MEERKAT_LEVEL_REALM_ID,
                ));
        }
        // Attach meerkat's per-session schedule tools so SDK-hosted members whose
        // profile sets tools.schedule=true get the meerkat_schedule_* surface (the
        // slot lives on the inner FactoryAgentBuilder and propagates through the
        // callback wrapper). The returned service backs the firing host spawned
        // after the runtime boots — meerkat's runtime-backed host is now generic
        // over the session builder, so scheduled sessions materialize through the
        // SDK build callback and keep their identity-scoped tools.
        let (schedule_tools, schedule_slot) =
            match meerkat_mobkit::schedule_wiring::attach_schedule_tools_with_identity_targets_reporting(
                &inner_builder,
                storage_layout.state_dir(),
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
                    // Sanctioned boot-without degradation: schedule tools are
                    // disabled and the fact is health-visible, not a warn
                    // line alone (M4).
                    meerkat_mobkit::storage_health::StorageSlotSummary::degraded(
                        "schedule",
                        format!("schedule store failed to open; schedule tools disabled: {error}"),
                    ),
                ),
            };
        // WorkGraph: durable store beside the schedule store (or in the
        // explicitly configured directory), realm scoped to the mob
        // definition id. Fills the member tool slot, threads to the
        // bootstrap spec (mob-executor attention overlays + child-mob
        // inheritance), the schedule host, and the RPC surface. The
        // state dir travels along for the cross-process admission
        // sidecar (a durable store is shareable across processes).
        let (workgraph, workgraph_slot) = match &gateway_options.workgraph {
            GatewayWorkgraphOption::Disabled => (
                None,
                meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                    "workgraph",
                    "disabled",
                    "explicitly disabled via runtime_options.workgraph = false",
                ),
            ),
            GatewayWorkgraphOption::Enabled => {
                match meerkat_mobkit::workgraph_wiring::attach_workgraph_tools_reporting(
                    &inner_builder,
                    storage_layout.state_dir(),
                    &schedule_owner_id,
                ) {
                    Ok((service, slot)) => (
                        Some((service, slot, storage_layout.state_dir().to_path_buf())),
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
                }
            }
            GatewayWorkgraphOption::DurableDir(dir) => {
                // An explicit directory overrides the state-dir default.
                // Open failure keeps the boot-without-workgraph posture
                // (health-visible as a degraded slot).
                let _ = std::fs::create_dir_all(dir);
                match meerkat_mobkit::workgraph_wiring::attach_workgraph_tools_reporting(
                    &inner_builder,
                    dir,
                    &schedule_owner_id,
                ) {
                    Ok((service, slot)) => (
                        Some((service, slot, dir.clone())),
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
                }
            }
        };
        let workgraph_service = workgraph.as_ref().map(|(service, _, _)| service.clone());
        let agent_mob_tools_slot = Arc::clone(&inner_builder.default_mob_tools);
        let detached_jobs = callback_job_store.as_ref().map(|store| {
            DetachedCallbackJobRuntime::new(
                meerkat_mobkit::storage_provider::MEERKAT_LEVEL_REALM_ID,
                Arc::clone(store),
                blob_store.clone(),
            )
            .with_runtime_delivery_store(Arc::clone(&runtime_store))
            .with_monitor_shell(
                state_path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or(state_path)
                    .to_path_buf(),
                shell,
            )
        });
        let callback_builder = StdioCallbackAgentBuilder {
            inner: inner_builder,
            bridge: bridge.clone(),
            has_session_builder,
            session_store: Some(session_store.clone()),
            detached_jobs: detached_jobs.clone(),
        };
        // Keep the CONCRETE typed service for the firing host (the runtime-backed
        // host needs PersistentSessionService<StdioCallbackAgentBuilder>, not the
        // erased Arc<dyn MobSessionService> the spec consumes).
        let concrete_service = Arc::new(meerkat_session::PersistentSessionService::new(
            callback_builder,
            gateway_options.max_sessions,
            session_store,
            Arc::clone(&runtime_store),
            blob_store,
        ));
        if let Some(detached_jobs) = detached_jobs.as_ref() {
            let delivery_service: Arc<dyn meerkat_mob::MobSessionService> =
                concrete_service.clone();
            detached_jobs.attach_delivery_service(delivery_service);
        }
        let schedule_host_inputs = schedule_tools.map(|tools| {
            (
                tools.service,
                tools.mob_target_registry,
                Arc::clone(&concrete_service),
                adapter.clone(),
                storage_layout.schedule_db(),
                tools.firing_host_binding,
            )
        });
        // Live (realtime) transport inputs: the concrete service +
        // machine + factory, captured only on opt-in
        // (`runtime_options.live`). Ephemeral mode cannot serve live —
        // the projection sink and machine authorities are
        // persistent-service seams.
        let live_inputs = if matches!(gateway_options.live, GatewayLiveOption::Enabled { .. })
            || cfg!(feature = "experimental-gpt-live") && {
                #[cfg(feature = "experimental-gpt-live")]
                {
                    gateway_options.experimental_live.is_some()
                }
                #[cfg(not(feature = "experimental-gpt-live"))]
                {
                    false
                }
            } {
            Some((
                Arc::clone(&concrete_service),
                live_machine,
                live_agent_factory,
            ))
        } else {
            None
        };
        // Heal seam (2026-07-29 incident): the CONCRETE persistent service is
        // the committed-boundary recoverer for the identity repair supervisor.
        let committed_boundary_recoverer: Arc<
            dyn meerkat_mobkit::identity_first::CommittedBoundaryRecoverer,
        > = Arc::clone(&concrete_service) as _;
        gateway_transcript_edit_service = Some(Arc::clone(&concrete_service) as _);
        let session_service: Arc<dyn meerkat_mob::MobSessionService> = concrete_service;
        let mut spec = MobBootstrapSpec::new(definition, mob_storage, session_service)
            .with_mob_storage_provenance(mob_storage_provenance)
            // A parsed option that never reaches the spec is the same defect as
            // an unreachable API, one layer down.
            .with_composition_authority(gateway_options.composition_authority)
            .with_optional_tool_consequence_policy_registry(
                tool_consequence_policy_registry.clone(),
            )
            .with_session_write_epochs(&session_write_epochs)
            // Resume-seam reads must carry the runtime store's archived
            // terminal (at 0.8.11 archive stamps the catalog/lifecycle row,
            // never the session body).
            .with_runtime_archived_terminal_authority(Arc::clone(&runtime_store))
            .with_session_runtime_adapter(adapter.clone())
            // Order matters: workgraph before agent mob tools so child mobs
            // inherit the service at mob-state install time.
            .with_workgraph_service(workgraph_service.clone())
            // Agent mob tools + the schedule host's mob authority (HomeCore
            // 0.7.26 last-link fix): without this, agent-authored schedules
            // can't rewrite to mob-member targets or deliver them.
            .with_agent_mob_tools(agent_mob_tools_slot)
            .with_options(MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: default_llm_client.clone(),
            });
        spec.committed_boundary_recoverer = Some(committed_boundary_recoverer);
        if let Some((_, admission_slot, workgraph_state_dir)) = &workgraph {
            // Durable (cross-process shareable) store: register the
            // tool-plane admission slot and the sidecar lock beside it.
            spec = spec
                .with_workgraph_admission_slot(admission_slot.clone())
                .with_workgraph_admission_sidecar(workgraph_state_dir);
        }
        spec.runtime_adapter = Some(adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        // Blob slot resolved fail-closed above (local disk under
        // <state_path>/blobs); record it plus the H2 probe result and the
        // per-slot census (M4) for the health surfaces.
        let mut slots = vec![
            if identity_session_store_adapter.is_some() {
                meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                    "sessions",
                    "ContinuitySessionStoreAdapter",
                )
                .with_detail("sessions ride the identity continuity store")
            } else {
                meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                    "sessions",
                    "SqliteSessionStore",
                )
            },
            runtime_store_slot,
            meerkat_mobkit::storage_health::blob_slot_summary(
                meerkat_mobkit::storage_health::BlobDurability::PersistentDisk,
            ),
            meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                "console",
                "SqliteConsoleLogStore",
            ),
            meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                "metadata",
                "SqliteMetadataStore",
            ),
            schedule_slot,
            workgraph_slot,
            meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                "jobs",
                "SqliteDetachedJobStore",
            ),
            gateway_event_log_slot(&gateway_options),
        ];
        // Configured agent memory is a durable disk-backed slot under the
        // state dir; the census must cover it like every other store.
        if let Some(agent_memory) = gateway_options.agent_memory.as_ref() {
            slots.push(agent_memory_census_slot(agent_memory));
        }
        if identity_continuity_store.is_some() {
            slots.push(if has_continuity_store {
                meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                    "continuity",
                    "GatewayContinuityStore (SDK-hosted)",
                )
                .with_detail("durability rides with the SDK-hosted continuity store")
            } else {
                meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                    "continuity",
                    "LocalContinuityStore",
                )
            });
        }
        slots.extend(meerkat_mobkit::storage_health::scratch_ring_buffer_slots());
        // The M4 census had no mob slot at all, which is why an in-memory mob
        // storage on this persistent launch was invisible to healthz and the
        // storage doctor while sessions correctly reported persistent. It now
        // reports whichever this launch actually composed.
        slots.push(if gateway_options.mob_storage_ephemeral {
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "mob",
                "MobStorage(in-memory)",
                "declared via runtime_options.mob_storage: mob events and adopted identity \
                 declarations do not survive gateway restart, in exchange for a mob_config \
                 that can still be edited across restarts",
            )
        } else {
            meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                "mob",
                "MobStorage(sqlite)",
            )
        });
        spec.resolved_storage = Some(
            meerkat_mobkit::storage_health::ResolvedStorageSummary::new(
                meerkat_mobkit::storage_health::BlobDurability::PersistentDisk,
                Some(session_store_incremental),
            )
            .with_state_dir(storage_layout.state_dir())
            .with_slots(slots),
        );
        (
            spec,
            None,
            schedule_host_inputs,
            workgraph_service,
            live_inputs,
            detached_jobs,
        )
    } else {
        // Ephemeral mode (original behavior).
        // Use MemoryStore to avoid JSONL writes — the gateway uses EphemeralSessionService
        // so agent-level persistence is not needed. This avoids failures on read-only
        // filesystems (e.g., GKE containers) where the default JSONL store can't write.
        let temp_dir = if scratch_dir.is_none() {
            Some(tempfile::tempdir().expect("create temp dir for agent working space"))
        } else {
            None
        };
        let agent_workspace = scratch_dir
            .as_deref()
            .or_else(|| temp_dir.as_ref().map(|dir| dir.path()))
            .expect("scratch dir or temp dir");
        if let Err(err) = std::fs::create_dir_all(agent_workspace) {
            fail_init(
                &request_id,
                STORAGE_RESOLUTION_CODE,
                format!("failed to create scratch directory: {err}"),
            );
        }
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
        let mut factory = AgentFactory::new(agent_workspace)
            .builtins(false)
            .shell(shell)
            .comms(true)
            .session_store(Arc::new(meerkat::MemoryStore::new()));
        if image_generation {
            factory = factory.with_image_generation_machine(adapter.clone());
        }
        let mut inner_builder =
            FactoryAgentBuilder::new(factory, gateway_agent_config(&gateway_options));
        inner_builder.default_blob_store = Some(blob_store.clone());
        // No-persistent_state launches default to a memory-backed
        // workgraph (tools stay profile-gated, so nothing changes for
        // members that do not opt in); an explicit directory gets the
        // durable store instead — and, being cross-process shareable, a
        // cross-process admission sidecar beside it.
        let mut workgraph_sidecar_dir: Option<PathBuf> = None;
        let (workgraph_service, ephemeral_workgraph_slot) = match &gateway_options.workgraph {
            GatewayWorkgraphOption::Disabled => (
                None,
                meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                    "workgraph",
                    "disabled",
                    "explicitly disabled via runtime_options.workgraph = false",
                ),
            ),
            GatewayWorkgraphOption::DurableDir(dir) => {
                let _ = std::fs::create_dir_all(dir);
                workgraph_sidecar_dir = Some(dir.clone());
                match meerkat_mobkit::workgraph_wiring::open_workgraph_service_reporting(
                    dir,
                    &schedule_owner_id,
                ) {
                    Ok(service) => (
                        Some(service),
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
                }
            }
            GatewayWorkgraphOption::Enabled => {
                if has_continuity_store {
                    // Identity-first launch persisting through the
                    // SDK-hosted continuity store: everything else about
                    // it is durable, but the continuity store is a stdio
                    // callback (no local db path to co-locate a default
                    // workgraph.sqlite3 with), so the workgraph silently
                    // rides memory unless a path is configured.
                    tracing::warn!(
                        "workgraph is MEMORY-BACKED for this launch: the continuity store \
                             is SDK-hosted (no local path) and no persistent_state dir is set, \
                             so goals, work items and attention bindings will NOT survive a \
                             gateway restart. Set runtime_options.workgraph to a directory \
                             path (or provide persistent_state) to place a durable \
                             workgraph.sqlite3.",
                    );
                }
                (
                    Some(
                        meerkat_mobkit::workgraph_wiring::ephemeral_workgraph_service(
                            &schedule_owner_id,
                        ),
                    ),
                    meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                        "workgraph",
                        "MemoryWorkGraphStore",
                        if has_continuity_store {
                            "memory-backed on an otherwise durable identity-first launch: set \
                             runtime_options.workgraph to a directory (or persistent_state) \
                             for a durable store"
                        } else {
                            "declared by the ephemeral launch mode"
                        },
                    ),
                )
            }
        };
        // One admission slot per builder that carries the tool surface;
        // the bootstrap spec registers them all so every dispatcher gets
        // the runtime-wide admission (round-3 R1).
        let mut workgraph_admission_slots = Vec::new();
        if let Some(service) = workgraph_service.as_ref() {
            workgraph_admission_slots.push(
                meerkat_mobkit::workgraph_wiring::install_workgraph_tools(&inner_builder, service),
            );
        }
        let callback_builder = StdioCallbackAgentBuilder {
            inner: inner_builder,
            bridge: bridge.clone(),
            has_session_builder,
            session_store: None,
            detached_jobs: None,
        };
        // Heal seam (2026-07-29 incident): captured from the CONCRETE
        // persistent service below; the erased MobSessionService does not
        // carry the heal API.
        let mut committed_boundary_recoverer: Option<
            Arc<dyn meerkat_mobkit::identity_first::CommittedBoundaryRecoverer>,
        > = None;
        let mut session_store_incremental: Option<bool> = None;
        let session_service: Arc<dyn meerkat_mob::MobSessionService> =
            if let Some(session_adapter) = identity_session_store_adapter.clone() {
                let session_store: Arc<dyn meerkat::SessionStore> = session_adapter.clone();
                // H2: identity-first launches persist sessions through the
                // continuity adapter — whole-blob only; make it loud.
                session_store_incremental = Some(
                    meerkat_mobkit::storage_health::probe_session_store_incremental(
                        &session_store,
                        "ContinuitySessionStoreAdapter",
                    ),
                );
                let mut factory = AgentFactory::new(agent_workspace)
                    .builtins(false)
                    .shell(shell)
                    .comms(true)
                    .session_store(Arc::new(meerkat::MemoryStore::new()));
                if image_generation {
                    factory = factory.with_image_generation_machine(adapter.clone());
                }
                let mut inner_builder =
                    FactoryAgentBuilder::new(factory, gateway_agent_config(&gateway_options));
                inner_builder.default_session_store = Some(Arc::new(
                    meerkat_store::StoreAdapter::new(session_store.clone()),
                ));
                inner_builder.default_blob_store = Some(blob_store.clone());
                if let Some(service) = workgraph_service.as_ref() {
                    workgraph_admission_slots.push(
                        meerkat_mobkit::workgraph_wiring::install_workgraph_tools(
                            &inner_builder,
                            service,
                        ),
                    );
                }
                let callback_builder = StdioCallbackAgentBuilder {
                    inner: inner_builder,
                    bridge: bridge.clone(),
                    has_session_builder,
                    session_store: Some(session_store.clone()),
                    detached_jobs: None,
                };
                let concrete = Arc::new(meerkat_session::PersistentSessionService::new(
                    callback_builder,
                    gateway_options.max_sessions,
                    session_store,
                    Arc::clone(&runtime_store),
                    blob_store.clone(),
                ));
                committed_boundary_recoverer = Some(Arc::clone(&concrete) as _);
                gateway_transcript_edit_service = Some(Arc::clone(&concrete) as _);
                concrete
            } else {
                Arc::new(EphemeralSessionService::new(
                    callback_builder,
                    gateway_options.max_sessions,
                ))
            };

        let mut spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
            .with_optional_tool_consequence_policy_registry(
                tool_consequence_policy_registry.clone(),
            )
            .with_session_write_epochs(&session_write_epochs)
            .with_session_runtime_adapter(adapter.clone())
            .with_workgraph_service(workgraph_service.clone())
            .with_options(MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: default_llm_client.clone(),
            });
        spec.committed_boundary_recoverer = committed_boundary_recoverer;
        for slot in workgraph_admission_slots {
            spec = spec.with_workgraph_admission_slot(slot);
        }
        if let Some(dir) = workgraph_sidecar_dir.as_deref() {
            spec = spec.with_workgraph_admission_sidecar(dir);
        }
        spec.runtime_adapter = Some(adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        // In-memory blobs are the declared choice of this launch mode; the
        // H2 flag is only set when the identity adapter backs a persistent
        // session service above.
        let mut slots = vec![
            if identity_session_store_adapter.is_some() {
                meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                    "sessions",
                    "ContinuitySessionStoreAdapter",
                )
                .with_detail("sessions ride the identity continuity store")
            } else {
                meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                    "sessions",
                    "EphemeralSessionService",
                    "declared by the ephemeral launch mode",
                )
            },
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "runtime",
                "InMemoryRuntimeStore",
                "declared by the ephemeral launch mode",
            ),
            meerkat_mobkit::storage_health::blob_slot_summary(
                meerkat_mobkit::storage_health::BlobDurability::DeclaredEphemeral,
            ),
            // The ephemeral gateway's console timeline and metadata cursor
            // are in-memory by this surface's contract — a declared default,
            // documented and health-visible (M4), not an error.
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "console",
                "InMemoryConsoleLogStore",
                "declared default of the ephemeral launch mode",
            ),
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "metadata",
                "InMemoryMetadataStore",
                "declared default of the ephemeral launch mode",
            ),
            ephemeral_workgraph_slot,
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "jobs",
                "disabled",
                "semantic detached admission is unavailable in ephemeral gateway mode",
            ),
            gateway_event_log_slot(&gateway_options),
        ];
        if identity_continuity_store.is_some() {
            slots.push(if has_continuity_store {
                meerkat_mobkit::storage_health::StorageSlotSummary::persistent(
                    "continuity",
                    "GatewayContinuityStore (SDK-hosted)",
                )
                .with_detail("durability rides with the SDK-hosted continuity store")
            } else {
                meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                    "continuity",
                    "LocalContinuityStore",
                    "backed by the declared-ephemeral scratch root",
                )
            });
        }
        slots.extend(meerkat_mobkit::storage_health::scratch_ring_buffer_slots());
        // Declared, not omitted: this launch has no persistent_state dir, so
        // mob state is in memory by design and the census now says so.
        slots.push(
            meerkat_mobkit::storage_health::StorageSlotSummary::declared_ephemeral(
                "mob",
                "MobStorage(in-memory)",
                "no persistent_state directory was supplied: mob events and adopted \
                 identity declarations do not survive gateway restart",
            ),
        );
        spec.resolved_storage = Some(
            meerkat_mobkit::storage_health::ResolvedStorageSummary::new(
                meerkat_mobkit::storage_health::BlobDurability::DeclaredEphemeral,
                session_store_incremental,
            )
            // The declared-ephemeral scratch root is still this runtime's
            // own state dir: a doctor request over it may see the census.
            .with_state_dir(storage_layout.state_dir())
            .with_slots(slots),
        );
        // Ephemeral sessions have no persistent service; firing is persistent-only.
        (spec, temp_dir, None, workgraph_service, None, None)
    };

    // Wire callback/after_create — notify Python/TS SDK after each session creation.
    // Uses notify_reliable to avoid silent drops under backpressure.
    let mob_spec = if has_session_builder {
        let after_bridge = bridge.clone();
        mob_spec.with_after_create_hook(Arc::new(
            move |session_id: meerkat_core::types::SessionId, ctx| {
                let b = after_bridge.clone();
                Box::pin(async move {
                    b.notify_reliable(
                        "callback/after_create",
                        json!({
                            "session_id": session_id.to_string(),
                            "model": ctx.model,
                            "labels": ctx.labels,
                            "system_prompt": ctx.system_prompt,
                        }),
                    )
                    .await;
                })
            },
        ))
    } else {
        mob_spec
    };

    // §10.1 dispatch-time taint join: keep the spec's late-bound slot so the
    // memory-stack region below can bind the tracker into every member build
    // (bootstrap members included - their decorators read the slot per call).
    let dispatch_taint_slot = mob_spec.dispatch_taint_slot();

    let timeout = GATEWAY_RUNTIME_EVENT_DRAIN_TIMEOUT;
    let persistent_metadata: Arc<dyn PersistentMetadataStore> = if persistent_state.is_some() {
        let metadata_path = match storage_layout.metadata_db() {
            Ok(resolved) => resolved.path,
            Err(e) => fail_init(&request_id, STORAGE_RESOLUTION_CODE, e.to_string()),
        };
        Arc::new(
            SqliteMetadataStore::open(&metadata_path).unwrap_or_else(|e| {
                fail_init(
                    &request_id,
                    STORAGE_RESOLUTION_CODE,
                    format!(
                        "failed to open the mobkit metadata store at {}: {e}",
                        metadata_path.display()
                    ),
                );
            }),
        )
    } else {
        Arc::new(InMemoryMetadataStore::new())
    };
    let bootstrap_plan =
        meerkat_mobkit::gateway_composition::GatewayRuntimeBootstrapPlan::stdio_rpc(
            mob_spec,
            module_config,
            Vec::new(),
            timeout,
            gateway_options.runtime_options.clone(),
            persistent_metadata,
        );
    let mut composition = meerkat_mobkit::gateway_composition::GatewayComposition::prepare(
        meerkat_mobkit::gateway_composition::GatewayCompatibilityProfile::StdioRpc,
        bootstrap_plan,
    )
    .bootstrap()
    .await
    .unwrap_or_else(|e| {
        let error_response = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": { "code": -32603, "message": format!("Runtime bootstrap failed: {e}") }
        });
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(
            stdout,
            "{}",
            serde_json::to_string(&error_response)
                .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
        );
        let _ = stdout.flush();
        std::process::exit(1);
    });
    let runtime = composition.runtime_mut();

    if persistent_state.is_some() {
        let console_log_path = match storage_layout.console_db() {
            Ok(resolved) => resolved.path,
            Err(e) => fail_init(&request_id, STORAGE_RESOLUTION_CODE, e.to_string()),
        };
        let console_log_store = Arc::new(
            SqliteConsoleLogStore::open(&console_log_path).unwrap_or_else(|e| {
                fail_init(
                    &request_id,
                    STORAGE_RESOLUTION_CODE,
                    format!(
                        "failed to open the mobkit console store at {}: {e}",
                        console_log_path.display()
                    ),
                );
            }),
        );
        runtime.set_console_log_store(console_log_store);
    }

    for route in gateway_options.routing_routes.iter().cloned() {
        if let Err(err) = runtime.add_runtime_route(route).await {
            fail_init(
                &request_id,
                -32602,
                format!("runtime_options.routing_config_path route failed validation: {err}"),
            );
        }
    }

    if let Some(event_log_config) = gateway_options.event_log.take() {
        runtime.start_event_log(event_log_config);
    }

    if let Some(access) = gateway_options.access.take() {
        runtime.set_access_controller(access);
    }

    // 5b. Wire error hook — forwards ErrorEvents to Python as JSON-RPC notifications
    let gateway_error_hook: meerkat_mobkit::ErrorHook = {
        let error_bridge = bridge.clone();
        Arc::new(move |event| {
            let b = error_bridge.clone();
            Box::pin(async move {
                if let Ok(params) = serde_json::to_value(&event) {
                    b.notify("mobkit/on_error", params);
                }
            })
        })
    };
    runtime.set_error_hook(gateway_error_hook.clone());

    // 5c. Build identity-first runtime if providers are configured
    // §8.5 steward late-bound seams: the gating/conflict bridges need the
    // Arc'd UnifiedRuntime, which exists only after identity-first init; a
    // OnceCell defers the binding without restructuring bootstrap. The
    // roster slot feeds mob-purpose context once restore_flow has run.
    let steward_late_runtime = StewardLateRuntime::default();
    let steward_roster_slot: Arc<
        std::sync::Mutex<Vec<meerkat_mobkit::identity_first::DurableAgentSpec>>,
    > = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut agent_memory_steward: Option<Arc<meerkat_mobkit::memory::steward::StewardEngine>> =
        None;

    let identity_ctx: Option<meerkat_mobkit::rpc::IdentityFirstContext> = if has_roster_provider {
        use meerkat_mobkit::identity_first::{
            AgentRuntimeServices, DurabilityPolicy, IdentityFirstRuntimeContext, IdentityRuntime,
            IdentityRuntimeConfig, RosterContext,
            gateway_bridges::{
                GatewayAgentCustomizer, GatewayRosterProvider, GatewayTopologyProvider,
            },
        };

        let continuity_store = identity_continuity_store
            .clone()
            .expect("identity continuity store initialized with roster provider");
        let lease_provider = identity_lease_provider
            .clone()
            .expect("identity lease provider initialized with roster provider");

        // Construct the session bridge from the mob handle. The gateway
        // uses the raw bootstrap path (not UnifiedRuntimeBuilder), so
        // session_bridge() won't be set — build it directly.
        let mob_handle = runtime.mob_handle();
        let mut identity_bridge = if let Some(adapter) = identity_session_store_adapter.clone() {
            meerkat_mobkit::identity_first::MobSessionBridge::with_continuity_session_store(
                mob_handle.clone(),
                adapter,
                runtime.mob_runtime().session_service().cloned(),
            )
        } else if let Some(session_service) = runtime.mob_runtime().session_service().cloned() {
            meerkat_mobkit::identity_first::MobSessionBridge::with_session_service(
                mob_handle.clone(),
                session_service,
            )
        } else {
            meerkat_mobkit::identity_first::MobSessionBridge::new(mob_handle.clone())
        };
        // Heal seam (2026-07-29 incident): the continuity repair supervisor
        // asks this recoverer to commit the durable head before declaring an
        // identity healed; without it, heal is a cosmetic entry reset that
        // the next materialization re-Breaks.
        if let Some(recoverer) = runtime.mob_runtime().committed_boundary_recoverer() {
            identity_bridge = identity_bridge.with_committed_boundary_recoverer(recoverer);
        }
        // A declared migration restamps a member's durable role, comms name
        // and binding, so the boot record must name every one the host armed.
        for declaration in &role_migration_declarations {
            tracing::info!(
                identity = %declaration.identity,
                from_role = %declaration.from_role,
                "activation declares a member role migration"
            );
        }
        identity_bridge =
            identity_bridge.with_role_migration_declarations(role_migration_declarations);
        // Operator-verb seam: share the bridge's compaction-floor registry
        // with the RPC context BEFORE the bridge is erased, so
        // mobkit/compact_member arms floors on the same registry the
        // materialization path reads.
        let compaction_floors = identity_bridge.compaction_floors();
        let bridge_arc: Arc<dyn meerkat_mobkit::identity_first::SessionBridge> =
            Arc::new(identity_bridge);

        let irt = Arc::new(
            IdentityRuntime::new(IdentityRuntimeConfig {
                continuity_store,
                lease_provider,
                runtime_instance_id: format!("gateway-{}", std::process::id()),
                has_runtime_store: identity_session_store_adapter.is_some()
                    || persistent_state.is_some(),
                durability_policy: DurabilityPolicy::SyncWriteThrough,
                bridge: Some(bridge_arc),
                default_timeout: None,
            })
            .with_runtime_services(AgentRuntimeServices::new(mob_handle)),
        );
        irt.set_error_hook(Some(gateway_error_hook.clone()));

        // Build provider bridges for callbacks to Python
        let roster: Arc<dyn meerkat_mobkit::identity_first::contracts::RosterProvider> =
            Arc::new(GatewayRosterProvider::new(bridge.clone()));
        let mob_definition = runtime.mob_handle().definition().clone();
        irt.set_reset_roster_provider_context(Some(roster.clone()), Some(mob_definition.clone()));
        let topology: Option<Arc<dyn meerkat_mobkit::identity_first::contracts::TopologyProvider>> =
            if has_topology_provider {
                Some(Arc::new(GatewayTopologyProvider::new(bridge.clone())))
            } else {
                None
            };
        let base_customizer: Option<
            Arc<dyn meerkat_mobkit::identity_first::contracts::AgentCustomizer>,
        > = if has_agent_customizer {
            Some(Arc::new(GatewayAgentCustomizer::new(bridge.clone())))
        } else {
            None
        };
        let mut agent_memory_taint: Option<meerkat_mobkit::SessionTaintTracker> = None;
        // Late-bound target for the always-on compaction reset sink: the
        // sink joins the observer before the injector exists, so it calls
        // through this slot (empty until wiring completes = no-op).
        let agent_memory_compaction_reset: Arc<
            std::sync::OnceLock<meerkat_mobkit::AgentMemoryRuntimeInjector>,
        > = Arc::new(std::sync::OnceLock::new());
        let mut agent_memory_distiller: Option<
            Arc<meerkat_mobkit::memory::distiller::DistillerEngine>,
        > = None;
        let agent_memory_provider: Option<Arc<dyn meerkat_mobkit::AgentMemoryProvider>> =
            if let Some(agent_memory) = gateway_options.agent_memory.as_ref() {
                // Concrete store construction is the only per-kind decision
                // left (M4 de-weld); everything downstream assembles on the
                // provider's advertised capabilities.
                // Agent memory is a durable storage slot: an open failure is
                // a storage-resolution refusal (-32014), same as the other
                // fail-closed store opens above.
                let provider: Arc<dyn meerkat_mobkit::AgentMemoryProvider> =
                    match agent_memory.store {
                        // The live markdown execution path is retired: refuse
                        // with the typed migration verdict instead of booting
                        // a store that silently has no firewall, no ledger
                        // and no judgment plane. The markdown READER is
                        // untouched and still runs, as the one-shot import
                        // inside SqliteAgentMemoryStore::open.
                        GatewayAgentMemoryStoreKind::Markdown => {
                            let migration = AgentMemoryStoreMigration::MarkdownIsImportOnly;
                            fail_init(&request_id, migration.code(), migration.message())
                        }
                        GatewayAgentMemoryStoreKind::Sqlite => Arc::new(
                            meerkat_mobkit::SqliteAgentMemoryStore::open(&agent_memory.path)
                                .unwrap_or_else(|e| {
                                    fail_init(
                                        &request_id,
                                        STORAGE_RESOLUTION_CODE,
                                        format!("failed to open agent memory store: {e}"),
                                    );
                                }),
                        ),
                    };
                if provider.as_taintable().is_some() {
                    // Shared read handle on the persistent session store
                    // for the Distiller's evidence windows and the
                    // steward's gather/usage/resolvability reads.
                    let memory_transcript_store: Option<Arc<dyn meerkat::SessionStore>> =
                        if agent_memory.distiller.enabled || agent_memory.steward.enabled {
                            if persistent_state.is_none() {
                                fail_init(
                                    &request_id,
                                    -32602,
                                    "agent memory distiller/steward require persistent_state"
                                        .to_string(),
                                );
                            }
                            Some(
                                if let Some(adapter) = identity_session_store_adapter.clone() {
                                    adapter
                                } else {
                                    // Second handle on the same session
                                    // database the mob bridge persists to;
                                    // WAL keeps the read-side safe.
                                    let session_db = match storage_layout.session_db() {
                                        Ok(resolved) => resolved.path,
                                        Err(e) => fail_init(
                                            &request_id,
                                            STORAGE_RESOLUTION_CODE,
                                            e.to_string(),
                                        ),
                                    };
                                    match meerkat_store::SqliteSessionStore::open(session_db) {
                                        Ok(store) => Arc::new(store),
                                        Err(e) => fail_init(
                                            &request_id,
                                            STORAGE_RESOLUTION_CODE,
                                            format!("agent memory session store: {e}"),
                                        ),
                                    }
                                },
                            )
                        } else {
                            None
                        };
                    // Converged assembly (memory_wiring): provider + §10.1
                    // firewall + Distiller + Steward, with the gateway's
                    // late-binding bridges passed as seams. Gateway-only
                    // extras (outbound taint declarer, panel registration,
                    // compaction reset, observer spawn, dream scheduling)
                    // follow below. Hygienist activation is parked and has
                    // no composition seam here.
                    let memory_events = runtime.memory_event_sink();
                    let engines = meerkat_mobkit::memory_wiring::MemoryEnginesConfig {
                        distiller: agent_memory.distiller.clone(),
                        steward: agent_memory.steward.clone(),
                    };
                    let stack = match meerkat_mobkit::memory_wiring::attach_memory_engines(
                        provider.clone(),
                        &agent_memory.config,
                        &engines,
                        meerkat_mobkit::memory_wiring::MemoryStackSeams {
                            persistent_state: persistent_state.clone(),
                            transcript_store: memory_transcript_store,
                            event_sink: Some(memory_events.clone()),
                            mob_purpose: Some(Arc::new(GatewayMobPurposeSource {
                                mob: mob_definition.id.to_string(),
                                roster: steward_roster_slot.clone(),
                            })),
                            steward_gating: Some(Arc::new(GatewayMemoryGatingBridge {
                                runtime: steward_late_runtime.clone(),
                            })),
                            steward_conflicts: Some(Arc::new(GatewayMemoryConflictBridge {
                                runtime: steward_late_runtime.clone(),
                                handle: tokio::runtime::Handle::current(),
                            })),
                        },
                    ) {
                        Ok(stack) => stack,
                        Err(e) => fail_init(&request_id, -32602, e),
                    };
                    let panel = stack.panel.clone();
                    let tracker = stack.taint;
                    let mut sinks = stack.sinks;
                    agent_memory_distiller = stack.distiller;
                    if let Some(engine) = agent_memory_distiller.as_ref() {
                        let _ = engine;
                        tracing::info!(
                            model = ?agent_memory.distiller.model,
                            "agent memory distiller installed"
                        );
                    }
                    if let Some(engine) = stack.steward.clone() {
                        // Dream cadence (§ ask 7 / P5): when this gateway
                        // runs a schedule host, the dream is driven as a
                        // durable host-runnable occurrence (registered at
                        // the schedule-host spawn). Only gateways WITHOUT
                        // a schedule host keep the in-process loop.
                        if schedule_host_inputs.is_none() {
                            std::mem::forget(engine.spawn_dream_loop());
                        }
                        agent_memory_steward = Some(engine);
                        tracing::info!(
                            model = ?agent_memory.steward.model,
                            cadence = %agent_memory.steward.cadence,
                            per_mob = agent_memory.steward.per_mob,
                            "agent memory steward installed"
                        );
                    }
                    // §10.1 ask 5 (outbound half): make the member's comms
                    // runtime the authenticated carrier of the host's taint
                    // fact. Fire-and-forget: the member may not be
                    // materialized, so a miss is logged, not fatal.
                    let taint_mob_handle = runtime.mob_handle();
                    tracker.set_outbound_taint_declarer(std::sync::Arc::new(
                            move |identity: &str,
                                  taint: Option<meerkat_core::comms::SenderContentTaint>| {
                                let handle = taint_mob_handle.clone();
                                let member =
                                    meerkat_mobkit::member_comms_id::mob_member_id(identity);
                                let identity_owned = identity.to_string();
                                tokio::spawn(async move {
                                    if let Err(err) = handle
                                        .declare_member_outbound_taint(member, taint)
                                        .await
                                    {
                                        tracing::debug!(
                                            identity = %identity_owned,
                                            ?taint,
                                            error = %err,
                                            "agent memory taint: outbound taint declaration \
                                             failed (member may not be materialized)"
                                        );
                                    }
                                });
                            },
                        ));
                    // §9.3 console Memory panel: must precede
                    // build_reference_app_router (the router builds after
                    // this region). Registered when — and only when —
                    // the provider advertises the panel read API.
                    if let Some(panel) = panel {
                        runtime.set_memory_panel_store(panel);
                    }
                    // §9.1 as-built: ALWAYS-ON compaction reset for the
                    // coordinator's session budgets — deliberately NOT
                    // gated on distiller.enabled (gate finding: budgeted
                    // injection without a distiller never reset).
                    let compaction_reset_slot = agent_memory_compaction_reset.clone();
                    sinks.push(Arc::new(meerkat_mobkit::CompactionResetSink::new(
                        Arc::new(move |session: &str| {
                            if let Some(injector) = compaction_reset_slot.get() {
                                injector.on_session_compacted(session);
                            }
                        }),
                    )));
                    // §10.1 dispatch-time taint join: bind the tracker into
                    // the member pre-build seam so every member's LLM client
                    // marks untrusted ingestion synchronously - ahead of the
                    // async observer spawned below (first-ingestion race).
                    dispatch_taint_slot.fill(tracker.clone());
                    // Observe-stream feed lives for the gateway process;
                    // forgetting the guard keeps the task running.
                    std::mem::forget(meerkat_mobkit::spawn_member_event_observer(
                        runtime.mob_handle(),
                        sinks,
                    ));
                    agent_memory_taint = Some(tracker);
                    Some(stack.provider)
                } else {
                    // Recall-only provider by its capability flags (M4
                    // de-weld): no firewall, no engines, no panel — and any
                    // engine explicitly configured against it refuses
                    // loudly instead of silently not existing.
                    if agent_memory.distiller.enabled || agent_memory.steward.enabled {
                        fail_init(
                            &request_id,
                            -32602,
                            "agent memory distiller/steward require a provider \
                             with judgment-plane capabilities (the bundled sqlite store)"
                                .to_string(),
                        );
                    }
                    Some(provider)
                }
            } else {
                None
            };
        // §7.2 / §16 Q1 operator-resolver seam. The PROVISIONAL keying is
        // the console auth principal; that resolver lives with the console
        // auth wiring and plugs in here as one line of glue. Until it is
        // installed, `operator_scope = "provisional"` composes nothing
        // (inert by design) while steward routing (proposal-keyed) is
        // already active.
        // §16 Q1 provisional keying (decided 2026-07-04): OperatorId = the
        // console auth principal. When the scope is provisional, install the
        // console-principal resolver and share it with the runtime so the
        // console send path can note authenticated interactions; recall
        // composition activates for identities an authenticated principal
        // has addressed (config AND resolver AND a real principal).
        let agent_memory_operator_resolver: Option<
            Arc<dyn meerkat_mobkit::memory::coordinator::OperatorResolver>,
        > = if gateway_options.agent_memory.as_ref().is_some_and(|memory| {
            memory.config.operator_scope == meerkat_mobkit::AgentMemoryOperatorScope::Provisional
        }) {
            let resolver = Arc::new(meerkat_mobkit::ConsolePrincipalOperatorResolver::new());
            runtime.set_console_operator_resolver(resolver.clone());
            tracing::info!(
                "agent memory operator scope active (provisional keying: console auth principal)"
            );
            Some(resolver)
        } else {
            None
        };
        // Degrade LOUD, not silent: recall composition of the Operator scope
        // needs BOTH the knob and a resolver (coordinator.rs scope_set), so a
        // resolver-less provisional deployment gets steward proposal-routing
        // only. Without this warning that half-activation is invisible.
        if operator_scope_recall_inert(
            gateway_options.agent_memory.as_ref(),
            agent_memory_operator_resolver.is_some(),
        ) {
            tracing::warn!(
                "agent_memory.operator_scope=\"provisional\" is configured but this gateway \
                 installs no operator resolver: operator-scope recall composition is INERT \
                 (records routed to the operator scope will not be recalled or injected); \
                 steward routing of operator-scope proposals remains active"
            );
        }
        // §7.2 mob-scope binding: every identity this gateway hosts runs
        // inside the one mob the runtime fronts, so a static binding closes
        // the mob-memory read path (write-only otherwise — gate blocker).
        let agent_memory_mob_resolver: Option<
            Arc<dyn meerkat_mobkit::memory::coordinator::MobScopeResolver>,
        > = gateway_options.agent_memory.as_ref().map(|memory| {
            Arc::new(meerkat_mobkit::memory::coordinator::StaticMobBinding {
                realm: memory.config.realm.clone(),
                mob: runtime.mob_handle().mob_id().to_string(),
            }) as Arc<dyn meerkat_mobkit::memory::coordinator::MobScopeResolver>
        });
        let customizer: Option<
            Arc<dyn meerkat_mobkit::identity_first::contracts::AgentCustomizer>,
        > = if let (Some(agent_memory), Some(provider)) = (
            gateway_options.agent_memory.as_ref(),
            agent_memory_provider.clone(),
        ) {
            Some(Arc::new(
                meerkat_mobkit::AgentMemoryCustomizer::wrap(
                    base_customizer,
                    provider,
                    agent_memory.config.clone(),
                )
                .with_operator_resolver(agent_memory_operator_resolver.clone())
                .with_mob_resolver(agent_memory_mob_resolver.clone()),
            ))
        } else {
            base_customizer
        };
        let agent_memory_injector = if let (Some(agent_memory), Some(provider)) = (
            gateway_options.agent_memory.as_ref(),
            agent_memory_provider.clone(),
        ) {
            let mut injector = meerkat_mobkit::AgentMemoryRuntimeInjector::new(
                provider,
                agent_memory.config.clone(),
            );
            if let Some(tracker) = agent_memory_taint.clone() {
                injector = injector.with_taint_tracker(tracker);
            }
            if let Some(distiller) = agent_memory_distiller.clone() {
                injector = injector.with_distiller(distiller);
            }
            if let Some(steward) = agent_memory_steward.clone() {
                injector = injector.with_steward(steward);
            }
            injector = injector.with_operator_resolver(agent_memory_operator_resolver.clone());
            injector = injector.with_mob_resolver(agent_memory_mob_resolver.clone());
            // Arm the always-on compaction reset sink (state is Arc-shared
            // across injector clones, so resetting through this clone
            // resets the delivery path's budgets too).
            let _ = agent_memory_compaction_reset.set(injector.clone());
            Some(injector)
        } else {
            None
        };
        irt.set_agent_memory(agent_memory_injector).await;

        // Bootstrap identities from the roster provider using the explicit
        // gateway mode (eager remains the compatibility default).
        let roster_specs = roster
            .roster(&RosterContext {
                mob_definition: Some(mob_definition.clone()),
                previous_identities: Vec::new(),
            })
            .await
            .unwrap_or_else(|e| {
                fail_init(&request_id, -32603, format!("roster provider failed: {e}"));
            });

        // Mob-purpose context for the steward's promotion judgment: no
        // mob-level purpose field exists (verified — `MobDefinition` is
        // structural only), so the roster's labels are the source.
        *steward_roster_slot
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = roster_specs.clone();

        let identity_context = Arc::new(IdentityFirstRuntimeContext::new_with_bootstrap_mode(
            irt.clone(),
            roster.clone(),
            topology.clone(),
            customizer.clone(),
            Some(runtime.mob_handle().definition().clone()),
            gateway_options
                .identity_bootstrap_mode
                .clone()
                .unwrap_or_default(),
        ));
        if let Err(e) = runtime
            .install_and_bootstrap_identity_first_context(
                Arc::clone(&identity_context),
                &roster_specs,
            )
            .await
        {
            // `install_and_bootstrap_identity_first_context` has already run
            // the full UnifiedRuntime shutdown sequence. Keep the callback
            // bridge/stdin reader alive until that returns: external lease
            // release is itself a callback that must be acknowledged before
            // the process emits the init error and exits.
            fail_init(
                &request_id,
                -32603,
                format!("identity-first bootstrap failed: {e}"),
            );
        }

        Some(meerkat_mobkit::rpc::IdentityFirstContext {
            runtime: irt,
            roster_provider: roster,
            topology_provider: topology,
            customizer,
            agent_memory_provider,
            mob_definition: Some(mob_definition),
            transcript_edit_service: gateway_transcript_edit_service.clone(),
            compaction_floors: Some(compaction_floors),
        })
    } else {
        None
    };

    // Run the schedule driver after identity-first restore, so legacy
    // resumable-session repair can see live member bridge-session bindings and
    // due occurrences do not race identity materialization.
    let (_schedule_host, _schedule_watchdog) = if let Some((
        schedule_service,
        mob_target_registry,
        service,
        adapter,
        schedule_store_path,
        schedule_firing_host_binding,
    )) = schedule_host_inputs
    {
        // Shared with mobkit_gateway. Kept a separate call from the host spawn
        // below on purpose: this gateway registers the steward dream BETWEEN
        // the two, and folding them into one helper would have silently
        // reordered a durable-store boot sequence.
        let mob_state = meerkat_mobkit::gateway_composition::adopt_schedule_mob_targets(
            runtime,
            &schedule_service,
            &mob_target_registry,
        )
        .await;
        // §8.5 / upstream ask 7 (P5): drive the memory steward's dream through
        // the durable schedule host instead of a bare interval loop. Register
        // the dream as a host runnable and find-or-create its cadence schedule
        // (idempotent across boots via the persistent store). The in-process
        // fallback loop is spawned only when there is no schedule host.
        // SDK-registered runnables (`runtime_options.host_runnables`) compose
        // into the same registry: their fires forward over the callback
        // bridge as `callback/schedule_fire`, so apps get deterministic
        // (non-LLM) schedule targets.
        let callback_runnables = (!gateway_options.host_runnables.is_empty()).then(|| {
            (
                Arc::new(bridge.clone())
                    as Arc<dyn meerkat_mobkit::identity_first::gateway_bridges::CallbackBridge>,
                gateway_options.host_runnables.clone(),
            )
        });
        let runnable_host = match meerkat_mobkit::schedule_wiring::gateway_runnable_host(
            agent_memory_steward.clone(),
            callback_runnables,
        ) {
            Ok(host) => host,
            Err(error) => {
                // Structurally unreachable: names are deduplicated and the
                // reserved steward name rejected at option parse time.
                fail_init(
                    &request_id,
                    -32602,
                    format!("failed to compose schedule host runnables: {error}"),
                );
            }
        };
        if let Some(steward) = agent_memory_steward.as_ref()
            && let Err(error) = meerkat_mobkit::schedule_wiring::ensure_steward_dream_schedule(
                &schedule_service,
                steward.dream_cadence(),
                chrono::Utc::now(),
            )
            .await
        {
            tracing::warn!(
                error = %error,
                "failed to ensure steward dream schedule; the steward will not dream on this gateway",
            );
        }
        // Shared with mobkit_gateway: the watchdog liveness contract log, the
        // host spawn, the firing-intent gate bind and the boot probe were
        // byte-identical in both binaries, every log string included. The one
        // real divergence, this gateway's runnable host (steward dream + the
        // SDK-declared `runtime_options.host_runnables` composed just above),
        // is now a named FIELD instead of a positional argument nobody could
        // see was a divergence.
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
                    runnable_host,
                    workgraph_service: workgraph_service.clone(),
                    owner_id: schedule_owner_id.clone(),
                },
            )
            .await;
        (schedule_host, Some(watchdog))
    } else {
        if !gateway_options.host_runnables.is_empty() {
            tracing::warn!(
                "runtime_options.host_runnables is configured but this gateway runs no \
                 schedule host (ephemeral mode, or schedule store unavailable): \
                 host-runnable schedule targets will never fire"
            );
        }
        (None, None)
    };

    // Cross-mob surfaces: install the inline contact directory (while the
    // runtime is still exclusively owned), the gateway signing identity,
    // and the control listener. The listener starts after identity-first
    // attachment above, but its handler re-reads the identity authority
    // per request either way.
    if let Some(directory) = gateway_options.contacts.clone() {
        runtime.set_contact_directory(directory);
    }
    // The gateway keypair signs cross-mob control responses (peers with
    // this gateway's pubkey pinned verify them) and backs mobkit/peer_pubkey.
    // Persistent boots keep it stable across restarts in the state dir;
    // ephemeral boots mint a per-process key.
    let gateway_peer_keys = match persistent_state.as_ref() {
        Some(state_path) => match meerkat_mobkit::GatewayPeerKeys::load_or_create(state_path) {
            Ok(keys) => keys,
            Err(error) => fail_init(
                &request_id,
                -32603,
                format!(
                    "failed to load or mint the gateway peer key under {}: {error}",
                    state_path.display()
                ),
            ),
        },
        None => meerkat_mobkit::GatewayPeerKeys::ephemeral(),
    };
    runtime.set_gateway_peer_keys(gateway_peer_keys);
    let control_listen_address = match control_listen.as_ref() {
        Some(addr) => {
            let authorizer = Arc::new(ControlAuthorizer::with_grants_for_audience(
                std::mem::take(&mut gateway_options.control_grants),
                runtime.mob_id(),
            ));
            match runtime
                .start_control_listener_with_authorizer(addr, authorizer)
                .await
            {
                Ok(advertised) => {
                    tracing::info!(%advertised, "cross-mob control listener bound");
                    Some(advertised)
                }
                Err(error) => fail_init(
                    &request_id,
                    -32603,
                    format!("--control-listen {addr}: {error}"),
                ),
            }
        }
        None => None,
    };

    let composition = composition.activate();
    let runtime = Arc::clone(composition.runtime());
    if let Some(detached_jobs) = gateway_detached_jobs.as_ref() {
        match detached_jobs.health_projection().await {
            Ok(projection) => runtime.set_job_health_projection(Some(projection)),
            Err(error) => {
                tracing::warn!(%error, "initial durable callback health projection failed");
                runtime.set_job_health_projection(Some(json!({
                    "status": "degraded",
                    "detached_jobs": {
                        "status": "degraded",
                        "reason": "job_health_projection_failed"
                    }
                })));
            }
        }
        detached_jobs.arm_delivery_driver(Arc::clone(&runtime));
    }
    // Bind the steward's late runtime seams and wire gating decisions back
    // to staged promotion commits (§10.2).
    steward_late_runtime.bind(runtime.clone());
    if let Some(steward) = agent_memory_steward.clone() {
        runtime
            .register_gating_resolution_observer(Arc::new(
                meerkat_mobkit::PromotionGateResolver::new(
                    steward,
                    tokio::runtime::Handle::current(),
                ),
            ))
            .await;
    }
    let event_drain_task = runtime.clone().spawn_event_drain_task();

    // 6. Bind HTTP server on ephemeral port
    let http_binding = meerkat_mobkit::gateway_composition::GatewayHttpBinding::bind_loopback()
        .await
        .expect("bind ephemeral port");
    let port = http_binding.port();
    let http_base_url = http_binding.http_base_url();

    // 7. Start HTTP with graceful shutdown
    let mut decision_state = gateway_options
        .decisions
        .clone()
        .unwrap_or_else(minimal_decision_state);
    decision_state.console.ui = gateway_options.console_ui.clone();
    let app = runtime.build_reference_app_router(decision_state);
    // Live (realtime) transport: mount the live WebSocket router on the SAME
    // HTTP listener the console uses (no second port — a LAN client or the
    // host's reverse proxy reaches it at {base}/live/ws), and erase the
    // live/* RPC handler over the gateway's concrete session-builder type
    // for the stdin dispatch loop.
    let (app, live_rpc) = if let Some((live_service, live_machine, live_agent_factory)) =
        live_inputs
    {
        let ws_base_url = match &gateway_options.live {
            GatewayLiveOption::Enabled {
                public_base_url: Some(public),
                ..
            } => public.trim_end_matches('/').to_string(),
            _ => format!("ws://127.0.0.1:{port}"),
        };
        let live_seed_max_chars = match &gateway_options.live {
            GatewayLiveOption::Enabled { seed_max_chars, .. } => *seed_max_chars,
            GatewayLiveOption::Disabled => None,
        };
        let live_ctx = Arc::new(meerkat_mobkit::live_wiring::attach_live(
            Arc::clone(&live_service),
            Arc::clone(&live_machine),
            &live_agent_factory,
            meerkat::Config::default(),
            ws_base_url,
            live_seed_max_chars,
        ));
        let app = if matches!(gateway_options.live, GatewayLiveOption::Enabled { .. }) {
            app.merge(meerkat_live::live_ws_router(Arc::clone(&live_ctx.ws_state)))
        } else {
            app
        };
        #[cfg(feature = "experimental-gpt-live")]
        let capability_provider = if let Some(experimental) =
            gateway_options.experimental_live.as_ref()
        {
            let mob_mcp_state = runtime
                .mob_runtime()
                .agent_mob_mcp_state()
                .unwrap_or_else(|| {
                    fail_init(
                        &request_id,
                        -32602,
                        "runtime_options.experimental_live requires the owning Mob MCP state"
                            .to_string(),
                    )
                });
            let access = gateway_options
                .access
                .clone()
                .unwrap_or_else(meerkat_mobkit::AccessController::disabled);
            let binding_authority = Arc::new(GatewayExperimentalLiveSessionBindingAuthority {
                handle: runtime.mob_handle(),
                machine: Arc::clone(&live_machine),
                access,
                principal: experimental.principal.clone(),
                allowed_binding: experimental.binding.clone(),
            });
            let transport =
                Arc::new(meerkat::experimental_gpt_live::ExperimentalGptLiveWebrtcTransport::new());
            let open_authority = Arc::new(
                meerkat::experimental_gpt_live::ExperimentalGptLiveOpenAuthority::new(
                    meerkat::experimental_gpt_live::ExperimentalGptLiveOpenAuthorityConfig {
                        agent_factory: live_agent_factory.clone(),
                        config_source: live_ctx.experimental_live_config_source(),
                        binding_authority,
                        realm: experimental.realm.clone(),
                        factory_identity: experimental.factory.clone(),
                        transport: Arc::clone(&transport),
                        voice: experimental.voice.clone(),
                        instructions: experimental.instructions.clone(),
                    },
                )
                .unwrap_or_else(|error| {
                    fail_init(
                        &request_id,
                        -32602,
                        format!("runtime_options.experimental_live composition failed: {error}"),
                    )
                }),
            );
            meerkat_mobkit::live_wiring::LiveCapabilityProvider::experimental(
                Arc::new(live_agent_factory.clone()),
                experimental.realm.clone(),
                experimental.factory.clone(),
                open_authority,
                transport,
                mob_mcp_state,
                Arc::new(StdioExperimentalLivePublicObservationPublisher::new(
                    Arc::clone(&live_machine),
                    bridge.clone(),
                )),
            )
        } else {
            meerkat_mobkit::live_wiring::LiveCapabilityProvider::disabled()
        };
        #[cfg(feature = "experimental-gpt-live")]
        let handler = meerkat_mobkit::live_wiring::live_rpc_handler_with_capabilities(
            live_ctx,
            live_service,
            live_machine,
            capability_provider,
        );
        #[cfg(not(feature = "experimental-gpt-live"))]
        let handler =
            meerkat_mobkit::live_wiring::live_rpc_handler(live_ctx, live_service, live_machine);
        (app, Some(handler))
    } else {
        (app, None)
    };
    let http_server = http_binding.serve(app);

    // 8. Send init response via stdout channel
    let loaded_modules = runtime.loaded_modules().await;
    let runtime_origin = if is_workspace_config {
        "workspace_config"
    } else {
        "fallback_minimal"
    };
    let runtime_fingerprint = {
        let mut hasher = Sha256::new();
        hasher.update(mob_config_toml.as_bytes());
        hasher.update(
            serde_json::to_string(&loaded_modules)
                .unwrap_or_default()
                .as_bytes(),
        );
        format!("{:x}", hasher.finalize())
    };
    let identity_bootstrap = identity_ctx
        .as_ref()
        .map(|ctx| ctx.runtime.identity_bootstrap_status());
    let init_response = json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "http_base_url": http_base_url,
            "loaded_modules": loaded_modules,
            "contract_version": MOBKIT_CONTRACT_VERSION,
            "runtime_origin": runtime_origin,
            "runtime_fingerprint": runtime_fingerprint,
            "identity_bootstrap": identity_bootstrap,
            // Private transport capability. SDKs use this to avoid sending
            // the shutdown control method to older/custom gateways that may
            // not implement normal JSON-RPC method-not-found semantics.
            "stdio_shutdown_handshake": true,
            // Complete, bounded host wait for the private shutdown request.
            // SDKs keep callback admission and stdin alive for this horizon.
            "stdio_shutdown_horizon_ms": GATEWAY_SHUTDOWN_HORIZON_MS,
            // Dialable address of the cross-mob control listener when the
            // gateway was launched with --control-listen (`tcp://ip:port`
            // with the real kernel-assigned port for host:0 binds, or
            // `uds:///path`); null otherwise. Peers put this address in
            // their contact directories. Wire-additive: older SDKs ignore
            // unknown result fields.
            "control_listen_address": control_listen_address,
        }
    });
    let _ = stdout_tx
        .send(GatewayStdoutLine::plain(
            serde_json::to_string(&init_response)
                .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string()),
        ))
        .await;

    // 9. RPC dispatch loop: each request runs on its own task. The loop must
    // never await a handler inline — a turn-running RPC can block on a
    // Python/TS callback round-trip, and the host may issue further RPCs
    // (e.g. mobkit/agent_memory/recall) from INSIDE that callback. With
    // sequential dispatch those reentrant requests starve behind the turn
    // until the callback times out (HomeCore recall deadlock). Both SDK
    // transports match responses by id, and the HTTP surface already serves
    // the same methods concurrently, so completion order is free.
    let identity_ctx = identity_ctx.map(Arc::new);
    let http_base_url_shared: Arc<str> = http_base_url.clone().into();
    let mut interrupted_with_open_stdin = false;
    let mut gateway_shutdown = None;
    {
        let mut inflight = tokio::task::JoinSet::new();
        loop {
            let request_line = tokio::select! {
                line = rpc_rx.recv() => line,
                // SIGINT *and* SIGTERM: a container stop sends SIGTERM, and
                // waiting on ctrl_c alone meant the graceful path never ran
                // on an ordinary deploy, leaving the schedule executor lease
                // held for up to its duration. See
                // `meerkat_mobkit::shutdown_signal`.
                _ = meerkat_mobkit::shutdown_signal::shutdown_signal() => {
                    interrupted_with_open_stdin = true;
                    None
                },
            };
            let Some(request_line) = request_line else {
                break; // stdin reader closed (EOF/error), or Ctrl-C won
            };
            if let Ok(message) = serde_json::from_str::<Value>(&request_line)
                && let Some(request) = gateway_shutdown_request(&message)
            {
                gateway_shutdown = Some(request);
                break;
            }
            let request_line =
                apply_gateway_runtime_config_to_request(&request_line, &gateway_options.gating);
            let runtime = runtime.clone();
            let stdout_tx = stdout_tx.clone();
            let identity_ctx = identity_ctx.clone();
            let http_base_url = http_base_url_shared.clone();
            let live_rpc = live_rpc.clone();
            let gateway_detached_jobs = gateway_detached_jobs.clone();
            let callback_bridge = bridge.clone();
            inflight.spawn(async move {
                let response = match handle_callback_job_rpc(
                    &request_line,
                    gateway_detached_jobs.as_ref(),
                    &callback_bridge,
                )
                .await
                {
                    Some(response) => GatewayStdoutLine::plain(response),
                    None => GatewayStdoutLine::delivery(
                        meerkat_mobkit::rpc::handle_unified_rpc_json_with_live_arc_delivery(
                            &runtime,
                            &request_line,
                            timeout,
                            Some(http_base_url.as_ref()),
                            identity_ctx.as_deref(),
                            live_rpc.as_ref(),
                        )
                        .await,
                    ),
                };
                if !response.is_empty() {
                    let _ = stdout_tx.send(response).await;
                }
            });
            // Reap completed handlers so the set does not grow unbounded.
            while inflight.try_join_next().is_some() {}
        }
        // EOF/Ctrl-C makes further callback responses impossible (or asks us
        // to stop immediately), so wake callback waiters. An explicit SDK
        // shutdown handshake is different: stdin stays open and callback
        // admission must survive until runtime-owned provider cleanup ends.
        if gateway_shutdown.is_none() {
            bridge.close().await;
        }
        // EOF closes request admission. Give ordinary handlers a bounded
        // response grace, then abort their outer waiters so runtime shutdown
        // can begin promptly. Identity materialization/delivery transactions
        // are independently owned by the runtime foreground supervisor and
        // are cancelled or joined below at their explicit cleanup boundary.
        let drain = async { while inflight.join_next().await.is_some() {} };
        if tokio::time::timeout(GATEWAY_RPC_DRAIN_TIMEOUT, drain)
            .await
            .is_err()
        {
            inflight.shutdown().await;
        }
    }

    // 10. Graceful shutdown: stop HTTP admission, bound the outer-handler
    // drain, then let the runtime cancel/join identity-owned transactions.
    // Dispatch can also end on Ctrl-C while stdin is still open. Stop the
    // reader now unless the SDK handshake must continue routing callback
    // responses through runtime shutdown. Aborting an already-completed EOF
    // reader is harmless.
    if gateway_shutdown.is_none() {
        stdin_reader.abort();
    }
    let shutdown = composition
        .shutdown(
            http_server,
            || async move {
                event_drain_task.abort();
                let _ = event_drain_task.await;
            },
            || async {},
        )
        .await;
    let runtime_shutdown = shutdown.runtime;
    bridge.close().await;
    if let Some(request) = gateway_shutdown {
        let response = gateway_shutdown_response(request.response_id, runtime_shutdown.as_ref());
        if let Ok(line) = serde_json::to_string(&response) {
            let _ = stdout_tx.send(GatewayStdoutLine::plain(line)).await;
        }
        // The SDK closes stdin after receiving the response. Abort our Tokio
        // reader task now; that close also releases Tokio's blocking stdin
        // helper before the runtime itself is dropped.
        stdin_reader.abort();
    }
    drop(stdout_tx);
    // Runtime and callback objects intentionally retain sender clones until
    // function exit. Signal the writer explicitly after all producers have
    // quiesced so it drains queued responses without waiting for channel
    // ownership to disappear.
    let _ = stdout_shutdown_tx.send(());
    let _ = stdin_reader.await;
    if tokio::time::timeout(GATEWAY_STDOUT_DRAIN_TIMEOUT, &mut stdout_writer)
        .await
        .is_err()
    {
        stdout_writer.abort();
        let _ = stdout_writer.await;
    }
    drop(_temp_dir);
    if interrupted_with_open_stdin {
        // Tokio's stdin adapter performs a blocking read that cannot be
        // cancelled by aborting its task. All MobKit/runtime/stdout cleanup is
        // complete above; exit explicitly so the runtime destructor does not
        // wait forever for that helper thread while stdin remains open.
        std::process::exit(0);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().skip(1).any(|a| a == "--version" || a == "-V") {
        println!(
            "rpc_gateway {} (meerkat-mobkit SDK stdin-RPC gateway)",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }
    // --control-listen <tcp://host:port | uds:///path>: bind the cross-mob
    // control listener so remote gateways can wire/unwire/inject/lookup
    // members of this runtime. Mirrors the mobkit_gateway flag; validated
    // here so a typo is a launch error, not a silently ignored flag. The
    // bound address is reported back in the mobkit/init response as
    // `control_listen_address` (important for tcp://host:0 ephemeral ports).
    let control_listen =
        match meerkat_mobkit::gateway_composition::parse_control_listen_arg(&args[1..]) {
            Ok(value) => value,
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(2);
            }
        };
    if args.iter().any(|a| a == "--persistent") {
        run_persistent(control_listen);
    } else {
        if control_listen.is_some() {
            eprintln!(
                "--control-listen requires --persistent (the single-shot gateway hosts no long-lived runtime to serve control RPC)"
            );
            std::process::exit(2);
        }
        run_single_shot(&args[1..]);
    }
}

// ---------------------------------------------------------------------------
// §8.5 steward runtime bridges
// ---------------------------------------------------------------------------

/// Late-bound Arc'd runtime for the steward's gating/conflict bridges: the
/// engine is constructed during identity-first init, before
/// `Arc::new(runtime)` exists; the cell binds right after.
#[derive(Clone, Default)]
struct StewardLateRuntime(Arc<tokio::sync::OnceCell<Arc<UnifiedRuntime>>>);

impl StewardLateRuntime {
    fn bind(&self, runtime: Arc<UnifiedRuntime>) {
        let _ = self.0.set(runtime);
    }

    fn get(&self) -> Option<Arc<UnifiedRuntime>> {
        self.0.get().cloned()
    }
}

/// Mob-purpose context from the hosted mob's id plus the restored roster's
/// labels (no mob-level purpose field exists — verified; a `mob_purpose`
/// or `purpose` label on any member spec is adopted as the mob's purpose).
struct GatewayMobPurposeSource {
    mob: String,
    roster: Arc<std::sync::Mutex<Vec<meerkat_mobkit::identity_first::DurableAgentSpec>>>,
}

impl meerkat_mobkit::MobPurposeSource for GatewayMobPurposeSource {
    fn mob_contexts(&self) -> Vec<meerkat_mobkit::memory::steward::MobContext> {
        let roster = self.roster.lock().unwrap_or_else(|err| err.into_inner());
        let purpose = roster.iter().find_map(|spec| {
            spec.labels
                .get("mob_purpose")
                .or_else(|| spec.labels.get("purpose"))
                .cloned()
        });
        let member_labels = roster
            .iter()
            .map(|spec| (spec.identity.as_str().to_string(), spec.labels.clone()))
            .collect();
        vec![meerkat_mobkit::memory::steward::MobContext {
            mob: self.mob.clone(),
            purpose,
            member_labels,
        }]
    }
}

/// Quarantine-promotion gate enqueue over the runtime's gating engine
/// (§10.2): risk tier R3, so the evaluation mints a pending entry the
/// operator decides through the existing console/RPC gating flow.
struct GatewayMemoryGatingBridge {
    runtime: StewardLateRuntime,
}

#[async_trait]
impl meerkat_mobkit::MemoryGatingBridge for GatewayMemoryGatingBridge {
    async fn enqueue_promotion_gate(
        &self,
        realm: &str,
        description: &str,
        entity: &str,
        topic: &str,
    ) -> Result<String, String> {
        let Some(runtime) = self.runtime.get() else {
            return Err("runtime not yet bound".to_string());
        };
        let result = runtime
            .evaluate_gating_action(meerkat_mobkit::runtime::GatingEvaluateRequest {
                action: description.to_string(),
                actor_id: format!("memory-steward:{realm}"),
                risk_tier: meerkat_mobkit::runtime::GatingRiskTier::R3,
                rationale: Some(
                    "memory steward quarantine promotion (agent-memory §10.2)".to_string(),
                ),
                requested_approver: None,
                approval_recipient: None,
                approval_channel: None,
                approval_timeout_ms: None,
                entity: Some(entity.to_string()),
                topic: Some(topic.to_string()),
            })
            .await;
        result.pending_id.ok_or_else(|| {
            format!(
                "gating evaluation returned outcome {:?} without a pending entry{}",
                result.outcome,
                result
                    .fallback_reason
                    .as_deref()
                    .map(|reason| format!(" ({reason})"))
                    .unwrap_or_default()
            )
        })
    }
}

/// Contradiction bridge (§8.5): dream findings with operational
/// consequence land as `MemoryConflictSignal`s in the operational ledger,
/// where gating's R2/R3 conflict probe reads them. Fire-and-forget.
struct GatewayMemoryConflictBridge {
    runtime: StewardLateRuntime,
    handle: tokio::runtime::Handle,
}

impl meerkat_mobkit::MemoryConflictBridge for GatewayMemoryConflictBridge {
    fn emit_conflict(&self, entity: &str, topic: &str, reason: &str) {
        let Some(runtime) = self.runtime.get() else {
            tracing::warn!(
                entity,
                topic,
                "memory conflict bridge: runtime not yet bound"
            );
            return;
        };
        let entity = entity.to_string();
        let topic = topic.to_string();
        let reason = reason.to_string();
        self.handle.spawn(async move {
            let request = meerkat_mobkit::runtime::MemoryIndexRequest {
                entity: entity.clone(),
                topic: topic.clone(),
                store: None,
                fact: None,
                metadata: None,
                conflict: Some(true),
                conflict_reason: Some(reason),
            };
            if let Err(err) = runtime.memory_index(request).await {
                tracing::warn!(
                    entity,
                    topic,
                    error = ?err,
                    "memory conflict bridge: conflict signal write failed"
                );
            }
        });
    }
}
