use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, Uri, header},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::Value;

use crate::http_sse::sse_request_authorized;
use crate::mobpack::MobpackRuntimeCatalogState;
use crate::rpc::{JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::runtime::RuntimeDecisionState;

const FLOW_EDITOR_FRONTEND_INDEX_HTML: &str = include_str!("../flow-editor-dist/index.html");
const FLOW_EDITOR_FRONTEND_VENDOR_JS: &str = include_str!("../flow-editor-dist/react-globals.js");
const FLOW_EDITOR_FRONTEND_APP_JS: &str = include_str!("../flow-editor-dist/flow-editor.js");
const FLOW_EDITOR_FRONTEND_APP_CSS: &str = include_str!("../flow-editor-dist/flow-editor.css");

/// Standalone HTTP surface for the MobKit Flow Editor.
///
/// This route plane deliberately serves only the editor shell and the
/// mobpack-authoring JSON-RPC methods. Console runtime methods remain on the
/// console RPC plane.
pub fn flow_editor_router() -> Router {
    flow_editor_frontend_router::<()>().merge(flow_editor_rpc_router::<()>())
}

pub fn flow_editor_router_with_host_deploy() -> Router {
    flow_editor_frontend_router::<()>().merge(flow_editor_rpc_router_allowing_host_deploy::<()>())
}

pub fn protected_flow_editor_router(decisions: RuntimeDecisionState) -> Router {
    flow_editor_frontend_router::<()>().merge(flow_editor_rpc_router_with_decisions(decisions))
}

pub(crate) fn protected_flow_editor_router_with_runtime_catalog(
    decisions: RuntimeDecisionState,
    runtime_catalog: MobpackRuntimeCatalogState,
) -> Router {
    flow_editor_frontend_router::<()>().merge(flow_editor_rpc_router_with_runtime_catalog(
        decisions,
        Some(runtime_catalog),
    ))
}

pub fn flow_editor_frontend_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/flow-editor", get(flow_editor_frontend_index_handler))
        .route("/flow-editor/", get(flow_editor_frontend_index_handler))
        .route(
            "/flow-editor/assets/react-globals.js",
            get(flow_editor_frontend_vendor_js_handler),
        )
        .route(
            "/flow-editor/assets/flow-editor.js",
            get(flow_editor_frontend_app_js_handler),
        )
        .route(
            "/flow-editor/assets/flow-editor.css",
            get(flow_editor_frontend_app_css_handler),
        )
}

pub fn flow_editor_rpc_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/flow-editor/rpc", post(flow_editor_rpc_handler))
}

pub fn flow_editor_rpc_router_allowing_host_deploy<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route(
        "/flow-editor/rpc",
        post(flow_editor_rpc_handler_allowing_host_deploy),
    )
}

pub fn flow_editor_rpc_router_with_decisions(decisions: RuntimeDecisionState) -> Router {
    flow_editor_rpc_router_with_runtime_catalog(decisions, None)
}

fn flow_editor_rpc_router_with_runtime_catalog(
    decisions: RuntimeDecisionState,
    runtime_catalog: Option<MobpackRuntimeCatalogState>,
) -> Router {
    Router::new()
        .route("/flow-editor/rpc", post(protected_flow_editor_rpc_handler))
        .with_state(FlowEditorRpcState {
            decisions,
            runtime_catalog,
        })
}

pub async fn flow_editor_frontend_index_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        FLOW_EDITOR_FRONTEND_INDEX_HTML,
    )
}

pub async fn flow_editor_frontend_vendor_js_handler() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        FLOW_EDITOR_FRONTEND_VENDOR_JS,
    )
}

pub async fn flow_editor_frontend_app_js_handler() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        FLOW_EDITOR_FRONTEND_APP_JS,
    )
}

pub async fn flow_editor_frontend_app_css_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        FLOW_EDITOR_FRONTEND_APP_CSS,
    )
}

pub async fn flow_editor_rpc_handler(Json(request): Json<Value>) -> impl IntoResponse {
    flow_editor_rpc_handler_with_policy(request, false)
}

pub async fn flow_editor_rpc_handler_allowing_host_deploy(
    Json(request): Json<Value>,
) -> impl IntoResponse {
    flow_editor_rpc_handler_with_policy(request, true)
}

fn flow_editor_rpc_handler_with_policy(
    request: Value,
    allow_host_deploy: bool,
) -> impl IntoResponse {
    let parsed_request = match serde_json::from_value::<JsonRpcRequest>(request) {
        Ok(req) => req,
        Err(_) => {
            return (
                StatusCode::OK,
                Json::<Value>(serde_json::json!({
                    "jsonrpc": JSONRPC_VERSION,
                    "id": Value::Null,
                    "error": { "code": -32600, "message": "Invalid Request" }
                })),
            );
        }
    };
    let response = handle_flow_editor_rpc_with_auth(
        parsed_request,
        FlowEditorAuthReport {
            authenticated: false,
            mode: if allow_host_deploy {
                "standalone_host_deploy"
            } else {
                "none"
            },
            reason: if allow_host_deploy {
                "standalone Flow Editor authoring server with explicit host deploy opt-in"
            } else {
                "standalone Flow Editor authoring server"
            },
            host_mutation_allowed: allow_host_deploy,
            deploy_execute_allowed: allow_host_deploy,
        },
        None,
    );
    (StatusCode::OK, Json::<Value>(response))
}

#[derive(Clone)]
struct FlowEditorRpcState {
    decisions: RuntimeDecisionState,
    runtime_catalog: Option<MobpackRuntimeCatalogState>,
}

#[derive(Clone, Copy)]
struct FlowEditorAuthReport {
    authenticated: bool,
    mode: &'static str,
    reason: &'static str,
    host_mutation_allowed: bool,
    deploy_execute_allowed: bool,
}

async fn protected_flow_editor_rpc_handler(
    State(state): State<FlowEditorRpcState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    let parsed_request = match serde_json::from_value::<JsonRpcRequest>(request) {
        Ok(req) => req,
        Err(_) => {
            return (
                StatusCode::OK,
                Json::<Value>(serde_json::json!({
                    "jsonrpc": JSONRPC_VERSION,
                    "id": Value::Null,
                    "error": { "code": -32600, "message": "Invalid Request" }
                })),
            );
        }
    };
    if !sse_request_authorized(Some(&state.decisions), &headers, &uri) {
        return (
            StatusCode::UNAUTHORIZED,
            Json::<Value>(serde_json::json!({
                "jsonrpc": JSONRPC_VERSION,
                "id": parsed_request.id.unwrap_or(Value::Null),
                "error": {
                    "code": -32600,
                    "message": "unauthorized: flow editor rpc requires a valid auth token",
                }
            })),
        );
    }
    let response = handle_flow_editor_rpc_with_auth(
        parsed_request,
        FlowEditorAuthReport {
            authenticated: true,
            mode: "reference_app",
            reason: "reference app Flow Editor authoring server",
            host_mutation_allowed: true,
            deploy_execute_allowed: true,
        },
        state.runtime_catalog.as_ref(),
    );
    (StatusCode::OK, Json::<Value>(response))
}

pub fn handle_flow_editor_rpc(request: JsonRpcRequest) -> Value {
    handle_flow_editor_rpc_with_auth(
        request,
        FlowEditorAuthReport {
            authenticated: false,
            mode: "none",
            reason: "standalone Flow Editor authoring server",
            host_mutation_allowed: false,
            deploy_execute_allowed: false,
        },
        None,
    )
}

fn handle_flow_editor_rpc_with_auth(
    request: JsonRpcRequest,
    auth: FlowEditorAuthReport,
    runtime_catalog: Option<&MobpackRuntimeCatalogState>,
) -> Value {
    let response_id = request.id.clone().unwrap_or(Value::Null);
    match request.method.as_str() {
        "mobkit/capabilities" => {
            let mut methods = vec!["mobkit/capabilities"];
            methods.extend_from_slice(crate::rpc::MOBPACK_AUTHORING_METHODS);
            let mut authoring_capabilities = crate::rpc::mobpack_authoring_capabilities();
            authoring_capabilities["host_mutation_allowed"] =
                serde_json::json!(auth.host_mutation_allowed);
            authoring_capabilities["deploy_execute_allowed"] =
                serde_json::json!(auth.deploy_execute_allowed);
            authoring_capabilities["runtime_backed_catalogs"] =
                serde_json::json!(runtime_catalog.is_some());
            response_value(
                response_id,
                Some(serde_json::json!({
                    "methods": methods,
                    "authenticated": auth.authenticated,
                    "auth": {
                        "mode": auth.mode,
                        "reason": auth.reason
                    },
                    "features": {
                        "flow_editor": true,
                        "mobpack_authoring": true,
                    },
                    "authoring_capabilities": authoring_capabilities,
                })),
                None,
            )
        }
        "mobkit/mobpacks/catalogs" => response_value(
            response_id,
            Some(crate::mobpack::mobpack_catalogs_response_with_runtime(
                runtime_catalog,
            )),
            None,
        ),
        "mobkit/tools/catalog" => response_value(
            response_id,
            Some(crate::mobpack::mobpack_tools_catalog_response_with_runtime(
                runtime_catalog,
            )),
            None,
        ),
        "mobkit/skills/catalog" => response_value(
            response_id,
            Some(crate::mobpack::mobpack_skills_catalog_response_with_runtime(
                runtime_catalog,
            )),
            None,
        ),
        "mobkit/agent_definitions/list" => response_value(
            response_id,
            Some(
                crate::mobpack::mobpack_agent_definitions_response_with_runtime(runtime_catalog),
            ),
            None,
        ),
        "mobkit/mobpacks/templates" => response_value(
            response_id,
            Some(crate::mobpack::mobpack_templates_response_with_runtime(
                runtime_catalog,
            )),
            None,
        ),
        "mobkit/mobpacks/deploy"
            if !auth.deploy_execute_allowed && deploy_execute_requested(&request.params) =>
        {
            response_value(
                response_id,
                None,
                Some(JsonRpcError {
                    code: -32602,
                    message: "standalone Flow Editor RPC cannot execute host deploys; use deploy planning or run rkat mob deploy manually".to_string(),
                    data: Some(serde_json::json!({
                        "method": "mobkit/mobpacks/deploy",
                        "execute": true,
                        "deploy_command": "rkat mob deploy"
                    })),
                }),
            )
        }
        method if crate::rpc::MOBPACK_AUTHORING_METHODS.contains(&method) => serde_json::to_value(
            crate::rpc::handle_mobpack_authoring_rpc(method, &request.params, response_id)
                .expect("known mobpack authoring method"),
        )
        .unwrap_or_else(|_| {
            serde_json::json!({
                "jsonrpc": JSONRPC_VERSION,
                "id": Value::Null,
                "error": {
                    "code": -32603,
                    "message": "serialization failed",
                }
            })
        }),
        other => response_value(
            response_id,
            None,
            Some(JsonRpcError {
                code: -32601,
                message: format!("method not found on flow editor rpc: {other}"),
                data: None,
            }),
        ),
    }
}

fn deploy_execute_requested(params: &Value) -> bool {
    params
        .get("execute")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn response_value(id: Value, result: Option<Value>, error: Option<JsonRpcError>) -> Value {
    serde_json::to_value(JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id,
        result,
        error,
    })
    .unwrap_or_else(|_| {
        serde_json::json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": Value::Null,
            "error": {
                "code": -32603,
                "message": "serialization failed",
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::rpc::JsonRpcRequest;

    #[test]
    fn flow_editor_rpc_exposes_only_mobpack_authoring_methods() {
        let response = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "mobkit/capabilities".to_string(),
            params: Value::Null,
        });
        let methods = response["result"]["methods"]
            .as_array()
            .expect("methods array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let mut expected_methods = vec!["mobkit/capabilities"];
        expected_methods.extend_from_slice(crate::rpc::MOBPACK_AUTHORING_METHODS);
        assert_eq!(methods, expected_methods);
        assert!(!methods.contains(&"mobkit/console/send"));
        assert_eq!(response["result"]["authenticated"], json!(false));
        assert_eq!(response["result"]["auth"]["mode"], json!("none"));
        assert_eq!(
            response["result"]["authoring_capabilities"]["domain"],
            json!("mobpack_authoring")
        );
        assert_eq!(
            response["result"]["authoring_capabilities"]["runtime_mutation"],
            json!(false)
        );
        assert_eq!(
            response["result"]["authoring_capabilities"]["host_mutation_allowed"],
            json!(false)
        );
        assert_eq!(
            response["result"]["authoring_capabilities"]["deploy_execute_allowed"],
            json!(false)
        );
        assert_eq!(
            response["result"]["authoring_capabilities"]["deploy_command"],
            json!("rkat mob deploy")
        );
        assert_eq!(
            response["result"]["authoring_capabilities"]["methods"]
                .as_array()
                .expect("authoring methods")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            crate::rpc::MOBPACK_AUTHORING_METHODS
        );
    }

    #[test]
    fn standalone_flow_editor_rpc_rejects_host_deploy_execution() {
        let catalogs = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "mobkit/mobpacks/catalogs".to_string(),
            params: Value::Null,
        });
        assert_eq!(catalogs["result"]["runtime_backed"], json!(false));
        assert_eq!(
            catalogs["result"]["authoring_provider"]["runtime_binding"],
            json!("unbound")
        );
        let sample = catalogs["result"]["sample_mobpacks"]
            .as_array()
            .expect("sample mobpacks")
            .iter()
            .find(|sample| sample["id"] == "sample_docs_only")
            .expect("docs sample");

        let response = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "mobkit/mobpacks/deploy".to_string(),
            params: json!({
                "document": sample["document"].clone(),
                "prompt": "Reply with exactly OK.",
                "execute": true
            }),
        });

        assert_eq!(response["error"]["code"], json!(-32602));
        assert!(
            response["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("cannot execute host deploys")),
            "{response:#?}"
        );
        assert_eq!(
            response["error"]["data"]["deploy_command"],
            json!("rkat mob deploy")
        );
    }

    #[test]
    fn protected_flow_editor_rpc_returns_runtime_bound_catalogs_when_state_is_available() {
        let response = super::handle_flow_editor_rpc_with_auth(
            JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(json!(1)),
                method: "mobkit/mobpacks/catalogs".to_string(),
                params: Value::Null,
            },
            super::FlowEditorAuthReport {
                authenticated: true,
                mode: "reference_app",
                reason: "reference app Flow Editor authoring server",
                host_mutation_allowed: true,
                deploy_execute_allowed: true,
            },
            Some(&crate::mobpack::MobpackRuntimeCatalogState {
                loaded_modules: vec!["worker".to_string()],
                runtime_methods: vec!["mobkit/mobpacks/deploy".to_string()],
                has_contact_directory: true,
                has_peer_mob_handles: false,
                has_inproc_contacts: false,
            }),
        );

        assert!(response["error"].is_null(), "{response:#?}");
        assert_eq!(response["result"]["runtime_backed"], json!(true));
        assert_eq!(
            response["result"]["authoring_provider"]["id"],
            json!("unified_runtime")
        );
        assert_eq!(
            response["result"]["authoring_provider"]["runtime_binding"],
            json!("bound")
        );
        assert_eq!(
            response["result"]["authoring_provider"]["loaded_modules"],
            json!(["worker"])
        );
        assert_eq!(
            response["result"]["authoring_provider"]["cross_mob"]["contact_directory"],
            json!(true)
        );
        assert_eq!(
            response["result"]["catalog_snapshot"]["runtime_backed"],
            json!(true)
        );
    }

    #[cfg(unix)]
    #[test]
    fn standalone_flow_editor_rpc_executes_host_deploy_when_explicitly_enabled() {
        let catalogs = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "mobkit/mobpacks/catalogs".to_string(),
            params: Value::Null,
        });
        let sample = catalogs["result"]["sample_mobpacks"]
            .as_array()
            .expect("sample mobpacks")
            .iter()
            .find(|sample| sample["id"] == "sample_docs_only")
            .expect("docs sample");
        let dir = tempfile::tempdir().expect("tempdir");
        let fake_rkat = dir.path().join("rkat");
        let args_file = dir.path().join("rkat.args");
        std::fs::write(
            &fake_rkat,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\necho flow-editor-rkat-ok\n",
                args_file.to_string_lossy()
            ),
        )
        .expect("write fake rkat");
        let mut permissions = std::fs::metadata(&fake_rkat)
            .expect("fake rkat metadata")
            .permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_rkat, permissions).expect("chmod fake rkat");

        let capabilities = super::handle_flow_editor_rpc_with_auth(
            JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(json!(2)),
                method: "mobkit/capabilities".to_string(),
                params: Value::Null,
            },
            super::FlowEditorAuthReport {
                authenticated: false,
                mode: "standalone_host_deploy",
                reason: "standalone Flow Editor authoring server with explicit host deploy opt-in",
                host_mutation_allowed: true,
                deploy_execute_allowed: true,
            },
            None,
        );
        assert_eq!(
            capabilities["result"]["authoring_capabilities"]["host_mutation_allowed"],
            json!(true)
        );
        assert_eq!(
            capabilities["result"]["authoring_capabilities"]["deploy_execute_allowed"],
            json!(true)
        );

        let response = super::handle_flow_editor_rpc_with_auth(
            JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(json!(3)),
                method: "mobkit/mobpacks/deploy".to_string(),
                params: json!({
                    "document": sample["document"].clone(),
                    "output_dir": dir.path(),
                    "prompt": "Reply with exactly OK.",
                    "rkat_bin": fake_rkat,
                    "execute": true
                }),
            },
            super::FlowEditorAuthReport {
                authenticated: false,
                mode: "standalone_host_deploy",
                reason: "standalone Flow Editor authoring server with explicit host deploy opt-in",
                host_mutation_allowed: true,
                deploy_execute_allowed: true,
            },
            None,
        );

        assert!(response["error"].is_null(), "{response:#?}");
        assert_eq!(response["result"]["executed"], json!(true));
        assert_eq!(response["result"]["success"], json!(true));
        assert_eq!(response["result"]["status_code"], json!(0));
        assert!(
            response["result"]["stdout"]
                .as_str()
                .is_some_and(|stdout| stdout.contains("flow-editor-rkat-ok")),
            "{response:#?}"
        );
        let argv = std::fs::read_to_string(args_file).expect("recorded fake rkat args");
        assert!(argv.lines().any(|line| line == "mob"));
        assert!(argv.lines().any(|line| line == "deploy"));
        assert!(argv.lines().any(|line| line == "Reply with exactly OK."));
    }

    #[test]
    fn flow_editor_rpc_plans_real_sample_mobpack_deploy() {
        let catalogs = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "mobkit/mobpacks/catalogs".to_string(),
            params: Value::Null,
        });
        let sample = catalogs["result"]["sample_mobpacks"]
            .as_array()
            .expect("sample mobpacks")
            .iter()
            .find(|sample| sample["id"] == "sample_docs_only")
            .expect("docs sample");

        let response = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "mobkit/mobpacks/deploy".to_string(),
            params: json!({
                "document": sample["document"].clone(),
                "prompt": "Reply with exactly OK."
            }),
        });

        assert!(response["error"].is_null(), "{response:#?}");
        assert_eq!(
            &response["result"]["argv"].as_array().expect("argv")[0..3],
            [json!("rkat"), json!("mob"), json!("deploy")]
        );
        assert!(
            response["result"]["plan_trace"]
                .as_array()
                .expect("plan trace")
                .iter()
                .any(|row| row["head"]
                    .as_str()
                    .is_some_and(|head| head.starts_with("PROFILE ·"))),
            "{response:#?}"
        );
    }

    #[test]
    fn flow_editor_rpc_previews_document_backed_deploy_command() {
        let catalogs = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "mobkit/mobpacks/catalogs".to_string(),
            params: Value::Null,
        });
        let sample = catalogs["result"]["sample_mobpacks"]
            .as_array()
            .expect("sample mobpacks")
            .iter()
            .find(|sample| sample["id"] == "sample_docs_only")
            .expect("docs sample");
        let response = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "mobkit/mobpacks/deploy_command".to_string(),
            params: json!({
                "document": sample["document"].clone(),
                "prompt": "Preview prompt."
            }),
        });

        assert!(response["error"].is_null(), "{response:#?}");
        assert_eq!(
            &response["result"]["argv"].as_array().expect("argv")[0..3],
            [json!("rkat"), json!("mob"), json!("deploy")]
        );
        assert_eq!(
            response["result"]["source"],
            json!("meerkat_mobkit::mobpack::deploy_argv")
        );
        assert_eq!(response["result"]["filename"], json!("docs-only.mobpack"));
        assert_eq!(response["result"]["validation"]["ok"], json!(true));
        assert!(
            response["result"]["command"]
                .as_str()
                .is_some_and(|command| command.contains("docs-only.mobpack")
                    && command.contains("Preview prompt.")),
            "{response:#?}"
        );

        let rejected = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(3)),
            method: "mobkit/mobpacks/deploy_command".to_string(),
            params: json!({
                "deploy": { "command": "rkat mob deploy" },
                "pack_path": "<pack.mobpack>"
            }),
        });
        assert!(
            rejected["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("requires document")),
            "{rejected:#?}"
        );
    }

    #[test]
    fn flow_editor_rpc_previews_source_without_exporting_archive_payload() {
        let catalogs = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "mobkit/mobpacks/catalogs".to_string(),
            params: Value::Null,
        });
        let sample = catalogs["result"]["sample_mobpacks"]
            .as_array()
            .expect("sample mobpacks")
            .iter()
            .find(|sample| sample["id"] == "sample_docs_only")
            .expect("docs sample");

        let response = super::handle_flow_editor_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "mobkit/mobpacks/source".to_string(),
            params: json!({ "document": sample["document"].clone() }),
        });

        assert!(response["error"].is_null(), "{response:#?}");
        assert_eq!(
            response["result"]["source"],
            json!("mobkit/mobpacks/source")
        );
        assert!(
            response["result"].get("content_base64").is_none(),
            "{response:#?}"
        );
        let source_files = response["result"]["source_files"]
            .as_array()
            .expect("source files");
        assert!(
            source_files
                .iter()
                .any(|file| file["path"] == "mobkit/mob.toml"
                    && file["text"]
                        .as_str()
                        .is_some_and(|text| text.contains("[mob]"))),
            "{response:#?}"
        );
    }
}
