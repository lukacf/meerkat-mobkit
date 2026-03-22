//! RPC handler implementations for mob member operations.

use meerkat_core::ContentInput;
use serde_json::Value;

use crate::unified_runtime::UnifiedRuntime;

use super::{JSONRPC_VERSION, JsonRpcError, JsonRpcResponse};

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
            match runtime.send_message(member_id, content).await {
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
            let members = runtime.find_members(key, value).await;
            JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(serde_json::to_value(&members).unwrap_or(Value::Null)),
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
    let profile = params.get("profile").and_then(Value::as_str);
    let meerkat_id = params.get("meerkat_id").and_then(Value::as_str);

    match (profile, meerkat_id) {
        (Some(profile), Some(meerkat_id)) if !profile.is_empty() && !meerkat_id.is_empty() => {
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

            let mut spec = meerkat_mob::SpawnMemberSpec::new(
                meerkat_mob::ProfileName::from(profile),
                meerkat_mob::MeerkatId::from(meerkat_id),
            );
            if let Some(context) = context {
                spec = spec.with_context(context);
            }
            if let Some(labels) = labels {
                spec = spec.with_labels(labels);
            }
            if let Some(sid) = resume_session_id {
                spec = spec.with_resume_session_id(sid);
            }
            if let Some(instructions) = additional_instructions {
                spec = spec.with_additional_instructions(instructions);
            }
            match runtime.ensure_member(spec).await {
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
                message: "Invalid params: profile and meerkat_id required".to_string(),
            }),
        },
    }
}

pub(super) async fn handle_list_members(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
    let members = runtime.list_members().await;
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: Some(serde_json::to_value(&members).unwrap_or(Value::Null)),
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
        Some(mid) if !mid.is_empty() => match runtime.get_member(mid).await {
            Some(snapshot) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
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

pub(super) async fn handle_retire_member(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => match runtime.retire_member(mid).await {
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

pub(super) async fn handle_respawn_member(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => match runtime.respawn_member(mid).await {
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
                    message: format!("respawn_member failed: {err}"),
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
    match runtime.query_events(query).await {
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
                "events": []
            })),
            error: None,
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
