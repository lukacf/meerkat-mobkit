//! Mobpack authoring contract used by the bundled Flow Editor.
//!
//! Mobpacks are authoring artifacts. Importing, validating, or exporting one
//! does not mutate any running mob runtime; callers launch the resulting
//! `mob.toml`/mobpack through a separate deploy path.

use base64::Engine;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use meerkat_mob::definition::{
    CollectionPolicy, ConditionExpr, DependencyMode, DispatchMode, FlowNodeSpec, FlowSpec,
    FlowStepSpec, FrameSpec, SkillSource, StepOutputFormat,
};
use meerkat_mob::{
    BudgetSplitPolicy, ForkContext, MemberLaunchMode, MobBackendKind, MobDefinition,
    MobRuntimeMode, ProfileBinding, ToolConfig,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use tar::{Builder, Header};

pub const MOBPACK_SCHEMA_VERSION: &str = "0.1.0";
pub const MOBPACK_MEDIA_TYPE: &str = "application/vnd.meerkat.mobpack";
pub const MOBPACK_VALIDATION_SOURCE: &str =
    "meerkat_mob::validate_definition + meerkat_mob::SpecValidator";
const GRAPH_GATE_KINDS: &[&str] = &["branch", "fork", "join"];
const GRAPH_PALETTE_GATE_KINDS: &[&str] = &["branch", "fork"];
const GRAPH_TERMINAL_KINDS: &[&str] = &["success", "failed", "human"];
const GRAPH_FRAME_KINDS: &[&str] = &["Branch", "Parallel", "RepeatUntil"];
const GRAPH_EDGE_KINDS: &[&str] = &["next", "cond", "fanout"];
const REPEAT_ITERATION_INPUTS: &[&str] = &["carry"];
const EDITOR_FLOW_STEP_TYPES: &[&str] = &["input", "member", "repeat", "branch", "parallel"];
const EDITOR_INPUT_STEP_ID_PREFIX: &str = "input";
const EDITOR_INPUT_STEP_DEFAULT_TASK: &str = "Run the mobpack flow.";
const DEFAULT_DEPLOY_EXEC_TIMEOUT_MS: u64 = 120_000;
const DEPLOY_EXEC_TIMEOUT_GRACE_MS: u64 = 250;
const EDITOR_SCHEMA_FIELD_TYPES: &[&str] = &[
    "string", "string[]", "number", "float", "int", "integer", "boolean", "bool", "enum", "bytes",
    "object",
];

fn editor_input_step_default_task() -> &'static str {
    EDITOR_INPUT_STEP_DEFAULT_TASK
}

fn editor_input_step_value(
    id: &str,
    task: String,
    fields: String,
    input_params: Vec<Value>,
) -> Value {
    json!({
        "id": id,
        "type": "input",
        "task": task,
        "fields": fields,
        "inputParams": input_params
    })
}

fn editor_input_step_draft_contract() -> Value {
    json!({
        "document_path": "document.flow.steps[type=input]",
        "default_step": {
            "id": EDITOR_INPUT_STEP_ID_PREFIX,
            "type": "input",
            "task": editor_input_step_default_task(),
            "fields": "",
            "inputParams": []
        }
    })
}

fn serialized_string_values<T: Serialize>(values: Vec<T>) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| {
            serde_json::to_value(value)
                .ok()
                .and_then(|value| value.as_str().map(ToString::to_string))
        })
        .collect()
}

fn editor_schema_field_type_values() -> Vec<String> {
    EDITOR_SCHEMA_FIELD_TYPES
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

fn is_editor_schema_field_type(value: &str) -> bool {
    EDITOR_SCHEMA_FIELD_TYPES.contains(&value)
}

fn serialized_tag_values<T: DeserializeOwned>(tag: &str, values: Vec<Value>) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| {
            serde_json::from_value::<T>(value.clone()).ok()?;
            value
                .get(tag)
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect()
}

fn runtime_mode_values() -> Vec<String> {
    serialized_string_values(vec![
        MobRuntimeMode::AutonomousHost,
        MobRuntimeMode::TurnDriven,
    ])
}

fn member_launch_mode_values() -> Vec<String> {
    serialized_tag_values::<MemberLaunchMode>(
        "mode",
        vec![
            json!({ "mode": "fresh" }),
            json!({ "mode": "resume", "bridge_session_id": "00000000-0000-0000-0000-000000000000" }),
            json!({
                "mode": "fork",
                "source_member_id": "lead",
                "fork_context": { "type": "full_history" }
            }),
        ],
    )
}

fn fork_context_values() -> Vec<String> {
    serialized_tag_values::<ForkContext>(
        "type",
        vec![
            json!({ "type": "full_history" }),
            json!({ "type": "last_messages", "count": 5 }),
        ],
    )
}

fn budget_split_policy_values() -> Vec<String> {
    serialized_tag_values::<BudgetSplitPolicy>(
        "type",
        vec![
            json!({ "type": "equal" }),
            json!({ "type": "proportional" }),
            json!({ "type": "remaining" }),
            json!({ "type": "fixed", "value": 4096 }),
        ],
    )
}

fn canonical_fork_context_value(value: &str) -> String {
    match value.trim() {
        "" | "FullHistory" => "full_history".to_string(),
        "LastMessages" => "last_messages".to_string(),
        other => other.to_string(),
    }
}

fn dispatch_mode_values() -> Vec<String> {
    serialized_string_values(vec![
        DispatchMode::FanOut,
        DispatchMode::OneToOne,
        DispatchMode::FanIn,
    ])
}

fn dispatch_mode_labels() -> Value {
    json!({
        "fan_out": "fan_out — broadcast to every lane",
        "one_to_one": "one_to_one — pair inputs with lanes",
        "fan_in": "fan_in — gather upstream outputs"
    })
}

fn collection_policy_values() -> Vec<String> {
    serialized_tag_values::<CollectionPolicy>(
        "type",
        vec![
            json!({ "type": "all" }),
            json!({ "type": "any" }),
            json!({ "type": "quorum", "n": 1 }),
        ],
    )
}

fn collection_policy_labels() -> Value {
    json!({
        "all": "all — wait for every branch",
        "any": "any — accept the first completed branch",
        "quorum": "quorum — require N branches"
    })
}

fn dependency_mode_values() -> Vec<String> {
    serialized_string_values(vec![DependencyMode::All, DependencyMode::Any])
}

fn dependency_mode_labels() -> Value {
    json!({
        "all": "all — every upstream node",
        "any": "any — any upstream node"
    })
}

fn step_output_format_values() -> Vec<String> {
    serialized_string_values(vec![StepOutputFormat::Json, StepOutputFormat::Text])
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MobpackDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    #[serde(default)]
    pub mob_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mob_settings: Value,
    #[serde(default)]
    pub members: Value,
    #[serde(default)]
    pub instances: Value,
    #[serde(default)]
    pub edges: Value,
    #[serde(default)]
    pub frames: Value,
    #[serde(default)]
    pub schemas: Value,
    #[serde(default)]
    pub skill_realms: Value,
    #[serde(default)]
    pub flow: Value,
    #[serde(default)]
    pub launch_modes: Value,
    #[serde(default)]
    pub deploy: Value,
    #[serde(default)]
    pub mob_toml: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobpackDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobpackDisplayRow {
    pub kind: String,
    pub glyph: String,
    pub head: String,
    pub sub: String,
    pub meta: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MobpackValidationResult {
    pub ok: bool,
    pub diagnostics: Vec<MobpackDiagnostic>,
    pub display_rows: Vec<MobpackDisplayRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mob_id: Option<String>,
    pub flow_ids: Vec<String>,
    pub validation_source: String,
    pub deploy_command: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MobpackExportResult {
    pub filename: String,
    pub media_type: String,
    pub content_base64: String,
    pub mob_toml: String,
    pub source_files: Vec<MobpackSourceFile>,
    pub validation: MobpackValidationResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobpackSourceFile {
    pub path: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub content_base64: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MobpackDeployResult {
    pub filename: String,
    pub pack_path: String,
    pub pack_sha256: String,
    pub command: String,
    pub argv: Vec<String>,
    pub plan_trace: Vec<Value>,
    pub executed: bool,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    pub validation: MobpackValidationResult,
    pub display_rows: Vec<MobpackDisplayRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MobpackDeployCommandResult {
    pub command: String,
    pub argv: Vec<String>,
    pub deploy_command: String,
    pub source: String,
}

fn default_schema_version() -> String {
    MOBPACK_SCHEMA_VERSION.to_string()
}

fn discover_skill_realms(sample_mobpacks: &Value) -> Value {
    let mut realms = Vec::new();
    for (realm_id, label, dir) in skill_catalog_dirs() {
        let skills = discover_skills_in_dir(&dir);
        if !skills.is_empty() {
            realms.push(json!({
                "id": realm_id,
                "label": label,
                "default": realms.is_empty(),
                "source": "filesystem",
                "path": dir.to_string_lossy(),
                "skills": skills,
            }));
        }
    }
    if let Some(sample_realm) = sample_skill_realm(sample_mobpacks, realms.is_empty()) {
        realms.push(sample_realm);
    }
    Value::Array(realms)
}

fn sample_skill_realm(sample_mobpacks: &Value, default: bool) -> Option<Value> {
    let mut skills = BTreeMap::<String, Value>::new();
    for sample in sample_mobpacks.as_array()? {
        let Some(source_mobpack) = sample
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(sample_source) = sample
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let source_name = sample
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(source_mobpack);
        let Some(realms) = sample
            .get("document")
            .and_then(|document| document.get("skill_realms"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for realm in realms {
            let Some(realm_id) = realm
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let Some(realm_skills) = realm.get("skills").and_then(Value::as_array) else {
                continue;
            };
            for skill in realm_skills {
                let Some(skill_id) = skill
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                skills.entry(skill_id.to_string()).or_insert_with(|| {
                    let mut projected = skill.as_object().cloned().unwrap_or_default();
                    projected
                        .entry("label".to_string())
                        .or_insert_with(|| Value::String(skill_id.to_string()));
                    projected.insert(
                        "origin".to_string(),
                        Value::String(sample_source.to_string()),
                    );
                    projected.insert(
                        "sourceMobpack".to_string(),
                        Value::String(source_mobpack.to_string()),
                    );
                    projected.insert(
                        "sourceMobpackName".to_string(),
                        Value::String(source_name.to_string()),
                    );
                    projected.insert(
                        "sourceRealm".to_string(),
                        Value::String(realm_id.to_string()),
                    );
                    projected.insert(
                        "sourceDocumentPath".to_string(),
                        Value::String(
                            "sample_mobpacks[].document.skill_realms[].skills[]".to_string(),
                        ),
                    );
                    Value::Object(projected)
                });
            }
        }
    }
    if skills.is_empty() {
        return None;
    }
    Some(json!({
        "id": "mobkit/sample-mobpacks",
        "label": "mobkit/sample-mobpacks",
        "default": default,
        "source": "mobkit/sample-mobpack",
        "sourceDocumentPath": "sample_mobpacks[].document.skill_realms[]",
        "skills": skills.into_values().collect::<Vec<_>>(),
    }))
}

fn skill_catalog_dirs() -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    if let Ok(raw) = std::env::var("MOBKIT_SKILL_DIRS") {
        for (index, path) in std::env::split_paths(&raw).enumerate() {
            out.push((
                format!("env/skills-{index}"),
                format!("env/skills-{index}"),
                path,
            ));
        }
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    out.push((
        "mobkit/repo".to_string(),
        "mobkit/repo".to_string(),
        manifest_dir.join("../.claude/skills"),
    ));
    if let Ok(cwd) = std::env::current_dir() {
        out.push((
            "workspace/skills".to_string(),
            "workspace/skills".to_string(),
            cwd.join(".rkat/skills"),
        ));
        out.push((
            "workspace/claude".to_string(),
            "workspace/claude".to_string(),
            cwd.join(".claude/skills"),
        ));
    }
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        out.push((
            "meerkat-mob/source".to_string(),
            "meerkat-mob/source".to_string(),
            home.join("src/meerkat/meerkat-mob/skills"),
        ));
        out.extend(meerkat_mob_registry_skill_dirs(
            &home.join(".cargo/registry/src"),
        ));
    }
    let mut seen = BTreeSet::new();
    out.into_iter()
        .filter(|(_, _, path)| seen.insert(path.clone()))
        .collect()
}

fn meerkat_mob_registry_skill_dirs(registry_root: &Path) -> Vec<(String, String, PathBuf)> {
    let Ok(registries) = std::fs::read_dir(registry_root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for registry in registries.flatten() {
        let Ok(crates) = std::fs::read_dir(registry.path()) else {
            continue;
        };
        for crate_entry in crates.flatten() {
            let path = crate_entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(version) = name.strip_prefix("meerkat-mob-") else {
                continue;
            };
            if !version.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
                continue;
            }
            let skills_dir = path.join("skills");
            if !skills_dir.is_dir() {
                continue;
            }
            out.push((
                format!("meerkat-mob/crate/{version}"),
                format!("meerkat-mob/crate/{version}"),
                skills_dir,
            ));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
    out
}

fn tool_catalog_response() -> Vec<Value> {
    let mut tools = tool_config_bool_fields()
        .into_iter()
        .map(|field| {
            json!({
                "id": field,
                "label": field,
                "kind": "runtime",
                "field": field,
                "source": "meerkat_mob::ToolConfig",
                "desc": tool_config_field_desc(&field),
            })
        })
        .collect::<Vec<_>>();
    for source in discover_mcp_sources() {
        tools.push(json!({
            "id": format!("mcp:{source}"),
            "label": format!("mcp:{source}"),
            "kind": "mcp",
            "source": source,
            "desc": "Configured host MCP source allowlist entry",
        }));
    }
    for bundle in discover_rust_tool_bundles() {
        tools.push(json!({
            "id": format!("rust:{bundle}"),
            "label": format!("rust:{bundle}"),
            "kind": "rust",
            "source": bundle,
            "desc": "Host-registered Rust tool bundle name",
        }));
    }
    tools
}

fn tool_config_bool_fields() -> Vec<String> {
    serde_json::to_value(ToolConfig::default())
        .ok()
        .and_then(|value| {
            value.as_object().map(|object| {
                object
                    .iter()
                    .filter(|(_, value)| value.is_boolean())
                    .map(|(field, _)| field.to_string())
                    .collect::<Vec<_>>()
            })
        })
        .filter(|fields| !fields.is_empty())
        .unwrap_or_default()
}

fn tool_config_field_desc(field: &str) -> String {
    match field {
        "builtins" => "Built-in host tools such as file reads where enabled",
        "shell" => "Host shell execution tool",
        "comms" => "Peer messaging and comms tools",
        "memory" => "Realm memory and semantic search tools",
        "workgraph" => "WorkGraph commitment tools",
        "mob" => "Mob management tools: spawn, retire, wire, unwire, list",
        "schedule" => "Schedule create/list/update/pause/resume/delete tools",
        "image_generation" => "Assistant image generation tools",
        other => return format!("MobKit ToolConfig.{other} runtime tool flag"),
    }
    .to_string()
}

fn discover_mcp_sources() -> BTreeSet<String> {
    let mut sources = BTreeSet::new();
    if let Ok(raw) = std::env::var("MOBKIT_MCP_SOURCES") {
        sources.extend(split_env_list(&raw));
    }
    for path in mcp_config_paths() {
        sources.extend(mcp_sources_from_toml(&path));
    }
    sources
}

fn discover_rust_tool_bundles() -> BTreeSet<String> {
    std::env::var("MOBKIT_RUST_TOOL_BUNDLES")
        .map(|raw| split_env_list(&raw))
        .unwrap_or_default()
}

fn split_env_list(raw: &str) -> BTreeSet<String> {
    raw.split([',', ':', ';'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn mcp_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".rkat/mcp.toml"));
    }
    if let Ok(home) = std::env::var("HOME") {
        paths.push(PathBuf::from(home).join(".rkat/mcp.toml"));
    }
    paths
}

fn mcp_sources_from_toml(path: &Path) -> BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    let Ok(value) = text.parse::<toml::Value>() else {
        return BTreeSet::new();
    };
    value
        .get("servers")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|server| server.get("name").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn discover_skills_in_dir(dir: &Path) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let skill_path = if path.is_dir() {
            path.join("SKILL.md")
        } else {
            path
        };
        if skill_path.file_name().and_then(|name| name.to_str()) != Some("SKILL.md") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&skill_path) {
            if let Some(skill) = skill_catalog_entry_from_markdown(&skill_path, &content) {
                skills.push(skill);
            }
        }
    }
    skills.sort_by(|a, b| {
        a.get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(b.get("id").and_then(Value::as_str).unwrap_or_default())
    });
    skills
}

fn skill_catalog_entry_from_markdown(path: &Path, content: &str) -> Option<Value> {
    let id = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())?
        .to_string();
    let metadata = parse_skill_frontmatter(content);
    let label = metadata.get("name").cloned().unwrap_or_else(|| id.clone());
    let desc = metadata.get("description").cloned().unwrap_or_default();
    let capabilities = metadata
        .get("requires_capabilities")
        .map(|raw| {
            raw.trim_matches(['[', ']'])
                .split(',')
                .map(|part| part.trim().trim_matches('"').to_string())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(json!({
        "id": id,
        "label": label,
        "source": "path",
        "origin": "filesystem",
        "path": path.to_string_lossy(),
        "desc": desc,
        "requires_capabilities": capabilities,
    }))
}

fn parse_skill_frontmatter(content: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return out;
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            out.insert(
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            );
        }
    }
    out
}

pub fn mobpack_catalogs_response() -> Value {
    let models: Vec<Value> = meerkat_models::catalog()
        .iter()
        .filter_map(|entry| {
            Some(json!({
                "id": entry.id,
                "label": entry.display_name,
                "vendor": entry.provider,
                "provider": entry.provider,
                "profile": meerkat_core::Provider::parse_strict(entry.provider)
                    .and_then(|provider| meerkat_models::profile_for(provider, entry.id))
                    .and_then(|profile| serde_json::to_value(profile).ok()),
            }))
        })
        .collect();
    let provider_defaults: Vec<Value> = meerkat_models::provider_defaults()
        .iter()
        .filter_map(|default| serde_json::to_value(default).ok())
        .collect();
    let sample_mobpacks = sample_mobpack_catalog();
    let blank_mobpack = blank_mobpack_template();
    let skill_realms = discover_skill_realms(&sample_mobpacks);
    let tool_catalog = tool_catalog_response();
    let agent_definitions =
        agent_definition_catalog(&sample_mobpacks, &tool_catalog, &skill_realms);
    json!({
        "tool_catalog": tool_catalog,
        "skill_realms": skill_realms,
        "blank_mobpack": blank_mobpack,
        "sample_mobpacks": sample_mobpacks,
        "agent_definitions": agent_definitions,
        "models": models,
        "provider_defaults": provider_defaults,
    })
}

pub fn mobpack_schema_response() -> Value {
    let mob_settings_defaults = json!({
        "orchestrator": "",
        "autoWireOrchestrator": false,
        "roleWiring": [],
        "backendDefault": "session",
        "externalAddressBase": "",
        "advanced": {
            "topology": null,
            "supervisor": null,
            "limits": null,
            "spawnPolicy": null,
            "eventRouter": null
        }
    });
    let deploy_settings = json!({
        "command": "rkat mob deploy",
        "defaults": {
            "command": "rkat mob deploy",
            "surface": "cli",
            "trust_policy": "permissive",
            "model": "",
            "max_duration": "30s",
            "max_tool_calls": 0,
            "max_total_tokens": 64,
            "isolated": true,
            "realm": "",
            "instance": "",
            "realm_backend": "jsonl",
            "context_root": "",
            "state_root": "",
            "user_config_root": "",
            "prompt": "Reply with exactly OK."
        },
        "surfaces": ["cli", "rpc"],
        "trust_policies": ["permissive", "strict"],
        "realm_backends": ["jsonl", "sqlite"],
        "options": [
            "model",
            "max_total_tokens",
            "max_duration",
            "max_tool_calls",
            "trust_policy",
            "surface",
            "realm",
            "isolated",
            "instance",
            "realm_backend",
            "state_root",
            "context_root",
            "user_config_root"
        ]
    });
    let editor_graph_draft = json!({
        "branch_gate_label": "branch",
        "branch_condition_lane_label": "condition",
        "branch_fallback_lane_label": "fallback",
        "branch_join_label": "join · branch paths",
        "fallback_edge_label": "fallback",
        "parallel_lane_labels": ["lane 1", "lane 2"],
        "parallel_edge_label": "parallel",
        "rework_edge_label": "rework",
        "terminal_edge_label_prefix": "to ",
        "join_label_prefix": "join · ",
        "join_quorum_label_prefix": "barrier · ",
        "branch_frame_label_prefix": "BRANCH · ",
        "branch_frame_singular_suffix": " path",
        "branch_frame_plural_suffix": " paths",
        "parallel_frame_label_prefix": "PARALLEL · ",
        "parallel_frame_join_infix": " · join ",
        "parallel_missing_dispatch_label": "missing dispatch",
        "parallel_missing_collection_label": "missing collection",
        "repeat_frame_label_prefix": "REPEAT-UNTIL · ",
        "repeat_max_iterations_prefix": "max ",
        "repeat_missing_max_iterations_label": "missing max_iterations",
        "repeat_edge_until_prefix": "until ",
        "repeat_edge_until_fallback": "until condition"
    });
    let editor_source_view = json!({
        "drawer_eyebrow": "SOURCE · mob.toml",
        "inline_title": "mob.toml",
        "loading_text": "rendering mob.toml from mobkit/mobpacks/export...",
        "copy_label": "copy",
        "close_label": "×"
    });
    let mut editor_graph_view = json!({
        "zoom_out_title": "Zoom out",
        "fit_title": "Fit to view",
        "zoom_in_title": "Zoom in",
        "port_drag_title": "Drag to a member to connect",
        "add_node_search_icon": "⌕",
        "add_node_search_placeholder": "Add a node…",
        "add_node_close_label": "✕",
        "add_node_close_title": "Close",
        "add_node_agents_label": "Agents",
        "add_node_controls_label": "Flow controls",
        "add_node_empty_prefix": "No matches for “",
        "add_node_empty_suffix": "”",
        "add_node_jump_label": "+ New agent in Agents →",
        "gate_palette_rows": [
            {
                "id": "branch",
                "glyph": "⑂",
                "label": "Branch gate",
                "meta": "conditional split"
            },
            {
                "id": "fork",
                "glyph": "‖",
                "label": "Parallel fork",
                "meta": "fan_out lanes"
            },
            {
                "id": "join",
                "glyph": "⋈",
                "label": "Join gate",
                "meta": "fan_in barrier"
            }
        ]
    });
    editor_graph_view["graph_gate_kind_labels"] = json!({
        "branch": "branch — conditional split",
        "fork": "fork — fan out in parallel",
        "join": "join — wait for branches"
    });
    editor_graph_view["graph_terminal_kind_labels"] = json!({
        "success": "success — done",
        "failed": "failed — blocked",
        "human": "human — needs human"
    });
    editor_graph_view["graph_frame_kind_labels"] = json!({
        "Branch": "Branch — conditional flow frame",
        "Parallel": "Parallel — concurrent flow frame",
        "RepeatUntil": "RepeatUntil — bounded loop frame"
    });
    editor_graph_view["graph_edge_kind_labels"] = json!({
        "next": "next — sequential handoff",
        "fanout": "fanout — parallel sibling",
        "cond": "cond — guarded branch"
    });
    editor_graph_view["inspector_delete_label"] = json!("DELETE");
    editor_graph_view["inspector_label_title"] = json!("LABEL");
    editor_graph_view["inspector_kind_title"] = json!("KIND");
    editor_graph_view["inspector_runtime_default_label"] = json!("runtime default");
    editor_graph_view["instance_eyebrow"] = json!("INSTANCE");
    editor_graph_view["instance_id_line_template"] = json!("{id} · cell ({col},{row})");
    editor_graph_view["instance_member_role_template"] = json!("MEMBER · {role}");
    editor_graph_view["instance_edit_member_label"] = json!("EDIT MEMBER →");
    editor_graph_view["instance_model_label"] = json!("model");
    editor_graph_view["instance_schema_label"] = json!("schema");
    editor_graph_view["instance_tools_label"] = json!("tools");
    editor_graph_view["instance_member_hint"] =
        json!("Editing the member updates every instance that uses it.");
    editor_graph_view["instance_position_title"] = json!("POSITION");
    editor_graph_view["instance_position_stage_label"] = json!("stage (col)");
    editor_graph_view["instance_position_slot_label"] = json!("slot (row)");
    editor_graph_view["instance_output_title_template"] = json!("MEMBER OUTPUT · {schema}");
    editor_graph_view["instance_output_required_label"] = json!("req");
    editor_graph_view["instance_output_hint"] = json!("Defined on the member.");
    editor_graph_view["instance_output_open_member_label"] = json!("Open member →");
    editor_graph_view["gate_eyebrow_template"] = json!("GATE · {kind}");
    editor_graph_view["gate_id_line_template"] = json!("{id} · cell ({col},{row})");
    editor_graph_view["gate_quorum_incoming_template"] = json!("of {count} incoming");
    editor_graph_view["gate_member_option_template"] = json!("{name} · {role}");
    editor_graph_view["terminal_eyebrow_template"] = json!("TERMINAL · {kind}");
    editor_graph_view["terminal_id_line_template"] = json!("{id} · cell ({col},{row})");
    editor_graph_view["edge_eyebrow_template"] = json!("EDGE · {kind}");
    editor_graph_view["edge_title_template"] = json!("{from} → {to}");
    editor_graph_view["edge_id_line_template"] = json!("{id}");
    editor_graph_view["edge_field_placeholder"] = json!("— field —");
    editor_graph_view["edge_field_no_schema_placeholder"] = json!("(no schema)");
    editor_graph_view["gate_collection_title"] = json!("COLLECTION POLICY");
    editor_graph_view["gate_join_member_label"] = json!("Join member");
    editor_graph_view["gate_join_member_placeholder"] = json!("— select member —");
    editor_graph_view["gate_join_member_hint"] =
        json!("MobKit uses this real profile to resolve non-all fan-in.");
    editor_graph_view["gate_dispatch_title"] = json!("DISPATCH MODE");
    editor_graph_view["gate_dispatch_hint"] =
        json!("Exports as the MobKit parallel flow dispatch mode.");
    editor_graph_view["gate_conditions_title"] = json!("CONDITIONS");
    editor_graph_view["gate_empty_branch_hint"] =
        json!("add outgoing edges, then configure each as a typed condition or fallback");
    editor_graph_view["gate_wiring_title"] = json!("WIRING");
    editor_graph_view["gate_incoming_label"] = json!("incoming");
    editor_graph_view["gate_outgoing_label"] = json!("outgoing");
    editor_graph_view["branch_condition_mode_condition_label"] = json!("condition");
    editor_graph_view["branch_condition_mode_fallback_label"] = json!("fallback");
    editor_graph_view["branch_condition_target_prefix"] = json!("→");
    editor_graph_view["graph_condition_target_missing_label"] = json!("?");
    editor_graph_view["graph_condition_owner_option_template"] = json!("{name}");
    editor_graph_view["graph_condition_field_option_template"] = json!("{name} · {type}");
    editor_graph_view["branch_input_param_source_label"] = json!("Input params");
    editor_graph_view["source_file_label"] = json!("mob.toml");
    editor_graph_view["source_file_aria_label"] = json!("Open mob.toml read-only source editor");
    editor_graph_view["source_file_glyph"] = json!("{ }");
    editor_graph_view["source_file_role_label"] = json!("source file");
    editor_graph_view["branch_condition_field_placeholder"] = json!("— field —");
    editor_graph_view["branch_condition_no_options_hint"] =
        json!("add input params or an upstream schema field for this condition");
    editor_graph_view["edge_condition_title"] = json!("CONDITION");
    editor_graph_view["edge_no_condition_options_hint"] =
        json!("Add an upstream agent with an output schema before configuring this edge.");
    editor_graph_view["edge_owner_placeholder"] = json!("— member —");
    editor_graph_view["edge_from_title"] = json!("FROM");
    editor_graph_view["edge_to_title"] = json!("TO");
    editor_graph_view["edge_row_instance_label"] = json!("instance");
    editor_graph_view["edge_row_member_label"] = json!("member");
    editor_graph_view["edge_row_schema_label"] = json!("schema");
    editor_graph_view["edge_row_missing_value"] = json!("—");
    editor_graph_view["edge_terminal_member_value"] = json!("(terminal)");
    let editor_condition_view = json!({
        "empty_value_label": "—",
        "text_value_placeholder": "value"
    });
    let editor_error_view = json!({
        "critical_glyph": "!",
        "generic_error_head": "MobKit error",
        "deploy_failed_head": "Deploy failed",
        "deploy_plan_failed_head": "Deploy plan failed",
        "deploy_error_meta": "mobkit/mobpacks/deploy",
        "source_failed_head": "Source render failed",
        "source_error_meta": "mobkit/mobpacks/export",
        "validation_api_failed_head": "MobKit API unavailable",
        "rpc_error_meta": "/flow-editor/rpc",
        "export_failed_head": "Export failed",
        "import_failed_head": "Import failed",
        "missing_editor_flow_head": "Imported mobpack is missing a MobKit editor flow",
        "missing_editor_flow_sub": "mobkit/mobpacks/import did not return document.flow.steps",
        "missing_editor_flow_meta": "missing_editor_flow"
    });
    let editor_agent_view = json!({
        "agents_heading": "AGENTS",
        "schemas_heading": "SCHEMAS",
        "add_schema_label": "+ new schema",
        "add_agent_title": "Create an agent from a MobKit profile-member definition.",
        "add_agent_unavailable_title": "MobKit schema contract has not provided agent definitions yet.",
        "add_agent_unavailable_label": "agents unavailable",
        "add_agent_placeholder_label": "+ new agent...",
        "add_agent_error_prefix": "Agent definition unavailable: ",
        "empty_title": "AGENT LIBRARY",
        "empty_lines": [
            "Select an agent or schema on the left.",
            "Agents are reusable across topologies. Edit one here and every placement updates."
        ],
        "missing_schema_label": "Schema not found.",
        "missing_agent_label": "Agent not found."
    });
    let editor_schema_view = json!({
        "eyebrow": "OUTPUT SCHEMA",
        "description_title": "DESCRIPTION",
        "description_placeholder": "What is this artifact and when is it emitted?",
        "fields_title_prefix": "FIELDS",
        "add_field_label": "+ field",
        "header_labels": {
            "name": "NAME",
            "type": "TYPE",
            "required": "REQ",
            "description": "DESCRIPTION",
            "action": ""
        },
        "empty_fields_hint": "No fields yet. Click + field to start.",
        "used_by_prefix": "USED BY",
        "empty_used_by_hint": "Not yet referenced by any agent.",
        "delete_label": "DELETE",
        "delete_blocked_title": "Unassign from agents first",
        "field_name_placeholder": "field_name",
        "field_description_placeholder": "—",
        "field_remove_title": "Remove field",
        "field_enum_label": "VALUES",
        "field_enum_add_label": "+ value",
        "field_enum_add_value": "value"
    });
    let mut editor_agent_detail_view = json!({
        "used_in_label": "used in",
        "instance_singular": "instance",
        "instance_plural": "instances",
        "delete_label": "DELETE",
        "delete_confirm_intro": "Delete agent",
        "delete_confirm_placed_prefix": "It is placed in",
        "cell_singular": "cell",
        "cell_plural": "cells",
        "delete_confirm_cells_suffix": "those nodes will be removed.",
        "usage_title_prefix": "USED IN",
        "empty_usage_hint": "Not yet placed in any cell. Switch to Topology to add.",
        "identity_title": "IDENTITY",
        "profile_binding_label": "Profile binding",
        "missing_profile_binding_label": "missing profile binding",
        "realm_profile_label": "Realm profile",
        "realm_profile_placeholder": "realm profile id",
        "realm_profile_import_hint_fallback": "Realm profile refs are import-only for this editor. Mobpack archives must use inline profiles before validation/export.",
        "realm_profile_title": "REALM PROFILE",
        "realm_profile_reference_hint_before": "This imported member references",
        "realm_profile_reference_hint_after_fallback": "from a target realm. Convert it to an inline profile before validating or exporting a deployable mobpack.",
        "model_label": "Model",
        "runtime_mode_label": "Runtime mode",
        "missing_runtime_mode_label": "missing runtime mode",
        "backend_label": "Backend",
        "backend_definition_default_label": "definition default",
        "inline_peer_notifications_label": "Inline peer notifications",
        "inline_peer_notifications_placeholder": "runtime default",
        "provider_params_label": "Provider params",
        "provider_params_placeholder": "{\"thinking_budget\":4096}",
        "provider_params_rows": 4,
        "provider_params_invalid_json_label": "invalid JSON",
        "provider_params_object_required_error": "provider_params must be a JSON object",
        "system_prompt_title": "SYSTEM PROMPT",
        "apply_skeleton_label": "APPLY SKELETON",
        "apply_skeleton_title": "Apply a MobKit profile prompt skeleton",
        "system_prompt_placeholder": "Describe the member mandate. This text is exported as the profile peer_description.",
        "output_schema_title": "OUTPUT SCHEMA",
        "schema_none_label": "— none —",
        "schema_required_label": "req",
        "edit_schema_label": "Edit schema →",
        "empty_schema_hint": "No structured output. Agent returns free-form text."
    });
    editor_agent_detail_view["source_title"] = json!("SOURCE");
    editor_agent_detail_view["source_empty_hint"] = json!("Created in this editor.");
    editor_agent_detail_view["source_definition_label"] = json!("Definition");
    editor_agent_detail_view["source_mobpack_label"] = json!("Mobpack");
    editor_agent_detail_view["source_origin_label"] = json!("Origin");
    editor_agent_detail_view["source_document_path_label"] = json!("Member path");
    editor_agent_detail_view["source_schema_path_label"] = json!("Schema path");
    editor_agent_detail_view["source_tools_label"] = json!("Tool refs");
    editor_agent_detail_view["source_skills_label"] = json!("Skill refs");
    let editor_agent_access_view = json!({
        "tool_invalid_error": "Use a MobKit-listed runtime tool or configured MCP/Rust source.",
        "tool_title": "TOOL ACCESS",
        "tool_hint": "Authority is calculated from this allowlist. Reviewed once here.",
        "tool_missing_description": "—",
        "tool_remove_label": "×",
        "tool_add_select_placeholder": "+ add tool…",
        "tool_source_label": "Configured tool source",
        "tool_source_placeholder": "choose from MobKit tool catalog",
        "tool_add_button_label": "ADD",
        "inline_skill_realm_id": "mobkit/editor-inline",
        "inline_skill_realm_label": "This mobpack",
        "inline_skill_default_description": "Inline MobKit skill stored in this mobpack.",
        "skill_default_description": "MobKit skill",
        "skill_selected_check_label": "✓",
        "skill_remove_label": "×",
        "skill_section_title": "SKILLS",
        "skill_inline_cancel_label": "CANCEL",
        "skill_inline_open_label": "+ INLINE",
        "skill_hint": "Selected skills are baked into the mobpack. Browse a realm to add more.",
        "skill_inline_label_placeholder": "mob.skill-name",
        "skill_inline_content_rows": 4,
        "skill_inline_content_placeholder": "Skill instructions stored as [skills.<id>] content",
        "skill_inline_create_hint": "Creates an inline skill definition in this mobpack.",
        "skill_inline_add_label": "ADD SKILL",
        "skill_inline_error_fallback": "Could not create inline skill.",
        "skill_inline_missing_label_error": "Inline skill id or label is required.",
        "skill_inline_missing_content_error": "Inline skill content is required.",
        "skill_inline_invalid_id_error": "Inline skill id or label must contain letters or numbers.",
        "skill_no_realms_message": "MobKit did not provide skill realms for this document.",
        "skill_realm_label": "Realm",
        "skill_default_realm_suffix": " · default",
        "skill_unavailable_heading": "Unavailable in MobKit skill realms:",
        "skill_outside_realm_heading": "Selected from other realms:"
    });
    let editor_new_flow_view = json!({
        "eyebrow_template": "NEW FLOW · STEP {step} OF 2",
        "close_label": "×",
        "name_label": "Name",
        "name_placeholder": "docs-only",
        "trigger_label": "Trigger",
        "trigger_placeholder": "label · docs",
        "start_from_label": "Start from",
        "back_label": "← BACK",
        "next_label": "NEXT →",
        "create_label": "CREATE"
    });
    let editor_flow_registry_view = json!({
        "eyebrow": "FLOWS",
        "title_singular_suffix": "flow",
        "title_plural_suffix": "flows",
        "create_label": "+ NEW FLOW",
        "create_ready_title": "Create a MobKit mobpack",
        "create_unavailable_title": "Waiting for MobKit schema",
        "columns": [
            { "key": "name", "label": "NAME" },
            { "key": "trigger", "label": "TRIGGER" },
            { "key": "version", "label": "VERSION" },
            { "key": "stage", "label": "STAGE" }
        ]
    });
    let editor_deploy_view = json!({
        "brand_label": "MobKit · Flow Editor",
        "flows_tab_label": "FLOWS",
        "agents_tab_label": "AGENTS",
        "mob_status_title": "Active mob configuration",
        "mob_file_label": "mob.toml",
        "api_error_label": "api error",
        "api_ready_label": "api ready",
        "api_loading_label": "loading",
        "deploy_prefix_label": "deploy:",
        "flows_crumb_label": "flows",
        "crumb_separator": "/",
        "plan_trace_label": "PLAN TRACE",
        "import_label": "IMPORT",
        "validate_label": "VALIDATE",
        "publish_label": "PUBLISH",
        "deploy_plan_label": "DEPLOY PLAN",
        "deploy_label": "DEPLOY",
        "theme_switch_prefix": "Switch to",
        "theme_switch_suffix": "mode",
        "dark_theme_label": "☾ dark",
        "light_theme_label": "☀ light",
        "basic_mode_title": "Basic Editor",
        "basic_mode_label": "Basic",
        "graph_mode_title": "Graph Editor",
        "graph_mode_label": "Graph",
        "validation_eyebrow": "VALIDATE · MobKit",
        "validation_passed_label": "passed",
        "validation_warnings_label": "warnings",
        "validation_blocking_label": "blocking",
        "close_label": "×",
        "plan_eyebrow": "DEPLOY PLAN",
        "plan_unavailable_head": "DEPLOY TRACE UNAVAILABLE",
        "plan_unavailable_body": "mobkit/mobpacks/deploy did not return plan_trace.",
        "plan_first_label": "first",
        "plan_step_label": "step",
        "plan_previous_label": "‹",
        "plan_next_label": "›"
    });
    let mut editor_settings_view = json!({
        "panel_title": "Tweaks",
        "load_mob_title": "Load mob",
        "load_mob_label": "Mobpack",
        "flow_stage_fallback": "draft",
        "option_separator": " · ",
        "canvas_title": "Canvas",
        "edge_style_label": "Edges",
        "density_label": "Density",
        "theme_title": "Theme",
        "theme_mode_label": "Mode",
        "mob_title": "Mob",
        "orchestrator_label": "Orchestrator",
        "profile_none_label": "none",
        "auto_wire_label": "Auto wire",
        "default_backend_label": "Default backend",
        "external_base_label": "External base",
        "external_base_placeholder": "http://127.0.0.1:9000",
        "deploy_title": "Deploy",
        "surface_label": "Surface",
        "trust_label": "Trust",
        "model_label": "Model",
        "model_default_label": "default",
        "model_vendor_fallback": "provider",
        "duration_label": "Duration",
        "duration_placeholder": "30s",
        "tool_calls_label": "Tool calls",
        "tool_calls_min": 0,
        "tool_calls_max": 999,
        "tokens_label": "Tokens",
        "tokens_min": 0,
        "tokens_max": 200000,
        "realm_label": "Realm",
        "realm_id_label": "Realm ID",
        "realm_id_placeholder": "realm id",
        "backend_label": "Backend",
        "prompt_label": "Prompt",
        "prompt_placeholder": "Deploy prompt",
        "command_label": "Command",
        "command_fallback": "--",
        "inspector_title": "Inspector",
        "inspector_layout_label": "Layout"
    });
    editor_settings_view["edge_style_options"] = json!([
        { "value": "text", "label": "Text" },
        { "value": "icons", "label": "Icons" },
        { "value": "colored", "label": "Color" }
    ]);
    editor_settings_view["density_options"] = json!([
        { "value": "compact", "label": "Compact" },
        { "value": "comfortable", "label": "Comfy" }
    ]);
    editor_settings_view["theme_mode_options"] = json!([
        { "value": "light", "label": "Light" },
        { "value": "dark", "label": "Dark" }
    ]);
    editor_settings_view["auto_wire_options"] = json!([
        { "value": "no", "label": "No" },
        { "value": "yes", "label": "Yes" }
    ]);
    editor_settings_view["role_wiring_label"] = json!("Role wiring");
    editor_settings_view["role_wiring_add_label"] = json!("+ rule");
    editor_settings_view["panel_close_label"] = json!("Close tweaks");
    editor_settings_view["advanced_label"] = json!("Advanced");
    editor_settings_view["advanced_object_required_error"] = json!("object required");
    editor_settings_view["advanced_invalid_json_error"] = json!("invalid JSON");
    editor_settings_view["realm_options"] = json!([
        { "value": "isolated", "label": "Isolated" },
        { "value": "shared", "label": "Shared" }
    ]);
    editor_settings_view["inspector_layout_options"] = json!([
        { "value": "right", "label": "Right" },
        { "value": "bottom", "label": "Bottom" },
        { "value": "modal", "label": "Modal" }
    ]);
    let editor_launch_view = json!({
        "launch_title": "Launch mode",
        "graph_launch_title": "LAUNCH MODE · this position",
        "resume_session_label": "Bridge session",
        "resume_session_placeholder": "session id",
        "fork_source_label": "Fork from",
        "fork_context_label": "Fork context",
        "graph_fork_context_label": "Context",
        "budget_policy_label": "Budget split policy",
        "fixed_budget_label": "Fixed token budget",
        "fixed_budget_default_value": 4096,
        "unsupported_label_separator": " — not in MobKit ",
        "unsupported_reason_prefix": "Unsupported by the MobKit ",
        "unsupported_reason_suffix": " contract.",
        "launch_modes_contract_label": "launch_modes",
        "fork_contexts_contract_label": "mob_definition.fork_contexts",
        "budget_split_policies_contract_label": "budget_split_policies",
        "launch_mode_labels": {
            "Fresh": "Fresh — empty context",
            "Resume": "Resume — existing bridge session",
            "Fork": "Fork — copy context from another step"
        },
        "fork_context_labels": {
            "full_history": "full_history — entire transcript",
            "last_messages": "last_messages — last N messages",
            "FullHistory": "FullHistory — legacy alias for full_history"
        },
        "budget_split_policy_labels": {
            "Equal": "Equal — split remaining budget evenly",
            "Proportional": "Proportional — MobKit proportional split",
            "Remaining": "Remaining — grant all remaining budget",
            "Fixed": "Fixed — token cap for this spawn"
        }
    });
    let mut editor_basic_view = json!({
        "start_label": "START",
        "loop_badge": "LOOP",
        "tips_title": "Tips",
        "empty_panel_title": "Build your mob flow",
        "empty_panel_subtitle_parts": [
            { "kind": "text", "text": "Pick a node to configure, or press " },
            { "kind": "strong", "text": "+" },
            { "kind": "text", "text": " to add a member turn or flow primitive. The result is a " },
            { "kind": "code", "text": "mob.toml" },
            { "kind": "text", "text": " flow." }
        ],
        "source_toggle_label": "{ } mob.toml"
    });
    editor_basic_view["member_step_panel_title_fallback"] = json!("Member step");
    editor_basic_view["member_step_panel_sub_fallback"] = json!("Assign a member to run this step");
    editor_basic_view["member_step_member_label"] = json!("Member (profile)");
    editor_basic_view["member_step_member_placeholder"] = json!("— select member —");
    editor_basic_view["member_step_runtime_default_label"] = json!("runtime default");
    editor_basic_view["member_step_instruction_label"] =
        json!("message — instruction for this turn");
    editor_basic_view["member_step_instruction_placeholder"] =
        json!("e.g. Run the focused tests and report failures.");
    editor_basic_view["member_step_dispatch_label"] = json!("Dispatch mode");
    editor_basic_view["member_step_collection_label"] = json!("Collection policy");
    editor_basic_view["member_step_quorum_label"] = json!("Quorum");
    editor_basic_view["member_step_quorum_placeholder"] = json!("required");
    editor_basic_view["member_step_timeout_label"] = json!("Timeout (ms)");
    editor_basic_view["member_step_dependency_label"] = json!("depends_on mode");
    editor_basic_view["member_step_output_format_label"] = json!("Output format");
    editor_basic_view["member_step_allowed_tools_label"] = json!("Allowed tools");
    editor_basic_view["member_step_allowed_tools_empty_label"] = json!("Runtime profile default");
    editor_basic_view["member_step_blocked_tools_label"] = json!("Blocked tools");
    editor_basic_view["member_step_blocked_tools_empty_label"] = json!("No step-level blocks");
    editor_basic_view["member_step_schema_hint_prefix"] = json!("Emits ");
    editor_basic_view["member_step_schema_hint_tools_prefix"] = json!(" · tools: ");
    editor_basic_view["member_step_schema_hint_empty_tools_label"] = json!("—");
    editor_basic_view["tool_scope_not_in_catalog_reason"] = json!("not in MobKit tool catalog");
    editor_basic_view["tool_scope_not_enabled_reason"] = json!("not enabled on profile");
    editor_basic_view["tool_scope_tool_description_fallback"] = json!("MobKit tool");
    editor_basic_view["tool_scope_remove_label"] = json!("×");
    editor_basic_view["tool_scope_select_member_placeholder"] = json!("select a member first");
    editor_basic_view["tool_scope_block_catalog_placeholder"] = json!("+ block MobKit tool...");
    editor_basic_view["tool_scope_add_profile_placeholder"] = json!("+ add profile tool...");
    editor_basic_view["input_panel_icon"] = json!("▤");
    editor_basic_view["input_panel_title"] = json!("Input");
    editor_basic_view["input_panel_sub"] = json!("The task this mob is run with — its ingress");
    editor_basic_view["input_task_label"] = json!("Task");
    editor_basic_view["input_task_placeholder"] = json!("e.g. Fix the issue described below.");
    editor_basic_view["input_params_title_prefix"] = json!("INPUT PARAMS");
    editor_basic_view["input_add_param_label"] = json!("+ param");
    editor_basic_view["input_param_source_label"] = json!("Input params");
    editor_basic_view["input_param_header_labels"] = json!({
        "name": "NAME",
        "type": "TYPE",
        "required": "REQ",
        "description": "DESCRIPTION",
        "action": ""
    });
    editor_basic_view["input_param_name_placeholder"] = json!("param_name");
    editor_basic_view["input_param_description_placeholder"] = json!("—");
    editor_basic_view["input_param_remove_title"] = json!("Remove param");
    editor_basic_view["input_param_enum_label"] = json!("VALUES");
    editor_basic_view["input_param_enum_add_label"] = json!("+ value");
    editor_basic_view["input_param_enum_add_value"] = json!("value");
    editor_basic_view["input_empty_params_parts"] = json!([
        { "key": "prefix", "text": "No params yet. Add one before branching on " },
        { "key": "ref", "text": "params.*", "kind": "code" },
        { "key": "suffix", "text": "." }
    ]);
    editor_basic_view["input_tips"] = json!([
        "Run with: rkat mob deploy <pack> \"<task>\" — or run_flow(input).",
        "Typed fields become the input schema the run is validated against.",
        "Event sources & schedules live outside the mobpack (e.g. fugue)."
    ]);
    editor_basic_view["branch_panel_title"] = json!("Branch");
    editor_basic_view["branch_panel_sub"] = json!("Choose one downstream path by condition");
    editor_basic_view["parallel_panel_title"] = json!("Parallel");
    editor_basic_view["parallel_panel_sub"] = json!("fan_out to members, then fan_in and collect");
    editor_basic_view["branch_route_member_label"] = json!("Route member");
    editor_basic_view["parallel_join_member_label"] = json!("Join member");
    editor_basic_view["branch_controller_placeholder_label"] = json!("— direct MobKit lanes —");
    editor_basic_view["branch_empty_controller_hint"] = json!(
        "Without a selected profile, MobKit conditions/parallel lanes attach directly to the first real member in each lane."
    );
    editor_basic_view["branch_condition_title"] = json!("Branch conditions");
    editor_basic_view["branch_condition_intro"] =
        json!("Read in order; the first match wins. Conditions read a member's structured output.");
    editor_basic_view["branch_condition_row_title_prefix"] = json!("Branch");
    editor_basic_view["branch_condition_empty_hint"] =
        json!("Add an upstream member with an output schema before configuring this branch.");
    editor_basic_view["branch_condition_source_placeholder"] = json!("— source —");
    editor_basic_view["branch_condition_field_placeholder"] = json!("— field —");
    editor_basic_view["branch_condition_no_schema_label"] = json!("(no schema)");
    editor_basic_view["branch_condition_preview_prefix"] = json!("when");
    editor_basic_view["branch_condition_preview_fallback"] = json!("…");
    editor_basic_view["branch_fallback_title"] = json!("Fallback");
    editor_basic_view["branch_fallback_hint"] =
        json!("If none match, the flow follows the fallback path; else it stops.");
    editor_basic_view["add_branch_label"] = json!("+ Add branch");
    editor_basic_view["add_parallel_branch_label"] = json!("+ Add parallel branch");
    editor_basic_view["parallel_dispatch_label"] = json!("Dispatch mode");
    editor_basic_view["parallel_collection_label"] = json!("Collection policy (fan_in)");
    editor_basic_view["parallel_quorum_label"] = json!("Quorum (N)");
    editor_basic_view["parallel_quorum_placeholder"] = json!("required");
    editor_basic_view["branch_dependency_label"] = json!("depends_on mode");
    editor_basic_view["repeat_panel_title"] = json!("Repeat until");
    editor_basic_view["repeat_panel_sub"] =
        json!("Loop the body, then evaluate the condition after each iteration");
    editor_basic_view["repeat_loop_id_label"] = json!("loop_id");
    editor_basic_view["repeat_loop_id_placeholder"] = json!("quality_loop");
    editor_basic_view["repeat_condition_title"] = json!("Until condition");
    editor_basic_view["repeat_condition_intro"] = json!(
        "Evaluated on a body member's structured output after each pass. The loop exits when it holds."
    );
    editor_basic_view["repeat_empty_body_hint"] =
        json!("Add a member step inside the loop first — the condition reads its output schema.");
    editor_basic_view["repeat_member_placeholder_label"] = json!("— member —");
    editor_basic_view["repeat_condition_field_placeholder"] = json!("— field —");
    editor_basic_view["repeat_condition_no_schema_label"] = json!("(no schema)");
    editor_basic_view["repeat_preview_label"] = json!("until");
    editor_basic_view["repeat_preview_fallback"] = json!("…");
    editor_basic_view["repeat_iteration_input_label"] =
        json!("Iteration input — what each pass receives");
    editor_basic_view["repeat_max_iterations_label"] = json!("max_iterations");
    editor_basic_view["repeat_max_iterations_placeholder"] = json!("required");
    editor_basic_view["repeat_tips"] = json!([
        "The body is its own FrameSpec — add member steps inside the loop.",
        "The condition reads a member's typed output (e.g. reviewer.verdict == green).",
        "max_iterations bounds the loop so it always terminates."
    ]);
    editor_basic_view["repeat_canvas_while_label"] = json!("while");
    editor_basic_view["repeat_canvas_not_label"] = json!("not");
    editor_basic_view["repeat_canvas_missing_max_iterations_label"] =
        json!("missing max_iterations");
    editor_basic_view["repeat_canvas_max_iterations_prefix"] = json!("max ");
    editor_basic_view["repeat_canvas_loop_back_prefix"] = json!("↑ loop back · ");
    editor_basic_view["repeat_canvas_exit_prefix"] = json!("↓ exit when ");
    editor_basic_view["repeat_canvas_exit_fallback"] = json!("condition met");
    editor_basic_view["repeat_iteration_runtime_default_label"] = json!("runtime default");
    editor_basic_view["repeat_iteration_carry_label"] = json!("carries last output");
    editor_basic_view["repeat_iteration_reuse_unsupported_label"] =
        json!("unsupported: re-use input task");
    editor_basic_view["repeat_iteration_feeds_unsupported_prefix"] = json!("unsupported: feeds ");
    editor_basic_view["repeat_iteration_unsupported_prefix"] = json!("unsupported: ");
    editor_basic_view["add_step_title"] = json!("Add step");
    editor_basic_view["input_step_card_title"] = json!("Input");
    editor_basic_view["input_step_card_desc_fallback"] = json!("the task this mob is run with");
    editor_basic_view["branch_step_card_title"] = json!("Branch");
    editor_basic_view["branch_step_card_desc"] = json!("Mob picks the first matching path");
    editor_basic_view["parallel_step_card_title"] = json!("Parallel");
    editor_basic_view["parallel_step_card_desc_prefix"] = json!("fan-out → join · ");
    editor_basic_view["parallel_step_card_collection_fallback"] = json!("—");
    editor_basic_view["repeat_step_card_title"] = json!("Repeat until");
    editor_basic_view["repeat_step_card_desc_prefix"] = json!("until ");
    editor_basic_view["repeat_step_card_desc_fallback"] = json!("loop body until condition");
    editor_basic_view["member_step_card_title_fallback"] = json!("Select member");
    editor_basic_view["picker_kickoff_title"] = json!("Input");
    editor_basic_view["picker_kickoff_sub"] =
        json!("Every mob run starts from a single task input");
    editor_basic_view["picker_kickoff_hint"] = json!(
        "This node is the mob's ingress — the task it's deployed/run with. Select it on the canvas to edit the task and any typed input fields."
    );
    editor_basic_view["picker_title"] = json!("Add step");
    editor_basic_view["picker_sub"] = json!("A flow node — a member turn or a flow primitive");
    editor_basic_view["picker_search_icon"] = json!("⌕");
    editor_basic_view["picker_search_placeholder"] = json!("Search members & primitives…");
    editor_basic_view["picker_members_label"] = json!("Mob members");
    editor_basic_view["picker_flow_label"] = json!("Flow");
    editor_basic_view["picker_empty_members_hint"] =
        json!("No members yet — define some in the Agents tab.");
    editor_basic_view["picker_new_badge_label"] = json!("NEW");
    editor_basic_view["flow_primitive_rows"] = json!([
        {
            "id": "repeat",
            "glyph": "↻",
            "tint": "member",
            "label": "Repeat until",
            "sub": "Loop a body of steps until a condition holds (max_iterations)"
        },
        {
            "id": "branch",
            "glyph": "⑂",
            "tint": "member",
            "label": "Branch",
            "sub": "Pick one downstream path by condition (first match wins)"
        },
        {
            "id": "parallel",
            "glyph": "‖",
            "tint": "member",
            "label": "Parallel",
            "sub": "fan_out to several members, then fan_in with a collection policy"
        }
    ]);
    let editor_graph_template_view = json!({
        "template_eyebrow": "TEMPLATE",
        "summary_title": "SUMMARY",
        "triggers_title": "TRIGGERS",
        "trigger_labels_label": "labels",
        "trigger_default_label": "default",
        "default_yes_label": "yes",
        "default_no_label": "no",
        "summary_members_label": "members",
        "summary_instances_label": "instances",
        "summary_terminals_label": "terminals",
        "summary_edges_label": "edges",
        "summary_frames_label": "frames",
        "summary_members_value_template": "{placed} placed / {total} in library",
        "quick_start_title": "QUICK START",
        "quick_start_rows": [
            [
                { "kind": "text", "text": "Click a " },
                { "kind": "strong", "text": "library member" },
                { "kind": "text", "text": " on the left to edit it." }
            ],
            [
                { "kind": "text", "text": "Click an " },
                { "kind": "strong", "text": "empty grid cell" },
                { "kind": "text", "text": " to place a member." }
            ],
            [
                { "kind": "text", "text": "Drag a node's " },
                { "kind": "strong", "text": "right port" },
                { "kind": "text", "text": " to wire it to another." }
            ],
            [
                { "kind": "text", "text": "⌫ deletes the selected instance or edge." }
            ]
        ]
    });
    let mut mob_definition = json!({
        "authoritative_type": "meerkat_mob::MobDefinition",
        "defaults": {
            "runtime_mode": "turn_driven",
            "launch_mode": "fresh",
            "fork_context": "full_history",
            "budget_split_policy": "equal",
            "dispatch_mode": "fan_out",
            "collection_policy": "all",
            "dependency_mode": "all",
            "condition_operator": "==",
            "schema_field_type": "string",
            "branch_param_type": "enum",
            "repeat_iteration_input": "carry",
            "step_output_format": "json",
            "graph_gate_kind": "branch",
            "graph_edge_kind": "next",
            "graph_condition_edge_kind": "cond",
            "graph_fanout_edge_kind": "fanout",
            "graph_terminal_kind": "success"
        },
        "profile_binding": ["inline"],
        "profile_binding_restrictions": {
            "inline": {
                "deployable": true,
                "label": "inline — define profile in this mobpack",
                "reason": "",
                "document_path": "document.members[].profileBinding",
                "archive_path": "profiles.<name>"
            },
            "realm_profile": {
                "deployable": false,
                "label": "realm_profile — import-only; rkat mob validate forbids realm refs in packs",
                "reason": "rkat mob validate rejects mobpack profiles that use realm_profile references; export deployable packs with inline profiles.",
                "document_path": "document.members[].realmProfile",
                "archive_path": "profiles.<name>.realm_profile"
            }
        },
        "runtime_modes": runtime_mode_values(),
        "profile_backends": ["session", "external"],
        "launch_modes": member_launch_mode_values(),
        "launch_mode_document_path": "document.launch_modes[]",
        "fork_contexts": fork_context_values(),
        "fork_context_document_path": "document.launch_modes[].context",
        "budget_split_policies": budget_split_policy_values(),
        "budget_split_policy_document_path": "document.launch_modes[].budget_split_policy",
        "input_schema_document_path": "document.flow.steps[type=input].inputParams",
        "input_schema_archive_path": "schemas/main-input.json",
        "editor_input_param_draft": {
            "document_path": "document.flow.steps[type=input].inputParams",
            "archive_path": "schemas/main-input.json",
            "added_field": {
                "name": "param",
                "required": true,
                "description": "",
                "enumValues": []
            }
        },
        "editor_schema_field_types": editor_schema_field_type_values(),
        "editor_schema_view": editor_schema_view,
        "editor_schema_draft": {
            "document_path": "document.schemas[]",
            "archive_path": "schemas/<schema-id>.json",
            "schema_id_prefix": "Artifact",
            "initial_field": {
                "name": "field_one",
                "required": true,
                "description": "",
                "enumValues": []
            },
            "added_field": {
                "name": "new_field",
                "required": false,
                "description": "",
                "enumValues": []
            }
        },
        "condition_operators": ["==", ">", "<"],
        "step_output_formats": step_output_format_values(),
        "skill_source_document_path": "document.skill_realms[].skills[]",
        "path_skill_archive_path": "skills/<skill-id>.md or a safe relative skill path",
        "mob_settings_document_path": "document.mob_settings",
        "mob_settings": {
            "defaults": mob_settings_defaults,
            "orchestrator": "profile name or empty string",
            "autoWireOrchestrator": "boolean",
            "roleWiring": [{ "a": "profile name", "b": "profile name" }],
            "backendDefault": ["session", "external"],
            "externalAddressBase": "required by MobKit validation when external backend is selected",
            "advanced": {
                "topology": "MobDefinition.topology JSON object or null",
                "supervisor": "MobDefinition.supervisor JSON object or null",
                "limits": "MobDefinition.limits JSON object or null",
                "spawnPolicy": "MobDefinition.spawn_policy JSON object or null",
                "eventRouter": "MobDefinition.event_router JSON object or null"
            }
        },
        "flow_node_kinds": ["step", "repeat_until"],
        "editor_flow_step_types": EDITOR_FLOW_STEP_TYPES,
        "repeat_iteration_inputs": REPEAT_ITERATION_INPUTS,
        "graph_gate_kinds": GRAPH_GATE_KINDS,
        "graph_palette_gate_kinds": GRAPH_PALETTE_GATE_KINDS,
        "graph_terminal_kinds": GRAPH_TERMINAL_KINDS,
        "graph_frame_kinds": GRAPH_FRAME_KINDS,
        "graph_edge_kinds": GRAPH_EDGE_KINDS,
        "editor_graph_draft": editor_graph_draft,
        "editor_graph_view": editor_graph_view,
        "editor_source_view": editor_source_view,
        "editor_agent_view": editor_agent_view,
        "editor_basic_view": editor_basic_view,
        "editor_graph_template_view": editor_graph_template_view,
        "dispatch_modes": dispatch_mode_values(),
        "collection_policies": collection_policy_values(),
        "dependency_modes": dependency_mode_values()
    });
    mob_definition["editor_condition_view"] = editor_condition_view;
    mob_definition["editor_error_view"] = editor_error_view;
    mob_definition["editor_agent_detail_view"] = editor_agent_detail_view;
    mob_definition["editor_agent_access_view"] = editor_agent_access_view;
    mob_definition["editor_new_flow_view"] = editor_new_flow_view;
    mob_definition["editor_flow_registry_view"] = editor_flow_registry_view;
    mob_definition["editor_deploy_view"] = editor_deploy_view;
    mob_definition["editor_settings_view"] = editor_settings_view;
    mob_definition["editor_launch_view"] = editor_launch_view;
    mob_definition["editor_input_step_draft"] = editor_input_step_draft_contract();
    mob_definition["deploy_runtime_mode_compatibility"] = deploy_runtime_mode_compatibility();
    mob_definition["runtime_mode_labels"] = runtime_mode_labels();
    mob_definition["dispatch_mode_labels"] = dispatch_mode_labels();
    mob_definition["collection_policy_labels"] = collection_policy_labels();
    mob_definition["dependency_mode_labels"] = dependency_mode_labels();
    mob_definition["option_unsupported_label_separator"] = json!(" — not in MobKit ");
    mob_definition["option_unsupported_reason_prefix"] = json!("Unsupported by the MobKit ");
    mob_definition["option_unsupported_reason_suffix"] = json!(" contract.");
    json!({
        "schema_version": MOBPACK_SCHEMA_VERSION,
        "media_type": MOBPACK_MEDIA_TYPE,
        "output": "mobpack",
        "archive": "tar.gz",
        "files": ["manifest.toml", "definition.json", "mobkit/editor.json", "mobkit/mob.toml", "schemas/*.json", "skills/*.md"],
        "runtime_mutation": false,
        "required_fields": ["document"],
        "commands": {
            "schema": "mobkit/mobpacks/schema",
            "catalogs": "mobkit/mobpacks/catalogs",
            "validate": "mobkit/mobpacks/validate",
            "export": "mobkit/mobpacks/export",
            "import": "mobkit/mobpacks/import",
            "deploy_command": "mobkit/mobpacks/deploy_command",
            "deploy_rpc": "mobkit/mobpacks/deploy",
            "deploy_cli": "rkat mob deploy <pack.mobpack> <prompt>"
        },
        "deploy_settings": deploy_settings,
        "mob_definition": mob_definition,
        "validation_source": MOBPACK_VALIDATION_SOURCE
    })
}

fn tool_catalog_by_id(tool_catalog: &[Value]) -> BTreeMap<String, Value> {
    tool_catalog
        .iter()
        .filter_map(|tool| {
            let id = tool.get("id").and_then(Value::as_str)?.trim();
            if id.is_empty() {
                None
            } else {
                Some((id.to_string(), tool.clone()))
            }
        })
        .collect()
}

fn skill_catalog_by_id(skill_realms: &Value) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let Some(realms) = skill_realms.as_array() else {
        return out;
    };
    for realm in realms {
        let realm_id = realm.get("id").and_then(Value::as_str).unwrap_or_default();
        let realm_label = realm
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or(realm_id);
        let Some(skills) = realm.get("skills").and_then(Value::as_array) else {
            continue;
        };
        for skill in skills {
            let Some(id) = skill
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            out.entry(id.to_string()).or_insert_with(|| {
                let mut projected = skill.as_object().cloned().unwrap_or_default();
                projected
                    .entry("label".to_string())
                    .or_insert_with(|| Value::String(id.to_string()));
                projected.insert("realm".to_string(), Value::String(realm_id.to_string()));
                projected.insert(
                    "realmLabel".to_string(),
                    Value::String(realm_label.to_string()),
                );
                Value::Object(projected)
            });
        }
    }
    out
}

fn resolved_catalog_refs(ids: &[String], catalog: &BTreeMap<String, Value>) -> Value {
    Value::Array(
        ids.iter()
            .filter_map(|id| catalog.get(id).cloned())
            .collect::<Vec<_>>(),
    )
}

fn agent_definition_catalog(
    sample_mobpacks: &Value,
    tool_catalog: &[Value],
    skill_realms: &Value,
) -> Value {
    let mut templates = BTreeMap::<String, Value>::new();
    let tools_by_id = tool_catalog_by_id(tool_catalog);
    let skills_by_id = skill_catalog_by_id(skill_realms);
    if let Some(samples) = sample_mobpacks.as_array() {
        for sample in samples {
            let Some(source_mobpack) = sample
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let Some(sample_source) = sample
                .get("source")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let schema_by_id = sample
                .get("document")
                .and_then(|document| document.get("schemas"))
                .and_then(Value::as_array)
                .map(|schemas| {
                    schemas
                        .iter()
                        .filter_map(|schema| {
                            let id = schema.get("id").and_then(Value::as_str)?;
                            Some((id.to_string(), schema.clone()))
                        })
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default();
            let source_name = sample
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(source_mobpack);
            let Some(members) = sample
                .get("document")
                .and_then(|document| document.get("members"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for member in members {
                let role = member
                    .get("role")
                    .and_then(Value::as_str)
                    .or_else(|| member.get("name").and_then(Value::as_str))
                    .unwrap_or("member");
                let Some(profile_binding) = member
                    .get("profileBinding")
                    .or_else(|| member.get("profile_binding"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                let Some(runtime_mode) = member
                    .get("runtimeMode")
                    .or_else(|| member.get("runtime_mode"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                let Some(model) = member
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                let key = sanitize_identifier(role);
                templates.entry(key.clone()).or_insert_with(|| {
                    let name = member.get("name").and_then(Value::as_str).unwrap_or(role);
                    let schema_id = member
                        .get("schema")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let schema_definition =
                        schema_by_id.get(schema_id).cloned().unwrap_or(Value::Null);
                    let tools = string_vec(member.get("tools"));
                    let skills = string_vec(member.get("skills"));
                    json!({
                        "id": key,
                        "role": role,
                        "label": name,
                        "name": name,
                        "model": model,
                        "schema": schema_id,
                        "schemaSourceDocumentPath": if schema_definition.is_null() { "" } else { "document.schemas[]" },
                        "schemaDefinition": schema_definition,
                        "skills": skills.clone(),
                        "skillDefinitions": resolved_catalog_refs(&skills, &skills_by_id),
                        "tools": tools.clone(),
                        "toolDefinitions": resolved_catalog_refs(&tools, &tools_by_id),
                        "profileBinding": profile_binding,
                        "realmProfile": member.get("realmProfile").or_else(|| member.get("realm_profile")).and_then(Value::as_str).unwrap_or_default(),
                        "runtimeMode": runtime_mode,
                        "externalAddressable": member.get("externalAddressable").and_then(Value::as_bool).unwrap_or(false),
                        "backend": member.get("backend").and_then(Value::as_str).unwrap_or_default(),
                        "maxInlinePeerNotifications": member.get("maxInlinePeerNotifications").or_else(|| member.get("max_inline_peer_notifications")).cloned().unwrap_or(Value::Null),
                        "systemPrompt": member.get("systemPrompt").and_then(Value::as_str).unwrap_or_default(),
                        "providerParams": member.get("providerParams").or_else(|| member.get("provider_params")).cloned().unwrap_or(Value::Null),
                        "definitionType": "mobkit/profile-member",
                        "source": "mobkit/mobpack-profile-member",
                        "sourceMobpack": source_mobpack,
                        "sourceMobpackName": source_name,
                        "sourceOrigin": sample_source,
                        "sourceDocumentPath": "document.members[]",
                    })
                });
            }
        }
    }
    Value::Array(templates.into_values().collect())
}

fn sample_mobpack_catalog() -> Value {
    let samples = [
        (
            "sample_planner_coder_review_loop",
            "planner-coder-reviewer",
            "label · small-fix",
            sample_planner_coder_review_loop_toml(),
        ),
        (
            "sample_docs_only",
            "docs-only",
            "path · docs/**",
            sample_docs_only_toml(),
        ),
        (
            "sample_review_pr",
            "review-pr",
            "kind · pull_request",
            sample_review_pr_toml(),
        ),
    ];
    Value::Array(
        samples
            .into_iter()
            .filter_map(|(id, name, trigger, mob_toml)| {
                let definition = MobDefinition::from_toml(mob_toml).ok()?;
                let document = project_definition_to_editor_document(
                    &definition,
                    mob_toml,
                    Some(name),
                    Value::Null,
                )
                .ok()?;
                let validation = validate_document(&document);
                Some(json!({
                    "id": id,
                    "name": name,
                    "version": MOBPACK_SCHEMA_VERSION,
                    "stage": if validation.ok { "valid" } else { "draft" },
                    "trigger": trigger,
                    "source": "mobkit/sample-mobpack",
                    "document": document,
                    "validation": validation,
                }))
            })
            .collect(),
    )
}

fn blank_mobpack_template() -> Value {
    let mob_toml = blank_mobpack_template_toml();
    let Ok(definition) = MobDefinition::from_toml(mob_toml) else {
        return Value::Null;
    };
    let Ok(document) = project_definition_to_editor_document(
        &definition,
        mob_toml,
        Some("blank-mob"),
        Value::Null,
    ) else {
        return Value::Null;
    };
    let validation = validate_document(&document);
    json!({
        "id": "blank",
        "name": "Blank",
        "version": MOBPACK_SCHEMA_VERSION,
        "stage": if validation.ok { "valid" } else { "draft" },
        "trigger": "label · small-fix",
        "source": "mobkit/blank-mobpack",
        "document": document,
        "validation": validation,
    })
}

fn blank_mobpack_template_toml() -> &'static str {
    r#"
[mob]
id = "blank-mob"

[profiles.worker]
model = "gpt-5.5"
peer_description = "Carry out the requested task and report concise progress."
runtime_mode = "turn_driven"

[profiles.worker.tools]
builtins = true
comms = true

[flows.main]
description = "Minimal deployable MobKit flow."

[flows.main.steps.work]
role = "worker"
message = "Handle the requested task."

[flows.main.root.nodes.node_work]
kind = "step"
step_id = "work"
depends_on = []
depends_on_mode = "all"
"#
}

fn sample_planner_coder_review_loop_toml() -> &'static str {
    r#"
[mob]
id = "planner-coder-reviewer"

[profiles.planner]
model = "gpt-5.5"
skills = ["mob.workpad"]
peer_description = "Own the issue plan and Workpad. Convert reviewer, PR, and check feedback into a focused plan before implementation resumes."
runtime_mode = "turn_driven"

[profiles.planner.tools]
builtins = true
comms = true
mob = true
workgraph = true

[profiles.coder]
model = "gpt-5.5"
skills = ["mob.workpad", "mob.tests"]
peer_description = "Implement only the current Workpad plan, keep validation evidence current, and hand back to the reviewer when local checks are ready."
runtime_mode = "turn_driven"

[profiles.coder.tools]
builtins = true
shell = true
comms = true
mob = true
workgraph = true

[profiles.reviewer]
model = "gpt-5.5"
skills = ["mob.review"]
peer_description = "Gate the local implementation and PR/check evidence. Green review may ask the repo-level mob to create or merge the PR; red review reports detailed findings back to the planner."
runtime_mode = "turn_driven"

[profiles.reviewer.tools]
builtins = true
shell = true
comms = true
mob = true

[profiles.reviewer.output_schema]
type = "object"
description = "Review verdict that gates the repeat-until frame."
required = ["verdict", "findings"]
additionalProperties = false

[profiles.reviewer.output_schema.properties]

[profiles.reviewer.output_schema.properties.verdict]
type = "string"
description = "Pass/fail."
enum = ["green", "red"]

[profiles.reviewer.output_schema.properties.findings]
type = "array"
description = "What needs to change before green."

[profiles.reviewer.output_schema.properties.findings.items]
type = "string"

[skills."mob.workpad"]
source = "inline"
content = "Maintain the shared mob workpad, current plan, evidence, and handoff notes."

[skills."mob.tests"]
source = "inline"
content = "Design and run focused validation. Prefer narrow tests that prove the changed behavior."

[skills."mob.review"]
source = "inline"
content = "Perform structured review. Report blocking findings with file paths, evidence, and suggested fixes."

[flows.main]
description = "Plan, implement, and review until the reviewer emits a green verdict."

[flows.main.steps.plan]
role = "planner"
message = "Plan the work."

[flows.main.steps.implement]
role = "coder"
message = "Implement the current plan and collect validation evidence."
depends_on = ["plan"]

[flows.main.steps.review]
role = "reviewer"
message = "Review the implementation and emit a green or red verdict."
depends_on = ["implement"]
expected_schema_ref = "schemas/reviewer.json"

[flows.main.root.nodes.node_plan]
kind = "step"
step_id = "plan"
depends_on = []
depends_on_mode = "all"

[flows.main.root.nodes.node_loop]
kind = "repeat_until"
loop_id = "review_loop"
depends_on = ["node_plan"]
depends_on_mode = "all"
until = { op = "eq", path = "steps.review.verdict", value = "green" }
max_iterations = 3

[flows.main.root.nodes.node_loop.body.nodes.node_implement]
kind = "step"
step_id = "implement"
depends_on = []
depends_on_mode = "all"

[flows.main.root.nodes.node_loop.body.nodes.node_review]
kind = "step"
step_id = "review"
depends_on = ["node_implement"]
depends_on_mode = "all"
"#
}

fn sample_docs_only_toml() -> &'static str {
    r#"
[mob]
id = "docs-only"

[profiles.writer]
model = "gpt-5.5"
skills = ["mob.workpad"]
peer_description = "Draft documentation changes from the requested scope and preserve existing documentation conventions."
runtime_mode = "turn_driven"

[profiles.writer.tools]
builtins = true
comms = true

[profiles.reviewer]
model = "gpt-5.5"
skills = ["mob.review"]
peer_description = "Review documentation for accuracy, navigation fit, and missing validation."
runtime_mode = "turn_driven"

[profiles.reviewer.tools]
builtins = true
comms = true

[skills."mob.workpad"]
source = "inline"
content = "Maintain the shared mob workpad, current plan, evidence, and handoff notes."

[skills."mob.review"]
source = "inline"
content = "Perform structured review. Report blocking findings with file paths, evidence, and suggested fixes."

[flows.main]
description = "Draft and review documentation-only changes."

[flows.main.steps.write]
role = "writer"
message = "Write the documentation update."

[flows.main.steps.review]
role = "reviewer"
message = "Review the documentation update."
depends_on = ["write"]

[flows.main.root.nodes.node_write]
kind = "step"
step_id = "write"
depends_on = []
depends_on_mode = "all"

[flows.main.root.nodes.node_review]
kind = "step"
step_id = "review"
depends_on = ["node_write"]
depends_on_mode = "all"
"#
}

fn sample_review_pr_toml() -> &'static str {
    r#"
[mob]
id = "review-pr"

[profiles.router]
model = "gpt-5.5"
skills = ["mob.workpad"]
peer_description = "Classify the pull request and choose the review lane."
runtime_mode = "turn_driven"

[profiles.router.tools]
builtins = true
comms = true
mob = true

[profiles.reviewer]
model = "gpt-5.5"
skills = ["mob.review"]
peer_description = "Review the pull request according to the selected lane."
runtime_mode = "turn_driven"

[profiles.reviewer.tools]
builtins = true
shell = true
comms = true

[skills."mob.workpad"]
source = "inline"
content = "Maintain the shared mob workpad, current plan, evidence, and handoff notes."

[skills."mob.review"]
source = "inline"
content = "Perform structured review. Report blocking findings with file paths, evidence, and suggested fixes."

[flows.main]
description = "Route a pull request into focused review lanes."

[flows.main.steps.route]
role = "router"
message = "Classify this pull request."

[flows.main.steps.review_code]
role = "reviewer"
message = "Review code behavior and tests."
depends_on = ["route"]
branch = "review_lane"
condition = { op = "eq", path = "params.kind", value = "code" }

[flows.main.steps.review_docs]
role = "reviewer"
message = "Review documentation accuracy and navigation."
depends_on = ["route"]
branch = "review_lane"
condition = { op = "eq", path = "params.kind", value = "docs" }

[flows.main.root.nodes.node_route]
kind = "step"
step_id = "route"
depends_on = []
depends_on_mode = "all"

[flows.main.root.nodes.node_review_code]
kind = "step"
step_id = "review_code"
depends_on = ["node_route"]
depends_on_mode = "all"
branch = "review_lane"

[flows.main.root.nodes.node_review_docs]
kind = "step"
step_id = "review_docs"
depends_on = ["node_route"]
depends_on_mode = "all"
branch = "review_lane"
"#
}

pub fn import_mobpack(params: &Value) -> Result<Value, String> {
    let mut document = document_from_params(params)?;
    if document.mob_toml.as_deref().unwrap_or("").trim().is_empty() {
        if let Some(text) = params.get("mob_toml").and_then(Value::as_str) {
            document.mob_toml = Some(text.to_string());
        }
    }
    if needs_editor_projection(&document)
        && let Some(mob_toml) = document
            .mob_toml
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
    {
        let definition = MobDefinition::from_toml(mob_toml)
            .map_err(|err| format!("failed to parse mob.toml for editor import: {err}"))?;
        document = project_definition_to_editor_document(
            &definition,
            mob_toml,
            (!document.name.trim().is_empty()).then_some(document.name.as_str()),
            document.deploy.clone(),
        )?;
    }
    let validation = validate_document(&document);
    if let Some(diagnostic) = validation
        .diagnostics
        .iter()
        .find(|diagnostic| is_starter_provenance_diagnostic(&diagnostic.code))
    {
        return Err(format!(
            "cannot import prototype starter skill data: {} at {}",
            diagnostic.code,
            diagnostic.path.as_deref().unwrap_or("skill_realms")
        ));
    }
    let source = import_source_from_params(params);
    Ok(json!({
        "document": document,
        "validation": validation,
        "source": source.source,
        "source_label": source.label,
        "source_media_type": source.media_type,
    }))
}

struct MobpackImportSource {
    source: &'static str,
    label: String,
    media_type: String,
}

fn import_source_from_params(params: &Value) -> MobpackImportSource {
    let label = params
        .get("source_name")
        .or_else(|| params.get("filename"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if params
        .get("content_base64")
        .and_then(Value::as_str)
        .is_some()
    {
        return MobpackImportSource {
            source: "mobkit/mobpacks/import:archive",
            label: label.unwrap_or("mobpack archive").to_string(),
            media_type: import_media_type(params, "application/vnd.meerkat.mobpack"),
        };
    }
    if params.get("mob_toml").and_then(Value::as_str).is_some() {
        return MobpackImportSource {
            source: "mobkit/mobpacks/import:mob.toml",
            label: label.unwrap_or("mob.toml").to_string(),
            media_type: import_media_type(params, "text/x-toml"),
        };
    }
    if params.get("document").is_some() {
        return MobpackImportSource {
            source: "mobkit/mobpacks/import:editor-document",
            label: label.unwrap_or("mobkit/editor.json").to_string(),
            media_type: import_media_type(params, "application/json"),
        };
    }
    MobpackImportSource {
        source: "mobkit/mobpacks/import:document",
        label: label.unwrap_or("mobpack document").to_string(),
        media_type: import_media_type(params, "application/json"),
    }
}

fn import_media_type(params: &Value, fallback: &str) -> String {
    params
        .get("source_media_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

pub fn validate_mobpack(params: &Value) -> Result<MobpackValidationResult, String> {
    let document = document_from_params(params)?;
    Ok(validate_document(&document))
}

pub fn deploy_command_preview(params: &Value) -> Result<MobpackDeployCommandResult, String> {
    let document = params
        .get("document")
        .map(document_from_value)
        .transpose()?;
    let deploy = params
        .get("deploy")
        .cloned()
        .or_else(|| document.as_ref().map(|document| document.deploy.clone()))
        .unwrap_or(Value::Null);
    let prompt = params
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            deploy
                .get("prompt")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("Reply with exactly OK.");
    let pack_path = params
        .get("pack_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| document.as_ref().map(document_export_filename))
        .unwrap_or_else(|| "<pack.mobpack>".to_string());
    let rkat_bin = deploy_rkat_bin(params);
    let argv = deploy_argv(&rkat_bin, &deploy, Path::new(&pack_path), prompt);
    let command = shell_command(&argv);
    Ok(MobpackDeployCommandResult {
        command,
        argv,
        deploy_command: "rkat mob deploy".to_string(),
        source: "meerkat_mobkit::mobpack::deploy_argv".to_string(),
    })
}

pub fn export_mobpack(params: &Value) -> Result<MobpackExportResult, String> {
    let document = document_from_params(params)?;
    let validation = validate_document(&document);
    if !validation.ok {
        return Err("cannot export invalid mobpack document".to_string());
    }
    let mob_toml = authoring_mob_toml(&document)?;
    let filename = document_export_filename(&document);
    let slug = filename
        .strip_suffix(".mobpack")
        .unwrap_or(filename.as_str())
        .to_string();
    let files = deployable_mobpack_archive_files(&slug, &document, &mob_toml)?;
    let bytes = encode_deployable_mobpack_archive(&files)?;
    Ok(MobpackExportResult {
        filename,
        media_type: MOBPACK_MEDIA_TYPE.to_string(),
        content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        mob_toml,
        source_files: source_files_from_archive_files(&files),
        validation,
    })
}

pub fn deploy_mobpack(params: &Value) -> Result<MobpackDeployResult, String> {
    let export = export_mobpack(params)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(export.content_base64.as_bytes())
        .map_err(|err| format!("failed to decode exported mobpack: {err}"))?;
    let pack_sha256 = source_file_sha256(&bytes);
    let pack_path = deploy_pack_path(params, &export.filename)?;
    if let Some(parent) = pack_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create deploy output directory: {err}"))?;
    }
    std::fs::write(&pack_path, bytes)
        .map_err(|err| format!("failed to write deploy mobpack: {err}"))?;

    let document = document_from_params(params)?;
    let deploy = document.deploy.clone();
    let prompt = params
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            deploy
                .get("prompt")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("Reply with exactly OK.");
    let rkat_bin = deploy_rkat_bin(params);
    let argv = deploy_argv(&rkat_bin, &deploy, &pack_path, prompt);
    let command = shell_command(&argv);
    let definition = MobDefinition::from_toml(&export.mob_toml)
        .map_err(|err| format!("failed to parse exported mob.toml for deploy plan: {err}"))?;
    let plan_trace =
        deploy_plan_trace_from_definition(&definition, &export.validation, &command, &pack_path);
    let execute = params
        .get("execute")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let (status_code, stdout, stderr) = if execute {
        let timeout = deploy_execution_timeout(&deploy);
        let output = run_deploy_command(&argv, timeout);
        (output.status_code, output.stdout, output.stderr)
    } else {
        (None, None, None)
    };
    let success = !execute || status_code == Some(0);
    let display_rows = deploy_display_rows(
        &export.validation,
        execute,
        success,
        status_code,
        stdout.as_deref(),
        stderr.as_deref(),
        &pack_path,
        &command,
    );

    Ok(MobpackDeployResult {
        filename: export.filename,
        pack_path: pack_path.to_string_lossy().to_string(),
        pack_sha256,
        command,
        argv,
        plan_trace,
        executed: execute,
        success,
        status_code,
        stdout,
        stderr,
        display_rows,
        validation: export.validation,
    })
}

fn deploy_display_rows(
    validation: &MobpackValidationResult,
    executed: bool,
    success: bool,
    status_code: Option<i32>,
    stdout: Option<&str>,
    stderr: Option<&str>,
    pack_path: &Path,
    command: &str,
) -> Vec<MobpackDisplayRow> {
    let mut rows = validation.display_rows.clone();
    rows.push(MobpackDisplayRow {
        kind: if executed {
            if success { "ok" } else { "crit" }
        } else {
            "warn"
        }
        .to_string(),
        glyph: if executed {
            if success { "✓" } else { "!" }
        } else {
            "△"
        }
        .to_string(),
        head: if executed {
            if success {
                "rkat mob deploy executed"
            } else {
                "rkat mob deploy failed"
            }
        } else {
            "Deploy plan ready"
        }
        .to_string(),
        sub: command.to_string(),
        meta: pack_path.to_string_lossy().to_string(),
    });
    if executed {
        let output = [stdout.unwrap_or_default(), stderr.unwrap_or_default()]
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if let Some(status_code) = status_code {
            rows.push(MobpackDisplayRow {
                kind: if status_code == 0 { "ok" } else { "crit" }.to_string(),
                glyph: if status_code == 0 { "✓" } else { "!" }.to_string(),
                head: format!("rkat exit {status_code}"),
                sub: output,
                meta: "rkat mob deploy".to_string(),
            });
        } else if !output.is_empty() {
            rows.push(MobpackDisplayRow {
                kind: "warn".to_string(),
                glyph: "△".to_string(),
                head: "rkat output".to_string(),
                sub: output,
                meta: "rkat mob deploy".to_string(),
            });
        }
    }
    rows
}

fn deploy_pack_path(params: &Value, filename: &str) -> Result<PathBuf, String> {
    if let Some(path) = params.get("pack_path").and_then(Value::as_str) {
        let path = path.trim();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let output_dir = params
        .get("output_dir")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| format!("system clock before unix epoch: {err}"))?
        .as_millis();
    Ok(output_dir.join(format!("{stamp}-{filename}")))
}

#[cfg(test)]
fn deploy_rkat_bin(params: &Value) -> String {
    params
        .get("rkat_bin")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(env_rkat_bin)
        .unwrap_or_else(|| "rkat".to_string())
}

#[cfg(not(test))]
fn deploy_rkat_bin(_params: &Value) -> String {
    env_rkat_bin().unwrap_or_else(|| "rkat".to_string())
}

fn env_rkat_bin() -> Option<String> {
    std::env::var("RKAT_BIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn deploy_argv(rkat_bin: &str, deploy: &Value, pack_path: &Path, prompt: &str) -> Vec<String> {
    let mut argv = vec![
        rkat_bin.to_string(),
        "mob".to_string(),
        "deploy".to_string(),
    ];
    if let Some(model) = deploy_string(deploy, "model") {
        argv.extend(["--model".to_string(), model]);
    }
    if let Some(tokens) = deploy_number(deploy, "max_total_tokens") {
        argv.extend(["--max-total-tokens".to_string(), tokens]);
    }
    if let Some(duration) = deploy_string(deploy, "max_duration") {
        argv.extend(["--max-duration".to_string(), duration]);
    }
    if let Some(tool_calls) = deploy_number(deploy, "max_tool_calls") {
        argv.extend(["--max-tool-calls".to_string(), tool_calls]);
    }
    if let Some(trust) = deploy_string(deploy, "trust_policy") {
        argv.extend(["--trust-policy".to_string(), trust]);
    }
    if let Some(surface) = deploy_string(deploy, "surface") {
        argv.extend(["--surface".to_string(), surface]);
    }
    if let Some(realm) = deploy_string(deploy, "realm") {
        argv.extend(["--realm".to_string(), realm]);
    } else if deploy
        .get("isolated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        argv.push("--isolated".to_string());
    }
    if let Some(instance) = deploy_string(deploy, "instance") {
        argv.extend(["--instance".to_string(), instance]);
    }
    if let Some(backend) = deploy_string(deploy, "realm_backend") {
        argv.extend(["--realm-backend".to_string(), backend]);
    }
    if let Some(context_root) = deploy_string(deploy, "context_root") {
        argv.extend(["--context-root".to_string(), context_root]);
    }
    if let Some(state_root) = deploy_string(deploy, "state_root") {
        argv.extend(["--state-root".to_string(), state_root]);
    }
    if let Some(user_config_root) = deploy_string(deploy, "user_config_root") {
        argv.extend(["--user-config-root".to_string(), user_config_root]);
    }
    argv.push(pack_path.to_string_lossy().to_string());
    argv.push(prompt.to_string());
    argv
}

struct DeployProcessOutput {
    status_code: Option<i32>,
    stdout: Option<String>,
    stderr: Option<String>,
}

fn run_deploy_command(argv: &[String], timeout: std::time::Duration) -> DeployProcessOutput {
    if argv.is_empty() {
        return DeployProcessOutput {
            status_code: None,
            stdout: None,
            stderr: Some("failed to run rkat mob deploy: empty argv".to_string()),
        };
    }
    let mut child = match std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            return DeployProcessOutput {
                status_code: None,
                stdout: None,
                stderr: Some(format!("failed to run rkat mob deploy: {err}")),
            };
        }
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return match child.wait_with_output() {
                    Ok(output) => DeployProcessOutput {
                        status_code: output.status.code(),
                        stdout: Some(String::from_utf8_lossy(&output.stdout).to_string()),
                        stderr: Some(String::from_utf8_lossy(&output.stderr).to_string()),
                    },
                    Err(err) => DeployProcessOutput {
                        status_code: None,
                        stdout: None,
                        stderr: Some(format!("failed to collect rkat mob deploy output: {err}")),
                    },
                };
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                return match child.wait_with_output() {
                    Ok(output) => {
                        let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
                        if !stderr.trim().is_empty() {
                            stderr.push('\n');
                        }
                        stderr.push_str(&format!(
                            "rkat mob deploy timed out after {}ms",
                            timeout.as_millis()
                        ));
                        DeployProcessOutput {
                            status_code: output.status.code(),
                            stdout: Some(String::from_utf8_lossy(&output.stdout).to_string()),
                            stderr: Some(stderr),
                        }
                    }
                    Err(err) => DeployProcessOutput {
                        status_code: None,
                        stdout: None,
                        stderr: Some(format!(
                            "rkat mob deploy timed out after {}ms; failed to collect output: {err}",
                            timeout.as_millis()
                        )),
                    },
                };
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(err) => {
                let _ = child.kill();
                return DeployProcessOutput {
                    status_code: None,
                    stdout: None,
                    stderr: Some(format!("failed to wait for rkat mob deploy: {err}")),
                };
            }
        }
    }
}

fn deploy_execution_timeout(deploy: &Value) -> std::time::Duration {
    let millis = deploy_string(deploy, "max_duration")
        .and_then(|value| parse_deploy_duration_ms(&value))
        .map(|millis| millis.saturating_add(DEPLOY_EXEC_TIMEOUT_GRACE_MS))
        .filter(|millis| *millis > 0)
        .unwrap_or(DEFAULT_DEPLOY_EXEC_TIMEOUT_MS);
    std::time::Duration::from_millis(millis)
}

fn parse_deploy_duration_ms(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(number) = lower.strip_suffix("ms") {
        return number.trim().parse::<u64>().ok();
    }
    if let Some(number) = lower.strip_suffix('s') {
        return number
            .trim()
            .parse::<u64>()
            .ok()
            .map(|seconds| seconds.saturating_mul(1_000));
    }
    if let Some(number) = lower.strip_suffix('m') {
        return number
            .trim()
            .parse::<u64>()
            .ok()
            .map(|minutes| minutes.saturating_mul(60_000));
    }
    if let Some(number) = lower.strip_suffix('h') {
        return number
            .trim()
            .parse::<u64>()
            .ok()
            .map(|hours| hours.saturating_mul(3_600_000));
    }
    lower
        .parse::<u64>()
        .ok()
        .map(|seconds| seconds.saturating_mul(1_000))
}

fn deploy_string(deploy: &Value, key: &str) -> Option<String> {
    deploy
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn deploy_number(deploy: &Value, key: &str) -> Option<String> {
    deploy
        .get(key)
        .and_then(|value| {
            value
                .as_i64()
                .map(|number| number.to_string())
                .or_else(|| value.as_u64().map(|number| number.to_string()))
        })
        .filter(|value| value != "0")
}

fn shell_command(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(arg: &str) -> String {
    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_./:=@%+-".contains(ch))
    {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

fn deploy_plan_trace_from_definition(
    definition: &MobDefinition,
    validation: &MobpackValidationResult,
    command: &str,
    pack_path: &Path,
) -> Vec<Value> {
    let mut rows = Vec::new();
    rows.push(plan_trace_row(
        Value::Null,
        format!("MOBPACK · {}", definition.id),
        compact_trace_lines([
            Some("source: mobkit/mob.toml".to_string()),
            Some(format!("command: {command}")),
            Some(format!("pack_path: {}", pack_path.to_string_lossy())),
            Some(format!("validation: {}", validation.validation_source)),
            Some(format!(
                "profiles: {} · flows: {}",
                definition.profiles.len(),
                definition.flows.len()
            )),
        ]),
    ));

    for (profile_name, binding) in &definition.profiles {
        rows.push(profile_plan_trace_row(profile_name.as_ref(), binding));
    }

    for (flow_id, flow) in &definition.flows {
        rows.push(plan_trace_row(
            Value::Null,
            format!("FLOW · {flow_id}"),
            compact_trace_lines([
                flow.description
                    .as_ref()
                    .filter(|description| !description.trim().is_empty())
                    .map(|description| format!("description: {description}")),
                Some(format!("steps: {}", flow.steps.len())),
                Some(format!(
                    "root: {}",
                    if flow.root.is_some() {
                        "FrameSpec"
                    } else {
                        "flat depends_on"
                    }
                )),
            ]),
        ));
        if let Some(root) = &flow.root {
            push_frame_plan_trace(&mut rows, flow, root, "root");
        } else {
            for step_id in topological_step_order(flow) {
                if let Some(step) = flow.steps.get(&step_id) {
                    rows.push(step_plan_trace_row(
                        Value::String(step_id.to_string()),
                        &step_id.to_string(),
                        step,
                    ));
                }
            }
        }
    }

    rows.push(plan_trace_row(
        Value::Null,
        if validation.ok {
            "VALIDATION · ACCEPTED".to_string()
        } else {
            "VALIDATION · BLOCKED".to_string()
        },
        compact_trace_lines([
            Some(validation.validation_source.clone()),
            Some(validation.deploy_command.clone()),
            Some(format!("diagnostics: {}", validation.diagnostics.len())),
        ]),
    ));
    rows
}

fn profile_plan_trace_row(profile_name: &str, binding: &ProfileBinding) -> Value {
    match binding {
        ProfileBinding::RealmRef { realm_profile } => plan_trace_row(
            Value::Null,
            format!("PROFILE · {profile_name}"),
            compact_trace_lines([
                Some("binding: realm_profile".to_string()),
                Some(format!("realm_profile: {realm_profile}")),
            ]),
        ),
        ProfileBinding::Inline(profile) => {
            let tools = tool_ids_from_config(&profile.tools);
            plan_trace_row(
                Value::Null,
                format!("PROFILE · {profile_name}"),
                compact_trace_lines([
                    Some("binding: inline".to_string()),
                    Some(format!("model: {}", profile.model)),
                    Some(format!("runtime_mode: {}", profile.runtime_mode)),
                    profile
                        .backend
                        .as_ref()
                        .map(|backend| format!("backend: {}", backend_kind_string(backend))),
                    Some(format!(
                        "tools: {}",
                        if tools.is_empty() {
                            "none".to_string()
                        } else {
                            tools.join(", ")
                        }
                    )),
                    Some(format!(
                        "skills: {}",
                        if profile.skills.is_empty() {
                            "none".to_string()
                        } else {
                            profile.skills.join(", ")
                        }
                    )),
                    profile
                        .output_schema
                        .as_ref()
                        .map(|_| "output_schema: configured".to_string()),
                    profile
                        .provider_params
                        .as_ref()
                        .map(|_| "provider_params: configured".to_string()),
                ]),
            )
        }
    }
}

fn push_frame_plan_trace(rows: &mut Vec<Value>, flow: &FlowSpec, frame: &FrameSpec, scope: &str) {
    for (node_id, node) in &frame.nodes {
        match node {
            FlowNodeSpec::Step(frame_step) => {
                if let Some(step) = flow.steps.get(&frame_step.step_id) {
                    rows.push(step_plan_trace_row(
                        Value::String(node_id.to_string()),
                        &frame_step.step_id.to_string(),
                        step,
                    ));
                } else {
                    rows.push(plan_trace_row(
                        Value::String(node_id.to_string()),
                        format!("STEP · {}", frame_step.step_id),
                        compact_trace_lines([
                            Some(format!("frame: {scope}")),
                            Some(
                                "definition error: step_id is missing from flows.*.steps"
                                    .to_string(),
                            ),
                        ]),
                    ));
                }
            }
            FlowNodeSpec::RepeatUntil(loop_spec) => {
                rows.push(plan_trace_row(
                    Value::String(node_id.to_string()),
                    format!("LOOP · {}", loop_spec.loop_id),
                    compact_trace_lines([
                        Some(format!("frame: {scope}")),
                        Some(format!(
                            "depends_on: {}",
                            list_or_none(&loop_spec.depends_on)
                        )),
                        Some(format!(
                            "depends_on_mode: {}",
                            dependency_mode_string(&loop_spec.depends_on_mode)
                        )),
                        Some(format!("until: {}", condition_to_label(&loop_spec.until))),
                        Some(format!("max_iterations: {}", loop_spec.max_iterations)),
                    ]),
                ));
                push_frame_plan_trace(
                    rows,
                    flow,
                    &loop_spec.body,
                    &format!("{scope}.{}", loop_spec.loop_id),
                );
            }
        }
    }
}

fn step_plan_trace_row(node: Value, step_id: &str, step: &FlowStepSpec) -> Value {
    plan_trace_row(
        node,
        format!("STEP · {step_id} · {}", step.role),
        compact_trace_lines([
            Some(format!("role: {}", step.role)),
            Some(format!("message: {}", content_input_summary(&step.message))),
            Some(format!("depends_on: {}", list_or_none(&step.depends_on))),
            Some(format!(
                "depends_on_mode: {}",
                dependency_mode_string(&step.depends_on_mode)
            )),
            step.condition
                .as_ref()
                .map(|condition| format!("condition: {}", condition_to_label(condition))),
            step.branch
                .as_ref()
                .map(|branch| format!("branch: {branch}")),
            Some(format!(
                "dispatch_mode: {}",
                dispatch_mode_string(&step.dispatch_mode)
            )),
            Some(format!(
                "collection_policy: {}",
                collection_policy_label(&step.collection_policy)
            )),
            step.timeout_ms
                .map(|timeout| format!("timeout_ms: {timeout}")),
            step.expected_schema_ref
                .as_ref()
                .map(|schema| format!("expected_schema_ref: {schema}")),
            step.allowed_tools
                .as_ref()
                .filter(|tools| !tools.is_empty())
                .map(|tools| format!("allowed_tools: {}", tools.join(", "))),
            step.blocked_tools
                .as_ref()
                .filter(|tools| !tools.is_empty())
                .map(|tools| format!("blocked_tools: {}", tools.join(", "))),
            Some(format!(
                "output_format: {}",
                step_output_format_string(&step.output_format)
            )),
        ]),
    )
}

fn plan_trace_row(node: Value, head: String, body: String) -> Value {
    json!({
        "node": node,
        "head": head,
        "body": body,
    })
}

fn compact_trace_lines<const N: usize>(lines: [Option<String>; N]) -> String {
    lines
        .into_iter()
        .flatten()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn content_input_summary(input: &meerkat_core::types::ContentInput) -> String {
    match serde_json::to_value(input) {
        Ok(Value::String(text)) => text,
        Ok(value) => value.to_string(),
        Err(_) => "<content>".to_string(),
    }
}

fn list_or_none<T: ToString>(items: &[T]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn collection_policy_label(policy: &CollectionPolicy) -> String {
    match policy {
        CollectionPolicy::All => "all".to_string(),
        CollectionPolicy::Any => "any".to_string(),
        CollectionPolicy::Quorum { n } => format!("quorum ({n})"),
    }
}

fn backend_kind_string(backend: &MobBackendKind) -> &'static str {
    match backend {
        MobBackendKind::Session => "session",
        MobBackendKind::External => "external",
    }
}

fn deployable_mobpack_archive_files(
    slug: &str,
    document: &MobpackDocument,
    mob_toml: &str,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let definition = meerkat_mob::MobDefinition::from_toml(mob_toml)
        .map_err(|err| format!("failed to parse mob.toml for archive export: {err}"))?;
    let expected_schema_files =
        expected_schema_files_from_definition(&definition).map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("; ")
        })?;
    let manifest = format!(
        r#"surfaces = ["cli", "rpc"]

[mobpack]
name = "{}"
version = "{}"
description = "{}"
"#,
        escape_toml_string(slug),
        MOBPACK_SCHEMA_VERSION,
        escape_toml_string(
            document
                .name
                .trim()
                .is_empty()
                .then_some("MobKit Flow Editor mobpack")
                .unwrap_or(document.name.trim())
        )
    );
    let definition_json = serde_json::to_vec_pretty(&definition)
        .map_err(|err| format!("failed to encode definition.json: {err}"))?;
    let editor_json = serde_json::to_vec_pretty(&json!({
        "schema_version": MOBPACK_SCHEMA_VERSION,
        "media_type": MOBPACK_MEDIA_TYPE,
        "document": document,
    }))
    .map_err(|err| format!("failed to encode mobkit/editor.json: {err}"))?;

    let mut files = BTreeMap::<String, Vec<u8>>::new();
    files.insert("manifest.toml".to_string(), manifest.into_bytes());
    files.insert("definition.json".to_string(), definition_json);
    files.insert("mobkit/editor.json".to_string(), editor_json);
    files.insert("mobkit/mob.toml".to_string(), mob_toml.as_bytes().to_vec());
    for (path, bytes) in selected_path_skill_files(document)? {
        files.insert(path, bytes);
    }
    for (path, schema) in expected_schema_files {
        let schema_json = serde_json::to_vec_pretty(&schema)
            .map_err(|err| format!("failed to encode {path}: {err}"))?;
        files.insert(path, schema_json);
    }
    for (path, schema) in editor_input_schema_files(document) {
        let schema_json = serde_json::to_vec_pretty(&schema)
            .map_err(|err| format!("failed to encode {path}: {err}"))?;
        files.insert(path, schema_json);
    }
    Ok(files)
}

fn encode_deployable_mobpack_archive(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = Builder::new(encoder);
    for (path, bytes) in files {
        append_archive_file(&mut archive, path, bytes)?;
    }
    let encoder = archive
        .into_inner()
        .map_err(|err| format!("failed to finish mobpack archive: {err}"))?;
    encoder
        .finish()
        .map_err(|err| format!("failed to finish mobpack gzip stream: {err}"))
}

fn source_files_from_archive_files(files: &BTreeMap<String, Vec<u8>>) -> Vec<MobpackSourceFile> {
    files
        .iter()
        .map(|(path, bytes)| MobpackSourceFile {
            path: path.clone(),
            media_type: source_file_media_type(path).to_string(),
            size_bytes: bytes.len() as u64,
            content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            sha256: source_file_sha256(bytes),
            text: String::from_utf8(bytes.clone()).ok(),
        })
        .collect()
}

fn source_file_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn source_file_media_type(path: &str) -> &'static str {
    if path.ends_with(".toml") {
        "text/toml"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".md") {
        "text/markdown"
    } else {
        "application/octet-stream"
    }
}

fn append_archive_file<W: Write>(
    archive: &mut Builder<W>,
    path: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let mut header = Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(&mut header, path, bytes)
        .map_err(|err| format!("failed to append {path}: {err}"))
}

fn selected_path_skill_files(
    document: &MobpackDocument,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    for (skill_id, source) in selected_skill_sources(document) {
        let source_kind = source
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("inline");
        if source_kind != "path" {
            continue;
        }
        let archive_path = packed_skill_archive_path(&skill_id, &source);
        let bytes = if let Some(content) = source.get("content").and_then(Value::as_str) {
            content.as_bytes().to_vec()
        } else {
            let path = source
                .get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    format!("selected path skill '{skill_id}' has no filesystem path")
                })?;
            std::fs::read(path).map_err(|err| {
                format!("failed to read selected path skill '{skill_id}' at {path}: {err}")
            })?
        };
        if let Some(existing) = files.get(&archive_path) {
            if existing != &bytes {
                return Err(format!(
                    "multiple selected skills resolve to archive path '{archive_path}' with different content"
                ));
            }
        } else {
            files.insert(archive_path, bytes);
        }
    }
    Ok(files)
}

fn selected_skill_sources(document: &MobpackDocument) -> BTreeMap<String, Value> {
    let mut selected = BTreeMap::<String, Value>::new();
    if let Some(members) = document.members.as_array() {
        for member in members {
            if member
                .get("missing")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            for skill in string_vec(member.get("skills")) {
                if selected.contains_key(&skill) {
                    continue;
                }
                if let Some(source) = skill_source_from_realms(&skill, &document.skill_realms) {
                    selected.insert(skill, source);
                }
            }
        }
    }
    selected
}

fn packed_skill_archive_path(skill_id: &str, source: &Value) -> String {
    if let Some(path) = source
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| is_safe_relative_archive_path(path))
    {
        return path.to_string();
    }
    let slug = sanitize_slug(skill_id).unwrap_or_else(|| "skill".to_string());
    format!("skills/{slug}.md")
}

fn is_safe_relative_archive_path(path: &str) -> bool {
    let path = path.trim();
    if path.is_empty() || path.contains('\\') || Path::new(path).is_absolute() {
        return false;
    }
    Path::new(path)
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn hydrate_path_skill_content_from_archive(
    document: &mut MobpackDocument,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    let Some(realms) = document.skill_realms.as_array_mut() else {
        return Ok(());
    };
    for realm in realms {
        let Some(skills) = realm.get_mut("skills").and_then(Value::as_array_mut) else {
            continue;
        };
        for skill in skills {
            if skill.get("source").and_then(Value::as_str) != Some("path")
                || skill.get("content").and_then(Value::as_str).is_some()
            {
                continue;
            }
            let Some(path) = skill
                .get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let Some(bytes) = files.get(path).or_else(|| {
                let skill_id = skill.get("id").and_then(Value::as_str).unwrap_or("skill");
                let archive_path = packed_skill_archive_path(skill_id, skill);
                files.get(&archive_path)
            }) else {
                continue;
            };
            let content = String::from_utf8(bytes.clone()).map_err(|err| {
                format!("packed skill archive member {path} is not valid UTF-8: {err}")
            })?;
            skill["content"] = json!(content);
        }
    }
    Ok(())
}

fn document_from_params(params: &Value) -> Result<MobpackDocument, String> {
    if let Some(encoded) = params.get("content_base64").and_then(Value::as_str) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|err| format!("invalid content_base64: {err}"))?;
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            return document_from_value(&value);
        }
        return document_from_archive_bytes(&bytes);
    }
    if let Some(document) = params.get("document") {
        return document_from_value(document);
    }
    document_from_value(params)
}

fn document_from_archive_bytes(bytes: &[u8]) -> Result<MobpackDocument, String> {
    let decoder = GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    let mut editor_json = None;
    let mut mob_toml = None;
    let mut manifest_toml = None;
    for entry in archive
        .entries()
        .map_err(|err| format!("invalid mobpack archive: {err}"))?
    {
        let mut entry = entry.map_err(|err| format!("invalid mobpack archive entry: {err}"))?;
        let path = entry
            .path()
            .map_err(|err| format!("invalid mobpack archive path: {err}"))?
            .to_string_lossy()
            .to_string();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|err| format!("failed to read archive member {path}: {err}"))?;
        if matches!(
            path.as_str(),
            "mobkit/editor.json" | "mobkit/mob.toml" | "manifest.toml"
        ) {
            let text = String::from_utf8(bytes.clone())
                .map_err(|err| format!("invalid UTF-8 archive member {path}: {err}"))?;
            match path.as_str() {
                "mobkit/editor.json" => editor_json = Some(text),
                "mobkit/mob.toml" => mob_toml = Some(text),
                "manifest.toml" => manifest_toml = Some(text),
                _ => {}
            }
        }
        files.insert(path, bytes);
    }
    if let Some(text) = editor_json {
        let value: Value = serde_json::from_str(&text)
            .map_err(|err| format!("invalid mobkit/editor.json: {err}"))?;
        let mut document = document_from_value(&value)?;
        if let Some(mob_toml) = mob_toml.as_ref() {
            document.mob_toml = Some(mob_toml.clone());
        }
        hydrate_path_skill_content_from_archive(&mut document, &files)?;
        if let Some(mob_toml) = mob_toml.as_ref()
            && archive_editor_projection_should_reproject(&document, mob_toml)
        {
            let definition = MobDefinition::from_toml(mob_toml)
                .map_err(|err| format!("failed to parse mob.toml from mobpack archive: {err}"))?;
            let fallback_name = document.name.trim().to_string();
            let deploy = document.deploy.clone();
            let mut projected = project_definition_to_editor_document(
                &definition,
                mob_toml,
                (!fallback_name.is_empty()).then_some(fallback_name.as_str()),
                deploy,
            )?;
            hydrate_path_skill_content_from_archive(&mut projected, &files)?;
            return Ok(projected);
        }
        return Ok(document);
    }
    let name = manifest_toml
        .as_deref()
        .and_then(extract_manifest_name)
        .unwrap_or_else(|| "mobpack".to_string());

    if let Some(mob_toml) = mob_toml {
        let definition = MobDefinition::from_toml(&mob_toml)
            .map_err(|err| format!("failed to parse mob.toml from mobpack archive: {err}"))?;
        let mut document = project_definition_to_editor_document(
            &definition,
            &mob_toml,
            Some(&name),
            Value::Null,
        )?;
        hydrate_path_skill_content_from_archive(&mut document, &files)?;
        return Ok(document);
    }

    Ok(MobpackDocument {
        schema_version: MOBPACK_SCHEMA_VERSION.to_string(),
        mob_id: sanitize_slug(&name).unwrap_or_else(|| "mobpack".to_string()),
        name,
        mob_settings: Value::Null,
        members: Value::Null,
        instances: Value::Null,
        edges: Value::Null,
        frames: Value::Null,
        schemas: Value::Null,
        skill_realms: Value::Null,
        flow: Value::Null,
        launch_modes: Value::Null,
        deploy: Value::Null,
        mob_toml: None,
    })
}

fn document_from_value(value: &Value) -> Result<MobpackDocument, String> {
    if let Some(document) = value.get("document") {
        return document_from_value(document);
    }
    let mut document: MobpackDocument = serde_json::from_value(value.clone())
        .map_err(|err| format!("invalid mobpack document: {err}"))?;
    if document.mob_toml.is_none()
        && let Some(files) = value.get("files").and_then(Value::as_object)
        && let Some(text) = files.get("mob.toml").and_then(Value::as_str)
    {
        document.mob_toml = Some(text.to_string());
    }
    Ok(document)
}

fn archive_editor_projection_should_reproject(
    document: &MobpackDocument,
    packed_mob_toml: &str,
) -> bool {
    match render_editor_document_mob_toml(document) {
        Ok(rendered) if rendered.trim() != packed_mob_toml.trim() => return true,
        Err(_) => return true,
        _ => {}
    }
    let validation = validate_document(document);
    validation
        .diagnostics
        .iter()
        .any(|diagnostic| is_archive_editor_projection_drift_code(&diagnostic.code))
}

fn is_archive_editor_projection_drift_code(code: &str) -> bool {
    code.starts_with("editor_profile_")
        || code.starts_with("editor_flow_")
        || code.starts_with("graph_")
        || code.starts_with("stale_editor_")
        || matches!(
            code,
            "flow_step_missing_graph_instance"
                | "graph_instance_missing_from_flow"
                | "missing_compiled_graph_gate"
                | "missing_compiled_graph_frame"
                | "missing_graph_member"
                | "unknown_graph_member"
                | "unknown_graph_edge_endpoint"
                | "uncompiled_graph_gate"
                | "uncompiled_graph_terminal"
                | "uncompiled_graph_frame"
        )
}

fn needs_editor_projection(document: &MobpackDocument) -> bool {
    editor_value_missing(&document.members)
        || editor_value_missing(&document.flow)
        || editor_value_missing(&document.instances)
        || !document.edges.is_array()
}

fn authoring_mob_toml(document: &MobpackDocument) -> Result<String, String> {
    if !needs_editor_projection(document) {
        return render_editor_document_mob_toml(document);
    }
    document
        .mob_toml
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| "mobpack document has no editor projection or mob.toml source".to_string())
}

fn editor_value_missing(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn profile_backend_to_value(backend: Option<MobBackendKind>) -> Value {
    backend
        .map(|kind| json!(kind.as_str()))
        .unwrap_or(Value::Null)
}

fn mob_settings_from_definition(definition: &MobDefinition) -> Value {
    json!({
        "orchestrator": definition.orchestrator.as_ref().map(|config| config.profile.to_string()).unwrap_or_default(),
        "autoWireOrchestrator": definition.wiring.auto_wire_orchestrator,
        "roleWiring": definition.wiring.role_wiring.iter().map(|rule| {
            json!({
                "a": rule.a.to_string(),
                "b": rule.b.to_string(),
            })
        }).collect::<Vec<_>>(),
        "backendDefault": definition.backend.default.as_str(),
        "externalAddressBase": definition.backend.external.as_ref().map(|config| config.address_base.as_str()).unwrap_or_default(),
        "advanced": {
            "topology": optional_definition_value(&definition.topology),
            "supervisor": optional_definition_value(&definition.supervisor),
            "limits": optional_definition_value(&definition.limits),
            "spawnPolicy": optional_definition_value(&definition.spawn_policy),
            "eventRouter": optional_definition_value(&definition.event_router),
        },
    })
}

fn optional_definition_value<T: Serialize>(value: &Option<T>) -> Value {
    value
        .as_ref()
        .and_then(|value| serde_json::to_value(value).ok())
        .unwrap_or(Value::Null)
}

fn project_definition_to_editor_document(
    definition: &MobDefinition,
    mob_toml: &str,
    fallback_name: Option<&str>,
    deploy: Value,
) -> Result<MobpackDocument, String> {
    let deploy = projected_deploy_for_runtime_modes(definition, deploy);
    let mut schemas = Vec::new();
    let mut members = Vec::new();
    for (profile_name, binding) in &definition.profiles {
        let profile_key = profile_name.to_string();
        match binding {
            ProfileBinding::Inline(profile) => {
                let schema_id = profile
                    .output_schema
                    .as_ref()
                    .and_then(|schema| {
                        let schema_id = format!("{}Output", pascal_identifier(&profile_key));
                        output_schema_to_editor_schema(&schema_id, schema)
                    })
                    .map(|schema| {
                        let id = schema
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        schemas.push(schema);
                        id
                    })
                    .unwrap_or_default();
                members.push(json!({
                    "id": member_id_for_profile(&profile_key),
                    "name": profile_key,
                    "role": profile_name.to_string(),
                    "profileBinding": "inline",
                    "model": profile.model,
                    "systemPrompt": profile.peer_description,
                    "tools": tool_ids_from_config(&profile.tools),
                    "schema": schema_id,
                    "skills": profile.skills,
                    "runtimeMode": profile.runtime_mode.to_string(),
                    "externalAddressable": profile.external_addressable,
                    "backend": profile_backend_to_value(profile.backend),
                    "maxInlinePeerNotifications": profile.max_inline_peer_notifications,
                    "providerParams": profile.provider_params,
                }));
            }
            ProfileBinding::RealmRef { realm_profile } => {
                members.push(json!({
                    "id": member_id_for_profile(&profile_key),
                    "name": profile_key,
                    "role": profile_name.to_string(),
                    "profileBinding": "realm_profile",
                    "model": "",
                    "systemPrompt": format!("Realm profile reference: {realm_profile}"),
                    "tools": [],
                    "schema": "",
                    "skills": [],
                    "runtimeMode": "turn_driven",
                    "externalAddressable": false,
                    "realmProfile": realm_profile,
                }));
            }
        }
    }

    let primary = definition.flows.iter().next();
    let (flow, instances, edges, frames) = if let Some((flow_id, flow_spec)) = primary {
        project_flow(flow_id.to_string().as_str(), flow_spec)
    } else {
        (
            json!({
                "name": definition.id.to_string(),
                "steps": [editor_input_step_value(
                    "input_1",
                    editor_input_step_default_task().to_string(),
                    String::new(),
                    Vec::new()
                )]
            }),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    };
    let launch_modes = launch_modes_from_instances(&instances);

    Ok(MobpackDocument {
        schema_version: MOBPACK_SCHEMA_VERSION.to_string(),
        mob_id: definition.id.to_string(),
        name: fallback_name
            .map(ToString::to_string)
            .unwrap_or_else(|| definition.id.to_string()),
        mob_settings: mob_settings_from_definition(definition),
        members: Value::Array(members),
        instances: Value::Array(instances),
        edges: Value::Array(edges),
        frames: Value::Array(frames),
        schemas: Value::Array(schemas),
        skill_realms: skill_realms_from_definition(definition),
        flow,
        launch_modes,
        deploy,
        mob_toml: Some(mob_toml.to_string()),
    })
}

fn projected_deploy_for_runtime_modes(definition: &MobDefinition, deploy: Value) -> Value {
    if deploy
        .get("surface")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        return deploy;
    }
    let has_autonomous_host = definition.profiles.values().any(|binding| match binding {
        ProfileBinding::Inline(profile) => profile.runtime_mode.to_string() == "autonomous_host",
        ProfileBinding::RealmRef { .. } => false,
    });
    if !has_autonomous_host {
        return deploy;
    }
    let mut object = deploy.as_object().cloned().unwrap_or_default();
    object
        .entry("command".to_string())
        .or_insert_with(|| json!("rkat mob deploy"));
    object.insert("surface".to_string(), json!("rpc"));
    Value::Object(object)
}

fn project_flow(flow_id: &str, flow: &FlowSpec) -> (Value, Vec<Value>, Vec<Value>, Vec<Value>) {
    let visual_steps = flow
        .root
        .as_ref()
        .map(|root| visual_steps_from_frame(root, flow))
        .unwrap_or_else(|| visual_steps_from_flat_flow(flow));
    let task = flow
        .description
        .clone()
        .unwrap_or_else(|| editor_input_step_default_task().to_string());
    let input_params = input_params_from_flow(flow);
    let input_fields = input_param_summary(&input_params);
    let mut steps = vec![editor_input_step_value(
        "input_1",
        task,
        input_fields,
        input_params,
    )];
    steps.extend(visual_steps);
    let (instances, edges, frames) = graph_projection_from_visual_steps(&steps);
    (
        json!({
            "name": flow_id,
            "steps": steps,
        }),
        instances,
        edges,
        frames,
    )
}

#[derive(Debug, Clone, Default)]
struct VisualGraphProjection {
    entries: Vec<String>,
    exits: Vec<String>,
    next_col: usize,
}

fn graph_projection_from_visual_steps(steps: &[Value]) -> (Vec<Value>, Vec<Value>, Vec<Value>) {
    let mut instances = Vec::new();
    let mut edges = Vec::new();
    let mut frames = Vec::new();
    let mut next_edge = 1usize;
    emit_visual_graph_sequence(
        steps,
        0,
        0,
        &mut instances,
        &mut edges,
        &mut frames,
        &mut next_edge,
    );
    (instances, edges, frames)
}

fn emit_visual_graph_sequence(
    steps: &[Value],
    start_col: usize,
    row: usize,
    instances: &mut Vec<Value>,
    edges: &mut Vec<Value>,
    frames: &mut Vec<Value>,
    next_edge: &mut usize,
) -> VisualGraphProjection {
    let mut entries: Vec<String> = Vec::new();
    let mut exits: Vec<String> = Vec::new();
    let mut col = start_col;
    for step in steps {
        if step.get("type").and_then(Value::as_str) == Some("input") {
            continue;
        }
        let emitted = emit_visual_graph_step(step, col, row, instances, edges, frames, next_edge);
        if entries.is_empty() {
            entries = emitted.entries.clone();
        }
        for from in &exits {
            for to in &emitted.entries {
                push_visual_graph_edge(edges, next_edge, from, to, "next", "", Value::Null);
            }
        }
        exits = emitted.exits;
        col = emitted.next_col;
    }
    VisualGraphProjection {
        entries,
        exits,
        next_col: col,
    }
}

fn emit_visual_graph_step(
    step: &Value,
    col: usize,
    row: usize,
    instances: &mut Vec<Value>,
    edges: &mut Vec<Value>,
    frames: &mut Vec<Value>,
    next_edge: &mut usize,
) -> VisualGraphProjection {
    let Some(id) = step
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return VisualGraphProjection {
            next_col: col,
            ..VisualGraphProjection::default()
        };
    };
    match step.get("type").and_then(Value::as_str) {
        Some("member") => {
            instances.push(json!({
                "id": id,
                "memberId": step.get("role").and_then(Value::as_str).unwrap_or_default(),
                "col": col,
                "row": row,
                "launchMode": step.get("launchMode").or_else(|| step.get("launch_mode")).cloned().unwrap_or(Value::Null),
                "lane": "",
                "dispatchMode": explicit_visual_dispatch_mode(step),
                "collection": explicit_visual_collection_policy(step),
                "quorum": step.get("quorum").or_else(|| step.get("collectionQuorum")).cloned().unwrap_or(Value::Null),
                "timeoutMs": step.get("timeoutMs").or_else(|| step.get("timeout_ms")).cloned().unwrap_or(Value::Null),
                "allowedTools": step.get("allowedTools").or_else(|| step.get("allowed_tools")).cloned().unwrap_or_else(|| json!([])),
                "blockedTools": step.get("blockedTools").or_else(|| step.get("blocked_tools")).cloned().unwrap_or_else(|| json!([])),
                "outputFormat": step.get("outputFormat").or_else(|| step.get("output_format")).cloned().unwrap_or(Value::Null),
            }));
            VisualGraphProjection {
                entries: vec![id.to_string()],
                exits: vec![id.to_string()],
                next_col: col + 1,
            }
        }
        Some("repeat") => {
            let frame_start = col;
            let body = emit_visual_graph_sequence(
                step.get("steps")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                col,
                row,
                instances,
                edges,
                frames,
                next_edge,
            );
            let cond = step
                .get("cond")
                .and_then(graph_cond_from_editor_cond)
                .unwrap_or(Value::Null);
            let label = step
                .get("until")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!("until {value}"))
                .unwrap_or_else(|| "until condition".to_string());
            for from in &body.exits {
                for to in &body.entries {
                    push_visual_graph_edge(
                        edges,
                        next_edge,
                        from,
                        to,
                        "cond",
                        &label,
                        cond.clone(),
                    );
                }
            }
            if !body.entries.is_empty() {
                let max_iterations_label = step
                    .get("maxIterations")
                    .and_then(Value::as_u64)
                    .filter(|value| *value > 0)
                    .map(|value| format!("max {value}"))
                    .unwrap_or_else(|| "missing max_iterations".to_string());
                frames.push(json!({
                    "id": format!("frame_{id}"),
                    "kind": "RepeatUntil",
                    "colStart": frame_start,
                    "colEnd": body.next_col.saturating_sub(1).max(frame_start),
                    "label": format!("REPEAT-UNTIL · {label} · {max_iterations_label}"),
                }));
            }
            body
        }
        Some("branch") | Some("parallel") => {
            emit_visual_graph_fork_step(step, id, col, row, instances, edges, frames, next_edge)
        }
        _ => VisualGraphProjection {
            next_col: col,
            ..VisualGraphProjection::default()
        },
    }
}

fn emit_visual_graph_fork_step(
    step: &Value,
    id: &str,
    col: usize,
    row: usize,
    instances: &mut Vec<Value>,
    edges: &mut Vec<Value>,
    frames: &mut Vec<Value>,
    next_edge: &mut usize,
) -> VisualGraphProjection {
    let step_type = step.get("type").and_then(Value::as_str).unwrap_or_default();
    let is_parallel = step_type == "parallel";
    let gate_id = format!("g_{step_type}_{id}");
    let join_id = format!("j_{step_type}_{id}");
    instances.push(json!({
        "id": gate_id,
        "isGate": true,
        "gateKind": if is_parallel { "fork" } else { "branch" },
        "label": if is_parallel { explicit_visual_dispatch_mode(step) } else { "branch".to_string() },
        "col": col,
        "row": row,
    }));
    let mut lane_exits = Vec::new();
    let mut max_col = col + 1;
    if let Some(branches) = step.get("branches").and_then(Value::as_array) {
        for (index, branch) in branches.iter().enumerate() {
            let lane = emit_visual_graph_sequence(
                branch
                    .get("steps")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                col + 1,
                row + index,
                instances,
                edges,
                frames,
                next_edge,
            );
            let condition = branch
                .get("condition")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let cond = branch
                .get("cond")
                .and_then(graph_cond_from_editor_cond)
                .unwrap_or(Value::Null);
            for entry in &lane.entries {
                push_visual_graph_edge(
                    edges,
                    next_edge,
                    &gate_id,
                    entry,
                    if is_parallel { "fanout" } else { "cond" },
                    condition,
                    cond.clone(),
                );
            }
            lane_exits.extend(lane.exits);
            max_col = max_col.max(lane.next_col);
        }
    }
    if !is_parallel {
        let fallback = emit_visual_graph_sequence(
            step.get("fallback")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            col + 1,
            row + step
                .get("branches")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default(),
            instances,
            edges,
            frames,
            next_edge,
        );
        for entry in &fallback.entries {
            push_visual_graph_edge(
                edges,
                next_edge,
                &gate_id,
                entry,
                "next",
                "fallback",
                Value::Null,
            );
        }
        lane_exits.extend(fallback.exits);
        max_col = max_col.max(fallback.next_col);
    }
    instances.push(json!({
        "id": join_id,
        "isGate": true,
        "gateKind": "join",
        "label": if is_parallel { format!("join · {}", explicit_visual_collection_policy(step)) } else { "join · branch paths".to_string() },
        "col": max_col,
        "row": row,
        "collection": if is_parallel { explicit_visual_collection_policy(step) } else { "any".to_string() },
        "controllerRole": step.get("controllerRole").or_else(|| step.get("controllerMemberId")).or_else(|| step.get("controlRole")).cloned().unwrap_or(Value::Null),
        "quorum": if step.get("collection").and_then(Value::as_str) == Some("quorum") { json!({ "mode": "NofM", "n": step.get("quorum").and_then(Value::as_u64).unwrap_or(2), "m": lane_exits.len().max(1) }) } else { Value::Null },
    }));
    for exit in &lane_exits {
        push_visual_graph_edge(edges, next_edge, exit, &join_id, "next", "", Value::Null);
    }
    frames.push(json!({
        "id": format!("frame_{step_type}_{id}"),
        "kind": if is_parallel { "Parallel" } else { "Branch" },
        "colStart": col,
        "colEnd": max_col,
        "label": if is_parallel {
            visual_parallel_frame_label(step)
        } else {
            format!(
                "BRANCH · {} path{}",
                lane_exits.len(),
                if lane_exits.len() == 1 { "" } else { "s" }
            )
        },
    }));
    VisualGraphProjection {
        entries: vec![gate_id],
        exits: vec![join_id],
        next_col: max_col + 1,
    }
}

fn explicit_visual_dispatch_mode(step: &Value) -> String {
    step.get("dispatch")
        .or_else(|| step.get("dispatchMode"))
        .or_else(|| step.get("dispatch_mode"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn explicit_visual_collection_policy(step: &Value) -> String {
    let Some(policy) = step
        .get("collection")
        .or_else(|| step.get("collectionPolicy"))
        .or_else(|| step.get("collection_policy"))
    else {
        return String::new();
    };
    match policy {
        Value::String(text) => text.trim().to_string(),
        Value::Object(map) => map
            .get("type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn visual_parallel_frame_label(step: &Value) -> String {
    let dispatch = explicit_visual_dispatch_mode(step);
    let collection = explicit_visual_collection_policy(step);
    format!(
        "PARALLEL · {} · join {}",
        if dispatch.is_empty() {
            "missing dispatch"
        } else {
            dispatch.as_str()
        },
        if collection.is_empty() {
            "missing collection"
        } else {
            collection.as_str()
        }
    )
}

fn push_visual_graph_edge(
    edges: &mut Vec<Value>,
    next_edge: &mut usize,
    from: &str,
    to: &str,
    kind: &str,
    label: &str,
    cond: Value,
) {
    edges.push(json!({
        "id": format!("e{}", *next_edge),
        "from": from,
        "to": to,
        "kind": kind,
        "label": label,
        "cond": cond,
    }));
    *next_edge += 1;
}

fn graph_cond_from_editor_cond(cond: &Value) -> Option<Value> {
    let step_id = cond.get("stepId").and_then(Value::as_str)?.trim();
    let field = cond.get("field").and_then(Value::as_str)?.trim();
    if step_id.is_empty() || field.is_empty() {
        return None;
    }
    let namespace = cond
        .get("namespace")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let var = if namespace == "params" || step_id == "params" {
        format!("params.{field}")
    } else {
        format!("steps.{step_id}.{field}")
    };
    Some(json!({
        "var": var,
        "op": cond.get("op").and_then(Value::as_str).unwrap_or("=="),
        "val": cond.get("val").and_then(Value::as_str).unwrap_or_default(),
    }))
}

fn input_params_from_flow(flow: &FlowSpec) -> Vec<Value> {
    let mut names = BTreeSet::new();
    for step in flow.steps.values() {
        if let Some(condition) = &step.condition {
            collect_condition_param_refs(condition, &mut names);
        }
    }
    if let Some(root) = &flow.root {
        collect_frame_param_refs(root, &mut names);
    }
    names
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            json!({
                "id": format!("p{}", index + 1),
                "name": name,
                "type": "string",
                "required": true,
                "description": "Activation parameter referenced by a MobKit condition.",
                "enumValues": [],
            })
        })
        .collect()
}

fn collect_frame_param_refs(frame: &FrameSpec, out: &mut BTreeSet<String>) {
    for node in frame.nodes.values() {
        if let FlowNodeSpec::RepeatUntil(loop_spec) = node {
            collect_condition_param_refs(&loop_spec.until, out);
            collect_frame_param_refs(&loop_spec.body, out);
        }
    }
}

fn collect_condition_param_refs(condition: &ConditionExpr, out: &mut BTreeSet<String>) {
    match condition {
        ConditionExpr::Eq { path, .. }
        | ConditionExpr::In { path, .. }
        | ConditionExpr::Gt { path, .. }
        | ConditionExpr::Lt { path, .. } => {
            if let Some(field) = path.strip_prefix("params.") {
                let field = field.split('.').next().unwrap_or_default().trim();
                if !field.is_empty() {
                    out.insert(field.to_string());
                }
            }
        }
        ConditionExpr::And { exprs } | ConditionExpr::Or { exprs } => {
            for expr in exprs {
                collect_condition_param_refs(expr, out);
            }
        }
        ConditionExpr::Not { expr } => collect_condition_param_refs(expr, out),
    }
}

fn input_param_summary(params: &[Value]) -> String {
    params
        .iter()
        .filter_map(|param| {
            let name = param.get("name").and_then(Value::as_str)?.trim();
            if name.is_empty() {
                return None;
            }
            let field_type = param
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("string")
                .trim();
            let optional = param
                .get("required")
                .and_then(Value::as_bool)
                .is_some_and(|required| !required);
            Some(format!(
                "{name}: {}{}",
                if field_type.is_empty() {
                    "string"
                } else {
                    field_type
                },
                if optional { "?" } else { "" }
            ))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn visual_steps_from_flat_flow(flow: &FlowSpec) -> Vec<Value> {
    let order = topological_step_order(flow);
    let mut consumed = BTreeSet::new();
    let mut out = Vec::new();
    for step_id in order {
        if consumed.contains(&step_id) {
            continue;
        }
        let Some(step) = flow.steps.get(&step_id) else {
            continue;
        };
        if let Some(branch_id) = &step.branch {
            let branch_steps = flow
                .steps
                .iter()
                .filter(|(_, candidate)| candidate.branch.as_ref() == Some(branch_id))
                .collect::<Vec<_>>();
            if branch_steps.len() > 1 {
                for (candidate_id, _) in &branch_steps {
                    consumed.insert((*candidate_id).clone());
                }
                out.push(json!({
                    "id": format!("branch_{}", sanitize_identifier(&branch_id.to_string())),
                    "type": "branch",
                    "dependsMode": dependency_mode_string(&step.depends_on_mode),
                    "branches": branch_steps.iter().map(|(candidate_id, candidate)| json!({
                        "id": format!("br_{}", sanitize_identifier(&candidate_id.to_string())),
                        "label": candidate_id.to_string(),
                        "condition": candidate.condition.as_ref().map(condition_to_label).unwrap_or_default(),
                        "cond": candidate.condition.as_ref().map(condition_to_editor_cond_json),
                        "steps": [visual_step_from_step(&candidate_id.to_string(), candidate)],
                    })).collect::<Vec<_>>(),
                    "fallback": [],
                }));
                continue;
            }
        }
        out.push(visual_step_from_step(&step_id.to_string(), step));
    }
    out
}

fn visual_steps_from_frame(frame: &FrameSpec, flow: &FlowSpec) -> Vec<Value> {
    let mut out = Vec::new();
    let mut consumed = BTreeSet::new();
    for (node_id, node) in &frame.nodes {
        if consumed.contains(node_id) {
            continue;
        }
        match node {
            FlowNodeSpec::Step(frame_step) => {
                if let Some(step) = flow.steps.get(&frame_step.step_id) {
                    if step.branch.is_none() {
                        let parallel_nodes = parallel_sibling_nodes(frame, flow, frame_step);
                        if parallel_nodes.len() > 1 {
                            for (candidate_node_id, _, _) in &parallel_nodes {
                                consumed.insert((*candidate_node_id).clone());
                            }
                            let id = parallel_nodes
                                .iter()
                                .map(|(_, candidate_frame_step, _)| {
                                    sanitize_identifier(&candidate_frame_step.step_id.to_string())
                                })
                                .collect::<Vec<_>>()
                                .join("_");
                            out.push(json!({
                                "id": format!("parallel_{id}"),
                                "type": "parallel",
                                "dispatch": "fan_out",
                                "collection": "all",
                                "dependsMode": dependency_mode_string(&frame_step.depends_on_mode),
                                "branches": parallel_nodes.iter().enumerate().map(|(index, (_, candidate_frame_step, candidate_step))| json!({
                                    "id": format!("br_{}", sanitize_identifier(&candidate_frame_step.step_id.to_string())),
                                    "label": candidate_frame_step.step_id.to_string(),
                                    "steps": [visual_step_from_step(&candidate_frame_step.step_id.to_string(), candidate_step)],
                                    "order": index,
                                })).collect::<Vec<_>>(),
                            }));
                            continue;
                        }
                    }
                    if let Some(branch_id) = &step.branch {
                        let branch_nodes = frame
                            .nodes
                            .iter()
                            .filter_map(|(candidate_node_id, candidate_node)| {
                                let FlowNodeSpec::Step(candidate_frame_step) = candidate_node
                                else {
                                    return None;
                                };
                                let candidate_step =
                                    flow.steps.get(&candidate_frame_step.step_id)?;
                                (candidate_step.branch.as_ref() == Some(branch_id)).then_some((
                                    candidate_node_id,
                                    candidate_frame_step,
                                    candidate_step,
                                ))
                            })
                            .collect::<Vec<_>>();
                        if branch_nodes.len() > 1 {
                            for (candidate_node_id, _, _) in &branch_nodes {
                                consumed.insert((*candidate_node_id).clone());
                            }
                            let controller_role =
                                branch_controller_role_from_depends_on(frame, flow, &branch_nodes);
                            out.push(json!({
                                "id": format!("branch_{}", sanitize_identifier(&branch_id.to_string())),
                                "type": "branch",
                                "controllerRole": controller_role,
                                "dependsMode": dependency_mode_string(&step.depends_on_mode),
                                "branches": branch_nodes.iter().enumerate().map(|(_, (_, candidate_frame_step, candidate_step))| json!({
                                    "id": format!("br_{}", sanitize_identifier(&candidate_frame_step.step_id.to_string())),
                                    "label": candidate_frame_step.step_id.to_string(),
                                    "condition": candidate_step.condition.as_ref().map(condition_to_label).unwrap_or_default(),
                                    "cond": candidate_step.condition.as_ref().map(condition_to_editor_cond_json),
                                    "steps": [visual_step_from_step(&candidate_frame_step.step_id.to_string(), candidate_step)],
                                })).collect::<Vec<_>>(),
                                "fallback": [],
                            }));
                            continue;
                        }
                    }
                    out.push(visual_step_from_step(&frame_step.step_id.to_string(), step));
                }
            }
            FlowNodeSpec::RepeatUntil(loop_spec) => {
                let body_steps = visual_steps_from_frame(&loop_spec.body, flow);
                let body_last_step = last_step_id_in_frame(&loop_spec.body);
                out.push(json!({
                    "id": node_id.to_string(),
                    "type": "repeat",
                    "loopId": loop_spec.loop_id.to_string(),
                    "maxIterations": loop_spec.max_iterations,
                    "iterationInput": Value::Null,
                    "cond": repeat_condition_to_editor(&loop_spec.until, body_last_step.as_deref()),
                    "steps": body_steps,
                }));
            }
        }
    }
    out
}

fn branch_controller_role_from_depends_on(
    frame: &FrameSpec,
    flow: &FlowSpec,
    branch_nodes: &[(
        &meerkat_mob::FlowNodeId,
        &meerkat_mob::definition::FrameStepSpec,
        &FlowStepSpec,
    )],
) -> Option<String> {
    let (_, frame_step, _) = branch_nodes.first()?;
    for node_id in &frame_step.depends_on {
        let Some(FlowNodeSpec::Step(dep_step)) = frame.nodes.get(node_id) else {
            continue;
        };
        let Some(step) = flow.steps.get(&dep_step.step_id) else {
            continue;
        };
        if step.branch.is_none() {
            return Some(member_id_for_profile(&step.role.to_string()));
        }
    }
    None
}

fn parallel_sibling_nodes<'a>(
    frame: &'a FrameSpec,
    flow: &'a FlowSpec,
    frame_step: &meerkat_mob::definition::FrameStepSpec,
) -> Vec<(
    &'a meerkat_mob::FlowNodeId,
    &'a meerkat_mob::definition::FrameStepSpec,
    &'a FlowStepSpec,
)> {
    frame
        .nodes
        .iter()
        .filter_map(|(candidate_node_id, candidate_node)| {
            let FlowNodeSpec::Step(candidate_frame_step) = candidate_node else {
                return None;
            };
            if candidate_frame_step.depends_on != frame_step.depends_on
                || candidate_frame_step.depends_on_mode != frame_step.depends_on_mode
                || candidate_frame_step.branch.is_some()
            {
                return None;
            }
            let candidate_step = flow.steps.get(&candidate_frame_step.step_id)?;
            if candidate_step.branch.is_some() || candidate_step.condition.is_some() {
                return None;
            }
            Some((candidate_node_id, candidate_frame_step, candidate_step))
        })
        .collect()
}

fn visual_step_from_step(id: &str, step: &FlowStepSpec) -> Value {
    json!({
        "id": id,
        "type": "member",
        "role": member_id_for_profile(&step.role.to_string()),
        "instruction": step.message.text_content(),
        "launchMode": { "kind": "Fresh" },
        "dependsMode": dependency_mode_string(&step.depends_on_mode),
        "dispatchMode": dispatch_mode_string(&step.dispatch_mode),
        "collection": collection_policy_kind_string(&step.collection_policy),
        "quorum": collection_policy_quorum(&step.collection_policy),
        "timeoutMs": step.timeout_ms,
        "allowedTools": step.allowed_tools,
        "blockedTools": step.blocked_tools,
        "outputFormat": imported_step_output_format_value(&step.output_format),
        "expectedSchemaRef": step.expected_schema_ref,
    })
}

fn step_output_format_string(format: &StepOutputFormat) -> &'static str {
    match format {
        StepOutputFormat::Json => "json",
        StepOutputFormat::Text => "text",
    }
}

fn imported_step_output_format_value(format: &StepOutputFormat) -> Value {
    match format {
        StepOutputFormat::Json => Value::Null,
        StepOutputFormat::Text => json!("text"),
    }
}

fn dispatch_mode_string(mode: &DispatchMode) -> &'static str {
    match mode {
        DispatchMode::FanOut => "fan_out",
        DispatchMode::OneToOne => "one_to_one",
        DispatchMode::FanIn => "fan_in",
    }
}

fn collection_policy_kind_string(policy: &CollectionPolicy) -> &'static str {
    match policy {
        CollectionPolicy::All => "all",
        CollectionPolicy::Any => "any",
        CollectionPolicy::Quorum { .. } => "quorum",
    }
}

fn collection_policy_quorum(policy: &CollectionPolicy) -> Option<u8> {
    match policy {
        CollectionPolicy::Quorum { n } => Some(*n),
        CollectionPolicy::All | CollectionPolicy::Any => None,
    }
}

fn launch_modes_from_instances(instances: &[Value]) -> Value {
    Value::Array(
        instances
            .iter()
            .filter(|instance| instance.get("memberId").and_then(Value::as_str).is_some())
            .map(|instance| {
                let mut entry = serde_json::Map::new();
                entry.insert(
                    "step_id".to_string(),
                    json!(
                        instance
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    ),
                );
                entry.insert(
                    "member_id".to_string(),
                    json!(
                        instance
                            .get("memberId")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    ),
                );
                entry.insert(
                    "launch_mode".to_string(),
                    instance.get("launchMode").cloned().unwrap_or(Value::Null),
                );
                if let Some(policy) =
                    budget_split_policy_from_launch_mode(instance.get("launchMode"))
                {
                    entry.insert("budget_split_policy".to_string(), policy);
                }
                Value::Object(entry)
            })
            .collect::<Vec<_>>(),
    )
}

fn budget_split_policy_from_launch_mode(launch_mode: Option<&Value>) -> Option<Value> {
    let launch_mode = launch_mode.filter(|mode| mode.is_object())?;
    launch_mode
        .get("budgetSplitPolicy")
        .or_else(|| launch_mode.get("budget_split_policy"))
        .or_else(|| launch_mode.get("budget"))
        .and_then(normalize_budget_split_policy_value)
}

fn normalize_budget_split_policy_value(policy: &Value) -> Option<Value> {
    let kind = policy
        .get("type")
        .or_else(|| policy.get("kind"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_ascii_lowercase();
    if kind == "fixed" {
        let Some(value) = policy
            .get("value")
            .or_else(|| policy.get("limit"))
            .or_else(|| policy.get("tokens"))
            .and_then(Value::as_u64)
        else {
            return Some(json!({ "type": "fixed" }));
        };
        return Some(json!({ "type": "fixed", "value": value }));
    }
    if matches!(kind.as_str(), "equal" | "proportional" | "remaining") {
        return Some(json!({ "type": kind }));
    }
    Some(json!({ "type": kind }))
}

fn topological_step_order(flow: &FlowSpec) -> Vec<meerkat_mob::StepId> {
    let mut indegree: BTreeMap<meerkat_mob::StepId, usize> = flow
        .steps
        .keys()
        .cloned()
        .map(|key| (key, 0usize))
        .collect();
    let mut outgoing: BTreeMap<meerkat_mob::StepId, Vec<meerkat_mob::StepId>> = BTreeMap::new();
    for (step_id, step) in &flow.steps {
        for dep in &step.depends_on {
            if indegree.contains_key(dep) {
                *indegree.entry(step_id.clone()).or_default() += 1;
                outgoing
                    .entry(dep.clone())
                    .or_default()
                    .push(step_id.clone());
            }
        }
    }
    let mut queue = flow
        .steps
        .keys()
        .filter(|step_id| indegree.get(*step_id).copied().unwrap_or_default() == 0)
        .cloned()
        .collect::<VecDeque<_>>();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    while let Some(step_id) = queue.pop_front() {
        if !seen.insert(step_id.clone()) {
            continue;
        }
        out.push(step_id.clone());
        for child in outgoing.get(&step_id).cloned().unwrap_or_default() {
            if let Some(count) = indegree.get_mut(&child) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    queue.push_back(child);
                }
            }
        }
    }
    for step_id in flow.steps.keys() {
        if !seen.contains(step_id) {
            out.push(step_id.clone());
        }
    }
    out
}

fn last_step_id_in_frame(frame: &FrameSpec) -> Option<String> {
    let mut out = None;
    for node in frame.nodes.values() {
        match node {
            FlowNodeSpec::Step(step) => out = Some(step.step_id.to_string()),
            FlowNodeSpec::RepeatUntil(loop_spec) => {
                if let Some(step_id) = last_step_id_in_frame(&loop_spec.body) {
                    out = Some(step_id);
                }
            }
        }
    }
    out
}

fn repeat_condition_to_editor(condition: &ConditionExpr, step_id: Option<&str>) -> Value {
    match condition {
        ConditionExpr::Eq { path, value } => json!({
            "stepId": step_id.unwrap_or_default(),
            "field": path.rsplit('.').next().unwrap_or(path),
            "op": "==",
            "val": scalar_condition_value(value),
        }),
        ConditionExpr::Gt { path, value } => json!({
            "stepId": step_id.unwrap_or_default(),
            "field": path.rsplit('.').next().unwrap_or(path),
            "op": ">",
            "val": scalar_condition_value(value),
        }),
        ConditionExpr::Lt { path, value } => json!({
            "stepId": step_id.unwrap_or_default(),
            "field": path.rsplit('.').next().unwrap_or(path),
            "op": "<",
            "val": scalar_condition_value(value),
        }),
        _ => json!({
            "stepId": step_id.unwrap_or_default(),
            "field": "condition",
            "op": "==",
            "val": condition_to_label(condition),
        }),
    }
}

fn condition_to_editor_cond_json(condition: &ConditionExpr) -> Value {
    fn simple(path: &str, op: &str, value: &Value) -> Value {
        if let Some(field) = path.strip_prefix("params.") {
            return json!({
                "namespace": "params",
                "stepId": "params",
                "field": field.split('.').next().unwrap_or(field),
                "op": op,
                "val": scalar_condition_value(value),
            });
        }
        if let Some(rest) = path.strip_prefix("steps.") {
            let mut parts = rest.split('.');
            let step_id = parts.next().unwrap_or_default();
            let field = parts.next().unwrap_or_default();
            return json!({
                "namespace": "steps",
                "stepId": step_id,
                "field": field,
                "op": op,
                "val": scalar_condition_value(value),
            });
        }
        json!({
            "namespace": "params",
            "stepId": "params",
            "field": path.rsplit('.').next().unwrap_or(path),
            "op": op,
            "val": scalar_condition_value(value),
        })
    }

    match condition {
        ConditionExpr::Eq { path, value } => simple(path, "==", value),
        ConditionExpr::Gt { path, value } => simple(path, ">", value),
        ConditionExpr::Lt { path, value } => simple(path, "<", value),
        _ => json!({}),
    }
}

fn condition_to_label(condition: &ConditionExpr) -> String {
    match condition {
        ConditionExpr::Eq { path, value } => format!("{path} == {}", scalar_condition_value(value)),
        ConditionExpr::In { path, values } => format!(
            "{path} in [{}]",
            values
                .iter()
                .map(scalar_condition_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ConditionExpr::Gt { path, value } => format!("{path} > {}", scalar_condition_value(value)),
        ConditionExpr::Lt { path, value } => format!("{path} < {}", scalar_condition_value(value)),
        ConditionExpr::And { exprs } => exprs
            .iter()
            .map(condition_to_label)
            .collect::<Vec<_>>()
            .join(" && "),
        ConditionExpr::Or { exprs } => exprs
            .iter()
            .map(condition_to_label)
            .collect::<Vec<_>>()
            .join(" || "),
        ConditionExpr::Not { expr } => format!("not ({})", condition_to_label(expr)),
    }
}

fn scalar_condition_value(value: &Value) -> String {
    value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn output_schema_to_editor_schema(schema_id: &str, schema: &Value) -> Option<Value> {
    let object = schema.as_object()?;
    let properties = object.get("properties").and_then(Value::as_object);
    let required = object
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let fields = properties
        .map(|props| {
            props
                .iter()
                .enumerate()
                .map(|(index, (name, value))| {
                    let enum_values = value
                        .get("enum")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let field_type = if enum_values.is_empty() {
                        editor_type_from_json_schema(value)
                    } else {
                        "enum".to_string()
                    };
                    json!({
                        "id": format!("f{}", index + 1),
                        "name": name,
                        "type": field_type,
                        "required": required.contains(name),
                        "description": value.get("description").and_then(Value::as_str).unwrap_or_default(),
                        "enumValues": enum_values,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(json!({
        "id": schema_id,
        "description": object.get("description").and_then(Value::as_str).unwrap_or_default(),
        "fields": fields,
    }))
}

fn editor_type_from_json_schema(value: &Value) -> String {
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("string")
    {
        "integer" => "integer".to_string(),
        "number" => "number".to_string(),
        "boolean" => "boolean".to_string(),
        "array" => value
            .get("items")
            .and_then(|items| items.get("type"))
            .and_then(Value::as_str)
            .map(|item| format!("{item}[]"))
            .unwrap_or_else(|| "string[]".to_string()),
        "object" => "object".to_string(),
        _ => "string".to_string(),
    }
}

fn tool_ids_from_config(config: &ToolConfig) -> Vec<String> {
    let mut out = serde_json::to_value(config)
        .ok()
        .and_then(|value| {
            value.as_object().map(|object| {
                object
                    .iter()
                    .filter(|(_, value)| value.as_bool() == Some(true))
                    .map(|(field, _)| field.to_string())
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default();
    out.retain(|field| tool_config_bool_fields().contains(field));
    out.extend(config.mcp.iter().map(|source| format!("mcp:{source}")));
    out.extend(
        config
            .rust_bundles
            .iter()
            .map(|bundle| format!("rust:{bundle}")),
    );
    out
}

fn skill_realms_from_definition(definition: &MobDefinition) -> Value {
    if definition.skills.is_empty() {
        return Value::Array(Vec::new());
    }
    let skills = definition
        .skills
        .iter()
        .map(|(id, source)| match source {
            SkillSource::Inline { content } => json!({
                "id": id,
                "label": id,
                "source": "inline",
                "origin": "meerkat_mob::SkillSource",
                "content": content,
            }),
            SkillSource::Path { path } => json!({
                "id": id,
                "label": id,
                "source": "path",
                "origin": "meerkat_mob::SkillSource",
                "path": path,
            }),
        })
        .collect::<Vec<_>>();
    json!([{
        "id": "imported/mob.toml",
        "label": "imported/mob.toml",
        "default": true,
        "source": "meerkat_mob::MobDefinition",
        "sourceDocumentPath": "[skills]",
        "skills": skills,
    }])
}

fn member_id_for_profile(name: &str) -> String {
    format!("m_{}", sanitize_identifier(name))
}

fn pascal_identifier(name: &str) -> String {
    let mut out = String::new();
    for part in name
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
    {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars.map(|ch| ch.to_ascii_lowercase()));
        }
    }
    if out.is_empty() {
        "Profile".to_string()
    } else {
        out
    }
}

fn sanitize_identifier(name: &str) -> String {
    let out = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if out.is_empty() {
        "member".to_string()
    } else {
        out
    }
}

fn dependency_mode_string(mode: &DependencyMode) -> &'static str {
    match mode {
        DependencyMode::All => "all",
        DependencyMode::Any => "any",
    }
}

#[derive(Debug, Clone)]
struct RenderedFlow {
    steps: Vec<RenderedStep>,
    root: RenderedFrame,
}

#[derive(Debug, Clone)]
struct RenderedFrame {
    nodes: Vec<RenderedNode>,
}

#[derive(Debug, Clone)]
enum RenderedNode {
    Step(RenderedStepNode),
    Repeat(RenderedRepeatNode),
}

#[derive(Debug, Clone)]
struct RenderedStepNode {
    id: String,
    step_id: String,
    depends_on: Vec<String>,
    depends_mode: String,
    branch: Option<String>,
}

#[derive(Debug, Clone)]
struct RenderedRepeatNode {
    id: String,
    loop_id: String,
    depends_on: Vec<String>,
    depends_mode: String,
    until: String,
    max_iterations: u64,
    body: RenderedFrame,
}

#[derive(Debug, Clone)]
struct RenderedStep {
    id: String,
    role: String,
    message: String,
    depends_on: Vec<String>,
    depends_mode: String,
    dispatch_mode: Option<String>,
    collection_policy: Option<String>,
    branch: Option<String>,
    condition: Option<String>,
    timeout_ms: Option<u64>,
    expected_schema_ref: Option<String>,
    allowed_tools: Vec<String>,
    blocked_tools: Vec<String>,
    output_format: Option<String>,
}

#[derive(Debug)]
struct RenderState<'a> {
    members: BTreeMap<String, &'a Value>,
    steps: Vec<RenderedStep>,
    step_ids_by_visual_id: BTreeMap<String, String>,
    flat_exits_by_node_id: BTreeMap<String, Vec<String>>,
    next: usize,
}

fn render_editor_document_mob_toml(document: &MobpackDocument) -> Result<String, String> {
    let mut lines = Vec::new();
    let members = document
        .members
        .as_array()
        .map(|members| {
            members
                .iter()
                .filter(|member| {
                    !member
                        .get("missing")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let schemas = editor_schema_map(&document.schemas);
    let settings = normalize_editor_mob_settings(&document.mob_settings);
    let flow_steps = document
        .flow
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let flow_member_steps = flow_steps
        .iter()
        .filter(|step| step.get("type").and_then(Value::as_str) != Some("input"))
        .cloned()
        .collect::<Vec<_>>();
    let compiled = compile_editor_flow(&flow_member_steps, &members);
    let mob_id = sanitize_identifier(
        document
            .name
            .trim()
            .strip_suffix(".mobpack")
            .unwrap_or_else(|| document.name.trim())
            .trim()
            .is_empty()
            .then_some(document.mob_id.as_str())
            .unwrap_or_else(|| document.name.trim()),
    );

    lines.push("[mob]".to_string());
    lines.push(format!("id = {}", toml_string(&mob_id)));
    if let Some(orchestrator) = settings.string("orchestrator") {
        lines.push(format!("orchestrator = {}", toml_string(&orchestrator)));
    }
    lines.push(String::new());

    emit_rendered_wiring(&mut lines, &settings);
    emit_rendered_backend(&mut lines, &settings);
    emit_rendered_advanced_mob_settings(&mut lines, &settings);

    let mut selected_skills = BTreeMap::<String, Value>::new();
    for member in &members {
        for skill in string_vec(member.get("skills")) {
            if selected_skills.contains_key(&skill) {
                continue;
            }
            if let Some(source) = skill_source_from_realms(&skill, &document.skill_realms) {
                selected_skills.insert(skill, source);
            }
        }
        emit_rendered_profile(&mut lines, member, &schemas);
    }

    for (skill, source) in selected_skills {
        lines.push(format!("[skills.{}]", toml_key(&skill)));
        let source_kind = source
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("inline");
        lines.push(format!("source = {}", toml_string(source_kind)));
        if source_kind == "path" {
            let path = packed_skill_archive_path(&skill, &source);
            lines.push(format!("path = {}", toml_string(&path)));
        } else {
            let content = source
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            lines.push(format!("content = {}", toml_string(content)));
        }
        lines.push(String::new());
    }

    let input = flow_steps
        .iter()
        .find(|step| step.get("type").and_then(Value::as_str) == Some("input"));
    lines.push("[flows.main]".to_string());
    lines.push(format!(
        "description = {}",
        toml_string(
            input
                .and_then(|step| step.get("task"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("Generated by MobKit Flow Editor"),
        )
    ));
    if let Some(summary) = input.and_then(input_param_summary_for_step) {
        lines.push(format!("# input params: {summary}"));
    }
    lines.push(String::new());

    for step in &compiled.steps {
        emit_rendered_flow_step(&mut lines, step);
    }
    emit_rendered_frame_toml(&mut lines, "main", "root", &compiled.root);

    Ok(format!("{}\n", lines.join("\n").trim_end()))
}

fn compile_editor_flow(steps: &[Value], members: &[&Value]) -> RenderedFlow {
    let mut state = RenderState {
        members: members
            .iter()
            .filter_map(|member| {
                member
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(|id| (id.to_string(), *member))
            })
            .collect(),
        steps: Vec::new(),
        step_ids_by_visual_id: BTreeMap::new(),
        flat_exits_by_node_id: BTreeMap::new(),
        next: 1,
    };
    let root = compile_editor_lane(steps, Vec::new(), &mut state);
    RenderedFlow {
        steps: state.steps,
        root,
    }
}

fn compile_editor_lane(
    steps: &[Value],
    mut depends_on_nodes: Vec<String>,
    state: &mut RenderState<'_>,
) -> RenderedFrame {
    let mut frame = RenderedFrame { nodes: Vec::new() };
    for step in steps {
        let (nodes, exits) = compile_editor_visual_step(step, depends_on_nodes, state);
        frame.nodes.extend(nodes);
        depends_on_nodes = exits;
    }
    frame
}

fn compile_editor_visual_step(
    step: &Value,
    depends_on_nodes: Vec<String>,
    state: &mut RenderState<'_>,
) -> (Vec<RenderedNode>, Vec<String>) {
    match step.get("type").and_then(Value::as_str).unwrap_or_default() {
        "member" => compile_editor_member_step(step, depends_on_nodes, state),
        "repeat" => compile_editor_repeat_step(step, depends_on_nodes, state),
        "parallel" => compile_editor_parallel_step(step, depends_on_nodes, state),
        "branch" => compile_editor_branch_step(step, depends_on_nodes, state),
        _ => (Vec::new(), depends_on_nodes),
    }
}

fn compile_editor_member_step(
    step: &Value,
    depends_on_nodes: Vec<String>,
    state: &mut RenderState<'_>,
) -> (Vec<RenderedNode>, Vec<String>) {
    let Some(member) = step
        .get("role")
        .and_then(Value::as_str)
        .map(str::trim)
        .and_then(|member_id| state.members.get(member_id).copied())
    else {
        return (Vec::new(), depends_on_nodes);
    };
    let Some(message) = editor_step_instruction(step) else {
        return (Vec::new(), depends_on_nodes);
    };
    let visual_id = step
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("step")
        .to_string();
    let profile = editor_profile_name(member);
    let step_id = format!("{:02}_{}", state.next, sanitize_identifier(&profile));
    state.next += 1;
    let node_id = format!("node_{step_id}");
    state
        .step_ids_by_visual_id
        .insert(visual_id, step_id.clone());
    let depends_on = flat_step_depends_on(&depends_on_nodes, state);
    let rendered = RenderedStep {
        id: step_id.clone(),
        role: profile.clone(),
        message,
        depends_on,
        depends_mode: step_string(step, "dependsMode").unwrap_or_else(|| "all".to_string()),
        dispatch_mode: explicit_editor_dispatch_mode(step),
        collection_policy: explicit_editor_collection_policy_toml(step),
        branch: None,
        condition: None,
        timeout_ms: editor_u64(step, "timeoutMs").or_else(|| editor_u64(step, "timeout_ms")),
        expected_schema_ref: step
            .get("expectedSchemaRef")
            .or_else(|| step.get("expected_schema_ref"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                member
                    .get("schema")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|schema| format!("schemas/{schema}.json"))
            }),
        allowed_tools: editor_string_vec(
            step.get("allowedTools")
                .or_else(|| step.get("allowed_tools")),
        ),
        blocked_tools: editor_string_vec(
            step.get("blockedTools")
                .or_else(|| step.get("blocked_tools")),
        ),
        output_format: explicit_editor_output_format(step),
    };
    state.steps.push(rendered);
    let node = RenderedNode::Step(RenderedStepNode {
        id: node_id.clone(),
        step_id: step_id.clone(),
        depends_on: depends_on_nodes,
        depends_mode: step_string(step, "dependsMode").unwrap_or_else(|| "all".to_string()),
        branch: None,
    });
    state
        .flat_exits_by_node_id
        .insert(node_id.clone(), vec![step_id]);
    (vec![node], vec![node_id])
}

fn compile_editor_repeat_step(
    step: &Value,
    depends_on_nodes: Vec<String>,
    state: &mut RenderState<'_>,
) -> (Vec<RenderedNode>, Vec<String>) {
    let Some(loop_id) = step_string(step, "loopId")
        .map(|value| sanitize_identifier(&value))
        .filter(|value| !value.is_empty())
    else {
        return (Vec::new(), depends_on_nodes);
    };
    let Some(max_iterations) = editor_u64(step, "maxIterations").filter(|value| *value > 0) else {
        return (Vec::new(), depends_on_nodes);
    };
    let body_steps = step
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let body = compile_editor_lane(&body_steps, Vec::new(), state);
    let node_id = format!("node_{loop_id}");
    let Some(until) = condition_from_repeat_step(step, state) else {
        return (Vec::new(), depends_on_nodes);
    };
    let exits = flat_step_depends_on(
        &body
            .nodes
            .iter()
            .filter_map(rendered_node_id)
            .collect::<Vec<_>>(),
        state,
    );
    state.flat_exits_by_node_id.insert(node_id.clone(), exits);
    (
        vec![RenderedNode::Repeat(RenderedRepeatNode {
            id: node_id.clone(),
            loop_id,
            depends_on: depends_on_nodes,
            depends_mode: step_string(step, "dependsMode").unwrap_or_else(|| "all".to_string()),
            until,
            max_iterations,
            body,
        })],
        vec![node_id],
    )
}

fn compile_editor_parallel_step(
    step: &Value,
    depends_on_nodes: Vec<String>,
    state: &mut RenderState<'_>,
) -> (Vec<RenderedNode>, Vec<String>) {
    let Some(_dispatch_mode) = explicit_editor_dispatch_mode(step) else {
        return (Vec::new(), depends_on_nodes);
    };
    let Some(collection_policy) = required_editor_collection_policy(step) else {
        return (Vec::new(), depends_on_nodes);
    };
    let collection_toml = collection_policy_toml_from_key(&collection_policy);
    let mut nodes = Vec::new();
    let mut exits = Vec::new();
    for branch in step
        .get("branches")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let branch_steps = branch
            .get("steps")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let result = compile_editor_lane(&branch_steps, depends_on_nodes.clone(), state);
        exits.extend(result.nodes.iter().filter_map(rendered_node_id));
        nodes.extend(result.nodes);
    }
    let Some(join_member) = control_member_for_step(step, state) else {
        return (nodes, exits);
    };
    let join = synthetic_rendered_step(
        state,
        &format!(
            "join_{}",
            step.get("id").and_then(Value::as_str).unwrap_or("parallel")
        ),
        &join_member,
        &format!("Join parallel branches ({collection_policy})."),
        exits.clone(),
        None,
        if collection_policy == "any" {
            "any"
        } else {
            "all"
        },
        Some(collection_toml),
        None,
        None,
    );
    let node_id = rendered_node_id(&join).unwrap_or_default();
    nodes.push(join);
    (nodes, vec![node_id])
}

fn compile_editor_branch_step(
    step: &Value,
    depends_on_nodes: Vec<String>,
    state: &mut RenderState<'_>,
) -> (Vec<RenderedNode>, Vec<String>) {
    let branch_id = format!(
        "branch_{}",
        sanitize_identifier(step.get("id").and_then(Value::as_str).unwrap_or("branch"))
    );
    let route_member = control_member_for_step(step, state);
    let join_member = control_member_for_step(step, state);
    if route_member.is_none() && join_member.is_none() {
        let mut nodes = Vec::new();
        let mut exits = Vec::new();
        let mut branch_conditions = Vec::new();
        for branch in step
            .get("branches")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let condition = condition_from_editor_branch(branch, state);
            if let Some(condition) = &condition {
                branch_conditions.push(condition.clone());
            }
            let branch_steps = branch
                .get("steps")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut result = compile_editor_lane(&branch_steps, depends_on_nodes.clone(), state);
            apply_branch_to_first_step(&mut result, state, &branch_id, condition);
            exits.extend(result.nodes.iter().filter_map(rendered_node_id));
            nodes.extend(result.nodes);
        }
        if let Some(fallback_steps) = step.get("fallback").and_then(Value::as_array) {
            if !fallback_steps.is_empty() {
                let mut result = compile_editor_lane(fallback_steps, depends_on_nodes, state);
                apply_branch_to_first_step(
                    &mut result,
                    state,
                    &branch_id,
                    fallback_condition_from_branch_conditions(&branch_conditions),
                );
                exits.extend(result.nodes.iter().filter_map(rendered_node_id));
                nodes.extend(result.nodes);
            }
        }
        return (nodes, exits);
    }

    let route_member = match route_member {
        Some(member) => member,
        None => return (Vec::new(), depends_on_nodes),
    };
    let mut nodes = Vec::new();
    let mut exits = Vec::new();
    let mut entry_nodes = Vec::new();
    let mut branch_conditions = Vec::new();
    for (index, branch) in step
        .get("branches")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let condition = condition_from_editor_branch(branch, state);
        if let Some(condition) = &condition {
            branch_conditions.push(condition.clone());
        }
        let label = branch
            .get("label")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("branch {}", index + 1));
        let entry = synthetic_rendered_step(
            state,
            &format!(
                "{}_{}",
                step.get("id").and_then(Value::as_str).unwrap_or("branch"),
                branch
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .unwrap_or_else(|| index.to_string())
            ),
            &route_member,
            &format!("Enter branch: {label}"),
            depends_on_nodes.clone(),
            None,
            "all",
            None,
            Some(branch_id.clone()),
            condition,
        );
        let entry_node_id = rendered_node_id(&entry).unwrap_or_default();
        entry_nodes.push(entry_node_id.clone());
        let branch_steps = branch
            .get("steps")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let result = compile_editor_lane(&branch_steps, vec![entry_node_id.clone()], state);
        exits.extend(
            result
                .nodes
                .iter()
                .filter_map(rendered_node_id)
                .collect::<Vec<_>>()
                .into_iter()
                .chain((result.nodes.is_empty()).then_some(entry_node_id.clone())),
        );
        nodes.push(entry);
        nodes.extend(result.nodes);
    }
    if let Some(fallback_steps) = step.get("fallback").and_then(Value::as_array) {
        if !fallback_steps.is_empty() {
            let fallback_entry = synthetic_rendered_step(
                state,
                &format!(
                    "{}_fallback",
                    step.get("id").and_then(Value::as_str).unwrap_or("branch")
                ),
                &route_member,
                "Enter branch: fallback",
                depends_on_nodes,
                None,
                "all",
                None,
                Some(branch_id.clone()),
                fallback_condition_from_branch_conditions(&branch_conditions),
            );
            let fallback_node_id = rendered_node_id(&fallback_entry).unwrap_or_default();
            entry_nodes.push(fallback_node_id.clone());
            let fallback_body =
                compile_editor_lane(fallback_steps, vec![fallback_node_id.clone()], state);
            exits.extend(
                fallback_body
                    .nodes
                    .iter()
                    .filter_map(rendered_node_id)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .chain((fallback_body.nodes.is_empty()).then_some(fallback_node_id.clone())),
            );
            nodes.push(fallback_entry);
            nodes.extend(fallback_body.nodes);
        }
    }
    let Some(join_member) = join_member else {
        return (nodes, exits);
    };
    let join = synthetic_rendered_step(
        state,
        &format!(
            "branch_join_{}",
            step.get("id").and_then(Value::as_str).unwrap_or("branch")
        ),
        &join_member,
        "Join branch paths.",
        exits.clone(),
        Some(entry_nodes),
        "any",
        Some("{ type = \"any\" }".to_string()),
        None,
        None,
    );
    let join_node_id = rendered_node_id(&join).unwrap_or_default();
    nodes.push(join);
    (nodes, vec![join_node_id])
}

#[allow(clippy::too_many_arguments)]
fn synthetic_rendered_step(
    state: &mut RenderState<'_>,
    _id: &str,
    member: &Value,
    message: &str,
    depends_on_nodes: Vec<String>,
    flat_depends_on_nodes: Option<Vec<String>>,
    depends_mode: &str,
    collection_policy: Option<String>,
    branch: Option<String>,
    condition: Option<String>,
) -> RenderedNode {
    let profile = editor_profile_name(member);
    let step_id = format!("{:02}_{}", state.next, sanitize_identifier(&profile));
    state.next += 1;
    let node_id = format!("node_{step_id}");
    let flat_source = flat_depends_on_nodes
        .as_ref()
        .unwrap_or(&depends_on_nodes)
        .clone();
    let rendered = RenderedStep {
        id: step_id.clone(),
        role: profile,
        message: message.to_string(),
        depends_on: flat_step_depends_on(&flat_source, state),
        depends_mode: depends_mode.to_string(),
        dispatch_mode: None,
        collection_policy,
        branch: branch.clone(),
        condition,
        timeout_ms: None,
        expected_schema_ref: None,
        allowed_tools: Vec::new(),
        blocked_tools: Vec::new(),
        output_format: None,
    };
    state.steps.push(rendered);
    state
        .flat_exits_by_node_id
        .insert(node_id.clone(), vec![step_id.clone()]);
    RenderedNode::Step(RenderedStepNode {
        id: node_id,
        step_id,
        depends_on: depends_on_nodes,
        depends_mode: depends_mode.to_string(),
        branch,
    })
}

fn rendered_node_id(node: &RenderedNode) -> Option<String> {
    match node {
        RenderedNode::Step(step) => Some(step.id.clone()),
        RenderedNode::Repeat(repeat) => Some(repeat.id.clone()),
    }
}

fn control_member_for_step(step: &Value, state: &RenderState<'_>) -> Option<Value> {
    let member_id = step
        .get("controllerRole")
        .or_else(|| step.get("controllerMemberId"))
        .or_else(|| step.get("controlRole"))
        .or_else(|| step.get("joinRole"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    state.members.get(member_id).map(|member| (*member).clone())
}

fn flat_step_depends_on(node_ids: &[String], state: &RenderState<'_>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for node_id in node_ids {
        let exits = state
            .flat_exits_by_node_id
            .get(node_id)
            .cloned()
            .unwrap_or_else(|| {
                node_id
                    .strip_prefix("node_")
                    .map(ToString::to_string)
                    .into_iter()
                    .collect()
            });
        for step_id in exits {
            if seen.insert(step_id.clone()) {
                out.push(step_id);
            }
        }
    }
    out
}

fn apply_branch_to_first_step(
    frame: &mut RenderedFrame,
    state: &mut RenderState<'_>,
    branch_id: &str,
    condition: Option<String>,
) {
    let Some(step_id) = first_rendered_step_node_id(frame) else {
        return;
    };
    if let Some(RenderedNode::Step(step_node)) = find_rendered_step_node_mut(frame, &step_id) {
        step_node.branch = Some(branch_id.to_string());
    }
    if let Some(step) = state.steps.iter_mut().find(|step| step.id == step_id) {
        step.branch = Some(branch_id.to_string());
        step.condition = condition;
    }
}

fn first_rendered_step_node_id(frame: &RenderedFrame) -> Option<String> {
    for node in &frame.nodes {
        match node {
            RenderedNode::Step(step) => return Some(step.step_id.clone()),
            RenderedNode::Repeat(repeat) => {
                if let Some(step_id) = first_rendered_step_node_id(&repeat.body) {
                    return Some(step_id);
                }
            }
        }
    }
    None
}

fn find_rendered_step_node_mut<'a>(
    frame: &'a mut RenderedFrame,
    step_id: &str,
) -> Option<&'a mut RenderedNode> {
    for node in &mut frame.nodes {
        match node {
            RenderedNode::Step(step) if step.step_id == step_id => return Some(node),
            RenderedNode::Step(_) => {}
            RenderedNode::Repeat(repeat) => {
                if let Some(node) = find_rendered_step_node_mut(&mut repeat.body, step_id) {
                    return Some(node);
                }
            }
        }
    }
    None
}

fn emit_rendered_wiring(lines: &mut Vec<String>, settings: &EditorMobSettings) {
    let role_wiring = settings.role_wiring();
    if settings.bool("autoWireOrchestrator") || !role_wiring.is_empty() {
        lines.push("[wiring]".to_string());
        lines.push(format!(
            "auto_wire_orchestrator = {}",
            if settings.bool("autoWireOrchestrator") {
                "true"
            } else {
                "false"
            }
        ));
        lines.push(String::new());
        for (a, b) in role_wiring {
            lines.push("[[wiring.role_wiring]]".to_string());
            lines.push(format!("a = {}", toml_string(&a)));
            lines.push(format!("b = {}", toml_string(&b)));
            lines.push(String::new());
        }
    }
}

fn emit_rendered_backend(lines: &mut Vec<String>, settings: &EditorMobSettings) {
    let backend_default = settings
        .string("backendDefault")
        .unwrap_or_else(|| "session".to_string());
    let external_base = settings.string("externalAddressBase").unwrap_or_default();
    if backend_default != "session" || !external_base.is_empty() {
        lines.push("[backend]".to_string());
        lines.push(format!("default = {}", toml_string(&backend_default)));
        lines.push(String::new());
        if !external_base.is_empty() {
            lines.push("[backend.external]".to_string());
            lines.push(format!("address_base = {}", toml_string(&external_base)));
            lines.push(String::new());
        }
    }
}

fn emit_rendered_advanced_mob_settings(lines: &mut Vec<String>, settings: &EditorMobSettings) {
    let Some(advanced) = settings.advanced_object() else {
        return;
    };
    for (editor_key, table) in [
        ("topology", "topology"),
        ("supervisor", "supervisor"),
        ("limits", "limits"),
        ("spawnPolicy", "spawn_policy"),
        ("eventRouter", "event_router"),
    ] {
        let value = advanced
            .get(editor_key)
            .or_else(|| advanced.get(&camel_to_snake(editor_key)));
        if let Some(Value::Object(map)) = value.filter(|value| !value.is_null()) {
            emit_json_toml(lines, table, &Value::Object(map.clone()));
        }
    }
}

fn emit_rendered_profile(
    lines: &mut Vec<String>,
    member: &Value,
    schemas: &BTreeMap<String, Value>,
) {
    let name = editor_profile_name(member);
    lines.push(format!("[profiles.{}]", toml_key(&name)));
    let realm_profile = member
        .get("realmProfile")
        .or_else(|| member.get("realm_profile"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let profile_binding = member
        .get("profileBinding")
        .or_else(|| member.get("profile_binding"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if profile_binding == "realm_profile" || realm_profile.is_some() {
        lines.push(format!(
            "realm_profile = {}",
            toml_string(realm_profile.unwrap_or(&name))
        ));
        lines.push(String::new());
        return;
    }
    lines.push(format!(
        "model = {}",
        toml_string(
            member
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
        )
    ));
    let skills = string_vec(member.get("skills"));
    if !skills.is_empty() {
        lines.push(format!("skills = {}", toml_array(&skills)));
    }
    if let Some(prompt) = member
        .get("systemPrompt")
        .or_else(|| member.get("system_prompt"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("peer_description = {}", toml_string(prompt)));
    }
    lines.push(format!(
        "external_addressable = {}",
        if member
            .get("externalAddressable")
            .or_else(|| member.get("external_addressable"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "true"
        } else {
            "false"
        }
    ));
    if let Some(backend) = editor_profile_backend(member) {
        lines.push(format!("backend = {}", toml_string(backend)));
    }
    lines.push(format!(
        "runtime_mode = {}",
        toml_string(
            member
                .get("runtimeMode")
                .or_else(|| member.get("runtime_mode"))
                .and_then(Value::as_str)
                .unwrap_or("turn_driven")
        )
    ));
    if let Some(max_inline) = editor_max_inline_peer_notifications(member) {
        lines.push(format!("max_inline_peer_notifications = {max_inline}"));
    }
    if let Some(provider_params) = member
        .get("providerParams")
        .or_else(|| member.get("provider_params"))
        .filter(|value| value.is_object())
    {
        lines.push(format!("provider_params = {}", toml_value(provider_params)));
    }
    lines.push(String::new());

    lines.push(format!("[profiles.{}.tools]", toml_key(&name)));
    let tool_config = editor_tool_config(member.get("tools"));
    for key in tool_config_bool_fields() {
        lines.push(format!(
            "{key} = {}",
            if tool_config.booleans.contains(key.as_str()) {
                "true"
            } else {
                "false"
            }
        ));
    }
    if !tool_config.mcp.is_empty() {
        lines.push(format!("mcp = {}", toml_array(&tool_config.mcp)));
    }
    if !tool_config.rust_bundles.is_empty() {
        lines.push(format!(
            "rust_bundles = {}",
            toml_array(&tool_config.rust_bundles)
        ));
    }
    lines.push(String::new());

    if let Some(schema_id) = member
        .get("schema")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && let Some(schema) = schemas.get(schema_id)
    {
        emit_json_toml(
            lines,
            &format!("profiles.{}.output_schema", toml_key(&name)),
            &editor_schema_to_json_schema(schema),
        );
    }
}

fn emit_rendered_flow_step(lines: &mut Vec<String>, step: &RenderedStep) {
    lines.push(format!("[flows.main.steps.{}]", toml_key(&step.id)));
    lines.push(format!("role = {}", toml_string(&step.role)));
    lines.push(format!("message = {}", toml_string(&step.message)));
    if !step.depends_on.is_empty() {
        lines.push(format!("depends_on = {}", toml_array(&step.depends_on)));
    }
    if step.depends_mode != "all" {
        lines.push(format!(
            "depends_on_mode = {}",
            toml_string(&step.depends_mode)
        ));
    }
    if let Some(dispatch_mode) = &step.dispatch_mode {
        lines.push(format!("dispatch_mode = {}", toml_string(dispatch_mode)));
    }
    if let Some(collection_policy) = &step.collection_policy {
        lines.push(format!("collection_policy = {collection_policy}"));
    }
    if let Some(branch) = &step.branch {
        lines.push(format!("branch = {}", toml_string(branch)));
    }
    if let Some(condition) = &step.condition {
        lines.push(format!("condition = {condition}"));
    }
    if let Some(timeout_ms) = step.timeout_ms {
        lines.push(format!("timeout_ms = {timeout_ms}"));
    }
    if let Some(schema_ref) = &step.expected_schema_ref {
        lines.push(format!("expected_schema_ref = {}", toml_string(schema_ref)));
    }
    if !step.allowed_tools.is_empty() {
        lines.push(format!(
            "allowed_tools = {}",
            toml_array(&step.allowed_tools)
        ));
    }
    if !step.blocked_tools.is_empty() {
        lines.push(format!(
            "blocked_tools = {}",
            toml_array(&step.blocked_tools)
        ));
    }
    if step
        .output_format
        .as_deref()
        .is_some_and(|format| format != "json")
    {
        lines.push(format!(
            "output_format = {}",
            toml_string(step.output_format.as_deref().unwrap_or("json"))
        ));
    }
    lines.push(String::new());
}

fn emit_rendered_frame_toml(
    lines: &mut Vec<String>,
    flow_id: &str,
    path: &str,
    frame: &RenderedFrame,
) {
    for node in &frame.nodes {
        match node {
            RenderedNode::Step(step) => {
                lines.push(format!(
                    "[flows.{flow_id}.{path}.nodes.{}]",
                    toml_key(&step.id)
                ));
                lines.push("kind = \"step\"".to_string());
                lines.push(format!("step_id = {}", toml_string(&step.step_id)));
                lines.push(format!("depends_on = {}", toml_array(&step.depends_on)));
                lines.push(format!(
                    "depends_on_mode = {}",
                    toml_string(&step.depends_mode)
                ));
                if let Some(branch) = &step.branch {
                    lines.push(format!("branch = {}", toml_string(branch)));
                }
                lines.push(String::new());
            }
            RenderedNode::Repeat(repeat) => {
                lines.push(format!(
                    "[flows.{flow_id}.{path}.nodes.{}]",
                    toml_key(&repeat.id)
                ));
                lines.push("kind = \"repeat_until\"".to_string());
                lines.push(format!("loop_id = {}", toml_string(&repeat.loop_id)));
                lines.push(format!("depends_on = {}", toml_array(&repeat.depends_on)));
                lines.push(format!(
                    "depends_on_mode = {}",
                    toml_string(&repeat.depends_mode)
                ));
                lines.push(format!("until = {}", repeat.until));
                lines.push(format!("max_iterations = {}", repeat.max_iterations));
                lines.push(String::new());
                emit_rendered_frame_toml(
                    lines,
                    flow_id,
                    &format!("{path}.nodes.{}.body", toml_key(&repeat.id)),
                    &repeat.body,
                );
            }
        }
    }
}

#[derive(Debug, Clone)]
struct EditorMobSettings {
    value: Value,
}

impl EditorMobSettings {
    fn string(&self, key: &str) -> Option<String> {
        self.value
            .get(key)
            .or_else(|| self.value.get(&camel_to_snake(key)))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    fn bool(&self, key: &str) -> bool {
        self.value
            .get(key)
            .or_else(|| self.value.get(&camel_to_snake(key)))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    fn role_wiring(&self) -> Vec<(String, String)> {
        self.value
            .get("roleWiring")
            .or_else(|| self.value.get("role_wiring"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|rule| {
                let a = rule.get("a").and_then(Value::as_str)?.trim();
                let b = rule.get("b").and_then(Value::as_str)?.trim();
                (!a.is_empty() && !b.is_empty()).then(|| (a.to_string(), b.to_string()))
            })
            .collect()
    }

    fn advanced_object(&self) -> Option<&serde_json::Map<String, Value>> {
        self.value
            .get("advanced")
            .filter(|value| !value.is_null())
            .and_then(Value::as_object)
    }
}

#[derive(Debug, Default)]
struct RenderedToolConfig {
    booleans: BTreeSet<String>,
    mcp: Vec<String>,
    rust_bundles: Vec<String>,
}

fn normalize_editor_mob_settings(value: &Value) -> EditorMobSettings {
    let mut settings = value.clone();
    if !settings.is_object() {
        settings = json!({});
    }
    if settings.get("backendDefault").is_none() && settings.get("backend_default").is_none() {
        settings["backendDefault"] = json!("session");
    }
    EditorMobSettings { value: settings }
}

fn input_param_summary_for_step(step: &Value) -> Option<String> {
    let params = step.get("inputParams").and_then(Value::as_array)?;
    let summary = input_param_summary(params);
    (!summary.is_empty()).then_some(summary)
}

fn editor_input_schema_files(document: &MobpackDocument) -> BTreeMap<String, Value> {
    let params = document
        .flow
        .get("steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|step| step.get("type").and_then(Value::as_str) == Some("input"))
        .and_then(|step| step.get("inputParams").and_then(Value::as_array));
    let Some(params) = params.filter(|params| !params.is_empty()) else {
        return BTreeMap::new();
    };
    BTreeMap::from([(
        "schemas/main-input.json".to_string(),
        editor_schema_to_json_schema(&json!({
            "id": "main-input",
            "description": "Activation parameters accepted by the main MobKit flow.",
            "fields": params,
        })),
    )])
}

fn step_string(step: &Value, key: &str) -> Option<String> {
    step.get(key)
        .or_else(|| step.get(&camel_to_snake(key)))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn string_vec(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect(),
        Some(Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn skill_source_from_realms(skill_id: &str, realms: &Value) -> Option<Value> {
    for realm in realms.as_array()? {
        for skill in realm.get("skills").and_then(Value::as_array)? {
            if skill.get("id").and_then(Value::as_str).map(str::trim) == Some(skill_id) {
                let mut source = skill.clone();
                if let Some(map) = source.as_object_mut() {
                    if !map.contains_key("realm_id")
                        && let Some(realm_id) = realm.get("id").and_then(Value::as_str)
                    {
                        map.insert("realm_id".to_string(), json!(realm_id));
                    }
                    if !map.contains_key("realm_source")
                        && let Some(realm_source) = realm.get("source").and_then(Value::as_str)
                    {
                        map.insert("realm_source".to_string(), json!(realm_source));
                    }
                }
                return Some(source);
            }
        }
    }
    None
}

fn editor_tool_config(value: Option<&Value>) -> RenderedToolConfig {
    let mut config = RenderedToolConfig::default();
    for tool in string_vec(value) {
        if let Some(source) = tool.strip_prefix("mcp:") {
            config.mcp.push(source.to_string());
        } else if let Some(bundle) = tool.strip_prefix("rust:") {
            config.rust_bundles.push(bundle.to_string());
        } else {
            config.booleans.insert(tool);
        }
    }
    config
}

fn collection_policy_toml_from_key(policy: &str) -> String {
    if policy == "any" {
        "{ type = \"any\" }".to_string()
    } else if let Some(n) = policy.strip_prefix("quorum:") {
        format!("{{ type = \"quorum\", n = {n} }}")
    } else {
        "{ type = \"all\" }".to_string()
    }
}

fn condition_from_repeat_step(step: &Value, state: &RenderState<'_>) -> Option<String> {
    let cond = step.get("cond")?;
    let field = cond
        .get("field")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let value = cond
        .get("val")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let op = cond
        .get("op")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let cond_source_is_params = cond
        .get("namespace")
        .and_then(Value::as_str)
        .is_some_and(|namespace| namespace == "params")
        || cond
            .get("stepId")
            .and_then(Value::as_str)
            .is_some_and(|step_id| step_id == "params");
    let path = if cond_source_is_params {
        format!("params.{field}")
    } else {
        let step_id = cond
            .get("stepId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|visual_id| {
                state
                    .step_ids_by_visual_id
                    .get(visual_id)
                    .cloned()
                    .or_else(|| Some(visual_id.to_string()))
            })?;
        if step_id == "params" {
            format!("params.{field}")
        } else {
            format!("steps.{step_id}.{field}")
        }
    };
    condition_expr_for_operator(op, &path, value)
        .or_else(|| condition_expr_for_operator("==", &path, value))
}

fn condition_from_editor_branch(branch: &Value, state: &RenderState<'_>) -> Option<String> {
    branch
        .get("cond")
        .and_then(|cond| condition_from_editor_cond(cond, state))
        .or_else(|| {
            branch
                .get("condition")
                .and_then(Value::as_str)
                .and_then(condition_from_text)
        })
}

fn condition_from_editor_cond(cond: &Value, state: &RenderState<'_>) -> Option<String> {
    let field = cond
        .get("field")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let value = cond
        .get("val")
        .or_else(|| cond.get("value"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let op = cond
        .get("op")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let source_is_params = cond
        .get("namespace")
        .and_then(Value::as_str)
        .is_some_and(|namespace| namespace == "params")
        || cond
            .get("stepId")
            .and_then(Value::as_str)
            .is_some_and(|step_id| step_id == "params");
    let path = if source_is_params {
        format!("params.{field}")
    } else {
        let step_id = cond
            .get("stepId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let compiled_step_id = state
            .step_ids_by_visual_id
            .get(step_id)
            .cloned()
            .unwrap_or_else(|| step_id.to_string());
        format!("steps.{compiled_step_id}.{field}")
    };
    condition_expr_for_operator(op, &path, value)
}

fn condition_from_text(text: &str) -> Option<String> {
    let raw = text.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('{') && raw.contains("op") {
        return Some(raw.to_string());
    }
    if raw.contains("&&") {
        let exprs = raw
            .split("&&")
            .map(condition_from_text)
            .collect::<Option<Vec<_>>>()?;
        return Some(format!(
            "{{ op = \"and\", exprs = [{}] }}",
            exprs.join(", ")
        ));
    }
    if raw.contains("||") {
        let exprs = raw
            .split("||")
            .map(condition_from_text)
            .collect::<Option<Vec<_>>>()?;
        return Some(format!("{{ op = \"or\", exprs = [{}] }}", exprs.join(", ")));
    }
    for op in ["==", ">", "<"] {
        if let Some((path, value)) = raw.split_once(op) {
            let value = value.trim().trim_matches(['"', '\'']);
            return condition_expr_for_operator(op, path.trim(), value);
        }
    }
    None
}

fn fallback_condition_from_branch_conditions(conditions: &[String]) -> Option<String> {
    let exprs = conditions
        .iter()
        .filter_map(|condition| condition_from_text(condition))
        .collect::<Vec<_>>();
    if exprs.is_empty() {
        None
    } else if exprs.len() == 1 {
        Some(format!("{{ op = \"not\", expr = {} }}", exprs[0]))
    } else {
        Some(format!(
            "{{ op = \"not\", expr = {{ op = \"or\", exprs = [{}] }} }}",
            exprs.join(", ")
        ))
    }
}

fn condition_expr_for_operator(op: &str, path: &str, value: &str) -> Option<String> {
    let op = match op {
        "==" => "eq",
        ">" => "gt",
        "<" => "lt",
        _ => return None,
    };
    Some(format!(
        "{{ op = \"{op}\", path = {}, value = {} }}",
        toml_string(path),
        toml_string(value)
    ))
}

fn toml_key(value: &str) -> String {
    let mut chars = value.chars();
    let valid_first = chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_');
    let valid_rest = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
    if valid_first && valid_rest {
        value.to_string()
    } else {
        toml_string(value)
    }
}

fn toml_string(value: &str) -> String {
    format!("\"{}\"", escape_toml_string(value))
}

fn toml_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| toml_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn toml_value(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => toml_string(value),
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(toml_value).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(map) => format!(
            "{{ {} }}",
            map.iter()
                .map(|(key, value)| format!("{} = {}", toml_key(key), toml_value(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Null => "\"\"".to_string(),
    }
}

fn emit_json_toml(lines: &mut Vec<String>, table: &str, value: &Value) {
    let Some(map) = value.as_object() else {
        return;
    };
    lines.push(format!("[{table}]"));
    for (key, val) in map {
        if val.is_object() {
            continue;
        }
        lines.push(format!("{} = {}", toml_key(key), toml_value(val)));
    }
    lines.push(String::new());
    for (key, val) in map {
        if val.is_object() {
            emit_json_toml(lines, &format!("{table}.{}", toml_key(key)), val);
        }
    }
}

fn validate_document(document: &MobpackDocument) -> MobpackValidationResult {
    let mut diagnostics = Vec::new();
    if document.schema_version.trim() != MOBPACK_SCHEMA_VERSION {
        diagnostics.push(MobpackDiagnostic {
            severity: "warning".to_string(),
            code: "schema_version_mismatch".to_string(),
            message: format!(
                "mobpack schema_version '{}' differs from supported '{}'",
                document.schema_version, MOBPACK_SCHEMA_VERSION
            ),
            path: Some("schema_version".to_string()),
        });
    }
    if document.name.trim().is_empty() {
        diagnostics.push(MobpackDiagnostic {
            severity: "warning".to_string(),
            code: "missing_name".to_string(),
            message: "mobpack document has no display name".to_string(),
            path: Some("name".to_string()),
        });
    }
    diagnostics.extend(validate_launch_modes(document));
    diagnostics.extend(validate_deploy_settings(&document.deploy));
    diagnostics.extend(validate_deploy_runtime_modes(document));
    diagnostics.extend(validate_editor_schemas(&document.schemas));
    diagnostics.extend(validate_editor_input_params(&document.flow));
    diagnostics.extend(validate_editor_flow_step_identities(&document.flow));
    diagnostics.extend(validate_editor_flow_step_types(&document.flow));
    diagnostics.extend(validate_editor_flow_step_metadata(&document.flow));
    diagnostics.extend(validate_editor_flow_member_roles(document));
    diagnostics.extend(validate_editor_flow_control_members(document));
    diagnostics.extend(validate_editor_flow_conditions(&document.flow));
    diagnostics.extend(validate_graph_projection(document));
    diagnostics.extend(validate_editor_member_identities(document));
    diagnostics.extend(validate_member_profile_bindings(document));
    diagnostics.extend(validate_skill_realms(document));
    diagnostics.extend(validate_member_catalog_references(document));
    diagnostics.extend(validate_editor_flow_step_tool_references(document));
    diagnostics.extend(validate_selected_skill_sources(document));
    diagnostics.extend(validate_selected_path_skill_files(document));

    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "error")
    {
        return invalid_validation_result(diagnostics);
    }

    let mob_toml = match authoring_mob_toml(document) {
        Ok(toml) => toml,
        Err(err) => {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "render_mob_toml_failed".to_string(),
                message: err,
                path: Some("mob_toml".to_string()),
            });
            return invalid_validation_result(diagnostics);
        }
    };

    match meerkat_mob::MobDefinition::from_toml(&mob_toml) {
        Ok(definition) => {
            diagnostics.extend(
                meerkat_mob::validate_definition(&definition)
                    .into_iter()
                    .chain(meerkat_mob::SpecValidator::validate(&definition))
                    .map(|diagnostic| MobpackDiagnostic {
                        severity: format!("{:?}", diagnostic.severity).to_ascii_lowercase(),
                        code: diagnostic.code.to_string(),
                        message: diagnostic.message,
                        path: diagnostic.location,
                    }),
            );
            diagnostics.extend(validate_editor_projection_matches_definition(
                document,
                &definition,
            ));
            diagnostics.extend(validate_mob_settings_match_definition(
                document,
                &definition,
            ));
            diagnostics.extend(validate_editor_flow_members_match_definition(
                document,
                &definition,
            ));
            diagnostics.extend(validate_editor_flow_step_metadata_matches_definition(
                document,
                &definition,
            ));
            if let Err(schema_diagnostics) = expected_schema_files_from_definition(&definition) {
                diagnostics.extend(schema_diagnostics);
            }
            let flow_ids = definition.flows.keys().map(ToString::to_string).collect();
            let ok = !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == "error");
            MobpackValidationResult {
                ok,
                display_rows: validation_display_rows(ok, &diagnostics, MOBPACK_VALIDATION_SOURCE),
                diagnostics,
                mob_id: Some(definition.id.to_string()),
                flow_ids,
                validation_source: MOBPACK_VALIDATION_SOURCE.to_string(),
                deploy_command: "rkat mob deploy <pack.mobpack> <prompt>".to_string(),
            }
        }
        Err(err) => {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_mob_toml".to_string(),
                message: err.to_string(),
                path: Some("mob_toml".to_string()),
            });
            invalid_validation_result(diagnostics)
        }
    }
}

fn invalid_validation_result(diagnostics: Vec<MobpackDiagnostic>) -> MobpackValidationResult {
    MobpackValidationResult {
        ok: false,
        display_rows: validation_display_rows(false, &diagnostics, MOBPACK_VALIDATION_SOURCE),
        diagnostics,
        mob_id: None,
        flow_ids: Vec::new(),
        validation_source: MOBPACK_VALIDATION_SOURCE.to_string(),
        deploy_command: "rkat mob deploy <pack.mobpack> <prompt>".to_string(),
    }
}

fn validation_display_rows(
    ok: bool,
    diagnostics: &[MobpackDiagnostic],
    validation_source: &str,
) -> Vec<MobpackDisplayRow> {
    let mut rows = diagnostics
        .iter()
        .map(|diagnostic| {
            let is_error = diagnostic.severity == "error";
            MobpackDisplayRow {
                kind: if is_error { "crit" } else { "warn" }.to_string(),
                glyph: if is_error { "!" } else { "△" }.to_string(),
                head: if diagnostic.code.is_empty() {
                    diagnostic.severity.clone()
                } else {
                    diagnostic.code.clone()
                },
                sub: diagnostic.message.clone(),
                meta: diagnostic.path.clone().unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    if rows.is_empty() && ok {
        rows.push(MobpackDisplayRow {
            kind: "ok".to_string(),
            glyph: "✓".to_string(),
            head: "MobKit mobpack validates".to_string(),
            sub: validation_source.to_string(),
            meta: "rkat mob validate".to_string(),
        });
    } else if rows.is_empty() {
        rows.push(MobpackDisplayRow {
            kind: "crit".to_string(),
            glyph: "!".to_string(),
            head: "MobKit validation failed".to_string(),
            sub: "The validation endpoint returned ok=false without diagnostics.".to_string(),
            meta: validation_source.to_string(),
        });
    }
    rows
}

fn validate_launch_modes(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    if document.launch_modes.is_null() {
        return Vec::new();
    }
    let Some(items) = document.launch_modes.as_array() else {
        return vec![MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_launch_modes".to_string(),
            message: "document.launch_modes must be an array".to_string(),
            path: Some("launch_modes".to_string()),
        }];
    };
    let member_ids = editor_member_ids(document);
    let step_ids = editor_flow_member_step_ids(&document.flow);
    let instance_ids = editor_instance_ids(&document.instances);
    let allowed_sources = step_ids
        .iter()
        .chain(instance_ids.iter())
        .chain(member_ids.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let profile_by_member = editor_profile_by_member_id(document);
    let flow_launch_modes = editor_flow_member_launch_modes(&document.flow);
    let mut launch_step_ids = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let path = format!("launch_modes[{index}].launch_mode");
        let step_id = item
            .get("step_id")
            .or_else(|| item.get("stepId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(step_id) = step_id {
            if !launch_step_ids.insert(step_id.to_string()) {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "duplicate_launch_step".to_string(),
                    message: format!(
                        "launch mode entry duplicates step '{step_id}', but each editor member step must have exactly one launch mode"
                    ),
                    path: Some(format!("launch_modes[{index}].step_id")),
                });
            }
            if !step_ids.is_empty() && !step_ids.contains(step_id) {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "unknown_launch_step".to_string(),
                    message: format!(
                        "launch mode references step '{step_id}', but the editor flow has no matching member step"
                    ),
                    path: Some(format!("launch_modes[{index}].step_id")),
                });
            }
        } else if !step_ids.is_empty() {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_launch_step".to_string(),
                message:
                    "launch mode entry must include step_id when editor flow steps are present"
                        .to_string(),
                path: Some(format!("launch_modes[{index}].step_id")),
            });
        }
        let member_id = item
            .get("member_id")
            .or_else(|| item.get("memberId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(member_id) = member_id {
            if !member_ids.is_empty() && !member_ids.contains(member_id) {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "unknown_launch_member".to_string(),
                    message: format!(
                        "launch mode references member '{member_id}', but document.members has no matching member"
                    ),
                    path: Some(format!("launch_modes[{index}].member_id")),
                });
            }
            if let Some(profile) = item
                .get("profile")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                && profile_by_member
                    .get(member_id)
                    .is_some_and(|expected| expected != profile)
            {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "launch_profile_mismatch".to_string(),
                    message: format!(
                        "launch mode profile '{profile}' does not match member '{member_id}'"
                    ),
                    path: Some(format!("launch_modes[{index}].profile")),
                });
            }
        } else if !member_ids.is_empty() {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_launch_member".to_string(),
                message:
                    "launch mode entry must include member_id when document.members are present"
                        .to_string(),
                path: Some(format!("launch_modes[{index}].member_id")),
            });
        }
        let Some(mode) = item
            .get("launch_mode")
            .or_else(|| item.get("launchMode"))
            .filter(|mode| mode.is_object())
        else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_launch_mode".to_string(),
                message: "launch mode entry must include launch_mode".to_string(),
                path: Some(path),
            });
            continue;
        };
        if let Some(step_id) = step_id
            && let Some(expected) = flow_launch_modes.get(step_id)
        {
            let actual = editor_launch_mode_entry_mode(item, mode);
            if &actual != expected {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "launch_mode_flow_mismatch".to_string(),
                    message: format!(
                        "launch mode entry '{actual:?}' does not match flow step launch mode '{expected:?}'"
                    ),
                    path: Some(path.clone()),
                });
            }
        }
        let Some(kind) = mode
            .get("kind")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_launch_mode_kind".to_string(),
                message: "launch mode entry must include launch_mode.kind".to_string(),
                path: Some(path),
            });
            continue;
        };
        let kind = kind.to_ascii_lowercase();
        match kind.as_str() {
            "fresh" => {}
            "resume" => {
                let has_session = mode
                    .get("sessionId")
                    .or_else(|| mode.get("session_id"))
                    .or_else(|| mode.get("bridgeSessionId"))
                    .or_else(|| mode.get("bridge_session_id"))
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty());
                if !has_session {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "resume_launch_missing_session".to_string(),
                        message: "Resume launch mode requires a bridge session id".to_string(),
                        path: Some(path),
                    });
                }
            }
            "fork" => {
                let source = mode
                    .get("from")
                    .or_else(|| mode.get("sourceMemberId"))
                    .or_else(|| mode.get("source_member_id"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if source.is_none() {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "fork_launch_missing_source".to_string(),
                        message: "Fork launch mode requires a source member or instance id"
                            .to_string(),
                        path: Some(path.clone()),
                    });
                }
                if let Some(source) = source
                    && !allowed_sources.is_empty()
                    && !allowed_sources.contains(source)
                {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "fork_launch_unknown_source".to_string(),
                        message: format!(
                            "Fork launch source '{source}' does not match any editor step, instance, or member"
                        ),
                        path: Some(path.clone()),
                    });
                }
                if let Some(context) = mode
                    .get("context")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    && !matches!(
                        canonical_fork_context_value(context).as_str(),
                        "full_history" | "last_messages"
                    )
                {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "invalid_fork_launch_context".to_string(),
                        message: format!("unsupported Fork launch context '{context}'"),
                        path: Some(path),
                    });
                }
            }
            _ => diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_launch_mode_kind".to_string(),
                message: format!("unsupported launch mode kind '{kind}'"),
                path: Some(path),
            }),
        }
        if let Some(policy) = item
            .get("budget_split_policy")
            .or_else(|| item.get("budgetSplitPolicy"))
            .or_else(|| mode.get("budget_split_policy"))
            .or_else(|| mode.get("budgetSplitPolicy"))
            .or_else(|| mode.get("budget"))
        {
            validate_budget_split_policy(
                policy,
                format!("launch_modes[{index}].budget_split_policy"),
                &mut diagnostics,
            );
        }
    }
    if !step_ids.is_empty() {
        for step_id in step_ids.difference(&launch_step_ids) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_step_launch_mode".to_string(),
                message: format!("editor member step '{step_id}' has no launch mode entry"),
                path: Some("launch_modes".to_string()),
            });
        }
    }
    diagnostics
}

fn validate_budget_split_policy(
    policy: &Value,
    path: String,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(object) = policy.as_object() else {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_budget_split_policy".to_string(),
            message: "budget_split_policy must be an object".to_string(),
            path: Some(path),
        });
        return;
    };
    let kind = object
        .get("type")
        .or_else(|| object.get("kind"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let Some(kind) = kind else {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_budget_split_policy".to_string(),
            message: "budget_split_policy must declare a non-empty type".to_string(),
            path: Some(path),
        });
        return;
    };
    match kind.as_str() {
        "equal" | "proportional" | "remaining" => {}
        "fixed" => {
            let valid_value = object
                .get("value")
                .or_else(|| object.get("limit"))
                .or_else(|| object.get("tokens"))
                .and_then(Value::as_u64)
                .is_some_and(|value| value > 0);
            if !valid_value {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_budget_split_policy".to_string(),
                    message: "fixed budget_split_policy requires a positive integer value"
                        .to_string(),
                    path: Some(path),
                });
            }
        }
        _ => diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_budget_split_policy".to_string(),
            message: format!("unsupported budget_split_policy type '{kind}'"),
            path: Some(path),
        }),
    }
}

fn validate_editor_flow_step_types(flow: &Value) -> Vec<MobpackDiagnostic> {
    let mut diagnostics = Vec::new();
    collect_editor_flow_step_type_diagnostics(
        flow.get("steps").and_then(Value::as_array),
        "flow.steps",
        &mut diagnostics,
    );
    diagnostics
}

fn collect_editor_flow_step_type_diagnostics(
    steps: Option<&Vec<Value>>,
    path: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(steps) = steps else {
        return;
    };
    for (index, step) in steps.iter().enumerate() {
        let step_path = format!("{path}[{index}]");
        let Some(step_type) = step.get("type").and_then(Value::as_str) else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_editor_flow_step_type".to_string(),
                message: "editor flow steps must include a type".to_string(),
                path: Some(format!("{step_path}.type")),
            });
            continue;
        };
        if !EDITOR_FLOW_STEP_TYPES.contains(&step_type) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "unsupported_editor_flow_step_type".to_string(),
                message: format!(
                    "editor flow step type '{step_type}' is not backed by current MobKit mob.toml semantics"
                ),
                path: Some(format!("{step_path}.type")),
            });
        }
        collect_editor_flow_step_type_diagnostics(
            step.get("steps").and_then(Value::as_array),
            &format!("{step_path}.steps"),
            diagnostics,
        );
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for (branch_index, branch) in branches.iter().enumerate() {
                collect_editor_flow_step_type_diagnostics(
                    branch.get("steps").and_then(Value::as_array),
                    &format!("{step_path}.branches[{branch_index}].steps"),
                    diagnostics,
                );
            }
        }
        collect_editor_flow_step_type_diagnostics(
            step.get("fallback").and_then(Value::as_array),
            &format!("{step_path}.fallback"),
            diagnostics,
        );
    }
}

fn validate_editor_flow_step_metadata(flow: &Value) -> Vec<MobpackDiagnostic> {
    let mut diagnostics = Vec::new();
    collect_editor_flow_step_metadata_diagnostics(
        flow.get("steps").and_then(Value::as_array),
        "flow.steps",
        &mut diagnostics,
    );
    diagnostics
}

fn collect_editor_flow_step_metadata_diagnostics(
    steps: Option<&Vec<Value>>,
    path: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(steps) = steps else {
        return;
    };
    for (index, step) in steps.iter().enumerate() {
        let step_path = format!("{path}[{index}]");
        let step_type = step.get("type").and_then(Value::as_str).unwrap_or_default();

        validate_optional_enum_field(
            step,
            "dependsMode",
            "depends_mode",
            &dependency_mode_values(),
            "invalid_editor_dependency_mode",
            "dependency mode",
            &step_path,
            diagnostics,
        );

        if step_type == "parallel" {
            validate_required_parallel_dispatch_field(step, &step_path, diagnostics);
            validate_required_parallel_collection_field(step, &step_path, diagnostics);
        }

        if step_type == "repeat" {
            validate_required_repeat_loop_id_field(step, &step_path, diagnostics);
            validate_required_repeat_max_iterations_field(step, &step_path, diagnostics);
        }

        if step_type == "member" {
            validate_optional_enum_field(
                step,
                "dispatchMode",
                "dispatch_mode",
                &dispatch_mode_values(),
                "invalid_editor_dispatch_mode",
                "dispatch mode",
                &step_path,
                diagnostics,
            );
            validate_collection_policy_field(step, &step_path, diagnostics);
        }

        if step_type == "member" {
            validate_optional_enum_field(
                step,
                "outputFormat",
                "output_format",
                &step_output_format_values(),
                "invalid_editor_output_format",
                "output format",
                &step_path,
                diagnostics,
            );
            validate_optional_positive_integer_field(
                step,
                "timeoutMs",
                "timeout_ms",
                "invalid_editor_timeout",
                "timeout",
                &step_path,
                diagnostics,
            );
        }

        collect_editor_flow_step_metadata_diagnostics(
            step.get("steps").and_then(Value::as_array),
            &format!("{step_path}.steps"),
            diagnostics,
        );
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for (branch_index, branch) in branches.iter().enumerate() {
                collect_editor_flow_step_metadata_diagnostics(
                    branch.get("steps").and_then(Value::as_array),
                    &format!("{step_path}.branches[{branch_index}].steps"),
                    diagnostics,
                );
            }
        }
        collect_editor_flow_step_metadata_diagnostics(
            step.get("fallback").and_then(Value::as_array),
            &format!("{step_path}.fallback"),
            diagnostics,
        );
    }
}

fn validate_required_parallel_dispatch_field(
    step: &Value,
    step_path: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(value) = step
        .get("dispatch")
        .or_else(|| step.get("dispatchMode"))
        .or_else(|| step.get("dispatch_mode"))
    else {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "missing_parallel_dispatch_mode".to_string(),
            message: "parallel editor steps must declare an explicit dispatch mode".to_string(),
            path: Some(format!("{step_path}.dispatch")),
        });
        return;
    };
    match value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(text) if dispatch_mode_values().iter().any(|allowed| allowed == text) => {}
        Some(text) => diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_editor_dispatch_mode".to_string(),
            message: format!(
                "editor dispatch mode '{text}' is not backed by current MobKit mob.toml semantics"
            ),
            path: Some(format!("{step_path}.dispatch")),
        }),
        None => diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_editor_dispatch_mode".to_string(),
            message: "editor dispatch mode must be a non-empty string when present".to_string(),
            path: Some(format!("{step_path}.dispatch")),
        }),
    }
}

fn validate_required_parallel_collection_field(
    step: &Value,
    step_path: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    if step
        .get("collection")
        .or_else(|| step.get("collectionPolicy"))
        .or_else(|| step.get("collection_policy"))
        .is_none()
    {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "missing_parallel_collection_policy".to_string(),
            message: "parallel editor steps must declare an explicit collection policy".to_string(),
            path: Some(format!("{step_path}.collection")),
        });
        return;
    }
    validate_collection_policy_field(step, step_path, diagnostics);
}

fn validate_required_repeat_loop_id_field(
    step: &Value,
    step_path: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    if step_string(step, "loopId").is_some() {
        return;
    }
    diagnostics.push(MobpackDiagnostic {
        severity: "error".to_string(),
        code: "missing_repeat_loop_id".to_string(),
        message: "repeat editor steps must declare an explicit non-empty loop_id".to_string(),
        path: Some(format!("{step_path}.loopId")),
    });
}

fn validate_required_repeat_max_iterations_field(
    step: &Value,
    step_path: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(value) = step
        .get("maxIterations")
        .or_else(|| step.get("max_iterations"))
    else {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "missing_repeat_max_iterations".to_string(),
            message: "repeat editor steps must declare explicit max_iterations".to_string(),
            path: Some(format!("{step_path}.maxIterations")),
        });
        return;
    };
    if value.as_u64().is_some_and(|number| number > 0) {
        return;
    }
    diagnostics.push(MobpackDiagnostic {
        severity: "error".to_string(),
        code: "invalid_repeat_max_iterations".to_string(),
        message: "repeat editor max_iterations must be a positive integer".to_string(),
        path: Some(format!("{step_path}.maxIterations")),
    });
}

fn validate_optional_enum_field(
    object: &Value,
    camel_key: &str,
    snake_key: &str,
    allowed: &[String],
    code: &str,
    label: &str,
    path: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(value) = object.get(camel_key).or_else(|| object.get(snake_key)) else {
        return;
    };
    if value.is_null() {
        return;
    }
    match value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(text) if allowed.iter().any(|allowed| allowed == text) => {}
        Some(text) => diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: code.to_string(),
            message: format!(
                "editor {label} '{text}' is not backed by current MobKit mob.toml semantics"
            ),
            path: Some(format!("{path}.{camel_key}")),
        }),
        None => diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: code.to_string(),
            message: format!("editor {label} must be a non-empty string when present"),
            path: Some(format!("{path}.{camel_key}")),
        }),
    }
}

fn validate_optional_positive_integer_field(
    object: &Value,
    camel_key: &str,
    snake_key: &str,
    code: &str,
    label: &str,
    path: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(value) = object.get(camel_key).or_else(|| object.get(snake_key)) else {
        return;
    };
    if value.is_null() {
        return;
    }
    if value.as_u64().is_some_and(|number| number > 0) {
        return;
    }
    diagnostics.push(MobpackDiagnostic {
        severity: "error".to_string(),
        code: code.to_string(),
        message: format!("editor {label} must be a positive integer when present"),
        path: Some(format!("{path}.{camel_key}")),
    });
}

fn validate_collection_policy_field(
    step: &Value,
    step_path: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(value) = step
        .get("collection")
        .or_else(|| step.get("collectionPolicy"))
        .or_else(|| step.get("collection_policy"))
    else {
        return;
    };
    let policy = match value {
        Value::String(text) => text.trim(),
        Value::Object(map) => map
            .get("type")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("all"),
        _ => "",
    };
    if !collection_policy_values()
        .iter()
        .any(|allowed| allowed == policy)
    {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_editor_collection_policy".to_string(),
            message: format!(
                "editor collection policy '{policy}' is not backed by current MobKit mob.toml semantics"
            ),
            path: Some(format!("{step_path}.collection")),
        });
        return;
    }
    if policy != "quorum" {
        return;
    }
    let quorum = match value {
        Value::Object(map) => map.get("n").or_else(|| map.get("quorum")),
        _ => step.get("quorum").or_else(|| step.get("collectionQuorum")),
    };
    match quorum {
        Some(Value::Null) | None => {}
        Some(value) if value.as_u64().is_some_and(|number| number > 0) => {}
        Some(_) => diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_editor_collection_quorum".to_string(),
            message: "editor quorum collection must use a positive integer threshold".to_string(),
            path: Some(format!("{step_path}.quorum")),
        }),
    }
}

fn validate_editor_flow_member_roles(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    let mut diagnostics = Vec::new();
    let member_ids = editor_member_ids(document);
    collect_editor_flow_member_role_diagnostics(
        document.flow.get("steps").and_then(Value::as_array),
        "flow.steps",
        &member_ids,
        &mut diagnostics,
    );
    diagnostics
}

fn validate_editor_flow_step_identities(flow: &Value) -> Vec<MobpackDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen = BTreeSet::new();
    collect_editor_flow_step_identity_diagnostics(
        flow.get("steps").and_then(Value::as_array),
        "flow.steps",
        &mut seen,
        &mut diagnostics,
    );
    diagnostics
}

fn collect_editor_flow_step_identity_diagnostics(
    steps: Option<&Vec<Value>>,
    path: &str,
    seen: &mut BTreeSet<String>,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(steps) = steps else {
        return;
    };
    for (index, step) in steps.iter().enumerate() {
        let step_path = format!("{path}[{index}]");
        let Some(step_id) = step
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_editor_flow_step_id".to_string(),
                message: "editor flow steps must have a non-empty id".to_string(),
                path: Some(format!("{step_path}.id")),
            });
            continue;
        };
        if !seen.insert(step_id.to_string()) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "duplicate_editor_flow_step_id".to_string(),
                message: format!("editor flow step id '{step_id}' is used more than once"),
                path: Some(format!("{step_path}.id")),
            });
        }
        collect_editor_flow_step_identity_diagnostics(
            step.get("steps").and_then(Value::as_array),
            &format!("{step_path}.steps"),
            seen,
            diagnostics,
        );
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for (branch_index, branch) in branches.iter().enumerate() {
                collect_editor_flow_step_identity_diagnostics(
                    branch.get("steps").and_then(Value::as_array),
                    &format!("{step_path}.branches[{branch_index}].steps"),
                    seen,
                    diagnostics,
                );
            }
        }
        collect_editor_flow_step_identity_diagnostics(
            step.get("fallback").and_then(Value::as_array),
            &format!("{step_path}.fallback"),
            seen,
            diagnostics,
        );
    }
}

fn collect_editor_flow_member_role_diagnostics(
    steps: Option<&Vec<Value>>,
    path: &str,
    member_ids: &BTreeSet<String>,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(steps) = steps else {
        return;
    };
    for (index, step) in steps.iter().enumerate() {
        let step_path = format!("{path}[{index}]");
        let step_type = step.get("type").and_then(Value::as_str).unwrap_or_default();
        if step_type == "member" {
            if editor_step_instruction(step).is_none() {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "missing_flow_member_instruction".to_string(),
                    message: "flow member step must include an explicit non-empty instruction"
                        .to_string(),
                    path: Some(format!("{step_path}.instruction")),
                });
            }
            match step
                .get("role")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(role) if !member_ids.contains(role) => {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "unknown_flow_member".to_string(),
                        message: format!(
                            "flow member step references member '{role}', but document.members has no matching id"
                        ),
                        path: Some(format!("{step_path}.role")),
                    });
                }
                Some(_) => {}
                None => diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "missing_flow_member".to_string(),
                    message: "flow member step must reference a real document.members id"
                        .to_string(),
                    path: Some(format!("{step_path}.role")),
                }),
            }
        }
        collect_editor_flow_member_role_diagnostics(
            step.get("steps").and_then(Value::as_array),
            &format!("{step_path}.steps"),
            member_ids,
            diagnostics,
        );
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for (branch_index, branch) in branches.iter().enumerate() {
                collect_editor_flow_member_role_diagnostics(
                    branch.get("steps").and_then(Value::as_array),
                    &format!("{step_path}.branches[{branch_index}].steps"),
                    member_ids,
                    diagnostics,
                );
            }
        }
        collect_editor_flow_member_role_diagnostics(
            step.get("fallback").and_then(Value::as_array),
            &format!("{step_path}.fallback"),
            member_ids,
            diagnostics,
        );
    }
}

fn validate_editor_flow_control_members(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    let mut diagnostics = Vec::new();
    let member_ids = editor_member_ids(document);
    collect_editor_flow_control_member_diagnostics(
        document.flow.get("steps").and_then(Value::as_array),
        "flow.steps",
        &member_ids,
        &mut diagnostics,
    );
    diagnostics
}

fn collect_editor_flow_control_member_diagnostics(
    steps: Option<&Vec<Value>>,
    path: &str,
    member_ids: &BTreeSet<String>,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(steps) = steps else {
        return;
    };
    for (index, step) in steps.iter().enumerate() {
        let step_path = format!("{path}[{index}]");
        let step_type = step.get("type").and_then(Value::as_str).unwrap_or_default();
        let controller_role = step
            .get("controllerRole")
            .or_else(|| step.get("controllerMemberId"))
            .or_else(|| step.get("controlRole"))
            .or_else(|| step.get("joinRole"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(controller_role) = controller_role
            && !member_ids.is_empty()
            && !member_ids.contains(controller_role)
        {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "unknown_flow_control_member".to_string(),
                message: format!(
                    "flow {step_type} control member '{controller_role}' is not declared in document.members"
                ),
                path: Some(format!("{step_path}.controllerRole")),
            });
        }
        if step_type == "branch"
            && controller_role.is_none()
            && editor_branch_requires_join_member(step)
        {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_branch_join_member".to_string(),
                message:
                    "branch convergence requires a real Join member declared in document.members"
                        .to_string(),
                path: Some(format!("{step_path}.controllerRole")),
            });
        }
        if step_type == "parallel" && controller_role.is_none() {
            let collection = step
                .get("collection")
                .or_else(|| step.get("collectionPolicy"))
                .or_else(|| step.get("collection_policy"))
                .and_then(|value| match value {
                    Value::String(text) => Some(text.as_str()),
                    Value::Object(map) => map.get("type").and_then(Value::as_str),
                    _ => None,
                })
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("all");
            if matches!(collection, "any" | "quorum") {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "missing_parallel_join_member".to_string(),
                    message: format!(
                        "parallel collection policy '{collection}' requires a real Join member"
                    ),
                    path: Some(format!("{step_path}.controllerRole")),
                });
            }
        }
        if let Some(nested) = step.get("steps").and_then(Value::as_array) {
            collect_editor_flow_control_member_diagnostics(
                Some(nested),
                &format!("{step_path}.steps"),
                member_ids,
                diagnostics,
            );
        }
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for (branch_index, branch) in branches.iter().enumerate() {
                collect_editor_flow_control_member_diagnostics(
                    branch.get("steps").and_then(Value::as_array),
                    &format!("{step_path}.branches[{branch_index}].steps"),
                    member_ids,
                    diagnostics,
                );
            }
        }
        collect_editor_flow_control_member_diagnostics(
            step.get("fallback").and_then(Value::as_array),
            &format!("{step_path}.fallback"),
            member_ids,
            diagnostics,
        );
    }
}

fn editor_branch_requires_join_member(step: &Value) -> bool {
    let branch_count = step
        .get("branches")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let has_fallback = step
        .get("fallback")
        .and_then(Value::as_array)
        .is_some_and(|steps| !steps.is_empty());
    branch_count + usize::from(has_fallback) > 1
}

fn validate_editor_flow_conditions(flow: &Value) -> Vec<MobpackDiagnostic> {
    let mut diagnostics = Vec::new();
    let input_params = editor_input_param_names(flow);
    collect_editor_flow_condition_diagnostics(
        flow.get("steps").and_then(Value::as_array),
        "flow.steps",
        &input_params,
        &mut diagnostics,
    );
    diagnostics
}

fn collect_editor_flow_condition_diagnostics(
    steps: Option<&Vec<Value>>,
    path: &str,
    input_params: &BTreeSet<String>,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(steps) = steps else {
        return;
    };
    for (index, step) in steps.iter().enumerate() {
        let step_path = format!("{path}[{index}]");
        if step.get("type").and_then(Value::as_str) == Some("repeat") {
            if let Some(iteration_input) = step
                .get("iterationInput")
                .or_else(|| step.get("iteration_input"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                && !REPEAT_ITERATION_INPUTS.contains(&iteration_input)
            {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "unsupported_repeat_iteration_input".to_string(),
                    message: format!("editor repeat iteration input '{iteration_input}' is not supported by current MobKit RepeatUntil"),
                    path: Some(format!("{step_path}.iterationInput")),
                });
            }
            validate_required_repeat_condition_fields(step, &step_path, input_params, diagnostics);
        }
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for (branch_index, branch) in branches.iter().enumerate() {
                let has_cond_object = branch.get("cond").and_then(Value::as_object).is_some();
                let has_condition_text = branch
                    .get("condition")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty());
                if step.get("type").and_then(Value::as_str) == Some("branch")
                    && !has_cond_object
                    && !has_condition_text
                {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "missing_editor_branch_condition".to_string(),
                        message: "editor branch lanes must define an explicit condition"
                            .to_string(),
                        path: Some(format!("{step_path}.branches[{branch_index}]")),
                    });
                }
                if let Some(cond) = branch.get("cond").and_then(Value::as_object) {
                    let cond_path = format!("{step_path}.branches[{branch_index}].cond");
                    let condition_source_is_params = cond
                        .get("namespace")
                        .and_then(Value::as_str)
                        .is_some_and(|namespace| namespace == "params")
                        || cond
                            .get("stepId")
                            .and_then(Value::as_str)
                            .is_some_and(|step_id| step_id == "params");
                    for key in ["stepId", "field", "op", "val"] {
                        if key == "stepId" && condition_source_is_params {
                            continue;
                        }
                        if !cond
                            .get(key)
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .is_some_and(|value| !value.is_empty())
                        {
                            diagnostics.push(MobpackDiagnostic {
                                severity: "error".to_string(),
                                code: "incomplete_editor_branch_condition".to_string(),
                                message: format!("editor branch condition must include {key}"),
                                path: Some(format!("{cond_path}.{key}")),
                            });
                        }
                    }
                    if let Some(op) = cond
                        .get("op")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        && !editor_condition_operator_supported(op)
                    {
                        diagnostics.push(MobpackDiagnostic {
                            severity: "error".to_string(),
                            code: "unsupported_editor_condition_operator".to_string(),
                            message: format!("editor branch condition operator '{op}' is not supported by current MobKit ConditionExpr"),
                            path: Some(format!("{cond_path}.op")),
                        });
                    }
                    if condition_source_is_params {
                        validate_editor_param_ref(
                            cond.get("field")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                            input_params,
                            &format!("{cond_path}.field"),
                            diagnostics,
                        );
                    }
                }
                if let Some(condition) = branch
                    .get("condition")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    && !editor_condition_text_supported(condition)
                {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "unsupported_editor_condition_operator".to_string(),
                        message: format!(
                            "editor branch condition '{condition}' is not supported by current MobKit ConditionExpr"
                        ),
                        path: Some(format!("{step_path}.branches[{branch_index}].condition")),
                    });
                }
                if let Some(condition) = branch
                    .get("condition")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    && let Some(field) = editor_condition_text_param_ref(condition)
                {
                    validate_editor_param_ref(
                        &field,
                        input_params,
                        &format!("{step_path}.branches[{branch_index}].condition"),
                        diagnostics,
                    );
                }
                collect_editor_flow_condition_diagnostics(
                    branch.get("steps").and_then(Value::as_array),
                    &format!("{step_path}.branches[{branch_index}].steps"),
                    input_params,
                    diagnostics,
                );
            }
        }
        collect_editor_flow_condition_diagnostics(
            step.get("steps").and_then(Value::as_array),
            &format!("{step_path}.steps"),
            input_params,
            diagnostics,
        );
        collect_editor_flow_condition_diagnostics(
            step.get("fallback").and_then(Value::as_array),
            &format!("{step_path}.fallback"),
            input_params,
            diagnostics,
        );
    }
}

fn editor_condition_text_param_ref(condition: &str) -> Option<String> {
    let trimmed = condition.trim();
    for separator in ["&&", "||"] {
        if trimmed.contains(separator) {
            return trimmed
                .split(separator)
                .find_map(|part| editor_condition_text_param_ref(part.trim()));
        }
    }
    let path = trimmed.split_whitespace().next()?;
    path.strip_prefix("params.")
        .and_then(|rest| rest.split('.').next())
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(ToString::to_string)
}

fn validate_required_repeat_condition_fields(
    step: &Value,
    step_path: &str,
    input_params: &BTreeSet<String>,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(cond) = step.get("cond").and_then(Value::as_object) else {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "missing_repeat_condition".to_string(),
            message: "repeat editor steps must define an explicit until condition".to_string(),
            path: Some(format!("{step_path}.cond")),
        });
        return;
    };

    let cond_path = format!("{step_path}.cond");
    for key in ["stepId", "field", "op", "val"] {
        if !cond
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
        {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "incomplete_repeat_condition".to_string(),
                message: format!("repeat editor condition must include {key}"),
                path: Some(format!("{cond_path}.{key}")),
            });
        }
    }

    if let Some(op) = cond
        .get("op")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && !editor_condition_operator_supported(op)
    {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "unsupported_editor_condition_operator".to_string(),
            message: format!(
                "editor repeat condition operator '{op}' is not supported by current MobKit ConditionExpr"
            ),
            path: Some(format!("{cond_path}.op")),
        });
    }

    let condition_source_is_params = cond
        .get("namespace")
        .and_then(Value::as_str)
        .is_some_and(|namespace| namespace == "params")
        || cond
            .get("stepId")
            .and_then(Value::as_str)
            .is_some_and(|step_id| step_id == "params");
    if condition_source_is_params {
        validate_editor_param_ref(
            cond.get("field")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            input_params,
            &format!("{cond_path}.field"),
            diagnostics,
        );
    }
}

fn validate_editor_param_ref(
    field: &str,
    input_params: &BTreeSet<String>,
    path: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let field = field.trim();
    if field.is_empty() || !input_params.contains(field) {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "unknown_editor_input_param".to_string(),
            message: format!("condition references undeclared input param '{field}'"),
            path: Some(path.to_string()),
        });
    }
}

fn editor_condition_text_supported(condition: &str) -> bool {
    for separator in ["&&", "||"] {
        if condition.contains(separator) {
            return condition
                .split(separator)
                .all(|part| editor_condition_text_supported(part.trim()));
        }
    }
    let Some((_, op_and_value)) = condition.split_once(char::is_whitespace) else {
        return false;
    };
    let op = op_and_value.split_whitespace().next().unwrap_or_default();
    editor_condition_operator_supported(op)
}

fn editor_condition_operator_supported(op: &str) -> bool {
    matches!(op, "==" | ">" | "<")
}

fn validate_deploy_settings(deploy: &Value) -> Vec<MobpackDiagnostic> {
    if deploy.is_null() {
        return Vec::new();
    }
    let Some(object) = deploy.as_object() else {
        return vec![MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_deploy_settings".to_string(),
            message: "document.deploy must be an object".to_string(),
            path: Some("deploy".to_string()),
        }];
    };
    let mut diagnostics = Vec::new();
    if let Some(command) = object
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && command != "rkat mob deploy"
    {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_deploy_command".to_string(),
            message: format!("deploy command must be 'rkat mob deploy', got '{command}'"),
            path: Some("deploy.command".to_string()),
        });
    }
    validate_deploy_string_enum(
        object,
        "surface",
        &["cli", "rpc"],
        "invalid_deploy_surface",
        &mut diagnostics,
    );
    validate_deploy_string_enum(
        object,
        "trust_policy",
        &["permissive", "strict"],
        "invalid_deploy_trust_policy",
        &mut diagnostics,
    );
    validate_deploy_string_enum(
        object,
        "realm_backend",
        &["jsonl", "sqlite"],
        "invalid_deploy_realm_backend",
        &mut diagnostics,
    );
    if let Some(model) = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        let known_models = meerkat_models::catalog()
            .into_iter()
            .map(|entry| entry.id.to_string())
            .collect::<BTreeSet<_>>();
        if !known_models.contains(model) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "unknown_deploy_model".to_string(),
                message: format!("deploy.model '{model}' is not in the MobKit model catalog"),
                path: Some("deploy.model".to_string()),
            });
        }
    }
    validate_deploy_non_negative_number(object, "max_total_tokens", &mut diagnostics);
    validate_deploy_non_negative_number(object, "max_tool_calls", &mut diagnostics);
    for key in [
        "model",
        "max_duration",
        "realm",
        "instance",
        "context_root",
        "state_root",
        "user_config_root",
    ] {
        if object
            .get(key)
            .is_some_and(|value| !value.is_string() && !value.is_null())
        {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_deploy_string".to_string(),
                message: format!("deploy.{key} must be a string when present"),
                path: Some(format!("deploy.{key}")),
            });
        }
    }
    if object
        .get("isolated")
        .is_some_and(|value| !value.is_boolean() && !value.is_null())
    {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_deploy_isolated".to_string(),
            message: "deploy.isolated must be a boolean when present".to_string(),
            path: Some("deploy.isolated".to_string()),
        });
    }
    if let Some(prompt) = object.get("prompt") {
        match prompt.as_str() {
            Some(value) if !value.trim().is_empty() => {}
            _ => diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_deploy_prompt".to_string(),
                message: "deploy.prompt must be a non-empty string when present".to_string(),
                path: Some("deploy.prompt".to_string()),
            }),
        }
    }
    diagnostics
}

fn validate_deploy_runtime_modes(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    let deploy_surface = document
        .deploy
        .get("surface")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("cli");
    if deploy_surface != "cli" {
        return Vec::new();
    }
    let Some(members) = document.members.as_array() else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    for (member_index, member) in members.iter().enumerate() {
        let runtime_mode = member
            .get("runtimeMode")
            .or_else(|| member.get("runtime_mode"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        if let Some(reason) = deploy_runtime_mode_block_reason(deploy_surface, runtime_mode) {
            let label = member
                .get("name")
                .or_else(|| member.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("member");
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "deploy_surface_runtime_mode_unsupported".to_string(),
                message: format!(
                    "profile '{label}' uses runtime mode '{runtime_mode}', but deploy surface '{deploy_surface}' does not support it: {reason}"
                ),
                path: Some(format!("members[{member_index}].runtimeMode")),
            });
        }
    }
    diagnostics
}

fn deploy_runtime_mode_compatibility() -> Value {
    json!({
        "cli": {
            "allowed": ["turn_driven"],
            "blocked": {
                "autonomous_host": "RPC surface only; rkat mob deploy requires turn_driven profiles."
            }
        },
        "rpc": {
            "allowed": runtime_mode_values(),
            "blocked": {}
        }
    })
}

fn deploy_runtime_mode_block_reason(surface: &str, runtime_mode: &str) -> Option<String> {
    let runtime_mode = runtime_mode.trim();
    if runtime_mode.is_empty() {
        return None;
    }
    let compatibility = deploy_runtime_mode_compatibility();
    let surface_contract = compatibility.get(surface)?;
    let allowed = surface_contract
        .get("allowed")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    if allowed.contains(runtime_mode) {
        return None;
    }
    surface_contract
        .get("blocked")
        .and_then(Value::as_object)
        .and_then(|blocked| blocked.get(runtime_mode))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| Some("runtime mode is not in this surface's allowed runtime_modes".to_string()))
}

fn runtime_mode_labels() -> Value {
    json!({
        "turn_driven": "turn_driven — explicit turn dispatch",
        "autonomous_host": "autonomous_host — RPC keep-alive member loop"
    })
}

fn validate_editor_schemas(schemas: &Value) -> Vec<MobpackDiagnostic> {
    if schemas.is_null() {
        return Vec::new();
    }
    let Some(items) = schemas.as_array() else {
        return vec![MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_editor_schemas".to_string(),
            message: "document.schemas must be an array".to_string(),
            path: Some("schemas".to_string()),
        }];
    };

    let mut diagnostics = Vec::new();
    let mut schema_ids = BTreeSet::new();
    for (schema_index, schema) in items.iter().enumerate() {
        let schema_path = format!("schemas[{schema_index}]");
        let Some(schema_id) = schema
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_editor_schema_id".to_string(),
                message: "editor schema id must be a non-empty string".to_string(),
                path: Some(format!("{schema_path}.id")),
            });
            continue;
        };
        if !schema_ids.insert(schema_id.to_string()) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "duplicate_editor_schema_id".to_string(),
                message: format!("editor schema id '{schema_id}' is duplicated"),
                path: Some(format!("{schema_path}.id")),
            });
        }

        let Some(fields) = schema.get("fields").and_then(Value::as_array) else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_editor_schema_fields".to_string(),
                message: "editor schema fields must be an array".to_string(),
                path: Some(format!("{schema_path}.fields")),
            });
            continue;
        };
        let mut field_names = BTreeSet::new();
        for (field_index, field) in fields.iter().enumerate() {
            let field_path = format!("{schema_path}.fields[{field_index}]");
            let field_name = field
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty());
            match field_name {
                Some(name) if !field_names.insert(name.to_string()) => {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "duplicate_editor_schema_field".to_string(),
                        message: format!("editor schema field '{name}' is duplicated"),
                        path: Some(format!("{field_path}.name")),
                    });
                }
                Some(_) => {}
                None => diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_editor_schema_field_name".to_string(),
                    message: "editor schema field name must be a non-empty string".to_string(),
                    path: Some(format!("{field_path}.name")),
                }),
            }

            let field_type = field
                .get("type")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|field_type| !field_type.is_empty())
                .unwrap_or("string");
            if !is_editor_schema_field_type(field_type) {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_editor_schema_field_type".to_string(),
                    message: format!("unsupported editor schema field type '{field_type}'"),
                    path: Some(format!("{field_path}.type")),
                });
            }

            let enum_values = field
                .get("enumValues")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(str::trim))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if field_type == "enum" {
                if enum_values.is_empty() {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "missing_editor_schema_enum_values".to_string(),
                        message: "enum schema fields must define at least one value".to_string(),
                        path: Some(format!("{field_path}.enumValues")),
                    });
                }
                let mut seen_values = BTreeSet::new();
                for (value_index, value) in enum_values.iter().enumerate() {
                    if value.is_empty() {
                        diagnostics.push(MobpackDiagnostic {
                            severity: "error".to_string(),
                            code: "invalid_editor_schema_enum_value".to_string(),
                            message: "enum values must be non-empty strings".to_string(),
                            path: Some(format!("{field_path}.enumValues[{value_index}]")),
                        });
                    } else if !seen_values.insert((*value).to_string()) {
                        diagnostics.push(MobpackDiagnostic {
                            severity: "error".to_string(),
                            code: "duplicate_editor_schema_enum_value".to_string(),
                            message: format!("enum value '{value}' is duplicated"),
                            path: Some(format!("{field_path}.enumValues[{value_index}]")),
                        });
                    }
                }
            } else if !enum_values.is_empty() {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "stale_editor_schema_enum_values".to_string(),
                    message: "enumValues are only valid on enum schema fields".to_string(),
                    path: Some(format!("{field_path}.enumValues")),
                });
            }
        }
    }
    diagnostics
}

fn validate_editor_input_params(flow: &Value) -> Vec<MobpackDiagnostic> {
    let mut diagnostics = Vec::new();
    let Some(steps) = flow.get("steps").and_then(Value::as_array) else {
        return diagnostics;
    };
    for (step_index, step) in steps.iter().enumerate() {
        if step.get("type").and_then(Value::as_str) != Some("input") {
            continue;
        }
        let Some(params) = step.get("inputParams") else {
            continue;
        };
        let path = format!("flow.steps[{step_index}].inputParams");
        let Some(params) = params.as_array() else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_editor_input_params".to_string(),
                message: "inputParams must be an array".to_string(),
                path: Some(path),
            });
            continue;
        };
        let mut names = BTreeSet::new();
        for (param_index, param) in params.iter().enumerate() {
            let param_path = format!("{path}[{param_index}]");
            let name = param
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty());
            match name {
                Some(name) if !names.insert(name.to_string()) => {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "duplicate_editor_input_param".to_string(),
                        message: format!("input param '{name}' is duplicated"),
                        path: Some(format!("{param_path}.name")),
                    });
                }
                Some(_) => {}
                None => diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_editor_input_param_name".to_string(),
                    message: "input param name must be a non-empty string".to_string(),
                    path: Some(format!("{param_path}.name")),
                }),
            }
            let field_type = param
                .get("type")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|field_type| !field_type.is_empty())
                .unwrap_or("string");
            if !is_editor_schema_field_type(field_type) {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_editor_input_param_type".to_string(),
                    message: format!("unsupported input param type '{field_type}'"),
                    path: Some(format!("{param_path}.type")),
                });
            }
            let enum_values = param
                .get("enumValues")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(str::trim))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if field_type == "enum" {
                if enum_values.is_empty() {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "missing_editor_input_param_enum_values".to_string(),
                        message: "enum input params must define at least one value".to_string(),
                        path: Some(format!("{param_path}.enumValues")),
                    });
                }
            } else if !enum_values.is_empty() {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "stale_editor_input_param_enum_values".to_string(),
                    message: "enumValues are only valid on enum input params".to_string(),
                    path: Some(format!("{param_path}.enumValues")),
                });
            }
        }
    }
    diagnostics
}

fn validate_graph_projection(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    if document.instances.is_null() && document.edges.is_null() && document.frames.is_null() {
        return Vec::new();
    }
    let mut diagnostics = Vec::new();
    let Some(instances) = document.instances.as_array() else {
        if !document.instances.is_null() {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_graph_instances".to_string(),
                message: "document.instances must be an array".to_string(),
                path: Some("instances".to_string()),
            });
        }
        return diagnostics;
    };
    let edges = match document.edges.as_array() {
        Some(edges) => edges,
        None => {
            if !document.edges.is_null() {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_graph_edges".to_string(),
                    message: "document.edges must be an array".to_string(),
                    path: Some("edges".to_string()),
                });
            }
            return diagnostics;
        }
    };
    let flow_members = editor_flow_member_step_roles(&document.flow);
    let flow_member_metadata = editor_flow_member_step_metadata(&document.flow);
    let flow_launch_modes = editor_flow_member_launch_modes(&document.flow);
    let graph_controls = editor_flow_graph_controls(&document.flow);
    let member_ids = editor_member_ids(document);
    let input_params = editor_input_param_names(&document.flow);
    let member_schema_refs = editor_member_schema_refs(document);
    let schema_field_names = editor_schema_field_names(&document.schemas);
    let mut instance_ids = BTreeSet::new();
    let mut gate_instance_ids = BTreeSet::new();
    let mut member_instance_ids = BTreeSet::new();
    let mut instance_members = BTreeMap::new();

    for (index, instance) in instances.iter().enumerate() {
        let path = format!("instances[{index}]");
        let Some(id) = instance
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_graph_instance_id".to_string(),
                message: "graph instances must have non-empty string ids".to_string(),
                path: Some(format!("{path}.id")),
            });
            continue;
        };
        if !instance_ids.insert(id.to_string()) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "duplicate_graph_instance_id".to_string(),
                message: format!("duplicate graph instance id '{id}'"),
                path: Some(format!("{path}.id")),
            });
        }
        let is_gate = instance
            .get("isGate")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let is_terminal = instance
            .get("isTerminal")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if is_gate {
            gate_instance_ids.insert(id.to_string());
            let gate_kind = instance
                .get("gateKind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !GRAPH_GATE_KINDS.contains(&gate_kind) {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_graph_gate_kind".to_string(),
                    message: format!("unsupported graph gate kind '{gate_kind}'"),
                    path: Some(format!("{path}.gateKind")),
                });
            }
            if !graph_controls.gate_ids.contains(id) {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "uncompiled_graph_gate".to_string(),
                    message: format!(
                        "graph gate '{id}' is not backed by a branch/parallel flow primitive"
                    ),
                    path: Some(path.clone()),
                });
            }
            if gate_kind == "fork"
                && let Some(expected) = graph_controls.fork_controls.get(id)
            {
                validate_graph_fork_control(instance, &path, expected, &mut diagnostics);
            }
            if gate_kind == "join"
                && let Some(expected) = graph_controls.join_controls.get(id)
            {
                validate_graph_join_control(instance, &path, expected, &mut diagnostics);
            }
            continue;
        }
        if is_terminal {
            let terminal_kind = instance
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !GRAPH_TERMINAL_KINDS.contains(&terminal_kind) {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_graph_terminal_kind".to_string(),
                    message: format!("unsupported graph terminal kind '{terminal_kind}'"),
                    path: Some(format!("{path}.kind")),
                });
            }
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "uncompiled_graph_terminal".to_string(),
                message: format!(
                    "graph terminal '{id}' is visual-only and does not compile into MobKit mob.toml"
                ),
                path: Some(path),
            });
            continue;
        }
        let Some(member_id) = instance
            .get("memberId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_graph_member".to_string(),
                message: "member graph instances must reference memberId".to_string(),
                path: Some(format!("{path}.memberId")),
            });
            continue;
        };
        member_instance_ids.insert(id.to_string());
        instance_members.insert(id.to_string(), member_id.to_string());
        if !member_ids.is_empty() && !member_ids.contains(member_id) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "unknown_graph_member".to_string(),
                message: format!("graph instance '{id}' references unknown member '{member_id}'"),
                path: Some(format!("{path}.memberId")),
            });
        }
        if !flow_members.is_empty() {
            match flow_members.get(id) {
                Some(expected_member) if expected_member != member_id => {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "graph_flow_member_mismatch".to_string(),
                        message: format!(
                            "graph instance '{id}' member '{member_id}' does not match flow member '{expected_member}'"
                        ),
                        path: Some(format!("{path}.memberId")),
                    });
                }
                Some(_) => {}
                None => diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "graph_instance_missing_from_flow".to_string(),
                    message: format!(
                        "graph member instance '{id}' has no matching member step in document.flow"
                    ),
                    path: Some(path.clone()),
                }),
            }
        }
        if let Some(expected_metadata) = flow_member_metadata.get(id) {
            validate_graph_member_metadata(instance, &path, expected_metadata, &mut diagnostics);
        }
        if let Some(expected_launch) = flow_launch_modes.get(id) {
            validate_graph_member_launch_mode(instance, &path, expected_launch, &mut diagnostics);
        }
    }

    for step_id in flow_members.keys() {
        if !member_instance_ids.contains(step_id) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "flow_step_missing_graph_instance".to_string(),
                message: format!("flow member step '{step_id}' has no matching graph instance"),
                path: Some("instances".to_string()),
            });
        }
    }
    for gate_id in &graph_controls.gate_ids {
        if !gate_instance_ids.contains(gate_id) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_compiled_graph_gate".to_string(),
                message: format!(
                    "flow compiles graph gate '{gate_id}', but document.instances has no matching gate instance"
                ),
                path: Some("instances".to_string()),
            });
        }
    }

    let mut edge_ids = BTreeSet::new();
    for (index, edge) in edges.iter().enumerate() {
        let path = format!("edges[{index}]");
        if let Some(id) = edge
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            && !edge_ids.insert(id.to_string())
        {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "duplicate_graph_edge_id".to_string(),
                message: format!("duplicate graph edge id '{id}'"),
                path: Some(format!("{path}.id")),
            });
        }
        for endpoint in ["from", "to"] {
            let Some(value) = edge
                .get(endpoint)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_graph_edge_endpoint".to_string(),
                    message: format!("graph edge must include non-empty {endpoint}"),
                    path: Some(format!("{path}.{endpoint}")),
                });
                continue;
            };
            if !instance_ids.contains(value) {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "unknown_graph_edge_endpoint".to_string(),
                    message: format!("graph edge {endpoint} '{value}' has no matching instance"),
                    path: Some(format!("{path}.{endpoint}")),
                });
            }
        }
        let kind = edge
            .get("kind")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
            .unwrap_or("");
        if !GRAPH_EDGE_KINDS.contains(&kind) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_graph_edge_kind".to_string(),
                message: if kind.is_empty() {
                    "graph edge must include non-empty kind".to_string()
                } else {
                    format!("unsupported graph edge kind '{kind}'")
                },
                path: Some(format!("{path}.kind")),
            });
        }
        if kind == "cond" {
            let Some(cond) = edge.get("cond").and_then(Value::as_object) else {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "incomplete_graph_condition".to_string(),
                    message: "conditional graph edges must reference a real member output field"
                        .to_string(),
                    path: Some(format!("{path}.cond")),
                });
                continue;
            };
            for key in ["var", "op", "val"] {
                if !cond
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty())
                {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "incomplete_graph_condition".to_string(),
                        message: format!("graph condition must include non-empty {key}"),
                        path: Some(format!("{path}.cond.{key}")),
                    });
                }
            }
            if let Some(op) = cond
                .get("op")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                && !editor_condition_operator_supported(op)
            {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "unsupported_graph_condition_operator".to_string(),
                    message: format!(
                        "graph condition operator '{op}' is not supported by current MobKit ConditionExpr"
                    ),
                    path: Some(format!("{path}.cond.op")),
                });
            }
            if let Some(var) = cond
                .get("var")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let parts = var.split('.').collect::<Vec<_>>();
                if parts.len() == 2 && parts[0] == "params" && !parts[1].trim().is_empty() {
                    validate_editor_param_ref(
                        parts[1],
                        &input_params,
                        &format!("{path}.cond.var"),
                        &mut diagnostics,
                    );
                } else if parts.len() != 3 || parts[0] != "steps" {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "invalid_graph_condition_ref".to_string(),
                        message: format!(
                            "graph condition ref '{var}' must use steps.<instance>.<field> or params.<field>"
                        ),
                        path: Some(format!("{path}.cond.var")),
                    });
                } else {
                    let step_id = parts[1];
                    let field = parts[2];
                    if !member_instance_ids.contains(step_id) {
                        diagnostics.push(MobpackDiagnostic {
                            severity: "error".to_string(),
                            code: "unknown_graph_condition_step".to_string(),
                            message: format!(
                                "graph condition references unknown member instance '{step_id}'"
                            ),
                            path: Some(format!("{path}.cond.var")),
                        });
                    } else if let Some(member_id) = instance_members.get(step_id) {
                        match member_schema_refs.get(member_id) {
                            Some(schema_id) => match schema_field_names.get(schema_id) {
                                Some(fields) if fields.contains(field) => {}
                                Some(_) => diagnostics.push(MobpackDiagnostic {
                                    severity: "error".to_string(),
                                    code: "unknown_graph_condition_field".to_string(),
                                    message: format!(
                                        "graph condition field '{field}' does not exist on schema '{schema_id}'"
                                    ),
                                    path: Some(format!("{path}.cond.var")),
                                }),
                                None => diagnostics.push(MobpackDiagnostic {
                                    severity: "error".to_string(),
                                    code: "unknown_graph_condition_schema".to_string(),
                                    message: format!(
                                        "graph condition references missing schema '{schema_id}'"
                                    ),
                                    path: Some(format!("{path}.cond.var")),
                                }),
                            },
                            None => diagnostics.push(MobpackDiagnostic {
                                severity: "error".to_string(),
                                code: "graph_condition_member_without_schema".to_string(),
                                message: format!(
                                    "graph condition member '{member_id}' has no output schema"
                                ),
                                path: Some(format!("{path}.cond.var")),
                            }),
                        }
                    }
                }
            }
        }
    }
    validate_expected_graph_edges(edges, &graph_controls.expected_edges, &mut diagnostics);

    let mut frame_ids = BTreeSet::new();
    if let Some(frames) = document.frames.as_array() {
        for (index, frame) in frames.iter().enumerate() {
            let kind = frame
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !GRAPH_FRAME_KINDS.contains(&kind) {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_graph_frame_kind".to_string(),
                    message: format!("unsupported graph frame kind '{kind}'"),
                    path: Some(format!("frames[{index}].kind")),
                });
            }
            if let Some(frame_id) = frame
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                frame_ids.insert(frame_id.to_string());
                if !graph_controls.frame_ids.contains(frame_id) {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "uncompiled_graph_frame".to_string(),
                        message: format!(
                            "graph frame '{frame_id}' is not backed by a branch/parallel/repeat flow primitive"
                        ),
                        path: Some(format!("frames[{index}]")),
                    });
                } else if let Some(expected_kind) = graph_controls.frame_kinds.get(frame_id)
                    && kind != expected_kind
                {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "graph_frame_kind_mismatch".to_string(),
                        message: format!(
                            "graph frame '{frame_id}' must render as {expected_kind}, not {kind}"
                        ),
                        path: Some(format!("frames[{index}].kind")),
                    });
                }
            }
        }
    } else if !document.frames.is_null() {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_graph_frames".to_string(),
            message: "document.frames must be an array".to_string(),
            path: Some("frames".to_string()),
        });
    }
    for frame_id in &graph_controls.frame_ids {
        if !frame_ids.contains(frame_id) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_compiled_graph_frame".to_string(),
                message: format!(
                    "flow compiles graph frame '{frame_id}', but document.frames has no matching frame"
                ),
                path: Some("frames".to_string()),
            });
        }
    }

    diagnostics
}

#[derive(Debug, Default)]
struct EditorGraphControls {
    gate_ids: BTreeSet<String>,
    frame_ids: BTreeSet<String>,
    frame_kinds: BTreeMap<String, String>,
    fork_controls: BTreeMap<String, EditorGraphForkControl>,
    join_controls: BTreeMap<String, EditorGraphJoinControl>,
    expected_edges: Vec<EditorGraphExpectedEdge>,
}

impl EditorGraphControls {
    fn register_frame(&mut self, id: String, kind: &str) {
        self.frame_ids.insert(id.clone());
        self.frame_kinds.insert(id, kind.to_string());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorGraphForkControl {
    dispatch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorGraphJoinControl {
    collection: String,
    quorum: Option<u64>,
    controller_role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorGraphExpectedEdge {
    from: String,
    to: String,
    kind: String,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorGraphMemberMetadata {
    timeout_ms: Option<u64>,
    allowed_tools: Vec<String>,
    blocked_tools: Vec<String>,
    output_format: String,
    dispatch_mode: String,
    collection_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorGraphLaunchMode {
    kind: String,
    session_id: Option<String>,
    source: Option<String>,
    context: Option<String>,
    budget_split_policy: String,
}

#[derive(Debug, Clone, Default)]
struct EditorGraphEndpointSet {
    entries: Vec<String>,
    exits: Vec<String>,
}

fn editor_flow_graph_controls(flow: &Value) -> EditorGraphControls {
    let mut controls = EditorGraphControls::default();
    if let Some(steps) = flow.get("steps").and_then(Value::as_array) {
        collect_editor_flow_graph_controls(steps, &mut controls);
    }
    controls
}

fn collect_editor_flow_graph_controls(steps: &[Value], controls: &mut EditorGraphControls) {
    collect_editor_sequence_graph_edges(steps, "flow.steps", controls);
    for (index, step) in steps.iter().enumerate() {
        let id = step
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        match (step.get("type").and_then(Value::as_str), id) {
            (Some("branch"), Some(id)) => {
                let gate_id = format!("g_branch_{id}");
                let join_id = format!("j_branch_{id}");
                controls.gate_ids.insert(gate_id.clone());
                controls.gate_ids.insert(join_id.clone());
                controls.register_frame(format!("frame_branch_{id}"), "Branch");
                controls
                    .join_controls
                    .insert(join_id.clone(), editor_graph_join_control_for_branch(step));
                collect_editor_branch_graph_edges(step, &gate_id, &join_id, controls);
            }
            (Some("parallel"), Some(id)) => {
                let gate_id = format!("g_parallel_{id}");
                let join_id = format!("j_parallel_{id}");
                controls.gate_ids.insert(gate_id.clone());
                controls.gate_ids.insert(join_id.clone());
                controls.register_frame(format!("frame_parallel_{id}"), "Parallel");
                controls.fork_controls.insert(
                    gate_id.clone(),
                    editor_graph_fork_control_for_parallel(step),
                );
                controls
                    .join_controls
                    .insert(join_id, editor_graph_join_control_for_parallel(step));
                collect_editor_parallel_graph_edges(
                    step,
                    &gate_id,
                    &format!("j_parallel_{id}"),
                    controls,
                );
            }
            (Some("repeat"), Some(id)) => {
                controls.register_frame(format!("frame_{id}"), "RepeatUntil");
                collect_editor_repeat_graph_edges(step, &format!("flow.steps[{index}]"), controls);
            }
            _ => {}
        }
        if let Some(nested) = step.get("steps").and_then(Value::as_array) {
            collect_editor_flow_graph_controls(nested, controls);
        }
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for branch in branches {
                if let Some(branch_steps) = branch.get("steps").and_then(Value::as_array) {
                    collect_editor_flow_graph_controls(branch_steps, controls);
                }
            }
        }
        if let Some(fallback) = step.get("fallback").and_then(Value::as_array) {
            collect_editor_flow_graph_controls(fallback, controls);
        }
    }
}

fn collect_editor_sequence_graph_edges(
    steps: &[Value],
    path: &str,
    controls: &mut EditorGraphControls,
) {
    let mut previous_exits: Vec<String> = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        if step.get("type").and_then(Value::as_str) == Some("input") {
            continue;
        }
        let endpoints = editor_graph_endpoints_for_step(step);
        for from in &previous_exits {
            for to in &endpoints.entries {
                controls.expected_edges.push(EditorGraphExpectedEdge {
                    from: from.clone(),
                    to: to.clone(),
                    kind: "next".to_string(),
                    path: format!("{path}[{index}]"),
                });
            }
        }
        if !endpoints.exits.is_empty() {
            previous_exits = endpoints.exits;
        }
    }
}

fn collect_editor_repeat_graph_edges(step: &Value, path: &str, controls: &mut EditorGraphControls) {
    let endpoints = editor_graph_endpoints_for_steps(step.get("steps").and_then(Value::as_array));
    for from in &endpoints.exits {
        for to in &endpoints.entries {
            controls.expected_edges.push(EditorGraphExpectedEdge {
                from: from.clone(),
                to: to.clone(),
                kind: "cond".to_string(),
                path: path.to_string(),
            });
        }
    }
}

fn collect_editor_parallel_graph_edges(
    step: &Value,
    gate_id: &str,
    join_id: &str,
    controls: &mut EditorGraphControls,
) {
    if let Some(branches) = step.get("branches").and_then(Value::as_array) {
        for (index, branch) in branches.iter().enumerate() {
            let endpoints =
                editor_graph_endpoints_for_steps(branch.get("steps").and_then(Value::as_array));
            for entry in &endpoints.entries {
                controls.expected_edges.push(EditorGraphExpectedEdge {
                    from: gate_id.to_string(),
                    to: entry.clone(),
                    kind: "fanout".to_string(),
                    path: format!("flow.steps.{}.branches[{index}]", graph_step_id(step)),
                });
            }
            for exit in &endpoints.exits {
                controls.expected_edges.push(EditorGraphExpectedEdge {
                    from: exit.clone(),
                    to: join_id.to_string(),
                    kind: "next".to_string(),
                    path: format!("flow.steps.{}.branches[{index}]", graph_step_id(step)),
                });
            }
        }
    }
}

fn collect_editor_branch_graph_edges(
    step: &Value,
    gate_id: &str,
    join_id: &str,
    controls: &mut EditorGraphControls,
) {
    if let Some(branches) = step.get("branches").and_then(Value::as_array) {
        for (index, branch) in branches.iter().enumerate() {
            let endpoints =
                editor_graph_endpoints_for_steps(branch.get("steps").and_then(Value::as_array));
            for entry in &endpoints.entries {
                controls.expected_edges.push(EditorGraphExpectedEdge {
                    from: gate_id.to_string(),
                    to: entry.clone(),
                    kind: "cond".to_string(),
                    path: format!("flow.steps.{}.branches[{index}]", graph_step_id(step)),
                });
            }
            for exit in &endpoints.exits {
                controls.expected_edges.push(EditorGraphExpectedEdge {
                    from: exit.clone(),
                    to: join_id.to_string(),
                    kind: "next".to_string(),
                    path: format!("flow.steps.{}.branches[{index}]", graph_step_id(step)),
                });
            }
        }
    }
    let fallback = editor_graph_endpoints_for_steps(step.get("fallback").and_then(Value::as_array));
    for entry in &fallback.entries {
        controls.expected_edges.push(EditorGraphExpectedEdge {
            from: gate_id.to_string(),
            to: entry.clone(),
            kind: "next".to_string(),
            path: format!("flow.steps.{}.fallback", graph_step_id(step)),
        });
    }
    for exit in &fallback.exits {
        controls.expected_edges.push(EditorGraphExpectedEdge {
            from: exit.clone(),
            to: join_id.to_string(),
            kind: "next".to_string(),
            path: format!("flow.steps.{}.fallback", graph_step_id(step)),
        });
    }
}

fn editor_graph_endpoints_for_steps(steps: Option<&Vec<Value>>) -> EditorGraphEndpointSet {
    let mut entries = Vec::new();
    let mut exits = Vec::new();
    let Some(steps) = steps else {
        return EditorGraphEndpointSet::default();
    };
    for step in steps {
        let endpoints = editor_graph_endpoints_for_step(step);
        if entries.is_empty() {
            entries = endpoints.entries;
        }
        if !endpoints.exits.is_empty() {
            exits = endpoints.exits;
        }
    }
    EditorGraphEndpointSet { entries, exits }
}

fn editor_graph_endpoints_for_step(step: &Value) -> EditorGraphEndpointSet {
    let Some(id) = step
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return EditorGraphEndpointSet::default();
    };
    match step.get("type").and_then(Value::as_str) {
        Some("member") => EditorGraphEndpointSet {
            entries: vec![id.to_string()],
            exits: vec![id.to_string()],
        },
        Some("branch") => EditorGraphEndpointSet {
            entries: vec![format!("g_branch_{id}")],
            exits: vec![format!("j_branch_{id}")],
        },
        Some("parallel") => EditorGraphEndpointSet {
            entries: vec![format!("g_parallel_{id}")],
            exits: vec![format!("j_parallel_{id}")],
        },
        Some("repeat") => {
            editor_graph_endpoints_for_steps(step.get("steps").and_then(Value::as_array))
        }
        _ => EditorGraphEndpointSet::default(),
    }
}

fn graph_step_id(step: &Value) -> String {
    step.get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

fn validate_expected_graph_edges(
    edges: &[Value],
    expected_edges: &[EditorGraphExpectedEdge],
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let actual = edges
        .iter()
        .filter_map(|edge| {
            let kind = edge.get("kind")?.as_str()?.trim().to_string();
            if kind.is_empty() {
                return None;
            }
            Some((
                edge.get("from")?.as_str()?.trim().to_string(),
                edge.get("to")?.as_str()?.trim().to_string(),
                kind,
            ))
        })
        .collect::<BTreeSet<_>>();
    let expected = expected_edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone(), edge.kind.clone()))
        .collect::<BTreeSet<_>>();
    for expected in expected_edges {
        let key = (
            expected.from.clone(),
            expected.to.clone(),
            expected.kind.clone(),
        );
        if !actual.contains(&key) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "graph_flow_edge_mismatch".to_string(),
                message: format!(
                    "graph is missing {} edge from '{}' to '{}' for {}",
                    expected.kind, expected.from, expected.to, expected.path
                ),
                path: Some("edges".to_string()),
            });
        }
    }
    for (from, to, kind) in actual {
        if !expected.contains(&(from.clone(), to.clone(), kind.clone())) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "graph_extra_uncompiled_edge".to_string(),
                message: format!(
                    "graph has {kind} edge from '{from}' to '{to}', but no matching MobKit flow edge compiles from document.flow"
                ),
                path: Some("edges".to_string()),
            });
        }
    }
}

fn editor_graph_join_control_for_parallel(step: &Value) -> EditorGraphJoinControl {
    let policy = editor_collection_policy(step);
    let (collection, quorum) = if let Some(n) = policy.strip_prefix("quorum:") {
        ("quorum".to_string(), n.parse::<u64>().ok())
    } else {
        (policy, None)
    };
    EditorGraphJoinControl {
        collection,
        quorum,
        controller_role: step
            .get("controllerRole")
            .or_else(|| step.get("controllerMemberId"))
            .or_else(|| step.get("controlRole"))
            .or_else(|| step.get("joinRole"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
    }
}

fn editor_graph_join_control_for_branch(step: &Value) -> EditorGraphJoinControl {
    EditorGraphJoinControl {
        collection: "any".to_string(),
        quorum: None,
        controller_role: step
            .get("controllerRole")
            .or_else(|| step.get("controllerMemberId"))
            .or_else(|| step.get("controlRole"))
            .or_else(|| step.get("joinRole"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
    }
}

fn editor_graph_fork_control_for_parallel(step: &Value) -> EditorGraphForkControl {
    EditorGraphForkControl {
        dispatch: graph_dispatch_mode_from_value(
            step.get("dispatch")
                .or_else(|| step.get("dispatchMode"))
                .or_else(|| step.get("dispatch_mode")),
        ),
    }
}

fn validate_graph_fork_control(
    instance: &Value,
    path: &str,
    expected: &EditorGraphForkControl,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let actual_dispatch = graph_dispatch_mode_from_value(
        instance
            .get("dispatch")
            .or_else(|| instance.get("dispatchMode"))
            .or_else(|| instance.get("dispatch_mode"))
            .or_else(|| instance.get("label")),
    );
    if actual_dispatch != expected.dispatch {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "graph_fork_dispatch_mismatch".to_string(),
            message: format!(
                "graph fork dispatch '{actual_dispatch}' does not match flow dispatch '{}'",
                expected.dispatch
            ),
            path: Some(format!("{path}.dispatch")),
        });
    }
}

fn graph_dispatch_mode_from_value(value: Option<&Value>) -> String {
    match value.and_then(Value::as_str).map(str::trim) {
        Some("one_to_one") => "one_to_one".to_string(),
        Some("fan_in") => "fan_in".to_string(),
        _ => "fan_out".to_string(),
    }
}

fn validate_graph_join_control(
    instance: &Value,
    path: &str,
    expected: &EditorGraphJoinControl,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let actual_collection = graph_join_collection(instance);
    if actual_collection != expected.collection {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "graph_join_collection_mismatch".to_string(),
            message: format!(
                "graph join collection '{actual_collection}' does not match flow collection '{}'",
                expected.collection
            ),
            path: Some(format!("{path}.collection")),
        });
    }
    if expected.collection == "quorum" {
        let actual_quorum = instance
            .get("quorum")
            .and_then(Value::as_object)
            .and_then(|quorum| quorum.get("n"))
            .and_then(Value::as_u64);
        if actual_quorum != expected.quorum {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "graph_join_quorum_mismatch".to_string(),
                message: format!(
                    "graph join quorum '{actual_quorum:?}' does not match flow quorum '{:?}'",
                    expected.quorum
                ),
                path: Some(format!("{path}.quorum.n")),
            });
        }
    }
    let actual_controller = instance
        .get("controllerRole")
        .or_else(|| instance.get("controllerMemberId"))
        .or_else(|| instance.get("controlRole"))
        .or_else(|| instance.get("joinRole"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if expected.collection != "all" && actual_controller != expected.controller_role.as_deref() {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "graph_join_controller_mismatch".to_string(),
            message: format!(
                "graph join controller '{:?}' does not match flow controller '{:?}'",
                actual_controller, expected.controller_role
            ),
            path: Some(format!("{path}.controllerRole")),
        });
    }
}

fn graph_join_collection(instance: &Value) -> String {
    if let Some(collection) = instance
        .get("collection")
        .or_else(|| instance.get("collectionPolicy"))
        .or_else(|| instance.get("collection_policy"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return match collection {
            "any" | "quorum" => collection.to_string(),
            _ => "all".to_string(),
        };
    }
    if instance
        .get("quorum")
        .and_then(Value::as_object)
        .and_then(|quorum| quorum.get("n"))
        .and_then(Value::as_u64)
        .is_some()
    {
        return "quorum".to_string();
    }
    let label = instance
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if label.contains("any") {
        "any".to_string()
    } else {
        "all".to_string()
    }
}

fn validate_graph_member_metadata(
    instance: &Value,
    path: &str,
    expected: &EditorGraphMemberMetadata,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let actual = editor_graph_member_metadata(instance);
    if actual.timeout_ms != expected.timeout_ms {
        diagnostics.push(graph_member_metadata_diagnostic(
            path,
            "timeoutMs",
            "timeout",
            format!("{:?}", actual.timeout_ms),
            format!("{:?}", expected.timeout_ms),
        ));
    }
    if actual.allowed_tools != expected.allowed_tools {
        diagnostics.push(graph_member_metadata_diagnostic(
            path,
            "allowedTools",
            "allowed tools",
            format!("{:?}", actual.allowed_tools),
            format!("{:?}", expected.allowed_tools),
        ));
    }
    if actual.blocked_tools != expected.blocked_tools {
        diagnostics.push(graph_member_metadata_diagnostic(
            path,
            "blockedTools",
            "blocked tools",
            format!("{:?}", actual.blocked_tools),
            format!("{:?}", expected.blocked_tools),
        ));
    }
    if actual.output_format != expected.output_format {
        diagnostics.push(graph_member_metadata_diagnostic(
            path,
            "outputFormat",
            "output format",
            actual.output_format,
            expected.output_format.clone(),
        ));
    }
    if actual.dispatch_mode != expected.dispatch_mode {
        diagnostics.push(graph_member_metadata_diagnostic(
            path,
            "dispatchMode",
            "dispatch mode",
            actual.dispatch_mode,
            expected.dispatch_mode.clone(),
        ));
    }
    if actual.collection_policy != expected.collection_policy {
        diagnostics.push(graph_member_metadata_diagnostic(
            path,
            "collection",
            "collection policy",
            actual.collection_policy,
            expected.collection_policy.clone(),
        ));
    }
}

fn editor_graph_member_metadata(value: &Value) -> EditorGraphMemberMetadata {
    EditorGraphMemberMetadata {
        timeout_ms: editor_u64(value, "timeoutMs").or_else(|| editor_u64(value, "timeout_ms")),
        allowed_tools: editor_string_vec(
            value
                .get("allowedTools")
                .or_else(|| value.get("allowed_tools")),
        ),
        blocked_tools: editor_string_vec(
            value
                .get("blockedTools")
                .or_else(|| value.get("blocked_tools")),
        ),
        output_format: editor_output_format(value),
        dispatch_mode: editor_dispatch_mode(value),
        collection_policy: editor_collection_policy(value),
    }
}

fn graph_member_metadata_diagnostic(
    path: &str,
    field: &str,
    label: &str,
    actual: String,
    expected: String,
) -> MobpackDiagnostic {
    MobpackDiagnostic {
        severity: "error".to_string(),
        code: "graph_member_metadata_mismatch".to_string(),
        message: format!(
            "graph member {label} '{actual}' does not match flow {label} '{expected}'"
        ),
        path: Some(format!("{path}.{field}")),
    }
}

fn validate_graph_member_launch_mode(
    instance: &Value,
    path: &str,
    expected: &EditorGraphLaunchMode,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let actual = editor_graph_launch_mode(
        instance
            .get("launchMode")
            .or_else(|| instance.get("launch_mode")),
    );
    if &actual != expected {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "graph_launch_mode_mismatch".to_string(),
            message: format!(
                "graph launch mode '{actual:?}' does not match flow launch mode '{expected:?}'"
            ),
            path: Some(format!("{path}.launchMode")),
        });
    }
}

fn editor_graph_launch_mode(value: Option<&Value>) -> EditorGraphLaunchMode {
    let Some(mode) = value.and_then(Value::as_object) else {
        return EditorGraphLaunchMode {
            kind: String::new(),
            session_id: None,
            source: None,
            context: None,
            budget_split_policy: String::new(),
        };
    };
    let kind = mode
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let canonical_kind = match kind {
        Some("Fresh") | Some("fresh") => "Fresh".to_string(),
        Some("Resume") | Some("resume") => "Resume".to_string(),
        Some("Fork") | Some("fork") => "Fork".to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    let session_id = mode
        .get("sessionId")
        .or_else(|| mode.get("session_id"))
        .or_else(|| mode.get("bridgeSessionId"))
        .or_else(|| mode.get("bridge_session_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let source = mode
        .get("from")
        .or_else(|| mode.get("sourceMemberId"))
        .or_else(|| mode.get("source_member_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let context = mode
        .get("context")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(canonical_fork_context_value);
    let policy = mode
        .get("budgetSplitPolicy")
        .or_else(|| mode.get("budget_split_policy"))
        .or_else(|| mode.get("budget"))
        .and_then(normalize_budget_split_policy_value);
    EditorGraphLaunchMode {
        kind: canonical_kind,
        session_id,
        source,
        context,
        budget_split_policy: policy.map(|policy| policy.to_string()).unwrap_or_default(),
    }
}

fn editor_launch_mode_entry_mode(item: &Value, mode: &Value) -> EditorGraphLaunchMode {
    let mut launch_mode = editor_graph_launch_mode(Some(mode));
    if let Some(policy) = item
        .get("budget_split_policy")
        .or_else(|| item.get("budgetSplitPolicy"))
    {
        launch_mode.budget_split_policy = normalize_budget_split_policy_value(policy)
            .map(|policy| policy.to_string())
            .unwrap_or_default();
    }
    launch_mode
}

fn validate_deploy_string_enum(
    object: &serde_json::Map<String, Value>,
    key: &str,
    allowed: &[&str],
    code: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    if let Some(value) = object.get(key) {
        let Some(text) = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            if !value.is_null() {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_deploy_string".to_string(),
                    message: format!("deploy.{key} must be a string when present"),
                    path: Some(format!("deploy.{key}")),
                });
            }
            return;
        };
        if !allowed.contains(&text) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: code.to_string(),
                message: format!(
                    "deploy.{key} must be one of {}, got '{text}'",
                    allowed.join(", ")
                ),
                path: Some(format!("deploy.{key}")),
            });
        }
    }
}

fn validate_deploy_non_negative_number(
    object: &serde_json::Map<String, Value>,
    key: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(value) = object.get(key) else {
        return;
    };
    let valid = value
        .as_i64()
        .map(|number| number >= 0)
        .or_else(|| value.as_u64().map(|_| true))
        .or_else(|| {
            value
                .as_f64()
                .map(|number| number >= 0.0 && number.fract() == 0.0)
        })
        .unwrap_or(false);
    if !valid {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_deploy_number".to_string(),
            message: format!("deploy.{key} must be a non-negative integer"),
            path: Some(format!("deploy.{key}")),
        });
    }
}

fn editor_member_ids(document: &MobpackDocument) -> BTreeSet<String> {
    document
        .members
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|member| member.get("id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn editor_input_param_names(flow: &Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Some(steps) = flow.get("steps").and_then(Value::as_array) {
        for step in steps {
            if step.get("type").and_then(Value::as_str) != Some("input") {
                continue;
            }
            if let Some(params) = step.get("inputParams").and_then(Value::as_array) {
                names.extend(params.iter().filter_map(|param| {
                    param
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(ToString::to_string)
                }));
            }
            if let Some(fields) = step.get("fields").and_then(Value::as_str) {
                for line in fields.split(['\n', ',']) {
                    if let Some((name, _)) = line.split_once(':') {
                        let name = name.trim();
                        if !name.is_empty() {
                            names.insert(name.to_string());
                        }
                    }
                }
            }
        }
    }
    names
}

fn editor_member_schema_refs(document: &MobpackDocument) -> BTreeMap<String, String> {
    document
        .members
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|member| {
            let id = member
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())?;
            let schema = member
                .get("schema")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|schema| !schema.is_empty())?;
            Some((id.to_string(), schema.to_string()))
        })
        .collect()
}

fn editor_schema_field_names(schemas: &Value) -> BTreeMap<String, BTreeSet<String>> {
    schemas
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|schema| {
            let id = schema
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())?;
            let fields = schema
                .get("fields")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|field| field.get("name").and_then(Value::as_str))
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(ToString::to_string)
                .collect::<BTreeSet<_>>();
            Some((id.to_string(), fields))
        })
        .collect()
}

fn editor_profile_by_member_id(document: &MobpackDocument) -> BTreeMap<String, String> {
    document
        .members
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|member| {
            let id = member
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())?;
            Some((id.to_string(), editor_profile_name(member)))
        })
        .collect()
}

fn editor_instance_ids(instances: &Value) -> BTreeSet<String> {
    instances
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|instance| instance.get("id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn editor_flow_member_step_ids(flow: &Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Some(steps) = flow.get("steps").and_then(Value::as_array) {
        collect_editor_flow_member_step_ids(steps, &mut ids);
    }
    ids
}

fn editor_flow_member_step_roles(flow: &Value) -> BTreeMap<String, String> {
    let mut roles = BTreeMap::new();
    if let Some(steps) = flow.get("steps").and_then(Value::as_array) {
        collect_editor_flow_member_step_roles(steps, &mut roles);
    }
    roles
}

fn editor_flow_member_step_metadata(flow: &Value) -> BTreeMap<String, EditorGraphMemberMetadata> {
    let mut metadata = BTreeMap::new();
    if let Some(steps) = flow.get("steps").and_then(Value::as_array) {
        collect_editor_flow_member_step_metadata_for_graph(steps, &mut metadata);
    }
    metadata
}

fn editor_flow_member_launch_modes(flow: &Value) -> BTreeMap<String, EditorGraphLaunchMode> {
    let mut launch_modes = BTreeMap::new();
    if let Some(steps) = flow.get("steps").and_then(Value::as_array) {
        collect_editor_flow_member_launch_modes(steps, &mut launch_modes);
    }
    launch_modes
}

fn collect_editor_flow_member_step_ids(steps: &[Value], ids: &mut BTreeSet<String>) {
    for step in steps {
        if step.get("type").and_then(Value::as_str) == Some("member")
            && let Some(id) = step
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
        {
            ids.insert(id.to_string());
        }
        if let Some(nested) = step.get("steps").and_then(Value::as_array) {
            collect_editor_flow_member_step_ids(nested, ids);
        }
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for branch in branches {
                if let Some(branch_steps) = branch.get("steps").and_then(Value::as_array) {
                    collect_editor_flow_member_step_ids(branch_steps, ids);
                }
            }
        }
        if let Some(fallback) = step.get("fallback").and_then(Value::as_array) {
            collect_editor_flow_member_step_ids(fallback, ids);
        }
    }
}

fn collect_editor_flow_member_launch_modes(
    steps: &[Value],
    launch_modes: &mut BTreeMap<String, EditorGraphLaunchMode>,
) {
    for step in steps {
        if step.get("type").and_then(Value::as_str) == Some("member")
            && let Some(id) = step
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
            && let Some(mode) = step.get("launchMode").or_else(|| step.get("launch_mode"))
        {
            launch_modes.insert(id.to_string(), editor_graph_launch_mode(Some(mode)));
        }
        if let Some(nested) = step.get("steps").and_then(Value::as_array) {
            collect_editor_flow_member_launch_modes(nested, launch_modes);
        }
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for branch in branches {
                if let Some(branch_steps) = branch.get("steps").and_then(Value::as_array) {
                    collect_editor_flow_member_launch_modes(branch_steps, launch_modes);
                }
            }
        }
        if let Some(fallback) = step.get("fallback").and_then(Value::as_array) {
            collect_editor_flow_member_launch_modes(fallback, launch_modes);
        }
    }
}

fn collect_editor_flow_member_step_metadata_for_graph(
    steps: &[Value],
    metadata: &mut BTreeMap<String, EditorGraphMemberMetadata>,
) {
    for step in steps {
        if step.get("type").and_then(Value::as_str) == Some("member")
            && let Some(id) = step
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
        {
            metadata.insert(id.to_string(), editor_graph_member_metadata(step));
        }
        if let Some(nested) = step.get("steps").and_then(Value::as_array) {
            collect_editor_flow_member_step_metadata_for_graph(nested, metadata);
        }
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for branch in branches {
                if let Some(branch_steps) = branch.get("steps").and_then(Value::as_array) {
                    collect_editor_flow_member_step_metadata_for_graph(branch_steps, metadata);
                }
            }
        }
        if let Some(fallback) = step.get("fallback").and_then(Value::as_array) {
            collect_editor_flow_member_step_metadata_for_graph(fallback, metadata);
        }
    }
}

fn collect_editor_flow_member_step_roles(steps: &[Value], roles: &mut BTreeMap<String, String>) {
    for step in steps {
        if step.get("type").and_then(Value::as_str) == Some("member")
            && let (Some(id), Some(role)) = (
                step.get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty()),
                step.get("role")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|role| !role.is_empty()),
            )
        {
            roles.insert(id.to_string(), role.to_string());
        }
        if let Some(nested) = step.get("steps").and_then(Value::as_array) {
            collect_editor_flow_member_step_roles(nested, roles);
        }
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for branch in branches {
                if let Some(branch_steps) = branch.get("steps").and_then(Value::as_array) {
                    collect_editor_flow_member_step_roles(branch_steps, roles);
                }
            }
        }
        if let Some(fallback) = step.get("fallback").and_then(Value::as_array) {
            collect_editor_flow_member_step_roles(fallback, roles);
        }
    }
}

fn validate_member_catalog_references(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    let Some(members) = document.members.as_array() else {
        return Vec::new();
    };
    let tool_ids = tool_catalog_response()
        .into_iter()
        .filter_map(|tool| {
            tool.get("id")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect::<BTreeSet<_>>();
    let skill_ids = skill_ids_from_realms(&document.skill_realms)
        .into_iter()
        .chain(skill_ids_from_document_definition(document))
        .collect::<BTreeSet<_>>();
    let model_ids = meerkat_models::catalog()
        .into_iter()
        .map(|entry| entry.id.to_string())
        .collect::<BTreeSet<_>>();
    let mut diagnostics = Vec::new();

    for (member_index, member) in members.iter().enumerate() {
        if let Some(model) = member.get("model").filter(|value| !value.is_null()) {
            match model.as_str().map(str::trim) {
                Some("") => {}
                Some(model_id) => {
                    if !model_ids.contains(model_id) {
                        diagnostics.push(MobpackDiagnostic {
                            severity: "warning".to_string(),
                            code: "unknown_model_ref".to_string(),
                            message: format!(
                                "member references model '{model_id}', but it is not present in the current MobKit editor model catalog"
                            ),
                            path: Some(format!("members[{member_index}].model")),
                        });
                    }
                }
                None => {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "invalid_model_ref".to_string(),
                        message: "member model references must be strings when set".to_string(),
                        path: Some(format!("members[{member_index}].model")),
                    });
                }
            }
        }

        if let Some(tools) = member.get("tools").and_then(Value::as_array) {
            for (tool_index, tool) in tools.iter().enumerate() {
                let Some(tool_id) = tool
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "invalid_tool_ref".to_string(),
                        message: "member tool references must be non-empty strings".to_string(),
                        path: Some(format!("members[{member_index}].tools[{tool_index}]")),
                    });
                    continue;
                };
                if !tool_ids.contains(tool_id) {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "unknown_tool_ref".to_string(),
                        message: format!(
                            "member references tool '{tool_id}', but it is not present in MobKit tool_catalog"
                        ),
                        path: Some(format!("members[{member_index}].tools[{tool_index}]")),
                    });
                }
            }
        }

        if let Some(skills) = member.get("skills").and_then(Value::as_array) {
            for (skill_index, skill) in skills.iter().enumerate() {
                let Some(skill_id) = skill
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "invalid_skill_ref".to_string(),
                        message: "member skill references must be non-empty strings".to_string(),
                        path: Some(format!("members[{member_index}].skills[{skill_index}]")),
                    });
                    continue;
                };
                if !skill_ids.contains(skill_id) {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "unknown_skill_ref".to_string(),
                        message: format!(
                            "member references skill '{skill_id}', but it is not present in the mobpack skill realms"
                        ),
                        path: Some(format!("members[{member_index}].skills[{skill_index}]")),
                    });
                }
            }
        }

        if let Some(provider_params) = member
            .get("providerParams")
            .or_else(|| member.get("provider_params"))
            .filter(|value| !value.is_null())
            && !provider_params.is_object()
        {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_provider_params".to_string(),
                message: "member providerParams must be a JSON object".to_string(),
                path: Some(format!("members[{member_index}].providerParams")),
            });
        }

        if let Some(backend) = member.get("backend").filter(|value| !value.is_null()) {
            let valid = backend
                .as_str()
                .map(str::trim)
                .is_some_and(|value| value.is_empty() || matches!(value, "session" | "external"));
            if !valid {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_profile_backend".to_string(),
                    message: "member backend must be 'session' or 'external' when set".to_string(),
                    path: Some(format!("members[{member_index}].backend")),
                });
            }
        }

        if let Some(value) = member
            .get("maxInlinePeerNotifications")
            .or_else(|| member.get("max_inline_peer_notifications"))
            .filter(|value| !value.is_null())
        {
            let valid = value
                .as_i64()
                .is_some_and(|threshold| i32::try_from(threshold).is_ok() && threshold >= -1);
            if !valid {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_max_inline_peer_notifications".to_string(),
                    message: "member maxInlinePeerNotifications must be an integer >= -1 when set"
                        .to_string(),
                    path: Some(format!(
                        "members[{member_index}].maxInlinePeerNotifications"
                    )),
                });
            }
        }
    }

    diagnostics
}

fn validate_editor_member_identities(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    let Some(members) = document.members.as_array() else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    let mut seen = BTreeSet::new();
    for (member_index, member) in members.iter().enumerate() {
        let Some(member_id) = member
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_editor_member_id".to_string(),
                message: "editor members must have a non-empty id".to_string(),
                path: Some(format!("members[{member_index}].id")),
            });
            continue;
        };
        if !seen.insert(member_id.to_string()) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "duplicate_editor_member_id".to_string(),
                message: format!("editor member id '{member_id}' is used more than once"),
                path: Some(format!("members[{member_index}].id")),
            });
        }
    }
    diagnostics
}

fn validate_member_profile_bindings(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    let Some(members) = document.members.as_array() else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    for (member_index, member) in members.iter().enumerate() {
        let binding_value = member
            .get("profileBinding")
            .or_else(|| member.get("profile_binding"));
        let realm_profile = member
            .get("realmProfile")
            .or_else(|| member.get("realm_profile"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let binding = match binding_value {
            Some(value) => match value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some("inline") => "inline",
                Some("realm_profile") => "realm_profile",
                Some(other) => {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "invalid_profile_binding".to_string(),
                        message: format!(
                            "member profile binding '{other}' is not backed by current MobKit profile_binding semantics"
                        ),
                        path: Some(format!("members[{member_index}].profileBinding")),
                    });
                    ""
                }
                None => {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "invalid_profile_binding".to_string(),
                        message: "member profile binding must be a non-empty string when present"
                            .to_string(),
                        path: Some(format!("members[{member_index}].profileBinding")),
                    });
                    ""
                }
            },
            None if realm_profile.is_some() => "realm_profile",
            None => {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "missing_profile_binding".to_string(),
                    message: "member profile binding must be explicitly authored".to_string(),
                    path: Some(format!("members[{member_index}].profileBinding")),
                });
                ""
            }
        };
        match member
            .get("runtimeMode")
            .or_else(|| member.get("runtime_mode"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(runtime_mode) if runtime_mode_values().iter().any(|mode| mode == runtime_mode) => {}
            Some(runtime_mode) => diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_runtime_mode".to_string(),
                message: format!(
                    "member runtime mode '{runtime_mode}' is not backed by current MobKit runtime_mode semantics"
                ),
                path: Some(format!("members[{member_index}].runtimeMode")),
            }),
            None => diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_runtime_mode".to_string(),
                message: "member runtime mode must be explicitly authored".to_string(),
                path: Some(format!("members[{member_index}].runtimeMode")),
            }),
        }
        if binding == "realm_profile" && realm_profile.is_none() {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_realm_profile_ref".to_string(),
                message: "realm_profile members must name the target realm profile".to_string(),
                path: Some(format!("members[{member_index}].realmProfile")),
            });
        }
        if binding == "realm_profile" && realm_profile.is_some() {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "unsupported_realm_profile_pack_binding".to_string(),
                message: "realm_profile members are not deployable in mobpack archives; use an inline profile definition".to_string(),
                path: Some(format!("members[{member_index}].profileBinding")),
            });
        }
        if binding == "inline" && realm_profile.is_some() {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "conflicting_profile_binding".to_string(),
                message: "inline profile members must not also define realmProfile".to_string(),
                path: Some(format!("members[{member_index}].realmProfile")),
            });
        }
    }
    diagnostics
}

fn validate_skill_realms(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    if document.skill_realms.is_null() {
        return Vec::new();
    }
    let Some(realms) = document.skill_realms.as_array() else {
        return vec![MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_skill_realms".to_string(),
            message: "document.skill_realms must be an array".to_string(),
            path: Some("skill_realms".to_string()),
        }];
    };

    let mut diagnostics = Vec::new();
    let mut seen_skill_ids = BTreeMap::<String, String>::new();
    for (realm_index, realm) in realms.iter().enumerate() {
        let realm_path = format!("skill_realms[{realm_index}]");
        let realm_id = realm
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        if realm_id.is_empty() {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_skill_realm_id".to_string(),
                message: "skill realms must have non-empty string ids".to_string(),
                path: Some(format!("{realm_path}.id")),
            });
        }
        if is_starter_skill_marker(realm_id) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_starter_skill_realm_id".to_string(),
                message:
                    "starter skill realms are prototype data and cannot be used in MobKit mobpacks"
                        .to_string(),
                path: Some(format!("{realm_path}.id")),
            });
        }
        if realm
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(is_starter_skill_marker)
        {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_starter_skill_realm_source".to_string(),
                message:
                    "starter skill realms are prototype data and cannot be used in MobKit mobpacks"
                        .to_string(),
                path: Some(format!("{realm_path}.source")),
            });
        }
        let Some(skills) = realm.get("skills").and_then(Value::as_array) else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_skill_realm_skills".to_string(),
                message: "skill realm skills must be an array".to_string(),
                path: Some(format!("{realm_path}.skills")),
            });
            continue;
        };
        for (skill_index, skill) in skills.iter().enumerate() {
            let skill_path = format!("{realm_path}.skills[{skill_index}]");
            let Some(skill_id) = skill
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_skill_id".to_string(),
                    message: "skill definitions must have non-empty string ids".to_string(),
                    path: Some(format!("{skill_path}.id")),
                });
                continue;
            };
            if let Some(first_path) =
                seen_skill_ids.insert(skill_id.to_string(), skill_path.clone())
            {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "duplicate_skill_id".to_string(),
                    message: format!(
                        "skill id '{skill_id}' is defined more than once; first definition is at {first_path}"
                    ),
                    path: Some(format!("{skill_path}.id")),
                });
            }
            if skill
                .get("source")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(is_starter_skill_marker)
            {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_starter_skill_source".to_string(),
                    message: "starter skills are prototype data; use inline, path, or imported MobKit skill definitions".to_string(),
                    path: Some(format!("{skill_path}.source")),
                });
            }
            if skill
                .get("origin")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(is_starter_skill_marker)
            {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_starter_skill_origin".to_string(),
                    message: "starter skills are prototype data; use inline, path, or imported MobKit skill definitions".to_string(),
                    path: Some(format!("{skill_path}.origin")),
                });
            }
        }
    }
    diagnostics
}

fn is_starter_skill_marker(value: &str) -> bool {
    matches!(value.trim(), "starter" | "mobkit/starters")
}

fn is_starter_provenance_diagnostic(code: &str) -> bool {
    matches!(
        code,
        "invalid_starter_skill_realm_id"
            | "invalid_starter_skill_realm_source"
            | "invalid_starter_skill_source"
            | "invalid_starter_skill_origin"
    )
}

fn validate_selected_skill_sources(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    let mut diagnostics = Vec::new();
    for (skill_id, source) in selected_skill_sources(document) {
        let source_path = format!("skill_realms.skills[{skill_id}].source");
        let source_kind = match source.get("source") {
            Some(Value::String(text)) => text.trim(),
            Some(_) => {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_selected_skill_source".to_string(),
                    message: format!(
                        "selected skill '{skill_id}' source must be 'inline' or 'path'"
                    ),
                    path: Some(source_path),
                });
                continue;
            }
            None => "inline",
        };
        let source_kind = if source_kind.is_empty() {
            "inline"
        } else {
            source_kind
        };
        match source_kind {
            "inline" => {
                let content = source
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("");
                if content.is_empty() {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "empty_selected_inline_skill".to_string(),
                        message: format!(
                            "selected inline skill '{skill_id}' must include non-empty content"
                        ),
                        path: Some(format!("skill_realms.skills[{skill_id}].content")),
                    });
                }
            }
            "path" => {}
            other => diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "invalid_selected_skill_source".to_string(),
                message: format!(
                    "selected skill '{skill_id}' uses unsupported source '{other}'; use inline or path"
                ),
                path: Some(source_path),
            }),
        }
    }
    diagnostics
}

fn validate_selected_path_skill_files(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut archive_paths = BTreeMap::<String, Vec<u8>>::new();
    for (skill_id, source) in selected_skill_sources(document) {
        let source_kind = source
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("inline");
        if source_kind != "path" {
            continue;
        }
        let archive_path = packed_skill_archive_path(&skill_id, &source);
        let bytes = if let Some(content) = source.get("content").and_then(Value::as_str) {
            content.as_bytes().to_vec()
        } else {
            let Some(path) = source
                .get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "selected_skill_path_missing".to_string(),
                    message: format!("selected path skill '{skill_id}' has no filesystem path"),
                    path: Some(format!("skill_realms.skills[{skill_id}].path")),
                });
                continue;
            };
            if source
                .get("realm_source")
                .and_then(Value::as_str)
                .map(str::trim)
                != Some("filesystem")
                && !Path::new(path).is_absolute()
            {
                continue;
            }
            match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(err) => {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "selected_skill_file_unreadable".to_string(),
                        message: format!(
                            "selected path skill '{skill_id}' cannot be read from {path}: {err}"
                        ),
                        path: Some(format!("skill_realms.skills[{skill_id}].path")),
                    });
                    continue;
                }
            }
        };
        if let Some(existing) = archive_paths.get(&archive_path) {
            if existing != &bytes {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "selected_skill_archive_path_conflict".to_string(),
                    message: format!(
                        "selected path skill '{skill_id}' resolves to archive path '{archive_path}' with different content"
                    ),
                    path: Some(format!("skill_realms.skills[{skill_id}].path")),
                });
            }
        } else {
            archive_paths.insert(archive_path, bytes);
        }
    }
    diagnostics
}

fn validate_editor_flow_step_tool_references(document: &MobpackDocument) -> Vec<MobpackDiagnostic> {
    let tool_ids = tool_catalog_response()
        .into_iter()
        .filter_map(|tool| {
            tool.get("id")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect::<BTreeSet<_>>();
    let member_tools = editor_member_tools_by_id(document);
    let mut diagnostics = Vec::new();
    collect_editor_flow_step_tool_reference_diagnostics(
        document.flow.get("steps").and_then(Value::as_array),
        "flow.steps",
        &tool_ids,
        &member_tools,
        &mut diagnostics,
    );
    diagnostics
}

fn editor_member_tools_by_id(document: &MobpackDocument) -> BTreeMap<String, BTreeSet<String>> {
    document
        .members
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|member| {
            let id = member
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())?;
            let tools = string_vec(member.get("tools")).into_iter().collect();
            Some((id.to_string(), tools))
        })
        .collect()
}

fn collect_editor_flow_step_tool_reference_diagnostics(
    steps: Option<&Vec<Value>>,
    path: &str,
    tool_ids: &BTreeSet<String>,
    member_tools: &BTreeMap<String, BTreeSet<String>>,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(steps) = steps else {
        return;
    };
    for (index, step) in steps.iter().enumerate() {
        let step_path = format!("{path}[{index}]");
        if step.get("type").and_then(Value::as_str) == Some("member") {
            let member_id = step
                .get("role")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            for field in [
                "allowedTools",
                "allowed_tools",
                "blockedTools",
                "blocked_tools",
            ] {
                if let Some(value) = step.get(field) {
                    validate_editor_flow_step_tool_list(
                        value,
                        &format!("{step_path}.{field}"),
                        member_id,
                        tool_ids,
                        member_tools,
                        field.starts_with("allowed"),
                        diagnostics,
                    );
                }
            }
        }
        if let Some(nested) = step.get("steps").and_then(Value::as_array) {
            collect_editor_flow_step_tool_reference_diagnostics(
                Some(nested),
                &format!("{step_path}.steps"),
                tool_ids,
                member_tools,
                diagnostics,
            );
        }
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for (branch_index, branch) in branches.iter().enumerate() {
                collect_editor_flow_step_tool_reference_diagnostics(
                    branch.get("steps").and_then(Value::as_array),
                    &format!("{step_path}.branches[{branch_index}].steps"),
                    tool_ids,
                    member_tools,
                    diagnostics,
                );
            }
        }
        collect_editor_flow_step_tool_reference_diagnostics(
            step.get("fallback").and_then(Value::as_array),
            &format!("{step_path}.fallback"),
            tool_ids,
            member_tools,
            diagnostics,
        );
    }
}

fn validate_editor_flow_step_tool_list(
    value: &Value,
    path: &str,
    member_id: Option<&str>,
    tool_ids: &BTreeSet<String>,
    member_tools: &BTreeMap<String, BTreeSet<String>>,
    require_member_tool: bool,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    match value {
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let item_path = format!("{path}[{index}]");
                let Some(tool_id) = item
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "invalid_step_tool_ref".to_string(),
                        message: "flow step tool references must be non-empty strings".to_string(),
                        path: Some(item_path),
                    });
                    continue;
                };
                validate_editor_flow_step_tool_ref(
                    tool_id,
                    &item_path,
                    member_id,
                    tool_ids,
                    member_tools,
                    require_member_tool,
                    diagnostics,
                );
            }
        }
        Value::String(text) => {
            for tool_id in text
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                validate_editor_flow_step_tool_ref(
                    tool_id,
                    path,
                    member_id,
                    tool_ids,
                    member_tools,
                    require_member_tool,
                    diagnostics,
                );
            }
        }
        Value::Null => {}
        _ => diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_step_tool_ref".to_string(),
            message: "flow step tool references must be an array or comma-separated string"
                .to_string(),
            path: Some(path.to_string()),
        }),
    }
}

fn validate_editor_flow_step_tool_ref(
    tool_id: &str,
    path: &str,
    member_id: Option<&str>,
    tool_ids: &BTreeSet<String>,
    member_tools: &BTreeMap<String, BTreeSet<String>>,
    require_member_tool: bool,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    if !tool_ids.contains(tool_id) {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "unknown_step_tool_ref".to_string(),
            message: format!(
                "flow step references tool '{tool_id}', but it is not present in MobKit tool_catalog"
            ),
            path: Some(path.to_string()),
        });
        return;
    }
    let Some(member_id) = member_id else {
        return;
    };
    if require_member_tool
        && let Some(tools) = member_tools.get(member_id)
        && !tools.contains(tool_id)
    {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "step_tool_not_enabled_on_member".to_string(),
            message: format!(
                "flow step references tool '{tool_id}', but member '{member_id}' has not enabled it"
            ),
            path: Some(path.to_string()),
        });
    }
}

fn skill_ids_from_realms(realms: &Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Some(realms) = realms.as_array() {
        for realm in realms {
            if let Some(skills) = realm.get("skills").and_then(Value::as_array) {
                ids.extend(skills.iter().filter_map(|skill| {
                    skill
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                }));
            }
        }
    }
    ids
}

fn editor_profile_backend(member: &Value) -> Option<&str> {
    member
        .get("backend")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn editor_max_inline_peer_notifications(member: &Value) -> Option<i32> {
    let value = member
        .get("maxInlinePeerNotifications")
        .or_else(|| member.get("max_inline_peer_notifications"))?;
    if value.is_null() {
        return None;
    }
    let threshold = value.as_i64()?;
    i32::try_from(threshold).ok()
}

fn validate_mob_settings_match_definition(
    document: &MobpackDocument,
    definition: &MobDefinition,
) -> Vec<MobpackDiagnostic> {
    if document.mob_settings.is_null() {
        return Vec::new();
    }
    let Some(settings) = document.mob_settings.as_object() else {
        return vec![MobpackDiagnostic {
            severity: "error".to_string(),
            code: "invalid_mob_settings".to_string(),
            message: "mob_settings must be an object".to_string(),
            path: Some("mob_settings".to_string()),
        }];
    };
    let mut diagnostics = Vec::new();
    let expected = mob_settings_from_definition(definition);
    let expected = expected.as_object().expect("mob settings object");

    compare_mob_setting_string(
        settings,
        expected,
        "orchestrator",
        "editor_mob_orchestrator_mismatch",
        &mut diagnostics,
    );
    compare_mob_setting_bool(
        settings,
        expected,
        "autoWireOrchestrator",
        "editor_mob_auto_wire_mismatch",
        &mut diagnostics,
    );
    compare_mob_setting_string(
        settings,
        expected,
        "backendDefault",
        "editor_mob_backend_default_mismatch",
        &mut diagnostics,
    );
    compare_mob_setting_string(
        settings,
        expected,
        "externalAddressBase",
        "editor_mob_external_address_base_mismatch",
        &mut diagnostics,
    );
    let editor_wiring = normalized_role_wiring(settings.get("roleWiring"));
    let definition_wiring = normalized_role_wiring(expected.get("roleWiring"));
    if editor_wiring != definition_wiring {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: "editor_mob_role_wiring_mismatch".to_string(),
            message: "editor mob role wiring does not match mob.toml wiring rules".to_string(),
            path: Some("mob_settings.roleWiring".to_string()),
        });
    }
    compare_advanced_mob_settings(settings, expected, &mut diagnostics);
    diagnostics
}

fn compare_advanced_mob_settings(
    settings: &serde_json::Map<String, Value>,
    expected: &serde_json::Map<String, Value>,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let actual_advanced = match settings.get("advanced").filter(|value| !value.is_null()) {
        Some(value) => match value.as_object() {
            Some(object) => Some(object),
            None => {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "invalid_mob_setting_advanced".to_string(),
                    message: "mob_settings.advanced must be an object".to_string(),
                    path: Some("mob_settings.advanced".to_string()),
                });
                return;
            }
        },
        None => None,
    };
    let expected_advanced = expected
        .get("advanced")
        .and_then(Value::as_object)
        .expect("expected advanced mob settings");
    for (key, code) in [
        ("topology", "editor_mob_topology_mismatch"),
        ("supervisor", "editor_mob_supervisor_mismatch"),
        ("limits", "editor_mob_limits_mismatch"),
        ("spawnPolicy", "editor_mob_spawn_policy_mismatch"),
        ("eventRouter", "editor_mob_event_router_mismatch"),
    ] {
        let actual = actual_advanced.and_then(|advanced| {
            advanced
                .get(key)
                .or_else(|| advanced.get(&camel_to_snake(key)))
        });
        let expected = expected_advanced.get(key).unwrap_or(&Value::Null);
        if actual.unwrap_or(&Value::Null) != expected {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: code.to_string(),
                message: format!("editor mob advanced setting '{key}' does not match mob.toml"),
                path: Some(format!("mob_settings.advanced.{key}")),
            });
        }
    }
}

fn camel_to_snake(key: &str) -> String {
    let mut out = String::new();
    for (index, ch) in key.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn compare_mob_setting_string(
    settings: &serde_json::Map<String, Value>,
    expected: &serde_json::Map<String, Value>,
    key: &str,
    code: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(value) = settings.get(key).filter(|value| !value.is_null()) else {
        return;
    };
    let Some(actual) = value.as_str() else {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: format!("invalid_mob_setting_{key}"),
            message: format!("mob_settings.{key} must be a string"),
            path: Some(format!("mob_settings.{key}")),
        });
        return;
    };
    let expected = expected
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default();
    if actual.trim() != expected {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: code.to_string(),
            message: format!("editor mob setting '{key}' does not match mob.toml"),
            path: Some(format!("mob_settings.{key}")),
        });
    }
}

fn compare_mob_setting_bool(
    settings: &serde_json::Map<String, Value>,
    expected: &serde_json::Map<String, Value>,
    key: &str,
    code: &str,
    diagnostics: &mut Vec<MobpackDiagnostic>,
) {
    let Some(value) = settings.get(key).filter(|value| !value.is_null()) else {
        return;
    };
    let Some(actual) = value.as_bool() else {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: format!("invalid_mob_setting_{key}"),
            message: format!("mob_settings.{key} must be a boolean"),
            path: Some(format!("mob_settings.{key}")),
        });
        return;
    };
    let expected = expected
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or_default();
    if actual != expected {
        diagnostics.push(MobpackDiagnostic {
            severity: "error".to_string(),
            code: code.to_string(),
            message: format!("editor mob setting '{key}' does not match mob.toml"),
            path: Some(format!("mob_settings.{key}")),
        });
    }
}

fn normalized_role_wiring(value: Option<&Value>) -> Option<Vec<(String, String)>> {
    let value = value?;
    let rules = value.as_array()?;
    let mut out = Vec::new();
    for rule in rules {
        let a = rule.get("a").and_then(Value::as_str)?.trim().to_string();
        let b = rule.get("b").and_then(Value::as_str)?.trim().to_string();
        if a.is_empty() || b.is_empty() {
            return None;
        }
        out.push((a, b));
    }
    Some(out)
}

fn validate_editor_projection_matches_definition(
    document: &MobpackDocument,
    definition: &MobDefinition,
) -> Vec<MobpackDiagnostic> {
    let Some(members) = document.members.as_array() else {
        return Vec::new();
    };
    let schemas = editor_schema_map(&document.schemas);
    let profiles = definition
        .profiles
        .iter()
        .map(|(name, binding)| (name.to_string(), binding))
        .collect::<BTreeMap<_, _>>();
    let mut diagnostics = Vec::new();
    let mut seen_profiles = BTreeSet::new();

    for (member_index, member) in members.iter().enumerate() {
        if member
            .get("missing")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "missing_editor_member_profile".to_string(),
                message: "editor member is marked missing and cannot be exported as a real profile"
                    .to_string(),
                path: Some(format!("members[{member_index}]")),
            });
            continue;
        }
        let profile_name = editor_profile_name(member);
        if !seen_profiles.insert(profile_name.clone()) {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "duplicate_editor_profile".to_string(),
                message: format!("multiple editor members compile to profile '{profile_name}'"),
                path: Some(format!("members[{member_index}]")),
            });
            continue;
        }
        let Some(binding) = profiles.get(&profile_name) else {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "editor_profile_missing_from_mob_toml".to_string(),
                message: format!(
                    "editor member compiles to profile '{profile_name}', but mob.toml has no matching profile"
                ),
                path: Some(format!("members[{member_index}]")),
            });
            continue;
        };
        let ProfileBinding::Inline(profile) = binding else {
            continue;
        };
        if let Some(model) = member
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            && model != profile.model.to_string()
        {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "editor_profile_model_mismatch".to_string(),
                message: format!(
                    "editor member model '{model}' does not match profile '{profile_name}' model '{}'",
                    profile.model
                ),
                path: Some(format!("members[{member_index}].model")),
            });
        }
        if let Some(member_tools) = member.get("tools").and_then(Value::as_array) {
            let editor_tools = member_tools
                .iter()
                .filter_map(|tool| tool.as_str().map(str::trim))
                .filter(|tool| !tool.is_empty())
                .map(ToString::to_string)
                .collect::<BTreeSet<_>>();
            let profile_tools = tool_ids_from_config(&profile.tools)
                .into_iter()
                .collect::<BTreeSet<_>>();
            if editor_tools != profile_tools {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "editor_profile_tools_mismatch".to_string(),
                    message: format!(
                        "editor member tools do not match profile '{profile_name}' tools in mob.toml"
                    ),
                    path: Some(format!("members[{member_index}].tools")),
                });
            }
        }
        if let Some(member_skills) = member.get("skills").and_then(Value::as_array) {
            let editor_skills = member_skills
                .iter()
                .filter_map(|skill| skill.as_str().map(str::trim))
                .filter(|skill| !skill.is_empty())
                .map(ToString::to_string)
                .collect::<BTreeSet<_>>();
            let profile_skills = profile
                .skills
                .iter()
                .map(ToString::to_string)
                .collect::<BTreeSet<_>>();
            if editor_skills != profile_skills {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "editor_profile_skills_mismatch".to_string(),
                    message: format!(
                        "editor member skills do not match profile '{profile_name}' skills in mob.toml"
                    ),
                    path: Some(format!("members[{member_index}].skills")),
                });
            }
        }
        let editor_provider_params = member
            .get("providerParams")
            .or_else(|| member.get("provider_params"))
            .filter(|value| !value.is_null());
        match (editor_provider_params, profile.provider_params.as_ref()) {
            (Some(editor), Some(profile_params)) => {
                if editor != profile_params {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "editor_profile_provider_params_mismatch".to_string(),
                        message: format!(
                            "editor member provider params do not match profile '{profile_name}' provider_params in mob.toml"
                        ),
                        path: Some(format!("members[{member_index}].providerParams")),
                    });
                }
            }
            (Some(_), None) => diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "editor_profile_provider_params_mismatch".to_string(),
                message: format!(
                    "editor member declares provider params, but profile '{profile_name}' has none"
                ),
                path: Some(format!("members[{member_index}].providerParams")),
            }),
            (None, Some(_)) => diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "editor_profile_provider_params_missing".to_string(),
                message: format!(
                    "profile '{profile_name}' has provider_params, but the editor member has none"
                ),
                path: Some(format!("members[{member_index}].providerParams")),
            }),
            (None, None) => {}
        }
        let editor_backend = editor_profile_backend(member);
        let profile_backend = profile.backend.map(|backend| backend.as_str());
        if editor_backend != profile_backend {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "editor_profile_backend_mismatch".to_string(),
                message: format!(
                    "editor member backend does not match profile '{profile_name}' backend in mob.toml"
                ),
                path: Some(format!("members[{member_index}].backend")),
            });
        }
        let editor_max_inline = editor_max_inline_peer_notifications(member);
        if editor_max_inline != profile.max_inline_peer_notifications {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "editor_profile_max_inline_peer_notifications_mismatch".to_string(),
                message: format!(
                    "editor member max inline peer notifications does not match profile '{profile_name}' max_inline_peer_notifications in mob.toml"
                ),
                path: Some(format!(
                    "members[{member_index}].maxInlinePeerNotifications"
                )),
            });
        }
        let editor_schema_id = member
            .get("schema")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|schema| !schema.is_empty());
        match (editor_schema_id, profile.output_schema.as_ref()) {
            (Some(_), None) => {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "editor_profile_schema_mismatch".to_string(),
                    message: format!(
                        "editor member declares an output schema, but profile '{profile_name}' has no output_schema"
                    ),
                    path: Some(format!("members[{member_index}].schema")),
                });
            }
            (None, Some(_)) => {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "editor_profile_schema_missing".to_string(),
                    message: format!(
                        "profile '{profile_name}' has an output_schema, but the editor member has no schema selected"
                    ),
                    path: Some(format!("members[{member_index}].schema")),
                });
            }
            (Some(schema_id), Some(profile_schema)) => {
                let Some(editor_schema) = schemas.get(schema_id) else {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "editor_schema_missing".to_string(),
                        message: format!(
                            "editor member references schema '{schema_id}', but document.schemas does not define it"
                        ),
                        path: Some(format!("members[{member_index}].schema")),
                    });
                    continue;
                };
                let compiled_schema = editor_schema_to_json_schema(editor_schema);
                if normalize_json_schema_for_compare(&compiled_schema)
                    != normalize_json_schema_for_compare(profile_schema)
                {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "editor_profile_schema_mismatch".to_string(),
                        message: format!(
                            "editor schema '{schema_id}' does not match profile '{profile_name}' output_schema in mob.toml"
                        ),
                        path: Some(format!("members[{member_index}].schema")),
                    });
                }
            }
            (None, None) => {}
        }
    }

    diagnostics
}

fn validate_editor_flow_members_match_definition(
    document: &MobpackDocument,
    definition: &MobDefinition,
) -> Vec<MobpackDiagnostic> {
    let profile_by_member = editor_profile_by_member_id(document);
    let mut expected = BTreeMap::<(String, String), usize>::new();
    collect_editor_flow_member_turns(
        document.flow.get("steps").and_then(Value::as_array),
        &profile_by_member,
        &mut expected,
    );
    if expected.is_empty() {
        return Vec::new();
    }

    let actual = definition_flow_step_turns(document, definition);
    let mut diagnostics = Vec::new();
    for ((profile, message), expected_count) in expected {
        let actual_count = actual
            .get(&(profile.clone(), message.clone()))
            .copied()
            .unwrap_or_default();
        if actual_count < expected_count {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "editor_flow_step_missing_from_mob_toml".to_string(),
                message: format!(
                    "editor flow member turn for profile '{profile}' is not present in mob.toml"
                ),
                path: Some("flow.steps".to_string()),
            });
        }
    }
    diagnostics
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FlowStepMetadataKey {
    profile: String,
    message: String,
    timeout_ms: Option<u64>,
    allowed_tools: Vec<String>,
    blocked_tools: Vec<String>,
    output_format: String,
    dispatch_mode: String,
    collection_policy: String,
}

fn validate_editor_flow_step_metadata_matches_definition(
    document: &MobpackDocument,
    definition: &MobDefinition,
) -> Vec<MobpackDiagnostic> {
    let profile_by_member = editor_profile_by_member_id(document);
    let mut expected = BTreeMap::<FlowStepMetadataKey, usize>::new();
    collect_editor_flow_step_metadata(
        document.flow.get("steps").and_then(Value::as_array),
        &profile_by_member,
        &mut expected,
    );
    if expected.is_empty() {
        return Vec::new();
    }
    let actual = definition_flow_step_metadata(document, definition);
    let mut diagnostics = Vec::new();
    for (key, expected_count) in expected {
        let actual_count = actual.get(&key).copied().unwrap_or_default();
        if actual_count < expected_count {
            diagnostics.push(MobpackDiagnostic {
                severity: "error".to_string(),
                code: "editor_flow_step_metadata_mismatch".to_string(),
                message: format!(
                    "editor flow member turn metadata for profile '{}' does not match mob.toml",
                    key.profile
                ),
                path: Some("flow.steps".to_string()),
            });
        }
    }
    diagnostics
}

fn collect_editor_flow_step_metadata(
    steps: Option<&Vec<Value>>,
    profile_by_member: &BTreeMap<String, String>,
    out: &mut BTreeMap<FlowStepMetadataKey, usize>,
) {
    let Some(steps) = steps else {
        return;
    };
    for step in steps {
        let step_type = step.get("type").and_then(Value::as_str).unwrap_or_default();
        if step_type == "member"
            && let Some(member_id) = step
                .get("role")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            && let Some(profile) = profile_by_member.get(member_id)
            && let Some(message) = editor_step_instruction(step)
        {
            let key = FlowStepMetadataKey {
                profile: profile.clone(),
                message,
                timeout_ms: editor_u64(step, "timeoutMs")
                    .or_else(|| editor_u64(step, "timeout_ms")),
                allowed_tools: editor_string_vec(
                    step.get("allowedTools")
                        .or_else(|| step.get("allowed_tools")),
                ),
                blocked_tools: editor_string_vec(
                    step.get("blockedTools")
                        .or_else(|| step.get("blocked_tools")),
                ),
                output_format: editor_output_format(step),
                dispatch_mode: editor_dispatch_mode(step),
                collection_policy: editor_collection_policy(step),
            };
            *out.entry(key).or_default() += 1;
        }
        if let Some(nested) = step.get("steps").and_then(Value::as_array) {
            collect_editor_flow_step_metadata(Some(nested), profile_by_member, out);
        }
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for branch in branches {
                collect_editor_flow_step_metadata(
                    branch.get("steps").and_then(Value::as_array),
                    profile_by_member,
                    out,
                );
            }
        }
        collect_editor_flow_step_metadata(
            step.get("fallback").and_then(Value::as_array),
            profile_by_member,
            out,
        );
    }
}

fn definition_flow_step_metadata(
    document: &MobpackDocument,
    definition: &MobDefinition,
) -> BTreeMap<FlowStepMetadataKey, usize> {
    let flow_name = document
        .flow
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let primary_flow = flow_name
        .and_then(|name| definition.flows.get_key_value(name))
        .or_else(|| definition.flows.get_key_value("main"))
        .or_else(|| definition.flows.iter().next());
    let mut out = BTreeMap::new();
    if let Some((_, flow)) = primary_flow {
        for step in flow.steps.values() {
            let key = FlowStepMetadataKey {
                profile: step.role.to_string(),
                message: step.message.text_content(),
                timeout_ms: step.timeout_ms,
                allowed_tools: step.allowed_tools.clone().unwrap_or_default(),
                blocked_tools: step.blocked_tools.clone().unwrap_or_default(),
                output_format: step_output_format_string(&step.output_format).to_string(),
                dispatch_mode: dispatch_mode_string(&step.dispatch_mode).to_string(),
                collection_policy: definition_collection_policy_key(&step.collection_policy),
            };
            *out.entry(key).or_default() += 1;
        }
    }
    out
}

fn editor_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn editor_step_instruction(step: &Value) -> Option<String> {
    step.get("instruction")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn editor_string_vec(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        Some(Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn editor_output_format(step: &Value) -> String {
    explicit_editor_output_format(step).unwrap_or_else(|| "json".to_string())
}

fn explicit_editor_output_format(step: &Value) -> Option<String> {
    match step
        .get("outputFormat")
        .or_else(|| step.get("output_format"))
        .and_then(Value::as_str)
        .map(str::trim)
    {
        Some("text") => Some("text".to_string()),
        Some("json") => Some("json".to_string()),
        _ => None,
    }
}

fn editor_dispatch_mode(step: &Value) -> String {
    explicit_editor_dispatch_mode(step).unwrap_or_else(|| "fan_out".to_string())
}

fn explicit_editor_dispatch_mode(step: &Value) -> Option<String> {
    match step
        .get("dispatch")
        .or_else(|| step.get("dispatchMode"))
        .or_else(|| step.get("dispatch_mode"))
        .and_then(Value::as_str)
        .map(str::trim)
    {
        Some("one_to_one") => Some("one_to_one".to_string()),
        Some("fan_in") => Some("fan_in".to_string()),
        Some("fan_out") => Some("fan_out".to_string()),
        _ => None,
    }
}

fn editor_collection_policy(step: &Value) -> String {
    required_editor_collection_policy(step).unwrap_or_else(|| "all".to_string())
}

fn explicit_editor_collection_policy_toml(step: &Value) -> Option<String> {
    required_editor_collection_policy(step).map(|policy| collection_policy_toml_from_key(&policy))
}

fn required_editor_collection_policy(step: &Value) -> Option<String> {
    let policy = step
        .get("collection")
        .or_else(|| step.get("collectionPolicy"))
        .or_else(|| step.get("collection_policy"));
    match policy {
        Some(Value::Object(map)) => match map.get("type").and_then(Value::as_str).map(str::trim) {
            Some("any") => Some("any".to_string()),
            Some("all") => Some("all".to_string()),
            Some("quorum") => map
                .get("n")
                .and_then(Value::as_u64)
                .filter(|value| *value > 0)
                .map(|n| format!("quorum:{n}")),
            _ => None,
        },
        Some(Value::String(text)) if text.trim() == "all" => Some("all".to_string()),
        Some(Value::String(text)) if text.trim() == "any" => Some("any".to_string()),
        Some(Value::String(text)) if text.trim() == "quorum" => step
            .get("quorum")
            .or_else(|| step.get("collectionQuorum"))
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .map(|n| format!("quorum:{n}")),
        _ => None,
    }
}

fn definition_collection_policy_key(policy: &CollectionPolicy) -> String {
    match policy {
        CollectionPolicy::All => "all".to_string(),
        CollectionPolicy::Any => "any".to_string(),
        CollectionPolicy::Quorum { n } => format!("quorum:{n}"),
    }
}

fn collect_editor_flow_member_turns(
    steps: Option<&Vec<Value>>,
    profile_by_member: &BTreeMap<String, String>,
    out: &mut BTreeMap<(String, String), usize>,
) {
    let Some(steps) = steps else {
        return;
    };
    for step in steps {
        let step_type = step.get("type").and_then(Value::as_str).unwrap_or_default();
        if step_type == "member" {
            if let Some(member_id) = step
                .get("role")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                && let Some(profile) = profile_by_member.get(member_id)
                && let Some(message) = editor_step_instruction(step)
            {
                *out.entry((profile.clone(), message)).or_default() += 1;
            }
        }
        if let Some(nested) = step.get("steps").and_then(Value::as_array) {
            collect_editor_flow_member_turns(Some(nested), profile_by_member, out);
        }
        if let Some(branches) = step.get("branches").and_then(Value::as_array) {
            for branch in branches {
                collect_editor_flow_member_turns(
                    branch.get("steps").and_then(Value::as_array),
                    profile_by_member,
                    out,
                );
            }
        }
        collect_editor_flow_member_turns(
            step.get("fallback").and_then(Value::as_array),
            profile_by_member,
            out,
        );
    }
}

fn definition_flow_step_turns(
    document: &MobpackDocument,
    definition: &MobDefinition,
) -> BTreeMap<(String, String), usize> {
    let flow_name = document
        .flow
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let primary_flow = flow_name
        .and_then(|name| definition.flows.get_key_value(name))
        .or_else(|| definition.flows.get_key_value("main"))
        .or_else(|| definition.flows.iter().next());
    let mut out = BTreeMap::new();
    if let Some((_, flow)) = primary_flow {
        for step in flow.steps.values() {
            *out.entry((step.role.to_string(), step.message.text_content()))
                .or_default() += 1;
        }
    }
    out
}

fn normalize_json_schema_for_compare(value: &Value) -> Value {
    normalize_json_schema_value(value, None)
}

fn normalize_json_schema_value(value: &Value, key: Option<&str>) -> Value {
    match value {
        Value::Array(items) if key == Some("required") => {
            let mut required = items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            required.sort();
            Value::Array(required.into_iter().map(Value::String).collect())
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| normalize_json_schema_value(item, None))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(child_key, child_value)| {
                    (
                        child_key.clone(),
                        normalize_json_schema_value(child_value, Some(child_key)),
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn expected_schema_files_from_definition(
    definition: &MobDefinition,
) -> Result<BTreeMap<String, Value>, Vec<MobpackDiagnostic>> {
    let mut files = BTreeMap::new();
    let mut diagnostics = Vec::new();

    for (flow_id, flow) in &definition.flows {
        for (step_id, step) in &flow.steps {
            let Some(schema_ref) = step
                .expected_schema_ref
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let path = format!("flows.{flow_id}.steps.{step_id}.expected_schema_ref");
            let archive_path = match schema_ref_archive_path(schema_ref) {
                Ok(Some(path)) => path,
                Ok(None) => continue,
                Err(message) => {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "invalid_expected_schema_ref".to_string(),
                        message,
                        path: Some(path),
                    });
                    continue;
                }
            };
            let Some(schema) = profile_output_schema_for_step(definition, step) else {
                diagnostics.push(MobpackDiagnostic {
                    severity: "error".to_string(),
                    code: "expected_schema_ref_missing_profile_schema".to_string(),
                    message: format!(
                        "step '{step_id}' references '{schema_ref}', but profile '{}' has no inline output_schema to export",
                        step.role
                    ),
                    path: Some(path),
                });
                continue;
            };
            if let Some(existing) = files.get(&archive_path) {
                if existing != schema {
                    diagnostics.push(MobpackDiagnostic {
                        severity: "error".to_string(),
                        code: "expected_schema_ref_conflict".to_string(),
                        message: format!(
                            "multiple steps write different output schemas to '{archive_path}'"
                        ),
                        path: Some(path),
                    });
                }
            } else {
                files.insert(archive_path, schema.clone());
            }
        }
    }

    if diagnostics.is_empty() {
        Ok(files)
    } else {
        Err(diagnostics)
    }
}

fn schema_ref_archive_path(schema_ref: &str) -> Result<Option<String>, String> {
    if serde_json::from_str::<Value>(schema_ref).is_ok() {
        return Ok(None);
    }
    let path = Path::new(schema_ref);
    if schema_ref.is_empty() {
        return Err("expected_schema_ref must not be empty".to_string());
    }
    if schema_ref.contains('\\') {
        return Err(format!(
            "expected_schema_ref '{schema_ref}' must use forward-slash archive paths"
        ));
    }
    if path.is_absolute() {
        return Err(format!(
            "expected_schema_ref '{schema_ref}' must be a relative mobpack path"
        ));
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Err(format!(
            "expected_schema_ref '{schema_ref}' must be inline JSON or a relative .json path"
        ));
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.is_empty() {
                    return Err(format!(
                        "expected_schema_ref '{schema_ref}' contains an empty path segment"
                    ));
                }
                parts.push(part.to_string());
            }
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(format!(
                    "expected_schema_ref '{schema_ref}' must stay within the mobpack archive"
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err("expected_schema_ref must not be empty".to_string());
    }
    Ok(Some(parts.join("/")))
}

fn profile_output_schema_for_step<'a>(
    definition: &'a MobDefinition,
    step: &FlowStepSpec,
) -> Option<&'a Value> {
    match definition.profiles.get(&step.role) {
        Some(ProfileBinding::Inline(profile)) => profile.output_schema.as_ref(),
        Some(ProfileBinding::RealmRef { .. }) | None => None,
    }
}

fn editor_schema_map(schemas: &Value) -> BTreeMap<String, Value> {
    schemas
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|schema| {
            schema
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(|id| (id.to_string(), schema.clone()))
        })
        .collect()
}

fn editor_schema_to_json_schema(schema: &Value) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    if let Some(fields) = schema.get("fields").and_then(Value::as_array) {
        for field in fields {
            let name = field
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let enum_values = field
                .get("enumValues")
                .and_then(Value::as_array)
                .filter(|_| field.get("type").and_then(Value::as_str) == Some("enum"))
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|value| Value::String(value.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let mut field_schema = serde_json::Map::new();
            if enum_values.is_empty() {
                let field_type = field.get("type").and_then(Value::as_str);
                field_schema.insert(
                    "type".to_string(),
                    Value::String(editor_schema_field_type(field_type)),
                );
                if let Some(item_type) = editor_schema_array_item_type(field_type) {
                    field_schema.insert(
                        "items".to_string(),
                        json!({
                            "type": item_type,
                        }),
                    );
                }
            } else {
                field_schema.insert("type".to_string(), Value::String("string".to_string()));
                field_schema.insert("enum".to_string(), Value::Array(enum_values));
            }
            if let Some(description) = field
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|description| !description.is_empty())
            {
                field_schema.insert(
                    "description".to_string(),
                    Value::String(description.to_string()),
                );
            }
            properties.insert(name.clone(), Value::Object(field_schema));
            if field
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                required.push(Value::String(name));
            }
        }
    }
    let mut out = serde_json::Map::new();
    out.insert("type".to_string(), Value::String("object".to_string()));
    if let Some(description) = schema
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|description| !description.is_empty())
    {
        out.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    out.insert("properties".to_string(), Value::Object(properties));
    out.insert("required".to_string(), Value::Array(required));
    out.insert("additionalProperties".to_string(), Value::Bool(false));
    Value::Object(out)
}

fn editor_schema_field_type(field_type: Option<&str>) -> String {
    match field_type.unwrap_or("string") {
        "int" | "integer" => "integer".to_string(),
        "number" | "float" => "number".to_string(),
        "bool" | "boolean" => "boolean".to_string(),
        "object" => "object".to_string(),
        value if value.ends_with("[]") => "array".to_string(),
        "bytes" => "string".to_string(),
        _ => "string".to_string(),
    }
}

fn editor_schema_array_item_type(field_type: Option<&str>) -> Option<String> {
    let field_type = field_type?;
    let item_type = field_type.strip_suffix("[]")?;
    Some(editor_schema_field_type(Some(item_type)))
}

fn editor_profile_name(member: &Value) -> String {
    let raw = member
        .get("name")
        .or_else(|| member.get("role"))
        .or_else(|| member.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("member");
    let cleaned = raw
        .chars()
        .filter_map(|ch| {
            let ch = ch.to_ascii_lowercase();
            (ch.is_ascii_alphanumeric() || ch == '_' || ch == ' ' || ch == '-').then_some(ch)
        })
        .collect::<String>();
    let out = cleaned
        .split(|ch: char| ch.is_ascii_whitespace() || ch == '-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
        .trim_matches('_')
        .to_string();
    if out.is_empty() {
        "member".to_string()
    } else {
        out
    }
}

fn skill_ids_from_document_definition(document: &MobpackDocument) -> BTreeSet<String> {
    let Some(definition) = definition_from_document_mob_toml(document) else {
        return BTreeSet::new();
    };
    definition.skills.keys().map(ToString::to_string).collect()
}

fn definition_from_document_mob_toml(document: &MobpackDocument) -> Option<MobDefinition> {
    let mob_toml = document
        .mob_toml
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    MobDefinition::from_toml(mob_toml).ok()
}

fn sanitize_slug(input: &str) -> Option<String> {
    let slug = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    (!slug.is_empty()).then_some(slug)
}

fn document_export_filename(document: &MobpackDocument) -> String {
    let slug = sanitize_slug(
        document
            .name
            .trim()
            .strip_suffix(".mobpack")
            .unwrap_or_else(|| document.name.trim()),
    )
    .or_else(|| sanitize_slug(document.mob_id.trim()))
    .unwrap_or_else(|| "mobpack".to_string());
    format!("{slug}.mobpack")
}

fn extract_manifest_name(text: &str) -> Option<String> {
    let mut in_mobpack = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_mobpack = line == "[mobpack]";
            continue;
        }
        if in_mobpack
            && let Some((_, raw_value)) = line
                .strip_prefix("name")
                .and_then(|rest| rest.split_once('='))
        {
            return Some(raw_value.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_document() -> MobpackDocument {
        MobpackDocument {
            schema_version: MOBPACK_SCHEMA_VERSION.to_string(),
            mob_id: "review-pack".to_string(),
            name: "Review Pack".to_string(),
            mob_settings: Value::Null,
            members: Value::Null,
            instances: Value::Null,
            edges: Value::Null,
            frames: Value::Null,
            schemas: Value::Null,
            skill_realms: Value::Null,
            flow: Value::Null,
            launch_modes: Value::Null,
            deploy: Value::Null,
            mob_toml: Some(
                r#"
[mob]
id = "review-pack"

[profiles.planner]
model = "gpt-5.5"
skills = []
peer_description = "Planner"

[profiles.planner.tools]
comms = true

[flows.review]
description = "Review flow"

[flows.review.steps.plan]
role = "planner"
message = "Plan the work"
"#
                .to_string(),
            ),
        }
    }

    fn author_inline_member_contracts(document: &mut MobpackDocument) {
        let Some(members) = document.members.as_array_mut() else {
            return;
        };
        for member in members {
            if member.get("profileBinding").is_none() {
                member["profileBinding"] = json!("inline");
            }
            if member.get("runtimeMode").is_none() {
                member["runtimeMode"] = json!("turn_driven");
            }
        }
    }

    fn document_with_member_catalog_refs(
        tools: Value,
        skills: Value,
        skill_realms: Value,
    ) -> MobpackDocument {
        let mut document = valid_document();
        document.members = json!([{
            "id": "m_planner",
            "name": "planner",
            "role": "planner",
            "model": "gpt-5.5",
            "tools": tools,
            "skills": skills
        }]);
        author_inline_member_contracts(&mut document);
        document.skill_realms = skill_realms;
        document.mob_toml = Some(
            r#"
[mob]
id = "review-pack"

[profiles.planner]
model = "gpt-5.5"
skills = ["mob.workpad"]
peer_description = "Planner"

[profiles.planner.tools]
builtins = true
comms = true

[skills."mob.workpad"]
source = "inline"
content = "Maintain the shared mob workpad."

[flows.review]
description = "Review flow"

[flows.review.steps.plan]
role = "planner"
message = "Plan the work"
"#
            .to_string(),
        );
        document
    }

    fn document_with_unused_reviewer_profile() -> MobpackDocument {
        let mut document = valid_document();
        document.members = json!([
            {
                "id": "m_planner",
                "name": "planner",
                "role": "planner",
                "model": "gpt-5.5",
                "tools": ["comms"],
                "skills": []
            },
            {
                "id": "m_reviewer",
                "name": "reviewer",
                "role": "reviewer",
                "model": "gpt-5.5",
                "tools": ["builtins", "shell"],
                "skills": []
            }
        ]);
        author_inline_member_contracts(&mut document);
        document.mob_toml = Some(
            r#"
[mob]
id = "review-pack"

[profiles.planner]
model = "gpt-5.5"
skills = []
peer_description = "Planner"

[profiles.planner.tools]
comms = true

[profiles.reviewer]
model = "gpt-5.5"
skills = []
peer_description = "Reviewer"

[profiles.reviewer.tools]
builtins = true
shell = true

[flows.review]
description = "Review flow"

[flows.review.steps.plan]
role = "planner"
message = "Plan the work"
"#
            .to_string(),
        );
        document
    }

    fn document_with_reviewer_output_schema() -> MobpackDocument {
        let mut document = document_with_unused_reviewer_profile();
        if let Some(members) = document.members.as_array_mut() {
            members[1]["schema"] = json!("ReviewArtifact");
        }
        document.schemas = json!([{
            "id": "ReviewArtifact",
            "description": "Review result",
            "fields": [{
                "id": "f1",
                "name": "verdict",
                "type": "enum",
                "required": true,
                "description": "Pass/fail.",
                "enumValues": ["green", "red"]
            }]
        }]);
        document.mob_toml = Some(
            r#"
[mob]
id = "review-pack"

[profiles.planner]
model = "gpt-5.5"
skills = []
peer_description = "Planner"

[profiles.planner.tools]
comms = true

[profiles.reviewer]
model = "gpt-5.5"
skills = []
peer_description = "Reviewer"

[profiles.reviewer.tools]
builtins = true
shell = true

[profiles.reviewer.output_schema]
type = "object"
description = "Review result"
required = ["verdict"]
additionalProperties = false

[profiles.reviewer.output_schema.properties]

[profiles.reviewer.output_schema.properties.verdict]
type = "string"
description = "Pass/fail."
enum = ["green", "red"]

[flows.review]
description = "Review flow"

[flows.review.steps.plan]
role = "planner"
message = "Plan the work"
"#
            .to_string(),
        );
        document
    }

    fn document_with_expected_schema_ref() -> MobpackDocument {
        let mut document = document_with_reviewer_output_schema();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "size: number", "inputParams": [
                    { "id": "p1", "name": "size", "type": "number", "required": true, "enumValues": [] }
                ] },
                { "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" },
                { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review", "schema": "ReviewArtifact", "expectedSchemaRef": "schemas/reviewer.json" }
            ]
        });
        document.instances = json!([
            { "id": "plan", "memberId": "m_planner", "col": 0, "row": 0, "launchMode": { "kind": "Fresh" } },
            { "id": "review", "memberId": "m_reviewer", "col": 1, "row": 0, "launchMode": { "kind": "Fresh" } }
        ]);
        document.edges = json!([
            { "id": "e1", "from": "plan", "to": "review", "kind": "next", "label": "" }
        ]);
        document.frames = json!([]);
        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "profile": "planner",
                "launch_mode": { "kind": "Fresh" }
            },
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "profile": "reviewer",
                "launch_mode": { "kind": "Fresh" }
            }
        ]);
        document.mob_toml = Some(
            r#"
[mob]
id = "review-pack"

[profiles.planner]
model = "gpt-5.5"
skills = []
peer_description = "Planner"

[profiles.planner.tools]
comms = true

[profiles.reviewer]
model = "gpt-5.5"
skills = []
peer_description = "Reviewer"

[profiles.reviewer.tools]
builtins = true
shell = true

[profiles.reviewer.output_schema]
type = "object"
description = "Review result"
required = ["verdict"]
additionalProperties = false

[profiles.reviewer.output_schema.properties]

[profiles.reviewer.output_schema.properties.verdict]
type = "string"
description = "Pass/fail."
enum = ["green", "red"]

[flows.review]
description = "Review flow"

[flows.review.steps.plan]
role = "planner"
message = "Plan"

[flows.review.steps.review]
role = "reviewer"
message = "Review"
depends_on = ["plan"]
expected_schema_ref = "schemas/reviewer.json"
"#
            .to_string(),
        );
        document
    }

    fn document_with_real_launch_modes() -> MobpackDocument {
        let mut document = document_with_unused_reviewer_profile();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                { "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" },
                { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review",
                  "launchMode": { "kind": "Fork", "from": "plan", "context": "FullHistory" } }
            ]
        });
        document.instances = json!([
            { "id": "plan", "memberId": "m_planner", "col": 0, "row": 0, "launchMode": { "kind": "Fresh" } },
            { "id": "review", "memberId": "m_reviewer", "col": 1, "row": 0, "launchMode": { "kind": "Fork", "from": "plan", "context": "FullHistory" } }
        ]);
        document.edges = json!([
            { "id": "e1", "from": "plan", "to": "review", "kind": "next", "label": "" }
        ]);
        document.frames = json!([]);
        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "profile": "planner",
                "launch_mode": { "kind": "Fresh" }
            },
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "profile": "reviewer",
                "launch_mode": { "kind": "Fork", "from": "plan", "context": "FullHistory" }
            }
        ]);
        document.mob_toml = Some(
            r#"
[mob]
id = "review-pack"

[profiles.planner]
model = "gpt-5.5"
skills = []
peer_description = "Planner"

[profiles.planner.tools]
comms = true

[profiles.reviewer]
model = "gpt-5.5"
skills = []
peer_description = "Reviewer"

[profiles.reviewer.tools]
builtins = true
shell = true

[flows.review]
description = "Review flow"

[flows.review.steps.plan]
role = "planner"
message = "Plan"

[flows.review.steps.review]
role = "reviewer"
message = "Review"
depends_on = ["plan"]
"#
            .to_string(),
        );
        document
    }

    fn document_with_parallel_graph_controls() -> MobpackDocument {
        let mut document = document_with_real_launch_modes();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                {
                    "id": "parallel_review",
                    "type": "parallel",
                    "dispatch": "fan_out",
                    "collection": "all",
                    "branches": [
                        {
                            "id": "br_plan",
                            "label": "Plan",
                            "steps": [
                                { "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" }
                            ]
                        },
                        {
                            "id": "br_review",
                            "label": "Review",
                            "steps": [
                                { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review" }
                            ]
                        }
                    ],
                    "dependsMode": "all"
                }
            ]
        });
        document.instances = json!([
            { "id": "g_parallel_parallel_review", "isGate": true, "gateKind": "fork", "label": "fan_out", "col": 0, "row": 1 },
            { "id": "plan", "memberId": "m_planner", "col": 1, "row": 1, "launchMode": { "kind": "Fresh" } },
            { "id": "review", "memberId": "m_reviewer", "col": 1, "row": 2, "launchMode": { "kind": "Fresh" } },
            { "id": "j_parallel_parallel_review", "isGate": true, "gateKind": "join", "label": "join · all", "col": 2, "row": 1 }
        ]);
        document.edges = json!([
            { "id": "e1", "from": "g_parallel_parallel_review", "to": "plan", "kind": "fanout", "label": "" },
            { "id": "e2", "from": "g_parallel_parallel_review", "to": "review", "kind": "fanout", "label": "" },
            { "id": "e3", "from": "plan", "to": "j_parallel_parallel_review", "kind": "next", "label": "" },
            { "id": "e4", "from": "review", "to": "j_parallel_parallel_review", "kind": "next", "label": "" }
        ]);
        document.frames = json!([
            { "id": "frame_parallel_parallel_review", "kind": "Parallel", "colStart": 0, "colEnd": 2, "label": "PARALLEL · fan_out · join all" }
        ]);
        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "profile": "planner",
                "launch_mode": { "kind": "Fresh" }
            },
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "profile": "reviewer",
                "launch_mode": { "kind": "Fresh" }
            }
        ]);
        document
    }

    fn document_with_branch_graph_controls() -> MobpackDocument {
        let mut document = document_with_real_launch_modes();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Route review", "fields": "route: enum" },
                {
                    "id": "branch_route",
                    "type": "branch",
                    "controllerRole": "m_reviewer",
                    "branches": [
                        {
                            "id": "br_plan",
                            "label": "Plan",
                            "condition": "route == plan",
                            "cond": { "field": "route", "namespace": "params", "op": "==", "val": "plan" },
                            "steps": [
                                { "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" }
                            ]
                        }
                    ],
                    "fallback": [
                        { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review" }
                    ]
                }
            ]
        });
        document.instances = json!([
            { "id": "g_branch_branch_route", "isGate": true, "gateKind": "branch", "label": "branch", "col": 0, "row": 1 },
            { "id": "plan", "memberId": "m_planner", "col": 1, "row": 1, "launchMode": { "kind": "Fresh" } },
            { "id": "review", "memberId": "m_reviewer", "col": 1, "row": 2, "launchMode": { "kind": "Fresh" } },
            { "id": "j_branch_branch_route", "isGate": true, "gateKind": "join", "label": "join · branch paths", "collection": "any", "controllerRole": "m_reviewer", "col": 2, "row": 1 }
        ]);
        document.edges = json!([
            { "id": "e1", "from": "g_branch_branch_route", "to": "plan", "kind": "cond", "label": "route == plan", "cond": { "var": "params.route", "op": "==", "val": "plan" } },
            { "id": "e2", "from": "g_branch_branch_route", "to": "review", "kind": "next", "label": "fallback" },
            { "id": "e3", "from": "plan", "to": "j_branch_branch_route", "kind": "next", "label": "" },
            { "id": "e4", "from": "review", "to": "j_branch_branch_route", "kind": "next", "label": "" }
        ]);
        document.frames = json!([
            { "id": "frame_branch_branch_route", "kind": "Branch", "colStart": 0, "colEnd": 2, "label": "BRANCH · 2 paths" }
        ]);
        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "profile": "planner",
                "launch_mode": { "kind": "Fresh" }
            },
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "profile": "reviewer",
                "launch_mode": { "kind": "Fresh" }
            }
        ]);
        document
    }

    fn importable_mob_toml() -> String {
        r#"
[mob]
id = "importable-real-mob"
orchestrator = "planner"

[wiring]
auto_wire_orchestrator = true

[[wiring.role_wiring]]
a = "planner"
b = "reviewer"

[backend]
default = "external"

[backend.external]
address_base = "http://127.0.0.1:9000"

[topology]
mode = "strict"
rules = [{ from_role = "planner", to_role = "reviewer", allowed = true }]

[supervisor]
role = "planner"
escalation_threshold = 2

[limits]
max_flow_duration_ms = 30000
max_step_retries = 1
max_orphaned_turns = 8
cancel_grace_timeout_ms = 500
max_active_nodes = 4
max_active_frames = 2
max_frame_depth = 3

[spawn_policy]
mode = "auto"

[spawn_policy.profile_map]
reviewer = "reviewer"

[event_router]
buffer_size = 128
include_patterns = ["text_complete"]
exclude_patterns = ["debug_*"]

[profiles.planner]
model = "gpt-5.2"
skills = ["mob.workpad"]
peer_description = "Plan the work"
backend = "session"
runtime_mode = "autonomous_host"
max_inline_peer_notifications = 20
provider_params = { thinking_budget = 8192, top_k = 20 }

[profiles.planner.tools]
builtins = true
comms = true
mob = true

[profiles.reviewer]
model = "gpt-5.2"
skills = ["mob.review"]
peer_description = "Review the work"
runtime_mode = "turn_driven"

[profiles.reviewer.tools]
builtins = true
shell = true
comms = true

[profiles.reviewer.output_schema]
type = "object"
description = "Review verdict"
required = ["verdict"]
additionalProperties = false

[profiles.reviewer.output_schema.properties]

[profiles.reviewer.output_schema.properties.verdict]
type = "string"
description = "Gate verdict"
enum = ["red", "green"]

[skills."mob.workpad"]
source = "inline"
content = "Maintain the shared mob workpad."

[skills."mob.review"]
source = "path"
path = "skills/review.md"

[flows.main]
description = "Imported flow"

[flows.main.steps.plan]
role = "planner"
message = "Plan the work"
dispatch_mode = "one_to_one"
collection_policy = { type = "quorum", n = 1 }
timeout_ms = 1500
allowed_tools = ["mob"]
blocked_tools = ["shell"]
output_format = "text"

[flows.main.steps.review]
role = "reviewer"
message = "Review the work"
depends_on = ["plan"]
expected_schema_ref = "schemas/reviewer.json"

[flows.main.root.nodes.node_plan]
kind = "step"
step_id = "plan"
depends_on = []
depends_on_mode = "all"

[flows.main.root.nodes.node_review_loop]
kind = "repeat_until"
loop_id = "review_loop"
depends_on = ["node_plan"]
depends_on_mode = "all"
until = { op = "eq", path = "steps.review.verdict", value = "green" }
max_iterations = 3

[flows.main.root.nodes.node_review_loop.body.nodes.node_review]
kind = "step"
step_id = "review"
depends_on = []
depends_on_mode = "all"
"#
        .to_string()
    }

    #[test]
    fn validates_parseable_mobpack_document() {
        let from_env = std::env::var("MOBKIT_MOBPACK_VALIDATE_IN").ok();
        let document = match from_env.as_deref() {
            Some(path) => {
                let params = serde_json::from_str::<Value>(
                    &std::fs::read_to_string(path).expect("read editor mobpack validation fixture"),
                )
                .expect("parse editor mobpack validation fixture");
                document_from_params(&params).expect("document from validation params")
            }
            None => valid_document(),
        };
        let result = validate_document(&document);
        assert_eq!(result.validation_source, MOBPACK_VALIDATION_SOURCE);
        assert_eq!(
            result.deploy_command,
            "rkat mob deploy <pack.mobpack> <prompt>"
        );
        if from_env.is_none() {
            assert!(result.ok, "{:?}", result.diagnostics);
            assert_eq!(result.mob_id.as_deref(), Some("review-pack"));
            assert_eq!(result.flow_ids, vec!["review".to_string()]);
            assert!(result.display_rows.iter().any(|row| {
                row.kind == "ok"
                    && row.head == "MobKit mobpack validates"
                    && row.sub == MOBPACK_VALIDATION_SOURCE
                    && row.meta == "rkat mob validate"
            }));
        }
        if let Ok(path) = std::env::var("MOBKIT_MOBPACK_VALIDATE_OUT") {
            std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap())
                .expect("write validation result");
        }
    }

    #[test]
    fn rejects_blank_draft_without_synthetic_profiles() {
        let document = MobpackDocument {
            schema_version: MOBPACK_SCHEMA_VERSION.to_string(),
            mob_id: "blank-real-mob".to_string(),
            name: "blank-real-mob".to_string(),
            mob_settings: json!({}),
            members: json!([]),
            instances: json!([]),
            edges: json!([]),
            frames: json!([]),
            schemas: json!([]),
            skill_realms: json!([]),
            flow: json!({
                "name": "blank-real-mob",
                "steps": [
                    { "id": "input_1", "type": "input", "task": "", "fields": "", "inputParams": [] }
                ]
            }),
            launch_modes: json!([]),
            deploy: json!({
                "command": "rkat mob deploy",
                "surface": "cli",
                "trust_policy": "permissive",
                "realm_backend": "jsonl",
                "prompt": "Review the draft."
            }),
            mob_toml: Some(
                r#"
[mob]
id = "blank_real_mob"

[flows.main]
description = "Generated by MobKit Flow Editor"
"#
                .to_string(),
            ),
        };

        let result = validate_document(&document);

        assert!(!result.ok);
        assert_eq!(result.mob_id.as_deref(), Some("blank_real_mob"));
        assert!(result.flow_ids.contains(&"main".to_string()));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "empty_profiles" && diagnostic.path.as_deref() == Some("profiles")
        }));
        assert!(!result.diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("lead") || diagnostic.message.contains("placeholder")
        }));
    }

    #[test]
    fn validates_launch_mode_metadata_shape() {
        let mut document = valid_document();
        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "launch_mode": { "kind": "Fork", "from": "coder", "context": "FullHistory" },
                "budget_split_policy": { "type": "fixed", "value": 4096 }
            },
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "launch_mode": { "kind": "Resume", "sessionId": "session-123" },
                "budget_split_policy": { "kind": "Remaining" }
            }
        ]);
        assert!(validate_document(&document).ok);

        document.launch_modes = json!([
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "launch_mode": { "kind": "Fork" }
            }
        ]);
        let result = validate_document(&document);
        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "fork_launch_missing_source")
        );

        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "launch_mode": { "kind": "Fresh" },
                "budget_split_policy": { "type": "fixed", "value": 0 }
            }
        ]);
        let result = validate_document(&document);
        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_budget_split_policy")
        );

        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "launch_mode": { "kind": "Fresh" },
                "budget_split_policy": {}
            }
        ]);
        let result = validate_document(&document);
        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_budget_split_policy"
                && diagnostic.message.contains("non-empty type")
        }));
    }

    #[test]
    fn graph_launch_projection_omits_absent_budget_split_policy() {
        let instances = vec![
            json!({
                "id": "plan",
                "memberId": "m_planner",
                "launchMode": { "kind": "Fresh" }
            }),
            json!({
                "id": "review",
                "memberId": "m_reviewer",
                "launchMode": {
                    "kind": "Fork",
                    "from": "plan",
                    "context": "FullHistory",
                    "budgetSplitPolicy": { "kind": "Fixed", "limit": 2048 }
                }
            }),
        ];
        let launch_modes = launch_modes_from_instances(&instances);
        let rows = launch_modes.as_array().expect("launch mode rows");

        assert_eq!(rows[0]["launch_mode"], json!({ "kind": "Fresh" }));
        assert!(
            rows[0].get("budget_split_policy").is_none(),
            "absent graph budget policy must not become equal"
        );
        assert_eq!(
            rows[1]["budget_split_policy"],
            json!({ "type": "fixed", "value": 2048 })
        );
    }

    #[test]
    fn validates_launch_modes_against_editor_steps_and_members() {
        let document = document_with_real_launch_modes();
        let result = validate_document(&document);

        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn rejects_launch_mode_entry_drift_from_flow_step() {
        let mut document = document_with_real_launch_modes();
        let launch_modes = document
            .launch_modes
            .as_array_mut()
            .expect("launch mode entries");
        launch_modes[1]["launch_mode"] = json!({ "kind": "Fresh" });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "launch_mode_flow_mismatch"
                && diagnostic.path.as_deref() == Some("launch_modes[1].launch_mode")
        }));
    }

    #[test]
    fn rejects_duplicate_launch_mode_steps() {
        let mut document = document_with_real_launch_modes();
        let launch_modes = document
            .launch_modes
            .as_array_mut()
            .expect("launch mode entries");
        launch_modes.push(json!({
            "step_id": "review",
            "member_id": "m_reviewer",
            "profile": "reviewer",
            "launch_mode": { "kind": "Fork", "from": "plan", "context": "FullHistory" }
        }));
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "duplicate_launch_step"
                && diagnostic.path.as_deref() == Some("launch_modes[2].step_id")
        }));
    }

    #[test]
    fn rejects_launch_budget_entry_drift_from_flow_step() {
        let mut document = document_with_real_launch_modes();
        document.flow["steps"][2]["launchMode"]["budgetSplitPolicy"] = json!({ "type": "equal" });
        let launch_modes = document
            .launch_modes
            .as_array_mut()
            .expect("launch mode entries");
        launch_modes[1]["budget_split_policy"] = json!({ "type": "fixed", "value": 4096 });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "launch_mode_flow_mismatch"
                && diagnostic.path.as_deref() == Some("launch_modes[1].launch_mode")
        }));
    }

    #[test]
    fn rejects_editor_flow_primitives_without_mobkit_semantics() {
        let mut document = valid_document();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                { "id": "wait_1", "type": "wait", "prompt": "Approve deploy" },
                { "id": "submob_1", "type": "subagent", "name": "nested review", "steps": [] }
            ]
        });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unsupported_editor_flow_step_type"
                && diagnostic.path.as_deref() == Some("flow.steps[1].type")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unsupported_editor_flow_step_type"
                && diagnostic.path.as_deref() == Some("flow.steps[2].type")
        }));
    }

    #[test]
    fn rejects_missing_or_duplicate_editor_flow_step_ids() {
        let mut document = document_with_parallel_graph_controls();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        steps[0].as_object_mut().unwrap().remove("id");
        steps[1]["branches"][1]["steps"][0]["id"] = json!("plan");

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_editor_flow_step_id"
                && diagnostic.path.as_deref() == Some("flow.steps[0].id")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "duplicate_editor_flow_step_id"
                && diagnostic.path.as_deref() == Some("flow.steps[1].branches[1].steps[0].id")
        }));
    }

    #[test]
    fn refuses_to_export_unsupported_editor_flow_primitives() {
        let mut document = valid_document();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                { "id": "wait_1", "type": "wait", "prompt": "Approve deploy" }
            ]
        });
        let err = export_mobpack(&json!({ "document": document })).expect_err("export fails");

        assert_eq!(err, "cannot export invalid mobpack document");
    }

    #[test]
    fn validates_supported_editor_condition_operators() {
        let mut document = valid_document();
        document.members = json!([{
            "id": "m_reviewer",
            "name": "reviewer",
            "role": "reviewer",
            "model": "gpt-5.5",
            "schema": "ReviewArtifact",
            "tools": [],
            "skills": [],
            "profileBinding": "inline",
            "runtimeMode": "turn_driven"
        }]);
        document.schemas = json!([{
            "id": "ReviewArtifact",
            "fields": [{ "id": "score", "name": "score", "type": "number", "required": true }]
        }]);
        document.mob_toml = None;
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "size: number", "inputParams": [
                    { "id": "p1", "name": "size", "type": "number", "required": true, "enumValues": [] }
                ] },
                { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review", "schema": "ReviewArtifact" },
                {
                    "id": "branch_1",
                    "type": "branch",
                    "controllerRole": "m_reviewer",
                    "branches": [
                        { "id": "br_fast", "label": "Fast", "condition": "steps.review.score > 10", "cond": { "stepId": "review", "field": "score", "op": ">", "val": "10" }, "steps": [] },
                        { "id": "br_small", "label": "Small", "condition": "params.size < 3", "steps": [] }
                    ],
                    "fallback": []
                },
                {
                    "id": "loop_1",
                    "type": "repeat",
                    "loopId": "loop_1",
                    "maxIterations": 3,
                    "cond": { "stepId": "review", "op": "<", "field": "score", "val": "10" },
                    "steps": [
                        { "id": "loop_review", "type": "member", "role": "m_reviewer", "instruction": "Review again", "schema": "ReviewArtifact" }
                    ]
                }
            ]
        });
        document.mob_toml =
            Some(render_editor_document_mob_toml(&document).expect("render supported operators"));
        let result = validate_document(&document);

        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn rejects_incomplete_structured_branch_condition() {
        let mut document = valid_document();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                {
                    "id": "branch_1",
                    "type": "branch",
                    "branches": [
                        { "id": "br_fast", "label": "Fast", "condition": "", "cond": { "stepId": "review", "op": ">=", "val": "10" }, "steps": [] }
                    ],
                    "fallback": []
                }
            ]
        });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "incomplete_editor_branch_condition"
                && diagnostic.path.as_deref() == Some("flow.steps[1].branches[0].cond.field")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unsupported_editor_condition_operator"
                && diagnostic.path.as_deref() == Some("flow.steps[1].branches[0].cond.op")
        }));
    }

    #[test]
    fn rejects_unsupported_editor_condition_operators() {
        let mut document = valid_document();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                {
                    "id": "branch_1",
                    "type": "branch",
                    "branches": [
                        { "id": "br_fast", "label": "Fast", "condition": "params.score >= 10", "steps": [] }
                    ],
                    "fallback": []
                },
                {
                    "id": "loop_1",
                    "type": "repeat",
                    "loopId": "loop_1",
                    "maxIterations": 3,
                    "cond": { "stepId": "review", "op": "!=", "field": "score", "val": "10" },
                    "steps": []
                }
            ]
        });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unsupported_editor_condition_operator"
                && diagnostic.path.as_deref() == Some("flow.steps[1].branches[0].condition")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unsupported_editor_condition_operator"
                && diagnostic.path.as_deref() == Some("flow.steps[2].cond.op")
        }));
    }

    #[test]
    fn rejects_unsupported_repeat_iteration_input() {
        let mut document = valid_document();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                {
                    "id": "loop_1",
                    "type": "repeat",
                    "loopId": "loop_1",
                    "maxIterations": 3,
                    "iterationInput": "reuse",
                    "cond": { "stepId": "review", "op": "==", "field": "score", "val": "10" },
                    "steps": []
                }
            ]
        });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unsupported_repeat_iteration_input"
                && diagnostic.path.as_deref() == Some("flow.steps[1].iterationInput")
        }));
    }

    #[test]
    fn rejects_repeat_steps_without_explicit_max_iterations_and_condition() {
        let mut document = valid_document();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                {
                    "id": "loop_1",
                    "type": "repeat",
                    "steps": []
                }
            ]
        });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_repeat_loop_id"
                && diagnostic.path.as_deref() == Some("flow.steps[1].loopId")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_repeat_max_iterations"
                && diagnostic.path.as_deref() == Some("flow.steps[1].maxIterations")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_repeat_condition"
                && diagnostic.path.as_deref() == Some("flow.steps[1].cond")
        }));
        assert_eq!(result.mob_id, None);
        assert!(result.flow_ids.is_empty());
    }

    #[test]
    fn rejects_incomplete_repeat_until_conditions() {
        let mut document = valid_document();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                {
                    "id": "loop_1",
                    "type": "repeat",
                    "loopId": "loop_1",
                    "maxIterations": 3,
                    "cond": { "field": "score", "op": ">", "val": "" },
                    "steps": []
                }
            ]
        });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "incomplete_repeat_condition"
                && diagnostic.path.as_deref() == Some("flow.steps[1].cond.stepId")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "incomplete_repeat_condition"
                && diagnostic.path.as_deref() == Some("flow.steps[1].cond.val")
        }));
    }

    #[test]
    fn repeat_condition_renderer_does_not_synthesize_missing_authoring_state() {
        let mut state = RenderState {
            members: BTreeMap::new(),
            steps: Vec::new(),
            step_ids_by_visual_id: BTreeMap::new(),
            flat_exits_by_node_id: BTreeMap::new(),
            next: 1,
        };
        state
            .step_ids_by_visual_id
            .insert("review".to_string(), "01_review".to_string());

        assert_eq!(condition_from_repeat_step(&json!({}), &state), None);
        assert_eq!(
            condition_from_repeat_step(
                &json!({ "cond": { "stepId": "review", "field": "verdict", "op": "==", "val": "" } }),
                &state,
            ),
            None
        );
        assert_eq!(
            condition_from_repeat_step(
                &json!({ "cond": { "field": "verdict", "op": "==", "val": "green" } }),
                &state,
            ),
            None
        );
        assert_eq!(
            condition_from_repeat_step(
                &json!({ "cond": { "stepId": "review", "field": "verdict", "op": "==", "val": "green" } }),
                &state,
            )
            .as_deref(),
            Some(r#"{ op = "eq", path = "steps.01_review.verdict", value = "green" }"#)
        );
        assert_eq!(
            condition_from_repeat_step(
                &json!({ "cond": { "namespace": "params", "field": "done", "op": "==", "val": "true" } }),
                &state,
            )
            .as_deref(),
            Some(r#"{ op = "eq", path = "params.done", value = "true" }"#)
        );
    }

    #[test]
    fn renderer_does_not_synthesize_missing_parallel_metadata_or_repeat_limits() {
        let planner = json!({
            "id": "m_planner",
            "role": "planner",
            "name": "Planner",
            "profileBinding": "inline",
            "runtimeMode": "turn_driven",
            "systemPrompt": "Plan."
        });
        let members = vec![&planner];

        let missing_parallel = vec![json!({
            "id": "parallel_missing",
            "type": "parallel",
            "branches": [{
                "id": "left",
                "steps": [{ "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" }]
            }]
        })];
        let rendered = compile_editor_flow(&missing_parallel, &members);
        assert!(rendered.steps.is_empty());
        assert!(rendered.root.nodes.is_empty());

        let explicit_parallel = vec![json!({
            "id": "parallel_explicit",
            "type": "parallel",
            "dispatchMode": "fan_out",
            "collection": "all",
            "controllerRole": "m_planner",
            "branches": [{
                "id": "left",
                "steps": [{ "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan", "launchMode": { "kind": "Fresh" } }]
            }]
        })];
        let rendered = compile_editor_flow(&explicit_parallel, &members);
        assert!(
            rendered
                .steps
                .iter()
                .any(|step| step.message == "Join parallel branches (all).")
        );

        let missing_repeat_limit = vec![json!({
            "id": "loop",
            "type": "repeat",
            "loopId": "loop",
            "cond": { "namespace": "params", "field": "done", "op": "==", "val": "true" },
            "steps": [{ "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" }]
        })];
        let rendered = compile_editor_flow(&missing_repeat_limit, &members);
        assert!(rendered.steps.is_empty());
        assert!(rendered.root.nodes.is_empty());

        let missing_repeat_loop_id = vec![json!({
            "id": "loop",
            "type": "repeat",
            "maxIterations": 2,
            "cond": { "namespace": "params", "field": "done", "op": "==", "val": "true" },
            "steps": [{ "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" }]
        })];
        let rendered = compile_editor_flow(&missing_repeat_loop_id, &members);
        assert!(rendered.steps.is_empty());
        assert!(rendered.root.nodes.is_empty());
    }

    #[test]
    fn exports_repeat_until_condition_from_input_param() {
        let mut document = document_with_unused_reviewer_profile();
        document.mob_toml = None;
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "done: boolean", "inputParams": [
                    { "id": "p1", "name": "done", "type": "boolean", "required": true, "enumValues": [] }
                ] },
                {
                    "id": "loop_1",
                    "type": "repeat",
                    "loopId": "review_loop",
                    "maxIterations": 2,
                    "cond": { "namespace": "params", "stepId": "params", "field": "done", "op": "==", "val": "true" },
                    "steps": [
                        { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review" }
                    ]
                }
            ]
        });
        let flow_steps = document
            .flow
            .get("steps")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let (instances, edges, frames) = graph_projection_from_visual_steps(&flow_steps);
        document.instances = Value::Array(instances);
        document.edges = Value::Array(edges);
        document.frames = Value::Array(frames);
        let validation = validate_document(&document);
        assert!(validation.ok, "{:#?}", validation.diagnostics);
        let export = export_mobpack(&json!({ "document": document })).expect("export succeeds");

        assert!(
            export
                .mob_toml
                .contains(r#"until = { op = "eq", path = "params.done", value = "true" }"#)
        );
        assert!(export.mob_toml.contains("max_iterations = 2"));
    }

    #[test]
    fn visual_graph_projection_preserves_missing_parallel_metadata() {
        let flow_steps = vec![
            json!({ "id": "input_1", "type": "input", "task": "Review" }),
            json!({
                "id": "parallel_review",
                "type": "parallel",
                "branches": [
                    {
                        "id": "br_plan",
                        "steps": [
                            { "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" }
                        ]
                    },
                    {
                        "id": "br_review",
                        "steps": [
                            { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review" }
                        ]
                    }
                ]
            }),
        ];

        let (instances, _, frames) = graph_projection_from_visual_steps(&flow_steps);
        let fork = instances
            .iter()
            .find(|instance| instance["id"] == "g_parallel_parallel_review")
            .expect("parallel fork");
        let join = instances
            .iter()
            .find(|instance| instance["id"] == "j_parallel_parallel_review")
            .expect("parallel join");
        let plan = instances
            .iter()
            .find(|instance| instance["id"] == "plan")
            .expect("parallel member");

        assert_eq!(fork["label"], "");
        assert_eq!(join["label"], "join · ");
        assert_eq!(join["collection"], "");
        assert_eq!(plan["launchMode"], Value::Null);
        assert_eq!(plan["dispatchMode"], "");
        assert_eq!(plan["collection"], "");
        assert_eq!(plan["outputFormat"], Value::Null);
        assert_eq!(
            frames[0]["label"],
            "PARALLEL · missing dispatch · join missing collection"
        );
    }

    #[test]
    fn visual_graph_projection_preserves_missing_repeat_metadata() {
        let flow_steps = vec![
            json!({ "id": "input_1", "type": "input", "task": "Review" }),
            json!({
                "id": "review_loop",
                "type": "repeat",
                "loopId": "review_loop",
                "cond": { "stepId": "review", "field": "verdict", "op": "==", "val": "green" },
                "steps": [
                    { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review" }
                ]
            }),
        ];

        let (_, _, frames) = graph_projection_from_visual_steps(&flow_steps);

        assert_eq!(frames[0]["kind"], "RepeatUntil");
        assert_eq!(
            frames[0]["label"],
            "REPEAT-UNTIL · until condition · missing max_iterations"
        );
        assert_ne!(frames[0]["label"], "REPEAT-UNTIL · until condition · max 3");
    }

    #[test]
    fn rejects_unsupported_graph_condition_operator() {
        let mut document = document_with_real_launch_modes();
        if let Some(edges) = document.edges.as_array_mut() {
            edges[0]["kind"] = json!("cond");
            edges[0]["cond"] = json!({ "var": "reviewer.verdict", "op": "!=", "val": "red" });
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unsupported_graph_condition_operator"
                && diagnostic.path.as_deref() == Some("edges[0].cond.op")
        }));
    }

    #[test]
    fn validates_schema_backed_graph_condition_ref() {
        let mut document = document_with_expected_schema_ref();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                {
                    "id": "node_loop",
                    "type": "repeat",
                    "loopId": "review_loop",
                    "maxIterations": 3,
                    "iterationInput": "carry",
                    "cond": { "stepId": "review", "field": "verdict", "op": "==", "val": "green" },
                    "steps": [
                        { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review", "schema": "ReviewArtifact" }
                    ]
                }
            ]
        });
        document.instances = json!([
            { "id": "review", "memberId": "m_reviewer", "col": 0, "row": 0, "launchMode": { "kind": "Fresh" } }
        ]);
        document.edges = json!([
            {
                "id": "e1",
                "from": "review",
                "to": "review",
                "kind": "cond",
                "label": "until reviewer verdict",
                "cond": { "var": "steps.review.verdict", "op": "==", "val": "green" }
            }
        ]);
        document.frames = json!([
            { "id": "frame_node_loop", "kind": "RepeatUntil", "colStart": 0, "colEnd": 0, "label": "REPEAT · until reviewer verdict" }
        ]);
        document.launch_modes = json!([
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "profile": "reviewer",
                "launch_mode": { "kind": "Fresh" }
            }
        ]);
        let result = validate_document(&document);

        assert!(result.ok, "{:#?}", result.diagnostics);
    }

    #[test]
    fn validates_runtime_param_graph_condition_ref() {
        let mut document = document_with_real_launch_modes();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "kind: string", "inputParams": [{
                    "id": "p1",
                    "name": "kind",
                    "type": "string",
                    "required": true,
                    "enumValues": []
                }] },
                {
                    "id": "kind_branch",
                    "type": "branch",
                    "controllerRole": "m_reviewer",
                    "dependsMode": "all",
                    "branches": [{
                        "id": "br_docs",
                        "label": "Docs",
                        "condition": "kind == docs",
                        "cond": { "namespace": "params", "stepId": "params", "field": "kind", "op": "==", "val": "docs" },
                        "steps": [
                            { "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" }
                        ]
                    }],
                    "fallback": [
                        { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review" }
                    ]
                }
            ]
        });
        document.instances = json!([
            { "id": "g_branch_kind_branch", "isGate": true, "gateKind": "branch", "label": "branch", "col": 0, "row": 0 },
            { "id": "plan", "memberId": "m_planner", "col": 1, "row": 0, "launchMode": { "kind": "Fresh" } },
            { "id": "review", "memberId": "m_reviewer", "col": 1, "row": 1, "launchMode": { "kind": "Fresh" } },
            { "id": "j_branch_kind_branch", "isGate": true, "gateKind": "join", "label": "join · branch paths", "collection": "any", "controllerRole": "m_reviewer", "col": 2, "row": 0 }
        ]);
        document.edges = json!([
            {
                "id": "e1",
                "from": "g_branch_kind_branch",
                "to": "plan",
                "kind": "cond",
                "label": "kind == docs",
                "cond": { "var": "params.kind", "op": "==", "val": "docs" }
            },
            { "id": "e2", "from": "g_branch_kind_branch", "to": "review", "kind": "next", "label": "fallback" },
            { "id": "e3", "from": "plan", "to": "j_branch_kind_branch", "kind": "next", "label": "" },
            { "id": "e4", "from": "review", "to": "j_branch_kind_branch", "kind": "next", "label": "" }
        ]);
        document.frames = json!([
            { "id": "frame_branch_kind_branch", "kind": "Branch", "colStart": 0, "colEnd": 2, "label": "BRANCH · kind" }
        ]);
        document.launch_modes.as_array_mut().expect("launch modes")[1]["launch_mode"] =
            json!({ "kind": "Fresh" });
        let result = validate_document(&document);

        assert!(result.ok, "{:#?}", result.diagnostics);
    }

    #[test]
    fn validates_runtime_param_basic_branch_condition_ref() {
        let mut document = document_with_real_launch_modes();
        document.instances = Value::Null;
        document.edges = Value::Null;
        document.frames = Value::Null;
        document.launch_modes = Value::Null;
        document.flow["steps"] = json!([
            {
                "id": "input_1",
                "type": "input",
                "task": "Review",
                "fields": "kind: enum",
                "inputParams": [{
                    "id": "p1",
                    "name": "kind",
                    "type": "enum",
                    "required": true,
                    "enumValues": ["docs", "code"]
                }]
            },
                {
                    "id": "branch_review",
                    "type": "branch",
                    "controllerRole": "m_reviewer",
                    "branches": [
                    {
                        "id": "br_plan",
                        "label": "Plan",
                        "cond": { "namespace": "params", "field": "kind", "op": "==", "val": "code" },
                        "condition": "params.kind == \"code\"",
                        "steps": [{ "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" }]
                    },
                    {
                        "id": "br_review",
                        "label": "Review",
                        "cond": { "stepId": "params", "field": "kind", "op": "==", "val": "docs" },
                        "condition": "params.kind == \"docs\"",
                        "steps": [{ "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review" }]
                    }
                ],
                "fallback": []
            }
        ]);
        let result = validate_document(&document);

        assert!(result.ok, "{:#?}", result.diagnostics);
    }

    #[test]
    fn exports_structured_branch_condition_without_display_text() {
        let mut document = document_with_unused_reviewer_profile();
        document.mob_toml = None;
        document.flow = json!({
            "name": "review",
            "steps": [
                {
                    "id": "input_1",
                    "type": "input",
                    "task": "Review",
                    "fields": "kind: enum",
                    "inputParams": [{
                        "id": "p1",
                        "name": "kind",
                        "type": "enum",
                        "required": true,
                        "enumValues": ["docs", "code"]
                    }]
                },
                {
                    "id": "branch_review",
                    "type": "branch",
                    "controllerRole": "m_reviewer",
                    "branches": [{
                        "id": "br_plan",
                        "label": "Plan",
                        "cond": { "namespace": "params", "stepId": "params", "field": "kind", "op": "==", "val": "code" },
                        "condition": "",
                        "steps": [{ "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" }]
                    }],
                    "fallback": [
                        { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review" }
                    ]
                }
            ]
        });
        let flow_steps = document
            .flow
            .get("steps")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let (instances, edges, frames) = graph_projection_from_visual_steps(&flow_steps);
        document.instances = Value::Array(instances);
        document.edges = Value::Array(edges);
        document.frames = Value::Array(frames);
        let validation = validate_document(&document);
        assert!(validation.ok, "{:#?}", validation.diagnostics);
        let export = export_mobpack(&json!({ "document": document })).expect("export succeeds");

        assert!(
            export
                .mob_toml
                .contains(r#"condition = { op = "eq", path = "params.kind", value = "code" }"#)
        );
    }

    #[test]
    fn rejects_incomplete_structured_branch_condition_operator_and_value() {
        let mut document = document_with_real_launch_modes();
        document.flow["steps"] = json!([
            { "id": "input_1", "type": "input", "task": "Review", "fields": "", "inputParams": [] },
            {
                "id": "branch_review",
                "type": "branch",
                "branches": [{
                    "id": "br_fast",
                    "label": "Fast",
                    "condition": "",
                    "cond": { "stepId": "review", "field": "verdict" },
                    "steps": []
                }],
                "fallback": []
            }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "incomplete_editor_branch_condition"
                && diagnostic.path.as_deref() == Some("flow.steps[1].branches[0].cond.op")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "incomplete_editor_branch_condition"
                && diagnostic.path.as_deref() == Some("flow.steps[1].branches[0].cond.val")
        }));
    }

    #[test]
    fn rejects_undeclared_runtime_param_condition_ref() {
        let mut document = document_with_real_launch_modes();
        if let Some(edges) = document.edges.as_array_mut() {
            edges[0]["kind"] = json!("cond");
            edges[0]["cond"] = json!({ "var": "params.kind", "op": "==", "val": "docs" });
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unknown_editor_input_param"
                && diagnostic.path.as_deref() == Some("edges[0].cond.var")
        }));
    }

    #[test]
    fn rejects_branch_without_explicit_condition() {
        let mut document = document_with_real_launch_modes();
        document.flow["steps"] = json!([
            { "id": "input_1", "type": "input", "task": "Review", "fields": "", "inputParams": [] },
            {
                "id": "branch_review",
                "type": "branch",
                "branches": [
                    { "id": "br_fast", "label": "Fast", "steps": [] }
                ],
                "fallback": []
            }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_editor_branch_condition"
                && diagnostic.path.as_deref() == Some("flow.steps[1].branches[0]")
        }));
    }

    #[test]
    fn rejects_fake_graph_condition_ref() {
        let mut document = document_with_real_launch_modes();
        if let Some(edges) = document.edges.as_array_mut() {
            edges[0]["kind"] = json!("cond");
            edges[0]["cond"] = json!({ "var": "reviewer.verdict", "op": "==", "val": "green" });
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_graph_condition_ref"
                && diagnostic.path.as_deref() == Some("edges[0].cond.var")
        }));
    }

    #[test]
    fn complete_editor_projection_overrides_stale_flow_turn_text() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = document
            .mob_toml
            .map(|text| text.replace("message = \"Review\"", "message = \"Stale review\""));
        let validation = validate_document(&document);

        assert!(validation.ok, "{:?}", validation.diagnostics);
        let result =
            export_mobpack(&json!({ "document": document })).expect("export rendered document");
        assert!(result.mob_toml.contains(r#"message = "Review""#));
        assert!(!result.mob_toml.contains("Stale review"));
    }

    #[test]
    fn rejects_launch_mode_references_to_unknown_editor_step_or_member() {
        let mut document = document_with_real_launch_modes();
        document.launch_modes = json!([
            {
                "step_id": "ghost",
                "member_id": "m_missing",
                "profile": "missing",
                "launch_mode": { "kind": "Fresh" }
            },
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "profile": "reviewer",
                "launch_mode": { "kind": "Fork", "from": "missing-source", "context": "FullHistory" }
            }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "unknown_launch_step")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "unknown_launch_member")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "fork_launch_unknown_source")
        );
    }

    #[test]
    fn rejects_missing_launch_mode_for_editor_member_step() {
        let mut document = document_with_real_launch_modes();
        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "profile": "planner",
                "launch_mode": { "kind": "Fresh" }
            }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "missing_step_launch_mode")
        );
    }

    #[test]
    fn rejects_null_launch_mode_for_editor_member_step() {
        let mut document = document_with_real_launch_modes();
        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "profile": "planner",
                "launch_mode": null
            },
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "profile": "reviewer",
                "launch_mode": { "kind": "Fork", "from": "plan", "context": "FullHistory" }
            }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_launch_mode"
                && diagnostic.path.as_deref() == Some("launch_modes[0].launch_mode")
        }));
    }

    #[test]
    fn rejects_launch_profile_mismatch_and_invalid_fork_context() {
        let mut document = document_with_real_launch_modes();
        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "profile": "reviewer",
                "launch_mode": { "kind": "Fresh" }
            },
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "profile": "reviewer",
                "launch_mode": { "kind": "Fork", "from": "plan", "context": "Everything" }
            }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "launch_profile_mismatch")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_fork_launch_context")
        );
    }

    #[test]
    fn rejects_stale_non_mobkit_fork_launch_contexts() {
        let mut document = document_with_real_launch_modes();
        document.launch_modes = json!([
            {
                "step_id": "plan",
                "member_id": "m_planner",
                "profile": "planner",
                "launch_mode": { "kind": "Fresh" }
            },
            {
                "step_id": "review",
                "member_id": "m_reviewer",
                "profile": "reviewer",
                "launch_mode": { "kind": "Fork", "from": "plan", "context": "DiffOnly" }
            }
        ]);
        if let Some(steps) = document.flow["steps"].as_array_mut() {
            steps[2]["launchMode"] =
                json!({ "kind": "Fork", "from": "plan", "context": "DiffOnly" });
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_fork_launch_context"
                && diagnostic.path.as_deref() == Some("launch_modes[1].launch_mode")
        }));
    }

    #[test]
    fn validates_graph_projection_against_editor_flow_and_members() {
        let document = document_with_real_launch_modes();
        let result = validate_document(&document);

        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn rejects_graph_member_metadata_drift_from_flow_step() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        let plan = steps
            .iter_mut()
            .find(|step| step["id"] == "plan")
            .expect("plan step");
        plan["timeoutMs"] = json!(2500);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_member_metadata_mismatch"
                && diagnostic.path.as_deref() == Some("instances[0].timeoutMs")
        }));
    }

    #[test]
    fn rejects_graph_launch_mode_drift_from_flow_step() {
        let mut document = document_with_real_launch_modes();
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances[1]["launchMode"] = json!({ "kind": "Fresh" });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_launch_mode_mismatch"
                && diagnostic.path.as_deref() == Some("instances[1].launchMode")
        }));
    }

    #[test]
    fn rejects_missing_graph_launch_mode_when_flow_requires_one() {
        let mut document = document_with_real_launch_modes();
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances[1]
            .as_object_mut()
            .expect("graph member object")
            .remove("launchMode");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_launch_mode_mismatch"
                && diagnostic.path.as_deref() == Some("instances[1].launchMode")
        }));
    }

    #[test]
    fn validates_editor_document_without_client_rendered_mob_toml() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        let result =
            validate_mobpack(&json!({ "document": document })).expect("validate rendered document");

        assert!(result.ok, "{:?}", result.diagnostics);
        assert_eq!(result.mob_id.as_deref(), Some("review_pack"));
        assert_eq!(result.flow_ids, vec!["main".to_string()]);
    }

    #[test]
    fn validates_single_member_editor_document_with_empty_graph_edges() {
        let mut document = document_with_reviewer_output_schema();
        document.mob_toml = None;
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "inputParams": [] },
                { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review" }
            ]
        });
        document.instances = json!([
            { "id": "review", "memberId": "m_reviewer", "col": 0, "row": 0, "launchMode": { "kind": "Fresh" } }
        ]);
        document.edges = json!([]);
        document.frames = json!([]);
        document.launch_modes = json!([{
            "step_id": "review",
            "member_id": "m_reviewer",
            "profile": "reviewer",
            "launch_mode": { "kind": "Fresh" },
            "budget_split_policy": { "type": "equal" }
        }]);

        let result =
            validate_mobpack(&json!({ "document": document })).expect("validate rendered document");

        assert!(result.ok, "{:?}", result.diagnostics);
        assert_eq!(result.flow_ids, vec!["main".to_string()]);
    }

    #[test]
    fn exports_editor_document_without_client_rendered_mob_toml() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        let result =
            export_mobpack(&json!({ "document": document })).expect("export rendered document");

        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        assert!(result.mob_toml.contains("[profiles.planner]"));
        assert!(result.mob_toml.contains("[flows.main.steps."));
        assert!(!result.mob_toml.contains("missing_"));
    }

    #[test]
    fn exports_agent_definition_source_metadata_only_in_editor_projection() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        let members = document.members.as_array_mut().expect("members");
        members[0]["sourceDefinition"] = json!({
            "definitionType": "mobkit/profile-member",
            "definitionId": "planner",
            "source": "mobkit/mobpack-profile-member",
            "sourceMobpack": "sample_planner_coder_review_loop",
            "sourceOrigin": "mobkit/sample-mobpack",
            "sourceDocumentPath": "document.members[]"
        });

        let result =
            export_mobpack(&json!({ "document": document })).expect("export rendered document");

        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        assert!(!result.mob_toml.contains("sourceDefinition"));
        assert!(!result.mob_toml.contains("sourceMobpack"));
        assert!(!result.mob_toml.contains("sample_planner_coder_review_loop"));
        let editor_json = result
            .source_files
            .iter()
            .find(|file| file.path == "mobkit/editor.json")
            .and_then(|file| file.text.as_deref())
            .expect("mobkit/editor.json source file");
        let editor_json: Value = serde_json::from_str(editor_json).expect("editor json");
        assert_eq!(
            editor_json["document"]["members"][0]["sourceDefinition"]["sourceMobpack"],
            json!("sample_planner_coder_review_loop")
        );
        assert_eq!(
            editor_json["document"]["members"][0]["sourceDefinition"]["sourceOrigin"],
            json!("mobkit/sample-mobpack")
        );
    }

    #[test]
    fn complete_editor_projection_overrides_stale_packed_mob_toml() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = Some(
            r#"
[mob]
id = "stale-pack"

[profiles.planner]
model = "gpt-5.5"
skills = []

[profiles.planner.tools]
comms = true

[flows.stale]
description = "Stale flow"

[flows.stale.steps.plan]
role = "planner"
message = "stale"
"#
            .to_string(),
        );

        let validation = validate_document(&document);
        assert!(validation.ok, "{:?}", validation.diagnostics);
        assert_eq!(validation.mob_id.as_deref(), Some("review_pack"));

        let result =
            export_mobpack(&json!({ "document": document })).expect("export rendered document");
        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        assert!(result.mob_toml.contains("[profiles.reviewer]"));
        assert!(result.mob_toml.contains(r#"role = "reviewer""#));
        assert!(result.mob_toml.contains(r#"message = "Review""#));
        assert!(!result.mob_toml.contains("stale-pack"));
        assert!(!result.mob_toml.contains("[flows.stale]"));
    }

    #[test]
    fn rejects_flow_member_step_without_real_member_definition() {
        let mut document = document_with_real_launch_modes();
        document.flow["steps"][1]["role"] = json!("m_missing");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unknown_flow_member"
                && diagnostic.path.as_deref() == Some("flow.steps[1].role")
        }));
    }

    #[test]
    fn rejects_flow_member_step_without_member_ref() {
        let mut document = document_with_real_launch_modes();
        document.flow["steps"][1]["role"] = json!("");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_flow_member"
                && diagnostic.path.as_deref() == Some("flow.steps[1].role")
        }));
    }

    #[test]
    fn validates_compiled_graph_gates_and_frames_from_flow_primitives() {
        let document = document_with_parallel_graph_controls();
        let result = validate_document(&document);

        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn validates_branch_join_member_as_real_profile_step() {
        let document = document_with_branch_graph_controls();
        let result = validate_document(&document);
        assert!(result.ok, "{:?}", result.diagnostics);

        let export = export_mobpack(&json!({ "document": document })).expect("export");
        assert!(export.validation.ok, "{:?}", export.validation.diagnostics);
        assert!(export.mob_toml.contains("Join branch paths."));
        assert!(export.mob_toml.contains(r#"role = "reviewer""#));
    }

    #[test]
    fn rejects_branch_convergence_without_real_join_member() {
        let mut document = document_with_branch_graph_controls();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        steps[1].as_object_mut().unwrap().remove("controllerRole");
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances[3]
            .as_object_mut()
            .unwrap()
            .remove("controllerRole");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_branch_join_member"
                && diagnostic.path.as_deref() == Some("flow.steps[1].controllerRole")
        }));
    }

    #[test]
    fn rejects_graph_join_controller_drift_from_branch_flow() {
        let mut document = document_with_branch_graph_controls();
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances[3]["controllerRole"] = json!("m_planner");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_join_controller_mismatch"
                && diagnostic.path.as_deref() == Some("instances[3].controllerRole")
        }));
    }

    #[test]
    fn rejects_missing_compiled_graph_gate() {
        let mut document = document_with_parallel_graph_controls();
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances.retain(|instance| instance["id"] != "g_parallel_parallel_review");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_compiled_graph_gate"
                && diagnostic.path.as_deref() == Some("instances")
        }));
    }

    #[test]
    fn rejects_missing_compiled_graph_frame() {
        let mut document = document_with_parallel_graph_controls();
        document.frames = json!([]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_compiled_graph_frame"
                && diagnostic.path.as_deref() == Some("frames")
        }));
    }

    #[test]
    fn rejects_graph_frame_kind_drift_from_repeat_flow() {
        let mut document = document_with_expected_schema_ref();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                { "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" },
                {
                    "id": "node_loop",
                    "type": "repeat",
                    "loopId": "review_loop",
                    "maxIterations": 3,
                    "iterationInput": "carry",
                    "cond": { "stepId": "review", "field": "verdict", "op": "==", "val": "green" },
                    "steps": [
                        { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review", "schema": "ReviewArtifact" }
                    ]
                }
            ]
        });
        document.edges = json!([
            { "id": "e1", "from": "plan", "to": "review", "kind": "next", "label": "" },
            {
                "id": "e2",
                "from": "review",
                "to": "review",
                "kind": "cond",
                "label": "until condition",
                "cond": { "var": "steps.review.verdict", "op": "==", "val": "green" }
            }
        ]);
        document.frames = json!([
            { "id": "frame_node_loop", "kind": "Branch", "colStart": 1, "colEnd": 1, "label": "BRANCH · wrong frame" }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_frame_kind_mismatch"
                && diagnostic.path.as_deref() == Some("frames[0].kind")
        }));
    }

    #[test]
    fn validates_graph_join_collection_and_controller_against_parallel_flow() {
        let mut document = document_with_parallel_graph_controls();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        steps[1]["collection"] = json!("any");
        steps[1]["controllerRole"] = json!("m_planner");
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances[3]["collection"] = json!("any");
        instances[3]["controllerRole"] = json!("m_planner");
        instances[3]["label"] = json!("join · any");
        let result = validate_document(&document);

        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn rejects_graph_join_collection_drift_from_parallel_flow() {
        let mut document = document_with_parallel_graph_controls();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        steps[1]["collection"] = json!("any");
        steps[1]["controllerRole"] = json!("m_planner");
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances[3]["collection"] = json!("all");
        instances[3]["controllerRole"] = json!("m_planner");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_join_collection_mismatch"
                && diagnostic.path.as_deref() == Some("instances[3].collection")
        }));
    }

    #[test]
    fn rejects_graph_fork_dispatch_drift_from_parallel_flow() {
        let mut document = document_with_parallel_graph_controls();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        steps[1]["dispatch"] = json!("one_to_one");
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances[0]["dispatch"] = json!("fan_out");
        instances[0]["label"] = json!("fan_out");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_fork_dispatch_mismatch"
                && diagnostic.path.as_deref() == Some("instances[0].dispatch")
        }));
    }

    #[test]
    fn rejects_graph_join_controller_drift_from_parallel_flow() {
        let mut document = document_with_parallel_graph_controls();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        steps[1]["collection"] = json!("quorum");
        steps[1]["quorum"] = json!(1);
        steps[1]["controllerRole"] = json!("m_planner");
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances[3]["collection"] = json!("quorum");
        instances[3]["quorum"] = json!({ "mode": "NofM", "n": 1, "m": 2 });
        instances[3]["controllerRole"] = json!("m_reviewer");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_join_controller_mismatch"
                && diagnostic.path.as_deref() == Some("instances[3].controllerRole")
        }));
    }

    #[test]
    fn rejects_graph_join_quorum_drift_from_parallel_flow() {
        let mut document = document_with_parallel_graph_controls();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        steps[1]["collection"] = json!("quorum");
        steps[1]["quorum"] = json!(2);
        steps[1]["controllerRole"] = json!("m_planner");
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances[3]["collection"] = json!("quorum");
        instances[3]["quorum"] = json!({ "mode": "NofM", "n": 1, "m": 2 });
        instances[3]["controllerRole"] = json!("m_planner");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_join_quorum_mismatch"
                && diagnostic.path.as_deref() == Some("instances[3].quorum.n")
        }));
    }

    #[test]
    fn rejects_graph_fork_edge_drift_from_parallel_flow() {
        let mut document = document_with_parallel_graph_controls();
        let edges = document.edges.as_array_mut().expect("graph edges");
        edges[0]["kind"] = json!("next");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_flow_edge_mismatch"
                && diagnostic.path.as_deref() == Some("edges")
                && diagnostic
                    .message
                    .contains("from 'g_parallel_parallel_review' to 'plan'")
        }));
    }

    #[test]
    fn rejects_graph_join_edge_drift_from_parallel_flow() {
        let mut document = document_with_parallel_graph_controls();
        let edges = document.edges.as_array_mut().expect("graph edges");
        edges[2]["to"] = json!("review");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_flow_edge_mismatch"
                && diagnostic.path.as_deref() == Some("edges")
                && diagnostic
                    .message
                    .contains("from 'plan' to 'j_parallel_parallel_review'")
        }));
    }

    #[test]
    fn rejects_graph_sequence_edge_drift_from_flow() {
        let mut document = document_with_expected_schema_ref();
        let edges = document.edges.as_array_mut().expect("graph edges");
        edges[0]["from"] = json!("review");
        edges[0]["to"] = json!("plan");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_flow_edge_mismatch"
                && diagnostic.path.as_deref() == Some("edges")
                && diagnostic.message.contains("from 'plan' to 'review'")
        }));
    }

    #[test]
    fn rejects_extra_graph_edge_not_compiled_from_flow() {
        let mut document = document_with_expected_schema_ref();
        let edges = document.edges.as_array_mut().expect("graph edges");
        edges.push(json!({
            "id": "e_extra",
            "from": "review",
            "to": "plan",
            "kind": "next",
            "label": "visual-only shortcut"
        }));

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_extra_uncompiled_edge"
                && diagnostic.path.as_deref() == Some("edges")
                && diagnostic.message.contains("from 'review' to 'plan'")
        }));
    }

    #[test]
    fn rejects_graph_repeat_loop_edge_drift_from_flow() {
        let mut document = document_with_expected_schema_ref();
        document.flow = json!({
            "name": "review",
            "steps": [
                { "id": "input_1", "type": "input", "task": "Review", "fields": "" },
                { "id": "plan", "type": "member", "role": "m_planner", "instruction": "Plan" },
                {
                    "id": "node_loop",
                    "type": "repeat",
                    "loopId": "review_loop",
                    "maxIterations": 3,
                    "iterationInput": "carry",
                    "cond": { "stepId": "review", "field": "verdict", "op": "==", "val": "green" },
                    "steps": [
                        { "id": "review", "type": "member", "role": "m_reviewer", "instruction": "Review", "schema": "ReviewArtifact" }
                    ]
                }
            ]
        });
        document.edges = json!([
            { "id": "e1", "from": "plan", "to": "review", "kind": "next", "label": "" },
            {
                "id": "e2",
                "from": "review",
                "to": "plan",
                "kind": "cond",
                "label": "until condition",
                "cond": { "var": "steps.review.verdict", "op": "==", "val": "green" }
            }
        ]);
        document.frames = json!([
            { "id": "frame_node_loop", "kind": "RepeatUntil", "colStart": 1, "colEnd": 1, "label": "REPEAT · until condition" }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_flow_edge_mismatch"
                && diagnostic.path.as_deref() == Some("edges")
                && diagnostic.message.contains("from 'review' to 'review'")
        }));
    }

    #[test]
    fn rejects_parallel_any_collection_without_real_join_member() {
        let mut document = document_with_parallel_graph_controls();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        steps[1]["collection"] = json!("any");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_parallel_join_member"
                && diagnostic.path.as_deref() == Some("flow.steps[1].controllerRole")
        }));
    }

    #[test]
    fn rejects_unknown_parallel_control_member() {
        let mut document = document_with_parallel_graph_controls();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        steps[1]["controllerRole"] = json!("m_missing");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unknown_flow_control_member"
                && diagnostic.path.as_deref() == Some("flow.steps[1].controllerRole")
        }));
    }

    #[test]
    fn rejects_graph_member_instance_that_disagrees_with_flow_step() {
        let mut document = document_with_real_launch_modes();
        if let Some(instances) = document.instances.as_array_mut() {
            instances[1]["memberId"] = json!("m_planner");
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "graph_flow_member_mismatch"
                    && diagnostic.path.as_deref() == Some("instances[1].memberId"))
        );
    }

    #[test]
    fn rejects_graph_edge_with_unknown_endpoint_and_kind() {
        let mut document = document_with_real_launch_modes();
        document.edges = json!([
            { "id": "e1", "from": "plan", "to": "ghost", "kind": "magic", "label": "" }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| diagnostic.code
            == "unknown_graph_edge_endpoint"
            && diagnostic.path.as_deref() == Some("edges[0].to")));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_graph_edge_kind"
                    && diagnostic.path.as_deref() == Some("edges[0].kind"))
        );
    }

    #[test]
    fn rejects_graph_edge_without_explicit_kind() {
        let mut document = document_with_real_launch_modes();
        document.edges = json!([
            { "id": "e1", "from": "plan", "to": "review", "label": "" }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_graph_edge_kind"
                    && diagnostic.path.as_deref() == Some("edges[0].kind")
                    && diagnostic.message.contains("non-empty kind"))
        );
    }

    #[test]
    fn rejects_visual_terminal_graph_edge_kind() {
        let mut document = document_with_real_launch_modes();
        document.edges = json!([
            { "id": "e1", "from": "plan", "to": "review", "kind": "term", "label": "done" }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_graph_edge_kind"
                && diagnostic.path.as_deref() == Some("edges[0].kind")
        }));
    }

    #[test]
    fn rejects_visual_only_graph_gate_terminal_and_frame() {
        let mut document = document_with_real_launch_modes();
        if let Some(instances) = document.instances.as_array_mut() {
            instances.push(
                json!({ "id": "branch_visual", "isGate": true, "gateKind": "branch", "col": 2, "row": 0 }),
            );
            instances.push(
                json!({ "id": "terminal_visual", "isTerminal": true, "kind": "success", "col": 3, "row": 0 }),
            );
        }
        document.frames = json!([
            { "id": "frame_visual", "kind": "Parallel", "colStart": 0, "colEnd": 2, "label": "visual" }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "uncompiled_graph_gate"
                && diagnostic.path.as_deref() == Some("instances[2]")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "uncompiled_graph_terminal"
                && diagnostic.path.as_deref() == Some("instances[3]")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "uncompiled_graph_frame"
                && diagnostic.path.as_deref() == Some("frames[0]")
        }));
    }

    #[test]
    fn rejects_missing_graph_instance_for_flow_member_step() {
        let mut document = document_with_real_launch_modes();
        document.instances = json!([
            { "id": "plan", "memberId": "m_planner", "col": 0, "row": 0, "launchMode": { "kind": "Fresh" } }
        ]);
        document.edges = json!([]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "flow_step_missing_graph_instance")
        );
    }

    #[test]
    fn rejects_invalid_graph_gate_terminal_and_frame_kinds() {
        let mut document = document_with_real_launch_modes();
        if let Some(instances) = document.instances.as_array_mut() {
            instances.push(json!({ "id": "gate_bad", "isGate": true, "gateKind": "merge", "col": 2, "row": 0 }));
            instances.push(json!({ "id": "terminal_bad", "isTerminal": true, "kind": "done-ish", "col": 3, "row": 0 }));
        }
        document.frames = json!([
            { "id": "frame_bad", "kind": "Decorative", "colStart": 0, "colEnd": 1, "label": "bad" }
        ]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_graph_gate_kind")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_graph_terminal_kind")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_graph_frame_kind")
        );
    }

    #[test]
    fn validates_rkat_deploy_settings() {
        let mut document = valid_document();
        document.deploy = json!({
            "command": "rkat mob deploy",
            "surface": "cli",
            "trust_policy": "permissive",
            "realm_backend": "jsonl",
            "max_total_tokens": 64,
            "max_tool_calls": 0,
            "model": "gpt-5.5",
            "isolated": true,
            "prompt": "Reply with exactly OK."
        });
        let result = validate_document(&document);

        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn rejects_non_rkat_deploy_command_and_invalid_deploy_enums() {
        let mut document = valid_document();
        document.deploy = json!({
            "command": "not-rkat deploy",
            "surface": "desktop",
            "trust_policy": "dangerous",
            "realm_backend": "memory",
            "model": "definitely-not-a-real-model",
            "prompt": "Reply with exactly OK."
        });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_deploy_command")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_deploy_surface")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_deploy_trust_policy")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "unknown_deploy_model")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_deploy_realm_backend")
        );
    }

    #[test]
    fn rejects_autonomous_host_profiles_for_cli_deploy() {
        let mut document = document_with_real_launch_modes();
        document.members[0]["runtimeMode"] = json!("autonomous_host");
        document.mob_toml = None;

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "deploy_surface_runtime_mode_unsupported"
                && diagnostic.path.as_deref() == Some("members[0].runtimeMode")
                && diagnostic.message.contains("deploy surface 'cli'")
                && diagnostic.message.contains("RPC surface only")
        }));
    }

    #[test]
    fn allows_autonomous_host_profiles_for_rpc_deploy_surface() {
        let mut document = document_with_real_launch_modes();
        document.deploy["surface"] = json!("rpc");
        document.members[0]["runtimeMode"] = json!("autonomous_host");
        document.mob_toml = None;

        let result = validate_document(&document);

        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn rejects_invalid_deploy_numbers_and_blank_prompt() {
        let mut document = valid_document();
        document.deploy = json!({
            "command": "rkat mob deploy",
            "max_total_tokens": -1,
            "max_tool_calls": 1.5,
            "isolated": "yes",
            "prompt": "  "
        });
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_deploy_number"
                    && diagnostic.path.as_deref() == Some("deploy.max_total_tokens"))
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_deploy_number"
                    && diagnostic.path.as_deref() == Some("deploy.max_tool_calls"))
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_deploy_isolated")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_deploy_prompt")
        );
    }

    #[test]
    fn validates_member_catalog_refs_from_real_mobkit_schema() {
        let document = document_with_member_catalog_refs(
            json!(["builtins", "comms"]),
            json!(["mob.workpad"]),
            json!([{
                "id": "mobkit/sample-mobpacks",
                "source": "mobkit/sample-mobpack",
                "skills": [{
                    "id": "mob.workpad",
                    "source": "inline",
                    "origin": "mobkit/sample-mobpack",
                    "content": "Keep the shared workpad concise and current."
                }]
            }]),
        );
        let result = validate_document(&document);
        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn rejects_unknown_editor_tool_and_skill_refs() {
        let document = document_with_member_catalog_refs(
            json!(["builtins", "__definitely_not_a_real_tool__"]),
            json!(["missing.skill"]),
            json!([{
                "id": "mobkit/sample-mobpacks",
                "source": "mobkit/sample-mobpack",
                "skills": [{ "id": "mob.workpad", "source": "inline", "origin": "mobkit/sample-mobpack" }]
            }]),
        );
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unknown_tool_ref"
                && diagnostic.path.as_deref() == Some("members[0].tools[1]")
                && diagnostic.message.contains("MobKit tool_catalog")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unknown_skill_ref"
                && diagnostic.path.as_deref() == Some("members[0].skills[0]")
        }));
    }

    #[test]
    fn warns_on_unknown_editor_member_model_ref() {
        let mut document = document_with_member_catalog_refs(
            json!(["builtins", "comms"]),
            json!(["mob.workpad"]),
            json!([{
                "id": "mobkit/sample-mobpacks",
                "skills": [{
                    "id": "mob.workpad",
                    "source": "inline",
                    "content": "Keep the shared workpad concise and current."
                }]
            }]),
        );
        document.members[0]["model"] = json!("definitely-not-a-real-model");
        document.mob_toml = document.mob_toml.map(|text| {
            text.replace(
                "model = \"gpt-5.5\"",
                "model = \"definitely-not-a-real-model\"",
            )
        });
        let result = validate_document(&document);

        assert!(result.ok, "{:?}", result.diagnostics);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unknown_model_ref"
                && diagnostic.severity == "warning"
                && diagnostic.path.as_deref() == Some("members[0].model")
        }));
    }

    #[test]
    fn rejects_non_string_editor_member_model_ref() {
        let mut document = document_with_member_catalog_refs(
            json!(["builtins", "comms"]),
            json!(["mob.workpad"]),
            json!([{
                "id": "mobkit/sample-mobpacks",
                "skills": [{ "id": "mob.workpad" }]
            }]),
        );
        document.members[0]["model"] = json!(7);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_model_ref"
                && diagnostic.path.as_deref() == Some("members[0].model")
        }));
    }

    #[test]
    fn rejects_self_declared_unconfigured_mcp_tool_ref() {
        let mut document = document_with_member_catalog_refs(
            json!(["builtins", "mcp:not-configured"]),
            json!([]),
            json!([]),
        );
        document.mob_toml = document
            .mob_toml
            .map(|text| text.replace("comms = true", "comms = false\nmcp = [\"not-configured\"]"));
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unknown_tool_ref"
                && diagnostic.path.as_deref() == Some("members[0].tools[1]")
        }));
    }

    #[test]
    fn rejects_non_string_editor_tool_and_skill_refs() {
        let document = document_with_member_catalog_refs(
            json!(["builtins", 7]),
            json!(["mob.workpad", null]),
            json!([{
                "id": "mobkit/sample-mobpacks",
                "source": "mobkit/sample-mobpack",
                "skills": [{ "id": "mob.workpad", "source": "inline", "origin": "mobkit/sample-mobpack" }]
            }]),
        );
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_tool_ref"
                && diagnostic.path.as_deref() == Some("members[0].tools[1]")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_skill_ref"
                && diagnostic.path.as_deref() == Some("members[0].skills[1]")
        }));
    }

    #[test]
    fn validates_unused_editor_members_as_real_profiles() {
        let document = document_with_unused_reviewer_profile();
        let result = validate_document(&document);

        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn rejects_editor_member_missing_from_mob_toml_profiles() {
        let mut document = document_with_unused_reviewer_profile();
        document.mob_toml = Some(
            r#"
[mob]
id = "review-pack"

[profiles.planner]
model = "gpt-5.5"
skills = []
peer_description = "Planner"

[profiles.planner.tools]
comms = true

[flows.review]
description = "Review flow"

[flows.review.steps.plan]
role = "planner"
message = "Plan the work"
"#
            .to_string(),
        );
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "editor_profile_missing_from_mob_toml"
                && diagnostic.path.as_deref() == Some("members[1]")
        }));
    }

    #[test]
    fn rejects_editor_profile_tool_mismatch() {
        let mut document = document_with_unused_reviewer_profile();
        if let Some(members) = document.members.as_array_mut() {
            members[0]["tools"] = json!(["builtins"]);
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "editor_profile_tools_mismatch"
                && diagnostic.path.as_deref() == Some("members[0].tools")
        }));
    }

    #[test]
    fn rejects_invalid_or_drifting_provider_params() {
        let mut document = document_with_unused_reviewer_profile();
        if let Some(members) = document.members.as_array_mut() {
            members[0]["providerParams"] = json!(["not", "an", "object"]);
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_provider_params"
                && diagnostic.path.as_deref() == Some("members[0].providerParams")
        }));

        let mut document = document_with_unused_reviewer_profile();
        if let Some(members) = document.members.as_array_mut() {
            members[0]["providerParams"] = json!({ "thinking_budget": 4096 });
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "editor_profile_provider_params_mismatch"
                && diagnostic.path.as_deref() == Some("members[0].providerParams")
        }));
    }

    #[test]
    fn rejects_invalid_or_drifting_profile_backend_settings() {
        let mut document = document_with_unused_reviewer_profile();
        if let Some(members) = document.members.as_array_mut() {
            members[0]["backend"] = json!("wire");
            members[0]["maxInlinePeerNotifications"] = json!(-2);
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_profile_backend"
                && diagnostic.path.as_deref() == Some("members[0].backend")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_max_inline_peer_notifications"
                && diagnostic.path.as_deref() == Some("members[0].maxInlinePeerNotifications")
        }));

        let mut document = document_with_unused_reviewer_profile();
        if let Some(members) = document.members.as_array_mut() {
            members[0]["backend"] = json!("external");
            members[0]["maxInlinePeerNotifications"] = json!(0);
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "editor_profile_backend_mismatch"
                && diagnostic.path.as_deref() == Some("members[0].backend")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "editor_profile_max_inline_peer_notifications_mismatch"
                && diagnostic.path.as_deref() == Some("members[0].maxInlinePeerNotifications")
        }));
    }

    #[test]
    fn complete_editor_mob_settings_render_as_authoritative() {
        let result = import_mobpack(&json!({ "mob_toml": importable_mob_toml() })).expect("import");
        let mut document: MobpackDocument =
            serde_json::from_value(result["document"].clone()).expect("document");
        document.mob_settings["orchestrator"] = json!("reviewer");
        document.mob_settings["autoWireOrchestrator"] = json!(false);
        document.mob_settings["roleWiring"] = json!([{ "a": "reviewer", "b": "planner" }]);
        document.mob_settings["backendDefault"] = json!("session");
        document.mob_settings["externalAddressBase"] = json!("http://127.0.0.1:9999");
        document.mob_settings["advanced"]["topology"]["mode"] = json!("advisory");
        document.mob_settings["advanced"]["supervisor"]["escalation_threshold"] = json!(3);
        document.mob_settings["advanced"]["limits"]["max_orphaned_turns"] = json!(9);
        document.mob_settings["advanced"]["spawnPolicy"]["profile_map"]["reviewer"] =
            json!("planner");
        document.mob_settings["advanced"]["eventRouter"]["buffer_size"] = json!(256);
        let validation = validate_document(&document);

        assert!(validation.ok, "{:?}", validation.diagnostics);
        let mob_toml = render_editor_document_mob_toml(&document).expect("rendered mob.toml");
        assert!(mob_toml.contains(r#"orchestrator = "reviewer""#));
        assert!(mob_toml.contains("[backend.external]"));
        assert!(mob_toml.contains(r#"address_base = "http://127.0.0.1:9999""#));
    }

    #[test]
    fn member_step_runtime_metadata_exports_only_when_authored() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;

        let validation = validate_document(&document);
        assert!(validation.ok, "{:?}", validation.diagnostics);
        let mob_toml = render_editor_document_mob_toml(&document).expect("rendered mob.toml");
        assert!(!mob_toml.contains("dispatch_mode ="));
        assert!(!mob_toml.contains("collection_policy ="));
        assert!(!mob_toml.contains("output_format ="));

        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        let plan = steps
            .iter_mut()
            .find(|step| step["id"] == "plan")
            .expect("plan step");
        plan["dispatchMode"] = json!("one_to_one");
        plan["collection"] = json!("quorum");
        plan["quorum"] = json!(1);
        plan["outputFormat"] = json!("text");
        if let Some(instances) = document.instances.as_array_mut() {
            instances[0]["dispatchMode"] = json!("one_to_one");
            instances[0]["collection"] = json!("quorum");
            instances[0]["quorum"] = json!(1);
            instances[0]["outputFormat"] = json!("text");
        }

        let validation = validate_document(&document);
        assert!(validation.ok, "{:?}", validation.diagnostics);
        let mob_toml = render_editor_document_mob_toml(&document).expect("rendered mob.toml");
        assert!(mob_toml.contains(r#"dispatch_mode = "one_to_one""#));
        assert!(mob_toml.contains(r#"collection_policy = { type = "quorum", n = 1 }"#));
        assert!(mob_toml.contains(r#"output_format = "text""#));
    }

    #[test]
    fn rejects_drifting_flow_step_runtime_metadata() {
        let result = import_mobpack(&json!({ "mob_toml": importable_mob_toml() })).expect("import");
        let mut document: MobpackDocument =
            serde_json::from_value(result["document"].clone()).expect("document");
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        let plan = steps
            .iter_mut()
            .find(|step| step["id"] == "plan")
            .expect("plan step");
        plan["timeoutMs"] = json!(2500);
        plan["allowedTools"] = json!(["comms"]);
        plan["blockedTools"] = json!(["mob"]);
        plan["outputFormat"] = json!("json");
        plan["dispatchMode"] = json!("fan_in");
        plan["collection"] = json!("any");
        plan["quorum"] = Value::Null;
        let validation = validate_document(&document);

        assert!(!validation.ok);
        assert!(validation.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "graph_member_metadata_mismatch"
                && diagnostic.path.as_deref() == Some("instances[0].timeoutMs")
        }));
    }

    #[test]
    fn validates_step_tool_refs_against_member_profile_tools() {
        let mut document = document_with_real_launch_modes();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        let plan = steps
            .iter_mut()
            .find(|step| step["id"] == "plan")
            .expect("plan step");
        plan["allowedTools"] = json!(["comms"]);
        let review = steps
            .iter_mut()
            .find(|step| step["id"] == "review")
            .expect("review step");
        review["blockedTools"] = json!(["shell"]);
        let instances = document.instances.as_array_mut().expect("graph instances");
        instances[0]["allowedTools"] = json!(["comms"]);
        instances[1]["blockedTools"] = json!(["shell"]);
        document.mob_toml = document.mob_toml.map(|text| {
            text.replace(
                "message = \"Plan\"",
                "message = \"Plan\"\nallowed_tools = [\"comms\"]",
            )
            .replace(
                "message = \"Review\"\ndepends_on = [\"plan\"]",
                "message = \"Review\"\ndepends_on = [\"plan\"]\nblocked_tools = [\"shell\"]",
            )
        });
        let validation = validate_document(&document);

        assert!(validation.ok, "{:?}", validation.diagnostics);
    }

    #[test]
    fn rejects_member_steps_without_explicit_instruction() {
        let mut document = document_with_real_launch_modes();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        let plan = steps
            .iter_mut()
            .find(|step| step["id"] == "plan")
            .expect("plan step");
        plan.as_object_mut()
            .expect("plan object")
            .remove("instruction");
        let review = steps
            .iter_mut()
            .find(|step| step["id"] == "review")
            .expect("review step");
        review["instruction"] = json!(" ");

        let validation = validate_document(&document);

        assert!(!validation.ok);
        assert!(validation.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_flow_member_instruction"
                && diagnostic.path.as_deref() == Some("flow.steps[1].instruction")
        }));
        assert!(validation.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_flow_member_instruction"
                && diagnostic.path.as_deref() == Some("flow.steps[2].instruction")
        }));
    }

    #[test]
    fn renderer_does_not_synthesize_missing_member_turn_text() {
        let mut document = document_with_real_launch_modes();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        for step in steps.iter_mut().filter(|step| step["type"] == "member") {
            step.as_object_mut()
                .expect("member step object")
                .remove("instruction");
        }

        let mob_toml = render_editor_document_mob_toml(&document).expect("rendered mob.toml");

        assert!(!mob_toml.contains("planner turn"));
        assert!(!mob_toml.contains("reviewer turn"));
        assert!(!mob_toml.contains("message ="));
    }

    #[test]
    fn rejects_unknown_step_tool_refs() {
        let mut document = document_with_real_launch_modes();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        let plan = steps
            .iter_mut()
            .find(|step| step["id"] == "plan")
            .expect("plan step");
        plan["allowedTools"] = json!(["__not_a_mobkit_tool__"]);
        let validation = validate_document(&document);

        assert!(!validation.ok);
        assert!(validation.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unknown_step_tool_ref"
                && diagnostic.path.as_deref() == Some("flow.steps[1].allowedTools[0]")
                && diagnostic.message.contains("MobKit tool_catalog")
        }));
    }

    #[test]
    fn rejects_unsupported_editor_step_runtime_metadata() {
        let mut document = document_with_real_launch_modes();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        let plan = steps
            .iter_mut()
            .find(|step| step["id"] == "plan")
            .expect("plan step");
        plan["dependsMode"] = json!("first");
        plan["dispatchMode"] = json!("broadcast");
        plan["collection"] = json!("winner");
        plan["outputFormat"] = json!("xml");
        plan["timeoutMs"] = json!(0);
        let validation = validate_document(&document);

        assert!(!validation.ok);
        for code in [
            "invalid_editor_dependency_mode",
            "invalid_editor_dispatch_mode",
            "invalid_editor_collection_policy",
            "invalid_editor_output_format",
            "invalid_editor_timeout",
        ] {
            assert!(
                validation
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == code),
                "missing {code}: {:?}",
                validation.diagnostics
            );
        }
    }

    #[test]
    fn rejects_parallel_steps_without_explicit_dispatch_and_collection() {
        let mut document = document_with_real_launch_modes();
        document.flow = json!({
            "name": "missing parallel metadata",
            "steps": [{
                "id": "parallel_missing_metadata",
                "type": "parallel",
                "branches": [{
                    "id": "br_plan",
                    "label": "Plan",
                    "steps": [{
                        "id": "plan",
                        "type": "member",
                        "role": "m_planner",
                        "instruction": "Plan"
                    }]
                }]
            }]
        });
        document.launch_modes = json!([]);

        let validation = validate_document(&document);

        assert!(!validation.ok);
        assert!(validation.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_parallel_dispatch_mode"
                && diagnostic.path.as_deref() == Some("flow.steps[0].dispatch")
        }));
        assert!(validation.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_parallel_collection_policy"
                && diagnostic.path.as_deref() == Some("flow.steps[0].collection")
        }));
    }

    #[test]
    fn rejects_invalid_editor_quorum_metadata() {
        let mut document = document_with_real_launch_modes();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        let plan = steps
            .iter_mut()
            .find(|step| step["id"] == "plan")
            .expect("plan step");
        plan["collection"] = json!("quorum");
        plan["quorum"] = json!(0);
        let validation = validate_document(&document);

        assert!(!validation.ok);
        assert!(validation.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_editor_collection_quorum"
                && diagnostic.path.as_deref() == Some("flow.steps[1].quorum")
        }));
    }

    #[test]
    fn rejects_non_string_step_tool_refs() {
        let mut document = document_with_real_launch_modes();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        let plan = steps
            .iter_mut()
            .find(|step| step["id"] == "plan")
            .expect("plan step");
        plan["blockedTools"] = json!(["comms", null]);
        let validation = validate_document(&document);

        assert!(!validation.ok);
        assert!(validation.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_step_tool_ref"
                && diagnostic.path.as_deref() == Some("flow.steps[1].blockedTools[1]")
        }));
    }

    #[test]
    fn rejects_step_tool_refs_not_enabled_on_member_profile() {
        let mut document = document_with_real_launch_modes();
        let steps = document
            .flow
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .expect("flow steps");
        let plan = steps
            .iter_mut()
            .find(|step| step["id"] == "plan")
            .expect("plan step");
        plan["allowedTools"] = json!(["mob"]);
        let validation = validate_document(&document);

        assert!(!validation.ok);
        assert!(validation.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "step_tool_not_enabled_on_member"
                && diagnostic.path.as_deref() == Some("flow.steps[1].allowedTools[0]")
        }));
    }

    #[test]
    fn validates_editor_schema_matches_profile_output_schema() {
        let document = document_with_reviewer_output_schema();
        let result = validate_document(&document);

        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn rejects_malformed_editor_schema_fields_before_export() {
        let mut document = document_with_reviewer_output_schema();
        if let Some(schemas) = document.schemas.as_array_mut() {
            schemas[0]["fields"] = json!([
                { "id": "f1", "name": "verdict", "type": "enum", "required": true, "enumValues": ["green", "", "green"] },
                { "id": "f2", "name": "verdict", "type": "string", "enumValues": ["stale"] },
                { "id": "f3", "name": "", "type": "mystery" }
            ]);
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        for code in [
            "invalid_editor_schema_enum_value",
            "duplicate_editor_schema_enum_value",
            "duplicate_editor_schema_field",
            "stale_editor_schema_enum_values",
            "invalid_editor_schema_field_name",
            "invalid_editor_schema_field_type",
        ] {
            assert!(
                result
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == code),
                "missing {code}: {:?}",
                result.diagnostics
            );
        }
    }

    #[test]
    fn rejects_editor_schema_missing_from_document() {
        let mut document = document_with_reviewer_output_schema();
        document.schemas = json!([]);
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "editor_schema_missing"
                && diagnostic.path.as_deref() == Some("members[1].schema")
        }));
    }

    #[test]
    fn rejects_profile_output_schema_missing_from_editor_member() {
        let mut document = document_with_reviewer_output_schema();
        if let Some(members) = document.members.as_array_mut() {
            members[1]["schema"] = json!("");
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "editor_profile_schema_missing"
                && diagnostic.path.as_deref() == Some("members[1].schema")
        }));
    }

    #[test]
    fn rejects_editor_schema_field_drift_from_profile_output_schema() {
        let mut document = document_with_reviewer_output_schema();
        if let Some(schemas) = document.schemas.as_array_mut() {
            schemas[0]["fields"][0]["enumValues"] = json!(["green", "yellow", "red"]);
        }
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "editor_profile_schema_mismatch"
                && diagnostic.path.as_deref() == Some("members[1].schema")
        }));
    }

    #[test]
    fn exports_expected_schema_ref_files_as_deployable_archive_members() {
        let mut document = document_with_expected_schema_ref();
        document.flow["steps"][0]["inputParams"] = json!([
            { "id": "p1", "name": "size", "type": "number", "required": true, "enumValues": [] },
            { "id": "p2", "name": "kind", "type": "enum", "required": true, "enumValues": ["docs", "code"] },
            { "id": "p3", "name": "tags", "type": "string[]", "required": false, "enumValues": [] },
            { "id": "p4", "name": "metadata", "type": "object", "required": false, "enumValues": [] }
        ]);
        document.flow["steps"][0]["fields"] =
            json!("size: number, kind: enum, tags: string[]?, metadata: object?");
        let validation = validate_document(&document);
        assert!(validation.ok, "{:?}", validation.diagnostics);
        let result = export_mobpack(&json!({ "document": document })).expect("export");
        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        let exported_paths = result
            .source_files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert!(exported_paths.contains(&"manifest.toml"));
        assert!(exported_paths.contains(&"definition.json"));
        assert!(exported_paths.contains(&"mobkit/editor.json"));
        assert!(exported_paths.contains(&"mobkit/mob.toml"));
        assert!(exported_paths.contains(&"schemas/reviewer.json"));
        assert!(exported_paths.contains(&"schemas/main-input.json"));
        let exported_mob_toml = result
            .source_files
            .iter()
            .find(|file| file.path == "mobkit/mob.toml")
            .expect("mob.toml source file");
        assert_eq!(exported_mob_toml.media_type, "text/toml");
        assert_eq!(
            exported_mob_toml.text.as_deref(),
            Some(result.mob_toml.as_str())
        );
        assert_eq!(
            exported_mob_toml.sha256,
            source_file_sha256(result.mob_toml.as_bytes())
        );
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&result.content_base64)
            .unwrap();
        let mut archive = tar::Archive::new(GzDecoder::new(bytes.as_slice()));
        let mut files = BTreeMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            files.insert(path, bytes);
        }
        let schema = files
            .get("schemas/reviewer.json")
            .expect("reviewer schema archive member");
        let schema: Value = serde_json::from_slice(schema).expect("schema json");
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["properties"]["verdict"]["enum"],
            json!(["green", "red"])
        );
        let input_schema = files
            .get("schemas/main-input.json")
            .expect("main input schema archive member");
        let input_schema: Value = serde_json::from_slice(input_schema).expect("input schema json");
        assert_eq!(input_schema["type"], "object");
        assert_eq!(input_schema["required"], json!(["size", "kind"]));
        assert_eq!(input_schema["properties"]["size"]["type"], "number");
        assert_eq!(
            input_schema["properties"]["kind"]["enum"],
            json!(["docs", "code"])
        );
        assert_eq!(input_schema["properties"]["tags"]["type"], "array");
        assert_eq!(
            input_schema["properties"]["tags"]["items"]["type"],
            "string"
        );
        assert_eq!(input_schema["properties"]["metadata"]["type"], "object");
    }

    #[test]
    fn exports_selected_filesystem_skills_as_packed_mobpack_members() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_path = dir.path().join("SKILL.md");
        std::fs::write(&skill_path, "Use the real MobKit platform contract.").expect("write skill");

        let mut document = document_with_expected_schema_ref();
        document.mob_toml = None;
        document.members.as_array_mut().unwrap()[0]["skills"] = json!(["mob.platform"]);
        document.skill_realms = json!([{
            "id": "local/filesystem",
            "label": "Local filesystem",
            "source": "filesystem",
            "skills": [{
                "id": "mob.platform",
                "label": "MobKit Platform",
                "source": "path",
                "path": skill_path
            }]
        }]);

        let result = export_mobpack(&json!({ "document": document })).expect("export");
        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        assert!(result.mob_toml.contains(r#"[skills."mob.platform"]"#));
        assert!(result.mob_toml.contains(r#"source = "path""#));
        assert!(
            result
                .mob_toml
                .contains(r#"path = "skills/mob-platform.md""#)
        );
        assert!(
            !result
                .mob_toml
                .contains(dir.path().to_string_lossy().as_ref())
        );
        let exported_skill = result
            .source_files
            .iter()
            .find(|file| file.path == "skills/mob-platform.md")
            .expect("packed skill source file");
        assert_eq!(exported_skill.media_type, "text/markdown");
        assert_eq!(
            exported_skill.text.as_deref(),
            Some("Use the real MobKit platform contract.")
        );

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&result.content_base64)
            .unwrap();
        let mut archive = tar::Archive::new(GzDecoder::new(bytes.as_slice()));
        let mut files = BTreeMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            files.insert(path, bytes);
        }
        assert_eq!(
            String::from_utf8(files["skills/mob-platform.md"].clone()).unwrap(),
            "Use the real MobKit platform contract."
        );

        let imported =
            import_mobpack(&json!({ "content_base64": result.content_base64 })).expect("import");
        let imported_skill = imported["document"]["skill_realms"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|realm| realm["skills"].as_array().unwrap())
            .find(|skill| skill["id"] == "mob.platform")
            .expect("imported skill");
        assert_eq!(
            imported_skill["content"],
            json!("Use the real MobKit platform contract.")
        );
    }

    #[test]
    fn exports_editor_inline_skills_as_real_mobpack_definitions() {
        let mut document = document_with_expected_schema_ref();
        document.mob_toml = None;
        document.members.as_array_mut().unwrap()[0]["skills"] = json!(["mob.editor.review"]);
        document.skill_realms = json!([{
            "id": "mobkit/editor-inline",
            "label": "This mobpack",
            "source": "editor",
            "skills": [{
                "id": "mob.editor.review",
                "label": "Editor Review Skill",
                "source": "inline",
                "content": "Review the mob output against the editor-authored acceptance contract."
            }]
        }]);

        let result = export_mobpack(&json!({ "document": document })).expect("export");

        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        assert!(result.mob_toml.contains(r#"[skills."mob.editor.review"]"#));
        assert!(result.mob_toml.contains(r#"source = "inline""#));
        assert!(
            result
                .mob_toml
                .contains("Review the mob output against the editor-authored acceptance contract.")
        );
    }

    #[test]
    fn rejects_duplicate_skill_ids_across_editor_realms() {
        let mut document = document_with_expected_schema_ref();
        document.mob_toml = None;
        document.members.as_array_mut().unwrap()[0]["skills"] = json!(["mob.editor.review"]);
        document.skill_realms = json!([
            {
                "id": "mobkit/editor-inline",
                "label": "This mobpack",
                "source": "editor",
                "skills": [{
                    "id": "mob.editor.review",
                    "label": "Review",
                    "source": "inline",
                    "content": "Review the output."
                }]
            },
            {
                "id": "imported/mob.toml",
                "label": "Imported",
                "source": "import",
                "skills": [{
                    "id": "mob.editor.review",
                    "label": "Conflicting Review",
                    "source": "inline",
                    "content": "Different skill definition."
                }]
            }
        ]);

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "duplicate_skill_id"
                && diagnostic.path.as_deref() == Some("skill_realms[1].skills[0].id")
        }));
    }

    #[test]
    fn rejects_selected_skill_sources_that_are_not_mobkit_skill_sources() {
        let mut document = document_with_expected_schema_ref();
        document.mob_toml = None;
        document.members.as_array_mut().unwrap()[0]["skills"] = json!(["mob.editor.review"]);
        document.skill_realms = json!([{
            "id": "mobkit/editor-inline",
            "label": "This mobpack",
            "source": "editor",
            "skills": [{
                "id": "mob.editor.review",
                "label": "Review",
                "source": "catalog",
                "content": "Review the output."
            }]
        }]);

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_selected_skill_source"
                && diagnostic.path.as_deref()
                    == Some("skill_realms.skills[mob.editor.review].source")
        }));
        assert!(export_mobpack(&json!({ "document": document })).is_err());
    }

    #[test]
    fn rejects_selected_inline_skills_without_real_content() {
        let mut document = document_with_expected_schema_ref();
        document.mob_toml = None;
        document.members.as_array_mut().unwrap()[0]["skills"] = json!(["mob.editor.review"]);
        document.skill_realms = json!([{
            "id": "mobkit/editor-inline",
            "label": "This mobpack",
            "source": "editor",
            "skills": [{
                "id": "mob.editor.review",
                "label": "Review",
                "source": "inline",
                "content": "   "
            }]
        }]);

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "empty_selected_inline_skill"
                && diagnostic.path.as_deref()
                    == Some("skill_realms.skills[mob.editor.review].content")
        }));
        assert!(export_mobpack(&json!({ "document": document })).is_err());
    }

    #[test]
    fn rejects_starter_skill_realm_sources() {
        let mut document = document_with_expected_schema_ref();
        document.mob_toml = None;
        document.members.as_array_mut().unwrap()[0]["skills"] = json!(["mob.editor.review"]);
        document.skill_realms = json!([{
            "id": "mobkit/starters",
            "label": "Starter skills",
            "source": "starter",
            "skills": [{
                "id": "mob.editor.review",
                "label": "Review",
                "source": "starter",
                "origin": "mobkit/starters",
                "content": "Prototype starter skill."
            }]
        }]);

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_starter_skill_realm_id"
                && diagnostic.path.as_deref() == Some("skill_realms[0].id")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_starter_skill_realm_source"
                && diagnostic.path.as_deref() == Some("skill_realms[0].source")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_starter_skill_source"
                && diagnostic.path.as_deref() == Some("skill_realms[0].skills[0].source")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_starter_skill_origin"
                && diagnostic.path.as_deref() == Some("skill_realms[0].skills[0].origin")
        }));
    }

    #[test]
    fn import_rejects_starter_skill_realm_projection() {
        let mut document = document_with_expected_schema_ref();
        document.mob_toml = None;
        document.members.as_array_mut().unwrap()[0]["skills"] = json!(["mob.editor.review"]);
        document.skill_realms = json!([{
            "id": "mobkit/starters",
            "label": "Starter skills",
            "skills": [{
                "id": "mob.editor.review",
                "label": "Review",
                "source": "inline",
                "origin": "mobkit/starters",
                "content": "Prototype starter skill."
            }]
        }]);

        let err = import_mobpack(&json!({ "document": document })).expect_err("starter import");

        assert!(
            err.contains("cannot import prototype starter skill data"),
            "{err}"
        );
        assert!(err.contains("invalid_starter_skill_realm_id"), "{err}");
    }

    #[test]
    fn import_reports_api_backed_source_metadata() {
        let toml = importable_mob_toml();
        let result = import_mobpack(&json!({
            "source_name": "review.mob.toml",
            "source_media_type": "application/vnd.mobkit.editor-toml",
            "mob_toml": toml,
        }))
        .expect("import mob.toml");

        assert_eq!(result["source"], "mobkit/mobpacks/import:mob.toml");
        assert_eq!(result["source_label"], "review.mob.toml");
        assert_eq!(
            result["source_media_type"],
            "application/vnd.mobkit.editor-toml"
        );
        assert_eq!(result["validation"]["ok"], json!(true));
    }

    #[test]
    fn rejects_invalid_skill_realm_shape() {
        let mut document = document_with_expected_schema_ref();
        document.mob_toml = None;
        document.skill_realms = json!([{ "id": "", "skills": { "id": "mob.bad" } }]);

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_skill_realm_id"
                && diagnostic.path.as_deref() == Some("skill_realms[0].id")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_skill_realm_skills"
                && diagnostic.path.as_deref() == Some("skill_realms[0].skills")
        }));
    }

    #[test]
    fn rejects_realm_profile_member_binding_for_deployable_packs() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        let members = document.members.as_array_mut().expect("members");
        members[0]["profileBinding"] = json!("realm_profile");
        members[0]["realmProfile"] = json!("planner-v2");

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unsupported_realm_profile_pack_binding"
                && diagnostic.path.as_deref() == Some("members[0].profileBinding")
        }));
        assert!(export_mobpack(&json!({ "document": document })).is_err());
    }

    #[test]
    fn imports_realm_profile_binding_into_editor_member() {
        let toml = r#"
[mob]
id = "realm-profile-import"

[profiles.worker]
realm_profile = "worker-v2"

[flows.main]
description = "Realm profile import"

[flows.main.steps.work]
role = "worker"
message = "Work"

[flows.main.root.nodes.node_work]
kind = "step"
step_id = "work"
depends_on = []
depends_on_mode = "all"
"#;

        let result = import_mobpack(&json!({ "mob_toml": toml })).expect("import");
        let document: MobpackDocument =
            serde_json::from_value(result["document"].clone()).expect("document");
        let member = document
            .members
            .as_array()
            .expect("members")
            .iter()
            .find(|member| member["role"] == "worker")
            .expect("worker member");

        assert_eq!(member["profileBinding"], json!("realm_profile"));
        assert_eq!(member["realmProfile"], json!("worker-v2"));
        assert_eq!(result["validation"]["ok"], json!(false));
        assert!(
            result["validation"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == "unsupported_realm_profile_pack_binding")
        );
    }

    #[test]
    fn rejects_invalid_or_ambiguous_editor_profile_bindings() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        let members = document.members.as_array_mut().expect("members");
        members[0]["profileBinding"] = json!("catalog");
        members[1]["profileBinding"] = json!("realm_profile");

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_profile_binding"
                && diagnostic.path.as_deref() == Some("members[0].profileBinding")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_realm_profile_ref"
                && diagnostic.path.as_deref() == Some("members[1].realmProfile")
        }));
    }

    #[test]
    fn rejects_members_without_explicit_profile_binding_and_runtime_mode() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        {
            let members = document.members.as_array_mut().expect("members");
            members[0].as_object_mut().unwrap().remove("profileBinding");
            members[0].as_object_mut().unwrap().remove("runtimeMode");
        }

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_profile_binding"
                && diagnostic.path.as_deref() == Some("members[0].profileBinding")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_runtime_mode"
                && diagnostic.path.as_deref() == Some("members[0].runtimeMode")
        }));

        let members = document.members.as_array_mut().expect("members");
        members[0]["profileBinding"] = json!("inline");
        members[0]["runtimeMode"] = json!("daemon");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "invalid_runtime_mode"
                && diagnostic.path.as_deref() == Some("members[0].runtimeMode")
        }));
    }

    #[test]
    fn rejects_missing_or_duplicate_editor_member_ids() {
        let mut document = document_with_unused_reviewer_profile();
        {
            let members = document.members.as_array_mut().expect("members");
            members[0].as_object_mut().unwrap().remove("id");
            members[1]["id"] = json!("m_reviewer");
            members.push(json!({
                "id": "m_reviewer",
                "name": "reviewer_copy",
                "role": "reviewer_copy",
                "model": "gpt-5.5",
                "profileBinding": "inline",
                "runtimeMode": "turn_driven",
                "tools": [],
                "skills": []
            }));
        }

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_editor_member_id"
                && diagnostic.path.as_deref() == Some("members[0].id")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "duplicate_editor_member_id"
                && diagnostic.path.as_deref() == Some("members[2].id")
        }));
    }

    #[test]
    fn rejects_inline_member_with_realm_profile_ref() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        let members = document.members.as_array_mut().expect("members");
        members[0]["profileBinding"] = json!("inline");
        members[0]["realmProfile"] = json!("planner-v2");

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "conflicting_profile_binding"
                && diagnostic.path.as_deref() == Some("members[0].realmProfile")
        }));
    }

    #[test]
    fn validates_selected_filesystem_skill_file_exists_before_export() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing_skill_path = dir.path().join("missing.md");
        let mut document = document_with_expected_schema_ref();
        document.mob_toml = None;
        document.members.as_array_mut().unwrap()[0]["skills"] = json!(["mob.missing"]);
        document.skill_realms = json!([{
            "id": "local/filesystem",
            "label": "Local filesystem",
            "source": "filesystem",
            "skills": [{
                "id": "mob.missing",
                "label": "Missing skill",
                "source": "path",
                "path": missing_skill_path
            }]
        }]);

        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "selected_skill_file_unreadable"
                && diagnostic.path.as_deref() == Some("skill_realms.skills[mob.missing].path")
        }));
    }

    #[test]
    fn validates_imported_packed_path_skill_without_original_filesystem_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_path = dir.path().join("SKILL.md");
        std::fs::write(&skill_path, "Packed skill content.").expect("write skill");

        let mut document = document_with_expected_schema_ref();
        document.mob_toml = None;
        document.members.as_array_mut().unwrap()[0]["skills"] = json!(["mob.packed"]);
        document.skill_realms = json!([{
            "id": "local/filesystem",
            "label": "Local filesystem",
            "source": "filesystem",
            "skills": [{
                "id": "mob.packed",
                "label": "Packed skill",
                "source": "path",
                "path": skill_path
            }]
        }]);
        let exported = export_mobpack(&json!({ "document": document })).expect("export");
        std::fs::remove_file(&skill_path).expect("remove original skill");

        let imported =
            import_mobpack(&json!({ "content_base64": exported.content_base64 })).expect("import");
        let imported_document: MobpackDocument =
            serde_json::from_value(imported["document"].clone()).expect("document");
        let result = validate_document(&imported_document);

        assert!(result.ok, "{:?}", result.diagnostics);
    }

    #[test]
    fn rejects_expected_schema_ref_without_profile_output_schema() {
        let mut document = valid_document();
        document.mob_toml = Some(
            r#"
[mob]
id = "schema-missing"

[profiles.reviewer]
model = "gpt-5.5"
skills = []
peer_description = "Reviewer"

[flows.review]
description = "Review flow"

[flows.review.steps.review]
role = "reviewer"
message = "Review"
expected_schema_ref = "schemas/reviewer.json"
"#
            .to_string(),
        );
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "expected_schema_ref_missing_profile_schema"
                && diagnostic.path.as_deref()
                    == Some("flows.review.steps.review.expected_schema_ref")
        }));
    }

    #[test]
    fn rejects_expected_schema_ref_outside_archive() {
        let mut document = document_with_expected_schema_ref();
        document.flow["steps"][2]["expectedSchemaRef"] = json!("../reviewer.json");
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "invalid_expected_schema_ref" })
        );
    }

    #[test]
    fn rejects_conflicting_expected_schema_ref_exports() {
        let mut document = valid_document();
        document.mob_toml = Some(
            r#"
[mob]
id = "schema-conflict"

[profiles.alpha]
model = "gpt-5.5"
skills = []
peer_description = "Alpha"

[profiles.alpha.output_schema]
type = "object"
required = ["verdict"]

[profiles.alpha.output_schema.properties]

[profiles.alpha.output_schema.properties.verdict]
type = "string"
enum = ["green"]

[profiles.beta]
model = "gpt-5.5"
skills = []
peer_description = "Beta"

[profiles.beta.output_schema]
type = "object"
required = ["verdict"]

[profiles.beta.output_schema.properties]

[profiles.beta.output_schema.properties.verdict]
type = "string"
enum = ["red"]

[flows.review]
description = "Review flow"

[flows.review.steps.alpha]
role = "alpha"
message = "Alpha"
expected_schema_ref = "schemas/shared.json"

[flows.review.steps.beta]
role = "beta"
message = "Beta"
expected_schema_ref = "schemas/shared.json"
"#
            .to_string(),
        );
        let result = validate_document(&document);

        assert!(!result.ok);
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "expected_schema_ref_conflict"
                && diagnostic.path.as_deref() == Some("flows.review.steps.beta.expected_schema_ref")
        }));
    }

    #[test]
    fn exports_mobpack_payload_without_runtime_mutation() {
        let params = match std::env::var("MOBKIT_MOBPACK_DOCUMENT_IN") {
            Ok(path) => serde_json::from_str(
                &std::fs::read_to_string(path).expect("read editor mobpack document fixture"),
            )
            .expect("parse editor mobpack document fixture"),
            Err(_) => json!({ "document": valid_document() }),
        };
        let result = export_mobpack(&params).expect("export");
        assert_eq!(result.media_type, MOBPACK_MEDIA_TYPE);
        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        assert!(result.mob_toml.contains("[flows."));
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&result.content_base64)
            .unwrap();
        if let Ok(path) = std::env::var("MOBKIT_MOBPACK_EXPORT_OUT") {
            std::fs::write(path, &bytes).expect("write exported mobpack fixture");
        }
        let mut archive = tar::Archive::new(GzDecoder::new(bytes.as_slice()));
        let names = archive
            .entries()
            .unwrap()
            .map(|entry| entry.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(names.contains(&"manifest.toml".to_string()));
        assert!(names.contains(&"definition.json".to_string()));
        assert!(names.contains(&"mobkit/editor.json".to_string()));
    }

    #[test]
    fn imports_exported_archive_with_packed_mob_toml_source() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        let exported = export_mobpack(&json!({ "document": document })).expect("export");
        let imported = import_mobpack(&json!({
            "content_base64": exported.content_base64
        }))
        .expect("import");
        let imported_document: MobpackDocument =
            serde_json::from_value(imported["document"].clone()).expect("document");

        assert_eq!(
            imported_document.mob_toml.as_deref(),
            Some(exported.mob_toml.as_str())
        );
        assert!(imported["validation"]["ok"].as_bool().unwrap_or(false));
    }

    #[test]
    fn archive_import_prefers_packed_mob_toml_over_stale_editor_json() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        let packed_mob_toml =
            render_editor_document_mob_toml(&document).expect("render packed mob.toml");
        document.mob_toml = Some(
            r#"
[mob]
id = "stale-editor-json"

[profiles.ghost]
model = "gpt-5.5"
"#
            .to_string(),
        );
        let editor_json = serde_json::to_vec_pretty(&json!({
            "schema_version": MOBPACK_SCHEMA_VERSION,
            "media_type": MOBPACK_MEDIA_TYPE,
            "document": document,
        }))
        .expect("editor json");

        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = Builder::new(encoder);
        append_archive_file(
            &mut archive,
            "manifest.toml",
            b"surfaces = [\"cli\"]\n\n[mobpack]\nname = \"stale-editor-json\"\n",
        )
        .expect("manifest");
        append_archive_file(&mut archive, "mobkit/editor.json", &editor_json).expect("editor json");
        append_archive_file(&mut archive, "mobkit/mob.toml", packed_mob_toml.as_bytes())
            .expect("mob toml");
        let encoder = archive.into_inner().expect("archive");
        let bytes = encoder.finish().expect("gzip");

        let imported = import_mobpack(&json!({
            "content_base64": base64::engine::general_purpose::STANDARD.encode(bytes)
        }))
        .expect("import");
        let imported_document: MobpackDocument =
            serde_json::from_value(imported["document"].clone()).expect("document");

        assert_eq!(
            imported_document.mob_toml.as_deref(),
            Some(packed_mob_toml.as_str())
        );
        assert!(imported["validation"]["ok"].as_bool().unwrap_or(false));
    }

    #[test]
    fn archive_import_reprojects_stale_editor_json_from_packed_mob_toml() {
        let mut document = document_with_real_launch_modes();
        document.mob_toml = None;
        let packed_mob_toml =
            render_editor_document_mob_toml(&document).expect("render packed mob.toml");
        document.members.as_array_mut().unwrap()[0]["model"] = json!("stale-model");
        document.flow["steps"].as_array_mut().unwrap()[1]["instruction"] = json!("Stale plan");
        let editor_json = serde_json::to_vec_pretty(&json!({
            "schema_version": MOBPACK_SCHEMA_VERSION,
            "media_type": MOBPACK_MEDIA_TYPE,
            "document": document,
        }))
        .expect("editor json");

        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = Builder::new(encoder);
        append_archive_file(
            &mut archive,
            "manifest.toml",
            b"surfaces = [\"cli\"]\n\n[mobpack]\nname = \"stale-editor-projection\"\n",
        )
        .expect("manifest");
        append_archive_file(&mut archive, "mobkit/editor.json", &editor_json).expect("editor json");
        append_archive_file(&mut archive, "mobkit/mob.toml", packed_mob_toml.as_bytes())
            .expect("mob toml");
        let encoder = archive.into_inner().expect("archive");
        let bytes = encoder.finish().expect("gzip");

        let imported = import_mobpack(&json!({
            "content_base64": base64::engine::general_purpose::STANDARD.encode(bytes)
        }))
        .expect("import");
        let imported_document: MobpackDocument =
            serde_json::from_value(imported["document"].clone()).expect("document");
        let members = imported_document.members.as_array().expect("members");
        let planner = members
            .iter()
            .find(|member| member["role"] == "planner")
            .expect("planner");
        let flow_json = serde_json::to_string(&imported_document.flow).expect("flow json");

        assert_eq!(planner["model"], json!("gpt-5.5"));
        assert!(flow_json.contains("Plan"), "{flow_json}");
        assert!(!flow_json.contains("Stale plan"), "{flow_json}");
        assert_eq!(
            imported_document.mob_toml.as_deref(),
            Some(packed_mob_toml.as_str())
        );
        assert!(imported["validation"]["ok"].as_bool().unwrap_or(false));
    }

    #[test]
    fn plans_rkat_deploy_without_executing_by_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pack_path = dir.path().join("preview-proof.mobpack");
        let preview = deploy_command_preview(&json!({
            "document": valid_document(),
            "pack_path": pack_path.clone(),
            "prompt": "Reply with exactly OK."
        }))
        .expect("deploy command preview");
        let result = deploy_mobpack(&json!({
            "document": valid_document(),
            "pack_path": pack_path.clone(),
            "output_dir": dir.path(),
            "prompt": "Reply with exactly OK."
        }))
        .expect("deploy plan");
        assert!(!result.executed);
        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        assert!(std::path::Path::new(&result.pack_path).exists());
        let packed_bytes = std::fs::read(&result.pack_path).expect("read deploy pack");
        assert_eq!(result.pack_sha256, source_file_sha256(&packed_bytes));
        assert_eq!(&result.argv[0..3], ["rkat", "mob", "deploy"]);
        assert_eq!(preview.argv, result.argv);
        assert_eq!(preview.command, result.command);
        assert_eq!(preview.deploy_command, "rkat mob deploy");
        assert_eq!(preview.source, "meerkat_mobkit::mobpack::deploy_argv");
        assert!(result.command.contains("rkat mob deploy"));
        assert!(result.command.contains("Reply with exactly OK."));
        assert!(result.display_rows.iter().any(|row| {
            row.kind == "ok"
                && row.head == "MobKit mobpack validates"
                && row.meta == "rkat mob validate"
        }));
        assert!(result.display_rows.iter().any(|row| {
            row.kind == "warn"
                && row.head == "Deploy plan ready"
                && row.sub.contains("rkat mob deploy")
                && row.meta == result.pack_path
        }));
    }

    #[test]
    fn deploy_command_preview_derives_pack_filename_from_document() {
        let document = valid_document();
        let preview = deploy_command_preview(&json!({
            "document": document,
            "prompt": "Reply with exactly OK."
        }))
        .expect("deploy command preview");

        assert_eq!(&preview.argv[0..3], ["rkat", "mob", "deploy"]);
        assert!(
            preview.argv.iter().any(|arg| arg == "review-pack.mobpack"),
            "{:?}",
            preview.argv
        );
        assert!(
            preview.command.contains("review-pack.mobpack"),
            "{}",
            preview.command
        );
        assert!(
            !preview.command.contains("<pack.mobpack>"),
            "{}",
            preview.command
        );
    }

    #[test]
    fn writes_deploy_result_fixture_when_requested() {
        let params = match std::env::var("MOBKIT_MOBPACK_DEPLOY_IN") {
            Ok(path) => serde_json::from_str(
                &std::fs::read_to_string(path).expect("read editor mobpack deploy params"),
            )
            .expect("parse editor mobpack deploy params"),
            Err(_) => json!({
                "document": valid_document(),
                "prompt": "Reply with exactly OK."
            }),
        };
        let result = deploy_mobpack(&params).expect("deploy");
        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        assert_eq!(&result.argv[1..3], ["mob", "deploy"]);
        assert!(result.plan_trace.iter().any(|row| {
            row["head"]
                .as_str()
                .unwrap_or_default()
                .starts_with("MOBPACK ·")
        }));
        assert!(result.plan_trace.iter().any(|row| {
            row["head"]
                .as_str()
                .unwrap_or_default()
                .starts_with("PROFILE ·")
        }));
        assert!(result.plan_trace.iter().any(|row| {
            row["head"]
                .as_str()
                .unwrap_or_default()
                .starts_with("STEP ·")
        }));
        assert!(std::path::Path::new(&result.pack_path).exists());
        if let Ok(path) = std::env::var("MOBKIT_MOBPACK_DEPLOY_OUT") {
            std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap())
                .expect("write deploy result");
        }
    }

    #[cfg(unix)]
    #[test]
    fn executes_rkat_mob_deploy_when_requested() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fake_rkat = dir.path().join("rkat");
        let args_file = dir.path().join("rkat.args");
        std::fs::write(
            &fake_rkat,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\necho fake-rkat-ok\n",
                args_file.to_string_lossy()
            ),
        )
        .expect("write fake rkat");
        let mut permissions = std::fs::metadata(&fake_rkat)
            .expect("fake rkat metadata")
            .permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_rkat, permissions).expect("chmod fake rkat");

        let result = deploy_mobpack(&json!({
            "document": valid_document(),
            "output_dir": dir.path(),
            "prompt": "Reply with exactly OK.",
            "rkat_bin": fake_rkat,
            "execute": true
        }))
        .expect("deploy execute");

        assert!(result.executed);
        assert_eq!(result.status_code, Some(0));
        assert!(
            result
                .stdout
                .as_deref()
                .unwrap_or("")
                .contains("fake-rkat-ok")
        );
        assert_eq!(result.argv[1], "mob");
        assert_eq!(result.argv[2], "deploy");
        assert!(std::path::Path::new(&result.pack_path).exists());
        let packed_bytes = std::fs::read(&result.pack_path).expect("read deploy pack");
        assert_eq!(result.pack_sha256, source_file_sha256(&packed_bytes));

        let argv = std::fs::read_to_string(args_file).expect("recorded fake rkat args");
        assert!(argv.lines().any(|line| line == "mob"));
        assert!(argv.lines().any(|line| line == "deploy"));
        assert!(argv.lines().any(|line| line == result.pack_path));
        assert!(argv.lines().any(|line| line == "Reply with exactly OK."));
    }

    #[cfg(unix)]
    #[test]
    fn times_out_hung_rkat_mob_deploy_execution() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fake_rkat = dir.path().join("rkat");
        std::fs::write(&fake_rkat, "#!/bin/sh\nexec sleep 2\n").expect("write fake rkat");
        let mut permissions = std::fs::metadata(&fake_rkat)
            .expect("fake rkat metadata")
            .permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_rkat, permissions).expect("chmod fake rkat");

        let mut document = valid_document();
        document.deploy = json!({
            "command": "rkat mob deploy",
            "surface": "cli",
            "max_duration": "1ms",
            "prompt": "Reply with exactly OK."
        });
        let started = std::time::Instant::now();
        let result = deploy_mobpack(&json!({
            "document": document,
            "output_dir": dir.path(),
            "rkat_bin": fake_rkat,
            "execute": true
        }))
        .expect("deploy result");

        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(result.executed);
        assert!(!result.success);
        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        assert!(
            result
                .stderr
                .as_deref()
                .unwrap_or_default()
                .contains("rkat mob deploy timed out"),
            "{result:?}"
        );
        assert!(result.display_rows.iter().any(|row| {
            row.kind == "crit"
                && row.head == "rkat mob deploy failed"
                && row.sub.contains("rkat mob deploy")
        }));
        assert!(result.display_rows.iter().any(|row| {
            row.kind == "warn" && row.head == "rkat output" && row.sub.contains("timed out")
        }));
    }

    #[cfg(unix)]
    #[test]
    fn reports_failed_rkat_mob_deploy_execution() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fake_rkat = dir.path().join("rkat");
        std::fs::write(&fake_rkat, "#!/bin/sh\necho deploy-failed >&2\nexit 7\n")
            .expect("write fake rkat");
        let mut permissions = std::fs::metadata(&fake_rkat)
            .expect("fake rkat metadata")
            .permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_rkat, permissions).expect("chmod fake rkat");

        let result = deploy_mobpack(&json!({
            "document": valid_document(),
            "output_dir": dir.path(),
            "prompt": "Reply with exactly OK.",
            "rkat_bin": fake_rkat,
            "execute": true
        }))
        .expect("deploy execute");

        assert!(result.executed);
        assert!(!result.success);
        assert_eq!(result.status_code, Some(7));
        assert!(
            result
                .stderr
                .as_deref()
                .unwrap_or("")
                .contains("deploy-failed")
        );
        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
    }

    #[test]
    fn reports_missing_rkat_mob_deploy_as_deploy_result() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing_rkat = dir.path().join("missing-rkat");

        let result = deploy_mobpack(&json!({
            "document": valid_document(),
            "output_dir": dir.path(),
            "prompt": "Reply with exactly OK.",
            "rkat_bin": missing_rkat,
            "execute": true
        }))
        .expect("deploy result");

        assert!(result.executed);
        assert!(!result.success);
        assert_eq!(result.status_code, None);
        assert!(result.validation.ok, "{:?}", result.validation.diagnostics);
        assert!(std::path::Path::new(&result.pack_path).exists());
        assert!(
            result
                .stderr
                .as_deref()
                .unwrap_or_default()
                .contains("failed to run rkat mob deploy")
        );
        assert!(result.plan_trace.iter().any(|row| {
            row["head"]
                .as_str()
                .unwrap_or_default()
                .starts_with("MOBPACK ·")
        }));
        assert!(result.display_rows.iter().any(|row| {
            row.kind == "crit"
                && row.head == "rkat mob deploy failed"
                && row.sub.contains("rkat mob deploy")
        }));
        assert!(result.display_rows.iter().any(|row| {
            row.kind == "warn"
                && row.head == "rkat output"
                && row.sub.contains("failed to run rkat mob deploy")
        }));
    }

    #[test]
    fn imports_mobpack_payload_without_runtime_mutation() {
        let params = match std::env::var("MOBKIT_MOBPACK_IMPORT_IN") {
            Ok(path) => serde_json::from_str(
                &std::fs::read_to_string(path).expect("read editor mobpack import params"),
            )
            .expect("parse editor mobpack import params"),
            Err(_) => json!({ "mob_toml": importable_mob_toml() }),
        };
        let result = import_mobpack(&params).expect("import");
        assert_eq!(result["validation"]["ok"], true);
        assert!(!result["document"]["members"].as_array().unwrap().is_empty());
        if let Ok(path) = std::env::var("MOBKIT_MOBPACK_IMPORT_OUT") {
            std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap())
                .expect("write imported mobpack result");
        }
    }

    #[test]
    fn catalogs_response_exposes_filesystem_skill_catalog() {
        let schema = mobpack_schema_response();
        let catalogs = mobpack_catalogs_response();
        assert!(
            schema.get("starter_skills").is_none(),
            "starter skills must be exposed through skill_realms, not a duplicate fallback catalog"
        );
        assert!(
            schema.get("skill_realms").is_none(),
            "dynamic skill realms must be loaded through mobkit/mobpacks/catalogs"
        );
        let realms = catalogs["skill_realms"].as_array().expect("skill realms");
        let mobkit_platform = realms
            .iter()
            .find(|realm| realm["source"] == "filesystem")
            .and_then(|realm| realm["skills"].as_array())
            .and_then(|skills| skills.iter().find(|skill| skill["id"] == "mobkit-platform"))
            .expect("mobkit-platform filesystem skill");
        assert_eq!(mobkit_platform["source"], "path");
        assert_eq!(mobkit_platform["origin"], "filesystem");
        assert!(
            mobkit_platform["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("SKILL.md"))
        );
        assert!(
            realms.iter().all(|realm| realm["source"] != "starter"),
            "skill realms must come from filesystem or real sample mobpacks, not starter mocks"
        );
        let sample_realm = realms
            .iter()
            .find(|realm| realm["id"] == "mobkit/sample-mobpacks")
            .expect("sample mobpack skill realm");
        assert_eq!(sample_realm["source"], "mobkit/sample-mobpack");
        assert_eq!(
            sample_realm["sourceDocumentPath"],
            "sample_mobpacks[].document.skill_realms[]"
        );
        for skill_id in ["mob.workpad", "mob.review"] {
            let skill = sample_realm["skills"]
                .as_array()
                .expect("sample realm skills")
                .iter()
                .find(|skill| skill["id"] == skill_id)
                .unwrap_or_else(|| panic!("missing {skill_id} sample skill"));
            assert_eq!(skill["source"], "inline");
            assert_eq!(skill["origin"], "mobkit/sample-mobpack");
            assert!(
                skill["content"]
                    .as_str()
                    .is_some_and(|content| !content.trim().is_empty())
            );
            assert!(
                skill["sourceMobpack"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("sample_"))
            );
        }
    }

    #[test]
    fn sample_skill_realm_requires_real_sample_source_metadata() {
        let samples = json!([
            {
                "id": "missing_source",
                "document": {
                    "skill_realms": [{
                        "id": "realm",
                        "skills": [{ "id": "mob.ghost", "source": "inline", "content": "ghost" }]
                    }]
                }
            },
            {
                "source": "mobkit/sample-mobpack",
                "document": {
                    "skill_realms": [{
                        "id": "realm",
                        "skills": [{ "id": "mob.no_id", "source": "inline", "content": "ghost" }]
                    }]
                }
            },
            {
                "id": "valid_sample",
                "source": "mobkit/sample-mobpack",
                "name": "Valid Sample",
                "document": {
                    "skill_realms": [{
                        "id": "realm",
                        "skills": [{ "id": "mob.real", "source": "inline", "content": "real" }]
                    }]
                }
            }
        ]);

        let realm = sample_skill_realm(&samples, true).expect("valid sample realm");
        assert_eq!(realm["source"], "mobkit/sample-mobpack");
        let skills = realm["skills"].as_array().expect("skills");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0]["id"], "mob.real");
        assert_eq!(skills[0]["origin"], "mobkit/sample-mobpack");
        assert_eq!(skills[0]["sourceMobpack"], "valid_sample");
        assert_eq!(skills[0]["sourceMobpackName"], "Valid Sample");
    }

    #[test]
    fn sample_agent_definitions_require_real_sample_source_metadata() {
        let samples = json!([
            {
                "id": "missing_source",
                "document": {
                    "members": [{
                        "role": "ghost",
                        "name": "Ghost",
                        "profileBinding": "inline",
                        "runtimeMode": "turn_driven",
                        "tools": ["builtins"],
                        "skills": []
                    }]
                }
            },
            {
                "source": "mobkit/sample-mobpack",
                "document": {
                    "members": [{
                        "role": "no_id",
                        "name": "No ID",
                        "profileBinding": "inline",
                        "runtimeMode": "turn_driven",
                        "tools": ["builtins"],
                        "skills": []
                    }]
                }
            },
            {
                "id": "model_less_sample",
                "source": "mobkit/sample-mobpack",
                "document": {
                    "members": [{
                        "role": "model_less",
                        "name": "Model Less",
                        "profileBinding": "inline",
                        "runtimeMode": "turn_driven",
                        "tools": ["builtins"],
                        "skills": []
                    }]
                }
            },
            {
                "id": "valid_sample",
                "name": "Valid Sample",
                "source": "mobkit/sample-mobpack",
                "document": {
                    "members": [{
                        "role": "real",
                        "name": "Real",
                        "model": "gpt-5.5",
                        "profileBinding": "inline",
                        "runtimeMode": "turn_driven",
                        "tools": ["builtins"],
                        "skills": ["mob.real"],
                        "systemPrompt": "Be real."
                    }],
                    "skill_realms": [{
                        "id": "sample",
                        "label": "Sample skills",
                        "source": "mobkit/sample-mobpack",
                        "skills": [{
                            "id": "mob.real",
                            "label": "Real skill",
                            "source": "inline",
                            "content": "real skill"
                        }]
                    }]
                }
            }
        ]);

        let skill_realms = sample_skill_realm(&samples, true).expect("sample skill realm");
        let definitions =
            agent_definition_catalog(&samples, &tool_catalog_response(), &json!([skill_realms]));
        let definitions = definitions.as_array().expect("agent definitions");
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0]["role"], "real");
        assert_eq!(definitions[0]["definitionType"], "mobkit/profile-member");
        assert_eq!(definitions[0]["source"], "mobkit/mobpack-profile-member");
        assert_eq!(definitions[0]["sourceMobpack"], "valid_sample");
        assert_eq!(definitions[0]["sourceMobpackName"], "Valid Sample");
        assert_eq!(definitions[0]["sourceOrigin"], "mobkit/sample-mobpack");
        assert_eq!(definitions[0]["tools"], json!(["builtins"]));
        assert_eq!(definitions[0]["toolDefinitions"][0]["id"], "builtins");
        assert_eq!(
            definitions[0]["toolDefinitions"][0]["source"],
            "meerkat_mob::ToolConfig"
        );
        assert_eq!(definitions[0]["skills"], json!(["mob.real"]));
        assert_eq!(definitions[0]["skillDefinitions"][0]["id"], "mob.real");
        assert_eq!(
            definitions[0]["skillDefinitions"][0]["origin"],
            "mobkit/sample-mobpack"
        );
        assert_eq!(
            definitions[0]["skillDefinitions"][0]["sourceMobpack"],
            "valid_sample"
        );
    }

    #[test]
    fn schema_response_exposes_rkat_deploy_defaults_for_editor_hydration() {
        let schema = mobpack_schema_response();
        let deploy = &schema["deploy_settings"];

        assert_eq!(deploy["command"], "rkat mob deploy");
        assert_eq!(deploy["defaults"]["command"], "rkat mob deploy");
        assert_eq!(deploy["defaults"]["surface"], "cli");
        assert_eq!(deploy["defaults"]["trust_policy"], "permissive");
        assert_eq!(deploy["defaults"]["realm_backend"], "jsonl");
        assert_eq!(deploy["defaults"]["isolated"], true);
        assert_eq!(deploy["defaults"]["prompt"], "Reply with exactly OK.");
        assert!(
            !deploy["options"]
                .as_array()
                .expect("deploy options")
                .contains(&json!("rkat_bin"))
        );

        let mob_defaults = &schema["mob_definition"]["mob_settings"]["defaults"];
        assert_eq!(mob_defaults["backendDefault"], "session");
        assert_eq!(mob_defaults["autoWireOrchestrator"], false);
        assert_eq!(mob_defaults["roleWiring"], json!([]));
        assert_eq!(mob_defaults["advanced"]["topology"], Value::Null);
    }

    #[test]
    fn schema_response_commands_match_mobpack_authoring_methods() {
        let schema = mobpack_schema_response();
        let commands = schema["commands"]
            .as_object()
            .expect("schema commands object");
        let expected = BTreeMap::from([
            ("schema", "mobkit/mobpacks/schema"),
            ("catalogs", "mobkit/mobpacks/catalogs"),
            ("validate", "mobkit/mobpacks/validate"),
            ("export", "mobkit/mobpacks/export"),
            ("import", "mobkit/mobpacks/import"),
            ("deploy_command", "mobkit/mobpacks/deploy_command"),
            ("deploy_rpc", "mobkit/mobpacks/deploy"),
            ("deploy_cli", "rkat mob deploy <pack.mobpack> <prompt>"),
        ]);

        assert_eq!(
            commands
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|value| (key.as_str(), value)))
                .collect::<BTreeMap<_, _>>(),
            expected
        );

        let rpc_methods = commands
            .iter()
            .filter(|(key, _)| key.as_str() != "deploy_cli")
            .filter_map(|(_, value)| value.as_str())
            .collect::<BTreeSet<_>>();
        let authoring_methods = crate::rpc::MOBPACK_AUTHORING_METHODS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(rpc_methods, authoring_methods);
    }

    #[test]
    fn catalogs_response_exposes_real_tool_catalog_without_schema_aliases() {
        let schema = mobpack_schema_response();
        let catalogs = mobpack_catalogs_response();
        assert!(
            schema.get("tool_config").is_none(),
            "schema must not expose compatibility tool_config aliases"
        );
        assert!(
            schema.get("tool_catalog").is_none(),
            "dynamic tool catalogs must be loaded through mobkit/mobpacks/catalogs"
        );
        assert!(
            schema.get("agent_definitions").is_none(),
            "dynamic agent definitions must be loaded through mobkit/mobpacks/catalogs"
        );
        let tool_catalog = catalogs["tool_catalog"].as_array().expect("tool_catalog");

        let tool_config_fields = serde_json::to_value(ToolConfig::default())
            .expect("ToolConfig serializes")
            .as_object()
            .expect("ToolConfig object")
            .iter()
            .filter(|(_, value)| value.is_boolean())
            .map(|(field, _)| field.to_string())
            .collect::<BTreeSet<_>>();
        let catalog_fields = tool_catalog
            .iter()
            .filter(|tool| tool["kind"] == "runtime")
            .map(|tool| {
                assert_eq!(tool["source"], "meerkat_mob::ToolConfig");
                let id = tool["id"].as_str().expect("tool id");
                assert_eq!(tool["field"], id);
                id.to_string()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(catalog_fields, tool_config_fields);
        assert!(tool_catalog.iter().any(|tool| {
            tool["id"] == "builtins" && tool["kind"] == "runtime" && tool["field"] == "builtins"
        }));
        assert!(tool_catalog.iter().any(|tool| {
            tool["id"] == "shell" && tool["kind"] == "runtime" && tool["field"] == "shell"
        }));
        assert!(tool_catalog.iter().any(|tool| {
            tool["id"] == "comms" && tool["kind"] == "runtime" && tool["field"] == "comms"
        }));
        assert!(tool_catalog.iter().any(|tool| {
            tool["id"] == "mob" && tool["kind"] == "runtime" && tool["field"] == "mob"
        }));
        assert!(tool_catalog.iter().all(|tool| {
            tool["desc"].as_str().is_some_and(|desc| !desc.is_empty())
                && tool["label"]
                    .as_str()
                    .is_some_and(|label| !label.is_empty())
        }));
    }

    #[test]
    fn schema_response_exposes_flow_control_contract_for_editor_hydration() {
        let schema = mobpack_schema_response();
        let mob_definition = &schema["mob_definition"];
        let defaults = &mob_definition["defaults"];

        assert_eq!(defaults["runtime_mode"], json!("turn_driven"));
        assert_eq!(defaults["launch_mode"], json!("fresh"));
        assert_eq!(defaults["fork_context"], json!("full_history"));
        assert_eq!(defaults["budget_split_policy"], json!("equal"));
        assert_eq!(defaults["dispatch_mode"], json!("fan_out"));
        assert_eq!(defaults["collection_policy"], json!("all"));
        assert_eq!(defaults["dependency_mode"], json!("all"));
        assert_eq!(defaults["condition_operator"], json!("=="));
        assert_eq!(defaults["schema_field_type"], json!("string"));
        assert_eq!(defaults["branch_param_type"], json!("enum"));
        assert_eq!(defaults["repeat_iteration_input"], json!("carry"));
        assert_eq!(defaults["step_output_format"], json!("json"));
        assert_eq!(defaults["graph_gate_kind"], json!("branch"));
        assert_eq!(defaults["graph_edge_kind"], json!("next"));
        assert_eq!(defaults["graph_condition_edge_kind"], json!("cond"));
        assert_eq!(defaults["graph_fanout_edge_kind"], json!("fanout"));
        assert_eq!(defaults["graph_terminal_kind"], json!("success"));
        assert_eq!(
            mob_definition["runtime_modes"],
            json!(runtime_mode_values())
        );
        assert_eq!(
            mob_definition["runtime_mode_labels"]["turn_driven"],
            json!("turn_driven — explicit turn dispatch")
        );
        assert_eq!(
            mob_definition["runtime_mode_labels"]["autonomous_host"],
            json!("autonomous_host — RPC keep-alive member loop")
        );
        assert_eq!(
            mob_definition["deploy_runtime_mode_compatibility"]["cli"]["allowed"],
            json!(["turn_driven"])
        );
        assert_eq!(
            mob_definition["deploy_runtime_mode_compatibility"]["cli"]["blocked"]["autonomous_host"],
            json!("RPC surface only; rkat mob deploy requires turn_driven profiles.")
        );
        assert_eq!(
            mob_definition["deploy_runtime_mode_compatibility"]["rpc"]["allowed"],
            json!(runtime_mode_values())
        );
        assert_eq!(
            mob_definition["launch_modes"],
            json!(member_launch_mode_values())
        );
        assert_eq!(
            mob_definition["dispatch_modes"],
            json!(dispatch_mode_values())
        );
        assert_eq!(
            mob_definition["dispatch_mode_labels"]["fan_out"],
            json!("fan_out — broadcast to every lane")
        );
        assert_eq!(
            mob_definition["collection_policy_labels"]["quorum"],
            json!("quorum — require N branches")
        );
        assert_eq!(
            mob_definition["dependency_mode_labels"]["any"],
            json!("any — any upstream node")
        );
        assert_eq!(
            mob_definition["option_unsupported_label_separator"],
            json!(" — not in MobKit ")
        );
        assert_eq!(
            mob_definition["option_unsupported_reason_prefix"],
            json!("Unsupported by the MobKit ")
        );
        assert_eq!(
            mob_definition["option_unsupported_reason_suffix"],
            json!(" contract.")
        );
        assert_eq!(
            mob_definition["profile_backends"],
            json!(["session", "external"])
        );
        assert_eq!(mob_definition["profile_binding"], json!(["inline"]));
        assert_eq!(
            mob_definition["profile_binding_restrictions"]["inline"]["deployable"],
            json!(true)
        );
        assert_eq!(
            mob_definition["profile_binding_restrictions"]["realm_profile"]["deployable"],
            json!(false)
        );
        assert!(
            mob_definition["profile_binding_restrictions"]["realm_profile"]["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("rkat mob validate"))
        );
        let catalogs = mobpack_catalogs_response();
        let blank_mobpack = &catalogs["blank_mobpack"];
        assert_eq!(blank_mobpack["id"], json!("blank"));
        assert_eq!(blank_mobpack["source"], json!("mobkit/blank-mobpack"));
        assert_eq!(blank_mobpack["validation"]["ok"], json!(true));
        assert!(
            blank_mobpack["document"]["members"]
                .as_array()
                .is_some_and(|members| !members.is_empty())
        );
        assert!(
            blank_mobpack["document"]["flow"]["steps"]
                .as_array()
                .is_some_and(|steps| steps.iter().any(|step| step["type"] == "member"))
        );
        assert_eq!(
            mob_definition["fork_contexts"],
            json!(fork_context_values())
        );
        assert_eq!(
            mob_definition["budget_split_policies"],
            json!(budget_split_policy_values())
        );
        assert_eq!(
            mob_definition["editor_schema_field_types"],
            json!(editor_schema_field_type_values())
        );
        assert_eq!(
            mob_definition["editor_input_param_draft"]["document_path"],
            json!("document.flow.steps[type=input].inputParams")
        );
        assert_eq!(
            mob_definition["editor_input_step_draft"]["document_path"],
            json!("document.flow.steps[type=input]")
        );
        assert_eq!(
            mob_definition["editor_input_step_draft"]["default_step"]["id"],
            json!("input")
        );
        assert_eq!(
            mob_definition["editor_input_step_draft"]["default_step"]["task"],
            json!("Run the mobpack flow.")
        );
        assert_eq!(
            mob_definition["editor_input_param_draft"]["archive_path"],
            json!("schemas/main-input.json")
        );
        assert_eq!(
            mob_definition["editor_input_param_draft"]["added_field"]["name"],
            json!("param")
        );
        assert_eq!(
            mob_definition["editor_input_param_draft"]["added_field"]["required"],
            json!(true)
        );
        assert_eq!(
            mob_definition["editor_schema_draft"]["schema_id_prefix"],
            json!("Artifact")
        );
        assert_eq!(
            mob_definition["editor_schema_draft"]["document_path"],
            json!("document.schemas[]")
        );
        assert_eq!(
            mob_definition["editor_schema_draft"]["archive_path"],
            json!("schemas/<schema-id>.json")
        );
        assert_eq!(
            mob_definition["editor_schema_draft"]["initial_field"]["name"],
            json!("field_one")
        );
        assert_eq!(
            mob_definition["editor_schema_draft"]["initial_field"]["required"],
            json!(true)
        );
        assert_eq!(
            mob_definition["editor_schema_draft"]["added_field"]["name"],
            json!("new_field")
        );
        assert_eq!(
            mob_definition["editor_schema_draft"]["added_field"]["required"],
            json!(false)
        );
        assert_eq!(
            mob_definition["condition_operators"],
            json!(["==", ">", "<"])
        );
        assert_eq!(
            mob_definition["step_output_formats"],
            json!(step_output_format_values())
        );
        assert_eq!(
            mob_definition["collection_policies"],
            json!(collection_policy_values())
        );
        assert_eq!(
            mob_definition["dependency_modes"],
            json!(dependency_mode_values())
        );
        assert_eq!(
            mob_definition["editor_flow_step_types"],
            json!(["input", "member", "repeat", "branch", "parallel"])
        );
        assert_eq!(mob_definition["repeat_iteration_inputs"], json!(["carry"]));
        assert_eq!(
            mob_definition["graph_gate_kinds"],
            json!(["branch", "fork", "join"])
        );
        assert_eq!(
            mob_definition["graph_palette_gate_kinds"],
            json!(["branch", "fork"])
        );
        assert_eq!(
            mob_definition["graph_terminal_kinds"],
            json!(["success", "failed", "human"])
        );
        assert_eq!(
            mob_definition["graph_frame_kinds"],
            json!(["Branch", "Parallel", "RepeatUntil"])
        );
        assert_eq!(
            mob_definition["graph_edge_kinds"],
            json!(["next", "cond", "fanout"])
        );
        assert_eq!(
            mob_definition["editor_graph_draft"]["branch_gate_label"],
            json!("branch")
        );
        assert_eq!(
            mob_definition["editor_graph_draft"]["fallback_edge_label"],
            json!("fallback")
        );
        assert_eq!(
            mob_definition["editor_graph_draft"]["parallel_lane_labels"],
            json!(["lane 1", "lane 2"])
        );
        assert_eq!(
            mob_definition["editor_graph_draft"]["parallel_missing_dispatch_label"],
            json!("missing dispatch")
        );
        assert_eq!(
            mob_definition["editor_graph_draft"]["join_quorum_label_prefix"],
            json!("barrier · ")
        );
        assert_eq!(
            mob_definition["editor_graph_draft"]["repeat_missing_max_iterations_label"],
            json!("missing max_iterations")
        );
        assert_eq!(
            mob_definition["editor_source_view"]["drawer_eyebrow"],
            json!("SOURCE · mob.toml")
        );
        assert_eq!(
            mob_definition["editor_source_view"]["inline_title"],
            json!("mob.toml")
        );
        assert_eq!(
            mob_definition["editor_source_view"]["loading_text"],
            json!("rendering mob.toml from mobkit/mobpacks/export...")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["fit_title"],
            json!("Fit to view")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["port_drag_title"],
            json!("Drag to a member to connect")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["add_node_search_placeholder"],
            json!("Add a node…")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["gate_palette_rows"][1]["id"],
            json!("fork")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["graph_gate_kind_labels"]["join"],
            json!("join — wait for branches")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["graph_edge_kind_labels"]["cond"],
            json!("cond — guarded branch")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["branch_input_param_source_label"],
            json!("Input params")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["source_file_label"],
            json!("mob.toml")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["source_file_aria_label"],
            json!("Open mob.toml read-only source editor")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["source_file_glyph"],
            json!("{ }")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["source_file_role_label"],
            json!("source file")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["gate_dispatch_hint"],
            json!("Exports as the MobKit parallel flow dispatch mode.")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["edge_terminal_member_value"],
            json!("(terminal)")
        );
        assert_eq!(
            mob_definition["editor_condition_view"]["text_value_placeholder"],
            json!("value")
        );
        assert_eq!(
            mob_definition["editor_condition_view"]["empty_value_label"],
            json!("—")
        );
        assert_eq!(
            mob_definition["editor_error_view"]["deploy_plan_failed_head"],
            json!("Deploy plan failed")
        );
        assert_eq!(
            mob_definition["editor_error_view"]["missing_editor_flow_meta"],
            json!("missing_editor_flow")
        );
        assert_eq!(
            mob_definition["editor_agent_view"]["agents_heading"],
            json!("AGENTS")
        );
        assert_eq!(
            mob_definition["editor_agent_view"]["schemas_heading"],
            json!("SCHEMAS")
        );
        assert_eq!(
            mob_definition["editor_agent_view"]["add_schema_label"],
            json!("+ new schema")
        );
        assert_eq!(
            mob_definition["editor_agent_view"]["empty_title"],
            json!("AGENT LIBRARY")
        );
        assert_eq!(
            mob_definition["editor_agent_view"]["empty_lines"][0],
            json!("Select an agent or schema on the left.")
        );
        assert_eq!(
            mob_definition["editor_agent_view"]["missing_agent_label"],
            json!("Agent not found.")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["identity_title"],
            json!("IDENTITY")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["missing_profile_binding_label"],
            json!("missing profile binding")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["backend_definition_default_label"],
            json!("definition default")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["provider_params_label"],
            json!("Provider params")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["provider_params_object_required_error"],
            json!("provider_params must be a JSON object")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["system_prompt_title"],
            json!("SYSTEM PROMPT")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["empty_schema_hint"],
            json!("No structured output. Agent returns free-form text.")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["source_title"],
            json!("SOURCE")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["source_mobpack_label"],
            json!("Mobpack")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["source_tools_label"],
            json!("Tool refs")
        );
        assert_eq!(
            mob_definition["editor_agent_detail_view"]["source_skills_label"],
            json!("Skill refs")
        );
        assert_eq!(
            mob_definition["editor_agent_access_view"]["tool_title"],
            json!("TOOL ACCESS")
        );
        assert_eq!(
            mob_definition["editor_agent_access_view"]["tool_invalid_error"],
            json!("Use a MobKit-listed runtime tool or configured MCP/Rust source.")
        );
        assert_eq!(
            mob_definition["editor_agent_access_view"]["inline_skill_realm_id"],
            json!("mobkit/editor-inline")
        );
        assert_eq!(
            mob_definition["editor_agent_access_view"]["skill_inline_add_label"],
            json!("ADD SKILL")
        );
        assert_eq!(
            mob_definition["editor_agent_access_view"]["skill_inline_missing_label_error"],
            json!("Inline skill id or label is required.")
        );
        assert_eq!(
            mob_definition["editor_agent_access_view"]["skill_inline_missing_content_error"],
            json!("Inline skill content is required.")
        );
        assert_eq!(
            mob_definition["editor_agent_access_view"]["skill_inline_invalid_id_error"],
            json!("Inline skill id or label must contain letters or numbers.")
        );
        assert_eq!(
            mob_definition["editor_new_flow_view"]["eyebrow_template"],
            json!("NEW FLOW · STEP {step} OF 2")
        );
        assert_eq!(
            mob_definition["editor_new_flow_view"]["name_placeholder"],
            json!("docs-only")
        );
        assert_eq!(
            mob_definition["editor_new_flow_view"]["create_label"],
            json!("CREATE")
        );
        assert_eq!(
            mob_definition["editor_flow_registry_view"]["eyebrow"],
            json!("FLOWS")
        );
        assert_eq!(
            mob_definition["editor_flow_registry_view"]["create_label"],
            json!("+ NEW FLOW")
        );
        assert_eq!(
            mob_definition["editor_flow_registry_view"]["columns"][3]["label"],
            json!("STAGE")
        );
        assert_eq!(
            mob_definition["editor_deploy_view"]["brand_label"],
            json!("MobKit · Flow Editor")
        );
        assert_eq!(
            mob_definition["editor_deploy_view"]["deploy_label"],
            json!("DEPLOY")
        );
        assert_eq!(
            mob_definition["editor_deploy_view"]["validation_eyebrow"],
            json!("VALIDATE · MobKit")
        );
        assert_eq!(
            mob_definition["editor_deploy_view"]["plan_unavailable_body"],
            json!("mobkit/mobpacks/deploy did not return plan_trace.")
        );
        assert_eq!(
            mob_definition["editor_settings_view"]["panel_title"],
            json!("Tweaks")
        );
        assert_eq!(
            mob_definition["editor_settings_view"]["panel_close_label"],
            json!("Close tweaks")
        );
        assert_eq!(
            mob_definition["editor_settings_view"]["duration_placeholder"],
            json!("30s")
        );
        assert_eq!(
            mob_definition["editor_settings_view"]["tool_calls_max"],
            json!(999)
        );
        assert_eq!(
            mob_definition["editor_settings_view"]["inspector_layout_options"][0]["label"],
            json!("Right")
        );
        assert_eq!(
            mob_definition["editor_settings_view"]["role_wiring_add_label"],
            json!("+ rule")
        );
        assert_eq!(
            mob_definition["editor_settings_view"]["advanced_object_required_error"],
            json!("object required")
        );
        assert_eq!(
            mob_definition["editor_launch_view"]["launch_title"],
            json!("Launch mode")
        );
        assert_eq!(
            mob_definition["editor_launch_view"]["launch_mode_labels"]["Fork"],
            json!("Fork — copy context from another step")
        );
        assert_eq!(
            mob_definition["editor_launch_view"]["fixed_budget_default_value"],
            json!(4096)
        );
        assert_eq!(
            mob_definition["editor_agent_view"]["add_agent_placeholder_label"],
            json!("+ new agent...")
        );
        assert_eq!(
            mob_definition["editor_agent_view"]["add_agent_unavailable_label"],
            json!("agents unavailable")
        );
        assert_eq!(
            mob_definition["editor_agent_view"]["add_agent_error_prefix"],
            json!("Agent definition unavailable: ")
        );
        assert_eq!(
            mob_definition["editor_schema_view"]["eyebrow"],
            json!("OUTPUT SCHEMA")
        );
        assert_eq!(
            mob_definition["editor_schema_view"]["fields_title_prefix"],
            json!("FIELDS")
        );
        assert_eq!(
            mob_definition["editor_schema_view"]["header_labels"]["required"],
            json!("REQ")
        );
        assert_eq!(
            mob_definition["editor_schema_view"]["delete_blocked_title"],
            json!("Unassign from agents first")
        );
        assert_eq!(
            mob_definition["editor_schema_view"]["field_name_placeholder"],
            json!("field_name")
        );
        assert_eq!(
            mob_definition["editor_schema_view"]["field_enum_add_value"],
            json!("value")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["start_label"],
            json!("START")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["loop_badge"],
            json!("LOOP")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["tips_title"],
            json!("Tips")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["empty_panel_subtitle_parts"][3]["text"],
            json!("mob.toml")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["source_toggle_label"],
            json!("{ } mob.toml")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["member_step_instruction_placeholder"],
            json!("e.g. Run the focused tests and report failures.")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["tool_scope_add_profile_placeholder"],
            json!("+ add profile tool...")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["input_panel_title"],
            json!("Input")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["input_param_header_labels"]["required"],
            json!("REQ")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["input_param_name_placeholder"],
            json!("param_name")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["input_param_enum_add_value"],
            json!("value")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["input_empty_params_parts"][1]["text"],
            json!("params.*")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["input_tips"][0],
            json!("Run with: rkat mob deploy <pack> \"<task>\" — or run_flow(input).")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["branch_condition_empty_hint"],
            json!("Add an upstream member with an output schema before configuring this branch.")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["parallel_collection_label"],
            json!("Collection policy (fan_in)")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["repeat_panel_title"],
            json!("Repeat until")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["repeat_canvas_loop_back_prefix"],
            json!("↑ loop back · ")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["picker_search_placeholder"],
            json!("Search members & primitives…")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["parallel_step_card_desc_prefix"],
            json!("fan-out → join · ")
        );
        assert_eq!(
            mob_definition["editor_basic_view"]["flow_primitive_rows"][2]["id"],
            json!("parallel")
        );
        assert_eq!(
            mob_definition["editor_graph_template_view"]["template_eyebrow"],
            json!("TEMPLATE")
        );
        assert_eq!(
            mob_definition["editor_graph_template_view"]["quick_start_title"],
            json!("QUICK START")
        );
        assert_eq!(
            mob_definition["editor_graph_template_view"]["summary_members_label"],
            json!("members")
        );
        assert_eq!(
            mob_definition["editor_graph_template_view"]["summary_frames_label"],
            json!("frames")
        );
        assert_eq!(
            mob_definition["editor_graph_template_view"]["summary_members_value_template"],
            json!("{placed} placed / {total} in library")
        );
        assert_eq!(
            mob_definition["editor_graph_template_view"]["quick_start_rows"][0][1]["text"],
            json!("library member")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["instance_eyebrow"],
            json!("INSTANCE")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["instance_id_line_template"],
            json!("{id} · cell ({col},{row})")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["instance_output_title_template"],
            json!("MEMBER OUTPUT · {schema}")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["gate_eyebrow_template"],
            json!("GATE · {kind}")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["gate_quorum_incoming_template"],
            json!("of {count} incoming")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["gate_member_option_template"],
            json!("{name} · {role}")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["terminal_eyebrow_template"],
            json!("TERMINAL · {kind}")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["edge_eyebrow_template"],
            json!("EDGE · {kind}")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["edge_title_template"],
            json!("{from} → {to}")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["edge_id_line_template"],
            json!("{id}")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["edge_field_placeholder"],
            json!("— field —")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["edge_field_no_schema_placeholder"],
            json!("(no schema)")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["graph_condition_target_missing_label"],
            json!("?")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["graph_condition_owner_option_template"],
            json!("{name}")
        );
        assert_eq!(
            mob_definition["editor_graph_view"]["graph_condition_field_option_template"],
            json!("{name} · {type}")
        );
    }

    #[test]
    fn schema_response_exposes_packed_skill_archive_contract() {
        let schema = mobpack_schema_response();
        let files = schema["files"].as_array().expect("archive files");

        assert!(files.contains(&json!("skills/*.md")));
        assert_eq!(
            schema["mob_definition"]["skill_source_document_path"],
            "document.skill_realms[].skills[]"
        );
        assert_eq!(
            schema["mob_definition"]["path_skill_archive_path"],
            "skills/<skill-id>.md or a safe relative skill path"
        );
    }

    #[test]
    fn writes_schema_response_fixture_when_requested() {
        let schema = mobpack_schema_response();
        let catalogs = mobpack_catalogs_response();
        assert_eq!(schema["schema_version"], MOBPACK_SCHEMA_VERSION);
        assert!(
            schema.get("sample_mobpacks").is_none(),
            "dynamic sample mobpacks must be loaded through mobkit/mobpacks/catalogs"
        );
        assert!(
            catalogs["sample_mobpacks"]
                .as_array()
                .is_some_and(|samples| {
                    samples.iter().all(|sample| {
                        sample["document"]["mob_toml"]
                            .as_str()
                            .is_some_and(|text| text.contains("[mob]"))
                    })
                })
        );
        let invalid_samples = catalogs["sample_mobpacks"]
            .as_array()
            .expect("sample mobpacks")
            .iter()
            .filter(|sample| sample["validation"]["ok"] != true)
            .map(|sample| {
                json!({
                    "name": sample["name"].clone(),
                    "diagnostics": sample["validation"]["diagnostics"].clone(),
                })
            })
            .collect::<Vec<_>>();
        assert!(invalid_samples.is_empty(), "{invalid_samples:#?}");
        assert!(
            schema.get("profile_templates").is_none(),
            "authoring schema must not expose profile-template aliases"
        );
        assert!(
            schema.get("agent_definitions").is_none(),
            "dynamic agent definitions must be loaded through mobkit/mobpacks/catalogs"
        );
        let agent_definitions = catalogs["agent_definitions"]
            .as_array()
            .expect("agent definitions");
        assert!(agent_definitions.iter().any(|definition| {
            definition["role"] == "router"
                && definition["tools"]
                    .as_array()
                    .is_some_and(|tools| tools.contains(&json!("mob")))
        }));
        let sample_members = catalogs["sample_mobpacks"]
            .as_array()
            .expect("sample mobpacks")
            .iter()
            .flat_map(|sample| {
                let source_mobpack = sample["id"].as_str().unwrap_or_default().to_string();
                sample["document"]["members"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(move |member| {
                        let role = member["role"]
                            .as_str()
                            .or_else(|| member["name"].as_str())?;
                        Some(((source_mobpack.clone(), role.to_string()), member.clone()))
                    })
            })
            .collect::<BTreeMap<_, _>>();
        assert!(agent_definitions.iter().any(|definition| {
            definition["definitionType"] == "mobkit/profile-member"
                && definition["source"] == "mobkit/mobpack-profile-member"
                && definition["sourceDocumentPath"] == "document.members[]"
                && definition["sourceMobpack"]
                    .as_str()
                    .is_some_and(|value| value.starts_with("sample_"))
        }));
        for definition in agent_definitions {
            let source_mobpack = definition["sourceMobpack"]
                .as_str()
                .expect("agent definition source mobpack");
            let role = definition["role"].as_str().expect("agent definition role");
            let member = sample_members
                .get(&(source_mobpack.to_string(), role.to_string()))
                .expect("agent definition must be projected from a real sample mobpack member");
            assert_eq!(definition["tools"], member["tools"]);
            assert_eq!(definition["skills"], member["skills"]);
            assert_eq!(definition["model"], member["model"]);
            assert_eq!(definition["profileBinding"], member["profileBinding"]);
            assert_eq!(definition["runtimeMode"], member["runtimeMode"]);
            assert_eq!(definition["systemPrompt"], member["systemPrompt"]);
        }
        assert!(agent_definitions.iter().any(|definition| {
            definition["role"] == "reviewer"
                && definition["schema"] == "ReviewerOutput"
                && definition["schemaDefinition"]["id"] == "ReviewerOutput"
                && definition["schemaDefinition"]["fields"]
                    .as_array()
                    .is_some_and(|fields| !fields.is_empty())
        }));
        if let Ok(path) = std::env::var("MOBKIT_MOBPACK_SCHEMA_OUT") {
            std::fs::write(path, serde_json::to_vec_pretty(&schema).unwrap())
                .expect("write schema response fixture");
        }
    }

    #[test]
    fn discovers_configured_tool_sources_without_mock_catalog() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mcp.toml");
        std::fs::write(
            &path,
            r#"
[[servers]]
name = "linear"
command = "npx"

[[servers]]
name = "github"
url = "https://example.invalid/mcp"
"#,
        )
        .expect("write mcp config");

        let sources = mcp_sources_from_toml(&path);
        assert!(sources.contains("linear"));
        assert!(sources.contains("github"));
        assert_eq!(sources.len(), 2);

        let env_sources = split_env_list("custom-a, custom-b:custom-c; custom-a");
        assert!(env_sources.contains("custom-a"));
        assert!(env_sources.contains("custom-b"));
        assert!(env_sources.contains("custom-c"));
        assert_eq!(env_sources.len(), 3);
    }

    #[test]
    fn discovers_meerkat_mob_registry_skill_dirs_without_pinned_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("index.crates.io-abc");
        std::fs::create_dir_all(registry.join("meerkat-mob-0.6.34/skills"))
            .expect("old skills dir");
        std::fs::create_dir_all(registry.join("meerkat-mob-0.6.58/skills"))
            .expect("new skills dir");
        std::fs::create_dir_all(registry.join("meerkat-mob-mcp-0.6.58/skills"))
            .expect("adjacent crate skills dir");
        std::fs::create_dir_all(registry.join("meerkat-mob-not-a-version/skills"))
            .expect("non-version skills dir");

        let dirs = meerkat_mob_registry_skill_dirs(dir.path());
        let ids = dirs
            .iter()
            .map(|(id, _, _)| id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["meerkat-mob/crate/0.6.34", "meerkat-mob/crate/0.6.58"]
        );
        assert!(dirs.iter().all(|(_, _, path)| path.ends_with("skills")));
    }

    #[test]
    fn imports_raw_mob_toml_into_editor_projection() {
        let result = import_mobpack(&json!({ "mob_toml": importable_mob_toml() })).expect("import");
        let document: MobpackDocument =
            serde_json::from_value(result["document"].clone()).expect("document");
        assert_eq!(document.mob_id, "importable-real-mob");
        assert!(
            document
                .mob_toml
                .as_deref()
                .unwrap_or("")
                .contains("[flows.main]")
        );
        let members = document.members.as_array().expect("members projected");
        assert_eq!(members.len(), 2);
        assert_eq!(document.mob_settings["orchestrator"], json!("planner"));
        assert_eq!(document.mob_settings["autoWireOrchestrator"], json!(true));
        assert_eq!(
            document.mob_settings["roleWiring"],
            json!([{ "a": "planner", "b": "reviewer" }])
        );
        assert_eq!(document.mob_settings["backendDefault"], json!("external"));
        assert_eq!(
            document.mob_settings["externalAddressBase"],
            json!("http://127.0.0.1:9000")
        );
        assert_eq!(
            document.mob_settings["advanced"]["topology"]["mode"],
            json!("strict")
        );
        assert_eq!(
            document.mob_settings["advanced"]["supervisor"]["role"],
            json!("planner")
        );
        assert_eq!(
            document.mob_settings["advanced"]["limits"]["max_orphaned_turns"],
            json!(8)
        );
        assert_eq!(
            document.mob_settings["advanced"]["spawnPolicy"]["profile_map"]["reviewer"],
            json!("reviewer")
        );
        assert_eq!(
            document.mob_settings["advanced"]["eventRouter"]["buffer_size"],
            json!(128)
        );
        assert!(members.iter().any(|member| {
            member["id"] == "m_reviewer"
                && member["runtimeMode"] == "turn_driven"
                && member["tools"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("shell"))
        }));
        assert!(members.iter().any(|member| {
            member["id"] == "m_planner"
                && member["backend"] == "session"
                && member["maxInlinePeerNotifications"] == 20
                && member["providerParams"] == json!({ "thinking_budget": 8192, "top_k": 20 })
        }));
        let plan_step = document.flow["steps"]
            .as_array()
            .and_then(|steps| steps.iter().find(|step| step["id"] == "plan"))
            .expect("plan step projected");
        assert_eq!(plan_step["timeoutMs"], json!(1500));
        assert_eq!(plan_step["allowedTools"], json!(["mob"]));
        assert_eq!(plan_step["blockedTools"], json!(["shell"]));
        assert_eq!(plan_step["outputFormat"], json!("text"));
        assert_eq!(plan_step["dispatchMode"], json!("one_to_one"));
        assert_eq!(plan_step["collection"], json!("quorum"));
        assert_eq!(plan_step["quorum"], json!(1));
        let schemas = document.schemas.as_array().expect("schemas projected");
        assert_eq!(schemas[0]["id"], "ReviewerOutput");
        assert_eq!(schemas[0]["fields"][0]["type"], "enum");
        let skill_realms = document
            .skill_realms
            .as_array()
            .expect("skill realms projected");
        assert!(
            skill_realms[0]["skills"]
                .as_array()
                .unwrap()
                .iter()
                .any(|skill| skill["id"] == "mob.review" && skill["source"] == "path")
        );
        let flow = document.flow.as_object().expect("flow projected");
        assert_eq!(flow["name"], "main");
        assert!(
            flow["steps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|step| step["type"] == "repeat")
        );
        let repeat = flow["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|step| step["type"] == "repeat")
            .expect("repeat step projected");
        assert_eq!(repeat["iterationInput"], Value::Null);
        let validation = result["validation"].as_object().expect("validation");
        assert_eq!(validation["ok"], true);
    }

    #[test]
    fn import_projection_does_not_materialize_default_json_output_format() {
        let toml = r#"
[mob]
id = "default-output-import"

[profiles.planner]
model = "gpt-5.2"

[flows.main]
description = "Do not materialize parser defaults"

[flows.main.steps.plan]
role = "planner"
message = "Plan without output format"
"#;
        let result = import_mobpack(&json!({ "mob_toml": toml })).expect("import");
        let document: MobpackDocument =
            serde_json::from_value(result["document"].clone()).expect("document");
        let plan_step = document.flow["steps"]
            .as_array()
            .and_then(|steps| steps.iter().find(|step| step["id"] == "plan"))
            .expect("plan step projected");
        assert_eq!(plan_step["outputFormat"], Value::Null);
    }

    #[test]
    fn import_projection_uses_editor_input_step_draft_for_missing_flow_description() {
        let toml = r#"
[mob]
id = "missing-flow-description"

[profiles.planner]
model = "gpt-5.2"

[flows.main.steps.plan]
role = "planner"
message = "Plan without a flow description"
"#;
        let result = import_mobpack(&json!({ "mob_toml": toml })).expect("import");
        let document: MobpackDocument =
            serde_json::from_value(result["document"].clone()).expect("document");
        let input_step = document.flow["steps"]
            .as_array()
            .and_then(|steps| steps.iter().find(|step| step["type"] == "input"))
            .expect("input step projected");
        assert_eq!(
            input_step["task"],
            editor_input_step_draft_contract()["default_step"]["task"]
        );
    }

    #[test]
    fn imports_repeat_gt_condition_into_basic_projection() {
        let toml = importable_mob_toml().replace(
            r#"until = { op = "eq", path = "steps.review.verdict", value = "green" }"#,
            r#"until = { op = "gt", path = "steps.review.score", value = 90 }"#,
        );
        let result = import_mobpack(&json!({ "mob_toml": toml })).expect("import");
        let document: MobpackDocument =
            serde_json::from_value(result["document"].clone()).expect("document");
        let repeat = document
            .flow
            .get("steps")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .find(|step| step["type"] == "repeat")
            .expect("repeat projected");

        assert_eq!(repeat["cond"]["op"], ">");
        assert_eq!(repeat["cond"]["field"], "score");
        assert_eq!(repeat["cond"]["val"], "90");
    }

    #[test]
    fn imports_parallel_frame_into_basic_parallel_projection() {
        let toml = r#"
[mob]
id = "parallel-import"

[profiles.coder]
model = "gpt-5.2"

[profiles.reviewer]
model = "gpt-5.2"

[flows.main]
description = "Parallel import"

[flows.main.steps.code]
role = "coder"
message = "Implement"

[flows.main.steps.review]
role = "reviewer"
message = "Review"

[flows.main.root.nodes.node_code]
kind = "step"
step_id = "code"
depends_on = []
depends_on_mode = "all"

[flows.main.root.nodes.node_review]
kind = "step"
step_id = "review"
depends_on = []
depends_on_mode = "all"
"#;
        let result = import_mobpack(&json!({ "mob_toml": toml })).expect("import");
        assert_eq!(result["validation"]["ok"], true);
        let document: MobpackDocument =
            serde_json::from_value(result["document"].clone()).expect("document");
        let parallel = document
            .flow
            .get("steps")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .find(|step| step["type"] == "parallel")
            .expect("parallel projected");

        assert_eq!(parallel["collection"], json!("all"));
        assert_eq!(parallel["dispatch"], json!("fan_out"));
        assert_eq!(parallel["branches"].as_array().unwrap().len(), 2);
        assert!(
            parallel["branches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|branch| branch["steps"][0]["role"] == "m_coder")
        );
        assert!(
            parallel["branches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|branch| branch["steps"][0]["role"] == "m_reviewer")
        );
    }

    #[test]
    fn imports_mobpack_archive_without_editor_json_into_projection() {
        let mob_toml = importable_mob_toml();
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = Builder::new(encoder);
        append_archive_file(
            &mut archive,
            "manifest.toml",
            b"surfaces = [\"cli\"]\n\n[mobpack]\nname = \"imported\"\n",
        )
        .expect("manifest");
        append_archive_file(&mut archive, "mobkit/mob.toml", mob_toml.as_bytes())
            .expect("mob toml");
        let encoder = archive.into_inner().expect("archive");
        let bytes = encoder.finish().expect("gzip");
        let result = import_mobpack(&json!({
            "content_base64": base64::engine::general_purpose::STANDARD.encode(bytes)
        }))
        .expect("import archive");
        let document: MobpackDocument =
            serde_json::from_value(result["document"].clone()).expect("document");
        assert_eq!(document.mob_id, "importable-real-mob");
        assert!(document.members.as_array().unwrap().len() >= 2);
        assert!(document.instances.as_array().unwrap().len() >= 2);
        assert!(
            document
                .edges
                .as_array()
                .unwrap()
                .iter()
                .any(|edge| edge["kind"] == "cond")
        );
    }

    #[test]
    fn imports_branch_flow_into_basic_branch_projection() {
        let toml = r#"
[mob]
id = "branch-import"

[profiles.router]
model = "gpt-5.2"

[profiles.worker]
model = "gpt-5.2"

[flows.main]
description = "Branch import"

[flows.main.steps.route]
role = "router"
message = "Route the work"

[flows.main.steps.left]
role = "worker"
message = "Handle left"
depends_on = ["route"]
branch = "choice"
condition = { op = "eq", path = "params.choice", value = "left" }

[flows.main.steps.right]
role = "worker"
message = "Handle right"
depends_on = ["route"]
branch = "choice"
condition = { op = "eq", path = "params.choice", value = "right" }

[flows.main.root.nodes.node_route]
kind = "step"
step_id = "route"
depends_on = []
depends_on_mode = "all"

[flows.main.root.nodes.node_left]
kind = "step"
step_id = "left"
depends_on = ["node_route"]
depends_on_mode = "all"
branch = "choice"

[flows.main.root.nodes.node_right]
kind = "step"
step_id = "right"
depends_on = ["node_route"]
depends_on_mode = "all"
branch = "choice"
"#;
        let result = import_mobpack(&json!({ "mob_toml": toml })).expect("import");
        assert_eq!(result["validation"]["ok"], true);
        let document: MobpackDocument =
            serde_json::from_value(result["document"].clone()).expect("document");
        let steps = document.flow["steps"].as_array().expect("flow steps");
        let input_params = steps[0]["inputParams"].as_array().expect("input params");
        assert!(input_params.iter().any(|param| param["name"] == "choice"));
        let branch = steps
            .iter()
            .find(|step| step["type"] == "branch")
            .expect("branch projected");
        assert_eq!(branch["controllerRole"], json!("m_router"));
        assert_eq!(branch["branches"].as_array().unwrap().len(), 2);
        assert!(
            branch["branches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|branch| branch["condition"] == "params.choice == left")
        );
    }
}
