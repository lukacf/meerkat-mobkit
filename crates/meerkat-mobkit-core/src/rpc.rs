use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::{
    handle_console_rest_json_route, route_module_call, validate_schedules,
    BigQuerySessionStoreAdapter, BigQuerySessionStoreError, ConsoleRestJsonRequest,
    ConsoleRestJsonResponse, DeliveryHistoryRequest, DeliverySendError, DeliverySendRequest,
    ElephantMemoryStoreError, GatingDecideError, GatingDecideRequest, GatingDecision,
    GatingEvaluateRequest, GatingRiskTier, MemoryIndexError, MemoryIndexRequest,
    MemoryQueryRequest, MobkitRuntimeHandle, ModuleRouteError, ModuleRouteRequest,
    RoutingResolveError, RoutingResolveRequest, RuntimeDecisionState, RuntimeRoute,
    RuntimeRouteMutationError, ScheduleDefinition, ScheduleValidationError, SessionPersistenceRow,
    SubscribeError, SubscribeRequest, SubscribeScope, ROUTING_RETRY_MAX_CAP,
};
use crate::unified_runtime::UnifiedRuntime;

mod console_ingress;
mod gating_methods;
mod memory_methods;
mod routing_delivery_methods;
mod scheduling_methods;
mod session_store_methods;
mod subscribe_methods;

pub use console_ingress::handle_console_ingress_json;

use gating_methods::{
    parse_gating_audit_params, parse_gating_decide_params, parse_gating_evaluate_params,
    parse_gating_pending_params, GatingParamsError,
};
use memory_methods::{
    parse_memory_index_params, parse_memory_query_params, parse_memory_stores_params,
    MemoryParamsError,
};
use routing_delivery_methods::{
    parse_delivery_history_params, parse_delivery_send_params, parse_routing_resolve_params,
    parse_routing_route_add_params, parse_routing_route_delete_params,
    parse_routing_routes_list_params, RoutingDeliveryParamsError,
};
use scheduling_methods::{format_schedule_validation_error, parse_scheduling_params};
use session_store_methods::{
    format_bigquery_store_error, parse_bigquery_session_store_params,
    run_bigquery_session_store_request, BigQuerySessionStoreRpcError,
};
use subscribe_methods::{parse_subscribe_request, SubscribeParamsError};

pub const JSONRPC_VERSION: &str = "2.0";
pub const MOBKIT_CONTRACT_VERSION: &str = "0.1.0";
pub const MAX_SCHEDULES_PER_REQUEST: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcCapabilitiesError {
    InvalidJson,
    InvalidSchema,
    MissingContractVersion,
    InvalidContractVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcCapabilities {
    pub contract_version: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub fn parse_rpc_capabilities(line: &str) -> Result<RpcCapabilities, RpcCapabilitiesError> {
    let raw: Value = serde_json::from_str(line).map_err(|_| RpcCapabilitiesError::InvalidJson)?;
    let object = raw.as_object().ok_or(RpcCapabilitiesError::InvalidSchema)?;
    let contract = object
        .get("contract_version")
        .ok_or(RpcCapabilitiesError::MissingContractVersion)?;
    let contract_str = contract
        .as_str()
        .ok_or(RpcCapabilitiesError::InvalidContractVersion)?;
    if contract_str.trim().is_empty() {
        return Err(RpcCapabilitiesError::InvalidContractVersion);
    }
    serde_json::from_value(raw).map_err(|_| RpcCapabilitiesError::InvalidSchema)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

pub fn handle_mobkit_rpc_json(
    runtime: &mut MobkitRuntimeHandle,
    request_json: &str,
    timeout: Duration,
) -> String {
    let raw_request: Value = match serde_json::from_str(request_json) {
        Ok(raw_request) => raw_request,
        Err(_) => {
            return serialize_response(&JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: Value::Null,
                result: None,
                error: Some(JsonRpcError {
                    code: -32700,
                    message: "Parse error".to_string(),
                }),
            });
        }
    };
    let response_id = raw_request
        .as_object()
        .and_then(|object| object.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    let request: JsonRpcRequest = match serde_json::from_value(raw_request) {
        Ok(request) => request,
        Err(_) => {
            return serialize_response(&JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32600,
                    message: "Invalid Request".to_string(),
                }),
            });
        }
    };
    let is_notification = request.id.is_none();
    let response_id = request.id.clone().unwrap_or(Value::Null);

    if request.jsonrpc != "2.0" {
        let response = JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request".to_string(),
            }),
        };
        return if is_notification {
            String::new()
        } else {
            serialize_response(&response)
        };
    }

    let response = match request.method.as_str() {
        "mobkit/status" => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id.clone(),
            result: Some(serde_json::json!({
                "contract_version": MOBKIT_CONTRACT_VERSION,
                "running": runtime.is_running(),
                "loaded_modules": runtime.loaded_modules(),
            })),
            error: None,
        },
        "mobkit/capabilities" => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id.clone(),
            result: Some(serde_json::json!({
                "contract_version": MOBKIT_CONTRACT_VERSION,
                "methods": [
                    "mobkit/status",
                    "mobkit/capabilities",
                    "mobkit/reconcile",
                    "mobkit/spawn_member",
                    "mobkit/scheduling/evaluate",
                    "mobkit/scheduling/dispatch",
                    "mobkit/routing/resolve",
                    "mobkit/routing/routes/list",
                    "mobkit/routing/routes/add",
                    "mobkit/routing/routes/delete",
                    "mobkit/delivery/send",
                    "mobkit/delivery/history",
                    "mobkit/events/subscribe",
                    "mobkit/memory/stores",
                    "mobkit/memory/index",
                    "mobkit/memory/query",
                    "mobkit/session_store/bigquery",
                    "mobkit/gating/evaluate",
                    "mobkit/gating/pending",
                    "mobkit/gating/decide",
                    "mobkit/gating/audit"
                ],
                "loaded_modules": runtime.loaded_modules()
            })),
            error: None,
        },
        "mobkit/reconcile" => {
            let modules = request
                .params
                .get("modules")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(ToString::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            match runtime.reconcile_modules(modules.clone(), timeout) {
                Ok(added) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "reconciled_modules": modules,
                        "added": added
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {err:?}"),
                    }),
                },
            }
        }
        "mobkit/spawn_member" => {
            let module_id = request
                .params
                .get("module_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if module_id.is_empty() {
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: "Invalid params: module_id required".to_string(),
                    }),
                }
            } else {
                match runtime.spawn_member(&module_id, timeout) {
                    Ok(()) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id.clone(),
                        result: Some(serde_json::json!({
                            "accepted": true,
                            "module_id": module_id
                        })),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id.clone(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: format!("Invalid params: {err:?}"),
                        }),
                    },
                }
            }
        }
        "mobkit/scheduling/evaluate" => match parse_scheduling_params(&request.params) {
            Ok((schedules, tick_ms)) => match runtime.evaluate_schedule_tick(&schedules, tick_ms) {
                Ok(evaluation) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(evaluation).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!(
                            "Invalid params: {}",
                            format_schedule_validation_error(err)
                        ),
                    }),
                },
            },
            Err(message) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {message}"),
                }),
            },
        },
        "mobkit/scheduling/dispatch" => match parse_scheduling_params(&request.params) {
            Ok((schedules, tick_ms)) => match runtime.dispatch_schedule_tick(&schedules, tick_ms) {
                Ok(dispatch) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(dispatch).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!(
                            "Invalid params: {}",
                            format_schedule_validation_error(err)
                        ),
                    }),
                },
            },
            Err(message) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {message}"),
                }),
            },
        },
        "mobkit/routing/resolve" => {
            match parse_routing_resolve_params(&request.params).and_then(|resolve_request| {
                runtime
                    .resolve_routing(resolve_request)
                    .map_err(RoutingDeliveryParamsError::Routing)
            }) {
                Ok(resolution) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(resolution).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/routing/routes/list" => match parse_routing_routes_list_params(&request.params) {
            Ok(()) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "routes": runtime.list_runtime_routes()
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/routing/routes/add" => match parse_routing_route_add_params(&request.params)
            .and_then(|route| {
                runtime
                    .add_runtime_route(route)
                    .map_err(RoutingDeliveryParamsError::RouteMutation)
            }) {
            Ok(route) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({ "route": route })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/routing/routes/delete" => match parse_routing_route_delete_params(&request.params)
            .and_then(|route_key| {
                runtime
                    .delete_runtime_route(&route_key)
                    .map_err(RoutingDeliveryParamsError::RouteMutation)
            }) {
            Ok(route) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({ "deleted": route })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/delivery/send" => {
            match parse_delivery_send_params(&request.params).and_then(|send_request| {
                runtime
                    .send_delivery(send_request)
                    .map_err(RoutingDeliveryParamsError::Delivery)
            }) {
                Ok(record) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(record).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/delivery/history" => match parse_delivery_history_params(&request.params) {
            Ok(history_request) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(
                    serde_json::to_value(runtime.delivery_history(history_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/events/subscribe" => {
            match parse_subscribe_request(&request.params).and_then(|subscribe_request| {
                runtime
                    .subscribe_events(subscribe_request)
                    .map_err(SubscribeParamsError::Runtime)
            }) {
                Ok(subscribe_result) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(subscribe_result).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/memory/stores" => match parse_memory_stores_params(&request.params) {
            Ok(()) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "stores": runtime.memory_stores(),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/memory/index" => match parse_memory_index_params(&request.params) {
            Ok(index_request) => match runtime.memory_index(index_request) {
                Ok(indexed) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(indexed).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(MemoryIndexError::BackendPersistFailed(error)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32010,
                        message: format!(
                            "Memory backend unavailable: {}",
                            MemoryParamsError::backend_message(&error)
                        ),
                    }),
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!(
                            "Invalid params: {}",
                            MemoryParamsError::Index(err).message()
                        ),
                    }),
                },
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/memory/query" => match parse_memory_query_params(&request.params) {
            Ok(query_request) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(
                    serde_json::to_value(runtime.memory_query(query_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/session_store/bigquery" => {
            match parse_bigquery_session_store_params(&request.params)
                .and_then(run_bigquery_session_store_request)
            {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(result),
                    error: None,
                },
                Err(BigQuerySessionStoreRpcError::Params(message)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {message}"),
                    }),
                },
                Err(BigQuerySessionStoreRpcError::Store(error)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32011,
                        message: format!(
                            "BigQuery session store request failed: {}",
                            format_bigquery_store_error(&error)
                        ),
                    }),
                },
            }
        }
        "mobkit/gating/evaluate" => match parse_gating_evaluate_params(&request.params) {
            Ok(gating_request) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(
                    serde_json::to_value(runtime.evaluate_gating_action(gating_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/gating/pending" => match parse_gating_pending_params(&request.params) {
            Ok(()) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "pending": runtime.list_gating_pending(),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/gating/decide" => {
            match parse_gating_decide_params(&request.params).and_then(|decide_request| {
                runtime
                    .decide_gating_action(decide_request)
                    .map_err(GatingParamsError::Decision)
            }) {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/gating/audit" => match parse_gating_audit_params(&request.params) {
            Ok(limit) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "entries": runtime.gating_audit_entries(limit),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        method if method.contains('/') && !method.starts_with("mobkit/") => {
            let module_id = method
                .split('/')
                .next()
                .map(ToString::to_string)
                .unwrap_or_default();
            let route = route_module_call(
                runtime,
                &ModuleRouteRequest {
                    module_id: module_id.clone(),
                    method: method.to_string(),
                    params: request.params,
                },
                timeout,
            );
            match route {
                Ok(response) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::json!({
                        "module_id": response.module_id,
                        "method": response.method,
                        "payload": response.payload
                    })),
                    error: None,
                },
                Err(ModuleRouteError::UnloadedModule(module_id)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: format!("Module '{module_id}' not loaded"),
                    }),
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("Module route failed: {err:?}"),
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
            }),
        },
    };
    if is_notification {
        String::new()
    } else {
        serialize_response(&response)
    }
}

pub async fn handle_unified_rpc_json(
    runtime: &mut UnifiedRuntime,
    request_json: &str,
    timeout: Duration,
    http_base_url: Option<&str>,
) -> String {
    let raw_request: Value = match serde_json::from_str(request_json) {
        Ok(raw_request) => raw_request,
        Err(_) => {
            return serialize_response(&JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: Value::Null,
                result: None,
                error: Some(JsonRpcError {
                    code: -32700,
                    message: "Parse error".to_string(),
                }),
            });
        }
    };
    let response_id = raw_request
        .as_object()
        .and_then(|object| object.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    let request: JsonRpcRequest = match serde_json::from_value(raw_request) {
        Ok(request) => request,
        Err(_) => {
            return serialize_response(&JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32600,
                    message: "Invalid Request".to_string(),
                }),
            });
        }
    };
    let is_notification = request.id.is_none();
    let response_id = request.id.clone().unwrap_or(Value::Null);

    if request.jsonrpc != "2.0" {
        let response = JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request".to_string(),
            }),
        };
        return if is_notification {
            String::new()
        } else {
            serialize_response(&response)
        };
    }

    let response = match request.method.as_str() {
        "mobkit/status" => {
            let mob_state = runtime.status();
            let mut result = serde_json::json!({
                "contract_version": MOBKIT_CONTRACT_VERSION,
                "running": runtime.module_is_running(),
                "loaded_modules": runtime.loaded_modules(),
                "mob_state": format!("{mob_state:?}"),
            });
            if let Some(url) = http_base_url {
                result["http_base_url"] = Value::String(url.to_string());
            }
            JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(result),
                error: None,
            }
        }
        "mobkit/capabilities" => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id.clone(),
            result: Some(serde_json::json!({
                "contract_version": MOBKIT_CONTRACT_VERSION,
                "runtime_type": "unified",
                "methods": [
                    "mobkit/init",
                    "mobkit/status",
                    "mobkit/capabilities",
                    "mobkit/reconcile",
                    "mobkit/spawn_member",
                    "mobkit/scheduling/evaluate",
                    "mobkit/scheduling/dispatch",
                    "mobkit/routing/resolve",
                    "mobkit/routing/routes/list",
                    "mobkit/routing/routes/add",
                    "mobkit/routing/routes/delete",
                    "mobkit/delivery/send",
                    "mobkit/delivery/history",
                    "mobkit/events/subscribe",
                    "mobkit/memory/stores",
                    "mobkit/memory/index",
                    "mobkit/memory/query",
                    "mobkit/session_store/bigquery",
                    "mobkit/gating/evaluate",
                    "mobkit/gating/pending",
                    "mobkit/gating/decide",
                    "mobkit/gating/audit"
                ],
                "loaded_modules": runtime.loaded_modules()
            })),
            error: None,
        },
        "mobkit/reconcile" => {
            let modules = request
                .params
                .get("modules")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(ToString::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            match runtime.reconcile_modules(modules.clone(), timeout) {
                Ok(added) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "reconciled_modules": modules,
                        "added": added
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {err:?}"),
                    }),
                },
            }
        }
        "mobkit/spawn_member" => {
            let profile = request
                .params
                .get("profile")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let meerkat_id = request
                .params
                .get("meerkat_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if profile.is_empty() || meerkat_id.is_empty() {
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: "Invalid params: profile and meerkat_id required".to_string(),
                    }),
                }
            } else {
                let spec = meerkat_mob::SpawnMemberSpec::from_wire(
                    profile,
                    meerkat_id.clone(),
                    request
                        .params
                        .get("initial_message")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    None,
                    None,
                );
                match runtime.spawn(spec).await {
                    Ok(_member_ref) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id.clone(),
                        result: Some(serde_json::json!({
                            "accepted": true,
                            "meerkat_id": meerkat_id
                        })),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id.clone(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: format!("Invalid params: {err}"),
                        }),
                    },
                }
            }
        }
        "mobkit/scheduling/evaluate" => match parse_scheduling_params(&request.params) {
            Ok((schedules, tick_ms)) => match runtime.evaluate_schedule_tick(&schedules, tick_ms) {
                Ok(evaluation) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(evaluation).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!(
                            "Invalid params: {}",
                            format_schedule_validation_error(err)
                        ),
                    }),
                },
            },
            Err(message) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {message}"),
                }),
            },
        },
        "mobkit/scheduling/dispatch" => match parse_scheduling_params(&request.params) {
            Ok((schedules, tick_ms)) => {
                match runtime.dispatch_schedule_tick(&schedules, tick_ms).await {
                    Ok(dispatch) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id.clone(),
                        result: Some(serde_json::to_value(dispatch).unwrap_or(Value::Null)),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id.clone(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: format!("Invalid params: {err}"),
                        }),
                    },
                }
            }
            Err(message) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {message}"),
                }),
            },
        },
        "mobkit/routing/resolve" => {
            match parse_routing_resolve_params(&request.params).and_then(|resolve_request| {
                runtime
                    .resolve_routing(resolve_request)
                    .map_err(RoutingDeliveryParamsError::Routing)
            }) {
                Ok(resolution) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(resolution).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/routing/routes/list" => match parse_routing_routes_list_params(&request.params) {
            Ok(()) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "routes": runtime.list_runtime_routes()
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/routing/routes/add" => match parse_routing_route_add_params(&request.params)
            .and_then(|route| {
                runtime
                    .add_runtime_route(route)
                    .map_err(RoutingDeliveryParamsError::RouteMutation)
            }) {
            Ok(route) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({ "route": route })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/routing/routes/delete" => {
            match parse_routing_route_delete_params(&request.params).and_then(|route_key| {
                runtime
                    .delete_runtime_route(&route_key)
                    .map_err(RoutingDeliveryParamsError::RouteMutation)
            }) {
                Ok(route) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::json!({ "deleted": route })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/delivery/send" => {
            match parse_delivery_send_params(&request.params).and_then(|send_request| {
                runtime
                    .send_delivery(send_request)
                    .map_err(RoutingDeliveryParamsError::Delivery)
            }) {
                Ok(record) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(record).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/delivery/history" => match parse_delivery_history_params(&request.params) {
            Ok(history_request) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(
                    serde_json::to_value(runtime.delivery_history(history_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/events/subscribe" => match parse_subscribe_request(&request.params) {
            Ok(subscribe_request) => match runtime.subscribe_events(subscribe_request) {
                Ok(subscribe_result) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(subscribe_result).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {err}"),
                    }),
                },
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/memory/stores" => match parse_memory_stores_params(&request.params) {
            Ok(()) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "stores": runtime.memory_stores(),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/memory/index" => match parse_memory_index_params(&request.params) {
            Ok(index_request) => match runtime.memory_index(index_request) {
                Ok(indexed) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(indexed).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(MemoryIndexError::BackendPersistFailed(error)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32010,
                        message: format!(
                            "Memory backend unavailable: {}",
                            MemoryParamsError::backend_message(&error)
                        ),
                    }),
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!(
                            "Invalid params: {}",
                            MemoryParamsError::Index(err).message()
                        ),
                    }),
                },
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/memory/query" => match parse_memory_query_params(&request.params) {
            Ok(query_request) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(
                    serde_json::to_value(runtime.memory_query(query_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/session_store/bigquery" => {
            match parse_bigquery_session_store_params(&request.params)
                .and_then(run_bigquery_session_store_request)
            {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(result),
                    error: None,
                },
                Err(BigQuerySessionStoreRpcError::Params(message)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {message}"),
                    }),
                },
                Err(BigQuerySessionStoreRpcError::Store(error)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32011,
                        message: format!(
                            "BigQuery session store request failed: {}",
                            format_bigquery_store_error(&error)
                        ),
                    }),
                },
            }
        }
        "mobkit/gating/evaluate" => match parse_gating_evaluate_params(&request.params) {
            Ok(gating_request) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(
                    serde_json::to_value(runtime.evaluate_gating_action(gating_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/gating/pending" => match parse_gating_pending_params(&request.params) {
            Ok(()) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "pending": runtime.list_gating_pending(),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/gating/decide" => {
            match parse_gating_decide_params(&request.params).and_then(|decide_request| {
                runtime
                    .decide_gating_action(decide_request)
                    .map_err(GatingParamsError::Decision)
            }) {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/gating/audit" => match parse_gating_audit_params(&request.params) {
            Ok(limit) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "entries": runtime.gating_audit_entries(limit),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        method if method.contains('/') && !method.starts_with("mobkit/") => {
            let module_id = method
                .split('/')
                .next()
                .map(ToString::to_string)
                .unwrap_or_default();
            let route = runtime.route_module_call(
                &ModuleRouteRequest {
                    module_id: module_id.clone(),
                    method: method.to_string(),
                    params: request.params,
                },
                timeout,
            );
            match route {
                Ok(response) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::json!({
                        "module_id": response.module_id,
                        "method": response.method,
                        "payload": response.payload
                    })),
                    error: None,
                },
                Err(ModuleRouteError::UnloadedModule(module_id)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: format!("Module '{module_id}' not loaded"),
                    }),
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("Module route failed: {err:?}"),
                    }),
                },
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
            }),
        },
    };
    if is_notification {
        String::new()
    } else {
        serialize_response(&response)
    }
}

fn serialize_response(response: &JsonRpcResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
            .to_string()
    })
}
