use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use meerkat_core::event::agent_event_type;
use meerkat_core::types::SessionId;
use meerkat_mob::{
    AttributedEvent, MeerkatId, MemberRef, MobEventRouterHandle, MobState, ProfileName,
    SpawnMemberSpec,
};
use serde_json::json;
use tokio::runtime::RuntimeFlavor;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

use crate::http_console::{console_frontend_router, console_json_router_with_runtime};
use crate::http_sse::interaction_sse_router_with_injector;
use crate::mob_handle_runtime::{
    MobBootstrapSpec, MobReconcileReport, MobRuntimeError, RealInteractionSubscription,
    RealMobRuntime,
};
use crate::runtime::{
    start_mobkit_runtime_with_options, DeliveryRecord, DeliverySendError, DeliverySendRequest,
    LifecycleEvent, MobkitRuntimeError, MobkitRuntimeHandle, ModuleHealthTransition,
    NormalizationError, RoutingResolution, RoutingResolveError, RoutingResolveRequest,
    RuntimeDecisionState, RuntimeMutationError, RuntimeOptions, RuntimeRoute,
    RuntimeRouteMutationError, RuntimeShutdownReport, ScheduleDefinition, ScheduleDispatchReport,
    ScheduleValidationError, SubscribeError, SubscribeRequest, SubscribeResponse,
};
use crate::types::{AgentDiscoverySpec, EventEnvelope, MobKitConfig, ModuleEvent, UnifiedEvent};

const ROSTER_ROUTE_PREFIX: &str = "mob.member.";
const ROSTER_ROUTE_CHANNEL: &str = "notification";
const ROSTER_ROUTE_SINK: &str = "mob_member";
const ROSTER_ROUTE_TARGET_MODULE: &str = "delivery";

/// Trait for discovering agents to spawn into a mob at bootstrap time.
///
/// Implementations return a list of [`AgentDiscoverySpec`] entries, each
/// of which is mapped to a [`SpawnMemberSpec`] and spawned in batch.
pub trait Discovery: Send + Sync {
    /// Return the set of agents that should be present in the mob.
    fn discover(&self) -> Pin<Box<dyn Future<Output = Vec<AgentDiscoverySpec>> + Send + '_>>;
}

/// A callback that runs before discovery/spawn for session preloading, cache warming, etc.
pub type PreSpawnHook = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

/// Map an [`AgentDiscoverySpec`] to a [`SpawnMemberSpec`] for spawning.
pub fn discovery_spec_to_spawn_spec(spec: &AgentDiscoverySpec) -> SpawnMemberSpec {
    let resume_session_id = spec
        .resume_session_id
        .as_deref()
        .and_then(|s| SessionId::parse(s).ok());

    SpawnMemberSpec {
        profile_name: ProfileName::from(spec.profile.as_str()),
        meerkat_id: MeerkatId::from(spec.meerkat_id.as_str()),
        initial_message: spec.additional_instructions.clone(),
        runtime_mode: None,
        backend: None,
        context: spec.context.clone(),
        labels: spec.labels.clone(),
        resume_session_id,
    }
}

#[derive(Debug)]
pub enum UnifiedRuntimeBootstrapError {
    Mob(MobRuntimeError),
    Module(MobkitRuntimeError),
    ModuleStartupThreadPanicked,
    ModuleStartupRollbackFailed {
        startup_error: Box<UnifiedRuntimeBootstrapError>,
        rollback_error: MobRuntimeError,
    },
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

pub struct UnifiedRuntimeBuilder {
    mob_spec: Option<MobBootstrapSpec>,
    module_config: Option<MobKitConfig>,
    module_agent_events: Vec<EventEnvelope<UnifiedEvent>>,
    timeout: Option<Duration>,
    options: RuntimeOptions,
    discovery: Option<Box<dyn Discovery>>,
    pre_spawn_hook: Option<PreSpawnHook>,
}

impl Default for UnifiedRuntimeBuilder {
    fn default() -> Self {
        Self {
            mob_spec: None,
            module_config: None,
            module_agent_events: Vec::new(),
            timeout: None,
            options: RuntimeOptions::default(),
            discovery: None,
            pre_spawn_hook: None,
        }
    }
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

    pub fn discovery(mut self, discovery: impl Discovery + 'static) -> Self {
        self.discovery = Some(Box::new(discovery));
        self
    }

    pub fn pre_spawn_hook(mut self, hook: PreSpawnHook) -> Self {
        self.pre_spawn_hook = Some(hook);
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
        let runtime = UnifiedRuntime::bootstrap_with_options(
            mob_spec,
            module_config,
            self.module_agent_events,
            timeout,
            self.options,
        )
        .await
        .map_err(UnifiedRuntimeBuilderError::Bootstrap)?;

        if let Some(hook) = self.pre_spawn_hook {
            hook().await;
        }

        if let Some(discovery) = self.discovery {
            let specs = discovery.discover().await;
            let spawn_specs: Vec<SpawnMemberSpec> =
                specs.iter().map(discovery_spec_to_spawn_spec).collect();
            runtime
                .spawn_many(spawn_specs)
                .await
                .map_err(UnifiedRuntimeBootstrapError::Mob)
                .map_err(UnifiedRuntimeBuilderError::Bootstrap)?;
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

pub struct UnifiedRuntime {
    mob_runtime: RealMobRuntime,
    module_runtime: MobkitRuntimeHandle,
    mob_event_ingress: Option<MobEventIngress>,
    shutting_down: bool,
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

    pub async fn spawn(&self, spec: SpawnMemberSpec) -> Result<MemberRef, MobRuntimeError> {
        self.mob_runtime.spawn(spec).await
    }

    pub async fn spawn_many(
        &self,
        specs: Vec<SpawnMemberSpec>,
    ) -> Result<Vec<MemberRef>, MobRuntimeError> {
        self.mob_runtime.spawn_many(specs).await
    }

    pub async fn reconcile(
        &mut self,
        desired_specs: Vec<SpawnMemberSpec>,
    ) -> Result<UnifiedRuntimeReconcileReport, UnifiedRuntimeReconcileError> {
        let mob = self
            .mob_runtime
            .reconcile(desired_specs)
            .await
            .map_err(UnifiedRuntimeReconcileError::Mob)?;
        let active_members = self
            .mob_runtime
            .discover()
            .await
            .into_iter()
            .map(|member| member.meerkat_id)
            .collect::<Vec<_>>();
        let routing = self.reconcile_routing_wiring(active_members)?;
        Ok(UnifiedRuntimeReconcileReport { mob, routing })
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

    pub fn module_lifecycle_events(&self) -> Vec<LifecycleEvent> {
        self.module_runtime.lifecycle_events.clone()
    }

    pub fn module_health_transitions(&self) -> Vec<ModuleHealthTransition> {
        self.module_runtime.supervisor_report.transitions.clone()
    }

    pub fn module_events(&self) -> Vec<EventEnvelope<UnifiedEvent>> {
        self.module_runtime.merged_events()
    }

    pub fn build_console_json_router(&self, decisions: RuntimeDecisionState) -> Router {
        console_json_router_with_runtime(decisions, self.mob_runtime.clone())
    }

    pub fn build_console_frontend_router(&self) -> Router {
        console_frontend_router()
    }

    pub fn build_interaction_sse_router(&self) -> Router {
        let runtime = self.interaction_sse_runtime();
        interaction_sse_router_with_injector(Arc::new(move |member_id: String, message: String| {
            let runtime = runtime.clone();
            Box::pin(async move { runtime.inject_and_subscribe(&member_id, message).await })
        }))
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
            let active_member_set = active_members.iter().cloned().collect::<BTreeSet<_>>();
            for route in self.managed_roster_routes() {
                if !active_member_set.contains(&route.recipient) {
                    self.module_runtime
                        .delete_runtime_route(&route.route_key)
                        .map_err(UnifiedRuntimeReconcileError::RouteMutation)?;
                    removed_route_keys.push(route.route_key);
                }
            }

            let existing_managed_recipients = self
                .managed_roster_routes()
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
        self.close_event_router().await;
        let module_shutdown = self.module_runtime.shutdown();
        let mob_stop = self.mob_runtime.stop().await;
        UnifiedRuntimeShutdownReport {
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
