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

impl UnifiedRuntime {
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

    pub fn subscribe_events(
        &mut self,
        request: SubscribeRequest,
    ) -> Result<SubscribeResponse, UnifiedRuntimeError> {
        self.drain_mob_agent_events()?;
        self.module_runtime
            .subscribe_events(request)
            .map_err(UnifiedRuntimeError::Subscribe)
    }
}
