//! Unified runtime — combines mob lifecycle, module management, and operational subsystems.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use futures::stream::{BoxStream, SelectAll, StreamExt};
use meerkat_core::comms::EventStream;
use meerkat_core::event::{AgentEvent, agent_event_type};
use meerkat_mob::{
    AgentIdentity, AgentRuntimeId, AttributedEvent, FenceToken, MobError, MobHandle,
    MobMemberStatus, ProfileName, SpawnMemberSpec,
};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

pub(crate) use self::console_events::ConsoleEventStore;
use self::mob_events::MobEventsStore;
use crate::console_aggregator::{ConsoleLogStore, InMemoryConsoleLogStore};
use crate::mob_handle_runtime::{MobBootstrapSpec, MobRuntime, MobRuntimeError};
use crate::runtime::{
    InMemoryMetadataStore, MetadataScope, MobkitRuntimeHandle, PersistentMetadataStore,
    RuntimeMetadataTable, RuntimeOptions, start_mobkit_runtime_with_options,
};
use crate::types::{
    AgentDiscoverySpec, EventEnvelope, MobKitConfig, MobStructuralEventEnvelope, UnifiedEvent,
};

pub mod builder;
pub(crate) mod console_events;
pub mod cross_mob;
pub mod edge_reconcile;
pub mod edge_types;
pub mod event_log;
pub mod http;
pub(crate) mod implicit_delegate_retirement;
pub mod lifecycle;
pub mod mob_events;
pub mod mob_ops;
pub mod module_ops;
pub mod types;

pub use crate::identity_first::IdentityBootstrapMode;
pub use builder::UnifiedRuntimeBuilder;
pub use edge_types::{
    DesiredPeerEdge, DesiredPeerEdgeError, Discovery, EdgeDiscovery, EdgeReconcileFailure,
    PreSpawnContext, PreSpawnHook,
};
pub use event_log::{
    EventLogConfig, EventLogError, EventLogStore, EventQuery, NullEventLogStore, PersistedEvent,
};
pub use http::DEFAULT_REFERENCE_APP_MAX_CONCURRENT_REQUESTS;
pub use mob_ops::MemberTurnAdmission;
pub use types::{
    ErrorEvent, IdentityAuthorityReleaseOutcome, RediscoverReport, ShutdownDrainReport,
    UnifiedRuntimeBootstrapError, UnifiedRuntimeBuilderError, UnifiedRuntimeBuilderField,
    UnifiedRuntimeError, UnifiedRuntimeReconcileEdgesReport, UnifiedRuntimeReconcileError,
    UnifiedRuntimeReconcileReport, UnifiedRuntimeReconcileRoutingReport, UnifiedRuntimeRunReport,
    UnifiedRuntimeShutdownReport,
};

/// Called after members are spawned. Receives the list of spawned member IDs.
pub type PostSpawnHook =
    Arc<dyn Fn(Vec<String>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Called after reconcile completes. Receives the reconcile report.
pub type PostReconcileHook = Arc<
    dyn Fn(UnifiedRuntimeReconcileReport) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

/// Called when a runtime operation fails. Fire-and-forget — the hook's
/// result is not checked and a failing hook cannot break the runtime.
pub type ErrorHook =
    Arc<dyn Fn(ErrorEvent) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

const ROSTER_ROUTE_PREFIX: &str = "mob.member.";
const ROSTER_ROUTE_CHANNEL: &str = "notification";
const ROSTER_ROUTE_SINK: &str = "mob_member";
const ROSTER_ROUTE_TARGET_MODULE: &str = "delivery";

const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Map an [`AgentDiscoverySpec`] to a [`SpawnMemberSpec`] for spawning.
///
/// `additional_instructions` maps directly to `SpawnMemberSpec.additional_instructions`,
/// which flows through Meerkat's build pipeline to `AgentBuildConfig.additional_instructions`.
pub fn discovery_spec_to_spawn_spec(spec: &AgentDiscoverySpec) -> SpawnMemberSpec {
    let resume_session_id = spec
        .resume_session_id
        .as_deref()
        .and_then(|s| meerkat_core::types::SessionId::parse(s).ok());
    let additional_instructions = if spec.additional_instructions.is_empty() {
        None
    } else {
        Some(spec.additional_instructions.clone())
    };
    let mut spawn = SpawnMemberSpec::new(
        meerkat_mob::ProfileName::from(spec.profile.as_str()),
        // The spec stays in the public alias space: the hook-aware
        // `UnifiedRuntime::spawn`/`spawn_many` own the encode to the
        // comms-safe roster id (meerkat 0.7 MemberCommsName), and the encode
        // is deliberately not idempotent (`mk--` is a reserved marker), so
        // encoding here too would double-encode `:`-bearing identities.
        meerkat_mob::ids::AgentIdentity::from(spec.meerkat_id.as_str()),
    );
    if let Some(context) = spec.context.clone() {
        spawn = spawn.with_context(context);
    }
    if let Some(labels) = spec.labels.clone() {
        spawn = spawn.with_labels(labels);
    }
    if let Some(sid) = resume_session_id {
        spawn = spawn.with_resume_bridge_session_id(sid);
    }
    if let Some(instructions) = additional_instructions {
        spawn = spawn.with_additional_instructions(instructions);
    }
    spawn
}

pub struct UnifiedRuntime {
    // Immutable after construction — &self access
    mob_runtime: MobRuntime,
    post_spawn_hook: Option<PostSpawnHook>,
    post_reconcile_hook: Option<PostReconcileHook>,
    error_hook: Option<ErrorHook>,
    drain_timeout: Duration,
    discovery: Option<Box<dyn Discovery>>,
    edge_discovery: Option<Arc<dyn EdgeDiscovery>>,

    // Fine-grained interior mutability
    module_runtime: Arc<tokio::sync::Mutex<MobkitRuntimeHandle>>,
    managed_dynamic_edges: Arc<tokio::sync::RwLock<BTreeSet<(String, String)>>>,
    shutting_down: AtomicBool,
    mob_event_ingress: tokio::sync::Mutex<Option<MobEventIngress>>,
    bootstrap_edges_report: tokio::sync::RwLock<Option<UnifiedRuntimeReconcileEdgesReport>>,
    event_log: Option<event_log::EventLogHandle>,
    console_log_store: Arc<dyn ConsoleLogStore>,
    console_events: ConsoleEventStore,
    mob_events: MobEventsStore,
    mob_events_subscriber_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    implicit_delegate_retirement_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    /// Late-bound identity authority observed by the already-running idle
    /// retirement sweeper. Gateways attach identity-first after base runtime
    /// bootstrap, so capturing `identity_runtime()` when the task starts would
    /// permanently capture `None`.
    implicit_delegate_identity_runtime:
        Arc<std::sync::RwLock<Option<Arc<crate::identity_first::IdentityRuntime>>>>,
    identity_lease_renewal_task:
        tokio::sync::Mutex<Option<crate::identity_first::runtime::TrackedLeaseRenewalTask>>,
    identity_continuity_repair_task:
        tokio::sync::Mutex<Option<crate::identity_first::runtime::TrackedContinuityRepairTask>>,
    agent_memory_observer_task:
        tokio::sync::Mutex<Option<crate::memory::taint::TaintObserverGuard>>,
    agent_memory_steward_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,

    // Cross-mob communication
    contact_directory: Option<crate::contact_directory::ContactDirectory>,
    peer_mob_handles: tokio::sync::RwLock<BTreeMap<String, cross_mob::PeerMobAuthority>>,
    /// Long-lived Ed25519 signing identity for cross-process peering.
    /// `None` is the default for inproc-only deployments and tests;
    /// production gateways set this via
    /// [`UnifiedRuntime::set_gateway_peer_keys`] during bootstrap so the
    /// `mobkit/peer_pubkey` RPC and non-inproc `wire_*` paths can stamp
    /// a real pubkey on outbound descriptors.
    gateway_peer_keys: Option<crate::auth::peer_keys::GatewayPeerKeys>,

    // Identity-first session bridge
    session_bridge: Option<Arc<dyn crate::identity_first::bridge::SessionBridge>>,
    identity_first_context: Option<Arc<crate::identity_first::IdentityFirstRuntimeContext>>,

    // Optional ABAC enforcement shared by the console/SSE surfaces.
    access_controller: Option<crate::access::AccessController>,

    // Optional product-level topology authority. The controller always
    // exists so query can remain available, but its policy defaults to
    // disabled and mutation methods are then absent/denied.
    topology_controller: crate::topology_control::TopologyController,

    // Optional panel-capable store handle backing the console Memory
    // panel's read-only RPCs (§9.3). Any provider advertising
    // `MemoryPanelStore` serves it (M4 de-weld). Interior-mutable so
    // gateways can wire it after the runtime is shared (`Arc`), wherever
    // the store is constructed.
    memory_panel_store:
        std::sync::RwLock<Option<Arc<dyn crate::memory::capabilities::MemoryPanelStore>>>,
    /// Rebuildable detached-job health/status projection supplied by the
    /// host that owns the canonical Meerkat job service.
    job_health_projection: Arc<std::sync::RwLock<Option<serde_json::Value>>>,
    // Realm-scoped WorkGraph service backing the `mobkit/workgraph/*` RPC
    // group and the console experience section. Seeded from the bootstrap
    // spec and deliberately FIXED from then on: the admission guards
    // (cross-process sidecar + agent tool-plane slots) freeze at
    // `MobRuntime::bootstrap`, so a service wired in later would run
    // guard-degraded. The spec (`MobBootstrapSpec::with_workgraph_service`
    // plus admission slot/sidecar) is the only blessed wiring.
    workgraph_service: Option<meerkat::WorkGraphService>,
    /// Identity-first console gateways: the mutable desired-identity roster
    /// that `mobkit/ensure_member` extends at runtime (ask K0). Set by the
    /// host beside `attach_identity_first_context`.
    console_identity_roster:
        std::sync::RwLock<Option<Arc<crate::identity_first::MutableRosterProvider>>>,
    /// §16 Q1 provisional operator keying: the console-principal resolver,
    /// shared between the memory coordinator (reads) and the console send
    /// path (notes interactions). `&self`-settable like the panel store.
    console_operator_resolver: std::sync::RwLock<
        Option<Arc<crate::memory::coordinator::ConsolePrincipalOperatorResolver>>,
    >,

    // Mobkit-side label sidecar for mob- and run-scoped metadata
    metadata_table: Arc<RuntimeMetadataTable>,

    // Persistent metadata adapter (currently used for the structural-events
    // subscription cursor). Falls back to `InMemoryMetadataStore` when not
    // explicitly configured — see `UnifiedRuntimeBuilder::persistent_metadata`.
    persistent_metadata: Arc<dyn PersistentMetadataStore>,
}

enum MobEventIngress {
    Forwarder(MobEventForwarder),
}

struct MobEventForwarder {
    event_rx: Receiver<EventEnvelope<UnifiedEvent>>,
    task: JoinHandle<()>,
    identity_stream_health_task: JoinHandle<()>,
}

impl UnifiedRuntime {
    pub fn builder() -> UnifiedRuntimeBuilder {
        UnifiedRuntimeBuilder::default()
    }

    pub(crate) async fn from_parts(
        mob_runtime: MobRuntime,
        module_runtime: MobkitRuntimeHandle,
        persistent_metadata: Arc<dyn PersistentMetadataStore>,
    ) -> Self {
        // Construct the metadata table first so the structural-events store
        // can be wired with it — every projected envelope picks up the
        // matching mob/run labels at projection time.
        let metadata_table = Arc::new(RuntimeMetadataTable::new());
        let mob_events_store = MobEventsStore::new().with_metadata_table(metadata_table.clone());
        let identity_runtime_authority = Arc::new(std::sync::RwLock::new(None));
        let mob_event_ingress = Some(Self::create_event_ingress(
            mob_runtime.handle(),
            mob_runtime.agent_mob_mcp_state(),
            mob_events_store.clone(),
            Arc::clone(&identity_runtime_authority),
        ));
        let mob_events_task = Self::spawn_mob_events_subscriber(
            mob_runtime.handle(),
            mob_events_store.clone(),
            persistent_metadata.clone(),
        );
        let console_events = ConsoleEventStore::new();
        // Agent-tool spawns (mob_spawn_member/delegate) project their members
        // into this runtime's console event store so spawned workers are
        // visible in the console without embedder-side workarounds.
        mob_runtime.install_console_spawn_sink(crate::console_spawn::ConsoleSpawnSink::new(
            console_events.clone(),
        ));
        let workgraph_service = mob_runtime.workgraph_service();
        let definition_edge_discovery =
            edge_reconcile::DefinitionWiringEdgeDiscovery::from_definition(
                mob_runtime.handle().definition(),
            )
            .map(|policy| Arc::new(policy) as Arc<dyn EdgeDiscovery>);
        Self {
            mob_runtime,
            post_spawn_hook: None,
            post_reconcile_hook: None,
            error_hook: None,
            drain_timeout: DEFAULT_DRAIN_TIMEOUT,
            discovery: None,
            // Default the edge policy to the definition's declared wiring
            // (auto_wire_orchestrator / role_wiring): upstream applies those
            // rules only at spawn time and only from the non-orchestrator
            // side, so bring-up order and restarts leave declared crews
            // unwired (HomeCore, 2026-07-09). With the default installed,
            // `reconcile_edges` converges the roster onto the declaration;
            // embedder-supplied policies (builder) override it.
            edge_discovery: definition_edge_discovery,
            module_runtime: Arc::new(tokio::sync::Mutex::new(module_runtime)),
            managed_dynamic_edges: Arc::new(tokio::sync::RwLock::new(BTreeSet::new())),
            shutting_down: AtomicBool::new(false),
            mob_event_ingress: tokio::sync::Mutex::new(mob_event_ingress),
            bootstrap_edges_report: tokio::sync::RwLock::new(None),
            event_log: None,
            console_log_store: Arc::new(InMemoryConsoleLogStore::new()),
            console_events,
            mob_events: mob_events_store,
            mob_events_subscriber_task: tokio::sync::Mutex::new(mob_events_task),
            implicit_delegate_retirement_task: tokio::sync::Mutex::new(None),
            implicit_delegate_identity_runtime: identity_runtime_authority,
            identity_lease_renewal_task: tokio::sync::Mutex::new(None),
            identity_continuity_repair_task: tokio::sync::Mutex::new(None),
            agent_memory_observer_task: tokio::sync::Mutex::new(None),
            agent_memory_steward_task: tokio::sync::Mutex::new(None),
            contact_directory: None,
            peer_mob_handles: tokio::sync::RwLock::new(BTreeMap::new()),
            gateway_peer_keys: None,
            session_bridge: None,
            identity_first_context: None,
            access_controller: None,
            topology_controller: crate::topology_control::TopologyController::default(),
            memory_panel_store: std::sync::RwLock::new(None),
            job_health_projection: Arc::new(std::sync::RwLock::new(None)),
            workgraph_service,
            console_identity_roster: std::sync::RwLock::new(None),
            console_operator_resolver: std::sync::RwLock::new(None),
            metadata_table,
            persistent_metadata,
        }
    }

    /// Spawn a background task that opens a streaming subscription to
    /// the meerkat mob event ledger and projects each [`MobEvent`] into
    /// the runtime's [`MobEventsStore`]. The task resumes from the
    /// last-projected cursor recorded in `persistent_metadata`, so the
    /// SDK-side cursor is durable across mobkit restarts on
    /// SQLite-backed deployments.
    ///
    /// Returns `None` when there is no current tokio runtime (e.g. unit
    /// tests outside an async context); in that case the store is still
    /// usable via direct projection.
    fn spawn_mob_events_subscriber(
        handle: MobHandle,
        store: MobEventsStore,
        persistent_metadata: Arc<dyn PersistentMetadataStore>,
    ) -> Option<JoinHandle<()>> {
        let runtime_handle = tokio::runtime::Handle::try_current().ok()?;
        Some(runtime_handle.spawn(run_mob_events_subscription(
            handle,
            store,
            persistent_metadata,
        )))
    }

    pub async fn bootstrap(
        mob_spec: MobBootstrapSpec,
        module_config: MobKitConfig,
        timeout: Duration,
    ) -> Result<Self, UnifiedRuntimeBootstrapError> {
        Box::pin(Self::bootstrap_with_options(
            mob_spec,
            module_config,
            Vec::new(),
            timeout,
            RuntimeOptions::default(),
            Arc::new(InMemoryMetadataStore::new()),
        ))
        .await
    }

    pub async fn bootstrap_with_options(
        mob_spec: MobBootstrapSpec,
        module_config: MobKitConfig,
        module_agent_events: Vec<EventEnvelope<UnifiedEvent>>,
        timeout: Duration,
        options: RuntimeOptions,
        persistent_metadata: Arc<dyn PersistentMetadataStore>,
    ) -> Result<Self, UnifiedRuntimeBootstrapError> {
        Self::bootstrap_with_options_and_topology(
            mob_spec,
            module_config,
            module_agent_events,
            timeout,
            options,
            persistent_metadata,
            crate::topology_control::TopologyBootstrapConfig::default(),
        )
        .await
    }

    /// Bootstrap the legacy runtime with the ordinary defaults plus an
    /// explicit optional topology-control configuration.
    ///
    /// This is the concise opt-in for embedders that do not otherwise need
    /// custom module events, runtime options, or metadata storage.
    pub async fn bootstrap_with_topology(
        mob_spec: MobBootstrapSpec,
        module_config: MobKitConfig,
        timeout: Duration,
        topology: crate::topology_control::TopologyBootstrapConfig,
    ) -> Result<Self, UnifiedRuntimeBootstrapError> {
        Self::bootstrap_with_options_and_topology(
            mob_spec,
            module_config,
            Vec::new(),
            timeout,
            RuntimeOptions::default(),
            Arc::new(InMemoryMetadataStore::new()),
            topology,
        )
        .await
    }

    /// Legacy bootstrap with an explicit optional topology-control seam.
    ///
    /// The default remains query-only with mutation disabled. Supplying an
    /// editable policy does not bypass console authentication or ABAC; every
    /// RPC mutation is still authorized against both endpoint resources.
    /// Supplying `state_path` makes desired additions, suppression tombstones,
    /// revisions, idempotency records, and recovery journals durable.
    #[allow(clippy::too_many_arguments)]
    pub async fn bootstrap_with_options_and_topology(
        mob_spec: MobBootstrapSpec,
        module_config: MobKitConfig,
        module_agent_events: Vec<EventEnvelope<UnifiedEvent>>,
        timeout: Duration,
        options: RuntimeOptions,
        persistent_metadata: Arc<dyn PersistentMetadataStore>,
        topology: crate::topology_control::TopologyBootstrapConfig,
    ) -> Result<Self, UnifiedRuntimeBootstrapError> {
        let topology_authority = mob_spec.definition.id.to_string();
        let topology_controller = match topology.state_path {
            Some(path) => {
                crate::topology_control::TopologyController::load_or_default(topology.policy, path)
            }
            None => crate::topology_control::TopologyController::new(topology.policy),
        }
        .map_err(|error| UnifiedRuntimeBootstrapError::Topology(error.to_string()))?;
        topology_controller
            .bind_authority(topology_authority)
            .await
            .map_err(|error| UnifiedRuntimeBootstrapError::Topology(error.to_string()))?;
        let mob_runtime = MobRuntime::bootstrap(mob_spec)
            .await
            .map_err(UnifiedRuntimeBootstrapError::Mob)?;
        let runtime_options = options.clone();
        let module_start_result = std::thread::spawn(move || {
            start_mobkit_runtime_with_options(module_config, module_agent_events, timeout, options)
        })
        .join();

        match module_start_result {
            Ok(Ok(module_runtime)) => {
                let mut runtime =
                    Self::from_parts(mob_runtime, module_runtime, persistent_metadata).await;
                runtime.topology_controller = topology_controller;
                runtime
                    .configure_implicit_delegate_retirement(&runtime_options)
                    .await;
                if runtime.edge_discovery.is_some()
                    || runtime.topology_controller.revision().await > 0
                    || runtime.topology_controller.has_pending().await
                {
                    let report = runtime.reconcile_edges().await;
                    *runtime.bootstrap_edges_report.write().await = Some(report);
                }
                Ok(runtime)
            }
            Ok(Err(error)) => {
                let startup_error = UnifiedRuntimeBootstrapError::Module(error);
                Self::rollback_mob_runtime(mob_runtime, startup_error).await
            }
            Err(_) => {
                let startup_error = UnifiedRuntimeBootstrapError::ModuleStartupThreadPanicked;
                Self::rollback_mob_runtime(mob_runtime, startup_error).await
            }
        }
    }

    /// Bootstrap edge reconciliation report, if edge discovery was configured.
    ///
    /// Inspect after `build()` to detect incomplete startup topology.
    /// Returns `None` if no edge discovery was configured.
    pub async fn bootstrap_edges_report(&self) -> Option<UnifiedRuntimeReconcileEdgesReport> {
        self.bootstrap_edges_report.read().await.clone()
    }

    /// Register an error hook after construction. Useful when the runtime
    /// is built via `bootstrap()` rather than the builder.
    pub fn set_error_hook(&mut self, hook: ErrorHook) {
        self.error_hook = Some(hook.clone());
        if let Some(identity_runtime) = self.identity_runtime() {
            identity_runtime.set_error_hook(Some(hook));
        }
    }

    /// Start the event log ingestion engine. Must be called after
    /// construction (the builder calls this automatically when event_log
    /// config is provided).
    pub fn start_event_log(&mut self, config: EventLogConfig) {
        let handle = event_log::start_event_log(config, self.error_hook.clone());
        self.event_log = Some(handle);
    }

    pub(crate) fn console_events(&self) -> ConsoleEventStore {
        self.console_events.clone()
    }

    /// A §9.3 memory-event sink projecting typed memory-plane events onto
    /// the console timeline (standard `ConsoleIdentityEventEnvelope`,
    /// `event_type = "memory.*"`). Must be called from async context — the
    /// sink captures the current runtime handle so sync emitters
    /// (store/taint/guard code) can fire-and-forget.
    pub fn memory_event_sink(&self) -> Arc<dyn crate::memory::events::MemoryEventSink> {
        Arc::new(ConsoleMemoryEventSink {
            store: self.console_events(),
            handle: tokio::runtime::Handle::current(),
        })
    }

    /// Register an observer for gating pending-entry resolutions
    /// (decisions and timeout fallbacks) — the seam the memory steward's
    /// gated promotions commit through (§10.2).
    pub async fn register_gating_resolution_observer(
        &self,
        observer: Arc<dyn crate::runtime::GatingResolutionObserver>,
    ) {
        self.module_runtime
            .lock()
            .await
            .register_gating_resolution_observer(observer);
    }

    /// Internal accessor used by console-facing RPC routers to share the
    /// in-memory structural mob events store without holding a full
    /// runtime reference.
    pub(crate) fn mob_events_store(&self) -> MobEventsStore {
        self.mob_events.clone()
    }

    pub fn binary_blob_store(&self) -> Option<Arc<dyn crate::blob_store::BinaryBlobStore>> {
        self.mob_runtime.binary_blob_store()
    }

    /// Publish the latest rebuildable detached-job observability projection.
    ///
    /// Lifecycle remains owned by Meerkat's generated job machine; this slot
    /// exists only so status, capability, console, and health surfaces can
    /// expose the host-owned projection without a parallel semantic store.
    pub fn set_job_health_projection(&self, projection: Option<serde_json::Value>) {
        *self
            .job_health_projection
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = projection;
    }

    pub fn job_health_projection(&self) -> Option<serde_json::Value> {
        self.job_health_projection
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn module_runtime_handle(&self) -> Arc<tokio::sync::Mutex<MobkitRuntimeHandle>> {
        Arc::clone(&self.module_runtime)
    }

    pub(crate) fn mobpack_runtime_catalog_state_snapshot(
        &self,
    ) -> crate::mobpack::MobpackRuntimeCatalogState {
        let loaded_modules = self
            .module_runtime
            .try_lock()
            .map(|runtime| runtime.loaded_modules())
            .unwrap_or_default();
        let has_peer_mob_handles = self
            .peer_mob_handles
            .try_read()
            .map(|handles| !handles.is_empty())
            .unwrap_or(false);
        let mut runtime_methods = vec![
            "mobkit/capabilities".to_string(),
            "mobkit/models/catalog".to_string(),
            "mobkit/spawn_member".to_string(),
            "mobkit/list_members".to_string(),
            "mobkit/get_member".to_string(),
            "mobkit/run_flow".to_string(),
            "mobkit/list_flows".to_string(),
            "mobkit/list_runs".to_string(),
        ];
        runtime_methods.extend(
            crate::rpc::MOBPACK_AUTHORING_METHODS
                .iter()
                .map(std::string::ToString::to_string),
        );
        if self.has_contact_directory() {
            runtime_methods.push("mobkit/cross_mob/directory".to_string());
        }
        if has_peer_mob_handles && self.has_inproc_contacts() {
            runtime_methods.extend([
                "mobkit/cross_mob/wire".to_string(),
                "mobkit/cross_mob/unwire".to_string(),
                "mobkit/cross_mob/send".to_string(),
            ]);
        }
        crate::mobpack::MobpackRuntimeCatalogState {
            loaded_modules,
            runtime_methods,
            has_contact_directory: self.has_contact_directory(),
            has_peer_mob_handles,
            has_inproc_contacts: self.has_inproc_contacts(),
            runtime_flow_rows: crate::mobpack::runtime_flow_registry_rows_from_definition(
                self.mob_handle().definition(),
            ),
            runtime_agent_definition_sources:
                crate::mobpack::runtime_agent_definition_sources_from_definition(
                    self.mob_handle().definition(),
                ),
            runtime_skill_realms: crate::mobpack::runtime_skill_realms_from_definition(
                self.mob_handle().definition(),
            ),
        }
    }

    /// Return the session bridge for identity-first operations, if configured.
    pub fn session_bridge(&self) -> Option<&Arc<dyn crate::identity_first::bridge::SessionBridge>> {
        self.session_bridge.as_ref()
    }

    pub fn identity_first_context(
        &self,
    ) -> Option<&Arc<crate::identity_first::IdentityFirstRuntimeContext>> {
        self.identity_first_context.as_ref()
    }

    pub fn identity_runtime(&self) -> Option<&Arc<crate::identity_first::IdentityRuntime>> {
        self.identity_first_context.as_ref().map(|ctx| &ctx.runtime)
    }

    pub async fn remember_agent_memory(
        &self,
        realm: &str,
        identity: &crate::identity_first::AgentIdentity,
        memory: crate::identity_first::NewAgentMemory,
    ) -> Result<crate::identity_first::AgentMemoryRecord, crate::identity_first::AgentMemoryError>
    {
        let runtime = self.identity_runtime().ok_or_else(|| {
            crate::identity_first::AgentMemoryError::InvalidConfig(
                "identity-first runtime is not configured".to_string(),
            )
        })?;
        runtime.remember_agent_memory(realm, identity, memory).await
    }

    pub async fn recall_agent_memory(
        &self,
        request: crate::identity_first::AgentMemoryRecallRequest,
    ) -> Result<
        Vec<crate::identity_first::AgentMemoryRecord>,
        crate::identity_first::AgentMemoryError,
    > {
        let runtime = self.identity_runtime().ok_or_else(|| {
            crate::identity_first::AgentMemoryError::InvalidConfig(
                "identity-first runtime is not configured".to_string(),
            )
        })?;
        runtime.recall_agent_memory(request).await
    }

    pub async fn forget_agent_memory(
        &self,
        realm: &str,
        identity: &crate::identity_first::AgentIdentity,
        memory_id: &str,
    ) -> Result<
        crate::identity_first::AgentMemoryForgetResult,
        crate::identity_first::AgentMemoryError,
    > {
        let runtime = self.identity_runtime().ok_or_else(|| {
            crate::identity_first::AgentMemoryError::InvalidConfig(
                "identity-first runtime is not configured".to_string(),
            )
        })?;
        runtime
            .forget_agent_memory(realm, identity, memory_id)
            .await
    }

    pub fn attach_identity_first_context(
        &mut self,
        context: Arc<crate::identity_first::IdentityFirstRuntimeContext>,
    ) {
        self.install_identity_first_context_authority(context);
        self.start_identity_first_supervisors();
    }

    /// Install identity authority before applying the initial roster.
    ///
    /// The gateway builds its base [`UnifiedRuntime`] before callback-backed
    /// identity providers are available. Identity bootstrap can partially
    /// materialize a roster before a later member fails, so the context must be
    /// visible to [`Self::shutdown`] before bootstrap starts. On failure this
    /// method drives the complete runtime shutdown order before returning the
    /// error; on success it starts the long-lived lease and repair supervisors.
    pub async fn install_and_bootstrap_identity_first_context(
        &mut self,
        context: Arc<crate::identity_first::IdentityFirstRuntimeContext>,
        roster: &[crate::identity_first::DurableAgentSpec],
    ) -> Result<crate::identity_first::RestoreFlowResult, crate::identity_first::IdentityRuntimeError>
    {
        self.install_identity_first_context_authority(Arc::clone(&context));
        match context.bootstrap_roster(roster).await {
            Ok(result) => {
                self.start_identity_first_supervisors();
                Ok(result)
            }
            Err(error) => {
                self.shutdown().await;
                Err(error)
            }
        }
    }

    fn install_identity_first_context_authority(
        &mut self,
        context: Arc<crate::identity_first::IdentityFirstRuntimeContext>,
    ) {
        self.install_identity_first_flow_target_provisioner(&context.runtime);
        self.mob_runtime
            .install_identity_runtime_authority(Arc::clone(&context.runtime));
        *self
            .implicit_delegate_identity_runtime
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(Arc::clone(&context.runtime));
        self.identity_first_context = Some(context);
    }

    pub(crate) fn install_identity_first_flow_target_provisioner(
        &self,
        runtime: &Arc<crate::identity_first::IdentityRuntime>,
    ) {
        let identity_runtime = Arc::downgrade(runtime);
        self.mob_runtime
            .handle()
            .install_flow_target_provisioner(Arc::new(move || {
                let identity_runtime = identity_runtime.clone();
                Box::pin(async move {
                    let runtime = identity_runtime.upgrade().ok_or_else(|| {
                        MobError::Internal(
                            "identity-first flow provisioner is no longer available".to_string(),
                        )
                    })?;
                    runtime
                        .materialize_all_required_tracked()
                        .await
                        .map(|_| ())
                        .map_err(|error| {
                            MobError::Internal(format!(
                                "identity-first flow materialization failed: {error}"
                            ))
                        })
                })
            }));
    }

    fn start_identity_first_supervisors(&mut self) {
        let Some(context) = self.identity_first_context.clone() else {
            return;
        };
        // Gateways attach identity-first after the base UnifiedRuntime has
        // been built, so they do not pass through the builder's supervisor
        // installation below. Active callback/local-provider leases need the
        // same proactive renewal regardless of which construction path is
        // used.
        let lease_task = context.runtime.clone().spawn_tracked_lease_renewal_task();
        if let Some(previous) = self
            .identity_lease_renewal_task
            .get_mut()
            .replace(lease_task)
        {
            previous.cancel();
            tokio::spawn(previous.cancel_and_join());
        }
        // Broken identities must self-heal: a rejected resume parks the
        // identity "pending reconcile retry", and this task is what runs
        // that retry in a live process (delivery and materialize both
        // refuse the Broken state by design).
        let repair_task = context.spawn_tracked_broken_identity_repair_task(Default::default());
        if let Some(previous) = self
            .identity_continuity_repair_task
            .get_mut()
            .replace(repair_task)
        {
            previous.cancel();
            tokio::spawn(previous.cancel_and_join());
        }
    }

    pub async fn refresh_desired_topology(
        &self,
    ) -> Result<
        Option<crate::identity_first::RestoreFlowResult>,
        crate::identity_first::IdentityRuntimeError,
    > {
        match self.identity_first_context.as_ref() {
            Some(ctx) => ctx.refresh_desired_topology_tracked().await.map(Some),
            None => Ok(None),
        }
    }

    /// Hydrate identity-first lazy members before handing control to concrete
    /// mob APIs that operate on already-materialized runtime members.
    pub async fn materialize_identity_first_for_flow(
        &self,
    ) -> Result<
        Vec<crate::identity_first::ContinuityRecord>,
        crate::identity_first::IdentityRuntimeError,
    > {
        match self.identity_runtime() {
            Some(runtime) => runtime.materialize_all_required_tracked().await,
            None => Ok(Vec::new()),
        }
    }

    /// Return the mob/run label sidecar table.
    ///
    /// Mobkit owns this table — meerkat-mob has no concept of mob- or
    /// run-level labels. Apps use it to attach external context (repo,
    /// branch, customer, deployment, environment) to a mob or a flow run.
    pub fn metadata_table(&self) -> &Arc<RuntimeMetadataTable> {
        &self.metadata_table
    }

    /// Install the shared access controller. Console routers built after
    /// this call enforce (and live-serve) the ABAC configuration.
    pub fn set_access_controller(&mut self, controller: crate::access::AccessController) {
        self.access_controller = Some(controller);
    }

    /// Wire the bundled sqlite memory store into the console Memory panel
    /// (§9.3). `&self` deliberately: gateways construct the store next to
    /// the memory subsystem wiring, which may run after the runtime is
    /// `Arc`-shared. Routers built *after* this call serve the panel RPCs.
    pub fn set_console_identity_roster(
        &self,
        roster: Arc<crate::identity_first::MutableRosterProvider>,
    ) {
        *self
            .console_identity_roster
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(roster);
    }

    pub fn console_identity_roster(
        &self,
    ) -> Option<Arc<crate::identity_first::MutableRosterProvider>> {
        self.console_identity_roster
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn set_memory_panel_store(
        &self,
        store: Arc<dyn crate::memory::capabilities::MemoryPanelStore>,
    ) {
        *self
            .memory_panel_store
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(store);
    }

    pub fn memory_panel_store(
        &self,
    ) -> Option<Arc<dyn crate::memory::capabilities::MemoryPanelStore>> {
        self.memory_panel_store
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The realm-scoped WorkGraph service backing the `mobkit/workgraph/*`
    /// RPC group and the console experience section, seeded from the
    /// bootstrap spec. There is deliberately NO late setter (round-5 S3): the
    /// admission guards — the cross-process sidecar and the agent tool-plane
    /// slots — freeze at `MobRuntime::bootstrap`, so a service wired in
    /// after the fact would silently run guard-degraded. Wire workgraph
    /// through `MobBootstrapSpec::with_workgraph_service` (plus
    /// `with_workgraph_admission_slot`/`with_workgraph_admission_sidecar`)
    /// or the stock spec constructors, which do all three.
    pub fn workgraph_service(&self) -> Option<meerkat::WorkGraphService> {
        self.workgraph_service.clone()
    }

    /// Composition-time storage durability resolution (H1/H2) carried from
    /// the bootstrap spec, reported by `mobkit/status` /
    /// `mobkit/capabilities`. `None` when the spec was composed externally
    /// without a declaration.
    pub fn resolved_storage(&self) -> Option<crate::storage_health::ResolvedStorageSummary> {
        self.mob_runtime.resolved_storage()
    }

    /// The runtime-wide admission authority serializing the workgraph
    /// duplicate-binding guards' check-then-act windows (RPC arms + agent
    /// tool plane). Lives on the mob runtime so console routers (which
    /// capture the mob runtime by value) and the unified stdin dispatch
    /// reach the SAME instance, frozen at bootstrap alongside the service.
    pub(crate) fn workgraph_admission(
        &self,
    ) -> std::sync::Arc<crate::workgraph_admission::WorkGraphAdmission> {
        self.mob_runtime.workgraph_admission()
    }

    /// Wire the §16 Q1 console-principal operator resolver (set by the
    /// gateway's memory wiring when `operator_scope = "provisional"`); the
    /// console send path notes authenticated interactions through it.
    pub fn set_console_operator_resolver(
        &self,
        resolver: Arc<crate::memory::coordinator::ConsolePrincipalOperatorResolver>,
    ) {
        *self
            .console_operator_resolver
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(resolver);
    }

    pub fn console_operator_resolver(
        &self,
    ) -> Option<Arc<crate::memory::coordinator::ConsolePrincipalOperatorResolver>> {
        self.console_operator_resolver
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Borrow the shared access controller if one was installed.
    pub fn access_controller(&self) -> Option<&crate::access::AccessController> {
        self.access_controller.as_ref()
    }

    /// Optional topology control-plane policy and durable intent store.
    pub fn topology_controller(&self) -> &crate::topology_control::TopologyController {
        &self.topology_controller
    }

    /// Cloneable topology seam for HTTP/RPC routers.
    pub fn topology_runtime_handle(&self) -> crate::topology_control::TopologyRuntimeHandle {
        crate::topology_control::TopologyRuntimeHandle::new(
            self.mob_handle(),
            self.edge_discovery.clone(),
            Arc::clone(&self.managed_dynamic_edges),
            self.topology_controller.clone(),
            self.identity_first_context.clone(),
        )
    }

    /// Replace the topology policy at runtime. Existing additions and
    /// suppression tombstones remain authoritative; disabling hides/denies
    /// mutation rather than silently discarding desired state.
    pub fn set_topology_control_policy(
        &self,
        policy: crate::topology_control::TopologyControlPolicy,
    ) -> Result<(), crate::topology_control::TopologyControlError> {
        self.topology_controller.set_policy(policy)
    }

    /// Return the persistent metadata adapter — used by the
    /// structural-events subscription to checkpoint its last-projected
    /// cursor. Tests and integration code that need to inspect the
    /// persisted cursor reach through this accessor.
    pub fn persistent_metadata(&self) -> &Arc<dyn PersistentMetadataStore> {
        &self.persistent_metadata
    }

    /// Replace the label set associated with this mob.
    ///
    /// An empty `labels` map clears the entry. Replacement is wholesale —
    /// existing labels not present in `labels` are dropped. To merge,
    /// read first via [`Self::get_mob_labels`] and combine.
    pub async fn set_mob_labels(&self, labels: BTreeMap<String, String>) {
        self.metadata_table
            .set_labels(MetadataScope::Mob(self.mob_id()), labels)
            .await;
    }

    /// Return the label set associated with this mob, or an empty map.
    pub async fn get_mob_labels(&self) -> BTreeMap<String, String> {
        self.metadata_table
            .get_labels(&MetadataScope::Mob(self.mob_id()))
            .await
    }

    /// Remove the label set associated with this mob.
    pub async fn delete_mob_labels(&self) {
        let _ = self
            .metadata_table
            .delete_labels(&MetadataScope::Mob(self.mob_id()))
            .await;
    }

    /// Replace the label set for `run_id` under this mob.
    pub async fn set_run_labels(&self, run_id: &str, labels: BTreeMap<String, String>) {
        self.metadata_table
            .set_labels(
                MetadataScope::Run(self.mob_id(), run_id.to_string()),
                labels,
            )
            .await;
    }

    /// Return the label set for `run_id` under this mob, or an empty map.
    pub async fn get_run_labels(&self, run_id: &str) -> BTreeMap<String, String> {
        self.metadata_table
            .get_labels(&MetadataScope::Run(self.mob_id(), run_id.to_string()))
            .await
    }

    /// Remove the label set for `run_id` under this mob.
    pub async fn delete_run_labels(&self, run_id: &str) {
        let _ = self
            .metadata_table
            .delete_labels(&MetadataScope::Run(self.mob_id(), run_id.to_string()))
            .await;
    }

    /// Return the underlying event log store if one is configured.
    ///
    /// Used to share the store with sub-handlers (e.g. console RPC) that
    /// don't hold a full `UnifiedRuntime` reference.
    pub fn event_log_store(&self) -> Option<std::sync::Arc<dyn event_log::EventLogStore>> {
        self.event_log
            .as_ref()
            .map(event_log::EventLogHandle::store)
    }

    pub fn console_log_store(&self) -> Arc<dyn ConsoleLogStore> {
        self.console_log_store.clone()
    }

    pub fn set_console_log_store(&mut self, store: Arc<dyn ConsoleLogStore>) {
        self.console_log_store = store;
    }

    /// Query structural mob events from the meerkat ledger.
    ///
    /// Returns events filtered by [`EventQuery`] in cursor-ascending
    /// order. `EventQuery::after_seq` acts as the pagination cursor: the
    /// caller passes the highest `cursor` seen so far to receive only
    /// strictly-newer events. Without `after_seq` the call returns the
    /// **latest** matching events up to `limit` (default 256), scanning
    /// the ledger backwards from `latest_cursor`.
    ///
    /// Errors propagate the typed [`mob_events::MobEventsQueryError`]
    /// so the JSON-RPC handler can surface `StaleEventCursor` as code
    /// `-32010`.
    pub async fn query_mob_events(
        &self,
        query: &EventQuery,
    ) -> Result<Vec<MobStructuralEventEnvelope>, mob_events::MobEventsQueryError> {
        let events = self.mob_runtime.handle().events();
        mob_events::query_ledger_with_filter(&events, &self.mob_events, query).await
    }

    /// Subscribe to live structural mob events. Returns a broadcast
    /// receiver that yields each newly-projected envelope. The receiver
    /// will report `RecvError::Lagged` if it falls behind the in-memory
    /// channel cap.
    pub fn subscribe_mob_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<MobStructuralEventEnvelope> {
        self.mob_events.subscribe()
    }

    /// Ingest an event into the event log (if configured). Non-blocking.
    pub(crate) fn ingest_event(&self, event: &EventEnvelope<UnifiedEvent>) {
        if let Some(ref log) = self.event_log {
            log.ingest(event.clone());
        }
    }

    pub(crate) async fn record_console_lifecycle(
        &self,
        identity: &str,
        event_type: &str,
        data: serde_json::Value,
    ) {
        self.console_events
            .record_lifecycle(identity, event_type, data)
            .await;
    }

    pub async fn reserve_identity_interaction(
        &self,
        identity: &str,
        runtime_member_id: Option<&str>,
        interaction_id: &str,
        origin: &str,
        content: serde_json::Value,
    ) -> Result<(), &'static str> {
        self.console_events
            .reserve_interaction_value(identity, runtime_member_id, interaction_id, origin, content)
            .await
    }

    pub(crate) async fn project_console_event_from_unified(
        &self,
        event: &EventEnvelope<UnifiedEvent>,
    ) {
        self.console_events.project_unified_event(event).await;
    }

    /// Fire an error event to the registered hook, if any.
    /// Truly fire-and-forget — spawns a detached task so slow hooks
    /// (HTTP to Slack, PagerDuty) never block the runtime operation.
    pub(crate) fn fire_error(&self, event: ErrorEvent) {
        if let Some(ref hook) = self.error_hook {
            let hook = hook.clone();
            tokio::spawn(async move {
                let () = hook(event).await;
            });
        }
    }

    fn create_event_ingress(
        mob_handle: MobHandle,
        agent_mob_mcp_state: Option<Arc<meerkat_mob_mcp::MobMcpState>>,
        mob_events: MobEventsStore,
        identity_runtime: Arc<
            std::sync::RwLock<Option<Arc<crate::identity_first::IdentityRuntime>>>,
        >,
    ) -> MobEventIngress {
        // Keep forwarding bounded to avoid unbounded memory growth under sustained ingress.
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(256);
        // Identity lifecycle repair must remain live even when an embedding
        // application does not drain the bounded console/event channel.
        // A dedicated subscription monitor drains its streams independently
        // and owns only permanent-loss detection; the ordinary forwarder
        // retains lossless backpressure for user-visible events.
        let identity_stream_health_task = tokio::spawn(run_identity_stream_health_monitor(
            mob_handle.clone(),
            agent_mob_mcp_state.clone(),
            identity_runtime,
        ));
        let task = tokio::spawn(run_resilient_mob_agent_event_forwarder(
            mob_handle,
            agent_mob_mcp_state,
            event_tx,
            mob_events,
        ));
        MobEventIngress::Forwarder(MobEventForwarder {
            event_rx,
            task,
            identity_stream_health_task,
        })
    }

    async fn rollback_mob_runtime(
        mob_runtime: MobRuntime,
        startup_error: UnifiedRuntimeBootstrapError,
    ) -> Result<Self, UnifiedRuntimeBootstrapError> {
        match mob_runtime.handle().stop().await {
            Ok(()) => Err(startup_error),
            Err(err) => Err(UnifiedRuntimeBootstrapError::ModuleStartupRollbackFailed {
                startup_error: Box::new(startup_error),
                rollback_error: MobRuntimeError::from(err),
            }),
        }
    }
}

// The trailing `Option<Arc<str>>` is the member's durable identity label,
// present only for identity-first owned members of the primary mob. The
// identity health monitor needs it to attribute a run completion to the
// durable identity; the console forwarder ignores it.
type TaggedAgentEvent = (
    AgentRuntimeId,
    FenceToken,
    ProfileName,
    meerkat_core::event::EventEnvelope<AgentEvent>,
    Option<Arc<str>>,
);

enum ForwardedAgentEvent {
    Event(Box<TaggedAgentEvent>),
    Closed(TrackedAgentEventStream),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TrackedAgentEventStream {
    mob_id: String,
    /// Trusted durable identity stamped by the identity-first spawn bridge.
    /// Ordinary/child mobs do not carry this label and are never handed to
    /// the primary identity repair authority.
    durable_identity: Option<String>,
    /// Concrete roster identity used to subscribe to Meerkat events.
    member_identity: AgentIdentity,
    runtime_id: AgentRuntimeId,
    /// Identity-authority fencing token captured when the health subscription
    /// was established. This is deliberately distinct from `fence_token`,
    /// which belongs to Meerkat Mob's member-binding fencing domain.
    identity_fencing_token: Option<u64>,
    fence_token: FenceToken,
}
type TaggedAgentEventStream = BoxStream<'static, ForwardedAgentEvent>;

/// Per-member subscribe-failure backoff for agent-event subscriptions.
/// The forwarder and independent identity-health monitor reconcile on
/// machine-state/mob-set change signals (plus a slow safety tick); without
/// backoff a member that keeps failing `subscribe_agent_events` is retried
/// on every wake indefinitely and floods the log (observed: ~49k "failed to
/// subscribe" warnings over 3.4h on a single wedged-retiring alias). Both
/// retry transient failures with exponential backoff; only the independent
/// health monitor hands persistent loss to identity repair.
struct SubscribeBackoff {
    next_attempt: tokio::time::Instant,
    consecutive_failures: u32,
}

/// First retry waits one backoff quantum; subsequent retries double up to a
/// cap so a persistently-unsubscribable member costs at most ~1 attempt per
/// `SUBSCRIBE_BACKOFF_MAX`.
const SUBSCRIBE_BACKOFF_BASE: Duration = Duration::from_millis(250);
const SUBSCRIBE_BACKOFF_MAX: Duration = Duration::from_secs(30);
const PERMANENT_STREAM_FAILURE_THRESHOLD: u32 = 4;

/// Upper bound between reconcile passes when no change signal fires. The
/// reconcilers are event-driven (machine-state watches + managed-mob-set
/// epoch + stream closures + backoff deadlines); this tick only bounds drift
/// from signals they cannot observe — identity-lease fencing-token motion
/// without a machine transition, and membership changes that land inside the
/// unwatched window while a brand-new mob's watcher is being bound. It must
/// stay slow: the historical 250ms tick made every pass's full member
/// projection an idle-CPU driver on restore-scale mobs.
const RECONCILE_SAFETY_INTERVAL: Duration = Duration::from_secs(30);

/// Wakes the stream reconcilers when membership/binding truth may have moved,
/// replacing the historical 250ms polling tick.
///
/// Wake sources, in no priority order:
/// - any tracked mob's [`meerkat_mob::MobMachineStateChanges`] firing (the
///   mob actor publishes on every applied machine input),
/// - the managed mob-set epoch changing (child mob created/removed),
/// - the earliest pending subscribe-backoff deadline,
/// - the [`RECONCILE_SAFETY_INTERVAL`] safety tick.
///
/// Watchers are keyed by mob id and RETAINED across rebinds so their
/// internally-tracked seen-version survives: a state change landing while a
/// reconcile pass runs still wakes the next wait. Only a mob first seen by
/// the previous pass starts a fresh watcher (its pre-bind changes are covered
/// by that same pass's subscription attempt and by the safety tick). Closed
/// watchers (actor gone) are dropped on rebind and on wake so a destroyed mob
/// cannot busy-wake the loop.
struct ReconcileCadence {
    machine_watchers: BTreeMap<String, meerkat_mob::MobMachineStateChanges>,
    mob_set_changes: Option<tokio::sync::watch::Receiver<u64>>,
    /// Absolute deadline for the next safety reconcile, anchored at the last
    /// completed reconcile pass ([`Self::rebind`]). Persisting it here is
    /// load-bearing: the callers' outer `select!` drops and recreates the
    /// [`Self::wait`] future on every forwarded member event, so a deadline
    /// computed inside `wait` would reset under sustained event traffic and
    /// the safety reconcile would never fire.
    next_safety_deadline: tokio::time::Instant,
}

impl ReconcileCadence {
    fn new(agent_mob_mcp_state: &Option<Arc<meerkat_mob_mcp::MobMcpState>>) -> Self {
        Self {
            machine_watchers: BTreeMap::new(),
            mob_set_changes: agent_mob_mcp_state
                .as_ref()
                .map(|state| state.mob_set_changes()),
            next_safety_deadline: tokio::time::Instant::now() + RECONCILE_SAFETY_INTERVAL,
        }
    }

    /// Rebind the watcher set to the exact handles the reconcile pass just
    /// enumerated, keeping existing watchers (and their seen-versions) alive.
    /// Every reconcile pass ends here, so this is also where the safety
    /// deadline is re-armed: drift from watch-invisible signals is bounded
    /// relative to the last reconcile, not the last wake attempt.
    fn rebind(&mut self, handles: &[MobHandle]) {
        let mut next = BTreeMap::new();
        for handle in handles {
            let key = handle.mob_id().to_string();
            let watcher = self
                .machine_watchers
                .remove(&key)
                .unwrap_or_else(|| handle.machine_state_changes());
            if !watcher.is_closed() {
                next.insert(key, watcher);
            }
        }
        self.machine_watchers = next;
        self.next_safety_deadline = tokio::time::Instant::now() + RECONCILE_SAFETY_INTERVAL;
    }

    /// Wait for the next reconcile trigger. `next_backoff_attempt` is the
    /// earliest pending subscribe retry, if any.
    async fn wait(&mut self, next_backoff_attempt: Option<tokio::time::Instant>) {
        let now = tokio::time::Instant::now();
        let mut deadline = self.next_safety_deadline;
        if let Some(attempt) = next_backoff_attempt {
            deadline = deadline.min(attempt.max(now));
        }

        let Self {
            machine_watchers,
            mob_set_changes,
            ..
        } = self;

        // Await "any machine watcher fired". A closed watcher is removed
        // in-place so it cannot immediately re-wake the caller.
        let machine_change = async {
            if machine_watchers.is_empty() {
                std::future::pending::<()>().await;
                return;
            }
            let keys: Vec<String> = machine_watchers.keys().cloned().collect();
            let closed_key = {
                let futures: Vec<_> = machine_watchers
                    .values_mut()
                    .map(|watcher| Box::pin(watcher.changed()))
                    .collect();
                let (result, index, rest) = futures::future::select_all(futures).await;
                drop(rest);
                result.is_err().then(|| keys[index].clone())
            };
            if let Some(key) = closed_key {
                machine_watchers.remove(&key);
            }
        };

        let mob_set_change = async {
            match mob_set_changes.as_mut() {
                Some(rx) => rx.changed().await,
                None => std::future::pending().await,
            }
        };

        let mob_set_closed = tokio::select! {
            () = machine_change => false,
            result = mob_set_change => result.is_err(),
            () = tokio::time::sleep_until(deadline) => false,
        };
        if mob_set_closed {
            // The dispatcher state is gone; a closed watch completes
            // immediately, so it must not stay selectable.
            self.mob_set_changes = None;
        }
    }
}

/// Earliest pending subscribe-backoff deadline, if any member is waiting.
fn earliest_backoff_attempt(
    subscribe_failures: &HashMap<TrackedAgentEventStream, SubscribeBackoff>,
) -> Option<tokio::time::Instant> {
    subscribe_failures
        .values()
        .map(|backoff| backoff.next_attempt)
        .min()
}

fn subscribe_backoff_delay(consecutive_failures: u32) -> Duration {
    SUBSCRIBE_BACKOFF_BASE
        .saturating_mul(1u32 << consecutive_failures.min(7))
        .min(SUBSCRIBE_BACKOFF_MAX)
}

/// Whether the console forwarder should hold a live agent-event subscription
/// for a member in this lifecycle state. Only `Active` members have a live
/// runtime delta stream; subscribing a `Retiring`/`Broken`/`Completed` member
/// (which can still carry stale binding atoms) fails every reconcile tick.
fn forwarder_should_subscribe(status: MobMemberStatus) -> bool {
    matches!(status, MobMemberStatus::Active)
}

fn durable_identity_label(labels: &BTreeMap<String, String>) -> Option<String> {
    labels.get("agent_identity").cloned()
}

async fn current_identity_fencing_token(
    primary_mob_id: &str,
    mob_id: &str,
    durable_identity: Option<&str>,
    identity_runtime: Option<
        &Arc<std::sync::RwLock<Option<Arc<crate::identity_first::IdentityRuntime>>>>,
    >,
) -> Option<u64> {
    if mob_id != primary_mob_id {
        return None;
    }
    let durable_identity = durable_identity?;
    let identity_runtime = identity_runtime?;
    let authority = identity_runtime
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()?;
    let identity = crate::identity_first::AgentIdentity::parse(durable_identity).ok()?;
    authority
        .status(&identity)
        .await
        .ok()?
        .lease
        .map(|lease| lease.fencing_token.get())
}

/// Advance an identity's completion cursor when meerkat reports that one of
/// its turns finished.
///
/// This is the only place the cursor moves in production, and it is driven by
/// the run-completion EVENT rather than by polling a projection on purpose: a
/// poll cannot distinguish "new turn, byte-identical output" from "no new
/// turn", which is exactly the defect the cursor closes. The identity health
/// monitor is the right host because it drains its own subscription set — a
/// full console channel cannot starve it.
///
/// Losing the subscription mid-turn means a missed completion, so the cursor
/// under-counts rather than over-counts: a waiter times out instead of being
/// told a turn finished that did not.
async fn record_identity_turn_completion(
    identity_runtime: &Arc<std::sync::RwLock<Option<Arc<crate::identity_first::IdentityRuntime>>>>,
    durable_identity: Option<&str>,
    envelope: &meerkat_core::event::EventEnvelope<AgentEvent>,
) {
    if !matches!(envelope.payload, AgentEvent::RunCompleted { .. }) {
        return;
    }
    let Some(durable_identity) = durable_identity else {
        return;
    };
    let authority = identity_runtime
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let Some(authority) = authority else {
        return;
    };
    let identity = match crate::identity_first::AgentIdentity::parse(durable_identity) {
        Ok(identity) => identity,
        Err(error) => {
            tracing::debug!(
                identity = %durable_identity,
                error = %error,
                "mobkit identity health monitor: run completion carried an unparseable durable identity"
            );
            return;
        }
    };
    authority.record_turn_completed(&identity).await;
}

async fn trigger_identity_stream_repair(
    primary_mob_id: &str,
    tracked_key: &TrackedAgentEventStream,
    identity_runtime: &Arc<std::sync::RwLock<Option<Arc<crate::identity_first::IdentityRuntime>>>>,
    detail: &str,
) {
    if tracked_key.mob_id != primary_mob_id {
        return;
    }
    let Some(durable_identity) = tracked_key.durable_identity.as_deref() else {
        return;
    };
    let Some(identity_fencing_token) = tracked_key.identity_fencing_token else {
        return;
    };
    let runtime_alias =
        crate::member_comms_id::runtime_alias_str(tracked_key.runtime_id.identity.as_str())
            .into_owned();
    let authority = identity_runtime
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let Some(authority) = authority else {
        return;
    };
    let identity = match crate::identity_first::AgentIdentity::parse(durable_identity) {
        Ok(identity) => identity,
        Err(error) => {
            tracing::warn!(
                identity = %durable_identity,
                error = %error,
                "mobkit agent event forwarder: roster identity cannot be mapped to identity authority"
            );
            return;
        }
    };
    if let Err(error) = authority
        .mark_active_runtime_broken(&identity, &runtime_alias, identity_fencing_token, detail)
        .await
    {
        tracing::warn!(
            identity = %identity,
            runtime_id = %tracked_key.runtime_id,
            error = %error,
            "mobkit agent event forwarder: failed to trigger identity repair after permanent stream loss"
        );
    }
}

async fn run_resilient_mob_agent_event_forwarder(
    handle: MobHandle,
    agent_mob_mcp_state: Option<Arc<meerkat_mob_mcp::MobMcpState>>,
    event_tx: Sender<EventEnvelope<UnifiedEvent>>,
    mob_events: MobEventsStore,
) {
    let mut streams: SelectAll<TaggedAgentEventStream> = SelectAll::new();
    let mut tracked = HashSet::new();
    let mut subscribe_failures: HashMap<TrackedAgentEventStream, SubscribeBackoff> = HashMap::new();
    let mut cadence = ReconcileCadence::new(&agent_mob_mcp_state);

    let handles = Box::pin(reconcile_agent_event_streams(
        &handle,
        &agent_mob_mcp_state,
        &mut tracked,
        &mut subscribe_failures,
        &mut streams,
        None,
    ))
    .await;
    cadence.rebind(&handles);

    loop {
        tokio::select! {
            Some(forwarded) = streams.next() => {
                match forwarded {
                    ForwardedAgentEvent::Event(event) => {
                        let (source, source_fence_token, role, envelope, _durable_identity) = *event;
                        let attributed_event = AttributedEvent {
                            source,
                            source_fence_token,
                            role,
                            envelope,
                        };
                        // Fan out to the structural mob events store. Today this is a
                        // no-op for attributed agent events (they don't carry mob/run/
                        // step fields), but the projection seam keeps the surface
                        // symmetric with the structural `MobEvent` subscriber and lets
                        // future code add attribution without touching this shape.
                        let _ = mob_events.project_attributed_event(&attributed_event).await;
                        if event_tx
                            .send(attributed_event_to_unified(attributed_event))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    ForwardedAgentEvent::Closed(tracked_key) => {
                        tracked.remove(&tracked_key);
                        subscribe_failures.remove(&tracked_key);
                        // A closure is itself the re-subscribe trigger: the
                        // member may still be live (stream lag/teardown race),
                        // and no machine transition is guaranteed to follow.
                        let handles = Box::pin(reconcile_agent_event_streams(&handle, &agent_mob_mcp_state, &mut tracked, &mut subscribe_failures, &mut streams, None)).await;
                        cadence.rebind(&handles);
                    }
                }
            }
            () = cadence.wait(earliest_backoff_attempt(&subscribe_failures)) => {
                let handles = Box::pin(reconcile_agent_event_streams(&handle, &agent_mob_mcp_state, &mut tracked, &mut subscribe_failures, &mut streams, None)).await;
                cadence.rebind(&handles);
            }
        }
    }
}

/// Drain a second subscription set dedicated to identity health. Keeping this
/// task separate from console/event projection ensures a full user-facing
/// output channel cannot suppress permanent stream-loss detection.
async fn run_identity_stream_health_monitor(
    handle: MobHandle,
    agent_mob_mcp_state: Option<Arc<meerkat_mob_mcp::MobMcpState>>,
    identity_runtime: Arc<std::sync::RwLock<Option<Arc<crate::identity_first::IdentityRuntime>>>>,
) {
    let mut streams: SelectAll<TaggedAgentEventStream> = SelectAll::new();
    let mut tracked = HashSet::new();
    let mut subscribe_failures: HashMap<TrackedAgentEventStream, SubscribeBackoff> = HashMap::new();
    let mut cadence = ReconcileCadence::new(&agent_mob_mcp_state);

    let handles = Box::pin(reconcile_agent_event_streams(
        &handle,
        &agent_mob_mcp_state,
        &mut tracked,
        &mut subscribe_failures,
        &mut streams,
        Some(&identity_runtime),
    ))
    .await;
    cadence.rebind(&handles);

    loop {
        tokio::select! {
            Some(forwarded) = streams.next() => {
                match forwarded {
                    ForwardedAgentEvent::Event(event) => {
                        let (_, _, _, envelope, durable_identity) = *event;
                        record_identity_turn_completion(
                            &identity_runtime,
                            durable_identity.as_deref(),
                            &envelope,
                        ).await;
                    }
                    ForwardedAgentEvent::Closed(tracked_key) => {
                        tracked.remove(&tracked_key);
                        subscribe_failures.remove(&tracked_key);
                        trigger_identity_stream_repair(
                            handle.mob_id().as_str(),
                            &tracked_key,
                            &identity_runtime,
                            "live agent event stream closed permanently",
                        ).await;
                        // Re-attach promptly after a closure; repair latency
                        // must not wait for an unrelated machine transition.
                        let handles = Box::pin(reconcile_agent_event_streams(
                            &handle,
                            &agent_mob_mcp_state,
                            &mut tracked,
                            &mut subscribe_failures,
                            &mut streams,
                            Some(&identity_runtime),
                        )).await;
                        cadence.rebind(&handles);
                    }
                }
            }
            () = cadence.wait(earliest_backoff_attempt(&subscribe_failures)) => {
                let handles = Box::pin(reconcile_agent_event_streams(
                    &handle,
                    &agent_mob_mcp_state,
                    &mut tracked,
                    &mut subscribe_failures,
                    &mut streams,
                    Some(&identity_runtime),
                )).await;
                cadence.rebind(&handles);
            }
        }
    }
}

/// Returns the handles it enumerated (primary + child mobs) so the caller can
/// rebind its [`ReconcileCadence`] watchers to the same set.
async fn reconcile_agent_event_streams(
    handle: &MobHandle,
    agent_mob_mcp_state: &Option<Arc<meerkat_mob_mcp::MobMcpState>>,
    tracked: &mut HashSet<TrackedAgentEventStream>,
    subscribe_failures: &mut HashMap<TrackedAgentEventStream, SubscribeBackoff>,
    streams: &mut SelectAll<TaggedAgentEventStream>,
    identity_runtime: Option<
        &Arc<std::sync::RwLock<Option<Arc<crate::identity_first::IdentityRuntime>>>>,
    >,
) -> Vec<MobHandle> {
    let primary_mob_id = handle.mob_id().to_string();
    let mut handles = vec![handle.clone()];
    if let Some(state) = agent_mob_mcp_state {
        handles.extend(
            Box::pin(state.mob_handles_snapshot())
                .await
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(mob_id, child_handle)| {
                    if mob_id.as_str() == primary_mob_id {
                        None
                    } else {
                        Some(child_handle)
                    }
                }),
        );
    }

    let mut current: HashSet<TrackedAgentEventStream> = HashSet::new();
    for handle in &handles {
        let mob_id = handle.mob_id().to_string();
        for entry in handle.list_members_including_retiring().await {
            // Members without current machine-supplied binding atoms have no
            // live runtime stream to track; their stale streams age out.
            let Some((runtime_id, fence_token)) = entry.binding_atoms() else {
                continue;
            };
            let durable_identity = durable_identity_label(&entry.labels);
            let identity_fencing_token = current_identity_fencing_token(
                &primary_mob_id,
                &mob_id,
                durable_identity.as_deref(),
                identity_runtime,
            )
            .await;
            // The health monitor exists solely for identity-first repair.
            // Avoid duplicating every ordinary/child-mob event stream.
            if identity_runtime.is_some() && identity_fencing_token.is_none() {
                continue;
            }
            current.insert(TrackedAgentEventStream {
                mob_id: mob_id.clone(),
                durable_identity,
                member_identity: entry.agent_identity.clone(),
                runtime_id,
                identity_fencing_token,
                fence_token,
            });
        }
    }

    tracked.retain(|tracked_key| current.contains(tracked_key));
    // Drop backoff bookkeeping for members that have left the roster so the
    // map can't grow without bound across the runtime's lifetime.
    subscribe_failures.retain(|key, _| current.contains(key));

    for handle in &handles {
        let mob_id = handle.mob_id().to_string();
        for entry in handle.list_members_including_retiring().await {
            let identity = entry.agent_identity.clone();
            // No binding atoms means no live runtime to subscribe to.
            let Some((runtime_id, fence_token)) = entry.binding_atoms() else {
                continue;
            };
            let durable_identity = durable_identity_label(&entry.labels);
            let identity_fencing_token = current_identity_fencing_token(
                &primary_mob_id,
                &mob_id,
                durable_identity.as_deref(),
                identity_runtime,
            )
            .await;
            if identity_runtime.is_some() && identity_fencing_token.is_none() {
                continue;
            }
            let tracked_key = TrackedAgentEventStream {
                mob_id: mob_id.clone(),
                durable_identity,
                member_identity: identity.clone(),
                runtime_id: runtime_id.clone(),
                identity_fencing_token,
                fence_token,
            };
            if tracked.contains(&tracked_key) {
                continue;
            }

            // Only Active members have a live runtime delta stream to attach
            // to. A Retiring/Broken/Completed member can still carry stale
            // binding atoms (so `binding_atoms()` is Some) while its session
            // injector is already gone, which makes `subscribe_agent_events`
            // fail every reconcile tick — the source of the 4×/s forwarder
            // hot-loop. Such members are skipped here; their final events
            // arrive via the structural ledger / session-history backfill and
            // their streams age out through `tracked.retain`.
            if !forwarder_should_subscribe(entry.status) {
                subscribe_failures.remove(&tracked_key);
                continue;
            }

            // Back off an Active member that keeps failing to subscribe (a
            // genuinely stuck injector), so even that case can't spin the log.
            let now = tokio::time::Instant::now();
            if let Some(backoff) = subscribe_failures.get(&tracked_key)
                && now < backoff.next_attempt
            {
                continue;
            }

            let role = entry.role.clone();

            match subscribe_agent_events_for_console_forwarder(handle, &tracked_key.member_identity)
                .await
            {
                Ok(stream) => {
                    let close_key = tracked_key.clone();
                    let durable_identity: Option<Arc<str>> = tracked_key
                        .durable_identity
                        .as_deref()
                        .map(Arc::<str>::from);
                    subscribe_failures.remove(&tracked_key);
                    tracked.insert(tracked_key);
                    let mapped = stream
                        .map(move |envelope| {
                            ForwardedAgentEvent::Event(Box::new((
                                runtime_id.clone(),
                                fence_token,
                                role.clone(),
                                envelope,
                                durable_identity.clone(),
                            )))
                        })
                        .chain(futures::stream::once(async move {
                            ForwardedAgentEvent::Closed(close_key)
                        }))
                        .boxed();
                    streams.push(mapped);
                }
                Err(error) => {
                    // Usually a short-lived spawn/resume race while Meerkat
                    // finishes installing the session event injector. Retry
                    // with exponential backoff and warn only on the first
                    // failure. A bounded number of misses remains a spawn
                    // race; persistent loss breaks the exact identity binding
                    // and lets the continuity supervisor rebuild it.
                    let repair_key = tracked_key.clone();
                    let backoff =
                        subscribe_failures
                            .entry(tracked_key)
                            .or_insert(SubscribeBackoff {
                                next_attempt: now,
                                consecutive_failures: 0,
                            });
                    if identity_runtime.is_none() && backoff.consecutive_failures == 0 {
                        tracing::warn!(
                            mob_id = %mob_id,
                            identity = %identity,
                            error = %error,
                            "mobkit agent event forwarder: failed to subscribe; will retry with backoff"
                        );
                    } else if identity_runtime.is_none() {
                        tracing::debug!(
                            mob_id = %mob_id,
                            identity = %identity,
                            error = %error,
                            consecutive_failures = backoff.consecutive_failures,
                            "mobkit agent event forwarder: subscribe still failing; backing off"
                        );
                    }
                    backoff.next_attempt =
                        now + subscribe_backoff_delay(backoff.consecutive_failures);
                    backoff.consecutive_failures = backoff.consecutive_failures.saturating_add(1);
                    let stream_is_permanently_lost =
                        backoff.consecutive_failures >= PERMANENT_STREAM_FAILURE_THRESHOLD;
                    if stream_is_permanently_lost && let Some(identity_runtime) = identity_runtime {
                        trigger_identity_stream_repair(
                            &primary_mob_id,
                            &repair_key,
                            identity_runtime,
                            "live agent event stream remained unavailable after bounded retries",
                        )
                        .await;
                    }
                }
            }
        }
    }
    handles
}

async fn subscribe_agent_events_for_console_forwarder(
    handle: &MobHandle,
    identity: &AgentIdentity,
) -> Result<EventStream, meerkat_mob::MobError> {
    // Keep the console forwarder on the same authoritative subscription path
    // as `/agents/{id}/events`. The observation shortcut can lag the actor's
    // runtime-member projection in identity-first/runtime-backed packs, which
    // leaves the console with only session-history backfill while direct agent
    // SSE streams live deltas correctly.
    handle.subscribe_agent_events(identity).await
}

/// Streaming subscription against the meerkat mob event ledger. Each
/// projected envelope's cursor is the upstream `MobEvent.cursor`; after
/// projection the cursor is checkpointed via `persistent_metadata` so
/// the next runtime instance can resume from where this one left off.
///
/// Resume semantics on startup:
/// - persisted cursor present → `subscribe_after(cursor)`. On
///   `MobError::StaleEventCursor` (the ledger has been truncated past
///   our checkpoint) the task logs a warning and falls through to a
///   fresh `subscribe()` at the current latest.
/// - no persisted cursor → `subscribe()` (latest, no replay).
///
/// Exits when the upstream `event_rx` closes (machine destroyed) or
/// when subscription setup fails after a stale-cursor fallback.
async fn run_mob_events_subscription(
    handle: MobHandle,
    store: MobEventsStore,
    persistent_metadata: Arc<dyn PersistentMetadataStore>,
) {
    let mob_id = handle.mob_id().as_str().to_string();
    let resume_cursor = match persistent_metadata.get_subscription_cursor(&mob_id).await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(
                mob_id = %mob_id,
                error = %err,
                "mob_events subscription: failed to read persisted cursor; resuming from latest"
            );
            None
        }
    };

    let events = handle.events();
    let mut subscription = match resume_cursor {
        Some(cursor) => match events.subscribe_after(cursor).await {
            Ok(sub) => sub,
            Err(MobError::StaleEventCursor {
                after_cursor,
                latest_cursor,
            }) => {
                tracing::warn!(
                    mob_id = %mob_id,
                    after_cursor,
                    latest_cursor,
                    "mob_events subscription: persisted cursor is past ledger frontier; resuming at latest"
                );
                match events.subscribe().await {
                    Ok(sub) => sub,
                    Err(err) => {
                        tracing::warn!(
                            mob_id = %mob_id,
                            error = %err,
                            "mob_events subscription: failed to subscribe at latest after stale-cursor recovery"
                        );
                        return;
                    }
                }
            }
            Err(err) => {
                tracing::warn!(
                    mob_id = %mob_id,
                    error = %err,
                    "mob_events subscription: failed to resume from persisted cursor"
                );
                return;
            }
        },
        None => match events.subscribe().await {
            Ok(sub) => sub,
            Err(err) => {
                tracing::warn!(
                    mob_id = %mob_id,
                    error = %err,
                    "mob_events subscription: initial subscribe failed"
                );
                return;
            }
        },
    };

    while let Some(event) = subscription.event_rx.recv().await {
        let envelope = store.project_mob_event(&event).await;
        if let Err(err) = persistent_metadata
            .set_subscription_cursor(&mob_id, envelope.cursor)
            .await
        {
            tracing::warn!(
                mob_id = %mob_id,
                cursor = envelope.cursor,
                error = %err,
                "mob_events subscription: failed to persist cursor; continuing"
            );
        }
    }
}

fn attributed_event_to_unified(attributed: AttributedEvent) -> EventEnvelope<UnifiedEvent> {
    EventEnvelope {
        event_id: format!("evt-agent-{}", attributed.envelope.event_id),
        source: "agent".to_string(),
        timestamp_ms: attributed.envelope.timestamp_ms,
        event: UnifiedEvent::Agent {
            // The runtime id's member component is the comms-safe roster
            // encoding (meerkat 0.7 `MemberCommsName`); decode back to the
            // public alias space here so console replay resolution, the
            // `mobkit/events/subscribe` buffer, and the event log all key
            // events by the same ids that spawn/reserve paths register.
            agent_id: crate::member_comms_id::runtime_event_alias(&attributed.source),
            event_type: agent_event_type(&attributed.envelope.payload).to_string(),
            // Project through the console wire shape (not the raw 0.7 event)
            // so downstream surfaces — console timeline frames, the
            // `mobkit/events/subscribe` replay buffer, and the event-log
            // query — keep the `result`/`tool_call_id` keys the SDKs parse.
            payload: Some(crate::mob_handle_runtime::console_agent_event_payload(
                &attributed.envelope.payload,
            )),
        },
    }
}

/// Projects [`crate::memory::events::MemoryTimelineEvent`]s onto the
/// console timeline. Sync fire-and-forget: the async append is spawned on
/// the captured runtime handle, so emitters inside mutexes or blocking
/// threads never wait on the event surface.
struct ConsoleMemoryEventSink {
    store: ConsoleEventStore,
    handle: tokio::runtime::Handle,
}

impl crate::memory::events::MemoryEventSink for ConsoleMemoryEventSink {
    fn emit(&self, event: crate::memory::events::MemoryTimelineEvent) {
        let store = self.store.clone();
        let identity = event
            .identity()
            .map(str::to_string)
            .unwrap_or_else(|| crate::console_contracts::SYSTEM_EVENT_IDENTITY.to_string());
        let event_type = event.event_type().to_string();
        let data = event.data();
        self.handle.spawn(async move {
            store.append(identity, None, event_type, data).await;
        });
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use meerkat_mob::ids::Generation;

    fn attributed_text_delta(member_id: &str, generation: u64) -> AttributedEvent {
        AttributedEvent {
            source: AgentRuntimeId::new(
                AgentIdentity::from(member_id),
                Generation::new(generation),
            ),
            source_fence_token: FenceToken::new(1),
            role: ProfileName::from("worker"),
            envelope: meerkat_core::event::EventEnvelope {
                event_id: Default::default(),
                source: meerkat_core::event::EventSourceIdentity::runtime("test"),
                seq: 0,
                mob_id: None,
                timestamp_ms: 1,
                payload: AgentEvent::TextDelta {
                    delta: "hello".to_string(),
                },
            },
        }
    }

    #[test]
    fn identity_stream_tracking_uses_trusted_durable_identity_label() {
        let labels =
            BTreeMap::from([("agent_identity".to_string(), "review:singleton".to_string())]);
        assert_eq!(
            durable_identity_label(&labels).as_deref(),
            Some("review:singleton")
        );
        assert_eq!(
            durable_identity_label(&BTreeMap::new()),
            None,
            "ordinary mobs must not be guessed into identity authority"
        );
    }

    /// Regression: identity-first members spawn under comms-safe encoded
    /// roster ids (`mk--…`); the agent-event ingest must decode the member
    /// component back to the public alias space before console/SDK
    /// projection, or events project under junk identities and reserved
    /// interactions never complete.
    #[test]
    fn attributed_event_ingest_decodes_encoded_roster_member_ids() {
        let encoded = crate::member_comms_id::mob_member_id_str("rt:review:singleton:0");
        assert!(encoded.starts_with("mk--"), "precondition: alias encodes");
        let unified = attributed_event_to_unified(attributed_text_delta(&encoded, 1));
        let UnifiedEvent::Agent { agent_id, .. } = unified.event else {
            panic!("expected agent event");
        };
        assert_eq!(agent_id, "rt:review:singleton:0:1");
    }

    #[test]
    fn attributed_event_ingest_passes_plain_member_ids_through() {
        let unified = attributed_event_to_unified(attributed_text_delta("worker-one", 0));
        let UnifiedEvent::Agent { agent_id, .. } = unified.event else {
            panic!("expected agent event");
        };
        assert_eq!(agent_id, "worker-one:0");
    }

    /// Regression: the callers' outer `select!` drops and recreates the
    /// `wait()` future on every forwarded member event, so a safety deadline
    /// computed inside `wait` would reset under sustained event traffic and
    /// the safety reconcile would never fire. The deadline must persist in
    /// the cadence and eventually complete a recreated `wait`.
    #[tokio::test(start_paused = true)]
    async fn reconcile_cadence_safety_deadline_survives_recreated_waits() {
        let mut cadence = ReconcileCadence::new(&None);
        let mut fired = false;
        // Seven 5s rounds = 35s of simulated event churn; the 30s deadline
        // anchored at construction must fire within them.
        for _ in 0..7 {
            tokio::select! {
                () = cadence.wait(None) => {
                    fired = true;
                    break;
                }
                () = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
        }
        assert!(
            fired,
            "safety reconcile starved: recreated wait futures reset the deadline"
        );
    }

    /// The safety bound is "at most 30s since the last reconcile pass", not
    /// "since construction": `rebind` (which ends every reconcile pass) must
    /// re-arm the deadline.
    #[tokio::test(start_paused = true)]
    async fn reconcile_cadence_rebind_rearms_safety_deadline() {
        let mut cadence = ReconcileCadence::new(&None);
        tokio::time::sleep(Duration::from_secs(20)).await;
        cadence.rebind(&[]);
        tokio::select! {
            () = cadence.wait(None) => {
                panic!("deadline fired 30s after construction despite rebind re-arm")
            }
            () = tokio::time::sleep(Duration::from_secs(29)) => {}
        }
        tokio::select! {
            () = cadence.wait(None) => {}
            () = tokio::time::sleep(Duration::from_secs(2)) => {
                panic!("re-armed deadline did not fire 30s after rebind")
            }
        }
    }

    /// Regression: the console forwarder must only hold a live subscription
    /// for Active members. A Retiring member can keep stale binding atoms
    /// while its session injector is gone, so subscribing it fails every
    /// 250ms reconcile tick — the 4×/s "failed to subscribe" hot-loop
    /// (observed ~49k warnings over 3.4h on one wedged-retiring alias).
    #[test]
    fn forwarder_only_subscribes_active_members() {
        assert!(forwarder_should_subscribe(MobMemberStatus::Active));
        assert!(!forwarder_should_subscribe(MobMemberStatus::Retiring));
        assert!(!forwarder_should_subscribe(MobMemberStatus::Broken));
        assert!(!forwarder_should_subscribe(MobMemberStatus::Completed));
        assert!(!forwarder_should_subscribe(MobMemberStatus::Unknown));
    }

    /// The backoff for a persistently-failing Active subscribe must grow from
    /// one reconcile tick and cap, so even a genuinely stuck member retries at
    /// most ~once per cap instead of 4×/s.
    #[test]
    fn subscribe_backoff_grows_and_caps() {
        const { assert!(PERMANENT_STREAM_FAILURE_THRESHOLD > 1) };
        assert_eq!(subscribe_backoff_delay(0), SUBSCRIBE_BACKOFF_BASE);
        assert_eq!(subscribe_backoff_delay(1), SUBSCRIBE_BACKOFF_BASE * 2);
        assert_eq!(subscribe_backoff_delay(3), SUBSCRIBE_BACKOFF_BASE * 8);
        assert_eq!(subscribe_backoff_delay(7), SUBSCRIBE_BACKOFF_MAX);
        // Saturates at the cap for arbitrarily many failures (no shift overflow).
        assert_eq!(subscribe_backoff_delay(50), SUBSCRIBE_BACKOFF_MAX);
        assert!(subscribe_backoff_delay(2) > subscribe_backoff_delay(1));
    }
}
