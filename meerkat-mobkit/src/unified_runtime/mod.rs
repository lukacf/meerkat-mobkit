//! Unified runtime — combines mob lifecycle, module management, and operational subsystems.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use meerkat_core::event::agent_event_type;
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::{AttributedEvent, MobEventRouterHandle, MobHandle, SpawnMemberSpec};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

use self::console_events::ConsoleEventStore;
use self::mob_events::MobEventsStore;
use crate::mob_handle_runtime::{MobBootstrapSpec, MobRuntime, MobRuntimeError};
use crate::runtime::{
    MetadataScope, MobkitRuntimeHandle, RuntimeMetadataTable, RuntimeOptions,
    start_mobkit_runtime_with_options,
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
pub mod lifecycle;
pub mod mob_events;
pub mod mob_ops;
pub mod module_ops;
pub mod types;

pub use builder::UnifiedRuntimeBuilder;
pub use edge_types::{
    DesiredPeerEdge, DesiredPeerEdgeError, Discovery, EdgeDiscovery, EdgeReconcileFailure,
    PreSpawnContext, PreSpawnHook,
};
pub use event_log::{EventLogConfig, EventLogError, EventLogStore, EventQuery, PersistedEvent};
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
        MeerkatId::from(spec.meerkat_id.as_str()),
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
    console_events: ConsoleEventStore,
    mob_events: MobEventsStore,
    mob_events_poll_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,

    // Cross-mob communication
    contact_directory: Option<crate::contact_directory::ContactDirectory>,
    peer_mob_handles: tokio::sync::RwLock<BTreeMap<String, MobHandle>>,

    // Identity-first session bridge
    session_bridge: Option<Arc<dyn crate::identity_first::bridge::SessionBridge>>,

    // Mobkit-side label sidecar for mob- and run-scoped metadata
    metadata_table: Arc<RuntimeMetadataTable>,
}

enum MobEventIngress {
    Pull(MobEventRouterHandle),
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
    ) -> Self {
        let mob_event_router = mob_runtime.handle().subscribe_mob_events().await;
        let mob_events_store = MobEventsStore::new();
        let mob_event_ingress = Some(Self::create_event_ingress(
            mob_event_router,
            mob_events_store.clone(),
        ));
        let mob_events_task =
            Self::spawn_mob_events_poller(mob_runtime.handle(), mob_events_store.clone());
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
            console_events: ConsoleEventStore::new(),
            mob_events: mob_events_store,
            mob_events_poll_task: tokio::sync::Mutex::new(mob_events_task),
            contact_directory: None,
            peer_mob_handles: tokio::sync::RwLock::new(BTreeMap::new()),
            session_bridge: None,
            metadata_table: Arc::new(RuntimeMetadataTable::new()),
        }
    }

    /// Spawn a background task that polls the mob's structural event log
    /// and projects each `MobEvent` into the in-memory `MobEventsStore`.
    /// Returns `None` when there is no current tokio runtime (e.g. unit
    /// tests outside an async context); in that case the store is still
    /// usable via direct projection.
    fn spawn_mob_events_poller(handle: MobHandle, store: MobEventsStore) -> Option<JoinHandle<()>> {
        let runtime_handle = tokio::runtime::Handle::try_current().ok()?;
        Some(runtime_handle.spawn(run_mob_events_poller(handle, store)))
    }

    pub async fn bootstrap(
        mob_spec: MobBootstrapSpec,
        module_config: MobKitConfig,
        timeout: Duration,
    ) -> Result<Self, UnifiedRuntimeBootstrapError> {
        Self::bootstrap_with_options(
            mob_spec,
            module_config,
            Vec::new(),
            timeout,
            RuntimeOptions::default(),
        )
        .await
    }

    pub async fn bootstrap_with_options(
        mob_spec: MobBootstrapSpec,
        module_config: MobKitConfig,
        module_agent_events: Vec<EventEnvelope<UnifiedEvent>>,
        timeout: Duration,
        options: RuntimeOptions,
    ) -> Result<Self, UnifiedRuntimeBootstrapError> {
        let mob_runtime = MobRuntime::bootstrap(mob_spec)
            .await
            .map_err(UnifiedRuntimeBootstrapError::Mob)?;
        let module_start_result = std::thread::spawn(move || {
            start_mobkit_runtime_with_options(module_config, module_agent_events, timeout, options)
        })
        .join();

        match module_start_result {
            Ok(Ok(module_runtime)) => Ok(Self::from_parts(mob_runtime, module_runtime).await),
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
        self.error_hook = Some(hook);
    }

    /// Start the event log ingestion engine. Must be called after
    /// construction (the builder calls this automatically when event_log
    /// config is provided).
    pub(crate) fn start_event_log(&mut self, config: EventLogConfig) {
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

    pub(crate) fn module_runtime_handle(&self) -> Arc<tokio::sync::Mutex<MobkitRuntimeHandle>> {
        Arc::clone(&self.module_runtime)
    }

    /// Return the session bridge for identity-first operations, if configured.
    pub fn session_bridge(&self) -> Option<&Arc<dyn crate::identity_first::bridge::SessionBridge>> {
        self.session_bridge.as_ref()
    }

    /// Return the mob/run label sidecar table.
    ///
    /// Mobkit owns this table — meerkat-mob has no concept of mob- or
    /// run-level labels. Apps use it to attach external context (repo,
    /// branch, customer, deployment, environment) to a mob or a flow run.
    pub fn metadata_table(&self) -> &Arc<RuntimeMetadataTable> {
        &self.metadata_table
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

    /// Query persisted operational events from the event log store.
    ///
    /// Returns `None` if no event log is configured.
    pub async fn query_events(
        &self,
        query: EventQuery,
    ) -> Option<Result<Vec<PersistedEvent>, EventLogError>> {
        if let Some(ref log) = self.event_log {
            Some(log.query(query).await)
        } else {
            None
        }
    }

    pub async fn query_console_events(
        &self,
        query: &EventQuery,
    ) -> Vec<crate::console_contracts::ConsoleIdentityEventEnvelope> {
        self.console_events.query(query).await
    }

    /// Query buffered structural mob events.
    ///
    /// Returns events filtered by [`EventQuery`] in cursor-ascending
    /// order. `EventQuery::after_seq` acts as the pagination cursor: the
    /// caller passes the highest `cursor` seen so far to receive only
    /// strictly-newer events.
    pub async fn query_mob_events(&self, query: &EventQuery) -> Vec<MobStructuralEventEnvelope> {
        self.mob_events.query(query).await
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

    pub(crate) async fn reserve_console_interaction(
        &self,
        identity: &str,
        runtime_member_id: Option<&str>,
        interaction_id: &str,
        origin: &str,
        content: &str,
    ) -> Result<(), &'static str> {
        self.console_events
            .reserve_interaction(identity, runtime_member_id, interaction_id, origin, content)
            .await
    }

    pub(crate) async fn accept_console_interaction(&self, identity: &str, interaction_id: &str) {
        self.console_events
            .accept_interaction(identity, interaction_id)
            .await;
    }

    pub(crate) async fn discard_console_interaction(&self, identity: &str, interaction_id: &str) {
        self.console_events
            .discard_interaction(identity, interaction_id)
            .await;
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

    pub(crate) async fn fail_console_interaction(
        &self,
        identity: &str,
        interaction_id: &str,
        reason: &str,
        data: serde_json::Value,
    ) {
        self.console_events
            .fail_interaction(identity, interaction_id, reason, data)
            .await;
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
        router: MobEventRouterHandle,
        mob_events: MobEventsStore,
    ) -> MobEventIngress {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return MobEventIngress::Pull(router);
        };

        // Keep forwarding bounded to avoid unbounded memory growth under sustained ingress.
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(256);
        let task = handle.spawn(run_mob_event_forwarder(router, event_tx, mob_events));
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

async fn run_mob_event_forwarder(
    mut router: MobEventRouterHandle,
    event_tx: Sender<EventEnvelope<UnifiedEvent>>,
    mob_events: MobEventsStore,
) {
    while let Some(attributed_event) = router.event_rx.recv().await {
        // Fan out to the structural mob events store. Today this is a
        // no-op for attributed agent events (they don't carry mob/run/
        // step fields), but the projection seam keeps the surface
        // symmetric with the `MobEvent` poller and lets future code add
        // attribution without touching the forwarder shape.
        let _ = mob_events.project_attributed_event(&attributed_event).await;
        if event_tx
            .send(attributed_event_to_unified(attributed_event))
            .await
            .is_err()
        {
            break;
        }
    }
    router.cancel();
}

/// Poll the mob's structural event log and project each `MobEvent` into
/// the in-memory [`MobEventsStore`]. Runs until the mob handle stops
/// returning events (machine destroyed) or the runtime drops.
///
/// Polls on the same cadence as the meerkat-mob `MobEventRouter` (500ms)
/// to avoid tight loops while keeping live SSE catchup snappy.
async fn run_mob_events_poller(handle: MobHandle, store: MobEventsStore) {
    use std::time::Duration;
    const MAX_CONSECUTIVE_ERRORS: u32 = 32;
    let mut cursor: u64 = 0;
    let mut consecutive_errors: u32 = 0;
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let events = match handle.poll_events(cursor, 256).await {
            Ok(events) => {
                consecutive_errors = 0;
                events
            }
            Err(err) => {
                // Polling failures are typically transient (machine
                // contention, store hiccups). After a long sustained run
                // of failures we assume the actor is gone for good and
                // exit so the task doesn't churn forever.
                consecutive_errors = consecutive_errors.saturating_add(1);
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    tracing::warn!(
                        error = %err,
                        "mob_events poller: giving up after sustained poll_events failures"
                    );
                    break;
                }
                tracing::debug!(
                    error = %err,
                    "mob_events poller: poll_events failed; will retry"
                );
                continue;
            }
        };
        if events.is_empty() {
            continue;
        }
        for event in events {
            cursor = cursor.max(event.cursor);
            let _ = store.project_mob_event(&event).await;
        }
    }
}

fn attributed_event_to_unified(attributed: AttributedEvent) -> EventEnvelope<UnifiedEvent> {
    EventEnvelope {
        event_id: format!("evt-agent-{}", attributed.envelope.event_id),
        source: "agent".to_string(),
        timestamp_ms: attributed.envelope.timestamp_ms,
        event: UnifiedEvent::Agent {
            agent_id: attributed.source.to_string(),
            event_type: agent_event_type(&attributed.envelope.payload).to_string(),
            payload: serde_json::to_value(&attributed.envelope.payload).ok(),
        },
    }
}
