//! Access-control configuration schema and validation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// View an agent in any console surface: sidebar, roster, topology,
/// timeline frames, identity status, inspection, and event streams.
pub const ACTION_AGENT_VIEW: &str = "agent.view";
/// Send a message to an agent (console send / chat).
pub const ACTION_AGENT_SEND: &str = "agent.send";
/// Create new members: ensure/spawn/fork helpers, run flows.
pub const ACTION_AGENT_SPAWN: &str = "agent.spawn";
/// Respawn an existing agent.
pub const ACTION_AGENT_RESPAWN: &str = "agent.respawn";
/// Retire / force-cancel / delete an agent.
pub const ACTION_AGENT_RETIRE: &str = "agent.retire";
/// Reset an agent's durable state.
pub const ACTION_AGENT_RESET: &str = "agent.reset";
/// Read gating queues and audit history.
pub const ACTION_GATING_VIEW: &str = "gating.view";
/// Decide pending gating approvals.
pub const ACTION_GATING_DECIDE: &str = "gating.decide";
/// Subscribe to whole-mob event surfaces (raw mob/structural event streams).
pub const ACTION_MOB_OBSERVE: &str = "mob.observe";
/// Operate runtime plumbing: routing tables, labels, wiring, reconcile.
pub const ACTION_RUNTIME_ADMIN: &str = "runtime.admin";
/// Read and mutate the access-control configuration itself.
pub const ACTION_ACCESS_ADMIN: &str = "access.admin";

/// The full action vocabulary, in display order.
pub const ACCESS_ACTIONS: &[&str] = &[
    ACTION_AGENT_VIEW,
    ACTION_AGENT_SEND,
    ACTION_AGENT_SPAWN,
    ACTION_AGENT_RESPAWN,
    ACTION_AGENT_RETIRE,
    ACTION_AGENT_RESET,
    ACTION_GATING_VIEW,
    ACTION_GATING_DECIDE,
    ACTION_MOB_OBSERVE,
    ACTION_RUNTIME_ADMIN,
    ACTION_ACCESS_ADMIN,
];

/// Root access-control configuration. Serializable as TOML
/// (`config/access.toml`) and JSON (RPC admin surface).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessControlConfig {
    /// Master switch. When `false` the controller is a transparent no-op
    /// and every surface behaves exactly as if no access control existed.
    #[serde(default)]
    pub enabled: bool,
    /// Subjects with unconditional full access, including the right to
    /// edit this configuration. Must be non-empty while `enabled` is true
    /// so a bad rule set can never lock every administrator out.
    #[serde(default)]
    pub admins: Vec<String>,
    /// Named groups of subjects. Group membership is the per-user live
    /// configuration surface: assigning a subject to a group immediately
    /// changes what every rule referencing that group grants them.
    #[serde(default)]
    pub groups: BTreeMap<String, AccessGroup>,
    /// Attribute rules, evaluated as a set (order is irrelevant;
    /// deny-overrides-allow).
    #[serde(default)]
    pub rules: Vec<AccessRule>,
}

/// A named set of subjects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub members: Vec<String>,
}

/// Allow or deny.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessEffect {
    #[default]
    Allow,
    Deny,
}

/// One attribute rule.
///
/// Dimension semantics:
/// - `subjects` / `groups`: the rule applies to a principal when its
///   subject is listed in `subjects` (or `subjects` contains `"*"`), or it
///   belongs to any listed group. When both lists are empty the rule
///   applies to every principal, including unauthenticated ones.
/// - `actions`: required, non-empty. Entries are exact action names,
///   `"prefix.*"` wildcards, or `"*"`.
/// - `agents` / `roles` / `match_labels`: resource selectors. Each
///   specified selector must match (logical AND across dimensions); within
///   `agents` and `roles` any listed value matches (logical OR), and
///   `"*"` matches every value. Empty selectors leave that dimension
///   unconstrained. A rule with all three
///   empty matches every resource, including action checks that have no
///   resource at all (e.g. `gating.decide`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessRule {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub effect: AccessEffect,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    pub actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub match_labels: BTreeMap<String, String>,
}

impl AccessRule {
    /// True when the rule constrains the resource in any way.
    pub fn has_resource_selector(&self) -> bool {
        !self.agents.is_empty() || !self.roles.is_empty() || !self.match_labels.is_empty()
    }
}

/// Validation failure for an [`AccessControlConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessConfigError {
    EnabledWithoutAdmins,
    EmptyRuleId,
    DuplicateRuleId(String),
    EmptyActions(String),
    UnknownAction { rule: String, action: String },
    UnknownGroup { rule: String, group: String },
    UnknownRule(String),
    Parse(String),
    Io(String),
}

impl std::fmt::Display for AccessConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EnabledWithoutAdmins => write!(
                f,
                "access control cannot be enabled without at least one admin subject"
            ),
            Self::EmptyRuleId => write!(f, "access rule id must not be empty"),
            Self::DuplicateRuleId(id) => write!(f, "duplicate access rule id: {id}"),
            Self::EmptyActions(id) => write!(f, "access rule {id}: actions must not be empty"),
            Self::UnknownAction { rule, action } => {
                write!(f, "access rule {rule}: unknown action {action:?}")
            }
            Self::UnknownGroup { rule, group } => {
                write!(f, "access rule {rule}: unknown group {group:?}")
            }
            Self::UnknownRule(id) => write!(f, "unknown access rule id: {id}"),
            Self::Parse(message) => write!(f, "access config could not be parsed: {message}"),
            Self::Io(message) => write!(f, "access config io error: {message}"),
        }
    }
}

impl std::error::Error for AccessConfigError {}

fn action_pattern_is_known(pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return ACCESS_ACTIONS.iter().any(|action| {
            action
                .rsplit_once('.')
                .is_some_and(|(action_prefix, _)| action_prefix == prefix)
        });
    }
    ACCESS_ACTIONS.contains(&pattern)
}

/// Validate a configuration before accepting it.
///
/// Enforces the anti-lockout invariant (enabled implies admins), unique
/// non-empty rule ids, a non-empty known action list per rule, and that
/// every referenced group is defined.
pub fn validate_access_config(config: &AccessControlConfig) -> Result<(), AccessConfigError> {
    if config.enabled && config.admins.iter().all(|admin| admin.trim().is_empty()) {
        return Err(AccessConfigError::EnabledWithoutAdmins);
    }
    let mut seen_ids = std::collections::BTreeSet::new();
    for rule in &config.rules {
        if rule.id.trim().is_empty() {
            return Err(AccessConfigError::EmptyRuleId);
        }
        if !seen_ids.insert(rule.id.as_str()) {
            return Err(AccessConfigError::DuplicateRuleId(rule.id.clone()));
        }
        if rule.actions.is_empty() {
            return Err(AccessConfigError::EmptyActions(rule.id.clone()));
        }
        for action in &rule.actions {
            if !action_pattern_is_known(action) {
                return Err(AccessConfigError::UnknownAction {
                    rule: rule.id.clone(),
                    action: action.clone(),
                });
            }
        }
        for group in &rule.groups {
            if !config.groups.contains_key(group) {
                return Err(AccessConfigError::UnknownGroup {
                    rule: rule.id.clone(),
                    group: group.clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn rule(id: &str, actions: &[&str]) -> AccessRule {
        AccessRule {
            id: id.to_string(),
            actions: actions.iter().map(ToString::to_string).collect(),
            ..AccessRule::default()
        }
    }

    #[test]
    fn default_config_is_disabled_and_valid() {
        let config = AccessControlConfig::default();
        assert!(!config.enabled);
        assert!(validate_access_config(&config).is_ok());
    }

    #[test]
    fn enabling_requires_admins() {
        let config = AccessControlConfig {
            enabled: true,
            ..AccessControlConfig::default()
        };
        assert_eq!(
            validate_access_config(&config),
            Err(AccessConfigError::EnabledWithoutAdmins)
        );
    }

    #[test]
    fn rules_require_known_actions() {
        let mut config = AccessControlConfig {
            admins: vec!["root@example.test".to_string()],
            rules: vec![rule("r1", &["agent.view", "agent.*", "*"])],
            ..AccessControlConfig::default()
        };
        assert!(validate_access_config(&config).is_ok());
        config.rules.push(rule("r2", &["agent.fly"]));
        assert!(matches!(
            validate_access_config(&config),
            Err(AccessConfigError::UnknownAction { .. })
        ));
    }

    #[test]
    fn rules_reject_duplicate_ids_and_unknown_groups() {
        let mut config = AccessControlConfig {
            rules: vec![rule("r1", &["agent.view"]), rule("r1", &["agent.send"])],
            ..AccessControlConfig::default()
        };
        assert_eq!(
            validate_access_config(&config),
            Err(AccessConfigError::DuplicateRuleId("r1".to_string()))
        );
        config.rules.pop();
        config.rules[0].groups = vec!["ops".to_string()];
        assert!(matches!(
            validate_access_config(&config),
            Err(AccessConfigError::UnknownGroup { .. })
        ));
    }

    #[test]
    fn config_round_trips_through_toml() {
        let config = AccessControlConfig {
            enabled: true,
            admins: vec!["root@example.test".to_string()],
            groups: BTreeMap::from([(
                "ops".to_string(),
                AccessGroup {
                    description: Some("Operations".to_string()),
                    members: vec!["alice@example.test".to_string()],
                },
            )]),
            rules: vec![AccessRule {
                id: "ops-see-all".to_string(),
                description: Some("ops see everything".to_string()),
                effect: AccessEffect::Allow,
                groups: vec!["ops".to_string()],
                actions: vec!["agent.view".to_string()],
                ..AccessRule::default()
            }],
        };
        let toml = toml::to_string_pretty(&config).expect("serialize");
        let parsed: AccessControlConfig = toml::from_str(&toml).expect("parse");
        assert_eq!(parsed, config);
    }
}
