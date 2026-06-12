//! JSON-RPC request handling for both module-only and unified runtime modes.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::console_aggregator::is_implicit_delegate_member;
use crate::mob_handle_runtime::{topology_restore_failed_peer_ids, topology_restore_warning_json};
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
use crate::unified_runtime::{EventQuery, UnifiedRuntime};

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
pub const MOBKIT_CONTRACT_VERSION: &str = "0.4.0";
pub const MAX_SCHEDULES_PER_REQUEST: usize = 256;
pub(crate) const MOBPACK_AUTHORING_METHODS: &[&str] = &[
    "mobkit/mobpacks/schema",
    "mobkit/mobpacks/catalogs",
    "mobkit/tools/catalog",
    "mobkit/skills/catalog",
    "mobkit/agent_definitions/list",
    "mobkit/mobpacks/templates",
    "mobkit/mobpacks/validate",
    "mobkit/mobpacks/source",
    "mobkit/mobpacks/export",
    "mobkit/mobpacks/import",
    "mobkit/mobpacks/list",
    "mobkit/mobpacks/get",
    "mobkit/mobpacks/create",
    "mobkit/mobpacks/save",
    "mobkit/mobpacks/delete",
    "mobkit/mobpacks/undo",
    "mobkit/mobpacks/redo",
    "mobkit/mobpacks/apply_operation",
    "mobkit/mobpacks/graph_projection",
    "mobkit/mobpacks/graph_to_flow",
    "mobkit/mobpacks/deploy_command",
    "mobkit/mobpacks/deploy",
];

pub(crate) fn mobpack_authoring_capabilities() -> Value {
    serde_json::json!({
        "domain": "mobpack_authoring",
        "runtime_mutation": false,
        "host_mutation_methods": {
            "mobkit/mobpacks/deploy": "when execute=true, writes a mobpack archive and runs rkat mob run on the host"
        },
        "deploy_command": "rkat mob run",
        "methods": MOBPACK_AUTHORING_METHODS,
        "operations": crate::mobpack::mobpack_authoring_operations(),
    })
}

async fn mobpack_runtime_catalog_state(
    runtime: &UnifiedRuntime,
) -> crate::mobpack::MobpackRuntimeCatalogState {
    let loaded_modules = runtime.loaded_modules().await;
    let runtime_flow_rows = crate::mobpack::runtime_flow_registry_rows_from_definition(
        runtime.mob_handle().definition(),
    );
    let runtime_agent_definition_sources =
        crate::mobpack::runtime_agent_definition_sources_from_definition(
            runtime.mob_handle().definition(),
        );
    let runtime_skill_realms =
        crate::mobpack::runtime_skill_realms_from_definition(runtime.mob_handle().definition());
    let mut runtime_methods = vec![
        "mobkit/capabilities".to_string(),
        "mobkit/models/catalog".to_string(),
        "mobkit/spawn_member".to_string(),
        "mobkit/list_members".to_string(),
        "mobkit/get_member".to_string(),
        "mobkit/run_flow".to_string(),
        "mobkit/list_flows".to_string(),
        "mobkit/list_runs".to_string(),
    ];
    runtime_methods.extend(
        MOBPACK_AUTHORING_METHODS
            .iter()
            .map(std::string::ToString::to_string),
    );
    if runtime.has_contact_directory() {
        runtime_methods.push("mobkit/cross_mob/directory".to_string());
    }
    if runtime.has_peer_mob_handles().await && runtime.has_inproc_contacts() {
        runtime_methods.extend([
            "mobkit/cross_mob/wire".to_string(),
            "mobkit/cross_mob/unwire".to_string(),
            "mobkit/cross_mob/send".to_string(),
        ]);
    }
    crate::mobpack::MobpackRuntimeCatalogState {
        loaded_modules,
        runtime_methods,
        has_contact_directory: runtime.has_contact_directory(),
        has_peer_mob_handles: runtime.has_peer_mob_handles().await,
        has_inproc_contacts: runtime.has_inproc_contacts(),
        runtime_flow_rows,
        runtime_agent_definition_sources,
        runtime_skill_realms,
    }
}

async fn handle_unified_mobpack_authoring_rpc(
    runtime: &UnifiedRuntime,
    method: &str,
    params: &Value,
    response_id: Value,
) -> JsonRpcResponse {
    let runtime_catalog_state = match method {
        "mobkit/mobpacks/schema"
        | "mobkit/mobpacks/catalogs"
        | "mobkit/tools/catalog"
        | "mobkit/skills/catalog"
        | "mobkit/agent_definitions/list"
        | "mobkit/mobpacks/templates"
        | "mobkit/mobpacks/list"
        | "mobkit/mobpacks/get"
        | "mobkit/mobpacks/apply_operation" => Some(mobpack_runtime_catalog_state(runtime).await),
        _ => None,
    };
    let result = match method {
        "mobkit/mobpacks/catalogs" => Ok(crate::mobpack::mobpack_catalogs_response_with_runtime(
            runtime_catalog_state.as_ref(),
        )),
        "mobkit/tools/catalog" => Ok(crate::mobpack::mobpack_tools_catalog_response_with_runtime(
            runtime_catalog_state.as_ref(),
        )),
        "mobkit/skills/catalog" => Ok(
            crate::mobpack::mobpack_skills_catalog_response_with_runtime(
                runtime_catalog_state.as_ref(),
            ),
        ),
        "mobkit/agent_definitions/list" => Ok(
            crate::mobpack::mobpack_agent_definitions_response_with_runtime(
                runtime_catalog_state.as_ref(),
            ),
        ),
        "mobkit/mobpacks/templates" => Ok(crate::mobpack::mobpack_templates_response_with_runtime(
            runtime_catalog_state.as_ref(),
        )),
        _ => {
            return handle_mobpack_authoring_rpc_with_runtime(
                method,
                params,
                response_id.clone(),
                runtime_catalog_state.as_ref(),
            )
            .unwrap_or_else(|| JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: "Method not found".to_string(),
                    data: None,
                }),
            });
        }
    };
    match result {
        Ok(result) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(result),
            error: None,
        },
        Err(message) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message,
                data: None,
            }),
        },
    }
}

pub(crate) fn handle_mobpack_authoring_rpc(
    method: &str,
    params: &Value,
    response_id: Value,
) -> Option<JsonRpcResponse> {
    handle_mobpack_authoring_rpc_with_runtime(method, params, response_id, None)
}

pub(crate) fn handle_mobpack_authoring_rpc_with_runtime(
    method: &str,
    params: &Value,
    response_id: Value,
    runtime: Option<&crate::mobpack::MobpackRuntimeCatalogState>,
) -> Option<JsonRpcResponse> {
    let result = match method {
        "mobkit/mobpacks/schema" => Ok(crate::mobpack::mobpack_schema_response_with_runtime(
            runtime,
        )),
        "mobkit/mobpacks/catalogs" => Ok(crate::mobpack::mobpack_catalogs_response_with_runtime(
            runtime,
        )),
        "mobkit/tools/catalog" => Ok(crate::mobpack::mobpack_tools_catalog_response_with_runtime(
            runtime,
        )),
        "mobkit/skills/catalog" => {
            Ok(crate::mobpack::mobpack_skills_catalog_response_with_runtime(runtime))
        }
        "mobkit/agent_definitions/list" => {
            Ok(crate::mobpack::mobpack_agent_definitions_response_with_runtime(runtime))
        }
        "mobkit/mobpacks/templates" => Ok(crate::mobpack::mobpack_templates_response_with_runtime(
            runtime,
        )),
        "mobkit/mobpacks/validate" => crate::mobpack::validate_mobpack(params)
            .and_then(|result| serde_json::to_value(result).map_err(|err| err.to_string())),
        "mobkit/mobpacks/source" => crate::mobpack::source_mobpack(params)
            .and_then(|result| serde_json::to_value(result).map_err(|err| err.to_string())),
        "mobkit/mobpacks/export" => crate::mobpack::export_mobpack(params)
            .and_then(|result| serde_json::to_value(result).map_err(|err| err.to_string())),
        "mobkit/mobpacks/import" => crate::mobpack::import_mobpack(params),
        "mobkit/mobpacks/list" => crate::mobpack::list_mobpack_drafts_with_runtime(params, runtime),
        "mobkit/mobpacks/get" => crate::mobpack::get_mobpack_draft_with_runtime(params, runtime),
        "mobkit/mobpacks/create" => crate::mobpack::create_mobpack_draft(params),
        "mobkit/mobpacks/save" => crate::mobpack::save_mobpack_draft(params),
        "mobkit/mobpacks/delete" => crate::mobpack::delete_mobpack_draft(params),
        "mobkit/mobpacks/undo" => crate::mobpack::undo_mobpack_draft(params),
        "mobkit/mobpacks/redo" => crate::mobpack::redo_mobpack_draft(params),
        "mobkit/mobpacks/apply_operation" => {
            crate::mobpack::apply_mobpack_authoring_operation_with_runtime(params, runtime)
        }
        "mobkit/mobpacks/graph_projection" => crate::mobpack::graph_projection_mobpack(params),
        "mobkit/mobpacks/graph_to_flow" => crate::mobpack::graph_to_flow_mobpack(params),
        "mobkit/mobpacks/deploy_command" => crate::mobpack::deploy_command_preview(params)
            .and_then(|result| serde_json::to_value(result).map_err(|err| err.to_string())),
        "mobkit/mobpacks/deploy" => crate::mobpack::deploy_mobpack(params)
            .and_then(|result| serde_json::to_value(result).map_err(|err| err.to_string())),
        _ => return None,
    };
    Some(match result {
        Ok(result) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(result),
            error: None,
        },
        Err(message) => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message,
                data: None,
            }),
        },
    })
}

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

/// JSON-RPC error code returned by `mobkit/console/query_timeline` when
/// the requested console cursor cannot be replayed from the durable console
/// timeline. Distinct from [`MOB_EVENTS_STALE_CURSOR_CODE`] because SDKs
/// reify `-32010` specifically as a mob-events ledger error.
pub const CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE: i64 = -32013;

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
        "mobkit/capabilities" => {
            let mut methods = vec![
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
            ];
            methods.extend_from_slice(MOBPACK_AUTHORING_METHODS);
            JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: response_id,
                result: Some(serde_json::json!({
                    "contract_version": MOBKIT_CONTRACT_VERSION,
                    "methods": methods,
                    "loaded_modules": runtime.loaded_modules(),
                    "runtime_capabilities": {
                        "can_spawn_members": false,
                        "can_send_messages": false,
                        "can_wire_members": false,
                        "can_retire_members": false,
                        "available_spawn_modes": ["module"],
                    },
                    "authoring_capabilities": mobpack_authoring_capabilities(),
                })),
                error: None,
            }
        }
        "mobkit/models/catalog" => JsonRpcResponse {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: response_id,
            result: Some(build_models_catalog_result()),
            error: None,
        },
        method if MOBPACK_AUTHORING_METHODS.contains(&method) => {
            handle_mobpack_authoring_rpc(method, &request.params, response_id.clone())
                .unwrap_or_else(|| JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: "Method not found".to_string(),
                        data: None,
                    }),
                })
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

pub fn handle_unified_rpc_json<'a>(
    runtime: &'a UnifiedRuntime,
    request_json: &'a str,
    timeout: Duration,
    http_base_url: Option<&'a str>,
    identity_ctx: Option<&'a IdentityFirstContext>,
) -> Pin<Box<dyn Future<Output = String> + Send + 'a>> {
    Box::pin(handle_unified_rpc_json_inner(
        runtime,
        request_json,
        timeout,
        http_base_url,
        identity_ctx,
    ))
}

async fn handle_unified_rpc_json_inner(
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
            let mob_state = Some(runtime.mob_handle().status_observation_snapshot());
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
                "mobkit/query_events",
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
            methods.extend_from_slice(MOBPACK_AUTHORING_METHODS);
            if identity_ctx.is_some() {
                methods.extend_from_slice(&[
                    "mobkit/send",
                    "mobkit/interact",
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
                    },
                    "authoring_capabilities": mobpack_authoring_capabilities(),
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
                match Box::pin(runtime.spawn(spec)).await {
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
        "mobkit/query_events" => {
            let query: EventQuery = if request.params.is_null() {
                EventQuery::default()
            } else {
                match serde_json::from_value(request.params.clone()) {
                    Ok(query) => query,
                    Err(err) => {
                        return serde_json::to_string(&JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32602,
                                message: format!("Invalid params: invalid query params: {err}"),
                                data: None,
                            }),
                        })
                        .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string());
                    }
                }
            };
            match runtime.event_log_store() {
                Some(store) => match store.query(query).await {
                    Ok(events) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::to_value(events).unwrap_or(Value::Null)),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32603,
                            message: format!("query_events failed: {err}"),
                            data: None,
                        }),
                    },
                },
                None => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "status": "no_event_log_configured",
                        "events": [],
                    })),
                    error: None,
                },
            }
        }
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
        method if MOBPACK_AUTHORING_METHODS.contains(&method) => {
            handle_unified_mobpack_authoring_rpc(runtime, method, &request.params, response_id)
                .await
        }
        "mobkit/blob/get" => {
            mob_methods::handle_blob_get(runtime, response_id, &request.params).await
        }
        "mobkit/send_message" => {
            // Pass the identity runtime so bare durable identities resolve
            // through the identity bridge when no roster member matches
            // (exact member-id match wins; see `SendMessageTarget`).
            Box::pin(mob_methods::handle_send_message(
                runtime,
                identity_ctx.map(|ctx| &ctx.runtime),
                response_id,
                &request.params,
            ))
            .await
        }
        "mobkit/find_members" => {
            mob_methods::handle_find_members(runtime, response_id, &request.params).await
        }
        "mobkit/ensure_member" => {
            Box::pin(mob_methods::handle_ensure_member(
                runtime,
                response_id,
                &request.params,
            ))
            .await
        }
        "mobkit/list_members" => mob_methods::handle_list_members(runtime, response_id).await,
        "mobkit/get_member" => {
            mob_methods::handle_get_member(runtime, response_id, &request.params).await
        }
        "mobkit/retire_member" => {
            mob_methods::handle_retire_member(runtime, response_id, &request.params).await
        }
        "mobkit/respawn_member" => {
            Box::pin(mob_methods::handle_respawn_member(
                runtime,
                response_id,
                &request.params,
            ))
            .await
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
            Box::pin(mob_methods::handle_cross_mob_wire(
                runtime,
                response_id,
                &request.params,
            ))
            .await
        }
        "mobkit/cross_mob/unwire" => {
            Box::pin(mob_methods::handle_cross_mob_unwire(
                runtime,
                response_id,
                &request.params,
            ))
            .await
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
            Box::pin(mob_methods::handle_spawn_helper(
                runtime,
                response_id,
                &request.params,
            ))
            .await
        }
        "mobkit/fork_helper" => {
            Box::pin(mob_methods::handle_fork_helper(
                runtime,
                response_id,
                &request.params,
            ))
            .await
        }
        "mobkit/attach_existing_session" => {
            Box::pin(mob_methods::handle_attach_existing_session(
                runtime,
                response_id,
                &request.params,
            ))
            .await
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
            Box::pin(mob_methods::handle_run_flow(
                runtime,
                response_id,
                &request.params,
            ))
            .await
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
                None => return maybe_identity_not_configured(is_notification, response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target =
                match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
                {
                    Ok(target) => target,
                    Err(e) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let identity = target.identity.clone();
            if let Some(response) =
                rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
            {
                return if is_notification {
                    String::new()
                } else {
                    serialize_response(&response)
                };
            }
            let content_val = request
                .params
                .get("content")
                .cloned()
                .unwrap_or(Value::Null);
            let content = match serde_json::from_value::<meerkat_core::ContentInput>(content_val) {
                Ok(content) => content,
                Err(err) => {
                    return maybe_error_response(
                        is_notification,
                        response_id,
                        -32602,
                        format!("invalid content: {err}"),
                    );
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
        "mobkit/interact" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return maybe_identity_not_configured(is_notification, response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target =
                match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
                {
                    Ok(target) => target,
                    Err(e) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let identity = target.identity.clone();
            if let Some(response) =
                rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
            {
                return if is_notification {
                    String::new()
                } else {
                    serialize_response(&response)
                };
            }
            let content_val = request
                .params
                .get("content")
                .cloned()
                .unwrap_or(Value::Null);
            let content =
                match serde_json::from_value::<meerkat_core::ContentInput>(content_val.clone()) {
                    Ok(content) => content,
                    Err(err) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid content: {err}"),
                        );
                    }
                };
            let origin = request
                .params
                .get("origin")
                .and_then(|v| v.as_str())
                .unwrap_or("console");
            let interaction_id = request
                .params
                .get("interaction_id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| meerkat_core::types::SessionId::new().to_string());
            let runtime_member_id = identity_rt
                .status(&identity)
                .await
                .ok()
                .and_then(|status| status.agent_runtime_id.map(|id| id.as_str().to_string()));

            if let Err(err) = runtime
                .reserve_identity_interaction(
                    identity.as_str(),
                    runtime_member_id.as_deref(),
                    &interaction_id,
                    origin,
                    content_val,
                )
                .await
            {
                return maybe_error_response(
                    is_notification,
                    response_id,
                    -32003,
                    format!("failed to reserve interaction: {err}"),
                );
            }

            match identity_rt.send(&identity, &content).await {
                Ok(token) => JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: response_id,
                    result: Some(serde_json::json!({
                        "interaction_id": interaction_id,
                        "fencing_token": token.get(),
                        "stream": {
                            "route": format!("/console/identity/{}/stream", identity.as_str()),
                            "identity": identity.as_str(),
                        }
                    })),
                    error: None,
                },
                Err(e) => {
                    runtime
                        .record_console_lifecycle(
                            identity.as_str(),
                            "interaction_failed",
                            serde_json::json!({
                                "interaction_id": interaction_id,
                                "origin": origin,
                                "error": e.to_string(),
                            }),
                        )
                        .await;
                    identity_error_response(response_id, &e)
                }
            }
        }
        "mobkit/dispatch" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return maybe_identity_not_configured(is_notification, response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target =
                match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
                {
                    Ok(target) => target,
                    Err(e) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let identity = target.identity.clone();
            if let Some(response) =
                rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
            {
                return if is_notification {
                    String::new()
                } else {
                    serialize_response(&response)
                };
            }
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
                    return maybe_error_response(
                        is_notification,
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
            let idempotency_key = di_val
                .get("idempotency_key")
                .and_then(|v| v.as_str())
                .map(crate::identity_first::DispatchIdempotencyKey::new);
            let dispatch_input = crate::identity_first::DispatchInput {
                content,
                origin,
                correlation_id,
                idempotency_key,
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
                None => return maybe_identity_not_configured(is_notification, response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target =
                match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
                {
                    Ok(target) => target,
                    Err(e) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let identity = target.identity.clone();
            if let Some(response) =
                rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
            {
                return if is_notification {
                    String::new()
                } else {
                    serialize_response(&response)
                };
            }
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
                None => return maybe_identity_not_configured(is_notification, response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target =
                match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
                {
                    Ok(target) => target,
                    Err(e) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let identity = target.identity.clone();
            if let Some(response) =
                rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
            {
                return if is_notification {
                    String::new()
                } else {
                    serialize_response(&response)
                };
            }
            match identity_rt.status(&identity).await {
                Ok(status) => {
                    let continuity_health =
                        serde_json::to_value(&status.continuity_health).unwrap_or(Value::Null);
                    let result = serde_json::json!({
                        "state": format!("{:?}", status.state),
                        "identity": status.identity.as_str(),
                        "agent_runtime_id": status.agent_runtime_id.as_ref().map(super::identity_first::AgentRuntimeId::as_str),
                        "session_id": status.session_id.as_ref().map(ToString::to_string),
                        "profile": status.profile.as_ref().map(meerkat_mob::ProfileName::as_str),
                        "addressability": addressability_json(status.addressability),
                        "display_name": status.display_name.as_ref().map(super::identity_first::DisplayName::as_str),
                        "labels": status.labels,
                        "generation": status.generation.map(super::identity_first::ContinuityGeneration::get),
                        "checkpoint_version": status.checkpoint_version.map(super::identity_first::CheckpointVersion::get),
                        "continuity_health": continuity_health,
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
                Err(e @ crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    if let Some(live) = target.live.as_ref() {
                        JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: Some(rpc_live_identity_status_json(live)),
                            error: None,
                        }
                    } else {
                        identity_error_response(response_id, &e)
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/respawn" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return maybe_identity_not_configured(is_notification, response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target =
                match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
                {
                    Ok(target) => target,
                    Err(e) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let identity = target.identity.clone();
            if let Some(response) =
                rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
            {
                return if is_notification {
                    String::new()
                } else {
                    serialize_response(&response)
                };
            }
            let registered_status = match identity_rt.status(&identity).await {
                Ok(status) => Some(status),
                Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => None,
                Err(e) => {
                    let response = identity_error_response(response_id, &e);
                    return if is_notification {
                        String::new()
                    } else {
                        serialize_response(&response)
                    };
                }
            };
            match identity_rt.respawn(&identity).await {
                Ok(mut record) => {
                    let live_respawn_warning = match Box::pin(respawn_rpc_runtime_member_id(
                        runtime,
                        record.agent_runtime_id.as_str(),
                    ))
                    .await
                    {
                        Ok(live_result) => {
                            let topology_restore_warning = live_result
                                .get("topology_restore_warning")
                                .filter(|warning| !warning.is_null())
                                .cloned();
                            let live_session_id =
                                live_result.get("session_id").and_then(Value::as_str);
                            if let Some(live_session_id) = live_session_id {
                                match meerkat_core::types::SessionId::parse(live_session_id) {
                                    Ok(session_id) => {
                                        match identity_rt
                                            .rebind_session_after_live_respawn(
                                                &identity, session_id,
                                            )
                                            .await
                                        {
                                            Ok(updated_record) => {
                                                record = updated_record;
                                                topology_restore_warning
                                            }
                                            Err(err) => Some(serde_json::json!({
                                                "kind": "identity_rebind_failed_after_member_respawn",
                                                "message": err.to_string(),
                                                "identity": identity.as_str(),
                                                "agent_runtime_id": record.agent_runtime_id.as_str(),
                                                "live_session_id": live_session_id,
                                            })),
                                        }
                                    }
                                    Err(err) => Some(serde_json::json!({
                                        "kind": "member_respawn_session_id_invalid",
                                        "message": err.to_string(),
                                        "identity": identity.as_str(),
                                        "agent_runtime_id": record.agent_runtime_id.as_str(),
                                        "live_session_id": live_session_id,
                                    })),
                                }
                            } else {
                                topology_restore_warning
                            }
                        }
                        Err(err) => Some(serde_json::json!({
                            "kind": "member_respawn_failed_after_identity_refresh",
                            "message": err,
                            "identity": identity.as_str(),
                            "agent_runtime_id": record.agent_runtime_id.as_str(),
                        })),
                    };
                    let cleanup_warning = if registered_status.is_some()
                        && let Err(err) = retire_stale_rpc_members_for_identity(
                            runtime,
                            identity.as_str(),
                            Some(record.agent_runtime_id.as_str()),
                        )
                        .await
                    {
                        Some(serde_json::json!({
                            "kind": "stale_member_cleanup_failed_after_identity_respawn",
                            "message": err,
                            "identity": identity.as_str(),
                            "agent_runtime_id": record.agent_runtime_id.as_str(),
                        }))
                    } else {
                        None
                    };
                    runtime
                        .record_console_lifecycle(
                            identity.as_str(),
                            "identity_respawned",
                            serde_json::json!({
                                "generation": record.generation.get(),
                                "checkpoint_version": record.checkpoint_version.get(),
                                "live_respawn_warning": live_respawn_warning.clone(),
                                "cleanup_warning": cleanup_warning.clone(),
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
                            "live_respawn_warning": live_respawn_warning,
                            "cleanup_warning": cleanup_warning,
                        })),
                        error: None,
                    }
                }
                Err(e @ crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    if let Some(live) = target.live.as_ref() {
                        match Box::pin(respawn_rpc_live_identity(runtime, live)).await {
                            Ok(result) => {
                                runtime
                                    .record_console_lifecycle(
                                        live.identity.as_str(),
                                        "identity_respawned",
                                        serde_json::json!({}),
                                    )
                                    .await;
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
                                    code: -32000,
                                    message: format!("respawn failed: {err}"),
                                    data: None,
                                }),
                            },
                        }
                    } else {
                        identity_error_response(response_id, &e)
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/retire" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return maybe_identity_not_configured(is_notification, response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target =
                match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
                {
                    Ok(target) => target,
                    Err(e) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let identity = target.identity.clone();
            if let Some(response) =
                rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
            {
                return if is_notification {
                    String::new()
                } else {
                    serialize_response(&response)
                };
            }
            let registered_status = match identity_rt.status(&identity).await {
                Ok(status) => Some(status),
                Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => None,
                Err(e) => {
                    let response = identity_error_response(response_id, &e);
                    return if is_notification {
                        String::new()
                    } else {
                        serialize_response(&response)
                    };
                }
            };
            match identity_rt.retire(&identity).await {
                Ok(token) => {
                    let keep_runtime_member_id = registered_status
                        .as_ref()
                        .and_then(|status| status.agent_runtime_id.as_ref())
                        .filter(|_| identity_rt.has_session_bridge())
                        .map(crate::identity_first::AgentRuntimeId::as_str);
                    let cleanup_warning = if registered_status.is_some()
                        && let Err(err) = retire_stale_rpc_members_for_identity(
                            runtime,
                            identity.as_str(),
                            keep_runtime_member_id,
                        )
                        .await
                    {
                        Some(serde_json::json!({
                            "kind": "stale_member_cleanup_failed_after_identity_retire",
                            "message": err,
                            "identity": identity.as_str(),
                        }))
                    } else {
                        None
                    };
                    runtime
                        .record_console_lifecycle(
                            identity.as_str(),
                            "identity_retired",
                            serde_json::json!({
                                "fencing_token": token.get(),
                                "cleanup_warning": cleanup_warning.clone(),
                            }),
                        )
                        .await;
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({
                            "fencing_token": token.get(),
                            "cleanup_warning": cleanup_warning,
                        })),
                        error: None,
                    }
                }
                Err(e @ crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    if let Some(live) = target.live.as_ref() {
                        match retire_rpc_live_identity(runtime, live).await {
                            Ok(()) => {
                                runtime
                                    .record_console_lifecycle(
                                        live.identity.as_str(),
                                        "identity_retired",
                                        serde_json::json!({}),
                                    )
                                    .await;
                                JsonRpcResponse {
                                    jsonrpc: JSONRPC_VERSION.to_string(),
                                    id: response_id,
                                    result: Some(
                                        serde_json::json!({ "identity": live.identity.as_str() }),
                                    ),
                                    error: None,
                                }
                            }
                            Err(err) => JsonRpcResponse {
                                jsonrpc: JSONRPC_VERSION.to_string(),
                                id: response_id,
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32000,
                                    message: format!("retire failed: {err}"),
                                    data: None,
                                }),
                            },
                        }
                    } else {
                        identity_error_response(response_id, &e)
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/reset" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return maybe_identity_not_configured(is_notification, response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target =
                match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
                {
                    Ok(target) => target,
                    Err(e) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let identity = target.identity.clone();
            if let Some(response) =
                rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
            {
                return if is_notification {
                    String::new()
                } else {
                    serialize_response(&response)
                };
            }
            let _registered_status = match identity_rt.status(&identity).await {
                Ok(status) => {
                    if !identity_rt.has_session_bridge() {
                        let response = rpc_reset_requires_session_bridge_response(response_id);
                        return if is_notification {
                            String::new()
                        } else {
                            serialize_response(&response)
                        };
                    }
                    status
                }
                Err(e @ crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    if let Some(live) = target.live.as_ref() {
                        let response =
                            match Box::pin(respawn_rpc_live_identity(runtime, live)).await {
                                Ok(result) => {
                                    runtime
                                        .record_console_lifecycle(
                                            live.identity.as_str(),
                                            "identity_reset",
                                            serde_json::json!({}),
                                        )
                                        .await;
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
                                        code: -32000,
                                        message: format!("reset failed: {err}"),
                                        data: None,
                                    }),
                                },
                            };
                        return if is_notification {
                            String::new()
                        } else {
                            serialize_response(&response)
                        };
                    }
                    let response = identity_error_response(response_id, &e);
                    return if is_notification {
                        String::new()
                    } else {
                        serialize_response(&response)
                    };
                }
                Err(e) => {
                    let response = identity_error_response(response_id, &e);
                    return if is_notification {
                        String::new()
                    } else {
                        serialize_response(&response)
                    };
                }
            };
            match identity_rt.reset(&identity).await {
                Ok(record) => {
                    let cleanup_warning = if let Err(err) = retire_stale_rpc_members_for_identity(
                        runtime,
                        identity.as_str(),
                        Some(record.agent_runtime_id.as_str()),
                    )
                    .await
                    {
                        Some(serde_json::json!({
                            "kind": "stale_member_cleanup_failed_after_identity_reset",
                            "message": err,
                            "identity": identity.as_str(),
                            "agent_runtime_id": record.agent_runtime_id.as_str(),
                        }))
                    } else {
                        None
                    };
                    runtime
                        .record_console_lifecycle(
                            identity.as_str(),
                            "identity_reset",
                            serde_json::json!({
                                "generation": record.generation.get(),
                                "checkpoint_version": record.checkpoint_version.get(),
                                "cleanup_warning": cleanup_warning.clone(),
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
                            "cleanup_warning": cleanup_warning,
                        })),
                        error: None,
                    }
                }
                Err(e @ crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    if let Some(live) = target.live.as_ref() {
                        match Box::pin(respawn_rpc_live_identity(runtime, live)).await {
                            Ok(result) => {
                                runtime
                                    .record_console_lifecycle(
                                        live.identity.as_str(),
                                        "identity_reset",
                                        serde_json::json!({}),
                                    )
                                    .await;
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
                                    code: -32000,
                                    message: format!("reset failed: {err}"),
                                    data: None,
                                }),
                            },
                        }
                    } else {
                        identity_error_response(response_id, &e)
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/delete_identity" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return maybe_identity_not_configured(is_notification, response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target =
                match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
                {
                    Ok(target) => target,
                    Err(e) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let identity = target.identity.clone();
            if let Some(response) =
                rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
            {
                return if is_notification {
                    String::new()
                } else {
                    serialize_response(&response)
                };
            }
            let registered_status = match identity_rt.status(&identity).await {
                Ok(status) => status,
                Err(e @ crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    if target.live.is_some() {
                        let response = JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32602,
                                message: format!(
                                    "delete_identity requires durable identity: {} is live-only",
                                    identity.as_str()
                                ),
                                data: Some(serde_json::json!({
                                    "kind": "live_only_identity_delete_unsupported",
                                    "identity": identity.as_str(),
                                })),
                            }),
                        };
                        return if is_notification {
                            String::new()
                        } else {
                            serialize_response(&response)
                        };
                    }
                    let response = identity_error_response(response_id, &e);
                    return if is_notification {
                        String::new()
                    } else {
                        serialize_response(&response)
                    };
                }
                Err(e) => {
                    let response = identity_error_response(response_id, &e);
                    return if is_notification {
                        String::new()
                    } else {
                        serialize_response(&response)
                    };
                }
            };
            let keep_runtime_member_id = registered_status
                .agent_runtime_id
                .as_ref()
                .filter(|_| identity_rt.has_session_bridge())
                .map(crate::identity_first::AgentRuntimeId::as_str);
            match identity_rt.delete_identity(&identity).await {
                Ok(()) => {
                    let cleanup_warning = if let Err(err) = retire_stale_rpc_members_for_identity(
                        runtime,
                        identity.as_str(),
                        keep_runtime_member_id,
                    )
                    .await
                    {
                        Some(serde_json::json!({
                            "kind": "stale_member_cleanup_failed_after_identity_delete",
                            "identity": identity.as_str(),
                            "message": err,
                        }))
                    } else {
                        None
                    };
                    runtime
                        .record_console_lifecycle(
                            identity.as_str(),
                            "identity_deleted",
                            serde_json::json!({
                                "cleanup_warning": cleanup_warning,
                            }),
                        )
                        .await;
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({
                            "identity": identity.as_str(),
                            "cleanup_warning": cleanup_warning,
                        })),
                        error: None,
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/inspect_identity" => {
            let identity_rt = match identity_ctx {
                Some(ctx) => &*ctx.runtime,
                None => return maybe_identity_not_configured(is_notification, response_id),
            };
            let identity_str = request
                .params
                .get("identity")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target =
                match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
                {
                    Ok(target) => target,
                    Err(e) => {
                        return maybe_error_response(
                            is_notification,
                            response_id,
                            -32602,
                            format!("invalid identity: {e}"),
                        );
                    }
                };
            let identity = target.identity.clone();
            let status = identity_rt.status(&identity).await;
            if let Some(response) =
                rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
            {
                return if is_notification {
                    String::new()
                } else {
                    serialize_response(&response)
                };
            }
            match identity_rt.inspect(&identity).await {
                Ok(inspection) => {
                    let status = status.ok();
                    JsonRpcResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        id: response_id,
                        result: Some(serde_json::json!({
                            "identity": identity.as_str(),
                            "state": status.as_ref().map(|status| format!("{:?}", status.state)),
                            "profile": status.as_ref().and_then(|status| status.profile.as_ref().map(meerkat_mob::ProfileName::as_str)),
                            "addressability": status.as_ref().map(|status| addressability_json(status.addressability)),
                            "display_name": status.as_ref().and_then(|status| status.display_name.as_ref().map(super::identity_first::DisplayName::as_str)),
                            "labels": status.as_ref().map(|status| status.labels.clone()).unwrap_or_default(),
                            "generation": status.as_ref().and_then(|status| status.generation.map(super::identity_first::ContinuityGeneration::get)),
                            "checkpoint_version": status.as_ref().and_then(|status| status.checkpoint_version.map(super::identity_first::CheckpointVersion::get)),
                            "continuity_health": status.as_ref().and_then(|status| serde_json::to_value(&status.continuity_health).ok()).unwrap_or(Value::Null),
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
                Err(e @ crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    if let Some(live) = target.live.as_ref() {
                        JsonRpcResponse {
                            jsonrpc: JSONRPC_VERSION.to_string(),
                            id: response_id,
                            result: Some(rpc_live_identity_inspect_json(runtime, live).await),
                            error: None,
                        }
                    } else {
                        identity_error_response(response_id, &e)
                    }
                }
                Err(e) => identity_error_response(response_id, &e),
            }
        }
        "mobkit/reconcile_identity" => {
            let ctx = match identity_ctx {
                Some(ctx) => ctx,
                None => return maybe_identity_not_configured(is_notification, response_id),
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
                    return maybe_error_response(
                        is_notification,
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
                                crate::identity_first::RestoreOutcome::Dormant {
                                    record, ..
                                } => {
                                    serde_json::json!({
                                        "outcome": "dormant",
                                        "identity": id.as_str(),
                                        "agent_runtime_id": record.as_ref().map(|record| record.agent_runtime_id.as_str()),
                                        "session_id": record.as_ref().map(|record| record.session_id.to_string()),
                                        "generation": record.as_ref().map(|record| record.generation.get()),
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

#[derive(Debug, Clone)]
struct RpcLiveIdentityAlias {
    identity: crate::identity_first::AgentIdentity,
    runtime_member_id: String,
    member: meerkat_mob::runtime::MobMemberListEntry,
    session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct RpcIdentityControlTarget {
    identity: crate::identity_first::AgentIdentity,
    live: Option<RpcLiveIdentityAlias>,
}

fn rpc_member_durable_identity(member: &meerkat_mob::runtime::MobMemberListEntry) -> String {
    member
        .labels
        .get("agent_identity")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        // Fallback surfaces the public alias, not the comms-safe roster id
        // (meerkat 0.7 MemberCommsName).
        .unwrap_or_else(|| {
            crate::member_comms_id::runtime_alias_str(member.agent_identity.as_str()).into_owned()
        })
}

async fn resolve_rpc_live_identity_alias(
    runtime: &UnifiedRuntime,
    requested_identity: &str,
) -> Result<Option<RpcLiveIdentityAlias>, String> {
    let matches = resolve_rpc_live_identity_alias_candidates(runtime, requested_identity).await?;
    if matches.len() > 1 {
        return Err(format!(
            "ambiguous live identity alias {requested_identity}: candidates [{}]",
            matches
                .iter()
                .map(|entry| entry.runtime_member_id.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(matches.into_iter().next())
}

async fn resolve_rpc_live_runtime_member_alias(
    runtime: &UnifiedRuntime,
    runtime_member_id: &str,
) -> Result<Option<RpcLiveIdentityAlias>, String> {
    let requested_member_id = crate::member_comms_id::mob_member_id(runtime_member_id);
    let handle = runtime.mob_handle();
    let Some(member) = handle
        .list_members_including_retiring()
        .await
        .into_iter()
        .find(|entry| entry.agent_identity == requested_member_id)
    else {
        return Ok(None);
    };
    if !rpc_live_identity_alias_member_visible(&member) {
        return Ok(None);
    }
    let durable_identity = rpc_member_durable_identity(&member);
    let identity = crate::identity_first::AgentIdentity::parse(&durable_identity)
        .map_err(|err| format!("invalid projected identity {durable_identity}: {err}"))?;
    let session_id = handle
        .resolve_bridge_session_id_observation(&member.agent_identity)
        .await
        .map(|session_id| session_id.to_string());
    Ok(Some(RpcLiveIdentityAlias {
        identity,
        runtime_member_id: crate::member_comms_id::runtime_alias_str(
            member.agent_identity.as_str(),
        )
        .into_owned(),
        member,
        session_id,
    }))
}

async fn rpc_runtime_member_alias_exists_hidden(
    runtime: &UnifiedRuntime,
    runtime_member_id: &str,
) -> bool {
    let requested_member_id = crate::member_comms_id::mob_member_id(runtime_member_id);
    runtime
        .mob_handle()
        .list_members_including_retiring()
        .await
        .into_iter()
        .find(|entry| entry.agent_identity == requested_member_id)
        .is_some_and(|member| !rpc_live_identity_alias_member_visible(&member))
}

async fn rpc_live_identity_alias_exists_hidden(
    runtime: &UnifiedRuntime,
    requested_identity: &str,
) -> bool {
    let requested_member_id = crate::member_comms_id::mob_member_id(requested_identity);
    runtime
        .mob_handle()
        .list_members_including_retiring()
        .await
        .into_iter()
        .any(|member| {
            (member.agent_identity == requested_member_id
                || member
                    .labels
                    .get("agent_identity")
                    .is_some_and(|identity| identity == requested_identity))
                && !rpc_live_identity_alias_member_visible(&member)
        })
}

async fn resolve_rpc_live_identity_alias_candidates(
    runtime: &UnifiedRuntime,
    requested_identity: &str,
) -> Result<Vec<RpcLiveIdentityAlias>, String> {
    let requested_member_id = crate::member_comms_id::mob_member_id(requested_identity);
    let handle = runtime.mob_handle();
    let members = handle.list_members_including_retiring().await;
    let exact_matches = members
        .iter()
        .filter(|entry| entry.agent_identity == requested_member_id)
        .cloned()
        .collect::<Vec<_>>();
    let label_matches = members
        .iter()
        .filter(|entry| {
            entry
                .labels
                .get("agent_identity")
                .is_some_and(|identity| identity == requested_identity)
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut matches = exact_matches;
    matches.extend(label_matches);
    let mut seen_member_ids = BTreeSet::new();
    matches.retain(|entry| seen_member_ids.insert(entry.agent_identity.to_string()));
    let mut aliases = Vec::with_capacity(matches.len());
    for member in matches {
        if !rpc_live_identity_alias_member_visible(&member) {
            continue;
        }
        let durable_identity = rpc_member_durable_identity(&member);
        let identity = crate::identity_first::AgentIdentity::parse(&durable_identity)
            .map_err(|err| format!("invalid projected identity {durable_identity}: {err}"))?;
        let session_id = handle
            .resolve_bridge_session_id_observation(&member.agent_identity)
            .await
            .map(|session_id| session_id.to_string());
        aliases.push(RpcLiveIdentityAlias {
            identity,
            runtime_member_id: crate::member_comms_id::runtime_alias_str(
                member.agent_identity.as_str(),
            )
            .into_owned(),
            member,
            session_id,
        });
    }
    Ok(aliases)
}

fn rpc_live_identity_alias_member_visible(
    member: &meerkat_mob::runtime::MobMemberListEntry,
) -> bool {
    rpc_live_identity_alias_visible(member.role.as_str(), &member.labels)
}

fn rpc_live_identity_alias_visible(
    member_role: &str,
    labels: &std::collections::BTreeMap<String, String>,
) -> bool {
    let projected_role = labels
        .get("role")
        .map(String::as_str)
        .unwrap_or(member_role);
    !is_implicit_delegate_member(member_role, labels)
        && !is_implicit_delegate_member(projected_role, labels)
}

async fn resolve_rpc_identity_control_target(
    runtime: &UnifiedRuntime,
    identity_rt: &crate::identity_first::IdentityRuntime,
    requested_identity: &str,
) -> Result<RpcIdentityControlTarget, String> {
    if requested_identity.starts_with("rt:") {
        for status in identity_rt.statuses().await {
            if status
                .agent_runtime_id
                .as_ref()
                .is_some_and(|runtime_id| runtime_id.as_str() == requested_identity)
            {
                let identity = status.identity;
                let registered_live =
                    resolve_rpc_live_runtime_member_alias(runtime, requested_identity).await?;
                if let Some(registered) = registered_live {
                    return Ok(RpcIdentityControlTarget {
                        identity,
                        live: Some(registered),
                    });
                }
                if rpc_runtime_member_alias_exists_hidden(runtime, requested_identity).await {
                    return Err(format!("identity hidden by policy: {requested_identity}"));
                }
                let durable_live_candidates =
                    resolve_rpc_live_identity_alias_candidates(runtime, identity.as_str()).await?;
                let durable_live = if durable_live_candidates.len() > 1 {
                    return Err(format!(
                        "ambiguous live identity alias {}: candidates [{}]",
                        identity.as_str(),
                        durable_live_candidates
                            .iter()
                            .map(|alias| alias.runtime_member_id.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                } else {
                    durable_live_candidates.into_iter().next()
                };
                return Ok(RpcIdentityControlTarget {
                    identity,
                    live: durable_live,
                });
            }
        }
        let live = resolve_rpc_live_identity_alias(runtime, requested_identity).await?;
        if let Some(live_alias) = live {
            let live_identity_candidates =
                resolve_rpc_live_identity_alias_candidates(runtime, live_alias.identity.as_str())
                    .await?;
            if live_identity_candidates.len() > 1 {
                return Err(format!(
                    "ambiguous live identity alias {}: candidates [{}]",
                    live_alias.identity.as_str(),
                    live_identity_candidates
                        .iter()
                        .map(|alias| alias.runtime_member_id.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            return Ok(RpcIdentityControlTarget {
                identity: live_alias.identity.clone(),
                live: Some(live_alias),
            });
        }
        if rpc_runtime_member_alias_exists_hidden(runtime, requested_identity).await {
            return Err(format!("identity hidden by policy: {requested_identity}"));
        }
        return Err(format!("runtime identity not found: {requested_identity}"));
    }
    if let Ok(identity) = crate::identity_first::AgentIdentity::parse(requested_identity) {
        match identity_rt.status(&identity).await {
            Ok(status) => {
                let registered_live = match status.agent_runtime_id.as_ref() {
                    Some(runtime_id) => {
                        resolve_rpc_live_runtime_member_alias(runtime, runtime_id.as_str()).await?
                    }
                    None => None,
                };
                if let Some(registered) = registered_live {
                    return Ok(RpcIdentityControlTarget {
                        identity,
                        live: Some(registered),
                    });
                }
                if let Some(runtime_id) = status.agent_runtime_id.as_ref()
                    && rpc_runtime_member_alias_exists_hidden(runtime, runtime_id.as_str()).await
                {
                    return Err(format!("identity hidden by policy: {requested_identity}"));
                }
                let requested_live_candidates =
                    resolve_rpc_live_identity_alias_candidates(runtime, requested_identity).await?;
                let requested_live = if requested_live_candidates.len() > 1 {
                    return Err(format!(
                        "ambiguous live identity alias {requested_identity}: candidates [{}]",
                        requested_live_candidates
                            .iter()
                            .map(|alias| alias.runtime_member_id.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                } else {
                    requested_live_candidates.into_iter().next()
                };
                return Ok(RpcIdentityControlTarget {
                    identity,
                    live: requested_live,
                });
            }
            Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {}
            Err(err) => return Err(err.to_string()),
        }
    }
    for status in identity_rt.statuses().await {
        if status
            .agent_runtime_id
            .as_ref()
            .is_some_and(|runtime_id| runtime_id.as_str() == requested_identity)
        {
            let identity = status.identity;
            let registered_live =
                resolve_rpc_live_runtime_member_alias(runtime, requested_identity).await?;
            let durable_live_candidates =
                resolve_rpc_live_identity_alias_candidates(runtime, identity.as_str()).await?;
            let durable_live = if durable_live_candidates.len() > 1 {
                return Err(format!(
                    "ambiguous live identity alias {}: candidates [{}]",
                    identity.as_str(),
                    durable_live_candidates
                        .iter()
                        .map(|alias| alias.runtime_member_id.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            } else {
                durable_live_candidates.into_iter().next()
            };
            let live = match (registered_live, durable_live) {
                (Some(registered), Some(durable))
                    if registered.runtime_member_id == durable.runtime_member_id =>
                {
                    Some(registered)
                }
                (Some(registered), None) => Some(registered),
                (Some(_registered), Some(durable)) => Some(durable),
                (None, durable) => durable,
            };
            return Ok(RpcIdentityControlTarget { identity, live });
        }
    }
    let live = resolve_rpc_live_identity_alias(runtime, requested_identity).await?;
    if let Some(live_alias) = live {
        if let Some(bound_status) = identity_rt.statuses().await.into_iter().find(|status| {
            status
                .agent_runtime_id
                .as_ref()
                .is_some_and(|runtime_id| runtime_id.as_str() == live_alias.runtime_member_id)
        }) && bound_status.identity != live_alias.identity
        {
            return Err(format!(
                "stale live identity alias: live console alias {} resolves to {}, but identity runtime binding belongs to {}",
                live_alias.identity.as_str(),
                live_alias.runtime_member_id,
                bound_status.identity.as_str(),
            ));
        }
        let live_identity_candidates =
            resolve_rpc_live_identity_alias_candidates(runtime, live_alias.identity.as_str())
                .await?;
        if live_identity_candidates.len() > 1 {
            return Err(format!(
                "ambiguous live identity alias {}: candidates [{}]",
                live_alias.identity.as_str(),
                live_identity_candidates
                    .iter()
                    .map(|alias| alias.runtime_member_id.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        return Ok(RpcIdentityControlTarget {
            identity: live_alias.identity.clone(),
            live: Some(live_alias),
        });
    }
    if rpc_live_identity_alias_exists_hidden(runtime, requested_identity).await {
        return Err(format!("identity hidden by policy: {requested_identity}"));
    }
    let identity = crate::identity_first::AgentIdentity::parse(requested_identity)
        .map_err(|err| err.to_string())?;
    Ok(RpcIdentityControlTarget {
        identity,
        live: None,
    })
}

fn rpc_reset_requires_session_bridge_response(response_id: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: None,
        error: Some(JsonRpcError {
            code: -32602,
            message: "reset requires an identity runtime with a session bridge".to_string(),
            data: Some(serde_json::json!({
                "kind": "identity_reset_requires_session_bridge",
            })),
        }),
    }
}

fn rpc_live_alias_matches_status_runtime(
    alias: Option<&RpcLiveIdentityAlias>,
    status: &crate::identity_first::IdentityStatus,
) -> bool {
    let Some(alias) = alias else {
        return true;
    };
    let session_matches = match (
        status.session_id.as_ref().map(ToString::to_string),
        alias.session_id.as_deref(),
    ) {
        (Some(status_session), Some(live_session)) => status_session == live_session,
        _ => true,
    };
    status
        .agent_runtime_id
        .as_ref()
        .is_some_and(|runtime_id| runtime_id.as_str() == alias.runtime_member_id)
        && alias.identity == status.identity
        && session_matches
}

async fn rpc_stale_live_alias_error_response(
    identity_rt: &crate::identity_first::IdentityRuntime,
    target: &RpcIdentityControlTarget,
    response_id: Value,
) -> Option<JsonRpcResponse> {
    let live = target.live.as_ref()?;
    let Ok(status) = identity_rt.status(&target.identity).await else {
        return None;
    };
    if rpc_live_alias_matches_status_runtime(Some(live), &status) {
        return None;
    }
    Some(JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: None,
        error: Some(JsonRpcError {
            code: -32000,
            message: format!(
                "identity runtime binding for {} points at {}, but requested live member is {}",
                target.identity.as_str(),
                status
                    .agent_runtime_id
                    .as_ref()
                    .map(crate::identity_first::AgentRuntimeId::as_str)
                    .unwrap_or("<none>"),
                live.runtime_member_id
            ),
            data: Some(serde_json::json!({
                "kind": "stale_identity_runtime_binding",
                "identity": target.identity.as_str(),
                "registered_runtime_member_id": status.agent_runtime_id.as_ref().map(crate::identity_first::AgentRuntimeId::as_str),
                "live_runtime_member_id": live.runtime_member_id,
                "registered_session_id": status.session_id.as_ref().map(ToString::to_string),
                "live_session_id": live.session_id,
            })),
        }),
    })
}

fn rpc_member_is_addressable(member: &meerkat_mob::runtime::MobMemberListEntry) -> bool {
    member
        .labels
        .get("addressable")
        .map(|value| !value.eq_ignore_ascii_case("false"))
        .unwrap_or(true)
}

fn rpc_live_identity_status_json(alias: &RpcLiveIdentityAlias) -> Value {
    serde_json::json!({
        "state": crate::mob_handle_runtime::member_status_state_string(alias.member.status),
        "identity": alias.identity.as_str(),
        "agent_runtime_id": alias.runtime_member_id,
        "session_id": alias.session_id,
        "profile": alias.member.role.to_string(),
        "addressability": if rpc_member_is_addressable(&alias.member) { "addressable" } else { "internal_only" },
        "display_name": alias.member.labels.get("display_name"),
        "labels": alias.member.labels,
        "generation": Value::Null,
        "checkpoint_version": Value::Null,
        "continuity_health": Value::Null,
        "lease_healthy": Value::Null,
        "lease": Value::Null,
    })
}

async fn rpc_live_identity_inspect_json(
    runtime: &UnifiedRuntime,
    alias: &RpcLiveIdentityAlias,
) -> Value {
    let snapshot = runtime
        .mob_handle()
        .member_status(&crate::member_comms_id::mob_member_id(
            alias.runtime_member_id.as_str(),
        ))
        .await
        .ok();
    serde_json::json!({
        "identity": alias.identity.as_str(),
        "state": crate::mob_handle_runtime::member_status_state_string(alias.member.status),
        "profile": alias.member.role.to_string(),
        "addressability": if rpc_member_is_addressable(&alias.member) { "addressable" } else { "internal_only" },
        "display_name": alias.member.labels.get("display_name"),
        "labels": alias.member.labels,
        "generation": Value::Null,
        "checkpoint_version": Value::Null,
        "continuity_health": Value::Null,
        "lease_healthy": Value::Null,
        "continuity": {
            "generation": Value::Null,
            "checkpoint_version": Value::Null,
            "session_id": alias.session_id,
            "agent_runtime_id": alias.runtime_member_id,
        },
        "lease": Value::Null,
        "output_preview": snapshot.as_ref().and_then(|snapshot| snapshot.output_preview.clone()),
        "is_final": snapshot.as_ref().map(|snapshot| snapshot.is_final).unwrap_or(false),
        "peer_reachable_count": alias.member.wired_to.len(),
    })
}

async fn retire_rpc_live_identity(
    runtime: &UnifiedRuntime,
    alias: &RpcLiveIdentityAlias,
) -> Result<(), String> {
    retire_rpc_runtime_member_id(runtime, alias.runtime_member_id.as_str()).await
}

async fn retire_rpc_runtime_member_id(
    runtime: &UnifiedRuntime,
    runtime_member_id: &str,
) -> Result<(), String> {
    match runtime
        .mob_handle()
        .retire(crate::member_comms_id::mob_member_id(runtime_member_id))
        .await
    {
        Ok(()) => Ok(()),
        Err(err) if mob_methods::lifecycle_archive_cleanup_completed(&err.to_string()) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

fn rpc_member_id_matches_durable_identity(member_id: &str, durable_identity: &str) -> bool {
    // Roster ids are comms-safe encodings of public aliases (meerkat 0.7
    // MemberCommsName); compare in the public alias space.
    crate::member_comms_id::runtime_alias_str(member_id) == durable_identity
}

async fn retire_stale_rpc_members_for_identity(
    runtime: &UnifiedRuntime,
    durable_identity: &str,
    keep_runtime_member_id: Option<&str>,
) -> Result<(), String> {
    let stale_members = runtime
        .mob_handle()
        .list_members_including_retiring()
        .await
        .into_iter()
        .filter(|member| {
            if !rpc_live_identity_alias_member_visible(member) {
                return false;
            }
            (rpc_member_id_matches_durable_identity(
                member.agent_identity.as_str(),
                durable_identity,
            ) || member
                .labels
                .get("agent_identity")
                .is_some_and(|identity| identity == durable_identity))
                && keep_runtime_member_id
                    .map(|keep| {
                        // `keep` is a public alias; compare decoded.
                        crate::member_comms_id::runtime_alias_str(member.agent_identity.as_str())
                            != keep
                    })
                    .unwrap_or(true)
        })
        // `retire_rpc_runtime_member_id` re-encodes; hand it the alias.
        .map(|member| {
            crate::member_comms_id::runtime_alias_str(member.agent_identity.as_str()).into_owned()
        })
        .collect::<Vec<_>>();
    for member_id in stale_members {
        retire_rpc_runtime_member_id(runtime, &member_id).await?;
    }
    Ok(())
}

async fn respawn_rpc_live_identity(
    runtime: &UnifiedRuntime,
    alias: &RpcLiveIdentityAlias,
) -> Result<Value, String> {
    let mut result = Box::pin(respawn_rpc_runtime_member_id(
        runtime,
        alias.runtime_member_id.as_str(),
    ))
    .await?;
    result["identity"] = serde_json::json!(alias.identity.as_str());
    Ok(result)
}

async fn respawn_rpc_runtime_member_id(
    runtime: &UnifiedRuntime,
    runtime_member_id: &str,
) -> Result<Value, String> {
    let handle = runtime.mob_handle();
    let member_id = crate::member_comms_id::mob_member_id(runtime_member_id);
    // Best-effort repair material: a faulted lookup degrades to None (the
    // respawn itself surfaces real faults).
    let entry_before_respawn = handle.get_member(&member_id).await.ok().flatten();
    let mut topology_restore_warning = None;
    match handle.respawn(member_id.clone(), None).await {
        Ok(_receipt) => {}
        Err(err) => {
            if let Some(failed_peer_ids) = topology_restore_failed_peer_ids(&err) {
                tracing::warn!(
                    member_id = %member_id,
                    failed_peer_count = failed_peer_ids.len(),
                    failed_peer_ids = ?failed_peer_ids,
                    "rpc member respawn restored member with isolated peer edges; continuing degraded respawn"
                );
                topology_restore_warning = Some(topology_restore_warning_json(&failed_peer_ids));
            } else if mob_methods::lifecycle_archive_cleanup_completed(&err.to_string()) {
                // A faulted lookup must not read as "absent" (that would mint
                // a spurious replacement member); surface it instead.
                if handle
                    .get_member(&member_id)
                    .await
                    .map_err(|lookup_err| lookup_err.to_string())?
                    .is_none()
                    && let Some(entry) = entry_before_respawn
                {
                    let mut spec =
                        meerkat_mob::SpawnMemberSpec::new(entry.role.clone(), member_id.clone());
                    if !entry.labels.is_empty() {
                        spec = spec.with_labels(entry.labels.clone());
                    }
                    handle
                        .ensure_member(spec)
                        .await
                        .map_err(|ensure_err| ensure_err.to_string())?;
                }
            } else {
                return Err(err.to_string());
            }
        }
    }
    let session_id = handle
        .resolve_bridge_session_id_observation(&member_id)
        .await
        .map(|session_id| session_id.to_string());
    Ok(serde_json::json!({
        "agent_runtime_id": runtime_member_id,
        "session_id": session_id,
        "generation": Value::Null,
        "checkpoint_version": Value::Null,
        "topology_restore_warning": topology_restore_warning,
    }))
}

fn identity_not_configured(response_id: Value) -> String {
    error_response(response_id, -32601, "identity-first runtime not configured")
}

fn maybe_identity_not_configured(is_notification: bool, response_id: Value) -> String {
    if is_notification {
        String::new()
    } else {
        identity_not_configured(response_id)
    }
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
    let message = message.into();
    let ambiguous_alias_rest = message
        .strip_prefix("ambiguous live identity alias ")
        .or_else(|| message.strip_prefix("invalid identity: ambiguous live identity alias "));
    let stale_live_alias_rest = message
        .strip_prefix("stale live identity alias: live console alias ")
        .or_else(|| {
            message.strip_prefix("invalid identity: stale live identity alias: live console alias ")
        });
    let hidden_policy_identity = message
        .strip_prefix("identity hidden by policy: ")
        .or_else(|| message.strip_prefix("invalid identity: identity hidden by policy: "));
    let data = if let Some(rest) = ambiguous_alias_rest {
        let (identity, candidates) = rest
            .split_once(": candidates [")
            .map(|(identity, candidates)| {
                (
                    identity.to_string(),
                    candidates
                        .trim_end_matches(']')
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_else(|| (rest.to_string(), Vec::new()));
        Some(serde_json::json!({
            "kind": "ambiguous_live_identity_alias",
            "identity": identity,
            "candidates": candidates,
        }))
    } else if let Some(rest) = stale_live_alias_rest {
        let (identity, rest) = rest.split_once(" resolves to ").unwrap_or((rest, ""));
        let (runtime_member_id, bound_identity) = rest
            .split_once(", but identity runtime binding belongs to ")
            .unwrap_or((rest, ""));
        Some(serde_json::json!({
            "kind": "stale_live_identity_alias",
            "identity": identity,
            "live_runtime_member_id": runtime_member_id,
            "bound_identity": bound_identity,
        }))
    } else {
        hidden_policy_identity.map(|identity| {
            serde_json::json!({
                "kind": "identity_hidden_by_policy",
                "identity": identity,
            })
        })
    };
    serialize_response(&JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message,
            data,
        }),
    })
}

fn maybe_error_response(
    is_notification: bool,
    response_id: Value,
    code: i64,
    message: impl Into<String>,
) -> String {
    if is_notification {
        String::new()
    } else {
        error_response(response_id, code, message)
    }
}

fn serialize_response(response: &JsonRpcResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#
            .to_string()
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{
        error_response, handle_unified_rpc_json, resolve_rpc_identity_control_target,
        rpc_live_identity_alias_visible, rpc_member_id_matches_durable_identity,
    };
    use crate::identity_first::contracts::RosterProvider;
    use crate::identity_first::{
        AgentAddressability, AgentIdentity, AgentRuntimeId, CheckpointVersion,
        ContinuityGeneration, ContinuityRecord, DurabilityPolicy, DurableAgentSpec, FencingToken,
        IdentityLifecycleState, IdentityRuntime, IdentityRuntimeConfig, LeaseGrant,
        LocalContinuityStore, LocalLeaseProvider, RosterContext, RosterError,
    };
    use crate::{
        DiscoverySpec, IdentityFirstContext, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig,
        UnifiedRuntime,
    };
    use async_trait::async_trait;
    use meerkat::{AgentFactory, Config, build_ephemeral_service};
    use meerkat_client::TestClient;
    use meerkat_mob::{MobDefinition, MobStorage, SpawnMemberSpec};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    #[derive(Debug, Default)]
    struct EmptyRosterProvider;

    #[async_trait]
    impl RosterProvider for EmptyRosterProvider {
        async fn roster(
            &self,
            _context: &RosterContext,
        ) -> Result<Vec<DurableAgentSpec>, RosterError> {
            Ok(Vec::new())
        }
    }

    fn rpc_test_mob_spec(
        temp_dir: &tempfile::TempDir,
    ) -> Result<MobBootstrapSpec, Box<dyn std::error::Error + Send + Sync>> {
        let session_path = temp_dir.path().join("sessions");
        std::fs::create_dir_all(&session_path)?;
        let factory = AgentFactory::new(&session_path).comms(true);
        let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));
        let definition = MobDefinition::from_toml(
            r#"
[mob]
id = "rpc-identity-alias-test"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true

[profiles.worker.tools]
comms = true
"#,
        )?;
        Ok(
            MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
                .with_options(MobBootstrapOptions {
                    allow_ephemeral_sessions: true,
                    notify_orchestrator_on_resume: true,
                    default_llm_client: Some(Arc::new(TestClient::default())),
                }),
        )
    }

    #[tokio::test]
    async fn unified_capabilities_separate_mobpack_authoring_from_runtime_controls()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::tempdir()?;
        let runtime = Box::pin(
            UnifiedRuntime::builder()
                .mob_spec(rpc_test_mob_spec(&temp_dir)?)
                .module_config(MobKitConfig {
                    modules: Vec::new(),
                    discovery: DiscoverySpec {
                        namespace: "rpc-authoring-capabilities-test".to_string(),
                        modules: Vec::new(),
                    },
                    pre_spawn: Vec::new(),
                })
                .timeout(Duration::from_secs(1))
                .build(),
        )
        .await?;

        let response: Value = serde_json::from_str(
            &handle_unified_rpc_json(
                &runtime,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "mobkit/capabilities",
                })
                .to_string(),
                Duration::from_secs(1),
                None,
                None,
            )
            .await,
        )?;

        assert!(response["error"].is_null(), "{response:#?}");
        let methods = response["result"]["methods"]
            .as_array()
            .expect("methods array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        for method in super::MOBPACK_AUTHORING_METHODS {
            assert!(
                methods.contains(method),
                "missing authoring method {method}"
            );
        }
        assert_eq!(
            response["result"]["authoring_capabilities"]["domain"],
            json!("mobpack_authoring")
        );
        assert_eq!(
            response["result"]["authoring_capabilities"]["runtime_mutation"],
            json!(false)
        );
        assert_eq!(
            response["result"]["authoring_capabilities"]["host_mutation_methods"]["mobkit/mobpacks/deploy"],
            json!("when execute=true, writes a mobpack archive and runs rkat mob run on the host")
        );
        assert_eq!(
            response["result"]["authoring_capabilities"]["methods"]
                .as_array()
                .expect("authoring methods")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            super::MOBPACK_AUTHORING_METHODS
        );
        assert_eq!(
            response["result"]["authoring_capabilities"]["deploy_command"],
            json!("rkat mob run")
        );

        Ok(())
    }

    #[tokio::test]
    async fn unified_rpc_dispatches_mobpack_authoring_methods()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::tempdir()?;
        let runtime = Box::pin(
            UnifiedRuntime::builder()
                .mob_spec(rpc_test_mob_spec(&temp_dir)?)
                .module_config(MobKitConfig {
                    modules: Vec::new(),
                    discovery: DiscoverySpec {
                        namespace: "rpc-authoring-dispatch-test".to_string(),
                        modules: Vec::new(),
                    },
                    pre_spawn: Vec::new(),
                })
                .timeout(Duration::from_secs(1))
                .build(),
        )
        .await?;

        let response: Value = serde_json::from_str(
            &handle_unified_rpc_json(
                &runtime,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "mobkit/mobpacks/schema",
                })
                .to_string(),
                Duration::from_secs(1),
                None,
                None,
            )
            .await,
        )?;

        assert!(response["error"].is_null(), "{response:#?}");
        assert_eq!(
            response["result"]["media_type"],
            json!("application/vnd.meerkat.mobpack")
        );
        assert_eq!(
            response["result"]["commands"]["deploy_rpc"],
            json!("mobkit/mobpacks/deploy")
        );
        assert_eq!(
            response["result"]["deploy_settings"]["runtime_backed"],
            json!(true)
        );
        assert_eq!(
            response["result"]["deploy_settings"]["authoring_provider"]["runtime_binding"],
            json!("bound")
        );
        assert_eq!(
            response["result"]["deploy_settings"]["provenance"]["source"],
            json!("UnifiedRuntime.authoring_provider.deploy_target")
        );
        assert!(response["result"]["sample_mobpacks"].is_null());
        assert!(response["result"]["agent_definitions"].is_null());

        let catalogs: Value = serde_json::from_str(
            &handle_unified_rpc_json(
                &runtime,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "mobkit/mobpacks/catalogs",
                })
                .to_string(),
                Duration::from_secs(1),
                None,
                None,
            )
            .await,
        )?;
        assert!(catalogs["error"].is_null(), "{catalogs:#?}");
        assert_eq!(catalogs["result"]["runtime_backed"], json!(true));
        assert_eq!(
            catalogs["result"]["authoring_provider"]["id"],
            json!("unified_runtime")
        );
        assert_eq!(
            catalogs["result"]["authoring_provider"]["runtime_binding"],
            json!("bound")
        );
        assert_eq!(
            catalogs["result"]["sources"]["runtime"],
            json!("unified_runtime")
        );
        assert_eq!(
            catalogs["result"]["sources"]["runtime_binding"],
            json!("bound")
        );
        assert!(
            catalogs["result"]["runtime_unavailable_reason"].is_null(),
            "{catalogs:#?}"
        );
        assert_eq!(
            catalogs["result"]["catalog_snapshot"]["runtime_backed"],
            json!(true)
        );
        assert_eq!(
            catalogs["result"]["authoring_provider"]["deploy_target"]["command"],
            json!("rkat mob run")
        );
        assert!(
            catalogs["result"]["authoring_provider"]["runtime_methods"]
                .as_array()
                .is_some_and(|methods| methods.contains(&json!("mobkit/mobpacks/deploy"))),
            "{catalogs:#?}"
        );

        Ok(())
    }

    #[test]
    fn mobpack_authoring_rpc_helper_preserves_runtime_catalog_binding()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let runtime = crate::mobpack::MobpackRuntimeCatalogState {
            loaded_modules: vec!["editor-host".to_string()],
            runtime_methods: vec![
                "mobkit/mobpacks/catalogs".to_string(),
                "mobkit/mobpacks/apply_operation".to_string(),
                "mobkit/mobpacks/deploy".to_string(),
            ],
            has_contact_directory: true,
            has_peer_mob_handles: false,
            has_inproc_contacts: false,
            runtime_flow_rows: vec![json!({
                "id": "runtime_rpc_main",
                "source": "mobkit/runtime/flow_projection",
                "document": {
                    "mob_id": "runtime_rpc",
                    "flow": { "name": "main", "steps": [] },
                    "members": []
                },
                "validation": { "ok": true }
            })],
            runtime_agent_definition_sources: vec![json!({
                "id": "runtime_profiles_rpc",
                "name": "Runtime RPC profiles",
                "source": "mobkit/runtime/agent-definitions",
                "document": {
                    "mob_id": "runtime_rpc",
                    "members": [{
                        "id": "m_runtime_reviewer",
                        "name": "Runtime reviewer",
                        "role": "runtime_reviewer",
                        "profileBinding": "inline",
                        "model": "gpt-5.5",
                        "runtimeMode": "turn_driven",
                        "tools": ["builtins"],
                        "skills": ["mob.runtime.review"],
                        "schema": ""
                    }],
                    "schemas": []
                }
            })],
            runtime_skill_realms: vec![json!({
                "id": "runtime_rpc",
                "label": "Runtime RPC",
                "source": "mobkit/runtime/skills",
                "skills": [{
                    "id": "mob.runtime.review",
                    "label": "Runtime review",
                    "source": "inline",
                    "content": "Review runtime work."
                }]
            })],
        };

        let catalogs = super::handle_mobpack_authoring_rpc_with_runtime(
            "mobkit/mobpacks/catalogs",
            &json!({}),
            json!(1),
            Some(&runtime),
        )
        .expect("catalogs method");
        let catalogs: Value = serde_json::to_value(catalogs)?;
        assert_eq!(catalogs["result"]["runtime_backed"], json!(true));
        assert_eq!(
            catalogs["result"]["authoring_provider"]["runtime_binding"],
            json!("bound")
        );
        assert_eq!(
            catalogs["result"]["runtime_flows"][0]["id"],
            json!("runtime_rpc_main")
        );
        let listed = super::handle_mobpack_authoring_rpc_with_runtime(
            "mobkit/mobpacks/list",
            &json!({}),
            json!(2),
            Some(&runtime),
        )
        .expect("list method");
        let listed: Value = serde_json::to_value(listed)?;
        assert_eq!(listed["result"]["runtime_backed"], json!(true));
        assert_eq!(listed["result"]["rows"][0]["id"], json!("runtime_rpc_main"));

        let fetched = super::handle_mobpack_authoring_rpc_with_runtime(
            "mobkit/mobpacks/get",
            &json!({ "id": "runtime_rpc_main" }),
            json!(3),
            Some(&runtime),
        )
        .expect("get method");
        let fetched: Value = serde_json::to_value(fetched)?;
        assert_eq!(fetched["result"]["runtime_backed"], json!(true));
        assert_eq!(fetched["result"]["row"]["id"], json!("runtime_rpc_main"));

        let definitions = super::handle_mobpack_authoring_rpc_with_runtime(
            "mobkit/agent_definitions/list",
            &json!({}),
            json!(4),
            Some(&runtime),
        )
        .expect("agent definitions method");
        let definitions: Value = serde_json::to_value(definitions)?;
        let runtime_definition = definitions["result"]["agent_definitions"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row["sourceOrigin"] == "mobkit/runtime/agent-definitions")
            })
            .expect("runtime profile definition");
        assert_eq!(runtime_definition["role"], json!("runtime_reviewer"));
        assert_eq!(
            runtime_definition["toolDefinitions"][0]["id"],
            json!("builtins")
        );
        assert_eq!(
            runtime_definition["skillDefinitions"][0]["id"],
            json!("mob.runtime.review")
        );
        assert_eq!(
            definitions["result"]["catalog_snapshot"]["runtime_backed"],
            json!(true)
        );

        let tools = super::handle_mobpack_authoring_rpc_with_runtime(
            "mobkit/tools/catalog",
            &json!({}),
            json!(5),
            Some(&runtime),
        )
        .expect("tools catalog method");
        let tools: Value = serde_json::to_value(tools)?;
        let mob_tool = tools["result"]["tool_catalog"]
            .as_array()
            .expect("tools")
            .iter()
            .find(|tool| tool["id"] == "mob")
            .expect("mob tool");
        assert_eq!(mob_tool["runtime_availability"]["available"], json!(false));

        let agents = super::handle_mobpack_authoring_rpc_with_runtime(
            "mobkit/agent_definitions/list",
            &json!({}),
            json!(3),
            Some(&runtime),
        )
        .expect("agent definitions method");
        let agents: Value = serde_json::to_value(agents)?;
        assert_eq!(agents["result"]["runtime_backed"], json!(true));
        let planner = agents["result"]["agent_definitions"]
            .as_array()
            .expect("agent definitions")
            .iter()
            .find(|definition| definition["role"] == "planner")
            .expect("planner definition");
        let planner_mob_tool = planner["toolDefinitions"]
            .as_array()
            .expect("planner tools")
            .iter()
            .find(|tool| tool["id"] == "mob")
            .expect("planner mob tool");
        assert_eq!(
            planner_mob_tool["runtimeAvailability"]["state"],
            json!("unavailable")
        );

        Ok(())
    }

    #[test]
    fn module_rpc_dispatches_mobpack_authoring_methods()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config = MobKitConfig {
            modules: Vec::new(),
            discovery: DiscoverySpec {
                namespace: "module-rpc-authoring-dispatch-test".to_string(),
                modules: Vec::new(),
            },
            pre_spawn: Vec::new(),
        };
        let mut runtime = crate::start_mobkit_runtime(config, Vec::new(), Duration::from_secs(1))?;

        let capabilities: Value = serde_json::from_str(&super::handle_mobkit_rpc_json(
            &mut runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "mobkit/capabilities",
            })
            .to_string(),
            Duration::from_secs(1),
        ))?;
        let methods = capabilities["result"]["methods"]
            .as_array()
            .expect("methods array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        for method in super::MOBPACK_AUTHORING_METHODS {
            assert!(
                methods.contains(method),
                "missing authoring method {method}"
            );
        }
        assert_eq!(
            capabilities["result"]["authoring_capabilities"]["runtime_mutation"],
            json!(false)
        );
        assert_eq!(
            capabilities["result"]["authoring_capabilities"]["host_mutation_methods"]["mobkit/mobpacks/deploy"],
            json!("when execute=true, writes a mobpack archive and runs rkat mob run on the host")
        );

        let schema: Value = serde_json::from_str(&super::handle_mobkit_rpc_json(
            &mut runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "mobkit/mobpacks/schema",
            })
            .to_string(),
            Duration::from_secs(1),
        ))?;
        assert!(schema["error"].is_null(), "{schema:#?}");
        assert_eq!(
            schema["result"]["commands"]["deploy_rpc"],
            json!("mobkit/mobpacks/deploy")
        );
        assert!(schema["result"]["agent_definitions"].is_null());

        let _ = runtime.shutdown();
        Ok(())
    }

    #[test]
    fn generated_runtime_ids_match_their_durable_identity_prefix() {
        assert!(!rpc_member_id_matches_durable_identity(
            "rt:review:singleton:0",
            "review:singleton",
        ));
        assert!(!rpc_member_id_matches_durable_identity(
            "review:singleton:gen1",
            "review:singleton",
        ));
        assert!(!rpc_member_id_matches_durable_identity(
            "review:singleton:1",
            "review:singleton",
        ));
        assert!(!rpc_member_id_matches_durable_identity(
            "rt:reviewer:singleton:0",
            "review:singleton",
        ));
        assert!(!rpc_member_id_matches_durable_identity(
            "rt:review:singleton:qa:0",
            "review:singleton",
        ));
        assert!(!rpc_member_id_matches_durable_identity(
            "review:singleton:qa",
            "review:singleton",
        ));
    }

    #[test]
    fn rpc_live_identity_visibility_matches_delegate_projection_labels() {
        assert!(rpc_live_identity_alias_visible("worker", &BTreeMap::new()));

        let mut labels = BTreeMap::new();
        labels.insert("role".to_string(), "delegate".to_string());
        labels.insert("source_mob_id".to_string(), "mob-a".to_string());
        labels.insert("agent_identity".to_string(), "review:singleton".to_string());
        assert!(!rpc_live_identity_alias_visible("worker", &labels));
        assert!(!rpc_live_identity_alias_visible("delegate", &labels));
    }

    #[test]
    fn ambiguous_live_alias_errors_include_structured_data() -> Result<(), serde_json::Error> {
        let response: Value = serde_json::from_str(&error_response(
            json!(1),
            -32602,
            "ambiguous live identity alias review:singleton: candidates [rt:review:singleton:0, rt:review:singleton:1]",
        ))?;

        assert_eq!(
            response["error"]["data"]["kind"],
            json!("ambiguous_live_identity_alias")
        );
        assert_eq!(
            response["error"]["data"]["identity"],
            json!("review:singleton")
        );
        assert_eq!(
            response["error"]["data"]["candidates"],
            json!(["rt:review:singleton:0", "rt:review:singleton:1"])
        );
        Ok(())
    }

    #[test]
    fn wrapped_ambiguous_live_alias_errors_include_structured_data() -> Result<(), serde_json::Error>
    {
        let response: Value = serde_json::from_str(&error_response(
            json!(1),
            -32602,
            "invalid identity: ambiguous live identity alias review:singleton: candidates [rt:review:singleton:0, rt:review:singleton:1]",
        ))?;

        assert_eq!(
            response["error"]["data"]["kind"],
            json!("ambiguous_live_identity_alias")
        );
        assert_eq!(
            response["error"]["data"]["identity"],
            json!("review:singleton")
        );
        assert_eq!(
            response["error"]["data"]["candidates"],
            json!(["rt:review:singleton:0", "rt:review:singleton:1"])
        );
        Ok(())
    }

    #[tokio::test]
    async fn runtime_id_live_only_resolution_rejects_duplicate_projected_identity()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::tempdir()?;
        let runtime = Box::pin(
            UnifiedRuntime::builder()
                .mob_spec(rpc_test_mob_spec(&temp_dir)?)
                .module_config(MobKitConfig {
                    modules: Vec::new(),
                    discovery: DiscoverySpec {
                        namespace: "rpc-identity-alias-test".to_string(),
                        modules: Vec::new(),
                    },
                    pre_spawn: Vec::new(),
                })
                .timeout(Duration::from_secs(1))
                .build(),
        )
        .await?;
        for runtime_id in ["rt:review:singleton:0", "rt:review:singleton:1"] {
            let mut labels = BTreeMap::new();
            labels.insert("agent_identity".to_string(), "review:singleton".to_string());
            runtime
                .spawn(
                    SpawnMemberSpec::from_wire(
                        "worker".to_string(),
                        runtime_id.to_string(),
                        Some("You are a duplicate Review Agent.".into()),
                        None,
                        None,
                    )
                    .with_labels(labels),
                )
                .await?;
        }
        let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "rpc-identity-alias-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        });

        let err =
            resolve_rpc_identity_control_target(&runtime, &identity_rt, "rt:review:singleton:0")
                .await
                .expect_err("runtime-id live-only fallback should reject duplicate durable alias");
        assert!(
            err.contains("ambiguous live identity alias review:singleton"),
            "unexpected error: {err}"
        );

        Ok(())
    }

    #[tokio::test]
    async fn durable_resolution_prefers_registered_live_binding_over_stale_duplicates()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::tempdir()?;
        let runtime = Box::pin(
            UnifiedRuntime::builder()
                .mob_spec(rpc_test_mob_spec(&temp_dir)?)
                .module_config(MobKitConfig {
                    modules: Vec::new(),
                    discovery: DiscoverySpec {
                        namespace: "rpc-identity-alias-test".to_string(),
                        modules: Vec::new(),
                    },
                    pre_spawn: Vec::new(),
                })
                .timeout(Duration::from_secs(1))
                .build(),
        )
        .await?;
        for runtime_id in ["rt:review:singleton:0", "rt:review:singleton:1"] {
            let mut labels = BTreeMap::new();
            labels.insert("agent_identity".to_string(), "review:singleton".to_string());
            runtime
                .spawn(
                    SpawnMemberSpec::from_wire(
                        "worker".to_string(),
                        runtime_id.to_string(),
                        Some("You are a duplicate Review Agent.".into()),
                        None,
                        None,
                    )
                    .with_labels(labels),
                )
                .await?;
        }
        let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "rpc-identity-alias-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        });
        let identity = AgentIdentity::parse("review:singleton")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:1")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(1),
            checkpoint_version: CheckpointVersion::new(0),
        };
        identity_rt
            .register(
                DurableAgentSpec {
                    identity,
                    profile: meerkat_mob::ProfileName::from("worker"),
                    addressability: AgentAddressability::Addressable,
                    display_name: None,
                    labels: BTreeMap::new(),
                    context: None,
                    additional_instructions: Vec::new(),
                    initial_message: None,
                    runtime_mode_override: None,
                    backend: None,
                    binding: None,
                },
                IdentityLifecycleState::Active,
                Some(record),
                None,
            )
            .await;

        let target =
            resolve_rpc_identity_control_target(&runtime, &identity_rt, "review:singleton").await?;
        assert_eq!(target.identity.as_str(), "review:singleton");
        assert_eq!(
            target
                .live
                .as_ref()
                .map(|alias| alias.runtime_member_id.as_str()),
            Some("rt:review:singleton:1")
        );

        let target =
            resolve_rpc_identity_control_target(&runtime, &identity_rt, "rt:review:singleton:1")
                .await?;
        assert_eq!(
            target
                .live
                .as_ref()
                .map(|alias| alias.runtime_member_id.as_str()),
            Some("rt:review:singleton:1")
        );

        Ok(())
    }

    #[tokio::test]
    async fn durable_resolution_rejects_hidden_registered_live_binding()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::tempdir()?;
        let runtime = Box::pin(
            UnifiedRuntime::builder()
                .mob_spec(rpc_test_mob_spec(&temp_dir)?)
                .module_config(MobKitConfig {
                    modules: Vec::new(),
                    discovery: DiscoverySpec {
                        namespace: "rpc-hidden-bound-test".to_string(),
                        modules: Vec::new(),
                    },
                    pre_spawn: Vec::new(),
                })
                .timeout(Duration::from_secs(1))
                .build(),
        )
        .await?;
        runtime
            .spawn(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "rt:review:singleton:0".to_string(),
                    Some("You are a hidden Review Agent.".into()),
                    None,
                    None,
                )
                .with_labels(BTreeMap::from([
                    ("agent_identity".to_string(), "review:singleton".to_string()),
                    ("role".to_string(), "delegate".to_string()),
                    ("source_mob_id".to_string(), "upstream".to_string()),
                ])),
            )
            .await?;
        let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "rpc-hidden-bound-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        });
        let identity = AgentIdentity::parse("review:singleton")?;
        identity_rt
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: meerkat_mob::ProfileName::from("worker"),
                    addressability: AgentAddressability::Addressable,
                    display_name: None,
                    labels: BTreeMap::new(),
                    context: None,
                    additional_instructions: Vec::new(),
                    initial_message: None,
                    runtime_mode_override: None,
                    backend: None,
                    binding: None,
                },
                IdentityLifecycleState::Active,
                Some(ContinuityRecord {
                    identity,
                    agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:0")?,
                    session_id: meerkat_core::types::SessionId::new(),
                    generation: ContinuityGeneration::new(0),
                    checkpoint_version: CheckpointVersion::new(0),
                }),
                None,
            )
            .await;

        for requested_identity in ["review:singleton", "rt:review:singleton:0"] {
            let err =
                resolve_rpc_identity_control_target(&runtime, &identity_rt, requested_identity)
                    .await
                    .expect_err("hidden registered live binding must not resolve");
            assert!(
                err.contains("identity hidden by policy"),
                "unexpected error for {requested_identity}: {err}"
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn live_only_hidden_alias_reports_policy_error()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::tempdir()?;
        let runtime = Box::pin(
            UnifiedRuntime::builder()
                .mob_spec(rpc_test_mob_spec(&temp_dir)?)
                .module_config(MobKitConfig {
                    modules: Vec::new(),
                    discovery: DiscoverySpec {
                        namespace: "rpc-hidden-live-only-test".to_string(),
                        modules: Vec::new(),
                    },
                    pre_spawn: Vec::new(),
                })
                .timeout(Duration::from_secs(1))
                .build(),
        )
        .await?;
        runtime
            .spawn(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "rt:review:singleton:0".to_string(),
                    Some("You are a hidden Review Agent.".into()),
                    None,
                    None,
                )
                .with_labels(BTreeMap::from([
                    ("agent_identity".to_string(), "review:singleton".to_string()),
                    ("role".to_string(), "delegate".to_string()),
                    ("source_mob_id".to_string(), "upstream".to_string()),
                ])),
            )
            .await?;
        let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "rpc-hidden-live-only-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        });

        for requested_identity in ["review:singleton", "rt:review:singleton:0"] {
            let err =
                resolve_rpc_identity_control_target(&runtime, &identity_rt, requested_identity)
                    .await
                    .expect_err("hidden live-only alias must not collapse into unknown identity");
            assert!(
                err.contains("identity hidden by policy"),
                "unexpected error for {requested_identity}: {err}"
            );
        }

        let identity_ctx = IdentityFirstContext {
            runtime: Arc::new(identity_rt),
            roster_provider: Arc::new(EmptyRosterProvider),
            topology_provider: None,
            customizer: None,
        };
        for requested_identity in ["review:singleton", "rt:review:singleton:0"] {
            let response: Value = serde_json::from_str(
                &handle_unified_rpc_json(
                    &runtime,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "mobkit/status_identity",
                        "params": { "identity": requested_identity },
                    })
                    .to_string(),
                    Duration::from_secs(1),
                    None,
                    Some(&identity_ctx),
                )
                .await,
            )?;
            assert_eq!(
                response["error"]["data"]["kind"],
                json!("identity_hidden_by_policy"),
                "unexpected hidden response for {requested_identity}: {response:#?}"
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn live_only_resolution_rejects_runtime_member_bound_to_other_durable_identity()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::tempdir()?;
        let runtime = Box::pin(
            UnifiedRuntime::builder()
                .mob_spec(rpc_test_mob_spec(&temp_dir)?)
                .module_config(MobKitConfig {
                    modules: Vec::new(),
                    discovery: DiscoverySpec {
                        namespace: "rpc-identity-alias-test".to_string(),
                        modules: Vec::new(),
                    },
                    pre_spawn: Vec::new(),
                })
                .timeout(Duration::from_secs(1))
                .build(),
        )
        .await?;
        let mut labels = BTreeMap::new();
        labels.insert("agent_identity".to_string(), "other:singleton".to_string());
        runtime
            .spawn(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "rt:review:singleton:0".to_string(),
                    Some("You are a wrong-projected Review Agent.".into()),
                    None,
                    None,
                )
                .with_labels(labels),
            )
            .await?;

        let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "rpc-identity-alias-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        });
        let identity = AgentIdentity::parse("review:singleton")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:0")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        identity_rt
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: meerkat_mob::ProfileName::from("worker"),
                    addressability: AgentAddressability::Addressable,
                    display_name: None,
                    labels: BTreeMap::new(),
                    context: None,
                    additional_instructions: Vec::new(),
                    initial_message: None,
                    runtime_mode_override: None,
                    backend: None,
                    binding: None,
                },
                IdentityLifecycleState::Active,
                Some(record),
                Some(LeaseGrant {
                    identity,
                    fencing_token: FencingToken::new(1),
                    ttl: Duration::from_mins(1),
                }),
            )
            .await;

        let err = resolve_rpc_identity_control_target(&runtime, &identity_rt, "other:singleton")
            .await
            .expect_err("wrong-projected live alias must not resolve as live-only");
        assert!(
            err.contains("identity runtime binding belongs to review:singleton"),
            "unexpected error: {err}"
        );

        Ok(())
    }

    /// Regression: under the meerkat 0.7.1 identity-first roster the runtime
    /// members are keyed `rt:{identity}:{generation}`, so a gateway-plane
    /// `mobkit/send_message` addressed to the bare durable identity (the
    /// only id the SDK hands out pre-burst) used to fail with
    /// `mob member not found`. Bare identities must bridge-resolve through
    /// the identity runtime — like console send — while an exact roster
    /// member id match keeps raw member-id semantics and wins over identity
    /// resolution.
    #[tokio::test]
    async fn send_message_resolves_bare_durable_identity_through_identity_bridge()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::tempdir()?;
        let runtime = Box::pin(
            UnifiedRuntime::builder()
                .mob_spec(rpc_test_mob_spec(&temp_dir)?)
                .module_config(MobKitConfig {
                    modules: Vec::new(),
                    discovery: DiscoverySpec {
                        namespace: "rpc-send-message-identity-bridge-test".to_string(),
                        modules: Vec::new(),
                    },
                    pre_spawn: Vec::new(),
                })
                .timeout(Duration::from_secs(5))
                .build(),
        )
        .await?;

        // Identity-first roster shape: each durable identity is seated as
        // runtime member rt:{identity}:0; the bare identity is NOT a roster id.
        for (runtime_id, durable) in [
            ("rt:atlas-base-001:0", "atlas-base-001"),
            ("rt:draco-base-001:0", "draco-base-001"),
        ] {
            runtime
                .spawn(
                    SpawnMemberSpec::from_wire(
                        "worker".to_string(),
                        runtime_id.to_string(),
                        Some("You are a swarm base agent.".into()),
                        None,
                        None,
                    )
                    .with_labels(BTreeMap::from([(
                        "agent_identity".to_string(),
                        durable.to_string(),
                    )])),
                )
                .await?;
        }
        // Precedence probe: a bare roster member whose id collides with a
        // registered durable identity.
        runtime
            .spawn(SpawnMemberSpec::from_wire(
                "worker".to_string(),
                "draco-base-001".to_string(),
                Some("You are the raw roster member.".into()),
                None,
                None,
            ))
            .await?;

        let session_service = runtime
            .mob_runtime()
            .session_service()
            .cloned()
            .expect("test mob spec has a session service");
        let bridge: Arc<dyn crate::identity_first::SessionBridge> = Arc::new(
            crate::identity_first::MobSessionBridge::with_session_service(
                runtime.mob_handle(),
                session_service,
            ),
        );
        let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "rpc-send-message-identity-bridge-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge),
            default_timeout: None,
        })
        .with_runtime_services(crate::identity_first::AgentRuntimeServices::new(
            runtime.mob_handle(),
        ));
        for (durable, runtime_id) in [
            ("atlas-base-001", "rt:atlas-base-001:0"),
            ("draco-base-001", "rt:draco-base-001:0"),
        ] {
            let identity = AgentIdentity::parse(durable)?;
            let session_id = runtime
                .mob_handle()
                .resolve_bridge_session_id(&crate::member_comms_id::mob_member_id(runtime_id))
                .await
                .unwrap_or_else(meerkat_core::types::SessionId::new);
            identity_rt
                .register(
                    DurableAgentSpec {
                        identity: identity.clone(),
                        profile: meerkat_mob::ProfileName::from("worker"),
                        addressability: AgentAddressability::Addressable,
                        display_name: None,
                        labels: BTreeMap::new(),
                        context: None,
                        additional_instructions: Vec::new(),
                        initial_message: None,
                        runtime_mode_override: None,
                        backend: None,
                        binding: None,
                    },
                    IdentityLifecycleState::Active,
                    Some(ContinuityRecord {
                        identity: identity.clone(),
                        agent_runtime_id: AgentRuntimeId::parse(runtime_id)?,
                        session_id,
                        generation: ContinuityGeneration::new(0),
                        checkpoint_version: CheckpointVersion::new(0),
                    }),
                    Some(LeaseGrant {
                        identity,
                        fencing_token: FencingToken::new(1),
                        ttl: Duration::from_mins(1),
                    }),
                )
                .await;
        }
        let identity_ctx = IdentityFirstContext {
            runtime: Arc::new(identity_rt),
            roster_provider: Arc::new(EmptyRosterProvider),
            topology_provider: None,
            customizer: None,
        };

        let send = |id: u64, params: Value| {
            let runtime = &runtime;
            let identity_ctx = &identity_ctx;
            async move {
                let raw = handle_unified_rpc_json(
                    runtime,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "method": "mobkit/send_message",
                        "params": params,
                    })
                    .to_string(),
                    Duration::from_secs(10),
                    None,
                    Some(identity_ctx),
                )
                .await;
                serde_json::from_str::<Value>(&raw)
            }
        };

        // 1. Bare durable identity bridges to the rt:{identity}:{generation}
        //    member and reports the bridge session that took the delivery.
        let response = send(
            1,
            json!({ "member_id": "atlas-base-001", "message": "status check" }),
        )
        .await?;
        assert!(
            response["error"].is_null(),
            "bare identity send must bridge-resolve: {response:#?}"
        );
        assert_eq!(response["result"]["accepted"], json!(true));
        assert_eq!(response["result"]["member_id"], json!("atlas-base-001"));
        let atlas_session = runtime
            .mob_handle()
            .resolve_bridge_session_id(&crate::member_comms_id::mob_member_id(
                "rt:atlas-base-001:0",
            ))
            .await
            .expect("atlas runtime member has a bridge session after send")
            .to_string();
        assert_eq!(response["result"]["session_id"], json!(atlas_session));

        // 2. Steer rides the same bridge resolution.
        let response = send(
            2,
            json!({
                "member_id": "atlas-base-001",
                "message": "steer: stand down",
                "handling_mode": "steer",
            }),
        )
        .await?;
        assert!(
            response["error"].is_null(),
            "bare identity steer must bridge-resolve: {response:#?}"
        );
        assert_eq!(response["result"]["accepted"], json!(true));

        // 3. Precedence: an exact roster member id wins over identity
        //    resolution — the bare member takes the delivery, not the
        //    identity's rt:draco-base-001:0 binding.
        let response = send(
            3,
            json!({ "member_id": "draco-base-001", "message": "raw roster delivery" }),
        )
        .await?;
        assert!(
            response["error"].is_null(),
            "exact roster member send must keep raw semantics: {response:#?}"
        );
        assert_eq!(response["result"]["accepted"], json!(true));
        let draco_raw_session = runtime
            .mob_handle()
            .resolve_bridge_session_id(&crate::member_comms_id::mob_member_id("draco-base-001"))
            .await
            .expect("bare draco member has a bridge session after send")
            .to_string();
        assert_eq!(response["result"]["session_id"], json!(draco_raw_session));
        if let Some(draco_rt_session) = runtime
            .mob_handle()
            .resolve_bridge_session_id(&crate::member_comms_id::mob_member_id(
                "rt:draco-base-001:0",
            ))
            .await
        {
            assert_ne!(
                response["result"]["session_id"],
                json!(draco_rt_session.to_string()),
                "exact member id match must not be shadowed by identity resolution"
            );
        }

        // 4. Unknown ids keep raw member-not-found semantics.
        let response = send(
            4,
            json!({ "member_id": "phantom-base-999", "message": "nobody home" }),
        )
        .await?;
        assert_eq!(response["error"]["code"], json!(-32000), "{response:#?}");
        let message = response["error"]["message"]
            .as_str()
            .expect("error message");
        assert!(
            message.starts_with("send_message failed:"),
            "unexpected error message: {message}"
        );

        Ok(())
    }
}
