//! Shared access-control handle: live config, persistence, attribute cache.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use super::engine::{
    AccessDecision, AccessPrincipal, AccessResource, evaluate_access, groups_for_subject,
};
use super::model::{
    ACTION_AGENT_VIEW, AccessConfigError, AccessControlConfig, AccessGroup, AccessRule,
    validate_access_config,
};

/// Cached resource attributes for one agent, keyed by console identity.
///
/// The console surfaces refresh this cache opportunistically whenever they
/// project a roster snapshot, so label/role selectors evaluate against the
/// most recent known attributes even on surfaces that only carry an
/// identity string (timeline frames, SSE streams, send requests).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentResourceAttributes {
    pub identity: String,
    pub agent_id: Option<String>,
    pub role: Option<String>,
    pub labels: BTreeMap<String, String>,
}

struct AccessState {
    config: Arc<AccessControlConfig>,
    revision: u64,
}

struct AccessControllerInner {
    state: RwLock<AccessState>,
    persist_path: RwLock<Option<PathBuf>>,
    attributes: RwLock<BTreeMap<String, Arc<AgentResourceAttributes>>>,
}

/// Shared, cheaply clonable handle to the live access-control state.
///
/// `None`/absent controller or a disabled config means the feature is off
/// and every surface behaves exactly as before. All mutations validate,
/// bump the revision, and persist to the configured TOML path (if any).
#[derive(Clone)]
pub struct AccessController {
    inner: Arc<AccessControllerInner>,
}

impl std::fmt::Debug for AccessController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (config, revision) = self.snapshot();
        f.debug_struct("AccessController")
            .field("enabled", &config.enabled)
            .field("revision", &revision)
            .field("rules", &config.rules.len())
            .finish()
    }
}

impl AccessController {
    /// Create a controller from a validated config.
    pub fn new(config: AccessControlConfig) -> Result<Self, AccessConfigError> {
        validate_access_config(&config)?;
        Ok(Self {
            inner: Arc::new(AccessControllerInner {
                state: RwLock::new(AccessState {
                    config: Arc::new(config),
                    revision: 0,
                }),
                persist_path: RwLock::new(None),
                attributes: RwLock::new(BTreeMap::new()),
            }),
        })
    }

    /// Create a disabled controller (feature off until an admin enables it).
    pub fn disabled() -> Self {
        Self::new(AccessControlConfig::default()).unwrap_or_else(|_| unreachable!())
    }

    /// Load a controller from a TOML file, remembering the path so future
    /// admin mutations persist back to it. A missing file yields a default
    /// (disabled) config that is written on first mutation.
    pub fn load_or_default(path: impl Into<PathBuf>) -> Result<Self, AccessConfigError> {
        let path = path.into();
        let config = if path.is_file() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|err| AccessConfigError::Io(err.to_string()))?;
            toml::from_str::<AccessControlConfig>(&raw)
                .map_err(|err| AccessConfigError::Parse(err.to_string()))?
        } else {
            AccessControlConfig::default()
        };
        let controller = Self::new(config)?;
        *controller
            .inner
            .persist_path
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);
        Ok(controller)
    }

    /// Set (or replace) the persistence path.
    pub fn with_persist_path(self, path: impl Into<PathBuf>) -> Self {
        *self
            .inner
            .persist_path
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path.into());
        self
    }

    /// Current config and revision.
    pub fn snapshot(&self) -> (Arc<AccessControlConfig>, u64) {
        let state = self
            .inner
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (Arc::clone(&state.config), state.revision)
    }

    /// True when checks are actually enforced.
    pub fn enabled(&self) -> bool {
        self.snapshot().0.enabled
    }

    /// Replace the whole configuration (admin surface).
    pub fn replace_config(&self, config: AccessControlConfig) -> Result<u64, AccessConfigError> {
        validate_access_config(&config)?;
        self.commit(config)
    }

    /// Insert or update one rule by id.
    pub fn upsert_rule(&self, rule: AccessRule) -> Result<u64, AccessConfigError> {
        let mut config = self.config_for_update();
        match config
            .rules
            .iter_mut()
            .find(|existing| existing.id == rule.id)
        {
            Some(existing) => *existing = rule,
            None => config.rules.push(rule),
        }
        validate_access_config(&config)?;
        self.commit(config)
    }

    /// Delete one rule by id.
    pub fn delete_rule(&self, rule_id: &str) -> Result<u64, AccessConfigError> {
        let mut config = self.config_for_update();
        let before = config.rules.len();
        config.rules.retain(|rule| rule.id != rule_id);
        if config.rules.len() == before {
            return Err(AccessConfigError::UnknownRule(rule_id.to_string()));
        }
        validate_access_config(&config)?;
        self.commit(config)
    }

    /// Create or replace a group (the live per-user assignment surface).
    pub fn set_group(&self, name: &str, group: AccessGroup) -> Result<u64, AccessConfigError> {
        let mut config = self.config_for_update();
        config.groups.insert(name.to_string(), group);
        validate_access_config(&config)?;
        self.commit(config)
    }

    /// Delete a group. Fails while rules still reference it.
    pub fn delete_group(&self, name: &str) -> Result<u64, AccessConfigError> {
        let mut config = self.config_for_update();
        config.groups.remove(name);
        validate_access_config(&config)?;
        self.commit(config)
    }

    /// Toggle enforcement. Enabling validates the anti-lockout invariant.
    pub fn set_enabled(&self, enabled: bool) -> Result<u64, AccessConfigError> {
        let mut config = self.config_for_update();
        config.enabled = enabled;
        validate_access_config(&config)?;
        self.commit(config)
    }

    fn config_for_update(&self) -> AccessControlConfig {
        (*self.snapshot().0).clone()
    }

    fn commit(&self, config: AccessControlConfig) -> Result<u64, AccessConfigError> {
        let persist_path = self
            .inner
            .persist_path
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(path) = persist_path {
            persist_config(&path, &config)?;
        }
        let mut state = self
            .inner
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.config = Arc::new(config);
        state.revision += 1;
        Ok(state.revision)
    }

    /// Build the per-request view for an authenticated subject (or `None`
    /// for an open/unauthenticated console).
    pub fn view_for_subject(&self, subject: Option<&str>) -> AccessView {
        let (config, _) = self.snapshot();
        let principal = match subject {
            Some(subject) => AccessPrincipal {
                subject: Some(subject.to_string()),
                groups: groups_for_subject(&config, subject),
            },
            None => AccessPrincipal::anonymous(),
        };
        let is_admin = principal
            .subject
            .as_deref()
            .is_some_and(|subject| config.admins.iter().any(|admin| admin == subject));
        AccessView {
            inner: Arc::clone(&self.inner),
            config,
            principal,
            is_admin,
        }
    }

    /// Refresh the cached resource attributes for one agent.
    pub fn record_agent_attributes(&self, attributes: AgentResourceAttributes) {
        if attributes.identity.is_empty() {
            return;
        }
        let mut cache = self
            .inner
            .attributes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.insert(attributes.identity.clone(), Arc::new(attributes));
    }
}

fn persist_config(path: &Path, config: &AccessControlConfig) -> Result<(), AccessConfigError> {
    let rendered =
        toml::to_string_pretty(config).map_err(|err| AccessConfigError::Parse(err.to_string()))?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| AccessConfigError::Io(err.to_string()))?;
    }
    let header = "# MobKit access control. Managed by the console Access panel;\n# hand edits are preserved until the next console save.\n\n";
    std::fs::write(path, format!("{header}{rendered}"))
        .map_err(|err| AccessConfigError::Io(err.to_string()))
}

/// An immutable per-request snapshot of one principal's access.
///
/// Holds the config `Arc` taken at request start so a single request
/// evaluates against one consistent config, plus a handle to the shared
/// attribute cache for label/role lookups by identity.
#[derive(Clone)]
pub struct AccessView {
    inner: Arc<AccessControllerInner>,
    config: Arc<AccessControlConfig>,
    principal: AccessPrincipal,
    is_admin: bool,
}

impl AccessView {
    /// True when this view actually enforces anything.
    pub fn enforced(&self) -> bool {
        self.config.enabled
    }

    pub fn subject(&self) -> Option<&str> {
        self.principal.subject.as_deref()
    }

    pub fn groups(&self) -> &BTreeSet<String> {
        &self.principal.groups
    }

    pub fn is_admin(&self) -> bool {
        self.is_admin
    }

    /// Full check against explicit resource attributes.
    pub fn decide(&self, action: &str, resource: &AccessResource<'_>) -> AccessDecision {
        evaluate_access(&self.config, &self.principal, action, resource)
    }

    /// Check an action with no resource (e.g. `gating.decide`).
    pub fn allows(&self, action: &str) -> bool {
        self.decide(action, &AccessResource::none()).is_allow()
    }

    /// Check an action against an agent identity, resolving cached
    /// attributes (role/labels) when available.
    pub fn allows_agent(&self, action: &str, identity: &str) -> bool {
        self.decide_agent(action, identity).is_allow()
    }

    /// Full decision for an action against an agent identity, resolving
    /// cached attributes (role/labels) when available. The argument may
    /// also be a runtime agent/member id; the cache resolves it back to
    /// the identity it belongs to.
    pub fn decide_agent(&self, action: &str, identity: &str) -> AccessDecision {
        if !self.config.enabled {
            return AccessDecision::Allow;
        }
        let cached = {
            let cache = self
                .inner
                .attributes
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            cache.get(identity).cloned().or_else(|| {
                cache
                    .values()
                    .find(|attributes| attributes.agent_id.as_deref() == Some(identity))
                    .cloned()
            })
        };
        let resource = match cached.as_deref() {
            Some(attributes) => AccessResource {
                identity: Some(attributes.identity.as_str()),
                agent_id: attributes.agent_id.as_deref().or(Some(identity)),
                role: attributes.role.as_deref(),
                labels: Some(&attributes.labels),
            },
            None => AccessResource::for_identity(identity),
        };
        self.decide(action, &resource)
    }

    /// Convenience: can this principal see the given agent at all?
    pub fn can_view_agent(&self, identity: &str) -> bool {
        self.allows_agent(ACTION_AGENT_VIEW, identity)
    }

    /// Can this principal read and edit the access configuration?
    ///
    /// Admins always can. While enforcement is enabled, subjects granted
    /// `access.admin` by rule also can. While the feature is *disabled* and
    /// no admins are configured yet, any caller can — this is the bootstrap
    /// path that lets a fresh deployment configure itself from the console
    /// before flipping enforcement on (enabling requires naming admins).
    pub fn can_administer(&self) -> bool {
        if self.is_admin {
            return true;
        }
        if !self.config.enabled {
            return self.config.admins.is_empty();
        }
        self.decide(super::model::ACTION_ACCESS_ADMIN, &AccessResource::none())
            .is_allow()
    }
}

impl std::fmt::Debug for AccessView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessView")
            .field("subject", &self.principal.subject)
            .field("groups", &self.principal.groups)
            .field("is_admin", &self.is_admin)
            .field("enforced", &self.config.enabled)
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::access::model::AccessEffect;

    fn enabled_config() -> AccessControlConfig {
        AccessControlConfig {
            enabled: true,
            admins: vec!["root@example.test".to_string()],
            groups: BTreeMap::from([(
                "ops".to_string(),
                AccessGroup {
                    description: None,
                    members: vec!["alice@example.test".to_string()],
                },
            )]),
            rules: vec![AccessRule {
                id: "ops-view-all".to_string(),
                groups: vec!["ops".to_string()],
                actions: vec!["agent.view".to_string()],
                ..AccessRule::default()
            }],
        }
    }

    #[test]
    fn view_resolves_groups_and_admin_flag() {
        let controller = AccessController::new(enabled_config()).expect("controller");
        let alice = controller.view_for_subject(Some("alice@example.test"));
        assert!(alice.groups().contains("ops"));
        assert!(!alice.is_admin());
        assert!(alice.can_view_agent("identity:scout-1"));
        assert!(!alice.allows_agent("agent.send", "identity:scout-1"));

        let root = controller.view_for_subject(Some("root@example.test"));
        assert!(root.is_admin());
        assert!(root.allows("access.admin"));
    }

    #[test]
    fn live_mutations_bump_revision_and_apply() {
        let controller = AccessController::new(enabled_config()).expect("controller");
        let bob = controller.view_for_subject(Some("bob@example.test"));
        assert!(!bob.can_view_agent("identity:scout-1"));

        let revision = controller
            .set_group(
                "ops",
                AccessGroup {
                    description: None,
                    members: vec![
                        "alice@example.test".to_string(),
                        "bob@example.test".to_string(),
                    ],
                },
            )
            .expect("set group");
        assert_eq!(revision, 1);

        // New views pick up the change immediately; the old snapshot stays
        // consistent for the request it was created for.
        let bob_after = controller.view_for_subject(Some("bob@example.test"));
        assert!(bob_after.can_view_agent("identity:scout-1"));
        assert!(!bob.can_view_agent("identity:scout-1"));
    }

    #[test]
    fn delete_rule_unknown_id_errors() {
        let controller = AccessController::new(enabled_config()).expect("controller");
        assert_eq!(
            controller.delete_rule("missing"),
            Err(AccessConfigError::UnknownRule("missing".to_string()))
        );
        controller.delete_rule("ops-view-all").expect("delete");
        let (config, revision) = controller.snapshot();
        assert!(config.rules.is_empty());
        assert_eq!(revision, 1);
    }

    #[test]
    fn attribute_cache_feeds_label_selectors() {
        let mut config = enabled_config();
        config.rules.push(AccessRule {
            id: "bob-payments".to_string(),
            subjects: vec!["bob@example.test".to_string()],
            actions: vec!["agent.view".to_string()],
            match_labels: BTreeMap::from([("org".to_string(), "payments".to_string())]),
            ..AccessRule::default()
        });
        let controller = AccessController::new(config).expect("controller");
        let bob = controller.view_for_subject(Some("bob@example.test"));
        assert!(!bob.can_view_agent("identity:pay-1"));

        controller.record_agent_attributes(AgentResourceAttributes {
            identity: "identity:pay-1".to_string(),
            agent_id: Some("pay-1".to_string()),
            role: Some("analyst".to_string()),
            labels: BTreeMap::from([("org".to_string(), "payments".to_string())]),
        });
        assert!(bob.can_view_agent("identity:pay-1"));
        assert!(!bob.can_view_agent("identity:other"));
    }

    #[test]
    fn persistence_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config").join("access.toml");
        let controller = AccessController::load_or_default(&path).expect("load default");
        assert!(!controller.enabled());

        let mut config = enabled_config();
        config.rules.push(AccessRule {
            id: "deny-secret".to_string(),
            effect: AccessEffect::Deny,
            actions: vec!["agent.*".to_string()],
            agents: vec!["identity:secret".to_string()],
            ..AccessRule::default()
        });
        controller.replace_config(config.clone()).expect("replace");

        let reloaded = AccessController::load_or_default(&path).expect("reload");
        let (reloaded_config, _) = reloaded.snapshot();
        assert_eq!(*reloaded_config, config);
    }

    #[test]
    fn lockout_protected_on_live_surface() {
        let controller = AccessController::new(enabled_config()).expect("controller");
        let mut config = (*controller.snapshot().0).clone();
        config.admins.clear();
        assert_eq!(
            controller.replace_config(config),
            Err(AccessConfigError::EnabledWithoutAdmins)
        );
    }
}
