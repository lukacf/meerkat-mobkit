//! JSON-RPC request handling for both module-only and unified runtime modes.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::time::Duration;

use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::console_contracts::{ConsoleInteractionAccepted, ConsoleInteractionRequest};
use crate::runtime::{
    BigQuerySessionStoreAdapter, BigQuerySessionStoreError, ConsoleRestJsonRequest,
    ConsoleRestJsonResponse, DeliveryHistoryRequest, DeliverySendError, DeliverySendRequest,
    ElephantMemoryStoreError, GatingDecideError, GatingDecideRequest, GatingDecision,
    GatingEvaluateRequest, GatingRiskTier, MemoryIndexError, MemoryIndexRequest,
    MemoryQueryRequest, MobkitRuntimeHandle, ModuleRouteError, ModuleRouteRequest,
    ROUTING_RETRY_MAX_CAP, RoutingResolveError, RoutingResolveRequest, RuntimeDecisionState,
    RuntimeRoute, RuntimeRouteMutationError, ScheduleDefinition, ScheduleValidationError,
    SessionPersistenceRow, SubscribeError, SubscribeRequest, SubscribeScope,
    handle_console_rest_json_route, route_module_call, validate_schedules,
};
use crate::unified_runtime::UnifiedRuntime;

mod console_ingress;
mod gating_methods;
mod memory_methods;
pub(crate) mod mob_methods;
pub(crate) mod params;
mod routing_delivery_methods;
mod scheduling_methods;
mod session_store_methods;
mod subscribe_methods;

pub use console_ingress::handle_console_ingress_json;

use gating_methods::{
    GatingParamsError, parse_gating_audit_params, parse_gating_decide_params,
    parse_gating_evaluate_params, parse_gating_pending_params,
};
use memory_methods::{
    MemoryParamsError, parse_memory_index_params, parse_memory_query_params,
    parse_memory_stores_params,
};
use routing_delivery_methods::{
    RoutingDeliveryParamsError, parse_delivery_history_params, parse_delivery_send_params,
    parse_routing_resolve_params, parse_routing_route_add_params,
    parse_routing_route_delete_params, parse_routing_routes_list_params,
};
use scheduling_methods::{format_schedule_validation_error, parse_scheduling_params};
use session_store_methods::{
    BigQuerySessionStoreRpcError, format_bigquery_store_error, parse_bigquery_session_store_params,
    run_bigquery_session_store_request,
};
use subscribe_methods::{SubscribeParamsError, parse_subscribe_request};

pub const JSONRPC_VERSION: &str = "2.0";
pub const MOBKIT_CONTRACT_VERSION: &str = "0.3.0";
pub const MAX_SCHEDULES_PER_REQUEST: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcCapabilitiesError {
    InvalidJson,
    InvalidSchema,
    MissingContractVersion,
    InvalidContractVersion,
}

impl std::fmt::Display for RpcCapabilitiesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => write!(f, "invalid JSON"),
            Self::InvalidSchema => write!(f, "invalid schema"),
            Self::MissingContractVersion => write!(f, "missing contract version"),
            Self::InvalidContractVersion => write!(f, "invalid contract version"),
        }
    }
}

impl std::error::Error for RpcCapabilitiesError {}

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
            id: response_id,
            result: Some(serde_json::json!({
                "contract_version": MOBKIT_CONTRACT_VERSION,
                "running": runtime.is_running(),
                "loaded_modules": runtime.loaded_modules(),
            })),
            error: None,
        },
        "mobkit/capabilities" => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
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
                    "mobkit/gating/audit",
                    "mobkit/call_tool",
                    "mobkit/models/catalog"
                ],
                "loaded_modules": runtime.loaded_modules(),
                "runtime_capabilities": {
                    "can_spawn_members": false,
                    "can_send_messages": false,
                    "can_wire_members": false,
                    "can_retire_members": false,
                    "available_spawn_modes": ["module"],
                }
            })),
            error: None,
        },
        "mobkit/models/catalog" => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(build_models_catalog_result()),
            error: None,
        },
        "mobkit/reconcile" => {
            let modules = match params::required_string_array(&request.params, "modules") {
                Ok(m) => m,
                Err(reason) => {
                    return serialize_response(&JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: format!("Invalid params: {reason}"),
                        }),
                    });
                }
            };

            match runtime.reconcile_modules(modules.clone(), timeout) {
                Ok(added) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "reconciled_modules": modules,
                        "added": added
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
                    id: response_id,
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
                        id: response_id,
                        result: Some(serde_json::json!({
                            "accepted": true,
                            "module_id": module_id
                        })),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
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
                    id: response_id,
                    result: Some(serde_json::to_value(evaluation).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
                id: response_id,
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
                    id: response_id,
                    result: Some(serde_json::to_value(dispatch).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
                id: response_id,
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
                    id: response_id,
                    result: Some(serde_json::to_value(resolution).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
                id: response_id,
                result: Some(serde_json::json!({
                    "routes": runtime.list_runtime_routes()
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
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
                id: response_id,
                result: Some(serde_json::json!({ "route": route })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
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
                id: response_id,
                result: Some(serde_json::json!({ "deleted": route })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
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
                    id: response_id,
                    result: Some(serde_json::to_value(record).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
                id: response_id,
                result: Some(
                    serde_json::to_value(runtime.delivery_history(history_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
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
                    id: response_id,
                    result: Some(serde_json::to_value(subscribe_result).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
                id: response_id,
                result: Some(serde_json::json!({
                    "stores": runtime.memory_stores(),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
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
                    id: response_id,
                    result: Some(serde_json::to_value(indexed).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(MemoryIndexError::BackendPersistFailed(error)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
                    id: response_id,
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
                id: response_id,
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
                id: response_id,
                result: Some(
                    serde_json::to_value(runtime.memory_query(query_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
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
                    id: response_id,
                    result: Some(result),
                    error: None,
                },
                Err(BigQuerySessionStoreRpcError::Params(message)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {message}"),
                    }),
                },
                Err(BigQuerySessionStoreRpcError::Store(error)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
                id: response_id,
                result: Some(
                    serde_json::to_value(runtime.evaluate_gating_action(gating_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
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
                id: response_id,
                result: Some(serde_json::json!({
                    "pending": runtime.list_gating_pending(),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
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
                    id: response_id,
                    result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
                id: response_id,
                result: Some(serde_json::json!({
                    "entries": runtime.gating_audit_entries(limit),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/call_tool" => {
            let module_id = request.params.get("module_id").and_then(Value::as_str);
            let tool = request.params.get("tool").and_then(Value::as_str);
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::json!({}));

            match (module_id, tool) {
                (Some(module_id), Some(tool)) if !module_id.is_empty() && !tool.is_empty() => {
                    let route = route_module_call(
                        runtime,
                        &ModuleRouteRequest {
                            module_id: module_id.to_string(),
                            method: tool.to_string(),
                            params: arguments,
                        },
                        timeout,
                    );
                    match route {
                        Ok(response) => JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: Some(serde_json::json!({
                                "module_id": response.module_id,
                                "tool": response.method,
                                "result": response.payload
                            })),
                            error: None,
                        },
                        Err(ModuleRouteError::UnloadedModule(mid)) => JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32601,
                                message: format!("Module '{mid}' not loaded"),
                            }),
                        },
                        Err(err) => JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32000,
                                message: format!("Tool call failed: {err:?}"),
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
                        message: "Invalid params: module_id and tool required".to_string(),
                    }),
                },
            }
        }
        method if method.contains('/') && !method.starts_with("mobkit/") => {
            let module_id = method
                .split('/')
                .next()
                .map(ToString::to_string)
                .unwrap_or_default();
            let route = route_module_call(
                runtime,
                &ModuleRouteRequest {
                    module_id,
                    method: method.to_string(),
                    params: request.params,
                },
                timeout,
            );
            match route {
                Ok(response) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "module_id": response.module_id,
                        "method": response.method,
                        "payload": response.payload
                    })),
                    error: None,
                },
                Err(ModuleRouteError::UnloadedModule(module_id)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: format!("Module '{module_id}' not loaded"),
                    }),
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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

/// Identity-first runtime context passed to the RPC handler.
pub struct IdentityFirstContext {
    pub runtime: std::sync::Arc<crate::identity_first::IdentityRuntime>,
    pub roster_provider: std::sync::Arc<dyn crate::identity_first::contracts::RosterProvider>,
    pub topology_provider:
        Option<std::sync::Arc<dyn crate::identity_first::contracts::TopologyProvider>>,
    pub customizer: Option<std::sync::Arc<dyn crate::identity_first::contracts::AgentCustomizer>>,
}

pub async fn handle_unified_rpc_json(
    runtime: &UnifiedRuntime,
    request_json: &str,
    timeout: Duration,
    http_base_url: Option<&str>,
    identity_ctx: Option<&IdentityFirstContext>,
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
            let mob_state = runtime.mob_handle().status().await.ok();
            let is_running = runtime.module_is_running().await;
            let loaded = runtime.loaded_modules().await;
            let mut result = serde_json::json!({
                "contract_version": MOBKIT_CONTRACT_VERSION,
                "running": is_running,
                "loaded_modules": loaded,
                "mob_state": format!("{mob_state:?}"),
            });
            if let Some(url) = http_base_url {
                result["http_base_url"] = Value::String(url.to_string());
            }
            JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(result),
                error: None,
            }
        }
        "mobkit/capabilities" => {
            let loaded = runtime.loaded_modules().await;
            let mut methods = vec![
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
                "mobkit/gating/audit",
                "mobkit/call_tool",
                "mobkit/models/catalog",
                "mobkit/send_message",
                "mobkit/find_members",
                "mobkit/ensure_member",
                "mobkit/list_members",
                "mobkit/get_member",
                "mobkit/retire_member",
                "mobkit/respawn_member",
                "mobkit/reconcile_edges",
                "mobkit/rediscover",
                "mobkit/query_events",
                // Always available: local-only member introspection
                "mobkit/cross_mob/peer_info",
                "mobkit/cross_mob/wire_local",
                "mobkit/cross_mob/unwire_local",
                "mobkit/member_status",
                "mobkit/force_cancel_member",
                "mobkit/spawn_helper",
                "mobkit/fork_helper",
                "mobkit/attach_existing_session",
                "mobkit/cancel_flow",
                "mobkit/flow_status",
                "mobkit/collect_completed",
                "mobkit/member_current_session_id",
                "mobkit/read_session_history",
                "mobkit/member_session_ref",
            ];
            if identity_ctx.is_some() {
                methods.extend_from_slice(&[
                    "mobkit/interact",
                    "mobkit/send",
                    "mobkit/dispatch",
                    "mobkit/status_identity",
                    "mobkit/respawn",
                    "mobkit/retire",
                    "mobkit/reset",
                    "mobkit/delete_identity",
                    "mobkit/inspect_identity",
                    "mobkit/reconcile_identity",
                ]);
            }
            // Cross-mob directory always advertised when configured
            if runtime.has_contact_directory() {
                methods.push("mobkit/cross_mob/directory");
            }
            // High-level wire/unwire/send require peer mob handles AND inproc contacts.
            // resolve_contact() rejects non-Inproc transports at execution time, so
            // advertising these methods for TCP/UDS-only deployments guarantees failures.
            if runtime.has_peer_mob_handles().await && runtime.has_inproc_contacts() {
                methods.extend_from_slice(&[
                    "mobkit/cross_mob/wire",
                    "mobkit/cross_mob/unwire",
                    "mobkit/cross_mob/send",
                ]);
            }
            JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(serde_json::json!({
                    "contract_version": MOBKIT_CONTRACT_VERSION,
                    "runtime_type": "unified",
                    "methods": methods,
                    "loaded_modules": loaded,
                    "runtime_capabilities": {
                        "can_spawn_members": true,
                        "can_send_messages": true,
                        "can_wire_members": true,
                        "can_retire_members": true,
                        "available_spawn_modes": ["module", "profile"],
                    }
                })),
                error: None,
            }
        }
        "mobkit/reconcile" => {
            let modules = match params::required_string_array(&request.params, "modules") {
                Ok(m) => m,
                Err(reason) => {
                    return serialize_response(&JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: format!("Invalid params: {reason}"),
                        }),
                    });
                }
            };

            match runtime.reconcile_modules(modules.clone(), timeout).await {
                Ok(added) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "reconciled_modules": modules,
                        "added": added
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {err:?}"),
                    }),
                },
            }
        }
        "mobkit/spawn_member" => {
            // Support both legacy module_id pattern and mob profile+meerkat_id pattern
            let module_id = request.params.get("module_id").and_then(Value::as_str);
            let profile = request.params.get("profile").and_then(Value::as_str);
            let meerkat_id = request.params.get("meerkat_id").and_then(Value::as_str);

            if let Some(module_id) = module_id {
                // Legacy module spawn: {"module_id": "routing"}
                if module_id.is_empty() {
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: "Invalid params: module_id required".to_string(),
                        }),
                    }
                } else {
                    match runtime.spawn_member(module_id, timeout).await {
                        Ok(()) => JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: Some(serde_json::json!({
                                "accepted": true,
                                "module_id": module_id
                            })),
                            error: None,
                        },
                        Err(err) => JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32602,
                                message: format!("Invalid params: {err:?}"),
                            }),
                        },
                    }
                }
            } else if let (Some(profile), Some(meerkat_id)) = (profile, meerkat_id) {
                // Mob agent spawn: {"profile": "default", "meerkat_id": "agent-1"}
                let spec = meerkat_mob::SpawnMemberSpec::from_wire(
                    profile.to_string(),
                    meerkat_id.to_string(),
                    request
                        .params
                        .get("initial_message")
                        .and_then(Value::as_str)
                        .map(|s| meerkat_core::ContentInput::from(s.to_string())),
                    None,
                    None,
                );
                match runtime.spawn(spec).await {
                    Ok(_member_ref) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({
                            "accepted": true,
                            "meerkat_id": meerkat_id
                        })),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: format!("Invalid params: {err}"),
                        }),
                    },
                }
            } else {
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: "Invalid params: module_id or (profile + meerkat_id) required"
                            .to_string(),
                    }),
                }
            }
        }
        "mobkit/scheduling/evaluate" => match parse_scheduling_params(&request.params) {
            Ok((schedules, tick_ms)) => {
                match runtime.evaluate_schedule_tick(&schedules, tick_ms).await {
                    Ok(evaluation) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::to_value(evaluation).unwrap_or(Value::Null)),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: format!(
                                "Invalid params: {}",
                                format_schedule_validation_error(err)
                            ),
                        }),
                    },
                }
            }
            Err(message) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
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
                        id: response_id,
                        result: Some(serde_json::to_value(dispatch).unwrap_or(Value::Null)),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
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
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {message}"),
                }),
            },
        },
        "mobkit/routing/resolve" => {
            let resolve_result = match parse_routing_resolve_params(&request.params) {
                Ok(resolve_request) => runtime
                    .resolve_routing(resolve_request)
                    .await
                    .map_err(RoutingDeliveryParamsError::Routing),
                Err(e) => Err(e),
            };
            match resolve_result {
                Ok(resolution) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(resolution).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/routing/routes/list" => match parse_routing_routes_list_params(&request.params) {
            Ok(()) => {
                let routes = runtime.list_runtime_routes().await;
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "routes": routes
                    })),
                    error: None,
                }
            }
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/routing/routes/add" => {
            let add_result = match parse_routing_route_add_params(&request.params) {
                Ok(route) => runtime
                    .add_runtime_route(route)
                    .await
                    .map_err(RoutingDeliveryParamsError::RouteMutation),
                Err(e) => Err(e),
            };
            match add_result {
                Ok(route) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({ "route": route })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/routing/routes/delete" => {
            let delete_result = match parse_routing_route_delete_params(&request.params) {
                Ok(route_key) => runtime
                    .delete_runtime_route(&route_key)
                    .await
                    .map_err(RoutingDeliveryParamsError::RouteMutation),
                Err(e) => Err(e),
            };
            match delete_result {
                Ok(route) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({ "deleted": route })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/delivery/send" => {
            let send_result = match parse_delivery_send_params(&request.params) {
                Ok(send_request) => runtime
                    .send_delivery(send_request)
                    .await
                    .map_err(RoutingDeliveryParamsError::Delivery),
                Err(e) => Err(e),
            };
            match send_result {
                Ok(record) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(record).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/delivery/history" => match parse_delivery_history_params(&request.params) {
            Ok(history_request) => {
                let history = runtime.delivery_history(history_request).await;
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(history).unwrap_or(Value::Null)),
                    error: None,
                }
            }
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/events/subscribe" => match parse_subscribe_request(&request.params) {
            Ok(subscribe_request) => match runtime.subscribe_events(subscribe_request).await {
                Ok(subscribe_result) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(subscribe_result).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {err}"),
                    }),
                },
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/memory/stores" => match parse_memory_stores_params(&request.params) {
            Ok(()) => {
                let stores = runtime.memory_stores().await;
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "stores": stores,
                    })),
                    error: None,
                }
            }
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/memory/index" => match parse_memory_index_params(&request.params) {
            Ok(index_request) => match runtime.memory_index(index_request).await {
                Ok(indexed) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(indexed).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(MemoryIndexError::BackendPersistFailed(error)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
                    id: response_id,
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
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/memory/query" => match parse_memory_query_params(&request.params) {
            Ok(query_request) => {
                let query_result = runtime.memory_query(query_request).await;
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(query_result).unwrap_or(Value::Null)),
                    error: None,
                }
            }
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
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
                    id: response_id,
                    result: Some(result),
                    error: None,
                },
                Err(BigQuerySessionStoreRpcError::Params(message)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {message}"),
                    }),
                },
                Err(BigQuerySessionStoreRpcError::Store(error)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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
            Ok(gating_request) => {
                let gating_result = runtime.evaluate_gating_action(gating_request).await;
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(gating_result).unwrap_or(Value::Null)),
                    error: None,
                }
            }
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/gating/pending" => match parse_gating_pending_params(&request.params) {
            Ok(()) => {
                let pending = runtime.list_gating_pending().await;
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "pending": pending,
                    })),
                    error: None,
                }
            }
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/gating/decide" => {
            let decide_result = match parse_gating_decide_params(&request.params) {
                Ok(decide_request) => runtime
                    .decide_gating_action(decide_request)
                    .await
                    .map_err(GatingParamsError::Decision),
                Err(e) => Err(e),
            };
            match decide_result {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {}", err.message()),
                    }),
                },
            }
        }
        "mobkit/gating/audit" => match parse_gating_audit_params(&request.params) {
            Ok(limit) => {
                let entries = runtime.gating_audit_entries(limit).await;
                JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "entries": entries,
                    })),
                    error: None,
                }
            }
            Err(err) => JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", err.message()),
                }),
            },
        },
        "mobkit/call_tool" => {
            let module_id = request.params.get("module_id").and_then(Value::as_str);
            let tool = request.params.get("tool").and_then(Value::as_str);
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::json!({}));

            match (module_id, tool) {
                (Some(module_id), Some(tool)) if !module_id.is_empty() && !tool.is_empty() => {
                    let route = runtime
                        .route_module_call(
                            &ModuleRouteRequest {
                                module_id: module_id.to_string(),
                                method: tool.to_string(),
                                params: arguments,
                            },
                            timeout,
                        )
                        .await;
                    match route {
                        Ok(response) => JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: Some(serde_json::json!({
                                "module_id": response.module_id,
                                "tool": response.method,
                                "result": response.payload
                            })),
                            error: None,
                        },
                        Err(ModuleRouteError::UnloadedModule(mid)) => JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32601,
                                message: format!("Module '{mid}' not loaded"),
                            }),
                        },
                        Err(err) => JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32000,
                                message: format!("Tool call failed: {err:?}"),
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
                        message: "Invalid params: module_id and tool required".to_string(),
                    }),
                },
            }
        }
        "mobkit/models/catalog" => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(build_models_catalog_result()),
            error: None,
        },
        "mobkit/send_message" => {
            mob_methods::handle_send_message(runtime, response_id, &request.params).await
        }
        "mobkit/find_members" => {
            mob_methods::handle_find_members(runtime, response_id, &request.params).await
        }
        "mobkit/ensure_member" => {
            mob_methods::handle_ensure_member(runtime, response_id, &request.params).await
        }
        "mobkit/list_members" => mob_methods::handle_list_members(runtime, response_id).await,
        "mobkit/get_member" => {
            mob_methods::handle_get_member(runtime, response_id, &request.params).await
        }
        "mobkit/retire_member" => {
            mob_methods::handle_retire_member(runtime, response_id, &request.params).await
        }
        "mobkit/respawn_member" => {
            mob_methods::handle_respawn_member(runtime, response_id, &request.params).await
        }
        "mobkit/reconcile_edges" => mob_methods::handle_reconcile_edges(runtime, response_id).await,
        "mobkit/rediscover" => mob_methods::handle_rediscover(runtime, response_id).await,
        "mobkit/query_events" => {
            mob_methods::handle_query_events(runtime, response_id, request.params).await
        }
        "mobkit/cross_mob/wire" => {
            mob_methods::handle_cross_mob_wire(runtime, response_id, &request.params).await
        }
        "mobkit/cross_mob/unwire" => {
            mob_methods::handle_cross_mob_unwire(runtime, response_id, &request.params).await
        }
        "mobkit/cross_mob/send" => {
            mob_methods::handle_cross_mob_send(runtime, response_id, &request.params).await
        }
        "mobkit/cross_mob/directory" => {
            mob_methods::handle_cross_mob_directory(runtime, response_id).await
        }
        "mobkit/cross_mob/peer_info" => {
            mob_methods::handle_cross_mob_peer_info(runtime, response_id, &request.params).await
        }
        "mobkit/cross_mob/wire_local" => {
            mob_methods::handle_cross_mob_wire_local(runtime, response_id, &request.params).await
        }
        "mobkit/cross_mob/unwire_local" => {
            mob_methods::handle_cross_mob_unwire_local(runtime, response_id, &request.params).await
        }
        "mobkit/member_status" => {
            mob_methods::handle_member_status(runtime, response_id, &request.params).await
        }
        "mobkit/force_cancel_member" => {
            mob_methods::handle_force_cancel_member(runtime, response_id, &request.params).await
        }
        "mobkit/spawn_helper" => {
            mob_methods::handle_spawn_helper(runtime, response_id, &request.params).await
        }
        "mobkit/fork_helper" => {
            mob_methods::handle_fork_helper(runtime, response_id, &request.params).await
        }
        "mobkit/attach_existing_session" => {
            mob_methods::handle_attach_existing_session(runtime, response_id, &request.params).await
        }
        "mobkit/cancel_flow" => {
            mob_methods::handle_cancel_flow(runtime, response_id, &request.params).await
        }
        "mobkit/flow_status" => {
            mob_methods::handle_flow_status(runtime, response_id, &request.params).await
        }
        "mobkit/collect_completed" => {
            mob_methods::handle_collect_completed(runtime, response_id).await
        }
        "mobkit/member_current_session_id" => {
            mob_methods::handle_member_current_session_id(runtime, response_id, &request.params)
                .await
        }
        "mobkit/read_session_history" => {
            mob_methods::handle_read_session_history(runtime, response_id, &request.params).await
        }
        "mobkit/member_session_ref" => {
            mob_methods::handle_member_session_ref(runtime, response_id, &request.params).await
        }
        // ----- identity-first methods -----
        "mobkit/interact" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return identity_not_configured(response_id),
            };
            let request_params: ConsoleInteractionRequest =
                match serde_json::from_value(request.params.clone()) {
                    Ok(params) => params,
                    Err(_) => {
                        return error_response(
                            response_id,
                            -32602,
                            "invalid params: expected { identity, content, origin }",
                        );
                    }
                };
            if let Err(message) = request_params.validate() {
                return error_response(response_id, -32602, format!("invalid params: {message}"));
            }
            let identity =
                match crate::identity_first::AgentIdentity::parse(&request_params.identity) {
                    Ok(id) => id,
                    Err(e) => {
                        return error_response(
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let content = request_params.content.clone();
            let origin = request_params.origin.clone();
            let interaction_id = mint_interaction_id();
            if let Ok(status) = identity_rt.status(&identity).await
                && matches!(
                    status.addressability,
                    crate::identity_first::AgentAddressability::InternalOnly
                )
            {
                return error_response(
                    response_id,
                    -32002,
                    format!("not addressable: {}", identity.as_str()),
                );
            }
            let runtime_member_id = identity_rt
                .runtime_id_for(&identity)
                .await
                .ok()
                .map(|runtime_id| runtime_id.to_string());
            let dispatch_input = crate::identity_first::DispatchInput::with_origin(
                request_params.content,
                map_dispatch_origin(&request_params.origin),
            )
            .with_correlation(interaction_id.clone());
            if let Err(message) = runtime
                .reserve_console_interaction(
                    identity.as_str(),
                    runtime_member_id.as_deref(),
                    &interaction_id,
                    &origin,
                    &content,
                )
                .await
            {
                return error_response(response_id, -32003, message);
            }
            let dispatch_result =
                AssertUnwindSafe(identity_rt.dispatch(&identity, &dispatch_input))
                    .catch_unwind()
                    .await;
            match dispatch_result {
                Err(panic_payload) => {
                    runtime
                        .discard_console_interaction(identity.as_str(), &interaction_id)
                        .await;
                    resume_unwind(panic_payload);
                }
                Ok(Ok((_token, _durable))) => {
                    runtime
                        .accept_console_interaction(identity.as_str(), &interaction_id)
                        .await;
                    if !identity_rt.has_session_bridge() {
                        runtime
                            .fail_console_interaction(
                                identity.as_str(),
                                &interaction_id,
                                "execution_unavailable",
                                serde_json::json!({
                                    "reason": "no_session_bridge",
                                }),
                            )
                            .await;
                    }
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(
                            serde_json::to_value(ConsoleInteractionAccepted {
                                interaction_id,
                                identity: identity.as_str().to_string(),
                            })
                            .unwrap_or_else(|_| serde_json::json!({})),
                        ),
                        error: None,
                    }
                }
                Ok(Err(e)) => {
                    runtime
                        .discard_console_interaction(identity.as_str(), &interaction_id)
                        .await;
                    identity_error_response(response_id, &e)
                }
            }
        }
        "mobkit/send" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return identity_not_configured(response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let identity = match crate::identity_first::AgentIdentity::parse(identity_str) {
                Ok(id) => id,
                Err(e) => {
                    return error_response(response_id, -32602, format!("invalid identity: {e}"));
                }
            };
            let content_val = request
                .params
                .get("content")
                .cloned()
                .unwrap_or(Value::Null);
            let content = if let Some(s) = content_val.as_str() {
                meerkat_core::ContentInput::Text(s.to_string())
            } else {
                meerkat_core::ContentInput::Text(content_val.to_string())
            };
            match identity_rt.send(&identity, &content).await {
                Ok(token) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({ "fencing_token": token.get() })),
                    error: None,
                },
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/dispatch" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return identity_not_configured(response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let identity = match crate::identity_first::AgentIdentity::parse(identity_str) {
                Ok(id) => id,
                Err(e) => {
                    return error_response(response_id, -32602, format!("invalid identity: {e}"));
                }
            };
            let di_val = request
                .params
                .get("dispatch_input")
                .cloned()
                .unwrap_or(Value::Null);
            let content_text = di_val
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let origin_str = di_val
                .get("origin")
                .and_then(|v| v.as_str())
                .unwrap_or("system");
            let origin = match origin_str {
                "connector" => crate::identity_first::DispatchOrigin::Connector,
                "scheduler" => crate::identity_first::DispatchOrigin::Scheduler,
                "policy" => crate::identity_first::DispatchOrigin::Policy,
                "flow" => crate::identity_first::DispatchOrigin::Flow,
                _ => crate::identity_first::DispatchOrigin::System,
            };
            let correlation_id = di_val
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .map(crate::identity_first::CorrelationId::new);
            let dispatch_input = crate::identity_first::DispatchInput {
                content: meerkat_core::ContentInput::Text(content_text),
                origin,
                correlation_id,
                idempotency_key: None,
            };
            match identity_rt.dispatch(&identity, &dispatch_input).await {
                Ok((token, durable)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(
                        serde_json::json!({ "fencing_token": token.get(), "durable": durable }),
                    ),
                    error: None,
                },
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/status_identity" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return identity_not_configured(response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let identity = match crate::identity_first::AgentIdentity::parse(identity_str) {
                Ok(id) => id,
                Err(e) => {
                    return error_response(response_id, -32602, format!("invalid identity: {e}"));
                }
            };
            match identity_rt.status(&identity).await {
                Ok(status) => {
                    let result = serde_json::json!({
                        "state": format!("{:?}", status.state),
                        "identity": identity_str,
                        "agent_runtime_id": status.agent_runtime_id.as_ref().map(super::identity_first::AgentRuntimeId::as_str),
                        "session_id": status.session_id.as_ref().map(ToString::to_string),
                        "profile": status.profile.as_ref().map(meerkat_mob::ProfileName::as_str),
                        "addressability": addressability_json(status.addressability),
                        "display_name": status.display_name.as_ref().map(super::identity_first::DisplayName::as_str),
                        "labels": status.labels,
                        "generation": status.generation.map(super::identity_first::ContinuityGeneration::get),
                        "checkpoint_version": status.checkpoint_version.map(super::identity_first::CheckpointVersion::get),
                        "lease_healthy": status.lease.as_ref().map(|lease| lease.healthy),
                        "lease": status.lease.as_ref().map(|lease| serde_json::json!({
                            "fencing_token": lease.fencing_token.get(),
                            "ttl_remaining_ms": lease.ttl_remaining.as_millis() as u64,
                            "healthy": lease.healthy,
                        })),
                    });
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(result),
                        error: None,
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/respawn" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return identity_not_configured(response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let identity = match crate::identity_first::AgentIdentity::parse(identity_str) {
                Ok(id) => id,
                Err(e) => {
                    return error_response(response_id, -32602, format!("invalid identity: {e}"));
                }
            };
            match identity_rt.respawn(&identity).await {
                Ok(record) => {
                    runtime
                        .record_console_lifecycle(
                            identity.as_str(),
                            "identity_respawned",
                            serde_json::json!({
                                "generation": record.generation.get(),
                                "checkpoint_version": record.checkpoint_version.get(),
                            }),
                        )
                        .await;
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({
                            "identity": record.identity.as_str(),
                            "agent_runtime_id": record.agent_runtime_id.as_str(),
                            "session_id": record.session_id.to_string(),
                            "generation": record.generation.get(),
                            "checkpoint_version": record.checkpoint_version.get(),
                        })),
                        error: None,
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/retire" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return identity_not_configured(response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let identity = match crate::identity_first::AgentIdentity::parse(identity_str) {
                Ok(id) => id,
                Err(e) => {
                    return error_response(response_id, -32602, format!("invalid identity: {e}"));
                }
            };
            match identity_rt.retire(&identity).await {
                Ok(token) => {
                    runtime
                        .record_console_lifecycle(
                            identity.as_str(),
                            "identity_retired",
                            serde_json::json!({ "fencing_token": token.get() }),
                        )
                        .await;
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({ "fencing_token": token.get() })),
                        error: None,
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/reset" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return identity_not_configured(response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let identity = match crate::identity_first::AgentIdentity::parse(identity_str) {
                Ok(id) => id,
                Err(e) => {
                    return error_response(response_id, -32602, format!("invalid identity: {e}"));
                }
            };
            match identity_rt.reset(&identity).await {
                Ok(record) => {
                    runtime
                        .record_console_lifecycle(
                            identity.as_str(),
                            "identity_reset",
                            serde_json::json!({
                                "generation": record.generation.get(),
                                "checkpoint_version": record.checkpoint_version.get(),
                            }),
                        )
                        .await;
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({
                            "identity": record.identity.as_str(),
                            "agent_runtime_id": record.agent_runtime_id.as_str(),
                            "session_id": record.session_id.to_string(),
                            "generation": record.generation.get(),
                            "checkpoint_version": record.checkpoint_version.get(),
                        })),
                        error: None,
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/delete_identity" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return identity_not_configured(response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let identity = match crate::identity_first::AgentIdentity::parse(identity_str) {
                Ok(id) => id,
                Err(e) => {
                    return error_response(response_id, -32602, format!("invalid identity: {e}"));
                }
            };
            match identity_rt.delete_identity(&identity).await {
                Ok(()) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({})),
                    error: None,
                },
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/inspect_identity" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return identity_not_configured(response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let identity = match crate::identity_first::AgentIdentity::parse(identity_str) {
                Ok(id) => id,
                Err(e) => {
                    return error_response(response_id, -32602, format!("invalid identity: {e}"));
                }
            };
            let status = identity_rt.status(&identity).await;
            match identity_rt.inspect(&identity).await {
                Ok(inspection) => {
                    let status = status.ok();
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({
                            "identity": identity_str,
                            "state": status.as_ref().map(|status| format!("{:?}", status.state)),
                            "profile": status.as_ref().and_then(|status| status.profile.as_ref().map(meerkat_mob::ProfileName::as_str)),
                            "addressability": status.as_ref().map(|status| addressability_json(status.addressability)),
                            "display_name": status.as_ref().and_then(|status| status.display_name.as_ref().map(super::identity_first::DisplayName::as_str)),
                            "labels": status.as_ref().map(|status| status.labels.clone()).unwrap_or_default(),
                            "generation": status.as_ref().and_then(|status| status.generation.map(super::identity_first::ContinuityGeneration::get)),
                            "checkpoint_version": status.as_ref().and_then(|status| status.checkpoint_version.map(super::identity_first::CheckpointVersion::get)),
                            "lease_healthy": status.as_ref().and_then(|status| status.lease.as_ref().map(|lease| lease.healthy)),
                            "continuity": status.as_ref().map(|status| serde_json::json!({
                                "generation": status.generation.map(super::identity_first::ContinuityGeneration::get),
                                "checkpoint_version": status.checkpoint_version.map(super::identity_first::CheckpointVersion::get),
                                "session_id": status.session_id.as_ref().map(ToString::to_string),
                                "agent_runtime_id": status.agent_runtime_id.as_ref().map(super::identity_first::AgentRuntimeId::as_str),
                            })).unwrap_or_else(|| serde_json::json!({})),
                            "lease": status.as_ref().and_then(|status| status.lease.as_ref().map(|lease| serde_json::json!({
                                "fencing_token": lease.fencing_token.get(),
                                "ttl_remaining_ms": lease.ttl_remaining.as_millis() as u64,
                                "healthy": lease.healthy,
                            }))),
                            "output_preview": inspection.output_preview,
                            "is_final": inspection.is_final,
                            "peer_reachable_count": inspection.peer_reachable_count,
                        })),
                        error: None,
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/reconcile_identity" => {
            let ctx = match identity_ctx {
                Some(ctx) => ctx,
                None => return identity_not_configured(response_id),
            };
            // Re-fetch roster from provider and re-run restore_flow
            let roster_specs = match ctx
                .roster_provider
                .roster(&crate::identity_first::RosterContext {
                    mob_definition: None,
                    previous_identities: Vec::new(),
                })
                .await
            {
                Ok(specs) => specs,
                Err(e) => {
                    return error_response(
                        response_id,
                        -32603,
                        format!("roster provider failed: {e}"),
                    );
                }
            };
            match crate::identity_first::restore_flow(
                &ctx.runtime,
                &roster_specs,
                ctx.topology_provider.as_deref(),
                ctx.customizer.as_deref(),
            )
            .await
            {
                Ok(result) => {
                    let outcomes: serde_json::Map<String, Value> = result
                        .outcomes
                        .iter()
                        .map(|(id, outcome)| {
                            let val = match outcome {
                                crate::identity_first::RestoreOutcome::Created {
                                    record, ..
                                } => {
                                    serde_json::json!({
                                        "outcome": "created",
                                        "identity": record.identity.as_str(),
                                        "agent_runtime_id": record.agent_runtime_id.as_str(),
                                        "session_id": record.session_id.to_string(),
                                        "generation": record.generation.get(),
                                    })
                                }
                                crate::identity_first::RestoreOutcome::Resumed {
                                    record, ..
                                } => {
                                    serde_json::json!({
                                        "outcome": "resumed",
                                        "identity": record.identity.as_str(),
                                        "agent_runtime_id": record.agent_runtime_id.as_str(),
                                        "session_id": record.session_id.to_string(),
                                        "generation": record.generation.get(),
                                    })
                                }
                                crate::identity_first::RestoreOutcome::Broken(failure) => {
                                    serde_json::json!({
                                        "outcome": "broken",
                                        "identity": failure.identity.as_str(),
                                        "detail": failure.detail,
                                    })
                                }
                            };
                            (id.to_string(), val)
                        })
                        .collect();
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({
                            "outcomes": outcomes,
                            "managed_edges": result.managed_edges.len(),
                        })),
                        error: None,
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        method if method.contains('/') && !method.starts_with("mobkit/") => {
            let module_id = method
                .split('/')
                .next()
                .map(ToString::to_string)
                .unwrap_or_default();
            let route = runtime
                .route_module_call(
                    &ModuleRouteRequest {
                        module_id: module_id.clone(),
                        method: method.to_string(),
                        params: request.params,
                    },
                    timeout,
                )
                .await;
            match route {
                Ok(response) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "module_id": response.module_id,
                        "method": response.method,
                        "payload": response.payload
                    })),
                    error: None,
                },
                Err(ModuleRouteError::UnloadedModule(module_id)) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: format!("Module '{module_id}' not loaded"),
                    }),
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
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

fn build_models_catalog_result() -> Value {
    let entries: Vec<Value> = meerkat_models::catalog()
        .iter()
        .filter_map(|e| {
            let mut val = serde_json::to_value(e).ok()?;
            if let Some(profile) = meerkat_models::profile_for(e.provider, e.id)
                && let Ok(p) = serde_json::to_value(&profile)
            {
                val["profile"] = p;
            }
            Some(val)
        })
        .collect();
    let defaults: Vec<Value> = meerkat_models::provider_defaults()
        .iter()
        .filter_map(|d| serde_json::to_value(d).ok())
        .collect();
    serde_json::json!({
        "models": entries,
        "provider_defaults": defaults,
    })
}

fn identity_not_configured(response_id: Value) -> String {
    error_response(response_id, -32601, "identity-first runtime not configured")
}

fn map_dispatch_origin(origin: &str) -> crate::identity_first::DispatchOrigin {
    match origin.split(':').next().unwrap_or("system") {
        "connector" => crate::identity_first::DispatchOrigin::Connector,
        "scheduler" => crate::identity_first::DispatchOrigin::Scheduler,
        "policy" => crate::identity_first::DispatchOrigin::Policy,
        "flow" => crate::identity_first::DispatchOrigin::Flow,
        _ => crate::identity_first::DispatchOrigin::System,
    }
}

fn mint_interaction_id() -> String {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("turn-{now_ns}")
}

fn addressability_json(addressability: crate::identity_first::AgentAddressability) -> &'static str {
    match addressability {
        crate::identity_first::AgentAddressability::Addressable => "addressable",
        crate::identity_first::AgentAddressability::InternalOnly => "internal_only",
    }
}

fn identity_error_response(
    response_id: Value,
    err: &crate::identity_first::IdentityRuntimeError,
) -> JsonRpcResponse {
    use crate::identity_first::IdentityRuntimeError;
    let (code, message) = match err {
        IdentityRuntimeError::UnknownIdentity(id) => (-32001, format!("unknown identity: {id}")),
        IdentityRuntimeError::NotAddressable(na) => {
            (-32002, format!("not addressable: {}", na.identity))
        }
        IdentityRuntimeError::NoActiveLease(id) => (-32003, format!("no active lease: {id}")),
        IdentityRuntimeError::LeaseLost(id) => (-32004, format!("lease lost: {id}")),
        _ => (-32603, format!("{err}")),
    };
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: None,
        error: Some(JsonRpcError { code, message }),
    }
}

fn error_response(response_id: Value, code: i64, message: impl Into<String>) -> String {
    serialize_response(&JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.into(),
        }),
    })
}

fn serialize_response(response: &JsonRpcResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
            .to_string()
    })
}
