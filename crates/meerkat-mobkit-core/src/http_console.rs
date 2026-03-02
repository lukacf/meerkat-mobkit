use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use meerkat_mob::MobState;
use serde_json::Value;

use crate::http_sse::interaction_sse_router;
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

pub fn build_reference_app_router(
    decisions: RuntimeDecisionState,
    runtime: RealMobRuntime,
) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(console_json_router_with_runtime(decisions, runtime.clone()))
        .merge(interaction_sse_router(runtime))
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
