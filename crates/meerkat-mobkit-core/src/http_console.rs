use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use meerkat_mob::MobState;
use serde_json::Value;

use crate::mob_handle_runtime::RealMobRuntime;
use crate::runtime::{
    handle_console_rest_json_route_with_snapshot, ConsoleLiveSnapshot, ConsoleRestJsonRequest,
    RuntimeDecisionState,
};

#[derive(Clone)]
pub struct ConsoleJsonState {
    pub decisions: RuntimeDecisionState,
    pub runtime: Option<RealMobRuntime>,
}

const CONSOLE_FRONTEND_INDEX_HTML: &str = include_str!("../../../console/dist/index.html");
const CONSOLE_FRONTEND_APP_JS: &str = include_str!("../../../console/dist/console-app.js");

pub fn console_json_router(decisions: RuntimeDecisionState) -> Router {
    console_json_router_with_state(ConsoleJsonState {
        decisions,
        runtime: None,
    })
}

pub fn console_json_router_with_runtime(
    decisions: RuntimeDecisionState,
    runtime: RealMobRuntime,
) -> Router {
    console_json_router_with_state(ConsoleJsonState {
        decisions,
        runtime: Some(runtime),
    })
}

pub fn console_frontend_router() -> Router {
    Router::new()
        .route("/console", get(console_frontend_index_handler))
        .route("/console/", get(console_frontend_index_handler))
        .route(
            "/console/assets/console-app.js",
            get(console_frontend_app_js_handler),
        )
}

fn console_json_router_with_state(state: ConsoleJsonState) -> Router {
    Router::new()
        .route("/console/experience", get(console_json_handler))
        .route("/console/modules", get(console_json_handler))
        .with_state(state)
}

pub async fn console_json_handler(
    State(state): State<ConsoleJsonState>,
    uri: Uri,
) -> impl IntoResponse {
    let path = uri
        .path_and_query()
        .map(|path_and_query| path_and_query.as_str().to_string())
        .unwrap_or_else(|| uri.path().to_string());

    let live_snapshot = match &state.runtime {
        Some(runtime) => Some(build_live_snapshot(runtime).await),
        None => None,
    };

    let response = handle_console_rest_json_route_with_snapshot(
        &state.decisions,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path,
            auth: None,
        },
        live_snapshot.as_ref(),
    );
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json::<Value>(response.body))
}

async fn build_live_snapshot(runtime: &RealMobRuntime) -> ConsoleLiveSnapshot {
    let running = matches!(runtime.status(), MobState::Creating | MobState::Running);
    let mut loaded_modules = runtime
        .discover()
        .await
        .into_iter()
        .map(|member| member.meerkat_id)
        .collect::<Vec<_>>();
    loaded_modules.sort();
    ConsoleLiveSnapshot::new(running, loaded_modules)
}

pub async fn console_frontend_index_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        CONSOLE_FRONTEND_INDEX_HTML,
    )
}

pub async fn console_frontend_app_js_handler() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        CONSOLE_FRONTEND_APP_JS,
    )
}
