//! HTTP server and route assembly for the unified runtime.

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use meerkat_mob::ids::MeerkatId;

use crate::console_aggregator::{
    ConsoleVisibilityPolicy, HideImplicitDelegateMembersConsoleVisibilityPolicy,
};
use crate::http_console::{
    console_frontend_router, console_json_router_with_runtime_events_and_policy,
};
use crate::http_flow_editor::protected_flow_editor_router_with_runtime_catalog;
use crate::http_sse::{
    agent_events_sse_router_with_access_and_priming, mob_events_sse_router_with_access_and_priming,
    mob_structural_events_sse_router_with_access_and_priming,
};
use crate::runtime::RuntimeDecisionState;
use tower::limit::ConcurrencyLimitLayer;

use super::UnifiedRuntime;

/// Default cap for the stock reference app router.
///
/// SSE routes keep HTTP requests open for the lifetime of the subscription, so
/// a demo-scale cap such as 20 can make the console look frozen while streams
/// occupy all slots. Hosts that wrap MobKit in their own axum service may still
/// choose a different outer limit, but the reference router itself defaults to
/// a ceiling high enough for real console usage.
pub const DEFAULT_REFERENCE_APP_MAX_CONCURRENT_REQUESTS: usize = 1024;

impl UnifiedRuntime {
    pub fn build_console_json_router(&self, decisions: RuntimeDecisionState) -> Router {
        self.build_console_json_router_with_policy(
            decisions,
            Arc::new(HideImplicitDelegateMembersConsoleVisibilityPolicy),
        )
    }

    pub fn build_console_json_router_with_policy(
        &self,
        decisions: RuntimeDecisionState,
        visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    ) -> Router {
        console_json_router_with_runtime_events_and_policy(
            decisions,
            self.mob_runtime.clone(),
            Some(self.module_runtime_handle()),
            self.contact_directory.clone(),
            self.event_log_store(),
            self.gateway_peer_keys().cloned(),
            Some(self.console_events()),
            Some(self.console_log_store()),
            Some(self.mob_events_store()),
            Some(Arc::clone(self.metadata_table())),
            self.identity_runtime().cloned(),
            visibility_policy,
            self.access_controller().cloned(),
        )
    }

    pub fn build_console_frontend_router(&self) -> Router {
        console_frontend_router()
    }

    pub fn build_reference_app_router(&self, decisions: RuntimeDecisionState) -> Router {
        self.build_reference_app_router_with_console_visibility_policy(
            decisions,
            Arc::new(HideImplicitDelegateMembersConsoleVisibilityPolicy),
        )
    }

    pub fn build_reference_app_router_with_console_visibility_policy(
        &self,
        decisions: RuntimeDecisionState,
        visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    ) -> Router {
        let agent_runtime = self.mob_runtime.clone();
        let mob_runtime = self.mob_runtime.clone();
        // Every SSE route shares the same `RuntimeDecisionState` the
        // console RPC route uses: when `require_app_auth` is on, requests
        // must carry a valid bearer / auth_token. Pre-fix, only the
        // structural-events route gated; tier-2 (`/agents/{id}/events`)
        // and tier-3 (`/mob/events`) shipped unauthenticated.
        let flow_editor_decisions = decisions.clone();
        let flow_editor_runtime_catalog = self.mobpack_runtime_catalog_state_snapshot();
        let sse_decisions_a = decisions.clone();
        let sse_decisions_b = decisions.clone();
        let sse_decisions_c = decisions.clone();
        let access = self.access_controller().cloned();
        Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .merge(self.build_console_frontend_router())
            .merge(protected_flow_editor_router_with_runtime_catalog(
                flow_editor_decisions,
                flow_editor_runtime_catalog,
                access.clone(),
            ))
            .merge(self.build_console_json_router_with_policy(decisions, visibility_policy))
            .merge(agent_events_sse_router_with_access_and_priming(
                Arc::new(move |agent_id| {
                    let runtime = agent_runtime.clone();
                    Box::pin(async move {
                        runtime
                            .handle()
                            .subscribe_agent_events(&MeerkatId::from(agent_id))
                            .await
                            .map_err(Into::into)
                    })
                }),
                Some(sse_decisions_a),
                access.clone(),
                access.as_ref().map(|_| self.mob_runtime.clone()),
            ))
            .merge(mob_events_sse_router_with_access_and_priming(
                Arc::new(move || {
                    let mob_runtime = mob_runtime.clone();
                    Box::pin(async move { mob_runtime.handle().subscribe_mob_events().await })
                }),
                Some(sse_decisions_b),
                access.clone(),
                access.as_ref().map(|_| self.mob_runtime.clone()),
            ))
            .merge(mob_structural_events_sse_router_with_access_and_priming(
                self.mob_runtime.handle(),
                self.mob_events_store(),
                Some(sse_decisions_c),
                access.clone(),
                access.as_ref().map(|_| self.mob_runtime.clone()),
            ))
            .layer(ConcurrencyLimitLayer::new(
                DEFAULT_REFERENCE_APP_MAX_CONCURRENT_REQUESTS,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::DEFAULT_REFERENCE_APP_MAX_CONCURRENT_REQUESTS;

    #[test]
    fn reference_router_default_concurrency_allows_sse_fanout() {
        assert_eq!(DEFAULT_REFERENCE_APP_MAX_CONCURRENT_REQUESTS, 1024);
    }
}
