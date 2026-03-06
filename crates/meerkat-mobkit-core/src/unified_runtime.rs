use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use meerkat_core::event::agent_event_type;
use meerkat_mob::{
    AttributedEvent, MeerkatId, MemberRef, MobEventRouterHandle, MobHandle, MobState,
    SpawnMemberSpec,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::runtime::RuntimeFlavor;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

use crate::http_console::{console_frontend_router, console_json_router_with_runtime};
use crate::http_sse::DEFAULT_KEEP_ALIVE_INTERVAL;
use crate::mob_handle_runtime::{
    MobBootstrapSpec, MobMemberSnapshot, MobReconcileReport, MobRuntimeError,
    RealInteractionSubscription, RealMobRuntime,
};
use crate::runtime::{
    start_mobkit_runtime_with_options, DeliveryHistoryRequest, DeliveryHistoryResponse,
    DeliveryRecord, DeliverySendError, DeliverySendRequest, GatingAuditEntry, GatingDecideError,
    GatingDecideRequest, GatingDecisionResult, GatingEvaluateRequest, GatingEvaluateResult,
    GatingPendingEntry, LifecycleEvent, MemoryIndexError, MemoryIndexRequest, MemoryIndexResult,
    MemoryQueryRequest, MemoryQueryResult, MemoryStoreInfo, MobkitRuntimeError,
    MobkitRuntimeHandle, ModuleHealthTransition, NormalizationError, RoutingResolution,
    RoutingResolveError, RoutingResolveRequest, RuntimeDecisionState, RuntimeMutationError,
    RuntimeOptions, RuntimeRoute, RuntimeRouteMutationError, RuntimeShutdownReport,
    ScheduleDefinition, ScheduleDispatchReport, ScheduleEvaluation, ScheduleValidationError,
    SubscribeError, SubscribeRequest, SubscribeResponse,
};
use crate::{route_module_call, ModuleRouteError, ModuleRouteRequest, ModuleRouteResponse};
use crate::types::{AgentDiscoverySpec, EventEnvelope, MobKitConfig, ModuleEvent, UnifiedEvent};

/// Opaque context produced by [`PreSpawnHook`] and consumed by [`Discovery::discover`].
///
/// Carries data from the pre-spawn phase (e.g. session resume maps, warmed caches)
/// into the discovery phase without requiring shared side-channel state.
pub type PreSpawnContext = serde_json::Value;

/// Trait for discovering agents to spawn into a mob at bootstrap time.
///
/// `discover` receives the [`PreSpawnContext`] produced by the pre-spawn hook
/// (or `Value::Null` if no hook ran). This enables the "query sessions once,
/// build a resume map, feed that into discovery" pattern without side-channel state.
pub trait Discovery: Send + Sync {
    fn discover(
        &self,
        context: PreSpawnContext,
    ) -> Pin<Box<dyn Future<Output = Vec<AgentDiscoverySpec>> + Send + '_>>;
}

/// A callback that runs before discovery/spawn for session preloading, cache warming, etc.
///
/// Returns a [`PreSpawnContext`] on success, which is passed to [`Discovery::discover`].
/// This enables pre-spawn to produce data (resume maps, session queries, etc.) that
/// discovery consumes, replacing the need for shared mutable side-channel state.
pub type PreSpawnHook = Box<
    dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<PreSpawnContext, Box<dyn std::error::Error + Send>>> + Send>>
        + Send,
>;

// ---------------------------------------------------------------------------
// Dynamic peer edge reconciliation
// ---------------------------------------------------------------------------

/// Error constructing a [`DesiredPeerEdge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesiredPeerEdgeError {
    EmptyEndpoint,
    SelfEdge,
}

impl Display for DesiredPeerEdgeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyEndpoint => write!(f, "edge endpoint must not be empty"),
            Self::SelfEdge => write!(f, "self-edges are not allowed"),
        }
    }
}

impl std::error::Error for DesiredPeerEdgeError {}

/// A canonical undirected peer edge. Endpoints are sorted at construction
/// time and self-edges are rejected, so the invariant `a < b` always holds.
///
/// Fields are private — use [`DesiredPeerEdge::new`] or [`endpoints`] to access.
/// Deserialization validates the invariant, rejecting non-canonical inputs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct DesiredPeerEdge {
    a: String,
    b: String,
}

impl<'de> Deserialize<'de> for DesiredPeerEdge {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            a: String,
            b: String,
        }
        let raw = Raw::deserialize(deserializer)?;
        DesiredPeerEdge::new(raw.a, raw.b).map_err(serde::de::Error::custom)
    }
}

impl DesiredPeerEdge {
    pub fn new(a: impl Into<String>, b: impl Into<String>) -> Result<Self, DesiredPeerEdgeError> {
        let mut a = a.into().trim().to_string();
        let mut b = b.into().trim().to_string();
        if a.is_empty() || b.is_empty() {
            return Err(DesiredPeerEdgeError::EmptyEndpoint);
        }
        if a == b {
            return Err(DesiredPeerEdgeError::SelfEdge);
        }
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        Ok(Self { a, b })
    }

    pub fn endpoints(&self) -> (&str, &str) {
        (&self.a, &self.b)
    }
}

/// Trait for computing desired peer edges from active mob members.
///
/// The app owns the policy (which agents should be wired). MobKit owns
/// the lifecycle-safe reconciliation that makes reality match the policy.
pub trait EdgeDiscovery: Send + Sync {
    fn discover_edges(
        &self,
        active_members: Vec<MobMemberSnapshot>,
    ) -> Pin<Box<dyn Future<Output = Vec<DesiredPeerEdge>> + Send + '_>>;
}

/// A failed edge operation during reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeReconcileFailure {
    pub edge: DesiredPeerEdge,
    pub operation: String,
    pub error: String,
}

/// Report from dynamic edge reconciliation.
///
/// Best-effort: partial success is reported clearly. Apps decide whether
/// to treat failures as fatal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UnifiedRuntimeReconcileEdgesReport {
    pub desired_edges: Vec<DesiredPeerEdge>,
    pub wired_edges: Vec<DesiredPeerEdge>,
    pub unwired_edges: Vec<DesiredPeerEdge>,
    pub retained_edges: Vec<DesiredPeerEdge>,
    pub preexisting_edges: Vec<DesiredPeerEdge>,
    pub skipped_missing_members: Vec<DesiredPeerEdge>,
    pub pruned_stale_managed_edges: Vec<DesiredPeerEdge>,
    #[serde(default)]
    pub failures: Vec<EdgeReconcileFailure>,
}

impl UnifiedRuntimeReconcileEdgesReport {
    /// True if all desired edges were successfully applied or retained.
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty() && self.skipped_missing_members.is_empty()
    }
}

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
    SpawnMemberSpec {
        profile_name: meerkat_mob::ProfileName::from(spec.profile.as_str()),
        meerkat_id: MeerkatId::from(spec.meerkat_id.as_str()),
        initial_message: None,
        runtime_mode: None,
        backend: None,
        context: spec.context.clone(),
        labels: spec.labels.clone(),
        resume_session_id,
        additional_instructions,
    }
}

/// Called after members are spawned. Receives the list of spawned member IDs.
pub type PostSpawnHook =
    Arc<dyn Fn(Vec<String>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Called after reconcile completes. Receives the reconcile report.
pub type PostReconcileHook = Arc<
    dyn Fn(UnifiedRuntimeReconcileReport) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

const ROSTER_ROUTE_PREFIX: &str = "mob.member.";
const ROSTER_ROUTE_CHANNEL: &str = "notification";
const ROSTER_ROUTE_SINK: &str = "mob_member";
const ROSTER_ROUTE_TARGET_MODULE: &str = "delivery";

#[derive(Debug)]
pub enum UnifiedRuntimeBootstrapError {
    Mob(MobRuntimeError),
    Module(MobkitRuntimeError),
    ModuleStartupThreadPanicked,
    ModuleStartupRollbackFailed {
        startup_error: Box<UnifiedRuntimeBootstrapError>,
        rollback_error: MobRuntimeError,
    },
    PreSpawnHook(String),
}

impl Display for UnifiedRuntimeBootstrapError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(err) => write!(f, "failed to bootstrap mob runtime: {err}"),
            Self::Module(err) => write!(f, "failed to bootstrap module runtime: {err:?}"),
            Self::ModuleStartupThreadPanicked => {
                write!(
                    f,
                    "failed to bootstrap module runtime: startup thread panicked"
                )
            }
            Self::PreSpawnHook(err) => {
                write!(f, "pre-spawn hook failed: {err}")
            }
            Self::ModuleStartupRollbackFailed {
                startup_error,
                rollback_error,
            } => {
                write!(
                    f,
                    "failed to bootstrap unified runtime: startup error ({startup_error}) and rollback failed: {rollback_error}"
                )
            }
        }
    }
}

impl std::error::Error for UnifiedRuntimeBootstrapError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifiedRuntimeBuilderField {
    MobSpec,
    ModuleConfig,
    Timeout,
}

#[derive(Debug)]
pub enum UnifiedRuntimeBuilderError {
    MissingRequiredField(UnifiedRuntimeBuilderField),
    Bootstrap(UnifiedRuntimeBootstrapError),
}

impl Display for UnifiedRuntimeBuilderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRequiredField(UnifiedRuntimeBuilderField::MobSpec) => {
                write!(f, "missing required builder field: mob_spec")
            }
            Self::MissingRequiredField(UnifiedRuntimeBuilderField::ModuleConfig) => {
                write!(f, "missing required builder field: module_config")
            }
            Self::MissingRequiredField(UnifiedRuntimeBuilderField::Timeout) => {
                write!(f, "missing required builder field: timeout")
            }
            Self::Bootstrap(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for UnifiedRuntimeBuilderError {}

#[derive(Default)]
pub struct UnifiedRuntimeBuilder {
    mob_spec: Option<MobBootstrapSpec>,
    module_config: Option<MobKitConfig>,
    module_agent_events: Vec<EventEnvelope<UnifiedEvent>>,
    timeout: Option<Duration>,
    options: RuntimeOptions,
    post_spawn_hook: Option<PostSpawnHook>,
    post_reconcile_hook: Option<PostReconcileHook>,
    drain_timeout: Option<Duration>,
    discovery: Option<Box<dyn Discovery>>,
    pre_spawn_hook: Option<PreSpawnHook>,
    edge_discovery: Option<Box<dyn EdgeDiscovery>>,
}

impl UnifiedRuntimeBuilder {
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

    pub async fn build(self) -> Result<UnifiedRuntime, UnifiedRuntimeBuilderError> {
        let mob_spec = self
            .mob_spec
            .ok_or(UnifiedRuntimeBuilderError::MissingRequiredField(
                UnifiedRuntimeBuilderField::MobSpec,
            ))?;
        let module_config =
            self.module_config
                .ok_or(UnifiedRuntimeBuilderError::MissingRequiredField(
                    UnifiedRuntimeBuilderField::ModuleConfig,
                ))?;
        let timeout = self
            .timeout
            .ok_or(UnifiedRuntimeBuilderError::MissingRequiredField(
                UnifiedRuntimeBuilderField::Timeout,
            ))?;
        let mut runtime = UnifiedRuntime::bootstrap_with_options(
            mob_spec,
            module_config,
            self.module_agent_events,
            timeout,
            self.options,
        )
        .await
        .map_err(UnifiedRuntimeBuilderError::Bootstrap)?;
        runtime.post_spawn_hook = self.post_spawn_hook;
        runtime.post_reconcile_hook = self.post_reconcile_hook;
        runtime.drain_timeout = self.drain_timeout.unwrap_or(DEFAULT_DRAIN_TIMEOUT);
        runtime.edge_discovery = self.edge_discovery;

        let pre_spawn_context = if let Some(hook) = self.pre_spawn_hook {
            hook()
                .await
                .map_err(|err| {
                    UnifiedRuntimeBuilderError::Bootstrap(
                        UnifiedRuntimeBootstrapError::PreSpawnHook(err.to_string()),
                    )
                })?
        } else {
            serde_json::Value::Null
        };
        if let Some(discovery) = self.discovery {
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
            runtime.reconcile_edges().await;
        }

        Ok(runtime)
    }
}

#[derive(Debug)]
pub enum UnifiedRuntimeError {
    Normalize(NormalizationError),
    Subscribe(SubscribeError),
    ScheduleValidation(ScheduleValidationError),
    RuntimeShuttingDown,
    ScheduleDispatchThreadPanicked,
}

impl Display for UnifiedRuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normalize(err) => write!(f, "failed to normalize unified event: {err:?}"),
            Self::Subscribe(err) => write!(f, "failed to subscribe to unified events: {err:?}"),
            Self::ScheduleValidation(err) => {
                write!(f, "failed to dispatch schedule tick: {err:?}")
            }
            Self::RuntimeShuttingDown => {
                write!(
                    f,
                    "failed to dispatch schedule tick: unified runtime is shutting down"
                )
            }
            Self::ScheduleDispatchThreadPanicked => {
                write!(
                    f,
                    "failed to dispatch schedule tick: dispatch thread panicked"
                )
            }
        }
    }
}

impl std::error::Error for UnifiedRuntimeError {}

impl From<NormalizationError> for UnifiedRuntimeError {
    fn from(value: NormalizationError) -> Self {
        Self::Normalize(value)
    }
}

impl From<SubscribeError> for UnifiedRuntimeError {
    fn from(value: SubscribeError) -> Self {
        Self::Subscribe(value)
    }
}

impl From<ScheduleValidationError> for UnifiedRuntimeError {
    fn from(value: ScheduleValidationError) -> Self {
        Self::ScheduleValidation(value)
    }
}

#[derive(Debug)]
pub struct UnifiedRuntimeShutdownReport {
    pub drain: ShutdownDrainReport,
    pub module_shutdown: RuntimeShutdownReport,
    pub mob_stop: Result<(), MobRuntimeError>,
}

#[derive(Debug)]
pub struct UnifiedRuntimeRunReport {
    pub serve_result: std::io::Result<()>,
    pub shutdown: UnifiedRuntimeShutdownReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedRuntimeReconcileRoutingReport {
    pub router_module_loaded: bool,
    pub active_members: Vec<String>,
    pub added_route_keys: Vec<String>,
    pub removed_route_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedRuntimeReconcileReport {
    pub mob: MobReconcileReport,
    pub edges: UnifiedRuntimeReconcileEdgesReport,
    pub routing: UnifiedRuntimeReconcileRoutingReport,
}

#[derive(Debug)]
pub enum UnifiedRuntimeReconcileError {
    Mob(MobRuntimeError),
    RouteMutation(RuntimeRouteMutationError),
}

impl Display for UnifiedRuntimeReconcileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(err) => write!(f, "failed to reconcile mob roster: {err}"),
            Self::RouteMutation(err) => {
                write!(f, "failed to reconcile routing wiring: {err:?}")
            }
        }
    }
}

impl std::error::Error for UnifiedRuntimeReconcileError {}

const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct ShutdownDrainReport {
    pub drained_count: usize,
    pub timed_out: bool,
    pub drain_duration_ms: u64,
}

pub struct UnifiedRuntime {
    mob_runtime: RealMobRuntime,
    module_runtime: MobkitRuntimeHandle,
    mob_event_ingress: Option<MobEventIngress>,
    shutting_down: bool,
    post_spawn_hook: Option<PostSpawnHook>,
    post_reconcile_hook: Option<PostReconcileHook>,
    drain_timeout: Duration,
    edge_discovery: Option<Box<dyn EdgeDiscovery>>,
    /// Dynamic edges managed by edge reconciliation. Only edges in this set
    /// can be unwired by the reconciler — static/preexisting edges are safe.
    managed_dynamic_edges: BTreeSet<(String, String)>,
}

enum MobEventIngress {
    Pull(MobEventRouterHandle),
    Forwarder(MobEventForwarder),
}

struct MobEventForwarder {
    event_rx: Receiver<EventEnvelope<UnifiedEvent>>,
    task: JoinHandle<()>,
}

#[derive(Clone)]
struct UnifiedInteractionSseRuntime {
    mob_runtime: RealMobRuntime,
}

impl UnifiedInteractionSseRuntime {
    fn new(mob_runtime: RealMobRuntime) -> Self {
        Self { mob_runtime }
    }

    async fn inject_and_subscribe(
        &self,
        member_id: &str,
        message: String,
    ) -> Result<RealInteractionSubscription, MobRuntimeError> {
        self.mob_runtime
            .inject_and_subscribe(member_id, message)
            .await
    }
}

#[derive(Debug)]
enum UnifiedRuntimeInjectionError {
    Mob(MobRuntimeError),
}

impl UnifiedRuntimeInjectionError {
    fn kind(&self) -> &'static str {
        match self {
            Self::Mob(_) => "mob_runtime",
        }
    }
}

impl Display for UnifiedRuntimeInjectionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(err) => write!(f, "mob injection failed: {err}"),
        }
    }
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
            module_runtime,
            mob_event_ingress,
            shutting_down: false,
            post_spawn_hook: None,
            post_reconcile_hook: None,
            drain_timeout: DEFAULT_DRAIN_TIMEOUT,
            edge_discovery: None,
            managed_dynamic_edges: BTreeSet::new(),
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

    pub fn status(&self) -> MobState {
        self.mob_runtime.status()
    }

    pub fn mob_handle(&self) -> MobHandle {
        self.mob_runtime.handle()
    }

    pub async fn spawn(&self, spec: SpawnMemberSpec) -> Result<MemberRef, MobRuntimeError> {
        let member_id = spec.meerkat_id.to_string();
        let member_ref = self.mob_runtime.spawn(spec).await?;
        if let Some(hook) = &self.post_spawn_hook {
            hook(vec![member_id]).await;
        }
        Ok(member_ref)
    }

    pub async fn spawn_many(
        &self,
        specs: Vec<SpawnMemberSpec>,
    ) -> Result<Vec<MemberRef>, MobRuntimeError> {
        let member_ids: Vec<String> = specs.iter().map(|s| s.meerkat_id.to_string()).collect();
        let refs = self.mob_runtime.spawn_many(specs).await?;
        if !member_ids.is_empty() {
            if let Some(hook) = &self.post_spawn_hook {
                hook(member_ids).await;
            }
        }
        Ok(refs)
    }

    pub async fn reconcile(
        &mut self,
        desired_specs: Vec<SpawnMemberSpec>,
    ) -> Result<UnifiedRuntimeReconcileReport, UnifiedRuntimeReconcileError> {
        // 1. Member reconcile
        let mob = self
            .mob_runtime
            .reconcile(desired_specs)
            .await
            .map_err(UnifiedRuntimeReconcileError::Mob)?;
        // 2. Refresh active members
        let active_snapshots = self.mob_runtime.discover().await;
        let active_member_ids = active_snapshots
            .iter()
            .map(|m| m.meerkat_id.clone())
            .collect::<Vec<_>>();
        // 3 + 4. Edge discovery + dynamic edge reconcile
        let edges = self
            .reconcile_edges_from_members(active_snapshots)
            .await;
        // 5. Routing reconcile
        let routing = self.reconcile_routing_wiring(active_member_ids)?;
        let report = UnifiedRuntimeReconcileReport { mob, edges, routing };
        if let Some(hook) = &self.post_reconcile_hook {
            hook(report.clone()).await;
        }
        Ok(report)
    }

    /// Reconcile dynamic peer edges using fresh roster state.
    ///
    /// Refreshes the roster, runs edge discovery if configured, diffs
    /// desired vs managed edges, and calls wire/unwire as needed.
    pub async fn reconcile_edges(
        &mut self,
    ) -> UnifiedRuntimeReconcileEdgesReport {
        let active_members = self.mob_runtime.discover().await;
        self.reconcile_edges_from_members(active_members).await
    }

    async fn reconcile_edges_from_members(
        &mut self,
        active_members: Vec<MobMemberSnapshot>,
    ) -> UnifiedRuntimeReconcileEdgesReport {
        let edge_discovery = match &self.edge_discovery {
            Some(d) => d,
            None => return UnifiedRuntimeReconcileEdgesReport::default(),
        };

        let active_ids: BTreeSet<String> = active_members
            .iter()
            .map(|m| m.meerkat_id.clone())
            .collect();

        // Build current wiring map from snapshots
        let mut current_edges: BTreeSet<(String, String)> = BTreeSet::new();
        for member in &active_members {
            for peer in &member.wired_to {
                let mut a = member.meerkat_id.clone();
                let mut b = peer.clone();
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                current_edges.insert((a, b));
            }
        }

        // Run edge discovery
        let raw_desired = edge_discovery.discover_edges(active_members).await;

        // Deduplicate and defensively validate (DesiredPeerEdge enforces
        // invariants at construction, but we still canonicalize the key set)
        let desired: BTreeSet<(String, String)> = raw_desired
            .iter()
            .map(|e| {
                let (a, b) = e.endpoints();
                (a.to_string(), b.to_string())
            })
            .collect();

        let mut report = UnifiedRuntimeReconcileEdgesReport {
            desired_edges: raw_desired,
            ..Default::default()
        };

        // Classify desired edges
        for (a, b) in &desired {
            // Skip if either endpoint is missing from the active roster
            if !active_ids.contains(a) || !active_ids.contains(b) {
                if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                    report.skipped_missing_members.push(edge);
                }
                continue;
            }
            let key = (a.clone(), b.clone());
            if self.managed_dynamic_edges.contains(&key) {
                // Managed by us — check if the actual edge still exists in the
                // mob graph. If an out-of-band unwire() removed it, re-wire.
                if current_edges.contains(&key) {
                    if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                        report.retained_edges.push(edge);
                    }
                } else {
                    // Managed edge disappeared from mob graph — heal it
                    let mid_a = MeerkatId::from(a.as_str());
                    let mid_b = MeerkatId::from(b.as_str());
                    match self.mob_runtime.handle().wire(mid_a, mid_b).await {
                        Ok(()) => {
                            if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                                report.wired_edges.push(edge);
                            }
                        }
                        Err(err) => {
                            if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                                report.failures.push(EdgeReconcileFailure {
                                    edge,
                                    operation: "wire (heal)".into(),
                                    error: format!("{err}"),
                                });
                            }
                        }
                    }
                }
            } else if current_edges.contains(&key) {
                // Exists but not managed by us (static or external) — don't claim
                if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                    report.preexisting_edges.push(edge);
                }
            } else {
                // New edge — wire it
                let mid_a = MeerkatId::from(a.as_str());
                let mid_b = MeerkatId::from(b.as_str());
                match self.mob_runtime.handle().wire(mid_a, mid_b).await {
                    Ok(()) => {
                        self.managed_dynamic_edges.insert(key);
                        if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                            report.wired_edges.push(edge);
                        }
                    }
                    Err(err) => {
                        if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                            report.failures.push(EdgeReconcileFailure {
                                edge,
                                operation: "wire".into(),
                                error: format!("{err}"),
                            });
                        }
                    }
                }
            }
        }

        // Unwire managed edges that are no longer desired
        let to_unwire: Vec<(String, String)> = self
            .managed_dynamic_edges
            .iter()
            .filter(|key| !desired.contains(*key))
            .cloned()
            .collect();

        for (a, b) in to_unwire {
            // If either endpoint is gone, just prune from managed set
            if !active_ids.contains(&a) || !active_ids.contains(&b) {
                self.managed_dynamic_edges.remove(&(a.clone(), b.clone()));
                if let Ok(edge) = DesiredPeerEdge::new(a, b) {
                    report.pruned_stale_managed_edges.push(edge);
                }
                continue;
            }
            let mid_a = MeerkatId::from(a.as_str());
            let mid_b = MeerkatId::from(b.as_str());
            match self.mob_runtime.handle().unwire(mid_a, mid_b).await {
                Ok(()) => {
                    self.managed_dynamic_edges.remove(&(a.clone(), b.clone()));
                    if let Ok(edge) = DesiredPeerEdge::new(a, b) {
                        report.unwired_edges.push(edge);
                    }
                }
                Err(err) => {
                    if let Ok(edge) = DesiredPeerEdge::new(a, b) {
                        report.failures.push(EdgeReconcileFailure {
                            edge,
                            operation: "unwire".into(),
                            error: format!("{err}"),
                        });
                    }
                }
            }
        }

        report
    }

    pub async fn inject_and_subscribe(
        &self,
        member_id: &str,
        message: String,
    ) -> Result<RealInteractionSubscription, MobRuntimeError> {
        self.mob_runtime
            .inject_and_subscribe(member_id, message)
            .await
    }

    /// Validate that a profile supports inject_and_subscribe before spawning.
    ///
    /// Checks: profile exists, external_addressable, and effective runtime mode
    /// is AutonomousHost. Returns the error that inject_and_subscribe would
    /// return, so callers get a clear rejection without roster mutation.
    fn validate_profile_for_inject(
        &self,
        profile_name: &meerkat_mob::ProfileName,
        meerkat_id: &meerkat_mob::MeerkatId,
        runtime_mode_override: Option<meerkat_mob::MobRuntimeMode>,
    ) -> Result<(), MobRuntimeError> {
        let handle = self.mob_runtime.handle();
        let definition = handle.definition();
        let profile = definition.profiles.get(profile_name).ok_or_else(|| {
            MobRuntimeError::Mob(meerkat_mob::MobError::ProfileNotFound(profile_name.clone()))
        })?;
        if !profile.external_addressable {
            return Err(MobRuntimeError::Mob(
                meerkat_mob::MobError::NotExternallyAddressable(meerkat_id.clone()),
            ));
        }
        let effective_mode = runtime_mode_override.unwrap_or(profile.runtime_mode);
        if effective_mode != meerkat_mob::MobRuntimeMode::AutonomousHost {
            return Err(MobRuntimeError::Mob(meerkat_mob::MobError::UnsupportedForMode {
                mode: effective_mode,
                reason: "inject_and_subscribe requires autonomous_host mode".into(),
            }));
        }
        Ok(())
    }

    /// Ensure a member exists (spawning from `spec` if missing), then inject a
    /// message and return a streaming subscription.
    ///
    /// Uses inject-first strategy: tries inject_and_subscribe first, and only
    /// spawns if the member doesn't exist. Pre-validates the profile before
    /// spawning to avoid leaving stray members for non-injectable profiles.
    ///
    /// For concurrent callers: if spawn returns `MeerkatAlreadyExists`, the
    /// member may still be pending (Meerkat returns the same error for both
    /// "already in roster" and "spawn in progress"). We retry inject with
    /// backoff to wait for the pending spawn to complete.
    pub async fn ensure_and_inject_and_subscribe(
        &self,
        spec: SpawnMemberSpec,
        message: String,
    ) -> Result<RealInteractionSubscription, MobRuntimeError> {
        let member_id_str = spec.meerkat_id.to_string();
        // Try inject first — if the member exists, this succeeds without spawning.
        match self
            .mob_runtime
            .inject_and_subscribe(&member_id_str, message.clone())
            .await
        {
            Ok(subscription) => return Ok(subscription),
            Err(MobRuntimeError::Mob(meerkat_mob::MobError::MeerkatNotFound(_))) => {
                // Member doesn't exist — fall through to spawn
            }
            Err(other) => return Err(other),
        }
        // Validate profile supports inject BEFORE spawning to avoid stray members.
        self.validate_profile_for_inject(&spec.profile_name, &spec.meerkat_id, spec.runtime_mode)?;
        // Spawn the member.
        let was_concurrent = match self.mob_runtime.spawn(spec).await {
            Ok(_member_ref) => {
                if let Some(hook) = &self.post_spawn_hook {
                    hook(vec![member_id_str.clone()]).await;
                }
                false
            }
            Err(MobRuntimeError::Mob(meerkat_mob::MobError::MeerkatAlreadyExists(_))) => {
                // Another caller is spawning or already spawned — may still be pending.
                true
            }
            Err(err) => return Err(err),
        };
        // Inject with retry: if the spawn was concurrent (MeerkatAlreadyExists),
        // the member may still be pending. Retry with backoff.
        let max_retries = if was_concurrent { 10 } else { 1 };
        let mut last_err = None;
        for attempt in 0..max_retries {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(100 * attempt as u64)).await;
            }
            match self
                .mob_runtime
                .inject_and_subscribe(&member_id_str, message.clone())
                .await
            {
                Ok(subscription) => return Ok(subscription),
                Err(MobRuntimeError::Mob(meerkat_mob::MobError::MeerkatNotFound(_)))
                    if was_concurrent =>
                {
                    last_err = Some(MobRuntimeError::Mob(
                        meerkat_mob::MobError::MeerkatNotFound(
                            meerkat_mob::MeerkatId::from(member_id_str.as_str()),
                        ),
                    ));
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            MobRuntimeError::Mob(meerkat_mob::MobError::MeerkatNotFound(
                meerkat_mob::MeerkatId::from(member_id_str.as_str()),
            ))
        }))
    }

    pub fn module_is_running(&self) -> bool {
        self.module_runtime.is_running()
    }

    pub fn loaded_modules(&self) -> Vec<String> {
        self.module_runtime.loaded_modules()
    }

    pub fn reconcile_modules(
        &mut self,
        modules: Vec<String>,
        timeout: Duration,
    ) -> Result<usize, RuntimeMutationError> {
        self.module_runtime.reconcile_modules(modules, timeout)
    }

    pub fn resolve_routing(
        &mut self,
        request: RoutingResolveRequest,
    ) -> Result<RoutingResolution, RoutingResolveError> {
        self.module_runtime.resolve_routing(request)
    }

    pub fn send_delivery(
        &mut self,
        request: DeliverySendRequest,
    ) -> Result<DeliveryRecord, DeliverySendError> {
        self.module_runtime.send_delivery(request)
    }

    pub fn evaluate_schedule_tick(
        &self,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> Result<ScheduleEvaluation, ScheduleValidationError> {
        self.module_runtime.evaluate_schedule_tick(schedules, tick_ms)
    }

    pub fn list_runtime_routes(&self) -> Vec<RuntimeRoute> {
        self.module_runtime.list_runtime_routes()
    }

    pub fn add_runtime_route(
        &mut self,
        route: RuntimeRoute,
    ) -> Result<RuntimeRoute, RuntimeRouteMutationError> {
        self.module_runtime.add_runtime_route(route)
    }

    pub fn delete_runtime_route(
        &mut self,
        route_key: &str,
    ) -> Result<RuntimeRoute, RuntimeRouteMutationError> {
        self.module_runtime.delete_runtime_route(route_key)
    }

    pub fn delivery_history(&self, request: DeliveryHistoryRequest) -> DeliveryHistoryResponse {
        self.module_runtime.delivery_history(request)
    }

    pub fn memory_stores(&self) -> Vec<MemoryStoreInfo> {
        self.module_runtime.memory_stores()
    }

    pub fn memory_index(
        &mut self,
        request: MemoryIndexRequest,
    ) -> Result<MemoryIndexResult, MemoryIndexError> {
        self.module_runtime.memory_index(request)
    }

    pub fn memory_query(&self, request: MemoryQueryRequest) -> MemoryQueryResult {
        self.module_runtime.memory_query(request)
    }

    pub fn evaluate_gating_action(
        &mut self,
        request: GatingEvaluateRequest,
    ) -> GatingEvaluateResult {
        self.module_runtime.evaluate_gating_action(request)
    }

    pub fn list_gating_pending(&mut self) -> Vec<GatingPendingEntry> {
        self.module_runtime.list_gating_pending()
    }

    pub fn decide_gating_action(
        &mut self,
        request: GatingDecideRequest,
    ) -> Result<GatingDecisionResult, GatingDecideError> {
        self.module_runtime.decide_gating_action(request)
    }

    pub fn gating_audit_entries(&mut self, limit: usize) -> Vec<GatingAuditEntry> {
        self.module_runtime.gating_audit_entries(limit)
    }

    pub fn spawn_member(
        &mut self,
        module_id: &str,
        timeout: Duration,
    ) -> Result<(), RuntimeMutationError> {
        self.module_runtime.spawn_member(module_id, timeout)
    }

    pub fn route_module_call(
        &self,
        request: &ModuleRouteRequest,
        timeout: Duration,
    ) -> Result<ModuleRouteResponse, ModuleRouteError> {
        route_module_call(&self.module_runtime, request, timeout)
    }

    pub fn module_lifecycle_events(&self) -> Vec<LifecycleEvent> {
        self.module_runtime.lifecycle_events.clone()
    }

    pub fn module_health_transitions(&self) -> Vec<ModuleHealthTransition> {
        self.module_runtime.supervisor_report.transitions.clone()
    }

    pub fn module_events(&self) -> &[EventEnvelope<UnifiedEvent>] {
        self.module_runtime.merged_events()
    }

    pub fn build_console_json_router(&self, decisions: RuntimeDecisionState) -> Router {
        console_json_router_with_runtime(decisions, self.mob_runtime.clone())
    }

    pub fn build_console_frontend_router(&self) -> Router {
        console_frontend_router()
    }

    pub fn build_interaction_sse_router(&self) -> Router {
        use crate::http_sse::{interaction_sse_router_full, InteractionSseEnsureInjectFn};
        let runtime = self.interaction_sse_runtime();
        let inject_fn = {
            let runtime = runtime.clone();
            Arc::new(move |member_id: String, message: String| {
                let runtime = runtime.clone();
                Box::pin(async move { runtime.inject_and_subscribe(&member_id, message).await })
                    as crate::http_sse::InteractionSseInjectFuture
            })
        };
        let ensure_runtime = self.mob_runtime.clone();
        let post_spawn = self.post_spawn_hook.clone();
        let ensure_fn: InteractionSseEnsureInjectFn = Arc::new(
            move |params: crate::http_sse::EnsureInjectParams| {
                let runtime = ensure_runtime.clone();
                let post_spawn = post_spawn.clone();
                Box::pin(async move {
                    // Inject-first: try inject, spawn only if member doesn't exist.
                    match runtime
                        .inject_and_subscribe(&params.member_id, params.message.clone())
                        .await
                    {
                        Ok(sub) => return Ok(sub),
                        Err(MobRuntimeError::Mob(meerkat_mob::MobError::MeerkatNotFound(_))) => {}
                        Err(other) => return Err(other),
                    }
                    // Validate profile supports inject BEFORE spawning.
                    let profile_name = meerkat_mob::ProfileName::from(params.profile.as_str());
                    {
                        let handle = runtime.handle();
                        let definition = handle.definition();
                        let profile = definition.profiles.get(&profile_name).ok_or_else(|| {
                            MobRuntimeError::Mob(meerkat_mob::MobError::ProfileNotFound(
                                profile_name.clone(),
                            ))
                        })?;
                        if !profile.external_addressable {
                            return Err(MobRuntimeError::Mob(
                                meerkat_mob::MobError::NotExternallyAddressable(
                                    meerkat_mob::MeerkatId::from(params.member_id.as_str()),
                                ),
                            ));
                        }
                        if profile.runtime_mode != meerkat_mob::MobRuntimeMode::AutonomousHost {
                            return Err(MobRuntimeError::Mob(
                                meerkat_mob::MobError::UnsupportedForMode {
                                    mode: profile.runtime_mode,
                                    reason: "inject_and_subscribe requires autonomous_host mode"
                                        .into(),
                                },
                            ));
                        }
                    }
                    let mid = meerkat_mob::MeerkatId::from(params.member_id.as_str());
                    let resume_session_id = params
                        .resume_session_id
                        .as_deref()
                        .and_then(|s| meerkat_core::types::SessionId::parse(s).ok());
                    let additional_instructions = params.additional_instructions.and_then(|v| {
                        if v.is_empty() { None } else { Some(v) }
                    });
                    let spec = SpawnMemberSpec {
                        profile_name: meerkat_mob::ProfileName::from(params.profile.as_str()),
                        meerkat_id: mid,
                        initial_message: None,
                        runtime_mode: None,
                        backend: None,
                        context: params.context,
                        labels: params.labels,
                        resume_session_id,
                        additional_instructions,
                    };
                    let was_concurrent = match runtime.spawn(spec).await {
                        Ok(_) => {
                            if let Some(hook) = &post_spawn {
                                hook(vec![params.member_id.clone()]).await;
                            }
                            false
                        }
                        Err(MobRuntimeError::Mob(
                            meerkat_mob::MobError::MeerkatAlreadyExists(_),
                        )) => true,
                        Err(err) => return Err(err),
                    };
                    // Retry inject with backoff if spawn was concurrent (may still be pending)
                    let max_retries: u64 = if was_concurrent { 10 } else { 1 };
                    let mut last_err = None;
                    for attempt in 0..max_retries {
                        if attempt > 0 {
                            tokio::time::sleep(Duration::from_millis(100 * attempt)).await;
                        }
                        match runtime
                            .inject_and_subscribe(&params.member_id, params.message.clone())
                            .await
                        {
                            Ok(sub) => return Ok(sub),
                            Err(MobRuntimeError::Mob(
                                meerkat_mob::MobError::MeerkatNotFound(_),
                            )) if was_concurrent => {
                                last_err = Some(MobRuntimeError::Mob(
                                    meerkat_mob::MobError::MeerkatNotFound(
                                        meerkat_mob::MeerkatId::from(params.member_id.as_str()),
                                    ),
                                ));
                                continue;
                            }
                            Err(err) => return Err(err),
                        }
                    }
                    Err(last_err.unwrap_or_else(|| {
                        MobRuntimeError::Mob(meerkat_mob::MobError::MeerkatNotFound(
                            meerkat_mob::MeerkatId::from(params.member_id.as_str()),
                        ))
                    }))
                })
            },
        );
        interaction_sse_router_full(inject_fn, Some(ensure_fn), DEFAULT_KEEP_ALIVE_INTERVAL)
    }

    pub fn build_reference_app_router(&self, decisions: RuntimeDecisionState) -> Router {
        Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .merge(self.build_console_frontend_router())
            .merge(self.build_console_json_router(decisions))
            .merge(self.build_interaction_sse_router())
    }

    pub async fn serve(
        &self,
        listener: tokio::net::TcpListener,
        decisions: RuntimeDecisionState,
    ) -> std::io::Result<()> {
        let app = self.build_reference_app_router(decisions);
        axum::serve(listener, app).await
    }

    pub async fn run<F>(
        &mut self,
        listener: tokio::net::TcpListener,
        decisions: RuntimeDecisionState,
        shutdown_signal: F,
    ) -> UnifiedRuntimeRunReport
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let app = self.build_reference_app_router(decisions);
        let serve_result = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal)
            .await;
        let shutdown = self.shutdown().await;
        UnifiedRuntimeRunReport {
            serve_result,
            shutdown,
        }
    }

    fn reconcile_routing_wiring(
        &mut self,
        mut active_members: Vec<String>,
    ) -> Result<UnifiedRuntimeReconcileRoutingReport, UnifiedRuntimeReconcileError> {
        active_members.sort();
        active_members.dedup();

        let router_module_loaded = self
            .module_runtime
            .loaded_modules()
            .iter()
            .any(|module_id| module_id == "router");
        let mut added_route_keys = Vec::new();
        let mut removed_route_keys = Vec::new();

        if router_module_loaded {
            let managed_routes = self.managed_roster_routes();
            let active_member_set = active_members.iter().cloned().collect::<BTreeSet<_>>();
            for route in &managed_routes {
                if !active_member_set.contains(&route.recipient) {
                    self.module_runtime
                        .delete_runtime_route(&route.route_key)
                        .map_err(UnifiedRuntimeReconcileError::RouteMutation)?;
                    removed_route_keys.push(route.route_key.clone());
                }
            }

            let existing_managed_recipients = managed_routes
                .into_iter()
                .map(|route| route.recipient)
                .collect::<BTreeSet<_>>();
            for member_id in &active_members {
                if existing_managed_recipients.contains(member_id) {
                    continue;
                }
                let route_key = format!("{ROSTER_ROUTE_PREFIX}{member_id}");
                self.module_runtime
                    .add_runtime_route(RuntimeRoute {
                        route_key: route_key.clone(),
                        recipient: member_id.clone(),
                        channel: Some(ROSTER_ROUTE_CHANNEL.to_string()),
                        sink: ROSTER_ROUTE_SINK.to_string(),
                        target_module: ROSTER_ROUTE_TARGET_MODULE.to_string(),
                        retry_max: None,
                        backoff_ms: None,
                        rate_limit_per_minute: None,
                    })
                    .map_err(UnifiedRuntimeReconcileError::RouteMutation)?;
                added_route_keys.push(route_key);
            }
        }

        added_route_keys.sort();
        removed_route_keys.sort();

        Ok(UnifiedRuntimeReconcileRoutingReport {
            router_module_loaded,
            active_members,
            added_route_keys,
            removed_route_keys,
        })
    }

    fn managed_roster_routes(&self) -> Vec<RuntimeRoute> {
        self.module_runtime
            .list_runtime_routes()
            .into_iter()
            .filter(|route| route.route_key.starts_with(ROSTER_ROUTE_PREFIX))
            .collect()
    }

    fn interaction_sse_runtime(&self) -> UnifiedInteractionSseRuntime {
        UnifiedInteractionSseRuntime::new(self.mob_runtime.clone())
    }

    pub fn subscribe_events(
        &mut self,
        request: SubscribeRequest,
    ) -> Result<SubscribeResponse, UnifiedRuntimeError> {
        self.drain_mob_agent_events()?;
        self.module_runtime
            .subscribe_events(request)
            .map_err(UnifiedRuntimeError::Subscribe)
    }

    pub async fn dispatch_schedule_tick(
        &mut self,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> Result<ScheduleDispatchReport, UnifiedRuntimeError> {
        if self.shutting_down {
            return Err(UnifiedRuntimeError::RuntimeShuttingDown);
        }
        let mut dispatch_report = self.dispatch_schedule_tick_blocking(schedules, tick_ms)?;

        for dispatch in &mut dispatch_report.dispatched {
            let Some(runtime_injection) = dispatch.runtime_injection.clone() else {
                continue;
            };

            let injection_result = self
                .mob_runtime
                .inject_and_subscribe(
                    &runtime_injection.member_id,
                    runtime_injection.message.clone(),
                )
                .await;

            match injection_result {
                Ok(subscription) => {
                    let interaction_id = subscription.interaction_id;
                    self.module_runtime.append_normalized_event(EventEnvelope {
                        event_id: format!("{}-executed", runtime_injection.injection_event_id),
                        source: "module".to_string(),
                        timestamp_ms: dispatch.tick_ms,
                        event: UnifiedEvent::Module(ModuleEvent {
                            module: "runtime".to_string(),
                            event_type: "runtime.injection.executed".to_string(),
                            payload: json!({
                                "schedule_id": dispatch.schedule_id.clone(),
                                "claim_key": dispatch.claim_key.clone(),
                                "member_id": runtime_injection.member_id,
                                "message": runtime_injection.message,
                                "interaction_id": interaction_id,
                            }),
                        }),
                    })?;
                }
                Err(error) => {
                    let error = UnifiedRuntimeInjectionError::Mob(error);
                    dispatch.runtime_injection_error = Some(error.to_string());
                    self.module_runtime.append_normalized_event(EventEnvelope {
                        event_id: format!("{}-failed", runtime_injection.injection_event_id),
                        source: "module".to_string(),
                        timestamp_ms: dispatch.tick_ms,
                        event: UnifiedEvent::Module(ModuleEvent {
                            module: "runtime".to_string(),
                            event_type: "runtime.injection.failed".to_string(),
                            payload: json!({
                                "schedule_id": dispatch.schedule_id.clone(),
                                "claim_key": dispatch.claim_key.clone(),
                                "member_id": runtime_injection.member_id,
                                "message": runtime_injection.message,
                                "error_kind": error.kind(),
                                "error": error.to_string(),
                            }),
                        }),
                    })?;
                }
            }
        }

        self.drain_mob_agent_events()?;
        Ok(dispatch_report)
    }

    pub async fn shutdown(&mut self) -> UnifiedRuntimeShutdownReport {
        self.shutting_down = true;

        // Phase 1: Drain in-flight events
        let drain_start = std::time::Instant::now();
        let mut drained_count = 0_usize;
        let drain_result = tokio::time::timeout(self.drain_timeout, async {
            loop {
                if self.drain_mob_agent_events().is_err() || self.mob_event_ingress.is_none() {
                    break;
                }
                drained_count += 1;
                tokio::time::sleep(Duration::from_millis(50)).await;
                if drained_count > 1 {
                    break;
                }
            }
        })
        .await;
        let drain = ShutdownDrainReport {
            drained_count,
            timed_out: drain_result.is_err(),
            drain_duration_ms: drain_start.elapsed().as_millis() as u64,
        };

        // Phase 2: Close event router
        self.close_event_router().await;

        // Phase 3: Shutdown modules and mob
        let module_shutdown = self.module_runtime.shutdown();
        let mob_stop = self.mob_runtime.stop().await;
        UnifiedRuntimeShutdownReport {
            drain,
            module_shutdown,
            mob_stop,
        }
    }

    fn drain_mob_agent_events(&mut self) -> Result<(), UnifiedRuntimeError> {
        let mut disconnected = false;
        if self.mob_event_ingress.is_none() {
            return Ok(());
        }

        loop {
            match self.try_recv_ingress_event() {
                Some(Ok(unified_event)) => {
                    self.module_runtime.append_normalized_event(unified_event)?
                }
                Some(Err(TryRecvError::Empty)) => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    disconnected = true;
                    break;
                }
                None => break,
            }
        }

        if disconnected {
            self.mob_event_ingress = None;
        }

        Ok(())
    }

    async fn close_event_router(&mut self) {
        match self.mob_event_ingress.take() {
            Some(MobEventIngress::Pull(router)) => {
                router.cancel();
            }
            Some(MobEventIngress::Forwarder(forwarder)) => {
                let task = forwarder.task;
                task.abort();
                let _ = task.await;
            }
            None => {}
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

    fn try_recv_ingress_event(
        &mut self,
    ) -> Option<Result<EventEnvelope<UnifiedEvent>, TryRecvError>> {
        let ingress = self.mob_event_ingress.as_mut()?;
        Some(match ingress {
            MobEventIngress::Pull(router) => {
                router.event_rx.try_recv().map(attributed_event_to_unified)
            }
            MobEventIngress::Forwarder(forwarder) => forwarder.event_rx.try_recv(),
        })
    }

    fn dispatch_schedule_tick_blocking(
        &mut self,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> Result<ScheduleDispatchReport, UnifiedRuntimeError> {
        let dispatch_result = if tokio::runtime::Handle::try_current()
            .is_ok_and(|handle| handle.runtime_flavor() == RuntimeFlavor::MultiThread)
        {
            tokio::task::block_in_place(|| {
                Self::dispatch_schedule_tick_in_joined_thread(
                    &mut self.module_runtime,
                    schedules,
                    tick_ms,
                )
            })
        } else {
            Self::dispatch_schedule_tick_in_joined_thread(
                &mut self.module_runtime,
                schedules,
                tick_ms,
            )
        };

        dispatch_result
            .map_err(|_| UnifiedRuntimeError::ScheduleDispatchThreadPanicked)?
            .map_err(UnifiedRuntimeError::ScheduleValidation)
    }

    fn dispatch_schedule_tick_in_joined_thread(
        module_runtime: &mut MobkitRuntimeHandle,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> std::thread::Result<Result<ScheduleDispatchReport, ScheduleValidationError>> {
        std::thread::scope(|scope| {
            scope
                .spawn(move || module_runtime.dispatch_schedule_tick(schedules, tick_ms))
                .join()
        })
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
