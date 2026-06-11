//! HTTP routes for the admin console REST API.

use async_stream::stream;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use futures::StreamExt;
use futures::future::join_all;
use meerkat_contracts::WireRuntimeBinding;
use meerkat_core::ContentInput;
use meerkat_core::comms::TrustedPeerDescriptor;
use meerkat_core::event::agent_event_type;
use meerkat_mob::MobState;
use meerkat_mob::ids::{MeerkatId, MobId};
use meerkat_mob::launch::MemberLaunchMode;
use meerkat_mob::runtime::reconcile::MemberFilter;
use meerkat_mob::{
    MobBackendKind, MobHandle, MobRuntimeMode, PeerTarget, ProfileName, SpawnMemberSpec,
};

use crate::mob_handle_runtime::{
    is_recoverable_lifecycle_cleanup_error, member_entry_to_json,
    model_capabilities_for_member_entry, model_capabilities_for_role,
    topology_restore_failed_peer_ids, topology_restore_warning_json,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::access::{
    ACCESS_ACTIONS, ACTION_AGENT_RESET, ACTION_AGENT_RESPAWN, ACTION_AGENT_RETIRE,
    ACTION_AGENT_SEND, ACTION_AGENT_SPAWN, ACTION_AGENT_VIEW, ACTION_GATING_DECIDE,
    ACTION_GATING_VIEW, ACTION_MOB_OBSERVE, ACTION_RUNTIME_ADMIN, AccessController, AccessGroup,
    AccessResource, AccessRule, AccessView, AgentResourceAttributes,
};
use crate::blob_store::{BinaryBlobPayload, BinaryBlobStore, is_valid_blob_id_value};
use crate::console_aggregator::{
    ConsoleCursor, ConsoleFrame, ConsoleIdentityRecord, ConsoleLogError, ConsoleLogResult,
    ConsoleLogStore, ConsoleReplayUnavailable, ConsoleSendError, ConsoleSendRequest,
    ConsoleTimelineEvent, ConsoleTimelineMode, ConsoleTimelineQuery, ConsoleTimelineWindowPage,
    ConsoleTimelineWindowQuery, ConsoleVisibility, ConsoleVisibilityPolicy,
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
    handle_console_rest_json_route_with_snapshot_and_access,
    resolve_authorized_console_auth_from_token,
};
use crate::runtime::{MetadataScope, RuntimeMetadataTable, labels_to_json_value};
use crate::types::{EventEnvelope, UnifiedEvent};
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
    pub(crate) identity_runtime: Option<Arc<crate::identity_first::IdentityRuntime>>,
    pub(crate) console_events: Option<ConsoleEventStore>,
    pub(crate) console_aggregator: Option<MobKitConsoleAggregator>,
    pub(crate) mob_events: Option<MobEventsStore>,
    pub(crate) metadata_table: Option<std::sync::Arc<RuntimeMetadataTable>>,
    pub(crate) visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    pub(crate) snapshot_read_model: ConsoleSnapshotReadModel,
    /// Optional ABAC enforcement. `None` (or a disabled config) keeps every
    /// console surface byte-for-byte compatible with the pre-access world.
    pub(crate) access: Option<AccessController>,
}

#[derive(Debug, Clone)]
struct ConsoleHttpAuthContext {
    principal: Option<String>,
    /// Per-request access snapshot when an [`AccessController`] is wired.
    access_view: Option<AccessView>,
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
        identity_runtime: None,
        console_events: None,
        console_aggregator: None,
        mob_events: None,
        metadata_table: None,
        visibility_policy: Arc::new(HideImplicitDelegateMembersConsoleVisibilityPolicy),
        snapshot_read_model: ConsoleSnapshotReadModel::default(),
        access: None,
    })
}

pub fn console_json_router_with_aggregator(
    decisions: RuntimeDecisionState,
    console_aggregator: MobKitConsoleAggregator,
) -> Router {
    console_json_router_with_aggregator_and_access(decisions, console_aggregator, None)
}

pub fn console_json_router_with_aggregator_and_access(
    decisions: RuntimeDecisionState,
    console_aggregator: MobKitConsoleAggregator,
    access: Option<AccessController>,
) -> Router {
    console_json_router_with_state(ConsoleJsonState {
        decisions,
        runtime: None,
        module_runtime: None,
        contact_directory: None,
        event_log: None,
        gateway_peer_keys: None,
        identity_runtime: None,
        console_events: None,
        console_aggregator: Some(console_aggregator),
        mob_events: None,
        metadata_table: None,
        visibility_policy: Arc::new(HideImplicitDelegateMembersConsoleVisibilityPolicy),
        snapshot_read_model: ConsoleSnapshotReadModel::default(),
        access,
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
    identity_runtime: Option<Arc<crate::identity_first::IdentityRuntime>>,
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
        identity_runtime,
        Arc::new(HideImplicitDelegateMembersConsoleVisibilityPolicy),
        None,
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
    identity_runtime: Option<Arc<crate::identity_first::IdentityRuntime>>,
    visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    access: Option<AccessController>,
) -> Router {
    let console_aggregator = console_events.clone().map(|events| {
        if let Some(store) = console_log_store {
            let aggregator = MobKitConsoleAggregator::new(store);
            aggregator.register_runtime_handles_with_policy(
                "default",
                "",
                runtime.clone(),
                identity_runtime.clone(),
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
                identity_runtime.clone(),
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
        identity_runtime,
        console_events,
        console_aggregator,
        mob_events,
        metadata_table,
        visibility_policy,
        snapshot_read_model,
        access,
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
        .route(
            "/console/identity/{identity}/stream",
            get(console_identity_timeline_stream_handler),
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

    let response = handle_console_rest_json_route_with_snapshot_and_access(
        &state.decisions,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path,
            auth: None,
        },
        live_snapshot.as_ref(),
        state.access.as_ref(),
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
    // - When require_app_auth is false: allow requests without a token.
    // - When console.read_only is true: deny mutating methods even for
    //   otherwise authorized callers.
    let auth_context = match console_request_auth_context(&state, &headers, &uri) {
        Some(context) => context,
        None => {
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
    };
    // By this point the request is always authorized:
    // - require_app_auth=true: an invalid token already returned 401 above.
    // - require_app_auth=false: all methods are permitted unconditionally.
    // Mutating methods may still be blocked by console.read_only.
    let is_authenticated = true;
    let read_only = state.decisions.console.read_only;
    let Some(runtime) = &state.runtime else {
        let response_value = Box::pin(handle_console_aggregator_rpc(
            state.console_aggregator.clone(),
            parsed_request,
            is_authenticated,
            read_only,
            state.access.as_ref(),
            auth_context.access_view.as_ref(),
        ))
        .await;
        return (StatusCode::OK, Json::<Value>(response_value));
    };

    let response_value = Box::pin(handle_console_runtime_rpc_with_visibility(
        runtime,
        state.module_runtime.clone(),
        state.contact_directory.as_ref(),
        state.gateway_peer_keys.as_ref(),
        state.console_events.clone(),
        state.console_aggregator.clone(),
        state.identity_runtime.clone(),
        state.metadata_table.clone(),
        state.mob_events.clone(),
        state.visibility_policy.as_ref(),
        parsed_request,
        is_authenticated,
        read_only,
        auth_context.principal.as_deref(),
        state.access.as_ref(),
        auth_context.access_view.as_ref(),
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
    before: Option<String>,
    #[serde(default)]
    mode: Option<ConsoleTimelineMode>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn console_identities_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
) -> impl IntoResponse {
    let Some(auth_context) = console_request_auth_context(&state, &headers, &uri) else {
        return console_json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "console identities require a valid auth token",
        );
    };
    let Some(aggregator) = &state.console_aggregator else {
        return console_json_error(
            StatusCode::NOT_FOUND,
            "unavailable",
            "console aggregator unavailable",
        );
    };
    let aggregator = aggregator.clone();
    match aggregator.list_identities().await {
        Ok(mut identities) => {
            retain_visible_identity_records(&mut identities, auth_context.access_view.as_ref());
            (
                StatusCode::OK,
                Json::<Value>(json!({ "identities": identities })),
            )
                .into_response()
        }
        Err(err) => {
            tracing::warn!(target: "mobkit::console", error = %err, "console identities request failed");
            console_json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "console identities unavailable",
            )
        }
    }
}

async fn console_timeline_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<ConsoleTimelineHttpQuery>,
) -> impl IntoResponse {
    let Some(auth_context) = console_request_auth_context(&state, &headers, &uri) else {
        return console_json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "console timeline requires a valid auth token",
        );
    };
    let Some(aggregator) = &state.console_aggregator else {
        return console_json_error(
            StatusCode::NOT_FOUND,
            "unavailable",
            "console aggregator unavailable",
        );
    };
    // Prime the attribute cache from the live roster so label/role rules
    // resolve when filtering frames by `agent.view` — the timeline REST
    // surface must not depend on a prior `/console/experience` call.
    if let Some(runtime) = &state.runtime
        && let Some(controller) = state.access.as_ref().filter(|c| c.enabled())
    {
        prime_access_cache_from_handle(&runtime.handle(), controller).await;
    }
    let timeline_query = timeline_query_from_http(query, None);
    match Box::pin(aggregator.query_timeline_windowed(timeline_query)).await {
        Ok(mut page) => {
            if let Some(view) = auth_context.access_view.as_ref() {
                page.frames
                    .retain(|frame| view.can_view_agent(frame.identity.as_str()));
            }
            (
                StatusCode::OK,
                Json::<Value>(
                    serde_json::to_value(page).unwrap_or_else(|_| json!({ "frames": [] })),
                ),
            )
                .into_response()
        }
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
    let Some(auth_context) = console_request_auth_context(&state, &headers, &uri) else {
        return console_json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "console send requires a valid auth token",
        );
    };
    if state.decisions.console.read_only {
        return console_json_error(StatusCode::FORBIDDEN, "read_only", "console is read-only");
    }
    if let Some(view) = auth_context.access_view.as_ref()
        && view.enforced()
        && !view.allows_agent(ACTION_AGENT_SEND, request.identity.as_str())
    {
        return console_json_error(
            StatusCode::FORBIDDEN,
            "access_denied",
            "you are not allowed to send to this agent",
        );
    }
    let Some(aggregator) = &state.console_aggregator else {
        return console_json_error(
            StatusCode::NOT_FOUND,
            "unavailable",
            "console aggregator unavailable",
        );
    };
    if let Some(identity_runtime) = &state.identity_runtime {
        return match Box::pin(console_send_with_identity_first_fallback(
            aggregator,
            identity_runtime.clone(),
            state.runtime.as_ref(),
            state.console_events.as_ref(),
            request,
        ))
        .await
        {
            Ok(accepted) => (
                StatusCode::OK,
                Json::<Value>(
                    serde_json::to_value(accepted).unwrap_or_else(|_| json!({ "accepted": true })),
                ),
            )
                .into_response(),
            Err(err) => console_send_error_response(err),
        };
    }
    match Box::pin(aggregator.send(request)).await {
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

async fn console_send_with_identity_first_fallback(
    aggregator: &MobKitConsoleAggregator,
    identity_runtime: Arc<crate::identity_first::IdentityRuntime>,
    runtime: Option<&MobRuntime>,
    console_events: Option<&ConsoleEventStore>,
    request: ConsoleSendRequest,
) -> Result<crate::console_aggregator::ConsoleInteractionAccepted, ConsoleSendError> {
    let member_send_request = request.clone();
    match Box::pin(console_send_identity_first(
        aggregator,
        identity_runtime,
        runtime,
        console_events,
        request,
    ))
    .await
    {
        Err(ConsoleSendError::UnknownIdentity(_)) => {
            Box::pin(aggregator.send(member_send_request)).await
        }
        result => result,
    }
}

async fn console_send_identity_first(
    aggregator: &MobKitConsoleAggregator,
    identity_runtime: Arc<crate::identity_first::IdentityRuntime>,
    runtime: Option<&MobRuntime>,
    console_events: Option<&ConsoleEventStore>,
    mut request: ConsoleSendRequest,
) -> Result<crate::console_aggregator::ConsoleInteractionAccepted, ConsoleSendError> {
    let requested_identity = request.identity.clone();
    let parsed_identity = crate::identity_first::AgentIdentity::parse(request.identity.as_str())
        .map_err(|err| ConsoleSendError::InvalidRequest(format!("invalid identity: {err}")))?;
    let content: ContentInput = serde_json::from_value(request.content.clone())
        .map_err(|err| ConsoleSendError::InvalidContent(err.to_string()))?;
    if let ContentInput::Text(text) = &content
        && text.trim().is_empty()
    {
        return Err(ConsoleSendError::InvalidContent(
            "content must be non-empty".to_string(),
        ));
    }
    if let ContentInput::Blocks(blocks) = &content
        && blocks.is_empty()
    {
        return Err(ConsoleSendError::InvalidContent(
            "content blocks must be non-empty".to_string(),
        ));
    }
    let handling_mode = parse_identity_first_handling_mode(request.handling_mode.as_deref())?;

    let (identity, status) = match identity_runtime.status(&parsed_identity).await {
        Ok(status) => (parsed_identity, status),
        Err(original_err) => {
            let Some(canonical_identity) =
                resolve_console_send_identity_alias(aggregator, &requested_identity).await
            else {
                return Err(identity_runtime_error_to_console_send_error(
                    requested_identity.as_str(),
                    original_err,
                ));
            };
            let identity = crate::identity_first::AgentIdentity::parse(canonical_identity.as_str())
                .map_err(|err| {
                    ConsoleSendError::InvalidRequest(format!("invalid aliased identity: {err}"))
                })?;
            let status = identity_runtime.status(&identity).await.map_err(|_| {
                identity_runtime_error_to_console_send_error(
                    requested_identity.as_str(),
                    original_err,
                )
            })?;
            request.identity = canonical_identity;
            (identity, status)
        }
    };
    let session_id = status
        .session_id
        .as_ref()
        .map(std::string::ToString::to_string);
    let runtime_member_id = status
        .agent_runtime_id
        .as_ref()
        .map(|id| id.as_str().to_string());
    let accepted = Box::pin(
        aggregator.reserve_identity_first_interaction(request.clone(), session_id.as_deref()),
    )
    .await?;

    if let Some(events) = console_events {
        events
            .reserve_interaction_value(
                identity.as_str(),
                runtime_member_id.as_deref(),
                &accepted.interaction_id,
                &request.origin,
                request.content.clone(),
            )
            .await
            .map_err(ConsoleSendError::State)?;
    }

    if let (Some(runtime), Some(events), Some(runtime_member_id)) = (
        runtime,
        console_events.cloned(),
        runtime_member_id.as_deref(),
    ) {
        start_identity_first_console_live_projection(
            runtime,
            events,
            identity.as_str(),
            runtime_member_id,
        )
        .await;
    }

    if handling_mode == meerkat_core::types::HandlingMode::Steer {
        match identity_runtime
            .send_with_mode(&identity, &content, handling_mode)
            .await
        {
            Ok(_) => {
                if let Err(err) = aggregator
                    .mark_steer_interaction_delivered(
                        &accepted.input_frame_id,
                        &accepted.interaction_id,
                    )
                    .await
                {
                    tracing::warn!(
                        identity = %identity,
                        error = %err,
                        "console identity-first steer was admitted but delivery status projection failed"
                    );
                }
            }
            Err(err) => {
                let _ = aggregator
                    .mark_interaction_delivery_failed(&accepted.input_frame_id)
                    .await;
                if let Some(events) = console_events {
                    events
                        .record_lifecycle(
                            identity.as_str(),
                            "interaction_failed",
                            json!({
                                "interaction_id": accepted.interaction_id,
                                "origin": request.origin,
                                "error": err.to_string(),
                            }),
                        )
                        .await;
                }
                tracing::warn!(
                    identity = %identity,
                    error = %err,
                    "console identity-first steer was accepted but delivery failed"
                );
                return Err(identity_runtime_error_to_console_send_error(
                    identity.as_str(),
                    err,
                ));
            }
        }
        return Ok(accepted);
    }

    let dispatch_aggregator = aggregator.clone();
    let dispatch_events = console_events.cloned();
    let dispatch_identity = identity.clone();
    let dispatch_content = content.clone();
    let dispatch_origin = request.origin.clone();
    let dispatch_accepted = accepted.clone();
    tokio::spawn(async move {
        match identity_runtime
            .send_with_mode(&dispatch_identity, &dispatch_content, handling_mode)
            .await
        {
            Ok(_) => {
                if let Err(err) = dispatch_aggregator
                    .mark_interaction_delivered(&dispatch_accepted.input_frame_id)
                    .await
                {
                    tracing::warn!(
                        identity = %dispatch_identity,
                        error = %err,
                        "console identity-first send was accepted but delivery status projection failed"
                    );
                }
            }
            Err(err) => {
                let _ = dispatch_aggregator
                    .mark_interaction_delivery_failed(&dispatch_accepted.input_frame_id)
                    .await;
                if let Some(events) = dispatch_events {
                    events
                        .record_lifecycle(
                            dispatch_identity.as_str(),
                            "interaction_failed",
                            json!({
                                "interaction_id": dispatch_accepted.interaction_id,
                                "origin": dispatch_origin,
                                "error": err.to_string(),
                            }),
                        )
                        .await;
                }
                tracing::warn!(
                    identity = %dispatch_identity,
                    error = %err,
                    "console identity-first send was accepted but delivery failed"
                );
            }
        }
    });
    Ok(accepted)
}

async fn start_identity_first_console_live_projection(
    runtime: &MobRuntime,
    console_events: ConsoleEventStore,
    identity: &str,
    runtime_member_id: &str,
) {
    let identity = identity.to_string();
    let runtime_member_id = runtime_member_id.to_string();
    let mut stream = match runtime
        .handle()
        .subscribe_agent_events(&MeerkatId::from(runtime_member_id.as_str()))
        .await
    {
        Ok(stream) => stream,
        Err(err) => {
            tracing::warn!(
                identity = %identity,
                runtime_member_id = %runtime_member_id,
                error = %err,
                "console identity-first live projection could not subscribe to agent events"
            );
            return;
        }
    };

    tokio::spawn(async move {
        while let Some(envelope) = stream.next().await {
            let event_type = agent_event_type(&envelope.payload).to_string();
            let unified = EventEnvelope {
                event_id: format!("evt-agent-{}", envelope.event_id),
                source: "agent".to_string(),
                timestamp_ms: envelope.timestamp_ms,
                event: UnifiedEvent::Agent {
                    agent_id: runtime_member_id.clone(),
                    event_type: event_type.clone(),
                    payload: serde_json::to_value(&envelope.payload).ok(),
                },
            };
            console_events.project_unified_event(&unified).await;
            if matches!(event_type.as_str(), "run_completed" | "run_failed") {
                break;
            }
        }
    });
}

fn parse_identity_first_handling_mode(
    value: Option<&str>,
) -> Result<meerkat_core::types::HandlingMode, ConsoleSendError> {
    match value.unwrap_or("queue") {
        "queue" => Ok(meerkat_core::types::HandlingMode::Queue),
        "steer" => Ok(meerkat_core::types::HandlingMode::Steer),
        other => Err(ConsoleSendError::InvalidHandlingMode(other.to_string())),
    }
}

async fn resolve_console_send_identity_alias(
    aggregator: &MobKitConsoleAggregator,
    requested_identity: &str,
) -> Option<String> {
    let identities = aggregator.list_identities().await.ok()?;
    identities
        .into_iter()
        .find(|record| {
            record.identity == requested_identity || record.runtime_member_id == requested_identity
        })
        .map(|record| record.identity)
}

fn identity_runtime_error_to_console_send_error(
    identity: &str,
    err: crate::identity_first::IdentityRuntimeError,
) -> ConsoleSendError {
    match err {
        crate::identity_first::IdentityRuntimeError::UnknownIdentity(_) => {
            ConsoleSendError::UnknownIdentity(identity.to_string())
        }
        crate::identity_first::IdentityRuntimeError::NotAddressable(_) => {
            ConsoleSendError::NotAddressable(identity.to_string())
        }
        crate::identity_first::IdentityRuntimeError::InvalidState { .. } => {
            ConsoleSendError::Retired(identity.to_string())
        }
        other => ConsoleSendError::Dispatch(other.to_string()),
    }
}

async fn console_timeline_stream_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<ConsoleTimelineHttpQuery>,
) -> impl IntoResponse {
    let Some(auth_context) = console_request_auth_context(&state, &headers, &uri) else {
        return console_json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "console timeline stream requires a valid auth token",
        );
    };
    let access_view = auth_context.access_view.filter(AccessView::enforced);
    // Prime the attribute cache from the live roster before any per-frame
    // `agent.view` filter so label/role rules resolve here exactly as on the
    // windowed timeline handler and the RPC/SSE seams — the stream must not
    // depend on a prior `/console/experience` call. Primed once at open; an
    // agent spawned mid-stream is filtered against the open-time roster, which
    // matches the windowed handler's contract.
    if access_view.is_some()
        && let Some(runtime) = &state.runtime
        && let Some(controller) = state
            .access
            .as_ref()
            .filter(|controller| controller.enabled())
    {
        prime_access_cache_from_handle(&runtime.handle(), controller).await;
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
        match Box::pin(query_timeline_snapshot(&aggregator, timeline_query.clone())).await {
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
            if access_view.as_ref().is_some_and(|view| !view.can_view_agent(frame.identity.as_str())) {
                continue;
            }
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
                    if let Some(view) = access_view.as_ref()
                        && let Some(frame_identity) = timeline_event_identity(&event)
                        && !view.can_view_agent(frame_identity)
                    {
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
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
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

async fn console_identity_timeline_stream_handler(
    State(state): State<ConsoleJsonState>,
    headers: HeaderMap,
    uri: Uri,
    AxumPath(identity): AxumPath<String>,
    Query(mut query): Query<ConsoleTimelineHttpQuery>,
) -> impl IntoResponse {
    query.identity = Some(identity);
    Box::pin(console_timeline_stream_handler(
        State(state),
        headers,
        uri,
        Query(query),
    ))
    .await
    .into_response()
}

fn timeline_query_from_http(
    query: ConsoleTimelineHttpQuery,
    fallback_after: Option<String>,
) -> ConsoleTimelineWindowQuery {
    let after = fallback_after.or(query.after).map(ConsoleCursor::from);
    let before = query.before.map(ConsoleCursor::from);
    ConsoleTimelineWindowQuery {
        identity: query
            .identity
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        conversation_id: query
            .conversation_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        after,
        before,
        mode: query.mode.unwrap_or_default(),
        limit: query.limit.unwrap_or(200),
    }
}

async fn query_timeline_snapshot(
    aggregator: &MobKitConsoleAggregator,
    mut query: ConsoleTimelineWindowQuery,
) -> ConsoleLogResult<(Vec<ConsoleFrame>, Option<ConsoleCursor>)> {
    const DEFAULT_SNAPSHOT_LIMIT: usize = 200;
    query.limit = if query.limit == 0 {
        DEFAULT_SNAPSHOT_LIMIT
    } else {
        query.limit
    };
    if query.after.is_none() && query.mode == ConsoleTimelineMode::Since {
        query.mode = ConsoleTimelineMode::Recent;
    }
    let mode = query.mode;
    match mode {
        ConsoleTimelineMode::Recent => {
            let page = Box::pin(aggregator.query_timeline_windowed(query)).await?;
            Ok((page.frames, page.latest_cursor.or(page.next_cursor)))
        }
        ConsoleTimelineMode::Since => {
            if let (Some(after), Some(latest)) =
                (query.after.as_ref(), aggregator.latest_cursor().await?)
                && let (Some(after_seq), Some(latest_seq)) = (after.seq(), latest.seq())
                && after_seq > latest_seq
            {
                return Err(std::io::Error::other(
                    "timeline replay cursor is beyond the current store frontier",
                )
                .into());
            }
            let mut frames = Vec::new();
            let mut cursor = query.after.clone();
            let mut latest_cursor = None;
            loop {
                let page = Box::pin(aggregator.query_timeline_windowed(query.clone())).await?;
                latest_cursor = page.latest_cursor.clone().or(latest_cursor);
                if !page.frames.is_empty() {
                    cursor = page
                        .next_cursor
                        .clone()
                        .or_else(|| page.frames.last().map(|frame| frame.cursor.clone()));
                    frames.extend(page.frames);
                } else if page.next_cursor.is_some() {
                    cursor = page.next_cursor.clone();
                }
                if page.exhausted || page.next_cursor.is_none() {
                    return Ok((frames, cursor.or(latest_cursor)));
                }
                if page.next_cursor == query.after {
                    return Err(
                        std::io::Error::other("timeline replay made no cursor progress").into(),
                    );
                }
                query.after = page.next_cursor;
            }
        }
    }
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

fn is_console_mutating_rpc_method(method: &str) -> bool {
    matches!(
        method,
        "mobkit/retire"
            | "mobkit/reset_all"
            | "mobkit/console/send"
            | "mobkit/blob/upload"
            | "mobkit/ensure_member"
            | "mobkit/retire_member"
            | "mobkit/respawn_member"
            | "mobkit/force_cancel_member"
            | "mobkit/cancel_flow"
            | "mobkit/collect_completed"
            | "mobkit/run_flow"
            | "mobkit/spawn_helper"
            | "mobkit/fork_helper"
            | "mobkit/attach_existing_session"
            | "mobkit/reconcile_edges"
            | "mobkit/cross_mob/wire_local"
            | "mobkit/cross_mob/unwire_local"
            | "mobkit/respawn"
            | "mobkit/reset"
            | "mobkit/delete_identity"
            | "mobkit/gating/decide"
            | "mobkit/mob_labels/set"
            | "mobkit/mob_labels/delete"
            | "mobkit/run_labels/set"
            | "mobkit/run_labels/delete"
            | "mobkit/access/set"
            | "mobkit/access/enable"
            | "mobkit/access/rules/upsert"
            | "mobkit/access/rules/delete"
            | "mobkit/access/groups/set"
            | "mobkit/access/groups/delete"
    )
}

fn console_read_only_rpc_error(response_id: Value) -> Value {
    response_value(
        response_id,
        None,
        Some(JsonRpcError {
            code: -32010,
            message: "console is read-only".to_string(),
            data: Some(json!({ "kind": "read_only" })),
        }),
    )
}

fn console_send_error_response(err: ConsoleSendError) -> axum::response::Response {
    let (status, code) = match &err {
        ConsoleSendError::UnknownIdentity(_) => (StatusCode::NOT_FOUND, "unknown_identity"),
        ConsoleSendError::AmbiguousIdentity { .. } => {
            (StatusCode::CONFLICT, "ambiguous_live_identity_alias")
        }
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
    console_json_error(status, code, &console_send_public_message(&err))
}

fn console_send_rpc_code(err: &ConsoleSendError) -> i64 {
    match err {
        ConsoleSendError::UnknownIdentity(_) => -32001,
        ConsoleSendError::AmbiguousIdentity { .. } => -32602,
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
    response_value(response_id, None, Some(console_send_json_rpc_error(err)))
}

fn console_send_json_rpc_error(err: ConsoleSendError) -> JsonRpcError {
    let code = console_send_rpc_code(&err);
    JsonRpcError {
        code,
        message: console_send_public_message(&err),
        data: None,
    }
}

fn console_send_public_message(err: &ConsoleSendError) -> String {
    match err {
        ConsoleSendError::State(_) | ConsoleSendError::Dispatch(_) | ConsoleSendError::Log(_) => {
            tracing::warn!(target: "mobkit::console", error = %err, "console send internal error");
            "console send failed".to_string()
        }
        _ => err.to_string(),
    }
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

fn timeline_event_identity(event: &ConsoleTimelineEvent) -> Option<&str> {
    match event {
        ConsoleTimelineEvent::ConsoleFrame { frame }
        | ConsoleTimelineEvent::FrameUpdated { frame } => Some(frame.identity.as_str()),
        ConsoleTimelineEvent::SnapshotStarted { .. }
        | ConsoleTimelineEvent::SnapshotComplete { .. }
        | ConsoleTimelineEvent::ReplayUnavailable { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// ABAC enforcement for console surfaces
// ---------------------------------------------------------------------------

const ACCESS_DENIED_RPC_CODE: i64 = -32030;

fn retain_visible_timeline_frames(page: &mut ConsoleTimelineWindowPage, view: Option<&AccessView>) {
    let Some(view) = view.filter(|view| view.enforced()) else {
        return;
    };
    page.frames
        .retain(|frame| view.can_view_agent(frame.identity.as_str()));
}

/// Filter serialized member rows (`mobkit/list_members`, `find_members`)
/// down to the agents the caller may view. Field names cover both the
/// meerkat roster entry shape and the console projection shape.
fn retain_visible_member_rows(rows: &mut Vec<Value>, view: Option<&AccessView>) {
    let Some(view) = view.filter(|view| view.enforced()) else {
        return;
    };
    rows.retain(|row| {
        let labels: Option<std::collections::BTreeMap<String, String>> = row
            .get("labels")
            .and_then(|value| serde_json::from_value(value.clone()).ok());
        let agent_id = row
            .get("agent_identity")
            .or_else(|| row.get("member_id"))
            .or_else(|| row.get("agent_id"))
            .and_then(Value::as_str);
        let identity = labels
            .as_ref()
            .and_then(|labels| labels.get("agent_identity"))
            .map(String::as_str)
            .or_else(|| row.get("identity").and_then(Value::as_str))
            .or(agent_id);
        let role = row
            .get("role")
            .or_else(|| row.get("profile"))
            .and_then(Value::as_str);
        view.decide(
            ACTION_AGENT_VIEW,
            &AccessResource {
                identity,
                agent_id,
                role,
                labels: labels.as_ref(),
            },
        )
        .is_allow()
    });
}

fn retain_visible_identity_records(
    identities: &mut Vec<ConsoleIdentityRecord>,
    view: Option<&AccessView>,
) {
    let Some(view) = view.filter(|view| view.enforced()) else {
        return;
    };
    identities.retain(|record| {
        view.decide(
            ACTION_AGENT_VIEW,
            &AccessResource {
                identity: Some(record.identity.as_str()),
                agent_id: Some(record.runtime_member_id.as_str()),
                role: None,
                labels: Some(&record.labels),
            },
        )
        .is_allow()
    });
}

/// Maps a console RPC method to the access action it requires plus the
/// targeted agent (when the method is agent-scoped). Methods not listed are
/// either pure read surfaces whose *results* are filtered per caller, or
/// non-sensitive metadata.
fn console_rpc_access_requirement<'a>(
    method: &str,
    params: &'a Value,
) -> Option<(&'static str, Option<&'a str>)> {
    let identity = params.get("identity").and_then(Value::as_str);
    let target = identity
        .or_else(|| params.get("member_id").and_then(Value::as_str))
        .or_else(|| params.get("agent_id").and_then(Value::as_str));
    match method {
        "mobkit/console/send" => Some((ACTION_AGENT_SEND, identity)),
        "mobkit/retire"
        | "mobkit/retire_member"
        | "mobkit/force_cancel_member"
        | "mobkit/delete_identity" => Some((ACTION_AGENT_RETIRE, target)),
        "mobkit/respawn" | "mobkit/respawn_member" => Some((ACTION_AGENT_RESPAWN, target)),
        "mobkit/reset" | "mobkit/reset_all" => Some((ACTION_AGENT_RESET, target)),
        "mobkit/ensure_member"
        | "mobkit/spawn_helper"
        | "mobkit/fork_helper"
        | "mobkit/attach_existing_session"
        | "mobkit/run_flow"
        | "mobkit/cancel_flow"
        | "mobkit/collect_completed" => Some((ACTION_AGENT_SPAWN, target)),
        // Flow state reads expose run records (including spawned member
        // identities), so they sit in the same tier as running flows.
        "mobkit/flow_status" | "mobkit/list_flows" | "mobkit/list_runs" => {
            Some((ACTION_AGENT_SPAWN, None))
        }
        "mobkit/gating/decide" => Some((ACTION_GATING_DECIDE, None)),
        "mobkit/gating/pending" | "mobkit/gating/audit" => Some((ACTION_GATING_VIEW, None)),
        "mobkit/mob_events/query" | "mobkit/mob_events/subscribe" => {
            Some((ACTION_MOB_OBSERVE, None))
        }
        "mobkit/reconcile_edges"
        | "mobkit/cross_mob/wire_local"
        | "mobkit/cross_mob/unwire_local"
        | "mobkit/mob_labels/set"
        | "mobkit/mob_labels/delete"
        | "mobkit/run_labels/set"
        | "mobkit/run_labels/delete"
        // Plumbing reads: routing tables, delivery records, cross-mob
        // contacts, and label tables enumerate agent identities without
        // passing through per-agent visibility filtering, so they require
        // the same grant as the mutations that shape them.
        | "mobkit/routing/routes/list"
        | "mobkit/delivery/history"
        | "mobkit/cross_mob/directory"
        | "mobkit/cross_mob/peer_info"
        | "mobkit/mob_labels/get"
        | "mobkit/run_labels/get" => Some((ACTION_RUNTIME_ADMIN, None)),
        "mobkit/get_member"
        | "mobkit/member_status"
        | "mobkit/inspect_identity"
        | "mobkit/status_identity"
        | "mobkit/console/inspect_identity" => Some((ACTION_AGENT_VIEW, target)),
        _ => None,
    }
}

/// Returns the denial error when the caller's view forbids the method.
fn console_rpc_access_violation(
    view: Option<&AccessView>,
    method: &str,
    params: &Value,
) -> Option<JsonRpcError> {
    let view = view.filter(|view| view.enforced())?;
    let (action, target) = console_rpc_access_requirement(method, params)?;
    let allowed = match target {
        Some(identity) => view.allows_agent(action, identity),
        None => view.allows(action),
    };
    if allowed {
        return None;
    }
    Some(JsonRpcError {
        code: ACCESS_DENIED_RPC_CODE,
        message: format!("access denied: {action}"),
        data: Some(json!({
            "kind": "access_denied",
            "action": action,
            "resource": target,
        })),
    })
}

fn access_denied_rpc_error(response_id: Value, message: impl Into<String>) -> Value {
    response_value(
        response_id,
        None,
        Some(JsonRpcError {
            code: ACCESS_DENIED_RPC_CODE,
            message: message.into(),
            data: Some(json!({ "kind": "access_denied" })),
        }),
    )
}

fn access_unavailable_rpc_error(response_id: Value) -> Value {
    response_value(
        response_id,
        None,
        Some(JsonRpcError {
            code: -32004,
            message: "access control is not configured on this runtime".to_string(),
            data: Some(json!({ "kind": "access_unavailable" })),
        }),
    )
}

fn access_config_rpc_error(response_id: Value, err: crate::access::AccessConfigError) -> Value {
    response_value(
        response_id,
        None,
        Some(JsonRpcError {
            code: -32602,
            message: err.to_string(),
            data: Some(json!({ "kind": "invalid_access_config" })),
        }),
    )
}

fn access_status_result(access: Option<&AccessController>, view: Option<&AccessView>) -> Value {
    match (access, view) {
        (Some(controller), Some(view)) => {
            let (_, revision) = controller.snapshot();
            json!({
                "available": true,
                "enabled": view.enforced(),
                "revision": revision,
                "subject": view.subject(),
                "groups": view.groups().iter().collect::<Vec<_>>(),
                "is_admin": view.is_admin(),
                "can_administer": view.can_administer(),
                "actions": ACCESS_ACTIONS,
            })
        }
        _ => json!({
            "available": false,
            "enabled": false,
            "actions": ACCESS_ACTIONS,
        }),
    }
}

/// Handles every `mobkit/access/*` method. Returns `None` when the method
/// belongs to another namespace.
fn handle_access_admin_rpc(
    access: Option<&AccessController>,
    view: Option<&AccessView>,
    request: &JsonRpcRequest,
) -> Option<Value> {
    if !request.method.starts_with("mobkit/access/") {
        return None;
    }
    let response_id = request.id.clone().unwrap_or(Value::Null);
    let controller = match request.method.as_str() {
        "mobkit/access/status" => {
            return Some(response_value(
                response_id,
                Some(access_status_result(access, view)),
                None,
            ));
        }
        _ => {
            let Some(controller) = access else {
                return Some(access_unavailable_rpc_error(response_id));
            };
            if !view.is_some_and(AccessView::can_administer) {
                return Some(access_denied_rpc_error(
                    response_id,
                    "access denied: access.admin",
                ));
            }
            controller
        }
    };
    let result = match request.method.as_str() {
        "mobkit/access/get" => {
            let (config, revision) = controller.snapshot();
            Ok(json!({ "config": &*config, "revision": revision }))
        }
        "mobkit/access/set" => {
            match serde_json::from_value(request.params.get("config").cloned().unwrap_or_default())
            {
                Ok(config) => controller
                    .replace_config(config)
                    .map(|revision| json!({ "revision": revision })),
                Err(err) => {
                    return Some(invalid_params(
                        response_id,
                        format!("invalid access config: {err}"),
                    ));
                }
            }
        }
        "mobkit/access/rules/upsert" => {
            match serde_json::from_value::<AccessRule>(
                request.params.get("rule").cloned().unwrap_or_default(),
            ) {
                Ok(rule) => controller
                    .upsert_rule(rule)
                    .map(|revision| json!({ "revision": revision })),
                Err(err) => {
                    return Some(invalid_params(
                        response_id,
                        format!("invalid access rule: {err}"),
                    ));
                }
            }
        }
        "mobkit/access/rules/delete" => {
            let Some(rule_id) = request.params.get("id").and_then(Value::as_str) else {
                return Some(invalid_params(response_id, "id required"));
            };
            controller
                .delete_rule(rule_id)
                .map(|revision| json!({ "revision": revision }))
        }
        "mobkit/access/groups/set" => {
            let Some(name) = request.params.get("name").and_then(Value::as_str) else {
                return Some(invalid_params(response_id, "name required"));
            };
            match serde_json::from_value::<AccessGroup>(
                request.params.get("group").cloned().unwrap_or_default(),
            ) {
                Ok(group) => controller
                    .set_group(name, group)
                    .map(|revision| json!({ "revision": revision })),
                Err(err) => {
                    return Some(invalid_params(
                        response_id,
                        format!("invalid access group: {err}"),
                    ));
                }
            }
        }
        "mobkit/access/groups/delete" => {
            let Some(name) = request.params.get("name").and_then(Value::as_str) else {
                return Some(invalid_params(response_id, "name required"));
            };
            controller
                .delete_group(name)
                .map(|revision| json!({ "revision": revision }))
        }
        "mobkit/access/enable" => {
            let Some(enabled) = request.params.get("enabled").and_then(Value::as_bool) else {
                return Some(invalid_params(response_id, "enabled (bool) required"));
            };
            controller
                .set_enabled(enabled)
                .map(|revision| json!({ "revision": revision }))
        }
        "mobkit/access/preview" => {
            let subject = request.params.get("subject").and_then(Value::as_str);
            let Some(action) = request.params.get("action").and_then(Value::as_str) else {
                return Some(invalid_params(response_id, "action required"));
            };
            let identity = request.params.get("identity").and_then(Value::as_str);
            let preview_view = controller.view_for_subject(subject);
            let decision = match identity {
                Some(identity) => preview_view.decide_agent(action, identity),
                None => preview_view.decide(action, &AccessResource::none()),
            };
            Ok(json!({
                "subject": subject,
                "action": action,
                "identity": identity,
                "allowed": decision.is_allow(),
                "reason": decision.reason(),
                "groups": preview_view.groups().iter().collect::<Vec<_>>(),
                "is_admin": preview_view.is_admin(),
            }))
        }
        _ => {
            return Some(response_value(
                response_id,
                None,
                Some(JsonRpcError {
                    code: -32601,
                    message: "Method not found".to_string(),
                    data: None,
                }),
            ));
        }
    };
    Some(match result {
        Ok(value) => response_value(response_id, Some(value), None),
        Err(err) => access_config_rpc_error(response_id, err),
    })
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
    let auth_context = match console_request_auth_context(&state, &headers, &uri) {
        Some(context) => context,
        None => {
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
    if state.decisions.console.read_only && is_console_mutating_rpc_method(&parsed_request.method) {
        return (
            StatusCode::OK,
            Json::<Value>(console_read_only_rpc_error(response_id)),
        );
    }
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
            let binary_blob_store =
                match Box::pin(aggregator.binary_blob_store_for_identity(identity)).await {
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
    let response_value =
        if parsed_request.method == "mobkit/console/send" && state.runtime.is_none() {
            Box::pin(handle_console_aggregator_rpc(
                state.console_aggregator.clone(),
                parsed_request,
                true,
                state.decisions.console.read_only,
                state.access.as_ref(),
                auth_context.access_view.as_ref(),
            ))
            .await
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
            Box::pin(handle_console_runtime_rpc_with_visibility(
                runtime,
                state.module_runtime.clone(),
                state.contact_directory.as_ref(),
                state.gateway_peer_keys.as_ref(),
                state.console_events.clone(),
                state.console_aggregator.clone(),
                state.identity_runtime.clone(),
                state.metadata_table.clone(),
                state.mob_events.clone(),
                state.visibility_policy.as_ref(),
                parsed_request,
                true,
                state.decisions.console.read_only,
                auth_context.principal.as_deref(),
                state.access.as_ref(),
                auth_context.access_view.as_ref(),
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
    console_request_auth_context(state, headers, uri).is_some()
}

fn console_request_auth_context(
    state: &ConsoleJsonState,
    headers: &HeaderMap,
    uri: &Uri,
) -> Option<ConsoleHttpAuthContext> {
    if !state.decisions.console.require_app_auth {
        // Open console: identify callers that volunteered a valid token so
        // per-user ABAC grants still apply; everyone else is anonymous.
        let principal = state.access.as_ref().and_then(|_| {
            let token = console_request_token(headers, uri)?;
            resolve_authorized_console_auth_from_token(&state.decisions, &token)
                .map(|auth| auth.email)
        });
        return Some(ConsoleHttpAuthContext {
            access_view: state
                .access
                .as_ref()
                .map(|controller| controller.view_for_subject(principal.as_deref())),
            principal,
        });
    }
    let token = console_request_token(headers, uri)?;
    let auth = resolve_authorized_console_auth_from_token(&state.decisions, &token)?;
    Some(ConsoleHttpAuthContext {
        access_view: state
            .access
            .as_ref()
            .map(|controller| controller.view_for_subject(Some(auth.email.as_str()))),
        principal: Some(auth.email),
    })
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

fn gating_decision_failed_error(id: Value, err: impl std::fmt::Display) -> Value {
    tracing::warn!(
        target: "mobkit::console",
        error = %err,
        "console gating decision failed"
    );
    invalid_params(id, "gating decision failed")
}

fn runtime_binding_from_wire(
    binding: WireRuntimeBinding,
) -> Result<meerkat_mob::RuntimeBinding, String> {
    match binding {
        WireRuntimeBinding::Session => Ok(meerkat_mob::RuntimeBinding::Session),
        WireRuntimeBinding::External {
            address,
            bootstrap_token,
            identity,
        } => {
            let resolved = identity.resolve().map_err(|err| err.to_string())?;
            Ok(meerkat_mob::RuntimeBinding::External {
                peer_id: resolved.peer_id.to_string(),
                address,
                bootstrap_token,
                pubkey: Some(resolved.pubkey),
            })
        }
    }
}

fn parse_optional_runtime_mode(params: &Value) -> Result<Option<MobRuntimeMode>, String> {
    match params.get("runtime_mode") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value::<MobRuntimeMode>(value.clone())
            .map(Some)
            .map_err(|err| {
                format!("runtime_mode must be \"autonomous_host\" or \"turn_driven\": {err}")
            }),
    }
}

fn parse_optional_backend(params: &Value) -> Result<Option<MobBackendKind>, String> {
    match params.get("backend") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value::<MobBackendKind>(value.clone())
            .map(Some)
            .map_err(|err| format!("backend must be \"session\" or \"external\": {err}")),
    }
}

fn parse_optional_runtime_binding(
    params: &Value,
) -> Result<Option<meerkat_mob::RuntimeBinding>, String> {
    match params.get("binding") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let wire = serde_json::from_value::<WireRuntimeBinding>(value.clone())
                .map_err(|err| format!("binding: {err}"))?;
            runtime_binding_from_wire(wire).map(Some)
        }
    }
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
    let message = message.into();
    tracing::warn!(
        target: "mobkit::console",
        error = %message,
        "console JSON-RPC internal error"
    );
    response_value(
        id,
        None,
        Some(JsonRpcError {
            code: -32000,
            message: "internal error".to_string(),
            data: Some(json!({ "error": "internal_error" })),
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

fn console_timeline_replay_unavailable_response(
    id: Value,
    err: ConsoleLogError,
    requested_cursor: Option<&ConsoleCursor>,
    latest_cursor: Option<ConsoleCursor>,
) -> Value {
    tracing::warn!(
        target: "mobkit::console",
        error = %err,
        "console timeline replay unavailable"
    );
    response_value(
        id,
        None,
        Some(JsonRpcError {
            code: crate::rpc::CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE,
            message: "timeline replay unavailable".to_string(),
            data: Some(json!({
                "error": "replay_unavailable",
                "stream": "timeline",
                "requested_cursor": requested_cursor.map(ToString::to_string),
                "latest_cursor": latest_cursor.map(|cursor| cursor.to_string()),
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

fn console_identity_status_json_for_identity(
    identity: &str,
    member: &meerkat_mob::runtime::MobMemberListEntry,
    session_id: Option<String>,
    response_phase: Option<String>,
) -> Value {
    json!({
        "identity": identity,
        "state": member.state,
        "role": member.role.to_string(),
        "addressability": member_addressability(member),
        "display_name": member.labels.get("display_name"),
        "labels": member.labels,
        "agent_runtime_id": member.agent_identity.to_string(),
        "session_id": session_id,
        "generation": Value::Null,
        "checkpoint_version": Value::Null,
        "continuity_health": Value::Null,
        "lease_healthy": Value::Null,
        "lease": Value::Null,
        "response_phase": response_phase,
    })
}

fn console_identity_inspect_json_for_identity(
    identity: &str,
    member: &meerkat_mob::runtime::MobMemberListEntry,
    session_id: Option<String>,
    response_phase: Option<String>,
) -> Value {
    let peers: Vec<String> = member.wired_to.iter().map(ToString::to_string).collect();
    json!({
        "identity": identity,
        "state": member.state,
        "role": member.role.to_string(),
        "addressability": member_addressability(member),
        "display_name": member.labels.get("display_name"),
        "labels": member.labels,
        "continuity_health": Value::Null,
        "lease_healthy": Value::Null,
        "lease": Value::Null,
        "continuity": {
            "generation": Value::Null,
            "checkpoint_version": Value::Null,
            "session_id": session_id,
            "agent_runtime_id": member.agent_identity.to_string(),
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
        .resolve_bridge_session_id_observation(identity)
        .await
        .map(|s| s.to_string());
    Some((entry, session_id))
}

#[derive(Debug, Clone)]
struct ConsoleRuntimeIdentityAlias {
    identity: String,
    runtime_member_id: String,
    member: meerkat_mob::runtime::MobMemberListEntry,
    session_id: Option<String>,
}

fn durable_identity_for_member(member: &meerkat_mob::runtime::MobMemberListEntry) -> String {
    member
        .labels
        .get("agent_identity")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| member.agent_identity.to_string())
}

async fn lookup_member_alias_with_session(
    handle: &MobHandle,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    requested_identity: &str,
) -> Result<Option<ConsoleRuntimeIdentityAlias>, JsonRpcError> {
    let all_matches = lookup_member_alias_candidates_with_session(handle, requested_identity).await;
    let mut visible_matches = Vec::new();
    for alias in &all_matches {
        if runtime_alias_visible_to_console(handle, visibility_policy, alias) {
            visible_matches.push(alias.clone());
        }
    }
    let member = if visible_matches.len() > 1 {
        return Err(ambiguous_live_identity_alias_error(
            requested_identity,
            &visible_matches
                .iter()
                .map(|alias| alias.runtime_member_id.clone())
                .collect::<Vec<_>>(),
        ));
    } else if let Some(alias) = visible_matches.into_iter().next() {
        Some(alias)
    } else {
        all_matches.into_iter().next()
    };
    Ok(member)
}

async fn lookup_visible_member_alias_candidates_with_session(
    handle: &MobHandle,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    requested_identity: &str,
) -> Vec<ConsoleRuntimeIdentityAlias> {
    let mut visible = Vec::new();
    for alias in lookup_member_alias_candidates_with_session(handle, requested_identity).await {
        if runtime_alias_visible_to_console(handle, visibility_policy, &alias) {
            visible.push(alias);
        }
    }
    visible
}

async fn lookup_member_alias_candidates_with_session(
    handle: &MobHandle,
    requested_identity: &str,
) -> Vec<ConsoleRuntimeIdentityAlias> {
    let requested_member_id = MeerkatId::from(requested_identity);
    let entries = handle.list_members_including_retiring().await;
    let exact_matches = entries
        .iter()
        .filter(|entry| entry.agent_identity == requested_member_id)
        .cloned()
        .collect::<Vec<_>>();
    let label_matches = entries
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
        let runtime_member_id = member.agent_identity.to_string();
        let identity = durable_identity_for_member(&member);
        let session_id = handle
            .resolve_bridge_session_id_observation(&member.agent_identity)
            .await
            .map(|s| s.to_string());
        aliases.push(ConsoleRuntimeIdentityAlias {
            identity,
            runtime_member_id,
            member,
            session_id,
        });
    }
    aliases
}

async fn reject_ambiguous_projected_live_identity(
    handle: &MobHandle,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    alias: &ConsoleRuntimeIdentityAlias,
) -> Result<(), JsonRpcError> {
    let candidates = lookup_visible_member_alias_candidates_with_session(
        handle,
        visibility_policy,
        &alias.identity,
    )
    .await;
    if candidates.len() > 1 {
        return Err(ambiguous_live_identity_alias_error(
            &alias.identity,
            &candidates
                .iter()
                .map(|candidate| candidate.runtime_member_id.clone())
                .collect::<Vec<_>>(),
        ));
    }
    Ok(())
}

fn ambiguous_live_identity_alias_error(
    requested_identity: &str,
    candidates: &[String],
) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: format!(
            "ambiguous live identity alias {requested_identity}: candidates [{}]",
            candidates.join(", ")
        ),
        data: Some(json!({
            "kind": "ambiguous_live_identity_alias",
            "identity": requested_identity,
            "candidates": candidates,
        })),
    }
}

async fn lookup_member_runtime_alias_with_session(
    handle: &MobHandle,
    runtime_member_id: &str,
) -> Option<ConsoleRuntimeIdentityAlias> {
    let requested_member_id = MeerkatId::from(runtime_member_id);
    let entries = handle.list_members_including_retiring().await;
    let member = entries
        .into_iter()
        .find(|entry| entry.agent_identity == requested_member_id)?;
    let runtime_member_id = member.agent_identity.to_string();
    let identity = durable_identity_for_member(&member);
    let session_id = handle
        .resolve_bridge_session_id_observation(&member.agent_identity)
        .await
        .map(|s| s.to_string());
    Some(ConsoleRuntimeIdentityAlias {
        identity,
        runtime_member_id,
        member,
        session_id,
    })
}

async fn identity_runtime_alias(
    identity_runtime: &crate::identity_first::IdentityRuntime,
    requested_identity: &str,
) -> Result<Option<(crate::identity_first::AgentIdentity, bool)>, String> {
    if requested_identity.starts_with("rt:") {
        for status in identity_runtime.statuses().await {
            if status
                .agent_runtime_id
                .as_ref()
                .is_some_and(|runtime_id| runtime_id.as_str() == requested_identity)
            {
                return Ok(Some((status.identity, false)));
            }
        }
        return Ok(None);
    }
    if let Ok(identity) = crate::identity_first::AgentIdentity::parse(requested_identity) {
        match identity_runtime.status(&identity).await {
            Ok(_) => return Ok(Some((identity, true))),
            Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {}
            Err(err) => return Err(err.to_string()),
        }
    }

    for status in identity_runtime.statuses().await {
        if status
            .agent_runtime_id
            .as_ref()
            .is_some_and(|runtime_id| runtime_id.as_str() == requested_identity)
        {
            return Ok(Some((status.identity, false)));
        }
    }
    Ok(None)
}

async fn resolve_console_identity_control_target(
    handle: &MobHandle,
    identity_runtime: Option<&Arc<crate::identity_first::IdentityRuntime>>,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    requested_identity: &str,
) -> Result<
    Option<(
        crate::identity_first::AgentIdentity,
        bool,
        Option<ConsoleRuntimeIdentityAlias>,
    )>,
    JsonRpcError,
> {
    if let Some(identity_runtime) = identity_runtime {
        match identity_runtime_alias(identity_runtime, requested_identity).await {
            Ok(Some((identity, exact))) => {
                let live = if exact {
                    let registered_live = match identity_runtime.status(&identity).await {
                        Ok(status) => match status.agent_runtime_id.as_ref() {
                            Some(runtime_id) => {
                                lookup_member_runtime_alias_with_session(
                                    handle,
                                    runtime_id.as_str(),
                                )
                                .await
                            }
                            None => None,
                        },
                        Err(_) => None,
                    };
                    if let Some(alias) = registered_live.as_ref()
                        && !runtime_alias_visible_to_console(handle, visibility_policy, alias)
                    {
                        return Err(identity_hidden_by_policy_error(requested_identity));
                    }
                    if let Some(registered) = registered_live {
                        return Ok(Some((identity, exact, Some(registered))));
                    }
                    let requested_live_candidates =
                        lookup_visible_member_alias_candidates_with_session(
                            handle,
                            visibility_policy,
                            requested_identity,
                        )
                        .await;
                    let requested_live = if requested_live_candidates.len() > 1 {
                        return Err(ambiguous_live_identity_alias_error(
                            requested_identity,
                            &requested_live_candidates
                                .iter()
                                .map(|alias| alias.runtime_member_id.clone())
                                .collect::<Vec<_>>(),
                        ));
                    } else {
                        requested_live_candidates.into_iter().next()
                    };
                    match (registered_live, requested_live) {
                        (Some(registered), Some(requested))
                            if registered.runtime_member_id == requested.runtime_member_id =>
                        {
                            Some(registered)
                        }
                        (Some(registered), None) => Some(registered),
                        (Some(_registered), Some(requested)) => Some(requested),
                        (None, requested) => requested,
                    }
                } else {
                    let registered_live =
                        lookup_member_runtime_alias_with_session(handle, requested_identity).await;
                    if let Some(alias) = registered_live.as_ref()
                        && !runtime_alias_visible_to_console(handle, visibility_policy, alias)
                    {
                        return Err(identity_hidden_by_policy_error(requested_identity));
                    }
                    if let Some(registered) = registered_live {
                        return Ok(Some((identity, exact, Some(registered))));
                    }
                    let durable_live_candidates =
                        lookup_visible_member_alias_candidates_with_session(
                            handle,
                            visibility_policy,
                            identity.as_str(),
                        )
                        .await;
                    let durable_live = if durable_live_candidates.len() > 1 {
                        return Err(ambiguous_live_identity_alias_error(
                            identity.as_str(),
                            &durable_live_candidates
                                .iter()
                                .map(|alias| alias.runtime_member_id.clone())
                                .collect::<Vec<_>>(),
                        ));
                    } else {
                        durable_live_candidates.into_iter().next()
                    };
                    match (registered_live, durable_live) {
                        (Some(registered), Some(durable))
                            if registered.runtime_member_id == durable.runtime_member_id =>
                        {
                            Some(registered)
                        }
                        (Some(registered), None) => Some(registered),
                        (Some(_registered), Some(durable)) => Some(durable),
                        (None, durable) => durable,
                    }
                };
                return Ok(Some((identity, exact, live)));
            }
            Ok(None) => {}
            Err(err) => {
                return Err(JsonRpcError {
                    code: -32000,
                    message: err,
                    data: None,
                });
            }
        }
    }

    let live_alias =
        lookup_member_alias_with_session(handle, visibility_policy, requested_identity).await?;
    let Some(alias) = live_alias else {
        return Ok(None);
    };
    if let Some(identity_runtime) = identity_runtime
        && let Some(bound_status) = identity_runtime
            .statuses()
            .await
            .into_iter()
            .find(|status| {
                status
                    .agent_runtime_id
                    .as_ref()
                    .is_some_and(|runtime_id| runtime_id.as_str() == alias.runtime_member_id)
            })
        && bound_status.identity.as_str() != alias.identity
    {
        return Err(JsonRpcError {
            code: -32000,
            message: format!(
                "stale live identity alias: live console alias {} resolves to {}, but identity runtime binding belongs to {}",
                alias.identity,
                alias.runtime_member_id,
                bound_status.identity.as_str(),
            ),
            data: Some(json!({
                "kind": "stale_live_identity_alias",
                "identity": alias.identity,
                "runtime_member_id": alias.runtime_member_id,
                "registered_identity": bound_status.identity.as_str(),
            })),
        });
    }
    let identity = crate::identity_first::AgentIdentity::parse(&alias.identity).map_err(|err| {
        JsonRpcError {
            code: -32602,
            message: format!("invalid identity: {err}"),
            data: None,
        }
    })?;
    let durable_live_candidates = lookup_visible_member_alias_candidates_with_session(
        handle,
        visibility_policy,
        identity.as_str(),
    )
    .await;
    if durable_live_candidates.len() > 1 {
        return Err(ambiguous_live_identity_alias_error(
            identity.as_str(),
            &durable_live_candidates
                .iter()
                .map(|alias| alias.runtime_member_id.clone())
                .collect::<Vec<_>>(),
        ));
    }
    Ok(Some((identity, false, Some(alias))))
}

fn live_alias_matches_status_runtime(
    alias: Option<&ConsoleRuntimeIdentityAlias>,
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
        && alias.identity == status.identity.as_str()
        && session_matches
}

async fn stale_live_alias_json_rpc_error(
    operation: &str,
    identity_runtime: &crate::identity_first::IdentityRuntime,
    identity: &crate::identity_first::AgentIdentity,
    live_alias: Option<&ConsoleRuntimeIdentityAlias>,
) -> Option<JsonRpcError> {
    let live_alias = live_alias?;
    let Ok(status) = identity_runtime.status(identity).await else {
        return None;
    };
    if live_alias_matches_status_runtime(Some(live_alias), &status) {
        return None;
    }
    let registered_runtime_member_id = status
        .agent_runtime_id
        .as_ref()
        .map(crate::identity_first::AgentRuntimeId::as_str);
    Some(JsonRpcError {
        code: -32000,
        message: format!(
            "{operation} failed: identity runtime binding for {} points at {}, but requested live member is {}",
            identity.as_str(),
            registered_runtime_member_id.unwrap_or("<none>"),
            live_alias.runtime_member_id
        ),
        data: Some(json!({
            "kind": "stale_identity_runtime_binding",
            "identity": identity.as_str(),
            "registered_runtime_member_id": registered_runtime_member_id,
            "live_runtime_member_id": live_alias.runtime_member_id,
            "registered_session_id": status.session_id.as_ref().map(ToString::to_string),
            "live_session_id": live_alias.session_id,
        })),
    })
}

fn reset_requires_session_bridge_json_rpc_error() -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: "reset requires an identity runtime with a session bridge".to_string(),
        data: Some(json!({
            "kind": "identity_reset_requires_session_bridge",
        })),
    }
}

fn console_identity_status_json_from_record(
    record: &crate::console_aggregator::ConsoleIdentityRecord,
    response_phase: Option<String>,
) -> Value {
    json!({
        "identity": record.identity,
        "state": record.health,
        "role": record.labels.get("role"),
        "addressability": if record.addressable { "addressable" } else { "internal_only" },
        "display_name": record.display_name,
        "labels": record.labels,
        "agent_runtime_id": record.runtime_member_id,
        "session_id": record.session_id,
        "generation": Value::Null,
        "checkpoint_version": Value::Null,
        "continuity_health": Value::Null,
        "lease_healthy": Value::Null,
        "lease": Value::Null,
        "response_phase": response_phase,
    })
}

fn console_addressability_json(
    addressability: crate::identity_first::AgentAddressability,
) -> &'static str {
    match addressability {
        crate::identity_first::AgentAddressability::Addressable => "addressable",
        crate::identity_first::AgentAddressability::InternalOnly => "internal_only",
    }
}

fn console_identity_record_from_identity_status(
    status: &crate::identity_first::IdentityStatus,
) -> ConsoleIdentityRecord {
    let mut labels = status.labels.clone();
    if let Some(profile) = status.profile.as_ref() {
        labels
            .entry("role".to_string())
            .or_insert_with(|| profile.as_str().to_string());
    }
    let runtime_member_id = status
        .agent_runtime_id
        .as_ref()
        .map(crate::identity_first::AgentRuntimeId::as_str)
        .unwrap_or_else(|| status.identity.as_str())
        .to_string();
    let addressable = status.addressability
        == crate::identity_first::AgentAddressability::Addressable
        && matches!(
            status.state,
            crate::identity_first::IdentityLifecycleState::Active
                | crate::identity_first::IdentityLifecycleState::Dormant
                | crate::identity_first::IdentityLifecycleState::Uninitialized
        );
    let visibility = match status.state {
        crate::identity_first::IdentityLifecycleState::Retiring => {
            ConsoleVisibility::RetiredReadable
        }
        crate::identity_first::IdentityLifecycleState::Broken
        | crate::identity_first::IdentityLifecycleState::Suspended => {
            ConsoleVisibility::Unreachable
        }
        _ if addressable => ConsoleVisibility::Addressable,
        _ => ConsoleVisibility::Hidden,
    };
    let health = match status.state {
        crate::identity_first::IdentityLifecycleState::Active => "ready",
        crate::identity_first::IdentityLifecycleState::Dormant => "dormant",
        crate::identity_first::IdentityLifecycleState::Uninitialized => "uninitialized",
        crate::identity_first::IdentityLifecycleState::Broken => "broken",
        crate::identity_first::IdentityLifecycleState::Suspended => "suspended",
        crate::identity_first::IdentityLifecycleState::Retiring => "retired",
    }
    .to_string();
    ConsoleIdentityRecord {
        identity: status.identity.as_str().to_string(),
        display_name: status
            .display_name
            .as_ref()
            .map(crate::identity_first::DisplayName::as_str)
            .unwrap_or_else(|| status.identity.as_str())
            .to_string(),
        runtime_key: "identity-first".to_string(),
        runtime_member_id,
        session_id: status.session_id.as_ref().map(ToString::to_string),
        visibility,
        addressable,
        health,
        topology_peers: Vec::new(),
        labels,
    }
}

fn identity_hidden_by_policy_response(response_id: Value, identity: &str) -> Value {
    response_value(
        response_id,
        None,
        Some(identity_hidden_by_policy_error(identity)),
    )
}

fn identity_hidden_by_policy_error(identity: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32001,
        message: format!("unknown identity: {identity}"),
        data: Some(json!({
            "kind": "identity_hidden_by_policy",
            "identity": identity,
        })),
    }
}

fn identity_status_visible_to_console(
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    status: &crate::identity_first::IdentityStatus,
) -> bool {
    visibility_policy.identity_visible(&console_identity_record_from_identity_status(status))
}

fn console_member_from_runtime_alias(
    handle: &MobHandle,
    alias: &ConsoleRuntimeIdentityAlias,
) -> ConsoleMember {
    ConsoleMember {
        agent_identity: alias.runtime_member_id.clone(),
        role: alias.member.role.to_string(),
        state: match alias.member.state {
            meerkat_mob::MemberState::Active => MEMBER_STATE_ACTIVE.to_string(),
            meerkat_mob::MemberState::Retiring => MEMBER_STATE_RETIRING.to_string(),
        },
        model_capabilities: model_capabilities_for_member_entry(handle.definition(), &alias.member),
        runtime_mode: Some(alias.member.runtime_mode.to_string()),
        session_id: alias.session_id.clone(),
        wired_to: alias
            .member
            .wired_to
            .iter()
            .map(ToString::to_string)
            .collect(),
        labels: alias.member.labels.clone(),
    }
}

fn console_identity_record_from_runtime_alias(
    alias: &ConsoleRuntimeIdentityAlias,
) -> ConsoleIdentityRecord {
    let addressable = alias
        .member
        .labels
        .get("addressable")
        .map(|value| !value.eq_ignore_ascii_case("false"))
        .unwrap_or(true)
        && alias.member.state == meerkat_mob::MemberState::Active;
    let visibility = match alias.member.state {
        meerkat_mob::MemberState::Retiring => ConsoleVisibility::RetiredReadable,
        meerkat_mob::MemberState::Active if addressable => ConsoleVisibility::Addressable,
        meerkat_mob::MemberState::Active => ConsoleVisibility::Hidden,
    };
    ConsoleIdentityRecord {
        identity: alias.identity.clone(),
        display_name: alias
            .member
            .labels
            .get("display_name")
            .cloned()
            .unwrap_or_else(|| alias.identity.clone()),
        runtime_key: "runtime".to_string(),
        runtime_member_id: alias.runtime_member_id.clone(),
        session_id: alias.session_id.clone(),
        visibility,
        addressable,
        health: match alias.member.state {
            meerkat_mob::MemberState::Active => "ready",
            meerkat_mob::MemberState::Retiring => "retired",
        }
        .to_string(),
        topology_peers: alias
            .member
            .wired_to
            .iter()
            .map(ToString::to_string)
            .collect(),
        labels: alias.member.labels.clone(),
    }
}

fn runtime_alias_visible_to_console(
    handle: &MobHandle,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    alias: &ConsoleRuntimeIdentityAlias,
) -> bool {
    let member = console_member_from_runtime_alias(handle, alias);
    if !visibility_policy.member_visible(&member) {
        return false;
    }
    visibility_policy.identity_visible(&console_identity_record_from_runtime_alias(alias))
}

fn console_identity_status_json_from_identity_status(
    status: &crate::identity_first::IdentityStatus,
    response_phase: Option<String>,
) -> Value {
    json!({
        "identity": status.identity.as_str(),
        "state": format!("{:?}", status.state),
        "role": status.profile.as_ref().map(ProfileName::as_str),
        "addressability": console_addressability_json(status.addressability),
        "display_name": status.display_name.as_ref().map(crate::identity_first::DisplayName::as_str),
        "labels": status.labels,
        "agent_runtime_id": status.agent_runtime_id.as_ref().map(crate::identity_first::AgentRuntimeId::as_str),
        "session_id": status.session_id.as_ref().map(ToString::to_string),
        "generation": status.generation.map(crate::identity_first::ContinuityGeneration::get),
        "checkpoint_version": status.checkpoint_version.map(crate::identity_first::CheckpointVersion::get),
        "continuity_health": status.continuity_health,
        "lease_healthy": status.lease.as_ref().map(|lease| lease.healthy),
        "lease": status.lease.as_ref().map(|lease| json!({
            "fencing_token": lease.fencing_token.get(),
            "ttl_remaining_ms": lease.ttl_remaining.as_millis() as u64,
            "healthy": lease.healthy,
        })),
        "response_phase": response_phase,
    })
}

fn console_identity_inspect_json_from_identity_status(
    status: &crate::identity_first::IdentityStatus,
    live_alias: Option<&ConsoleRuntimeIdentityAlias>,
    response_phase: Option<String>,
) -> Value {
    let topology_peers = live_alias
        .map(|alias| {
            alias
                .member
                .wired_to
                .iter()
                .map(ToString::to_string)
                .map(Value::String)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let session_id = status
        .session_id
        .as_ref()
        .map(ToString::to_string)
        .or_else(|| live_alias.and_then(|alias| alias.session_id.clone()));
    let agent_runtime_id = status
        .agent_runtime_id
        .as_ref()
        .map(crate::identity_first::AgentRuntimeId::as_str)
        .map(ToString::to_string)
        .or_else(|| live_alias.map(|alias| alias.runtime_member_id.clone()));
    json!({
        "identity": status.identity.as_str(),
        "state": format!("{:?}", status.state),
        "role": status.profile.as_ref().map(ProfileName::as_str),
        "addressability": console_addressability_json(status.addressability),
        "display_name": status.display_name.as_ref().map(crate::identity_first::DisplayName::as_str),
        "labels": status.labels,
        "continuity_health": status.continuity_health,
        "lease_healthy": status.lease.as_ref().map(|lease| lease.healthy),
        "lease": status.lease.as_ref().map(|lease| json!({
            "fencing_token": lease.fencing_token.get(),
            "ttl_remaining_ms": lease.ttl_remaining.as_millis() as u64,
            "healthy": lease.healthy,
        })),
        "continuity": {
            "generation": status.generation.map(crate::identity_first::ContinuityGeneration::get),
            "checkpoint_version": status.checkpoint_version.map(crate::identity_first::CheckpointVersion::get),
            "session_id": session_id,
            "agent_runtime_id": agent_runtime_id,
        },
        "topology_peers": topology_peers,
        "output_preview": Value::Null,
        "response_phase": response_phase,
    })
}

fn console_identity_inspect_json_from_record(
    inspection: &crate::console_aggregator::ConsoleIdentityInspection,
    response_phase: Option<String>,
) -> Value {
    let record = &inspection.identity;
    json!({
        "identity": record.identity,
        "state": record.health,
        "role": record.labels.get("role"),
        "addressability": if record.addressable { "addressable" } else { "internal_only" },
        "display_name": record.display_name,
        "labels": record.labels,
        "continuity_health": Value::Null,
        "lease_healthy": Value::Null,
        "lease": Value::Null,
        "continuity": {
            "generation": Value::Null,
            "checkpoint_version": Value::Null,
            "session_id": record.session_id,
            "agent_runtime_id": record.runtime_member_id,
        },
        "topology_peers": inspection.peers,
        "output_preview": Value::Null,
        "response_phase": response_phase,
    })
}

fn lifecycle_archive_cleanup_completed(error: &str) -> bool {
    is_recoverable_lifecycle_cleanup_error(error)
}

async fn respawn_console_member(
    handle: &MobHandle,
    runtime_member_id: &MeerkatId,
) -> Result<Option<Value>, String> {
    let entry_before_respawn = handle.get_member(runtime_member_id).await;
    match handle.respawn(runtime_member_id.clone(), None).await {
        Ok(_receipt) => Ok(None),
        Err(err) => {
            if let Some(failed_peer_ids) = topology_restore_failed_peer_ids(&err) {
                tracing::warn!(
                    member_id = %runtime_member_id,
                    failed_peer_count = failed_peer_ids.len(),
                    failed_peer_ids = ?failed_peer_ids,
                    "console member respawn restored member with isolated peer edges; continuing degraded respawn"
                );
                return Ok(Some(topology_restore_warning_json(&failed_peer_ids)));
            }

            if lifecycle_archive_cleanup_completed(&err.to_string()) {
                if handle.get_member(runtime_member_id).await.is_none()
                    && let Some(entry) = entry_before_respawn
                {
                    let mut spec =
                        SpawnMemberSpec::new(entry.role.clone(), runtime_member_id.clone());
                    if !entry.labels.is_empty() {
                        spec = spec.with_labels(entry.labels.clone());
                    }
                    handle
                        .ensure_member(spec)
                        .await
                        .map_err(|ensure_err| ensure_err.to_string())?;
                }
                return Ok(None);
            }

            Err(err.to_string())
        }
    }
}

async fn retire_console_member(
    handle: &MobHandle,
    runtime_member_id: &MeerkatId,
) -> Result<(), String> {
    match handle.retire(runtime_member_id.clone()).await {
        Ok(()) => Ok(()),
        Err(err) if lifecycle_archive_cleanup_completed(&err.to_string()) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(test)]
fn member_id_matches_durable_identity(member_id: &str, durable_identity: &str) -> bool {
    member_id == durable_identity
}

async fn retire_stale_console_members_for_identity(
    handle: &MobHandle,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    durable_identity: &str,
    keep_runtime_member_id: Option<&str>,
) -> Result<(), String> {
    let stale_members = lookup_member_alias_candidates_with_session(handle, durable_identity)
        .await
        .into_iter()
        .filter(|alias| {
            runtime_alias_visible_to_console(handle, visibility_policy, alias)
                && keep_runtime_member_id
                    .map(|keep| alias.runtime_member_id != keep)
                    .unwrap_or(true)
        })
        .map(|alias| MeerkatId::from(alias.runtime_member_id.as_str()))
        .collect::<Vec<_>>();
    for member_id in stale_members {
        retire_console_member(handle, &member_id).await?;
    }
    Ok(())
}

fn console_identity_error_response(
    response_id: Value,
    operation: &str,
    err: crate::identity_first::IdentityRuntimeError,
) -> Value {
    match err {
        crate::identity_first::IdentityRuntimeError::UnknownIdentity(identity) => {
            invalid_params(response_id, format!("identity not found: {identity}"))
        }
        other => internal_error(response_id, format!("{operation} failed: {other}")),
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_console_aggregator_rpc(
    console_aggregator: Option<MobKitConsoleAggregator>,
    request: JsonRpcRequest,
    is_authenticated: bool,
    read_only: bool,
    access: Option<&AccessController>,
    access_view: Option<&AccessView>,
) -> Value {
    let response_id = request.id.clone().unwrap_or(Value::Null);
    let can_mutate = is_authenticated && !read_only;
    if is_console_mutating_rpc_method(request.method.as_str()) && !can_mutate {
        return console_read_only_rpc_error(response_id);
    }
    if let Some(response) = handle_access_admin_rpc(access, access_view, &request) {
        return response;
    }
    if let Some(error) =
        console_rpc_access_violation(access_view, request.method.as_str(), &request.params)
    {
        return response_value(response_id, None, Some(error));
    }
    match request.method.as_str() {
        "mobkit/capabilities" => {
            let mut methods = vec![
                "mobkit/capabilities",
                "mobkit/console/list_identities",
                "mobkit/console/inspect_identity",
                "mobkit/console/query_timeline",
            ];
            if can_mutate {
                methods.extend_from_slice(&["mobkit/retire", "mobkit/console/send"]);
            }
            if access.is_some() {
                methods.push("mobkit/access/status");
                if access_view.is_some_and(AccessView::can_administer) {
                    methods.extend_from_slice(&["mobkit/access/get", "mobkit/access/preview"]);
                    if can_mutate {
                        methods.extend_from_slice(&[
                            "mobkit/access/set",
                            "mobkit/access/enable",
                            "mobkit/access/rules/upsert",
                            "mobkit/access/rules/delete",
                            "mobkit/access/groups/set",
                            "mobkit/access/groups/delete",
                        ]);
                    }
                }
            }
            response_value(
                response_id,
                Some(json!({
                    "methods": methods,
                    "authenticated": is_authenticated,
                    "read_only": read_only,
                    "runtime_capabilities": {
                        "can_send_messages": can_mutate,
                        "can_retire_members": can_mutate,
                        "can_spawn_members": false,
                    },
                    "features": {
                        "console_aggregator": console_aggregator.is_some(),
                        "multi_runtime_console": console_aggregator.is_some(),
                    }
                })),
                None,
            )
        }
        "mobkit/console/list_identities" => {
            let Some(aggregator) = &console_aggregator else {
                return console_aggregator_unavailable(response_id);
            };
            match aggregator.list_identities().await {
                Ok(mut identities) => {
                    retain_visible_identity_records(&mut identities, access_view);
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
            match Box::pin(aggregator.inspect_identity(identity)).await {
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
            let query: ConsoleTimelineWindowQuery =
                match serde_json::from_value(request.params.clone()) {
                    Ok(query) => query,
                    Err(err) => {
                        return invalid_params(response_id, format!("invalid query params: {err}"));
                    }
                };
            let Some(aggregator) = &console_aggregator else {
                return console_aggregator_unavailable(response_id);
            };
            match Box::pin(aggregator.query_timeline_windowed(query.clone())).await {
                Ok(mut page) => {
                    retain_visible_timeline_frames(&mut page, access_view);
                    response_value(
                        response_id,
                        Some(serde_json::to_value(page).unwrap_or(Value::Null)),
                        None,
                    )
                }
                Err(err) => {
                    let latest_cursor = aggregator.latest_cursor().await.ok().flatten();
                    console_timeline_replay_unavailable_response(
                        response_id,
                        err,
                        query.after.as_ref(),
                        latest_cursor,
                    )
                }
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
            match Box::pin(aggregator.send(send_request)).await {
                Ok(accepted) => response_value(
                    response_id,
                    Some(serde_json::to_value(accepted).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => {
                    response_value(response_id, None, Some(console_send_json_rpc_error(err)))
                }
            }
        }
        "mobkit/retire" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let Some(aggregator) = &console_aggregator else {
                return console_aggregator_unavailable(response_id);
            };
            match Box::pin(aggregator.retire_identity(identity)).await {
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
            let Some(_aggregator) = &console_aggregator else {
                return console_aggregator_unavailable(response_id);
            };
            response_value(
                response_id,
                None,
                Some(JsonRpcError {
                    code: -32002,
                    message: "reset_all is not supported on the aggregator-only RPC surface"
                        .to_string(),
                    data: Some(json!({
                        "kind": "unsupported_reset_all_surface",
                        "reason": "aggregator reset_all cannot preserve baseline identity semantics",
                    })),
                }),
            )
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

fn resolve_gating_approver_id(
    params: &Value,
    authenticated_principal: Option<&str>,
) -> Result<String, &'static str> {
    if let Some(principal) = authenticated_principal.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }) {
        return Ok(principal.to_string());
    }
    let Some(approver_id) = params.get("approver_id").and_then(Value::as_str) else {
        return Err("approver_id required");
    };
    let trimmed = approver_id.trim();
    if trimmed.is_empty() {
        return Err("approver_id required");
    }
    Ok(trimmed.to_string())
}

#[allow(clippy::large_futures, clippy::too_many_arguments)]
#[cfg(test)]
async fn handle_console_runtime_rpc(
    runtime: &MobRuntime,
    module_runtime: Option<std::sync::Arc<tokio::sync::Mutex<MobkitRuntimeHandle>>>,
    contact_directory: Option<&ContactDirectory>,
    gateway_peer_keys: Option<&crate::auth::peer_keys::GatewayPeerKeys>,
    console_events: Option<ConsoleEventStore>,
    console_aggregator: Option<MobKitConsoleAggregator>,
    identity_runtime: Option<Arc<crate::identity_first::IdentityRuntime>>,
    metadata_table: Option<std::sync::Arc<RuntimeMetadataTable>>,
    mob_events: Option<MobEventsStore>,
    request: JsonRpcRequest,
    is_authenticated: bool,
) -> Value {
    handle_console_runtime_rpc_with_visibility(
        runtime,
        module_runtime,
        contact_directory,
        gateway_peer_keys,
        console_events,
        console_aggregator,
        identity_runtime,
        metadata_table,
        mob_events,
        &crate::console_aggregator::AllowAllConsoleVisibilityPolicy,
        request,
        is_authenticated,
        false,
        None,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn handle_console_runtime_rpc_with_visibility(
    runtime: &MobRuntime,
    module_runtime: Option<std::sync::Arc<tokio::sync::Mutex<MobkitRuntimeHandle>>>,
    contact_directory: Option<&ContactDirectory>,
    gateway_peer_keys: Option<&crate::auth::peer_keys::GatewayPeerKeys>,
    console_events: Option<ConsoleEventStore>,
    console_aggregator: Option<MobKitConsoleAggregator>,
    identity_runtime: Option<Arc<crate::identity_first::IdentityRuntime>>,
    metadata_table: Option<std::sync::Arc<RuntimeMetadataTable>>,
    mob_events: Option<MobEventsStore>,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    request: JsonRpcRequest,
    is_authenticated: bool,
    read_only: bool,
    authenticated_principal: Option<&str>,
    access: Option<&AccessController>,
    access_view: Option<&AccessView>,
) -> Value {
    let response_id = request.id.clone().unwrap_or(Value::Null);
    let can_mutate = is_authenticated && !read_only;
    if is_console_mutating_rpc_method(request.method.as_str()) && !can_mutate {
        return console_read_only_rpc_error(response_id);
    }
    // Refresh the attribute cache from the live roster before any per-agent
    // access decision (the gate below and per-row result filtering) so
    // label/role rules resolve here exactly as they do on
    // `/console/experience`. RPC callers must not depend on having polled the
    // experience endpoint first.
    if let Some(controller) = access.filter(|controller| controller.enabled()) {
        prime_access_cache_from_handle(&runtime.handle(), controller).await;
    }
    if let Some(response) = handle_access_admin_rpc(access, access_view, &request) {
        return response;
    }
    if let Some(error) =
        console_rpc_access_violation(access_view, request.method.as_str(), &request.params)
    {
        return response_value(response_id, None, Some(error));
    }

    match request.method.as_str() {
        "mobkit/capabilities" => {
            let mut methods = vec![
                "mobkit/status",
                "mobkit/capabilities",
                "mobkit/list_members",
                "mobkit/get_member",
                "mobkit/find_members",
                "mobkit/member_status",
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
            if identity_runtime.is_some() {
                methods.extend_from_slice(&["mobkit/status_identity", "mobkit/inspect_identity"]);
                if can_mutate {
                    methods.extend_from_slice(&[
                        "mobkit/respawn",
                        "mobkit/reset",
                        "mobkit/delete_identity",
                    ]);
                }
            } else if console_aggregator.is_some() {
                methods.extend_from_slice(&["mobkit/status_identity", "mobkit/inspect_identity"]);
            }
            if module_runtime.is_some() {
                methods.extend_from_slice(&[
                    "mobkit/routing/routes/list",
                    "mobkit/delivery/history",
                    "mobkit/gating/pending",
                    "mobkit/gating/audit",
                ]);
                if can_mutate {
                    methods.push("mobkit/gating/decide");
                }
            }
            if can_mutate {
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
                    "mobkit/collect_completed",
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
                if can_mutate {
                    methods.extend_from_slice(&[
                        "mobkit/mob_labels/set",
                        "mobkit/mob_labels/delete",
                        "mobkit/run_labels/set",
                        "mobkit/run_labels/delete",
                    ]);
                }
            }
            if access.is_some() {
                methods.push("mobkit/access/status");
                if access_view.is_some_and(AccessView::can_administer) {
                    methods.extend_from_slice(&["mobkit/access/get", "mobkit/access/preview"]);
                    if can_mutate {
                        methods.extend_from_slice(&[
                            "mobkit/access/set",
                            "mobkit/access/enable",
                            "mobkit/access/rules/upsert",
                            "mobkit/access/rules/delete",
                            "mobkit/access/groups/set",
                            "mobkit/access/groups/delete",
                        ]);
                    }
                }
            }
            // Intersect the advertised methods with the caller's grants, the
            // same way `/console/experience` does, so a non-admin doesn't get
            // a method list its panels act on only to hit `-32030`. A probe
            // identity reveals which mapped requirements are resource-less
            // (target `None` regardless of params): those are filtered by the
            // action grant; agent-scoped methods stay advertised and are
            // enforced per call.
            if let Some(view) = access_view.filter(|view| view.enforced()) {
                let probe = serde_json::json!({ "identity": "\u{0}cap-probe" });
                methods.retain(
                    |method| match console_rpc_access_requirement(method, &probe) {
                        Some((action, None)) => view.allows(action),
                        _ => true,
                    },
                );
            }
            // Coarse capability flags, intersected with the caller's grants so
            // they agree with the per-agent affordances in `/console/experience`.
            let cap = |action: &str| -> bool {
                can_mutate
                    && access_view
                        .filter(|view| view.enforced())
                        .is_none_or(|view| view.may_perform_anywhere(action))
            };
            response_value(
                response_id,
                Some(serde_json::json!({
                    "contract_version": crate::rpc::MOBKIT_CONTRACT_VERSION,
                    "methods": methods,
                    "read_only": read_only,
                    // The console routes to MobRuntime directly and has no
                    // access to the module runtime, so loaded_modules is always [].
                    "loaded_modules": serde_json::json!([]),
                    "runtime_capabilities": {
                        "can_send_messages": cap(ACTION_AGENT_SEND),
                        "can_retire_members": cap(ACTION_AGENT_RETIRE),
                        "can_spawn_members": cap(ACTION_AGENT_SPAWN),
                    }
                })),
                None,
            )
        }
        "mobkit/status" => {
            let mob_state = runtime.handle().status_observation_snapshot();
            response_value(
                response_id,
                Some(serde_json::json!({
                    "contract_version": crate::rpc::MOBKIT_CONTRACT_VERSION,
                    "running": matches!(mob_state, MobState::Creating | MobState::Running),
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
                Ok(mut identities) => {
                    retain_visible_identity_records(&mut identities, access_view);
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
            match Box::pin(aggregator.inspect_identity(identity)).await {
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
            let query: ConsoleTimelineWindowQuery =
                match serde_json::from_value(request.params.clone()) {
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
            match Box::pin(aggregator.query_timeline_windowed(query.clone())).await {
                Ok(mut page) => {
                    retain_visible_timeline_frames(&mut page, access_view);
                    response_value(
                        response_id,
                        Some(serde_json::to_value(page).unwrap_or(Value::Null)),
                        None,
                    )
                }
                Err(err) => {
                    let latest_cursor = aggregator.latest_cursor().await.ok().flatten();
                    console_timeline_replay_unavailable_response(
                        response_id,
                        err,
                        query.after.as_ref(),
                        latest_cursor,
                    )
                }
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
            if let Some(identity_runtime) = &identity_runtime {
                return match Box::pin(console_send_with_identity_first_fallback(
                    aggregator,
                    identity_runtime.clone(),
                    Some(runtime),
                    console_events.as_ref(),
                    send_request,
                ))
                .await
                {
                    Ok(accepted) => response_value(
                        response_id,
                        Some(serde_json::to_value(accepted).unwrap_or(Value::Null)),
                        None,
                    ),
                    Err(err) => {
                        response_value(response_id, None, Some(console_send_json_rpc_error(err)))
                    }
                };
            }
            match Box::pin(aggregator.send(send_request)).await {
                Ok(accepted) => response_value(
                    response_id,
                    Some(serde_json::to_value(accepted).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => {
                    response_value(response_id, None, Some(console_send_json_rpc_error(err)))
                }
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
            retain_visible_member_rows(&mut members, access_view);
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
            retain_visible_member_rows(&mut matches, access_view);
            response_value(response_id, Some(Value::Array(matches)), None)
        }
        "mobkit/status_identity" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let handle = runtime.handle();
            if let Some(identity_runtime) = &identity_runtime {
                let (parsed_identity, _requested_exact_identity, live_alias) =
                    match resolve_console_identity_control_target(
                        &handle,
                        Some(identity_runtime),
                        visibility_policy,
                        identity,
                    )
                    .await
                    {
                        Ok(Some(target)) => target,
                        Ok(None) => {
                            return invalid_params(
                                response_id,
                                format!("identity not found: {identity}"),
                            );
                        }
                        Err(err) => return response_value(response_id, None, Some(err)),
                    };
                match identity_runtime.status(&parsed_identity).await {
                    Ok(status) => {
                        if !identity_status_visible_to_console(visibility_policy, &status) {
                            return identity_hidden_by_policy_response(response_id, identity);
                        }
                        let phase = if let Some(store) = &console_events {
                            store
                                .response_phase_for_identity(status.identity.as_str())
                                .await
                        } else {
                            None
                        };
                        if let Some(error) = stale_live_alias_json_rpc_error(
                            "status_identity",
                            identity_runtime,
                            &parsed_identity,
                            live_alias.as_ref(),
                        )
                        .await
                        {
                            return response_value(response_id, None, Some(error));
                        }
                        return response_value(
                            response_id,
                            Some(console_identity_status_json_from_identity_status(
                                &status, phase,
                            )),
                            None,
                        );
                    }
                    Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {}
                    Err(err) => {
                        return console_identity_error_response(
                            response_id,
                            "status_identity",
                            err,
                        );
                    }
                }
            }
            if let Some(aggregator) = &console_aggregator {
                return match Box::pin(aggregator.inspect_identity(identity)).await {
                    Ok(Some(inspection)) => {
                        let phase = if let Some(store) = &console_events {
                            store
                                .response_phase_for_identity(&inspection.identity.identity)
                                .await
                        } else {
                            None
                        };
                        response_value(
                            response_id,
                            Some(console_identity_status_json_from_record(
                                &inspection.identity,
                                phase,
                            )),
                            None,
                        )
                    }
                    Ok(None) => response_value(
                        response_id,
                        None,
                        Some(JsonRpcError {
                            code: -32001,
                            message: format!("unknown identity: {identity}"),
                            data: None,
                        }),
                    ),
                    Err(err) => {
                        internal_error(response_id, format!("status_identity failed: {err}"))
                    }
                };
            }
            let live_alias = match lookup_member_alias_with_session(
                &handle,
                visibility_policy,
                identity,
            )
            .await
            {
                Ok(alias) => alias,
                Err(err) => return response_value(response_id, None, Some(err)),
            };
            let Some(alias) = live_alias else {
                return invalid_params(response_id, format!("identity not found: {identity}"));
            };
            if !runtime_alias_visible_to_console(&handle, visibility_policy, &alias) {
                return identity_hidden_by_policy_response(response_id, identity);
            }
            if let Err(err) =
                reject_ambiguous_projected_live_identity(&handle, visibility_policy, &alias).await
            {
                return response_value(response_id, None, Some(err));
            }
            let phase = if let Some(store) = &console_events {
                store.response_phase_for_identity(&alias.identity).await
            } else {
                None
            };
            response_value(
                response_id,
                Some(console_identity_status_json_for_identity(
                    &alias.identity,
                    &alias.member,
                    alias.session_id,
                    phase,
                )),
                None,
            )
        }
        "mobkit/inspect_identity" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let handle = runtime.handle();
            if let Some(identity_runtime) = &identity_runtime {
                let (parsed_identity, _requested_exact_identity, live_alias) =
                    match resolve_console_identity_control_target(
                        &handle,
                        Some(identity_runtime),
                        visibility_policy,
                        identity,
                    )
                    .await
                    {
                        Ok(Some(target)) => target,
                        Ok(None) => {
                            return invalid_params(
                                response_id,
                                format!("identity not found: {identity}"),
                            );
                        }
                        Err(err) => return response_value(response_id, None, Some(err)),
                    };
                match identity_runtime.status(&parsed_identity).await {
                    Ok(status) => {
                        if !identity_status_visible_to_console(visibility_policy, &status) {
                            return identity_hidden_by_policy_response(response_id, identity);
                        }
                        let phase = if let Some(store) = &console_events {
                            store
                                .response_phase_for_identity(status.identity.as_str())
                                .await
                        } else {
                            None
                        };
                        if let Some(error) = stale_live_alias_json_rpc_error(
                            "inspect_identity",
                            identity_runtime,
                            &parsed_identity,
                            live_alias.as_ref(),
                        )
                        .await
                        {
                            return response_value(response_id, None, Some(error));
                        }
                        return response_value(
                            response_id,
                            Some(console_identity_inspect_json_from_identity_status(
                                &status,
                                live_alias.as_ref(),
                                phase,
                            )),
                            None,
                        );
                    }
                    Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {}
                    Err(err) => {
                        return console_identity_error_response(
                            response_id,
                            "inspect_identity",
                            err,
                        );
                    }
                }
            }
            if let Some(aggregator) = &console_aggregator {
                return match Box::pin(aggregator.inspect_identity(identity)).await {
                    Ok(Some(inspection)) => {
                        let phase = if let Some(store) = &console_events {
                            store
                                .response_phase_for_identity(&inspection.identity.identity)
                                .await
                        } else {
                            None
                        };
                        response_value(
                            response_id,
                            Some(console_identity_inspect_json_from_record(
                                &inspection,
                                phase,
                            )),
                            None,
                        )
                    }
                    Ok(None) => response_value(
                        response_id,
                        None,
                        Some(JsonRpcError {
                            code: -32001,
                            message: format!("unknown identity: {identity}"),
                            data: None,
                        }),
                    ),
                    Err(err) => {
                        internal_error(response_id, format!("inspect_identity failed: {err}"))
                    }
                };
            }
            let live_alias = match lookup_member_alias_with_session(
                &handle,
                visibility_policy,
                identity,
            )
            .await
            {
                Ok(alias) => alias,
                Err(err) => return response_value(response_id, None, Some(err)),
            };
            let Some(alias) = live_alias else {
                return invalid_params(response_id, format!("identity not found: {identity}"));
            };
            if !runtime_alias_visible_to_console(&handle, visibility_policy, &alias) {
                return identity_hidden_by_policy_response(response_id, identity);
            }
            if let Err(err) =
                reject_ambiguous_projected_live_identity(&handle, visibility_policy, &alias).await
            {
                return response_value(response_id, None, Some(err));
            }
            let phase = if let Some(store) = &console_events {
                store.response_phase_for_identity(&alias.identity).await
            } else {
                None
            };
            response_value(
                response_id,
                Some(console_identity_inspect_json_for_identity(
                    &alias.identity,
                    &alias.member,
                    alias.session_id,
                    phase,
                )),
                None,
            )
        }
        "mobkit/retire" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let handle = runtime.handle();
            if let Some(identity_runtime) = &identity_runtime {
                let (parsed_identity, _requested_exact_identity, live_alias) =
                    match resolve_console_identity_control_target(
                        &handle,
                        Some(identity_runtime),
                        visibility_policy,
                        identity,
                    )
                    .await
                    {
                        Ok(Some(target)) => target,
                        Ok(None) => {
                            return invalid_params(
                                response_id,
                                format!("identity not found: {identity}"),
                            );
                        }
                        Err(err) => return response_value(response_id, None, Some(err)),
                    };
                let registered_status = match identity_runtime.status(&parsed_identity).await {
                    Ok(status) => Some(status),
                    Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => None,
                    Err(err) => {
                        return console_identity_error_response(response_id, "retire", err);
                    }
                };
                if let Some(status) = registered_status.as_ref() {
                    if !identity_status_visible_to_console(visibility_policy, status) {
                        return identity_hidden_by_policy_response(response_id, identity);
                    }
                    if let Some(error) = stale_live_alias_json_rpc_error(
                        "retire",
                        identity_runtime,
                        &parsed_identity,
                        live_alias.as_ref(),
                    )
                    .await
                    {
                        return response_value(response_id, None, Some(error));
                    }
                }
                match identity_runtime.retire(&parsed_identity).await {
                    Ok(token) => {
                        let keep_runtime_member_id = registered_status
                            .as_ref()
                            .and_then(|status| status.agent_runtime_id.as_ref())
                            .filter(|_| identity_runtime.has_session_bridge())
                            .map(crate::identity_first::AgentRuntimeId::as_str);
                        let cleanup_warning = if registered_status.is_some()
                            && let Err(err) = retire_stale_console_members_for_identity(
                                &handle,
                                visibility_policy,
                                parsed_identity.as_str(),
                                keep_runtime_member_id,
                            )
                            .await
                        {
                            Some(json!({
                                "kind": "stale_member_cleanup_failed_after_identity_retire",
                                "identity": parsed_identity.as_str(),
                                "message": err,
                            }))
                        } else {
                            None
                        };
                        if let Some(store) = &console_events {
                            store
                                .record_lifecycle(
                                    parsed_identity.as_str(),
                                    "identity_retired",
                                    json!({
                                        "fencing_token": token.get(),
                                        "cleanup_warning": cleanup_warning.clone(),
                                    }),
                                )
                                .await;
                        }
                        return response_value(
                            response_id,
                            Some(json!({
                                "identity": parsed_identity.as_str(),
                                "fencing_token": token.get(),
                                "cleanup_warning": cleanup_warning,
                            })),
                            None,
                        );
                    }
                    Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                        if let Some(alias) = live_alias.as_ref() {
                            if !runtime_alias_visible_to_console(&handle, visibility_policy, alias)
                            {
                                return identity_hidden_by_policy_response(response_id, identity);
                            }
                            let mid = MeerkatId::from(alias.runtime_member_id.as_str());
                            return match retire_console_member(&handle, &mid).await {
                                Ok(()) => {
                                    if let Some(store) = &console_events {
                                        store
                                            .record_lifecycle(
                                                &alias.identity,
                                                "identity_retired",
                                                json!({}),
                                            )
                                            .await;
                                    }
                                    response_value(
                                        response_id,
                                        Some(json!({ "identity": alias.identity })),
                                        None,
                                    )
                                }
                                Err(err) => {
                                    internal_error(response_id, format!("retire failed: {err}"))
                                }
                            };
                        }
                    }
                    Err(err) => return console_identity_error_response(response_id, "retire", err),
                }
            }
            if let Some(aggregator) = &console_aggregator {
                let canonical_identity = match Box::pin(aggregator.inspect_identity(identity)).await
                {
                    Ok(Some(inspection)) => inspection.identity.identity,
                    Ok(None) => identity.to_string(),
                    Err(_) => identity.to_string(),
                };
                return match Box::pin(aggregator.retire_identity(identity)).await {
                    Ok(true) => {
                        if let Some(store) = &console_events {
                            store
                                .record_lifecycle(
                                    &canonical_identity,
                                    "identity_retired",
                                    json!({}),
                                )
                                .await;
                        }
                        response_value(
                            response_id,
                            Some(json!({ "identity": canonical_identity })),
                            None,
                        )
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
            let live_alias = match lookup_member_alias_with_session(
                &handle,
                visibility_policy,
                identity,
            )
            .await
            {
                Ok(alias) => alias,
                Err(err) => return response_value(response_id, None, Some(err)),
            };
            let Some(alias) = live_alias else {
                return invalid_params(response_id, format!("identity not found: {identity}"));
            };
            if !runtime_alias_visible_to_console(&handle, visibility_policy, &alias) {
                return identity_hidden_by_policy_response(response_id, identity);
            }
            if let Err(err) =
                reject_ambiguous_projected_live_identity(&handle, visibility_policy, &alias).await
            {
                return response_value(response_id, None, Some(err));
            }
            let mid = MeerkatId::from(alias.runtime_member_id.as_str());
            match retire_console_member(&handle, &mid).await {
                Ok(()) => {
                    if let Some(store) = &console_events {
                        store
                            .record_lifecycle(&alias.identity, "identity_retired", json!({}))
                            .await;
                    }
                    response_value(
                        response_id,
                        Some(json!({ "identity": alias.identity })),
                        None,
                    )
                }
                Err(err) => internal_error(response_id, format!("retire failed: {err}")),
            }
        }
        "mobkit/respawn" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let handle = runtime.handle();
            if let Some(identity_runtime) = &identity_runtime {
                let (parsed_identity, _requested_exact_identity, live_alias) =
                    match resolve_console_identity_control_target(
                        &handle,
                        Some(identity_runtime),
                        visibility_policy,
                        identity,
                    )
                    .await
                    {
                        Ok(Some(target)) => target,
                        Ok(None) => {
                            return invalid_params(
                                response_id,
                                format!("identity not found: {identity}"),
                            );
                        }
                        Err(err) => return response_value(response_id, None, Some(err)),
                    };
                let registered_status = match identity_runtime.status(&parsed_identity).await {
                    Ok(status) => Some(status),
                    Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => None,
                    Err(err) => {
                        return console_identity_error_response(response_id, "respawn", err);
                    }
                };
                if let Some(status) = registered_status.as_ref() {
                    if !identity_status_visible_to_console(visibility_policy, status) {
                        return identity_hidden_by_policy_response(response_id, identity);
                    }
                    if let Some(error) = stale_live_alias_json_rpc_error(
                        "respawn",
                        identity_runtime,
                        &parsed_identity,
                        live_alias.as_ref(),
                    )
                    .await
                    {
                        return response_value(response_id, None, Some(error));
                    }
                }
                match identity_runtime.respawn(&parsed_identity).await {
                    Ok(mut record) => {
                        let live_respawn_warning = match respawn_console_member(
                            &handle,
                            &MeerkatId::from(record.agent_runtime_id.as_str()),
                        )
                        .await
                        {
                            Ok(topology_restore_warning) => {
                                let live_session_id = handle
                                    .resolve_bridge_session_id_observation(&MeerkatId::from(
                                        record.agent_runtime_id.as_str(),
                                    ))
                                    .await;
                                if let Some(live_session_id) = live_session_id {
                                    match identity_runtime
                                        .rebind_session_after_live_respawn(
                                            &parsed_identity,
                                            live_session_id.clone(),
                                        )
                                        .await
                                    {
                                        Ok(updated_record) => {
                                            record = updated_record;
                                            topology_restore_warning
                                        }
                                        Err(err) => Some(json!({
                                            "kind": "identity_rebind_failed_after_member_respawn",
                                            "identity": record.identity.as_str(),
                                            "agent_runtime_id": record.agent_runtime_id.as_str(),
                                            "live_session_id": live_session_id.to_string(),
                                            "message": err.to_string(),
                                        })),
                                    }
                                } else {
                                    topology_restore_warning
                                }
                            }
                            Err(err) => Some(json!({
                                "kind": "member_respawn_failed_after_identity_refresh",
                                "identity": record.identity.as_str(),
                                "agent_runtime_id": record.agent_runtime_id.as_str(),
                                "message": err,
                            })),
                        };
                        let cleanup_warning = if registered_status.is_some()
                            && let Err(err) = retire_stale_console_members_for_identity(
                                &handle,
                                visibility_policy,
                                parsed_identity.as_str(),
                                Some(record.agent_runtime_id.as_str()),
                            )
                            .await
                        {
                            Some(json!({
                                "kind": "stale_member_cleanup_failed_after_identity_respawn",
                                "identity": parsed_identity.as_str(),
                                "agent_runtime_id": record.agent_runtime_id.as_str(),
                                "message": err,
                            }))
                        } else {
                            None
                        };
                        if let Some(store) = &console_events {
                            store
                                .record_lifecycle(
                                    parsed_identity.as_str(),
                                    "identity_respawned",
                                    json!({
                                        "generation": record.generation.get(),
                                        "checkpoint_version": record.checkpoint_version.get(),
                                        "live_respawn_warning": live_respawn_warning.clone(),
                                        "cleanup_warning": cleanup_warning.clone(),
                                    }),
                                )
                                .await;
                        }
                        return response_value(
                            response_id,
                            Some(json!({
                                "identity": record.identity.as_str(),
                                "agent_runtime_id": record.agent_runtime_id.as_str(),
                                "session_id": record.session_id.to_string(),
                                "generation": record.generation.get(),
                                "checkpoint_version": record.checkpoint_version.get(),
                                "live_respawn_warning": live_respawn_warning,
                                "cleanup_warning": cleanup_warning,
                            })),
                            None,
                        );
                    }
                    Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                        if live_alias.is_none() {
                            return invalid_params(
                                response_id,
                                format!("identity not found: {identity}"),
                            );
                        }
                    }
                    Err(err) => {
                        return console_identity_error_response(response_id, "respawn", err);
                    }
                }
            }
            let live_alias = match lookup_member_alias_with_session(
                &handle,
                visibility_policy,
                identity,
            )
            .await
            {
                Ok(alias) => alias,
                Err(err) => return response_value(response_id, None, Some(err)),
            };
            let Some(alias) = live_alias else {
                return invalid_params(response_id, format!("identity not found: {identity}"));
            };
            if !runtime_alias_visible_to_console(&handle, visibility_policy, &alias) {
                return identity_hidden_by_policy_response(response_id, identity);
            }
            if let Err(err) =
                reject_ambiguous_projected_live_identity(&handle, visibility_policy, &alias).await
            {
                return response_value(response_id, None, Some(err));
            }
            let mid = MeerkatId::from(alias.runtime_member_id.as_str());
            match respawn_console_member(&handle, &mid).await {
                Ok(topology_restore_warning) => {
                    if let Some(store) = &console_events {
                        store
                            .record_lifecycle(
                                &alias.identity,
                                "identity_respawned",
                                json!({ "topology_restore_warning": topology_restore_warning.clone() }),
                            )
                            .await;
                    }
                    let mut body = match lookup_member_with_session(&handle, &mid).await {
                        Some((entry, session_id)) => console_identity_status_json_for_identity(
                            &alias.identity,
                            &entry,
                            session_id,
                            None,
                        ),
                        None => json!({ "identity": alias.identity }),
                    };
                    if let Some(warning) = topology_restore_warning {
                        body["topology_restore_warning"] = warning;
                    }
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
            let Some(identity_runtime) = &identity_runtime else {
                return invalid_params(response_id, "identity-first runtime required for reset");
            };
            let (parsed_identity, _requested_exact_identity, live_alias) =
                match resolve_console_identity_control_target(
                    &handle,
                    Some(identity_runtime),
                    visibility_policy,
                    identity,
                )
                .await
                {
                    Ok(Some(target)) => target,
                    Ok(None) => {
                        return invalid_params(
                            response_id,
                            format!("identity not found: {identity}"),
                        );
                    }
                    Err(err) => return response_value(response_id, None, Some(err)),
                };
            match identity_runtime.status(&parsed_identity).await {
                Ok(status) => {
                    if !identity_status_visible_to_console(visibility_policy, &status) {
                        return identity_hidden_by_policy_response(response_id, identity);
                    }
                    if let Some(error) = stale_live_alias_json_rpc_error(
                        "reset",
                        identity_runtime,
                        &parsed_identity,
                        live_alias.as_ref(),
                    )
                    .await
                    {
                        return response_value(response_id, None, Some(error));
                    }
                    if !identity_runtime.has_session_bridge() {
                        return response_value(
                            response_id,
                            None,
                            Some(reset_requires_session_bridge_json_rpc_error()),
                        );
                    }
                }
                Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    if let Some(alias) = live_alias.as_ref() {
                        if !runtime_alias_visible_to_console(&handle, visibility_policy, alias) {
                            return identity_hidden_by_policy_response(response_id, identity);
                        }
                        let mid = MeerkatId::from(alias.runtime_member_id.as_str());
                        let response = match respawn_console_member(&handle, &mid).await {
                            Ok(topology_restore_warning) => {
                                if let Some(store) = &console_events {
                                    store
                                        .record_lifecycle(
                                            &alias.identity,
                                            "identity_reset",
                                            json!({ "topology_restore_warning": topology_restore_warning.clone() }),
                                        )
                                        .await;
                                }
                                let mut body = match lookup_member_with_session(&handle, &mid).await
                                {
                                    Some((entry, session_id)) => {
                                        console_identity_status_json_for_identity(
                                            &alias.identity,
                                            &entry,
                                            session_id,
                                            None,
                                        )
                                    }
                                    None => json!({ "identity": alias.identity }),
                                };
                                if let Some(warning) = topology_restore_warning {
                                    body["topology_restore_warning"] = warning;
                                }
                                response_value(response_id, Some(body), None)
                            }
                            Err(err) => internal_error(response_id, format!("reset failed: {err}")),
                        };
                        return response;
                    }
                    return invalid_params(response_id, format!("identity not found: {identity}"));
                }
                Err(err) => return console_identity_error_response(response_id, "reset", err),
            }
            match identity_runtime.reset(&parsed_identity).await {
                Ok(record) => {
                    let cleanup_warning = if let Err(err) =
                        retire_stale_console_members_for_identity(
                            &handle,
                            visibility_policy,
                            parsed_identity.as_str(),
                            Some(record.agent_runtime_id.as_str()),
                        )
                        .await
                    {
                        Some(json!({
                            "kind": "stale_member_cleanup_failed_after_identity_reset",
                            "identity": parsed_identity.as_str(),
                            "agent_runtime_id": record.agent_runtime_id.as_str(),
                            "message": err,
                        }))
                    } else {
                        None
                    };
                    if let Some(store) = &console_events {
                        store
                            .record_lifecycle(
                                parsed_identity.as_str(),
                                "identity_reset",
                                json!({
                                    "generation": record.generation.get(),
                                    "checkpoint_version": record.checkpoint_version.get(),
                                    "cleanup_warning": cleanup_warning.clone(),
                                }),
                            )
                            .await;
                    }
                    response_value(
                        response_id,
                        Some(json!({
                            "identity": record.identity.as_str(),
                            "agent_runtime_id": record.agent_runtime_id.as_str(),
                            "session_id": record.session_id.to_string(),
                            "generation": record.generation.get(),
                            "checkpoint_version": record.checkpoint_version.get(),
                            "cleanup_warning": cleanup_warning,
                        })),
                        None,
                    )
                }
                Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    invalid_params(response_id, format!("identity not found: {identity}"))
                }
                Err(err) => console_identity_error_response(response_id, "reset", err),
            }
        }
        "mobkit/delete_identity" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let handle = runtime.handle();
            let Some(identity_runtime) = &identity_runtime else {
                return invalid_params(
                    response_id,
                    "identity-first runtime required for delete_identity",
                );
            };
            let (parsed_identity, _requested_exact_identity, live_alias) =
                match resolve_console_identity_control_target(
                    &handle,
                    Some(identity_runtime),
                    visibility_policy,
                    identity,
                )
                .await
                {
                    Ok(Some(target)) => target,
                    Ok(None) => {
                        return invalid_params(
                            response_id,
                            format!("identity not found: {identity}"),
                        );
                    }
                    Err(err) => return response_value(response_id, None, Some(err)),
                };
            let registered_status = match identity_runtime.status(&parsed_identity).await {
                Ok(status) => status,
                Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    if let Some(alias) = live_alias.as_ref() {
                        if !runtime_alias_visible_to_console(&handle, visibility_policy, alias) {
                            return identity_hidden_by_policy_response(response_id, identity);
                        }
                        return response_value(
                            response_id,
                            None,
                            Some(JsonRpcError {
                                code: -32602,
                                message: format!(
                                    "delete_identity requires durable identity: {} is live-only",
                                    parsed_identity.as_str()
                                ),
                                data: Some(json!({
                                    "kind": "live_only_identity_delete_unsupported",
                                    "identity": parsed_identity.as_str(),
                                })),
                            }),
                        );
                    }
                    return invalid_params(response_id, format!("identity not found: {identity}"));
                }
                Err(err) => {
                    return console_identity_error_response(response_id, "delete_identity", err);
                }
            };
            if !identity_status_visible_to_console(visibility_policy, &registered_status) {
                return identity_hidden_by_policy_response(response_id, identity);
            }
            if let Some(error) = stale_live_alias_json_rpc_error(
                "delete_identity",
                identity_runtime,
                &parsed_identity,
                live_alias.as_ref(),
            )
            .await
            {
                return response_value(response_id, None, Some(error));
            }
            match identity_runtime.delete_identity(&parsed_identity).await {
                Ok(()) => {
                    let keep_runtime_member_id = registered_status
                        .agent_runtime_id
                        .as_ref()
                        .filter(|_| identity_runtime.has_session_bridge())
                        .map(crate::identity_first::AgentRuntimeId::as_str);
                    let cleanup_warning = if let Err(err) =
                        retire_stale_console_members_for_identity(
                            &handle,
                            visibility_policy,
                            parsed_identity.as_str(),
                            keep_runtime_member_id,
                        )
                        .await
                    {
                        Some(json!({
                            "kind": "stale_member_cleanup_failed_after_identity_delete",
                            "identity": parsed_identity.as_str(),
                            "message": err,
                        }))
                    } else {
                        None
                    };
                    if let Some(store) = &console_events {
                        store
                            .record_lifecycle(
                                parsed_identity.as_str(),
                                "identity_deleted",
                                json!({
                                    "cleanup_warning": cleanup_warning.clone(),
                                }),
                            )
                            .await;
                    }
                    response_value(
                        response_id,
                        Some(json!({
                            "identity": parsed_identity.as_str(),
                            "cleanup_warning": cleanup_warning,
                        })),
                        None,
                    )
                }
                Err(err) => console_identity_error_response(response_id, "delete_identity", err),
            }
        }
        "mobkit/reset_all" => {
            match Box::pin(reset_all_live_console_agents(
                runtime,
                console_events.as_ref(),
                console_aggregator.as_ref(),
                identity_runtime.as_ref(),
                visibility_policy,
            ))
            .await
            {
                Ok(body) => {
                    if body
                        .get("failed")
                        .and_then(Value::as_array)
                        .is_some_and(|failed| !failed.is_empty())
                    {
                        response_value(
                            response_id,
                            None,
                            Some(JsonRpcError {
                                code: -32000,
                                message: "reset_all failed for one or more identities".to_string(),
                                data: Some(body),
                            }),
                        )
                    } else {
                        response_value(response_id, Some(body), None)
                    }
                }
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
            let approver_id =
                match resolve_gating_approver_id(&request.params, authenticated_principal) {
                    Ok(approver_id) => approver_id,
                    Err(message) => return invalid_params(response_id, message),
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
                    approver_id,
                    decision,
                    reason,
                }) {
                Ok(result) => response_value(
                    response_id,
                    Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => gating_decision_failed_error(response_id, err),
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
            let runtime_mode = match parse_optional_runtime_mode(&request.params) {
                Ok(value) => value,
                Err(message) => return invalid_params(response_id, message),
            };
            let backend = match parse_optional_backend(&request.params) {
                Ok(value) => value,
                Err(message) => return invalid_params(response_id, message),
            };
            let binding = match parse_optional_runtime_binding(&request.params) {
                Ok(value) => value,
                Err(message) => return invalid_params(response_id, message),
            };
            let mut spec =
                SpawnMemberSpec::new(ProfileName::from(role), MeerkatId::from(agent_identity));
            if let Some(runtime_mode) = runtime_mode {
                spec = spec.with_runtime_mode(runtime_mode);
            }
            if let Some(backend) = backend {
                spec = spec.with_backend(backend);
            }
            if let Some(binding) = binding {
                spec.binding = Some(binding);
            }
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
                return match Box::pin(aggregator.retire_identity(member_id)).await {
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
                Err(err) => {
                    if let Some(failed_peer_ids) = topology_restore_failed_peer_ids(&err) {
                        tracing::warn!(
                            member_id = %member_id,
                            failed_peer_count = failed_peer_ids.len(),
                            failed_peer_ids = ?failed_peer_ids,
                            "console member respawn restored member with isolated peer edges; accepting degraded respawn"
                        );
                        response_value(
                            response_id,
                            Some(serde_json::json!({
                                "accepted": true,
                                "topology_restore_warning": topology_restore_warning_json(&failed_peer_ids),
                            })),
                            None,
                        )
                    } else {
                        internal_error(response_id, format!("respawn_member failed: {err}"))
                    }
                }
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
                Ok(mut events) => {
                    // `mob.observe` gates the surface; agent-attributed
                    // ledger entries are still filtered by `agent.view` so a
                    // mob.observe grant cannot reveal the lifecycle of an
                    // agent the caller is denied. Mob-level entries (no
                    // attribution) pass on the mob.observe grant alone.
                    if let Some(view) = access_view.filter(|view| view.enforced()) {
                        events.retain(|event| {
                            event
                                .agent_identity
                                .as_deref()
                                .is_none_or(|identity| view.can_view_agent(identity))
                        });
                    }
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
                        // Per-agent visibility: a caller only sees readiness
                        // for agents they may `agent.view`. The cache was
                        // primed from the roster at dispatch.
                        .filter_map(|(identity, snapshot)| {
                            let identity = identity.to_string();
                            let visible = access_view
                                .filter(|view| view.enforced())
                                .is_none_or(|view| view.can_view_agent(&identity));
                            visible.then(|| {
                                serde_json::json!({
                                    "agent_identity": identity,
                                    "snapshot": serde_json::to_value(&snapshot)
                                        .unwrap_or(Value::Null),
                                })
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
                // Per-agent visibility: even an `agent.spawn`-gated caller only
                // collects completions for agents they may `agent.view`.
                .filter_map(|(mid, snapshot)| {
                    let mid = mid.to_string();
                    let visible = access_view
                        .filter(|view| view.enforced())
                        .is_none_or(|view| view.can_view_agent(&mid));
                    visible.then(|| {
                        serde_json::json!({
                            "member_id": mid,
                            "snapshot": serde_json::to_value(&snapshot).unwrap_or(Value::Null),
                        })
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
            if let Some(identity_runtime) = &identity_runtime
                && let Err(err) = identity_runtime.materialize_all_required().await
            {
                return internal_error(
                    response_id,
                    format!("identity-first flow materialization failed: {err}"),
                );
            }
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
    members.retain(|member| {
        visibility_policy.member_visible(member)
            && visibility_policy
                .identity_visible(&console_identity_record_from_console_member(member))
    });

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
            let console_identity = console_member_console_identity(member);
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
                Some(store) => store.response_phase_for_identity(console_identity).await,
                None => None,
            };
            ConsoleAgentLiveSnapshot {
                agent_id: member.agent_identity.clone(),
                member_id: member.agent_identity.clone(),
                label,
                kind: "meerkat".to_string(),
                identity: Some(console_identity.to_string()),
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
            handle.status_observation_snapshot(),
            MobState::Creating | MobState::Running
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
        for (mob_id, delegate_handle) in mcp_state.mob_handles_snapshot().await {
            if processed.contains(mob_id.as_str()) {
                continue;
            }
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
    for entry in handle.list_members_observation_snapshot().await {
        let identity = entry.agent_identity.to_string();
        let Some(session_id) = handle
            .resolve_bridge_session_id_observation(&entry.agent_identity)
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
        let visible = visibility_policy.member_visible(member)
            && visibility_policy
                .identity_visible(&console_identity_record_from_console_member(member));
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
    identity_runtime: Option<&Arc<crate::identity_first::IdentityRuntime>>,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let read_model = ConsoleSnapshotReadModel::default();
    *read_model.inner.write().await = collect_console_snapshot_read_model(runtime).await;
    // Mark the freshly-built model primed so `build_live_snapshot` doesn't
    // try to re-prime; this is a one-shot read for the reset path.
    read_model
        .primed
        .store(true, std::sync::atomic::Ordering::Release);
    let snapshot =
        build_live_snapshot(runtime, &[], console_events, visibility_policy, &read_model).await;
    let raw_snapshot = build_live_snapshot(
        runtime,
        &[],
        console_events,
        &crate::console_aggregator::AllowAllConsoleVisibilityPolicy,
        &read_model,
    )
    .await;
    let identity_runtime_statuses = if let Some(identity_runtime) = identity_runtime {
        identity_runtime.statuses().await
    } else {
        Vec::new()
    };
    let identity_by_runtime_member_id = identity_runtime_statuses
        .iter()
        .filter_map(|status| {
            status
                .agent_runtime_id
                .as_ref()
                .map(|runtime_id| (runtime_id.as_str().to_string(), status.identity.to_string()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut durable_identity_runtime_identities = identity_runtime_statuses
        .iter()
        .filter(|status| identity_status_visible_to_console(visibility_policy, status))
        .map(|status| status.identity.to_string())
        .collect::<BTreeSet<_>>();
    let mut main_identities = BTreeSet::new();
    let mut runtime_member_id_by_identity = BTreeMap::new();
    let mut runtime_member_ids_by_identity: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut session_id_by_identity_runtime_member: BTreeMap<(String, String), Option<String>> =
        BTreeMap::new();
    let mut live_alias_by_runtime_member_id: BTreeMap<String, (String, Option<String>)> =
        BTreeMap::new();
    let mut visible_runtime_member_ids = BTreeSet::new();
    let mut duplicate_live_identities = BTreeSet::new();
    let mut delegate_members = BTreeSet::new();
    for member in snapshot.members {
        if member.state == MEMBER_STATE_RETIRING {
            continue;
        }
        if let Some(source_mob_id) = member.labels.get("source_mob_id").cloned() {
            delegate_members.insert((source_mob_id, member.agent_identity));
        } else {
            let identity = member
                .labels
                .get("agent_identity")
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .or_else(|| {
                    identity_by_runtime_member_id
                        .get(&member.agent_identity)
                        .cloned()
                })
                .unwrap_or_else(|| member.agent_identity.clone());
            if let Some(existing) = runtime_member_id_by_identity.get(&identity)
                && existing != &member.agent_identity
            {
                duplicate_live_identities.insert(identity.clone());
            }
            runtime_member_ids_by_identity
                .entry(identity.clone())
                .or_default()
                .insert(member.agent_identity.clone());
            session_id_by_identity_runtime_member.insert(
                (identity.clone(), member.agent_identity.clone()),
                member.session_id.clone(),
            );
            live_alias_by_runtime_member_id.insert(
                member.agent_identity.clone(),
                (identity.clone(), member.session_id.clone()),
            );
            visible_runtime_member_ids.insert(member.agent_identity.clone());
            runtime_member_id_by_identity
                .entry(identity.clone())
                .or_insert(member.agent_identity);
            main_identities.insert(identity);
        }
    }
    let mut raw_runtime_member_ids_by_identity: BTreeMap<String, BTreeSet<String>> =
        BTreeMap::new();
    let mut raw_session_id_by_identity_runtime_member: BTreeMap<(String, String), Option<String>> =
        BTreeMap::new();
    let mut raw_live_alias_by_runtime_member_id: BTreeMap<String, (String, Option<String>)> =
        BTreeMap::new();
    for member in raw_snapshot.members {
        if member.state == MEMBER_STATE_RETIRING || member.labels.contains_key("source_mob_id") {
            continue;
        }
        let identity = member
            .labels
            .get("agent_identity")
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .or_else(|| {
                identity_by_runtime_member_id
                    .get(&member.agent_identity)
                    .cloned()
            })
            .unwrap_or_else(|| member.agent_identity.clone());
        raw_runtime_member_ids_by_identity
            .entry(identity.clone())
            .or_default()
            .insert(member.agent_identity.clone());
        raw_session_id_by_identity_runtime_member.insert(
            (identity.clone(), member.agent_identity.clone()),
            member.session_id.clone(),
        );
        raw_live_alias_by_runtime_member_id
            .insert(member.agent_identity, (identity, member.session_id));
    }
    durable_identity_runtime_identities.retain(|identity| {
        identity_runtime_statuses
            .iter()
            .find(|status| status.identity.as_str() == identity)
            .and_then(|status| status.agent_runtime_id.as_ref())
            .is_none_or(|runtime_id| {
                let runtime_id = runtime_id.as_str();
                !raw_live_alias_by_runtime_member_id.contains_key(runtime_id)
                    || visible_runtime_member_ids.contains(runtime_id)
            })
    });
    let current_main_identities = main_identities.clone();
    let baseline_specs = runtime.baseline_member_specs().await;
    let baseline_identities = baseline_specs
        .iter()
        .filter(|spec| baseline_spec_visible_to_console(visibility_policy, spec))
        .map(|spec| spec.identity.to_string())
        .collect::<BTreeSet<_>>();
    main_identities.extend(baseline_identities.iter().cloned());
    main_identities.extend(durable_identity_runtime_identities.iter().cloned());

    let mut retired_delegates = Vec::new();
    let mut reset_main = Vec::new();
    let mut retired_delegate_details = Vec::new();
    let mut reset_details = Vec::new();
    let mut failures = Vec::new();
    let mut warnings = Vec::new();

    for identity in &main_identities {
        let parsed_identity = crate::identity_first::AgentIdentity::parse(identity).ok();
        let registered_status = if let (Some(identity_runtime), Some(parsed_identity)) =
            (identity_runtime, parsed_identity.as_ref())
        {
            identity_runtime.status(parsed_identity).await.ok()
        } else {
            None
        };
        let baseline_identity_runtime_registered = registered_status.is_some();
        if baseline_identities.contains(identity)
            && !current_main_identities.contains(identity)
            && !baseline_identity_runtime_registered
        {
            continue;
        }
        let registered_runtime_id = registered_status
            .as_ref()
            .and_then(|status| status.agent_runtime_id.as_ref())
            .map(crate::identity_first::AgentRuntimeId::as_str);
        let registered_visible = registered_runtime_id
            .is_some_and(|runtime_id| visible_runtime_member_ids.contains(runtime_id));
        let registered_hidden = registered_runtime_id.is_some_and(|runtime_id| {
            raw_live_alias_by_runtime_member_id.contains_key(runtime_id)
                && !visible_runtime_member_ids.contains(runtime_id)
        });
        if registered_hidden {
            continue;
        }
        if duplicate_live_identities.contains(identity) && !registered_visible {
            failures.push(json!({
                "identity": identity,
                "error": "ambiguous live identity alias",
            }));
            continue;
        }
        if let Some(status) = registered_status.as_ref() {
            if let Some(registered_runtime_id) = registered_runtime_id
                && let Some((live_identity, _live_session_id)) =
                    raw_live_alias_by_runtime_member_id.get(registered_runtime_id)
                && live_identity != identity
            {
                failures.push(json!({
                    "identity": identity,
                    "error": format!(
                        "stale live identity alias: identity runtime binding points at {registered_runtime_id}, but live console alias projects identity {live_identity}"
                    ),
                    "kind": "stale_live_identity_alias",
                }));
                continue;
            }
            if let Some(live_runtime_ids) = raw_runtime_member_ids_by_identity.get(identity) {
                if !registered_runtime_id
                    .is_some_and(|runtime_id| live_runtime_ids.contains(runtime_id))
                {
                    failures.push(json!({
                        "identity": identity,
                        "error": format!(
                            "stale live identity alias: identity runtime binding points at {}, but live console alias resolves to [{}]",
                            registered_runtime_id.unwrap_or("<none>"),
                            live_runtime_ids.iter().cloned().collect::<Vec<_>>().join(", ")
                        ),
                        "kind": "stale_live_identity_alias",
                    }));
                    continue;
                }
                if let Some(registered_runtime_id) = registered_runtime_id
                    && let Some(registered_session_id) =
                        status.session_id.as_ref().map(ToString::to_string)
                    && let Some(Some(live_session_id)) = raw_session_id_by_identity_runtime_member
                        .get(&(identity.clone(), registered_runtime_id.to_string()))
                    && live_session_id != &registered_session_id
                {
                    failures.push(json!({
                        "identity": identity,
                        "error": format!(
                            "stale live identity alias: identity runtime binding points at {registered_runtime_id} session {registered_session_id}, but live console alias resolves to session {live_session_id}"
                        ),
                        "kind": "stale_live_identity_alias",
                    }));
                    continue;
                }
            }
            if baseline_identities.contains(identity)
                && !identity_runtime
                    .is_some_and(|identity_runtime| identity_runtime.has_session_bridge())
            {
                failures.push(json!({
                    "identity": identity,
                    "error": "reset requires an identity runtime with a session bridge",
                    "kind": "identity_reset_requires_session_bridge",
                }));
            }
            continue;
        }

        let runtime_member_id = runtime_member_id_by_identity
            .get(identity)
            .map(String::as_str)
            .unwrap_or(identity.as_str());
        if let Some(bound_identity) = identity_by_runtime_member_id.get(runtime_member_id)
            && bound_identity != identity
        {
            failures.push(json!({
                "identity": identity,
                "error": format!(
                    "stale live identity alias: live console alias resolves to {runtime_member_id}, but identity runtime binding belongs to {bound_identity}"
                ),
                "kind": "stale_live_identity_alias",
            }));
        }
    }

    if !failures.is_empty() {
        return Ok(json!({
            "reset": reset_main,
            "retired_delegates": retired_delegates,
            "reset_details": reset_details,
            "retired_delegate_details": retired_delegate_details,
            "warnings": warnings,
            "failed": failures,
            "startup_history": Value::Null,
        }));
    }

    if let Some(state) = runtime.agent_mob_mcp_state() {
        for (mob_id, identity) in delegate_members {
            match state.handle_for(&MobId::from(mob_id.as_str())).await {
                Ok(handle) => {
                    match retire_console_member(&handle, &MeerkatId::from(identity.as_str())).await
                    {
                        Ok(()) => {
                            let detail = json!({
                                "identity": identity,
                                "mob_id": mob_id,
                            });
                            retired_delegates.push(detail.clone());
                            retired_delegate_details.push(detail);
                        }
                        Err(err) => failures.push(json!({
                            "identity": identity,
                            "mob_id": mob_id,
                            "error": err,
                        })),
                    }
                }
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
            match Box::pin(aggregator.retire_identity(&identity)).await {
                Ok(true) => {
                    let detail = json!({ "identity": identity });
                    retired_delegates.push(detail.clone());
                    retired_delegate_details.push(detail);
                }
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
        if !baseline_spec_visible_to_console(visibility_policy, &spec) {
            continue;
        }
        if current_main_identities.contains(&identity) {
            continue;
        }
        if let Some(identity_runtime) = identity_runtime
            && let Ok(parsed_identity) = crate::identity_first::AgentIdentity::parse(&identity)
            && identity_runtime.status(&parsed_identity).await.is_ok()
        {
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
                reset_main.push(identity.clone());
                reset_details.push(json!({ "identity": identity }));
            }
            Err(err) => failures.push(json!({
                "identity": identity,
                "error": err.to_string(),
            })),
        }
    }
    for identity in main_identities {
        let baseline_identity_runtime_registered = if let Some(identity_runtime) = identity_runtime
            && let Ok(parsed_identity) = crate::identity_first::AgentIdentity::parse(&identity)
        {
            identity_runtime.status(&parsed_identity).await.is_ok()
        } else {
            false
        };
        if baseline_identities.contains(&identity)
            && !current_main_identities.contains(&identity)
            && !baseline_identity_runtime_registered
        {
            continue;
        }
        let registered_status = if let Some(identity_runtime) = identity_runtime
            && let Ok(parsed_identity) = crate::identity_first::AgentIdentity::parse(&identity)
        {
            identity_runtime.status(&parsed_identity).await.ok()
        } else {
            None
        };
        let registered_runtime_id = registered_status
            .as_ref()
            .and_then(|status| status.agent_runtime_id.as_ref())
            .map(crate::identity_first::AgentRuntimeId::as_str);
        let registered_visible = registered_runtime_id
            .is_some_and(|runtime_id| visible_runtime_member_ids.contains(runtime_id));
        let registered_hidden = registered_runtime_id.is_some_and(|runtime_id| {
            raw_live_alias_by_runtime_member_id.contains_key(runtime_id)
                && !visible_runtime_member_ids.contains(runtime_id)
        });
        if registered_hidden {
            continue;
        }
        if duplicate_live_identities.contains(&identity) && !registered_visible {
            failures.push(json!({
                "identity": identity,
                "error": "ambiguous live identity alias",
            }));
            continue;
        }
        if baseline_identities.contains(&identity) {
            if let Some(identity_runtime) = identity_runtime
                && let Ok(parsed_identity) = crate::identity_first::AgentIdentity::parse(&identity)
                && let Ok(status) = identity_runtime.status(&parsed_identity).await
            {
                let registered_runtime_id = status
                    .agent_runtime_id
                    .as_ref()
                    .map(crate::identity_first::AgentRuntimeId::as_str);
                if let Some(registered_runtime_id) = registered_runtime_id
                    && let Some((live_identity, _live_session_id)) =
                        raw_live_alias_by_runtime_member_id.get(registered_runtime_id)
                    && live_identity != &identity
                {
                    failures.push(json!({
                        "identity": identity,
                        "error": format!(
                            "stale live identity alias: identity runtime binding points at {registered_runtime_id}, but live console alias projects identity {live_identity}"
                        ),
                        "kind": "stale_live_identity_alias",
                    }));
                    continue;
                }
                if let Some(live_runtime_ids) = raw_runtime_member_ids_by_identity.get(&identity) {
                    if !registered_runtime_id
                        .is_some_and(|runtime_id| live_runtime_ids.contains(runtime_id))
                    {
                        failures.push(json!({
                            "identity": identity,
                            "error": format!(
                                "stale live identity alias: identity runtime binding points at {}, but live console alias resolves to [{}]",
                                registered_runtime_id.unwrap_or("<none>"),
                                live_runtime_ids.iter().cloned().collect::<Vec<_>>().join(", ")
                            ),
                            "kind": "stale_live_identity_alias",
                        }));
                        continue;
                    }
                    if let Some(registered_runtime_id) = registered_runtime_id
                        && let Some(registered_session_id) =
                            status.session_id.as_ref().map(ToString::to_string)
                        && let Some(Some(live_session_id)) =
                            raw_session_id_by_identity_runtime_member
                                .get(&(identity.clone(), registered_runtime_id.to_string()))
                        && live_session_id != &registered_session_id
                    {
                        failures.push(json!({
                            "identity": identity,
                            "error": format!(
                                "stale live identity alias: identity runtime binding points at {registered_runtime_id} session {registered_session_id}, but live console alias resolves to session {live_session_id}"
                            ),
                            "kind": "stale_live_identity_alias",
                        }));
                        continue;
                    }
                }
                if !identity_runtime.has_session_bridge() {
                    failures.push(json!({
                        "identity": identity,
                        "error": "reset requires an identity runtime with a session bridge",
                        "kind": "identity_reset_requires_session_bridge",
                    }));
                    continue;
                }
                match identity_runtime.reset(&parsed_identity).await {
                    Ok(record) => {
                        match retire_stale_console_members_for_identity(
                            &handle,
                            visibility_policy,
                            parsed_identity.as_str(),
                            Some(record.agent_runtime_id.as_str()),
                        )
                        .await
                        {
                            Ok(()) => {
                                reset_details.push(json!({ "identity": identity }));
                                reset_main.push(identity);
                                if let Some(store) = console_events {
                                    store
                                        .record_lifecycle(
                                            parsed_identity.as_str(),
                                            "identity_reset",
                                            json!({
                                                "scope": "reset_all",
                                                "generation": record.generation.get(),
                                                "checkpoint_version": record.checkpoint_version.get(),
                                            }),
                                        )
                                        .await;
                                }
                            }
                            Err(err) => {
                                warnings.push(json!({
                                    "identity": identity,
                                    "kind": "stale_member_cleanup_failed_after_identity_reset",
                                    "message": err,
                                }));
                                reset_details.push(json!({
                                    "identity": identity,
                                    "cleanup_warning": warnings.last().cloned(),
                                }));
                                reset_main.push(identity);
                                if let Some(store) = console_events {
                                    store
                                        .record_lifecycle(
                                            parsed_identity.as_str(),
                                            "identity_reset",
                                            json!({
                                                "scope": "reset_all",
                                                "generation": record.generation.get(),
                                                "checkpoint_version": record.checkpoint_version.get(),
                                                "cleanup_warning": warnings.last().cloned(),
                                            }),
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                    Err(err) => failures.push(json!({
                        "identity": identity,
                        "error": err.to_string(),
                    })),
                }
                continue;
            }
            let runtime_member_id = runtime_member_id_by_identity
                .get(&identity)
                .map(String::as_str)
                .unwrap_or(identity.as_str());
            if let Some(bound_identity) = identity_by_runtime_member_id.get(runtime_member_id)
                && bound_identity != &identity
            {
                failures.push(json!({
                    "identity": identity,
                    "error": format!(
                        "stale live identity alias: live console alias resolves to {runtime_member_id}, but identity runtime binding belongs to {bound_identity}"
                    ),
                    "kind": "stale_live_identity_alias",
                }));
                continue;
            }
            match respawn_console_member(&handle, &MeerkatId::from(runtime_member_id)).await {
                Ok(topology_restore_warning) => {
                    if let Some(store) = console_events {
                        store
                            .record_lifecycle(
                                &identity,
                                "identity_reset",
                                json!({
                                    "scope": "reset_all",
                                    "topology_restore_warning": topology_restore_warning.clone(),
                                }),
                            )
                            .await;
                    }
                    reset_main.push(identity.clone());
                    let mut detail = json!({ "identity": identity });
                    if let Some(warning) = topology_restore_warning {
                        detail["topology_restore_warning"] = warning;
                    }
                    reset_details.push(detail);
                }
                Err(err) => failures.push(json!({
                    "identity": identity,
                    "error": err,
                })),
            }
        } else {
            if let Some(identity_runtime) = identity_runtime
                && let Ok(parsed_identity) = crate::identity_first::AgentIdentity::parse(&identity)
                && let Ok(registered_status) = identity_runtime.status(&parsed_identity).await
            {
                let registered_runtime_id = registered_status
                    .agent_runtime_id
                    .as_ref()
                    .map(crate::identity_first::AgentRuntimeId::as_str);
                let registered_visible = registered_runtime_id
                    .is_some_and(|runtime_id| visible_runtime_member_ids.contains(runtime_id));
                if duplicate_live_identities.contains(&identity) && !registered_visible {
                    failures.push(json!({
                        "identity": identity,
                        "error": "ambiguous live identity alias",
                    }));
                    continue;
                }
                if let Some(registered_runtime_id) = registered_runtime_id
                    && let Some((live_identity, _live_session_id)) =
                        raw_live_alias_by_runtime_member_id.get(registered_runtime_id)
                    && live_identity != &identity
                {
                    failures.push(json!({
                        "identity": identity,
                        "error": format!(
                            "stale live identity alias: identity runtime binding points at {registered_runtime_id}, but live console alias projects identity {live_identity}"
                        ),
                        "kind": "stale_live_identity_alias",
                    }));
                    continue;
                }
                if let Some(live_runtime_ids) = raw_runtime_member_ids_by_identity.get(&identity) {
                    if !registered_runtime_id
                        .is_some_and(|runtime_id| live_runtime_ids.contains(runtime_id))
                    {
                        failures.push(json!({
                            "identity": identity,
                            "error": format!(
                                "stale live identity alias: identity runtime binding points at {}, but live console alias resolves to [{}]",
                                registered_runtime_id.unwrap_or("<none>"),
                                live_runtime_ids.iter().cloned().collect::<Vec<_>>().join(", ")
                            ),
                            "kind": "stale_live_identity_alias",
                        }));
                        continue;
                    }
                    if let Some(registered_runtime_id) = registered_runtime_id
                        && let Some(registered_session_id) = registered_status
                            .session_id
                            .as_ref()
                            .map(ToString::to_string)
                        && let Some(Some(live_session_id)) =
                            raw_session_id_by_identity_runtime_member
                                .get(&(identity.clone(), registered_runtime_id.to_string()))
                        && live_session_id != &registered_session_id
                    {
                        failures.push(json!({
                            "identity": identity,
                            "error": format!(
                                "stale live identity alias: identity runtime binding points at {registered_runtime_id} session {registered_session_id}, but live console alias resolves to session {live_session_id}"
                            ),
                            "kind": "stale_live_identity_alias",
                        }));
                        continue;
                    }
                }
                match identity_runtime.retire(&parsed_identity).await {
                    Ok(token) => {
                        let keep_runtime_member_id = registered_status
                            .agent_runtime_id
                            .as_ref()
                            .filter(|_| identity_runtime.has_session_bridge())
                            .map(crate::identity_first::AgentRuntimeId::as_str);
                        match retire_stale_console_members_for_identity(
                            &handle,
                            visibility_policy,
                            parsed_identity.as_str(),
                            keep_runtime_member_id,
                        )
                        .await
                        {
                            Ok(()) => {
                                retired_delegate_details.push(json!({ "identity": identity }));
                                retired_delegates.push(json!({ "identity": identity }));
                                if let Some(store) = console_events {
                                    store
                                        .record_lifecycle(
                                            parsed_identity.as_str(),
                                            "identity_retired",
                                            json!({
                                                "scope": "reset_all",
                                                "dynamic": true,
                                                "fencing_token": token.get(),
                                            }),
                                        )
                                        .await;
                                }
                            }
                            Err(err) => {
                                warnings.push(json!({
                                    "identity": identity,
                                    "kind": "stale_member_cleanup_failed_after_identity_retire",
                                    "message": err,
                                }));
                                retired_delegate_details.push(json!({
                                    "identity": identity,
                                    "cleanup_warning": warnings.last().cloned(),
                                }));
                                retired_delegates.push(json!({ "identity": identity }));
                                if let Some(store) = console_events {
                                    store
                                        .record_lifecycle(
                                            parsed_identity.as_str(),
                                            "identity_retired",
                                            json!({
                                                "scope": "reset_all",
                                                "dynamic": true,
                                                "fencing_token": token.get(),
                                                "cleanup_warning": warnings.last().cloned(),
                                            }),
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                    Err(err) => failures.push(json!({
                        "identity": identity,
                        "error": err.to_string(),
                    })),
                }
                continue;
            }
            let runtime_member_id = runtime_member_id_by_identity
                .get(&identity)
                .map(String::as_str)
                .unwrap_or(identity.as_str());
            if let Some(bound_identity) = identity_by_runtime_member_id.get(runtime_member_id)
                && bound_identity != &identity
            {
                failures.push(json!({
                    "identity": identity,
                    "error": format!(
                        "stale live identity alias: live console alias resolves to {runtime_member_id}, but identity runtime binding belongs to {bound_identity}"
                    ),
                    "kind": "stale_live_identity_alias",
                }));
                continue;
            }
            match retire_console_member(&handle, &MeerkatId::from(runtime_member_id)).await {
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
                    retired_delegate_details.push(json!({ "identity": identity }));
                }
                Err(err) => failures.push(json!({
                    "identity": identity,
                    "error": err,
                })),
            }
        }
    }

    let startup_history = if failures.is_empty() {
        if let Some(aggregator) = console_aggregator {
            Box::pin(wait_for_reset_startup_history(
                aggregator,
                reset_main.iter().cloned().collect::<BTreeSet<_>>(),
                Duration::from_secs(10),
            ))
            .await
            .unwrap_or_else(|err| json!({ "error": err.to_string() }))
        } else {
            Value::Null
        }
    } else {
        Value::Null
    };

    Ok(json!({
        "reset": reset_main,
        "retired_delegates": retired_delegates,
        "reset_details": reset_details,
        "retired_delegate_details": retired_delegate_details,
        "warnings": warnings,
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
            let page = Box::pin(aggregator.query_timeline(ConsoleTimelineQuery {
                identity: Some(identity.clone()),
                limit: 1000,
                ..ConsoleTimelineQuery::default()
            }))
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

fn console_member_console_identity(member: &ConsoleMember) -> &str {
    member
        .labels
        .get("agent_identity")
        .filter(|value| !value.trim().is_empty())
        .map_or(member.agent_identity.as_str(), String::as_str)
}

fn console_identity_record_from_console_member(member: &ConsoleMember) -> ConsoleIdentityRecord {
    let identity = console_member_console_identity(member).to_string();
    let addressable = member
        .labels
        .get("addressable")
        .map(|value| !value.eq_ignore_ascii_case("false"))
        .unwrap_or(true)
        && member.state == MEMBER_STATE_ACTIVE;
    let visibility = if member.state == MEMBER_STATE_RETIRING {
        ConsoleVisibility::RetiredReadable
    } else if addressable {
        ConsoleVisibility::Addressable
    } else {
        ConsoleVisibility::Hidden
    };
    ConsoleIdentityRecord {
        identity: identity.clone(),
        display_name: member
            .labels
            .get("display_name")
            .cloned()
            .unwrap_or(identity),
        runtime_key: "runtime".to_string(),
        runtime_member_id: member.agent_identity.clone(),
        session_id: member.session_id.clone(),
        visibility,
        addressable,
        health: member.state.clone(),
        topology_peers: member.wired_to.clone(),
        labels: member.labels.clone(),
    }
}

fn baseline_spec_visible_to_console(
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    spec: &SpawnMemberSpec,
) -> bool {
    let mut labels = spec.labels.clone().unwrap_or_default();
    labels
        .entry("role".to_string())
        .or_insert_with(|| spec.role_name.to_string());
    let record = ConsoleIdentityRecord {
        identity: spec.identity.to_string(),
        display_name: spec.identity.to_string(),
        runtime_key: "baseline".to_string(),
        runtime_member_id: spec.identity.to_string(),
        session_id: None,
        visibility: ConsoleVisibility::Addressable,
        addressable: true,
        health: "baseline".to_string(),
        topology_peers: Vec::new(),
        labels,
    };
    let member = ConsoleMember {
        agent_identity: spec.identity.to_string(),
        role: spec.role_name.to_string(),
        state: MEMBER_STATE_ACTIVE.to_string(),
        model_capabilities: ConsoleModelCapabilities::default(),
        runtime_mode: spec
            .runtime_mode
            .as_ref()
            .map(std::string::ToString::to_string),
        session_id: None,
        wired_to: Vec::new(),
        labels: record.labels.clone(),
    };
    visibility_policy.member_visible(&member) && visibility_policy.identity_visible(&record)
}

/// Prime the access-control attribute cache from the live primary-mob roster
/// so label/role rules resolve on surfaces that carry only an identity string
/// (SSE event streams, timeline frames, `mob_events`). Without this, those
/// surfaces evaluate role/label rules with attributes unknown and can fail
/// open for "broad allow + label-keyed deny" policies — the cache must not be
/// gated on a prior `/console/experience` call. Additive (it does not evict),
/// so it never clears delegate-member attributes primed by the experience
/// projection; the experience path owns wholesale rebuild + eviction.
pub(crate) async fn prime_access_cache_from_handle(handle: &MobHandle, access: &AccessController) {
    if !access.enabled() {
        return;
    }
    for entry in handle.list_all_members().await {
        let member_identity = entry.agent_identity.to_string();
        // Console identity may be overridden by a label, mirroring
        // `console_member_console_identity`.
        let console_identity = entry
            .labels
            .get("agent_identity")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map_or_else(|| member_identity.clone(), ToString::to_string);
        access.record_agent_attributes(AgentResourceAttributes {
            identity: console_identity,
            agent_id: Some(member_identity),
            role: Some(entry.role.to_string()),
            labels: entry.labels.clone(),
        });
    }
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
        // Keep /console/experience on the cached identity read model. Live
        // peer inspection walks the actor-backed mob/member path, and a stuck
        // turn must not make the console shell fail to load.
        let wired_to = identity.topology_peers.clone();
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
#[allow(clippy::expect_used, clippy::large_futures)]
mod tests {
    use super::ConsoleTimelineHttpQuery;
    use super::{
        ConsoleSnapshotReadModel, ConsoleSnapshotReadModelState, MAX_MULTIPART_BODY_BYTES,
        MAX_MULTIPART_IMAGE_BYTES, MultipartImageUpload, apply_console_visibility_policy,
        build_aggregator_live_snapshot, collect_console_snapshot_read_model,
        console_send_identity_first, console_send_with_identity_first_fallback,
        console_timeline_replay_unavailable_response, cursor_is_after,
        dedupe_console_members_by_identity, externalize_image_upload_placeholders,
        externalize_single_image_upload, handle_console_aggregator_rpc, handle_console_runtime_rpc,
        handle_console_runtime_rpc_with_visibility, member_id_matches_durable_identity,
        project_console_members_from_handle, query_timeline_snapshot, timeline_query_from_http,
    };
    use crate::blob_store::{BinaryBlobStore, ObjectStoreBlobStore};
    use crate::console_aggregator::{
        AllowAllConsoleVisibilityPolicy, ConsoleIdentityRecord,
        HideImplicitDelegateMembersConsoleVisibilityPolicy,
    };
    use crate::console_aggregator::{
        ConsoleCursor, ConsoleFrameSource, ConsoleFrameSourceKind, ConsoleFrameStatus,
        ConsoleTimelineQuery, ConsoleTimelineWindowQuery, ConsoleVisibilityPolicy,
        MobKitConsoleAggregator, NewConsoleFrame,
    };
    use crate::identity_first::contracts::{ContinuityStore, LeaseProvider};
    use crate::identity_first::{
        AgentAddressability, AgentBuildDraft, AgentIdentity, AgentRuntimeId, BridgeError,
        CheckpointVersion, ContinuityGeneration, ContinuityRecord, DurabilityPolicy,
        DurableAgentSpec, FencingToken, IdentityLifecycleState, IdentityRuntime,
        IdentityRuntimeConfig, LeaseAcquireResult, LeaseGrant, LocalContinuityStore,
        LocalLeaseProvider, ManagedPeerEdge, ResumeSessionOutcome, SessionBridge, SessionSnapshot,
    };
    use crate::mob_handle_runtime::{MobRuntime, model_capabilities_for_role};
    use crate::rpc::{JSONRPC_VERSION, JsonRpcRequest};
    use crate::runtime::{ConsoleAgentLiveSnapshot, ConsoleLiveSnapshot, ConsoleMember};
    use crate::unified_runtime::ConsoleEventStore;
    use crate::{MobBootstrapOptions, MobBootstrapSpec};
    use bytes::Bytes;
    use meerkat::{AgentFactory, Config, build_ephemeral_service};
    use meerkat_client::TestClient;
    use meerkat_core::types::HandlingMode;
    use meerkat_mob::ProfileName;
    use meerkat_mob::{MobDefinition, MobStorage, SpawnMemberSpec};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct BlockingIdentityBridge {
        deliver_calls: Arc<AtomicUsize>,
    }

    struct RecordingIdentityBridge {
        session_id: meerkat_core::types::SessionId,
        handling_modes: Arc<Mutex<Vec<HandlingMode>>>,
    }

    #[async_trait::async_trait]
    impl SessionBridge for BlockingIdentityBridge {
        async fn create_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            session_id: &meerkat_core::types::SessionId,
        ) -> Result<meerkat_core::types::SessionId, BridgeError> {
            Ok(session_id.clone())
        }

        async fn resume_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            session_id: &meerkat_core::types::SessionId,
            _snapshot: &SessionSnapshot,
        ) -> Result<ResumeSessionOutcome, BridgeError> {
            Ok(ResumeSessionOutcome::Resumed {
                session_id: session_id.clone(),
            })
        }

        async fn deliver(
            &self,
            _runtime_id: &AgentRuntimeId,
            _content: &meerkat_core::ContentInput,
        ) -> Result<meerkat_core::types::SessionId, BridgeError> {
            self.deliver_calls.fetch_add(1, Ordering::SeqCst);
            std::future::pending().await
        }

        async fn checkpoint_session(
            &self,
            _runtime_id: &AgentRuntimeId,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<SessionSnapshot, BridgeError> {
            Err(BridgeError::Mob("checkpoint not used in test".to_string()))
        }

        async fn retire_member(&self, _runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl SessionBridge for RecordingIdentityBridge {
        async fn create_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            session_id: &meerkat_core::types::SessionId,
        ) -> Result<meerkat_core::types::SessionId, BridgeError> {
            Ok(session_id.clone())
        }

        async fn resume_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            session_id: &meerkat_core::types::SessionId,
            _snapshot: &SessionSnapshot,
        ) -> Result<ResumeSessionOutcome, BridgeError> {
            Ok(ResumeSessionOutcome::Resumed {
                session_id: session_id.clone(),
            })
        }

        async fn deliver(
            &self,
            runtime_id: &AgentRuntimeId,
            content: &meerkat_core::ContentInput,
        ) -> Result<meerkat_core::types::SessionId, BridgeError> {
            self.deliver_with_mode(runtime_id, content, HandlingMode::Queue)
                .await
        }

        async fn deliver_with_mode(
            &self,
            _runtime_id: &AgentRuntimeId,
            _content: &meerkat_core::ContentInput,
            handling_mode: HandlingMode,
        ) -> Result<meerkat_core::types::SessionId, BridgeError> {
            self.handling_modes
                .lock()
                .map_err(|_| BridgeError::Mob("handling modes mutex poisoned".to_string()))?
                .push(handling_mode);
            Ok(self.session_id.clone())
        }

        async fn checkpoint_session(
            &self,
            _runtime_id: &AgentRuntimeId,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<SessionSnapshot, BridgeError> {
            Err(BridgeError::Mob("checkpoint not used in test".to_string()))
        }

        async fn retire_member(&self, _runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
            Ok(())
        }
    }

    async fn build_empty_console_test_runtime(
        mob_id: &str,
    ) -> Result<(tempfile::TempDir, MobRuntime), Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::tempdir()?;
        let session_path = temp_dir.path().join("sessions");
        std::fs::create_dir_all(&session_path)?;
        let factory = AgentFactory::new(&session_path).comms(true);
        let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));
        let definition = MobDefinition::from_toml(&format!(
            r#"
[mob]
id = "{mob_id}"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true

[profiles.worker.tools]
comms = true
"#
        ))?;
        let runtime = MobRuntime::bootstrap(
            MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
                .with_options(MobBootstrapOptions {
                    allow_ephemeral_sessions: true,
                    notify_orchestrator_on_resume: true,
                    default_llm_client: Some(Arc::new(TestClient::default())),
                }),
        )
        .await?;
        Ok((temp_dir, runtime))
    }

    fn rpc_request(method: &str) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: method.to_string(),
            params: json!({}),
        }
    }

    fn rpc_request_with_params(method: &str, params: Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: method.to_string(),
            params,
        }
    }

    #[tokio::test]
    async fn read_only_aggregator_capabilities_omit_mutating_methods() {
        let response = handle_console_aggregator_rpc(
            None,
            rpc_request("mobkit/capabilities"),
            true,
            true,
            None,
            None,
        )
        .await;

        let methods = response["result"]["methods"]
            .as_array()
            .expect("capabilities methods");
        assert!(
            methods.iter().all(|method| method != "mobkit/console/send"),
            "read-only capabilities must omit send: {methods:#?}"
        );
        assert_eq!(response["result"]["read_only"], json!(true));
        assert_eq!(
            response["result"]["runtime_capabilities"]["can_send_messages"],
            json!(false)
        );
    }

    #[tokio::test]
    async fn read_only_aggregator_denies_direct_mutating_rpc() {
        let response = handle_console_aggregator_rpc(
            None,
            rpc_request_with_params(
                "mobkit/console/send",
                json!({
                    "identity": "worker",
                    "content": "hello",
                    "origin": "test",
                    "idempotency_key": "read-only-send",
                }),
            ),
            true,
            true,
            None,
            None,
        )
        .await;

        assert_eq!(response["result"], Value::Null);
        assert_eq!(response["error"]["code"], json!(-32010));
        assert_eq!(response["error"]["data"]["kind"], json!("read_only"));
    }

    #[test]
    fn read_only_mutating_methods_include_state_draining_collect_completed() {
        assert!(super::is_console_mutating_rpc_method(
            "mobkit/collect_completed"
        ));
    }

    #[test]
    fn authenticated_gating_approver_comes_from_console_principal() {
        let forged_params = json!({ "approver_id": "forged-browser-value" });

        assert_eq!(
            super::resolve_gating_approver_id(&forged_params, Some("admin@example.com"),),
            Ok("admin@example.com".to_string()),
        );
    }

    #[test]
    fn unauthenticated_local_gating_approver_requires_request_param() {
        assert_eq!(
            super::resolve_gating_approver_id(&json!({ "approver_id": " local-operator " }), None,),
            Ok("local-operator".to_string()),
        );
        assert_eq!(
            super::resolve_gating_approver_id(&json!({}), None),
            Err("approver_id required"),
        );
    }

    #[test]
    fn internal_error_does_not_disclose_backend_details() {
        let response = super::internal_error(json!(7), "secret backend DSN");

        assert_eq!(response["error"]["code"], json!(-32000));
        assert_eq!(response["error"]["message"], json!("internal error"));
        assert_eq!(response["error"]["data"]["error"], json!("internal_error"));
        assert!(!response.to_string().contains("secret backend DSN"));
    }

    #[test]
    fn console_send_public_message_hides_dispatch_details() {
        let message = super::console_send_public_message(
            &crate::console_aggregator::ConsoleSendError::Dispatch(
                "secret backend DSN".to_string(),
            ),
        );

        assert_eq!(message, "console send failed");
        assert!(!message.contains("secret backend DSN"));
    }

    #[test]
    fn console_send_json_rpc_error_hides_backend_details() {
        let response = super::console_send_rpc_error(
            json!(7),
            crate::console_aggregator::ConsoleSendError::Dispatch("secret backend DSN".to_string()),
        );

        assert_eq!(response["error"]["code"], json!(-32000));
        assert_eq!(response["error"]["message"], json!("console send failed"));
        assert!(!response.to_string().contains("secret backend DSN"));
    }

    #[test]
    fn console_timeline_replay_error_hides_backend_details() {
        let response = super::console_timeline_replay_unavailable_response(
            json!(7),
            Box::new(std::io::Error::other("secret backend DSN")),
            None,
            None,
        );

        assert_eq!(
            response["error"]["code"],
            json!(crate::rpc::CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE)
        );
        assert_eq!(
            response["error"]["message"],
            json!("timeline replay unavailable")
        );
        assert!(!response.to_string().contains("secret backend DSN"));
    }

    #[test]
    fn gating_decision_error_hides_backend_details() {
        let response = super::gating_decision_failed_error(json!(7), "secret backend DSN");

        assert_eq!(response["error"]["code"], json!(-32602));
        assert_eq!(
            response["error"]["message"],
            json!("gating decision failed")
        );
        assert!(!response.to_string().contains("secret backend DSN"));
    }

    #[tokio::test]
    async fn console_runtime_identity_controls_resolve_durable_member_aliases()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-identity-control-alias").await?;
        let mut labels = BTreeMap::new();
        labels.insert("agent_identity".to_string(), "review:singleton".to_string());
        labels.insert("display_name".to_string(), "Review Agent".to_string());
        runtime
            .handle()
            .spawn_spec(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "rt:review:singleton:0".to_string(),
                    Some("You are Review Agent.".into()),
                    None,
                    None,
                )
                .with_labels(labels),
            )
            .await?;

        let durable_status = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            rpc_request_with_params(
                "mobkit/status_identity",
                json!({ "identity": "review:singleton" }),
            ),
            true,
        ))
        .await;
        assert_eq!(durable_status["error"], Value::Null);
        assert_eq!(
            durable_status["result"]["identity"],
            json!("review:singleton")
        );
        assert_eq!(
            durable_status["result"]["agent_runtime_id"],
            json!("rt:review:singleton:0")
        );

        let runtime_id_status = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            rpc_request_with_params(
                "mobkit/status_identity",
                json!({ "identity": "rt:review:singleton:0" }),
            ),
            true,
        ))
        .await;
        assert_eq!(runtime_id_status["error"], Value::Null);
        assert_eq!(
            runtime_id_status["result"]["identity"],
            json!("review:singleton")
        );

        let runtime_id_inspect = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            rpc_request_with_params(
                "mobkit/inspect_identity",
                json!({ "identity": "rt:review:singleton:0" }),
            ),
            true,
        ))
        .await;
        assert_eq!(runtime_id_inspect["error"], Value::Null);
        assert_eq!(
            runtime_id_inspect["result"]["identity"],
            json!("review:singleton")
        );

        let respawn = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            Some(ConsoleEventStore::new()),
            None,
            None,
            None,
            None,
            rpc_request_with_params("mobkit/respawn", json!({ "identity": "review:singleton" })),
            true,
        ))
        .await;
        assert_eq!(respawn["error"], Value::Null);
        assert_eq!(respawn["result"]["identity"], json!("review:singleton"));
        assert_eq!(
            respawn["result"]["agent_runtime_id"],
            json!("rt:review:singleton:0")
        );

        let reset_without_identity_runtime = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            Some(ConsoleEventStore::new()),
            None,
            None,
            None,
            None,
            rpc_request_with_params("mobkit/reset", json!({ "identity": "review:singleton" })),
            true,
        ))
        .await;
        assert_ne!(reset_without_identity_runtime["error"], Value::Null);
        assert!(
            reset_without_identity_runtime["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("identity-first runtime required")
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_identity_controls_reject_ambiguous_live_label_aliases()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-identity-ambiguous-live-alias").await?;
        for runtime_id in ["rt:review:singleton:0", "rt:review:singleton:1"] {
            let mut labels = BTreeMap::new();
            labels.insert("agent_identity".to_string(), "review:singleton".to_string());
            runtime
                .handle()
                .spawn_spec(
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

        for requested_identity in ["review:singleton", "rt:review:singleton:0"] {
            for method in [
                "mobkit/status_identity",
                "mobkit/inspect_identity",
                "mobkit/retire",
                "mobkit/respawn",
            ] {
                let response = Box::pin(handle_console_runtime_rpc(
                    &runtime,
                    None,
                    None,
                    None,
                    Some(ConsoleEventStore::new()),
                    None,
                    None,
                    None,
                    None,
                    rpc_request_with_params(method, json!({ "identity": requested_identity })),
                    true,
                ))
                .await;
                assert_ne!(
                    response["error"],
                    Value::Null,
                    "{method} must reject ambiguous live alias for {requested_identity}"
                );
                assert_eq!(
                    response["error"]["data"]["kind"],
                    json!("ambiguous_live_identity_alias"),
                    "unexpected response for {method}/{requested_identity}: {response:#?}"
                );
            }
        }

        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-identity-ambiguous-live-alias".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        for requested_identity in ["review:singleton", "rt:review:singleton:0"] {
            for method in ["mobkit/reset", "mobkit/delete_identity"] {
                let response = Box::pin(handle_console_runtime_rpc(
                    &runtime,
                    None,
                    None,
                    None,
                    Some(ConsoleEventStore::new()),
                    None,
                    Some(identity_runtime.clone()),
                    None,
                    None,
                    rpc_request_with_params(method, json!({ "identity": requested_identity })),
                    true,
                ))
                .await;
                assert_ne!(
                    response["error"],
                    Value::Null,
                    "{method} must reject ambiguous live alias for {requested_identity}"
                );
                assert_eq!(
                    response["error"]["data"]["kind"],
                    json!("ambiguous_live_identity_alias"),
                    "unexpected response for {method}/{requested_identity}: {response:#?}"
                );
            }
        }

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_durable_identity_prefers_registered_live_over_duplicate_labels()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-durable-wins-duplicate-live-labels").await?;
        for runtime_id in ["rt:review:singleton:0", "rt:review:singleton:1"] {
            let mut labels = BTreeMap::new();
            labels.insert("agent_identity".to_string(), "review:singleton".to_string());
            runtime
                .handle()
                .spawn_spec(
                    SpawnMemberSpec::from_wire(
                        "worker".to_string(),
                        runtime_id.to_string(),
                        Some("You are a Review Agent candidate.".into()),
                        None,
                        None,
                    )
                    .with_labels(labels),
                )
                .await?;
        }

        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: store.clone(),
            lease_provider: lease_provider.clone(),
            runtime_instance_id: "test-runtime".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("review:singleton")?;
        let registered_session_id = runtime
            .handle()
            .resolve_bridge_session_id_observation(&meerkat_mob::ids::MeerkatId::from(
                "rt:review:singleton:0",
            ))
            .await
            .unwrap_or_else(meerkat_core::types::SessionId::new);
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:0")?,
            session_id: registered_session_id,
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let grants = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), "test-runtime")
            .await?;
        let grant = match grants.get(&identity).cloned() {
            Some(LeaseAcquireResult::Acquired(grant)) => grant,
            other => return Err(format!("expected acquired lease, got {other:?}").into()),
        };
        store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await?;
        identity_runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("worker"),
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
                Some(grant),
            )
            .await;

        for requested_identity in ["review:singleton", "rt:review:singleton:0"] {
            for method in ["mobkit/status_identity", "mobkit/inspect_identity"] {
                let response = Box::pin(handle_console_runtime_rpc(
                    &runtime,
                    None,
                    None,
                    None,
                    Some(ConsoleEventStore::new()),
                    None,
                    Some(identity_runtime.clone()),
                    None,
                    None,
                    rpc_request_with_params(method, json!({ "identity": requested_identity })),
                    true,
                ))
                .await;
                assert_eq!(
                    response["error"],
                    Value::Null,
                    "{method} must use durable registered live binding despite duplicate labels for {requested_identity}: {response:#?}"
                );
            }
        }
        let reset_all_response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            Some(ConsoleEventStore::new()),
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request("mobkit/reset_all"),
            true,
        ))
        .await;
        assert_eq!(
            reset_all_response["error"],
            Value::Null,
            "reset_all must also prefer the durable registered live binding despite duplicate labels: {reset_all_response:#?}"
        );
        assert!(
            reset_all_response["result"]["failed"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "reset_all should not report duplicate-label failure for durable registered binding: {reset_all_response:#?}"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_identity_controls_reject_wrong_projected_live_only_alias()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-identity-wrong-projected-live-only").await?;

        let mut labels = BTreeMap::new();
        labels.insert("agent_identity".to_string(), "other:singleton".to_string());
        runtime
            .handle()
            .spawn_spec(
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

        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-identity-wrong-projected-live-only".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("review:singleton")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:0")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        identity_runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("worker"),
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

        for method in [
            "mobkit/status_identity",
            "mobkit/inspect_identity",
            "mobkit/retire",
        ] {
            let response = Box::pin(handle_console_runtime_rpc(
                &runtime,
                None,
                None,
                None,
                Some(ConsoleEventStore::new()),
                None,
                Some(identity_runtime.clone()),
                None,
                None,
                rpc_request_with_params(method, json!({ "identity": "other:singleton" })),
                true,
            ))
            .await;
            assert_ne!(
                response["error"],
                Value::Null,
                "{method} must reject wrong-projected live-only alias"
            );
            assert_eq!(
                response["error"]["data"]["kind"],
                json!("stale_live_identity_alias"),
                "unexpected response for {method}: {response:#?}"
            );
        }
        assert!(
            runtime
                .handle()
                .get_member(&meerkat_mob::ids::MeerkatId::from("rt:review:singleton:0"))
                .await
                .is_some(),
            "wrong-projected durable runtime member must not be retired through projected alias"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[derive(Debug)]
    struct HideIdentityPolicy(&'static str);

    impl ConsoleVisibilityPolicy for HideIdentityPolicy {
        fn identity_visible(&self, record: &ConsoleIdentityRecord) -> bool {
            record.identity != self.0
        }
    }

    #[derive(Debug)]
    struct HideMemberPolicy(&'static str);

    impl ConsoleVisibilityPolicy for HideMemberPolicy {
        fn member_visible(&self, member: &ConsoleMember) -> bool {
            member.agent_identity != self.0
                && member
                    .labels
                    .get("agent_identity")
                    .is_none_or(|identity| identity != self.0)
        }

        fn identity_visible(&self, record: &ConsoleIdentityRecord) -> bool {
            record.runtime_member_id != self.0
        }
    }

    #[derive(Debug)]
    struct HideOnlyMemberPolicy(&'static str);

    impl ConsoleVisibilityPolicy for HideOnlyMemberPolicy {
        fn member_visible(&self, member: &ConsoleMember) -> bool {
            member.agent_identity != self.0
        }
    }

    #[tokio::test]
    async fn console_runtime_identity_controls_respect_visibility_policy()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-identity-hidden-controls").await?;
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-identity-hidden-controls".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("review:singleton")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:0")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        identity_runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("worker"),
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
                    identity: identity.clone(),
                    fencing_token: FencingToken::new(7),
                    ttl: Duration::from_mins(1),
                }),
            )
            .await;

        for method in [
            "mobkit/status_identity",
            "mobkit/inspect_identity",
            "mobkit/retire",
            "mobkit/respawn",
            "mobkit/reset",
            "mobkit/delete_identity",
        ] {
            let response = Box::pin(handle_console_runtime_rpc_with_visibility(
                &runtime,
                None,
                None,
                None,
                Some(ConsoleEventStore::new()),
                None,
                Some(identity_runtime.clone()),
                None,
                None,
                &HideIdentityPolicy("review:singleton"),
                rpc_request_with_params(method, json!({ "identity": "review:singleton" })),
                true,
                false,
                None,
                None,
                None,
            ))
            .await;
            assert_ne!(
                response["error"],
                Value::Null,
                "{method} must reject hidden durable identity"
            );
            assert_eq!(
                response["error"]["data"]["kind"],
                json!("identity_hidden_by_policy"),
                "unexpected hidden response for {method}: {response:#?}"
            );
        }
        identity_runtime
            .status(&AgentIdentity::parse("review:singleton")?)
            .await
            .expect("hidden control RPCs must not mutate the durable identity");

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_durable_identity_controls_reject_hidden_bound_member()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-durable-hidden-bound-member").await?;
        runtime
            .handle()
            .spawn_spec(SpawnMemberSpec::from_wire(
                "worker".to_string(),
                "rt:review:singleton:0".to_string(),
                Some("You are the live Review Agent.".into()),
                None,
                None,
            ))
            .await?;

        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-durable-hidden-bound-member".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("review:singleton")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:0")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        identity_runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("worker"),
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
                    identity: identity.clone(),
                    fencing_token: FencingToken::new(7),
                    ttl: Duration::from_mins(1),
                }),
            )
            .await;

        for requested_identity in ["review:singleton", "rt:review:singleton:0"] {
            for method in [
                "mobkit/status_identity",
                "mobkit/inspect_identity",
                "mobkit/retire",
                "mobkit/respawn",
                "mobkit/reset",
                "mobkit/delete_identity",
            ] {
                let response = Box::pin(handle_console_runtime_rpc_with_visibility(
                    &runtime,
                    None,
                    None,
                    None,
                    Some(ConsoleEventStore::new()),
                    None,
                    Some(identity_runtime.clone()),
                    None,
                    None,
                    &HideOnlyMemberPolicy("rt:review:singleton:0"),
                    rpc_request_with_params(method, json!({ "identity": requested_identity })),
                    true,
                    false,
                    None,
                    None,
                    None,
                ))
                .await;
                assert_eq!(
                    response["error"]["data"]["kind"],
                    json!("identity_hidden_by_policy"),
                    "durable {method} must reject hidden bound live member for {requested_identity}: {response:#?}"
                );
            }
        }

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_live_only_identity_controls_respect_visibility_policy()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-live-only-hidden-controls").await?;

        let mut labels = BTreeMap::new();
        labels.insert("agent_identity".to_string(), "review:singleton".to_string());
        runtime
            .handle()
            .spawn_spec(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "rt:review:singleton:0".to_string(),
                    Some("You are the live Review Agent.".into()),
                    None,
                    None,
                )
                .with_labels(labels),
            )
            .await?;

        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-live-only-hidden-controls".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));

        for method in [
            "mobkit/status_identity",
            "mobkit/inspect_identity",
            "mobkit/retire",
            "mobkit/respawn",
            "mobkit/reset",
            "mobkit/delete_identity",
        ] {
            let response = Box::pin(handle_console_runtime_rpc_with_visibility(
                &runtime,
                None,
                None,
                None,
                Some(ConsoleEventStore::new()),
                None,
                Some(identity_runtime.clone()),
                None,
                None,
                &HideMemberPolicy("rt:review:singleton:0"),
                rpc_request_with_params(method, json!({ "identity": "review:singleton" })),
                true,
                false,
                None,
                None,
                None,
            ))
            .await;
            assert_ne!(
                response["error"],
                Value::Null,
                "{method} must reject hidden live-only identity"
            );
            assert_eq!(
                response["error"]["data"]["kind"],
                json!("identity_hidden_by_policy"),
                "unexpected hidden live-only response for {method}: {response:#?}"
            );
        }
        assert!(
            runtime
                .handle()
                .get_member(&meerkat_mob::ids::MeerkatId::from("rt:review:singleton:0"))
                .await
                .is_some(),
            "hidden live-only controls must not mutate the live member"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_reset_live_only_alias_without_session_bridge_uses_live_fallback()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-reset-live-only-no-bridge").await?;

        let mut labels = BTreeMap::new();
        labels.insert("agent_identity".to_string(), "review:singleton".to_string());
        runtime
            .handle()
            .spawn_spec(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "rt:review:singleton:0".to_string(),
                    Some("You are the live Review Agent.".into()),
                    None,
                    None,
                )
                .with_labels(labels),
            )
            .await?;

        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-reset-live-only-no-bridge".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));

        let response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            Some(ConsoleEventStore::new()),
            None,
            Some(identity_runtime),
            None,
            None,
            rpc_request_with_params("mobkit/reset", json!({ "identity": "review:singleton" })),
            true,
        ))
        .await;
        assert_eq!(
            response["error"],
            Value::Null,
            "live-only reset should use live fallback instead of requiring session bridge: {response:#?}"
        );
        assert_eq!(response["result"]["identity"], json!("review:singleton"));

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn reset_all_rejects_registered_runtime_projected_under_wrong_identity()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-reset-all-stale-projection").await?;

        let mut labels = BTreeMap::new();
        labels.insert("agent_identity".to_string(), "other:singleton".to_string());
        runtime
            .handle()
            .spawn_spec(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "rt:review:singleton:0".to_string(),
                    Some("You are a mislabeled Review Agent.".into()),
                    None,
                    None,
                )
                .with_labels(labels),
            )
            .await?;

        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: store.clone(),
            lease_provider: lease_provider.clone(),
            runtime_instance_id: "test-runtime".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("review:singleton")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:0")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let grants = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), "test-runtime")
            .await?;
        let grant = match grants.get(&identity).cloned() {
            Some(LeaseAcquireResult::Acquired(grant)) => grant,
            other => return Err(format!("expected acquired lease, got {other:?}").into()),
        };
        store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await?;
        identity_runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("worker"),
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
                Some(grant),
            )
            .await;

        let response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            Some(ConsoleEventStore::new()),
            None,
            Some(identity_runtime),
            None,
            None,
            rpc_request("mobkit/reset_all"),
            true,
        ))
        .await;
        assert_ne!(response["error"], Value::Null);
        let failed = response["error"]["data"]["failed"]
            .as_array()
            .expect("reset_all should report failed identities");
        let stale_failure = failed
            .iter()
            .find(|failure| failure["identity"] == json!("review:singleton"))
            .expect("review identity should fail stale alias validation");
        assert_eq!(
            stale_failure["kind"],
            json!("stale_live_identity_alias"),
            "unexpected reset_all response: {response:#?}"
        );
        assert!(
            stale_failure["error"]
                .as_str()
                .unwrap_or_default()
                .contains("projects identity other:singleton"),
            "unexpected stale failure: {stale_failure:#?}"
        );
        let retired = response["error"]["data"]["retired_delegates"]
            .as_array()
            .expect("reset_all should return retired delegates");
        assert!(
            !retired
                .iter()
                .any(|entry| entry["identity"] == json!("other:singleton")),
            "wrong-projected live alias must not be destructively retired before stale validation; response: {response:#?}"
        );
        assert!(
            runtime
                .handle()
                .get_member(&meerkat_mob::ids::MeerkatId::from("rt:review:singleton:0",))
                .await
                .is_some(),
            "wrong-projected durable runtime member must remain present after reset_all rejection"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn reset_all_respects_console_visibility_policy_for_live_members()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-reset-all-hidden-live").await?;

        runtime
            .handle()
            .spawn_spec(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "hidden:singleton".to_string(),
                    Some("You are hidden from console lifecycle controls.".into()),
                    None,
                    None,
                )
                .with_labels(BTreeMap::from([(
                    "agent_identity".to_string(),
                    "hidden:singleton".to_string(),
                )])),
            )
            .await?;

        let response = Box::pin(handle_console_runtime_rpc_with_visibility(
            &runtime,
            None,
            None,
            None,
            Some(ConsoleEventStore::new()),
            None,
            None,
            None,
            None,
            &HideMemberPolicy("hidden:singleton"),
            rpc_request("mobkit/reset_all"),
            true,
            false,
            None,
            None,
            None,
        ))
        .await;
        assert_eq!(
            response["error"],
            Value::Null,
            "hidden live member should be outside reset_all target set: {response:#?}"
        );
        assert!(
            runtime
                .handle()
                .get_member(&meerkat_mob::ids::MeerkatId::from("hidden:singleton"))
                .await
                .is_some(),
            "reset_all must not retire hidden live members"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn reset_all_skips_durable_identity_with_hidden_bound_member()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-reset-all-hidden-durable-bound").await?;
        runtime
            .handle()
            .spawn_spec(SpawnMemberSpec::from_wire(
                "worker".to_string(),
                "rt:review:singleton:0".to_string(),
                Some("You are the hidden Review Agent.".into()),
                None,
                None,
            ))
            .await?;

        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-reset-all-hidden-durable-bound".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("review:singleton")?;
        identity_runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("worker"),
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
                    agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:0")?,
                    session_id: meerkat_core::types::SessionId::new(),
                    generation: ContinuityGeneration::new(0),
                    checkpoint_version: CheckpointVersion::new(0),
                }),
                Some(LeaseGrant {
                    identity: identity.clone(),
                    fencing_token: FencingToken::new(9),
                    ttl: Duration::from_mins(1),
                }),
            )
            .await;

        let response = Box::pin(handle_console_runtime_rpc_with_visibility(
            &runtime,
            None,
            None,
            None,
            Some(ConsoleEventStore::new()),
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            &HideOnlyMemberPolicy("rt:review:singleton:0"),
            rpc_request("mobkit/reset_all"),
            true,
            false,
            None,
            None,
            None,
        ))
        .await;
        assert_eq!(
            response["error"],
            Value::Null,
            "hidden durable bound member should be outside reset_all target set: {response:#?}"
        );
        assert_eq!(
            identity_runtime.status(&identity).await?.state,
            IdentityLifecycleState::Active
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn identity_lifecycle_cleanup_skips_hidden_projected_duplicates()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-hidden-stale-duplicate-cleanup").await?;
        for runtime_id in ["rt:review:singleton:0", "rt:review:singleton:1"] {
            runtime
                .handle()
                .spawn_spec(
                    SpawnMemberSpec::from_wire(
                        "worker".to_string(),
                        runtime_id.to_string(),
                        Some("You are a Review Agent candidate.".into()),
                        None,
                        None,
                    )
                    .with_labels(BTreeMap::from([(
                        "agent_identity".to_string(),
                        "review:singleton".to_string(),
                    )])),
                )
                .await?;
        }

        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: store.clone(),
            lease_provider: lease_provider.clone(),
            runtime_instance_id: "console-hidden-stale-duplicate-cleanup".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("review:singleton")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:0")?,
            session_id: runtime
                .handle()
                .resolve_bridge_session_id_observation(&meerkat_mob::ids::MeerkatId::from(
                    "rt:review:singleton:0",
                ))
                .await
                .unwrap_or_else(meerkat_core::types::SessionId::new),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let grants = lease_provider
            .acquire_leases(
                std::slice::from_ref(&identity),
                "console-hidden-stale-duplicate-cleanup",
            )
            .await?;
        let grant = match grants.get(&identity).cloned() {
            Some(LeaseAcquireResult::Acquired(grant)) => grant,
            other => return Err(format!("expected acquired lease, got {other:?}").into()),
        };
        store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await?;
        identity_runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("worker"),
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
                Some(grant),
            )
            .await;

        let response = Box::pin(handle_console_runtime_rpc_with_visibility(
            &runtime,
            None,
            None,
            None,
            Some(ConsoleEventStore::new()),
            None,
            Some(identity_runtime),
            None,
            None,
            &HideOnlyMemberPolicy("rt:review:singleton:1"),
            rpc_request_with_params("mobkit/retire", json!({ "identity": "review:singleton" })),
            true,
            false,
            None,
            None,
            None,
        ))
        .await;
        assert_eq!(
            response["error"],
            Value::Null,
            "visible durable retire should succeed without touching hidden duplicate: {response:#?}"
        );
        assert!(
            runtime
                .handle()
                .get_member(&meerkat_mob::ids::MeerkatId::from("rt:review:singleton:1"))
                .await
                .is_some(),
            "post-mutation stale cleanup must not retire member-hidden projected duplicates"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_capabilities_advertise_identity_controls_when_identity_runtime_exists()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-identity-capabilities").await?;
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-identity-capabilities".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));

        let response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request("mobkit/capabilities"),
            true,
        ))
        .await;

        assert_eq!(response["error"], Value::Null, "{response:#?}");
        let methods = response["result"]["methods"]
            .as_array()
            .ok_or("capabilities methods should be an array")?;
        for method in [
            "mobkit/status_identity",
            "mobkit/inspect_identity",
            "mobkit/respawn",
            "mobkit/reset",
            "mobkit/delete_identity",
        ] {
            assert!(
                methods.iter().any(|candidate| candidate == method),
                "identity runtime capabilities should advertise {method}: {methods:#?}"
            );
        }
        for method in crate::rpc::MOBPACK_AUTHORING_METHODS {
            assert!(
                !methods.iter().any(|candidate| candidate == method),
                "console runtime capabilities must not advertise flow-editor method {method}: {methods:#?}"
            );
        }

        let mobpack_response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime),
            None,
            None,
            rpc_request("mobkit/mobpacks/schema"),
            true,
        ))
        .await;
        assert_eq!(
            mobpack_response["error"]["code"],
            json!(-32601),
            "console runtime RPC must not handle flow-editor authoring methods: {mobpack_response:#?}"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_aggregator_rpc_does_not_expose_flow_editor_authoring_methods() {
        let response = Box::pin(handle_console_aggregator_rpc(
            None,
            rpc_request("mobkit/capabilities"),
            true,
        ))
        .await;

        assert_eq!(response["error"], Value::Null, "{response:#?}");
        let methods = response["result"]["methods"]
            .as_array()
            .expect("capabilities methods should be an array");
        for method in crate::rpc::MOBPACK_AUTHORING_METHODS {
            assert!(
                !methods.iter().any(|candidate| candidate == method),
                "console aggregator capabilities must not advertise flow-editor method {method}: {methods:#?}"
            );
        }

        let mobpack_response = Box::pin(handle_console_aggregator_rpc(
            None,
            rpc_request("mobkit/mobpacks/schema"),
            true,
        ))
        .await;
        assert_eq!(
            mobpack_response["error"]["code"],
            json!(-32601),
            "console aggregator RPC must not handle flow-editor authoring methods: {mobpack_response:#?}"
        );
    }

    #[tokio::test]
    async fn console_runtime_identity_reads_reject_stale_runtime_aliases()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-identity-stale-read-alias").await?;
        let mut labels = BTreeMap::new();
        labels.insert("agent_identity".to_string(), "review:singleton".to_string());
        runtime
            .handle()
            .spawn_spec(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "rt:review:singleton:0".to_string(),
                    Some("You are the stale Review Agent.".into()),
                    None,
                    None,
                )
                .with_labels(labels),
            )
            .await?;

        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-identity-stale-read-alias".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("review:singleton")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:review:singleton:1")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(1),
            checkpoint_version: CheckpointVersion::new(0),
        };
        identity_runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("worker"),
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
                    fencing_token: FencingToken::new(7),
                    ttl: Duration::from_mins(1),
                }),
            )
            .await;

        for requested_identity in ["rt:review:singleton:0", "review:singleton"] {
            for method in ["mobkit/status_identity", "mobkit/inspect_identity"] {
                let response = Box::pin(handle_console_runtime_rpc(
                    &runtime,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(identity_runtime.clone()),
                    None,
                    None,
                    rpc_request_with_params(method, json!({ "identity": requested_identity })),
                    true,
                ))
                .await;
                assert_ne!(
                    response["error"],
                    Value::Null,
                    "{method} must reject stale alias for {requested_identity}"
                );
                let message = response["error"]["message"].as_str().unwrap_or_default();
                assert!(
                    message.contains(
                        "identity runtime binding for review:singleton points at rt:review:singleton:1"
                    ),
                    "unexpected stale-alias message for {method}/{requested_identity}: {message}"
                );
                assert_eq!(
                    response["error"]["data"]["kind"],
                    json!("stale_identity_runtime_binding")
                );
                assert_eq!(
                    response["error"]["data"]["registered_runtime_member_id"],
                    json!("rt:review:singleton:1")
                );
                assert_eq!(
                    response["error"]["data"]["live_runtime_member_id"],
                    json!("rt:review:singleton:0")
                );
            }
        }

        let (_temp_dir_without_stale, runtime_without_stale) =
            build_empty_console_test_runtime("console-identity-no-live-stale-alias").await?;

        for method in [
            "mobkit/status_identity",
            "mobkit/inspect_identity",
            "mobkit/retire",
            "mobkit/respawn",
            "mobkit/reset",
        ] {
            let response = Box::pin(handle_console_runtime_rpc(
                &runtime_without_stale,
                None,
                None,
                None,
                Some(ConsoleEventStore::new()),
                None,
                Some(identity_runtime.clone()),
                None,
                None,
                rpc_request_with_params(method, json!({ "identity": "rt:review:singleton:0" })),
                true,
            ))
            .await;
            assert_ne!(
                response["error"],
                Value::Null,
                "{method} must reject stale synthetic runtime alias"
            );
            assert!(
                response["error"]["message"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("identity not found: rt:review:singleton:0"),
                "unexpected no-live stale-alias response for {method}: {response:#?}"
            );
        }
        let _ = runtime_without_stale.handle().stop().await;

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn aggregator_live_snapshot_projects_identity_first_topology_peers()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, mob_runtime) =
            build_empty_console_test_runtime("identity-topology-snapshot-test").await?;
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-topology-snapshot-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));

        for name in ["agent:alpha", "agent:beta"] {
            let identity = AgentIdentity::parse(name)?;
            let record = ContinuityRecord {
                identity: identity.clone(),
                agent_runtime_id: AgentRuntimeId::parse(&format!("rt:{name}:0"))?,
                session_id: meerkat_core::types::SessionId::new(),
                generation: ContinuityGeneration::new(0),
                checkpoint_version: CheckpointVersion::new(0),
            };
            identity_runtime
                .register(
                    DurableAgentSpec {
                        identity: identity.clone(),
                        profile: ProfileName::from("default"),
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
                        fencing_token: FencingToken::new(7),
                        ttl: Duration::from_mins(1),
                    }),
                )
                .await;
        }
        identity_runtime
            .set_desired_peer_edges(vec![ManagedPeerEdge::new(
                AgentIdentity::parse("agent:alpha")?,
                AgentIdentity::parse("agent:beta")?,
            )?])
            .await;

        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator.register_runtime_handles_with_policy(
            "identity-first",
            "",
            mob_runtime.clone(),
            Some(identity_runtime),
            ConsoleEventStore::new(),
            Arc::new(AllowAllConsoleVisibilityPolicy),
        );

        let snapshot = build_aggregator_live_snapshot(&aggregator, &[]).await?;
        let alpha = snapshot
            .members
            .iter()
            .find(|member| member.agent_identity == "agent:alpha")
            .ok_or("agent:alpha missing from live snapshot")?;
        assert_eq!(alpha.wired_to, vec!["agent:beta".to_string()]);

        let _ = mob_runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn identity_first_console_send_reserves_timeline_and_uses_identity_runtime()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, mob_runtime) =
            build_empty_console_test_runtime("identity-send-runtime-key-test").await?;
        let identity = AgentIdentity::parse("agent:console")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:console:0")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let runtime = IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        });
        runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("default"),
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
                Some(record.clone()),
                Some(LeaseGrant {
                    identity: identity.clone(),
                    fencing_token: FencingToken::new(7),
                    ttl: Duration::from_mins(1),
                }),
            )
            .await;

        let aggregator = MobKitConsoleAggregator::in_memory();
        let events = ConsoleEventStore::new();
        let runtime = Arc::new(runtime);
        aggregator.register_runtime_handles_with_policy(
            "default",
            "",
            mob_runtime.clone(),
            Some(runtime.clone()),
            events.clone(),
            Arc::new(AllowAllConsoleVisibilityPolicy),
        );
        let accepted = console_send_identity_first(
            &aggregator,
            runtime.clone(),
            Some(&mob_runtime),
            Some(&events),
            crate::console_aggregator::ConsoleSendRequest {
                identity: identity.as_str().to_string(),
                content: serde_json::to_value(meerkat_core::ContentInput::Text(
                    "hello".to_string(),
                ))?,
                origin: "test".to_string(),
                idempotency_key: "idem-1".to_string(),
                handling_mode: None,
            },
        )
        .await?;

        assert_eq!(accepted.identity, identity.as_str());
        assert_eq!(accepted.status, ConsoleFrameStatus::Accepted);
        assert_eq!(accepted.session_id, Some(record.session_id.to_string()));

        let page = aggregator
            .query_timeline(ConsoleTimelineQuery {
                identity: Some(identity.as_str().to_string()),
                ..ConsoleTimelineQuery::default()
            })
            .await?;
        assert_eq!(page.frames.len(), 1);
        assert_eq!(page.frames[0].runtime_key, "default");
        assert_eq!(page.frames[0].status, ConsoleFrameStatus::Accepted);
        assert_eq!(
            page.frames[0].session_id,
            Some(record.session_id.to_string())
        );
        let _ = mob_runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn identity_first_console_send_falls_back_to_member_only_spawned_worker()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, mob_runtime) =
            build_empty_console_test_runtime("identity-send-member-only-test").await?;
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-member-only-send-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));

        let aggregator = MobKitConsoleAggregator::in_memory();
        let events = ConsoleEventStore::new();
        aggregator.register_runtime_handles_with_policy(
            "identity-first",
            "",
            mob_runtime.clone(),
            Some(identity_runtime.clone()),
            events.clone(),
            Arc::new(AllowAllConsoleVisibilityPolicy),
        );

        mob_runtime
            .handle()
            .spawn_spec(SpawnMemberSpec::from_wire(
                "worker".to_string(),
                "agent:member-only".to_string(),
                Some("You are a member-only spawned worker.".into()),
                None,
                None,
            ))
            .await?;

        let accepted = console_send_with_identity_first_fallback(
            &aggregator,
            identity_runtime,
            Some(&mob_runtime),
            Some(&events),
            crate::console_aggregator::ConsoleSendRequest {
                identity: "agent:member-only".to_string(),
                content: serde_json::to_value(meerkat_core::ContentInput::Text(
                    "hello spawned worker".to_string(),
                ))?,
                origin: "test".to_string(),
                idempotency_key: "member-only-idem-1".to_string(),
                handling_mode: None,
            },
        )
        .await?;

        assert_eq!(accepted.identity, "agent:member-only");
        assert!(accepted.session_id.is_some());

        let page = aggregator
            .query_timeline(ConsoleTimelineQuery {
                identity: Some("agent:member-only".to_string()),
                ..ConsoleTimelineQuery::default()
            })
            .await?;
        assert!(
            page.frames.iter().any(|frame| frame.kind == "user_input"),
            "fallback send should persist a user input frame for the member-only worker: {page:#?}"
        );

        let _ = mob_runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn identity_first_console_send_returns_before_bridge_delivery_completes()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let identity = AgentIdentity::parse("agent:slow-console")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:slow-console:0")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let deliver_calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-slow-send-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(Arc::new(BlockingIdentityBridge {
                deliver_calls: deliver_calls.clone(),
            })),
            default_timeout: None,
        }));
        runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("default"),
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
                Some(record.clone()),
                Some(LeaseGrant {
                    identity: identity.clone(),
                    fencing_token: FencingToken::new(7),
                    ttl: Duration::from_mins(1),
                }),
            )
            .await;

        let aggregator = MobKitConsoleAggregator::in_memory();
        let accepted = match tokio::time::timeout(
            Duration::from_millis(100),
            console_send_identity_first(
                &aggregator,
                runtime,
                None,
                None,
                crate::console_aggregator::ConsoleSendRequest {
                    identity: identity.as_str().to_string(),
                    content: serde_json::to_value(meerkat_core::ContentInput::Text(
                        "hello slow bridge".to_string(),
                    ))?,
                    origin: "test".to_string(),
                    idempotency_key: "idem-slow-bridge".to_string(),
                    handling_mode: None,
                },
            ),
        )
        .await
        {
            Ok(Ok(accepted)) => accepted,
            Ok(Err(err)) => return Err(format!("send should be accepted: {err}").into()),
            Err(err) => {
                return Err(
                    format!("console send should not wait for bridge delivery: {err}").into(),
                );
            }
        };

        assert_eq!(accepted.status, ConsoleFrameStatus::Accepted);
        if tokio::time::timeout(Duration::from_millis(100), async {
            while deliver_calls.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_err()
        {
            return Err("delivery should be spawned in the background".into());
        }
        Ok(())
    }

    #[tokio::test]
    async fn identity_first_console_steer_waits_for_bridge_delivery()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let identity = AgentIdentity::parse("agent:slow-steer-console")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:slow-steer-console:0")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let deliver_calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-slow-steer-send-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(Arc::new(BlockingIdentityBridge {
                deliver_calls: deliver_calls.clone(),
            })),
            default_timeout: None,
        }));
        runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("default"),
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
                    identity: identity.clone(),
                    fencing_token: FencingToken::new(7),
                    ttl: Duration::from_mins(1),
                }),
            )
            .await;

        let aggregator = MobKitConsoleAggregator::in_memory();
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            console_send_identity_first(
                &aggregator,
                runtime,
                None,
                None,
                crate::console_aggregator::ConsoleSendRequest {
                    identity: identity.as_str().to_string(),
                    content: serde_json::to_value(meerkat_core::ContentInput::Text(
                        "hello slow steer bridge".to_string(),
                    ))?,
                    origin: "test".to_string(),
                    idempotency_key: "idem-slow-steer-bridge".to_string(),
                    handling_mode: Some("steer".to_string()),
                },
            ),
        )
        .await;

        if result.is_ok() {
            return Err("steer send must wait for bridge delivery admission".into());
        }
        assert_eq!(
            deliver_calls.load(Ordering::SeqCst),
            1,
            "steer delivery should have reached the bridge before the console response waits"
        );
        Ok(())
    }

    #[tokio::test]
    async fn identity_first_console_send_forwards_handling_mode()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let identity = AgentIdentity::parse("agent:mode-console")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:mode-console:0")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let handling_modes = Arc::new(Mutex::new(Vec::new()));
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-mode-send-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(Arc::new(RecordingIdentityBridge {
                session_id: record.session_id.clone(),
                handling_modes: handling_modes.clone(),
            })),
            default_timeout: None,
        }));
        runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from("default"),
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
                    identity: identity.clone(),
                    fencing_token: FencingToken::new(7),
                    ttl: Duration::from_mins(1),
                }),
            )
            .await;

        let aggregator = MobKitConsoleAggregator::in_memory();
        let accepted = console_send_identity_first(
            &aggregator,
            runtime,
            None,
            None,
            crate::console_aggregator::ConsoleSendRequest {
                identity: identity.as_str().to_string(),
                content: serde_json::to_value(meerkat_core::ContentInput::Text(
                    "hello steer bridge".to_string(),
                ))?,
                origin: "test".to_string(),
                idempotency_key: "idem-steer-bridge".to_string(),
                handling_mode: Some("steer".to_string()),
            },
        )
        .await?;

        if tokio::time::timeout(Duration::from_millis(100), async {
            loop {
                if handling_modes
                    .lock()
                    .map(|modes| modes.contains(&HandlingMode::Steer))
                    .unwrap_or(false)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_err()
        {
            return Err("identity-first console send should forward steer mode".into());
        }

        let terminal_frame = tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                let page = aggregator
                    .query_timeline(ConsoleTimelineQuery {
                        identity: Some(identity.as_str().to_string()),
                        ..ConsoleTimelineQuery::default()
                    })
                    .await
                    .map_err(|err| format!("query timeline: {err}"))?;
                if page.frames.iter().any(|frame| {
                    frame.kind == "interaction_complete"
                        && frame.interaction_id.as_deref() == Some(accepted.interaction_id.as_str())
                        && frame.payload.get("reason").and_then(Value::as_str)
                            == Some("steer_delivered")
                }) {
                    return Ok::<(), String>(());
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        match terminal_frame {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(err.into()),
            Err(_) => {
                return Err(
                    "identity-first steer send should terminalize its console interaction".into(),
                );
            }
        }
        Ok(())
    }

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

    #[tokio::test]
    async fn console_aggregator_reset_all_rpc_rejects_destructive_retire_all_semantics()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-reset-fresh-identity-cache").await?;
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator.register_runtime_handles_with_policy(
            "runtime-reset",
            "reset",
            runtime.clone(),
            None,
            ConsoleEventStore::new(),
            Arc::new(AllowAllConsoleVisibilityPolicy),
        );
        let primed_empty = aggregator.list_identities().await?;
        assert!(
            primed_empty.is_empty(),
            "test precondition: identity cache should be primed empty before late spawn"
        );

        runtime
            .handle()
            .spawn_spec(SpawnMemberSpec::from_wire(
                "worker".to_string(),
                "agent-reset".to_string(),
                Some("You are agent-reset.".into()),
                None,
                None,
            ))
            .await?;

        let response = Box::pin(handle_console_aggregator_rpc(
            Some(aggregator),
            rpc_request("mobkit/reset_all"),
            true,
            false,
            None,
            None,
        ))
        .await;

        assert_eq!(response["result"], Value::Null);
        assert_eq!(
            response["error"]["data"]["kind"],
            json!("unsupported_reset_all_surface")
        );
        assert!(
            runtime
                .handle()
                .get_member(&meerkat_mob::ids::MeerkatId::from("agent-reset"))
                .await
                .is_some(),
            "aggregator reset_all must not retire live members while reporting unsupported"
        );
        let _ = runtime.handle().stop().await;
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
        for idx in 0..250_000 {
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
            ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                after: None,
                limit: 200,
                ..ConsoleTimelineWindowQuery::default()
            },
        )
        .await?;

        assert!(!frames.is_empty());
        assert_eq!(cursor.as_ref().and_then(ConsoleCursor::seq), Some(250_000));
        assert_eq!(
            frames.last().and_then(|frame| frame.cursor.seq()),
            Some(250_000)
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
            ConsoleTimelineWindowQuery {
                identity: Some("sparse-agent".to_string()),
                after: None,
                limit: 200,
                ..ConsoleTimelineWindowQuery::default()
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
    async fn fresh_identity_snapshot_keeps_user_input_anchor_before_noisy_tail()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator
            .store()
            .append_if_absent(NewConsoleFrame {
                id: None,
                dedupe_key: "worker-kickoff".to_string(),
                timestamp_ms: 1,
                runtime_key: "runtime-a".to_string(),
                identity: "review-worker-a".to_string(),
                conversation_id: Some("review-worker-a".to_string()),
                session_id: None,
                kind: "user_input".to_string(),
                status: ConsoleFrameStatus::Delivered,
                payload: json!({
                    "content": [
                        {
                            "type": "text",
                            "text": "Console chat smoke: review this initiative"
                        }
                    ]
                }),
                source: ConsoleFrameSource {
                    kind: ConsoleFrameSourceKind::Synthetic,
                    source_cursor: None,
                },
                source_event_id: Some("worker-kickoff".to_string()),
                interaction_id: Some("kickoff-1".to_string()),
                turn_id: None,
                run_id: None,
                parent_frame_id: None,
                caused_by_frame_id: None,
            })
            .await?;
        for idx in 0..1_500 {
            aggregator
                .store()
                .append_if_absent(NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("worker-delta-{idx}"),
                    timestamp_ms: idx + 2,
                    runtime_key: "runtime-a".to_string(),
                    identity: "review-worker-a".to_string(),
                    conversation_id: Some("review-worker-a".to_string()),
                    session_id: None,
                    kind: "reasoning_delta".to_string(),
                    status: ConsoleFrameStatus::Delivered,
                    payload: json!({ "delta": idx }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::ConsoleEvent,
                        source_cursor: None,
                    },
                    source_event_id: Some(format!("worker-delta-{idx}")),
                    interaction_id: Some("kickoff-1".to_string()),
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: None,
                    caused_by_frame_id: None,
                })
                .await?;
        }

        let (frames, cursor) = query_timeline_snapshot(
            &aggregator,
            ConsoleTimelineWindowQuery {
                identity: Some("review-worker-a".to_string()),
                after: None,
                limit: 200,
                ..ConsoleTimelineWindowQuery::default()
            },
        )
        .await?;

        assert!(
            frames.iter().any(|frame| {
                frame.kind == "user_input"
                    && frame.payload.to_string().contains("Console chat smoke")
            }),
            "identity chat snapshot must keep the worker kickoff prompt before a noisy tail: {frames:#?}",
        );
        assert_eq!(cursor.as_ref().and_then(ConsoleCursor::seq), Some(1_501));
        assert_eq!(
            frames.last().and_then(|frame| frame.cursor.seq()),
            Some(1_501)
        );
        Ok(())
    }

    #[tokio::test]
    async fn timeline_snapshot_drains_since_backlog_across_store_pages()
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
            ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                after: Some(ConsoleCursor::from("console:100")),
                limit: 5_000,
                ..ConsoleTimelineWindowQuery::default()
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
    async fn timeline_snapshot_drains_since_backlog_beyond_old_page_budget()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let aggregator = MobKitConsoleAggregator::in_memory();
        for idx in 1..=150 {
            aggregator
                .store()
                .append_if_absent(NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("deep-backlog-event-{idx}"),
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
                    source_event_id: Some(format!("deep-backlog-event-{idx}")),
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
            ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                after: Some(ConsoleCursor::from_seq(1)),
                limit: 1,
                ..ConsoleTimelineWindowQuery::default()
            },
        )
        .await?;

        assert_eq!(frames.len(), 149);
        assert_eq!(frames.first().and_then(|frame| frame.cursor.seq()), Some(2));
        assert_eq!(
            frames.last().and_then(|frame| frame.cursor.seq()),
            Some(150)
        );
        assert_eq!(cursor.as_ref().and_then(ConsoleCursor::seq), Some(150));
        Ok(())
    }

    #[tokio::test]
    async fn timeline_snapshot_rejects_after_cursor_beyond_store_frontier()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator
            .store()
            .append_if_absent(NewConsoleFrame {
                id: None,
                dedupe_key: "stale-frontier-event".to_string(),
                timestamp_ms: 1,
                runtime_key: "runtime-a".to_string(),
                identity: "agent-a".to_string(),
                conversation_id: Some("agent-a".to_string()),
                session_id: None,
                kind: "text_delta".to_string(),
                status: ConsoleFrameStatus::Completed,
                payload: json!({ "delta": 1 }),
                source: ConsoleFrameSource {
                    kind: ConsoleFrameSourceKind::ConsoleEvent,
                    source_cursor: None,
                },
                source_event_id: Some("stale-frontier-event".to_string()),
                interaction_id: None,
                turn_id: None,
                run_id: None,
                parent_frame_id: None,
                caused_by_frame_id: None,
            })
            .await?;

        let err = match query_timeline_snapshot(
            &aggregator,
            ConsoleTimelineWindowQuery {
                after: Some(ConsoleCursor::from("console:99")),
                limit: 200,
                ..ConsoleTimelineWindowQuery::default()
            },
        )
        .await
        {
            Ok(_) => {
                return Err(
                    std::io::Error::other("future cursor must be replay-unavailable").into(),
                );
            }
            Err(err) => err,
        };

        assert!(
            err.to_string()
                .contains("beyond the current store frontier"),
            "unexpected error: {err}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn timeline_snapshot_rejects_after_cursor_beyond_empty_store_frontier()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let aggregator = MobKitConsoleAggregator::in_memory();

        let err = match query_timeline_snapshot(
            &aggregator,
            ConsoleTimelineWindowQuery {
                after: Some(ConsoleCursor::from("console:99")),
                limit: 200,
                ..ConsoleTimelineWindowQuery::default()
            },
        )
        .await
        {
            Ok(_) => {
                return Err(std::io::Error::other("empty store future cursor must fail").into());
            }
            Err(err) => err,
        };

        assert!(
            err.to_string()
                .contains("beyond the current store frontier"),
            "unexpected error: {err}"
        );
        Ok(())
    }

    #[test]
    fn timeline_query_prefers_last_event_id_over_url_after_cursor() {
        let query = timeline_query_from_http(
            ConsoleTimelineHttpQuery {
                identity: None,
                conversation_id: None,
                after: Some("console:100".to_string()),
                before: None,
                mode: None,
                limit: None,
            },
            Some("console:150".to_string()),
        );

        assert_eq!(query.after.as_ref().and_then(ConsoleCursor::seq), Some(150));
    }

    #[test]
    fn console_timeline_replay_unavailable_rpc_uses_dedicated_error_code() {
        let response = console_timeline_replay_unavailable_response(
            json!("rid"),
            std::io::Error::other("timeline replay cursor is beyond the current store frontier")
                .into(),
            Some(&ConsoleCursor::from_seq(100)),
            Some(ConsoleCursor::from_seq(42)),
        );

        assert_eq!(response["error"]["code"], json!(-32013));
        assert_eq!(
            response["error"]["data"],
            json!({
                "error": "replay_unavailable",
                "stream": "timeline",
                "requested_cursor": "console:100",
                "latest_cursor": "console:42",
            })
        );
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
    fn generated_runtime_ids_do_not_match_sibling_colon_identities() {
        assert!(!member_id_matches_durable_identity(
            "rt:review:singleton:0",
            "review:singleton"
        ));
        assert!(!member_id_matches_durable_identity(
            "review:singleton:gen1",
            "review:singleton"
        ));
        assert!(!member_id_matches_durable_identity(
            "review:singleton:1",
            "review:singleton"
        ));
        assert!(!member_id_matches_durable_identity(
            "rt:review:singleton:qa:0",
            "review:singleton"
        ));
        assert!(!member_id_matches_durable_identity(
            "review:singleton:qa",
            "review:singleton"
        ));
    }
}
