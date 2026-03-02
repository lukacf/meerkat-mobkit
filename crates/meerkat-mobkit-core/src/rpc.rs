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

pub const MOBKIT_CONTRACT_VERSION: &str = "0.1.0";
pub const MAX_SCHEDULES_PER_REQUEST: usize = 256;
const MEMORY_SUPPORTED_STORES: [&str; 5] = [
    "knowledge_graph",
    "vector",
    "timeline",
    "todo",
    "top_of_mind",
];

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum BigQuerySessionStoreOperation {
    StreamInsert,
    ReadAll,
    ReadLatest,
    ReadLive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BigQuerySessionStoreRequest {
    operation: BigQuerySessionStoreOperation,
    dataset: String,
    table: String,
    project_id: Option<String>,
    access_token: Option<String>,
    api_base_url: Option<String>,
    timeout_ms: Option<u64>,
    rows: Vec<SessionPersistenceRow>,
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
                jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
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
            jsonrpc: "2.0".to_string(),
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
            jsonrpc: "2.0".to_string(),
            id: response_id.clone(),
            result: Some(serde_json::json!({
                "contract_version": MOBKIT_CONTRACT_VERSION,
                "running": runtime.is_running(),
                "loaded_modules": runtime.loaded_modules(),
            })),
            error: None,
        },
        "mobkit/capabilities" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::json!({
                        "accepted": true,
                        "reconciled_modules": modules,
                        "added": added
                    })),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
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
                        jsonrpc: "2.0".to_string(),
                        id: response_id.clone(),
                        result: Some(serde_json::json!({
                            "accepted": true,
                            "module_id": module_id
                        })),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(evaluation).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(dispatch).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(resolution).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "routes": runtime.list_runtime_routes()
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({ "route": route })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({ "deleted": route })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(record).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
                id: response_id.clone(),
                result: Some(
                    serde_json::to_value(runtime.delivery_history(history_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(subscribe_result).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "stores": runtime.memory_stores(),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(indexed).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(MemoryIndexError::BackendPersistFailed(error)) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
                id: response_id.clone(),
                result: Some(
                    serde_json::to_value(runtime.memory_query(query_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: Some(result),
                    error: None,
                },
                Err(BigQuerySessionStoreRpcError::Params(message)) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid params: {message}"),
                    }),
                },
                Err(BigQuerySessionStoreRpcError::Store(error)) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
                id: response_id.clone(),
                result: Some(
                    serde_json::to_value(runtime.evaluate_gating_action(gating_request))
                        .unwrap_or(Value::Null),
                ),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "pending": runtime.list_gating_pending(),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
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
                jsonrpc: "2.0".to_string(),
                id: response_id.clone(),
                result: Some(serde_json::json!({
                    "entries": runtime.gating_audit_entries(limit),
                })),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
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
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: Some(serde_json::json!({
                        "module_id": response.module_id,
                        "method": response.method,
                        "payload": response.payload
                    })),
                    error: None,
                },
                Err(ModuleRouteError::UnloadedModule(module_id)) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: response_id.clone(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: format!("Module '{module_id}' not loaded"),
                    }),
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
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
            jsonrpc: "2.0".to_string(),
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

pub fn handle_console_ingress_json(decisions: &RuntimeDecisionState, request_json: &str) -> String {
    let request: ConsoleRestJsonRequest = match serde_json::from_str(request_json) {
        Ok(request) => request,
        Err(_) => {
            let response = ConsoleRestJsonResponse {
                status: 400,
                body: serde_json::json!({"error":"invalid_request"}),
            };
            return serde_json::to_string(&response).unwrap_or_else(|_| {
                r#"{"status":500,"body":{"error":"internal_error"}}"#.to_string()
            });
        }
    };
    let response = handle_console_rest_json_route(decisions, &request);
    serde_json::to_string(&response)
        .unwrap_or_else(|_| r#"{"status":500,"body":{"error":"internal_error"}}"#.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubscribeParamsError {
    ParamsMustBeObject,
    ScopeMustBeString,
    UnsupportedScope(String),
    LastEventIdMustBeString,
    AgentIdMustBeString,
    Runtime(SubscribeError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RoutingDeliveryParamsError {
    ParamsMustBeObject,
    RouteFieldMustBeObject,
    RouteKeyRequired,
    RecipientRequired,
    ChannelMustBeString,
    SinkMustBeString,
    TargetModuleMustBeString,
    RetryMaxMustBeInteger,
    RetryMaxOverflow,
    RetryMaxAboveCap { cap: u32 },
    BackoffMsMustBeInteger,
    RateLimitMustBeInteger,
    RateLimitMustBePositive,
    RateLimitOverflow,
    ResolutionRequired,
    PayloadRequired,
    IdempotencyKeyMustBeString,
    HistoryRecipientMustBeString,
    HistorySinkMustBeString,
    HistoryLimitOutOfRange,
    Routing(RoutingResolveError),
    Delivery(DeliverySendError),
    RouteMutation(RuntimeRouteMutationError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GatingParamsError {
    ParamsMustBeObject,
    ActionRequired,
    ActorRequired,
    RiskTierRequired,
    UnknownRiskTier(String),
    RationaleMustBeString,
    RequestedApproverMustBeString,
    ApprovalRecipientMustBeString,
    ApprovalChannelMustBeString,
    ApprovalTimeoutMustBeInteger,
    EntityMustBeString,
    TopicMustBeString,
    PendingIdRequired,
    ApproverIdRequired,
    DecisionRequired,
    UnknownDecision(String),
    ReasonMustBeString,
    LimitOutOfRange,
    Decision(GatingDecideError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemoryParamsError {
    ParamsMustBeObject,
    EntityRequired,
    TopicRequired,
    StoreMustBeString,
    UnsupportedStore(String),
    FactMustBeString,
    MetadataMustBeJson,
    ConflictMustBeBoolean,
    ConflictReasonMustBeString,
    EntityMustBeString,
    TopicMustBeString,
    Index(MemoryIndexError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BigQuerySessionStoreRpcError {
    Params(String),
    Store(BigQuerySessionStoreError),
}

impl RoutingDeliveryParamsError {
    fn message(&self) -> String {
        match self {
            RoutingDeliveryParamsError::ParamsMustBeObject => {
                "params must be a JSON object".to_string()
            }
            RoutingDeliveryParamsError::RouteFieldMustBeObject => {
                "route must be an object".to_string()
            }
            RoutingDeliveryParamsError::RouteKeyRequired => {
                "route_key must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::RecipientRequired => {
                "recipient must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::ChannelMustBeString => {
                "channel must be a string".to_string()
            }
            RoutingDeliveryParamsError::SinkMustBeString => "sink must be a string".to_string(),
            RoutingDeliveryParamsError::TargetModuleMustBeString => {
                "target_module must be a string".to_string()
            }
            RoutingDeliveryParamsError::RetryMaxMustBeInteger => {
                "retry_max must be a non-negative integer".to_string()
            }
            RoutingDeliveryParamsError::RetryMaxOverflow => {
                "retry_max exceeds maximum supported integer range".to_string()
            }
            RoutingDeliveryParamsError::RetryMaxAboveCap { cap } => {
                format!("retry_max must be <= {cap}")
            }
            RoutingDeliveryParamsError::BackoffMsMustBeInteger => {
                "backoff_ms must be a non-negative integer".to_string()
            }
            RoutingDeliveryParamsError::RateLimitMustBeInteger => {
                "rate_limit_per_minute must be a non-negative integer".to_string()
            }
            RoutingDeliveryParamsError::RateLimitMustBePositive => {
                "rate_limit_per_minute must be greater than 0".to_string()
            }
            RoutingDeliveryParamsError::RateLimitOverflow => {
                "rate_limit_per_minute exceeds maximum supported integer range".to_string()
            }
            RoutingDeliveryParamsError::ResolutionRequired => {
                "resolution must be an object".to_string()
            }
            RoutingDeliveryParamsError::PayloadRequired => "payload is required".to_string(),
            RoutingDeliveryParamsError::IdempotencyKeyMustBeString => {
                "idempotency_key must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::HistoryRecipientMustBeString => {
                "recipient filter must be a string".to_string()
            }
            RoutingDeliveryParamsError::HistorySinkMustBeString => {
                "sink filter must be a string".to_string()
            }
            RoutingDeliveryParamsError::HistoryLimitOutOfRange => {
                "limit must be an integer between 1 and 200".to_string()
            }
            RoutingDeliveryParamsError::Routing(RoutingResolveError::RouterModuleNotLoaded) => {
                "router module is not loaded".to_string()
            }
            RoutingDeliveryParamsError::Routing(RoutingResolveError::DeliveryModuleNotLoaded) => {
                "delivery module is not loaded".to_string()
            }
            RoutingDeliveryParamsError::Routing(RoutingResolveError::EmptyRecipient) => {
                "recipient must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::Routing(RoutingResolveError::InvalidChannel) => {
                "channel must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::Routing(RoutingResolveError::InvalidRateLimitPerMinute) => {
                "rate_limit_per_minute must be greater than 0".to_string()
            }
            RoutingDeliveryParamsError::Routing(RoutingResolveError::RetryMaxExceedsCap {
                cap,
                ..
            }) => {
                format!("retry_max must be <= {cap}")
            }
            RoutingDeliveryParamsError::Routing(RoutingResolveError::RouterBoundary(err)) => {
                format!("router boundary failed: {err:?}")
            }
            RoutingDeliveryParamsError::Delivery(DeliverySendError::DeliveryModuleNotLoaded) => {
                "delivery module is not loaded".to_string()
            }
            RoutingDeliveryParamsError::Delivery(DeliverySendError::InvalidRouteTarget(target)) => {
                format!("resolution.target_module must be 'delivery' (got '{target}')")
            }
            RoutingDeliveryParamsError::Delivery(DeliverySendError::InvalidRouteId) => {
                "resolution.route_id must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::Delivery(DeliverySendError::UnknownRouteId(route_id)) => {
                format!("resolution.route_id '{route_id}' was not issued by routing/resolve")
            }
            RoutingDeliveryParamsError::Delivery(DeliverySendError::ForgedResolution) => {
                "resolution does not match the trusted route for route_id".to_string()
            }
            RoutingDeliveryParamsError::Delivery(DeliverySendError::InvalidRecipient) => {
                "resolution.recipient must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::Delivery(DeliverySendError::InvalidSink) => {
                "resolution.sink must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::Delivery(DeliverySendError::InvalidIdempotencyKey) => {
                "idempotency_key must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::Delivery(DeliverySendError::IdempotencyPayloadMismatch) => {
                "idempotency_key replay payload does not match original request".to_string()
            }
            RoutingDeliveryParamsError::Delivery(DeliverySendError::RateLimited {
                sink,
                window_start_ms,
                limit,
            }) => format!(
                "rate limit exceeded for sink '{sink}' at window {window_start_ms} (limit={limit})"
            ),
            RoutingDeliveryParamsError::Delivery(DeliverySendError::DeliveryBoundary(err)) => {
                format!("delivery boundary failed: {err:?}")
            }
            RoutingDeliveryParamsError::RouteMutation(RuntimeRouteMutationError::EmptyRouteKey) => {
                "route_key must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::RouteMutation(
                RuntimeRouteMutationError::EmptyRecipient,
            ) => "recipient must be a non-empty string".to_string(),
            RoutingDeliveryParamsError::RouteMutation(
                RuntimeRouteMutationError::InvalidChannel,
            ) => "channel must be a non-empty string when provided".to_string(),
            RoutingDeliveryParamsError::RouteMutation(RuntimeRouteMutationError::EmptySink) => {
                "sink must be a non-empty string".to_string()
            }
            RoutingDeliveryParamsError::RouteMutation(
                RuntimeRouteMutationError::EmptyTargetModule,
            ) => "target_module must be a non-empty string".to_string(),
            RoutingDeliveryParamsError::RouteMutation(
                RuntimeRouteMutationError::InvalidRateLimitPerMinute,
            ) => "rate_limit_per_minute must be greater than 0".to_string(),
            RoutingDeliveryParamsError::RouteMutation(
                RuntimeRouteMutationError::RetryMaxExceedsCap { cap, .. },
            ) => format!("retry_max must be <= {cap}"),
            RoutingDeliveryParamsError::RouteMutation(
                RuntimeRouteMutationError::RouteNotFound(route_key),
            ) => format!("route_key '{route_key}' was not found"),
        }
    }
}

impl GatingParamsError {
    fn message(&self) -> String {
        match self {
            GatingParamsError::ParamsMustBeObject => "params must be a JSON object".to_string(),
            GatingParamsError::ActionRequired => "action must be a non-empty string".to_string(),
            GatingParamsError::ActorRequired => "actor_id must be a non-empty string".to_string(),
            GatingParamsError::RiskTierRequired => {
                "risk_tier must be one of: r0, r1, r2, r3".to_string()
            }
            GatingParamsError::UnknownRiskTier(tier) => {
                format!("risk_tier '{tier}' is unsupported (allowed: r0, r1, r2, r3)")
            }
            GatingParamsError::RationaleMustBeString => "rationale must be a string".to_string(),
            GatingParamsError::RequestedApproverMustBeString => {
                "requested_approver must be a string".to_string()
            }
            GatingParamsError::ApprovalRecipientMustBeString => {
                "approval_recipient must be a string".to_string()
            }
            GatingParamsError::ApprovalChannelMustBeString => {
                "approval_channel must be a string".to_string()
            }
            GatingParamsError::ApprovalTimeoutMustBeInteger => {
                "approval_timeout_ms must be a non-negative integer".to_string()
            }
            GatingParamsError::EntityMustBeString => {
                "entity must be a string when provided".to_string()
            }
            GatingParamsError::TopicMustBeString => {
                "topic must be a string when provided".to_string()
            }
            GatingParamsError::PendingIdRequired => {
                "pending_id must be a non-empty string".to_string()
            }
            GatingParamsError::ApproverIdRequired => {
                "approver_id must be a non-empty string".to_string()
            }
            GatingParamsError::DecisionRequired => {
                "decision must be either 'approve' or 'reject'".to_string()
            }
            GatingParamsError::UnknownDecision(decision) => {
                format!("decision '{decision}' is unsupported (allowed: approve, reject)")
            }
            GatingParamsError::ReasonMustBeString => "reason must be a string".to_string(),
            GatingParamsError::LimitOutOfRange => {
                "limit must be an integer between 1 and 500".to_string()
            }
            GatingParamsError::Decision(GatingDecideError::UnknownPendingId(pending_id)) => {
                format!("pending_id '{pending_id}' was not found")
            }
            GatingParamsError::Decision(GatingDecideError::SelfApprovalForbidden) => {
                "approver_id cannot self-approve the action actor".to_string()
            }
            GatingParamsError::Decision(GatingDecideError::ApproverMismatch {
                expected,
                provided,
            }) => {
                format!("approver_id '{provided}' does not match requested_approver '{expected}'")
            }
        }
    }
}

impl MemoryParamsError {
    fn backend_message(error: &ElephantMemoryStoreError) -> String {
        match error {
            ElephantMemoryStoreError::InvalidConfig(reason)
            | ElephantMemoryStoreError::Io(reason)
            | ElephantMemoryStoreError::Serialize(reason)
            | ElephantMemoryStoreError::InvalidStoreData(reason)
            | ElephantMemoryStoreError::ExternalCallFailed(reason) => reason.clone(),
        }
    }

    fn message(&self) -> String {
        match self {
            MemoryParamsError::ParamsMustBeObject => {
                "params must be a JSON object".to_string()
            }
            MemoryParamsError::EntityRequired => "entity must be a non-empty string".to_string(),
            MemoryParamsError::TopicRequired => "topic must be a non-empty string".to_string(),
            MemoryParamsError::StoreMustBeString => {
                "store must be a non-empty string when provided".to_string()
            }
            MemoryParamsError::UnsupportedStore(store) => format!(
                "store '{store}' is unsupported (allowed: knowledge_graph, vector, timeline, todo, top_of_mind)"
            ),
            MemoryParamsError::FactMustBeString => {
                "fact must be a non-empty string when provided".to_string()
            }
            MemoryParamsError::MetadataMustBeJson => {
                "metadata must be a JSON object when provided".to_string()
            }
            MemoryParamsError::ConflictMustBeBoolean => {
                "conflict must be a boolean when provided".to_string()
            }
            MemoryParamsError::ConflictReasonMustBeString => {
                "conflict_reason must be a string when provided".to_string()
            }
            MemoryParamsError::EntityMustBeString => {
                "entity filter must be a string".to_string()
            }
            MemoryParamsError::TopicMustBeString => {
                "topic filter must be a string".to_string()
            }
            MemoryParamsError::Index(MemoryIndexError::EntityRequired) => {
                "entity must be a non-empty string".to_string()
            }
            MemoryParamsError::Index(MemoryIndexError::TopicRequired) => {
                "topic must be a non-empty string".to_string()
            }
            MemoryParamsError::Index(MemoryIndexError::UnsupportedStore(store)) => format!(
                "store '{store}' is unsupported (allowed: knowledge_graph, vector, timeline, todo, top_of_mind)"
            ),
            MemoryParamsError::Index(MemoryIndexError::FactRequiredWhenConflictUnset) => {
                "fact is required unless conflict=true".to_string()
            }
            MemoryParamsError::Index(MemoryIndexError::BackendPersistFailed(error)) => {
                format!(
                    "memory backend persistence failed: {}",
                    Self::backend_message(error)
                )
            }
        }
    }
}

impl SubscribeParamsError {
    fn message(&self) -> String {
        match self {
            SubscribeParamsError::ParamsMustBeObject => {
                "subscribe params must be a JSON object".to_string()
            }
            SubscribeParamsError::ScopeMustBeString => "scope must be a string".to_string(),
            SubscribeParamsError::UnsupportedScope(scope) => {
                format!("unsupported scope '{scope}' (allowed: mob, agent, interaction)")
            }
            SubscribeParamsError::LastEventIdMustBeString => {
                "last_event_id must be a string".to_string()
            }
            SubscribeParamsError::AgentIdMustBeString => "agent_id must be a string".to_string(),
            SubscribeParamsError::Runtime(SubscribeError::EmptyCheckpoint) => {
                "last_event_id cannot be empty".to_string()
            }
            SubscribeParamsError::Runtime(SubscribeError::UnknownCheckpoint(checkpoint)) => {
                format!("unknown last_event_id '{checkpoint}'")
            }
            SubscribeParamsError::Runtime(SubscribeError::MissingAgentId) => {
                "agent_id is required when scope is 'agent'".to_string()
            }
            SubscribeParamsError::Runtime(SubscribeError::InvalidAgentId) => {
                "agent_id cannot be empty when scope is 'agent'".to_string()
            }
        }
    }
}

fn parse_bigquery_session_store_params(
    params: &Value,
) -> Result<BigQuerySessionStoreRequest, BigQuerySessionStoreRpcError> {
    let object = params.as_object().ok_or_else(|| {
        BigQuerySessionStoreRpcError::Params("params must be a JSON object".to_string())
    })?;

    let operation_raw = object
        .get("operation")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            BigQuerySessionStoreRpcError::Params(
                "operation must be one of: stream_insert_rows, read_rows, read_latest_rows, read_live_rows"
                    .to_string(),
            )
        })?;
    let operation = match operation_raw {
        "stream_insert_rows" | "stream_insert" => BigQuerySessionStoreOperation::StreamInsert,
        "read_rows" => BigQuerySessionStoreOperation::ReadAll,
        "read_latest_rows" | "read_latest" => BigQuerySessionStoreOperation::ReadLatest,
        "read_live_rows" | "read_live" => BigQuerySessionStoreOperation::ReadLive,
        _ => {
            return Err(BigQuerySessionStoreRpcError::Params(format!(
                "unsupported operation '{operation_raw}'"
            )));
        }
    };

    let dataset = object
        .get("dataset")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            BigQuerySessionStoreRpcError::Params("dataset must be a non-empty string".to_string())
        })?
        .to_string();
    let table = object
        .get("table")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            BigQuerySessionStoreRpcError::Params("table must be a non-empty string".to_string())
        })?
        .to_string();

    let project_id = parse_optional_bigquery_string_field(object, "project_id")?;
    let access_token = parse_optional_bigquery_string_field(object, "access_token")?;
    let api_base_url = parse_optional_bigquery_string_field(object, "api_base_url")?;
    let timeout_ms = match object.get("timeout_ms") {
        None => None,
        Some(value) => {
            let timeout_ms = value.as_u64().ok_or_else(|| {
                BigQuerySessionStoreRpcError::Params(
                    "timeout_ms must be a positive integer when provided".to_string(),
                )
            })?;
            if timeout_ms == 0 {
                return Err(BigQuerySessionStoreRpcError::Params(
                    "timeout_ms must be greater than 0".to_string(),
                ));
            }
            Some(timeout_ms)
        }
    };

    let rows = match operation {
        BigQuerySessionStoreOperation::StreamInsert => {
            let rows_value = object.get("rows").ok_or_else(|| {
                BigQuerySessionStoreRpcError::Params(
                    "rows must be provided for stream_insert_rows".to_string(),
                )
            })?;
            serde_json::from_value::<Vec<SessionPersistenceRow>>(rows_value.clone()).map_err(
                |_| {
                    BigQuerySessionStoreRpcError::Params(
                        "rows must be an array of session persistence rows".to_string(),
                    )
                },
            )?
        }
        _ => {
            if object.contains_key("rows") {
                return Err(BigQuerySessionStoreRpcError::Params(
                    "rows is only valid for stream_insert_rows".to_string(),
                ));
            }
            Vec::new()
        }
    };

    Ok(BigQuerySessionStoreRequest {
        operation,
        dataset,
        table,
        project_id,
        access_token,
        api_base_url,
        timeout_ms,
        rows,
    })
}

fn parse_optional_bigquery_string_field(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<String>, BigQuerySessionStoreRpcError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let parsed = value.as_str().ok_or_else(|| {
                BigQuerySessionStoreRpcError::Params(format!(
                    "{field} must be a non-empty string when provided"
                ))
            })?;
            let trimmed = parsed.trim();
            if trimmed.is_empty() {
                return Err(BigQuerySessionStoreRpcError::Params(format!(
                    "{field} must be a non-empty string when provided"
                )));
            }
            Ok(Some(trimmed.to_string()))
        }
    }
}

fn run_bigquery_session_store_request(
    request: BigQuerySessionStoreRequest,
) -> Result<Value, BigQuerySessionStoreRpcError> {
    let mut store = BigQuerySessionStoreAdapter::new_native(request.dataset, request.table);
    if let Some(project_id) = request.project_id {
        store = store.with_project_id(project_id);
    }
    if let Some(access_token) = request.access_token {
        store = store.with_access_token(access_token);
    }
    if let Some(api_base_url) = request.api_base_url {
        store = store.with_api_base_url(api_base_url);
    }
    if let Some(timeout_ms) = request.timeout_ms {
        store = store.with_http_timeout(Duration::from_millis(timeout_ms));
    }

    match request.operation {
        BigQuerySessionStoreOperation::StreamInsert => {
            store
                .stream_insert_rows(&request.rows)
                .map_err(BigQuerySessionStoreRpcError::Store)?;
            Ok(serde_json::json!({
                "operation": "stream_insert_rows",
                "accepted": true,
                "inserted_rows": request.rows.len(),
            }))
        }
        BigQuerySessionStoreOperation::ReadAll => {
            let rows = store
                .read_rows()
                .map_err(BigQuerySessionStoreRpcError::Store)?;
            Ok(serde_json::json!({
                "operation": "read_rows",
                "rows": rows,
            }))
        }
        BigQuerySessionStoreOperation::ReadLatest => {
            let rows = store
                .read_latest_rows()
                .map_err(BigQuerySessionStoreRpcError::Store)?;
            Ok(serde_json::json!({
                "operation": "read_latest_rows",
                "rows": rows,
            }))
        }
        BigQuerySessionStoreOperation::ReadLive => {
            let rows = store
                .read_live_rows()
                .map_err(BigQuerySessionStoreRpcError::Store)?;
            Ok(serde_json::json!({
                "operation": "read_live_rows",
                "rows": rows,
            }))
        }
    }
}

fn format_bigquery_store_error(error: &BigQuerySessionStoreError) -> String {
    match error {
        BigQuerySessionStoreError::Io(reason)
        | BigQuerySessionStoreError::Serialize(reason)
        | BigQuerySessionStoreError::Configuration(reason)
        | BigQuerySessionStoreError::Http(reason)
        | BigQuerySessionStoreError::Api(reason)
        | BigQuerySessionStoreError::InvalidQueryResponse(reason) => reason.clone(),
        BigQuerySessionStoreError::ProcessFailed { command, stderr } => {
            format!("command '{command}' failed: {stderr}")
        }
    }
}

fn parse_routing_resolve_params(
    params: &Value,
) -> Result<RoutingResolveRequest, RoutingDeliveryParamsError> {
    let object = params
        .as_object()
        .ok_or(RoutingDeliveryParamsError::ParamsMustBeObject)?;
    let recipient = object
        .get("recipient")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(RoutingDeliveryParamsError::RecipientRequired)?;
    let channel = match object.get("channel") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(RoutingDeliveryParamsError::ChannelMustBeString)?
                .to_string(),
        ),
    };
    let retry_max = match object.get("retry_max") {
        None => None,
        Some(value) => {
            let raw = value
                .as_u64()
                .ok_or(RoutingDeliveryParamsError::RetryMaxMustBeInteger)?;
            let retry_max =
                u32::try_from(raw).map_err(|_| RoutingDeliveryParamsError::RetryMaxOverflow)?;
            if retry_max > ROUTING_RETRY_MAX_CAP {
                return Err(RoutingDeliveryParamsError::RetryMaxAboveCap {
                    cap: ROUTING_RETRY_MAX_CAP,
                });
            }
            Some(retry_max)
        }
    };
    let backoff_ms = match object.get("backoff_ms") {
        None => None,
        Some(value) => Some(
            value
                .as_u64()
                .ok_or(RoutingDeliveryParamsError::BackoffMsMustBeInteger)?,
        ),
    };
    let rate_limit_per_minute = match object.get("rate_limit_per_minute") {
        None => None,
        Some(value) => {
            let raw = value
                .as_u64()
                .ok_or(RoutingDeliveryParamsError::RateLimitMustBeInteger)?;
            let rate_limit_per_minute =
                u32::try_from(raw).map_err(|_| RoutingDeliveryParamsError::RateLimitOverflow)?;
            if rate_limit_per_minute == 0 {
                return Err(RoutingDeliveryParamsError::RateLimitMustBePositive);
            }
            Some(rate_limit_per_minute)
        }
    };

    Ok(RoutingResolveRequest {
        recipient: recipient.to_string(),
        channel,
        retry_max,
        backoff_ms,
        rate_limit_per_minute,
    })
}

fn parse_routing_routes_list_params(params: &Value) -> Result<(), RoutingDeliveryParamsError> {
    if params.is_null() || params.is_object() {
        return Ok(());
    }
    Err(RoutingDeliveryParamsError::ParamsMustBeObject)
}

fn parse_routing_route_add_params(
    params: &Value,
) -> Result<RuntimeRoute, RoutingDeliveryParamsError> {
    let object = params
        .as_object()
        .ok_or(RoutingDeliveryParamsError::ParamsMustBeObject)?;
    let route_value = object.get("route").unwrap_or(params);
    let route_object = route_value
        .as_object()
        .ok_or(RoutingDeliveryParamsError::RouteFieldMustBeObject)?;

    let route_key = route_object
        .get("route_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(RoutingDeliveryParamsError::RouteKeyRequired)?
        .to_string();
    let recipient = route_object
        .get("recipient")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(RoutingDeliveryParamsError::RecipientRequired)?
        .to_string();
    let channel = match route_object.get("channel") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(RoutingDeliveryParamsError::ChannelMustBeString)?
                .to_string(),
        ),
    };
    let sink = route_object
        .get("sink")
        .and_then(Value::as_str)
        .ok_or(RoutingDeliveryParamsError::SinkMustBeString)?
        .to_string();
    let target_module = route_object
        .get("target_module")
        .and_then(Value::as_str)
        .ok_or(RoutingDeliveryParamsError::TargetModuleMustBeString)?
        .to_string();
    let retry_max = match route_object.get("retry_max") {
        None => None,
        Some(value) => {
            let raw = value
                .as_u64()
                .ok_or(RoutingDeliveryParamsError::RetryMaxMustBeInteger)?;
            let retry_max =
                u32::try_from(raw).map_err(|_| RoutingDeliveryParamsError::RetryMaxOverflow)?;
            if retry_max > ROUTING_RETRY_MAX_CAP {
                return Err(RoutingDeliveryParamsError::RetryMaxAboveCap {
                    cap: ROUTING_RETRY_MAX_CAP,
                });
            }
            Some(retry_max)
        }
    };
    let backoff_ms = match route_object.get("backoff_ms") {
        None => None,
        Some(value) => Some(
            value
                .as_u64()
                .ok_or(RoutingDeliveryParamsError::BackoffMsMustBeInteger)?,
        ),
    };
    let rate_limit_per_minute = match route_object.get("rate_limit_per_minute") {
        None => None,
        Some(value) => {
            let raw = value
                .as_u64()
                .ok_or(RoutingDeliveryParamsError::RateLimitMustBeInteger)?;
            let rate_limit_per_minute =
                u32::try_from(raw).map_err(|_| RoutingDeliveryParamsError::RateLimitOverflow)?;
            if rate_limit_per_minute == 0 {
                return Err(RoutingDeliveryParamsError::RateLimitMustBePositive);
            }
            Some(rate_limit_per_minute)
        }
    };

    Ok(RuntimeRoute {
        route_key,
        recipient,
        channel,
        sink,
        target_module,
        retry_max,
        backoff_ms,
        rate_limit_per_minute,
    })
}

fn parse_routing_route_delete_params(params: &Value) -> Result<String, RoutingDeliveryParamsError> {
    let object = params
        .as_object()
        .ok_or(RoutingDeliveryParamsError::ParamsMustBeObject)?;
    let route_key = object
        .get("route_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(RoutingDeliveryParamsError::RouteKeyRequired)?;
    Ok(route_key.to_string())
}

fn parse_delivery_send_params(
    params: &Value,
) -> Result<DeliverySendRequest, RoutingDeliveryParamsError> {
    let object = params
        .as_object()
        .ok_or(RoutingDeliveryParamsError::ParamsMustBeObject)?;
    let resolution = object
        .get("resolution")
        .cloned()
        .ok_or(RoutingDeliveryParamsError::ResolutionRequired)?;
    let resolution = serde_json::from_value(resolution)
        .map_err(|_| RoutingDeliveryParamsError::ResolutionRequired)?;
    let payload = object
        .get("payload")
        .cloned()
        .ok_or(RoutingDeliveryParamsError::PayloadRequired)?;
    let idempotency_key = match object.get("idempotency_key") {
        None => None,
        Some(value) => {
            let key = value
                .as_str()
                .ok_or(RoutingDeliveryParamsError::IdempotencyKeyMustBeString)?
                .trim()
                .to_string();
            if key.is_empty() {
                return Err(RoutingDeliveryParamsError::IdempotencyKeyMustBeString);
            }
            Some(key)
        }
    };

    Ok(DeliverySendRequest {
        resolution,
        payload,
        idempotency_key,
    })
}

fn parse_delivery_history_params(
    params: &Value,
) -> Result<DeliveryHistoryRequest, RoutingDeliveryParamsError> {
    if params.is_null() {
        return Ok(DeliveryHistoryRequest {
            recipient: None,
            sink: None,
            limit: 20,
        });
    }

    let object = params
        .as_object()
        .ok_or(RoutingDeliveryParamsError::ParamsMustBeObject)?;
    let recipient = match object.get("recipient") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(RoutingDeliveryParamsError::HistoryRecipientMustBeString)?
                .to_string(),
        ),
    };
    let sink = match object.get("sink") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(RoutingDeliveryParamsError::HistorySinkMustBeString)?
                .to_string(),
        ),
    };
    let limit = match object.get("limit") {
        None => 20,
        Some(value) => {
            let Some(limit) = value.as_u64() else {
                return Err(RoutingDeliveryParamsError::HistoryLimitOutOfRange);
            };
            if !(1..=200).contains(&limit) {
                return Err(RoutingDeliveryParamsError::HistoryLimitOutOfRange);
            }
            limit as usize
        }
    };

    Ok(DeliveryHistoryRequest {
        recipient,
        sink,
        limit,
    })
}

fn parse_subscribe_request(params: &Value) -> Result<SubscribeRequest, SubscribeParamsError> {
    if params.is_null() {
        return Ok(SubscribeRequest::default());
    }

    let object = params
        .as_object()
        .ok_or(SubscribeParamsError::ParamsMustBeObject)?;

    let scope = match object.get("scope") {
        None => SubscribeScope::Mob,
        Some(value) => {
            let scope = value
                .as_str()
                .ok_or(SubscribeParamsError::ScopeMustBeString)?;
            match scope {
                "mob" => SubscribeScope::Mob,
                "agent" => SubscribeScope::Agent,
                "interaction" => SubscribeScope::Interaction,
                other => {
                    return Err(SubscribeParamsError::UnsupportedScope(other.to_string()));
                }
            }
        }
    };

    let last_event_id = match object.get("last_event_id") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(SubscribeParamsError::LastEventIdMustBeString)?
                .to_string(),
        ),
    };
    let agent_id = match object.get("agent_id") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(SubscribeParamsError::AgentIdMustBeString)?
                .to_string(),
        ),
    };

    Ok(SubscribeRequest {
        scope,
        last_event_id,
        agent_id,
    })
}

fn parse_gating_evaluate_params(
    params: &Value,
) -> Result<GatingEvaluateRequest, GatingParamsError> {
    let object = params
        .as_object()
        .ok_or(GatingParamsError::ParamsMustBeObject)?;
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(GatingParamsError::ActionRequired)?;
    let actor_id = object
        .get("actor_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(GatingParamsError::ActorRequired)?;
    let risk_tier = object
        .get("risk_tier")
        .and_then(Value::as_str)
        .ok_or(GatingParamsError::RiskTierRequired)?;
    let risk_tier = parse_gating_risk_tier(risk_tier)?;
    let rationale = match object.get("rationale") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::RationaleMustBeString)?
                .to_string(),
        ),
    };
    let requested_approver = match object.get("requested_approver") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::RequestedApproverMustBeString)?
                .to_string(),
        ),
    };
    let approval_recipient = match object.get("approval_recipient") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::ApprovalRecipientMustBeString)?
                .to_string(),
        ),
    };
    let approval_channel = match object.get("approval_channel") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::ApprovalChannelMustBeString)?
                .to_string(),
        ),
    };
    let approval_timeout_ms = match object.get("approval_timeout_ms") {
        None => None,
        Some(value) => Some(
            value
                .as_u64()
                .ok_or(GatingParamsError::ApprovalTimeoutMustBeInteger)?,
        ),
    };
    let entity = match object.get("entity") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::EntityMustBeString)?
                .to_string(),
        ),
    };
    let topic = match object.get("topic") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::TopicMustBeString)?
                .to_string(),
        ),
    };

    Ok(GatingEvaluateRequest {
        action: action.to_string(),
        actor_id: actor_id.to_string(),
        risk_tier,
        rationale,
        requested_approver,
        approval_recipient,
        approval_channel,
        approval_timeout_ms,
        entity,
        topic,
    })
}

fn parse_memory_stores_params(params: &Value) -> Result<(), MemoryParamsError> {
    if params.is_null() || params.is_object() {
        return Ok(());
    }
    Err(MemoryParamsError::ParamsMustBeObject)
}

fn parse_memory_store_field(value: &Value) -> Result<String, MemoryParamsError> {
    let store = value.as_str().ok_or(MemoryParamsError::StoreMustBeString)?;
    let canonical = store.trim().to_ascii_lowercase();
    if canonical.is_empty() {
        return Err(MemoryParamsError::StoreMustBeString);
    }
    if MEMORY_SUPPORTED_STORES.contains(&canonical.as_str()) {
        Ok(canonical)
    } else {
        Err(MemoryParamsError::UnsupportedStore(canonical))
    }
}

fn parse_memory_index_params(params: &Value) -> Result<MemoryIndexRequest, MemoryParamsError> {
    let object = params
        .as_object()
        .ok_or(MemoryParamsError::ParamsMustBeObject)?;
    let entity = object
        .get("entity")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(MemoryParamsError::EntityRequired)?;
    let topic = object
        .get("topic")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(MemoryParamsError::TopicRequired)?;
    let store = match object.get("store") {
        None => None,
        Some(value) => Some(parse_memory_store_field(value)?),
    };
    let fact = match object.get("fact") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(MemoryParamsError::FactMustBeString)?
                .trim()
                .to_string(),
        ),
    };
    if fact.as_deref().is_some_and(str::is_empty) {
        return Err(MemoryParamsError::FactMustBeString);
    }
    let metadata = match object.get("metadata") {
        None => None,
        Some(value) => {
            if !value.is_object() {
                return Err(MemoryParamsError::MetadataMustBeJson);
            }
            Some(value.clone())
        }
    };
    let conflict = match object.get("conflict") {
        None => None,
        Some(value) => Some(
            value
                .as_bool()
                .ok_or(MemoryParamsError::ConflictMustBeBoolean)?,
        ),
    };
    let conflict_reason = match object.get("conflict_reason") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(MemoryParamsError::ConflictReasonMustBeString)?
                .to_string(),
        ),
    };

    Ok(MemoryIndexRequest {
        entity: entity.to_string(),
        topic: topic.to_string(),
        store,
        fact,
        metadata,
        conflict,
        conflict_reason,
    })
}

fn parse_memory_query_params(params: &Value) -> Result<MemoryQueryRequest, MemoryParamsError> {
    if params.is_null() {
        return Ok(MemoryQueryRequest {
            entity: None,
            topic: None,
            store: None,
        });
    }
    let object = params
        .as_object()
        .ok_or(MemoryParamsError::ParamsMustBeObject)?;
    let entity = match object.get("entity") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(MemoryParamsError::EntityMustBeString)?
                .to_string(),
        ),
    };
    let topic = match object.get("topic") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(MemoryParamsError::TopicMustBeString)?
                .to_string(),
        ),
    };
    let store = match object.get("store") {
        None => None,
        Some(value) => Some(parse_memory_store_field(value)?),
    };
    Ok(MemoryQueryRequest {
        entity,
        topic,
        store,
    })
}

fn parse_gating_pending_params(params: &Value) -> Result<(), GatingParamsError> {
    if params.is_null() || params.is_object() {
        return Ok(());
    }
    Err(GatingParamsError::ParamsMustBeObject)
}

fn parse_gating_decide_params(params: &Value) -> Result<GatingDecideRequest, GatingParamsError> {
    let object = params
        .as_object()
        .ok_or(GatingParamsError::ParamsMustBeObject)?;
    let pending_id = object
        .get("pending_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(GatingParamsError::PendingIdRequired)?;
    let approver_id = object
        .get("approver_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(GatingParamsError::ApproverIdRequired)?;
    let decision = object
        .get("decision")
        .and_then(Value::as_str)
        .ok_or(GatingParamsError::DecisionRequired)?;
    let decision = parse_gating_decision(decision)?;
    let reason = match object.get("reason") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::ReasonMustBeString)?
                .to_string(),
        ),
    };

    Ok(GatingDecideRequest {
        pending_id: pending_id.to_string(),
        approver_id: approver_id.to_string(),
        decision,
        reason,
    })
}

fn parse_gating_audit_params(params: &Value) -> Result<usize, GatingParamsError> {
    if params.is_null() {
        return Ok(50);
    }
    let object = params
        .as_object()
        .ok_or(GatingParamsError::ParamsMustBeObject)?;
    let limit = match object.get("limit") {
        None => 50,
        Some(value) => {
            let Some(raw_limit) = value.as_u64() else {
                return Err(GatingParamsError::LimitOutOfRange);
            };
            if !(1..=500).contains(&raw_limit) {
                return Err(GatingParamsError::LimitOutOfRange);
            }
            raw_limit as usize
        }
    };
    Ok(limit)
}

fn parse_gating_risk_tier(raw: &str) -> Result<GatingRiskTier, GatingParamsError> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "r0" => Ok(GatingRiskTier::R0),
        "r1" => Ok(GatingRiskTier::R1),
        "r2" => Ok(GatingRiskTier::R2),
        "r3" => Ok(GatingRiskTier::R3),
        "" => Err(GatingParamsError::RiskTierRequired),
        _ => Err(GatingParamsError::UnknownRiskTier(raw.to_string())),
    }
}

fn parse_gating_decision(raw: &str) -> Result<GatingDecision, GatingParamsError> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "approve" => Ok(GatingDecision::Approve),
        "reject" => Ok(GatingDecision::Reject),
        "" => Err(GatingParamsError::DecisionRequired),
        _ => Err(GatingParamsError::UnknownDecision(raw.to_string())),
    }
}

fn parse_scheduling_params(params: &Value) -> Result<(Vec<ScheduleDefinition>, u64), String> {
    let object = params
        .as_object()
        .ok_or_else(|| "scheduling params must be a JSON object".to_string())?;
    let tick_ms = object
        .get("tick_ms")
        .and_then(Value::as_u64)
        .ok_or_else(|| "tick_ms must be a u64".to_string())?;
    if tick_ms > i64::MAX as u64 {
        return Err(format!("tick_ms must be <= {}", i64::MAX));
    }
    let schedules = object
        .get("schedules")
        .and_then(Value::as_array)
        .ok_or_else(|| "schedules must be an array".to_string())?;
    if schedules.len() > MAX_SCHEDULES_PER_REQUEST {
        return Err(format!(
            "schedules must contain at most {MAX_SCHEDULES_PER_REQUEST} entries"
        ));
    }

    let mut parsed = Vec::with_capacity(schedules.len());
    for schedule in schedules {
        let entry = schedule
            .as_object()
            .ok_or_else(|| "each schedule must be an object".to_string())?;
        let schedule_id = entry
            .get("schedule_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "schedule_id must be a non-empty string".to_string())?;
        let interval = entry
            .get("interval")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "interval must be a non-empty string".to_string())?;
        let timezone = entry
            .get("timezone")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "timezone must be a non-empty string".to_string())?;
        let jitter_ms = entry
            .get("jitter_ms")
            .map(|value| {
                value
                    .as_u64()
                    .ok_or_else(|| "jitter_ms must be a u64".to_string())
            })
            .transpose()?
            .unwrap_or(0);
        let catch_up = entry
            .get("catch_up")
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| "catch_up must be a boolean".to_string())
            })
            .transpose()?
            .unwrap_or(false);
        let enabled = entry
            .get("enabled")
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| "enabled must be a boolean".to_string())
            })
            .transpose()?
            .unwrap_or(true);
        parsed.push(ScheduleDefinition {
            schedule_id: schedule_id.trim().to_string(),
            interval: interval.to_string(),
            timezone: timezone.to_string(),
            enabled,
            jitter_ms,
            catch_up,
        });
    }

    validate_schedules(&parsed).map_err(format_schedule_validation_error)?;

    Ok((parsed, tick_ms))
}

fn format_schedule_validation_error(err: ScheduleValidationError) -> String {
    match err {
        ScheduleValidationError::EmptyScheduleId => {
            "schedule_id must be a non-empty string".to_string()
        }
        ScheduleValidationError::DuplicateScheduleId(schedule_id) => {
            format!("duplicate schedule_id '{schedule_id}' is not allowed")
        }
        ScheduleValidationError::InvalidTickMs(tick_ms) => {
            format!(
                "tick_ms '{tick_ms}' is unsupported (must be <= {})",
                i64::MAX
            )
        }
        ScheduleValidationError::InvalidInterval {
            schedule_id,
            interval,
        } => format!("invalid interval '{interval}' for schedule_id '{schedule_id}'"),
        ScheduleValidationError::InvalidTimezone {
            schedule_id,
            timezone,
        } => format!("invalid timezone '{timezone}' for schedule_id '{schedule_id}'"),
    }
}

fn serialize_response(response: &JsonRpcResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
            .to_string()
    })
}
