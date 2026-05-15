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
use crate::http_sse::{
    agent_events_sse_router, mob_events_sse_router, mob_structural_events_sse_router,
};
use crate::runtime::RuntimeDecisionState;

use super::UnifiedRuntime;

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
            visibility_policy,
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
        let sse_decisions_a = decisions.clone();
        let sse_decisions_b = decisions.clone();
        let sse_decisions_c = decisions.clone();
        Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .merge(self.build_console_frontend_router())
            .merge(self.build_console_json_router_with_policy(decisions, visibility_policy))
            .merge(agent_events_sse_router(
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
            ))
            .merge(mob_events_sse_router(
                Arc::new(move || {
                    let mob_runtime = mob_runtime.clone();
                    Box::pin(async move { mob_runtime.handle().subscribe_mob_events().await })
                }),
                Some(sse_decisions_b),
            ))
            .merge(mob_structural_events_sse_router(
                self.mob_runtime.handle(),
                self.mob_events_store(),
                Some(sse_decisions_c),
            ))
    }
}
