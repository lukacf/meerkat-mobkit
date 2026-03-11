//! Module lifecycle operations — registration, health checks, and capability probing.

use std::time::Duration;

use crate::runtime::{
    DeliveryHistoryRequest, DeliveryHistoryResponse, DeliveryRecord, DeliverySendError,
    DeliverySendRequest, GatingAuditEntry, GatingDecideError, GatingDecideRequest,
    GatingDecisionResult, GatingEvaluateRequest, GatingEvaluateResult, GatingPendingEntry,
    LifecycleEvent, MemoryIndexError, MemoryIndexRequest, MemoryIndexResult, MemoryQueryRequest,
    MemoryQueryResult, MemoryStoreInfo, ModuleHealthTransition, RoutingResolution,
    RoutingResolveError, RoutingResolveRequest, RuntimeMutationError, RuntimeRoute,
    RuntimeRouteMutationError, ScheduleDefinition, ScheduleEvaluation, ScheduleValidationError,
    SubscribeRequest, SubscribeResponse,
};
use crate::types::{EventEnvelope, UnifiedEvent};
use crate::{route_module_call, ModuleRouteError, ModuleRouteRequest, ModuleRouteResponse};

use super::types::UnifiedRuntimeError;
use super::UnifiedRuntime;

/// Run a blocking closure on a dedicated thread to isolate it from the
/// tokio runtime. MCP boundary calls check `Handle::try_current()` and
/// refuse to block inside an active runtime, so `block_in_place` is not
/// sufficient — we need a thread that has no runtime handle at all.
///
/// Uses `std::thread::scope` so the closure can borrow from the caller.
fn run_blocking<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    std::thread::scope(|scope| {
        scope.spawn(f).join().unwrap_or_else(|e| std::panic::resume_unwind(e))
    })
}

impl UnifiedRuntime {
    pub async fn module_is_running(&self) -> bool {
        self.module_runtime.lock().await.is_running()
    }

    pub async fn loaded_modules(&self) -> Vec<String> {
        self.module_runtime.lock().await.loaded_modules()
    }

    /// Reconcile modules — runs blocking subprocess I/O via `block_in_place`.
    pub async fn reconcile_modules(
        &self,
        modules: Vec<String>,
        timeout: Duration,
    ) -> Result<usize, RuntimeMutationError> {
        let mut rt = self.module_runtime.lock().await;
        run_blocking(|| rt.reconcile_modules(modules, timeout))
    }

    /// Resolve routing — runs blocking MCP boundary call via `block_in_place`.
    pub async fn resolve_routing(
        &self,
        request: RoutingResolveRequest,
    ) -> Result<RoutingResolution, RoutingResolveError> {
        let mut rt = self.module_runtime.lock().await;
        run_blocking(|| rt.resolve_routing(request))
    }

    /// Send delivery — runs blocking MCP boundary call via `block_in_place`.
    pub async fn send_delivery(
        &self,
        request: DeliverySendRequest,
    ) -> Result<DeliveryRecord, DeliverySendError> {
        let mut rt = self.module_runtime.lock().await;
        run_blocking(|| rt.send_delivery(request))
    }

    pub async fn evaluate_schedule_tick(
        &self,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> Result<ScheduleEvaluation, ScheduleValidationError> {
        self.module_runtime.lock().await.evaluate_schedule_tick(schedules, tick_ms)
    }

    pub async fn list_runtime_routes(&self) -> Vec<RuntimeRoute> {
        self.module_runtime.lock().await.list_runtime_routes()
    }

    pub async fn add_runtime_route(
        &self,
        route: RuntimeRoute,
    ) -> Result<RuntimeRoute, RuntimeRouteMutationError> {
        self.module_runtime.lock().await.add_runtime_route(route)
    }

    pub async fn delete_runtime_route(
        &self,
        route_key: &str,
    ) -> Result<RuntimeRoute, RuntimeRouteMutationError> {
        self.module_runtime.lock().await.delete_runtime_route(route_key)
    }

    pub async fn delivery_history(&self, request: DeliveryHistoryRequest) -> DeliveryHistoryResponse {
        self.module_runtime.lock().await.delivery_history(request)
    }

    pub async fn memory_stores(&self) -> Vec<MemoryStoreInfo> {
        self.module_runtime.lock().await.memory_stores()
    }

    pub async fn memory_index(
        &self,
        request: MemoryIndexRequest,
    ) -> Result<MemoryIndexResult, MemoryIndexError> {
        self.module_runtime.lock().await.memory_index(request)
    }

    pub async fn memory_query(&self, request: MemoryQueryRequest) -> MemoryQueryResult {
        self.module_runtime.lock().await.memory_query(request)
    }

    pub async fn evaluate_gating_action(
        &self,
        request: GatingEvaluateRequest,
    ) -> GatingEvaluateResult {
        self.module_runtime.lock().await.evaluate_gating_action(request)
    }

    pub async fn list_gating_pending(&self) -> Vec<GatingPendingEntry> {
        self.module_runtime.lock().await.list_gating_pending()
    }

    pub async fn decide_gating_action(
        &self,
        request: GatingDecideRequest,
    ) -> Result<GatingDecisionResult, GatingDecideError> {
        self.module_runtime.lock().await.decide_gating_action(request)
    }

    pub async fn gating_audit_entries(&self, limit: usize) -> Vec<GatingAuditEntry> {
        self.module_runtime.lock().await.gating_audit_entries(limit)
    }

    /// Spawn a module member — runs blocking subprocess I/O via `block_in_place`.
    pub async fn spawn_member(
        &self,
        module_id: &str,
        timeout: Duration,
    ) -> Result<(), RuntimeMutationError> {
        let mut rt = self.module_runtime.lock().await;
        run_blocking(|| rt.spawn_member(module_id, timeout))
    }

    /// Route a module call — runs blocking MCP boundary call via `block_in_place`.
    pub async fn route_module_call(
        &self,
        request: &ModuleRouteRequest,
        timeout: Duration,
    ) -> Result<ModuleRouteResponse, ModuleRouteError> {
        let rt = self.module_runtime.lock().await;
        run_blocking(|| route_module_call(&rt, request, timeout))
    }

    pub async fn module_lifecycle_events(&self) -> Vec<LifecycleEvent> {
        self.module_runtime.lock().await.lifecycle_events.clone()
    }

    pub async fn module_health_transitions(&self) -> Vec<ModuleHealthTransition> {
        self.module_runtime.lock().await.supervisor_report.transitions.clone()
    }

    pub async fn module_events(&self) -> Vec<EventEnvelope<UnifiedEvent>> {
        self.module_runtime.lock().await.merged_events().to_vec()
    }

    pub async fn subscribe_events(
        &self,
        request: SubscribeRequest,
    ) -> Result<SubscribeResponse, UnifiedRuntimeError> {
        self.drain_mob_agent_events().await?;
        self.module_runtime.lock().await
            .subscribe_events(request)
            .map_err(UnifiedRuntimeError::Subscribe)
    }
}
