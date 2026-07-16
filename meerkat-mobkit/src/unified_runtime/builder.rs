//! Builder for constructing a configured UnifiedRuntime instance.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use meerkat_client::LlmClient;
use meerkat_mob::{MobDefinition, MobStorage, SpawnMemberSpec};

use crate::console_aggregator::{ConsoleLogStore, InMemoryConsoleLogStore, SqliteConsoleLogStore};
use crate::contact_directory::ContactDirectory;
pub use crate::identity_first::IdentityBootstrapMode;
use crate::identity_first::{
    AgentCustomizer, AgentMemoryConfig, AgentMemoryCustomizer, AgentMemoryProvider,
    AgentMemoryRuntimeInjector, AgentRuntimeServices, ContinuitySessionStoreAdapter,
    DurabilityPolicy, IdentityFirstRuntimeContext, IdentityRuntime, IdentityRuntimeConfig,
    LocalContinuityStore, LocalLeaseProvider, MarkdownAgentMemoryStore, RosterContext,
    RosterProvider, TopologyProvider,
};
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
    meerkat_config: Option<meerkat::Config>,
    default_llm_client: Option<Arc<dyn LlmClient>>,
    max_sessions: Option<usize>,
    capability_flags: CapabilityFlags,

    // --- Identity-first external path ---
    continuity_store: Option<Arc<dyn crate::identity_first::contracts::ContinuityStore>>,
    lease_provider: Option<Arc<dyn crate::identity_first::contracts::LeaseProvider>>,
    roster_provider: Option<Arc<dyn RosterProvider>>,
    topology_provider: Option<Arc<dyn TopologyProvider>>,
    agent_customizer: Option<Arc<dyn AgentCustomizer>>,
    agent_memory_provider: Option<Arc<dyn AgentMemoryProvider>>,
    agent_memory_config: Option<AgentMemoryConfig>,
    agent_memory_from_persistent_state: bool,
    agent_memory_engines: Option<crate::memory_wiring::MemoryEnginesConfig>,
    identity_bootstrap_mode: IdentityBootstrapMode,
    identity_runtime_instance_id: Option<String>,
    scratch_dir: Option<PathBuf>,
    blob_store: Option<Arc<dyn meerkat_core::BlobStore>>,
    console_log_store: Option<Arc<dyn ConsoleLogStore>>,

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
    persistent_metadata: Option<Arc<dyn PersistentMetadataStore>>,
    access_controller: Option<crate::access::AccessController>,
    topology_control_policy: crate::topology_control::TopologyControlPolicy,
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
    /// creates a `SqliteSessionStore`, runtime store, metadata store, console
    /// log store, and binary blob store under this directory. Mob storage stays
    /// in-memory. When not set, the builder uses an ephemeral session service
    /// with an auto-created temp directory.
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
    /// (provides durable sessions without local mob storage).
    pub fn session_store(mut self, store: Arc<dyn meerkat::SessionStore>) -> Self {
        self.custom_session_store = Some(store);
        self
    }

    /// Set the Meerkat agent factory configuration used by builder-created
    /// session services.
    ///
    /// Applications with long-lived durable coordinator agents can use this to
    /// tune session compaction and other factory-level Meerkat behavior without
    /// constructing a full `MobBootstrapSpec` by hand.
    pub fn meerkat_config(mut self, config: meerkat::Config) -> Self {
        self.meerkat_config = Some(config);
        self
    }

    /// Set the default LLM client (used for test stubs).
    pub fn default_llm_client(mut self, client: Arc<dyn LlmClient>) -> Self {
        self.default_llm_client = Some(client);
        self
    }

    /// Set the maximum number of active sessions for builder-created session
    /// services.
    ///
    /// This only applies to the definition-based path. A legacy `.mob_spec()`
    /// supplies its own already-built session service and capacity.
    pub fn max_sessions(mut self, max_sessions: usize) -> Self {
        self.max_sessions = Some(max_sessions);
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

    /// Set the desired identity roster provider for identity-first bootstrap.
    pub fn roster_provider(mut self, provider: Arc<dyn RosterProvider>) -> Self {
        self.roster_provider = Some(provider);
        self
    }

    /// Set the managed topology provider for identity-first bootstrap and refresh.
    pub fn topology_provider(mut self, provider: Arc<dyn TopologyProvider>) -> Self {
        self.topology_provider = Some(provider);
        self
    }

    /// Set the identity-first build customizer.
    pub fn agent_customizer(mut self, customizer: Arc<dyn AgentCustomizer>) -> Self {
        self.agent_customizer = Some(customizer);
        self
    }

    /// Enable identity-first agent memory injection using the provided memory provider.
    pub fn agent_memory(
        mut self,
        provider: Arc<dyn AgentMemoryProvider>,
        config: AgentMemoryConfig,
    ) -> Self {
        self.agent_memory_provider = Some(provider);
        self.agent_memory_config = Some(config);
        self
    }

    /// Enable identity-first agent memory using the bundled markdown store
    /// under `persistent_state()/agent-memory`.
    pub fn persistent_agent_memory(mut self, config: AgentMemoryConfig) -> Self {
        self.agent_memory_from_persistent_state = true;
        self.agent_memory_config = Some(config);
        self
    }

    /// Enable the FULL agent-memory stack (bundled SQLite store + the taint
    /// firewall + the enabled judgment-plane engines) — the same stack the
    /// rpc gateway assembles, reachable from the Rust builder (the OB3
    /// deployment shape). Requires `persistent_state()`; the store lives at
    /// `<persistent_state>/agent-memory-sqlite`.
    ///
    /// v1 boundaries (documented in `memory_wiring`): the Hygienist stays
    /// gateway-only; engines are driven by the member-event observe stream
    /// (their primary trigger path — the gateway's additional injector-side
    /// rotation hooks are not wired here yet); the steward dream runs on the
    /// in-process loop (no schedule host in library mode).
    pub fn persistent_agent_memory_stack(
        mut self,
        config: AgentMemoryConfig,
        engines: crate::memory_wiring::MemoryEnginesConfig,
    ) -> Self {
        self.agent_memory_config = Some(config);
        self.agent_memory_engines = Some(engines);
        self
    }

    fn composed_agent_customizer(
        &self,
        memory_provider: Option<Arc<dyn AgentMemoryProvider>>,
    ) -> Option<Arc<dyn AgentCustomizer>> {
        match memory_provider {
            Some(provider) => Some(Arc::new(AgentMemoryCustomizer::wrap(
                self.agent_customizer.clone(),
                provider,
                self.agent_memory_config.clone().unwrap_or_default(),
            ))),
            None => self.agent_customizer.clone(),
        }
    }

    /// Set how identity-first durable agents are materialized during build.
    pub fn identity_bootstrap_mode(mut self, mode: IdentityBootstrapMode) -> Self {
        self.identity_bootstrap_mode = mode;
        self
    }

    /// Set the identity runtime instance id used when acquiring leases.
    pub fn identity_runtime_instance_id(mut self, id: impl Into<String>) -> Self {
        self.identity_runtime_instance_id = Some(id.into());
        self
    }

    /// Set a scratch directory for the external-authoritative path.
    ///
    /// Required when using `continuity_store()` + `lease_provider()`.
    pub fn scratch_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.scratch_dir = Some(path.into());
        self
    }

    /// Set an optional blob store for custom blob persistence.
    ///
    /// The same store is used for runtime image blobs and for MobKit's
    /// `/blobs/{id}` and `mobkit/blob/*` serving/upload paths.
    pub fn blob_store(mut self, store: Arc<dyn meerkat_core::BlobStore>) -> Self {
        self.blob_store = Some(store);
        self
    }

    /// Set a custom console log store.
    ///
    /// This lets applications pair a durable console timeline (for fast
    /// cursor-based history replay) with otherwise ephemeral mob state.
    pub fn with_console_log_store(mut self, store: Arc<dyn ConsoleLogStore>) -> Self {
        self.console_log_store = Some(store);
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
    /// visibility decision.
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

    /// Install a pre-built access controller (ABAC enforcement for the
    /// console and SSE surfaces). Absent — the default — access control is
    /// off and every surface behaves exactly as before.
    pub fn access_controller(mut self, controller: crate::access::AccessController) -> Self {
        self.access_controller = Some(controller);
        self
    }

    /// Enable access control backed by a TOML file (conventionally
    /// `config/access.toml`). A missing file starts disabled; admin edits
    /// from the console persist back to the same path.
    pub fn access_control_file(
        mut self,
        path: impl Into<PathBuf>,
    ) -> Result<Self, crate::access::AccessConfigError> {
        self.access_controller = Some(crate::access::AccessController::load_or_default(path)?);
        Ok(self)
    }

    /// Configure the optional topology control plane. The default policy is
    /// disabled, single-operation, local-authority only.
    pub fn topology_control(
        mut self,
        policy: crate::topology_control::TopologyControlPolicy,
    ) -> Result<Self, crate::topology_control::TopologyControlError> {
        policy.validate()?;
        self.topology_control_policy = policy;
        Ok(self)
    }

    // -----------------------------------------------------------------------
    // Build
    // -----------------------------------------------------------------------

    pub async fn build(mut self) -> Result<UnifiedRuntime, UnifiedRuntimeBuilderError> {
        // --- Identity-first builder validation ---

        self.identity_bootstrap_mode
            .validate()
            .map_err(UnifiedRuntimeBuilderError::ConflictingConfiguration)?;

        let has_persistent_state = self.persistent_state_path.is_some();
        let has_continuity_store = self.continuity_store.is_some();
        let has_lease_provider = self.lease_provider.is_some();
        let has_roster_provider = self.roster_provider.is_some();
        let has_topology_provider = self.topology_provider.is_some();
        // A2 decouple: agent memory is keyed by AgentIdentity, which every
        // mob member already has, so enabling it must NOT pull in the
        // identity-first orchestration layer (roster/continuity/leases).
        // Without a roster the BASIC memory surface (recorder tool +
        // build-time injection + panel store) rides the classic path via a
        // `MemorySpawnCustomizer`; with a roster, memory composes into the
        // IdentityRuntime customizer exactly as before (advanced lifecycle
        // features included).
        let has_agent_customizer = self.agent_customizer.is_some();
        let has_identity_runtime_instance_id = self.identity_runtime_instance_id.is_some();
        let has_scratch_dir = self.scratch_dir.is_some();
        let has_external_identity_storage =
            has_continuity_store || has_lease_provider || has_scratch_dir;
        let wants_identity_first = has_external_identity_storage
            || has_roster_provider
            || has_topology_provider
            || has_agent_customizer
            || has_identity_runtime_instance_id;
        if self.agent_memory_provider.is_some() && self.agent_memory_from_persistent_state {
            return Err(UnifiedRuntimeBuilderError::ConflictingConfiguration(
                "agent_memory() and persistent_agent_memory() are mutually exclusive".to_string(),
            ));
        }

        if self.agent_memory_from_persistent_state && !has_persistent_state {
            return Err(UnifiedRuntimeBuilderError::ConflictingConfiguration(
                "persistent_agent_memory() requires persistent_state()".to_string(),
            ));
        }

        // REQ-23: persistent_state and explicit continuity/lease/scratch
        // providers are mutually exclusive. Roster/topology/customizers are
        // identity inputs and can use the bundled persistent identity store.
        if has_persistent_state && has_external_identity_storage {
            return Err(UnifiedRuntimeBuilderError::ConflictingConfiguration(
                "persistent_state() and identity-first continuity_store()/lease_provider()/scratch_dir() setters \
                 are mutually exclusive — use one storage authority"
                    .to_string(),
            ));
        }

        if has_persistent_state && wants_identity_first && !has_roster_provider {
            return Err(UnifiedRuntimeBuilderError::ConflictingConfiguration(
                "persistent_state() identity-first path requires roster_provider()".to_string(),
            ));
        }

        // REQ-24: external path requires all three storage inputs plus a roster.
        if !has_persistent_state
            && wants_identity_first
            && !(has_continuity_store
                && has_lease_provider
                && has_roster_provider
                && has_scratch_dir)
        {
            let mut missing = Vec::new();
            if !has_continuity_store {
                missing.push("continuity_store");
            }
            if !has_lease_provider {
                missing.push("lease_provider");
            }
            if !has_roster_provider {
                missing.push("roster_provider");
            }
            if !has_scratch_dir {
                missing.push("scratch_dir");
            }
            return Err(UnifiedRuntimeBuilderError::ConflictingConfiguration(
                format!(
                    "identity-first path requires continuity_store() + lease_provider() + \
                     roster_provider() + scratch_dir(); missing: {}",
                    missing.join(", ")
                ),
            ));
        }

        let continuity_session_store = self
            .continuity_store
            .as_ref()
            .map(|store| Arc::new(ContinuitySessionStoreAdapter::new(store.clone())));
        if let Some(store) = continuity_session_store.as_ref() {
            self.custom_session_store = Some(store.clone());
        }

        // Legacy mob_spec path takes precedence — must be consumed before
        // resolve_mob_spec (which borrows &self for the definition path).
        let mut mob_spec = match self.mob_spec.take() {
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

        let module_config = self.module_config.take().unwrap_or_else(|| MobKitConfig {
            modules: Vec::new(),
            discovery: crate::types::DiscoverySpec {
                namespace: String::new(),
                modules: Vec::new(),
            },
            pre_spawn: Vec::new(),
        });
        let timeout = self.timeout.unwrap_or(DEFAULT_TIMEOUT);
        if let Some(state_path) = self.persistent_state_path.as_ref() {
            std::fs::create_dir_all(state_path).map_err(|e| {
                UnifiedRuntimeBuilderError::Io(format!(
                    "failed to create state directory at {}: {e}",
                    state_path.display()
                ))
            })?;
        }
        let persistent_agent_memory_provider: Option<Arc<dyn AgentMemoryProvider>> =
            if self.agent_memory_from_persistent_state {
                let Some(state_path) = self.persistent_state_path.as_ref() else {
                    return Err(UnifiedRuntimeBuilderError::ConflictingConfiguration(
                        "persistent_agent_memory() requires persistent_state()".to_string(),
                    ));
                };
                let memory_path = state_path.join("agent-memory");
                Some(Arc::new(
                    MarkdownAgentMemoryStore::open(&memory_path).map_err(|e| {
                        UnifiedRuntimeBuilderError::Io(format!(
                            "failed to open agent memory store at {}: {e}",
                            memory_path.display()
                        ))
                    })?,
                ))
            } else {
                None
            };
        // Full-stack path: the bundled SQLite store is opened pre-runtime so
        // it can serve as the provider (recorder + recall) from the first
        // spawn; the firewall + engines attach post-construction, when the
        // memory event sink and mob handle exist.
        let stack_sqlite_store = if self.agent_memory_engines.is_some() {
            let Some(state_path) = self.persistent_state_path.as_ref() else {
                return Err(UnifiedRuntimeBuilderError::ConflictingConfiguration(
                    "persistent_agent_memory_stack() requires persistent_state()".to_string(),
                ));
            };
            let memory_path = state_path.join("agent-memory-sqlite");
            Some(
                crate::memory::sqlite_store::SqliteAgentMemoryStore::open(&memory_path).map_err(
                    |e| {
                        UnifiedRuntimeBuilderError::Io(format!(
                            "failed to open agent memory store at {}: {e}",
                            memory_path.display()
                        ))
                    },
                )?,
            )
        } else {
            None
        };
        let agent_memory_provider = self
            .agent_memory_provider
            .clone()
            .or_else(|| {
                stack_sqlite_store
                    .clone()
                    .map(|store| Arc::new(store) as Arc<dyn AgentMemoryProvider>)
            })
            .or(persistent_agent_memory_provider);
        let agent_memory_injector = agent_memory_provider.as_ref().map(|provider| {
            AgentMemoryRuntimeInjector::new(
                provider.clone(),
                self.agent_memory_config.clone().unwrap_or_default(),
            )
        });
        let agent_customizer = self.composed_agent_customizer(agent_memory_provider.clone());

        // Classic (roster-less) agent memory: register the per-spawn memory
        // customizer on the mob runtime itself, so every member spawn —
        // consumer, agent-tool, policy, respawn, resume — gets the recorder
        // tool and the build-time injection keyed on its AgentIdentity. The
        // identity-first path keeps its AgentCustomizer instead (composing
        // both would double-inject on identity-first materializations).
        let classic_agent_memory = if wants_identity_first {
            None
        } else {
            agent_memory_provider.clone()
        };
        if let Some(provider) = classic_agent_memory.as_ref() {
            mob_spec.spawn_member_customizer =
                Some(Arc::new(crate::memory::MemorySpawnCustomizer::new(
                    provider.clone(),
                    self.agent_memory_config.clone().unwrap_or_default(),
                )));
        }

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
        let console_log_store: Arc<dyn ConsoleLogStore> =
            if let Some(store) = self.console_log_store.clone() {
                store
            } else if let Some(state_path) = self.persistent_state_path.as_ref() {
                let console_log_path = state_path.join("mobkit_console.sqlite");
                Arc::new(SqliteConsoleLogStore::open(&console_log_path).map_err(|e| {
                    UnifiedRuntimeBuilderError::Io(format!(
                        "failed to open mobkit_console.sqlite at {}: {e}",
                        console_log_path.display()
                    ))
                })?)
            } else {
                Arc::new(InMemoryConsoleLogStore::new())
            };
        let runtime = Box::pin(UnifiedRuntime::bootstrap_with_options(
            mob_spec,
            module_config,
            self.module_agent_events,
            timeout,
            self.options,
            persistent_metadata,
        ))
        .await
        .map_err(UnifiedRuntimeBuilderError::Bootstrap)?;

        // Construct session bridge from the mob handle for identity-first wiring.
        // Available for BOTH persistent_state and external-authoritative paths —
        // the bridge connects the identity-first control plane to real sessions.
        let session_bridge: Option<Arc<dyn crate::identity_first::bridge::SessionBridge>> = {
            let handle = runtime.mob_runtime.handle();
            let session_service = runtime.mob_runtime.session_service().cloned();
            let session_store = self.custom_session_store.clone();
            let bridge: Arc<dyn crate::identity_first::bridge::SessionBridge> =
                if let Some(store) = continuity_session_store.clone() {
                    Arc::new(
                    crate::identity_first::bridge::MobSessionBridge::with_continuity_session_store(
                        handle,
                        store,
                        session_service,
                    ),
                )
                } else if let (Some(store), Some(service)) =
                    (session_store.clone(), session_service.clone())
                {
                    Arc::new(
                    crate::identity_first::bridge::MobSessionBridge::with_session_store_and_service(
                        handle, store, service,
                    ),
                )
                } else if let Some(store) = session_store {
                    Arc::new(
                        crate::identity_first::bridge::MobSessionBridge::with_session_store(
                            handle, store,
                        ),
                    )
                } else if let Some(service) = session_service {
                    Arc::new(
                        crate::identity_first::bridge::MobSessionBridge::with_session_service(
                            handle, service,
                        ),
                    )
                } else {
                    Arc::new(crate::identity_first::bridge::MobSessionBridge::new(handle))
                };
            Some(bridge)
        };

        // Construct the durable control-plane authority before identity
        // restore. TopologyProvider output is only a declaration; the
        // IdentityRuntime composes this controller's additions/suppressions
        // under the same admission lock before it touches peer wiring.
        let topology_controller = if let Some(state_path) = self.persistent_state_path.as_ref() {
            crate::topology_control::TopologyController::load_or_default(
                self.topology_control_policy.clone(),
                state_path.join("topology-control.json"),
            )
            .map_err(|error| UnifiedRuntimeBuilderError::Io(error.to_string()))?
        } else {
            crate::topology_control::TopologyController::new(self.topology_control_policy.clone())
                .map_err(|error| UnifiedRuntimeBuilderError::Io(error.to_string()))?
        };
        topology_controller
            .bind_authority(runtime.mob_id())
            .await
            .map_err(|error| UnifiedRuntimeBuilderError::Io(error.to_string()))?;

        let (identity_first_context, identity_roster_specs) = if wants_identity_first {
            let (continuity_store, lease_provider): (
                Arc<dyn crate::identity_first::contracts::ContinuityStore>,
                Arc<dyn crate::identity_first::contracts::LeaseProvider>,
            ) = if let Some(state_path) = self.persistent_state_path.as_ref() {
                let continuity_path = state_path.join("identity_continuity.sqlite");
                let (local_store, high_water) =
                    LocalContinuityStore::open_with_fencing_floor(continuity_path.clone())
                        .await
                        .map_err(|e| {
                            UnifiedRuntimeBuilderError::Io(format!(
                                "failed to open identity_continuity.sqlite and read its fencing high-water at {}: {e}",
                                continuity_path.display()
                            ))
                        })?;
                (
                    Arc::new(local_store),
                    Arc::new(LocalLeaseProvider::with_floor(high_water)),
                )
            } else {
                let Some(continuity_store) = self.continuity_store.clone() else {
                    return Err(UnifiedRuntimeBuilderError::Bootstrap(
                        UnifiedRuntimeBootstrapError::IdentityFirst(
                            "identity-first validation requires continuity_store".to_string(),
                        ),
                    ));
                };
                let Some(lease_provider) = self.lease_provider.clone() else {
                    return Err(UnifiedRuntimeBuilderError::Bootstrap(
                        UnifiedRuntimeBootstrapError::IdentityFirst(
                            "identity-first validation requires lease_provider".to_string(),
                        ),
                    ));
                };
                (continuity_store, lease_provider)
            };
            let Some(roster_provider) = self.roster_provider.clone() else {
                return Err(UnifiedRuntimeBuilderError::Bootstrap(
                    UnifiedRuntimeBootstrapError::IdentityFirst(
                        "identity-first validation requires roster_provider".to_string(),
                    ),
                ));
            };
            let bridge = session_bridge.clone();
            let identity_runtime = Arc::new(
                IdentityRuntime::new(IdentityRuntimeConfig {
                    continuity_store,
                    lease_provider,
                    runtime_instance_id: self
                        .identity_runtime_instance_id
                        .clone()
                        .unwrap_or_else(|| format!("mobkit-{}", std::process::id())),
                    has_runtime_store: true,
                    durability_policy: DurabilityPolicy::SyncWriteThrough,
                    bridge,
                    default_timeout: None,
                })
                .with_runtime_services(AgentRuntimeServices::new(runtime.mob_runtime.handle()))
                .with_reset_roster_provider_context(
                    roster_provider.clone(),
                    Some(runtime.mob_runtime.handle().definition().clone()),
                ),
            );
            identity_runtime
                .set_agent_customizer(agent_customizer.clone())
                .await;
            identity_runtime
                .set_agent_memory(agent_memory_injector.clone())
                .await;
            identity_runtime.set_error_hook(self.error_hook.clone());
            identity_runtime.set_topology_controller(topology_controller.clone());

            let roster_specs = roster_provider
                .roster(&RosterContext {
                    mob_definition: Some(runtime.mob_runtime.handle().definition().clone()),
                    previous_identities: Vec::new(),
                })
                .await
                .map_err(|err| {
                    UnifiedRuntimeBuilderError::Bootstrap(
                        UnifiedRuntimeBootstrapError::IdentityFirst(format!(
                            "roster provider failed: {err}"
                        )),
                    )
                })?;

            let identity_context = Arc::new(IdentityFirstRuntimeContext::new_with_bootstrap_mode(
                identity_runtime,
                roster_provider,
                self.topology_provider.clone(),
                agent_customizer.clone(),
                Some(runtime.mob_runtime.handle().definition().clone()),
                self.identity_bootstrap_mode.clone(),
            ));
            (Some(identity_context), Some(roster_specs))
        } else {
            (None, None)
        };
        if let Some(context) = identity_first_context.as_ref() {
            runtime
                .mob_runtime
                .install_identity_runtime_authority(Arc::clone(&context.runtime));
            *runtime
                .implicit_delegate_identity_runtime
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(Arc::clone(&context.runtime));
        }

        // Set immutable outer fields by rebuilding the struct
        let runtime = UnifiedRuntime {
            access_controller: self.access_controller,
            topology_controller,
            post_spawn_hook: self.post_spawn_hook,
            post_reconcile_hook: self.post_reconcile_hook,
            error_hook: self.error_hook,
            drain_timeout: self.drain_timeout.unwrap_or(DEFAULT_DRAIN_TIMEOUT),
            discovery: self.discovery,
            // A custom embedder policy overrides the definition-derived
            // default `bootstrap_with_options` installed (HomeCore,
            // 2026-07-09); with none supplied the default is preserved.
            edge_discovery: self
                .edge_discovery
                .map(std::sync::Arc::<dyn EdgeDiscovery>::from)
                .or(runtime.edge_discovery),
            contact_directory: self.contact_directory,
            session_bridge,
            identity_first_context,
            identity_lease_renewal_task: tokio::sync::Mutex::new(None),
            identity_continuity_repair_task: tokio::sync::Mutex::new(None),
            console_log_store,
            ..runtime
        };

        // Run the host's last fallible pre-bootstrap hook before identity
        // hydration can start any runtime-owned background work. In
        // particular, LazyWithBackgroundWarm spawns materialization from
        // bootstrap_roster; a later hook failure must not detach that task or
        // leave its leases alive after build() returns an error.
        let pre_spawn_context = if let Some(hook) = self.pre_spawn_hook {
            hook().await.map_err(|err| {
                UnifiedRuntimeBuilderError::Bootstrap(UnifiedRuntimeBootstrapError::PreSpawnHook(
                    err.to_string(),
                ))
            })?
        } else {
            serde_json::Value::Null
        };

        // Classic-path bundled-store wiring: the console Memory panel (§9.3),
        // the §10.1 posture write gate (only when the embedder did not
        // install a taint-tracking gate already), and the §9.3 timeline sink
        // for quarantined writes. Providers other than the bundled SQLite
        // store keep injection + recorder without a panel.
        if let Some(store) = classic_agent_memory
            .as_ref()
            .and_then(|provider| provider.as_sqlite_store())
        {
            let llm_writes = self
                .agent_memory_config
                .as_ref()
                .map(|config| config.llm_writes)
                .unwrap_or_default();
            store.set_llm_write_gate_if_absent(Arc::new(
                crate::memory::taint::TaintLlmWriteGate::new(None, llm_writes),
            ));
            store.set_event_sink_if_absent(runtime.memory_event_sink());
            runtime.set_memory_panel_store(store.clone());
        }

        // Full-stack path (persistent_agent_memory_stack): firewall + engines
        // + observer over the pre-opened SQLite store.
        if let (Some(store), Some(engines)) =
            (stack_sqlite_store, self.agent_memory_engines.as_ref())
        {
            let config = self.agent_memory_config.clone().unwrap_or_default();
            let persistent_state = self.persistent_state_path.clone();
            let transcript_store: Option<Arc<dyn meerkat::SessionStore>> =
                if engines.distiller.enabled || engines.steward.enabled {
                    let state = persistent_state.as_ref().ok_or_else(|| {
                        UnifiedRuntimeBuilderError::ConflictingConfiguration(
                            "agent memory engines require persistent_state()".to_string(),
                        )
                    })?;
                    Some(Arc::new(
                        meerkat_store::SqliteSessionStore::open(state.join("sessions.db"))
                            .map_err(|e| {
                                UnifiedRuntimeBuilderError::Io(format!(
                                    "agent memory session store: {e}"
                                ))
                            })?,
                    ))
                } else {
                    None
                };
            let stack = crate::memory_wiring::attach_memory_engines(
                store,
                &config,
                engines,
                crate::memory_wiring::MemoryStackSeams {
                    persistent_state,
                    transcript_store,
                    event_sink: Some(runtime.memory_event_sink()),
                    ..Default::default()
                },
            )
            .map_err(UnifiedRuntimeBuilderError::Io)?;
            runtime.set_memory_panel_store(stack.store.clone());
            // Runtime ownership makes both infinite supervisors visible to
            // normal shutdown and to the identity-bootstrap failure cleanup
            // path below. Leaking either handle here would leave ghost memory
            // work behind after build() returned Err.
            *runtime.agent_memory_observer_task.lock().await = Some(
                crate::spawn_member_event_observer(runtime.mob_handle(), stack.sinks),
            );
            if let Some(steward) = stack.steward.as_ref() {
                // Library mode has no schedule host; the guarded interval
                // loop drives dreams.
                *runtime.agent_memory_steward_task.lock().await = Some(steward.spawn_dream_loop());
            }
            tracing::info!(
                distiller = stack.distiller.is_some(),
                steward = stack.steward.is_some(),
                "agent memory stack installed (builder path)"
            );
        }

        // All fallible builder-only setup is complete. Identity bootstrap may
        // now launch background warming; if bootstrap itself fails, drive the
        // fully assembled runtime through the same cooperative shutdown path
        // used by embedders so partially acquired authority is not detached.
        if let (Some(context), Some(roster_specs)) = (
            runtime.identity_first_context.as_ref(),
            identity_roster_specs.as_ref(),
        ) {
            if let Err(err) = context.bootstrap_roster(roster_specs).await {
                let build_error = UnifiedRuntimeBuilderError::Bootstrap(
                    UnifiedRuntimeBootstrapError::IdentityFirst(format!(
                        "identity bootstrap failed: {err}"
                    )),
                );
                runtime.shutdown().await;
                return Err(build_error);
            }

            *runtime.identity_lease_renewal_task.lock().await =
                Some(context.runtime.clone().spawn_tracked_lease_renewal_task());
            *runtime.identity_continuity_repair_task.lock().await = Some(
                context
                    .clone()
                    .spawn_tracked_broken_identity_repair_task(Default::default()),
            );
        }

        if runtime.identity_first_context.is_none()
            && let Some(ref discovery) = runtime.discovery
        {
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
        if runtime.edge_discovery.is_some()
            || runtime.topology_controller.revision().await > 0
            || runtime.topology_controller.has_pending().await
        {
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
        let max_sessions = self.max_sessions.unwrap_or(DEFAULT_MAX_SESSIONS);
        if max_sessions == 0 {
            return Err(UnifiedRuntimeBuilderError::ConflictingConfiguration(
                "max_sessions() must be greater than 0".to_string(),
            ));
        }

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

        // Note: blocking I/O (fs, SQLite) — acceptable at startup.
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
            let mob_storage = MobStorage::in_memory();

            MobBootstrapSpec::persistent_inner(
                definition,
                mob_storage,
                state_path.clone(),
                max_sessions,
                session_store,
                self.blob_store.clone(),
                hook,
                caps,
                after_hook.clone(),
                self.meerkat_config.clone(),
            )
        } else if let Some(ref scratch_dir) = self.scratch_dir {
            std::fs::create_dir_all(scratch_dir).map_err(|e| {
                UnifiedRuntimeBuilderError::Io(format!(
                    "failed to create scratch directory at {}: {e}",
                    scratch_dir.display()
                ))
            })?;

            MobBootstrapSpec::ephemeral_runtime_backed_inner(
                definition,
                MobStorage::in_memory(),
                scratch_dir.clone(),
                max_sessions,
                self.custom_session_store.clone(),
                self.blob_store.clone(),
                hook,
                caps,
                after_hook,
                self.meerkat_config.clone(),
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
                max_sessions,
                self.custom_session_store.clone(),
                self.blob_store.clone(),
                hook,
                caps,
                after_hook,
                self.meerkat_config.clone(),
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
    use meerkat_core::service::{
        CreateSessionRequest, DeferredPromptPolicy, InitialTurnPolicy, SessionService,
    };

    fn deferred_capacity_request(prompt: impl Into<String>) -> CreateSessionRequest {
        let build = meerkat_core::service::SessionBuildOptions {
            llm_client_override: Some(meerkat::encode_llm_client_override_for_service(Arc::new(
                meerkat_client::TestClient::default(),
            ))),
            ..Default::default()
        };

        CreateSessionRequest {
            model: "gpt-5.5".to_string(),
            prompt: meerkat_core::ContentInput::Text(prompt.into()),
            system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            initial_turn: InitialTurnPolicy::Defer,
            deferred_prompt_policy: DeferredPromptPolicy::Discard,
            build: Some(build),
            labels: None,
            injected_context: Vec::new(),
        }
    }

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
        assert!(
            spec.runtime_adapter.is_some(),
            "definition-based ephemeral specs should expose runtime authority",
        );
    }

    #[tokio::test]
    async fn definition_based_ephemeral_spec_uses_configured_max_sessions() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "builder-max-sessions"

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "autonomous_host"
"#,
        )
        .expect("definition parses");

        let builder = UnifiedRuntimeBuilder::default()
            .definition(definition)
            .max_sessions(65);
        let spec = builder.resolve_mob_spec().await.expect("spec resolves");

        for index in 0..65 {
            SessionService::create_session(
                spec.session_service.as_ref(),
                deferred_capacity_request(format!("session {index}")),
            )
            .await
            .expect("configured capacity should admit session");
        }

        let blocked = SessionService::create_session(
            spec.session_service.as_ref(),
            deferred_capacity_request("one too many"),
        )
        .await
        .expect_err("configured capacity should block the next session");
        assert!(
            blocked.to_string().contains("Max sessions reached (65/65)"),
            "unexpected capacity error: {blocked}",
        );
    }

    #[tokio::test]
    async fn definition_based_spec_accepts_custom_meerkat_config() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "builder-custom-config"

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "autonomous_host"
"#,
        )
        .expect("definition parses");
        let mut config = meerkat::Config::default();
        config.compaction.auto_compact_threshold = 42_000;
        config.compaction.auto_compact_threshold_explicit = true;
        config.compaction.recent_turn_budget = 2;

        let builder = UnifiedRuntimeBuilder::default()
            .definition(definition)
            .meerkat_config(config)
            .max_sessions(1);
        let spec = builder.resolve_mob_spec().await.expect("spec resolves");

        SessionService::create_session(
            spec.session_service.as_ref(),
            deferred_capacity_request("custom config session"),
        )
        .await
        .expect("custom Meerkat config should still build a usable session service");
    }

    #[tokio::test]
    async fn definition_based_persistent_spec_uses_configured_max_sessions() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "builder-persistent-max-sessions"

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "autonomous_host"
"#,
        )
        .expect("definition parses");
        let tmp = tempfile::tempdir().expect("temp dir");

        let builder = UnifiedRuntimeBuilder::default()
            .definition(definition)
            .persistent_state(tmp.path().join("state"))
            .max_sessions(2);
        let spec = builder.resolve_mob_spec().await.expect("spec resolves");

        for index in 0..2 {
            SessionService::create_session(
                spec.session_service.as_ref(),
                deferred_capacity_request(format!("persistent session {index}")),
            )
            .await
            .expect("configured persistent capacity should admit session");
        }

        let blocked = SessionService::create_session(
            spec.session_service.as_ref(),
            deferred_capacity_request("persistent one too many"),
        )
        .await
        .expect_err("configured persistent capacity should block the next session");
        assert!(
            blocked.to_string().contains("Max sessions reached (2/2)"),
            "unexpected capacity error: {blocked}",
        );
    }

    #[tokio::test]
    async fn definition_based_spec_rejects_zero_max_sessions() {
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "builder-zero-max-sessions"

[profiles.worker]
model = "gpt-5.5"
"#,
        )
        .expect("definition parses");

        let result = UnifiedRuntimeBuilder::default()
            .definition(definition)
            .max_sessions(0)
            .resolve_mob_spec()
            .await;
        assert!(result.is_err(), "zero max sessions should be rejected");
        let err = result.err().expect("zero max sessions error");

        assert!(
            err.to_string().contains("max_sessions"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn builder_accepts_custom_console_log_store_for_ephemeral_mob_state() {
        let store: Arc<dyn ConsoleLogStore> = Arc::new(InMemoryConsoleLogStore::new());
        let builder = UnifiedRuntimeBuilder::default().with_console_log_store(store.clone());

        assert!(
            Arc::ptr_eq(
                builder.console_log_store.as_ref().expect("custom store"),
                &store
            ),
            "builder should retain the exact console log store supplied by the app"
        );
    }
}
