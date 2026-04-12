//! RPC handler implementations for mob member operations.

use meerkat_core::ContentInput;
use meerkat_mob::launch::ForkContext;
use meerkat_mob::{HelperOptions, MobBackendKind, MobRuntimeMode, ProfileName};
use serde_json::Value;

use crate::unified_runtime::UnifiedRuntime;

use super::{JSONRPC_VERSION, JsonRpcError, JsonRpcResponse};

/// Parse HelperOptions from an optional JSON "options" object.
pub(crate) fn parse_helper_options(options_val: Option<&Value>) -> Result<HelperOptions, String> {
    let mut opts = HelperOptions::default();
    if let Some(o) = options_val {
        opts.role_name = o
            .get("profile")
            .and_then(Value::as_str)
            .map(ProfileName::from);
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
        Some(mid) if !mid.is_empty() => match runtime.member_status(mid).await {
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

pub(super) async fn handle_force_cancel_member(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => match runtime.force_cancel_member(mid).await {
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

pub(super) async fn handle_spawn_helper(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let meerkat_id = params.get("meerkat_id").and_then(Value::as_str);
    let task = params.get("task").and_then(Value::as_str);

    match (meerkat_id, task) {
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
            match runtime.spawn_helper(mid, task_str, options).await {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "output": result.output,
                        "tokens_used": result.tokens_used,
                        "session_id": result.session_id.map(|s| s.to_string()),
                    })),
                    error: None,
                },
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
                message: "Invalid params: meerkat_id and task required".to_string(),
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
    let meerkat_id = params.get("meerkat_id").and_then(Value::as_str);
    let task = params.get("task").and_then(Value::as_str);
    let fork_ctx_val = params.get("fork_context").cloned();

    match (source_member_id, meerkat_id, task) {
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
            match runtime
                .fork_helper(source, mid, task_str, fork_context, options)
                .await
            {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "output": result.output,
                        "tokens_used": result.tokens_used,
                        "session_id": result.session_id.map(|s| s.to_string()),
                    })),
                    error: None,
                },
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
                message: "Invalid params: source_member_id, meerkat_id, and task required"
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
    let profile = params.get("profile").and_then(Value::as_str);
    let meerkat_id = params.get("meerkat_id").and_then(Value::as_str);
    let session_id = params.get("session_id").and_then(Value::as_str);

    match (profile, meerkat_id, session_id) {
        (Some(prof), Some(mid), Some(sid))
            if !prof.is_empty() && !mid.is_empty() && !sid.is_empty() =>
        {
            match runtime.attach_existing_session(prof, mid, sid).await {
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
                message: "Invalid params: profile, meerkat_id, and session_id required".to_string(),
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
        Some(rid) if !rid.is_empty() => match runtime.cancel_flow(rid).await {
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
        },
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
        Some(rid) if !rid.is_empty() => match runtime.flow_status(rid).await {
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
        },
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

pub(super) async fn handle_collect_completed(
    runtime: &UnifiedRuntime,
    response_id: Value,
) -> JsonRpcResponse {
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
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: Some(serde_json::json!({ "completed": entries })),
        error: None,
    }
}

pub(super) async fn handle_member_current_session_id(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => match runtime.member_current_session_id(mid).await {
            Ok(session_id) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(serde_json::json!({
                    "member_id": mid,
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
                    message: format!("member_current_session_id failed: {err}"),
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
            match runtime.read_session_history(sid, offset, limit).await {
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

pub(super) async fn handle_member_session_ref(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    match member_id {
        Some(mid) if !mid.is_empty() => match runtime.member_session_ref(mid).await {
            Ok(Some(session_ref)) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(serde_json::to_value(&session_ref).unwrap_or(Value::Null)),
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
                    message: format!("member_session_ref failed: {err}"),
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
