//! HTTP routes for the admin console REST API.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use meerkat_mob::MobState;
use serde_json::Value;

use crate::mob_handle_runtime::RealMobRuntime;
use crate::runtime::{
    ConsoleLiveSnapshot, ConsoleRestJsonRequest, RuntimeDecisionState,
    extract_bearer_token_from_header, handle_console_rest_json_route_with_snapshot,
};

#[derive(Clone)]
pub struct ConsoleJsonState {
    pub decisions: RuntimeDecisionState,
    pub runtime: Option<RealMobRuntime>,
}

const CONSOLE_FRONTEND_INDEX_HTML: &str = include_str!("../console-dist/index.html");
const CONSOLE_FRONTEND_APP_JS: &str = include_str!("../console-dist/console-app.js");

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
    headers: HeaderMap,
    uri: Uri,
) -> impl IntoResponse {
    let mut path = uri
        .path_and_query()
        .map(|path_and_query| path_and_query.as_str().to_string())
        .unwrap_or_else(|| uri.path().to_string());

    // If the request carries a Bearer token and the URL doesn't already have
    // an auth_token query param, inject it so the console-ingress auth
    // resolver can validate it through the existing query-param path.
    //
    // JWT tokens use base64url characters (A-Za-z0-9_-.) plus optional '='
    // padding.  split_path_and_query uses split_once('=') for key/value
    // separation, so '=' in the token body lands in the value side correctly
    // and '&' never appears in valid JWTs, so no percent-encoding is needed.
    if !path.contains("auth_token=")
        && let Some(bearer) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(extract_bearer_token_from_header)
        && bearer.bytes().all(|b| b != b'&')
    {
        let sep = if path.contains('?') { '&' } else { '?' };
        path = format!("{path}{sep}auth_token={bearer}");
    }

    let config_module_ids: Vec<String> = state
        .decisions
        .modules
        .iter()
        .map(|m| m.id.clone())
        .collect();
    let live_snapshot = match &state.runtime {
        Some(runtime) => Some(build_live_snapshot(runtime, &config_module_ids).await),
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

async fn build_live_snapshot(
    runtime: &RealMobRuntime,
    config_module_ids: &[String],
) -> ConsoleLiveSnapshot {
    let running = matches!(runtime.status(), MobState::Creating | MobState::Running);
    let members = runtime.discover().await;
    // Use config module IDs for loaded_modules when available (correct for
    // topology/health which show modules, not individual mob agents).
    // Fall back to member IDs for pure mob runtimes with no config modules.
    let loaded_modules = if config_module_ids.is_empty() {
        let mut mods: Vec<String> = members.iter().map(|m| m.meerkat_id.clone()).collect();
        mods.sort();
        mods
    } else {
        let mut mods = config_module_ids.to_vec();
        mods.sort();
        mods
    };
    ConsoleLiveSnapshot::new(running, loaded_modules, members, true)
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
