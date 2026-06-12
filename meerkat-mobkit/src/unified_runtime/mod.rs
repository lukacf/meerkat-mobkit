//! Unified runtime — combines mob lifecycle, module management, and operational subsystems.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use futures::stream::{BoxStream, SelectAll, StreamExt};
use meerkat_core::comms::EventStream;
use meerkat_core::event::{AgentEvent, agent_event_type};
use meerkat_mob::{
    AgentIdentity, AgentRuntimeId, AttributedEvent, FenceToken, MobError, MobHandle, ProfileName,
    SpawnMemberSpec,
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

pub use builder::{IdentityBootstrapMode, UnifiedRuntimeBuilder};
pub use edge_types::{
    DesiredPeerEdge, DesiredPeerEdgeError, Discovery, EdgeDiscovery, EdgeReconcileFailure,
    PreSpawnContext, PreSpawnHook,
};
pub use event_log::{EventLogConfig, EventLogError, EventLogStore, EventQuery, PersistedEvent};
pub use http::DEFAULT_REFERENCE_APP_MAX_CONCURRENT_REQUESTS;
pub use types::{
    ErrorEvent, RediscoverReport, ShutdownDrainReport, UnifiedRuntimeBootstrapError,
    UnifiedRuntimeBuilderError, UnifiedRuntimeBuilderField, UnifiedRuntimeError,
    UnifiedRuntimeReconcileEdgesReport, UnifiedRuntimeReconcileError,
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
    edge_discovery: Option<Box<dyn EdgeDiscovery>>,

    // Fine-grained interior mutability
    module_runtime: Arc<tokio::sync::Mutex<MobkitRuntimeHandle>>,
    managed_dynamic_edges: tokio::sync::RwLock<BTreeSet<(String, String)>>,
    shutting_down: AtomicBool,
    mob_event_ingress: tokio::sync::Mutex<Option<MobEventIngress>>,
    bootstrap_edges_report: tokio::sync::RwLock<Option<UnifiedRuntimeReconcileEdgesReport>>,
    event_log: Option<event_log::EventLogHandle>,
    console_log_store: Arc<dyn ConsoleLogStore>,
    console_events: ConsoleEventStore,
    mob_events: MobEventsStore,
    mob_events_subscriber_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    implicit_delegate_retirement_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    identity_lease_renewal_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,

    // Cross-mob communication
    contact_directory: Option<crate::contact_directory::ContactDirectory>,
    peer_mob_handles: tokio::sync::RwLock<BTreeMap<String, MobHandle>>,
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
        let mob_event_ingress = Some(Self::create_event_ingress(
            mob_runtime.handle(),
            mob_runtime.agent_mob_mcp_state(),
            mob_events_store.clone(),
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
        Self {
            mob_runtime,
            post_spawn_hook: None,
            post_reconcile_hook: None,
            error_hook: None,
            drain_timeout: DEFAULT_DRAIN_TIMEOUT,
            discovery: None,
            edge_discovery: None,
            module_runtime: Arc::new(tokio::sync::Mutex::new(module_runtime)),
            managed_dynamic_edges: tokio::sync::RwLock::new(BTreeSet::new()),
            shutting_down: AtomicBool::new(false),
            mob_event_ingress: tokio::sync::Mutex::new(mob_event_ingress),
            bootstrap_edges_report: tokio::sync::RwLock::new(None),
            event_log: None,
            console_log_store: Arc::new(InMemoryConsoleLogStore::new()),
            console_events,
            mob_events: mob_events_store,
            mob_events_subscriber_task: tokio::sync::Mutex::new(mob_events_task),
            implicit_delegate_retirement_task: tokio::sync::Mutex::new(None),
            identity_lease_renewal_task: tokio::sync::Mutex::new(None),
            contact_directory: None,
            peer_mob_handles: tokio::sync::RwLock::new(BTreeMap::new()),
            gateway_peer_keys: None,
            session_bridge: None,
            identity_first_context: None,
            access_controller: None,
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
                let runtime =
                    Self::from_parts(mob_runtime, module_runtime, persistent_metadata).await;
                runtime
                    .configure_implicit_delegate_retirement(&runtime_options)
                    .await;
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

    /// Internal accessor used by console-facing RPC routers to share the
    /// in-memory structural mob events store without holding a full
    /// runtime reference.
    pub(crate) fn mob_events_store(&self) -> MobEventsStore {
        self.mob_events.clone()
    }

    pub fn binary_blob_store(&self) -> Option<Arc<dyn crate::blob_store::BinaryBlobStore>> {
        self.mob_runtime.binary_blob_store()
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

    pub fn attach_identity_first_context(
        &mut self,
        context: Arc<crate::identity_first::IdentityFirstRuntimeContext>,
    ) {
        self.identity_first_context = Some(context);
    }

    pub async fn refresh_desired_topology(
        &self,
    ) -> Result<
        Option<crate::identity_first::RestoreFlowResult>,
        crate::identity_first::IdentityRuntimeError,
    > {
        match self.identity_first_context.as_ref() {
            Some(ctx) => ctx.refresh_desired_topology().await.map(Some),
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
            Some(runtime) => runtime.materialize_all_required().await,
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

    /// Borrow the shared access controller if one was installed.
    pub fn access_controller(&self) -> Option<&crate::access::AccessController> {
        self.access_controller.as_ref()
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
    ) -> MobEventIngress {
        // Keep forwarding bounded to avoid unbounded memory growth under sustained ingress.
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(256);
        let task = tokio::spawn(run_resilient_mob_agent_event_forwarder(
            mob_handle,
            agent_mob_mcp_state,
            event_tx,
            mob_events,
        ));
        MobEventIngress::Forwarder(MobEventForwarder { event_rx, task })
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

type TaggedAgentEvent = (
    AgentRuntimeId,
    FenceToken,
    ProfileName,
    meerkat_core::event::EventEnvelope<AgentEvent>,
);

enum ForwardedAgentEvent {
    Event(Box<TaggedAgentEvent>),
    Closed(TrackedAgentEventStream),
}

type TrackedAgentEventStream = (String, AgentIdentity, AgentRuntimeId, FenceToken);
type TaggedAgentEventStream = BoxStream<'static, ForwardedAgentEvent>;

async fn run_resilient_mob_agent_event_forwarder(
    handle: MobHandle,
    agent_mob_mcp_state: Option<Arc<meerkat_mob_mcp::MobMcpState>>,
    event_tx: Sender<EventEnvelope<UnifiedEvent>>,
    mob_events: MobEventsStore,
) {
    let mut streams: SelectAll<TaggedAgentEventStream> = SelectAll::new();
    let mut tracked = HashSet::new();
    let mut reconcile_interval = tokio::time::interval(Duration::from_millis(250));
    #[cfg(not(target_arch = "wasm32"))]
    reconcile_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    Box::pin(reconcile_agent_event_streams(
        &handle,
        &agent_mob_mcp_state,
        &mut tracked,
        &mut streams,
    ))
    .await;

    loop {
        tokio::select! {
            Some(forwarded) = streams.next() => {
                match forwarded {
                    ForwardedAgentEvent::Event(event) => {
                        let (source, source_fence_token, role, envelope) = *event;
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
                    }
                }
            }
            _ = reconcile_interval.tick() => {
                Box::pin(reconcile_agent_event_streams(&handle, &agent_mob_mcp_state, &mut tracked, &mut streams)).await;
            }
        }
    }
}

async fn reconcile_agent_event_streams(
    handle: &MobHandle,
    agent_mob_mcp_state: &Option<Arc<meerkat_mob_mcp::MobMcpState>>,
    tracked: &mut HashSet<TrackedAgentEventStream>,
    streams: &mut SelectAll<TaggedAgentEventStream>,
) {
    let mut handles = vec![handle.clone()];
    if let Some(state) = agent_mob_mcp_state {
        let primary_mob_id = handle.mob_id().to_string();
        handles.extend(
            Box::pin(state.mob_handles_snapshot())
                .await
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
            current.insert((
                mob_id.clone(),
                entry.agent_identity.clone(),
                runtime_id,
                fence_token,
            ));
        }
    }

    tracked.retain(|tracked_key| current.contains(tracked_key));

    for handle in handles {
        let mob_id = handle.mob_id().to_string();
        for entry in handle.list_members_including_retiring().await {
            let identity = entry.agent_identity.clone();
            // No binding atoms means no live runtime to subscribe to.
            let Some((runtime_id, fence_token)) = entry.binding_atoms() else {
                continue;
            };
            let tracked_key = (
                mob_id.clone(),
                identity.clone(),
                runtime_id.clone(),
                fence_token,
            );
            if tracked.contains(&tracked_key) {
                continue;
            }

            let role = entry.role.clone();

            match subscribe_agent_events_for_console_forwarder(&handle, &identity).await {
                Ok(stream) => {
                    let close_key = tracked_key.clone();
                    tracked.insert(tracked_key);
                    let mapped = stream
                        .map(move |envelope| {
                            ForwardedAgentEvent::Event(Box::new((
                                runtime_id.clone(),
                                fence_token,
                                role.clone(),
                                envelope,
                            )))
                        })
                        .chain(futures::stream::once(async move {
                            ForwardedAgentEvent::Closed(close_key)
                        }))
                        .boxed();
                    streams.push(mapped);
                }
                Err(error) => {
                    // This can be a short-lived spawn/resume race while Meerkat
                    // finishes installing the session event injector. Leave the
                    // identity untracked so the next reconcile tick tries again.
                    tracing::warn!(
                        mob_id = %mob_id,
                        identity = %identity,
                        error = %error,
                        "mobkit agent event forwarder: failed to subscribe; will retry"
                    );
                }
            }
        }
    }
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
}
