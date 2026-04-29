//! RPC handler implementations for mob member operations.

use meerkat_core::ContentInput;
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::launch::{ForkContext, MemberLaunchMode};
use meerkat_mob::runtime::reconcile::MemberFilter;
use meerkat_mob::{HelperOptions, MobBackendKind, MobRuntimeMode, ProfileName, SpawnMemberSpec};
use serde_json::Value;

use crate::mob_handle_runtime::{member_entry_to_json, send_message_on_mob};
use crate::unified_runtime::UnifiedRuntime;

use super::{JSONRPC_VERSION, JsonRpcError, JsonRpcResponse};

/// Parse HelperOptions from an optional JSON "options" object.
pub(crate) fn parse_helper_options(options_val: Option<&Value>) -> Result<HelperOptions, String> {
    let mut opts = HelperOptions::default();
    if let Some(o) = options_val {
        opts.role_name = o.get("role").and_then(Value::as_str).map(ProfileName::from);
        if let Some(mode_str) = o.get("runtime_mode").and_then(Value::as_str) {
            opts.runtime_mode = Some(
                serde_json::from_value::<MobRuntimeMode>(Value::String(mode_str.to_string()))
                    .map_err(|_| {
                        format!(
                            "invalid runtime_mode '{mode_str}': \
                             expected 'autonomous_host' or 'turn_driven'"
                        )
                    })?,
            );
        }
        if let Some(backend_str) = o.get("backend").and_then(Value::as_str) {
            opts.backend = Some(
                serde_json::from_value::<MobBackendKind>(Value::String(backend_str.to_string()))
                    .map_err(|_| format!("invalid backend '{backend_str}'"))?,
            );
        }
    }
    Ok(opts)
}

/// Extract content from params as `ContentInput`.
///
/// Accepts either:
/// - `"message": "plain text"` (string — backwards-compatible)
/// - `"content": "plain text"` (string shorthand)
/// - `"content": [{"type":"text","text":"..."},{"type":"image",...}]` (multimodal blocks)
///
/// `message` takes precedence if both are present.
fn extract_content(params: &Value) -> Option<ContentInput> {
    if let Some(s) = params.get("message").and_then(Value::as_str)
        && !s.is_empty()
    {
        return Some(ContentInput::Text(s.to_string()));
    }
    if let Some(content_val) = params.get("content")
        && let Ok(input) = serde_json::from_value::<ContentInput>(content_val.clone())
    {
        return Some(input);
    }
    None
}

pub(super) async fn handle_send_message(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    let content = extract_content(params);

    match (member_id, content) {
        (Some(member_id), Some(content)) if !member_id.is_empty() => {
            match send_message_on_mob(&runtime.mob_handle(), member_id, content).await {
                Ok(session_id) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "member_id": member_id,
                        "session_id": session_id
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("send_message failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: member_id and message (or content) required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_find_members(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let label_key = params.get("label_key").and_then(Value::as_str);
    let label_value = params.get("label_value").and_then(Value::as_str);

    match (label_key, label_value) {
        (Some(key), Some(value)) if !key.is_empty() => {
            let filter = MemberFilter {
                labels: std::collections::BTreeMap::from([(key.to_string(), value.to_string())]),
                role: None,
                state: None,
            };
            let handle = runtime.mob_handle();
            let entries = handle.list_members_matching(filter).await;
            let mut members = Vec::with_capacity(entries.len());
            for entry in &entries {
                members.push(member_entry_to_json(entry));
            }
            JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(Value::Array(members)),
                error: None,
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: label_key and label_value required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_ensure_member(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let role = params.get("role").and_then(Value::as_str);
    let agent_identity = params.get("agent_identity").and_then(Value::as_str);

    match (role, agent_identity) {
        (Some(role), Some(agent_identity)) if !role.is_empty() && !agent_identity.is_empty() => {
            let labels = match params.get("labels") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    match serde_json::from_value::<std::collections::BTreeMap<String, String>>(
                        v.clone(),
                    ) {
                        Ok(map) => Some(map),
                        Err(err) => {
                            return JsonRpcResponse {
                                jsonrpc: JSONRPC_VERSION.to_string(),
                                id: response_id,
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32602,
                                    message: format!(
                                        "Invalid params: labels must be a map of string to string: {err}"
                                    ),
                                    data: None,
                                }),
                            };
                        }
                    }
                }
            };
            let context = params.get("context").cloned();
            let resume_session_id = match params.get("resume_session_id") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    let s = match v.as_str() {
                        Some(s) => s,
                        None => {
                            return JsonRpcResponse {
                                jsonrpc: JSONRPC_VERSION.to_string(),
                                id: response_id,
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32602,
                                    message: "Invalid params: resume_session_id must be a string"
                                        .to_string(),
                                    data: None,
                                }),
                            };
                        }
                    };
                    match meerkat_core::types::SessionId::parse(s) {
                        Ok(sid) => Some(sid),
                        Err(_) => {
                            return JsonRpcResponse {
                                jsonrpc: JSONRPC_VERSION.to_string(),
                                id: response_id,
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32602,
                                    message: format!(
                                        "Invalid params: resume_session_id is not a valid session ID: {s}"
                                    ),
                                    data: None,
                                }),
                            };
                        }
                    }
                }
            };
            let additional_instructions = match params.get("additional_instructions") {
                None | Some(Value::Null) => None,
                Some(Value::Array(arr)) => {
                    let mut strs = Vec::with_capacity(arr.len());
                    for (i, entry) in arr.iter().enumerate() {
                        match entry.as_str() {
                            Some(s) => strs.push(s.to_string()),
                            None => {
                                return JsonRpcResponse {
                                    jsonrpc: JSONRPC_VERSION.to_string(),
                                    id: response_id,
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: -32602,
                                        message: format!(
                                            "Invalid params: additional_instructions[{i}] must be a string"
                                        ),
                                        data: None,
                                    }),
                                };
                            }
                        }
                    }
                    if strs.is_empty() { None } else { Some(strs) }
                }
                Some(_) => {
                    return JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: "Invalid params: additional_instructions must be an array of strings".to_string(),
                    data: None,
                }),
                    };
                }
            };

            let mut spec =
                SpawnMemberSpec::new(ProfileName::from(role), MeerkatId::from(agent_identity));
            if let Some(context) = context {
                spec = spec.with_context(context);
            }
            if let Some(labels) = labels {
                spec = spec.with_labels(labels);
            }
            if let Some(sid) = resume_session_id {
                spec = spec.with_resume_bridge_session_id(sid);
            }
            if let Some(instructions) = additional_instructions {
                spec = spec.with_additional_instructions(instructions);
            }
            let handle = runtime.mob_handle();
            let mid = spec.identity.clone();
            match handle.ensure_member(spec).await {
                Ok(_outcome) => {
                    let entries = handle.list_members_including_retiring().await;
                    let entry = entries.into_iter().find(|e| e.agent_identity == mid);
                    let result = match entry {
                        Some(e) => member_entry_to_json(&e),
                        None => Value::Null,
                    };
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(result),
                        error: None,
                    }
                }
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("ensure_member failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: role and agent_identity required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_list_members(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
    let handle = runtime.mob_handle();
    let entries = handle.list_members_including_retiring().await;
    let mut members = Vec::with_capacity(entries.len());
    for entry in &entries {
        members.push(member_entry_to_json(entry));
    }
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: Some(Value::Array(members)),
        error: None,
    }
}

pub(super) async fn handle_get_member(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => {
            let handle = runtime.mob_handle();
            let identity = MeerkatId::from(mid);
            let entries = handle.list_members_including_retiring().await;
            match entries.into_iter().find(|e| e.agent_identity == identity) {
                Some(entry) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(member_entry_to_json(&entry)),
                    error: None,
                },
                None => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("member not found: {mid}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: member_id required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_retire_member(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => {
            match runtime.mob_handle().retire(MeerkatId::from(mid)).await {
                Ok(()) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({"accepted": true})),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("retire_member failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: member_id required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_respawn_member(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => {
            match runtime
                .mob_handle()
                .respawn(MeerkatId::from(mid), None)
                .await
            {
                Ok(_receipt) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({"accepted": true})),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("respawn_member failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: member_id required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_reconcile_edges(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
    let report = runtime.reconcile_edges().await;
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: Some(serde_json::to_value(&report).unwrap_or(Value::Null)),
        error: None,
    }
}

pub(super) async fn handle_rediscover(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
    match runtime.rediscover().await {
        Ok(Some(report)) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(serde_json::to_value(&report).unwrap_or(Value::Null)),
            error: None,
        },
        Ok(None) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(serde_json::json!({
                "status": "no_discovery_configured"
            })),
            error: None,
        },
        Err(err) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: format!("rediscover failed: {err}"),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_query_events(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: Value,
) -> JsonRpcResponse {
    let query: crate::unified_runtime::EventQuery = match serde_json::from_value(params) {
        Ok(q) => q,
        Err(err) => {
            return JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {err}"),
                    data: None,
                }),
            };
        }
    };
    match runtime.query_events(query.clone()).await {
        Some(Ok(events)) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(serde_json::to_value(&events).unwrap_or(Value::Null)),
            error: None,
        },
        Some(Err(err)) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: format!("query_events failed: {err}"),
                data: None,
            }),
        },
        None => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(serde_json::json!({
                "status": "no_event_log_configured",
                "events": runtime.query_console_events(&query).await,
            })),
            error: None,
        },
    }
}

fn parse_mob_events_query(
    response_id: &Value,
    params: Value,
) -> Result<crate::unified_runtime::EventQuery, Box<JsonRpcResponse>> {
    if params.is_null() {
        return Ok(crate::unified_runtime::EventQuery::default());
    }
    serde_json::from_value(params).map_err(|err| {
        Box::new(JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id.clone(),
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: format!("Invalid params: {err}"),
                data: None,
            }),
        })
    })
}

/// JSON-RPC error code reported when the caller passes an `after_seq`
/// past the current ledger frontier. The error `data` field carries
/// `{ after_cursor, latest_cursor }` so SDKs can surface a typed
/// exception.
pub(super) const MOB_EVENTS_STALE_CURSOR_CODE: i64 = -32010;

fn stale_cursor_response(
    response_id: Value,
    after_cursor: u64,
    latest_cursor: u64,
) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: None,
        error: Some(JsonRpcError {
            code: MOB_EVENTS_STALE_CURSOR_CODE,
            message: format!(
                "stale mob event cursor: requested {after_cursor}, latest {latest_cursor}"
            ),
            data: Some(serde_json::json!({
                "error": "event_query_stale",
                "after_cursor": after_cursor,
                "latest_cursor": latest_cursor,
            })),
        }),
    }
}

/// Handle `mobkit/mob_events/query` — return structural mob events
/// matching the supplied [`EventQuery`] by scanning the meerkat ledger.
pub(super) async fn handle_mob_events_query(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: Value,
) -> JsonRpcResponse {
    let query = match parse_mob_events_query(&response_id, params) {
        Ok(q) => q,
        Err(response) => return *response,
    };
    match runtime.query_mob_events(&query).await {
        Ok(events) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(serde_json::json!({
                "events": serde_json::to_value(&events).unwrap_or(Value::Null),
                "next_after_seq": events.last().map(|event| event.cursor),
            })),
            error: None,
        },
        Err(crate::unified_runtime::mob_events::MobEventsQueryError::Stale {
            after_cursor,
            latest_cursor,
        }) => stale_cursor_response(response_id, after_cursor, latest_cursor),
        Err(err) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: format!("mob_events/query failed: {err}"),
                data: None,
            }),
        },
    }
}

/// Handle `mobkit/mob_events/subscribe` — JSON-RPC handshake that
/// returns a snapshot of structural events plus a `subscribe_url`
/// pointing to the SSE route. Pre-handshake validation rejects stale
/// cursors with the typed `-32010` error before any SSE connection is
/// opened.
pub(super) async fn handle_mob_events_subscribe(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: Value,
) -> JsonRpcResponse {
    let query = match parse_mob_events_query(&response_id, params) {
        Ok(q) => q,
        Err(response) => return *response,
    };
    match runtime.query_mob_events(&query).await {
        Ok(events) => {
            let last_cursor = events.last().map(|event| event.cursor);
            JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(serde_json::json!({
                    "stream": "mob_events",
                    "events": serde_json::to_value(&events).unwrap_or(Value::Null),
                    "next_after_seq": last_cursor,
                    "subscribe_url": "/mobkit/mob_events/stream",
                    "keep_alive": {
                        "interval_ms": 15_000_u64,
                        "event": "keep_alive",
                    },
                })),
                error: None,
            }
        }
        Err(crate::unified_runtime::mob_events::MobEventsQueryError::Stale {
            after_cursor,
            latest_cursor,
        }) => stale_cursor_response(response_id, after_cursor, latest_cursor),
        Err(err) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: format!("mob_events/subscribe failed: {err}"),
                data: None,
            }),
        },
    }
}

// ---------------------------------------------------------------------------
// Cross-mob operations
// ---------------------------------------------------------------------------

pub(super) async fn handle_cross_mob_wire(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let local_member_id = params.get("local_member_id").and_then(Value::as_str);
    let remote_member_id = params.get("remote_member_id").and_then(Value::as_str);
    let remote_mob_id = params.get("remote_mob_id").and_then(Value::as_str);

    match (local_member_id, remote_member_id, remote_mob_id) {
        (Some(local), Some(remote), Some(mob))
            if !local.is_empty() && !remote.is_empty() && !mob.is_empty() =>
        {
            match runtime.wire_cross_mob(local, remote, mob).await {
                Ok(()) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "local_member_id": local,
                        "remote_member_id": remote,
                        "remote_mob_id": mob,
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("cross_mob/wire failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message:
                    "Invalid params: local_member_id, remote_member_id, and remote_mob_id required"
                        .to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_cross_mob_unwire(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let local_member_id = params.get("local_member_id").and_then(Value::as_str);
    let remote_member_id = params.get("remote_member_id").and_then(Value::as_str);
    let remote_mob_id = params.get("remote_mob_id").and_then(Value::as_str);

    match (local_member_id, remote_member_id, remote_mob_id) {
        (Some(local), Some(remote), Some(mob))
            if !local.is_empty() && !remote.is_empty() && !mob.is_empty() =>
        {
            match runtime.unwire_cross_mob(local, remote, mob).await {
                Ok(()) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "local_member_id": local,
                        "remote_member_id": remote,
                        "remote_mob_id": mob,
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("cross_mob/unwire failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message:
                    "Invalid params: local_member_id, remote_member_id, and remote_mob_id required"
                        .to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_cross_mob_send(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let from_member_id = params.get("from_member_id").and_then(Value::as_str);
    let remote_member_id = params.get("remote_member_id").and_then(Value::as_str);
    let remote_mob_id = params.get("remote_mob_id").and_then(Value::as_str);
    let content = extract_content(params);

    match (from_member_id, remote_member_id, remote_mob_id, content) {
        (Some(from), Some(remote), Some(mob), Some(content))
            if !from.is_empty() && !remote.is_empty() && !mob.is_empty() =>
        {
            match runtime.send_cross_mob(from, remote, mob, content).await {
                Ok(session_id) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "from_member_id": from,
                        "remote_member_id": remote,
                        "remote_mob_id": mob,
                        "session_id": session_id,
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("cross_mob/send failed: {err}"),
                    data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: from_member_id, remote_member_id, remote_mob_id, and message (or content) required".to_string(),
                    data: None,
                }),
        },
    }
}

pub(super) async fn handle_cross_mob_directory(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
    let entries: Vec<Value> = runtime
        .list_external_mobs()
        .into_iter()
        .filter_map(|e| serde_json::to_value(&e).ok())
        .collect();
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: Some(serde_json::json!({ "mobs": entries })),
        error: None,
    }
}

/// Return comms peer info for a local member — used by the Python SDK
/// to build `TrustedPeerSpec` for cross-mob wiring.
pub(super) async fn handle_cross_mob_peer_info(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => match runtime.local_member_peer_info(mid).await {
            Ok((peer_id, comms_name, address)) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(serde_json::json!({
                    "member_id": mid,
                    "mob_id": runtime.mob_id(),
                    "comms_name": comms_name,
                    "peer_id": peer_id,
                    "address": address,
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32000,
                    message: format!("cross_mob/peer_info failed: {err}"),
                    data: None,
                }),
            },
        },
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: member_id required".to_string(),
                data: None,
            }),
        },
    }
}

// ---------------------------------------------------------------------------
// 0.5 API surface handlers
// ---------------------------------------------------------------------------

pub(super) async fn handle_member_status(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => {
            match runtime
                .mob_handle()
                .member_status(&MeerkatId::from(mid))
                .await
            {
                Ok(snapshot) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("member_status failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: member_id required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_force_cancel_member(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => {
            match runtime
                .mob_handle()
                .force_cancel_member(MeerkatId::from(mid))
                .await
            {
                Ok(()) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({"accepted": true})),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("force_cancel_member failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: member_id required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_spawn_helper(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent_identity = params.get("agent_identity").and_then(Value::as_str);
    let task = params.get("task").and_then(Value::as_str);

    match (agent_identity, task) {
        (Some(mid), Some(task_str)) if !mid.is_empty() && !task_str.is_empty() => {
            let options = match parse_helper_options(params.get("options")) {
                Ok(opts) => opts,
                Err(msg) => {
                    return JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: format!("Invalid params: {msg}"),
                            data: None,
                        }),
                    };
                }
            };
            let handle = runtime.mob_handle();
            match handle
                .spawn_helper(MeerkatId::from(mid), task_str, options)
                .await
            {
                Ok(result) => {
                    // Note: meerkat 0.6's `spawn_helper` retires the helper
                    // before returning, so `resolve_bridge_session_id` would
                    // come back `None` here. We drop `session_id` from the
                    // response rather than silently emit `null`. If meerkat
                    // grows `HelperResult.bridge_session_id` in a future
                    // release, we'll re-add it.
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({
                            "output": result.output,
                            "tokens_used": result.tokens_used,
                        })),
                        error: None,
                    }
                }
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("spawn_helper failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: agent_identity and task required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_fork_helper(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let source_member_id = params.get("source_member_id").and_then(Value::as_str);
    let agent_identity = params.get("agent_identity").and_then(Value::as_str);
    let task = params.get("task").and_then(Value::as_str);
    let fork_ctx_val = params.get("fork_context").cloned();

    match (source_member_id, agent_identity, task) {
        (Some(source), Some(mid), Some(task_str))
            if !source.is_empty() && !mid.is_empty() && !task_str.is_empty() =>
        {
            let fork_context = match fork_ctx_val {
                Some(v) if !v.is_null() => match serde_json::from_value::<ForkContext>(v) {
                    Ok(fc) => fc,
                    Err(err) => {
                        return JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32602,
                                message: format!("Invalid params: fork_context: {err}"),
                                data: None,
                            }),
                        };
                    }
                },
                _ => ForkContext::default(),
            };
            let options = match parse_helper_options(params.get("options")) {
                Ok(opts) => opts,
                Err(msg) => {
                    return JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: format!("Invalid params: {msg}"),
                            data: None,
                        }),
                    };
                }
            };
            let handle = runtime.mob_handle();
            match handle
                .fork_helper(
                    &MeerkatId::from(source),
                    MeerkatId::from(mid),
                    task_str,
                    fork_context,
                    options,
                )
                .await
            {
                Ok(result) => {
                    // See `handle_spawn_helper`: meerkat 0.6 retires the
                    // forked helper before returning, so session_id is
                    // omitted rather than silently null.
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({
                            "output": result.output,
                            "tokens_used": result.tokens_used,
                        })),
                        error: None,
                    }
                }
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("fork_helper failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: source_member_id, agent_identity, and task required"
                    .to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_attach_existing_session(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let role = params.get("role").and_then(Value::as_str);
    let agent_identity = params.get("agent_identity").and_then(Value::as_str);
    let session_id = params.get("session_id").and_then(Value::as_str);

    match (role, agent_identity, session_id) {
        (Some(role), Some(mid), Some(sid))
            if !role.is_empty() && !mid.is_empty() && !sid.is_empty() =>
        {
            let bridge_session_id = match meerkat_core::types::SessionId::parse(sid) {
                Ok(s) => s,
                Err(_) => {
                    return JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: "Invalid params: session_id must be a valid session ID"
                                .to_string(),
                            data: None,
                        }),
                    };
                }
            };
            let identity = MeerkatId::from(mid);
            let spec = SpawnMemberSpec::new(ProfileName::from(role), identity.clone())
                .with_launch_mode(MemberLaunchMode::Resume { bridge_session_id });
            let handle = runtime.mob_handle();
            match handle.spawn_spec(spec).await {
                Ok(_) => match handle.member_status(&identity).await {
                    Ok(snapshot) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32000,
                            message: format!("attach_existing_session status lookup failed: {err}"),
                            data: None,
                        }),
                    },
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("attach_existing_session failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: role, agent_identity, and session_id required"
                    .to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_list_flows(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
    let flows: Vec<String> = runtime
        .mob_handle()
        .list_flows()
        .into_iter()
        .map(|id| id.to_string())
        .collect();
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: Some(serde_json::json!({ "flows": flows })),
        error: None,
    }
}

/// Handle `mobkit/list_runs` — return [`meerkat_mob::run::MobRun`]
/// records for this mob, optionally filtered by `flow_id`. Each run
/// serializes via meerkat's existing `Serialize` impl, so the response
/// carries the full ledger projection (`step_ledger`, `failure_ledger`,
/// `frames`, `loops`, `loop_iteration_ledger`, `flow_state`, etc.).
pub(super) async fn handle_list_runs(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let flow_id = params
        .get("flow_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(meerkat_mob::FlowId::from);
    match runtime.mob_handle().list_runs(flow_id.as_ref()).await {
        Ok(runs) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(serde_json::json!({
                "runs": serde_json::to_value(&runs).unwrap_or(Value::Null),
            })),
            error: None,
        },
        Err(err) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: format!("list_runs failed: {err}"),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_run_flow(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let flow_id_str = params.get("flow_id").and_then(Value::as_str);
    let flow_params = params.get("params").cloned().unwrap_or(Value::Null);

    match flow_id_str {
        Some(fid) if !fid.is_empty() => {
            let flow_id = meerkat_mob::FlowId::from(fid);
            match runtime.mob_handle().run_flow(flow_id, flow_params).await {
                Ok(run_id) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({ "run_id": run_id.to_string() })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("run_flow failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: flow_id required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_cancel_flow(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let run_id = params.get("run_id").and_then(Value::as_str);
    match run_id {
        Some(rid) if !rid.is_empty() => {
            let run_id: meerkat_mob::RunId = match rid.parse() {
                Ok(id) => id,
                Err(_) => {
                    return JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: "Invalid params: run_id not a valid run id".to_string(),
                            data: None,
                        }),
                    };
                }
            };
            match runtime.mob_handle().cancel_flow(run_id).await {
                Ok(()) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({"accepted": true})),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("cancel_flow failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: run_id required".to_string(),
                data: None,
            }),
        },
    }
}

pub(super) async fn handle_flow_status(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let run_id = params.get("run_id").and_then(Value::as_str);
    match run_id {
        Some(rid) if !rid.is_empty() => {
            let run_id: meerkat_mob::RunId = match rid.parse() {
                Ok(id) => id,
                Err(_) => {
                    return JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: "Invalid params: run_id not a valid run id".to_string(),
                            data: None,
                        }),
                    };
                }
            };
            match runtime.mob_handle().flow_status(run_id).await {
                Ok(Some(mob_run)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(&mob_run).unwrap_or(Value::Null)),
                    error: None,
                },
                Ok(None) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(Value::Null),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("flow_status failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: run_id required".to_string(),
                data: None,
            }),
        },
    }
}

/// Wait until all current mob members are startup-ready for orchestration.
///
/// Relays meerkat 0.6's `MobHandle::wait_for_ready`. Returns a map of
/// `{ ready: [{ agent_identity, snapshot }], timeout: bool }`. The
/// `timeout` flag distinguishes "all members converged" from "partial
/// readiness, hit the wall".
pub(super) async fn handle_wait_ready(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let timeout = params
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .map(std::time::Duration::from_millis);
    match runtime.mob_handle().wait_for_ready(timeout).await {
        Ok(ready) => {
            let entries: Vec<Value> = ready
                .into_iter()
                .map(|(identity, snapshot)| {
                    serde_json::json!({
                        "agent_identity": identity.to_string(),
                        "snapshot": serde_json::to_value(&snapshot).unwrap_or(Value::Null),
                    })
                })
                .collect();
            JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(serde_json::json!({
                    "ready": entries,
                    "timeout": false,
                })),
                error: None,
            }
        }
        Err(err) => {
            let message = err.to_string();
            let timed_out = message.to_lowercase().contains("timeout");
            if timed_out {
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "ready": Vec::<Value>::new(),
                        "timeout": true,
                    })),
                    error: None,
                }
            } else {
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("wait_for_ready failed: {message}"),
                        data: None,
                    }),
                }
            }
        }
    }
}

pub(super) async fn handle_collect_completed(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
    let completed = runtime.mob_handle().collect_completed().await;
    let entries: Vec<Value> = completed
        .into_iter()
        .map(|(mid, snapshot)| {
            serde_json::json!({
                "member_id": mid.to_string(),
                "snapshot": serde_json::to_value(&snapshot).unwrap_or(Value::Null),
            })
        })
        .collect();
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: Some(serde_json::json!({ "completed": entries })),
        error: None,
    }
}

pub(super) async fn handle_read_session_history(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let session_id = params.get("session_id").and_then(Value::as_str);
    let offset = params
        .get("offset")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(0);
    let limit = match params.get("limit") {
        Some(Value::Number(number)) => number.as_u64().map(|value| value as usize),
        Some(Value::Null) | None => None,
        Some(_) => {
            return JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: "Invalid params: limit must be a positive integer".to_string(),
                    data: None,
                }),
            };
        }
    };

    match session_id {
        Some(sid) if !sid.is_empty() => {
            match runtime
                .mob_runtime()
                .read_session_history(sid, offset, limit)
                .await
            {
                Ok(page) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(page).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("read_session_history failed: {err}"),
                        data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: session_id required".to_string(),
                data: None,
            }),
        },
    }
}

/// Unwire a local member from a previously wired peer (local side only).
/// Symmetric counterpart to `handle_cross_mob_wire_local`.
pub(super) async fn handle_cross_mob_unwire_local(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let local_member_id = params.get("local_member_id").and_then(Value::as_str);
    let remote_comms_name = params.get("remote_comms_name").and_then(Value::as_str);
    let remote_peer_id = params.get("remote_peer_id").and_then(Value::as_str);
    let remote_address = params.get("remote_address").and_then(Value::as_str);
    let remote_pubkey = match parse_optional_pubkey(params, "remote_pubkey_b64") {
        Ok(value) => value,
        Err(err) => {
            return JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {err}"),
                    data: None,
                }),
            };
        }
    };

    match (local_member_id, remote_comms_name, remote_peer_id, remote_address) {
        (Some(local), Some(comms_name), Some(peer_id), Some(addr))
            if !local.is_empty()
                && !comms_name.is_empty()
                && !peer_id.is_empty()
                && !addr.is_empty() =>
        {
            match runtime
                .unwire_local(local, comms_name, peer_id, addr, remote_pubkey)
                .await
            {
                Ok(()) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "local_member_id": local,
                        "remote_comms_name": comms_name,
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("cross_mob/unwire_local failed: {err}"),
                    data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: local_member_id, remote_comms_name, remote_peer_id, and remote_address required".to_string(),
                    data: None,
                }),
        },
    }
}

/// Wire a local member to an external peer using a provided spec.
/// Only wires the local side — the remote side must do its own call.
pub(super) async fn handle_cross_mob_wire_local(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let local_member_id = params.get("local_member_id").and_then(Value::as_str);
    let remote_comms_name = params.get("remote_comms_name").and_then(Value::as_str);
    let remote_peer_id = params.get("remote_peer_id").and_then(Value::as_str);
    let remote_address = params.get("remote_address").and_then(Value::as_str);
    let remote_pubkey = match parse_optional_pubkey(params, "remote_pubkey_b64") {
        Ok(value) => value,
        Err(err) => {
            return JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {err}"),
                    data: None,
                }),
            };
        }
    };

    match (local_member_id, remote_comms_name, remote_peer_id, remote_address) {
        (Some(local), Some(comms_name), Some(peer_id), Some(addr))
            if !local.is_empty()
                && !comms_name.is_empty()
                && !peer_id.is_empty()
                && !addr.is_empty() =>
        {
            match runtime
                .wire_local(local, comms_name, peer_id, addr, remote_pubkey)
                .await
            {
                Ok(()) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "local_member_id": local,
                        "remote_comms_name": comms_name,
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("cross_mob/wire_local failed: {err}"),
                    data: None,
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params: local_member_id, remote_comms_name, remote_peer_id, and remote_address required".to_string(),
                    data: None,
                }),
        },
    }
}

// ---------------------------------------------------------------------------
// Mob/run labels — mobkit-side sidecar metadata
// ---------------------------------------------------------------------------

use crate::runtime::{
    LabelRpcResult, MetadataScope, dispatch_labels_delete, dispatch_labels_get,
    dispatch_labels_set, labels_to_json_value, parse_run_id_param,
};

fn label_response(response_id: Value, outcome: LabelRpcResult) -> JsonRpcResponse {
    let (result, error) = match outcome {
        LabelRpcResult::Accepted => (Some(serde_json::json!({"accepted": true})), None),
        LabelRpcResult::Labels(labels) => (
            Some(serde_json::json!({"labels": labels_to_json_value(&labels)})),
            None,
        ),
        LabelRpcResult::InvalidParams(message) => (
            None,
            Some(JsonRpcError {
                code: -32602,
                message: format!("Invalid params: {message}"),
                data: None,
            }),
        ),
    };
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result,
        error,
    }
}

fn mob_scope(runtime: &UnifiedRuntime) -> MetadataScope {
    MetadataScope::Mob(runtime.mob_id())
}

fn run_scope_or_error(
    runtime: &UnifiedRuntime,
    params: &Value,
) -> Result<MetadataScope, LabelRpcResult> {
    parse_run_id_param(params)
        .map(|run_id| MetadataScope::Run(runtime.mob_id(), run_id.to_string()))
        .map_err(LabelRpcResult::InvalidParams)
}

pub(super) async fn handle_mob_labels_set(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let outcome = dispatch_labels_set(runtime.metadata_table(), mob_scope(runtime), params).await;
    label_response(response_id, outcome)
}

pub(super) async fn handle_mob_labels_get(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
    let outcome = dispatch_labels_get(runtime.metadata_table(), mob_scope(runtime)).await;
    label_response(response_id, outcome)
}

pub(super) async fn handle_mob_labels_delete(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
    let outcome = dispatch_labels_delete(runtime.metadata_table(), mob_scope(runtime)).await;
    label_response(response_id, outcome)
}

pub(super) async fn handle_run_labels_set(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let outcome = match run_scope_or_error(runtime, params) {
        Ok(scope) => dispatch_labels_set(runtime.metadata_table(), scope, params).await,
        Err(err) => err,
    };
    label_response(response_id, outcome)
}

pub(super) async fn handle_run_labels_get(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let outcome = match run_scope_or_error(runtime, params) {
        Ok(scope) => dispatch_labels_get(runtime.metadata_table(), scope).await,
        Err(err) => err,
    };
    label_response(response_id, outcome)
}

pub(super) async fn handle_run_labels_delete(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let outcome = match run_scope_or_error(runtime, params) {
        Ok(scope) => dispatch_labels_delete(runtime.metadata_table(), scope).await,
        Err(err) => err,
    };
    label_response(response_id, outcome)
}

// ---------------------------------------------------------------------------
// Cross-mob peer trust — Ed25519 peer descriptor support
// ---------------------------------------------------------------------------

/// Parse an optional `<field>` (base64 Ed25519 pubkey) from a params object.
///
/// Returns `Ok(None)` if the field is absent or null. Returns the decoded
/// 32 bytes if the field is a non-empty string. Empty strings are treated
/// as absent so callers can pass `""` from a UI to mean "use defaults".
fn parse_optional_pubkey(params: &Value, field: &str) -> Result<Option<[u8; 32]>, String> {
    let value = match params.get(field) {
        Some(v) => v,
        None => return Ok(None),
    };
    if value.is_null() {
        return Ok(None);
    }
    let s = value
        .as_str()
        .ok_or_else(|| format!("{field} must be a base64 string"))?;
    if s.is_empty() {
        return Ok(None);
    }
    crate::auth::peer_keys::decode_pubkey_b64(s)
        .map(Some)
        .map_err(|err| format!("{field}: {err}"))
}

/// Handle `mobkit/peer_pubkey` — return the local gateway's Ed25519
/// signing pubkey so peer gateways can bootstrap trust.
///
/// Returns `{"pubkey_b64": "<base64>"}` when a keypair is configured.
/// Returns a `-32004 capability_unavailable` error when the gateway has
/// no keys (e.g. inproc-only deployment that never called
/// `set_gateway_peer_keys`) so callers can fall back to TOFU rather than
/// stalling.
pub(super) async fn handle_peer_pubkey(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
    match runtime.gateway_peer_keys() {
        Some(keys) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(serde_json::json!({
                "pubkey_b64": keys.pubkey_b64(),
            })),
            error: None,
        },
        None => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32004,
                message: "gateway has no signing keypair configured".to_string(),
                data: None,
            }),
        },
    }
}
