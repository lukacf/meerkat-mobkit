//! Unified runtime — combines mob lifecycle, module management, and operational subsystems.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use meerkat_core::event::agent_event_type;
use meerkat_mob::{AttributedEvent, MeerkatId, MobEventRouterHandle, MobHandle, SpawnMemberSpec};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

use crate::mob_handle_runtime::{MobBootstrapSpec, RealMobRuntime};
use crate::runtime::{MobkitRuntimeHandle, RuntimeOptions, start_mobkit_runtime_with_options};
use crate::types::{AgentDiscoverySpec, EventEnvelope, MobKitConfig, UnifiedEvent};

pub mod builder;
pub mod cross_mob;
pub mod edge_reconcile;
pub mod edge_types;
pub mod event_log;
pub mod http;
pub mod lifecycle;
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
        spawn = spawn.with_resume_session_id(sid);
    }
    if let Some(instructions) = additional_instructions {
        spawn = spawn.with_additional_instructions(instructions);
    }
    spawn
}

pub struct UnifiedRuntime {
    // Immutable after construction — &self access
    mob_runtime: RealMobRuntime,
    post_spawn_hook: Option<PostSpawnHook>,
    post_reconcile_hook: Option<PostReconcileHook>,
    error_hook: Option<ErrorHook>,
    drain_timeout: Duration,
    discovery: Option<Box<dyn Discovery>>,
    edge_discovery: Option<Box<dyn EdgeDiscovery>>,

    // Fine-grained interior mutability
    module_runtime: tokio::sync::Mutex<MobkitRuntimeHandle>,
    managed_dynamic_edges: tokio::sync::RwLock<BTreeSet<(String, String)>>,
    shutting_down: AtomicBool,
    mob_event_ingress: tokio::sync::Mutex<Option<MobEventIngress>>,
    bootstrap_edges_report: tokio::sync::RwLock<Option<UnifiedRuntimeReconcileEdgesReport>>,
    event_log: Option<event_log::EventLogHandle>,

    // Cross-mob communication
    contact_directory: Option<crate::contact_directory::ContactDirectory>,
    peer_mob_handles: tokio::sync::RwLock<BTreeMap<String, MobHandle>>,
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

    pub(crate) fn from_parts(
        mob_runtime: RealMobRuntime,
        module_runtime: MobkitRuntimeHandle,
    ) -> Self {
        let mob_event_router = mob_runtime.handle().subscribe_mob_events();
        let mob_event_ingress = Some(Self::create_event_ingress(mob_event_router));
        Self {
            mob_runtime,
            post_spawn_hook: None,
            post_reconcile_hook: None,
            error_hook: None,
            drain_timeout: DEFAULT_DRAIN_TIMEOUT,
            discovery: None,
            edge_discovery: None,
            module_runtime: tokio::sync::Mutex::new(module_runtime),
            managed_dynamic_edges: tokio::sync::RwLock::new(BTreeSet::new()),
            shutting_down: AtomicBool::new(false),
            mob_event_ingress: tokio::sync::Mutex::new(mob_event_ingress),
            bootstrap_edges_report: tokio::sync::RwLock::new(None),
            event_log: None,
            contact_directory: None,
            peer_mob_handles: tokio::sync::RwLock::new(BTreeMap::new()),
        }
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
        let mob_runtime = RealMobRuntime::bootstrap(mob_spec)
            .await
            .map_err(UnifiedRuntimeBootstrapError::Mob)?;
        let module_start_result = std::thread::spawn(move || {
            start_mobkit_runtime_with_options(module_config, module_agent_events, timeout, options)
        })
        .join();

        match module_start_result {
            Ok(Ok(module_runtime)) => Ok(Self::from_parts(mob_runtime, module_runtime)),
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

    /// Ingest an event into the event log (if configured). Non-blocking.
    pub(crate) fn ingest_event(&self, event: &EventEnvelope<UnifiedEvent>) {
        if let Some(ref log) = self.event_log {
            log.ingest(event.clone());
        }
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

    fn create_event_ingress(router: MobEventRouterHandle) -> MobEventIngress {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return MobEventIngress::Pull(router);
        };

        // Keep forwarding bounded to avoid unbounded memory growth under sustained ingress.
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(256);
        let task = handle.spawn(run_mob_event_forwarder(router, event_tx));
        MobEventIngress::Forwarder(MobEventForwarder { event_rx, task })
    }

    async fn rollback_mob_runtime(
        mob_runtime: RealMobRuntime,
        startup_error: UnifiedRuntimeBootstrapError,
    ) -> Result<Self, UnifiedRuntimeBootstrapError> {
        match mob_runtime.stop().await {
            Ok(()) => Err(startup_error),
            Err(rollback_error) => Err(UnifiedRuntimeBootstrapError::ModuleStartupRollbackFailed {
                startup_error: Box::new(startup_error),
                rollback_error,
            }),
        }
    }
}

async fn run_mob_event_forwarder(
    mut router: MobEventRouterHandle,
    event_tx: Sender<EventEnvelope<UnifiedEvent>>,
) {
    while let Some(attributed_event) = router.event_rx.recv().await {
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

fn attributed_event_to_unified(attributed: AttributedEvent) -> EventEnvelope<UnifiedEvent> {
    EventEnvelope {
        event_id: format!("evt-agent-{}", attributed.envelope.event_id),
        source: "agent".to_string(),
        timestamp_ms: attributed.envelope.timestamp_ms,
        event: UnifiedEvent::Agent {
            agent_id: attributed.source.to_string(),
            event_type: agent_event_type(&attributed.envelope.payload).to_string(),
        },
    }
}
