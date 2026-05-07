//! HTTP routes for the admin console REST API.

use async_stream::stream;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use futures::future::join_all;
use meerkat_core::ContentInput;
use meerkat_core::comms::TrustedPeerDescriptor;
use meerkat_mob::MobState;
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::launch::MemberLaunchMode;
use meerkat_mob::runtime::reconcile::MemberFilter;
use meerkat_mob::{MobHandle, PeerTarget, ProfileName, SpawnMemberSpec};

use crate::mob_handle_runtime::{
    assert_member_accepts_images, member_entry_to_json, model_capabilities_for_member,
    send_message_on_mob_with_mode,
};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::warn;

use crate::blob_store::is_valid_blob_id_value;
use crate::console_contracts::{
    ALL_EVENTS_CONTROL_IDENTITY, ALL_EVENTS_STREAM_NAME, ConsoleIdentityEventEnvelope,
    IDENTITY_STREAM_NAME, IdentityStreamRequest, ReplayUnavailableError,
};
use crate::contact_directory::ContactDirectory;
use crate::http_sse::{DEFAULT_KEEP_ALIVE_INTERVAL, KEEP_ALIVE_TEXT};
use crate::mob_handle_runtime::{MEMBER_STATE_ACTIVE, MEMBER_STATE_RETIRING, MobRuntime};
use crate::rpc::{JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::runtime::MobkitRuntimeHandle;
use crate::runtime::{
    ConsoleAgentLiveSnapshot, ConsoleLiveSnapshot, ConsoleRestJsonRequest, DeliveryHistoryRequest,
    GatingDecideRequest, GatingDecision, RuntimeDecisionState, extract_bearer_token_from_header,
    handle_console_rest_json_route_with_snapshot, validate_console_token,
};
use crate::runtime::{MetadataScope, RuntimeMetadataTable, labels_to_json_value};
use crate::unified_runtime::console_events::{
    ConsoleEventStore, is_empty_web_search_annotations_event,
};
use crate::unified_runtime::mob_events::MobEventsStore;
use crate::unified_runtime::{EventLogStore, EventQuery, PersistedEvent};

#[derive(Clone)]
pub struct ConsoleJsonState {
    pub decisions: RuntimeDecisionState,
    pub runtime: Option<MobRuntime>,
    pub module_runtime: Option<std::sync::Arc<tokio::sync::Mutex<MobkitRuntimeHandle>>>,
    pub contact_directory: Option<ContactDirectory>,
    pub event_log: Option<std::sync::Arc<dyn EventLogStore>>,
    /// Local gateway signing identity. Plumbed in so the console RPC
    /// dispatch can answer `mobkit/peer_pubkey` and stamp non-inproc
    /// `cross_mob/wire_local` descriptors with a real pubkey.
    pub gateway_peer_keys: Option<crate::auth::peer_keys::GatewayPeerKeys>,
    pub(crate) console_events: Option<ConsoleEventStore>,
    pub(crate) mob_events: Option<MobEventsStore>,
    pub(crate) stream_routes_enabled: bool,
    pub(crate) metadata_table: Option<std::sync::Arc<RuntimeMetadataTable>>,
}

const CONSOLE_FRONTEND_INDEX_HTML: &str = include_str!("../console-dist/index.html");
const CONSOLE_FRONTEND_APP_JS: &str = include_str!("../console-dist/console-app.js");
const CONSOLE_FRONTEND_APP_CSS: &str = include_str!("../console-dist/console-app.css");
static CONSOLE_INTERACTION_COUNTER: AtomicU64 = AtomicU64::new(1);
const MAX_MULTIPART_IMAGE_BYTES: usize = 25 * 1024 * 1024;
const MAX_MULTIPART_IMAGES: usize = 4;
const MAX_MULTIPART_BODY_BYTES: usize =
    (MAX_MULTIPART_IMAGE_BYTES * MAX_MULTIPART_IMAGES) + 1024 * 1024;

pub fn console_json_router(decisions: RuntimeDecisionState) -> Router {
    console_json_router_with_state(ConsoleJsonState {
        decisions,
        runtime: None,
        module_runtime: None,
        contact_directory: None,
        event_log: None,
        gateway_peer_keys: None,
        console_events: None,
        mob_events: None,
        stream_routes_enabled: true,
        metadata_table: None,
    })
}

pub fn console_json_router_with_runtime(
    decisions: RuntimeDecisionState,
    runtime: MobRuntime,
    contact_directory: Option<ContactDirectory>,
    event_log: Option<std::sync::Arc<dyn EventLogStore>>,
) -> Router {
    console_json_router_with_runtime_and_events(
        decisions,
        runtime,
        None,
        contact_directory,
        event_log,
        None,
        None,
        None,
        false,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn console_json_router_with_runtime_and_events(
    decisions: RuntimeDecisionState,
    runtime: MobRuntime,
    module_runtime: Option<std::sync::Arc<tokio::sync::Mutex<MobkitRuntimeHandle>>>,
    contact_directory: Option<ContactDirectory>,
    event_log: Option<std::sync::Arc<dyn EventLogStore>>,
    gateway_peer_keys: Option<crate::auth::peer_keys::GatewayPeerKeys>,
    console_events: Option<ConsoleEventStore>,
    mob_events: Option<MobEventsStore>,
    stream_routes_enabled: bool,
    metadata_table: Option<std::sync::Arc<RuntimeMetadataTable>>,
) -> Router {
    console_json_router_with_state(ConsoleJsonState {
        decisions,
        runtime: Some(runtime),
        module_runtime,
        contact_directory,
        event_log,
        gateway_peer_keys,
        console_events,
        mob_events,
        stream_routes_enabled,
        metadata_table,
    })
}

pub fn console_frontend_router() -> Router {
    Router::new()
        .route("/console", get(console_frontend_index_handler))
        .route("/console/", get(console_frontend_index_handler))
        .route(
            "/console/assets/console-app.js",
            get(console_frontend_app_js_handler),
        )
        .route(
            "/console/assets/console-app.css",
            get(console_frontend_app_css_handler),
        )
}

fn console_json_router_with_state(state: ConsoleJsonState) -> Router {
    let router = Router::new()
        .route("/console/experience", get(console_json_handler))
        .route("/console/modules", get(console_json_handler))
        .route("/console/rpc", post(console_rpc_handler))
        .route(
            "/console/rpc/multipart",
            post(console_rpc_multipart_handler)
                .layer(DefaultBodyLimit::max(MAX_MULTIPART_BODY_BYTES)),
        )
        .route("/blobs/{blob_id}", get(blob_get_handler));
    let router = if state.stream_routes_enabled {
        router
            .route(
                "/console/identity/stream",
                post(console_identity_stream_handler),
            )
            .route("/console/events/stream", get(console_events_stream_handler))
    } else {
        router
    };
    router.with_state(state)
}

pub async fn console_json_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
) -> impl IntoResponse {
    let mut path = uri
        .path_and_query()
        .map(|path_and_query| path_and_query.as_str().to_string())
        .unwrap_or_else(|| uri.path().to_string());

    // If the request carries a Bearer token and the URL doesn't already have
    // an auth_token query param, inject it so the console-ingress auth
    // resolver can validate it through the existing query-param path.
    //
    // JWT tokens use base64url characters (A-Za-z0-9_-.) plus optional '='
    // padding. We percent-encode the bearer when injecting so opaque
    // bearer tokens containing `&`, `=`, `+`, `%`, etc. (legal under
    // RFC 6750 §2.1) survive the round trip — pre-fix, an `&` made
    // injection skip and authentication fail. Substring detection of
    // an existing `auth_token=` is now key-aware via form_urlencoded
    // so `xauth_token=` doesn't masquerade as the real key.
    let already_has_token = path
        .split_once('?')
        .map(|(_, q)| form_urlencoded::parse(q.as_bytes()).any(|(key, _)| key == "auth_token"))
        .unwrap_or(false);
    if !already_has_token
        && let Some(bearer) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(extract_bearer_token_from_header)
    {
        let encoded: String = form_urlencoded::byte_serialize(bearer.as_bytes()).collect();
        let sep = if path.contains('?') { '&' } else { '?' };
        path = format!("{path}{sep}auth_token={encoded}");
    }

    let config_module_ids: Vec<String> = state
        .decisions
        .modules
        .iter()
        .map(|m| m.id.clone())
        .collect();
    let live_snapshot = match &state.runtime {
        Some(runtime) => Some(
            build_live_snapshot(runtime, &config_module_ids, state.console_events.as_ref()).await,
        ),
        None => None,
    };

    let response = handle_console_rest_json_route_with_snapshot(
        &state.decisions,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path,
            auth: None,
        },
        live_snapshot.as_ref(),
    );
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json::<Value>(response.body))
}

pub async fn console_rpc_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    // Parse the request early so we can check the method for auth gating.
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

    // Auth enforcement:
    // - When require_app_auth is true: validate bearer token (OIDC + allowlist)
    // - When require_app_auth is false: only allow read-only methods
    //   (mutating operations require auth to be configured)
    if !console_request_authorized(&state, &headers, &uri) {
        return (
            StatusCode::UNAUTHORIZED,
            Json::<Value>(serde_json::json!({
                "jsonrpc": JSONRPC_VERSION,
                "id": parsed_request.id.unwrap_or(Value::Null),
                "error": {
                    "code": -32600,
                    "message": "unauthorized: console rpc requires a valid auth token",
                }
            })),
        );
    }
    // No auth configured: all methods allowed. The operator has explicitly
    // opted out of authentication (require_app_auth = false), so the console
    // is an open local deployment where every RPC method should work.

    let Some(runtime) = &state.runtime else {
        return (
            StatusCode::NOT_FOUND,
            Json::<Value>(serde_json::json!({
                "jsonrpc": JSONRPC_VERSION,
                "id": parsed_request.id.unwrap_or(Value::Null),
                "error": {
                    "code": -32600,
                    "message": "console rpc requires a unified runtime",
                }
            })),
        );
    };

    // By this point the request is always authorized:
    // - require_app_auth=true: an invalid token already returned 401 above.
    // - require_app_auth=false: all methods are permitted unconditionally.
    // Either way, capabilities should reflect that all methods are available.
    let is_authenticated = true;
    let response_value = handle_console_runtime_rpc(
        runtime,
        state.module_runtime.clone(),
        state.contact_directory.as_ref(),
        state.event_log.clone(),
        state.gateway_peer_keys.as_ref(),
        state.console_events.clone(),
        state.metadata_table.clone(),
        state.mob_events.clone(),
        parsed_request,
        is_authenticated,
    )
    .await;
    (StatusCode::OK, Json::<Value>(response_value))
}

pub async fn console_rpc_multipart_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    mut multipart: Multipart,
) -> impl IntoResponse {
    if !console_request_authorized(&state, &headers, &uri) {
        return (
            StatusCode::UNAUTHORIZED,
            Json::<Value>(serde_json::json!({
                "jsonrpc": JSONRPC_VERSION,
                "id": Value::Null,
                "error": {
                    "code": -32600,
                    "message": "unauthorized: console rpc requires a valid auth token",
                }
            })),
        );
    }

    let Some(runtime) = &state.runtime else {
        return (
            StatusCode::NOT_FOUND,
            Json::<Value>(serde_json::json!({
                "jsonrpc": JSONRPC_VERSION,
                "id": Value::Null,
                "error": {
                    "code": -32600,
                    "message": "console rpc multipart requires a unified runtime",
                }
            })),
        );
    };
    let Some(binary_blob_store) = runtime.binary_blob_store() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json::<Value>(serde_json::json!({
                "jsonrpc": JSONRPC_VERSION,
                "id": Value::Null,
                "error": {
                    "code": -32000,
                    "message": "binary blob store unavailable",
                }
            })),
        );
    };

    let mut payload: Option<String> = None;
    let mut files: std::collections::BTreeMap<String, MultipartImageUpload> =
        std::collections::BTreeMap::new();

    while let Some(mut field) = match multipart.next_field().await {
        Ok(field) => field,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json::<Value>(json_rpc_error_value(
                    Value::Null,
                    -32602,
                    format!("invalid multipart body: {err}"),
                )),
            );
        }
    } {
        let name = field.name().unwrap_or("").to_string();
        if name == "payload" {
            if payload.is_some() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json::<Value>(json_rpc_error_value(
                        Value::Null,
                        -32602,
                        "duplicate payload part",
                    )),
                );
            }
            payload = match field.text().await {
                Ok(text) => Some(text),
                Err(err) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json::<Value>(json_rpc_error_value(
                            Value::Null,
                            -32602,
                            format!("invalid payload part: {err}"),
                        )),
                    );
                }
            };
            continue;
        }

        let Some(upload_id) = name.strip_prefix("file:").filter(|id| !id.is_empty()) else {
            return (
                StatusCode::BAD_REQUEST,
                Json::<Value>(json_rpc_error_value(
                    Value::Null,
                    -32602,
                    format!("unexpected multipart field: {name}"),
                )),
            );
        };
        if files.len() >= MAX_MULTIPART_IMAGES {
            return (
                StatusCode::BAD_REQUEST,
                Json::<Value>(json_rpc_error_value(
                    Value::Null,
                    -32602,
                    format!("too many image attachments; max {MAX_MULTIPART_IMAGES}"),
                )),
            );
        }
        if files.contains_key(upload_id) {
            return (
                StatusCode::BAD_REQUEST,
                Json::<Value>(json_rpc_error_value(
                    Value::Null,
                    -32602,
                    format!("duplicate file part for upload_id {upload_id}"),
                )),
            );
        }
        let media_type = field
            .content_type()
            .map(str::to_string)
            .unwrap_or_else(|| "application/octet-stream".to_string());
        if !is_allowed_image_media_type(&media_type) {
            return (
                StatusCode::BAD_REQUEST,
                Json::<Value>(json_rpc_error_value(
                    Value::Null,
                    -32602,
                    format!("unsupported image media type: {media_type}"),
                )),
            );
        }
        let mut bytes = bytes::BytesMut::new();
        loop {
            let chunk = match field.chunk().await {
                Ok(chunk) => chunk,
                Err(err) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json::<Value>(json_rpc_error_value(
                            Value::Null,
                            -32602,
                            format!("invalid file part {upload_id}: {err}"),
                        )),
                    );
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
            if bytes.len() + chunk.len() > MAX_MULTIPART_IMAGE_BYTES {
                return (
                    StatusCode::BAD_REQUEST,
                    Json::<Value>(json_rpc_error_value(
                        Value::Null,
                        -32602,
                        format!("image attachment {upload_id} exceeds 25 MiB"),
                    )),
                );
            }
            bytes.extend_from_slice(&chunk);
        }
        files.insert(
            upload_id.to_string(),
            MultipartImageUpload {
                media_type,
                bytes: bytes.freeze(),
            },
        );
    }

    let payload = match payload {
        Some(payload) => payload,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json::<Value>(json_rpc_error_value(
                    Value::Null,
                    -32602,
                    "payload part required",
                )),
            );
        }
    };
    let mut parsed_request = match serde_json::from_str::<JsonRpcRequest>(&payload) {
        Ok(req) => req,
        Err(err) => {
            return (
                StatusCode::OK,
                Json::<Value>(json_rpc_error_value(
                    Value::Null,
                    -32600,
                    format!("Invalid Request: {err}"),
                )),
            );
        }
    };
    let response_id = parsed_request.id.clone().unwrap_or(Value::Null);
    match parsed_request.method.as_str() {
        "mobkit/send_message" => {
            if let Err(message) = externalize_image_upload_placeholders(
                &mut parsed_request.params,
                files,
                binary_blob_store,
            )
            .await
            {
                return (
                    StatusCode::OK,
                    Json::<Value>(invalid_params(response_id, message)),
                );
            }
        }
        "mobkit/blob/upload" => {
            let result = match externalize_single_image_upload(
                &parsed_request.params,
                files,
                binary_blob_store,
            )
            .await
            {
                Ok(result) => result,
                Err(message) => {
                    return (
                        StatusCode::OK,
                        Json::<Value>(invalid_params(response_id, message)),
                    );
                }
            };
            return (
                StatusCode::OK,
                Json::<Value>(response_value(response_id, Some(result), None)),
            );
        }
        _ => {
            return (
                StatusCode::OK,
                Json::<Value>(invalid_params(
                    response_id,
                    "multipart RPC supports mobkit/send_message and mobkit/blob/upload only",
                )),
            );
        }
    }

    let response_value = handle_console_runtime_rpc(
        runtime,
        state.module_runtime.clone(),
        state.contact_directory.as_ref(),
        state.event_log.clone(),
        state.gateway_peer_keys.as_ref(),
        state.console_events.clone(),
        state.metadata_table.clone(),
        state.mob_events.clone(),
        parsed_request,
        true,
    )
    .await;
    (StatusCode::OK, Json::<Value>(response_value))
}

pub async fn blob_get_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    AxumPath(blob_id): AxumPath<String>,
) -> impl IntoResponse {
    if !console_request_authorized(&state, &headers, &uri) {
        return (
            StatusCode::UNAUTHORIZED,
            Json::<Value>(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let Some(runtime) = &state.runtime else {
        return (
            StatusCode::NOT_FOUND,
            Json::<Value>(serde_json::json!({ "error": "runtime_unavailable" })),
        )
            .into_response();
    };
    let Some(store) = runtime.binary_blob_store() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json::<Value>(serde_json::json!({ "error": "blob_store_unavailable" })),
        )
            .into_response();
    };
    if !is_valid_blob_id_value(&blob_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json::<Value>(serde_json::json!({ "error": "invalid_blob_id" })),
        )
            .into_response();
    }
    match store
        .get_bytes(&meerkat_core::BlobId::from(blob_id.as_str()))
        .await
    {
        Ok(payload) => {
            let mut response_headers = HeaderMap::new();
            let content_type = HeaderValue::from_str(&payload.media_type)
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
            response_headers.insert(header::CONTENT_TYPE, content_type);
            if let Ok(content_length) = HeaderValue::from_str(&payload.size.to_string()) {
                response_headers.insert(header::CONTENT_LENGTH, content_length);
            }
            response_headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=31536000, immutable"),
            );
            (StatusCode::OK, response_headers, payload.data).into_response()
        }
        Err(meerkat_core::BlobStoreError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json::<Value>(serde_json::json!({ "error": "blob_not_found" })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json::<Value>(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn console_identity_stream_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<IdentityStreamRequest>,
) -> impl IntoResponse {
    if !console_request_authorized(&state, &headers, &uri) {
        return (
            StatusCode::UNAUTHORIZED,
            Json::<Value>(serde_json::json!({
                "error": "unauthorized",
                "reason": "console stream requires a valid auth token",
            })),
        )
            .into_response();
    }
    if let Err(message) = request.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json::<Value>(serde_json::json!({ "error": message })),
        )
            .into_response();
    }
    let identity = request.identity;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let replayed = match &state.console_events {
        Some(store) => match store
            .replay_identity(&identity, last_event_id.as_deref())
            .await
        {
            Ok(events) => events,
            Err(err) => {
                return (
                    StatusCode::CONFLICT,
                    Json::<Value>(serde_json::to_value(err).unwrap_or_else(|_| {
                        json!({
                            "error": "replay_unavailable"
                        })
                    })),
                )
                    .into_response();
            }
        },
        None => {
            if let Some(response) = replay_unavailable_response(&headers, IDENTITY_STREAM_NAME) {
                return response.into_response();
            }
            Vec::new()
        }
    };
    let subscribed = console_stream_control_envelope(IDENTITY_STREAM_NAME, Some(identity.clone()));
    if let Some(store) = &state.console_events {
        store
            .note_identity_stream_checkpoint(&identity, subscribed.event_id.clone())
            .await;
    }
    let mut rx = state
        .console_events
        .as_ref()
        .map(ConsoleEventStore::subscribe);
    let stream = stream! {
        if let Some(event) = sse_event_from_envelope(&subscribed) {
            yield Ok::<Event, Infallible>(event);
        }
        for envelope in replayed {
            if let Some(event) = sse_event_from_envelope(&envelope) {
                yield Ok::<Event, Infallible>(event);
            }
        }
        if let Some(ref mut rx) = rx {
            loop {
                match rx.recv().await {
                    Ok(envelope) if envelope.identity == identity => {
                        if let Some(event) = sse_event_from_envelope(&envelope) {
                            yield Ok::<Event, Infallible>(event);
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        // Emit a synthetic `stream_lagged` frame so the
                        // client knows it missed events; pre-fix we
                        // silently `break`-ed and the connection stayed
                        // open via keep-alive forever.
                        let lagged = console_stream_lagged_envelope(
                            IDENTITY_STREAM_NAME,
                            skipped,
                        );
                        if let Some(event) = sse_event_from_envelope(&lagged) {
                            yield Ok::<Event, Infallible>(event);
                        }
                        // Keep the connection live; the receiver has
                        // re-anchored to the broadcast tail.
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    };
    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(DEFAULT_KEEP_ALIVE_INTERVAL)
                .text(KEEP_ALIVE_TEXT),
        )
        .into_response()
}

async fn console_events_stream_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
) -> impl IntoResponse {
    if !console_request_authorized(&state, &headers, &uri) {
        return (
            StatusCode::UNAUTHORIZED,
            Json::<Value>(serde_json::json!({
                "error": "unauthorized",
                "reason": "console stream requires a valid auth token",
            })),
        )
            .into_response();
    }
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let replayed = match &state.console_events {
        Some(store) => match store.replay_all(last_event_id.as_deref()).await {
            Ok(events) => events,
            Err(err) => {
                return (
                    StatusCode::CONFLICT,
                    Json::<Value>(serde_json::to_value(err).unwrap_or_else(|_| {
                        json!({
                            "error": "replay_unavailable"
                        })
                    })),
                )
                    .into_response();
            }
        },
        None => {
            if let Some(response) = replay_unavailable_response(&headers, ALL_EVENTS_STREAM_NAME) {
                return response.into_response();
            }
            Vec::new()
        }
    };
    let subscribed = console_stream_control_envelope(ALL_EVENTS_STREAM_NAME, None);
    if let Some(store) = &state.console_events {
        store
            .note_all_stream_checkpoint(subscribed.event_id.clone())
            .await;
    }
    let mut rx = state
        .console_events
        .as_ref()
        .map(ConsoleEventStore::subscribe);
    let stream = stream! {
        if let Some(event) = sse_event_from_envelope(&subscribed) {
            yield Ok::<Event, Infallible>(event);
        }
        for envelope in replayed {
            if let Some(event) = sse_event_from_envelope(&envelope) {
                yield Ok::<Event, Infallible>(event);
            }
        }
        if let Some(ref mut rx) = rx {
            loop {
                match rx.recv().await {
                    Ok(envelope) => {
                        if let Some(event) = sse_event_from_envelope(&envelope) {
                            yield Ok::<Event, Infallible>(event);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        let lagged = console_stream_lagged_envelope(
                            ALL_EVENTS_STREAM_NAME,
                            skipped,
                        );
                        if let Some(event) = sse_event_from_envelope(&lagged) {
                            yield Ok::<Event, Infallible>(event);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    };
    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(DEFAULT_KEEP_ALIVE_INTERVAL)
                .text(KEEP_ALIVE_TEXT),
        )
        .into_response()
}

fn console_request_authorized(state: &ConsoleJsonState, headers: &HeaderMap, uri: &Uri) -> bool {
    if !state.decisions.console.require_app_auth {
        return true;
    }
    console_request_token(headers, uri)
        .is_some_and(|token| validate_console_token(&state.decisions, &token))
}

fn console_request_token(headers: &HeaderMap, uri: &Uri) -> Option<String> {
    let bearer_token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(extract_bearer_token_from_header)
        .map(String::from);
    // Parse with form_urlencoded so percent-encoded tokens decode and
    // `xauth_token=` substring shadowing does NOT match the real key.
    let query_token = uri.query().and_then(|q| {
        form_urlencoded::parse(q.as_bytes())
            .find(|(key, _)| key == "auth_token")
            .map(|(_, value)| value.into_owned())
    });
    bearer_token.or(query_token)
}

/// Validate that `last_event_id` matches the format the server actually
/// mints (`console-stream-{stream}-{millis}` per
/// [`stream_head_event_id`]). Rejects any client value containing
/// characters outside `[A-Za-z0-9_\-]`. Pre-fix the unvalidated header
/// was reflected verbatim into the error JSON; an operator console
/// rendering it without escaping was exposed to log-injection /
/// display-side XSS via headers like `<script>alert(1)</script>`.
fn is_valid_last_event_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn replay_unavailable_response(
    headers: &HeaderMap,
    stream_name: &str,
) -> Option<(StatusCode, Json<Value>)> {
    let requested_last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if !is_valid_last_event_id(requested_last_event_id) {
        return Some((
            StatusCode::BAD_REQUEST,
            Json::<Value>(serde_json::json!({
                "error": "invalid_last_event_id",
                "stream": stream_name,
            })),
        ));
    }
    Some((
        StatusCode::CONFLICT,
        Json::<Value>(
            serde_json::to_value(ReplayUnavailableError {
                error: "replay_unavailable".to_string(),
                stream: stream_name.to_string(),
                requested_last_event_id: requested_last_event_id.to_string(),
                latest_event_id: stream_head_event_id(stream_name),
            })
            .unwrap_or_else(|_| serde_json::json!({ "error": "replay_unavailable" })),
        ),
    ))
}

fn stream_head_event_id(stream_name: &str) -> String {
    format!("console-stream-{stream_name}-{}", current_time_ms())
}

fn console_stream_control_envelope(
    stream_name: &str,
    identity: Option<String>,
) -> ConsoleIdentityEventEnvelope {
    ConsoleIdentityEventEnvelope {
        event_id: stream_head_event_id(stream_name),
        interaction_id: None,
        identity: identity.unwrap_or_else(|| ALL_EVENTS_CONTROL_IDENTITY.to_string()),
        event_type: "subscribed".to_string(),
        timestamp_ms: current_time_ms(),
        data: serde_json::json!({
            "stream": stream_name,
        }),
    }
}

/// Synthetic envelope emitted when the broadcast receiver lags. Pre-fix
/// the handler `break`-ed on `Lagged`, leaving the SSE connection open
/// under keep-alive but silent forever. Clients now see this frame and
/// know to refetch via the catch-up endpoint or accept missed events.
fn console_stream_lagged_envelope(stream_name: &str, skipped: u64) -> ConsoleIdentityEventEnvelope {
    ConsoleIdentityEventEnvelope {
        event_id: format!("console-stream-{stream_name}-lagged-{}", current_time_ms()),
        interaction_id: None,
        identity: ALL_EVENTS_CONTROL_IDENTITY.to_string(),
        event_type: "stream_lagged".to_string(),
        timestamp_ms: current_time_ms(),
        data: serde_json::json!({
            "stream": stream_name,
            "skipped": skipped,
            "advice": "client should refetch state via catch-up endpoint or accept the gap",
        }),
    }
}

fn sse_event_from_envelope(envelope: &ConsoleIdentityEventEnvelope) -> Option<Event> {
    match serde_json::to_string(envelope) {
        Ok(data) => Some(
            Event::default()
                .id(envelope.event_id.clone())
                .event(&envelope.event_type)
                .data(data),
        ),
        Err(err) => {
            warn!(
                event_id = %envelope.event_id,
                event_type = %envelope.event_type,
                identity = %envelope.identity,
                "skipping unserializable console SSE envelope: {err}"
            );
            None
        }
    }
}

#[derive(Debug)]
struct MultipartImageUpload {
    media_type: String,
    bytes: bytes::Bytes,
}

fn json_rpc_error_value(id: Value, code: i64, message: impl Into<String>) -> Value {
    serde_json::json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        }
    })
}

fn project_query_events_for_console(events: Vec<PersistedEvent>, query: &EventQuery) -> Value {
    let mut projected = Vec::new();
    for event in events {
        if persisted_event_is_empty_web_search_annotations(&event) {
            continue;
        }
        let assistant_images = assistant_image_events_from_persisted(&event, query);
        projected.push(serde_json::to_value(&event).unwrap_or(Value::Null));
        projected.extend(assistant_images);
    }
    Value::Array(projected)
}

fn persisted_event_is_empty_web_search_annotations(event: &PersistedEvent) -> bool {
    let crate::types::UnifiedEvent::Agent {
        event_type,
        payload,
        ..
    } = &event.event
    else {
        return false;
    };
    is_empty_web_search_annotations_event(event_type, payload.as_ref())
}

fn assistant_image_events_from_persisted(event: &PersistedEvent, query: &EventQuery) -> Vec<Value> {
    let crate::types::UnifiedEvent::Agent {
        agent_id,
        event_type,
        payload,
    } = &event.event
    else {
        return Vec::new();
    };
    let identity = derive_console_identity_from_runtime_id(agent_id)
        .unwrap_or_else(|| event.member_id.clone().unwrap_or_else(|| agent_id.clone()));
    if let Some(query_identity) = query.identity.as_deref()
        && query_identity != identity
        && query_identity != agent_id
        && event.member_id.as_deref() != Some(query_identity)
    {
        return Vec::new();
    }
    let Some(payload) = payload else {
        return Vec::new();
    };
    let Some(image_result) = parse_generate_image_tool_result_value(payload) else {
        return Vec::new();
    };
    image_result
        .images
        .iter()
        .enumerate()
        .map(|(idx, image)| {
            serde_json::to_value(ConsoleIdentityEventEnvelope {
                event_id: format!("{}#assistant_image:{idx}", event.id),
                interaction_id: None,
                identity: identity.clone(),
                event_type: "assistant_image".to_string(),
                timestamp_ms: event.timestamp_ms,
                data: json!({
                    "source_event_type": event_type,
                    "tool_call_id": payload.get("id").cloned().unwrap_or(Value::Null),
                    "image_id": image.image_id.0.to_string(),
                    "blob_id": image.blob_ref.blob_id.to_string(),
                    "media_type": image.media_type.as_str(),
                    "width": image.width,
                    "height": image.height,
                    "revised_prompt": image_result.revised_prompt.clone(),
                }),
            })
            .unwrap_or(Value::Null)
        })
        .collect()
}

fn parse_generate_image_tool_result_value(
    payload: &Value,
) -> Option<meerkat_core::image_generation::ImageGenerationToolResult> {
    if payload.get("name").and_then(Value::as_str) != Some("generate_image") {
        return None;
    }
    let result_value = payload.get("result")?;
    if let Some(result_text) = result_value.as_str() {
        serde_json::from_str(result_text).ok()
    } else {
        serde_json::from_value(result_value.clone()).ok()
    }
}

fn derive_console_identity_from_runtime_id(runtime_id: &str) -> Option<String> {
    let (identity, suffix) = runtime_id.rsplit_once(':')?;
    if identity.is_empty() || suffix.is_empty() {
        return None;
    }
    if !suffix.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let identity = identity.strip_prefix("rt:").unwrap_or(identity);
    if identity.is_empty() {
        return None;
    }
    Some(identity.to_string())
}

fn is_allowed_image_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    )
}

fn image_upload_part_name<'a>(
    object: &'a serde_json::Map<String, Value>,
    context: &str,
) -> Result<&'a str, String> {
    object
        .get("upload_id")
        .or_else(|| object.get("part_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{context}.upload_id or {context}.part_name is required"))
}

async fn externalize_image_upload_placeholders(
    params: &mut Value,
    files: std::collections::BTreeMap<String, MultipartImageUpload>,
    blob_store: std::sync::Arc<dyn crate::blob_store::BinaryBlobStore>,
) -> Result<(), String> {
    let Some(content) = params.get_mut("content") else {
        return Err("multipart payload params.content is required".to_string());
    };
    let mut placeholders = std::collections::BTreeMap::<String, String>::new();
    collect_image_upload_placeholders(content, &mut placeholders)?;
    if placeholders.is_empty() {
        return Err(
            "multipart payload must contain at least one image_upload placeholder".to_string(),
        );
    }
    if placeholders.len() > MAX_MULTIPART_IMAGES {
        return Err(format!(
            "too many image_upload placeholders; max {MAX_MULTIPART_IMAGES}"
        ));
    }
    for upload_id in files.keys() {
        if !placeholders.contains_key(upload_id) {
            return Err(format!(
                "file part has no matching image_upload placeholder: {upload_id}"
            ));
        }
    }
    for upload_id in placeholders.keys() {
        if !files.contains_key(upload_id) {
            return Err(format!(
                "image_upload placeholder missing file part: {upload_id}"
            ));
        }
    }

    let mut refs = std::collections::BTreeMap::<String, Value>::new();
    for (upload_id, file) in files {
        let declared_media_type = placeholders
            .get(&upload_id)
            .cloned()
            .unwrap_or_else(|| file.media_type.clone());
        if !is_allowed_image_media_type(&declared_media_type) {
            return Err(format!(
                "unsupported image media type in placeholder {upload_id}: {declared_media_type}"
            ));
        }
        if declared_media_type != file.media_type {
            return Err(format!(
                "media type mismatch for {upload_id}: placeholder {declared_media_type}, file {}",
                file.media_type
            ));
        }
        let blob_ref = blob_store
            .put_bytes(&file.media_type, file.bytes)
            .await
            .map_err(|err| format!("failed to store image {upload_id}: {err}"))?;
        refs.insert(
            upload_id,
            serde_json::json!({
                "type": "image",
                "media_type": blob_ref.media_type,
                "source": "blob",
                "blob_id": blob_ref.blob_id,
            }),
        );
    }
    replace_image_upload_placeholders(content, &refs)?;
    if let Some(object) = params.as_object_mut() {
        object.remove("message");
    }
    Ok(())
}

async fn externalize_single_image_upload(
    params: &Value,
    files: std::collections::BTreeMap<String, MultipartImageUpload>,
    blob_store: std::sync::Arc<dyn crate::blob_store::BinaryBlobStore>,
) -> Result<Value, String> {
    let upload = params.get("upload").unwrap_or(params);
    if upload
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "image_upload")
    {
        return Err("upload.type must be image_upload".to_string());
    }
    let upload_object = upload
        .as_object()
        .ok_or_else(|| "upload must be an object".to_string())?;
    let upload_id = image_upload_part_name(upload_object, "upload")?;
    let Some(file) = files.get(upload_id) else {
        return Err(format!(
            "image_upload placeholder missing file part: {upload_id}"
        ));
    };
    if files.len() != 1 {
        return Err("mobkit/blob/upload accepts exactly one file part".to_string());
    }
    let declared_media_type = upload
        .get("media_type")
        .and_then(Value::as_str)
        .unwrap_or(file.media_type.as_str());
    if !is_allowed_image_media_type(declared_media_type) {
        return Err(format!(
            "unsupported image media type in upload {upload_id}: {declared_media_type}"
        ));
    }
    if declared_media_type != file.media_type {
        return Err(format!(
            "media type mismatch for {upload_id}: placeholder {declared_media_type}, file {}",
            file.media_type
        ));
    }
    let size = file.bytes.len() as u64;
    let blob_ref = blob_store
        .put_bytes(&file.media_type, file.bytes.clone())
        .await
        .map_err(|err| format!("failed to store image {upload_id}: {err}"))?;
    Ok(json!({
        "blob_id": blob_ref.blob_id,
        "media_type": blob_ref.media_type,
        "size": size,
    }))
}

fn collect_image_upload_placeholders(
    value: &Value,
    placeholders: &mut std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_image_upload_placeholders(item, placeholders)?;
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("image_upload") {
                let upload_id = image_upload_part_name(object, "image_upload")?;
                let media_type = object
                    .get("media_type")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| format!("image_upload {upload_id} requires media_type"))?;
                if placeholders
                    .insert(upload_id.to_string(), media_type.to_string())
                    .is_some()
                {
                    return Err(format!("duplicate image_upload placeholder: {upload_id}"));
                }
            } else {
                for child in object.values() {
                    collect_image_upload_placeholders(child, placeholders)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn replace_image_upload_placeholders(
    value: &mut Value,
    refs: &std::collections::BTreeMap<String, Value>,
) -> Result<(), String> {
    match value {
        Value::Array(items) => {
            for item in items {
                replace_image_upload_placeholders(item, refs)?;
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("image_upload") {
                let upload_id = image_upload_part_name(object, "image_upload")?;
                let replacement = refs
                    .get(upload_id)
                    .ok_or_else(|| format!("missing blob replacement for {upload_id}"))?;
                *value = replacement.clone();
            } else {
                for child in object.values_mut() {
                    replace_image_upload_placeholders(child, refs)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn mint_console_interaction_id() -> String {
    let now = current_time_ms();
    let seq = CONSOLE_INTERACTION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("turn-{now}-{seq}")
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

fn invalid_params(id: Value, message: impl Into<String>) -> Value {
    response_value(
        id,
        None,
        Some(JsonRpcError {
            code: -32602,
            message: message.into(),
            data: None,
        }),
    )
}

async fn member_entry_to_console_json(
    runtime: &MobRuntime,
    entry: &meerkat_mob::runtime::MobMemberListEntry,
) -> Value {
    let mut value = member_entry_to_json(entry);
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "model_capabilities".to_string(),
            serde_json::to_value(
                model_capabilities_for_member(
                    &runtime.handle(),
                    runtime.session_service(),
                    &entry.agent_identity,
                )
                .await,
            )
            .unwrap_or(Value::Null),
        );
    }
    value
}

fn internal_error(id: Value, message: impl Into<String>) -> Value {
    response_value(
        id,
        None,
        Some(JsonRpcError {
            code: -32000,
            message: message.into(),
            data: None,
        }),
    )
}

/// Render a stale-cursor failure as a JSON-RPC envelope with code
/// `-32010`, a typed error body the SDKs can parse into the
/// `MobEventsStaleError` exception, and a `data` field carrying both
/// cursors so callers can rewind to the current frontier.
fn stale_event_cursor_response(id: Value, after_cursor: u64, latest_cursor: u64) -> Value {
    response_value(
        id,
        None,
        Some(JsonRpcError {
            code: crate::rpc::MOB_EVENTS_STALE_CURSOR_CODE,
            message: format!(
                "stale mob event cursor: requested {after_cursor}, latest {latest_cursor}"
            ),
            data: Some(serde_json::json!({
                "error": "event_query_stale",
                "after_cursor": after_cursor,
                "latest_cursor": latest_cursor,
            })),
        }),
    )
}

/// Optional JSON-RPC param `handling_mode: "queue" | "steer"`.
/// Missing/null defaults to `Queue`; unknown strings remain invalid params.
/// The direct MobKit send path normalizes `Steer` to `Queue` until a
/// runtime-backed steering surface is available.
fn parse_handling_mode(params: &Value) -> Result<meerkat_core::types::HandlingMode, &'static str> {
    let Some(raw) = params.get("handling_mode") else {
        return Ok(meerkat_core::types::HandlingMode::Queue);
    };
    if raw.is_null() {
        return Ok(meerkat_core::types::HandlingMode::Queue);
    }
    match raw.as_str() {
        Some("queue") => Ok(meerkat_core::types::HandlingMode::Queue),
        Some("steer") => Ok(meerkat_core::types::HandlingMode::Steer),
        _ => Err("handling_mode must be \"queue\" or \"steer\""),
    }
}

fn parse_console_helper_options(
    options_val: Option<&Value>,
) -> Result<meerkat_mob::HelperOptions, String> {
    crate::rpc::mob_methods::parse_helper_options(options_val)
}

fn member_is_addressable(member: &meerkat_mob::runtime::MobMemberListEntry) -> bool {
    member
        .labels
        .get("addressable")
        .map(|value: &String| !value.eq_ignore_ascii_case("false"))
        .unwrap_or(true)
}

fn member_addressability(member: &meerkat_mob::runtime::MobMemberListEntry) -> &'static str {
    if member_is_addressable(member) {
        "addressable"
    } else {
        "internal_only"
    }
}

fn console_identity_status_json(
    member: &meerkat_mob::runtime::MobMemberListEntry,
    session_id: Option<String>,
    response_phase: Option<String>,
) -> Value {
    json!({
        "identity": member.agent_identity.to_string(),
        "state": member.state,
        "role": member.role.to_string(),
        "addressability": member_addressability(member),
        "display_name": member.labels.get("display_name"),
        "labels": member.labels,
        "agent_runtime_id": member.binding_atoms().0.to_string(),
        "session_id": session_id,
        "generation": Value::Null,
        "checkpoint_version": Value::Null,
        "lease_healthy": Value::Null,
        "lease": Value::Null,
        "response_phase": response_phase,
    })
}

fn console_identity_inspect_json(
    member: &meerkat_mob::runtime::MobMemberListEntry,
    session_id: Option<String>,
    response_phase: Option<String>,
) -> Value {
    let peers: Vec<String> = member.wired_to.iter().map(ToString::to_string).collect();
    json!({
        "identity": member.agent_identity.to_string(),
        "state": member.state,
        "role": member.role.to_string(),
        "addressability": member_addressability(member),
        "display_name": member.labels.get("display_name"),
        "labels": member.labels,
        "lease_healthy": Value::Null,
        "lease": Value::Null,
        "continuity": {
            "generation": Value::Null,
            "checkpoint_version": Value::Null,
            "session_id": session_id,
            "agent_runtime_id": member.binding_atoms().0.to_string(),
        },
        "topology_peers": peers,
        "output_preview": Value::Null,
        "response_phase": response_phase,
    })
}

/// Resolve a mob member by identity plus its current bridge session id.
///
/// Returns `None` if no member with the given identity exists.
async fn lookup_member_with_session(
    handle: &MobHandle,
    identity: &MeerkatId,
) -> Option<(meerkat_mob::runtime::MobMemberListEntry, Option<String>)> {
    let entries = handle.list_members_including_retiring().await;
    let entry = entries
        .into_iter()
        .find(|e| &e.agent_identity == identity)?;
    let session_id = handle
        .resolve_bridge_session_id(identity)
        .await
        .map(|s| s.to_string());
    Some((entry, session_id))
}

#[allow(clippy::too_many_arguments)]
async fn handle_console_runtime_rpc(
    runtime: &MobRuntime,
    module_runtime: Option<std::sync::Arc<tokio::sync::Mutex<MobkitRuntimeHandle>>>,
    contact_directory: Option<&ContactDirectory>,
    event_log: Option<std::sync::Arc<dyn EventLogStore>>,
    gateway_peer_keys: Option<&crate::auth::peer_keys::GatewayPeerKeys>,
    console_events: Option<ConsoleEventStore>,
    metadata_table: Option<std::sync::Arc<RuntimeMetadataTable>>,
    mob_events: Option<MobEventsStore>,
    request: JsonRpcRequest,
    is_authenticated: bool,
) -> Value {
    let response_id = request.id.clone().unwrap_or(Value::Null);

    match request.method.as_str() {
        "mobkit/capabilities" => {
            let mut methods = vec![
                "mobkit/status",
                "mobkit/capabilities",
                "mobkit/list_members",
                "mobkit/get_member",
                "mobkit/find_members",
                "mobkit/member_status",
                "mobkit/read_session_history",
                "mobkit/collect_completed",
                "mobkit/blob/get",
                "mobkit/wait_ready",
                "mobkit/flow_status",
                "mobkit/list_flows",
                "mobkit/list_runs",
                "mobkit/query_events",
                "mobkit/mob_events/query",
                "mobkit/mob_events/subscribe",
                "mobkit/cross_mob/peer_info",
                "mobkit/cross_mob/directory",
                "mobkit/peer_pubkey",
            ];
            if module_runtime.is_some() {
                methods.extend_from_slice(&[
                    "mobkit/interact",
                    "mobkit/status_identity",
                    "mobkit/inspect_identity",
                    "mobkit/retire",
                    "mobkit/respawn",
                    "mobkit/reset",
                    "mobkit/routing/routes/list",
                    "mobkit/delivery/history",
                    "mobkit/gating/pending",
                    "mobkit/gating/audit",
                    "mobkit/gating/decide",
                ]);
            }
            if is_authenticated {
                methods.extend_from_slice(&[
                    "mobkit/send_message",
                    "mobkit/blob/upload",
                    "mobkit/ensure_member",
                    "mobkit/retire_member",
                    "mobkit/respawn_member",
                    "mobkit/force_cancel_member",
                    "mobkit/cancel_flow",
                    "mobkit/run_flow",
                    "mobkit/spawn_helper",
                    "mobkit/fork_helper",
                    "mobkit/attach_existing_session",
                    "mobkit/reconcile_edges",
                    "mobkit/cross_mob/wire_local",
                    "mobkit/cross_mob/unwire_local",
                ]);
            }
            if metadata_table.is_some() {
                methods.extend_from_slice(&["mobkit/mob_labels/get", "mobkit/run_labels/get"]);
                if is_authenticated {
                    methods.extend_from_slice(&[
                        "mobkit/mob_labels/set",
                        "mobkit/mob_labels/delete",
                        "mobkit/run_labels/set",
                        "mobkit/run_labels/delete",
                    ]);
                }
            }
            response_value(
                response_id,
                Some(serde_json::json!({
                    "contract_version": crate::rpc::MOBKIT_CONTRACT_VERSION,
                    "methods": methods,
                    // The console routes to MobRuntime directly and has no
                    // access to the module runtime, so loaded_modules is always [].
                    "loaded_modules": serde_json::json!([]),
                    "runtime_capabilities": {
                        "can_send_messages": is_authenticated,
                        "can_retire_members": is_authenticated,
                        "can_spawn_members": is_authenticated,
                    }
                })),
                None,
            )
        }
        "mobkit/status" => {
            let mob_state = runtime.handle().status().await.ok();
            response_value(
                response_id,
                Some(serde_json::json!({
                    "contract_version": crate::rpc::MOBKIT_CONTRACT_VERSION,
                    "running": matches!(mob_state, Some(MobState::Creating | MobState::Running)),
                    // Console routes to MobRuntime directly — no module runtime available.
                    // Return [] to keep StatusResult.loaded_modules schema-consistent.
                    "loaded_modules": serde_json::json!([]),
                })),
                None,
            )
        }
        "mobkit/blob/get" => {
            let Some(blob_id) = request
                .params
                .get("blob_id")
                .or_else(|| request.params.get("id"))
                .and_then(Value::as_str)
            else {
                return invalid_params(response_id, "blob_id required");
            };
            if !is_valid_blob_id_value(blob_id) {
                return invalid_params(response_id, "invalid blob_id");
            }
            let Some(store) = runtime.binary_blob_store() else {
                return internal_error(response_id, "binary blob store unavailable");
            };
            match store.get_bytes(&meerkat_core::BlobId::from(blob_id)).await {
                Ok(payload) => response_value(
                    response_id,
                    Some(serde_json::json!({
                        "blob_id": payload.blob_id,
                        "media_type": payload.media_type,
                        "size": payload.size,
                        "data": base64::engine::general_purpose::STANDARD.encode(payload.data.as_ref()),
                    })),
                    None,
                ),
                Err(meerkat_core::BlobStoreError::NotFound(_)) => response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32001,
                        message: format!("blob not found: {blob_id}"),
                        data: Some(json!({ "kind": "not_found", "blob_id": blob_id })),
                    }),
                ),
                Err(err) => internal_error(response_id, format!("blob get failed: {err}")),
            }
        }
        "mobkit/list_members" => {
            let handle = runtime.handle();
            let entries = handle.list_members_including_retiring().await;
            let mut members = Vec::with_capacity(entries.len());
            for entry in &entries {
                members.push(member_entry_to_console_json(runtime, entry).await);
            }
            response_value(response_id, Some(Value::Array(members)), None)
        }
        "mobkit/get_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            let handle = runtime.handle();
            let identity = MeerkatId::from(member_id);
            let entries = handle.list_members_including_retiring().await;
            match entries.into_iter().find(|e| e.agent_identity == identity) {
                Some(entry) => response_value(
                    response_id,
                    Some(member_entry_to_console_json(runtime, &entry).await),
                    None,
                ),
                None => invalid_params(response_id, format!("member not found: {member_id}")),
            }
        }
        "mobkit/find_members" => {
            let Some(label_key) = request.params.get("label_key").and_then(Value::as_str) else {
                return invalid_params(response_id, "label_key required");
            };
            let Some(label_value) = request.params.get("label_value").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "label_value required");
            };
            let handle = runtime.handle();
            let filter = MemberFilter {
                labels: std::collections::BTreeMap::from([(
                    label_key.to_string(),
                    label_value.to_string(),
                )]),
                role: None,
                state: None,
            };
            let entries = handle.list_members_matching(filter).await;
            let mut matches = Vec::with_capacity(entries.len());
            for entry in &entries {
                matches.push(member_entry_to_console_json(runtime, entry).await);
            }
            response_value(response_id, Some(Value::Array(matches)), None)
        }
        "mobkit/send_message" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            let raw_content = if let Some(content) = request.params.get("content") {
                content.clone()
            } else if let Some(message) = request.params.get("message").and_then(Value::as_str) {
                Value::String(message.to_string())
            } else {
                return invalid_params(response_id, "message or content required");
            };
            let content = if request.params.get("content").is_some() {
                match serde_json::from_value::<ContentInput>(raw_content.clone()) {
                    Ok(content) => content,
                    Err(err) => {
                        return invalid_params(response_id, format!("invalid content: {err}"));
                    }
                }
            } else if let Some(message) = raw_content.as_str() {
                ContentInput::Text(message.to_string())
            } else {
                return invalid_params(response_id, "message or content required");
            };
            let handling_mode = match parse_handling_mode(&request.params) {
                Ok(mode) => mode,
                Err(message) => return invalid_params(response_id, message),
            };
            if let Err(err) = assert_member_accepts_images(
                &runtime.handle(),
                runtime.session_service(),
                member_id,
                &content,
            )
            .await
            {
                return invalid_params(response_id, err.to_string());
            }
            let interaction_id = mint_console_interaction_id();
            if let Some(store) = &console_events {
                if let Err(message) = store
                    .reserve_interaction_value(
                        member_id,
                        Some(member_id),
                        &interaction_id,
                        "console:send_message",
                        raw_content,
                    )
                    .await
                {
                    return response_value(
                        response_id,
                        None,
                        Some(JsonRpcError {
                            code: -32003,
                            message: message.to_string(),
                            data: None,
                        }),
                    );
                }
                store.accept_interaction(member_id, &interaction_id).await;
            }
            match send_message_on_mob_with_mode(
                &runtime.handle(),
                member_id,
                content,
                handling_mode,
            )
            .await
            {
                Ok(session_id) => response_value(
                    response_id,
                    Some(serde_json::json!({
                        "accepted": true,
                        "member_id": member_id,
                        "session_id": session_id,
                        "interaction_id": interaction_id,
                    })),
                    None,
                ),
                Err(err) => {
                    if let Some(store) = &console_events {
                        store
                            .fail_interaction(
                                member_id,
                                &interaction_id,
                                "send_message_failed",
                                json!(err.to_string()),
                            )
                            .await;
                    }
                    internal_error(response_id, format!("send_message failed: {err}"))
                }
            }
        }
        "mobkit/interact" => {
            let request_params: crate::console_contracts::ConsoleInteractionRequest =
                match serde_json::from_value(request.params.clone()) {
                    Ok(params) => params,
                    Err(_) => {
                        return invalid_params(
                            response_id,
                            "invalid params: expected { identity, content, origin }",
                        );
                    }
                };
            if let Err(message) = request_params.validate() {
                return invalid_params(response_id, format!("invalid params: {message}"));
            }
            let identity = request_params.identity.trim();
            let handle = runtime.handle();
            let mid = MeerkatId::from(identity);
            let Some((member, _session_id)) = lookup_member_with_session(&handle, &mid).await
            else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32001,
                        message: format!("unknown identity: {identity}"),
                        data: None,
                    }),
                );
            };
            if !member_is_addressable(&member) {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32002,
                        message: format!("not addressable: {identity}"),
                        data: None,
                    }),
                );
            }
            if member.state == meerkat_mob::MemberState::Retiring {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32004,
                        message: format!("identity retiring: {identity}"),
                        data: None,
                    }),
                );
            }

            let interaction_id = mint_console_interaction_id();
            if let Some(store) = &console_events
                && let Err(message) = store
                    .reserve_interaction(
                        identity,
                        Some(member.agent_identity.as_str()),
                        &interaction_id,
                        &request_params.origin,
                        &request_params.content,
                    )
                    .await
            {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32003,
                        message: message.to_string(),
                        data: None,
                    }),
                );
            }

            // Emit interaction_started BEFORE dispatching so the event has
            // a lower sequence number than any agent response events it triggers.
            if let Some(store) = &console_events {
                store.accept_interaction(identity, &interaction_id).await;
            }

            // `handling_mode` is read off the raw params rather than added
            // to `ConsoleInteractionRequest` so the contract struct stays
            // stable for callers that don't care which mode they sent in.
            let handling_mode = match parse_handling_mode(&request.params) {
                Ok(mode) => mode,
                Err(message) => return invalid_params(response_id, message),
            };

            match send_message_on_mob_with_mode(
                &handle,
                identity,
                ContentInput::Text(request_params.content.clone()),
                handling_mode,
            )
            .await
            {
                Ok(_session_id) => response_value(
                    response_id,
                    Some(json!({
                        "interaction_id": interaction_id,
                        "identity": identity,
                    })),
                    None,
                ),
                Err(err) => {
                    if let Some(store) = &console_events {
                        store
                            .fail_interaction(
                                identity,
                                &interaction_id,
                                "dispatch_failed",
                                json!({ "reason": err.to_string() }),
                            )
                            .await;
                    }
                    internal_error(response_id, format!("interact failed: {err}"))
                }
            }
        }
        "mobkit/status_identity" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let handle = runtime.handle();
            let mid = MeerkatId::from(identity);
            let Some((member, session_id)) = lookup_member_with_session(&handle, &mid).await else {
                return invalid_params(response_id, format!("identity not found: {identity}"));
            };
            let phase = if let Some(store) = &console_events {
                store.response_phase_for_identity(identity).await
            } else {
                None
            };
            response_value(
                response_id,
                Some(console_identity_status_json(&member, session_id, phase)),
                None,
            )
        }
        "mobkit/inspect_identity" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let handle = runtime.handle();
            let mid = MeerkatId::from(identity);
            let Some((member, session_id)) = lookup_member_with_session(&handle, &mid).await else {
                return invalid_params(response_id, format!("identity not found: {identity}"));
            };
            let phase = if let Some(store) = &console_events {
                store.response_phase_for_identity(identity).await
            } else {
                None
            };
            response_value(
                response_id,
                Some(console_identity_inspect_json(&member, session_id, phase)),
                None,
            )
        }
        "mobkit/retire" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            match runtime.handle().retire(MeerkatId::from(identity)).await {
                Ok(()) => {
                    if let Some(store) = &console_events {
                        store
                            .record_lifecycle(identity, "identity_retired", json!({}))
                            .await;
                    }
                    response_value(response_id, Some(json!({ "identity": identity })), None)
                }
                Err(err) => internal_error(response_id, format!("retire failed: {err}")),
            }
        }
        "mobkit/respawn" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let handle = runtime.handle();
            let mid = MeerkatId::from(identity);
            match handle.respawn(mid.clone(), None).await {
                Ok(_receipt) => {
                    if let Some(store) = &console_events {
                        store
                            .record_lifecycle(identity, "identity_respawned", json!({}))
                            .await;
                    }
                    let body = match lookup_member_with_session(&handle, &mid).await {
                        Some((entry, session_id)) => {
                            console_identity_status_json(&entry, session_id, None)
                        }
                        None => json!({ "identity": identity }),
                    };
                    response_value(response_id, Some(body), None)
                }
                Err(err) => internal_error(response_id, format!("respawn failed: {err}")),
            }
        }
        "mobkit/reset" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let handle = runtime.handle();
            let mid = MeerkatId::from(identity);
            match handle.respawn(mid.clone(), None).await {
                Ok(_receipt) => {
                    if let Some(store) = &console_events {
                        store
                            .record_lifecycle(identity, "identity_reset", json!({}))
                            .await;
                    }
                    let body = match lookup_member_with_session(&handle, &mid).await {
                        Some((entry, session_id)) => {
                            console_identity_status_json(&entry, session_id, None)
                        }
                        None => json!({ "identity": identity }),
                    };
                    response_value(response_id, Some(body), None)
                }
                Err(err) => internal_error(response_id, format!("reset failed: {err}")),
            }
        }
        "mobkit/routing/routes/list" => {
            let Some(module_runtime) = &module_runtime else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32601,
                        message: "Method not found".to_string(),
                        data: None,
                    }),
                );
            };
            let routes = module_runtime.lock().await.list_runtime_routes();
            response_value(response_id, Some(json!({ "routes": routes })), None)
        }
        "mobkit/delivery/history" => {
            let Some(module_runtime) = &module_runtime else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32601,
                        message: "Method not found".to_string(),
                        data: None,
                    }),
                );
            };
            let limit = request
                .params
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50) as usize;
            let history = module_runtime
                .lock()
                .await
                .delivery_history(DeliveryHistoryRequest {
                    recipient: None,
                    sink: None,
                    limit,
                });
            response_value(
                response_id,
                Some(serde_json::to_value(history).unwrap_or(Value::Null)),
                None,
            )
        }
        "mobkit/gating/pending" => {
            let Some(module_runtime) = &module_runtime else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32601,
                        message: "Method not found".to_string(),
                        data: None,
                    }),
                );
            };
            let pending = module_runtime.lock().await.list_gating_pending();
            response_value(response_id, Some(json!({ "pending": pending })), None)
        }
        "mobkit/gating/audit" => {
            let Some(module_runtime) = &module_runtime else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32601,
                        message: "Method not found".to_string(),
                        data: None,
                    }),
                );
            };
            let limit = request
                .params
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50) as usize;
            let entries = module_runtime.lock().await.gating_audit_entries(limit);
            response_value(response_id, Some(json!({ "entries": entries })), None)
        }
        "mobkit/gating/decide" => {
            let Some(module_runtime) = &module_runtime else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32601,
                        message: "Method not found".to_string(),
                        data: None,
                    }),
                );
            };
            let Some(pending_id) = request.params.get("pending_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "pending_id required");
            };
            let Some(approver_id) = request.params.get("approver_id").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "approver_id required");
            };
            let Some(raw_decision) = request.params.get("decision").and_then(Value::as_str) else {
                return invalid_params(response_id, "decision required");
            };
            let decision = match raw_decision {
                "approve" => GatingDecision::Approve,
                "reject" | "deny" => GatingDecision::Reject,
                "escalate" => GatingDecision::Escalate,
                _ => {
                    return invalid_params(
                        response_id,
                        format!("unsupported decision: {raw_decision}"),
                    );
                }
            };
            let reason = request
                .params
                .get("reason")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            match module_runtime
                .lock()
                .await
                .decide_gating_action(GatingDecideRequest {
                    pending_id: pending_id.to_string(),
                    approver_id: approver_id.to_string(),
                    decision,
                    reason,
                }) {
                Ok(result) => response_value(
                    response_id,
                    Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => invalid_params(response_id, format!("gating decision failed: {err}")),
            }
        }
        "mobkit/ensure_member" => {
            let Some(role) = request.params.get("role").and_then(Value::as_str) else {
                return invalid_params(response_id, "role required");
            };
            let Some(agent_identity) = request.params.get("agent_identity").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "agent_identity required");
            };
            let labels = match request.params.get("labels") {
                None | Some(Value::Null) => std::collections::BTreeMap::new(),
                Some(value) => match serde_json::from_value(value.clone()) {
                    Ok(map) => map,
                    Err(err) => {
                        return invalid_params(response_id, format!("invalid labels: {err}"));
                    }
                },
            };
            let context = request.params.get("context").cloned();
            let resume_session_id = match request.params.get("resume_session_id") {
                None => None,
                Some(Value::Null) => None,
                Some(v) => match v.as_str() {
                    Some(s) => match meerkat_core::types::SessionId::parse(s) {
                        Ok(sid) => Some(sid),
                        Err(_) => {
                            return invalid_params(
                                response_id,
                                format!("invalid resume_session_id: {s:?}"),
                            );
                        }
                    },
                    None => {
                        return invalid_params(
                            response_id,
                            "resume_session_id must be a string".to_string(),
                        );
                    }
                },
            };
            let additional_instructions = match request.params.get("additional_instructions") {
                None | Some(Value::Null) => None,
                Some(Value::Array(arr)) => {
                    let mut strs = Vec::with_capacity(arr.len());
                    for (i, entry) in arr.iter().enumerate() {
                        match entry.as_str() {
                            Some(s) => strs.push(s.to_string()),
                            None => {
                                return invalid_params(
                                    response_id,
                                    format!("additional_instructions[{i}] must be a string"),
                                );
                            }
                        }
                    }
                    if strs.is_empty() { None } else { Some(strs) }
                }
                Some(_) => {
                    return invalid_params(
                        response_id,
                        "additional_instructions must be an array of strings",
                    );
                }
            };
            let mut spec =
                SpawnMemberSpec::new(ProfileName::from(role), MeerkatId::from(agent_identity));
            if !labels.is_empty() {
                spec = spec.with_labels(labels);
            }
            if let Some(ctx) = context {
                spec = spec.with_context(ctx);
            }
            if let Some(sid) = resume_session_id {
                spec = spec.with_resume_bridge_session_id(sid);
            }
            if let Some(instructions) = additional_instructions {
                spec = spec.with_additional_instructions(instructions);
            }
            let handle = runtime.handle();
            let mid = spec.identity.clone();
            match handle.ensure_member(spec).await {
                Ok(_outcome) => {
                    let body = match lookup_member_with_session(&handle, &mid).await {
                        Some((entry, _sid)) => member_entry_to_json(&entry),
                        None => Value::Null,
                    };
                    response_value(response_id, Some(body), None)
                }
                Err(err) => internal_error(response_id, format!("ensure_member failed: {err}")),
            }
        }
        "mobkit/retire_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime.handle().retire(MeerkatId::from(member_id)).await {
                Ok(()) => response_value(
                    response_id,
                    Some(serde_json::json!({ "accepted": true })),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("retire_member failed: {err}")),
            }
        }
        "mobkit/respawn_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime
                .handle()
                .respawn(MeerkatId::from(member_id), None)
                .await
            {
                Ok(_receipt) => response_value(
                    response_id,
                    Some(serde_json::json!({ "accepted": true })),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("respawn_member failed: {err}")),
            }
        }
        "mobkit/reconcile_edges" => response_value(
            response_id,
            Some(serde_json::json!({
                "status": "noop",
                "reason": "console runtime routes directly to MobRuntime",
            })),
            None,
        ),
        "mobkit/query_events" => {
            let query: EventQuery = match serde_json::from_value(request.params.clone()) {
                Ok(q) => q,
                Err(err) => {
                    return invalid_params(response_id, format!("invalid query params: {err}"));
                }
            };
            match event_log {
                Some(store) => match store.query(query.clone()).await {
                    Ok(events) => response_value(
                        response_id,
                        Some(project_query_events_for_console(events, &query)),
                        None,
                    ),
                    Err(err) => {
                        internal_error(response_id, format!("event log query failed: {err}"))
                    }
                },
                None => {
                    let fallback_events = match console_events {
                        Some(store) => store.query(&query).await,
                        None => Vec::new(),
                    };
                    response_value(
                        response_id,
                        Some(serde_json::json!({
                            "status": "no_event_log_configured",
                            "events": fallback_events,
                        })),
                        None,
                    )
                }
            }
        }
        "mobkit/mob_events/query" | "mobkit/mob_events/subscribe" => {
            let query: EventQuery = if request.params.is_null() {
                EventQuery::default()
            } else {
                match serde_json::from_value(request.params.clone()) {
                    Ok(q) => q,
                    Err(err) => {
                        return invalid_params(response_id, format!("invalid query params: {err}"));
                    }
                }
            };
            let Some(store) = mob_events.as_ref() else {
                return response_value(
                    response_id,
                    Some(serde_json::json!({
                        "events": [],
                        "next_after_seq": Value::Null,
                    })),
                    None,
                );
            };
            let events_view = runtime.handle().events();
            // Capture latest_cursor at handshake so the SSE continuation
            // URL still covers the empty-snapshot case without losing
            // events between the JSON-RPC response and the SSE connect.
            let latest_at_handshake = events_view.latest_cursor().await.unwrap_or(0);
            let result = crate::unified_runtime::mob_events::query_ledger_with_filter(
                &events_view,
                store,
                &query,
            )
            .await;
            match result {
                Ok(events) => {
                    let last_cursor = events.last().map(|event| event.cursor);
                    let body = if request.method == "mobkit/mob_events/subscribe" {
                        let subscribe_url = crate::unified_runtime::mob_events::build_subscribe_url(
                            &query,
                            last_cursor,
                            latest_at_handshake,
                        );
                        serde_json::json!({
                            "stream": "mob_events",
                            "events": events,
                            "next_after_seq": last_cursor,
                            "subscribe_url": subscribe_url,
                            "keep_alive": {
                                "interval_ms": 15_000_u64,
                                "event": "keep_alive",
                            },
                        })
                    } else {
                        serde_json::json!({
                            "events": events,
                            "next_after_seq": last_cursor,
                        })
                    };
                    response_value(response_id, Some(body), None)
                }
                Err(crate::unified_runtime::mob_events::MobEventsQueryError::Stale {
                    after_cursor,
                    latest_cursor,
                }) => stale_event_cursor_response(response_id, after_cursor, latest_cursor),
                Err(err) => internal_error(response_id, format!("mob_events query failed: {err}")),
            }
        }
        // 0.5 API methods
        "mobkit/member_status" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime
                .handle()
                .member_status(&MeerkatId::from(member_id))
                .await
            {
                Ok(snapshot) => response_value(
                    response_id,
                    Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("member_status failed: {err}")),
            }
        }
        "mobkit/force_cancel_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            match runtime
                .handle()
                .force_cancel_member(MeerkatId::from(member_id))
                .await
            {
                Ok(()) => response_value(
                    response_id,
                    Some(serde_json::json!({ "accepted": true })),
                    None,
                ),
                Err(err) => {
                    internal_error(response_id, format!("force_cancel_member failed: {err}"))
                }
            }
        }
        "mobkit/read_session_history" => {
            let Some(session_id) = request.params.get("session_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "session_id required");
            };
            let offset = request
                .params
                .get("offset")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(0);
            let limit = match request.params.get("limit") {
                Some(Value::Number(number)) => number.as_u64().map(|value| value as usize),
                Some(Value::Null) | None => None,
                Some(_) => return invalid_params(response_id, "limit must be a positive integer"),
            };
            match runtime
                .read_session_history(session_id, offset, limit)
                .await
            {
                Ok(page) => response_value(
                    response_id,
                    Some(serde_json::to_value(page).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => {
                    internal_error(response_id, format!("read_session_history failed: {err}"))
                }
            }
        }
        "mobkit/wait_ready" => {
            let timeout = request
                .params
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .map(std::time::Duration::from_millis);
            match runtime.handle().wait_for_ready(timeout).await {
                Ok(ready) => {
                    let entries: Vec<Value> = ready
                        .into_iter()
                        .map(|(identity, snapshot)| {
                            serde_json::json!({
                                "agent_identity": identity.to_string(),
                                "snapshot": serde_json::to_value(&snapshot)
                                    .unwrap_or(Value::Null),
                            })
                        })
                        .collect();
                    response_value(
                        response_id,
                        Some(serde_json::json!({
                            "ready": entries,
                            "timeout": false,
                        })),
                        None,
                    )
                }
                Err(err) => {
                    let message = err.to_string();
                    if message.to_lowercase().contains("timeout") {
                        response_value(
                            response_id,
                            Some(serde_json::json!({
                                "ready": Vec::<Value>::new(),
                                "timeout": true,
                            })),
                            None,
                        )
                    } else {
                        internal_error(response_id, format!("wait_for_ready failed: {message}"))
                    }
                }
            }
        }
        "mobkit/collect_completed" => {
            let completed = runtime.handle().collect_completed().await;
            let entries: Vec<Value> = completed
                .into_iter()
                .map(|(mid, snapshot)| {
                    serde_json::json!({
                        "member_id": mid.to_string(),
                        "snapshot": serde_json::to_value(&snapshot).unwrap_or(Value::Null),
                    })
                })
                .collect();
            response_value(
                response_id,
                Some(serde_json::json!({ "completed": entries })),
                None,
            )
        }
        "mobkit/cancel_flow" => {
            let Some(run_id) = request.params.get("run_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "run_id required");
            };
            let run_id: meerkat_mob::RunId = match run_id.parse() {
                Ok(id) => id,
                Err(_) => return invalid_params(response_id, "invalid run_id format"),
            };
            match runtime.handle().cancel_flow(run_id).await {
                Ok(()) => response_value(
                    response_id,
                    Some(serde_json::json!({ "accepted": true })),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("cancel_flow failed: {err}")),
            }
        }
        "mobkit/flow_status" => {
            let Some(run_id) = request.params.get("run_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "run_id required");
            };
            let run_id: meerkat_mob::RunId = match run_id.parse() {
                Ok(id) => id,
                Err(_) => return invalid_params(response_id, "invalid run_id format"),
            };
            match runtime.handle().flow_status(run_id).await {
                Ok(Some(mob_run)) => response_value(
                    response_id,
                    Some(serde_json::to_value(&mob_run).unwrap_or(Value::Null)),
                    None,
                ),
                Ok(None) => response_value(response_id, Some(Value::Null), None),
                Err(err) => internal_error(response_id, format!("flow_status failed: {err}")),
            }
        }
        "mobkit/list_flows" => {
            let flows: Vec<String> = runtime
                .handle()
                .list_flows()
                .into_iter()
                .map(|id| id.to_string())
                .collect();
            response_value(
                response_id,
                Some(serde_json::json!({ "flows": flows })),
                None,
            )
        }
        "mobkit/list_runs" => {
            let flow_id = request
                .params
                .get("flow_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(meerkat_mob::FlowId::from);
            match runtime.handle().list_runs(flow_id.as_ref()).await {
                Ok(runs) => response_value(
                    response_id,
                    Some(serde_json::json!({
                        "runs": serde_json::to_value(&runs).unwrap_or(Value::Null),
                    })),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("list_runs failed: {err}")),
            }
        }
        "mobkit/run_flow" => {
            let Some(flow_id_str) = request.params.get("flow_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "flow_id required");
            };
            if flow_id_str.is_empty() {
                return invalid_params(response_id, "flow_id required");
            }
            let flow_id = meerkat_mob::FlowId::from(flow_id_str);
            let flow_params = request.params.get("params").cloned().unwrap_or(Value::Null);
            match runtime.handle().run_flow(flow_id, flow_params).await {
                Ok(run_id) => response_value(
                    response_id,
                    Some(serde_json::json!({ "run_id": run_id.to_string() })),
                    None,
                ),
                Err(err) => invalid_params(response_id, format!("run_flow failed: {err}")),
            }
        }
        "mobkit/spawn_helper" => {
            let Some(agent_identity) = request.params.get("agent_identity").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "agent_identity required");
            };
            let Some(task) = request.params.get("task").and_then(Value::as_str) else {
                return invalid_params(response_id, "task required");
            };
            let options = match parse_console_helper_options(request.params.get("options")) {
                Ok(opts) => opts,
                Err(msg) => return invalid_params(response_id, msg),
            };
            let handle = runtime.handle();
            match handle
                .spawn_helper(MeerkatId::from(agent_identity), task, options)
                .await
            {
                Ok(result) => {
                    // Meerkat 0.6 retires the helper before `spawn_helper`
                    // returns, so a post-hoc `resolve_bridge_session_id`
                    // call would come back `None`. We drop `session_id`
                    // from the response rather than emit a misleading null.
                    response_value(
                        response_id,
                        Some(serde_json::json!({
                            "output": result.output,
                            "tokens_used": result.tokens_used,
                        })),
                        None,
                    )
                }
                Err(err) => internal_error(response_id, format!("spawn_helper failed: {err}")),
            }
        }
        "mobkit/fork_helper" => {
            let Some(source) = request
                .params
                .get("source_member_id")
                .and_then(Value::as_str)
            else {
                return invalid_params(response_id, "source_member_id required");
            };
            let Some(agent_identity) = request.params.get("agent_identity").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "agent_identity required");
            };
            let Some(task) = request.params.get("task").and_then(Value::as_str) else {
                return invalid_params(response_id, "task required");
            };
            let fork_context = match request.params.get("fork_context") {
                Some(v) if !v.is_null() => {
                    match serde_json::from_value::<meerkat_mob::launch::ForkContext>(v.clone()) {
                        Ok(ctx) => ctx,
                        Err(err) => {
                            return invalid_params(
                                response_id,
                                format!("invalid fork_context: {err}"),
                            );
                        }
                    }
                }
                _ => meerkat_mob::launch::ForkContext::default(),
            };
            let options = match parse_console_helper_options(request.params.get("options")) {
                Ok(opts) => opts,
                Err(msg) => return invalid_params(response_id, msg),
            };
            let handle = runtime.handle();
            match handle
                .fork_helper(
                    &MeerkatId::from(source),
                    MeerkatId::from(agent_identity),
                    task,
                    fork_context,
                    options,
                )
                .await
            {
                Ok(result) => {
                    // See `spawn_helper`: meerkat 0.6 retires the forked
                    // helper before returning, so session_id is omitted
                    // rather than silently null.
                    response_value(
                        response_id,
                        Some(serde_json::json!({
                            "output": result.output,
                            "tokens_used": result.tokens_used,
                        })),
                        None,
                    )
                }
                Err(err) => internal_error(response_id, format!("fork_helper failed: {err}")),
            }
        }
        "mobkit/attach_existing_session" => {
            let Some(role) = request.params.get("role").and_then(Value::as_str) else {
                return invalid_params(response_id, "role required");
            };
            let Some(agent_identity) = request.params.get("agent_identity").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "agent_identity required");
            };
            let Some(session_id_str) = request.params.get("session_id").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "session_id required");
            };
            let bridge_session_id = match meerkat_core::types::SessionId::parse(session_id_str) {
                Ok(s) => s,
                Err(_) => return invalid_params(response_id, "invalid session_id format"),
            };
            let mid = MeerkatId::from(agent_identity);
            let spec = SpawnMemberSpec::new(ProfileName::from(role), mid.clone())
                .with_launch_mode(MemberLaunchMode::Resume { bridge_session_id });
            let handle = runtime.handle();
            match handle.spawn_spec(spec).await {
                Ok(_) => match handle.member_status(&mid).await {
                    Ok(snapshot) => response_value(
                        response_id,
                        Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
                        None,
                    ),
                    Err(err) => internal_error(
                        response_id,
                        format!("attach_existing_session status lookup failed: {err}"),
                    ),
                },
                Err(err) => internal_error(
                    response_id,
                    format!("attach_existing_session failed: {err}"),
                ),
            }
        }
        "mobkit/cross_mob/wire_local" => {
            handle_console_wire_local(runtime, &request.params, response_id, true).await
        }
        "mobkit/cross_mob/unwire_local" => {
            handle_console_wire_local(runtime, &request.params, response_id, false).await
        }
        "mobkit/peer_pubkey" => match gateway_peer_keys {
            Some(keys) => response_value(
                response_id,
                Some(serde_json::json!({ "pubkey_b64": keys.pubkey_b64() })),
                None,
            ),
            None => response_value(
                response_id,
                None,
                Some(JsonRpcError {
                    code: -32004,
                    message: "gateway has no signing keypair configured".to_string(),
                    data: None,
                }),
            ),
        },
        "mobkit/cross_mob/peer_info" => {
            let member_id = request.params.get("member_id").and_then(Value::as_str);
            match member_id {
                Some(mid) if !mid.is_empty() => {
                    let handle = runtime.handle();
                    let mob_id = handle.mob_id().to_string();
                    let meerkat_id = MeerkatId::from(mid);
                    match handle.get_member(&meerkat_id).await {
                        Some(entry) => match entry.peer_id() {
                            Some(peer_id) => {
                                let comms_name = format!("{}/{}/{}", mob_id, entry.role, mid);
                                let address = format!("inproc://{comms_name}");
                                response_value(
                                    response_id,
                                    Some(serde_json::json!({
                                        "member_id": mid,
                                        "mob_id": mob_id,
                                        "comms_name": comms_name,
                                        "peer_id": peer_id,
                                        "address": address,
                                    })),
                                    None,
                                )
                            }
                            None => response_value(
                                response_id,
                                None,
                                Some(JsonRpcError {
                                    code: -32000,
                                    message: format!("member {mid:?} has no comms runtime"),
                                    data: None,
                                }),
                            ),
                        },
                        None => response_value(
                            response_id,
                            None,
                            Some(JsonRpcError {
                                code: -32000,
                                message: format!("member {mid:?} not found"),
                                data: None,
                            }),
                        ),
                    }
                }
                _ => invalid_params(response_id, "member_id required".to_string()),
            }
        }
        "mobkit/cross_mob/directory" => {
            let entries: Vec<Value> = contact_directory
                .map(|dir| {
                    dir.list()
                        .into_iter()
                        .filter_map(|e| serde_json::to_value(e).ok())
                        .collect()
                })
                .unwrap_or_default();
            response_value(
                response_id,
                Some(serde_json::json!({ "mobs": entries })),
                None,
            )
        }
        method
            if matches!(
                method,
                "mobkit/mob_labels/set"
                    | "mobkit/mob_labels/get"
                    | "mobkit/mob_labels/delete"
                    | "mobkit/run_labels/set"
                    | "mobkit/run_labels/get"
                    | "mobkit/run_labels/delete",
            ) =>
        {
            dispatch_label_method(
                method,
                metadata_table.as_deref(),
                runtime.handle().mob_id().as_str(),
                response_id,
                &request.params,
            )
            .await
        }
        _ => response_value(
            response_id,
            None,
            Some(JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
                data: None,
            }),
        ),
    }
}

/// Dispatch the six `mobkit/{mob,run}_labels/*` RPCs against a metadata table.
///
/// Single entrypoint shared by every label method; the dispatch arms in
/// `handle_console_runtime_rpc` simply delegate based on the matched method
/// name. Mirrors the unified-runtime handlers in `rpc::mob_methods` — both
/// transports project the same outcomes to the same wire shape.
async fn dispatch_label_method(
    method: &str,
    metadata_table: Option<&RuntimeMetadataTable>,
    mob_id: &str,
    response_id: Value,
    params: &Value,
) -> Value {
    let Some(table) = metadata_table else {
        return invalid_params(
            response_id,
            "metadata table not configured for this runtime",
        );
    };

    let scope = match method {
        "mobkit/mob_labels/set" | "mobkit/mob_labels/get" | "mobkit/mob_labels/delete" => {
            MetadataScope::Mob(mob_id.to_string())
        }
        _ => match crate::runtime::parse_run_id_param(params) {
            Ok(run_id) => MetadataScope::Run(mob_id.to_string(), run_id.to_string()),
            Err(message) => return invalid_params(response_id, message),
        },
    };

    let outcome = match method {
        "mobkit/mob_labels/set" | "mobkit/run_labels/set" => {
            crate::runtime::dispatch_labels_set(table, scope, params).await
        }
        "mobkit/mob_labels/get" | "mobkit/run_labels/get" => {
            crate::runtime::dispatch_labels_get(table, scope).await
        }
        "mobkit/mob_labels/delete" | "mobkit/run_labels/delete" => {
            crate::runtime::dispatch_labels_delete(table, scope).await
        }
        _ => unreachable!("dispatch_label_method called with non-label method: {method}"),
    };

    match outcome {
        crate::runtime::LabelRpcResult::Accepted => response_value(
            response_id,
            Some(serde_json::json!({"accepted": true})),
            None,
        ),
        crate::runtime::LabelRpcResult::Labels(labels) => response_value(
            response_id,
            Some(serde_json::json!({"labels": labels_to_json_value(&labels)})),
            None,
        ),
        crate::runtime::LabelRpcResult::InvalidParams(message) => {
            invalid_params(response_id, message)
        }
    }
}

/// Shared body for `mobkit/cross_mob/wire_local` and `unwire_local` over
/// the console transport. `wire = true` calls `MobHandle::wire`, `false`
/// calls `MobHandle::unwire`. Both share param parsing and response shape.
///
/// Non-inproc transports (`tcp://`, `uds://`) require a non-zero pubkey;
/// the caller may supply it via `remote_pubkey_b64` or rely on TOFU
/// flows configured at the contact-directory layer (which this handler
/// does not consult — it only sees the explicit params).
async fn handle_console_wire_local(
    runtime: &MobRuntime,
    params: &Value,
    response_id: Value,
    wire: bool,
) -> Value {
    let local = params.get("local_member_id").and_then(Value::as_str);
    let comms_name = params.get("remote_comms_name").and_then(Value::as_str);
    let peer_id = params.get("remote_peer_id").and_then(Value::as_str);
    let addr = params.get("remote_address").and_then(Value::as_str);

    let remote_pubkey = match params.get("remote_pubkey_b64") {
        None => None,
        Some(v) if v.is_null() => None,
        Some(v) => match v.as_str() {
            Some(s) if !s.is_empty() => match crate::auth::peer_keys::decode_pubkey_b64(s) {
                Ok(bytes) => Some(bytes),
                Err(err) => {
                    return invalid_params(response_id, format!("remote_pubkey_b64: {err}"));
                }
            },
            _ => None,
        },
    };

    let (local_id, cname, pid, address) = match (local, comms_name, peer_id, addr) {
        (Some(l), Some(c), Some(p), Some(a))
            if !l.is_empty() && !c.is_empty() && !p.is_empty() && !a.is_empty() =>
        {
            (l, c, p, a)
        }
        _ => {
            return invalid_params(
                response_id,
                "local_member_id, remote_comms_name, remote_peer_id, and remote_address required",
            );
        }
    };

    let is_inproc = address.starts_with("inproc://");
    let spec_result = match (is_inproc, remote_pubkey) {
        (true, None) => TrustedPeerDescriptor::test_only_unsigned(cname, pid, address),
        (true, Some(bytes)) => {
            TrustedPeerDescriptor::unsigned_with_pubkey(cname, pid, bytes, address)
        }
        (false, None) => {
            return invalid_params(
                response_id,
                "remote_pubkey_b64 is required for non-inproc transports",
            );
        }
        (false, Some(bytes)) => {
            if bytes == [0u8; 32] {
                return invalid_params(
                    response_id,
                    "remote_pubkey_b64 must be non-zero for non-inproc transports",
                );
            }
            TrustedPeerDescriptor::unsigned_with_pubkey(cname, pid, bytes, address)
        }
    };

    let spec = match spec_result {
        Ok(spec) => spec,
        Err(err) => {
            return invalid_params(response_id, format!("invalid peer spec: {err}"));
        }
    };

    let result = if wire {
        runtime
            .handle()
            .wire(MeerkatId::from(local_id), PeerTarget::External(spec))
            .await
    } else {
        runtime
            .handle()
            .unwire(MeerkatId::from(local_id), PeerTarget::External(spec))
            .await
    };

    let action = if wire { "wire_local" } else { "unwire_local" };
    match result {
        Ok(()) => response_value(
            response_id,
            Some(serde_json::json!({
                "accepted": true,
                "local_member_id": local_id,
                "remote_comms_name": cname,
            })),
            None,
        ),
        Err(err) => internal_error(response_id, format!("cross_mob/{action} failed: {err}")),
    }
}

async fn build_live_snapshot(
    runtime: &MobRuntime,
    config_module_ids: &[String],
    console_events: Option<&ConsoleEventStore>,
) -> ConsoleLiveSnapshot {
    let handle = runtime.handle();
    let running = matches!(
        handle.status().await.ok(),
        Some(MobState::Creating | MobState::Running)
    );
    let entries = handle.list_members_including_retiring().await;

    // Project each meerkat entry → mobkit's ConsoleMember, resolving the
    // current bridge session id that console consumers rely on.
    let mut members: Vec<crate::runtime::ConsoleMember> = Vec::with_capacity(entries.len());
    for entry in &entries {
        let session_id = handle
            .resolve_bridge_session_id(&entry.agent_identity)
            .await
            .map(|s| s.to_string());
        let model_capabilities = model_capabilities_for_member(
            &handle,
            runtime.session_service(),
            &entry.agent_identity,
        )
        .await;
        members.push(crate::runtime::ConsoleMember {
            agent_identity: entry.agent_identity.to_string(),
            role: entry.role.to_string(),
            state: match entry.state {
                meerkat_mob::MemberState::Active => MEMBER_STATE_ACTIVE.to_string(),
                meerkat_mob::MemberState::Retiring => MEMBER_STATE_RETIRING.to_string(),
            },
            model_capabilities,
            runtime_mode: Some(entry.runtime_mode.to_string()),
            session_id,
            wired_to: entry.wired_to.iter().map(ToString::to_string).collect(),
            labels: entry.labels.clone(),
        });
    }

    // Use configured module IDs when available because topology and health
    // surfaces describe loaded modules, not live mob members.
    // Fall back to member IDs only for pure mob runtimes with no module config.
    let loaded_modules = if config_module_ids.is_empty() {
        let mut mods: Vec<String> = members
            .iter()
            .filter(|member| member.state != MEMBER_STATE_RETIRING)
            .map(|member| member.agent_identity.clone())
            .collect();
        mods.sort();
        mods
    } else {
        let mut mods = config_module_ids.to_vec();
        mods.sort();
        mods
    };

    let agents = members
        .iter()
        .map(|member| async move {
            let label = member
                .labels
                .get("display_name")
                .cloned()
                .unwrap_or_else(|| member.agent_identity.clone());
            let watched = member
                .labels
                .get("console_watched")
                .map(|value: &String| value == "true");
            let alert_level = member
                .labels
                .get("console_alert_level")
                .filter(|value: &&String| matches!(value.as_str(), "elevated" | "critical"))
                .cloned();
            let degraded = member
                .labels
                .get("console_degraded")
                .map(|value: &String| value == "true");
            let degraded_reason = member.labels.get("console_degraded_reason").cloned();
            let response_phase = match console_events {
                Some(store) => {
                    store
                        .response_phase_for_identity(&member.agent_identity)
                        .await
                }
                None => None,
            };
            ConsoleAgentLiveSnapshot {
                agent_id: member.agent_identity.clone(),
                member_id: member.agent_identity.clone(),
                label,
                kind: "meerkat".to_string(),
                identity: Some(member.agent_identity.clone()),
                role: Some(member.role.clone()),
                state: Some(member.state.clone()),
                session_id: member.session_id.clone(),
                model_capabilities: member.model_capabilities.clone(),
                response_phase,
                watched,
                alert_level,
                degraded,
                degraded_reason,
            }
        })
        .collect::<Vec<_>>();
    let mut agents = join_all(agents).await;
    agents.sort_by(|left, right| left.label.cmp(&right.label));
    ConsoleLiveSnapshot::new(
        Some(runtime.handle().mob_id().to_string()),
        running,
        loaded_modules,
        agents,
        members,
        true,
    )
}

pub async fn console_frontend_index_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        CONSOLE_FRONTEND_INDEX_HTML,
    )
}

pub async fn console_frontend_app_js_handler() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        CONSOLE_FRONTEND_APP_JS,
    )
}

pub async fn console_frontend_app_css_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        CONSOLE_FRONTEND_APP_CSS,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_MULTIPART_BODY_BYTES, MAX_MULTIPART_IMAGE_BYTES, MultipartImageUpload,
        externalize_image_upload_placeholders, externalize_single_image_upload,
        mint_console_interaction_id, project_query_events_for_console,
    };
    use crate::blob_store::{BinaryBlobStore, ObjectStoreBlobStore};
    use crate::types::UnifiedEvent;
    use crate::unified_runtime::{EventQuery, PersistedEvent};
    use bytes::Bytes;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    #[test]
    fn mint_console_interaction_id_is_unique_across_same_tick_calls() {
        let ids = (0..32)
            .map(|_| mint_console_interaction_id())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 32);
    }

    #[test]
    fn multipart_body_limit_covers_configured_image_limit() {
        const _: () = assert!(MAX_MULTIPART_BODY_BYTES > MAX_MULTIPART_IMAGE_BYTES);
        const _: () = assert!(MAX_MULTIPART_BODY_BYTES > 2 * 1024 * 1024);
    }

    #[tokio::test]
    async fn multipart_blob_upload_stores_one_file() -> Result<(), Box<dyn std::error::Error>> {
        let store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let mut files = BTreeMap::new();
        files.insert(
            "upload-1".to_string(),
            MultipartImageUpload {
                media_type: "image/png".to_string(),
                bytes: Bytes::from_static(b"png-data"),
            },
        );
        let result = externalize_single_image_upload(
            &json!({
                "upload": {
                    "type": "image_upload",
                    "upload_id": "upload-1",
                    "media_type": "image/png"
                }
            }),
            files,
            store.clone(),
        )
        .await
        .map_err(std::io::Error::other)?;

        assert_eq!(result["media_type"], json!("image/png"));
        assert_eq!(result["size"], json!(8));
        let Some(blob_id) = result["blob_id"].as_str() else {
            return Err(std::io::Error::other("blob id").into());
        };
        let payload = store
            .get_bytes(&meerkat_core::BlobId::from(blob_id))
            .await?;
        assert_eq!(payload.data.as_ref(), b"png-data");
        Ok(())
    }

    #[tokio::test]
    async fn multipart_blob_upload_accepts_part_name_alias()
    -> Result<(), Box<dyn std::error::Error>> {
        let store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let mut files = BTreeMap::new();
        files.insert(
            "image-field".to_string(),
            MultipartImageUpload {
                media_type: "image/png".to_string(),
                bytes: Bytes::from_static(b"png-data"),
            },
        );
        let result = externalize_single_image_upload(
            &json!({
                "upload": {
                    "type": "image_upload",
                    "part_name": "image-field",
                    "media_type": "image/png"
                }
            }),
            files,
            store,
        )
        .await
        .map_err(std::io::Error::other)?;

        assert_eq!(result["media_type"], json!("image/png"));
        assert!(
            result["blob_id"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn multipart_blob_upload_rejects_media_mismatch() -> Result<(), Box<dyn std::error::Error>>
    {
        let store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let mut files = BTreeMap::new();
        files.insert(
            "upload-1".to_string(),
            MultipartImageUpload {
                media_type: "image/jpeg".to_string(),
                bytes: Bytes::from_static(b"jpeg-data"),
            },
        );
        let err = match externalize_single_image_upload(
            &json!({
                "upload": {
                    "type": "image_upload",
                    "upload_id": "upload-1",
                    "media_type": "image/png"
                }
            }),
            files,
            store,
        )
        .await
        {
            Ok(_) => return Err(std::io::Error::other("media mismatch").into()),
            Err(err) => err,
        };
        assert!(
            err.contains("media type mismatch"),
            "unexpected error: {err}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn multipart_blob_upload_rejects_extra_file() -> Result<(), Box<dyn std::error::Error>> {
        let store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let mut files = BTreeMap::new();
        for id in ["upload-1", "upload-2"] {
            files.insert(
                id.to_string(),
                MultipartImageUpload {
                    media_type: "image/png".to_string(),
                    bytes: Bytes::from_static(b"png"),
                },
            );
        }
        let err = match externalize_single_image_upload(
            &json!({
                "upload": {
                    "type": "image_upload",
                    "upload_id": "upload-1",
                    "media_type": "image/png"
                }
            }),
            files,
            store,
        )
        .await
        {
            Ok(_) => return Err(std::io::Error::other("one file only").into()),
            Err(err) => err,
        };
        assert!(
            err.contains("exactly one file part"),
            "unexpected error: {err}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn multipart_send_replaces_placeholders_and_removes_shadow_message()
    -> Result<(), Box<dyn std::error::Error>> {
        let store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let mut files = BTreeMap::new();
        files.insert(
            "upload-1".to_string(),
            MultipartImageUpload {
                media_type: "image/webp".to_string(),
                bytes: Bytes::from_static(b"webp-data"),
            },
        );
        let mut params = json!({
            "member_id": "artist",
            "message": "stale shadow text",
            "content": [
                { "type": "text", "text": "describe" },
                {
                    "type": "image_upload",
                    "upload_id": "upload-1",
                    "media_type": "image/webp"
                }
            ]
        });
        externalize_image_upload_placeholders(&mut params, files, store)
            .await
            .map_err(std::io::Error::other)?;

        assert!(params.get("message").is_none());
        assert_eq!(params["content"][1]["type"], json!("image"));
        assert_eq!(params["content"][1]["source"], json!("blob"));
        assert_eq!(params["content"][1]["media_type"], json!("image/webp"));
        assert!(
            params["content"][1]["blob_id"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn multipart_send_accepts_part_name_placeholder() -> Result<(), Box<dyn std::error::Error>>
    {
        let store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let mut files = BTreeMap::new();
        files.insert(
            "image-field".to_string(),
            MultipartImageUpload {
                media_type: "image/png".to_string(),
                bytes: Bytes::from_static(b"png-data"),
            },
        );
        let mut params = json!({
            "member_id": "analyst",
            "content": [
                { "type": "text", "text": "describe" },
                {
                    "type": "image_upload",
                    "part_name": "image-field",
                    "media_type": "image/png"
                }
            ]
        });

        externalize_image_upload_placeholders(&mut params, files, store)
            .await
            .map_err(std::io::Error::other)?;

        assert_eq!(params["content"][1]["type"], json!("image"));
        assert_eq!(params["content"][1]["source"], json!("blob"));
        assert_eq!(params["content"][1]["media_type"], json!("image/png"));
        Ok(())
    }

    #[tokio::test]
    async fn multipart_send_rejects_placeholder_without_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let store: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let mut params = json!({
            "content": [{
                "type": "image_upload",
                "upload_id": "missing",
                "media_type": "image/png"
            }]
        });
        let err = match externalize_image_upload_placeholders(&mut params, BTreeMap::new(), store)
            .await
        {
            Ok(()) => return Err(std::io::Error::other("missing file").into()),
            Err(err) => err,
        };
        assert!(err.contains("missing file part"), "unexpected error: {err}");
        Ok(())
    }

    #[test]
    fn query_events_projects_assistant_images_for_console_history()
    -> Result<(), Box<dyn std::error::Error>> {
        let events = vec![PersistedEvent {
            id: "evt-image-tool".to_string(),
            seq: 7,
            timestamp_ms: 42,
            member_id: Some("rt:identity:luka:0".to_string()),
            event: UnifiedEvent::Agent {
                agent_id: "rt:identity:luka:0".to_string(),
                event_type: "tool_execution_completed".to_string(),
                payload: Some(serde_json::json!({
                    "id": "tool-call-1",
                    "name": "generate_image",
                    "result": {
                        "operation_id": "00000000-0000-0000-0000-000000000001",
                        "terminal": { "terminal": "generated" },
                        "images": [{
                            "image_id": "00000000-0000-0000-0000-000000000002",
                            "blob_ref": {
                                "blob_id": "generated-blob-1",
                                "media_type": "image/png"
                            },
                            "media_type": "image/png",
                            "width": 640,
                            "height": 480
                        }],
                        "provider_text": { "disposition": "not_emitted" },
                        "revised_prompt": { "disposition": "unchanged" },
                        "native_metadata": { "provider": "not_emitted" }
                    }
                })),
            },
        }];

        let projected = project_query_events_for_console(
            events,
            &EventQuery {
                identity: Some("identity:luka".to_string()),
                ..EventQuery::default()
            },
        );
        let serde_json::Value::Array(items) = projected else {
            return Err(std::io::Error::other("array projection").into());
        };

        assert_eq!(items.len(), 2);
        assert_eq!(items[1]["event_type"], "assistant_image");
        assert_eq!(items[1]["identity"], "identity:luka");
        assert_eq!(items[1]["data"]["blob_id"], "generated-blob-1");
        Ok(())
    }

    #[test]
    fn query_events_hides_empty_web_search_annotation_history()
    -> Result<(), Box<dyn std::error::Error>> {
        let events = vec![PersistedEvent {
            id: "evt-annotation".to_string(),
            seq: 8,
            timestamp_ms: 42,
            member_id: Some("rt:identity:luka:0".to_string()),
            event: UnifiedEvent::Agent {
                agent_id: "rt:identity:luka:0".to_string(),
                event_type: "server_tool_content".to_string(),
                payload: Some(serde_json::json!({
                    "id": "msg-1",
                    "name": "web_search_annotations",
                    "content": {
                        "type": "message_annotations",
                        "annotations": []
                    }
                })),
            },
        }];

        let projected = project_query_events_for_console(events, &EventQuery::default());
        let serde_json::Value::Array(items) = projected else {
            return Err(std::io::Error::other("array projection").into());
        };

        assert!(items.is_empty());
        Ok(())
    }
}
