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
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use meerkat_mobkit::unified_runtime::EventLogError;
use meerkat_mobkit::{
    AuthPolicy, AuthProvider, Base64BlobStoreAdapter, BigQueryNaming, BinaryBlobStore,
    ConsolePolicy, ConsoleUiConfig, DiscoverySpec, ElephantMemoryBackendConfig, EventLogConfig,
    EventLogStore, EventQuery, InMemoryMetadataStore, MOBKIT_CONTRACT_VERSION, MemoryBackendConfig,
    MobBootstrapOptions, MobBootstrapSpec, MobKitConfig, ModuleConfig, ObjectStoreBlobStore,
    PersistedEvent, PersistentMetadataStore, PreSpawnData, ReleaseMetadata, RestartPolicy,
    RuntimeDecisionState, RuntimeOpsPolicy, RuntimeOptions, RuntimeRoute, ScheduleDefinition,
    SqliteConsoleLogStore, SqliteMetadataStore, TrustedOidcRuntimeConfig, UnifiedRuntime,
    handle_mobkit_rpc_json, handle_unified_rpc_json, load_console_ui_config_from_path_for_realm,
    mob_handle_runtime::mob_definition_may_use_image_generation, start_mobkit_runtime,
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
}

impl Default for GatewayRuntimeOptions {
    fn default() -> Self {
        Self {
            runtime_options: RuntimeOptions::default(),
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
        "implicit_delegate_idle_retire_secs",
        "implicit_delegate_idle_sweep_interval_ms",
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
    if let Some(memory_config) = runtime_options.get("memory_config") {
        parsed.runtime_options.memory_backend = Some(parse_gateway_memory_config(
            memory_config,
            persistent_state,
        )?);
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

fn parse_gateway_memory_config(
    memory_config: &Value,
    persistent_state: Option<&std::path::Path>,
) -> Result<MemoryBackendConfig, String> {
    let object = memory_config
        .as_object()
        .ok_or_else(|| "runtime_options.memory_config must be a JSON object".to_string())?;
    let backend = object
        .get("backend")
        .and_then(Value::as_str)
        .ok_or_else(|| "runtime_options.memory_config.backend must be 'elephant'".to_string())?;
    if backend != "elephant" {
        return Err(format!(
            "unsupported runtime_options.memory_config.backend '{backend}'"
        ));
    }
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
    let state_path = persistent_state
        .ok_or_else(|| {
            "runtime_options.memory_config requires persistent_state so Elephant memory state has a stable path"
                .to_string()
        })?
        .join("elephant-memory-state.json");
    Ok(MemoryBackendConfig::Elephant(ElephantMemoryBackendConfig {
        endpoint: endpoint.to_string(),
        state_path: state_path.to_string_lossy().to_string(),
    }))
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
    /// Pending callback responses keyed by callback ID.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    /// Counter for generating unique callback IDs.
    counter: Arc<std::sync::atomic::AtomicU64>,
}

impl StdioCallbackBridge {
    fn new(stdout_tx: mpsc::Sender<String>) -> Self {
        Self {
            stdout_tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
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
        self.pending.lock().await.insert(id_str.clone(), tx);

        let request = json!({
            "jsonrpc": "2.0",
            "id": id_str,
            "method": method,
            "params": params,
        });
        let line = match serde_json::to_string(&request) {
            Ok(l) => l,
            Err(e) => {
                self.pending.lock().await.remove(&id_str);
                return Err(e.to_string());
            }
        };
        if let Err(_) = self.stdout_tx.send(line).await {
            self.pending.lock().await.remove(&id_str);
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
                self.pending.lock().await.remove(&id_str);
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
        if let Some(tx) = self.pending.lock().await.remove(&id) {
            let _ = tx.send(msg);
        }
    }
}

#[async_trait]
impl meerkat_mobkit::identity_first::gateway_bridges::CallbackBridge for StdioCallbackBridge {
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.call(method, params).await
    }
}

/// Tool dispatcher that routes tool calls to Python via the callback bridge.
///
/// Created from tool name strings provided by `SessionBuildOptions.add_tools()`.
/// When the agent calls a tool, `dispatch()` sends `callback/call_tool` to Python
/// and returns the result.
struct CallbackToolDispatcher {
    bridge: StdioCallbackBridge,
    scope_id: String,
    tool_defs: Arc<[Arc<ToolDef>]>,
}

impl CallbackToolDispatcher {
    fn new(bridge: StdioCallbackBridge, scope_id: String, tool_names: Vec<String>) -> Self {
        let tool_defs: Vec<Arc<ToolDef>> = tool_names
            .into_iter()
            .map(|name| {
                Arc::new(ToolDef {
                    name: name.into(),
                    description: "Python callback tool".to_string(),
                    input_schema: json!({"type": "object"}),
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
            Ok(result) => {
                let text = result
                    .get("content")
                    .map(|v| {
                        if let Some(s) = v.as_str() {
                            s.to_string()
                        } else {
                            serde_json::to_string(v).unwrap_or_default()
                        }
                    })
                    .unwrap_or_else(|| serde_json::to_string(&result).unwrap_or_default());
                Ok(ToolResult {
                    tool_use_id: call.id.to_string(),
                    content: vec![ContentBlock::Text { text }],
                    is_error: false,
                }
                .into())
            }
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

#[async_trait]
impl SessionAgentBuilder for StdioCallbackAgentBuilder {
    type Agent = FactoryAgent;

    async fn build_agent(
        &self,
        req: &CreateSessionRequest,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Self::Agent, SessionError> {
        if !self.has_session_builder {
            return self.inner.build_agent(req, event_tx).await;
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
        let options = json!({
            "scope_id": scope_id,
            "session_id": req.labels.as_ref().and_then(|l| l.get("session_id")),
            "model": &req.model,
            "prompt": &req.prompt,
            "labels": &req.labels,
            "app_context": req.build.as_ref()
                .and_then(|b| b.app_context.as_ref()),
        });
        let params = json!({ "options": options });
        let callback_result = self.bridge.call("callback/build_agent", params).await;

        match callback_result {
            Ok(result) => {
                // Apply Python-returned options to a cloned request
                let mut modified_req = CreateSessionRequest {
                    model: req.model.clone(),
                    prompt: req.prompt.clone(),
                    render_metadata: req.render_metadata.clone(),
                    system_prompt: req.system_prompt.clone(),
                    max_tokens: req.max_tokens,
                    event_tx: req.event_tx.clone(),
                    skill_references: req.skill_references.clone(),
                    initial_turn: req.initial_turn.clone(),
                    build: req.build.clone(),
                    labels: req.labels.clone(),
                    deferred_prompt_policy: req.deferred_prompt_policy,
                };
                // Apply additional_instructions as system prompt extension
                if let Some(instructions) = result.get("additional_instructions") {
                    if let Some(arr) = instructions.as_array() {
                        let combined: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                        if !combined.is_empty() {
                            let extra = combined.join("\n");
                            modified_req.system_prompt = Some(match &modified_req.system_prompt {
                                Some(existing) => format!("{existing}\n{extra}"),
                                None => extra,
                            });
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
                                SessionError::Agent(AgentError::ToolError(format!(
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
                                return Err(SessionError::Agent(AgentError::ToolError(format!(
                                    "callback/build_agent: resume_session_id conflict: \
                                     spawn set {} but hook set {resume_id}",
                                    existing.id()
                                ))));
                            }
                            // Same ID — already loaded, skip.
                        } else {
                            let session = store.load(&sid).await.map_err(|e| {
                                SessionError::Agent(AgentError::ToolError(format!(
                                    "callback/build_agent: failed to load resume session {resume_id}: {e}"
                                )))
                            })?;
                            let session = session.ok_or_else(|| {
                                SessionError::Agent(AgentError::ToolError(format!(
                                    "callback/build_agent: resume session not found: {resume_id}"
                                )))
                            })?;
                            let build = modified_req.build.get_or_insert_with(|| {
                                meerkat_core::service::SessionBuildOptions::default()
                            });
                            build.resume_session = Some(session);
                        }
                    } else {
                        return Err(SessionError::Agent(AgentError::ToolError(
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
                            let mut tool_names = Vec::with_capacity(arr.len());
                            for v in arr {
                                if let Some(name) = v.as_str() {
                                    tool_names.push(name.to_string());
                                } else {
                                    return Err(SessionError::Agent(AgentError::ToolError(
                                        format!(
                                            "callback/build_agent: tools must be strings, got: {v}"
                                        ),
                                    )));
                                }
                            }
                            if !tool_names.is_empty() {
                                let dispatcher = CallbackToolDispatcher::new(
                                    self.bridge.clone(),
                                    scope_id.clone(),
                                    tool_names,
                                );
                                let build = modified_req.build.get_or_insert_with(|| {
                                    meerkat_core::service::SessionBuildOptions::default()
                                });
                                build.external_tools = Some(Arc::new(dispatcher));
                            }
                        }
                        None => {
                            return Err(SessionError::Agent(AgentError::ToolError(format!(
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
                Err(SessionError::Agent(AgentError::ToolError(format!(
                    "callback/build_agent failed: {err}"
                ))))
            }
        }
    }
}

/// Persistent mode: reads JSON-RPC over stdin, bootstraps unified runtime, serves HTTP.
#[tokio::main]
async fn run_persistent() {
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
model = "gpt-5.2"
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
    let stdout_writer = tokio::spawn(async move {
        while let Some(line) = stdout_rx.recv().await {
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
        }
    });

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
    let default_llm_client: Option<Arc<dyn meerkat_client::LlmClient>> =
        match gateway_options.demo_llm {
            true => {
                let client: Arc<dyn meerkat_client::LlmClient> =
                    Arc::new(meerkat_client::TestClient::default());
                Some(client)
            }
            false => None,
        };
    let identity_continuity_store: Option<
        Arc<dyn meerkat_mobkit::identity_first::ContinuityStore>,
    > = if has_roster_provider {
        Some(if has_continuity_store {
            Arc::new(meerkat_mobkit::identity_first::GatewayContinuityStore::new(
                bridge.clone(),
            ))
        } else if let Some(ref state_path) = persistent_state {
            Arc::new(
                meerkat_mobkit::identity_first::LocalContinuityStore::open(
                    state_path.join("continuity.db"),
                )
                .unwrap_or_else(|e| {
                    fail_init(
                        &request_id,
                        -32603,
                        format!("failed to open continuity store: {e}"),
                    );
                }),
            )
        } else {
            Arc::new(
                meerkat_mobkit::identity_first::LocalContinuityStore::open(
                    std::env::temp_dir()
                        .join(format!("mobkit-continuity-{}.db", std::process::id())),
                )
                .unwrap_or_else(|e| {
                    fail_init(
                        &request_id,
                        -32603,
                        format!("failed to open continuity store: {e}"),
                    );
                }),
            )
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
            ))
        } else {
            Arc::new(meerkat_mobkit::identity_first::LocalLeaseProvider::new())
        })
    } else {
        None
    };
    let identity_session_store_adapter = identity_continuity_store.as_ref().map(|store| {
        Arc::new(meerkat_mobkit::identity_first::ContinuitySessionStoreAdapter::new(store.clone()))
    });

    // 5. Build session service with callback bridge.
    let (mob_spec, _temp_dir) = if let Some(ref state_path) = persistent_state {
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
        let mut factory = AgentFactory::new(state_path).builtins(false).comms(true);
        if image_generation {
            factory = factory.with_image_generation_machine(adapter.clone());
        }
        let mut inner_builder = FactoryAgentBuilder::new(factory, Config::default());
        inner_builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
            session_store.clone(),
        )));
        inner_builder.default_blob_store = Some(blob_store.clone());
        let callback_builder = StdioCallbackAgentBuilder {
            inner: inner_builder,
            bridge: bridge.clone(),
            has_session_builder,
            session_store: Some(session_store.clone()),
        };
        let session_service: Arc<dyn meerkat_mob::MobSessionService> =
            Arc::new(meerkat_session::PersistentSessionService::new(
                callback_builder,
                gateway_options.max_sessions,
                session_store,
                Some(Arc::clone(&runtime_store)),
                blob_store,
            ));
        let mut spec = MobBootstrapSpec::new(definition, mob_storage, session_service)
            .with_session_runtime_adapter(adapter.clone())
            .with_options(MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: default_llm_client.clone(),
            });
        spec.runtime_adapter = Some(adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        (spec, None)
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
            .comms(true)
            .session_store(Arc::new(meerkat::MemoryStore::new()));
        if image_generation {
            factory = factory.with_image_generation_machine(adapter.clone());
        }
        let mut inner_builder = FactoryAgentBuilder::new(factory, Config::default());
        inner_builder.default_blob_store = Some(blob_store.clone());
        let callback_builder = StdioCallbackAgentBuilder {
            inner: inner_builder,
            bridge: bridge.clone(),
            has_session_builder,
            session_store: None,
        };
        let session_service: Arc<dyn meerkat_mob::MobSessionService> =
            if let Some(session_adapter) = identity_session_store_adapter.clone() {
                let session_store: Arc<dyn meerkat::SessionStore> = session_adapter.clone();
                let mut factory = AgentFactory::new(agent_workspace)
                    .builtins(false)
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
                let callback_builder = StdioCallbackAgentBuilder {
                    inner: inner_builder,
                    bridge: bridge.clone(),
                    has_session_builder,
                    session_store: Some(session_store.clone()),
                };
                Arc::new(meerkat_session::PersistentSessionService::new(
                    callback_builder,
                    gateway_options.max_sessions,
                    session_store,
                    Some(Arc::clone(&runtime_store)),
                    blob_store.clone(),
                ))
            } else {
                Arc::new(EphemeralSessionService::new(
                    callback_builder,
                    gateway_options.max_sessions,
                ))
            };

        let mut spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
            .with_session_runtime_adapter(adapter.clone())
            .with_options(MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: default_llm_client.clone(),
            });
        spec.runtime_adapter = Some(adapter);
        spec.binary_blob_store = Some(binary_blob_store);
        (spec, temp_dir)
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
    let mut runtime = UnifiedRuntime::bootstrap_with_options(
        mob_spec,
        module_config,
        Vec::new(),
        timeout,
        gateway_options.runtime_options.clone(),
        persistent_metadata,
    )
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

        let irt = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store,
            lease_provider,
            runtime_instance_id: format!("gateway-{}", std::process::id()),
            has_runtime_store: identity_session_store_adapter.is_some()
                || persistent_state.is_some(),
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge_arc),
            default_timeout: None,
        })
        .with_runtime_services(AgentRuntimeServices::new(mob_handle));
        irt.set_error_hook(Some(gateway_error_hook.clone()));

        // Build provider bridges for callbacks to Python
        let roster: Arc<dyn meerkat_mobkit::identity_first::contracts::RosterProvider> =
            Arc::new(GatewayRosterProvider::new(bridge.clone()));
        let topology: Option<Arc<dyn meerkat_mobkit::identity_first::contracts::TopologyProvider>> =
            if has_topology_provider {
                Some(Arc::new(GatewayTopologyProvider::new(bridge.clone())))
            } else {
                None
            };
        let customizer: Option<
            Arc<dyn meerkat_mobkit::identity_first::contracts::AgentCustomizer>,
        > = if has_agent_customizer {
            Some(Arc::new(GatewayAgentCustomizer::new(bridge.clone())))
        } else {
            None
        };

        // Call restore_flow to bootstrap identities from the roster provider
        let roster_specs = roster
            .roster(&RosterContext {
                mob_definition: None,
                previous_identities: Vec::new(),
            })
            .await
            .unwrap_or_else(|e| {
                fail_init(&request_id, -32603, format!("roster provider failed: {e}"));
            });

        if let Err(e) = meerkat_mobkit::identity_first::restore_flow(
            &irt,
            &roster_specs,
            topology.as_deref(),
            customizer.as_deref(),
        )
        .await
        {
            fail_init(
                &request_id,
                -32603,
                format!("identity-first restore_flow failed: {e}"),
            );
        }

        let identity_runtime = Arc::new(irt);
        runtime.attach_identity_first_context(Arc::new(IdentityFirstRuntimeContext::new(
            identity_runtime.clone(),
            roster.clone(),
            topology.clone(),
            customizer.clone(),
            Some(runtime.mob_handle().definition().clone()),
        )));

        Some(meerkat_mobkit::rpc::IdentityFirstContext {
            runtime: identity_runtime,
            roster_provider: roster,
            topology_provider: topology,
            customizer,
        })
    } else {
        None
    };

    let runtime = Arc::new(runtime);
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
    let serve_task = tokio::spawn({
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
    let init_response = json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "http_base_url": http_base_url,
            "loaded_modules": loaded_modules,
            "contract_version": MOBKIT_CONTRACT_VERSION,
            "runtime_origin": runtime_origin,
            "runtime_fingerprint": runtime_fingerprint,
        }
    });
    let _ = stdout_tx
        .send(
            serde_json::to_string(&init_response)
                .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string()),
        )
        .await;

    // 9. RPC dispatch loop: process queued requests from the stdin reader task
    {
        loop {
            let request_line = tokio::select! {
                line = rpc_rx.recv() => match line {
                    Some(l) => l,
                    None => break, // stdin reader closed (EOF or error)
                },
                _ = tokio::signal::ctrl_c() => break,
            };
            let request_line = apply_gateway_runtime_config_to_request(
                &request_line,
                &gateway_options.schedules,
                &gateway_options.gating,
            );
            let response = handle_unified_rpc_json(
                &runtime,
                &request_line,
                timeout,
                Some(&http_base_url),
                identity_ctx.as_ref(),
            )
            .await;
            if !response.is_empty() {
                let _ = stdout_tx.send(response).await;
            }
        }
    }

    // 10. Graceful shutdown: stop HTTP server, then runtime
    let _ = shutdown_tx.send(true);
    let _ = serve_task.await;
    event_drain_task.abort();
    runtime.shutdown().await;
    drop(rpc_tx);
    drop(stdout_tx);
    let _ = stdin_reader.await;
    let _ = stdout_writer.await;
    drop(_temp_dir);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--persistent") {
        run_persistent();
    } else {
        run_single_shot();
    }
}
