//! JSON-RPC request handling for both module-only and unified runtime modes.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// JSON-RPC error code returned by `mobkit/mob_events/{query,subscribe}`
/// when the caller's `after_seq` is past the current ledger frontier.
/// The error `data` field carries `{ after_cursor, latest_cursor }` so
/// SDKs can surface a typed exception. Single source of truth — keep
/// this in sync with `MobEventsStaleError` in the Python and TypeScript
/// SDKs.
pub const MOB_EVENTS_STALE_CURSOR_CODE: i64 = -32010;

/// JSON-RPC error code returned by `mobkit/memory/index` and
/// `mobkit/memory/query` when the configured memory backend cannot
/// persist or retrieve the row. Distinct from
/// [`MOB_EVENTS_STALE_CURSOR_CODE`] so SDKs can branch on `-32010`
/// without misclassifying a memory backend failure as a stale-cursor
/// event.
pub const MEMORY_BACKEND_UNAVAILABLE_CODE: i64 = -32012;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    /// Optional structured payload as defined by JSON-RPC 2.0. Used by
    /// typed errors (e.g. `event_query_stale` with `after_cursor` /
    /// `latest_cursor`) so SDKs can surface a typed exception. Existing
    /// construction sites can omit it via `..Default::default()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
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
                    data: None,
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
                    data: None,
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
                data: None,
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
                            data: None,
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
                        data: None,
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
                        data: None,
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
                            data: None,
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
                        data: None,
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
                    data: None,
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
                        data: None,
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
                    data: None,
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
                        data: None,
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
                    data: None,
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
                    data: None,
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
                    data: None,
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
                        data: None,
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
                    data: None,
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
                        data: None,
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
                    data: None,
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
                        code: MEMORY_BACKEND_UNAVAILABLE_CODE,
                        message: format!(
                            "Memory backend unavailable: {}",
                            MemoryParamsError::backend_message(&error)
                        ),
                        data: None,
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
                        data: None,
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
                    data: None,
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
                    data: None,
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
                        data: None,
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
                        data: None,
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
                    data: None,
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
                    data: None,
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
                        data: None,
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
                    data: None,
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
                                data: None,
                            }),
                        },
                        Err(err) => JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32000,
                                message: format!("Tool call failed: {err:?}"),
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
                        message: "Invalid params: module_id and tool required".to_string(),
                        data: None,
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
                        data: None,
                    }),
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("Module route failed: {err:?}"),
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
                code: -32601,
                message: "Method not found".to_string(),
                data: None,
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
                    data: None,
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
                    data: None,
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
                data: None,
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
                "mobkit/blob/get",
                "mobkit/send_message",
                "mobkit/find_members",
                "mobkit/ensure_member",
                "mobkit/list_members",
                "mobkit/get_member",
                "mobkit/retire_member",
                "mobkit/respawn_member",
                "mobkit/reconcile_edges",
                "mobkit/rediscover",
                "mobkit/mob_events/query",
                "mobkit/mob_events/subscribe",
                // Always available: local-only member introspection
                "mobkit/cross_mob/peer_info",
                "mobkit/cross_mob/wire_local",
                "mobkit/cross_mob/unwire_local",
                "mobkit/peer_pubkey",
                "mobkit/member_status",
                "mobkit/force_cancel_member",
                "mobkit/spawn_helper",
                "mobkit/fork_helper",
                "mobkit/attach_existing_session",
                "mobkit/cancel_flow",
                "mobkit/flow_status",
                "mobkit/list_flows",
                "mobkit/list_runs",
                "mobkit/run_flow",
                "mobkit/collect_completed",
                "mobkit/wait_ready",
                "mobkit/mob_labels/set",
                "mobkit/mob_labels/get",
                "mobkit/mob_labels/delete",
                "mobkit/run_labels/set",
                "mobkit/run_labels/get",
                "mobkit/run_labels/delete",
            ];
            if identity_ctx.is_some() {
                methods.extend_from_slice(&[
                    "mobkit/send",
                    "mobkit/dispatch",
                    "mobkit/subscribe",
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
                            data: None,
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
                        data: None,
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
                            data: None,
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
                                data: None,
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
                            data: None,
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
                        data: None,
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
                            data: None,
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
                    data: None,
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
                            data: None,
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
                    data: None,
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
                        data: None,
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
                    data: None,
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
                        data: None,
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
                        data: None,
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
                        data: None,
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
                    data: None,
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
                        data: None,
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
                    data: None,
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
                    data: None,
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
                        code: MEMORY_BACKEND_UNAVAILABLE_CODE,
                        message: format!(
                            "Memory backend unavailable: {}",
                            MemoryParamsError::backend_message(&error)
                        ),
                        data: None,
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
                        data: None,
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
                    data: None,
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
                    data: None,
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
                        data: None,
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
                        data: None,
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
                    data: None,
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
                    data: None,
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
                        data: None,
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
                    data: None,
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
                                data: None,
                            }),
                        },
                        Err(err) => JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32000,
                                message: format!("Tool call failed: {err:?}"),
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
                        message: "Invalid params: module_id and tool required".to_string(),
                        data: None,
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
        "mobkit/blob/get" => {
            mob_methods::handle_blob_get(runtime, response_id, &request.params).await
        }
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
        "mobkit/mob_events/query" => {
            mob_methods::handle_mob_events_query(runtime, response_id, request.params).await
        }
        "mobkit/mob_events/subscribe" => {
            mob_methods::handle_mob_events_subscribe(runtime, response_id, request.params).await
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
        "mobkit/peer_pubkey" => mob_methods::handle_peer_pubkey(runtime, response_id).await,
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
        "mobkit/list_flows" => mob_methods::handle_list_flows(runtime, response_id).await,
        "mobkit/list_runs" => {
            mob_methods::handle_list_runs(runtime, response_id, &request.params).await
        }
        "mobkit/run_flow" => {
            mob_methods::handle_run_flow(runtime, response_id, &request.params).await
        }
        "mobkit/collect_completed" => {
            mob_methods::handle_collect_completed(runtime, response_id).await
        }
        "mobkit/wait_ready" => {
            mob_methods::handle_wait_ready(runtime, response_id, &request.params).await
        }
        "mobkit/mob_labels/set" => {
            mob_methods::handle_mob_labels_set(runtime, response_id, &request.params).await
        }
        "mobkit/mob_labels/get" => mob_methods::handle_mob_labels_get(runtime, response_id).await,
        "mobkit/mob_labels/delete" => {
            mob_methods::handle_mob_labels_delete(runtime, response_id).await
        }
        "mobkit/run_labels/set" => {
            mob_methods::handle_run_labels_set(runtime, response_id, &request.params).await
        }
        "mobkit/run_labels/get" => {
            mob_methods::handle_run_labels_get(runtime, response_id, &request.params).await
        }
        "mobkit/run_labels/delete" => {
            mob_methods::handle_run_labels_delete(runtime, response_id, &request.params).await
        }
        // ----- identity-first methods -----
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
            let content = match serde_json::from_value::<meerkat_core::ContentInput>(content_val) {
                Ok(content) => content,
                Err(err) => {
                    return error_response(response_id, -32602, format!("invalid content: {err}"));
                }
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
            let content_val = di_val
                .get("content")
                .cloned()
                .unwrap_or_else(|| Value::String(String::new()));
            let content = match serde_json::from_value::<meerkat_core::ContentInput>(content_val) {
                Ok(content) => content,
                Err(err) => {
                    return error_response(
                        response_id,
                        -32602,
                        format!("invalid dispatch_input.content: {err}"),
                    );
                }
            };
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
                content,
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
        "mobkit/subscribe" => {
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
            match identity_rt.subscribe(&identity).await {
                Ok(_receiver) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "identity": identity.as_str(),
                        "stream_id": identity.as_str(),
                        "subscribed": true,
                    })),
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
                        data: None,
                    }),
                },
                Err(err) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: format!("Module route failed: {err:?}"),
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
                code: -32601,
                message: "Method not found".to_string(),
                data: None,
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
            if let Some(provider) = meerkat_core::Provider::parse_strict(e.provider)
                && let Some(profile) = meerkat_models::profile_for(provider, e.id)
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
        error: Some(JsonRpcError {
            code,
            message,
            data: None,
        }),
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
            data: None,
        }),
    })
}

fn serialize_response(response: &JsonRpcResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
            .to_string()
    })
}
