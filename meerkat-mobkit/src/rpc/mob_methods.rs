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
) -> Result<crate::unified_runtime::EventQuery, JsonRpcResponse> {
    if params.is_null() {
        return Ok(crate::unified_runtime::EventQuery::default());
    }
    serde_json::from_value(params).map_err(|err| JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id.clone(),
        result: None,
        error: Some(JsonRpcError {
            code: -32602,
            message: format!("Invalid params: {err}"),
        }),
    })
}

/// Handle `mobkit/mob_events/query` — return structural mob events
/// matching the supplied [`EventQuery`].
pub(super) async fn handle_mob_events_query(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: Value,
) -> JsonRpcResponse {
    let query = match parse_mob_events_query(&response_id, params) {
        Ok(q) => q,
        Err(response) => return response,
    };
    let events = runtime.query_mob_events(&query).await;
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: Some(serde_json::json!({
            "events": serde_json::to_value(&events).unwrap_or(Value::Null),
            "next_after_seq": events.last().map(|event| event.cursor),
        })),
        error: None,
    }
}

/// Handle `mobkit/mob_events/subscribe` — replay buffered structural mob
/// events for SSE catchup. Mirrors the snapshot-frame shape of the
/// existing `mobkit/events/subscribe` so consumers can take the latest
/// `cursor` and resume via `mobkit/mob_events/query` with `after_seq`.
pub(super) async fn handle_mob_events_subscribe(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: Value,
) -> JsonRpcResponse {
    let query = match parse_mob_events_query(&response_id, params) {
        Ok(q) => q,
        Err(response) => return response,
    };
    let events = runtime.query_mob_events(&query).await;
    let last_cursor = events.last().map(|event| event.cursor);
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: Some(serde_json::json!({
            "stream": "mob_events",
            "events": serde_json::to_value(&events).unwrap_or(Value::Null),
            "next_after_seq": last_cursor,
            "keep_alive": {
                "interval_ms": 15_000_u64,
                "event": "keep_alive",
            },
        })),
        error: None,
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

    match (local_member_id, remote_comms_name, remote_peer_id, remote_address) {
        (Some(local), Some(comms_name), Some(peer_id), Some(addr))
            if !local.is_empty()
                && !comms_name.is_empty()
                && !peer_id.is_empty()
                && !addr.is_empty() =>
        {
            match runtime.unwire_local(local, comms_name, peer_id, addr).await {
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

    match (local_member_id, remote_comms_name, remote_peer_id, remote_address) {
        (Some(local), Some(comms_name), Some(peer_id), Some(addr))
            if !local.is_empty()
                && !comms_name.is_empty()
                && !peer_id.is_empty()
                && !addr.is_empty() =>
        {
            match runtime.wire_local(local, comms_name, peer_id, addr).await {
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
            }),
        },
    }
}
