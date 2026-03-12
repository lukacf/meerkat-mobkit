use serde_json::Value;

use crate::unified_runtime::UnifiedRuntime;

use super::{JsonRpcError, JsonRpcResponse, JSONRPC_VERSION};

pub(super) async fn handle_send_message(
    runtime: &UnifiedRuntime,
    response_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let member_id = params.get("member_id").and_then(Value::as_str);
    let message = params.get("message").and_then(Value::as_str);

    match (member_id, message) {
        (Some(member_id), Some(message)) if !member_id.is_empty() && !message.is_empty() => {
            match runtime.send_message(member_id, message.to_string()).await {
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
                message: "Invalid params: member_id and message required".to_string(),
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
            let labels = params
                .get("labels")
                .and_then(|v| {
                    serde_json::from_value::<std::collections::BTreeMap<String, String>>(v.clone())
                        .ok()
                });
            let context = params.get("context").cloned();
            let resume_session_id = params
                .get("resume_session_id")
                .and_then(Value::as_str)
                .and_then(|s| meerkat_core::types::SessionId::parse(s).ok());
            let additional_instructions = params
                .get("additional_instructions")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect::<Vec<_>>()
                })
                .and_then(|v| if v.is_empty() { None } else { Some(v) });

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
    let query: crate::unified_runtime::EventQuery =
        serde_json::from_value(params).unwrap_or_default();
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
