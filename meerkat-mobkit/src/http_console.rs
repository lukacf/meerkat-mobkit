//! HTTP routes for the admin console REST API.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use meerkat_core::ContentInput;
use meerkat_mob::MobState;
use meerkat_mob::{MeerkatId, ProfileName, SpawnMemberSpec};
use serde_json::Value;

use crate::mob_handle_runtime::RealMobRuntime;
use crate::rpc::{JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::runtime::{
    ConsoleAgentLiveSnapshot, ConsoleLiveSnapshot, ConsoleRestJsonRequest, RuntimeDecisionState,
    extract_bearer_token_from_header, handle_console_rest_json_route_with_snapshot,
    validate_console_token,
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
        .route("/console/rpc", post(console_rpc_handler))
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

pub async fn console_rpc_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    // Parse the request early so we can check the method for auth gating.
    let parsed_request = match serde_json::from_value::<JsonRpcRequest>(request) {
        Ok(req) => req,
        Err(_) => {
            return (
                StatusCode::OK,
                Json::<Value>(serde_json::json!({
                    "jsonrpc": JSONRPC_VERSION,
                    "id": Value::Null,
                    "error": { "code": -32600, "message": "Invalid Request" }
                })),
            );
        }
    };

    // Auth enforcement:
    // - When require_app_auth is true: validate bearer token (OIDC + allowlist)
    // - When require_app_auth is false: only allow read-only methods
    //   (mutating operations require auth to be configured)
    if state.decisions.console.require_app_auth {
        // Accept token from Bearer header OR auth_token query param
        let bearer_token = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(extract_bearer_token_from_header)
            .map(String::from);
        let query_token = uri.query().and_then(|q| {
            q.split('&')
                .find_map(|pair| pair.strip_prefix("auth_token=").map(String::from))
        });
        let token_valid = bearer_token
            .or(query_token)
            .is_some_and(|token| validate_console_token(&state.decisions, &token));
        if !token_valid {
            return (
                StatusCode::UNAUTHORIZED,
                Json::<Value>(serde_json::json!({
                    "error": "unauthorized",
                    "reason": "console rpc requires a valid auth token",
                })),
            );
        }
    } else {
        // No auth configured — block mutating methods
        let read_only_methods = [
            "mobkit/status",
            "mobkit/capabilities",
            "mobkit/list_members",
            "mobkit/get_member",
            "mobkit/find_members",
            "mobkit/member_status",
            "mobkit/member_current_session_id",
            "mobkit/member_session_ref",
            "mobkit/collect_completed",
            "mobkit/flow_status",
            "mobkit/query_events",
            "mobkit/cross_mob/directory",
            "mobkit/cross_mob/peer_info",
        ];
        if !read_only_methods.contains(&parsed_request.method.as_str()) {
            return (
                StatusCode::FORBIDDEN,
                Json::<Value>(serde_json::json!({
                    "jsonrpc": JSONRPC_VERSION,
                    "id": parsed_request.id.clone().unwrap_or(Value::Null),
                    "error": {
                        "code": -32600,
                        "message": "mutating RPC methods require console auth to be configured",
                    }
                })),
            );
        }
    }

    let Some(runtime) = &state.runtime else {
        return (
            StatusCode::NOT_FOUND,
            Json::<Value>(serde_json::json!({
                "error": "rpc_unavailable",
                "reason": "console rpc requires a unified runtime",
            })),
        );
    };

    let is_authenticated = state.decisions.console.require_app_auth;
    let response_value =
        handle_console_runtime_rpc(runtime, parsed_request, is_authenticated).await;
    (StatusCode::OK, Json::<Value>(response_value))
}

fn response_value(id: Value, result: Option<Value>, error: Option<JsonRpcError>) -> Value {
    serde_json::to_value(JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id,
        result,
        error,
    })
    .unwrap_or_else(|_| {
        serde_json::json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": Value::Null,
            "error": {
                "code": -32603,
                "message": "serialization failed",
            }
        })
    })
}

fn invalid_params(id: Value, message: impl Into<String>) -> Value {
    response_value(
        id,
        None,
        Some(JsonRpcError {
            code: -32602,
            message: message.into(),
        }),
    )
}

fn internal_error(id: Value, message: impl Into<String>) -> Value {
    response_value(
        id,
        None,
        Some(JsonRpcError {
            code: -32000,
            message: message.into(),
        }),
    )
}

async fn handle_console_runtime_rpc(
    runtime: &RealMobRuntime,
    request: JsonRpcRequest,
    is_authenticated: bool,
) -> Value {
    let response_id = request.id.clone().unwrap_or(Value::Null);

    match request.method.as_str() {
        "mobkit/capabilities" => {
            let mut methods = vec![
                "mobkit/status",
                "mobkit/capabilities",
                "mobkit/list_members",
                "mobkit/get_member",
                "mobkit/find_members",
                "mobkit/member_status",
                "mobkit/member_current_session_id",
                "mobkit/member_session_ref",
                "mobkit/collect_completed",
                "mobkit/flow_status",
                "mobkit/query_events",
            ];
            if is_authenticated {
                methods.extend_from_slice(&[
                    "mobkit/send_message",
                    "mobkit/ensure_member",
                    "mobkit/retire_member",
                    "mobkit/respawn_member",
                    "mobkit/force_cancel_member",
                    "mobkit/cancel_flow",
                    "mobkit/spawn_helper",
                    "mobkit/fork_helper",
                    "mobkit/attach_existing_session",
                    "mobkit/reconcile_edges",
                ]);
            }
            let loaded_modules: Vec<String> = runtime
                .discover()
                .await
                .into_iter()
                .map(|m| m.meerkat_id)
                .collect();
            response_value(
                response_id,
                Some(serde_json::json!({
                    "contract_version": crate::rpc::MOBKIT_CONTRACT_VERSION,
                    "methods": methods,
                    "loaded_modules": loaded_modules,
                    "runtime_capabilities": {
                        "can_send_messages": is_authenticated,
                        "can_retire_members": is_authenticated,
                        "can_spawn_members": is_authenticated,
                    }
                })),
                None,
            )
        }
        "mobkit/status" => {
            let members = runtime.discover().await;
            response_value(
                response_id,
                Some(serde_json::json!({
                    "contract_version": crate::rpc::MOBKIT_CONTRACT_VERSION,
                    "running": matches!(runtime.status(), MobState::Creating | MobState::Running),
                    "loaded_modules": members.iter().map(|member| member.meerkat_id.clone()).collect::<Vec<_>>(),
                })),
                None,
            )
        }
        "mobkit/list_members" => {
            let members = runtime.discover().await;
            response_value(
                response_id,
                Some(serde_json::to_value(members).unwrap_or(Value::Null)),
                None,
            )
        }
        "mobkit/get_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime.get_member(member_id).await {
                Some(snapshot) => response_value(
                    response_id,
                    Some(serde_json::to_value(snapshot).unwrap_or(Value::Null)),
                    None,
                ),
                None => invalid_params(response_id, format!("member not found: {member_id}")),
            }
        }
        "mobkit/find_members" => {
            let Some(label_key) = request.params.get("label_key").and_then(Value::as_str) else {
                return invalid_params(response_id, "label_key required");
            };
            let Some(label_value) = request.params.get("label_value").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "label_value required");
            };
            let matches = runtime.find_members(label_key, label_value).await;
            response_value(
                response_id,
                Some(serde_json::to_value(matches).unwrap_or(Value::Null)),
                None,
            )
        }
        "mobkit/send_message" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            let content =
                if let Some(message) = request.params.get("message").and_then(Value::as_str) {
                    ContentInput::Text(message.to_string())
                } else if let Some(content) = request.params.get("content") {
                    match serde_json::from_value::<ContentInput>(content.clone()) {
                        Ok(content) => content,
                        Err(err) => {
                            return invalid_params(response_id, format!("invalid content: {err}"));
                        }
                    }
                } else {
                    return invalid_params(response_id, "message or content required");
                };
            match runtime.send_message(member_id, content).await {
                Ok(session_id) => response_value(
                    response_id,
                    Some(serde_json::json!({
                        "accepted": true,
                        "member_id": member_id,
                        "session_id": session_id,
                    })),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("send_message failed: {err}")),
            }
        }
        "mobkit/ensure_member" => {
            let Some(profile) = request.params.get("profile").and_then(Value::as_str) else {
                return invalid_params(response_id, "profile required");
            };
            let Some(meerkat_id) = request.params.get("meerkat_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "meerkat_id required");
            };
            let labels = match request.params.get("labels") {
                None | Some(Value::Null) => std::collections::BTreeMap::new(),
                Some(value) => match serde_json::from_value(value.clone()) {
                    Ok(map) => map,
                    Err(err) => {
                        return invalid_params(response_id, format!("invalid labels: {err}"));
                    }
                },
            };
            let context = request.params.get("context").cloned();
            let resume_session_id = request
                .params
                .get("resume_session_id")
                .and_then(Value::as_str)
                .and_then(|s| meerkat_core::types::SessionId::parse(s).ok());
            let additional_instructions: Option<Vec<String>> = request
                .params
                .get("additional_instructions")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                });
            let mut spec =
                SpawnMemberSpec::new(ProfileName::from(profile), MeerkatId::from(meerkat_id));
            if !labels.is_empty() {
                spec = spec.with_labels(labels);
            }
            if let Some(ctx) = context {
                spec = spec.with_context(ctx);
            }
            if let Some(sid) = resume_session_id {
                spec = spec.with_resume_session_id(sid);
            }
            if let Some(instructions) = additional_instructions {
                spec = spec.with_additional_instructions(instructions);
            }
            match runtime.ensure_member(spec).await {
                Ok(snapshot) => response_value(
                    response_id,
                    Some(serde_json::to_value(snapshot).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("ensure_member failed: {err}")),
            }
        }
        "mobkit/retire_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime.retire_member(member_id).await {
                Ok(()) => response_value(
                    response_id,
                    Some(serde_json::json!({ "accepted": true })),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("retire_member failed: {err}")),
            }
        }
        "mobkit/respawn_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime.respawn_member(member_id).await {
                Ok(()) => response_value(
                    response_id,
                    Some(serde_json::json!({ "accepted": true })),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("respawn_member failed: {err}")),
            }
        }
        "mobkit/reconcile_edges" => response_value(
            response_id,
            Some(serde_json::json!({
                "status": "noop",
                "reason": "console runtime routes directly to RealMobRuntime",
            })),
            None,
        ),
        "mobkit/query_events" => response_value(
            response_id,
            Some(serde_json::json!({
                "status": "no_event_log_configured",
                "events": [],
            })),
            None,
        ),
        // 0.5 API methods
        "mobkit/member_status" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime.member_status(member_id).await {
                Ok(snapshot) => response_value(
                    response_id,
                    Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("member_status failed: {err}")),
            }
        }
        "mobkit/force_cancel_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime.force_cancel_member(member_id).await {
                Ok(()) => response_value(
                    response_id,
                    Some(serde_json::json!({ "accepted": true })),
                    None,
                ),
                Err(err) => {
                    internal_error(response_id, format!("force_cancel_member failed: {err}"))
                }
            }
        }
        "mobkit/member_current_session_id" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime.member_current_session_id(member_id).await {
                Ok(session_id) => response_value(
                    response_id,
                    Some(serde_json::json!({
                        "member_id": member_id,
                        "session_id": session_id,
                    })),
                    None,
                ),
                Err(err) => internal_error(
                    response_id,
                    format!("member_current_session_id failed: {err}"),
                ),
            }
        }
        "mobkit/member_session_ref" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime.member_session_ref(member_id).await {
                Ok(session_ref) => response_value(
                    response_id,
                    Some(
                        session_ref
                            .map(|r| serde_json::to_value(&r).unwrap_or(Value::Null))
                            .unwrap_or(Value::Null),
                    ),
                    None,
                ),
                Err(err) => {
                    internal_error(response_id, format!("member_session_ref failed: {err}"))
                }
            }
        }
        "mobkit/collect_completed" => {
            let completed = runtime.collect_completed().await;
            let entries: Vec<Value> = completed
                .into_iter()
                .map(|(member_id, snapshot)| {
                    serde_json::json!({
                        "member_id": member_id,
                        "snapshot": serde_json::to_value(&snapshot).unwrap_or(Value::Null),
                    })
                })
                .collect();
            response_value(
                response_id,
                Some(serde_json::json!({ "completed": entries })),
                None,
            )
        }
        "mobkit/cancel_flow" => {
            let Some(run_id) = request.params.get("run_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "run_id required");
            };
            match runtime.cancel_flow(run_id).await {
                Ok(()) => response_value(
                    response_id,
                    Some(serde_json::json!({ "accepted": true })),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("cancel_flow failed: {err}")),
            }
        }
        "mobkit/flow_status" => {
            let Some(run_id) = request.params.get("run_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "run_id required");
            };
            match runtime.flow_status(run_id).await {
                Ok(Some(mob_run)) => response_value(
                    response_id,
                    Some(serde_json::to_value(&mob_run).unwrap_or(Value::Null)),
                    None,
                ),
                Ok(None) => response_value(response_id, Some(Value::Null), None),
                Err(err) => internal_error(response_id, format!("flow_status failed: {err}")),
            }
        }
        "mobkit/spawn_helper" => {
            let Some(meerkat_id) = request.params.get("meerkat_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "meerkat_id required");
            };
            let Some(task) = request.params.get("task").and_then(Value::as_str) else {
                return invalid_params(response_id, "task required");
            };
            let mut options = meerkat_mob::HelperOptions::default();
            if let Some(o) = request.params.get("options") {
                options.profile_name = o
                    .get("profile")
                    .and_then(Value::as_str)
                    .map(meerkat_mob::ProfileName::from);
            }
            match runtime.spawn_helper(meerkat_id, task, options).await {
                Ok(result) => response_value(
                    response_id,
                    Some(serde_json::json!({
                        "output": result.output,
                        "tokens_used": result.tokens_used,
                        "session_id": result.session_id.map(|s| s.to_string()),
                    })),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("spawn_helper failed: {err}")),
            }
        }
        "mobkit/fork_helper" => {
            let Some(source) = request
                .params
                .get("source_member_id")
                .and_then(Value::as_str)
            else {
                return invalid_params(response_id, "source_member_id required");
            };
            let Some(meerkat_id) = request.params.get("meerkat_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "meerkat_id required");
            };
            let Some(task) = request.params.get("task").and_then(Value::as_str) else {
                return invalid_params(response_id, "task required");
            };
            let fork_context = request
                .params
                .get("fork_context")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let mut options = meerkat_mob::HelperOptions::default();
            if let Some(o) = request.params.get("options") {
                options.profile_name = o
                    .get("profile")
                    .and_then(Value::as_str)
                    .map(meerkat_mob::ProfileName::from);
            }
            match runtime
                .fork_helper(source, meerkat_id, task, fork_context, options)
                .await
            {
                Ok(result) => response_value(
                    response_id,
                    Some(serde_json::json!({
                        "output": result.output,
                        "tokens_used": result.tokens_used,
                        "session_id": result.session_id.map(|s| s.to_string()),
                    })),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("fork_helper failed: {err}")),
            }
        }
        "mobkit/attach_existing_session" => {
            let Some(profile) = request.params.get("profile").and_then(Value::as_str) else {
                return invalid_params(response_id, "profile required");
            };
            let Some(meerkat_id) = request.params.get("meerkat_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "meerkat_id required");
            };
            let Some(session_id) = request.params.get("session_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "session_id required");
            };
            match runtime
                .attach_existing_session(profile, meerkat_id, session_id)
                .await
            {
                Ok(snapshot) => response_value(
                    response_id,
                    Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => internal_error(
                    response_id,
                    format!("attach_existing_session failed: {err}"),
                ),
            }
        }
        _ => response_value(
            response_id,
            None,
            Some(JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
            }),
        ),
    }
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
    let mut agents = members
        .iter()
        .map(|member| ConsoleAgentLiveSnapshot {
            agent_id: member.meerkat_id.clone(),
            member_id: member.meerkat_id.clone(),
            label: member.meerkat_id.clone(),
            kind: "meerkat".to_string(),
            profile: Some(member.profile.clone()),
            state: Some(member.state.clone()),
            session_id: member.session_id.clone(),
        })
        .collect::<Vec<_>>();
    agents.sort_by(|left, right| left.label.cmp(&right.label));
    ConsoleLiveSnapshot::new(
        Some(runtime.handle().mob_id().to_string()),
        running,
        loaded_modules,
        agents,
        members,
        true,
    )
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
