//! Core type definitions shared across the MobKit runtime.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Timestamped wrapper around an event payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub event_id: String,
    pub source: String,
    pub timestamp_ms: u64,
    pub event: T,
}

/// A runtime event from either an agent or a module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnifiedEvent {
    Agent {
        agent_id: String,
        event_type: String,
    },
    Module(ModuleEvent),
}

/// An event originating from a loaded MCP module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleEvent {
    pub module: String,
    pub event_type: String,
    pub payload: Value,
}

/// Configuration for a single MCP module to be loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleConfig {
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    pub restart_policy: RestartPolicy,
}

/// Policy controlling automatic module restart behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    Never,
    OnFailure,
    Always,
}

/// Specification for module discovery at startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverySpec {
    pub namespace: String,
    pub modules: Vec<String>,
}

/// Agent-level discovery specification for spawning agents into a mob.
///
/// Unlike [`DiscoverySpec`] (which describes module discovery for `MobKitConfig`),
/// this type captures the fields needed to discover and spawn individual agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDiscoverySpec {
    /// Agent profile name (maps to a profile in the mob definition).
    pub profile: String,
    /// Unique agent ID within the mob.
    pub meerkat_id: String,
    /// Application-defined labels for this agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
    /// Opaque application context passed through to the agent build pipeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
    /// Extra instructions appended to the agent prompt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_instructions: Vec<String>,
    /// Resume an existing session instead of creating a new one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_session_id: Option<String>,
}

/// Pre-spawn context passed to modules during discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreSpawnData {
    pub module_id: String,
    pub env: Vec<(String, String)>,
}

/// Top-level configuration for a MobKit runtime instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobKitConfig {
    pub modules: Vec<ModuleConfig>,
    pub discovery: DiscoverySpec,
    pub pre_spawn: Vec<PreSpawnData>,
}
