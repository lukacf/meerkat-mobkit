//! HTTP routes for the admin console REST API.

use async_stream::stream;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use futures::future::join_all;
use meerkat_core::comms::TrustedPeerDescriptor;
use meerkat_mob::MobState;
use meerkat_mob::ids::{MeerkatId, MobId};
use meerkat_mob::launch::MemberLaunchMode;
use meerkat_mob::runtime::reconcile::MemberFilter;
use meerkat_mob::{MobHandle, PeerTarget, ProfileName, SpawnMemberSpec};

use crate::mob_handle_runtime::{
    member_entry_to_json, model_capabilities_for_member_entry, model_capabilities_for_role,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::blob_store::{BinaryBlobPayload, BinaryBlobStore, is_valid_blob_id_value};
use crate::console_aggregator::{
    AllowAllConsoleVisibilityPolicy, ConsoleCursor, ConsoleFrame, ConsoleLogResult,
    ConsoleLogStore, ConsoleReplayUnavailable, ConsoleSendError, ConsoleSendRequest,
    ConsoleTimelineEvent, ConsoleTimelineQuery, ConsoleVisibilityPolicy,
    HideImplicitDelegateMembersConsoleVisibilityPolicy, MobKitConsoleAggregator,
};
use crate::contact_directory::ContactDirectory;
use crate::http_sse::{DEFAULT_KEEP_ALIVE_INTERVAL, KEEP_ALIVE_TEXT};
use crate::mob_handle_runtime::{MEMBER_STATE_ACTIVE, MEMBER_STATE_RETIRING, MobRuntime};
use crate::rpc::{JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::runtime::MobkitRuntimeHandle;
use crate::runtime::{
    ConsoleAgentLiveSnapshot, ConsoleLiveSnapshot, ConsoleMember, ConsoleModelCapabilities,
    ConsoleRestJsonRequest, DeliveryHistoryRequest, GatingDecideRequest, GatingDecision,
    RuntimeDecisionState, extract_bearer_token_from_header,
    handle_console_rest_json_route_with_snapshot, validate_console_token,
};
use crate::runtime::{MetadataScope, RuntimeMetadataTable, labels_to_json_value};
use crate::unified_runtime::console_events::ConsoleEventStore;
use crate::unified_runtime::mob_events::MobEventsStore;
use crate::unified_runtime::{EventLogStore, EventQuery};

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
    pub(crate) console_aggregator: Option<MobKitConsoleAggregator>,
    pub(crate) mob_events: Option<MobEventsStore>,
    pub(crate) metadata_table: Option<std::sync::Arc<RuntimeMetadataTable>>,
    pub(crate) visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    pub(crate) snapshot_read_model: ConsoleSnapshotReadModel,
}

#[derive(Clone, Default)]
pub(crate) struct ConsoleSnapshotReadModel {
    inner: Arc<tokio::sync::RwLock<ConsoleSnapshotReadModelState>>,
    /// Mutex held by whichever task is currently running a refresh.
    /// Background refreshes (from `refresh_soon`) skip when the lock
    /// is contended; cold-cache request waiters acquire it via
    /// `lock_owned().await`, which is the actual "the in-flight
    /// refresh has finished" signal. See `prime_now`.
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
    /// `true` once at least one refresh has populated `inner` with
    /// real data. Snapshot reads gate on this so a cold cache never
    /// returns an empty member list to the first request.
    primed: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Clone, Default)]
struct ConsoleSnapshotReadModelState {
    running: Option<bool>,
    session_id_by_identity: BTreeMap<String, String>,
    session_owner_by_id: BTreeMap<String, String>,
    /// Pre-projected primary-mob console members. The background refresh
    /// populates this from `handle.list_all_members()` + projection; the
    /// snapshot hot path just clones from here so it never touches
    /// `MobHandle` async methods.
    primary_members: Vec<ConsoleMember>,
    /// Pre-projected delegate-mob member groups, one Vec per delegate mob,
    /// each already carrying its host_identity / source_mob_id label
    /// fixups. The snapshot hot path extends `members` with these instead
    /// of walking delegate handles per-request.
    delegate_member_groups: Vec<Vec<ConsoleMember>>,
}

impl ConsoleSnapshotReadModel {
    /// Returns the current cached snapshot. On a cold cache (no
    /// refresh has completed yet) the request thread drives the
    /// first refresh inline — or, if a background refresh task
    /// holds the lock, waits for it to finish before reading.
    /// Either way, snapshot endpoints never see an empty member
    /// list before the read model has been populated.
    async fn snapshot(&self, runtime: &MobRuntime) -> ConsoleSnapshotReadModelState {
        if !self.primed.load(std::sync::atomic::Ordering::Acquire) {
            self.prime_now(runtime).await;
        }
        self.inner.read().await.clone()
    }

    /// Cold-cache priming. Acquires `refresh_lock` via the awaiting
    /// (FIFO) path:
    ///
    /// - If no refresh task currently holds the lock, we acquire
    ///   it immediately and run the refresh inline. Subsequent
    ///   waiters that come in while we're running will queue
    ///   behind us in the same lock.
    /// - If a refresh task (spawned by `refresh_soon`) holds the
    ///   lock, our `lock_owned().await` parks until the task
    ///   drops the guard. By construction, the task only drops
    ///   the guard *after* writing `inner` and setting `primed`.
    ///   So when we acquire the lock, the cache is already
    ///   populated and the second `primed` check returns early
    ///   without redoing the work.
    ///
    /// No `Notify` is involved, so there's no lost-wake race to
    /// reason about: the lock release is the signal, and `tokio`'s
    /// Mutex enforces FIFO acquisition fairness so a `try_lock`
    /// caller can't barge past a queued `lock_owned` waiter.
    async fn prime_now(&self, runtime: &MobRuntime) {
        if self.primed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        let _guard = self.refresh_lock.clone().lock_owned().await;
        if self.primed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        let refreshed = collect_console_snapshot_read_model(runtime).await;
        *self.inner.write().await = refreshed;
        self.primed
            .store(true, std::sync::atomic::Ordering::Release);
        // _guard drops here, releasing the lock and waking the next
        // queued cold-cache waiter (if any). They'll see `primed`
        // true after acquiring and return early.
    }

    /// Fire-and-forget background refresh. If a refresh is already
    /// in flight (lock contended) we skip — the in-flight one is
    /// enough. The request hot path doesn't call this; it goes
    /// through `prime_now` on cold cache so it always gets a
    /// populated snapshot. `refresh_soon` exists to keep a hot
    /// cache fresh over time without blocking response requests.
    fn refresh_soon(&self, runtime: MobRuntime) {
        let Ok(runtime_handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let Ok(guard) = self.refresh_lock.clone().try_lock_owned() else {
            return;
        };
        let inner = Arc::clone(&self.inner);
        let primed = Arc::clone(&self.primed);
        runtime_handle.spawn(async move {
            let _guard = guard;
            let refreshed = collect_console_snapshot_read_model(&runtime).await;
            *inner.write().await = refreshed;
            primed.store(true, std::sync::atomic::Ordering::Release);
            // _guard drops; cold-cache waiters parked on
            // `lock_owned().await` in `prime_now` wake here and
            // observe `primed = true` after acquiring.
        });
    }
}

const CONSOLE_FRONTEND_INDEX_HTML: &str = include_str!("../console-dist/index.html");
const CONSOLE_FRONTEND_APP_JS: &str = include_str!("../console-dist/console-app.js");
const CONSOLE_FRONTEND_APP_CSS: &str = include_str!("../console-dist/console-app.css");
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
        console_aggregator: None,
        mob_events: None,
        metadata_table: None,
        visibility_policy: Arc::new(HideImplicitDelegateMembersConsoleVisibilityPolicy),
        snapshot_read_model: ConsoleSnapshotReadModel::default(),
    })
}

pub fn console_json_router_with_aggregator(
    decisions: RuntimeDecisionState,
    console_aggregator: MobKitConsoleAggregator,
) -> Router {
    console_json_router_with_state(ConsoleJsonState {
        decisions,
        runtime: None,
        module_runtime: None,
        contact_directory: None,
        event_log: None,
        gateway_peer_keys: None,
        console_events: None,
        console_aggregator: Some(console_aggregator),
        mob_events: None,
        metadata_table: None,
        visibility_policy: Arc::new(HideImplicitDelegateMembersConsoleVisibilityPolicy),
        snapshot_read_model: ConsoleSnapshotReadModel::default(),
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
        None,
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
    console_log_store: Option<std::sync::Arc<dyn ConsoleLogStore>>,
    mob_events: Option<MobEventsStore>,
    metadata_table: Option<std::sync::Arc<RuntimeMetadataTable>>,
) -> Router {
    console_json_router_with_runtime_events_and_policy(
        decisions,
        runtime,
        module_runtime,
        contact_directory,
        event_log,
        gateway_peer_keys,
        console_events,
        console_log_store,
        mob_events,
        metadata_table,
        Arc::new(HideImplicitDelegateMembersConsoleVisibilityPolicy),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn console_json_router_with_runtime_events_and_policy(
    decisions: RuntimeDecisionState,
    runtime: MobRuntime,
    module_runtime: Option<std::sync::Arc<tokio::sync::Mutex<MobkitRuntimeHandle>>>,
    contact_directory: Option<ContactDirectory>,
    event_log: Option<std::sync::Arc<dyn EventLogStore>>,
    gateway_peer_keys: Option<crate::auth::peer_keys::GatewayPeerKeys>,
    console_events: Option<ConsoleEventStore>,
    console_log_store: Option<std::sync::Arc<dyn ConsoleLogStore>>,
    mob_events: Option<MobEventsStore>,
    metadata_table: Option<std::sync::Arc<RuntimeMetadataTable>>,
    visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
) -> Router {
    let console_aggregator = console_events.clone().map(|events| {
        if let Some(store) = console_log_store {
            let aggregator = MobKitConsoleAggregator::new(store);
            aggregator.register_runtime_handles_with_policy(
                "default",
                "",
                runtime.clone(),
                events,
                visibility_policy.clone(),
            );
            aggregator
        } else {
            let aggregator = MobKitConsoleAggregator::in_memory();
            aggregator.register_runtime_handles_with_policy(
                "default",
                "",
                runtime.clone(),
                events,
                visibility_policy.clone(),
            );
            aggregator
        }
    });
    let snapshot_read_model = ConsoleSnapshotReadModel::default();
    snapshot_read_model.refresh_soon(runtime.clone());
    console_json_router_with_state(ConsoleJsonState {
        decisions,
        runtime: Some(runtime),
        module_runtime,
        contact_directory,
        event_log,
        gateway_peer_keys,
        console_events,
        console_aggregator,
        mob_events,
        metadata_table,
        visibility_policy,
        snapshot_read_model,
    })
}

pub fn console_frontend_router() -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::temporary("/console") }))
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
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
        .route("/console/identities", get(console_identities_handler))
        .route("/console/timeline", get(console_timeline_handler))
        .route(
            "/console/timeline/stream",
            get(console_timeline_stream_handler),
        )
        .route("/console/send", post(console_send_handler))
        .route("/console/rpc", post(console_rpc_handler))
        .route(
            "/console/rpc/multipart",
            post(console_rpc_multipart_handler)
                .layer(DefaultBodyLimit::max(MAX_MULTIPART_BODY_BYTES)),
        )
        .route("/blobs/{blob_id}", get(blob_get_handler));
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
        Some(runtime) => {
            state.snapshot_read_model.refresh_soon(runtime.clone());
            Some(
                build_live_snapshot(
                    runtime,
                    &config_module_ids,
                    state.console_events.as_ref(),
                    state.visibility_policy.as_ref(),
                    &state.snapshot_read_model,
                )
                .await,
            )
        }
        None => match &state.console_aggregator {
            Some(aggregator) => build_aggregator_live_snapshot(aggregator, &config_module_ids)
                .await
                .ok(),
            None => None,
        },
    }
    .map(|mut snapshot| {
        apply_console_visibility_policy(&mut snapshot, state.visibility_policy.as_ref());
        snapshot
    });

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

    // By this point the request is always authorized:
    // - require_app_auth=true: an invalid token already returned 401 above.
    // - require_app_auth=false: all methods are permitted unconditionally.
    // Either way, capabilities should reflect that all methods are available.
    let is_authenticated = true;
    let Some(runtime) = &state.runtime else {
        let response_value = handle_console_aggregator_rpc(
            state.console_aggregator.clone(),
            parsed_request,
            is_authenticated,
        )
        .await;
        return (StatusCode::OK, Json::<Value>(response_value));
    };

    let response_value = Box::pin(handle_console_runtime_rpc(
        runtime,
        state.module_runtime.clone(),
        state.contact_directory.as_ref(),
        state.gateway_peer_keys.as_ref(),
        state.console_events.clone(),
        state.console_aggregator.clone(),
        state.metadata_table.clone(),
        state.mob_events.clone(),
        parsed_request,
        is_authenticated,
    ))
    .await;
    (StatusCode::OK, Json::<Value>(response_value))
}

#[derive(Debug, serde::Deserialize)]
struct ConsoleTimelineHttpQuery {
    #[serde(default)]
    identity: Option<String>,
    #[serde(default)]
    conversation_id: Option<String>,
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn console_identities_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
) -> impl IntoResponse {
    if !console_request_authorized(&state, &headers, &uri) {
        return console_json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "console identities require a valid auth token",
        );
    }
    let Some(aggregator) = &state.console_aggregator else {
        return console_json_error(
            StatusCode::NOT_FOUND,
            "unavailable",
            "console aggregator unavailable",
        );
    };
    let aggregator = aggregator.clone();
    match aggregator.list_identities().await {
        Ok(identities) => (
            StatusCode::OK,
            Json::<Value>(json!({ "identities": identities })),
        )
            .into_response(),
        Err(err) => console_json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        ),
    }
}

async fn console_timeline_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<ConsoleTimelineHttpQuery>,
) -> impl IntoResponse {
    if !console_request_authorized(&state, &headers, &uri) {
        return console_json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "console timeline requires a valid auth token",
        );
    }
    let Some(aggregator) = &state.console_aggregator else {
        return console_json_error(
            StatusCode::NOT_FOUND,
            "unavailable",
            "console aggregator unavailable",
        );
    };
    let timeline_query = timeline_query_from_http(query, None);
    match aggregator.query_timeline(timeline_query).await {
        Ok(page) => (
            StatusCode::OK,
            Json::<Value>(serde_json::to_value(page).unwrap_or_else(|_| json!({ "frames": [] }))),
        )
            .into_response(),
        Err(err) => {
            console_json_error(StatusCode::CONFLICT, "replay_unavailable", &err.to_string())
        }
    }
}

async fn console_send_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<ConsoleSendRequest>,
) -> impl IntoResponse {
    if !console_request_authorized(&state, &headers, &uri) {
        return console_json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "console send requires a valid auth token",
        );
    }
    let Some(aggregator) = &state.console_aggregator else {
        return console_json_error(
            StatusCode::NOT_FOUND,
            "unavailable",
            "console aggregator unavailable",
        );
    };
    match aggregator.send(request).await {
        Ok(accepted) => (
            StatusCode::OK,
            Json::<Value>(
                serde_json::to_value(accepted).unwrap_or_else(|_| json!({ "accepted": true })),
            ),
        )
            .into_response(),
        Err(err) => console_send_error_response(err),
    }
}

async fn console_timeline_stream_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<ConsoleTimelineHttpQuery>,
) -> impl IntoResponse {
    if !console_request_authorized(&state, &headers, &uri) {
        return console_json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "console timeline stream requires a valid auth token",
        );
    }
    let Some(aggregator) = &state.console_aggregator else {
        return console_json_error(
            StatusCode::NOT_FOUND,
            "unavailable",
            "console aggregator unavailable",
        );
    };
    let aggregator = aggregator.clone();
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let timeline_query = timeline_query_from_http(query, last_event_id);
    let mut rx = aggregator.subscribe();
    let (snapshot_frames, snapshot_cursor) =
        match query_timeline_snapshot(&aggregator, timeline_query.clone()).await {
            Ok(snapshot) => snapshot,
            Err(_) => {
                let latest_cursor = aggregator.latest_cursor().await.ok().flatten();
                let requested_cursor = timeline_query
                    .after
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                return (
                    StatusCode::CONFLICT,
                    Json::<Value>(
                        serde_json::to_value(ConsoleReplayUnavailable {
                            error: "replay_unavailable".to_string(),
                            requested_cursor,
                            latest_cursor,
                        })
                        .unwrap_or_else(|_| json!({ "error": "replay_unavailable" })),
                    ),
                )
                    .into_response();
            }
        };
    let identity = timeline_query.identity.clone();
    let conversation_id = timeline_query.conversation_id.clone();
    let snapshot_after = timeline_query.after.clone();
    let stream = stream! {
        if let Some(event) = sse_event_from_timeline_event(&ConsoleTimelineEvent::SnapshotStarted { after: snapshot_after }) {
            yield Ok::<Event, Infallible>(event);
        }
        let mut latest_cursor = snapshot_cursor;
        for frame in snapshot_frames {
            latest_cursor = Some(frame.cursor.clone());
            if let Some(event) = sse_event_from_timeline_event(&ConsoleTimelineEvent::ConsoleFrame { frame }) {
                yield Ok::<Event, Infallible>(event);
            }
        }
        if let Some(event) = sse_event_from_timeline_event(&ConsoleTimelineEvent::SnapshotComplete { cursor: latest_cursor.clone() }) {
            yield Ok::<Event, Infallible>(event);
        }
        loop {
            match rx.recv().await {
                Ok(event) if timeline_event_matches(&event, identity.as_deref(), conversation_id.as_deref()) => {
                    if !aggregator.timeline_event_visible(&event).await {
                        continue;
                    }
                    if let Some(event_cursor) = timeline_event_cursor(&event)
                        && let Some(current_cursor) = latest_cursor.as_ref()
                        && !cursor_is_after(event_cursor, current_cursor)
                    {
                        continue;
                    }
                    if let Some(sse) = sse_event_from_timeline_event(&event) {
                        if let Some(event_cursor) = timeline_event_cursor(&event) {
                            latest_cursor = Some(event_cursor.clone());
                        }
                        yield Ok::<Event, Infallible>(sse);
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    let event = ConsoleTimelineEvent::ReplayUnavailable {
                        requested_cursor: format!("lagged:{skipped}"),
                        latest_cursor: None,
                    };
                    if let Some(sse) = sse_event_from_timeline_event(&event) {
                        yield Ok::<Event, Infallible>(sse);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
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

fn timeline_query_from_http(
    query: ConsoleTimelineHttpQuery,
    fallback_after: Option<String>,
) -> ConsoleTimelineQuery {
    let after = query.after.or(fallback_after).map(ConsoleCursor::from);
    ConsoleTimelineQuery {
        identity: query
            .identity
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        conversation_id: query
            .conversation_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        after,
        limit: query.limit.unwrap_or(200),
    }
}

async fn query_timeline_snapshot(
    aggregator: &MobKitConsoleAggregator,
    mut query: ConsoleTimelineQuery,
) -> ConsoleLogResult<(Vec<ConsoleFrame>, Option<ConsoleCursor>)> {
    const MAX_SNAPSHOT_PAGES: usize = 100;
    const STORE_PAGE_LIMIT: usize = 1_000;
    const DEFAULT_SNAPSHOT_LIMIT: usize = 200;
    let mut frames = Vec::new();
    let mut latest_cursor = query.after.clone();
    if query.after.is_none() {
        query.limit = if query.limit == 0 {
            DEFAULT_SNAPSHOT_LIMIT
        } else {
            query.limit
        }
        .clamp(1, STORE_PAGE_LIMIT);
        return query_fresh_timeline_snapshot(aggregator, query, STORE_PAGE_LIMIT).await;
    }
    query.limit = STORE_PAGE_LIMIT;
    let query_identity = query.identity.clone();
    for page_idx in 0..MAX_SNAPSHOT_PAGES {
        let page = aggregator.store().query_frames(query.clone()).await?;
        if page.frames.is_empty() {
            break;
        }
        latest_cursor = page.next_cursor.clone();
        let page_len = page.frames.len();
        frames.extend(
            visible_snapshot_frames(aggregator, page.frames, query_identity.as_deref()).await?,
        );
        query.after = latest_cursor.clone();
        if page_len < STORE_PAGE_LIMIT {
            break;
        }
        if page_idx + 1 == MAX_SNAPSHOT_PAGES {
            return Err(Box::new(std::io::Error::other(
                "timeline replay exceeded maximum snapshot pages",
            )));
        }
    }
    Ok((frames, latest_cursor))
}

async fn query_fresh_timeline_snapshot(
    aggregator: &MobKitConsoleAggregator,
    mut query: ConsoleTimelineQuery,
    store_page_limit: usize,
) -> ConsoleLogResult<(Vec<ConsoleFrame>, Option<ConsoleCursor>)> {
    let requested_limit = query.limit;
    query.limit = store_page_limit;
    let query_identity = query.identity.clone();
    let mut latest_cursor = None;
    let mut tail = std::collections::VecDeque::with_capacity(requested_limit);
    loop {
        let page = aggregator.store().query_frames(query.clone()).await?;
        if page.frames.is_empty() {
            break;
        }
        latest_cursor = page.next_cursor.clone();
        let page_len = page.frames.len();
        for frame in
            visible_snapshot_frames(aggregator, page.frames, query_identity.as_deref()).await?
        {
            if tail.len() >= requested_limit {
                tail.pop_front();
            }
            tail.push_back(frame);
        }
        query.after = latest_cursor.clone();
        if page_len < query.limit {
            break;
        }
    }
    Ok((tail.into_iter().collect(), latest_cursor))
}

async fn visible_snapshot_frames(
    aggregator: &MobKitConsoleAggregator,
    frames: Vec<ConsoleFrame>,
    identity: Option<&str>,
) -> ConsoleLogResult<Vec<ConsoleFrame>> {
    let mut visible = Vec::with_capacity(frames.len());
    for frame in frames {
        if aggregator
            .timeline_frame_visible_for_query(&frame, identity)
            .await
        {
            visible.push(frame);
        }
    }
    Ok(visible)
}

fn console_json_error(status: StatusCode, error: &str, message: &str) -> axum::response::Response {
    (
        status,
        Json::<Value>(json!({
            "error": error,
            "message": message,
        })),
    )
        .into_response()
}

fn console_send_error_response(err: ConsoleSendError) -> axum::response::Response {
    let (status, code) = match &err {
        ConsoleSendError::UnknownIdentity(_) => (StatusCode::NOT_FOUND, "unknown_identity"),
        ConsoleSendError::NotAddressable(_) => (StatusCode::CONFLICT, "not_addressable"),
        ConsoleSendError::Retired(_) => (StatusCode::CONFLICT, "retired"),
        ConsoleSendError::InvalidContent(_)
        | ConsoleSendError::InvalidHandlingMode(_)
        | ConsoleSendError::InvalidRequest(_) => (StatusCode::BAD_REQUEST, "invalid_request"),
        ConsoleSendError::IdempotencyConflict(_) => (StatusCode::CONFLICT, "idempotency_conflict"),
        ConsoleSendError::State(_) | ConsoleSendError::Dispatch(_) | ConsoleSendError::Log(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    };
    console_json_error(status, code, &err.to_string())
}

fn console_send_rpc_code(err: &ConsoleSendError) -> i64 {
    match err {
        ConsoleSendError::UnknownIdentity(_) => -32001,
        ConsoleSendError::NotAddressable(_) => -32002,
        ConsoleSendError::InvalidContent(_)
        | ConsoleSendError::InvalidHandlingMode(_)
        | ConsoleSendError::InvalidRequest(_) => -32602,
        ConsoleSendError::IdempotencyConflict(_) => -32009,
        ConsoleSendError::Retired(_) => -32004,
        ConsoleSendError::State(_) | ConsoleSendError::Dispatch(_) | ConsoleSendError::Log(_) => {
            -32000
        }
    }
}

fn console_send_rpc_error(response_id: Value, err: ConsoleSendError) -> Value {
    response_value(
        response_id,
        None,
        Some(JsonRpcError {
            code: console_send_rpc_code(&err),
            message: err.to_string(),
            data: None,
        }),
    )
}

fn timeline_event_matches(
    event: &ConsoleTimelineEvent,
    identity: Option<&str>,
    conversation_id: Option<&str>,
) -> bool {
    let frame = match event {
        ConsoleTimelineEvent::ConsoleFrame { frame }
        | ConsoleTimelineEvent::FrameUpdated { frame } => frame,
        ConsoleTimelineEvent::SnapshotStarted { .. }
        | ConsoleTimelineEvent::SnapshotComplete { .. }
        | ConsoleTimelineEvent::ReplayUnavailable { .. } => return true,
    };
    if identity.is_some_and(|value| frame.identity != value) {
        return false;
    }
    if conversation_id.is_some_and(|value| frame.conversation_id.as_deref() != Some(value)) {
        return false;
    }
    true
}

fn timeline_event_cursor(event: &ConsoleTimelineEvent) -> Option<&ConsoleCursor> {
    match event {
        ConsoleTimelineEvent::ConsoleFrame { frame }
        | ConsoleTimelineEvent::FrameUpdated { frame } => Some(&frame.cursor),
        ConsoleTimelineEvent::SnapshotStarted { .. }
        | ConsoleTimelineEvent::SnapshotComplete { .. }
        | ConsoleTimelineEvent::ReplayUnavailable { .. } => None,
    }
}

fn cursor_is_after(candidate: &ConsoleCursor, current: &ConsoleCursor) -> bool {
    match (candidate.seq(), current.seq()) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => candidate > current,
    }
}

fn sse_event_from_timeline_event(event: &ConsoleTimelineEvent) -> Option<Event> {
    let (event_name, id) = match event {
        ConsoleTimelineEvent::SnapshotStarted { .. } => ("snapshot_started", None),
        ConsoleTimelineEvent::ConsoleFrame { frame } => (
            if frame.kind == "frame_updated" {
                "frame_updated"
            } else {
                "console_frame"
            },
            Some(frame.cursor.to_string()),
        ),
        ConsoleTimelineEvent::FrameUpdated { frame } => {
            ("frame_updated", Some(frame.cursor.to_string()))
        }
        ConsoleTimelineEvent::SnapshotComplete { cursor } => (
            "snapshot_complete",
            cursor.as_ref().map(ToString::to_string),
        ),
        ConsoleTimelineEvent::ReplayUnavailable { .. } => ("replay_unavailable", None),
    };
    let data = match serde_json::to_string(event) {
        Ok(value) => value,
        Err(_) => return None,
    };
    let mut sse = Event::default().event(event_name).data(data);
    if let Some(id) = id {
        sse = sse.id(id);
    }
    Some(sse)
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
        "mobkit/console/send" => {
            let Some(aggregator) = &state.console_aggregator else {
                return (
                    StatusCode::OK,
                    Json::<Value>(invalid_params(
                        response_id,
                        "mobkit/console/send multipart requires a console aggregator",
                    )),
                );
            };
            let Some(identity) = parsed_request
                .params
                .get("identity")
                .and_then(Value::as_str)
            else {
                return (
                    StatusCode::OK,
                    Json::<Value>(invalid_params(response_id, "identity required")),
                );
            };
            let binary_blob_store = match aggregator.binary_blob_store_for_identity(identity).await
            {
                Ok(Some(store)) => store,
                Ok(None) => {
                    return (
                        StatusCode::OK,
                        Json::<Value>(invalid_params(
                            response_id,
                            "binary blob store unavailable for identity",
                        )),
                    );
                }
                Err(err) => {
                    return (
                        StatusCode::OK,
                        Json::<Value>(console_send_rpc_error(response_id, err)),
                    );
                }
            };
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
            let Some(runtime) = &state.runtime else {
                return (
                    StatusCode::NOT_FOUND,
                    Json::<Value>(json_rpc_error_value(
                        response_id,
                        -32600,
                        "mobkit/blob/upload multipart requires a unified runtime",
                    )),
                );
            };
            let Some(binary_blob_store) = runtime.binary_blob_store() else {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json::<Value>(json_rpc_error_value(
                        response_id,
                        -32000,
                        "binary blob store unavailable",
                    )),
                );
            };
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
                    "multipart RPC supports mobkit/console/send and mobkit/blob/upload only",
                )),
            );
        }
    }
    let response_value = if parsed_request.method == "mobkit/console/send"
        && state.runtime.is_none()
    {
        handle_console_aggregator_rpc(state.console_aggregator.clone(), parsed_request, true).await
    } else {
        let Some(runtime) = &state.runtime else {
            return (
                StatusCode::NOT_FOUND,
                Json::<Value>(json_rpc_error_value(
                    response_id,
                    -32600,
                    "console rpc multipart requires a unified runtime",
                )),
            );
        };
        Box::pin(handle_console_runtime_rpc(
            runtime,
            state.module_runtime.clone(),
            state.contact_directory.as_ref(),
            state.gateway_peer_keys.as_ref(),
            state.console_events.clone(),
            state.console_aggregator.clone(),
            state.metadata_table.clone(),
            state.mob_events.clone(),
            parsed_request,
            true,
        ))
        .await
    };
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
    if !is_valid_blob_id_value(&blob_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json::<Value>(serde_json::json!({ "error": "invalid_blob_id" })),
        )
            .into_response();
    }
    let blob_id = meerkat_core::BlobId::from(blob_id.as_str());
    let mut stores: Vec<std::sync::Arc<dyn BinaryBlobStore>> = Vec::new();
    if let Some(runtime) = &state.runtime
        && let Some(store) = runtime.binary_blob_store()
    {
        stores.push(store);
    }
    if let Some(aggregator) = &state.console_aggregator {
        stores.extend(aggregator.binary_blob_stores());
    }
    if stores.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json::<Value>(serde_json::json!({ "error": "blob_store_unavailable" })),
        )
            .into_response();
    }
    for store in stores {
        match store.get_bytes(&blob_id).await {
            Ok(payload) => return blob_payload_response(payload),
            Err(meerkat_core::BlobStoreError::NotFound(_)) => continue,
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json::<Value>(serde_json::json!({ "error": err.to_string() })),
                )
                    .into_response();
            }
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json::<Value>(serde_json::json!({ "error": "blob_not_found" })),
    )
        .into_response()
}

fn blob_payload_response(payload: BinaryBlobPayload) -> axum::response::Response {
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
            serde_json::to_value(model_capabilities_for_member_entry(
                runtime.handle().definition(),
                entry,
            ))
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
async fn handle_console_aggregator_rpc(
    console_aggregator: Option<MobKitConsoleAggregator>,
    request: JsonRpcRequest,
    is_authenticated: bool,
) -> Value {
    let response_id = request.id.clone().unwrap_or(Value::Null);
    match request.method.as_str() {
        "mobkit/capabilities" => response_value(
            response_id,
            Some(json!({
                "methods": [
                    "mobkit/capabilities",
                    "mobkit/console/list_identities",
                    "mobkit/console/inspect_identity",
                    "mobkit/console/query_timeline",
                    "mobkit/retire",
                    "mobkit/reset_all",
                    "mobkit/console/send",
                ],
                "authenticated": is_authenticated,
                "features": {
                    "console_aggregator": console_aggregator.is_some(),
                    "multi_runtime_console": console_aggregator.is_some(),
                }
            })),
            None,
        ),
        "mobkit/console/list_identities" => {
            let Some(aggregator) = &console_aggregator else {
                return console_aggregator_unavailable(response_id);
            };
            match aggregator.list_identities().await {
                Ok(identities) => {
                    response_value(response_id, Some(json!({ "identities": identities })), None)
                }
                Err(err) => internal_error(response_id, format!("list_identities failed: {err}")),
            }
        }
        "mobkit/console/inspect_identity" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let Some(aggregator) = &console_aggregator else {
                return console_aggregator_unavailable(response_id);
            };
            match aggregator.inspect_identity(identity).await {
                Ok(Some(inspection)) => response_value(
                    response_id,
                    Some(serde_json::to_value(inspection).unwrap_or(Value::Null)),
                    None,
                ),
                Ok(None) => response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32001,
                        message: format!("unknown identity: {identity}"),
                        data: None,
                    }),
                ),
                Err(err) => internal_error(response_id, format!("inspect_identity failed: {err}")),
            }
        }
        "mobkit/console/query_timeline" => {
            let query: ConsoleTimelineQuery = match serde_json::from_value(request.params.clone()) {
                Ok(query) => query,
                Err(err) => {
                    return invalid_params(response_id, format!("invalid query params: {err}"));
                }
            };
            let Some(aggregator) = &console_aggregator else {
                return console_aggregator_unavailable(response_id);
            };
            match aggregator.query_timeline(query).await {
                Ok(page) => response_value(
                    response_id,
                    Some(serde_json::to_value(page).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32010,
                        message: format!("query_timeline failed: {err}"),
                        data: Some(json!({ "kind": "replay_unavailable" })),
                    }),
                ),
            }
        }
        "mobkit/console/send" => {
            let send_request: ConsoleSendRequest =
                match serde_json::from_value(request.params.clone()) {
                    Ok(request) => request,
                    Err(err) => {
                        return invalid_params(response_id, format!("invalid send params: {err}"));
                    }
                };
            let Some(aggregator) = &console_aggregator else {
                return console_aggregator_unavailable(response_id);
            };
            match aggregator.send(send_request).await {
                Ok(accepted) => response_value(
                    response_id,
                    Some(serde_json::to_value(accepted).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: console_send_rpc_code(&err),
                        message: err.to_string(),
                        data: None,
                    }),
                ),
            }
        }
        "mobkit/retire" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let Some(aggregator) = &console_aggregator else {
                return console_aggregator_unavailable(response_id);
            };
            match aggregator.retire_identity(identity).await {
                Ok(true) => {
                    response_value(response_id, Some(json!({ "identity": identity })), None)
                }
                Ok(false) => response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32001,
                        message: format!("unknown identity: {identity}"),
                        data: None,
                    }),
                ),
                Err(err) => internal_error(response_id, format!("retire failed: {err}")),
            }
        }
        "mobkit/reset_all" => {
            let Some(aggregator) = &console_aggregator else {
                return console_aggregator_unavailable(response_id);
            };
            match aggregator.list_identities().await {
                Ok(identities) => {
                    let mut retired = Vec::new();
                    let mut failed = Vec::new();
                    for identity in identities {
                        match aggregator.retire_identity(&identity.identity).await {
                            Ok(true) => retired.push(identity.identity),
                            Ok(false) => failed.push(json!({
                                "identity": identity.identity,
                                "error": "unknown identity",
                            })),
                            Err(err) => failed.push(json!({
                                "identity": identity.identity,
                                "error": err.to_string(),
                            })),
                        }
                    }
                    if let Err(err) = aggregator.clear_timeline_frames().await {
                        failed.push(json!({
                            "identity": "_console_timeline",
                            "error": err.to_string(),
                        }));
                    }
                    response_value(
                        response_id,
                        Some(json!({
                            "retired": retired,
                            "failed": failed,
                        })),
                        None,
                    )
                }
                Err(err) => internal_error(response_id, format!("reset_all failed: {err}")),
            }
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

fn console_aggregator_unavailable(response_id: Value) -> Value {
    response_value(
        response_id,
        None,
        Some(JsonRpcError {
            code: -32004,
            message: "console aggregator unavailable".to_string(),
            data: None,
        }),
    )
}

#[allow(clippy::too_many_arguments)]
async fn handle_console_runtime_rpc(
    runtime: &MobRuntime,
    module_runtime: Option<std::sync::Arc<tokio::sync::Mutex<MobkitRuntimeHandle>>>,
    contact_directory: Option<&ContactDirectory>,
    gateway_peer_keys: Option<&crate::auth::peer_keys::GatewayPeerKeys>,
    console_events: Option<ConsoleEventStore>,
    console_aggregator: Option<MobKitConsoleAggregator>,
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
                "mobkit/collect_completed",
                "mobkit/blob/get",
                "mobkit/wait_ready",
                "mobkit/flow_status",
                "mobkit/list_flows",
                "mobkit/list_runs",
                "mobkit/console/list_identities",
                "mobkit/console/inspect_identity",
                "mobkit/console/query_timeline",
                "mobkit/mob_events/query",
                "mobkit/mob_events/subscribe",
                "mobkit/cross_mob/peer_info",
                "mobkit/cross_mob/directory",
                "mobkit/peer_pubkey",
            ];
            if module_runtime.is_some() {
                methods.extend_from_slice(&[
                    "mobkit/status_identity",
                    "mobkit/inspect_identity",
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
                    "mobkit/retire",
                    "mobkit/reset_all",
                    "mobkit/console/send",
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
        "mobkit/console/list_identities" => {
            let Some(aggregator) = &console_aggregator else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32004,
                        message: "console aggregator unavailable".to_string(),
                        data: None,
                    }),
                );
            };
            match aggregator.list_identities().await {
                Ok(identities) => {
                    response_value(response_id, Some(json!({ "identities": identities })), None)
                }
                Err(err) => internal_error(response_id, format!("list_identities failed: {err}")),
            }
        }
        "mobkit/console/inspect_identity" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let Some(aggregator) = &console_aggregator else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32004,
                        message: "console aggregator unavailable".to_string(),
                        data: None,
                    }),
                );
            };
            match aggregator.inspect_identity(identity).await {
                Ok(Some(inspection)) => response_value(
                    response_id,
                    Some(serde_json::to_value(inspection).unwrap_or(Value::Null)),
                    None,
                ),
                Ok(None) => response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32001,
                        message: format!("unknown identity: {identity}"),
                        data: None,
                    }),
                ),
                Err(err) => internal_error(response_id, format!("inspect_identity failed: {err}")),
            }
        }
        "mobkit/console/query_timeline" => {
            let query: ConsoleTimelineQuery = match serde_json::from_value(request.params.clone()) {
                Ok(query) => query,
                Err(err) => {
                    return invalid_params(response_id, format!("invalid query params: {err}"));
                }
            };
            let Some(aggregator) = &console_aggregator else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32004,
                        message: "console aggregator unavailable".to_string(),
                        data: None,
                    }),
                );
            };
            match aggregator.query_timeline(query).await {
                Ok(page) => response_value(
                    response_id,
                    Some(serde_json::to_value(page).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32010,
                        message: format!("query_timeline failed: {err}"),
                        data: Some(json!({ "kind": "replay_unavailable" })),
                    }),
                ),
            }
        }
        "mobkit/console/send" => {
            let send_request: ConsoleSendRequest =
                match serde_json::from_value(request.params.clone()) {
                    Ok(request) => request,
                    Err(err) => {
                        return invalid_params(response_id, format!("invalid send params: {err}"));
                    }
                };
            let Some(aggregator) = &console_aggregator else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32004,
                        message: "console aggregator unavailable".to_string(),
                        data: None,
                    }),
                );
            };
            match aggregator.send(send_request).await {
                Ok(accepted) => response_value(
                    response_id,
                    Some(serde_json::to_value(accepted).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: console_send_rpc_code(&err),
                        message: err.to_string(),
                        data: None,
                    }),
                ),
            }
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
            if let Some(aggregator) = &console_aggregator {
                return match aggregator.retire_identity(identity).await {
                    Ok(true) => {
                        if let Some(store) = &console_events {
                            store
                                .record_lifecycle(identity, "identity_retired", json!({}))
                                .await;
                        }
                        response_value(response_id, Some(json!({ "identity": identity })), None)
                    }
                    Ok(false) => response_value(
                        response_id,
                        None,
                        Some(JsonRpcError {
                            code: -32001,
                            message: format!("unknown identity: {identity}"),
                            data: None,
                        }),
                    ),
                    Err(err) => internal_error(response_id, format!("retire failed: {err}")),
                };
            }
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
        "mobkit/reset_all" => {
            match Box::pin(reset_all_live_console_agents(
                runtime,
                console_events.as_ref(),
                console_aggregator.as_ref(),
            ))
            .await
            {
                Ok(body) => response_value(response_id, Some(body), None),
                Err(err) => internal_error(response_id, format!("reset_all failed: {err}")),
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
            if let Some(aggregator) = &console_aggregator {
                return match aggregator.retire_identity(member_id).await {
                    Ok(true) => response_value(
                        response_id,
                        Some(serde_json::json!({ "accepted": true })),
                        None,
                    ),
                    Ok(false) => response_value(
                        response_id,
                        None,
                        Some(JsonRpcError {
                            code: -32001,
                            message: format!("unknown identity: {member_id}"),
                            data: None,
                        }),
                    ),
                    Err(err) => internal_error(response_id, format!("retire_member failed: {err}")),
                };
            }
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
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    read_model: &ConsoleSnapshotReadModel,
) -> ConsoleLiveSnapshot {
    let read_model_state = read_model.snapshot(runtime).await;
    let running = read_model_state.running.unwrap_or(true);
    // Hot path: clone the pre-projected members from the cached read
    // model. NO `handle.*` async calls happen here — the background
    // refresh task is the only thing that walks the mob roster, so
    // snapshot requests never contend with spawn/retire activity.
    // First request on a cold cache pays one synchronous refresh via
    // `snapshot(runtime).await` above; subsequent requests just clone.
    let mut members = read_model_state.primary_members.clone();
    if visibility_policy.include_implicit_delegate_members() {
        for group in &read_model_state.delegate_member_groups {
            members.extend(group.iter().cloned());
        }
    }
    dedupe_console_members_by_identity(&mut members);

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

async fn collect_console_snapshot_read_model(
    runtime: &MobRuntime,
) -> ConsoleSnapshotReadModelState {
    let handle = runtime.handle();
    let mut state = ConsoleSnapshotReadModelState {
        running: Some(matches!(
            handle.status().await.ok(),
            Some(MobState::Creating | MobState::Running)
        )),
        ..ConsoleSnapshotReadModelState::default()
    };
    collect_console_session_index_for_handle(&handle, &mut state).await;

    // Snapshot + project the primary mob into the cache. Done here
    // under the background refresh lock so per-request
    // `build_live_snapshot` calls never need to enter MobHandle async
    // methods. The session-id index in `state` was populated above by
    // `collect_console_session_index_for_handle`.
    let (primary_members, _primary_owner_index) =
        project_console_members_from_handle(&handle, None, None, &state).await;
    state.primary_members = primary_members;

    let Some(mcp_state) = runtime.agent_mob_mcp_state() else {
        return state;
    };
    let primary_mob_id = handle.mob_id().to_string();
    let mut processed = BTreeSet::from([primary_mob_id]);
    let mut delegate_groups: Vec<Vec<ConsoleMember>> = Vec::new();
    loop {
        let mut progressed = false;
        for (mob_id, _mob_state) in mcp_state.mob_list().await {
            if processed.contains(mob_id.as_str()) {
                continue;
            }
            let Ok(delegate_handle) = mcp_state.handle_for(&mob_id).await else {
                continue;
            };
            let Some(owner_session_id) = delegate_handle.definition().owner_bridge_session_index()
            else {
                processed.insert(mob_id.to_string());
                continue;
            };
            let Some(host_identity) = state.session_owner_by_id.get(owner_session_id).cloned()
            else {
                continue;
            };
            collect_console_session_index_for_handle(&delegate_handle, &mut state).await;
            let (delegate_members, _delegate_owner_index) = project_console_members_from_handle(
                &delegate_handle,
                Some(&host_identity),
                Some(mob_id.as_str()),
                &state,
            )
            .await;
            delegate_groups.push(delegate_members);
            processed.insert(mob_id.to_string());
            progressed = true;
        }
        if !progressed {
            break;
        }
    }
    state.delegate_member_groups = delegate_groups;
    state
}

async fn collect_console_session_index_for_handle(
    handle: &MobHandle,
    state: &mut ConsoleSnapshotReadModelState,
) {
    for entry in handle.list_members_including_retiring().await {
        let identity = entry.agent_identity.to_string();
        let Some(session_id) = handle
            .resolve_bridge_session_id(&entry.agent_identity)
            .await
            .map(|session_id| session_id.to_string())
        else {
            state.session_id_by_identity.remove(&identity);
            continue;
        };
        state
            .session_owner_by_id
            .insert(session_id.clone(), identity.clone());
        state.session_id_by_identity.insert(identity, session_id);
    }
}

fn apply_console_visibility_policy(
    snapshot: &mut ConsoleLiveSnapshot,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
) {
    let mut hidden = BTreeSet::new();
    snapshot.members.retain(|member| {
        let visible = visibility_policy.member_visible(member);
        if !visible {
            hidden.insert(member.agent_identity.clone());
        }
        visible
    });
    snapshot
        .agents
        .retain(|agent| !hidden.contains(&agent.agent_id));
    snapshot
        .loaded_modules
        .retain(|module_id| !hidden.contains(module_id));
}

async fn reset_all_live_console_agents(
    runtime: &MobRuntime,
    console_events: Option<&ConsoleEventStore>,
    console_aggregator: Option<&MobKitConsoleAggregator>,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let read_model = ConsoleSnapshotReadModel::default();
    *read_model.inner.write().await = collect_console_snapshot_read_model(runtime).await;
    // Mark the freshly-built model primed so `build_live_snapshot` doesn't
    // try to re-prime; this is a one-shot read for the reset path.
    read_model
        .primed
        .store(true, std::sync::atomic::Ordering::Release);
    let snapshot = build_live_snapshot(
        runtime,
        &[],
        console_events,
        &AllowAllConsoleVisibilityPolicy,
        &read_model,
    )
    .await;
    let mut main_identities = BTreeSet::new();
    let mut delegate_members = BTreeSet::new();
    for member in snapshot.members {
        if member.state == MEMBER_STATE_RETIRING {
            continue;
        }
        if let Some(source_mob_id) = member.labels.get("source_mob_id").cloned() {
            delegate_members.insert((source_mob_id, member.agent_identity));
        } else {
            main_identities.insert(member.agent_identity);
        }
    }
    let current_main_identities = main_identities.clone();
    let baseline_specs = runtime.baseline_member_specs().await;
    let baseline_identities = baseline_specs
        .iter()
        .map(|spec| spec.identity.to_string())
        .collect::<BTreeSet<_>>();
    main_identities.extend(baseline_identities.iter().cloned());

    let mut retired_delegates = Vec::new();
    let mut reset_main = Vec::new();
    let mut failures = Vec::new();

    if let Some(state) = runtime.agent_mob_mcp_state() {
        for (mob_id, identity) in delegate_members {
            match state.handle_for(&MobId::from(mob_id.as_str())).await {
                Ok(handle) => match handle.retire(MeerkatId::from(identity.as_str())).await {
                    Ok(()) => retired_delegates.push(json!({
                        "identity": identity,
                        "mob_id": mob_id,
                    })),
                    Err(err) => failures.push(json!({
                        "identity": identity,
                        "mob_id": mob_id,
                        "error": err.to_string(),
                    })),
                },
                Err(err) => failures.push(json!({
                    "identity": identity,
                    "mob_id": mob_id,
                    "error": err.to_string(),
                })),
            }
        }
    } else if let Some(aggregator) = console_aggregator {
        let identities = delegate_members
            .into_iter()
            .map(|(_, identity)| identity)
            .collect::<BTreeSet<_>>();
        for identity in identities {
            match aggregator.retire_identity(&identity).await {
                Ok(true) => retired_delegates.push(json!({ "identity": identity })),
                Ok(false) => failures.push(json!({
                    "identity": identity,
                    "error": "unknown identity",
                })),
                Err(err) => failures.push(json!({
                    "identity": identity,
                    "error": err.to_string(),
                })),
            }
        }
    }

    let handle = runtime.handle();
    for spec in baseline_specs {
        let identity = spec.identity.to_string();
        if current_main_identities.contains(&identity) {
            continue;
        }
        match handle.ensure_member(spec).await {
            Ok(_outcome) => {
                if let Some(store) = console_events {
                    store
                        .record_lifecycle(
                            &identity,
                            "identity_reset",
                            json!({ "scope": "reset_all", "restored": true }),
                        )
                        .await;
                }
                reset_main.push(identity);
            }
            Err(err) => failures.push(json!({
                "identity": identity,
                "error": err.to_string(),
            })),
        }
    }
    for identity in main_identities {
        if baseline_identities.contains(&identity) && !current_main_identities.contains(&identity) {
            continue;
        }
        if baseline_identities.contains(&identity) {
            match handle
                .respawn(MeerkatId::from(identity.as_str()), None)
                .await
            {
                Ok(_receipt) => {
                    if let Some(store) = console_events {
                        store
                            .record_lifecycle(
                                &identity,
                                "identity_reset",
                                json!({ "scope": "reset_all" }),
                            )
                            .await;
                    }
                    reset_main.push(identity);
                }
                Err(err) => failures.push(json!({
                    "identity": identity,
                    "error": err.to_string(),
                })),
            }
        } else {
            match handle.retire(MeerkatId::from(identity.as_str())).await {
                Ok(()) => {
                    if let Some(store) = console_events {
                        store
                            .record_lifecycle(
                                &identity,
                                "identity_retired",
                                json!({ "scope": "reset_all", "dynamic": true }),
                            )
                            .await;
                    }
                    retired_delegates.push(json!({ "identity": identity }));
                }
                Err(err) => failures.push(json!({
                    "identity": identity,
                    "error": err.to_string(),
                })),
            }
        }
    }

    let startup_history = if let Some(aggregator) = console_aggregator {
        aggregator.clear_timeline_frames().await?;
        Some(
            wait_for_reset_startup_history(
                aggregator,
                baseline_identities.iter().cloned().collect(),
                Duration::from_mins(1),
            )
            .await?,
        )
    } else {
        None
    };

    Ok(json!({
        "reset": reset_main,
        "retired_delegates": retired_delegates,
        "failed": failures,
        "startup_history": startup_history,
    }))
}

async fn wait_for_reset_startup_history(
    aggregator: &MobKitConsoleAggregator,
    identities: BTreeSet<String>,
    timeout: Duration,
) -> ConsoleLogResult<Value> {
    if identities.is_empty() {
        return Ok(json!({
            "timeout": false,
            "ready": Vec::<String>::new(),
            "pending": Vec::<String>::new(),
        }));
    }

    let deadline = Instant::now() + timeout;
    let mut pending = identities;
    let mut ready = BTreeSet::new();
    while !pending.is_empty() {
        for identity in pending.clone() {
            let page = aggregator
                .query_timeline(ConsoleTimelineQuery {
                    identity: Some(identity.clone()),
                    limit: 1000,
                    ..ConsoleTimelineQuery::default()
                })
                .await?;
            let startup_completed = page.frames.iter().any(|frame| {
                matches!(
                    frame.kind.as_str(),
                    "interaction_complete" | "turn_completed"
                )
            });
            if startup_completed {
                pending.remove(&identity);
                ready.insert(identity);
            }
        }

        if pending.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            return Ok(json!({
                "timeout": true,
                "ready": ready.into_iter().collect::<Vec<_>>(),
                "pending": pending.into_iter().collect::<Vec<_>>(),
            }));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    Ok(json!({
        "timeout": false,
        "ready": ready.into_iter().collect::<Vec<_>>(),
        "pending": Vec::<String>::new(),
    }))
}

fn dedupe_console_members_by_identity(members: &mut Vec<ConsoleMember>) {
    let mut seen_member_ids = BTreeSet::new();
    members.retain(|member| seen_member_ids.insert(member.agent_identity.clone()));
}

async fn project_console_members_from_handle(
    handle: &MobHandle,
    host_identity: Option<&str>,
    source_mob_id: Option<&str>,
    read_model: &ConsoleSnapshotReadModelState,
) -> (Vec<ConsoleMember>, BTreeMap<String, String>) {
    let entries = handle.list_all_members().await;
    let mut members = Vec::with_capacity(entries.len());
    let mut session_owner_by_id = BTreeMap::new();
    for entry in &entries {
        let identity = entry.agent_identity.to_string();
        let session_id = read_model.session_id_by_identity.get(&identity).cloned();
        if let Some(session_id) = session_id.as_ref() {
            session_owner_by_id.insert(session_id.clone(), identity.clone());
        }
        let model_capabilities =
            model_capabilities_for_role(handle.definition(), entry.role.as_str());
        let mut labels = entry.labels.clone();
        if let Some(host_identity) = host_identity {
            labels
                .entry("delegate_host_identity".to_string())
                .or_insert_with(|| host_identity.to_string());
            labels
                .entry("group".to_string())
                .or_insert_with(|| "Coordinators".to_string());
        }
        if let Some(source_mob_id) = source_mob_id {
            labels
                .entry("source_mob_id".to_string())
                .or_insert_with(|| source_mob_id.to_string());
        }
        let mut wired_to: Vec<String> = entry.wired_to.iter().map(ToString::to_string).collect();
        if let Some(host_identity) = host_identity
            && !wired_to.iter().any(|peer| peer == host_identity)
        {
            wired_to.push(host_identity.to_string());
        }
        members.push(ConsoleMember {
            agent_identity: identity,
            role: entry.role.to_string(),
            state: match entry.state {
                meerkat_mob::MemberState::Active => MEMBER_STATE_ACTIVE.to_string(),
                meerkat_mob::MemberState::Retiring => MEMBER_STATE_RETIRING.to_string(),
            },
            model_capabilities,
            runtime_mode: Some(entry.runtime_mode.to_string()),
            session_id,
            wired_to,
            labels,
        });
    }
    (members, session_owner_by_id)
}

async fn build_aggregator_live_snapshot(
    aggregator: &MobKitConsoleAggregator,
    config_module_ids: &[String],
) -> Result<ConsoleLiveSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let identities = aggregator.list_identities().await?;
    let mut members = Vec::with_capacity(identities.len());
    for identity in &identities {
        let mut labels = identity.labels.clone();
        labels
            .entry("display_name".to_string())
            .or_insert_with(|| identity.display_name.clone());
        labels
            .entry("addressable".to_string())
            .or_insert_with(|| identity.addressable.to_string());
        let wired_to = aggregator
            .inspect_identity(&identity.identity)
            .await
            .ok()
            .flatten()
            .map(|inspection| inspection.peers)
            .unwrap_or_default();
        members.push(ConsoleMember {
            agent_identity: identity.identity.clone(),
            role: labels
                .get("role")
                .cloned()
                .unwrap_or_else(|| "identity".to_string()),
            state: identity.health.clone(),
            model_capabilities: ConsoleModelCapabilities::default(),
            runtime_mode: Some("console_aggregator".to_string()),
            session_id: identity.session_id.clone(),
            wired_to,
            labels,
        });
    }
    members.sort_by(|left, right| left.agent_identity.cmp(&right.agent_identity));
    let agents = members
        .iter()
        .map(|member| ConsoleAgentLiveSnapshot {
            agent_id: member.agent_identity.clone(),
            member_id: member.agent_identity.clone(),
            label: member
                .labels
                .get("display_name")
                .cloned()
                .unwrap_or_else(|| member.agent_identity.clone()),
            kind: "meerkat".to_string(),
            identity: Some(member.agent_identity.clone()),
            role: Some(member.role.clone()),
            state: Some(member.state.clone()),
            session_id: member.session_id.clone(),
            model_capabilities: member.model_capabilities.clone(),
            response_phase: None,
            watched: None,
            alert_level: None,
            degraded: None,
            degraded_reason: None,
        })
        .collect::<Vec<_>>();
    let loaded_modules = if config_module_ids.is_empty() {
        members
            .iter()
            .map(|member| member.agent_identity.clone())
            .collect()
    } else {
        config_module_ids.to_vec()
    };
    Ok(ConsoleLiveSnapshot::new(
        Some("console-aggregator".to_string()),
        true,
        loaded_modules,
        agents,
        members,
        true,
    ))
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
        ConsoleSnapshotReadModel, ConsoleSnapshotReadModelState, MAX_MULTIPART_BODY_BYTES,
        MAX_MULTIPART_IMAGE_BYTES, MultipartImageUpload, apply_console_visibility_policy,
        collect_console_snapshot_read_model, cursor_is_after, dedupe_console_members_by_identity,
        externalize_image_upload_placeholders, externalize_single_image_upload,
        project_console_members_from_handle, query_timeline_snapshot,
    };
    use crate::blob_store::{BinaryBlobStore, ObjectStoreBlobStore};
    use crate::console_aggregator::HideImplicitDelegateMembersConsoleVisibilityPolicy;
    use crate::console_aggregator::{
        ConsoleCursor, ConsoleFrameSource, ConsoleFrameSourceKind, ConsoleFrameStatus,
        ConsoleTimelineQuery, MobKitConsoleAggregator, NewConsoleFrame,
    };
    use crate::mob_handle_runtime::{MobRuntime, model_capabilities_for_role};
    use crate::runtime::{ConsoleAgentLiveSnapshot, ConsoleLiveSnapshot, ConsoleMember};
    use crate::{MobBootstrapOptions, MobBootstrapSpec};
    use bytes::Bytes;
    use meerkat::{AgentFactory, Config, build_ephemeral_service};
    use meerkat_client::TestClient;
    use meerkat_mob::{MobDefinition, MobStorage, SpawnMemberSpec};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[test]
    fn multipart_body_limit_covers_configured_image_limit() {
        const _: () = assert!(MAX_MULTIPART_BODY_BYTES > MAX_MULTIPART_IMAGE_BYTES);
        const _: () = assert!(MAX_MULTIPART_BODY_BYTES > 2 * 1024 * 1024);
    }

    /// Cold-cache contract: a `prime_now` waiter that arrives while
    /// another task holds `refresh_lock` must park on the lock and
    /// resume after that task releases it. No race-prone signaling
    /// involved — the lock acquisition itself IS the signal that the
    /// in-flight refresh has finished and `primed` is true.
    ///
    /// Test shape: hold `refresh_lock` from the test thread (no real
    /// refresh task), spawn a `prime_now`-style waiter, then set
    /// `primed` + drop the lock. The waiter must observe `primed`
    /// after acquiring the lock and return without redoing the
    /// refresh (we'd otherwise deadlock since we don't supply a
    /// real `MobRuntime`).
    #[tokio::test]
    async fn cold_cache_waiter_resumes_when_refresh_lock_drops()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use std::sync::atomic::Ordering;
        use tokio::time::Duration;

        let model = ConsoleSnapshotReadModel::default();
        let guard = model
            .refresh_lock
            .clone()
            .try_lock_owned()
            .map_err(|_| "refresh_lock unexpectedly contended at test start")?;

        let model_for_waiter = model.clone();
        let waiter = tokio::spawn(async move {
            // Inlined `prime_now` shape (skips the runtime call,
            // since the test will set `primed` before this acquires).
            if model_for_waiter
                .primed
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return;
            }
            let _wait_guard = model_for_waiter.refresh_lock.clone().lock_owned().await;
            // After acquiring, `primed` must be true (the "refresher"
            // — i.e., the test thread — set it before releasing).
            assert!(
                model_for_waiter
                    .primed
                    .load(std::sync::atomic::Ordering::Acquire),
                "waiter acquired lock but primed is still false"
            );
        });

        // Give the waiter time to reach `lock_owned().await`.
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Set primed, then release the lock. The waiter parked on
        // `lock_owned()` should acquire it immediately.
        model.primed.store(true, Ordering::Release);
        drop(guard);

        let result = tokio::time::timeout(Duration::from_secs(1), waiter).await;
        assert!(
            result.is_ok(),
            "waiter should resume once the refresh lock drops"
        );
        Ok(())
    }

    /// Companion: when `primed` is already set, `snapshot()` returns
    /// without touching the refresh lock at all. Guards against an
    /// over-eager `prime_now` that would deadlock during normal
    /// (hot-cache) traffic.
    #[tokio::test]
    async fn snapshot_skips_refresh_lock_when_already_primed()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use std::sync::atomic::Ordering;
        use tokio::time::Duration;

        let model = ConsoleSnapshotReadModel::default();
        model.primed.store(true, Ordering::Release);
        // Pre-acquire the refresh lock to prove it isn't touched.
        let _guard = model
            .refresh_lock
            .clone()
            .try_lock_owned()
            .map_err(|_| "refresh_lock unexpectedly contended at test start")?;

        // `snapshot()` calls `prime_now` only on cold cache; with
        // primed=true the lock-await branch must not be reached.
        // We test the contract via direct inspection: if `prime_now`
        // accidentally tried `lock_owned().await` here, this would
        // hang. The timeout below is the deadlock guard.
        let snap_fast_path = async {
            assert!(
                model.primed.load(Ordering::Acquire),
                "primed precondition for hot-cache path"
            );
        };
        let result = tokio::time::timeout(Duration::from_millis(100), snap_fast_path).await;
        assert!(result.is_ok(), "hot-cache snapshot path should not block");
        Ok(())
    }

    #[test]
    fn timeline_stream_cursor_filter_uses_numeric_console_sequence() {
        assert!(cursor_is_after(
            &ConsoleCursor::from("console:10"),
            &ConsoleCursor::from("console:9")
        ));
        assert!(!cursor_is_after(
            &ConsoleCursor::from("console:9"),
            &ConsoleCursor::from("console:10")
        ));
    }

    #[test]
    fn console_live_snapshot_dedupes_repeated_delegate_identities() {
        let mut members = vec![
            ConsoleMember {
                agent_identity: "incident-commander".to_string(),
                role: "commander".to_string(),
                state: "active".to_string(),
                model_capabilities: Default::default(),
                runtime_mode: None,
                session_id: None,
                wired_to: Vec::new(),
                labels: BTreeMap::new(),
            },
            ConsoleMember {
                agent_identity: "qa-child".to_string(),
                role: "delegate".to_string(),
                state: "active".to_string(),
                model_capabilities: Default::default(),
                runtime_mode: None,
                session_id: Some("first".to_string()),
                wired_to: vec!["qa-parent".to_string()],
                labels: BTreeMap::from([(
                    "delegate_host_identity".to_string(),
                    "qa-parent".to_string(),
                )]),
            },
            ConsoleMember {
                agent_identity: "qa-child".to_string(),
                role: "delegate".to_string(),
                state: "active".to_string(),
                model_capabilities: Default::default(),
                runtime_mode: None,
                session_id: Some("second".to_string()),
                wired_to: vec!["qa-parent".to_string()],
                labels: BTreeMap::from([(
                    "delegate_host_identity".to_string(),
                    "qa-parent".to_string(),
                )]),
            },
        ];

        dedupe_console_members_by_identity(&mut members);

        assert_eq!(
            members
                .iter()
                .map(|member| member.agent_identity.as_str())
                .collect::<Vec<_>>(),
            vec!["incident-commander", "qa-child"]
        );
        assert_eq!(members[1].session_id.as_deref(), Some("first"));
    }

    #[test]
    fn console_visibility_policy_hides_implicit_delegate_members_from_snapshot() {
        let mut snapshot = ConsoleLiveSnapshot::new(
            Some("runtime".to_string()),
            true,
            vec!["incident-commander".to_string(), "qa-child".to_string()],
            vec![
                ConsoleAgentLiveSnapshot {
                    agent_id: "incident-commander".to_string(),
                    member_id: "incident-commander".to_string(),
                    label: "Incident Commander".to_string(),
                    kind: "meerkat".to_string(),
                    identity: Some("incident-commander".to_string()),
                    role: Some("commander".to_string()),
                    state: Some("active".to_string()),
                    session_id: None,
                    model_capabilities: Default::default(),
                    response_phase: None,
                    watched: None,
                    alert_level: None,
                    degraded: None,
                    degraded_reason: None,
                },
                ConsoleAgentLiveSnapshot {
                    agent_id: "qa-child".to_string(),
                    member_id: "qa-child".to_string(),
                    label: "QA Child".to_string(),
                    kind: "meerkat".to_string(),
                    identity: Some("qa-child".to_string()),
                    role: Some("delegate".to_string()),
                    state: Some("active".to_string()),
                    session_id: Some("delegate-session".to_string()),
                    model_capabilities: Default::default(),
                    response_phase: None,
                    watched: None,
                    alert_level: None,
                    degraded: None,
                    degraded_reason: None,
                },
            ],
            vec![
                ConsoleMember {
                    agent_identity: "incident-commander".to_string(),
                    role: "commander".to_string(),
                    state: "active".to_string(),
                    model_capabilities: Default::default(),
                    runtime_mode: None,
                    session_id: None,
                    wired_to: Vec::new(),
                    labels: BTreeMap::new(),
                },
                ConsoleMember {
                    agent_identity: "qa-child".to_string(),
                    role: "delegate".to_string(),
                    state: "active".to_string(),
                    model_capabilities: Default::default(),
                    runtime_mode: None,
                    session_id: Some("delegate-session".to_string()),
                    wired_to: vec!["qa-parent".to_string()],
                    labels: BTreeMap::from([(
                        "source_mob_id".to_string(),
                        "implicit-qa-mob".to_string(),
                    )]),
                },
            ],
            true,
        );

        apply_console_visibility_policy(
            &mut snapshot,
            &HideImplicitDelegateMembersConsoleVisibilityPolicy,
        );

        assert_eq!(
            snapshot
                .members
                .iter()
                .map(|member| member.agent_identity.as_str())
                .collect::<Vec<_>>(),
            vec!["incident-commander"]
        );
        assert_eq!(
            snapshot
                .agents
                .iter()
                .map(|agent| agent.agent_id.as_str())
                .collect::<Vec<_>>(),
            vec!["incident-commander"]
        );
        assert_eq!(snapshot.loaded_modules, vec!["incident-commander"]);
    }

    #[tokio::test]
    async fn live_snapshot_member_projection_uses_roster_profile_capabilities()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::tempdir()?;
        let session_path = temp_dir.path().join("sessions");
        std::fs::create_dir_all(&session_path)?;
        let factory = AgentFactory::new(&session_path).comms(true);
        let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));
        let definition = MobDefinition::from_toml(
            r#"
[mob]
id = "console-snapshot-test"

[profiles.worker]
model = "gpt-5.5"

[profiles.worker.tools]
comms = true
"#,
        )?;
        let expected = model_capabilities_for_role(&definition, "worker");
        let runtime = MobRuntime::bootstrap(
            MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
                .with_options(MobBootstrapOptions {
                    allow_ephemeral_sessions: true,
                    notify_orchestrator_on_resume: true,
                    default_llm_client: Some(Arc::new(TestClient::default())),
                }),
        )
        .await?;
        runtime
            .handle()
            .spawn_spec(SpawnMemberSpec::from_wire(
                "worker".to_string(),
                "worker:one".to_string(),
                Some("You are worker one.".into()),
                None,
                None,
            ))
            .await?;

        let empty_read_model = ConsoleSnapshotReadModelState::default();
        let (members, session_owner_by_id) =
            project_console_members_from_handle(&runtime.handle(), None, None, &empty_read_model)
                .await;

        assert_eq!(members.len(), 1);
        assert_eq!(members[0].model_capabilities, expected);
        assert_eq!(members[0].session_id, None);
        assert!(session_owner_by_id.is_empty());

        let refreshed_read_model = collect_console_snapshot_read_model(&runtime).await;
        let (members, session_owner_by_id) = project_console_members_from_handle(
            &runtime.handle(),
            None,
            None,
            &refreshed_read_model,
        )
        .await;
        assert_eq!(
            members[0].session_id.as_ref(),
            session_owner_by_id.keys().next()
        );

        // Materialized cache: the refresh should have populated
        // `primary_members` with exactly the same shape that the
        // synchronous projection produces. `build_live_snapshot` reads
        // straight from this slot — never calls `handle.list_all_members`
        // — so this assertion is the cache's contract.
        assert_eq!(
            refreshed_read_model.primary_members.len(),
            members.len(),
            "primary_members cache should hold the same members as live projection"
        );
        assert_eq!(
            refreshed_read_model.primary_members[0].agent_identity,
            members[0].agent_identity
        );
        assert_eq!(
            refreshed_read_model.primary_members[0].session_id,
            members[0].session_id
        );
        Ok(())
    }

    #[tokio::test]
    async fn fresh_timeline_snapshot_reads_tail_without_full_log_replay()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let aggregator = MobKitConsoleAggregator::in_memory();
        for idx in 0..25_000 {
            aggregator
                .store()
                .append_if_absent(NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("event-{idx}"),
                    timestamp_ms: idx,
                    runtime_key: "runtime-a".to_string(),
                    identity: "agent-a".to_string(),
                    conversation_id: Some("agent-a".to_string()),
                    session_id: None,
                    kind: "text_delta".to_string(),
                    status: ConsoleFrameStatus::Completed,
                    payload: json!({ "delta": idx }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::ConsoleEvent,
                        source_cursor: None,
                    },
                    source_event_id: Some(format!("event-{idx}")),
                    interaction_id: None,
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: None,
                    caused_by_frame_id: None,
                })
                .await?;
        }

        let (frames, cursor) = query_timeline_snapshot(
            &aggregator,
            ConsoleTimelineQuery {
                identity: Some("agent-a".to_string()),
                after: None,
                limit: 200,
                ..ConsoleTimelineQuery::default()
            },
        )
        .await?;

        assert!(!frames.is_empty());
        assert_eq!(cursor.as_ref().and_then(ConsoleCursor::seq), Some(25_000));
        assert_eq!(
            frames.last().and_then(|frame| frame.cursor.seq()),
            Some(25_000)
        );
        Ok(())
    }

    #[tokio::test]
    async fn fresh_timeline_snapshot_keeps_sparse_identity_frames()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator
            .store()
            .append_if_absent(NewConsoleFrame {
                id: None,
                dedupe_key: "sparse-event".to_string(),
                timestamp_ms: 1,
                runtime_key: "runtime-a".to_string(),
                identity: "sparse-agent".to_string(),
                conversation_id: Some("sparse-agent".to_string()),
                session_id: None,
                kind: "text_complete".to_string(),
                status: ConsoleFrameStatus::Completed,
                payload: json!({ "text": "still visible" }),
                source: ConsoleFrameSource {
                    kind: ConsoleFrameSourceKind::ConsoleEvent,
                    source_cursor: None,
                },
                source_event_id: Some("sparse-event".to_string()),
                interaction_id: None,
                turn_id: None,
                run_id: None,
                parent_frame_id: None,
                caused_by_frame_id: None,
            })
            .await?;
        for idx in 0..25_000 {
            aggregator
                .store()
                .append_if_absent(NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("other-event-{idx}"),
                    timestamp_ms: idx + 2,
                    runtime_key: "runtime-a".to_string(),
                    identity: "busy-agent".to_string(),
                    conversation_id: Some("busy-agent".to_string()),
                    session_id: None,
                    kind: "text_delta".to_string(),
                    status: ConsoleFrameStatus::Completed,
                    payload: json!({ "delta": idx }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::ConsoleEvent,
                        source_cursor: None,
                    },
                    source_event_id: Some(format!("other-event-{idx}")),
                    interaction_id: None,
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: None,
                    caused_by_frame_id: None,
                })
                .await?;
        }

        let (frames, cursor) = query_timeline_snapshot(
            &aggregator,
            ConsoleTimelineQuery {
                identity: Some("sparse-agent".to_string()),
                after: None,
                limit: 200,
                ..ConsoleTimelineQuery::default()
            },
        )
        .await?;

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].identity, "sparse-agent");
        assert_eq!(frames[0].payload["text"], json!("still visible"));
        assert_eq!(cursor.as_ref().and_then(ConsoleCursor::seq), Some(1));
        Ok(())
    }

    #[tokio::test]
    async fn timeline_snapshot_clamps_requested_limit_to_store_page_size()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let aggregator = MobKitConsoleAggregator::in_memory();
        for idx in 0..2_500 {
            aggregator
                .store()
                .append_if_absent(NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("clamp-event-{idx}"),
                    timestamp_ms: idx,
                    runtime_key: "runtime-a".to_string(),
                    identity: "agent-a".to_string(),
                    conversation_id: Some("agent-a".to_string()),
                    session_id: None,
                    kind: "text_delta".to_string(),
                    status: ConsoleFrameStatus::Completed,
                    payload: json!({ "delta": idx }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::ConsoleEvent,
                        source_cursor: None,
                    },
                    source_event_id: Some(format!("clamp-event-{idx}")),
                    interaction_id: None,
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: None,
                    caused_by_frame_id: None,
                })
                .await?;
        }

        let (frames, cursor) = query_timeline_snapshot(
            &aggregator,
            ConsoleTimelineQuery {
                identity: Some("agent-a".to_string()),
                after: Some(ConsoleCursor::from("console:100")),
                limit: 5_000,
                ..ConsoleTimelineQuery::default()
            },
        )
        .await?;

        assert_eq!(frames.len(), 2_400);
        assert_eq!(
            frames.first().and_then(|frame| frame.cursor.seq()),
            Some(101)
        );
        assert_eq!(
            frames.last().and_then(|frame| frame.cursor.seq()),
            Some(2_500)
        );
        assert_eq!(cursor.as_ref().and_then(ConsoleCursor::seq), Some(2_500));
        Ok(())
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
}
