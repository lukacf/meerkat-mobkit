//! Builder for constructing a configured UnifiedRuntime instance.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use meerkat_client::LlmClient;
use meerkat_mob::{MobDefinition, MobStorage, SpawnMemberSpec};

use crate::contact_directory::ContactDirectory;
use crate::mob_handle_runtime::{
    CapabilityFlags, MobBootstrapOptions, MobBootstrapSpec, SessionHook,
};
use crate::runtime::RuntimeOptions;
use crate::types::{EventEnvelope, MobKitConfig, UnifiedEvent};

use super::edge_types::{Discovery, EdgeDiscovery, PreSpawnHook};
use super::types::{
    UnifiedRuntimeBootstrapError, UnifiedRuntimeBuilderError, UnifiedRuntimeBuilderField,
};
use super::{
    DEFAULT_DRAIN_TIMEOUT, ErrorHook, EventLogConfig, PostReconcileHook, PostSpawnHook,
    UnifiedRuntime, discovery_spec_to_spawn_spec,
};

/// How the mob definition is supplied to the builder.
pub(crate) enum DefinitionSource {
    Inline(Box<MobDefinition>),
    TomlPath(PathBuf),
}

/// Default max concurrent sessions for builder-created session services.
const DEFAULT_MAX_SESSIONS: usize = 64;

/// Default builder timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct UnifiedRuntimeBuilder {
    // --- Legacy path (mob_spec directly) ---
    mob_spec: Option<MobBootstrapSpec>,

    // --- New convenience path ---
    definition_source: Option<DefinitionSource>,
    persistent_state_path: Option<PathBuf>,
    session_hook: Option<Arc<dyn SessionHook>>,
    custom_session_store: Option<Arc<dyn meerkat::SessionStore>>,
    default_llm_client: Option<Arc<dyn LlmClient>>,
    capability_flags: CapabilityFlags,

    // --- Common fields ---
    module_config: Option<MobKitConfig>,
    module_agent_events: Vec<EventEnvelope<UnifiedEvent>>,
    timeout: Option<Duration>,
    options: RuntimeOptions,
    post_spawn_hook: Option<PostSpawnHook>,
    post_reconcile_hook: Option<PostReconcileHook>,
    error_hook: Option<ErrorHook>,
    event_log_config: Option<EventLogConfig>,
    drain_timeout: Option<Duration>,
    discovery: Option<Box<dyn Discovery>>,
    pre_spawn_hook: Option<PreSpawnHook>,
    edge_discovery: Option<Box<dyn EdgeDiscovery>>,
    contact_directory: Option<ContactDirectory>,
}

impl UnifiedRuntimeBuilder {
    // -----------------------------------------------------------------------
    // New convenience API
    // -----------------------------------------------------------------------

    /// Set the mob definition from an inline `MobDefinition`.
    pub fn definition(mut self, def: MobDefinition) -> Self {
        self.definition_source = Some(DefinitionSource::Inline(Box::new(def)));
        self
    }

    /// Set the mob definition from a TOML file path.
    pub fn definition_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.definition_source = Some(DefinitionSource::TomlPath(path.into()));
        self
    }

    /// Enable persistent state at the given path. When set, the builder
    /// creates a `SqliteSessionStore`, `FsBlobStore`, and `MobStorage::redb()`
    /// under this directory. When not set, the builder uses an ephemeral
    /// session service with an auto-created temp directory.
    pub fn persistent_state(mut self, path: impl Into<PathBuf>) -> Self {
        self.persistent_state_path = Some(path.into());
        self
    }

    /// Set a session lifecycle hook.
    pub fn session_hook(mut self, hook: Arc<dyn SessionHook>) -> Self {
        self.session_hook = Some(hook);
        self
    }

    /// Set a custom session store for the persistent path. When set, the
    /// builder uses this store instead of creating a default `SqliteSessionStore`.
    /// Only valid with `.persistent_state()` — ignored for ephemeral builds.
    pub fn session_store(mut self, store: Arc<dyn meerkat::SessionStore>) -> Self {
        self.custom_session_store = Some(store);
        self
    }

    /// Set the default LLM client (used for test stubs).
    pub fn default_llm_client(mut self, client: Arc<dyn LlmClient>) -> Self {
        self.default_llm_client = Some(client);
        self
    }

    /// Enable or disable builtin tools (default: true).
    pub fn builtins(mut self, enabled: bool) -> Self {
        self.capability_flags.builtins = enabled;
        self
    }

    /// Enable or disable shell tool (default: true).
    pub fn shell(mut self, enabled: bool) -> Self {
        self.capability_flags.shell = enabled;
        self
    }

    /// Enable or disable mob tools (default: true).
    pub fn mob(mut self, enabled: bool) -> Self {
        self.capability_flags.mob = enabled;
        self
    }

    /// Enable or disable comms (default: true).
    pub fn comms(mut self, enabled: bool) -> Self {
        self.capability_flags.comms = enabled;
        self
    }

    /// Enable or disable memory tools (default: true).
    pub fn memory(mut self, enabled: bool) -> Self {
        self.capability_flags.memory = enabled;
        self
    }

    // -----------------------------------------------------------------------
    // Legacy API (preserved for backward compat)
    // -----------------------------------------------------------------------

    pub fn mob_spec(mut self, spec: MobBootstrapSpec) -> Self {
        self.mob_spec = Some(spec);
        self
    }

    pub fn module_config(mut self, config: MobKitConfig) -> Self {
        self.module_config = Some(config);
        self
    }

    pub fn module_agent_events(mut self, events: Vec<EventEnvelope<UnifiedEvent>>) -> Self {
        self.module_agent_events = events;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn runtime_options(mut self, options: RuntimeOptions) -> Self {
        self.options = options;
        self
    }

    pub fn post_spawn_hook(mut self, hook: PostSpawnHook) -> Self {
        self.post_spawn_hook = Some(hook);
        self
    }

    pub fn post_reconcile_hook(mut self, hook: PostReconcileHook) -> Self {
        self.post_reconcile_hook = Some(hook);
        self
    }

    pub fn on_error(mut self, hook: ErrorHook) -> Self {
        self.error_hook = Some(hook);
        self
    }

    pub fn event_log(mut self, config: EventLogConfig) -> Self {
        self.event_log_config = Some(config);
        self
    }

    pub fn drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = Some(timeout);
        self
    }

    pub fn discovery(mut self, discovery: impl Discovery + 'static) -> Self {
        self.discovery = Some(Box::new(discovery));
        self
    }

    pub fn pre_spawn_hook(mut self, hook: PreSpawnHook) -> Self {
        self.pre_spawn_hook = Some(hook);
        self
    }

    pub fn edge_discovery(mut self, edge_discovery: impl EdgeDiscovery + 'static) -> Self {
        self.edge_discovery = Some(Box::new(edge_discovery));
        self
    }

    /// Set the contact directory for cross-mob address resolution.
    pub fn contact_directory(mut self, directory: ContactDirectory) -> Self {
        self.contact_directory = Some(directory);
        self
    }

    // -----------------------------------------------------------------------
    // Build
    // -----------------------------------------------------------------------

    pub async fn build(mut self) -> Result<UnifiedRuntime, UnifiedRuntimeBuilderError> {
        // Legacy mob_spec path takes precedence — must be consumed before
        // resolve_mob_spec (which borrows &self for the definition path).
        let mob_spec = match self.mob_spec.take() {
            Some(spec) => {
                // Legacy path: require module_config and timeout as before.
                if self.module_config.is_none() {
                    return Err(UnifiedRuntimeBuilderError::MissingRequiredField(
                        UnifiedRuntimeBuilderField::ModuleConfig,
                    ));
                }
                if self.timeout.is_none() {
                    return Err(UnifiedRuntimeBuilderError::MissingRequiredField(
                        UnifiedRuntimeBuilderField::Timeout,
                    ));
                }
                spec
            }
            None => self.resolve_mob_spec().await?,
        };

        let module_config = self.module_config.unwrap_or_else(|| MobKitConfig {
            modules: Vec::new(),
            discovery: crate::types::DiscoverySpec {
                namespace: String::new(),
                modules: Vec::new(),
            },
            pre_spawn: Vec::new(),
        });
        let timeout = self.timeout.unwrap_or(DEFAULT_TIMEOUT);

        let runtime = UnifiedRuntime::bootstrap_with_options(
            mob_spec,
            module_config,
            self.module_agent_events,
            timeout,
            self.options,
        )
        .await
        .map_err(UnifiedRuntimeBuilderError::Bootstrap)?;

        // Set immutable outer fields by rebuilding the struct
        let runtime = UnifiedRuntime {
            post_spawn_hook: self.post_spawn_hook,
            post_reconcile_hook: self.post_reconcile_hook,
            error_hook: self.error_hook,
            drain_timeout: self.drain_timeout.unwrap_or(DEFAULT_DRAIN_TIMEOUT),
            discovery: self.discovery,
            edge_discovery: self.edge_discovery,
            contact_directory: self.contact_directory,
            ..runtime
        };

        let pre_spawn_context = if let Some(hook) = self.pre_spawn_hook {
            hook().await.map_err(|err| {
                UnifiedRuntimeBuilderError::Bootstrap(UnifiedRuntimeBootstrapError::PreSpawnHook(
                    err.to_string(),
                ))
            })?
        } else {
            serde_json::Value::Null
        };
        if let Some(ref discovery) = runtime.discovery {
            let specs = discovery.discover(pre_spawn_context).await;
            let spawn_specs: Vec<SpawnMemberSpec> =
                specs.iter().map(discovery_spec_to_spawn_spec).collect();
            runtime
                .spawn_many(spawn_specs)
                .await
                .map_err(UnifiedRuntimeBootstrapError::Mob)
                .map_err(UnifiedRuntimeBuilderError::Bootstrap)?;
        }

        // Run initial edge reconciliation after spawn completes
        if runtime.edge_discovery.is_some() {
            let report = runtime.reconcile_edges().await;
            *runtime.bootstrap_edges_report.write().await = Some(report);
        }

        // Start event log ingestion if configured
        let mut runtime = runtime;
        if let Some(event_log_config) = self.event_log_config {
            runtime.start_event_log(event_log_config);
        }

        Ok(runtime)
    }

    /// Resolve the mob spec from the definition-based path.
    /// Called only when `mob_spec` is not set (legacy path handled in `build()`).
    async fn resolve_mob_spec(&self) -> Result<MobBootstrapSpec, UnifiedRuntimeBuilderError> {
        let definition = match self.definition_source {
            Some(DefinitionSource::Inline(ref def)) => *def.clone(),
            Some(DefinitionSource::TomlPath(ref path)) => {
                let toml_content = std::fs::read_to_string(path).map_err(|e| {
                    UnifiedRuntimeBuilderError::Io(format!(
                        "failed to read definition TOML at {}: {e}",
                        path.display()
                    ))
                })?;
                MobDefinition::from_toml(&toml_content).map_err(|e| {
                    UnifiedRuntimeBuilderError::DefinitionLoad(format!(
                        "failed to parse definition TOML at {}: {e}",
                        path.display()
                    ))
                })?
            }
            None => {
                return Err(UnifiedRuntimeBuilderError::MissingRequiredField(
                    UnifiedRuntimeBuilderField::MobSpec,
                ));
            }
        };

        let hook = self
            .session_hook
            .as_ref()
            .map(|h| -> crate::mob_handle_runtime::PreBuildHook {
                let hook = h.clone();
                Arc::new(
                    move |req: &mut meerkat_core::service::CreateSessionRequest| {
                        let hook = hook.clone();
                        Box::pin(async move { hook.before_create(req).await })
                    },
                )
            });

        let after_hook: Option<crate::mob_handle_runtime::AfterCreateHook> = self
            .session_hook
            .as_ref()
            .map(|h| -> crate::mob_handle_runtime::AfterCreateHook {
                let hook = h.clone();
                Arc::new(move |session_id, ctx| {
                    let hook = hook.clone();
                    Box::pin(async move {
                        hook.after_create(&session_id, &ctx).await;
                    })
                })
            });

        let caps = self.capability_flags;

        // Note: blocking I/O (fs, SQLite, redb) — acceptable at startup.
        let mut spec = if let Some(ref state_path) = self.persistent_state_path {
            std::fs::create_dir_all(state_path).map_err(|e| {
                UnifiedRuntimeBuilderError::Io(format!(
                    "failed to create state directory at {}: {e}",
                    state_path.display()
                ))
            })?;

            let session_store: Arc<dyn meerkat::SessionStore> =
                if let Some(ref store) = self.custom_session_store {
                    store.clone()
                } else {
                    let sqlite_path = state_path.join("sessions.db");
                    Arc::new(
                        meerkat_store::SqliteSessionStore::open(sqlite_path).map_err(|e| {
                            UnifiedRuntimeBuilderError::Io(format!(
                                "failed to open SQLite session store: {e}"
                            ))
                        })?,
                    )
                };
            let mob_storage = MobStorage::redb(state_path.join("mob.redb")).map_err(|e| {
                UnifiedRuntimeBuilderError::Io(format!("failed to open redb mob storage: {e}"))
            })?;

            MobBootstrapSpec::persistent_inner(
                definition,
                mob_storage,
                state_path.clone(),
                DEFAULT_MAX_SESSIONS,
                session_store,
                hook,
                caps,
                after_hook.clone(),
            )
        } else {
            // Ephemeral: create a temp dir that lives as long as the runtime.
            let temp_dir = tempfile::tempdir().map_err(|e| {
                UnifiedRuntimeBuilderError::Io(format!("failed to create temp dir: {e}"))
            })?;
            let store_path = temp_dir.path().to_path_buf();

            let mut spec = MobBootstrapSpec::ephemeral_inner(
                definition,
                MobStorage::in_memory(),
                store_path,
                DEFAULT_MAX_SESSIONS,
                None,
                hook,
                caps,
                after_hook,
            );
            spec._ephemeral_dir = Some(Arc::new(temp_dir));
            spec
        };

        spec.options = MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: self.default_llm_client.clone(),
        };

        Ok(spec)
    }
}
