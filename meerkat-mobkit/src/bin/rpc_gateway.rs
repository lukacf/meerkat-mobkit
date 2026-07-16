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

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use meerkat_mobkit::unified_runtime::EventLogError;
use meerkat_mobkit::{
    AuthPolicy, AuthProvider, Base64BlobStoreAdapter, BigQueryNaming, BinaryBlobStore,
    ConsolePolicy, ConsoleUiConfig, DiscoverySpec, EventLogConfig, EventLogStore, EventQuery,
    InMemoryMetadataStore, LocalJsonMemoryBackendConfig, MOBKIT_CONTRACT_VERSION,
    MemoryBackendConfig, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig, ModuleConfig,
    ObjectStoreBlobStore, PersistedEvent, PersistentMetadataStore, PreSpawnData, ReleaseMetadata,
    RestartPolicy, RuntimeDecisionState, RuntimeOpsPolicy, RuntimeOptions, RuntimeRoute,
    ScheduleDefinition, SqliteConsoleLogStore, SqliteMetadataStore, TrustedOidcRuntimeConfig,
    UnifiedRuntime, handle_mobkit_rpc_json, load_console_ui_config_from_path_for_realm,
    mob_handle_runtime::{
        ensure_shell_tooling_build_substrate, mob_definition_may_use_image_generation,
        mob_definition_may_use_shell,
    },
    start_mobkit_runtime,
};
use sha2::{Digest, Sha256};

use async_trait::async_trait;
use meerkat::{
    AgentEvent, AgentFactory, Config, CreateSessionRequest, EphemeralSessionService, FactoryAgent,
    FactoryAgentBuilder, SessionAgentBuilder, SessionError,
};
use meerkat_core::AgentToolDispatcher;
use meerkat_core::ContentBlock;
use meerkat_core::error::{AgentError, ToolError};
use meerkat_core::ops::ToolDispatchOutcome;
use meerkat_core::types::{ToolCallView, ToolDef, ToolResult};
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
    identity_bootstrap_mode: meerkat_mobkit::IdentityBootstrapMode,
    max_sessions: usize,
    routing_routes: Vec<RuntimeRoute>,
    schedules: Vec<ScheduleDefinition>,
    gating: GatewayGatingConfig,
    event_log: Option<EventLogConfig>,
    decisions: Option<RuntimeDecisionState>,
    console_ui: ConsoleUiConfig,
    console_require_app_auth: Option<bool>,
    console_read_only: Option<bool>,
    console_fetch_timeout_ms: Option<u64>,
    access: Option<meerkat_mobkit::AccessController>,
    demo_llm: bool,
    agent_memory: Option<GatewayAgentMemoryOptions>,
    /// WorkGraph service construction switch (default on). `false` disables
    /// the store, member tools, overlays, and the mobkit/workgraph/* RPCs.
    workgraph: GatewayWorkgraphOption,
    /// Live (realtime) transport opt-in (default off). Persistent mode only.
    live: GatewayLiveOption,
    /// SDK-registered deterministic schedule targets
    /// (`runtime_options.host_runnables`): each name registers a schedule
    /// host runnable whose fire forwards over the callback bridge as
    /// `callback/schedule_fire`.
    host_runnables: Vec<meerkat::HostRunnableName>,
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
    /// §8.3 selector switch from `agent_memory.selector`. `None` = not
    /// configured; the wiring then falls back to the
    /// `MOBKIT_AGENT_MEMORY_SELECTOR` env var (config takes precedence).
    selector: Option<meerkat_mobkit::memory::selector::SelectorSpec>,
    /// §8.4 distiller block from `agent_memory.distiller`. Disabled by
    /// default (flipping it is a calibration decision, §11).
    distiller: meerkat_mobkit::memory::distiller::DistillerConfig,
    /// §8.5 steward block from `agent_memory.steward`. Disabled by
    /// default; enablement is the application's call (mechanism from
    /// MobKit, policy from the app).
    steward: meerkat_mobkit::memory::steward::StewardConfig,
    /// §8.6 hygienist block from `agent_memory.hygienist`. Disabled by
    /// default (§15 ships it last; flipping is a calibration decision).
    hygienist: meerkat_mobkit::memory::hygienist::HygienistConfig,
}

/// Which bundled store backs agent memory. SQLite is the default now that
/// the P1 recall coordinator and injection ledger ride on it
/// (docs/design/agent-memory-architecture.md §15); existing markdown files
/// are auto-imported on first open. Markdown remains selectable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum GatewayAgentMemoryStoreKind {
    Markdown,
    #[default]
    Sqlite,
}

/// §7.2: `operator_scope = "provisional"` composes operator-scope recall
/// only when an `OperatorResolver` is installed; the shipped gateway
/// installs none, so a provisional deployment without one runs steward
/// proposal-routing while recall composition is INERT. True exactly when
/// the startup warning about that half-activation must fire.
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
            identity_bootstrap_mode: meerkat_mobkit::IdentityBootstrapMode::default(),
            max_sessions: 16,
            routing_routes: Vec::new(),
            schedules: Vec::new(),
            gating: GatewayGatingConfig::default(),
            event_log: None,
            decisions: None,
            console_ui: ConsoleUiConfig::default(),
            console_require_app_auth: None,
            console_read_only: None,
            console_fetch_timeout_ms: None,
            access: None,
            demo_llm: false,
            agent_memory: None,
            workgraph: GatewayWorkgraphOption::Enabled,
            live: GatewayLiveOption::Disabled,
            host_runnables: Vec::new(),
        }
    }
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
        let (stdout_tx, _stdout_rx) = mpsc::channel::<String>(4);
        StdioCallbackBridge::new(stdout_tx)
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
                "input_schema": schema
            }))
            .expect("object spec"),
        ];
        let dispatcher =
            CallbackToolDispatcher::new(test_callback_bridge(), "build-1".to_string(), specs);
        let defs = AgentToolDispatcher::tools(&dispatcher);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].name.as_ref(), "legacy_name");
        assert_eq!(defs[0].description, "Python callback tool");
        assert_eq!(defs[0].input_schema, json!({"type": "object"}));
        assert_eq!(defs[1].name.as_ref(), "weather");
        assert_eq!(defs[1].description, "Look up the weather");
        assert_eq!(defs[1].input_schema, schema);
    }

    #[test]
    fn callback_tool_spec_rejects_malformed_wire_entries() {
        for value in [
            json!(7),
            json!({"description": "no name"}),
            json!({"name": ""}),
            json!({"name": "x", "input_schema": "not-an-object"}),
            json!({"name": "x", "description": 3}),
        ] {
            assert!(
                CallbackToolSpec::parse(&value).is_err(),
                "expected rejection for {value}"
            );
        }
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
    fn gateway_runtime_options_parse_max_sessions() {
        let params = json!({
            "runtime_options": {
                "max_sessions": 320
            }
        });

        let options = parse_gateway_runtime_options(&params, None).expect("runtime options");

        assert_eq!(options.max_sessions, 320);
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
    fn gateway_runtime_options_agent_memory_hygienist_parse_matrix() {
        let tmp = tempfile::tempdir().expect("tempdir");

        // Defaults: disabled, 2 runs/day, no model override.
        let params = json!({ "runtime_options": { "agent_memory": true } });
        let options =
            parse_gateway_runtime_options(&params, Some(tmp.path())).expect("defaults parse");
        let hygienist = options.agent_memory.expect("agent memory").hygienist;
        assert!(!hygienist.enabled);
        assert_eq!(hygienist.runs_per_day, 2);
        assert_eq!(hygienist.model, None);

        // Full object form.
        let params = json!({
            "runtime_options": {
                "agent_memory": {
                    "hygienist": {
                        "enabled": true,
                        "runs_per_day": 4,
                        "model": "claude-sonnet-4-6"
                    }
                }
            }
        });
        let hygienist = parse_gateway_runtime_options(&params, Some(tmp.path()))
            .expect("object form parses")
            .agent_memory
            .expect("agent memory")
            .hygienist;
        assert!(hygienist.enabled);
        assert_eq!(hygienist.runs_per_day, 4);
        assert_eq!(hygienist.model.as_deref(), Some("claude-sonnet-4-6"));

        // Bare true / bare object are opt-ins.
        let params = json!({
            "runtime_options": { "agent_memory": { "hygienist": true } }
        });
        assert!(
            parse_gateway_runtime_options(&params, Some(tmp.path()))
                .expect("bool form parses")
                .agent_memory
                .expect("agent memory")
                .hygienist
                .enabled
        );
        let params = json!({
            "runtime_options": { "agent_memory": { "hygienist": { "runs_per_day": 1 } } }
        });
        assert!(
            parse_gateway_runtime_options(&params, Some(tmp.path()))
                .expect("object without enabled parses")
                .agent_memory
                .expect("agent memory")
                .hygienist
                .enabled
        );

        // Fail-loud matrix.
        for (params, needle) in [
            (
                json!({ "runtime_options": { "agent_memory": { "hygienist": { "cadence": "*/6h" } } } }),
                "unsupported runtime_options.agent_memory.hygienist fields",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "hygienist": "on" } } }),
                "must be a boolean or object",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "hygienist": { "runs_per_day": 0 } } } }),
                "runs_per_day must be between 1 and 24",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "hygienist": { "runs_per_day": 48 } } } }),
                "runs_per_day must be between 1 and 24",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "hygienist": { "model": "" } } } }),
                "model must be a non-empty string",
            ),
            (
                json!({ "runtime_options": { "agent_memory": { "hygienist": { "enabled": "yes" } } } }),
                "enabled must be a boolean",
            ),
        ] {
            let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
                Ok(_) => panic!("expected fail-loud parse for {params}"),
                Err(err) => err,
            };
            assert!(err.contains(needle), "{err}");
        }

        // The §8.6 quarantine hard-block reads sqlite-store machinery.
        let params = json!({
            "runtime_options": {
                "agent_memory": { "store": "markdown", "hygienist": { "enabled": true } }
            }
        });
        let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
            Ok(_) => panic!("markdown + hygienist should fail loudly"),
            Err(err) => err,
        };
        assert!(err.contains("hygienist requires store='sqlite'"), "{err}");
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

    #[test]
    fn gateway_runtime_options_parse_agent_memory_selector() {
        use meerkat_mobkit::memory::selector::SelectorSpec;

        let tmp = tempfile::tempdir().expect("temp dir");

        // Default: not configured — the env var stays the fallback.
        for params in [
            json!({ "runtime_options": { "agent_memory": true } }),
            json!({ "runtime_options": { "agent_memory": {} } }),
        ] {
            let options = parse_gateway_runtime_options(&params, Some(tmp.path()))
                .expect("agent memory config should parse");
            let agent_memory = options.agent_memory.expect("agent memory options");
            assert_eq!(agent_memory.selector, None);
        }

        for (value, want) in [
            ("off", SelectorSpec::Off),
            ("default", SelectorSpec::Default),
            (
                "profile:/etc/mobkit/selector.toml",
                SelectorSpec::Profile(std::path::PathBuf::from("/etc/mobkit/selector.toml")),
            ),
        ] {
            let params = json!({
                "runtime_options": { "agent_memory": { "selector": value } }
            });
            let options = parse_gateway_runtime_options(&params, Some(tmp.path()))
                .expect("selector value should parse");
            let agent_memory = options.agent_memory.expect("agent memory options");
            assert_eq!(agent_memory.selector, Some(want), "selector '{value}'");
        }
    }

    #[test]
    fn gateway_runtime_options_reject_bad_agent_memory_selector() {
        let tmp = tempfile::tempdir().expect("temp dir");
        for (block, needle) in [
            (json!({ "selector": "on" }), "'off', 'default'"),
            (json!({ "selector": "profile:" }), "'off', 'default'"),
            (json!({ "selector": true }), "must be a string"),
            (
                // The markdown provider has no manifest; a configured
                // selector would silently do nothing.
                json!({ "store": "markdown", "selector": "default" }),
                "selector requires store='sqlite'",
            ),
        ] {
            let params = json!({ "runtime_options": { "agent_memory": block } });
            let err = match parse_gateway_runtime_options(&params, Some(tmp.path())) {
                Ok(_) => panic!("selector config must fail loudly: {params}"),
                Err(err) => err,
            };
            assert!(err.contains(needle), "{err} (wanted '{needle}')");
        }

        // Explicit off with markdown is fine (nothing to enforce).
        let params = json!({
            "runtime_options": {
                "agent_memory": { "store": "markdown", "selector": "off" }
            }
        });
        let options = parse_gateway_runtime_options(&params, Some(tmp.path()))
            .expect("selector=off with markdown should parse");
        let agent_memory = options.agent_memory.expect("agent memory options");
        assert_eq!(
            agent_memory.selector,
            Some(meerkat_mobkit::memory::selector::SelectorSpec::Off)
        );
    }

    #[test]
    fn selector_config_takes_precedence_over_env_fallback() {
        use meerkat_mobkit::memory::selector::SelectorSpec;

        // The crate denies `unsafe`, so the env var cannot be mutated in a
        // test; precedence is shown structurally instead. With the env
        // unset (nextest never sets MOBKIT_AGENT_MEMORY_SELECTOR), the env
        // fallback alone resolves to Off — so a non-Off result for a
        // configured spec proves the config path won, not the env.
        assert_eq!(
            resolve_selector_spec(Some(&SelectorSpec::Default)).expect("configured spec resolves"),
            SelectorSpec::Default,
            "agent_memory.selector must be used ahead of the env fallback"
        );
        let profile = SelectorSpec::Profile(std::path::PathBuf::from("/etc/mobkit/selector.toml"));
        assert_eq!(
            resolve_selector_spec(Some(&profile)).expect("configured profile resolves"),
            profile
        );
        assert_eq!(
            resolve_selector_spec(None).expect("env fallback resolves"),
            SelectorSpec::Off,
            "unset config must fall back to MOBKIT_AGENT_MEMORY_SELECTOR (unset ⇒ off)"
        );
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
        assert_eq!(
            defaults.identity_bootstrap_mode,
            IdentityBootstrapMode::EagerMaterialize
        );

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
            assert_eq!(options.identity_bootstrap_mode, expected);
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
        "scheduling_files",
        "gating_config_path",
        "auth_config",
        "access_config_path",
        "console_config_path",
        "console_require_app_auth",
        "console_read_only",
        "console_fetch_timeout_ms",
        "demo_llm",
        "max_sessions",
        "event_log",
        "agent_memory",
        "implicit_delegate_idle_retire_secs",
        "implicit_delegate_idle_sweep_interval_ms",
        "workgraph",
        "live",
        "host_runnables",
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
        parsed.identity_bootstrap_mode = parse_gateway_identity_bootstrap_mode(value)?;
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
    if let Some(files) = runtime_options.get("scheduling_files") {
        parsed.schedules = parse_gateway_scheduling_files(files)?;
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

fn parse_gateway_scheduling_files(files: &Value) -> Result<Vec<ScheduleDefinition>, String> {
    let files = files
        .as_array()
        .ok_or_else(|| "runtime_options.scheduling_files must be an array".to_string())?;
    let mut schedules = Vec::new();
    for file in files {
        let path = file.as_str().ok_or_else(|| {
            "runtime_options.scheduling_files entries must be strings".to_string()
        })?;
        let value = read_gateway_config_file(path, "scheduling_files")?;
        let schedules_value = value
            .get("schedules")
            .cloned()
            .unwrap_or_else(|| value.clone());
        let mut parsed: Vec<ScheduleDefinition> =
            serde_json::from_value(schedules_value).map_err(|err| {
                format!("runtime_options.scheduling_files schedule definitions are invalid: {err}")
            })?;
        schedules.append(&mut parsed);
    }
    meerkat_mobkit::evaluate_schedules_at_tick(&schedules, 0)
        .map_err(|err| format!("runtime_options.scheduling_files are invalid: {err:?}"))?;
    Ok(schedules)
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
        .ok_or_else(|| "runtime_options.event_log.storage must be 'memory'".to_string())?;
    if !matches!(storage, "memory" | "in_memory") {
        return Err(format!(
            "unsupported runtime_options.event_log.storage '{storage}'"
        ));
    }
    let batch_size = object
        .get("batch_size")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(64);
    let flush_interval_ms = object
        .get("flush_interval_ms")
        .and_then(Value::as_u64)
        .unwrap_or(1_000);
    Ok(EventLogConfig {
        store: Box::new(InMemoryEventLogStore::default()),
        filter: None,
        batch_size,
        flush_interval: Duration::from_millis(flush_interval_ms),
    })
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
    schedules: &[ScheduleDefinition],
    gating: &GatewayGatingConfig,
) -> String {
    let Ok(mut request) = serde_json::from_str::<Value>(request_line) else {
        return request_line.to_string();
    };
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    match method {
        "mobkit/scheduling/evaluate" | "mobkit/scheduling/dispatch" if !schedules.is_empty() => {
            let params = request.get_mut("params").and_then(Value::as_object_mut);
            if let Some(params) = params
                && !params.contains_key("schedules")
            {
                params.insert(
                    "schedules".to_string(),
                    serde_json::to_value(schedules).unwrap_or(Value::Null),
                );
            }
        }
        "mobkit/gating/evaluate" => {
            let params = request.get_mut("params").and_then(Value::as_object_mut);
            if let Some(params) = params
                && !params.contains_key("risk_tier")
                && let Some(action) = params.get("action").and_then(Value::as_str)
                && let Some(risk_tier) = gating.action_risk_tiers.get(action.trim())
            {
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

fn parse_gateway_agent_memory_config(
    agent_memory: &Value,
    persistent_state: Option<&std::path::Path>,
) -> Result<Option<GatewayAgentMemoryOptions>, String> {
    if let Some(enabled) = agent_memory.as_bool() {
        if !enabled {
            return Ok(None);
        }
        let path = persistent_state
            .ok_or_else(|| {
                "runtime_options.agent_memory=true requires persistent_state".to_string()
            })?
            .join("agent-memory");
        return Ok(Some(GatewayAgentMemoryOptions {
            config: meerkat_mobkit::AgentMemoryConfig::default(),
            path,
            store: GatewayAgentMemoryStoreKind::default(),
            selector: None,
            distiller: meerkat_mobkit::memory::distiller::DistillerConfig::default(),
            steward: meerkat_mobkit::memory::steward::StewardConfig::default(),
            hygienist: meerkat_mobkit::memory::hygienist::HygienistConfig::default(),
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
    // §8.3 selector switch. When set it takes precedence over the
    // MOBKIT_AGENT_MEMORY_SELECTOR env var; when absent the env var stays
    // the fallback.
    let selector = match object.get("selector") {
        None => None,
        Some(value) => {
            let value = value.as_str().map(str::trim).ok_or_else(|| {
                "runtime_options.agent_memory.selector must be a string \
                 ('off', 'default', or 'profile:<path>')"
                    .to_string()
            })?;
            match value {
                "off" => Some(meerkat_mobkit::memory::selector::SelectorSpec::Off),
                "default" => Some(meerkat_mobkit::memory::selector::SelectorSpec::Default),
                other => match other.strip_prefix("profile:") {
                    Some(path) if !path.trim().is_empty() => {
                        Some(meerkat_mobkit::memory::selector::SelectorSpec::Profile(
                            std::path::PathBuf::from(path.trim()),
                        ))
                    }
                    _ => {
                        return Err(format!(
                            "runtime_options.agent_memory.selector must be 'off', 'default', \
                             or 'profile:<path>' (got '{other}'); this option overrides the \
                             MOBKIT_AGENT_MEMORY_SELECTOR environment variable"
                        ));
                    }
                },
            }
        }
    };
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
    // §8.6 hygienist block: fail-loud parse; enabled defaults false.
    let hygienist = match object.get("hygienist") {
        None => meerkat_mobkit::memory::hygienist::HygienistConfig::default(),
        Some(value) => parse_gateway_hygienist_config(value)?,
    };
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
    // Same rationale for the selector: the markdown provider has no
    // manifest, so a configured selector would silently do nothing.
    if store == GatewayAgentMemoryStoreKind::Markdown
        && selector
            .as_ref()
            .is_some_and(|spec| *spec != meerkat_mobkit::memory::selector::SelectorSpec::Off)
    {
        return Err("runtime_options.agent_memory.selector requires store='sqlite'".to_string());
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
    // And for the hygienist: the §8.6 quarantine hard-block reads the
    // sqlite store's quarantine queue and evidence refs.
    if store == GatewayAgentMemoryStoreKind::Markdown && hygienist.enabled {
        return Err("runtime_options.agent_memory.hygienist requires store='sqlite'".to_string());
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
    let path = persistent_state
        .ok_or_else(|| "runtime_options.agent_memory requires persistent_state".to_string())?
        .join("agent-memory");

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
        selector,
        distiller,
        steward,
        hygienist,
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
    let supported = ["enabled", "runs_per_hour", "min_interactions", "model"];
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

/// Fail-loud parse of `runtime_options.agent_memory.hygienist` (§8.6):
/// `{enabled, runs_per_day, model}`; unknown fields and wrong types are
/// errors, never silently ignored.
fn parse_gateway_hygienist_config(
    value: &Value,
) -> Result<meerkat_mobkit::memory::hygienist::HygienistConfig, String> {
    let mut config = meerkat_mobkit::memory::hygienist::HygienistConfig::default();
    if let Some(enabled) = value.as_bool() {
        config.enabled = enabled;
        return Ok(config);
    }
    let object = value.as_object().ok_or_else(|| {
        "runtime_options.agent_memory.hygienist must be a boolean or object".to_string()
    })?;
    let supported = ["enabled", "runs_per_day", "model"];
    let unsupported = object
        .keys()
        .filter(|key| !supported.contains(&key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(format!(
            "unsupported runtime_options.agent_memory.hygienist fields: {}",
            unsupported.join(", ")
        ));
    }
    if let Some(enabled) = object.get("enabled") {
        config.enabled = enabled.as_bool().ok_or_else(|| {
            "runtime_options.agent_memory.hygienist.enabled must be a boolean".to_string()
        })?;
    } else {
        // An object block without `enabled` is an explicit opt-in.
        config.enabled = true;
    }
    if let Some(value) = object.get("runs_per_day") {
        let runs = value.as_u64().ok_or_else(|| {
            "runtime_options.agent_memory.hygienist.runs_per_day must be a positive integer"
                .to_string()
        })?;
        if runs == 0 || runs > 24 {
            return Err(
                "runtime_options.agent_memory.hygienist.runs_per_day must be between 1 and 24"
                    .to_string(),
            );
        }
        config.runs_per_day = runs as u32;
    }
    if let Some(value) = object.get("model") {
        let model = value
            .as_str()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .ok_or_else(|| {
                "runtime_options.agent_memory.hygienist.model must be a non-empty string"
                    .to_string()
            })?;
        config.model = Some(model.to_string());
    }
    Ok(config)
}

/// §8.3 selector precedence: `agent_memory.selector` config wins; the
/// `MOBKIT_AGENT_MEMORY_SELECTOR` env var stays as the fallback for
/// deployments that have not migrated to the config option.
fn resolve_selector_spec(
    configured: Option<&meerkat_mobkit::memory::selector::SelectorSpec>,
) -> Result<
    meerkat_mobkit::memory::selector::SelectorSpec,
    meerkat_mobkit::memory::selector::SelectorError,
> {
    match configured {
        Some(spec) => Ok(spec.clone()),
        None => meerkat_mobkit::memory::selector::spec_from_env(),
    }
}

/// Original single-shot mode: reads request from env, runs once, prints response.
fn run_single_shot() {
    let request = std::env::var("MOBKIT_RPC_REQUEST")
        .expect("MOBKIT_RPC_REQUEST must be set for rpc_gateway");

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

/// Shared handle for sending lines to stdout and receiving callback responses.
#[derive(Clone)]
struct StdioCallbackBridge {
    /// Send a line to stdout (the stdout writer task reads from this).
    stdout_tx: mpsc::Sender<String>,
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

impl StdioCallbackBridge {
    fn new(stdout_tx: mpsc::Sender<String>) -> Self {
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
            let _ = self.stdout_tx.try_send(line);
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
            if let Err(e) = self.stdout_tx.send(line).await {
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
        if let Err(_) = self.stdout_tx.send(line).await {
            self.state.lock().await.pending.remove(&id_str);
            return Err("stdout channel closed".to_string());
        }

        // Wait for Python to respond (routed by the stdin multiplexer)
        match tokio::time::timeout(Duration::from_mins(2), rx).await {
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
                Err("callback timed out after 120s".to_string())
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
}

impl CallbackToolSpec {
    fn parse(value: &Value) -> Result<Self, String> {
        if let Some(name) = value.as_str() {
            return Ok(Self {
                name: name.to_string(),
                description: None,
                input_schema: None,
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
        Ok(Self {
            name,
            description,
            input_schema,
        })
    }
}

/// Tool dispatcher that routes tool calls to Python via the callback bridge.
///
/// Created from the tool specs the SDK returned from `callback/build_agent`
/// (`add_tools()` names or `register_tool()` name + description + schema).
/// When the agent calls a tool, `dispatch()` sends `callback/call_tool` to
/// Python and returns the result.
struct CallbackToolDispatcher {
    bridge: StdioCallbackBridge,
    scope_id: String,
    tool_defs: Arc<[Arc<ToolDef>]>,
}

impl CallbackToolDispatcher {
    fn new(bridge: StdioCallbackBridge, scope_id: String, tools: Vec<CallbackToolSpec>) -> Self {
        let tool_defs: Vec<Arc<ToolDef>> = tools
            .into_iter()
            .map(|tool| {
                Arc::new(ToolDef {
                    name: tool.name.into(),
                    description: tool
                        .description
                        .unwrap_or_else(|| "Python callback tool".to_string()),
                    input_schema: tool
                        .input_schema
                        .unwrap_or_else(|| json!({"type": "object"})),
                    provenance: None,
                })
            })
            .collect();
        Self {
            bridge,
            scope_id,
            tool_defs: tool_defs.into(),
        }
    }
}

#[async_trait]
impl AgentToolDispatcher for CallbackToolDispatcher {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        Arc::clone(&self.tool_defs)
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
}

/// Wraps FactoryAgentBuilder — sends callback/build_agent to Python before building.
struct StdioCallbackAgentBuilder {
    inner: FactoryAgentBuilder,
    bridge: StdioCallbackBridge,
    has_session_builder: bool,
    /// Session store for loading sessions by ID when the Python builder
    /// sets `resume_session_id`. Only populated in persistent mode.
    session_store: Option<Arc<dyn meerkat::SessionStore>>,
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
    json!({
        "scope_id": scope_id,
        "session_id": labels.as_ref().and_then(|l| l.get("session_id")),
        "profile_name": profile_name,
        "model": &req.model,
        "prompt": &req.prompt,
        "labels": &labels,
        "app_context": req.build.as_ref()
            .and_then(|b| b.app_context.as_ref()),
    })
}

#[async_trait]
impl SessionAgentBuilder for StdioCallbackAgentBuilder {
    type Agent = FactoryAgent;

    async fn build_agent(
        &self,
        req: &CreateSessionRequest,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Self::Agent, SessionError> {
        if !self.has_session_builder {
            let mut normalized_req = CreateSessionRequest {
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
            ensure_shell_tooling_build_substrate(&mut normalized_req);
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
                // Apply additional_instructions as system prompt extension
                if let Some(instructions) = result.get("additional_instructions") {
                    if let Some(arr) = instructions.as_array() {
                        let combined: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                        if !combined.is_empty() {
                            let extra = combined.join("\n");
                            use meerkat_core::config::SystemPromptOverride;
                            modified_req.system_prompt = match &modified_req.system_prompt {
                                SystemPromptOverride::Set(existing) => {
                                    SystemPromptOverride::Set(format!("{existing}\n{extra}"))
                                }
                                SystemPromptOverride::Inherit => SystemPromptOverride::Set(extra),
                                // An explicit Disable suppresses every prompt
                                // source; honor it rather than resurrecting a
                                // prompt from hook-supplied instructions.
                                SystemPromptOverride::Disable => {
                                    tracing::warn!(
                                        "callback/build_agent: additional_instructions ignored \
                                         because system prompt is explicitly disabled"
                                    );
                                    SystemPromptOverride::Disable
                                }
                            };
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
                                );
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
                ensure_shell_tooling_build_substrate(&mut modified_req);
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
fn run_persistent() {
    // Meerkat 0.7's generated machine-authority apply path needs deep worker
    // stacks (mirrors meerkat-rpc's explicit 16 MiB tokio worker sizing).
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
    {
        Ok(runtime) => runtime.block_on(run_persistent_inner()),
        Err(error) => {
            eprintln!("failed to build tokio runtime: {error}");
            std::process::exit(1);
        }
    }
}

async fn run_persistent_inner() {
    use tokio::io::{AsyncBufReadExt, BufReader};

    // Initialize tracing subscriber so meerkat-mob/meerkat-runtime errors
    // are visible on stderr. Without this, all tracing events are silently
    // dropped and runtime failures (agent build, LLM calls, comms drain)
    // are invisible.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

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

    // 3. Set up stdout writer channel for multiplexed output
    let (stdout_tx, mut stdout_rx) = mpsc::channel::<String>(64);
    let (stdout_shutdown_tx, mut stdout_shutdown_rx) = oneshot::channel::<()>();
    let mut stdout_writer = tokio::spawn(async move {
        loop {
            let line = tokio::select! {
                shutdown = &mut stdout_shutdown_rx => {
                    let _ = shutdown;
                    while let Ok(line) = stdout_rx.try_recv() {
                        let mut stdout = std::io::stdout().lock();
                        let _ = writeln!(stdout, "{line}");
                        let _ = stdout.flush();
                    }
                    break;
                }
                line = stdout_rx.recv() => match line {
                    Some(line) => line,
                    None => break,
                },
            };
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{line}");
            let _ = stdout.flush();
            drop(stdout); // release lock before next await
        }
    });

    // 4. Build callback bridge and start stdin multiplexer BEFORE bootstrap.
    // This ensures callback responses (e.g. callback/build_agent during discovery
    // spawn) are routed even while UnifiedRuntime::bootstrap is running.
    let bridge = StdioCallbackBridge::new(stdout_tx.clone());
    let (rpc_tx, mut rpc_rx) = mpsc::channel::<String>(64);

    let stdin_reader = tokio::spawn({
        let bridge = bridge.clone();
        let rpc_tx = rpc_tx.clone();
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
                } else {
                    // Queue RPC request for the dispatch loop
                    let _ = rpc_tx.send(trimmed.to_string()).await;
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
    fn fail_init(request_id: &Value, code: i32, message: String) -> ! {
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
    if gateway_options.identity_bootstrap_mode.is_lazy() && !has_roster_provider {
        fail_init(
            &request_id,
            -32602,
            "runtime_options.identity_bootstrap_mode requires an identity-first roster provider"
                .to_string(),
        );
    }
    if gateway_options.agent_memory.is_some() && !has_roster_provider {
        fail_init(
            &request_id,
            -32602,
            "runtime_options.agent_memory requires an identity-first roster provider".to_string(),
        );
    }
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
            let db_path = if let Some(ref state_path) = persistent_state {
                state_path.join("continuity.db")
            } else {
                std::env::temp_dir().join(format!("mobkit-continuity-{}.db", std::process::id()))
            };
            let substrate = meerkat_mobkit::gateway_wiring::open_identity_substrate(&db_path)
                .unwrap_or_else(|e| fail_init(&request_id, -32603, e));
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
    let (
        mob_spec,
        _temp_dir,
        schedule_host_inputs,
        transcript_edit_service,
        workgraph_service,
        live_inputs,
    ) = if let Some(ref state_path) = persistent_state {
        if let Err(e) = std::fs::create_dir_all(state_path) {
            fail_init(
                &request_id,
                -32603,
                format!("failed to create persistent state directory: {e}"),
            );
        }
        let sqlite_path = state_path.join("sessions.db");
        let session_store: Arc<dyn meerkat::SessionStore> =
            if let Some(adapter) = identity_session_store_adapter.clone() {
                adapter
            } else {
                match meerkat_store::SqliteSessionStore::open(sqlite_path) {
                    Ok(s) => Arc::new(s),
                    Err(e) => fail_init(
                        &request_id,
                        -32603,
                        format!("failed to open SQLite session store: {e}"),
                    ),
                }
            };
        let mob_storage = MobStorage::in_memory();
        let binary_blob_store: Arc<dyn BinaryBlobStore> =
            match ObjectStoreBlobStore::local(state_path.join("blobs")) {
                Ok(store) => Arc::new(store),
                Err(e) => fail_init(
                    &request_id,
                    -32603,
                    format!("failed to open binary blob store: {e}"),
                ),
            };
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
        // Persistent runtime store at <state_path>/runtime.sqlite — must
        // be Some() on the session service so archive/retire can mutate
        // the authoritative session, and must be persistent so resume
        // works across gateway restart.
        let runtime_db_path = state_path.join("runtime.sqlite");
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
        let live_agent_factory = factory.clone();
        let live_machine = Arc::clone(&adapter);
        let mut inner_builder = FactoryAgentBuilder::new(factory, Config::default());
        inner_builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
            session_store.clone(),
        )));
        inner_builder.default_blob_store = Some(blob_store.clone());
        // Attach meerkat's per-session schedule tools so SDK-hosted members whose
        // profile sets tools.schedule=true get the meerkat_schedule_* surface (the
        // slot lives on the inner FactoryAgentBuilder and propagates through the
        // callback wrapper). The returned service backs the firing host spawned
        // after the runtime boots — meerkat's runtime-backed host is now generic
        // over the session builder, so scheduled sessions materialize through the
        // SDK build callback and keep their identity-scoped tools.
        let schedule_tools =
            meerkat_mobkit::schedule_wiring::attach_schedule_tools_with_identity_targets(
                &inner_builder,
                state_path,
            );
        // WorkGraph: durable store beside the schedule store (or in the
        // explicitly configured directory), realm scoped to the mob
        // definition id. Fills the member tool slot, threads to the
        // bootstrap spec (mob-executor attention overlays + child-mob
        // inheritance), the schedule host, and the RPC surface. The
        // state dir travels along for the cross-process admission
        // sidecar (a durable store is shareable across processes).
        let workgraph = match &gateway_options.workgraph {
            GatewayWorkgraphOption::Disabled => None,
            GatewayWorkgraphOption::Enabled => {
                meerkat_mobkit::workgraph_wiring::attach_workgraph_tools(
                    &inner_builder,
                    state_path,
                    &schedule_owner_id,
                )
                .map(|(service, slot)| (service, slot, state_path.clone()))
            }
            GatewayWorkgraphOption::DurableDir(dir) => {
                // An explicit directory overrides the state-dir default.
                // Open failure keeps the boot-without-workgraph posture
                // (attach_workgraph_tools warns with the path).
                let _ = std::fs::create_dir_all(dir);
                meerkat_mobkit::workgraph_wiring::attach_workgraph_tools(
                    &inner_builder,
                    dir,
                    &schedule_owner_id,
                )
                .map(|(service, slot)| (service, slot, dir.clone()))
            }
        };
        let workgraph_service = workgraph.as_ref().map(|(service, _, _)| service.clone());
        let agent_mob_tools_slot = Arc::clone(&inner_builder.default_mob_tools);
        let callback_builder = StdioCallbackAgentBuilder {
            inner: inner_builder,
            bridge: bridge.clone(),
            has_session_builder,
            session_store: Some(session_store.clone()),
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
        let schedule_host_inputs = schedule_tools.map(|tools| {
            (
                tools.service,
                tools.mob_target_registry,
                Arc::clone(&concrete_service),
                adapter.clone(),
                state_path.join(meerkat_mobkit::schedule_wiring::SCHEDULE_STORE_FILE),
            )
        });
        // §8.6 Hygienist apply seam: the CONCRETE service implements
        // meerkat's transcript-edit extension; the erased MobSessionService
        // does not carry it, so the typed handle is kept here.
        let transcript_edit_service: Option<
            Arc<dyn meerkat_mobkit::memory::hygienist::TranscriptEditSessionService>,
        > = Some(Arc::clone(&concrete_service) as _);
        // Live (realtime) transport inputs: the concrete service +
        // machine + factory, captured only on opt-in
        // (`runtime_options.live`). Ephemeral mode cannot serve live —
        // the projection sink and machine authorities are
        // persistent-service seams.
        let live_inputs = if matches!(gateway_options.live, GatewayLiveOption::Enabled { .. }) {
            Some((
                Arc::clone(&concrete_service),
                live_machine,
                live_agent_factory,
            ))
        } else {
            None
        };
        let session_service: Arc<dyn meerkat_mob::MobSessionService> = concrete_service;
        let mut spec = MobBootstrapSpec::new(definition, mob_storage, session_service)
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
        if let Some((_, admission_slot, workgraph_state_dir)) = &workgraph {
            // Durable (cross-process shareable) store: register the
            // tool-plane admission slot and the sidecar lock beside it.
            spec = spec
                .with_workgraph_admission_slot(admission_slot.clone())
                .with_workgraph_admission_sidecar(workgraph_state_dir);
        }
        spec.runtime_adapter = Some(adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        (
            spec,
            None,
            schedule_host_inputs,
            transcript_edit_service,
            workgraph_service,
            live_inputs,
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
                -32603,
                format!("failed to create scratch directory: {err}"),
            );
        }
        let binary_blob_store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(Base64BlobStoreAdapter::new(binary_blob_store.clone()));
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
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
        let mut inner_builder = FactoryAgentBuilder::new(factory, Config::default());
        inner_builder.default_blob_store = Some(blob_store.clone());
        // No-persistent_state launches default to a memory-backed
        // workgraph (tools stay profile-gated, so nothing changes for
        // members that do not opt in); an explicit directory gets the
        // durable store instead — and, being cross-process shareable, a
        // cross-process admission sidecar beside it.
        let mut workgraph_sidecar_dir: Option<PathBuf> = None;
        let workgraph_service = match &gateway_options.workgraph {
            GatewayWorkgraphOption::Disabled => None,
            GatewayWorkgraphOption::DurableDir(dir) => {
                let _ = std::fs::create_dir_all(dir);
                workgraph_sidecar_dir = Some(dir.clone());
                meerkat_mobkit::workgraph_wiring::open_workgraph_service(dir, &schedule_owner_id)
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
                Some(
                    meerkat_mobkit::workgraph_wiring::ephemeral_workgraph_service(
                        &schedule_owner_id,
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
        };
        let mut transcript_edit_service: Option<
            Arc<dyn meerkat_mobkit::memory::hygienist::TranscriptEditSessionService>,
        > = None;
        let session_service: Arc<dyn meerkat_mob::MobSessionService> =
            if let Some(session_adapter) = identity_session_store_adapter.clone() {
                let session_store: Arc<dyn meerkat::SessionStore> = session_adapter.clone();
                let mut factory = AgentFactory::new(agent_workspace)
                    .builtins(false)
                    .shell(shell)
                    .comms(true)
                    .session_store(Arc::new(meerkat::MemoryStore::new()));
                if image_generation {
                    factory = factory.with_image_generation_machine(adapter.clone());
                }
                let mut inner_builder = FactoryAgentBuilder::new(factory, Config::default());
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
                };
                let concrete = Arc::new(meerkat_session::PersistentSessionService::new(
                    callback_builder,
                    gateway_options.max_sessions,
                    session_store,
                    Arc::clone(&runtime_store),
                    blob_store.clone(),
                ));
                transcript_edit_service = Some(Arc::clone(&concrete) as _);
                concrete
            } else {
                Arc::new(EphemeralSessionService::new(
                    callback_builder,
                    gateway_options.max_sessions,
                ))
            };

        let mut spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
            .with_session_runtime_adapter(adapter.clone())
            .with_workgraph_service(workgraph_service.clone())
            .with_options(MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: default_llm_client.clone(),
            });
        for slot in workgraph_admission_slots {
            spec = spec.with_workgraph_admission_slot(slot);
        }
        if let Some(dir) = workgraph_sidecar_dir.as_deref() {
            spec = spec.with_workgraph_admission_sidecar(dir);
        }
        spec.runtime_adapter = Some(adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        // Ephemeral sessions have no persistent service; firing is persistent-only.
        (
            spec,
            temp_dir,
            None,
            transcript_edit_service,
            workgraph_service,
            None,
        )
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

    let timeout = Duration::from_secs(30);
    let persistent_metadata: Arc<dyn PersistentMetadataStore> =
        if let Some(state_path) = persistent_state.as_ref() {
            let metadata_path = state_path.join("mobkit_metadata.sqlite");
            Arc::new(
                SqliteMetadataStore::open(&metadata_path).unwrap_or_else(|e| {
                    fail_init(
                        &request_id,
                        -32603,
                        format!("failed to open mobkit_metadata.sqlite: {e}"),
                    );
                }),
            )
        } else {
            Arc::new(InMemoryMetadataStore::new())
        };
    let mut runtime = Box::pin(UnifiedRuntime::bootstrap_with_options(
        mob_spec,
        module_config,
        Vec::new(),
        timeout,
        gateway_options.runtime_options.clone(),
        persistent_metadata,
    ))
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

    if let Some(state_path) = persistent_state.as_ref() {
        let console_log_path = state_path.join("mobkit_console.sqlite");
        let console_log_store = Arc::new(
            SqliteConsoleLogStore::open(&console_log_path).unwrap_or_else(|e| {
                fail_init(
                    &request_id,
                    -32603,
                    format!(
                        "failed to open mobkit_console.sqlite at {}: {e}",
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
        let bridge_arc: Arc<dyn meerkat_mobkit::identity_first::SessionBridge> =
            if let Some(adapter) = identity_session_store_adapter.clone() {
                Arc::new(
                    meerkat_mobkit::identity_first::MobSessionBridge::with_continuity_session_store(
                        mob_handle.clone(),
                        adapter,
                        runtime.mob_runtime().session_service().cloned(),
                    ),
                )
            } else if let Some(session_service) = runtime.mob_runtime().session_service().cloned() {
                Arc::new(
                    meerkat_mobkit::identity_first::MobSessionBridge::with_session_service(
                        mob_handle.clone(),
                        session_service,
                    ),
                )
            } else {
                Arc::new(meerkat_mobkit::identity_first::MobSessionBridge::new(
                    mob_handle.clone(),
                ))
            };

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

        // §8.3 LLM Selector (P1.3): process-wide install BEFORE the memory
        // customizer/injector are constructed below, so their coordinators
        // snapshot the stage. Operator switch is
        // `agent_memory.selector = "off" | "default" | "profile:<path>"`;
        // the MOBKIT_AGENT_MEMORY_SELECTOR env var remains a fallback for
        // unmigrated deployments, with config taking precedence.
        {
            use meerkat_mobkit::memory::selector as memory_selector;
            let configured = gateway_options
                .agent_memory
                .as_ref()
                .and_then(|agent_memory| agent_memory.selector.as_ref());
            let spec = resolve_selector_spec(configured).unwrap_or_else(|e| {
                fail_init(&request_id, -32602, format!("agent memory selector: {e}"));
            });
            let profile = memory_selector::profile_for_spec(&spec).unwrap_or_else(|e| {
                fail_init(&request_id, -32602, format!("agent memory selector: {e}"));
            });
            if let Some(profile) = profile {
                let Some(agent_memory) = gateway_options.agent_memory.as_ref() else {
                    fail_init(
                        &request_id,
                        -32602,
                        "agent memory selector requires runtime_options.agent_memory".to_string(),
                    );
                };
                if agent_memory.store != GatewayAgentMemoryStoreKind::Sqlite {
                    fail_init(
                        &request_id,
                        -32602,
                        "agent memory selector requires the sqlite agent-memory store".to_string(),
                    );
                }
                // Second handle on the same per-realm database files, used
                // only for selected-body fetch; WAL keeps this safe.
                let fetch = meerkat_mobkit::SqliteAgentMemoryStore::open(&agent_memory.path)
                    .unwrap_or_else(|e| {
                        fail_init(
                            &request_id,
                            -32603,
                            format!("agent memory selector store: {e}"),
                        );
                    });
                let factory_state = persistent_state.clone().unwrap_or_else(std::env::temp_dir);
                let handle = memory_selector::FactorySelectorHandle::new(
                    factory_state,
                    meerkat::Config::default(),
                    agent_memory.config.realm.clone(),
                    &profile,
                );
                let model = profile.model.clone();
                memory_selector::install(Arc::new(memory_selector::SelectorRuntime {
                    stage: Arc::new(memory_selector::SelectorStage::new(
                        profile,
                        Arc::new(handle),
                    )),
                    fetch: Arc::new(fetch),
                }));
                tracing::info!(model = %model, "agent memory selector installed");
            }
        }

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
        let mut agent_memory_hygienist: Option<
            Arc<meerkat_mobkit::memory::hygienist::HygienistEngine>,
        > = None;
        let agent_memory_provider: Option<Arc<dyn meerkat_mobkit::AgentMemoryProvider>> =
            if let Some(agent_memory) = gateway_options.agent_memory.as_ref() {
                match agent_memory.store {
                    GatewayAgentMemoryStoreKind::Markdown => Some(Arc::new(
                        meerkat_mobkit::MarkdownAgentMemoryStore::open(&agent_memory.path)
                            .unwrap_or_else(|e| {
                                fail_init(
                                    &request_id,
                                    -32603,
                                    format!("failed to open agent memory store: {e}"),
                                );
                            }),
                    )
                        as Arc<dyn meerkat_mobkit::AgentMemoryProvider>),
                    GatewayAgentMemoryStoreKind::Sqlite => {
                        // Shared read handle on the persistent session store
                        // for the Distiller's evidence windows and the
                        // steward's gather/usage/resolvability reads.
                        let memory_transcript_store: Option<Arc<dyn meerkat::SessionStore>> =
                            if agent_memory.distiller.enabled || agent_memory.steward.enabled {
                                let Some(state) = persistent_state.clone() else {
                                    fail_init(
                                        &request_id,
                                        -32602,
                                        "agent memory distiller/steward require persistent_state"
                                            .to_string(),
                                    );
                                };
                                Some(
                                    if let Some(adapter) = identity_session_store_adapter.clone() {
                                        adapter
                                    } else {
                                        // Second handle on the same session
                                        // database the mob bridge persists to;
                                        // WAL keeps the read-side safe.
                                        match meerkat_store::SqliteSessionStore::open(
                                            state.join("sessions.db"),
                                        ) {
                                            Ok(store) => Arc::new(store),
                                            Err(e) => fail_init(
                                                &request_id,
                                                -32603,
                                                format!("agent memory session store: {e}"),
                                            ),
                                        }
                                    },
                                )
                            } else {
                                None
                            };
                        // Converged assembly (memory_wiring): store + §10.1
                        // firewall + Distiller + Steward, with the gateway's
                        // late-binding bridges passed as seams. The Hygienist
                        // and the gateway-only extras (outbound taint
                        // declarer, panel registration, compaction reset,
                        // observer spawn, dream scheduling) follow below.
                        let memory_events = runtime.memory_event_sink();
                        let engines = meerkat_mobkit::memory_wiring::MemoryEnginesConfig {
                            distiller: agent_memory.distiller.clone(),
                            steward: agent_memory.steward.clone(),
                        };
                        let stack = match meerkat_mobkit::memory_wiring::build_sqlite_memory_stack(
                            &agent_memory.path,
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
                            Err(e) => {
                                let code = if e.starts_with("failed to open") {
                                    -32603
                                } else {
                                    -32602
                                };
                                fail_init(&request_id, code, e);
                            }
                        };
                        let store = stack.store;
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
                        // this region).
                        runtime.set_memory_panel_store(store.clone());
                        // §8.6 Hygienist: audited transcript curation at
                        // compaction boundaries and on demand, applied
                        // through meerkat's typed transcript-revision
                        // extension on the concrete session service.
                        if agent_memory.hygienist.enabled {
                            use meerkat_mobkit::memory::hygienist as memory_hygienist;
                            let Some(edit_service) = transcript_edit_service.clone() else {
                                fail_init(
                                    &request_id,
                                    -32602,
                                    "agent memory hygienist requires persistent sessions \
                                     (runtime_options.persistent_state or an identity session \
                                     store): the transcript-revision seam only exists on the \
                                     persistent session service"
                                        .to_string(),
                                );
                            };
                            let mut profile =
                                memory_hygienist::HygienistProfile::embedded_default();
                            if let Some(model) = agent_memory.hygienist.model.as_deref() {
                                profile = profile.with_model_override(model).unwrap_or_else(|e| {
                                    fail_init(
                                        &request_id,
                                        -32602,
                                        format!("agent memory hygienist: {e}"),
                                    );
                                });
                            }
                            let Some(state) = persistent_state.clone() else {
                                fail_init(
                                    &request_id,
                                    -32602,
                                    "agent memory hygienist requires persistent_state".to_string(),
                                );
                            };
                            let handle = memory_hygienist::FactoryHygienistHandle::new(
                                state,
                                meerkat::Config::default(),
                                agent_memory.config.realm.clone(),
                                &profile,
                            );
                            let model = profile.model.clone();
                            let gate: Option<Arc<dyn memory_hygienist::DistillationGate>> =
                                agent_memory_distiller.clone().map(|engine| {
                                    engine as Arc<dyn memory_hygienist::DistillationGate>
                                });
                            let engine = Arc::new(memory_hygienist::HygienistEngine::new(
                                profile,
                                agent_memory.hygienist.clone(),
                                Arc::new(handle),
                                Arc::new(memory_hygienist::SessionServiceRevisionSeam::new(
                                    edit_service,
                                )),
                                Arc::new(memory_hygienist::StoreSpanReferenceSource::new(
                                    Arc::new(store.clone()),
                                    agent_memory.config.realm.clone(),
                                )),
                                gate,
                                agent_memory.config.realm.clone(),
                            ));
                            engine.set_event_sink(memory_events.clone());
                            // §8.6 trigger sequencing: behind the distiller's
                            // compaction harvest when one exists; directly off
                            // the compaction event otherwise. Never both —
                            // that would race the ordering this preserves.
                            match agent_memory_distiller.as_ref() {
                                Some(distiller) => distiller.set_compaction_follow_up(
                                    memory_hygienist::distiller_follow_up(engine.clone()),
                                ),
                                None => sinks.push(Arc::new(
                                    memory_hygienist::HygienistTriggers::new(engine.clone()),
                                )),
                            }
                            agent_memory_hygienist = Some(engine);
                            tracing::info!(
                                model = %model,
                                runs_per_day = agent_memory.hygienist.runs_per_day,
                                "agent memory hygienist installed"
                            );
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
                        // Observe-stream feed lives for the gateway process;
                        // forgetting the guard keeps the task running.
                        std::mem::forget(meerkat_mobkit::spawn_member_event_observer(
                            runtime.mob_handle(),
                            sinks,
                        ));
                        agent_memory_taint = Some(tracker);
                        Some(stack.provider)
                    }
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
            if let Some(hygienist) = agent_memory_hygienist.clone() {
                injector = injector.with_hygienist(hygienist);
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
            gateway_options.identity_bootstrap_mode.clone(),
        ));
        if let Err(e) = identity_context.bootstrap_roster(&roster_specs).await {
            fail_init(
                &request_id,
                -32603,
                format!("identity-first bootstrap failed: {e}"),
            );
        }

        runtime.attach_identity_first_context(identity_context);

        Some(meerkat_mobkit::rpc::IdentityFirstContext {
            runtime: irt,
            roster_provider: roster,
            topology_provider: topology,
            customizer,
            agent_memory_provider,
            mob_definition: Some(mob_definition),
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
        // The firing driver discards its own tick errors upstream; the
        // watchdog is what turns "everything stays pending forever" into a
        // loud, row-level diagnosis in the gateway log.
        let watchdog = meerkat_mobkit::schedule_wiring::spawn_schedule_claim_watchdog(
            schedule_service.clone(),
            schedule_store_path,
            Default::default(),
        );
        (
            meerkat_mobkit::schedule_wiring::spawn_schedule_host_with_identity_runtime(
                service,
                adapter,
                schedule_service,
                mob_state,
                runtime.mob_handle(),
                runtime.identity_runtime().cloned(),
                runnable_host,
                workgraph_service.clone(),
                schedule_owner_id.clone(),
            ),
            Some(watchdog),
        )
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

    let runtime = Arc::new(runtime);
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    let http_base_url = format!("http://127.0.0.1:{port}");

    // 7. Start HTTP with graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
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
    let (app, live_rpc) =
        if let Some((live_service, live_machine, live_agent_factory)) = live_inputs {
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
            let app = app.merge(meerkat_live::live_ws_router(Arc::clone(&live_ctx.ws_state)));
            let handler =
                meerkat_mobkit::live_wiring::live_rpc_handler(live_ctx, live_service, live_machine);
            (app, Some(handler))
        } else {
            (app, None)
        };
    let mut serve_task = tokio::spawn({
        let mut shutdown_rx = shutdown_rx.clone();
        async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_rx.changed().await.ok();
                })
                .await
        }
    });

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
        }
    });
    let _ = stdout_tx
        .send(
            serde_json::to_string(&init_response)
                .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string()),
        )
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
    {
        let mut inflight = tokio::task::JoinSet::new();
        loop {
            let request_line = tokio::select! {
                line = rpc_rx.recv() => line,
                _ = tokio::signal::ctrl_c() => {
                    interrupted_with_open_stdin = true;
                    None
                },
            };
            let Some(request_line) = request_line else {
                break; // stdin reader closed (EOF/error), or Ctrl-C won
            };
            let request_line = apply_gateway_runtime_config_to_request(
                &request_line,
                &gateway_options.schedules,
                &gateway_options.gating,
            );
            let runtime = runtime.clone();
            let stdout_tx = stdout_tx.clone();
            let identity_ctx = identity_ctx.clone();
            let http_base_url = http_base_url_shared.clone();
            let live_rpc = live_rpc.clone();
            inflight.spawn(async move {
                let response = meerkat_mobkit::rpc::handle_unified_rpc_json_with_live_arc(
                    &runtime,
                    &request_line,
                    timeout,
                    Some(http_base_url.as_ref()),
                    identity_ctx.as_deref(),
                    live_rpc.as_ref(),
                )
                .await;
                if !response.is_empty() {
                    let _ = stdout_tx.send(response).await;
                }
            });
            // Reap completed handlers so the set does not grow unbounded.
            while inflight.try_join_next().is_some() {}
        }
        // Ctrl-C can stop dispatch while stdin remains open. Close callback
        // admission here too so handler/customizer waits wake immediately.
        bridge.close().await;
        // EOF closes request admission. Give ordinary handlers a bounded
        // response grace, then abort their outer waiters so runtime shutdown
        // can begin promptly. Identity materialization/delivery transactions
        // are independently owned by the runtime foreground supervisor and
        // are cancelled or joined below at their explicit cleanup boundary.
        let drain = async { while inflight.join_next().await.is_some() {} };
        if tokio::time::timeout(Duration::from_secs(5), drain)
            .await
            .is_err()
        {
            inflight.shutdown().await;
        }
    }

    // 10. Graceful shutdown: stop HTTP admission, bound the outer-handler
    // drain, then let the runtime cancel/join identity-owned transactions.
    // Dispatch can also end on Ctrl-C while stdin is still open. Stop the
    // reader now so its blocking read cannot outlive the runtime shutdown.
    // Aborting an already-completed EOF reader is harmless.
    stdin_reader.abort();
    let _ = shutdown_tx.send(true);
    if tokio::time::timeout(Duration::from_secs(5), &mut serve_task)
        .await
        .is_err()
    {
        serve_task.abort();
        let _ = serve_task.await;
    }
    event_drain_task.abort();
    runtime.shutdown().await;
    drop(stdout_tx);
    // Runtime and callback objects intentionally retain sender clones until
    // function exit. Signal the writer explicitly after all producers have
    // quiesced so it drains queued responses without waiting for channel
    // ownership to disappear.
    let _ = stdout_shutdown_tx.send(());
    let _ = stdin_reader.await;
    if tokio::time::timeout(Duration::from_secs(5), &mut stdout_writer)
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
    if args.iter().any(|a| a == "--persistent") {
        run_persistent();
    } else {
        run_single_shot();
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
