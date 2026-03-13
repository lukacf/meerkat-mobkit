//! HTTP server and route assembly for the unified runtime.

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use meerkat_mob::MeerkatId;

use crate::http_console::{console_frontend_router, console_json_router_with_runtime};
use crate::http_interactions::interaction_stream_router;
use crate::http_sse::{agent_events_sse_router, mob_events_sse_router};
use crate::runtime::RuntimeDecisionState;

use super::UnifiedRuntime;

impl UnifiedRuntime {
    pub fn build_console_json_router(&self, decisions: RuntimeDecisionState) -> Router {
        console_json_router_with_runtime(decisions, self.mob_runtime.clone())
    }

    pub fn build_console_frontend_router(&self) -> Router {
        console_frontend_router()
    }

    pub fn build_reference_app_router(&self, decisions: RuntimeDecisionState) -> Router {
        let agent_runtime = self.mob_runtime.clone();
        let mob_runtime = self.mob_runtime.clone();
        Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .merge(self.build_console_frontend_router())
            .merge(self.build_console_json_router(decisions))
            .merge(agent_events_sse_router(Arc::new(move |agent_id| {
                let runtime = agent_runtime.clone();
                Box::pin(async move {
                    runtime
                        .handle()
                        .subscribe_agent_events(&MeerkatId::from(agent_id))
                        .await
                        .map_err(Into::into)
                })
            })))
            .merge(mob_events_sse_router(Arc::new(move || {
                let router_handle = mob_runtime.handle().subscribe_mob_events();
                Box::pin(async move { router_handle })
            })))
            .merge(interaction_stream_router(self.mob_runtime.clone()))
    }
}
