//! HTTP server and route assembly for the unified runtime.

use axum::routing::get;
use axum::Router;

use crate::http_console::{console_frontend_router, console_json_router_with_runtime};
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
        Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .merge(self.build_console_frontend_router())
            .merge(self.build_console_json_router(decisions))
    }
}
