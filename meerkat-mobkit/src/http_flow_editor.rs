use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, Uri, header},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::Value;

use crate::http_sse::sse_request_authorized;
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

pub fn protected_flow_editor_router(decisions: RuntimeDecisionState) -> Router {
    flow_editor_frontend_router::<()>().merge(flow_editor_rpc_router_with_decisions(decisions))
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

pub fn flow_editor_rpc_router_with_decisions(decisions: RuntimeDecisionState) -> Router {
    Router::new()
        .route("/flow-editor/rpc", post(protected_flow_editor_rpc_handler))
        .with_state(FlowEditorRpcState { decisions })
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
            mode: "none",
            reason: "standalone Flow Editor authoring server",
        },
    );
    (StatusCode::OK, Json::<Value>(response))
}

#[derive(Clone)]
struct FlowEditorRpcState {
    decisions: RuntimeDecisionState,
}

#[derive(Clone, Copy)]
struct FlowEditorAuthReport {
    authenticated: bool,
    mode: &'static str,
    reason: &'static str,
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
        },
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
        },
    )
}

fn handle_flow_editor_rpc_with_auth(request: JsonRpcRequest, auth: FlowEditorAuthReport) -> Value {
    let response_id = request.id.clone().unwrap_or(Value::Null);
    match request.method.as_str() {
        "mobkit/capabilities" => {
            let mut methods = vec!["mobkit/capabilities"];
            methods.extend_from_slice(crate::rpc::MOBPACK_AUTHORING_METHODS);
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
                    "authoring_capabilities": crate::rpc::mobpack_authoring_capabilities(),
                })),
                None,
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
