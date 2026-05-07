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
use crate::runtime::{
    InMemoryMetadataStore, PersistentMetadataStore, RuntimeOptions, SqliteMetadataStore,
};
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

    // --- Identity-first external path ---
    continuity_store: Option<Arc<dyn crate::identity_first::contracts::ContinuityStore>>,
    lease_provider: Option<Arc<dyn crate::identity_first::contracts::LeaseProvider>>,
    scratch_dir: Option<PathBuf>,
    runtime_store: Option<Arc<dyn meerkat::SessionStore>>,
    blob_store: Option<Arc<dyn meerkat_core::BlobStore>>,

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
    mob_storage_in_memory: bool,
    persistent_metadata: Option<Arc<dyn PersistentMetadataStore>>,
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
    /// creates a `SqliteSessionStore`, binary blob store, and
    /// `MobStorage::redb()` under this directory. When not set, the builder
    /// uses an ephemeral session service with an auto-created temp directory.
    pub fn persistent_state(mut self, path: impl Into<PathBuf>) -> Self {
        self.persistent_state_path = Some(path.into());
        self
    }

    /// Set a session lifecycle hook.
    pub fn session_hook(mut self, hook: Arc<dyn SessionHook>) -> Self {
        self.session_hook = Some(hook);
        self
    }

    /// Set a custom session store. When set, the builder uses this store
    /// instead of creating a default one. Works with both `.persistent_state()`
    /// (overrides the auto-created SQLite store) and ephemeral builds
    /// (provides durable sessions without redb mob storage).
    pub fn session_store(mut self, store: Arc<dyn meerkat::SessionStore>) -> Self {
        self.custom_session_store = Some(store);
        self
    }

    /// Use in-memory mob storage instead of redb. When set with
    /// `.persistent_state()`, sessions are still persisted (via the session
    /// store) but mob-level metadata (events, flow runs, specs) is ephemeral.
    /// Avoids the redb exclusive file lock.
    pub fn mob_storage_in_memory(mut self) -> Self {
        self.mob_storage_in_memory = true;
        self
    }

    /// Set the default LLM client (used for test stubs).
    pub fn default_llm_client(mut self, client: Arc<dyn LlmClient>) -> Self {
        self.default_llm_client = Some(client);
        self
    }

    /// Set an external `ContinuityStore` for the identity-first path.
    ///
    /// Mutually exclusive with `persistent_state()`.
    pub fn continuity_store(
        mut self,
        store: Arc<dyn crate::identity_first::contracts::ContinuityStore>,
    ) -> Self {
        self.continuity_store = Some(store);
        self
    }

    /// Set an external `LeaseProvider` for the identity-first path.
    ///
    /// Mutually exclusive with `persistent_state()`.
    pub fn lease_provider(
        mut self,
        provider: Arc<dyn crate::identity_first::contracts::LeaseProvider>,
    ) -> Self {
        self.lease_provider = Some(provider);
        self
    }

    /// Set a scratch directory for the external-authoritative path.
    ///
    /// Required when using `continuity_store()` + `lease_provider()`.
    pub fn scratch_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.scratch_dir = Some(path.into());
        self
    }

    /// Set an optional runtime store for durable dispatch.
    ///
    /// Without this, dispatch acknowledgment means in-memory only.
    /// Can be used with both `persistent_state()` and the external path.
    pub fn runtime_store(mut self, store: Arc<dyn meerkat::SessionStore>) -> Self {
        self.runtime_store = Some(store);
        self
    }

    /// Set an optional blob store for custom blob persistence.
    ///
    /// Can be used with both `persistent_state()` and the external path.
    pub fn blob_store(mut self, store: Arc<dyn meerkat_core::BlobStore>) -> Self {
        self.blob_store = Some(store);
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

    /// Force image-generation runtime substrate wiring.
    ///
    /// Definition-based builders also infer this from
    /// `profiles.<name>.tools.image_generation`; Meerkat owns the per-profile
    /// visibility decision. With Meerkat 0.6.1, profiles that opt into
    /// image generation must also leave `tools.builtins = true` because
    /// `generate_image` is registered by the builtin dispatcher.
    pub fn image_generation(mut self, enabled: bool) -> Self {
        self.capability_flags.image_generation = enabled;
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

    /// Install a persistent metadata store. Used for the structural-events
    /// subscription cursor — see `runtime::metadata::PersistentMetadataStore`.
    /// When unset, the builder defaults to an `InMemoryMetadataStore`, which
    /// is correct for in-memory mob deployments. Production gateways with a
    /// SQLite mob storage should pass `SqliteMetadataStore::open(path)`
    /// against the same database the mob uses, so the structural-events
    /// subscription can resume from the last-projected cursor on restart
    /// rather than jumping forward to "latest" and dropping events emitted
    /// between processes.
    pub fn persistent_metadata(mut self, store: Arc<dyn PersistentMetadataStore>) -> Self {
        self.persistent_metadata = Some(store);
        self
    }

    // -----------------------------------------------------------------------
    // Build
    // -----------------------------------------------------------------------

    pub async fn build(mut self) -> Result<UnifiedRuntime, UnifiedRuntimeBuilderError> {
        // --- Identity-first builder validation ---

        let has_persistent_state = self.persistent_state_path.is_some();
        let has_continuity_store = self.continuity_store.is_some();
        let has_lease_provider = self.lease_provider.is_some();
        let has_scratch_dir = self.scratch_dir.is_some();
        let has_any_external = has_continuity_store || has_lease_provider || has_scratch_dir;

        // REQ-23: persistent_state and explicit providers are mutually exclusive
        if has_persistent_state && has_any_external {
            return Err(UnifiedRuntimeBuilderError::ConflictingConfiguration(
                "persistent_state() and continuity_store()/lease_provider()/scratch_dir() \
                 are mutually exclusive — use one path or the other"
                    .to_string(),
            ));
        }

        // REQ-24: external path requires all three
        if has_any_external && !(has_continuity_store && has_lease_provider && has_scratch_dir) {
            let mut missing = Vec::new();
            if !has_continuity_store {
                missing.push("continuity_store");
            }
            if !has_lease_provider {
                missing.push("lease_provider");
            }
            if !has_scratch_dir {
                missing.push("scratch_dir");
            }
            return Err(UnifiedRuntimeBuilderError::ConflictingConfiguration(
                format!(
                    "external-authoritative path requires continuity_store() + lease_provider() + \
                     scratch_dir(); missing: {}",
                    missing.join(", ")
                ),
            ));
        }

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

        // The structural-events subscription cursor lives in the
        // persistent metadata adapter. For ephemeral builds this can be
        // in-memory (the ledger itself isn't durable, so there's nothing
        // to resume from). For persistent_state builds we MUST default
        // to SQLite — otherwise after a gateway restart the subscriber
        // resumes from `latest_cursor` and silently skips every event
        // that was written to the durable mob ledger while MobKit was
        // down. Callers can still override via `.persistent_metadata()`.
        let persistent_metadata: Arc<dyn PersistentMetadataStore> =
            if let Some(store) = self.persistent_metadata.clone() {
                store
            } else if let Some(state_path) = self.persistent_state_path.as_ref() {
                let metadata_path = state_path.join("mobkit_metadata.sqlite");
                Arc::new(SqliteMetadataStore::open(&metadata_path).map_err(|e| {
                    UnifiedRuntimeBuilderError::Io(format!(
                        "failed to open mobkit_metadata.sqlite at {}: {e}",
                        metadata_path.display()
                    ))
                })?)
            } else {
                Arc::new(InMemoryMetadataStore::new())
            };

        let runtime = UnifiedRuntime::bootstrap_with_options(
            mob_spec,
            module_config,
            self.module_agent_events,
            timeout,
            self.options,
            persistent_metadata,
        )
        .await
        .map_err(UnifiedRuntimeBuilderError::Bootstrap)?;

        // Construct session bridge from the mob handle for identity-first wiring.
        // Available for BOTH persistent_state and external-authoritative paths —
        // the bridge connects the identity-first control plane to real sessions.
        let session_bridge: Option<Arc<dyn crate::identity_first::bridge::SessionBridge>> = {
            let handle = runtime.mob_runtime.handle();
            let session_service = runtime.mob_runtime.session_service().cloned();
            let session_store = self.custom_session_store.clone();
            let bridge = if let (Some(store), Some(service)) =
                (session_store.clone(), session_service.clone())
            {
                crate::identity_first::bridge::MobSessionBridge::with_session_store_and_service(
                    handle, store, service,
                )
            } else if let Some(store) = session_store {
                crate::identity_first::bridge::MobSessionBridge::with_session_store(handle, store)
            } else if let Some(service) = session_service {
                crate::identity_first::bridge::MobSessionBridge::with_session_service(
                    handle, service,
                )
            } else {
                crate::identity_first::bridge::MobSessionBridge::new(handle)
            };
            Some(Arc::new(bridge))
        };

        // Set immutable outer fields by rebuilding the struct
        let runtime = UnifiedRuntime {
            post_spawn_hook: self.post_spawn_hook,
            post_reconcile_hook: self.post_reconcile_hook,
            error_hook: self.error_hook,
            drain_timeout: self.drain_timeout.unwrap_or(DEFAULT_DRAIN_TIMEOUT),
            discovery: self.discovery,
            edge_discovery: self.edge_discovery,
            contact_directory: self.contact_directory,
            session_bridge,
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
        let mut caps = self.capability_flags;
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
        caps.image_generation |=
            crate::mob_handle_runtime::mob_definition_may_use_image_generation(&definition);

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
            let mob_storage = if self.mob_storage_in_memory {
                MobStorage::in_memory()
            } else {
                MobStorage::persistent(state_path.join("mob.redb")).map_err(|e| {
                    UnifiedRuntimeBuilderError::Io(format!("failed to open redb mob storage: {e}"))
                })?
            };

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

            let mut spec = MobBootstrapSpec::ephemeral_runtime_backed_inner(
                definition,
                MobStorage::in_memory(),
                store_path,
                DEFAULT_MAX_SESSIONS,
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn definition_based_ephemeral_spec_provides_runtime_adapter() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "builder-ephemeral"

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.worker.tools]
comms = true
"#,
        )
        .expect("definition parses");

        let builder = UnifiedRuntimeBuilder::default().definition(definition);
        let spec = builder.resolve_mob_spec().await.expect("spec resolves");
        // Adapter is wired both via spec.runtime_adapter (explicit gateway
        // pattern) AND via the session service's runtime_store (so
        // archive/retire control ops succeed). Both ride a persistent
        // SqliteRuntimeStore at <store_path>/runtime.sqlite.
        assert!(
            spec.runtime_adapter.is_some(),
            "definition-based ephemeral specs must provide spec.runtime_adapter directly",
        );
        assert!(
            spec.session_service.runtime_adapter().is_some(),
            "session service must own a runtime_store so archive/retire control ops succeed",
        );
    }
}
