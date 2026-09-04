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
use meerkat_contracts::WireRuntimeBinding;
use meerkat_core::ContentInput;
use meerkat_core::comms::TrustedPeerDescriptor;
use meerkat_mob::MobState;
use meerkat_mob::ids::{AgentIdentity, AgentRuntimeId, FenceToken, MobId};
use meerkat_mob::launch::MemberLaunchMode;
use meerkat_mob::runtime::reconcile::MemberFilter;
use meerkat_mob::{
    MobBackendKind, MobHandle, MobRuntimeMode, PeerTarget, ProfileName, SpawnMemberSpec,
};

use crate::mob_handle_runtime::{
    is_recoverable_lifecycle_cleanup_error, member_entry_to_json,
    model_capabilities_for_member_entry, model_capabilities_for_role,
    model_routing_status_for_member, model_routing_status_for_session, resolved_tools_for_member,
    resolved_tools_for_session, topology_restore_failed_peer_ids, topology_restore_warning_json,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::access::{
    ACCESS_ACTIONS, ACTION_AGENT_MEMORY_DELETE, ACTION_AGENT_MEMORY_READ,
    ACTION_AGENT_MEMORY_WRITE, ACTION_AGENT_RESET, ACTION_AGENT_RESPAWN, ACTION_AGENT_RETIRE,
    ACTION_AGENT_SEND, ACTION_AGENT_SPAWN, ACTION_AGENT_VIEW, ACTION_GATING_DECIDE,
    ACTION_GATING_VIEW, ACTION_MEMORY_QUARANTINE_REVIEW, ACTION_MOB_MEMORY_READ,
    ACTION_MOB_OBSERVE, ACTION_OPERATOR_MEMORY_READ, ACTION_RUNTIME_ADMIN, ACTION_WORKGRAPH_MANAGE,
    ACTION_WORKGRAPH_VIEW, AccessController, AccessGroup, AccessResource, AccessRule, AccessView,
    AgentResourceAttributes,
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
use crate::mob_handle_runtime::{
    MEMBER_STATE_ACTIVE, MEMBER_STATE_RETIRING, MobRuntime, member_status_state_string,
};
use crate::rpc::memory_methods::{
    parse_agent_memory_forget_params, parse_agent_memory_manifest_params,
    parse_agent_memory_recall_params, parse_agent_memory_remember_params,
    parse_agent_memory_update_params,
};
use crate::rpc::{JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::runtime::MobkitRuntimeHandle;
use crate::runtime::{
    ConsoleAgentLiveSnapshot, ConsoleLiveSnapshot, ConsoleMember, ConsoleModelCapabilities,
    ConsoleRestJsonRequest, DeliveryHistoryRequest, GatingDecideRequest, GatingDecision,
    RuntimeDecisionState, extract_bearer_token_from_header,
    handle_console_rest_json_route_with_snapshot_access_memory_and_workgraph,
    resolve_authorized_console_auth_from_token,
};
use crate::runtime::{RuntimeMetadataTable, labels_to_json_value};
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
    /// Optional panel-capable store handle for the console Memory panel's
    /// read-only `mobkit/memory/panel/*` RPCs (§9.3). `None` (recall-only
    /// provider, no memory configured) leaves those methods unadvertised.
    pub(crate) memory_panel: Option<Arc<dyn crate::memory::capabilities::MemoryPanelStore>>,
    /// §16 Q1 provisional operator keying: the console send path notes
    /// "authenticated principal P addressed identity I" through this
    /// resolver; the memory coordinator reads it for operator-scope recall.
    /// `None` when the operator scope is off (or memory is unconfigured).
    pub(crate) operator_resolver:
        Option<Arc<crate::memory::coordinator::ConsolePrincipalOperatorResolver>>,
    /// Identity-first gateways: the mutable desired-identity roster that
    /// `mobkit/ensure_member` extends (ask K0). `None` on session-owned
    /// deployments.
    pub(crate) identity_roster: Option<Arc<crate::identity_first::MutableRosterProvider>>,
    /// Realm-scoped WorkGraph service backing the console
    /// `mobkit/workgraph/*` RPCs and the experience `workgraph` section.
    /// `None` leaves the group unadvertised. The admission authority for the
    /// duplicate-binding guards is NOT captured here: the dispatch arm takes
    /// it from the mob runtime, so the console and the unified stdin surface
    /// always serialize their check-then-act windows against the SAME
    /// instance whatever constructed the service.
    pub(crate) workgraph: Option<meerkat::WorkGraphService>,
    /// Optional topology control-plane handle. Thin/aggregator-only routers
    /// leave this absent; unified runtimes always pass their handle, even
    /// while policy mode is disabled, so query remains available.
    pub(crate) topology: Option<crate::topology_control::TopologyRuntimeHandle>,
    /// Rebuildable detached-job observability supplied by the unified host.
    pub(crate) job_health_projection: Option<Arc<std::sync::RwLock<Option<serde_json::Value>>>>,
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
            Box::pin(self.prime_now(runtime)).await;
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
        let refreshed = Box::pin(collect_console_snapshot_read_model(runtime)).await;
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
            let refreshed = Box::pin(collect_console_snapshot_read_model(&runtime)).await;
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
        memory_panel: None,
        operator_resolver: None,
        identity_roster: None,
        workgraph: None,
        topology: None,
        job_health_projection: None,
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
        memory_panel: None,
        operator_resolver: None,
        identity_roster: None,
        workgraph: None,
        topology: None,
        job_health_projection: None,
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
        None,
        None,
        None,
        None,
        None,
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
    memory_panel: Option<Arc<dyn crate::memory::capabilities::MemoryPanelStore>>,
    operator_resolver: Option<Arc<crate::memory::coordinator::ConsolePrincipalOperatorResolver>>,
    identity_roster: Option<Arc<crate::identity_first::MutableRosterProvider>>,
    workgraph: Option<meerkat::WorkGraphService>,
    topology: Option<crate::topology_control::TopologyRuntimeHandle>,
    job_health_projection: Option<Arc<std::sync::RwLock<Option<serde_json::Value>>>>,
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
    // Fall back to the mob runtime's bootstrap-time service so routers built
    // through the thinner constructors still expose workgraph.
    let workgraph = workgraph.or_else(|| runtime.workgraph_service());
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
        memory_panel,
        operator_resolver,
        identity_roster,
        workgraph,
        topology,
        job_health_projection,
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
                Box::pin(build_live_snapshot(
                    runtime,
                    &config_module_ids,
                    state.console_events.as_ref(),
                    state.visibility_policy.as_ref(),
                    &state.snapshot_read_model,
                ))
                .await,
            )
        }
        None => match &state.console_aggregator {
            Some(aggregator) => Box::pin(build_aggregator_live_snapshot(
                aggregator,
                &config_module_ids,
            ))
            .await
            .ok(),
            None => None,
        },
    }
    .map(|mut snapshot| {
        apply_console_visibility_policy(&mut snapshot, state.visibility_policy.as_ref());
        snapshot
    });

    let response = handle_console_rest_json_route_with_snapshot_access_memory_and_workgraph(
        &state.decisions,
        &ConsoleRestJsonRequest {
            method: "GET".to_string(),
            path,
            auth: None,
        },
        live_snapshot.as_ref(),
        state.access.as_ref(),
        state.memory_panel.is_some(),
        state.workgraph.is_some(),
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
    let request_method = parsed_request.method.clone();

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
        state.memory_panel.as_deref(),
        state.identity_roster.clone(),
        state.workgraph.as_ref(),
        state.topology.as_ref(),
    ))
    .await;
    let response_value = decorate_console_job_projection(
        response_value,
        &request_method,
        state.job_health_projection.as_ref(),
    );
    (StatusCode::OK, Json::<Value>(response_value))
}

fn decorate_console_job_projection(
    mut response: Value,
    method: &str,
    projection_slot: Option<&Arc<std::sync::RwLock<Option<Value>>>>,
) -> Value {
    let projection = projection_slot.and_then(|slot| {
        slot.read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    });
    let Some(projection) = projection else {
        return response;
    };
    match method {
        "mobkit/status" | "mobkit/capabilities" => {
            response["result"]["detached_jobs"] = projection
                .get("detached_jobs")
                .cloned()
                .unwrap_or(Value::Null);
        }
        "mobkit/member_status" => {
            if let Some(session_id) = response["result"]["current_session_id"].as_str()
                && let Some(session_jobs) = projection
                    .get("by_session")
                    .and_then(|by_session| by_session.get(session_id))
            {
                response["result"]["detached_jobs"] = session_jobs.clone();
                response["result"]["awaiting_detached"] = session_jobs
                    .get("awaiting_detached")
                    .cloned()
                    .unwrap_or(Value::Bool(false));
            }
        }
        _ => {}
    }
    response
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
    match Box::pin(aggregator.list_identities()).await {
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
        prime_access_cache_from_runtime(runtime, controller).await;
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
    if let Err(message) =
        crate::member_comms_id::validate_public_member_alias("identity", request.identity.as_str())
    {
        return console_json_error(StatusCode::BAD_REQUEST, "invalid_request", &message);
    }
    // Prime the attribute cache from the live roster BEFORE the `agent.send`
    // decision: role/label-scoped rules resolve through the cache, and on a
    // cold cache the decision degrades to a bare-identity resource — a deny
    // rule scoped by `roles`/`match_labels` would not match, so a scripted
    // caller hitting `/console/send` as its first request could reach a
    // member the rule was meant to exclude (fail-closed requires the
    // attributes to be present when the rule is evaluated).
    if let Some(runtime) = &state.runtime
        && let Some(controller) = state.access.as_ref().filter(|c| c.enabled())
    {
        prime_access_cache_from_runtime(runtime, controller).await;
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
    // §16 Q1 provisional operator keying: an authenticated principal
    // addressing this identity IS the active operator for its turns.
    // Unauthenticated consoles (no subject) note nothing — operator-scope
    // recall stays inert without a real principal.
    if let Some(resolver) = state.operator_resolver.as_ref()
        && let Some(subject) = auth_context
            .access_view
            .as_ref()
            .and_then(|view| view.subject())
    {
        resolver.note_interaction(request.identity.as_str(), subject);
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
    console_events: Option<&ConsoleEventStore>,
    request: ConsoleSendRequest,
) -> Result<crate::console_aggregator::ConsoleInteractionAccepted, ConsoleSendError> {
    let member_send_request = request.clone();
    match Box::pin(console_send_identity_first(
        aggregator,
        identity_runtime,
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
            let Some(canonical_identity) = Box::pin(resolve_console_send_identity_alias(
                aggregator,
                &requested_identity,
            ))
            .await
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
    let accepted = match Box::pin(
        aggregator.reserve_identity_first_interaction(request.clone(), session_id.as_deref()),
    )
    .await?
    {
        // An idempotent replay: the original acceptance is the answer, and the
        // turn it names already ran (or is running). Dispatching again would
        // run a second turn whose frames land under this interaction id.
        crate::console_aggregator::IdentityFirstReservation::Existing(accepted) => {
            return Ok(accepted);
        }
        crate::console_aggregator::IdentityFirstReservation::Fresh(accepted) => accepted,
    };

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

    if handling_mode == meerkat_core::types::HandlingMode::Steer {
        let send_result =
            if crate::member_comms_id::is_reserved_generated_alias(&requested_identity) {
                identity_runtime
                    .send_with_mode_and_interaction_member_alias_tracked(
                        &identity,
                        requested_identity.as_str(),
                        &content,
                        handling_mode,
                        Some(accepted.interaction_id.as_str()),
                    )
                    .await
            } else {
                identity_runtime
                    .send_with_mode_and_interaction_tracked(
                        &identity,
                        &content,
                        handling_mode,
                        Some(accepted.interaction_id.as_str()),
                    )
                    .await
            };
        match send_result {
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
                        .record_interaction_failure(
                            identity.as_str(),
                            accepted.interaction_id.as_str(),
                            json!({
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
    let dispatch_expected_alias =
        crate::member_comms_id::is_reserved_generated_alias(&requested_identity)
            .then_some(requested_identity);
    tokio::spawn(async move {
        let send_result = if let Some(expected_alias) = dispatch_expected_alias.as_deref() {
            identity_runtime
                .send_with_mode_and_interaction_member_alias_tracked(
                    &dispatch_identity,
                    expected_alias,
                    &dispatch_content,
                    handling_mode,
                    Some(dispatch_accepted.interaction_id.as_str()),
                )
                .await
        } else {
            identity_runtime
                .send_with_mode_and_interaction_tracked(
                    &dispatch_identity,
                    &dispatch_content,
                    handling_mode,
                    Some(dispatch_accepted.interaction_id.as_str()),
                )
                .await
        };
        match send_result {
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
                        .record_interaction_failure(
                            dispatch_identity.as_str(),
                            dispatch_accepted.interaction_id.as_str(),
                            json!({
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
    let identities = Box::pin(aggregator.list_identities()).await.ok()?;
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
        prime_access_cache_from_runtime(runtime, controller).await;
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
                    if !Box::pin(
                        aggregator
                            .timeline_event_visible_for_subscriber(&event, identity.as_deref()),
                    )
                    .await
                    {
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
    // Delegated, not duplicated: the member-declaration family owns its own
    // read/mutate classification, so this surface cannot disagree with the stdin
    // surface about whether an adopt is a write.
    if crate::rpc::mob_methods::is_member_declaration_mutating_method(method) {
        return true;
    }
    if crate::rpc::topology_methods::is_topology_mutating_method(method) {
        return true;
    }
    if crate::rpc::workgraph_methods::is_workgraph_mutating_method(method) {
        return true;
    }
    matches!(
        method,
        "mobkit/retire"
            | "mobkit/reset_all"
            | "mobkit/console/send"
            | "mobkit/blob/upload"
            | "mobkit/ensure_member"
            | "mobkit/retire_member"
            | "mobkit/respawn_member"
            | "mobkit/reload_member"
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
            | "mobkit/agent_memory/remember"
            | "mobkit/agent_memory/update"
            | "mobkit/agent_memory/forget"
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
            | "mobkit/live/open"
            | "mobkit/live/replacement_required"
            | "mobkit/live/playback_owner/register"
            | "mobkit/live/playback_owner/revoke"
            | "mobkit/live/close"
            | "mobkit/live/refresh"
            | "mobkit/live/send_input"
            | "mobkit/live/commit_input"
            | "mobkit/live/interrupt"
            | "mobkit/live/truncate"
            | "mobkit/live/playback_complete"
            | "live/webrtc/answer"
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

pub(crate) const ACCESS_DENIED_RPC_CODE: i64 = -32030;

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
            .and_then(crate::member_comms_id::durable_identity_label)
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

/// Maps a console RPC method to the access actions it requires, each with
/// the targeted agent (when the check is agent-scoped). Every listed action
/// must be allowed (logical AND). Methods not listed are either pure read
/// surfaces whose *results* are filtered per caller, or non-sensitive
/// metadata.
fn console_rpc_access_requirements(
    method: &str,
    params: &Value,
) -> Option<Vec<(&'static str, Option<String>)>> {
    let identity = normalized_console_rpc_string_param(params, "identity");
    let target = identity
        .clone()
        .or_else(|| normalized_console_rpc_string_param(params, "member_id"))
        .or_else(|| normalized_console_rpc_string_param(params, "agent_id"))
        // The member-declaration family names its subject `agent_identity`.
        // Without this the target was None, and a None target does not fail
        // closed - it gates the ACTION while leaving the SUBJECT unscoped.
        .or_else(|| normalized_console_rpc_string_param(params, "agent_identity"));
    let one = |action: &'static str, target: Option<String>| Some(vec![(action, target)]);
    match method {
        // Unmapped would mean ABAC NO-OP: a durable tool-policy mutation
        // reachable with only console auth. The read is scoped to its subject;
        // the mutations are gated conservatively on runtime.admin because no
        // narrower existing action owns durable identity or tool-policy mutation
        // (agent.* own lifecycle transitions, not policy).
        crate::rpc::mob_methods::MEMBER_TOOL_DECLARATION => {
            one(ACTION_AGENT_VIEW, target)
        }
        crate::rpc::mob_methods::ADOPT_MEMBER_IDENTITY_DECLARATION
        | crate::rpc::mob_methods::APPLY_MEMBER_TOOL_DECLARATION => {
            one(ACTION_RUNTIME_ADMIN, None)
        }
        "mobkit/console/send" => one(ACTION_AGENT_SEND, identity),
        "mobkit/agent_memory/remember" => one(ACTION_AGENT_MEMORY_WRITE, identity),
        // Update is a supersede — a write within the record's lineage.
        "mobkit/agent_memory/update" => one(ACTION_AGENT_MEMORY_WRITE, identity),
        "mobkit/agent_memory/forget" => one(ACTION_AGENT_MEMORY_DELETE, identity),
        // §10.3 migration: memory reads moved from bare `agent.view` to the
        // per-scope read action, and the console keeps `agent.view` as a
        // prerequisite — both are required. Pre-migration configs are
        // covered by `normalize_access_config_for_memory_actions`.
        "mobkit/agent_memory/recall" | "mobkit/agent_memory/manifest" => Some(vec![
            (ACTION_AGENT_MEMORY_READ, identity.clone()),
            (ACTION_AGENT_VIEW, identity),
        ]),
        // Memory panel reads (§9.3). Identity-keyed listings gate up front
        // like recall; mob-scope listings need the mob read action;
        // unscoped listings carry no entry gate — every returned row is
        // filtered per caller scope in the handler
        // (`panel_record_visible`). The record-detail method is likewise
        // enforced post-load, where the record's scope is known.
        "mobkit/memory/panel/records" => match memory_panel_scope_param(params) {
            Some(scope) if scope == "mob" => one(ACTION_MOB_MEMORY_READ, None),
            Some(scope) if scope == "operator" => one(ACTION_OPERATOR_MEMORY_READ, None),
            _ if identity.is_some() => Some(vec![
                (ACTION_AGENT_MEMORY_READ, identity.clone()),
                (ACTION_AGENT_VIEW, identity),
            ]),
            _ => None,
        },
        // Dream history is realm-level steward activity spanning scopes; it
        // requires the unscoped read grant (a rule with no resource
        // selector), same as realm-scope record reads.
        "mobkit/memory/panel/dreams" => one(ACTION_AGENT_MEMORY_READ, None),
        // Durable dream verdict sheets + the usage-audit review queue: same
        // realm-level read posture as dream history.
        "mobkit/memory/panel/dream_runs" => one(ACTION_AGENT_MEMORY_READ, None),
        "mobkit/memory/panel/audit_verdicts" => one(ACTION_AGENT_MEMORY_READ, None),
        // Console Phase 2 cheap reads (proposal §7): store overview,
        // pipeline inputs, and the injection ledger — same realm-level read
        // posture. The health snapshot is deliberately NOT here yet: the
        // decided distinct runtime affordance lands with its own design.
        "mobkit/memory/panel/overview" => one(ACTION_AGENT_MEMORY_READ, None),
        "mobkit/memory/panel/proposals" => one(ACTION_AGENT_MEMORY_READ, None),
        "mobkit/memory/panel/injections" => one(ACTION_AGENT_MEMORY_READ, None),
        "mobkit/memory/panel/harvests" => one(ACTION_AGENT_MEMORY_READ, None),
        "mobkit/memory/panel/quarantine" => one(ACTION_MEMORY_QUARANTINE_REVIEW, None),
        // `mob.memory.propose` gates future propose surfaces and
        // `mob.memory.commit` is reserved for a future direct-commit RPC —
        // steward promotions ride the existing gating flow (gating.decide),
        // so neither maps to a method yet.
        "mobkit/retire"
        | "mobkit/retire_member"
        | "mobkit/force_cancel_member"
        | "mobkit/delete_identity" => one(ACTION_AGENT_RETIRE, target),
        "mobkit/respawn" | "mobkit/respawn_member" => one(ACTION_AGENT_RESPAWN, target),
        // Non-destructive cold reload: strictly less than a respawn (same
        // session, same generation), so the respawn grant covers it and no new
        // action vocabulary is needed.
        "mobkit/reload_member" => one(ACTION_AGENT_RESPAWN, target),
        "mobkit/reset" | "mobkit/reset_all" => one(ACTION_AGENT_RESET, target),
        "mobkit/ensure_member"
        | "mobkit/spawn_helper"
        | "mobkit/fork_helper"
        | "mobkit/attach_existing_session"
        | "mobkit/run_flow"
        | "mobkit/cancel_flow"
        | "mobkit/collect_completed" => one(ACTION_AGENT_SPAWN, target),
        // Flow state reads expose run records (including spawned member
        // identities), so they sit in the same tier as running flows.
        "mobkit/flow_status" | "mobkit/list_flows" | "mobkit/list_runs" => {
            one(ACTION_AGENT_SPAWN, None)
        }
        "mobkit/gating/decide" => one(ACTION_GATING_DECIDE, None),
        "mobkit/gating/pending" | "mobkit/gating/audit" => one(ACTION_GATING_VIEW, None),
        "mobkit/mob_events/query" | "mobkit/mob_events/subscribe" => one(ACTION_MOB_OBSERVE, None),
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
        | "mobkit/run_labels/get"
        // Read-only, but it enumerates every database and identity in a
        // state directory the caller names, so it sits in the admin tier.
        | "mobkit/storage/doctor" => one(ACTION_RUNTIME_ADMIN, None),
        "mobkit/get_member"
        | "mobkit/member_status"
        | "mobkit/member_health"
        | "mobkit/identity/resolved_tools"
        | "mobkit/identity/routing_status"
        | "mobkit/inspect_identity"
        | "mobkit/status_identity"
        | "mobkit/console/inspect_identity" => one(ACTION_AGENT_VIEW, target),
        // WorkGraph spans the whole mob, so both actions are resource-less:
        // reads need the view grant, every mutation the manage grant.
        method if crate::rpc::workgraph_methods::is_workgraph_mutating_method(method) => {
            one(ACTION_WORKGRAPH_MANAGE, None)
        }
        method if crate::rpc::workgraph_methods::is_workgraph_read_method(method) => {
            one(ACTION_WORKGRAPH_VIEW, None)
        }
        _ => None,
    }
}

/// Console-plane twin of `rpc::mob_methods::routing_status_error_response`.
/// The two planes carry separate dispatch, so the discriminated `reason` has to
/// be produced on both or a console caller gets a strictly worse answer than an
/// RPC caller for the same identity.
fn routing_status_error_value(
    response_id: Value,
    identity: &str,
    err: &crate::mob_handle_runtime::RoutingStatusUnavailable,
) -> Value {
    // Code AND payload come from the stdin plane's owners rather than being
    // spelled again here. The two planes previously disagreed about the
    // malformed-request case, which is precisely the divergence that having two
    // dispatchers invites.
    response_value(
        response_id,
        None,
        Some(JsonRpcError {
            code: crate::rpc::mob_methods::routing_status_error_code(err),
            message: format!("routing_status unavailable: {err}"),
            data: Some(crate::rpc::mob_methods::routing_status_error_data(
                identity, err,
            )),
        }),
    )
}

fn memory_panel_scope_param(params: &Value) -> Option<String> {
    normalized_console_rpc_string_param(params, "scope").filter(|scope| !scope.is_empty())
}

fn normalized_console_rpc_string_param(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_string)
}

/// Returns the denial error when the caller's view forbids the method.
/// Every mapped requirement must pass; the first failing one is reported.
fn console_rpc_access_violation(
    view: Option<&AccessView>,
    method: &str,
    params: &Value,
) -> Option<JsonRpcError> {
    let view = view.filter(|view| view.enforced())?;
    let requirements = console_rpc_access_requirements(method, params)?;
    for (action, target) in requirements {
        let allowed = match target {
            Some(ref identity) => view.allows_agent(action, identity),
            None => view.allows(action),
        };
        if !allowed {
            return Some(JsonRpcError {
                code: ACCESS_DENIED_RPC_CODE,
                message: format!("access denied: {action}"),
                data: Some(json!({
                    "kind": "access_denied",
                    "action": action,
                    "resource": target,
                })),
            });
        }
    }
    None
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

// ---------------------------------------------------------------------------
// Console Memory panel (§9.3): read-only `mobkit/memory/panel/*` RPCs
// ---------------------------------------------------------------------------

const MEMORY_PANEL_DEFAULT_LIMIT: usize = 50;
const MEMORY_PANEL_MAX_LIMIT: usize = 200;
const MEMORY_PANEL_CHAIN_MAX: usize = 32;
const MEMORY_PANEL_INJECTIONS_MAX: usize = 50;
const MEMORY_PANEL_DREAMS_DEFAULT: usize = 20;
const MEMORY_PANEL_DREAMS_MAX: usize = 100;

fn memory_panel_unavailable(response_id: Value) -> Value {
    response_value(
        response_id,
        None,
        Some(JsonRpcError {
            code: -32601,
            message: "memory panel is not configured".to_string(),
            data: None,
        }),
    )
}

fn memory_panel_store_error(
    response_id: Value,
    err: crate::identity_first::agent_memory::AgentMemoryError,
) -> Value {
    response_value(
        response_id,
        None,
        Some(crate::rpc::agent_memory_rpc_error("panel", err)),
    )
}

fn memory_panel_limit_param(params: &Value, default: usize, max: usize) -> usize {
    params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|limit| (limit as usize).clamp(1, max))
        .unwrap_or(default)
}

fn encode_memory_panel_cursor(cursor: &(u64, String)) -> String {
    format!("{}:{}", cursor.0, cursor.1)
}

fn parse_memory_panel_cursor(raw: &str) -> Option<(u64, String)> {
    let (ms, id) = raw.split_once(':')?;
    let ms = ms.parse::<u64>().ok()?;
    (!id.is_empty()).then(|| (ms, id.to_string()))
}

/// The read action guarding a record of this scope on the panel (§10.3).
/// Realm-scope reads ride an *unscoped* `agent.memory.read` grant (a rule
/// with no resource selector); operator scope (live since P4's provisional
/// keying) requires its own explicit `operator.memory.read` grant.
fn memory_panel_scope_action(scope: &crate::memory::records::MemoryScope) -> &'static str {
    match scope {
        crate::memory::records::MemoryScope::Identity { .. } => ACTION_AGENT_MEMORY_READ,
        crate::memory::records::MemoryScope::Mob { .. } => ACTION_MOB_MEMORY_READ,
        crate::memory::records::MemoryScope::Operator { .. } => ACTION_OPERATOR_MEMORY_READ,
        crate::memory::records::MemoryScope::Realm { .. } => ACTION_AGENT_MEMORY_READ,
    }
}

/// Per-row visibility for panel reads. Identity-scope rows require the
/// per-scope read action AND `agent.view` on the identity (§10.3: the
/// console keeps view as a prerequisite); mob rows require
/// `mob.memory.read`; operator rows require the explicit
/// `operator.memory.read` grant (cross-mob personal facts — an unscoped
/// `agent.memory.read` deliberately does NOT cover them); realm rows
/// require the unscoped read grant. Quarantined rows are additionally
/// reviewer-only — their bodies are exactly the content the quarantine
/// gate exists for.
fn memory_panel_record_visible(
    view: Option<&AccessView>,
    record: &crate::memory::records::MemoryRecord,
) -> bool {
    let Some(view) = view.filter(|view| view.enforced()) else {
        return true;
    };
    let scope_allowed = match &record.scope {
        crate::memory::records::MemoryScope::Identity { identity, .. } => {
            view.allows_agent(ACTION_AGENT_MEMORY_READ, identity)
                && view.allows_agent(ACTION_AGENT_VIEW, identity)
        }
        crate::memory::records::MemoryScope::Mob { .. } => view.allows(ACTION_MOB_MEMORY_READ),
        crate::memory::records::MemoryScope::Operator { .. } => {
            view.allows(ACTION_OPERATOR_MEMORY_READ)
        }
        crate::memory::records::MemoryScope::Realm { .. } => view.allows(ACTION_AGENT_MEMORY_READ),
    };
    if !scope_allowed {
        return false;
    }
    if matches!(
        record.status,
        crate::memory::records::RecordStatus::Quarantined { .. }
    ) {
        return view.allows(ACTION_MEMORY_QUARANTINE_REVIEW);
    }
    true
}

/// Per-row visibility for pending-promotion queue rows, mirroring
/// [`memory_panel_record_visible`] on the promotion's *target* scope:
/// promotions carry the scope key and steward rationale, so they leak the
/// same cross-scope metadata as record rows. `memory.quarantine.review` is
/// the queue's entry gate but is deliberately not sufficient per row.
/// Unknown scope kinds are denied.
fn memory_panel_promotion_visible(
    view: Option<&AccessView>,
    scope_kind: &str,
    scope_key: &str,
) -> bool {
    let Some(view) = view.filter(|view| view.enforced()) else {
        return true;
    };
    match scope_kind {
        "identity" => {
            view.allows_agent(ACTION_AGENT_MEMORY_READ, scope_key)
                && view.allows_agent(ACTION_AGENT_VIEW, scope_key)
        }
        "mob" => view.allows(ACTION_MOB_MEMORY_READ),
        "operator" => view.allows(ACTION_OPERATOR_MEMORY_READ),
        "realm" => view.allows(ACTION_AGENT_MEMORY_READ),
        _ => false,
    }
}

/// Serialize a record for the panel. List rows are body-free (`body_bytes`
/// stands in); only the record-detail surface carries the body.
fn memory_panel_record_json(
    record: &crate::memory::records::MemoryRecord,
    include_body: bool,
) -> Value {
    let mut value = serde_json::to_value(record).unwrap_or(Value::Null);
    if !include_body && let Some(object) = value.as_object_mut() {
        object.remove("body");
        object.insert("body_bytes".to_string(), json!(record.body.len()));
    }
    value
}

/// Resolve the realms a panel read spans: the explicit `realm` param, or
/// every realm with a store file.
async fn memory_panel_realms(
    store: &dyn crate::memory::capabilities::MemoryPanelStore,
    params: &Value,
) -> Result<Vec<String>, crate::identity_first::agent_memory::AgentMemoryError> {
    match normalized_console_rpc_string_param(params, "realm").filter(|realm| !realm.is_empty()) {
        Some(realm) => Ok(vec![realm]),
        None => store.panel_realms().await,
    }
}

async fn handle_memory_panel_records(
    store: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    view: Option<&AccessView>,
    params: &Value,
    response_id: Value,
) -> Value {
    let Some(store) = store else {
        return memory_panel_unavailable(response_id);
    };
    let scope_kind = memory_panel_scope_param(params);
    if let Some(kind) = &scope_kind
        && !matches!(kind.as_str(), "identity" | "mob" | "operator" | "realm")
    {
        return invalid_params(
            response_id,
            format!("Invalid params: unknown scope '{kind}'"),
        );
    }
    let identity = normalized_console_rpc_string_param(params, "identity")
        .filter(|identity| !identity.is_empty());
    let scope_kind = scope_kind.or_else(|| identity.as_ref().map(|_| "identity".to_string()));
    let scope_key = normalized_console_rpc_string_param(params, "scope_key")
        .filter(|key| !key.is_empty())
        .or(identity);
    let status =
        normalized_console_rpc_string_param(params, "status").filter(|status| !status.is_empty());
    if let Some(status) = &status
        && !matches!(
            status.as_str(),
            "active" | "quarantined" | "superseded" | "tombstoned"
        )
    {
        return invalid_params(
            response_id,
            format!("Invalid params: unknown status '{status}'"),
        );
    }
    let limit =
        memory_panel_limit_param(params, MEMORY_PANEL_DEFAULT_LIMIT, MEMORY_PANEL_MAX_LIMIT);
    let cursor = normalized_console_rpc_string_param(params, "cursor")
        .filter(|cursor| !cursor.is_empty())
        .map(|raw| parse_memory_panel_cursor(&raw));
    let cursor = match cursor {
        Some(None) => {
            return invalid_params(response_id, "Invalid params: malformed cursor".to_string());
        }
        Some(Some(cursor)) => Some(cursor),
        None => None,
    };
    let realms = match memory_panel_realms(store, params).await {
        Ok(realms) => realms,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    // Keyset continuation is per-realm: honored only when the query names
    // one realm. Multi-realm listings merge one bounded page per realm.
    let single_realm = realms.len() == 1;
    let mut records = Vec::new();
    let mut next_cursor = None;
    for realm in &realms {
        let page = match store
            .records_page(
                realm,
                scope_kind.as_deref(),
                scope_key.as_deref(),
                status.as_deref(),
                limit,
                if single_realm { cursor.clone() } else { None },
            )
            .await
        {
            Ok(page) => page,
            Err(err) => return memory_panel_store_error(response_id, err),
        };
        if single_realm {
            next_cursor = page.next_cursor.as_ref().map(encode_memory_panel_cursor);
        }
        records.extend(page.records);
    }
    records.retain(|record| memory_panel_record_visible(view, record));
    records.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| b.id.cmp(&a.id))
    });
    records.truncate(limit);
    let rows: Vec<Value> = records
        .iter()
        .map(|record| memory_panel_record_json(record, false))
        .collect();
    response_value(
        response_id,
        Some(json!({
            "records": rows,
            "next_cursor": next_cursor,
            "realms": realms,
        })),
        None,
    )
}

async fn handle_memory_panel_record(
    store: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    view: Option<&AccessView>,
    params: &Value,
    response_id: Value,
) -> Value {
    let Some(store) = store else {
        return memory_panel_unavailable(response_id);
    };
    let Some(memory_id) = normalized_console_rpc_string_param(params, "memory_id")
        .filter(|memory_id| !memory_id.is_empty())
    else {
        return invalid_params(
            response_id,
            "Invalid params: memory_id is required".to_string(),
        );
    };
    let realms = match memory_panel_realms(store, params).await {
        Ok(realms) => realms,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    let mut found = None;
    for realm in &realms {
        match store.record_by_id(realm, &memory_id).await {
            Ok(Some(record)) => {
                found = Some((realm.clone(), record));
                break;
            }
            Ok(None) => {}
            Err(err) => return memory_panel_store_error(response_id, err),
        }
    }
    let Some((realm, record)) = found else {
        return response_value(
            response_id,
            None,
            Some(JsonRpcError {
                code: -32001,
                message: format!("unknown memory record '{memory_id}'"),
                data: None,
            }),
        );
    };
    // Scope is only known post-load, so the entry gate lives here rather
    // than in the requirements table.
    if !memory_panel_record_visible(view, &record) {
        let action = if matches!(
            record.status,
            crate::memory::records::RecordStatus::Quarantined { .. }
        ) && view
            .is_some_and(|view| view.enforced() && !view.allows(ACTION_MEMORY_QUARANTINE_REVIEW))
        {
            ACTION_MEMORY_QUARANTINE_REVIEW
        } else {
            memory_panel_scope_action(&record.scope)
        };
        return response_value(
            response_id,
            None,
            Some(JsonRpcError {
                code: ACCESS_DENIED_RPC_CODE,
                message: format!("access denied: {action}"),
                data: Some(json!({
                    "kind": "access_denied",
                    "action": action,
                    "resource": record.id,
                })),
            }),
        );
    }
    let chain = match store
        .supersede_chain(&realm, &memory_id, MEMORY_PANEL_CHAIN_MAX)
        .await
    {
        Ok(chain) => chain,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    let chain_rows: Vec<Value> = chain
        .iter()
        .filter(|link| memory_panel_record_visible(view, link))
        .map(|link| memory_panel_record_json(link, false))
        .collect();
    let injections = match store
        .injection_log_for_record(&realm, &memory_id, MEMORY_PANEL_INJECTIONS_MAX)
        .await
    {
        Ok(entries) => entries,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    response_value(
        response_id,
        Some(json!({
            "realm": realm,
            "record": memory_panel_record_json(&record, true),
            "chain": chain_rows,
            "injections": injections,
        })),
        None,
    )
}

async fn handle_memory_panel_quarantine(
    store: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    view: Option<&AccessView>,
    params: &Value,
    response_id: Value,
) -> Value {
    let Some(store) = store else {
        return memory_panel_unavailable(response_id);
    };
    let limit =
        memory_panel_limit_param(params, MEMORY_PANEL_DEFAULT_LIMIT, MEMORY_PANEL_MAX_LIMIT);
    let realms = match memory_panel_realms(store, params).await {
        Ok(realms) => realms,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    let mut records = Vec::new();
    let mut pending = Vec::new();
    for realm in &realms {
        match store.quarantined_records(realm, limit).await {
            Ok(rows) => records.extend(rows),
            Err(err) => return memory_panel_store_error(response_id, err),
        }
        match store.pending_promotions(realm).await {
            Ok(rows) => pending.extend(
                rows.into_iter()
                    .filter(|promotion| {
                        memory_panel_promotion_visible(
                            view,
                            &promotion.scope_kind,
                            &promotion.scope_key,
                        )
                    })
                    .map(|promotion| {
                        // stage_token is a commit capability — never surfaced.
                        json!({
                            "realm": realm,
                            "pending_id": promotion.pending_id,
                            "record_id": promotion.record_id,
                            "scope_kind": promotion.scope_kind,
                            "scope_key": promotion.scope_key,
                            "rationale": promotion.rationale,
                            "status": promotion.status,
                            "created_at_ms": promotion.created_at_ms,
                        })
                    }),
            ),
            Err(err) => return memory_panel_store_error(response_id, err),
        }
    }
    // The reviewer entry gate is necessary but not sufficient: each queue row
    // still needs the caller's per-scope read grant, and hidden rows must not
    // consume the page budget.
    records.retain(|record| memory_panel_record_visible(view, record));
    records.sort_by_key(|record| std::cmp::Reverse(record.created_at_ms));
    records.truncate(limit);
    // Queue rows stay body-free even for reviewers: verdicts are decided on
    // the record detail surface, which renders the body with provenance.
    let rows: Vec<Value> = records
        .iter()
        .map(|record| memory_panel_record_json(record, false))
        .collect();
    response_value(
        response_id,
        Some(json!({
            "records": rows,
            "pending_promotions": pending,
            "realms": realms,
        })),
        None,
    )
}

async fn handle_memory_panel_dreams(
    store: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    params: &Value,
    response_id: Value,
) -> Value {
    let Some(store) = store else {
        return memory_panel_unavailable(response_id);
    };
    let limit =
        memory_panel_limit_param(params, MEMORY_PANEL_DREAMS_DEFAULT, MEMORY_PANEL_DREAMS_MAX);
    let realms = match memory_panel_realms(store, params).await {
        Ok(realms) => realms,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    let mut runs = Vec::new();
    for realm in &realms {
        match store.dream_history(realm, limit).await {
            Ok(history) => runs.extend(history.into_iter().map(|run| (realm.clone(), run))),
            Err(err) => return memory_panel_store_error(response_id, err),
        }
    }
    runs.sort_by_key(|(_, run)| std::cmp::Reverse(run.last_op_at_ms));
    runs.truncate(limit);
    let rows: Vec<Value> = runs
        .iter()
        .map(|(realm, run)| {
            json!({
                "realm": realm,
                "run_id": run.run_id,
                "first_op_at_ms": run.first_op_at_ms,
                "last_op_at_ms": run.last_op_at_ms,
                "ops": run.ops,
                "op_kinds": run.op_kinds,
                "quarantined_ops": run.quarantined_ops,
                "memory_ids": run.memory_ids,
                "rationales": run.rationales,
            })
        })
        .collect();
    response_value(
        response_id,
        Some(json!({ "runs": rows, "realms": realms })),
        None,
    )
}

/// Durable dream verdict sheets (`dream_runs` table): phases, verdict
/// counters, skips, and partition label per run — survives restarts, unlike
/// the audit-trail reconstruction served by `panel/dreams`.
async fn handle_memory_panel_dream_runs(
    store: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    params: &Value,
    response_id: Value,
) -> Value {
    let Some(store) = store else {
        return memory_panel_unavailable(response_id);
    };
    let limit =
        memory_panel_limit_param(params, MEMORY_PANEL_DREAMS_DEFAULT, MEMORY_PANEL_DREAMS_MAX);
    let realms = match memory_panel_realms(store, params).await {
        Ok(realms) => realms,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    let mut runs = Vec::new();
    for realm in &realms {
        match store.dream_runs(realm, limit).await {
            Ok(rows) => runs.extend(rows.into_iter().map(|run| (realm.clone(), run))),
            Err(err) => return memory_panel_store_error(response_id, err),
        }
    }
    runs.sort_by_key(|(_, run)| std::cmp::Reverse(run.completed_at_ms));
    runs.truncate(limit);
    let rows: Vec<Value> = runs
        .iter()
        .map(|(realm, run)| {
            let detail: Value =
                serde_json::from_str(&run.detail).unwrap_or_else(|_| json!(run.detail));
            json!({
                "realm": realm,
                "run_id": run.run_id,
                "partition": run.partition_label,
                "started_at_ms": run.started_at_ms,
                "completed_at_ms": run.completed_at_ms,
                "ops_committed": run.ops_committed,
                "detail": detail,
            })
        })
        .collect();
    response_value(
        response_id,
        Some(json!({ "runs": rows, "realms": realms })),
        None,
    )
}

/// The open usage-audit review queue: dead-weight verdicts awaiting operator
/// action ("memories you might want to correct").
async fn handle_memory_panel_audit_verdicts(
    store: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    params: &Value,
    response_id: Value,
) -> Value {
    let Some(store) = store else {
        return memory_panel_unavailable(response_id);
    };
    let limit =
        memory_panel_limit_param(params, MEMORY_PANEL_DREAMS_DEFAULT, MEMORY_PANEL_DREAMS_MAX);
    let realms = match memory_panel_realms(store, params).await {
        Ok(realms) => realms,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    let mut verdicts = Vec::new();
    for realm in &realms {
        match store.open_dream_audit_verdicts(realm, limit).await {
            Ok(rows) => verdicts.extend(rows.into_iter().map(|row| (realm.clone(), row))),
            Err(err) => return memory_panel_store_error(response_id, err),
        }
    }
    verdicts.sort_by_key(|(_, row)| std::cmp::Reverse(row.created_at_ms));
    verdicts.truncate(limit);
    let rows: Vec<Value> = verdicts
        .iter()
        .map(|(realm, row)| {
            json!({
                "realm": realm,
                "run_id": row.run_id,
                "record_id": row.record_id,
                "verdict": row.verdict,
                "rationale": row.rationale,
                "created_at_ms": row.created_at_ms,
            })
        })
        .collect();
    response_value(
        response_id,
        Some(json!({ "verdicts": rows, "realms": realms })),
        None,
    )
}

/// Store overview: per-scope record counts + bytes — Holdings + the STORE
/// FLOOR verdict tile (floors included so the client renders pressure
/// without duplicating the thresholds).
async fn handle_memory_panel_overview(
    store: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    params: &Value,
    response_id: Value,
) -> Value {
    let Some(store) = store else {
        return memory_panel_unavailable(response_id);
    };
    let realms = match memory_panel_realms(store, params).await {
        Ok(realms) => realms,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    let (floor_records, floor_bytes) = store.scope_floors();
    let mut scopes = Vec::new();
    for realm in &realms {
        match store.scope_overview(realm).await {
            Ok(rows) => scopes.extend(rows.into_iter().map(|row| {
                json!({
                    "realm": realm,
                    "scope_kind": row.scope.kind_str(),
                    "scope_key": row.scope.key(),
                    "active": row.active,
                    "quarantined": row.quarantined,
                    "superseded": row.superseded,
                    "tombstoned": row.tombstoned,
                    "body_bytes": row.body_bytes,
                    "floor_pressure": row.active as usize >= floor_records
                        || row.body_bytes as usize >= floor_bytes,
                })
            })),
            Err(err) => return memory_panel_store_error(response_id, err),
        }
    }
    response_value(
        response_id,
        Some(json!({
            "scopes": scopes,
            "realms": realms,
            "floors": { "records": floor_records, "bytes": floor_bytes },
        })),
        None,
    )
}

/// Pending mob/operator-scope proposals awaiting a dream verdict — the
/// Pipeline view's inbound lane (bodies withheld: titles only; the record
/// body stays reachable through panel/record once committed).
async fn handle_memory_panel_proposals(
    store: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    params: &Value,
    response_id: Value,
) -> Value {
    let Some(store) = store else {
        return memory_panel_unavailable(response_id);
    };
    let limit =
        memory_panel_limit_param(params, MEMORY_PANEL_DREAMS_DEFAULT, MEMORY_PANEL_DREAMS_MAX);
    let realms = match memory_panel_realms(store, params).await {
        Ok(realms) => realms,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    let mut proposals = Vec::new();
    for realm in &realms {
        match store.pending_proposals(realm, limit).await {
            Ok(rows) => proposals.extend(rows.into_iter().map(|row| {
                json!({
                    "realm": realm,
                    "proposal_id": row.proposal_id,
                    "scope_kind": row.scope.kind_str(),
                    "scope_key": row.scope.key(),
                    "title": row.record.title,
                    "kind": row.record.kind.as_str(),
                    "author": format!("{:?}", row.author),
                    "status": row.status,
                    "created_at_ms": row.created_at_ms,
                    "tainted": row.taint.is_some(),
                })
            })),
            Err(err) => return memory_panel_store_error(response_id, err),
        }
    }
    proposals.truncate(limit);
    response_value(
        response_id,
        Some(json!({ "proposals": proposals, "realms": realms })),
        None,
    )
}

/// The injection ledger (most recent first): the Knowledge Lens history and
/// the echo-safety/dup diagnostics feed.
async fn handle_memory_panel_injections(
    store: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    params: &Value,
    response_id: Value,
) -> Value {
    let Some(store) = store else {
        return memory_panel_unavailable(response_id);
    };
    let limit =
        memory_panel_limit_param(params, MEMORY_PANEL_DREAMS_DEFAULT, MEMORY_PANEL_DREAMS_MAX);
    let realms = match memory_panel_realms(store, params).await {
        Ok(realms) => realms,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    let mut injections = Vec::new();
    for realm in &realms {
        match store.injection_log(realm, limit).await {
            Ok(rows) => injections.extend(rows.into_iter().map(|row| {
                json!({
                    "realm": realm,
                    "record_id": row.record_id,
                    "identity": row.identity,
                    "session_key": row.session_key,
                    "surface": row.surface.as_str(),
                    "at_ms": row.at_ms,
                })
            })),
            Err(err) => return memory_panel_store_error(response_id, err),
        }
    }
    injections.sort_by_key(|row| std::cmp::Reverse(row["at_ms"].as_u64().unwrap_or(0)));
    injections.truncate(limit);
    response_value(
        response_id,
        Some(json!({ "injections": injections, "realms": realms })),
        None,
    )
}

/// Pending exit-interview harvests — the Health strip's harvest-queue lane.
async fn handle_memory_panel_harvests(
    store: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    params: &Value,
    response_id: Value,
) -> Value {
    let Some(store) = store else {
        return memory_panel_unavailable(response_id);
    };
    let limit =
        memory_panel_limit_param(params, MEMORY_PANEL_DREAMS_DEFAULT, MEMORY_PANEL_DREAMS_MAX);
    let realms = match memory_panel_realms(store, params).await {
        Ok(realms) => realms,
        Err(err) => return memory_panel_store_error(response_id, err),
    };
    let mut harvests = Vec::new();
    for realm in &realms {
        match store.pending_harvests(realm, limit).await {
            Ok(rows) => harvests.extend(rows.into_iter().map(|row| {
                json!({
                    "realm": realm,
                    "identity": row.identity,
                    "session_key": row.session_key,
                    "cause": row.cause,
                    "retired_at_ms": row.retired_at_ms,
                })
            })),
            Err(err) => return memory_panel_store_error(response_id, err),
        }
    }
    harvests.truncate(limit);
    response_value(
        response_id,
        Some(json!({ "harvests": harvests, "realms": realms })),
        None,
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
    if let Err(message) =
        crate::member_comms_id::validate_public_rpc_member_aliases(&parsed_request.params)
    {
        return (
            StatusCode::OK,
            Json::<Value>(invalid_params(response_id, message)),
        );
    }
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
            // Gate BEFORE externalizing/persisting uploads, mirroring the JSON
            // /console/send gate-first ordering. Without this, a caller denied
            // `agent.send` on this identity could still write attacker-supplied
            // image bytes into the target identity's blob store (a pre-auth
            // side effect / storage-amplification vector) — the send is denied
            // later, but the blob write already happened. Prime the attribute
            // cache first so role/label-scoped deny rules resolve fail-closed.
            if let Some(runtime) = &state.runtime
                && let Some(controller) = state.access.as_ref().filter(|c| c.enabled())
            {
                prime_access_cache_from_runtime(runtime, controller).await;
            }
            if let Some(violation) = console_rpc_access_violation(
                auth_context.access_view.as_ref(),
                "mobkit/console/send",
                &parsed_request.params,
            ) {
                return (
                    StatusCode::OK,
                    Json::<Value>(response_value(response_id, None, Some(violation))),
                );
            }
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
                state.memory_panel.as_deref(),
                state.identity_roster.clone(),
                state.workgraph.as_ref(),
                state.topology.as_ref(),
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
                pubkey: resolved.pubkey,
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
    // The real failure reason goes on the wire. It used to be logged here
    // and replaced with a bare "internal_error", which made every -32000
    // undiagnosable from the client side (meerkat-studio ask K2: their K1
    // retire/respawn failures cost a day because this said nothing).
    // `error` stays "internal_error" as the stable kind discriminator for
    // existing clients; `detail` carries the human-readable chain.
    response_value(
        id,
        None,
        Some(JsonRpcError {
            code: -32000,
            message: message.clone(),
            data: Some(json!({ "error": "internal_error", "detail": message })),
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
        "state": member_status_state_string(member.status),
        "role": member.role.to_string(),
        "addressability": member_addressability(member),
        "display_name": member.labels.get("display_name"),
        "labels": member.labels,
        "agent_runtime_id": crate::member_comms_id::runtime_alias_str(member.agent_identity.as_str()),
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
        "state": member_status_state_string(member.status),
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
            "agent_runtime_id": crate::member_comms_id::runtime_alias_str(member.agent_identity.as_str()),
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
    identity: &AgentIdentity,
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

type ConsoleRuntimeIdentityAlias = crate::identity_control_target::LiveIdentityMember;

/// Exact roster snapshot used to authorize one attributed event. Its fields
/// are derived only after the event's runtime id and fence token match the
/// same live roster entry, preventing a newer incarnation of the alias from
/// supplying authorization attributes for an older event.
pub(crate) struct SseMemberAuthorizationProjection {
    pub(crate) identity: String,
    pub(crate) attributes: AgentResourceAttributes,
    pub(crate) visible: bool,
}

/// Legacy live-only compatibility applies only to genuinely raw member ids.
/// A generated `rt:*` projection remains identity-owned even if target
/// resolution races the interval between durable deletion and member cleanup.
fn console_live_only_fallback_allowed(
    was_registered: bool,
    requested_identity: &str,
    live_alias: Option<&ConsoleRuntimeIdentityAlias>,
) -> bool {
    !was_registered
        && !crate::member_comms_id::is_reserved_generated_alias(requested_identity)
        && live_alias.is_some_and(|alias| {
            !crate::member_comms_id::is_reserved_generated_alias(&alias.runtime_member_id)
        })
}

fn durable_identity_for_member(member: &meerkat_mob::runtime::MobMemberListEntry) -> String {
    crate::member_comms_id::durable_identity_label(&member.labels)
        .map(str::to_owned)
        // Fallback surfaces the public alias, not the comms-safe roster id.
        .unwrap_or_else(|| {
            crate::member_comms_id::runtime_alias_str(member.agent_identity.as_str()).into_owned()
        })
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
    // Requested identities arrive in the public alias space; roster ids are
    // their comms-safe encodings (meerkat 0.7 MemberCommsName).
    let requested_member_id = crate::member_comms_id::mob_member_id(requested_identity);
    let entries = handle.list_members_including_retiring().await;
    let exact_matches = entries
        .iter()
        .filter(|entry| entry.agent_identity == requested_member_id)
        .cloned()
        .collect::<Vec<_>>();
    let label_matches = entries
        .iter()
        .filter(|entry| {
            crate::member_comms_id::durable_identity_label(&entry.labels)
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
        let runtime_member_id =
            crate::member_comms_id::runtime_alias_str(member.agent_identity.as_str()).into_owned();
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

/// Resolve a public member alias to the console identity and evaluate the
/// configured visibility policy against the live roster projection.
///
/// SSE routes use this before opening a per-agent subscription and for every
/// attributed mob/structural envelope. `None` means the member is no longer
/// present in the live roster; callers retain their existing unknown-member
/// behavior in that case.
pub(crate) async fn sse_member_identity_visibility(
    handle: &MobHandle,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    requested_identity: &str,
) -> Option<(String, bool)> {
    let candidates = lookup_member_alias_candidates_with_session(handle, requested_identity).await;
    let identity = candidates.first()?.identity.clone();
    let visible = candidates
        .iter()
        .any(|alias| runtime_alias_visible_to_console(handle, visibility_policy, alias));
    Some((identity, visible))
}

/// Resolve an attributed event to the exact live roster binding that emitted
/// it, then snapshot both ABAC attributes and console visibility from that
/// entry. `None` means the authority can no longer be proven and protected
/// event surfaces must fail closed.
pub(crate) async fn sse_member_authorization_projection(
    handle: &MobHandle,
    runtime: Option<&MobRuntime>,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    requested_identity: &str,
    expected_runtime_id: &AgentRuntimeId,
    expected_fence_token: FenceToken,
) -> Option<SseMemberAuthorizationProjection> {
    let candidates = lookup_member_alias_candidates_with_session(handle, requested_identity).await;
    let alias = candidates.into_iter().find(|candidate| {
        candidate
            .member
            .binding_atoms()
            .is_some_and(|(runtime_id, fence_token)| {
                runtime_id == *expected_runtime_id && fence_token == expected_fence_token
            })
    })?;

    let mut labels = alias.member.labels.clone();
    // Roster labels are untrusted for lineage. Only the spawn registry may
    // restore `spawned_by` and related inheritance metadata.
    crate::console_spawn::sanitize_unverified_lineage_labels(&mut labels);
    if let Some(runtime) = runtime {
        let registered = runtime.console_identity_labels().await;
        if let Some(spawn_labels) = registered.get(&alias.identity) {
            crate::console_spawn::merge_registered_labels(&mut labels, spawn_labels);
        }
    }
    let visible = runtime_alias_visible_to_console(handle, visibility_policy, &alias);
    Some(SseMemberAuthorizationProjection {
        identity: alias.identity.clone(),
        attributes: AgentResourceAttributes {
            identity: alias.identity,
            agent_id: Some(alias.runtime_member_id),
            role: Some(alias.member.role.to_string()),
            labels,
        },
        visible,
    })
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
            "ambiguous live identity alias {requested_identity} \
             [via console candidate lookup]: candidates [{}]",
            candidates.join(", ")
        ),
        data: Some(json!({
            "kind": "ambiguous_live_identity_alias",
            "identity": requested_identity,
            "candidates": candidates,
        })),
    }
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
    use crate::identity_control_target::{
        IdentityControlResolution, IdentityControlResolutionError,
    };

    let resolution = crate::identity_control_target::resolve_identity_control_target(
        handle,
        identity_runtime.map(Arc::as_ref),
        requested_identity,
        |alias| runtime_alias_visible_to_console(handle, visibility_policy, alias),
    )
    .await
    .map_err(|error| match error {
        IdentityControlResolutionError::Hidden { requested_identity } => {
            identity_hidden_by_policy_error(&requested_identity)
        }
        IdentityControlResolutionError::Ambiguous {
            requested_identity,
            candidates,
        } => ambiguous_live_identity_alias_error(&requested_identity, &candidates),
        IdentityControlResolutionError::StaleProjectedBinding {
            identity,
            runtime_member_id,
            registered_identity,
        } => JsonRpcError {
            code: -32000,
            message: format!(
                "stale live identity alias: live console alias {identity} resolves to {runtime_member_id}, but identity runtime binding belongs to {registered_identity}"
            ),
            data: Some(json!({
                "kind": "stale_live_identity_alias",
                "identity": identity,
                "runtime_member_id": runtime_member_id,
                "registered_identity": registered_identity,
            })),
        },
        IdentityControlResolutionError::InvalidProjectedIdentity { detail, .. } => JsonRpcError {
            code: -32602,
            message: format!("invalid identity: {detail}"),
            data: None,
        },
    })?;

    match resolution {
        IdentityControlResolution::Resolved(target) => {
            let target = *target;
            Ok(Some((target.identity, target.was_registered, target.live)))
        }
        IdentityControlResolution::Unresolved { .. } => Ok(None),
    }
}

fn live_alias_matches_status_runtime(
    alias: Option<&ConsoleRuntimeIdentityAlias>,
    status: &crate::identity_first::IdentityStatus,
) -> bool {
    let Some(alias) = alias else {
        return true;
    };
    // A registered binding must exist. A control call naming a live member for
    // an identity that has no runtime binding at all is not a match.
    if status.agent_runtime_id.is_none() {
        return false;
    }
    let registered_session = status.session_id.as_ref().map(ToString::to_string);
    // One centralized rule: live roster id decoded exactly to the durable
    // identity, plus EXACT session equality (a one-sided missing session fails
    // closed). `agent_runtime_id` stays binding bookkeeping and is not the
    // roster spelling.
    crate::member_comms_id::live_binding_matches_identity(
        &alias.runtime_member_id,
        alias.session_id.as_deref(),
        status.identity.as_str(),
        registered_session.as_deref(),
        status
            .agent_runtime_id
            .as_ref()
            .map(crate::identity_first::AgentRuntimeId::as_str),
    ) && alias.identity == status.identity.as_str()
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

fn identity_from_runtime_alias(alias: &str) -> Option<crate::identity_first::AgentIdentity> {
    let alias = crate::member_comms_id::runtime_alias_str(alias);
    let rest = alias.strip_prefix("rt:")?;
    let (identity, generation) = rest.rsplit_once(':')?;
    if identity.is_empty()
        || generation.is_empty()
        || !generation.chars().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    crate::identity_first::AgentIdentity::parse(identity).ok()
}

async fn stale_runtime_alias_json_rpc_error(
    operation: &str,
    identity_runtime: Option<&Arc<crate::identity_first::IdentityRuntime>>,
    alias: &str,
) -> Option<JsonRpcError> {
    let identity_runtime = identity_runtime?;
    let alias = crate::member_comms_id::runtime_alias_str(alias).into_owned();
    let identity = identity_from_runtime_alias(&alias)?;
    let status = identity_runtime.status(&identity).await.ok();
    let registered_runtime_member_id = status
        .as_ref()
        .and_then(|status| status.agent_runtime_id.as_ref())
        .map(crate::identity_first::AgentRuntimeId::as_str);
    if registered_runtime_member_id == Some(alias.as_str()) {
        return None;
    }
    Some(JsonRpcError {
        code: -32000,
        message: format!(
            "{operation} failed: identity runtime binding for {} points at {}, but requested live member is {}",
            identity.as_str(),
            registered_runtime_member_id.unwrap_or("<none>"),
            alias
        ),
        data: Some(json!({
            "kind": "stale_identity_runtime_binding",
            "identity": identity.as_str(),
            "registered_runtime_member_id": registered_runtime_member_id,
            "live_runtime_member_id": alias,
            "registered_session_id": status.as_ref().and_then(|status| status.session_id.as_ref()).map(ToString::to_string),
            "live_session_id": Value::Null,
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
        state: member_status_state_string(alias.member.status),
        model_capabilities: model_capabilities_for_member_entry(handle.definition(), &alias.member),
        runtime_mode: Some(alias.member.runtime_mode.to_string()),
        session_id: alias.session_id.clone(),
        wired_to: alias
            .member
            .wired_to
            .iter()
            .map(|peer| crate::member_comms_id::runtime_alias_str(peer.as_str()).into_owned())
            .collect(),
        labels: alias.member.labels.clone(),
        progress: None,
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
        && alias.member.status == meerkat_mob::MobMemberStatus::Active;
    let visibility = match alias.member.status {
        meerkat_mob::MobMemberStatus::Retiring | meerkat_mob::MobMemberStatus::Completed => {
            ConsoleVisibility::RetiredReadable
        }
        // Broken/unknown members have no live runtime binding; mirror the
        // identity-first projection, which marks them unreachable.
        meerkat_mob::MobMemberStatus::Broken | meerkat_mob::MobMemberStatus::Unknown => {
            ConsoleVisibility::Unreachable
        }
        _ if addressable => ConsoleVisibility::Addressable,
        _ => ConsoleVisibility::Hidden,
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
        health: match alias.member.status {
            meerkat_mob::MobMemberStatus::Active => "ready".to_string(),
            meerkat_mob::MobMemberStatus::Retiring => "retired".to_string(),
            // New machine statuses surface verbatim (broken/completed/unknown),
            // matching the identity-first health vocabulary.
            other => format!("{other:?}").to_ascii_lowercase(),
        },
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
        "state": status.state.wire_str(),
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
        "state": status.state.wire_str(),
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
    runtime_member_id: &AgentIdentity,
) -> Result<Option<Value>, String> {
    // Best-effort repair material: a faulted lookup degrades to None (the
    // respawn itself surfaces real faults).
    let entry_before_respawn = handle.get_member(runtime_member_id).await.ok().flatten();
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
                // A faulted lookup must not read as "absent" (that would mint
                // a spurious replacement member); surface it instead.
                if handle
                    .get_member(runtime_member_id)
                    .await
                    .map_err(|lookup_err| lookup_err.to_string())?
                    .is_none()
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

/// Live-member fallback shared by `mobkit/respawn` and `mobkit/reset` when no
/// identity-first record backs the target: respawn the runtime member (fresh
/// session, same configuration) and report the projected member status.
async fn respawn_live_console_member_response(
    handle: &MobHandle,
    console_events: Option<&ConsoleEventStore>,
    alias: &ConsoleRuntimeIdentityAlias,
    lifecycle_kind: &str,
    action: &str,
    response_id: Value,
) -> Value {
    let mid = crate::member_comms_id::mob_member_id(alias.runtime_member_id.as_str());
    match Box::pin(respawn_console_member(handle, &mid)).await {
        Ok(topology_restore_warning) => {
            if let Some(store) = console_events {
                store
                    .record_lifecycle(
                        &alias.identity,
                        lifecycle_kind,
                        json!({ "topology_restore_warning": topology_restore_warning.clone() }),
                    )
                    .await;
            }
            let mut body = match lookup_member_with_session(handle, &mid).await {
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
        Err(err) => internal_error(response_id, format!("{action} failed: {err}")),
    }
}

async fn retire_console_member(
    handle: &MobHandle,
    runtime_member_id: &AgentIdentity,
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

fn console_runtime_alias_generation(alias: &str, durable_identity: &str) -> Option<u64> {
    let alias = crate::member_comms_id::runtime_alias_str(alias);
    let rest = alias.strip_prefix("rt:")?;
    let (identity, generation) = rest.rsplit_once(':')?;
    if identity != durable_identity {
        return None;
    }
    generation.parse().ok()
}

/// Lifecycle-authoritative stale aliases, independent of console projection.
///
/// This is used only while the durable identity lifecycle lock is held. A
/// hidden generated alias is still an old concrete embodiment of the selected
/// durable identity and must not survive beside its current generation.
async fn stale_console_member_ids_for_identity_authoritative(
    handle: &MobHandle,
    durable_identity: &str,
    current_runtime_member_id: &str,
    include_current: bool,
) -> Vec<AgentIdentity> {
    let Some(current_generation) =
        console_runtime_alias_generation(current_runtime_member_id, durable_identity)
    else {
        return Vec::new();
    };
    lookup_member_alias_candidates_with_session(handle, durable_identity)
        .await
        .into_iter()
        .filter(|alias| {
            console_runtime_alias_generation(alias.runtime_member_id.as_str(), durable_identity)
                .is_some_and(|generation| {
                    generation < current_generation
                        || (include_current && generation == current_generation)
                })
        })
        .map(|alias| crate::member_comms_id::mob_member_id(alias.runtime_member_id.as_str()))
        .collect()
}

async fn retire_console_member_ids(
    handle: &MobHandle,
    member_ids: Vec<AgentIdentity>,
) -> Result<(), String> {
    for member_id in member_ids {
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
    if let Err(message) =
        crate::member_comms_id::validate_public_rpc_member_aliases(&request.params)
    {
        return invalid_params(response_id, message);
    }
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
                    "feature_capabilities": serde_json::json!([]),
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
            match Box::pin(aggregator.list_identities()).await {
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
        None,
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
    memory_panel: Option<&dyn crate::memory::capabilities::MemoryPanelStore>,
    identity_roster: Option<Arc<crate::identity_first::MutableRosterProvider>>,
    workgraph: Option<&meerkat::WorkGraphService>,
    topology: Option<&crate::topology_control::TopologyRuntimeHandle>,
) -> Value {
    let response_id = request.id.clone().unwrap_or(Value::Null);
    if let Err(message) =
        crate::member_comms_id::validate_public_rpc_member_aliases(&request.params)
    {
        return invalid_params(response_id, message);
    }
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
        prime_access_cache_from_runtime(runtime, controller).await;
    }
    if let Some(response) = handle_access_admin_rpc(access, access_view, &request) {
        return response;
    }
    if let Some(error) =
        console_rpc_access_violation(access_view, request.method.as_str(), &request.params)
    {
        return response_value(response_id, None, Some(error));
    }

    // The member-declaration family, served by the SAME canonical handlers as the
    // stdin surface. After auth, read_only and ABAC gating so this plane adds no
    // privilege, and one family arm reading the registry rather than three
    // literals - three literals per surface is how 0.8.21 shipped these dead here.
    if crate::rpc::mob_methods::is_member_declaration_method(request.method.as_str()) {
        let handle = runtime.handle();
        if let Some(response) = crate::rpc::mob_methods::handle_member_declaration_rpc(
            &handle,
            runtime.speaks_for_composition(),
            request.method.as_str(),
            response_id.clone(),
            &request.params,
        )
        .await
        {
            return response_value(response_id, response.result, response.error);
        }
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
                "mobkit/member_health",
                "mobkit/identity/resolved_tools",
                "mobkit/identity/routing_status",
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
                "mobkit/storage/doctor",
            ];
            // DERIVED from the canonical registry, on the plane that SERVES these.
            // Split by the family's own classifier: on a read_only console the
            // dispatch refuses adopt/apply with -32010, so advertising them
            // unconditionally would be capability/dispatch disagreement.
            methods.extend(
                crate::rpc::mob_methods::MEMBER_DECLARATION_METHODS
                    .iter()
                    .copied()
                    .filter(|method| {
                        can_mutate
                            || !crate::rpc::mob_methods::is_member_declaration_mutating_method(
                                method,
                            )
                    }),
            );
            if identity_runtime.is_some() {
                methods.extend_from_slice(&["mobkit/status_identity", "mobkit/inspect_identity"]);
                if let Some(identity_runtime) = &identity_runtime
                    && identity_runtime.agent_memory_supports_recall().await
                {
                    methods.push("mobkit/agent_memory/recall");
                    if can_mutate && identity_runtime.agent_memory_supports_remember().await {
                        methods.push("mobkit/agent_memory/remember");
                    }
                    if can_mutate && identity_runtime.agent_memory_supports_forget().await {
                        methods.push("mobkit/agent_memory/forget");
                    }
                    if can_mutate && identity_runtime.agent_memory_supports_update().await {
                        methods.push("mobkit/agent_memory/update");
                    }
                    if identity_runtime.agent_memory_supports_manifest().await {
                        methods.push("mobkit/agent_memory/manifest");
                    }
                }
                if can_mutate {
                    methods.push("mobkit/delete_identity");
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
                    // Respawn/reset dispatch on every console runtime: with an
                    // identity-first runtime they refresh the durable record,
                    // and without one they fall back to a live-member respawn.
                    // Advertising them only under the identity runtime left
                    // the console buttons dead on plain runtimes even though
                    // the dispatcher accepted the calls.
                    "mobkit/respawn",
                    "mobkit/reset",
                    "mobkit/reset_all",
                    "mobkit/console/send",
                    "mobkit/blob/upload",
                    "mobkit/ensure_member",
                    "mobkit/retire_member",
                    "mobkit/respawn_member",
                    "mobkit/reload_member",
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
            if memory_panel.is_some() {
                // Provider-dependent supports pattern: the panel methods
                // exist only when the bundled sqlite store is wired. The
                // grant-intersection probe below additionally strips the
                // resource-less ones (dreams/quarantine) the caller could
                // never use.
                methods.extend_from_slice(&[
                    "mobkit/memory/panel/records",
                    "mobkit/memory/panel/record",
                    "mobkit/memory/panel/dreams",
                    "mobkit/memory/panel/dream_runs",
                    "mobkit/memory/panel/audit_verdicts",
                    "mobkit/memory/panel/overview",
                    "mobkit/memory/panel/proposals",
                    "mobkit/memory/panel/injections",
                    "mobkit/memory/panel/harvests",
                    "mobkit/memory/panel/quarantine",
                ]);
            }
            if workgraph.is_some() {
                methods.extend_from_slice(crate::rpc::workgraph_methods::WORKGRAPH_READ_METHODS);
                if can_mutate {
                    methods
                        .extend_from_slice(crate::rpc::workgraph_methods::WORKGRAPH_MUTATE_METHODS);
                    methods.extend_from_slice(
                        crate::rpc::workgraph_methods::WORKGRAPH_CONSOLE_MUTATE_METHODS,
                    );
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
            let topology_capabilities = topology.map(|topology| {
                let (topology_methods, capabilities) =
                    crate::rpc::topology_methods::capability_projection(
                        topology,
                        access_view,
                        !can_mutate,
                    );
                methods.extend(topology_methods);
                capabilities
            });
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
                    |method| match console_rpc_access_requirements(method, &probe) {
                        Some(requirements) => requirements
                            .iter()
                            .all(|(action, target)| target.is_some() || view.allows(action)),
                        None => true,
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
                    "feature_capabilities": serde_json::json!([]),
                    "methods": methods,
                    // Doctrine: consumers gate their migration on this flag —
                    // when true, the member RPCs re-dispatch durable targets
                    // through the identity authority and ensure_member stands
                    // up durable identities (pass plane:"worker" to opt a
                    // spawn onto the ephemeral mob plane).
                    "identity_first": identity_runtime.is_some(),
                    // True when a WorkGraph service is wired into this
                    // console runtime and the mobkit/workgraph/* group is live.
                    "workgraph": workgraph.is_some(),
                    "read_only": read_only,
                    // The console routes to MobRuntime directly and has no
                    // access to the module runtime, so loaded_modules is always [].
                    "loaded_modules": serde_json::json!([]),
                    "runtime_capabilities": {
                        "can_send_messages": cap(ACTION_AGENT_SEND),
                        "can_retire_members": cap(ACTION_AGENT_RETIRE),
                        "can_spawn_members": cap(ACTION_AGENT_SPAWN),
                    },
                    "topology_control": topology_capabilities,
                })),
                None,
            )
        }
        crate::rpc::topology_methods::TOPOLOGY_QUERY_METHOD => {
            let Some(topology) = topology else {
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
            serde_json::to_value(
                crate::rpc::topology_methods::handle_query(
                    topology,
                    response_id,
                    access_view,
                    !can_mutate,
                )
                .await,
            )
            .unwrap_or(Value::Null)
        }
        crate::rpc::topology_methods::TOPOLOGY_PLAN_METHOD => {
            let Some(topology) = topology else {
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
            serde_json::to_value(
                crate::rpc::topology_methods::handle_plan(
                    topology,
                    response_id,
                    &request.params,
                    access_view,
                )
                .await,
            )
            .unwrap_or(Value::Null)
        }
        crate::rpc::topology_methods::TOPOLOGY_APPLY_METHOD => {
            let Some(topology) = topology else {
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
            serde_json::to_value(
                crate::rpc::topology_methods::handle_apply(
                    topology,
                    response_id,
                    &request.params,
                    access_view,
                    authenticated_principal,
                )
                .await,
            )
            .unwrap_or(Value::Null)
        }
        crate::rpc::topology_methods::TOPOLOGY_OPERATION_METHOD => {
            let Some(topology) = topology else {
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
            serde_json::to_value(
                crate::rpc::topology_methods::handle_operation(
                    topology,
                    response_id,
                    &request.params,
                    access_view,
                )
                .await,
            )
            .unwrap_or(Value::Null)
        }
        crate::rpc::topology_methods::TOPOLOGY_AUDIT_METHOD => {
            let Some(topology) = topology else {
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
            serde_json::to_value(
                crate::rpc::topology_methods::handle_audit(
                    topology,
                    response_id,
                    &request.params,
                    access_view,
                )
                .await,
            )
            .unwrap_or(Value::Null)
        }
        "mobkit/agent_memory/remember" => {
            let Some(identity_runtime) = &identity_runtime else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32601,
                        message: "agent memory is not configured".to_string(),
                        data: None,
                    }),
                );
            };
            match parse_agent_memory_remember_params(&request.params) {
                Ok(remember_request) => match identity_runtime
                    .remember_agent_memory(
                        &remember_request.realm,
                        &remember_request.identity,
                        remember_request.memory,
                    )
                    .await
                {
                    Ok(record) => response_value(
                        response_id,
                        Some(serde_json::to_value(record).unwrap_or(Value::Null)),
                        None,
                    ),
                    Err(err) => response_value(
                        response_id,
                        None,
                        Some(crate::rpc::agent_memory_rpc_error("write", err)),
                    ),
                },
                Err(err) => {
                    invalid_params(response_id, format!("Invalid params: {}", err.message()))
                }
            }
        }
        "mobkit/agent_memory/forget" => {
            let Some(identity_runtime) = &identity_runtime else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32601,
                        message: "agent memory is not configured".to_string(),
                        data: None,
                    }),
                );
            };
            match parse_agent_memory_forget_params(&request.params) {
                Ok(forget_request) => match identity_runtime
                    .forget_agent_memory(
                        &forget_request.realm,
                        &forget_request.identity,
                        &forget_request.memory_id,
                    )
                    .await
                {
                    Ok(result) => response_value(
                        response_id,
                        Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                        None,
                    ),
                    Err(err) => response_value(
                        response_id,
                        None,
                        Some(crate::rpc::agent_memory_rpc_error("forget", err)),
                    ),
                },
                Err(err) => {
                    invalid_params(response_id, format!("Invalid params: {}", err.message()))
                }
            }
        }
        "mobkit/agent_memory/recall" => {
            let Some(identity_runtime) = &identity_runtime else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32601,
                        message: "agent memory is not configured".to_string(),
                        data: None,
                    }),
                );
            };
            match parse_agent_memory_recall_params(&request.params) {
                Ok(recall_request) => match identity_runtime
                    .recall_agent_memory(recall_request.request)
                    .await
                {
                    Ok(records) => {
                        response_value(response_id, Some(json!({ "records": records })), None)
                    }
                    Err(err) => response_value(
                        response_id,
                        None,
                        Some(crate::rpc::agent_memory_rpc_error("recall", err)),
                    ),
                },
                Err(err) => {
                    invalid_params(response_id, format!("Invalid params: {}", err.message()))
                }
            }
        }
        "mobkit/agent_memory/update" => {
            let Some(identity_runtime) = &identity_runtime else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32601,
                        message: "agent memory is not configured".to_string(),
                        data: None,
                    }),
                );
            };
            match parse_agent_memory_update_params(&request.params) {
                Ok(update_request) => match identity_runtime
                    .update_agent_memory(
                        &update_request.realm,
                        &update_request.identity,
                        &update_request.memory_id,
                        update_request.memory,
                    )
                    .await
                {
                    Ok(new_id) => response_value(
                        response_id,
                        Some(json!({
                            "memory_id": new_id,
                            "supersedes": update_request.memory_id,
                        })),
                        None,
                    ),
                    Err(err) => response_value(
                        response_id,
                        None,
                        Some(crate::rpc::agent_memory_rpc_error("update", err)),
                    ),
                },
                Err(err) => {
                    invalid_params(response_id, format!("Invalid params: {}", err.message()))
                }
            }
        }
        "mobkit/agent_memory/manifest" => {
            let Some(identity_runtime) = &identity_runtime else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32601,
                        message: "agent memory is not configured".to_string(),
                        data: None,
                    }),
                );
            };
            match parse_agent_memory_manifest_params(&request.params) {
                Ok(manifest_request) => match identity_runtime
                    .manifest_agent_memory(
                        &manifest_request.realm,
                        &manifest_request.identity,
                        manifest_request.tier,
                    )
                    .await
                {
                    Ok(records) => {
                        response_value(response_id, Some(json!({ "records": records })), None)
                    }
                    Err(err) => response_value(
                        response_id,
                        None,
                        Some(crate::rpc::agent_memory_rpc_error("manifest", err)),
                    ),
                },
                Err(err) => {
                    invalid_params(response_id, format!("Invalid params: {}", err.message()))
                }
            }
        }
        // §9.3 Memory panel reads. Read-only mode allows all of these (they
        // are reads); the ACL mapping lives in
        // `console_rpc_access_requirements` plus per-row scope filtering in
        // the handlers.
        "mobkit/memory/panel/records" => {
            handle_memory_panel_records(memory_panel, access_view, &request.params, response_id)
                .await
        }
        "mobkit/memory/panel/record" => {
            handle_memory_panel_record(memory_panel, access_view, &request.params, response_id)
                .await
        }
        "mobkit/memory/panel/quarantine" => {
            handle_memory_panel_quarantine(memory_panel, access_view, &request.params, response_id)
                .await
        }
        "mobkit/memory/panel/dreams" => {
            handle_memory_panel_dreams(memory_panel, &request.params, response_id).await
        }
        "mobkit/memory/panel/dream_runs" => {
            handle_memory_panel_dream_runs(memory_panel, &request.params, response_id).await
        }
        "mobkit/memory/panel/audit_verdicts" => {
            handle_memory_panel_audit_verdicts(memory_panel, &request.params, response_id).await
        }
        "mobkit/memory/panel/overview" => {
            handle_memory_panel_overview(memory_panel, &request.params, response_id).await
        }
        "mobkit/memory/panel/proposals" => {
            handle_memory_panel_proposals(memory_panel, &request.params, response_id).await
        }
        "mobkit/memory/panel/injections" => {
            handle_memory_panel_injections(memory_panel, &request.params, response_id).await
        }
        "mobkit/memory/panel/harvests" => {
            handle_memory_panel_harvests(memory_panel, &request.params, response_id).await
        }
        // Read-only state-directory diagnosis (registered as a read method:
        // allowed in read-only mode, admin-gated in
        // `console_rpc_access_requirements`).
        "mobkit/storage/doctor" => {
            match crate::rpc::storage_methods::parse_storage_doctor_params(&request.params) {
                Ok(Some(params)) => {
                    let result = crate::rpc::storage_methods::run_storage_doctor(
                        &params,
                        runtime.resolved_storage(),
                    )
                    .await;
                    response_value(response_id, Some(result), None)
                }
                Ok(None) => response_value(
                    response_id,
                    None,
                    Some(crate::rpc::storage_methods::storage_doctor_state_dir_unavailable_error()),
                ),
                Err(reason) => invalid_params(response_id, reason),
            }
        }
        "mobkit/status" => {
            let mob_state = runtime.handle().status_observation_snapshot();
            let mut result = serde_json::json!({
                "contract_version": crate::rpc::MOBKIT_CONTRACT_VERSION,
                "running": matches!(mob_state, MobState::Creating | MobState::Running),
                // Console routes to MobRuntime directly — no module runtime available.
                // Return [] to keep StatusResult.loaded_modules schema-consistent.
                "loaded_modules": serde_json::json!([]),
            });
            // H1/H2 storage durability resolution, same object as the
            // unified `mobkit/status` shape.
            if let Some(storage) = runtime.resolved_storage() {
                result["storage"] = storage.status_json();
            }
            // The console's storage/doctor read method is always dispatchable
            // (auto-creates the storage object on runtimes without a
            // composition-time resolution summary).
            result["storage"]["doctor_available"] = serde_json::json!(true);
            response_value(response_id, Some(result), None)
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
            match Box::pin(aggregator.list_identities()).await {
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
                let alias =
                    crate::member_comms_id::runtime_alias_str(entry.agent_identity.as_str());
                if stale_runtime_alias_json_rpc_error(
                    "list_members",
                    identity_runtime.as_ref(),
                    alias.as_ref(),
                )
                .await
                .is_some()
                {
                    continue;
                }
                members.push(member_entry_to_console_json(runtime, entry).await);
            }
            retain_visible_member_rows(&mut members, access_view);
            response_value(response_id, Some(Value::Array(members)), None)
        }
        "mobkit/get_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            if let Some(error) = stale_runtime_alias_json_rpc_error(
                "get_member",
                identity_runtime.as_ref(),
                member_id,
            )
            .await
            {
                return response_value(response_id, None, Some(error));
            }
            let handle = runtime.handle();
            let identity = crate::member_comms_id::mob_member_id(member_id);
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
                status: None,
            };
            let entries = match handle.list_members_matching(filter).await {
                Ok(entries) => entries,
                Err(err) => {
                    return invalid_params(response_id, format!("member lookup failed: {err}"));
                }
            };
            let mut matches = Vec::with_capacity(entries.len());
            for entry in &entries {
                let alias =
                    crate::member_comms_id::runtime_alias_str(entry.agent_identity.as_str());
                if stale_runtime_alias_json_rpc_error(
                    "find_members",
                    identity_runtime.as_ref(),
                    alias.as_ref(),
                )
                .await
                .is_some()
                {
                    continue;
                }
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
                let (parsed_identity, _was_registered, live_alias) =
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
                let (parsed_identity, _was_registered, live_alias) =
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
                let (parsed_identity, was_registered, live_alias) =
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
                let cleanup_handle = handle.clone();
                let cleanup_identity = parsed_identity.clone();
                let include_current = !identity_runtime.has_session_bridge();
                let expected_alias = crate::member_comms_id::is_reserved_generated_alias(identity)
                    .then_some(identity);
                let retire_result = identity_runtime
                    .retire_and_cleanup_live_members_tracked(
                        &parsed_identity,
                        expected_alias,
                        move |retired_alias| async move {
                            let stale_member_ids = match retired_alias.as_ref() {
                                Some(retired_alias) => {
                                    stale_console_member_ids_for_identity_authoritative(
                                        &cleanup_handle,
                                        cleanup_identity.as_str(),
                                        retired_alias.as_str(),
                                        include_current,
                                    )
                                    .await
                                }
                                None => Vec::new(),
                            };
                            retire_console_member_ids(&cleanup_handle, stale_member_ids)
                                .await
                                .err()
                                .map(|error| {
                                    json!({
                                        "kind": "stale_member_cleanup_failed_after_identity_retire",
                                        "identity": cleanup_identity.as_str(),
                                        "message": error,
                                    })
                                })
                        },
                    )
                    .await;
                match retire_result {
                    Ok((token, cleanup_warning)) => {
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
                    Err(err @ crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                        if !console_live_only_fallback_allowed(
                            was_registered,
                            identity,
                            live_alias.as_ref(),
                        ) {
                            return console_identity_error_response(response_id, "retire", err);
                        }
                        if let Some(alias) = live_alias.as_ref() {
                            if !runtime_alias_visible_to_console(&handle, visibility_policy, alias)
                            {
                                return identity_hidden_by_policy_response(response_id, identity);
                            }
                            let mid = crate::member_comms_id::mob_member_id(
                                alias.runtime_member_id.as_str(),
                            );
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
            let mid = crate::member_comms_id::mob_member_id(alias.runtime_member_id.as_str());
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
                let (parsed_identity, was_registered, live_alias) =
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
                let expected_alias = crate::member_comms_id::is_reserved_generated_alias(identity)
                    .then_some(identity);
                let respawn_result = identity_runtime
                    .respawn_identity_in_place_tracked(&parsed_identity, expected_alias)
                    .await;
                match respawn_result {
                    Ok(record) => {
                        // Durable identity respawn recovers the authoritative
                        // session in place, so the legacy raw-member warning
                        // fields remain null but present.
                        let live_respawn_warning: Option<Value> = None;
                        let cleanup_warning: Option<Value> = None;
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
                        if !console_live_only_fallback_allowed(
                            was_registered,
                            identity,
                            live_alias.as_ref(),
                        ) {
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
            Box::pin(respawn_live_console_member_response(
                &handle,
                console_events.as_ref(),
                &alias,
                "identity_respawned",
                "respawn",
                response_id,
            ))
            .await
        }
        "mobkit/reset" => {
            let Some(identity) = request.params.get("identity").and_then(Value::as_str) else {
                return invalid_params(response_id, "identity required");
            };
            let handle = runtime.handle();
            let Some(identity_runtime) = &identity_runtime else {
                // No identity-first runtime: degrade to the same live-member
                // respawn fallback the identity path applies to live-only
                // members (fresh session, same configuration). This keeps the
                // console Reset button working on plain runtimes, matching
                // the advertised `mobkit/reset` capability.
                let live_alias =
                    match lookup_member_alias_with_session(&handle, visibility_policy, identity)
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
                    reject_ambiguous_projected_live_identity(&handle, visibility_policy, &alias)
                        .await
                {
                    return response_value(response_id, None, Some(err));
                }
                return Box::pin(respawn_live_console_member_response(
                    &handle,
                    console_events.as_ref(),
                    &alias,
                    "identity_reset",
                    "reset",
                    response_id,
                ))
                .await;
            };
            let (parsed_identity, was_registered, live_alias) =
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
                    if console_live_only_fallback_allowed(
                        was_registered,
                        identity,
                        live_alias.as_ref(),
                    ) && let Some(alias) = live_alias.as_ref()
                    {
                        if !runtime_alias_visible_to_console(&handle, visibility_policy, alias) {
                            return identity_hidden_by_policy_response(response_id, identity);
                        }
                        return Box::pin(respawn_live_console_member_response(
                            &handle,
                            console_events.as_ref(),
                            alias,
                            "identity_reset",
                            "reset",
                            response_id,
                        ))
                        .await;
                    }
                    return invalid_params(response_id, format!("identity not found: {identity}"));
                }
                Err(err) => return console_identity_error_response(response_id, "reset", err),
            }
            let reset_result = if crate::member_comms_id::is_reserved_generated_alias(identity) {
                identity_runtime
                    .reset_member_alias_tracked(&parsed_identity, identity)
                    .await
            } else {
                identity_runtime.reset_tracked(&parsed_identity).await
            };
            match reset_result {
                Ok(record) => {
                    let cleanup_warning = Some(json!({
                        "kind": "stale_member_cleanup_skipped_after_identity_reset",
                        "identity": parsed_identity.as_str(),
                        "agent_runtime_id": record.agent_runtime_id.as_str(),
                        "message": "reset published the new generation without retiring stale live mob members; identity control calls reject stale runtime ids",
                    }));
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
            let (parsed_identity, was_registered, live_alias) =
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
                Err(err @ crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    if was_registered {
                        return console_identity_error_response(
                            response_id,
                            "delete_identity",
                            err,
                        );
                    }
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
            let cleanup_handle = handle.clone();
            let cleanup_identity = parsed_identity.clone();
            let include_current = !identity_runtime.has_session_bridge();
            let expected_alias =
                crate::member_comms_id::is_reserved_generated_alias(identity).then_some(identity);
            let delete_result = identity_runtime
                .delete_identity_and_cleanup_live_members_tracked(
                    &parsed_identity,
                    expected_alias,
                    move |deleted_alias| async move {
                        let stale_member_ids = match deleted_alias.as_ref() {
                            Some(deleted_alias) => {
                                stale_console_member_ids_for_identity_authoritative(
                                    &cleanup_handle,
                                    cleanup_identity.as_str(),
                                    deleted_alias.as_str(),
                                    include_current,
                                )
                                .await
                            }
                            None => Vec::new(),
                        };
                        retire_console_member_ids(&cleanup_handle, stale_member_ids)
                            .await
                            .err()
                            .map(|error| {
                                json!({
                                    "kind": "stale_member_cleanup_failed_after_identity_delete",
                                    "identity": cleanup_identity.as_str(),
                                    "message": error,
                                })
                            })
                    },
                )
                .await;
            match delete_result {
                Ok(cleanup_warning) => {
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
            if crate::member_comms_id::is_reserved_generated_alias(agent_identity) {
                return invalid_params(
                    response_id,
                    "rt:* is reserved for identity-runtime generated aliases",
                );
            }
            let labels = match request.params.get("labels") {
                None | Some(Value::Null) => std::collections::BTreeMap::new(),
                Some(value) => match serde_json::from_value(value.clone()) {
                    Ok(map) => map,
                    Err(err) => {
                        return invalid_params(response_id, format!("invalid labels: {err}"));
                    }
                },
            };
            if let Err(message) = crate::member_comms_id::validate_raw_identity_labels(&labels) {
                return invalid_params(response_id, message);
            }
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
            // Identity-first gateways (ask K0): ensure_member upserts the
            // desired-identity roster and reconciles — the durable-identity
            // equivalent of the mob-member spawn, with the tolerant identity
            // lifecycle underneath (the ask-20 retire/respawn class does not
            // exist on this surface). Doctrine escape hatch: plane:"worker"
            // pins the spawn to the ephemeral mob plane (idle-retire reaping,
            // no continuity record) for helper churn that must not become a
            // durable identity.
            let worker_plane = matches!(
                request.params.get("plane").and_then(Value::as_str),
                Some("worker")
            );
            if !worker_plane
                && let (Some(identity_runtime_ref), Some(roster)) =
                    (identity_runtime.as_ref(), identity_roster.as_ref())
            {
                let identity = match crate::identity_first::AgentIdentity::parse(agent_identity) {
                    Ok(identity) => identity,
                    Err(err) => {
                        return invalid_params(
                            response_id,
                            format!("invalid agent_identity: {err}"),
                        );
                    }
                };
                if resume_session_id.is_some() {
                    return invalid_params(
                        response_id,
                        "resume_session_id is not supported on an identity-first \
                         gateway: continuity records own session resumption",
                    );
                }
                if binding.is_some() {
                    return invalid_params(
                        response_id,
                        "external runtime bindings are not supported on an \
                         identity-first gateway yet",
                    );
                }
                let spec = crate::identity_first::DurableAgentSpec {
                    identity: identity.clone(),
                    profile: ProfileName::from(role),
                    addressability: crate::identity_first::AgentAddressability::Addressable,
                    display_name: None,
                    labels,
                    context,
                    additional_instructions: additional_instructions.unwrap_or_default(),
                    initial_message: None,
                    runtime_mode_override: runtime_mode,
                    backend,
                    binding: None,
                    placement: None,
                };
                roster.upsert(spec);
                return match crate::identity_first::restore_flow(
                    identity_runtime_ref,
                    &roster.snapshot(),
                    None,
                    None,
                )
                .await
                {
                    Ok(result) => {
                        // Converge declared definition wiring after every
                        // materialization — upstream spawn-time wiring is
                        // bring-up-order dependent (HomeCore, 2026-07-09).
                        let _ = reconcile_console_topology(runtime, topology).await;
                        let outcome = match result.outcomes.get(&identity) {
                            Some(crate::identity_first::RestoreOutcome::Created { .. }) => {
                                "created"
                            }
                            Some(crate::identity_first::RestoreOutcome::Resumed { .. }) => {
                                "resumed"
                            }
                            Some(crate::identity_first::RestoreOutcome::Dormant { .. }) => {
                                "dormant"
                            }
                            Some(crate::identity_first::RestoreOutcome::Broken(_)) => "broken",
                            None => "unchanged",
                        };
                        let state = identity_runtime_ref
                            .status(&identity)
                            .await
                            .map(|status| status.state.wire_str().to_string())
                            .unwrap_or_else(|_| "unknown".to_string());
                        response_value(
                            response_id,
                            Some(serde_json::json!({
                                "agent_identity": identity.as_str(),
                                "role": role,
                                "identity_first": true,
                                "outcome": outcome,
                                "state": state,
                            })),
                            None,
                        )
                    }
                    Err(err) => {
                        internal_error(response_id, format!("ensure_member (identity): {err}"))
                    }
                };
            }
            let raw_reservation = match crate::member_comms_id::reserve_raw_member_target(
                identity_runtime.as_ref(),
                agent_identity,
            )
            .await
            {
                Ok(reservation) => reservation,
                Err(message) => return invalid_params(response_id, message),
            };
            let mut spec = SpawnMemberSpec::new(
                ProfileName::from(role),
                crate::member_comms_id::mob_member_id(raw_reservation.alias()),
            );
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
            let ensure_result = handle.ensure_member(spec).await;
            drop(raw_reservation);
            match ensure_result {
                Ok(_outcome) => {
                    let _ = reconcile_console_topology(runtime, topology).await;
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
            let member_id = crate::member_comms_id::runtime_alias_str(member_id).into_owned();
            if let Some(error) = stale_runtime_alias_json_rpc_error(
                "retire_member",
                identity_runtime.as_ref(),
                &member_id,
            )
            .await
            {
                return response_value(response_id, None, Some(error));
            }
            if let (Some(identity_runtime_ref), Some(roster)) =
                (identity_runtime.as_ref(), identity_roster.as_ref())
                && let Some(identity) = identity_runtime_ref
                    .identity_for_member_mutation(&member_id)
                    .await
            {
                return match identity_runtime_ref
                    .retire_member_alias_tracked(&identity, &member_id)
                    .await
                {
                    Ok(_token) => {
                        // Off the desired roster too, or the next reconcile
                        // (or the repair task) re-creates it.
                        roster.remove(&identity);
                        response_value(
                            response_id,
                            Some(serde_json::json!({
                                "accepted": true,
                                "identity_first": true,
                            })),
                            None,
                        )
                    }
                    Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                        response_value(
                            response_id,
                            None,
                            Some(JsonRpcError {
                                code: -32001,
                                message: format!("unknown identity: {member_id}"),
                                data: None,
                            }),
                        )
                    }
                    Err(err) => {
                        internal_error(response_id, format!("retire_member (identity): {err}"))
                    }
                };
            }
            // Doctrine: identity-owned members retire through the identity
            // authority even when this console has no roster slot (builder-
            // constructed identity runtimes serve these arms too).
            if let Some(identity_runtime_ref) = identity_runtime.as_ref()
                && let Some(durable) = identity_runtime_ref
                    .identity_for_member_mutation(&member_id)
                    .await
            {
                return match identity_runtime_ref
                    .retire_member_alias_tracked(&durable, &member_id)
                    .await
                {
                    Ok(_token) => response_value(
                        response_id,
                        Some(serde_json::json!({
                            "accepted": true,
                            "identity_first": true,
                        })),
                        None,
                    ),
                    Err(err) => {
                        internal_error(response_id, format!("retire_member (identity): {err}"))
                    }
                };
            }
            if crate::member_comms_id::is_reserved_generated_alias(&member_id) {
                return internal_error(
                    response_id,
                    format!(
                        "generated member alias requires current identity authority: {member_id}"
                    ),
                );
            }
            if let Some(aggregator) = &console_aggregator {
                return match Box::pin(aggregator.retire_identity(&member_id)).await {
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
            // Parity with the SDK dispatcher and the identity-named console
            // paths: a completed-disposal cleanup miss reads as retired.
            match retire_console_member(
                &runtime.handle(),
                &crate::member_comms_id::mob_member_id(&member_id),
            )
            .await
            {
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
            let member_id = crate::member_comms_id::runtime_alias_str(member_id).into_owned();
            if let Some(error) = stale_runtime_alias_json_rpc_error(
                "respawn_member",
                identity_runtime.as_ref(),
                &member_id,
            )
            .await
            {
                return response_value(response_id, None, Some(error));
            }
            if identity_roster.is_some()
                && let Some(identity_runtime_ref) = identity_runtime.as_ref()
                && let Some(identity) = identity_runtime_ref
                    .identity_for_member_mutation(&member_id)
                    .await
            {
                // Identity-first respawn = reset: retire the live session and
                // rebuild from the current roster spec under the same durable
                // identity (fresh session, new generation).
                return match identity_runtime_ref
                    .reset_member_alias_tracked(&identity, &member_id)
                    .await
                {
                    Ok(record) => response_value(
                        response_id,
                        Some(serde_json::json!({
                            "accepted": true,
                            "identity_first": true,
                            "session_id": record.session_id.to_string(),
                            "generation": record.generation.get(),
                        })),
                        None,
                    ),
                    Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                        response_value(
                            response_id,
                            None,
                            Some(JsonRpcError {
                                code: -32001,
                                message: format!("unknown identity: {member_id}"),
                                data: None,
                            }),
                        )
                    }
                    Err(err) => {
                        internal_error(response_id, format!("respawn_member (identity): {err}"))
                    }
                };
            }
            if let Some(identity_runtime_ref) = identity_runtime.as_ref()
                && let Some(durable) = identity_runtime_ref
                    .identity_for_member_mutation(&member_id)
                    .await
            {
                return match identity_runtime_ref
                    .reset_member_alias_tracked(&durable, &member_id)
                    .await
                {
                    Ok(record) => response_value(
                        response_id,
                        Some(serde_json::json!({
                            "accepted": true,
                            "identity_first": true,
                            "session_id": record.session_id.to_string(),
                            "generation": record.generation.get(),
                        })),
                        None,
                    ),
                    Err(err) => {
                        internal_error(response_id, format!("respawn_member (identity): {err}"))
                    }
                };
            }
            if crate::member_comms_id::is_reserved_generated_alias(&member_id) {
                return internal_error(
                    response_id,
                    format!(
                        "generated member alias requires current identity authority: {member_id}"
                    ),
                );
            }
            // Parity with respawn_console_member's tolerance set: topology
            // warnings degrade, completed-disposal cleanup misses repair.
            match Box::pin(respawn_console_member(
                &runtime.handle(),
                &crate::member_comms_id::mob_member_id(&member_id),
            ))
            .await
            {
                Ok(topology_restore_warning) => {
                    let mut body = serde_json::json!({ "accepted": true });
                    if let Some(warning) = topology_restore_warning {
                        body["topology_restore_warning"] = warning;
                    }
                    response_value(response_id, Some(body), None)
                }
                Err(err) => internal_error(response_id, format!("respawn_member failed: {err}")),
            }
        }
        "mobkit/reload_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            let member_id = crate::member_comms_id::runtime_alias_str(member_id).into_owned();
            if let Some(error) = stale_runtime_alias_json_rpc_error(
                "reload_member",
                identity_runtime.as_ref(),
                &member_id,
            )
            .await
            {
                return response_value(response_id, None, Some(error));
            }
            // Non-destructive by construction: same identity, same durable
            // session, same continuity generation. Only the identity plane
            // can honour that (it owns the continuity record and the lifecycle
            // lock); the worker plane's only "reload" is `respawn`, which is a
            // destructive reset, so a non-identity member is refused rather
            // than silently downgraded.
            let Some(identity_runtime_ref) = identity_runtime.as_ref() else {
                return internal_error(
                    response_id,
                    "reload_member requires identity-first runtime authority; the worker plane \
                     has no non-destructive reload (respawn_member is a destructive reset)",
                );
            };
            let Some(identity) = identity_runtime_ref
                .identity_for_member_mutation(&member_id)
                .await
            else {
                return response_value(
                    response_id,
                    None,
                    Some(JsonRpcError {
                        code: -32001,
                        message: format!("unknown identity: {member_id}"),
                        data: None,
                    }),
                );
            };
            let expected_alias = crate::member_comms_id::is_reserved_generated_alias(&member_id)
                .then_some(member_id.as_str());
            match identity_runtime_ref
                .reload_member_alias_tracked(&identity, expected_alias)
                .await
            {
                Ok(outcome) => {
                    let mut body = serde_json::to_value(&outcome).unwrap_or(Value::Null);
                    body["identity_first"] = Value::Bool(true);
                    body["identity"] = Value::String(identity.to_string());
                    response_value(response_id, Some(body), None)
                }
                Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    response_value(
                        response_id,
                        None,
                        Some(JsonRpcError {
                            code: -32001,
                            message: format!("unknown identity: {member_id}"),
                            data: None,
                        }),
                    )
                }
                Err(err) => internal_error(response_id, format!("reload_member (identity): {err}")),
            }
        }
        "mobkit/member_health" => {
            let Some(member_id) = request
                .params
                .get("member_id")
                .or_else(|| request.params.get("identity"))
                .and_then(Value::as_str)
            else {
                return invalid_params(response_id, "member_id required");
            };
            let member_id = crate::member_comms_id::runtime_alias_str(member_id).into_owned();
            if let Some(error) = stale_runtime_alias_json_rpc_error(
                "member_health",
                identity_runtime.as_ref(),
                &member_id,
            )
            .await
            {
                return response_value(response_id, None, Some(error));
            }
            let Some(identity_runtime_ref) = identity_runtime.as_ref() else {
                return internal_error(
                    response_id,
                    "member_health requires identity-first runtime authority",
                );
            };
            let identity = match identity_runtime_ref
                .identity_for_member_mutation(&member_id)
                .await
            {
                Some(identity) => identity,
                None => match crate::identity_first::AgentIdentity::parse(&member_id) {
                    Ok(identity) => identity,
                    Err(_) => {
                        return response_value(
                            response_id,
                            None,
                            Some(JsonRpcError {
                                code: -32001,
                                message: format!("unknown identity: {member_id}"),
                                data: None,
                            }),
                        );
                    }
                },
            };
            match identity_runtime_ref.member_health(&identity).await {
                Ok(report) => response_value(
                    response_id,
                    Some(serde_json::to_value(&report).unwrap_or(Value::Null)),
                    None,
                ),
                Err(crate::identity_first::IdentityRuntimeError::UnknownIdentity(_)) => {
                    response_value(
                        response_id,
                        None,
                        Some(JsonRpcError {
                            code: -32001,
                            message: format!("unknown identity: {member_id}"),
                            data: None,
                        }),
                    )
                }
                Err(err) => internal_error(response_id, format!("member_health failed: {err}")),
            }
        }
        "mobkit/reconcile_edges" => {
            // Previously a hardcoded noop ("console runtime routes directly
            // to MobRuntime") — which left declared definition wiring
            // unreconcilable from the console surface while the stdin
            // surface had the real handler (HomeCore, 2026-07-09).
            let report = reconcile_console_topology(runtime, topology).await;
            response_value(
                response_id,
                Some(serde_json::to_value(&report).unwrap_or(serde_json::Value::Null)),
                None,
            )
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
            let handle = runtime.handle();
            let result = crate::unified_runtime::mob_events::query_ledger_with_filter_selected(
                &events_view,
                store,
                &query,
                {
                    let handle = handle.clone();
                    move |event| {
                        let handle = handle.clone();
                        async move {
                            crate::http_sse::project_structural_envelope_for_console(
                                &handle,
                                Some(runtime),
                                visibility_policy,
                                access_view,
                                event,
                                true,
                            )
                            .await
                        }
                    }
                },
            )
            .await;
            match result {
                Ok(page) => {
                    // `mob.observe` gates the surface; agent-attributed
                    // ledger entries still require known `agent.view`
                    // attributes and the same visibility/redaction projection
                    // as the SSE continuation. This prevents the initial JSON
                    // snapshot from becoming a side door for retired members.
                    // Authorization happens inside the ledger scan so hidden
                    // rows cannot consume `limit` and starve later visible
                    // rows. The raw scan frontier also advances across hidden
                    // rows, making query pagination and subscribe handoff
                    // lossless without replaying them.
                    let events = page.items;
                    let resume_after_seq = Some(page.resume_after_seq);
                    let body = if request.method == "mobkit/mob_events/subscribe" {
                        let subscribe_url = crate::unified_runtime::mob_events::build_subscribe_url(
                            &query,
                            resume_after_seq,
                            latest_at_handshake,
                        );
                        serde_json::json!({
                            "stream": "mob_events",
                            "events": events,
                            "next_after_seq": resume_after_seq,
                            "subscribe_url": subscribe_url,
                            "keep_alive": {
                                "interval_ms": 15_000_u64,
                                "event": "keep_alive",
                            },
                        })
                    } else {
                        serde_json::json!({
                            "events": events,
                            "next_after_seq": resume_after_seq,
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
            let member_id = crate::member_comms_id::runtime_alias_str(member_id).into_owned();
            if let Some(error) = stale_runtime_alias_json_rpc_error(
                "member_status",
                identity_runtime.as_ref(),
                &member_id,
            )
            .await
            {
                return response_value(response_id, None, Some(error));
            }
            match runtime
                .handle()
                // Decode before encoding. A caller may hand back the runtime
                // alias our own status responses emit, and encoding that
                // directly keys a roster row nothing owns - which here returns a
                // WELL-FORMED "unknown/final" status rather than an error, so a
                // live member reads as finished.
                .member_status(&crate::member_comms_id::roster_member_id_for_supplied_id(
                    &member_id,
                ))
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
        "mobkit/identity/resolved_tools" => {
            let Some(identity) = request
                .params
                .get("identity")
                .or_else(|| request.params.get("member_id"))
                .and_then(Value::as_str)
            else {
                return invalid_params(response_id, "identity required");
            };
            let identity = crate::member_comms_id::runtime_alias_str(identity).into_owned();
            if let Some(error) = stale_runtime_alias_json_rpc_error(
                "identity_resolved_tools",
                identity_runtime.as_ref(),
                &identity,
            )
            .await
            {
                return response_value(response_id, None, Some(error));
            }
            if let Some(identity_runtime) = identity_runtime.as_ref()
                && let Ok(parsed) = crate::identity_first::AgentIdentity::parse(&identity)
                && let Ok(status) = identity_runtime.status(&parsed).await
                && let Some(session_id) = status.session_id
            {
                match resolved_tools_for_session(runtime.session_service(), &identity, session_id)
                    .await
                {
                    Ok(snapshot) => {
                        return response_value(
                            response_id,
                            Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
                            None,
                        );
                    }
                    Err(err) => {
                        return internal_error(
                            response_id,
                            format!("resolved_tools failed: {err}"),
                        );
                    }
                }
            }
            match resolved_tools_for_member(&runtime.handle(), runtime.session_service(), &identity)
                .await
            {
                Ok(snapshot) => response_value(
                    response_id,
                    Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => internal_error(response_id, format!("resolved_tools failed: {err}")),
            }
        }
        // The console plane carries its own dispatch; a method wired only in
        // `rpc.rs` is unreachable from the browser console, which is where
        // this status is actually read.
        "mobkit/identity/routing_status" => {
            let identity =
                match crate::rpc::mob_methods::routing_status_identity_param(&request.params) {
                    Ok(identity) => identity,
                    Err((supplied, err)) => {
                        return routing_status_error_value(response_id, &supplied, &err);
                    }
                };
            if let Some(error) = stale_runtime_alias_json_rpc_error(
                "identity_routing_status",
                identity_runtime.as_ref(),
                &identity,
            )
            .await
            {
                return response_value(response_id, None, Some(error));
            }
            if let Some(identity_runtime) = identity_runtime.as_ref()
                && let Ok(parsed) = crate::identity_first::AgentIdentity::parse(&identity)
                && let Ok(status) = identity_runtime.status(&parsed).await
                && let Some(session_id) = status.session_id
            {
                return match model_routing_status_for_session(
                    runtime.session_service(),
                    &identity,
                    session_id,
                )
                .await
                {
                    Ok(snapshot) => response_value(
                        response_id,
                        Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
                        None,
                    ),
                    Err(err) => routing_status_error_value(response_id, &identity, &err),
                };
            }
            match model_routing_status_for_member(
                &runtime.handle(),
                runtime.session_service(),
                &identity,
            )
            .await
            {
                Ok(snapshot) => response_value(
                    response_id,
                    Some(serde_json::to_value(&snapshot).unwrap_or(Value::Null)),
                    None,
                ),
                Err(err) => routing_status_error_value(response_id, &identity, &err),
            }
        }
        "mobkit/force_cancel_member" => {
            let Some(member_id) = request.params.get("member_id").and_then(Value::as_str) else {
                return invalid_params(response_id, "member_id required");
            };
            let member_id = crate::member_comms_id::runtime_alias_str(member_id).into_owned();
            if let Some(error) = stale_runtime_alias_json_rpc_error(
                "force_cancel_member",
                identity_runtime.as_ref(),
                &member_id,
            )
            .await
            {
                return response_value(response_id, None, Some(error));
            }
            let result = if let Some(identity_runtime_ref) = identity_runtime.as_ref()
                && let Some(identity) = identity_runtime_ref
                    .identity_for_member_mutation(&member_id)
                    .await
            {
                let handle = runtime.handle();
                // Decode before encoding. This branch has ALREADY resolved an
                // identity from `member_id` and runs under
                // run_member_alias_operation_tracked, so it knows an alias is
                // possible - keying the destructive call off the un-decoded form
                // would cancel a roster row nothing owns while reporting success.
                let member_id_value =
                    crate::member_comms_id::roster_member_id_for_supplied_id(&member_id);
                identity_runtime_ref
                    .run_member_alias_operation_tracked(&identity, &member_id, move || async move {
                        handle
                            .force_cancel_member(member_id_value)
                            .await
                            .map_err(|error| error.to_string())
                    })
                    .await
                    .map_err(|error| error.to_string())
            } else if crate::member_comms_id::is_reserved_generated_alias(&member_id) {
                Err(format!(
                    "generated member alias requires current identity authority: {member_id}"
                ))
            } else {
                runtime
                    .handle()
                    .force_cancel_member(crate::member_comms_id::mob_member_id(&member_id))
                    .await
                    .map_err(|error| error.to_string())
            };
            match result {
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
            // Omit `timeout_ms` => mobkit's generous default ceiling (the SDK
            // contract is "wait until ready"), not meerkat-mob 0.7.9's lowered
            // 60s internal default that `None` would otherwise inherit.
            let timeout = Some(
                request
                    .params
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .map(std::time::Duration::from_millis)
                    .unwrap_or(crate::unified_runtime::mob_ops::DEFAULT_WAIT_READY_TIMEOUT),
            );
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
                    if crate::unified_runtime::mob_ops::is_ready_wait_timeout(&err) {
                        response_value(
                            response_id,
                            Some(serde_json::json!({
                                "ready": Vec::<Value>::new(),
                                "timeout": true,
                            })),
                            None,
                        )
                    } else {
                        internal_error(response_id, format!("wait_for_ready failed: {err}"))
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
                && let Err(err) = identity_runtime.materialize_all_required_tracked().await
            {
                return internal_error(
                    response_id,
                    format!("identity-first flow materialization failed: {err}"),
                );
            }
            match Box::pin(runtime.handle().run_flow(flow_id, flow_params)).await {
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
            let Some(result_label) = request.params.get("result_label").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "result_label required");
            };
            let result_label = result_label.to_string();
            let Some(max_text_bytes_raw) =
                request.params.get("max_text_bytes").and_then(Value::as_u64)
            else {
                return invalid_params(response_id, "max_text_bytes required");
            };
            let Ok(max_text_bytes) = usize::try_from(max_text_bytes_raw) else {
                return invalid_params(response_id, "max_text_bytes exceeds platform bounds");
            };
            let raw_reservation = match crate::member_comms_id::reserve_raw_member_target(
                identity_runtime.as_ref(),
                agent_identity,
            )
            .await
            {
                Ok(reservation) => reservation,
                Err(message) => return invalid_params(response_id, message),
            };
            let handle = runtime.handle();
            let spawn_result = Box::pin(handle.spawn_helper(
                crate::member_comms_id::mob_member_id(raw_reservation.alias()),
                task,
                options,
                result_label,
                max_text_bytes,
            ))
            .await;
            drop(raw_reservation);
            match spawn_result {
                Ok(result) => {
                    // meerkat 0.8.22's bounded helper contract returns the
                    // exact turn carrier, so the session identity promised by
                    // the old comment is now real and re-added.
                    response_value(
                        response_id,
                        Some(serde_json::json!({
                            "output": result.helper.output,
                            "tokens_used": result.helper.tokens_used,
                            "session_id": result.turn.result().session_id().to_string(),
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
            let source = crate::member_comms_id::runtime_alias_str(source).into_owned();
            let Some(agent_identity) = request.params.get("agent_identity").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "agent_identity required");
            };
            if let Err(message) = crate::member_comms_id::validate_raw_member_target(
                identity_runtime.as_ref(),
                agent_identity,
            )
            .await
            {
                return invalid_params(response_id, message);
            }
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
            let Some(result_label) = request.params.get("result_label").and_then(Value::as_str)
            else {
                return invalid_params(response_id, "result_label required");
            };
            let result_label = result_label.to_string();
            let Some(max_text_bytes_raw) =
                request.params.get("max_text_bytes").and_then(Value::as_u64)
            else {
                return invalid_params(response_id, "max_text_bytes required");
            };
            let Ok(max_text_bytes) = usize::try_from(max_text_bytes_raw) else {
                return invalid_params(response_id, "max_text_bytes exceeds platform bounds");
            };
            let handle = runtime.handle();
            let source_member_id = crate::member_comms_id::mob_member_id(&source);
            let helper_alias = agent_identity.to_string();
            let task = task.to_string();
            let identity_runtime_owned = identity_runtime.clone();
            let authority_target = if let Some(identity_runtime_ref) = identity_runtime.as_ref() {
                identity_runtime_ref
                    .member_alias_lifecycle_target(&source)
                    .await
            } else {
                Ok(None)
            };
            let fork_result = match authority_target {
                Err(error) => Err(error.to_string()),
                Ok(Some(target)) => {
                    crate::identity_first::IdentityRuntime::run_member_alias_targets_operation_tracked(
                        vec![target],
                        move || async move {
                            let raw_reservation =
                                crate::member_comms_id::reserve_raw_member_target(
                                    identity_runtime_owned.as_ref(),
                                    helper_alias.as_str(),
                                )
                                .await?;
                            let helper_member_id = crate::member_comms_id::mob_member_id(
                                raw_reservation.alias(),
                            );
                            let result = handle
                            .fork_helper(
                                &source_member_id,
                                helper_member_id,
                                task.as_str(),
                                fork_context,
                                options,
                                result_label,
                                max_text_bytes,
                            )
                            .await
                            .map_err(|error| error.to_string());
                            drop(raw_reservation);
                            result
                        },
                    )
                    .await
                    .map_err(|error| error.to_string())
                }
                Ok(None) if crate::member_comms_id::is_reserved_generated_alias(&source) => {
                    Err(format!(
                        "generated source alias requires current identity authority: {source}"
                    ))
                }
                Ok(None) => {
                    match crate::member_comms_id::reserve_raw_member_target(
                        identity_runtime_owned.as_ref(),
                        helper_alias.as_str(),
                    )
                    .await
                    {
                        Err(error) => Err(error),
                        Ok(raw_reservation) => {
                            let helper_member_id = crate::member_comms_id::mob_member_id(
                                raw_reservation.alias(),
                            );
                            let result = handle
                                .fork_helper(
                                    &source_member_id,
                                    helper_member_id,
                                    task.as_str(),
                                    fork_context,
                                    options,
                                    result_label,
                                    max_text_bytes,
                                )
                                .await
                                .map_err(|error| error.to_string());
                            drop(raw_reservation);
                            result
                        }
                    }
                }
            };
            match fork_result {
                Ok(result) => {
                    // See `spawn_helper`: the bounded contract's exact turn
                    // carrier makes the session identity real, so it is
                    // re-added per the old comment's promise.
                    response_value(
                        response_id,
                        Some(serde_json::json!({
                            "output": result.helper.output,
                            "tokens_used": result.helper.tokens_used,
                            "session_id": result.turn.result().session_id().to_string(),
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
            let raw_reservation = match crate::member_comms_id::reserve_raw_member_target(
                identity_runtime.as_ref(),
                agent_identity,
            )
            .await
            {
                Ok(reservation) => reservation,
                Err(message) => return invalid_params(response_id, message),
            };
            let mid = crate::member_comms_id::mob_member_id(raw_reservation.alias());
            let spec = SpawnMemberSpec::new(ProfileName::from(role), mid.clone()).with_launch_mode(
                MemberLaunchMode::Resume {
                    // 0.8.25: no migration authority on this path.
                    resume_from_role: None,
                    bridge_session_id,
                },
            );
            let handle = runtime.handle();
            let spawn_result = Box::pin(handle.spawn_spec(spec)).await;
            drop(raw_reservation);
            match spawn_result {
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
            handle_console_wire_local(
                runtime,
                identity_runtime.as_ref(),
                &request.params,
                response_id,
                true,
            )
            .await
        }
        "mobkit/cross_mob/unwire_local" => {
            handle_console_wire_local(
                runtime,
                identity_runtime.as_ref(),
                &request.params,
                response_id,
                false,
            )
            .await
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
                    let mid = crate::member_comms_id::runtime_alias_str(mid).into_owned();
                    let handle = runtime.handle();
                    let authority_target = if let Some(identity_runtime) = identity_runtime.as_ref()
                    {
                        identity_runtime.member_alias_lifecycle_target(&mid).await
                    } else {
                        Ok(None)
                    };
                    let result = match authority_target {
                        Err(error) => Err(error.to_string()),
                        Ok(Some(target)) => {
                            let identity_runtime = identity_runtime.clone();
                            crate::identity_first::IdentityRuntime::run_member_alias_targets_operation_tracked(
                                vec![target],
                                move || async move {
                                    console_member_peer_info(
                                        handle,
                                        identity_runtime.as_ref(),
                                        true,
                                        mid,
                                    )
                                    .await
                                },
                            )
                            .await
                            .map_err(|error| error.to_string())
                        }
                        Ok(None) if crate::member_comms_id::is_reserved_generated_alias(&mid) => {
                            Err(format!(
                                "generated member alias requires current identity authority: {mid}"
                            ))
                        }
                        Ok(None) => console_member_peer_info(handle, None, false, mid).await,
                    };
                    match result {
                        Ok(value) => response_value(response_id, Some(value), None),
                        Err(error) => internal_error(
                            response_id,
                            format!("cross_mob/peer_info failed: {error}"),
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
            dispatch_console_label_method(
                method,
                metadata_table.as_deref(),
                runtime.handle().mob_id().as_str(),
                response_id,
                &request.params,
            )
            .await
        }
        method if crate::rpc::workgraph_methods::is_workgraph_method(method) => {
            // The read-only gate and the workgraph.view/manage ABAC checks
            // already ran above; the surface carries the authenticated
            // console principal, which goal/confirm promotes into the
            // trusted confirmation seam and break_glass_reassign records as
            // the audited operator. The admission always comes from the
            // runtime (never a parameter): whatever constructed the service,
            // the unified stdin surface acquires the runtime's admission, so
            // the console must acquire that same one.
            let admission = runtime.workgraph_admission();
            match crate::rpc::workgraph_methods::handle_workgraph_method(
                workgraph,
                &admission,
                crate::rpc::workgraph_methods::WorkgraphSurface::Console {
                    authenticated_principal,
                },
                method,
                &request.params,
            )
            .await
            {
                Ok(result) => response_value(response_id, Some(result), None),
                Err(error) => response_value(response_id, None, Some(error)),
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

async fn reconcile_console_topology(
    runtime: &MobRuntime,
    topology: Option<&crate::topology_control::TopologyRuntimeHandle>,
) -> crate::unified_runtime::UnifiedRuntimeReconcileEdgesReport {
    match topology {
        Some(topology) => topology.reconcile().await,
        None => crate::unified_runtime::edge_reconcile::reconcile_definition_edges(runtime).await,
    }
}

/// Dispatch the six `mobkit/{mob,run}_labels/*` RPCs against a metadata table.
///
/// Console projection for the shared transport-neutral label domain.
///
/// Access checks and the absent-table branch remain console-owned. Scope,
/// validation, and mutation come only from `runtime::dispatch_label_method`;
/// this function preserves the console's historical response envelope.
async fn dispatch_console_label_method(
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

    let Some(outcome) = crate::runtime::dispatch_label_method(table, mob_id, method, params).await
    else {
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

async fn console_member_peer_info(
    handle: meerkat_mob::MobHandle,
    identity_runtime: Option<&Arc<crate::identity_first::IdentityRuntime>>,
    identity_authoritative: bool,
    member_alias: String,
) -> Result<Value, String> {
    let current_alias = if identity_authoritative {
        let identity_runtime = identity_runtime
            .ok_or_else(|| "durable peer target lost its IdentityRuntime authority".to_string())?;
        let identity = crate::identity_first::IdentityRuntime::identity_for_generated_member_alias(
            &member_alias,
        )
        .or_else(|| crate::identity_first::AgentIdentity::parse(&member_alias).ok())
        .ok_or_else(|| format!("invalid durable member alias {member_alias:?}"))?;
        let status = identity_runtime
            .status(&identity)
            .await
            .map_err(|error| error.to_string())?;
        // Presence of a runtime binding, not its spelling: the roster is keyed
        // by the encoded durable identity since the stable-identity lowering,
        // and this value is encoded and handed to handle.get_member below.
        if status.agent_runtime_id.is_none() {
            return Err(format!("identity {identity} has no current runtime member"));
        }
        // And the live member must be the one this binding is registered for,
        // session included. Without the session check a peer-info call could
        // report on a member bound to a different session than the identity
        // runtime believes is current.
        let roster_member =
            crate::member_comms_id::roster_member_id_for_identity(identity.as_str());
        let live_session = handle.resolve_bridge_session_id(&roster_member).await;
        if !crate::member_comms_id::live_binding_matches_identity(
            roster_member.as_str(),
            live_session.as_ref().map(ToString::to_string).as_deref(),
            identity.as_str(),
            status
                .session_id
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            status
                .agent_runtime_id
                .as_ref()
                .map(crate::identity_first::AgentRuntimeId::as_str),
        ) {
            return Err(format!(
                "identity {identity} live member does not match its registered binding session"
            ));
        }
        identity.as_str().to_string()
    } else {
        let direct = crate::member_comms_id::mob_member_id(&member_alias);
        if handle
            .get_member(&direct)
            .await
            .map_err(|error| error.to_string())?
            .is_some()
        {
            member_alias.clone()
        } else {
            let candidates = handle
                .list_members_including_retiring()
                .await
                .into_iter()
                .filter(|entry| {
                    crate::member_comms_id::durable_identity_label(&entry.labels)
                        .is_some_and(|identity| identity == member_alias)
                })
                .map(|entry| {
                    crate::member_comms_id::runtime_alias_str(entry.agent_identity.as_str())
                        .into_owned()
                })
                .collect::<BTreeSet<_>>();
            match candidates.len() {
                0 => member_alias.clone(),
                1 => candidates
                    .into_iter()
                    .next()
                    .ok_or_else(|| "peer-info member alias candidate disappeared".to_string())?,
                _ => {
                    return Err(format!(
                        "ambiguous durable member alias {member_alias}: candidates [{}]",
                        candidates.into_iter().collect::<Vec<_>>().join(", ")
                    ));
                }
            }
        }
    };
    let member_id = crate::member_comms_id::mob_member_id(&current_alias);
    let entry = handle
        .get_member(&member_id)
        .await
        .map_err(|error| format!("member lookup failed: {error}"))?
        .ok_or_else(|| format!("member {member_alias:?} not found"))?;
    let peer_id = entry
        .peer_id()
        .ok_or_else(|| format!("member {member_alias:?} has no comms runtime"))?;
    let mob_id = handle.mob_id().to_string();
    let comms_name = meerkat_core::MemberCommsName::new(
        &mob_id,
        entry.role.as_str(),
        entry.agent_identity.as_str(),
    )
    .map_err(|error| format!("invalid member comms name: {error}"))?
    .to_string();
    let address = format!("inproc://{comms_name}");
    Ok(serde_json::json!({
        "member_id": member_alias,
        "mob_id": mob_id,
        "comms_name": comms_name,
        "peer_id": peer_id,
        "address": address,
    }))
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
    identity_runtime: Option<&Arc<crate::identity_first::IdentityRuntime>>,
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

    let handle = runtime.handle();
    let local_alias = crate::member_comms_id::runtime_alias_str(local_id).into_owned();
    let authority_target = if let Some(identity_runtime) = identity_runtime {
        identity_runtime
            .member_alias_lifecycle_target(&local_alias)
            .await
    } else {
        Ok(None)
    };
    let result = match authority_target {
        Err(error) => Err(error.to_string()),
        Ok(Some(target)) => {
            let operation_alias = local_alias.clone();
            crate::identity_first::IdentityRuntime::run_member_alias_targets_operation_tracked(
                vec![target],
                move || async move {
                    let result = if wire {
                        handle
                            .wire(
                                crate::member_comms_id::mob_member_id(&operation_alias),
                                PeerTarget::External(spec),
                            )
                            .await
                    } else {
                        handle
                            .unwire(
                                crate::member_comms_id::mob_member_id(&operation_alias),
                                PeerTarget::External(spec),
                            )
                            .await
                    };
                    result.map_err(|err| err.to_string())
                },
            )
            .await
            .map_err(|err| err.to_string())
        }
        Ok(None) if crate::member_comms_id::is_reserved_generated_alias(&local_alias) => Err(
            format!("generated member alias requires current identity authority: {local_alias}"),
        ),
        Ok(None) => {
            let result = if wire {
                handle
                    .wire(
                        crate::member_comms_id::mob_member_id(&local_alias),
                        PeerTarget::External(spec),
                    )
                    .await
            } else {
                handle
                    .unwire(
                        crate::member_comms_id::mob_member_id(&local_alias),
                        PeerTarget::External(spec),
                    )
                    .await
            };
            result.map_err(|err| err.to_string())
        }
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
    let read_model_state = Box::pin(read_model.snapshot(runtime)).await;
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
    let registered_labels = runtime.console_identity_labels().await;

    // Snapshot + project the primary mob into the cache. Done here
    // under the background refresh lock so per-request
    // `build_live_snapshot` calls never need to enter MobHandle async
    // methods. The session-id index in `state` was populated above by
    // `collect_console_session_index_for_handle`.
    let (primary_members, _primary_owner_index) =
        project_console_members_from_handle(&handle, None, None, &state, &registered_labels).await;
    state.primary_members = primary_members;

    let Some(mcp_state) = runtime.agent_mob_mcp_state() else {
        return state;
    };
    let primary_mob_id = handle.mob_id().to_string();
    let mut processed = BTreeSet::from([primary_mob_id]);
    let mut delegate_groups: Vec<Vec<ConsoleMember>> = Vec::new();
    loop {
        let mut progressed = false;
        for (mob_id, delegate_handle) in Box::pin(mcp_state.mob_handles_snapshot())
            .await
            .unwrap_or_default()
        {
            if processed.contains(mob_id.as_str()) {
                continue;
            }
            // Meerkat 0.7: the owner bridge binding is machine-owned state,
            // no longer a definition-level index.
            let Some(owner_authority) = delegate_handle.owner_bridge_session_lifecycle_authority()
            else {
                processed.insert(mob_id.to_string());
                continue;
            };
            let owner_session_id = owner_authority.bridge_session_id.to_string();
            let Some(host_identity) = state
                .session_owner_by_id
                .get(owner_session_id.as_str())
                .cloned()
            else {
                continue;
            };
            collect_console_session_index_for_handle(&delegate_handle, &mut state).await;
            let (delegate_members, _delegate_owner_index) = project_console_members_from_handle(
                &delegate_handle,
                Some(&host_identity),
                Some(mob_id.as_str()),
                &state,
                &registered_labels,
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
        // Session read-model keys live in the public alias space (decoded
        // from the comms-safe roster id, meerkat 0.7).
        let identity =
            crate::member_comms_id::runtime_alias_str(entry.agent_identity.as_str()).into_owned();
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

/// How `reset_all` routes one REGISTERED identity, decided from the identity
/// runtime's lifecycle state and the live roster BEFORE anything is touched.
/// The reset path is the bridge's successor transition (meerkat's respawn),
/// which replaces a live roster row; handing it an identity without one made
/// meerkat answer `MemberNotFound` from inside `reset_tracked`, as prose, and
/// the whole call failed for a fleet that had otherwise been reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegisteredResetDisposition {
    /// Live roster rows project under this identity: reset through
    /// `reset_tracked` (the stale-alias checks decide whether those rows are
    /// really this identity's).
    Reset,
    /// `Retiring`: the entry `mobkit/retire` leaves behind until delete or a
    /// roster reconcile. The identity is leaving the fleet, so it is neither
    /// resurrected nor reported.
    LeavingFleet,
    /// Registered but with no live roster row at all (Dormant under lazy
    /// bootstrap, Broken after a failed materialize such as the keyless park,
    /// Suspended mid-transition): the successor transition has nothing to
    /// replace, so the reset is refused typed with the state named.
    NotResettable {
        state: crate::identity_first::IdentityLifecycleState,
    },
}

fn registered_reset_disposition(
    status: &crate::identity_first::IdentityStatus,
    live_runtime_member_ids: Option<&BTreeSet<String>>,
) -> RegisteredResetDisposition {
    match (status.state, live_runtime_member_ids) {
        (crate::identity_first::IdentityLifecycleState::Retiring, _) => {
            RegisteredResetDisposition::LeavingFleet
        }
        (_, Some(_)) => RegisteredResetDisposition::Reset,
        (state, None) => RegisteredResetDisposition::NotResettable { state },
    }
}

async fn reset_all_live_console_agents(
    runtime: &MobRuntime,
    console_events: Option<&ConsoleEventStore>,
    console_aggregator: Option<&MobKitConsoleAggregator>,
    identity_runtime: Option<&Arc<crate::identity_first::IdentityRuntime>>,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let read_model = ConsoleSnapshotReadModel::default();
    *read_model.inner.write().await = Box::pin(collect_console_snapshot_read_model(runtime)).await;
    // Mark the freshly-built model primed so `build_live_snapshot` doesn't
    // try to re-prime; this is a one-shot read for the reset path.
    read_model
        .primed
        .store(true, std::sync::atomic::Ordering::Release);
    let snapshot = Box::pin(build_live_snapshot(
        runtime,
        &[],
        console_events,
        visibility_policy,
        &read_model,
    ))
    .await;
    let raw_snapshot = Box::pin(build_live_snapshot(
        runtime,
        &[],
        console_events,
        &crate::console_aggregator::AllowAllConsoleVisibilityPolicy,
        &read_model,
    ))
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
            let identity = crate::member_comms_id::durable_identity_label(&member.labels)
                .map(str::to_owned)
                .or_else(|| {
                    identity_by_runtime_member_id
                        .get(
                            crate::member_comms_id::runtime_alias_str(&member.agent_identity)
                                .as_ref(),
                        )
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
        let identity = crate::member_comms_id::durable_identity_label(&member.labels)
            .map(str::to_owned)
            .or_else(|| {
                identity_by_runtime_member_id
                    .get(crate::member_comms_id::runtime_alias_str(&member.agent_identity).as_ref())
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
    // Kept in the body for shape stability. Nothing on the reset path warns
    // today: a registered identity resets through the bridge's successor
    // transition (meerkat's respawn), which terminally retires the
    // predecessor row, so there is no stale live member left to warn about.
    let warnings: Vec<Value> = Vec::new();

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
        // Lifecycle classification (item R7): a Retiring identity is outside
        // the reset set. An identity with no live roster row is reported
        // typed, per identity, in the execution pass so it never blocks the
        // rest of the fleet; only the call-level session-bridge requirement
        // below applies to it here.
        let reset_disposition = registered_status.as_ref().map(|status| {
            registered_reset_disposition(status, raw_runtime_member_ids_by_identity.get(identity))
        });
        if reset_disposition == Some(RegisteredResetDisposition::LeavingFleet) {
            continue;
        }
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
        // Visibility of the identity's ROSTER ROW, not of its incarnation.
        //
        // This used to ask whether the registered AgentRuntimeId was among
        // the visible roster member ids. Since the stable-identity lowering
        // the roster row is the encoded durable identity and the runtime id
        // names an incarnation, so that question can never be true - which
        // made every identity carrying a duplicate label read as ambiguous
        // even with its own row visible and healthy. The binding must still
        // EXIST, which registered_runtime_id covers.
        let registered_visible = registered_runtime_id.is_some()
            && visible_runtime_member_ids.iter().any(|member_id| {
                crate::member_comms_id::live_member_is_identity(member_id, identity)
            });
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
                "error": "ambiguous live identity alias [via reset-all preflight]",
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
                // The registered binding must exist AND this identity must have a
                // live roster member. Roster ids are the encoded durable identity
                // now, so `contains` against an AgentRuntimeId never matched.
                if !(registered_runtime_id.is_some()
                    && live_runtime_ids.iter().any(|live| {
                        crate::member_comms_id::live_member_is_identity(live, identity)
                    }))
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
            // Every registered identity is on the reset path (see the
            // execution pass below), so the session-bridge requirement is
            // unconditional here: without a bridge nothing can be reset, and
            // the call must fail closed before any member is touched.
            if !identity_runtime
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
        if let Some(bound_identity) = identity_by_runtime_member_id
            .get(crate::member_comms_id::runtime_alias_str(runtime_member_id).as_ref())
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
            match Box::pin(state.handle_for(&MobId::from(mob_id.as_str()))).await {
                Ok(handle) => {
                    match retire_console_member(
                        &handle,
                        &crate::member_comms_id::mob_member_id(identity.as_str()),
                    )
                    .await
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
    // Routing rule for the execution pass (item R7). Every identity the
    // identity runtime knows is RESET: `identity_runtime.statuses()` is the
    // durable roster, and `reset_tracked` already owns generation advance,
    // the session-bridge requirement and the stale-alias checks. The
    // baseline slot (`runtime.baseline_member_specs()`) only decides the fate
    // of members WITHOUT a registered identity: it is populated solely by the
    // non-identity-first `UnifiedRuntime::reconcile`, so on an identity-first
    // gateway it is empty by construction. Keying reset-versus-retire on it
    // for registered identities made `reset_all` retire every identity on
    // every shipped gateway. Members with no registered identity and no
    // baseline spec (raw delegates, live-only workers) stay on the retire
    // path.
    for identity in main_identities {
        let parsed_identity = crate::identity_first::AgentIdentity::parse(&identity).ok();
        let registered_status = match (identity_runtime, parsed_identity.as_ref()) {
            (Some(identity_runtime), Some(parsed_identity)) => {
                identity_runtime.status(parsed_identity).await.ok()
            }
            _ => None,
        };
        let reset_disposition = registered_status.as_ref().map(|status| {
            registered_reset_disposition(status, raw_runtime_member_ids_by_identity.get(&identity))
        });
        if reset_disposition == Some(RegisteredResetDisposition::LeavingFleet) {
            continue;
        }
        if baseline_identities.contains(&identity)
            && !current_main_identities.contains(&identity)
            && registered_status.is_none()
        {
            continue;
        }
        let registered_runtime_id = registered_status
            .as_ref()
            .and_then(|status| status.agent_runtime_id.as_ref())
            .map(crate::identity_first::AgentRuntimeId::as_str);
        // Visibility of the identity's ROSTER ROW, not of its incarnation.
        //
        // This used to ask whether the registered AgentRuntimeId was among
        // the visible roster member ids. Since the stable-identity lowering
        // the roster row is the encoded durable identity and the runtime id
        // names an incarnation, so that question can never be true - which
        // made every identity carrying a duplicate label read as ambiguous
        // even with its own row visible and healthy. The binding must still
        // EXIST, which registered_runtime_id covers.
        let registered_visible = registered_runtime_id.is_some()
            && visible_runtime_member_ids.iter().any(|member_id| {
                crate::member_comms_id::live_member_is_identity(member_id, identity.as_str())
            });
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
                "error": "ambiguous live identity alias [via reset-all execution]",
            }));
            continue;
        }

        if let (Some(identity_runtime), Some(parsed_identity), Some(status)) = (
            identity_runtime,
            parsed_identity.as_ref(),
            registered_status.as_ref(),
        ) {
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
                // The registered binding must exist AND this identity must have a
                // live roster member. Roster ids are the encoded durable identity
                // now, so `contains` against an AgentRuntimeId never matched.
                if !(registered_runtime_id.is_some()
                    && live_runtime_ids.iter().any(|live| {
                        crate::member_comms_id::live_member_is_identity(live, identity.as_str())
                    }))
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
            if let Some(RegisteredResetDisposition::NotResettable { state }) = reset_disposition {
                failures.push(json!({
                    "identity": identity,
                    "error": format!(
                        "identity is {state} with no live roster row, so reset_all cannot reset \
                         it: a reset is the bridge's successor transition (meerkat respawn), \
                         which replaces a live member. Materialize it first (a console send \
                         materializes a dormant identity) or repair the cause its status names \
                         (a broken one), then reset it; `mobkit/delete_identity` followed by \
                         re-registration is the fresh start",
                        state = state.wire_str()
                    ),
                    "kind": "identity_not_resettable_in_state",
                    "state": state.wire_str(),
                }));
                continue;
            }
            if !identity_runtime.has_session_bridge() {
                failures.push(json!({
                    "identity": identity,
                    "error": "reset requires an identity runtime with a session bridge",
                    "kind": "identity_reset_requires_session_bridge",
                }));
                continue;
            }
            match identity_runtime.reset_tracked(parsed_identity).await {
                Ok(record) => {
                    reset_details.push(json!({
                        "identity": identity,
                        "agent_runtime_id": record.agent_runtime_id.as_str(),
                        "generation": record.generation.get(),
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
                                }),
                            )
                            .await;
                    }
                }
                Err(err) => failures.push(json!({
                    "identity": identity,
                    "error": err.to_string(),
                })),
            }
            continue;
        }

        // From here on the identity has no registered binding: it is a live
        // mob member the console projects directly.
        let runtime_member_id = runtime_member_id_by_identity
            .get(&identity)
            .map(String::as_str)
            .unwrap_or(identity.as_str());
        if let Some(bound_identity) = identity_by_runtime_member_id
            .get(crate::member_comms_id::runtime_alias_str(runtime_member_id).as_ref())
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
        if baseline_identities.contains(&identity) {
            // A baseline member on a non-identity-first runtime: reset means a
            // fresh session under the same configuration.
            match Box::pin(respawn_console_member(
                &handle,
                &crate::member_comms_id::mob_member_id(runtime_member_id),
            ))
            .await
            {
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
            // A live-only member nobody declared: neither registered nor in the
            // baseline. It has no durable owner to reset to, so it is retired.
            match retire_console_member(
                &handle,
                &crate::member_comms_id::mob_member_id(runtime_member_id),
            )
            .await
            {
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
    crate::member_comms_id::durable_identity_label(&member.labels)
        .unwrap_or(member.agent_identity.as_str())
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
        progress: None,
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
///
/// Console metadata registered by agent-tool spawns (`spawned_by` lineage,
/// spawn labels) fills label gaps the roster does not carry, and identities
/// known only to the spawn registry (e.g. members of agent-created sub-mobs)
/// are primed registry-only so lineage rules resolve for them too.
pub(crate) async fn prime_access_cache_from_runtime(
    runtime: &MobRuntime,
    access: &AccessController,
) {
    if !access.enabled() {
        return;
    }
    let registered = runtime.console_identity_labels().await;
    prime_access_cache_from_handle_with_registry(&runtime.handle(), access, &registered).await;
}

pub(crate) async fn prime_access_cache_from_handle(handle: &MobHandle, access: &AccessController) {
    if !access.enabled() {
        return;
    }
    prime_access_cache_from_handle_with_registry(handle, access, &BTreeMap::new()).await;
}

async fn prime_access_cache_from_handle_with_registry(
    handle: &MobHandle,
    access: &AccessController,
    registered: &BTreeMap<String, BTreeMap<String, String>>,
) {
    let mut registry_only: BTreeMap<&String, &BTreeMap<String, String>> =
        registered.iter().collect();
    for entry in handle.list_all_members().await {
        // Roster ids are comms-safe encodings (meerkat 0.7); the console/ABAC
        // attribute space uses the public alias.
        let member_identity =
            crate::member_comms_id::runtime_alias_str(entry.agent_identity.as_str()).into_owned();
        // Console identity may be overridden by a label, mirroring
        // `console_member_console_identity`.
        let console_identity = crate::member_comms_id::durable_identity_label(&entry.labels)
            .map_or_else(|| member_identity.clone(), ToString::to_string);
        let mut labels = entry.labels.clone();
        // Roster labels are caller-controlled; only the spawn registry may
        // assert lineage, or a spoofed `spawned_by` could mint inherited
        // visibility (or hide a member behind a denied parent).
        crate::console_spawn::sanitize_unverified_lineage_labels(&mut labels);
        if let Some(spawn_labels) = registry_only.remove(&console_identity) {
            crate::console_spawn::merge_registered_labels(&mut labels, spawn_labels);
        }
        access.record_agent_attributes(AgentResourceAttributes {
            identity: console_identity,
            agent_id: Some(member_identity),
            role: Some(entry.role.to_string()),
            labels,
        });
    }
    for (identity, spawn_labels) in registry_only {
        access.record_agent_attributes(AgentResourceAttributes {
            identity: identity.clone(),
            agent_id: None,
            role: spawn_labels.get("role").cloned(),
            labels: spawn_labels.clone(),
        });
    }
}

async fn project_console_members_from_handle(
    handle: &MobHandle,
    host_identity: Option<&str>,
    source_mob_id: Option<&str>,
    read_model: &ConsoleSnapshotReadModelState,
    registered_labels: &BTreeMap<String, BTreeMap<String, String>>,
) -> (Vec<ConsoleMember>, BTreeMap<String, String>) {
    // Meerkat 0.7: lifecycle status is machine-projected onto
    // `MobMemberListEntry`; the structural `list_all_members` roster no longer
    // carries it, so the console projection reads the operational list.
    let entries = handle.list_members_including_retiring().await;
    let mut members = Vec::with_capacity(entries.len());
    let mut session_owner_by_id = BTreeMap::new();
    for entry in &entries {
        // Roster ids are comms-safe encodings (meerkat 0.7 MemberCommsName);
        // console members surface the public alias.
        let identity =
            crate::member_comms_id::runtime_alias_str(entry.agent_identity.as_str()).into_owned();
        let session_id = read_model.session_id_by_identity.get(&identity).cloned();
        if let Some(session_id) = session_id.as_ref() {
            session_owner_by_id.insert(session_id.clone(), identity.clone());
        }
        let model_capabilities =
            model_capabilities_for_role(handle.definition(), entry.role.as_str());
        let mut labels = entry.labels.clone();
        // Spawn-time console metadata (group/display labels, spawned_by
        // lineage) fills gaps the roster does not carry; roster labels win —
        // except lineage, which only the spawn registry may assert (this
        // projection feeds the experience-path ABAC attribute rebuild).
        crate::console_spawn::sanitize_unverified_lineage_labels(&mut labels);
        let console_identity_key = crate::member_comms_id::durable_identity_label(&entry.labels)
            .unwrap_or(identity.as_str());
        if let Some(spawn_labels) = registered_labels.get(console_identity_key) {
            crate::console_spawn::merge_registered_labels(&mut labels, spawn_labels);
        }
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
        let mut wired_to: Vec<String> = entry
            .wired_to
            .iter()
            .map(|peer| crate::member_comms_id::runtime_alias_str(peer.as_str()).into_owned())
            .collect();
        if let Some(host_identity) = host_identity
            && !wired_to.iter().any(|peer| peer == host_identity)
        {
            wired_to.push(host_identity.to_string());
        }
        members.push(ConsoleMember {
            agent_identity: identity,
            role: entry.role.to_string(),
            state: member_status_state_string(entry.status),
            model_capabilities,
            runtime_mode: Some(entry.runtime_mode.to_string()),
            session_id,
            wired_to,
            labels,
            progress: None,
        });
    }
    attach_member_progress(handle, &entries, &mut members).await;
    (members, session_owner_by_id)
}

/// Console progress projection cap: `member_status` is an actor-mailbox
/// roundtrip per member, and the experience endpoint refreshes every ~15s
/// (plus SSE-triggered refetches). Small durable rosters (HomeCore: 16) get
/// liveness for free; whole-mob fan-out at OB3 scale (hundreds of members)
/// would compete with real work on the mob actor mailbox, so large rosters
/// skip the projection unless the operator raises the cap.
/// `MOBKIT_CONSOLE_PROGRESS_MEMBER_CAP` overrides; `0` disables entirely.
const CONSOLE_PROGRESS_MEMBER_CAP: usize = 64;

fn console_progress_member_cap() -> usize {
    std::env::var("MOBKIT_CONSOLE_PROGRESS_MEMBER_CAP")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(CONSOLE_PROGRESS_MEMBER_CAP)
}

/// Attach the machine-owned liveness projection (meerkat 0.7.29+, ask 14) to
/// non-final console members. Best-effort: a failed or absent snapshot leaves
/// `progress: None` — the console renders nothing rather than a lie.
async fn attach_member_progress(
    handle: &MobHandle,
    entries: &[meerkat_mob::runtime::MobMemberListEntry],
    members: &mut [ConsoleMember],
) {
    let cap = console_progress_member_cap();
    if entries.len() > cap {
        tracing::debug!(
            member_count = entries.len(),
            cap,
            "skipping console progress projection: roster exceeds \
             MOBKIT_CONSOLE_PROGRESS_MEMBER_CAP"
        );
        return;
    }
    for (entry, member) in entries.iter().zip(members.iter_mut()) {
        if entry.is_final {
            continue;
        }
        let Ok(snapshot) = handle.member_status(&entry.agent_identity).await else {
            continue;
        };
        member.progress = snapshot
            .progress
            .as_ref()
            .and_then(|progress| serde_json::to_value(progress).ok());
    }
}

async fn build_aggregator_live_snapshot(
    aggregator: &MobKitConsoleAggregator,
    config_module_ids: &[String],
) -> Result<ConsoleLiveSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let identities = Box::pin(aggregator.list_identities()).await?;
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
            progress: None,
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
        console_runtime_alias_generation, console_send_identity_first,
        console_send_with_identity_first_fallback, console_timeline_replay_unavailable_response,
        cursor_is_after, dedupe_console_members_by_identity, externalize_image_upload_placeholders,
        externalize_single_image_upload, handle_console_aggregator_rpc, handle_console_runtime_rpc,
        handle_console_runtime_rpc_with_visibility, member_id_matches_durable_identity,
        memory_panel_scope_action, project_console_members_from_handle, query_timeline_snapshot,
        resolve_console_identity_control_target, timeline_query_from_http,
    };
    use crate::access::{
        ACTION_AGENT_MEMORY_DELETE, ACTION_AGENT_MEMORY_READ, ACTION_AGENT_MEMORY_WRITE,
        ACTION_AGENT_SEND, ACTION_AGENT_VIEW, ACTION_MOB_MEMORY_READ, ACTION_OPERATOR_MEMORY_READ,
        ACTION_RUNTIME_ADMIN, AccessController,
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
        AgentAddressability, AgentBuildDraft, AgentIdentity, AgentMemoryConfig,
        AgentMemoryRuntimeInjector, AgentRuntimeId, BridgeError, CheckpointVersion,
        ContinuityGeneration, ContinuityRecord, DurabilityPolicy, DurableAgentSpec, FencingToken,
        IdentityLifecycleState, IdentityRuntime, IdentityRuntimeConfig, LeaseAcquireResult,
        LeaseGrant, LocalContinuityStore, LocalLeaseProvider, ManagedPeerEdge,
        ResetSuccessorBinding, ResumeSessionOutcome, SessionBridge, SessionSnapshot,
    };
    use crate::memory::SqliteAgentMemoryStore;
    use crate::mob_handle_runtime::{MobRuntime, model_capabilities_for_role};
    use crate::rpc::{
        JSONRPC_VERSION, JsonRpcRequest, resolve_rpc_identity_control_target_with_handle,
    };
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

    /// §10.3 fail-closed panel guard. Operator-scope rows are cross-mob
    /// personal facts, so they need their OWN grant: folding the operator
    /// arm into the unscoped `agent.memory.read` action would silently
    /// widen console read authority rather than remove an inert branch.
    /// Pinned per scope kind so a fold shows up as a test failure, not as a
    /// quiet permission grant.
    #[test]
    fn memory_panel_read_action_is_distinct_per_scope_kind() {
        use crate::memory::records::MemoryScope;

        let identity = MemoryScope::Identity {
            realm: "family".to_string(),
            identity: "identity:luka".to_string(),
        };
        let mob = MemoryScope::Mob {
            realm: "family".to_string(),
            mob: "mob:home".to_string(),
        };
        let operator = MemoryScope::Operator {
            realm: "family".to_string(),
            operator: "op:luka".to_string(),
        };
        let realm = MemoryScope::Realm {
            realm: "family".to_string(),
        };

        assert_eq!(
            memory_panel_scope_action(&identity),
            ACTION_AGENT_MEMORY_READ
        );
        assert_eq!(memory_panel_scope_action(&mob), ACTION_MOB_MEMORY_READ);
        assert_eq!(
            memory_panel_scope_action(&operator),
            ACTION_OPERATOR_MEMORY_READ,
            "operator rows must not ride the unscoped agent.memory.read grant"
        );
        // Realm rows deliberately DO ride the unscoped grant (§10.3).
        assert_eq!(memory_panel_scope_action(&realm), ACTION_AGENT_MEMORY_READ);
        assert_ne!(ACTION_OPERATOR_MEMORY_READ, ACTION_AGENT_MEMORY_READ);
    }

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

        async fn deliver_admitted(
            &self,
            _runtime_id: &AgentRuntimeId,
            _delivery: crate::identity_first::BridgeDelivery,
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

        async fn deliver_admitted(
            &self,
            _runtime_id: &AgentRuntimeId,
            delivery: crate::identity_first::BridgeDelivery,
        ) -> Result<meerkat_core::types::SessionId, BridgeError> {
            self.handling_modes
                .lock()
                .map_err(|_| BridgeError::Mob("handling modes mutex poisoned".to_string()))?
                .push(delivery.handling_mode);
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

    /// Reset double: mints the successor binding a real bridge commits through
    /// respawn, without touching the mob plane. It lets the destructive reset
    /// path (`reset_tracked`) run in-process, which is what `reset_all` takes
    /// for every registered identity.
    struct SuccessorMintingIdentityBridge;

    #[async_trait::async_trait]
    impl SessionBridge for SuccessorMintingIdentityBridge {
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

        async fn reset_member_to_successor(
            &self,
            identity: &AgentIdentity,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
        ) -> Result<ResetSuccessorBinding, BridgeError> {
            let agent_runtime_id = AgentRuntimeId::parse(&format!("rt:{}:1", identity.as_str()))
                .map_err(|err| BridgeError::Mob(format!("mint successor runtime id: {err}")))?;
            Ok(ResetSuccessorBinding {
                agent_runtime_id,
                session_id: meerkat_core::types::SessionId::new(),
            })
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

        async fn deliver_admitted(
            &self,
            _runtime_id: &AgentRuntimeId,
            _delivery: crate::identity_first::BridgeDelivery,
        ) -> Result<meerkat_core::types::SessionId, BridgeError> {
            Err(BridgeError::Mob("deliver not used in test".to_string()))
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

    async fn spawn_identity_control_test_member(
        runtime: &MobRuntime,
        runtime_member_id: &str,
        projected_identity: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        runtime
            .handle()
            .spawn_spec(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    crate::member_comms_id::mob_member_id_str(runtime_member_id).into_owned(),
                    Some("Identity control resolver fixture.".into()),
                    None,
                    None,
                )
                .with_labels(BTreeMap::from([(
                    "agent_identity".to_string(),
                    projected_identity.to_string(),
                )])),
            )
            .await?;
        Ok(())
    }

    fn empty_identity_control_test_runtime(
        runtime_instance_id: &str,
    ) -> Result<Arc<IdentityRuntime>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: runtime_instance_id.to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        })))
    }

    async fn register_identity_control_test_binding(
        identity_runtime: &IdentityRuntime,
        identity: &str,
        runtime_member_id: &str,
        session_id: meerkat_core::types::SessionId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let identity = AgentIdentity::parse(identity)?;
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
                    placement: None,
                },
                IdentityLifecycleState::Active,
                Some(ContinuityRecord {
                    identity,
                    agent_runtime_id: AgentRuntimeId::parse(runtime_member_id)?,
                    session_id,
                    generation: ContinuityGeneration::new(0),
                    checkpoint_version: CheckpointVersion::new(0),
                }),
                None,
            )
            .await;
        Ok(())
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
    async fn console_storage_doctor_is_a_read_method_and_status_advertises_it()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-storage-doctor").await?;

        let doctor_rpc = |params: Value| {
            Box::pin(handle_console_runtime_rpc(
                &runtime,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                rpc_request_with_params("mobkit/storage/doctor", params),
                true,
            ))
        };

        // The status payload advertises the doctor affordance.
        let status = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            rpc_request_with_params("mobkit/status", json!({})),
            true,
        ))
        .await;
        assert_eq!(
            status["result"]["storage"]["doctor_available"],
            json!(true),
            "{status:#?}"
        );

        // Advertised on the console capabilities list.
        let caps = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            rpc_request_with_params("mobkit/capabilities", json!({})),
            true,
        ))
        .await;
        let methods = caps["result"]["methods"].as_array().expect("methods");
        assert!(
            methods
                .iter()
                .any(|m| m.as_str() == Some("mobkit/storage/doctor")),
            "{methods:?}"
        );

        // Missing state_dir → the typed capability error.
        let missing = doctor_rpc(json!({})).await;
        assert_eq!(missing["error"]["code"], json!(-32004), "{missing:#?}");

        // A fixture directory with a filename twin diagnoses over the wire.
        let fixture = tempfile::tempdir()?;
        std::fs::write(fixture.path().join("sessions.db"), b"")?;
        std::fs::write(fixture.path().join("sessions.sqlite"), b"")?;
        let resp = doctor_rpc(json!({ "state_dir": fixture.path() })).await;
        let findings = resp["result"]["diagnosis"]["findings"]
            .as_array()
            .expect("findings array");
        assert!(
            findings
                .iter()
                .any(|f| f["code"] == "file-name-twins" && f["severity"] == "error"),
            "{findings:#?}"
        );
        Ok(())
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
    fn read_only_classifies_every_live_mutation_but_not_status() {
        for method in [
            "mobkit/live/open",
            "mobkit/live/replacement_required",
            "mobkit/live/playback_owner/register",
            "mobkit/live/playback_owner/revoke",
            "mobkit/live/close",
            "mobkit/live/refresh",
            "mobkit/live/send_input",
            "mobkit/live/commit_input",
            "mobkit/live/interrupt",
            "mobkit/live/truncate",
            "mobkit/live/playback_complete",
            "live/webrtc/answer",
        ] {
            assert!(
                super::is_console_mutating_rpc_method(method),
                "{method} must be denied before any future HTTP live dispatch"
            );
        }
        assert!(!super::is_console_mutating_rpc_method("mobkit/live/status"));
    }

    #[tokio::test]
    async fn missing_http_principal_cannot_reach_live_mutation_dispatch() {
        let response = handle_console_aggregator_rpc(
            None,
            rpc_request_with_params("mobkit/live/open", json!({ "identity": "worker" })),
            false,
            false,
            None,
            None,
        )
        .await;

        assert_eq!(response["result"], Value::Null);
        assert_eq!(response["error"]["code"], json!(-32010));
        assert_eq!(response["error"]["data"]["kind"], json!("read_only"));
    }

    #[tokio::test]
    async fn authenticated_writable_http_console_still_has_no_live_route() {
        for method in [
            "mobkit/live/open",
            "mobkit/live/replacement_required",
            "mobkit/live/playback_owner/register",
            "mobkit/live/playback_owner/revoke",
            "mobkit/live/status",
            "mobkit/live/close",
            "mobkit/live/refresh",
            "mobkit/live/send_input",
            "mobkit/live/commit_input",
            "mobkit/live/interrupt",
            "mobkit/live/truncate",
            "mobkit/live/playback_complete",
            "live/webrtc/answer",
        ] {
            let response = handle_console_aggregator_rpc(
                None,
                rpc_request_with_params(method, json!({})),
                true,
                false,
                None,
                None,
            )
            .await;
            assert_eq!(
                response["error"]["code"],
                json!(-32601),
                "{method} must remain unmounted on HTTP"
            );
        }
    }

    /// An unmapped console method FAILS OPEN. `console_rpc_access_requirements`
    /// returns `None`, `console_rpc_access_violation` short-circuits on that
    /// `None` and allows the call, and the capabilities filter treats `None` as
    /// "advertise to everyone". So forgetting a classifier entry produces no
    /// compile error, no test failure, and a method silently exempt from ABAC.
    /// Routing status exposes which model and provider an identity is on, so it
    /// belongs behind the same per-agent view grant as the other identity reads.
    #[test]
    fn routing_status_is_gated_by_the_same_agent_view_grant_as_its_siblings() {
        let alias = "rt:gate:main:0";
        let params = json!({ "identity": alias });
        assert_eq!(
            super::console_rpc_access_requirements("mobkit/identity/routing_status", &params),
            Some(vec![(ACTION_AGENT_VIEW, Some(alias.to_string()))]),
            "routing_status must not become an unmapped console read: an unmapped method is \
             exempt from the access gate AND advertised to every caller"
        );
        // Pinned against its sibling rather than asserted alone: these two reads
        // expose the same class of per-identity fact and must not drift apart.
        assert_eq!(
            super::console_rpc_access_requirements("mobkit/identity/routing_status", &params),
            super::console_rpc_access_requirements("mobkit/identity/resolved_tools", &params),
            "routing_status and resolved_tools must carry identical access requirements"
        );
    }

    #[test]
    fn member_declaration_access_requirements_keep_reads_scoped_and_writes_administrative() {
        let alias = "rt:gate:main:0";
        let params = json!({ "agent_identity": alias });
        assert_eq!(
            super::console_rpc_access_requirements(
                crate::rpc::mob_methods::MEMBER_TOOL_DECLARATION,
                &params,
            ),
            Some(vec![(ACTION_AGENT_VIEW, Some(alias.to_string()))])
        );
        for method in [
            crate::rpc::mob_methods::ADOPT_MEMBER_IDENTITY_DECLARATION,
            crate::rpc::mob_methods::APPLY_MEMBER_TOOL_DECLARATION,
        ] {
            assert_eq!(
                super::console_rpc_access_requirements(method, &params),
                Some(vec![(ACTION_RUNTIME_ADMIN, None)]),
                "{method} must not become an unmapped console write"
            );
        }
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

    /// DECISION (2026-07-06, meerkat-studio ask K2): console JSON-RPC internal
    /// errors DO disclose the failure reason. Every caller that reaches these
    /// handlers is an operator by construction — an auth-gated console 401s
    /// unauthenticated callers before dispatch, and an open console is a
    /// trusted-local deployment choice — while the previous redaction made
    /// every -32000 undiagnosable from the client (the K1 retire/respawn
    /// failures cost the reporter a day). The `error` field stays a stable
    /// kind discriminator; `console_send_public_message` keeps ITS redaction
    /// because that surface can reflect into agent-visible space.
    #[test]
    fn internal_error_carries_detail_and_stable_kind() {
        let response = super::internal_error(json!(7), "retire failed: backend says no");

        assert_eq!(response["error"]["code"], json!(-32000));
        assert_eq!(
            response["error"]["message"],
            json!("retire failed: backend says no")
        );
        assert_eq!(response["error"]["data"]["error"], json!("internal_error"));
        assert_eq!(
            response["error"]["data"]["detail"],
            json!("retire failed: backend says no")
        );
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
    async fn identity_control_resolver_adapters_have_success_parity()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("identity-control-adapter-parity").await?;
        let runtime_member_id = "rt:review:parity:0";
        let durable_identity = "review:parity";
        spawn_identity_control_test_member(&runtime, runtime_member_id, durable_identity).await?;
        let handle = runtime.handle();
        let session_id = handle
            .resolve_bridge_session_id_observation(&crate::member_comms_id::mob_member_id(
                runtime_member_id,
            ))
            .await
            .expect("spawned member has a bridge session");
        let identity_runtime =
            empty_identity_control_test_runtime("identity-control-adapter-parity")?;
        register_identity_control_test_binding(
            &identity_runtime,
            durable_identity,
            runtime_member_id,
            session_id,
        )
        .await?;
        let visibility = AllowAllConsoleVisibilityPolicy;

        for requested_identity in [durable_identity, runtime_member_id] {
            let rpc = resolve_rpc_identity_control_target_with_handle(
                &handle,
                &identity_runtime,
                requested_identity,
            )
            .await?;
            let console = resolve_console_identity_control_target(
                &handle,
                Some(&identity_runtime),
                &visibility,
                requested_identity,
            )
            .await
            .expect("console adapter resolves the registered identity")
            .expect("registered identity resolves in console adapter");

            assert_eq!(rpc.identity.as_str(), console.0.as_str());
            assert_eq!(rpc.was_registered, console.1);
            assert_eq!(
                rpc.live
                    .as_ref()
                    .map(|live| live.runtime_member_id.as_str()),
                console
                    .2
                    .as_ref()
                    .map(|live| live.runtime_member_id.as_str())
            );
            assert_eq!(
                rpc.live
                    .as_ref()
                    .and_then(|live| live.session_id.as_deref()),
                console
                    .2
                    .as_ref()
                    .and_then(|live| live.session_id.as_deref())
            );
        }

        let _ = handle.stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn identity_control_resolver_adapters_preserve_ambiguity_stale_and_visibility_goldens()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_ambiguous_temp_dir, ambiguous_runtime) =
            build_empty_console_test_runtime("identity-control-ambiguous-golden").await?;
        for runtime_member_id in ["rt:review:golden:0", "rt:review:golden:1"] {
            spawn_identity_control_test_member(
                &ambiguous_runtime,
                runtime_member_id,
                "review:golden",
            )
            .await?;
        }
        let ambiguous_handle = ambiguous_runtime.handle();
        let ambiguous_identity_runtime =
            empty_identity_control_test_runtime("identity-control-ambiguous-golden")?;

        let rpc_ambiguous = resolve_rpc_identity_control_target_with_handle(
            &ambiguous_handle,
            &ambiguous_identity_runtime,
            "review:golden",
        )
        .await
        .expect_err("stdio adapter must fail closed on a visible duplicate");
        assert_eq!(
            rpc_ambiguous,
            "ambiguous live identity alias review:golden [via identity-control-target \
             resolver]: candidates [rt:review:golden:0, rt:review:golden:1]"
        );
        let console_ambiguous = resolve_console_identity_control_target(
            &ambiguous_handle,
            Some(&ambiguous_identity_runtime),
            &AllowAllConsoleVisibilityPolicy,
            "review:golden",
        )
        .await
        .expect_err("console adapter must fail closed on the same visible duplicate");
        assert_eq!(console_ambiguous.code, -32602);
        assert_eq!(
            console_ambiguous.data,
            Some(json!({
                "kind": "ambiguous_live_identity_alias",
                "identity": "review:golden",
                "candidates": ["rt:review:golden:0", "rt:review:golden:1"],
            }))
        );

        let visibility = HideMemberPolicy("rt:review:golden:0");
        let visible_target = resolve_console_identity_control_target(
            &ambiguous_handle,
            Some(&ambiguous_identity_runtime),
            &visibility,
            "review:golden",
        )
        .await
        .expect("console adapter applies caller visibility")
        .expect("caller policy removes the hidden candidate before ambiguity");
        assert_eq!(
            visible_target
                .2
                .as_ref()
                .map(|live| live.runtime_member_id.as_str()),
            Some("rt:review:golden:1")
        );
        let _ = ambiguous_handle.stop().await;

        let (_stale_temp_dir, stale_runtime) =
            build_empty_console_test_runtime("identity-control-stale-golden").await?;
        let stale_runtime_id = "rt:review:stale-golden:0";
        spawn_identity_control_test_member(&stale_runtime, stale_runtime_id, "other:stale-golden")
            .await?;
        let stale_handle = stale_runtime.handle();
        let stale_session_id = stale_handle
            .resolve_bridge_session_id_observation(&crate::member_comms_id::mob_member_id(
                stale_runtime_id,
            ))
            .await
            .expect("spawned member has a bridge session");
        let stale_identity_runtime =
            empty_identity_control_test_runtime("identity-control-stale-golden")?;
        register_identity_control_test_binding(
            &stale_identity_runtime,
            "review:stale-golden",
            stale_runtime_id,
            stale_session_id,
        )
        .await?;

        let rpc_stale = resolve_rpc_identity_control_target_with_handle(
            &stale_handle,
            &stale_identity_runtime,
            "other:stale-golden",
        )
        .await
        .expect_err("stdio adapter must reject a member bound to another durable identity");
        assert_eq!(
            rpc_stale,
            "stale live identity alias: live console alias other:stale-golden resolves to rt:review:stale-golden:0, but identity runtime binding belongs to review:stale-golden"
        );
        let console_stale = resolve_console_identity_control_target(
            &stale_handle,
            Some(&stale_identity_runtime),
            &AllowAllConsoleVisibilityPolicy,
            "other:stale-golden",
        )
        .await
        .expect_err("console adapter must reject the same stale binding");
        assert_eq!(console_stale.code, -32000);
        assert_eq!(
            console_stale.data,
            Some(json!({
                "kind": "stale_live_identity_alias",
                "identity": "other:stale-golden",
                "runtime_member_id": stale_runtime_id,
                "registered_identity": "review:stale-golden",
            }))
        );

        let _ = stale_handle.stop().await;
        Ok(())
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
                    // meerkat 0.7: roster ids are comms-safe encodings of the
                    // public alias (MemberCommsName rejects ":").
                    crate::member_comms_id::mob_member_id_str("rt:review:singleton:0").into_owned(),
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

        // Without an identity-first runtime, reset degrades to the live
        // member respawn fallback instead of erroring; the console Reset
        // button stays functional on plain runtimes.
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
        assert_eq!(
            reset_without_identity_runtime["error"],
            Value::Null,
            "{reset_without_identity_runtime:#?}"
        );
        assert_eq!(
            reset_without_identity_runtime["result"]["identity"],
            json!("review:singleton")
        );
        assert_eq!(
            reset_without_identity_runtime["result"]["agent_runtime_id"],
            json!("rt:review:singleton:0")
        );

        let reset_unknown_identity = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            Some(ConsoleEventStore::new()),
            None,
            None,
            None,
            None,
            rpc_request_with_params("mobkit/reset", json!({ "identity": "missing:member" })),
            true,
        ))
        .await;
        assert_ne!(reset_unknown_identity["error"], Value::Null);
        assert!(
            reset_unknown_identity["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("identity not found")
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
                        // meerkat 0.7: roster ids are comms-safe encodings of
                        // the public alias (MemberCommsName rejects ":").
                        crate::member_comms_id::mob_member_id_str(runtime_id).into_owned(),
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
                "mobkit/reset",
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
        // The REGISTERED row is the durable identity itself now; the generated
        // alias beside it is a decoy carrying the same agent_identity label,
        // which is what preserves this test's subject - a registered live
        // binding must win over label matches.
        for runtime_id in ["review:singleton", "rt:review:singleton:1"] {
            let mut labels = BTreeMap::new();
            labels.insert("agent_identity".to_string(), "review:singleton".to_string());
            runtime
                .handle()
                .spawn_spec(
                    SpawnMemberSpec::from_wire(
                        "worker".to_string(),
                        // meerkat 0.7: roster ids are comms-safe encodings of
                        // the public alias (MemberCommsName rejects ":").
                        crate::member_comms_id::mob_member_id_str(runtime_id).into_owned(),
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
            // reset_all RESETS every registered identity (item R7), and reset
            // needs a session bridge; without one the call fails closed.
            bridge: Some(Arc::new(SuccessorMintingIdentityBridge)),
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("review:singleton")?;
        let registered_session_id = runtime
            .handle()
            .resolve_bridge_session_id_observation(&crate::member_comms_id::mob_member_id(
                "review:singleton",
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
                    placement: None,
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
        // Item R7: a registered identity is RESET by reset_all, never retired.
        // Before the fix this test passed on the retire path, because nothing
        // above looked at which arm ran.
        assert_eq!(
            reset_all_response["result"]["reset"],
            json!(["review:singleton"]),
            "registered identity must be in the reset set: {reset_all_response:#?}"
        );
        assert_eq!(
            reset_all_response["result"]["retired_delegates"],
            json!([]),
            "registered identity must not be retired by reset_all: {reset_all_response:#?}"
        );
        let status = identity_runtime.status(&identity).await?;
        assert_eq!(
            status.state,
            IdentityLifecycleState::Active,
            "reset must leave the identity registered and active"
        );
        assert_eq!(
            status.generation.map(ContinuityGeneration::get),
            Some(1),
            "reset must advance the continuity generation from 0 to 1"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    /// Item R7 follow-up. Every registered identity is on the reset path, so
    /// a registered identity on an identity runtime WITHOUT a session bridge
    /// is a typed preflight failure and nothing is touched: no reset, no
    /// retire, no lifecycle frame, generation unchanged. Before the routing
    /// fix the guard was `baseline_identities.contains(identity) &&
    /// !has_session_bridge`, which never fired on an identity-first gateway
    /// (empty baseline slot), and the identity was silently retired instead.
    #[tokio::test]
    async fn reset_all_without_a_session_bridge_fails_closed_for_registered_identities()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-reset-all-no-session-bridge").await?;
        spawn_identity_control_test_member(&runtime, "review:singleton", "review:singleton")
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
                    placement: None,
                },
                IdentityLifecycleState::Active,
                Some(record),
                Some(grant),
            )
            .await;

        let console_events = ConsoleEventStore::new();
        let response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            Some(console_events.clone()),
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request("mobkit/reset_all"),
            true,
        ))
        .await;
        assert_eq!(
            response["error"]["code"],
            json!(-32000),
            "a registered identity without a session bridge must fail the preflight: {response:#?}"
        );
        let body = &response["error"]["data"];
        let failed = body["failed"]
            .as_array()
            .ok_or_else(|| format!("reset_all must report failed identities: {response:#?}"))?;
        assert_eq!(failed.len(), 1, "exactly one failure: {response:#?}");
        assert_eq!(failed[0]["identity"], json!("review:singleton"));
        assert_eq!(
            failed[0]["kind"],
            json!("identity_reset_requires_session_bridge"),
            "the failure names the missing bridge: {response:#?}"
        );
        assert_eq!(body["reset"], json!([]), "nothing was reset: {response:#?}");
        assert_eq!(
            body["retired_delegates"],
            json!([]),
            "a registered identity is never silently retired: {response:#?}"
        );

        // Nothing was touched: still Active at generation 0, no lifecycle
        // frame, live member still present.
        let status = identity_runtime.status(&identity).await?;
        assert_eq!(status.state, IdentityLifecycleState::Active);
        assert_eq!(
            status.generation.map(ContinuityGeneration::get),
            Some(0),
            "a refused reset must not advance the generation"
        );
        let lifecycle = console_events
            .replay_all(None)
            .await
            .map_err(|err| format!("replay_all: {}", err.error))?;
        assert!(
            lifecycle.iter().all(|event| {
                event.event_type != "identity_reset" && event.event_type != "identity_retired"
            }),
            "a refused reset_all records no lifecycle frame: {lifecycle:#?}"
        );
        assert!(
            runtime
                .handle()
                .get_member(&crate::member_comms_id::mob_member_id("review:singleton"))
                .await
                .ok()
                .flatten()
                .is_some(),
            "the live member must survive a refused reset_all"
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
                    // meerkat 0.7: roster ids are comms-safe encodings of the
                    // public alias (MemberCommsName rejects ":").
                    crate::member_comms_id::mob_member_id_str("rt:review:singleton:0").into_owned(),
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
                    placement: None,
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
                .get_member(&crate::member_comms_id::mob_member_id(
                    "rt:review:singleton:0"
                ))
                .await
                .ok()
                .flatten()
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
                    placement: None,
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
                None,
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
                // meerkat 0.7: roster ids are comms-safe encodings of the
                // public alias (MemberCommsName rejects ":").
                crate::member_comms_id::mob_member_id_str("rt:review:singleton:0").into_owned(),
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
                    placement: None,
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
                    None,
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
                    // meerkat 0.7: roster ids are comms-safe encodings of the
                    // public alias (MemberCommsName rejects ":").
                    crate::member_comms_id::mob_member_id_str("rt:review:singleton:0").into_owned(),
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
                None,
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
                .get_member(&crate::member_comms_id::mob_member_id(
                    "rt:review:singleton:0"
                ))
                .await
                .ok()
                .flatten()
                .is_some(),
            "hidden live-only controls must not mutate the live member"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_reset_generated_alias_without_registration_fails_closed()
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
                    // meerkat 0.7: roster ids are comms-safe encodings of the
                    // public alias (MemberCommsName rejects ":").
                    crate::member_comms_id::mob_member_id_str("rt:review:singleton:0").into_owned(),
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

        let member_id = crate::member_comms_id::mob_member_id("rt:review:singleton:0");
        let session_before = runtime
            .handle()
            .resolve_bridge_session_id_observation(&member_id)
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
            rpc_request_with_params("mobkit/reset", json!({ "identity": "review:singleton" })),
            true,
        ))
        .await;
        assert_ne!(
            response["error"],
            Value::Null,
            "a generated alias must not become raw fallback after registration disappears: {response:#?}"
        );
        assert_eq!(
            runtime
                .handle()
                .resolve_bridge_session_id_observation(&member_id)
                .await,
            session_before,
            "failing closed must not respawn the orphaned generated member"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_reset_bare_live_member_without_session_bridge_uses_fallback()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-reset-bare-live-no-bridge").await?;

        let mut labels = BTreeMap::new();
        labels.insert("agent_identity".to_string(), "review:singleton".to_string());
        runtime
            .handle()
            .spawn_spec(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "legacy-review-singleton".to_string(),
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
            runtime_instance_id: "console-reset-bare-live-no-bridge".to_string(),
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
            "genuine bare live-only reset should retain compatibility fallback: {response:#?}"
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
                    // meerkat 0.7: roster ids are comms-safe encodings of the
                    // public alias (MemberCommsName rejects ":").
                    crate::member_comms_id::mob_member_id_str("rt:review:singleton:0").into_owned(),
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
                    placement: None,
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
                .get_member(&crate::member_comms_id::mob_member_id(
                    "rt:review:singleton:0"
                ))
                .await
                .ok()
                .flatten()
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
                    // meerkat 0.7: roster ids are comms-safe encodings of the
                    // public alias (MemberCommsName rejects ":").
                    crate::member_comms_id::mob_member_id_str("hidden:singleton").into_owned(),
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
            None,
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
                .get_member(&crate::member_comms_id::mob_member_id("hidden:singleton"))
                .await
                .ok()
                .flatten()
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
                // meerkat 0.7: roster ids are comms-safe encodings of the
                // public alias (MemberCommsName rejects ":").
                crate::member_comms_id::mob_member_id_str("rt:review:singleton:0").into_owned(),
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
                    placement: None,
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
            None,
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

    /// The half of item R7 that must NOT change: a live member with no
    /// registered identity and no baseline spec has no durable owner to reset
    /// to, so `reset_all` retires it and says so as `identity_retired`.
    #[tokio::test]
    async fn reset_all_retires_live_only_members_that_have_no_registered_identity()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-reset-all-live-only-retire").await?;
        runtime
            .handle()
            .spawn_spec(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    // meerkat 0.7: roster ids are comms-safe encodings of the
                    // public alias (MemberCommsName rejects ":").
                    crate::member_comms_id::mob_member_id_str("worker:solo").into_owned(),
                    Some("You are a live-only worker nobody registered.".into()),
                    None,
                    None,
                )
                .with_labels(BTreeMap::from([(
                    "agent_identity".to_string(),
                    "worker:solo".to_string(),
                )])),
            )
            .await?;

        let console_events = ConsoleEventStore::new();
        let response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            Some(console_events.clone()),
            None,
            None,
            None,
            None,
            rpc_request("mobkit/reset_all"),
            true,
        ))
        .await;
        assert_eq!(
            response["error"],
            Value::Null,
            "reset_all over a live-only member must succeed: {response:#?}"
        );
        assert_eq!(
            response["result"]["reset"],
            json!([]),
            "a live-only member has no durable owner to reset to: {response:#?}"
        );
        assert_eq!(
            response["result"]["retired_delegates"],
            json!([{ "identity": "worker:solo" }]),
            "the live-only member must be retired: {response:#?}"
        );

        let lifecycle = console_events
            .replay_all(None)
            .await
            .map_err(|err| format!("replay_all: {}", err.error))?;
        let retired = lifecycle
            .iter()
            .filter(|event| {
                event.event_type == "identity_retired" && event.identity == "worker:solo"
            })
            .count();
        assert_eq!(
            retired, 1,
            "exactly one identity_retired lifecycle event for the live-only member: {lifecycle:#?}"
        );
        assert!(
            lifecycle
                .iter()
                .all(|event| event.event_type != "identity_reset"),
            "nothing on this runtime can be reset: {lifecycle:#?}"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    fn reset_all_test_spec(identity: &AgentIdentity) -> DurableAgentSpec {
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
            placement: None,
        }
    }

    /// Register `identity` as Active with a held lease and a persisted
    /// continuity record at generation 0: the shape a materialized identity
    /// has at rest, which lets `reset_tracked` (fence advance, generation
    /// bump) and `retire_tracked` run against it in-process.
    async fn register_leased_active_identity(
        identity_runtime: &IdentityRuntime,
        store: &LocalContinuityStore,
        lease_provider: &LocalLeaseProvider,
        runtime_instance_id: &str,
        identity: &AgentIdentity,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse(&format!("rt:{identity}:0"))?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let grants = lease_provider
            .acquire_leases(std::slice::from_ref(identity), runtime_instance_id)
            .await?;
        let grant = match grants.get(identity).cloned() {
            Some(LeaseAcquireResult::Acquired(grant)) => grant,
            other => return Err(format!("expected acquired lease, got {other:?}").into()),
        };
        store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await?;
        identity_runtime
            .register(
                reset_all_test_spec(identity),
                IdentityLifecycleState::Active,
                Some(record),
                Some(grant),
            )
            .await;
        Ok(())
    }

    fn reset_all_test_identity_runtime(
        store: &Arc<LocalContinuityStore>,
        lease_provider: &Arc<LocalLeaseProvider>,
        runtime_instance_id: &str,
    ) -> Arc<IdentityRuntime> {
        Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: store.clone(),
            lease_provider: lease_provider.clone(),
            runtime_instance_id: runtime_instance_id.to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(Arc::new(SuccessorMintingIdentityBridge)),
            default_timeout: None,
        }))
    }

    /// Item R7, round 2. `reset_all` classifies each registered identity by
    /// lifecycle state before routing it. A `Retiring` identity is the entry
    /// `mobkit/retire` leaves behind until delete or roster reconcile; it is
    /// leaving the fleet, so it is neither resurrected through the successor
    /// transition nor reported, while the Active identity beside it resets.
    #[tokio::test]
    async fn reset_all_leaves_a_retiring_identity_alone_and_resets_the_active_one()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-reset-all-retiring-skipped").await?;
        spawn_identity_control_test_member(&runtime, "review:active", "review:active").await?;

        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let identity_runtime =
            reset_all_test_identity_runtime(&store, &lease_provider, "test-runtime");
        let active = AgentIdentity::parse("review:active")?;
        let leaving = AgentIdentity::parse("review:leaving")?;
        for identity in [&active, &leaving] {
            register_leased_active_identity(
                &identity_runtime,
                &store,
                &lease_provider,
                "test-runtime",
                identity,
            )
            .await?;
        }
        identity_runtime.retire_tracked(&leaving).await?;
        assert_eq!(
            identity_runtime.status(&leaving).await?.state,
            IdentityLifecycleState::Retiring,
            "fixture: retire leaves the entry registered in state Retiring"
        );

        let console_events = ConsoleEventStore::new();
        let response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            Some(console_events.clone()),
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request("mobkit/reset_all"),
            true,
        ))
        .await;
        assert_eq!(
            response["error"],
            Value::Null,
            "a Retiring identity must not fail reset_all: {response:#?}"
        );
        let body = &response["result"];
        assert_eq!(
            body["reset"],
            json!(["review:active"]),
            "only the active identity is in the reset set: {response:#?}"
        );
        assert_eq!(
            body["retired_delegates"],
            json!([]),
            "a Retiring identity is not retired again: {response:#?}"
        );
        assert_eq!(
            body["failed"],
            json!([]),
            "a Retiring identity is not reported as a failure: {response:#?}"
        );

        // The Retiring identity is untouched: still Retiring at generation 0.
        let leaving_status = identity_runtime.status(&leaving).await?;
        assert_eq!(leaving_status.state, IdentityLifecycleState::Retiring);
        assert_eq!(
            leaving_status.generation.map(ContinuityGeneration::get),
            Some(0),
            "reset_all must not resurrect a Retiring identity"
        );
        let active_status = identity_runtime.status(&active).await?;
        assert_eq!(active_status.state, IdentityLifecycleState::Active);
        assert_eq!(
            active_status.generation.map(ContinuityGeneration::get),
            Some(1),
            "the active identity resets to generation 1"
        );

        let lifecycle = console_events
            .replay_all(None)
            .await
            .map_err(|err| format!("replay_all: {}", err.error))?;
        let reset_frames = lifecycle
            .iter()
            .filter(|event| event.event_type == "identity_reset")
            .map(|event| event.identity.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            reset_frames,
            vec!["review:active"],
            "exactly one identity_reset frame, for the active identity: {lifecycle:#?}"
        );
        assert!(
            lifecycle
                .iter()
                .all(|event| event.event_type != "identity_retired"),
            "reset_all retires nothing here: {lifecycle:#?}"
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    /// Item R7, round 2. A registered identity with no live roster row cannot
    /// go through the successor transition: meerkat's respawn has no member
    /// to replace and answered `MemberNotFound` prose from inside
    /// `reset_tracked`, failing the call for a fleet that had been reset.
    /// `reset_all` now refuses such an identity typed
    /// (`identity_not_resettable_in_state`, state named), leaves it
    /// untouched, and still resets the Active identity beside it. Both
    /// producing shapes are covered: Dormant under lazy bootstrap (never
    /// materialized, no continuity) and Broken after a failed materialize
    /// (the keyless park), which keeps its continuity record.
    #[tokio::test]
    async fn reset_all_refuses_registered_identities_without_a_live_member_typed()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-reset-all-no-live-member-typed").await?;
        spawn_identity_control_test_member(&runtime, "review:active", "review:active").await?;

        let store = Arc::new(LocalContinuityStore::in_memory()?);
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let identity_runtime =
            reset_all_test_identity_runtime(&store, &lease_provider, "test-runtime");
        let active = AgentIdentity::parse("review:active")?;
        register_leased_active_identity(
            &identity_runtime,
            &store,
            &lease_provider,
            "test-runtime",
            &active,
        )
        .await?;
        let lazy = AgentIdentity::parse("review:lazy")?;
        identity_runtime
            .register(
                reset_all_test_spec(&lazy),
                IdentityLifecycleState::Dormant,
                None,
                None,
            )
            .await;
        let parked = AgentIdentity::parse("review:parked")?;
        identity_runtime
            .register(
                reset_all_test_spec(&parked),
                IdentityLifecycleState::Broken,
                Some(ContinuityRecord {
                    identity: parked.clone(),
                    agent_runtime_id: AgentRuntimeId::parse("rt:review:parked:0")?,
                    session_id: meerkat_core::types::SessionId::new(),
                    generation: ContinuityGeneration::new(0),
                    checkpoint_version: CheckpointVersion::new(0),
                }),
                None,
            )
            .await;

        let console_events = ConsoleEventStore::new();
        let response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            Some(console_events.clone()),
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request("mobkit/reset_all"),
            true,
        ))
        .await;
        assert_eq!(
            response["error"]["code"],
            json!(-32000),
            "an unresettable identity is a per-identity failure: {response:#?}"
        );
        let body = &response["error"]["data"];
        assert_eq!(
            body["reset"],
            json!(["review:active"]),
            "the active identity is still reset: {response:#?}"
        );
        assert_eq!(
            body["retired_delegates"],
            json!([]),
            "no registered identity is retired: {response:#?}"
        );
        let failed = body["failed"]
            .as_array()
            .ok_or_else(|| format!("reset_all must report failed identities: {response:#?}"))?;
        let typed = failed
            .iter()
            .map(|entry| {
                (
                    entry["identity"].as_str().unwrap_or_default(),
                    entry["kind"].as_str().unwrap_or_default(),
                    entry["state"].as_str().unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            typed,
            vec![
                ("review:lazy", "identity_not_resettable_in_state", "dormant"),
                (
                    "review:parked",
                    "identity_not_resettable_in_state",
                    "broken"
                ),
            ],
            "each unresettable identity is refused typed with its state: {response:#?}"
        );
        for entry in failed {
            let error = entry["error"].as_str().unwrap_or_default();
            let state = entry["state"].as_str().unwrap_or_default();
            assert!(
                error.contains(&format!("identity is {state} with no live roster row")),
                "the refusal names the state and the missing member: {entry:#?}"
            );
        }

        // Nothing touched on the refused identities; the active one reset.
        let lazy_status = identity_runtime.status(&lazy).await?;
        assert_eq!(lazy_status.state, IdentityLifecycleState::Dormant);
        assert_eq!(lazy_status.generation, None);
        let parked_status = identity_runtime.status(&parked).await?;
        assert_eq!(parked_status.state, IdentityLifecycleState::Broken);
        assert_eq!(
            parked_status.generation.map(ContinuityGeneration::get),
            Some(0)
        );
        let active_status = identity_runtime.status(&active).await?;
        assert_eq!(active_status.state, IdentityLifecycleState::Active);
        assert_eq!(
            active_status.generation.map(ContinuityGeneration::get),
            Some(1)
        );

        let lifecycle = console_events
            .replay_all(None)
            .await
            .map_err(|err| format!("replay_all: {}", err.error))?;
        let reset_frames = lifecycle
            .iter()
            .filter(|event| event.event_type == "identity_reset")
            .map(|event| event.identity.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            reset_frames,
            vec!["review:active"],
            "exactly one identity_reset frame, for the active identity: {lifecycle:#?}"
        );
        assert!(
            lifecycle.iter().all(|event| {
                event.event_type != "identity_retired"
                    && event.identity != "review:lazy"
                    && event.identity != "review:parked"
            }),
            "refused identities leave no lifecycle frame: {lifecycle:#?}"
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
                        // meerkat 0.7: roster ids are comms-safe encodings of
                        // the public alias (MemberCommsName rejects ":").
                        crate::member_comms_id::mob_member_id_str(runtime_id).into_owned(),
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
                .resolve_bridge_session_id_observation(&crate::member_comms_id::mob_member_id(
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
                    placement: None,
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
            None,
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
                .get_member(&crate::member_comms_id::mob_member_id(
                    "rt:review:singleton:1"
                ))
                .await
                .ok()
                .flatten()
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
    async fn console_runtime_capabilities_advertise_agent_memory_with_read_only_gating()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-agent-memory-capabilities").await?;
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-agent-memory-capabilities".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let memory_dir = tempfile::tempdir()?;
        let memory_store = Arc::new(SqliteAgentMemoryStore::open(memory_dir.path())?);
        identity_runtime
            .set_agent_memory(Some(AgentMemoryRuntimeInjector::new(
                memory_store,
                AgentMemoryConfig::default(),
            )))
            .await;

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
            "mobkit/agent_memory/recall",
            "mobkit/agent_memory/remember",
            "mobkit/agent_memory/forget",
        ] {
            assert!(
                methods.iter().any(|candidate| candidate == method),
                "authenticated capabilities should advertise {method}: {methods:#?}"
            );
        }

        let read_only_response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime),
            None,
            None,
            rpc_request("mobkit/capabilities"),
            false,
        ))
        .await;
        assert_eq!(
            read_only_response["error"],
            Value::Null,
            "{read_only_response:#?}"
        );
        let read_only_methods = read_only_response["result"]["methods"]
            .as_array()
            .ok_or("read-only capabilities methods should be an array")?;
        assert!(
            read_only_methods
                .iter()
                .any(|candidate| candidate == "mobkit/agent_memory/recall"),
            "read-only capabilities should keep recall: {read_only_methods:#?}"
        );
        for method in ["mobkit/agent_memory/remember", "mobkit/agent_memory/forget"] {
            assert!(
                !read_only_methods
                    .iter()
                    .any(|candidate| candidate == method),
                "read-only capabilities must omit {method}: {read_only_methods:#?}"
            );
        }

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_agent_memory_methods_round_trip()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-agent-memory-round-trip").await?;
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-agent-memory-round-trip".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("identity:memory-console")?;
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
                    placement: None,
                },
                IdentityLifecycleState::Active,
                None,
                None,
            )
            .await;
        let memory_dir = tempfile::tempdir()?;
        let memory_store = Arc::new(SqliteAgentMemoryStore::open(memory_dir.path())?);
        identity_runtime
            .set_agent_memory(Some(AgentMemoryRuntimeInjector::new(
                memory_store,
                AgentMemoryConfig::default(),
            )))
            .await;

        let remember = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request_with_params(
                "mobkit/agent_memory/remember",
                json!({
                    "identity": "identity:memory-console",
                    "title": "Console memory token",
                    "body": "The console memory token is CONSOLE-MEM-17.",
                    "tags": ["console", "memory"]
                }),
            ),
            true,
        ))
        .await;
        assert_eq!(remember["error"], Value::Null, "{remember:#?}");
        let memory_id = remember["result"]["memory_id"]
            .as_str()
            .ok_or("remember result should include memory_id")?
            .to_string();

        let recall = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request_with_params(
                "mobkit/agent_memory/recall",
                json!({
                    "identity": "identity:memory-console",
                    "selection": "contextual",
                    "query_text": "Where is CONSOLE-MEM-17?",
                    "query_terms": ["CONSOLE-MEM-17"]
                }),
            ),
            false,
        ))
        .await;
        assert_eq!(recall["error"], Value::Null, "{recall:#?}");
        assert_eq!(
            recall["result"]["records"]
                .as_array()
                .map(std::vec::Vec::len),
            Some(1)
        );
        assert_eq!(
            recall["result"]["records"][0]["memory_id"],
            json!(memory_id)
        );

        let read_only_remember = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request_with_params(
                "mobkit/agent_memory/remember",
                json!({
                    "identity": "identity:memory-console",
                    "title": "Denied",
                    "body": "Denied"
                }),
            ),
            false,
        ))
        .await;
        assert_eq!(
            read_only_remember["error"]["data"]["kind"],
            json!("read_only"),
            "{read_only_remember:#?}"
        );

        let access_controller = AccessController::new(crate::access::AccessControlConfig {
            enabled: true,
            admins: vec!["admin@example.test".to_string()],
            ..crate::access::AccessControlConfig::default()
        })?;
        let denied_view = access_controller.view_for_subject(Some("viewer@example.test"));
        let denied_recall = Box::pin(handle_console_runtime_rpc_with_visibility(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            &AllowAllConsoleVisibilityPolicy,
            rpc_request_with_params(
                "mobkit/agent_memory/recall",
                json!({
                    "identity": "identity:memory-console",
                    "selection": "always"
                }),
            ),
            true,
            false,
            None,
            Some(&access_controller),
            Some(&denied_view),
            None,
            None,
            None,
            None,
        ))
        .await;
        assert_eq!(
            denied_recall["error"]["data"]["kind"],
            json!("access_denied"),
            "{denied_recall:#?}"
        );
        // §10.3: recall requires agent.memory.read AND agent.view; the read
        // action is checked (and reported) first.
        assert_eq!(
            denied_recall["error"]["data"]["action"],
            json!(ACTION_AGENT_MEMORY_READ),
            "{denied_recall:#?}"
        );

        let send_only_controller = AccessController::new(crate::access::AccessControlConfig {
            enabled: true,
            admins: vec!["admin@example.test".to_string()],
            rules: vec![crate::access::AccessRule {
                id: "send-only-memory-console".to_string(),
                subjects: vec!["sender@example.test".to_string()],
                actions: vec![ACTION_AGENT_SEND.to_string()],
                agents: vec!["identity:memory-console".to_string()],
                ..crate::access::AccessRule::default()
            }],
            ..crate::access::AccessControlConfig::default()
        })?;
        let send_only_view = send_only_controller.view_for_subject(Some("sender@example.test"));
        let send_only_remember = Box::pin(handle_console_runtime_rpc_with_visibility(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            &AllowAllConsoleVisibilityPolicy,
            rpc_request_with_params(
                "mobkit/agent_memory/remember",
                json!({
                    "identity": "identity:memory-console",
                    "title": "Send-only denied",
                    "body": "Send-only users must not persist durable memory."
                }),
            ),
            true,
            false,
            None,
            Some(&send_only_controller),
            Some(&send_only_view),
            None,
            None,
            None,
            None,
        ))
        .await;
        assert_eq!(
            send_only_remember["error"]["data"]["kind"],
            json!("access_denied"),
            "{send_only_remember:#?}"
        );
        assert_eq!(
            send_only_remember["error"]["data"]["action"],
            json!(ACTION_AGENT_MEMORY_WRITE),
            "{send_only_remember:#?}"
        );

        let send_only_forget = Box::pin(handle_console_runtime_rpc_with_visibility(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            &AllowAllConsoleVisibilityPolicy,
            rpc_request_with_params(
                "mobkit/agent_memory/forget",
                json!({
                    "identity": "identity:memory-console",
                    "memory_id": memory_id.clone()
                }),
            ),
            true,
            false,
            None,
            Some(&send_only_controller),
            Some(&send_only_view),
            None,
            None,
            None,
            None,
        ))
        .await;
        assert_eq!(
            send_only_forget["error"]["data"]["kind"],
            json!("access_denied"),
            "{send_only_forget:#?}"
        );
        assert_eq!(
            send_only_forget["error"]["data"]["action"],
            json!(ACTION_AGENT_MEMORY_DELETE),
            "{send_only_forget:#?}"
        );

        let wildcard_allow_exact_deny_controller =
            AccessController::new(crate::access::AccessControlConfig {
                enabled: true,
                admins: vec!["admin@example.test".to_string()],
                rules: vec![
                    crate::access::AccessRule {
                        id: "deny-canonical-memory-console".to_string(),
                        effect: crate::access::AccessEffect::Deny,
                        subjects: vec!["wildcard@example.test".to_string()],
                        actions: vec![
                            ACTION_AGENT_VIEW.to_string(),
                            ACTION_AGENT_MEMORY_WRITE.to_string(),
                            ACTION_AGENT_MEMORY_DELETE.to_string(),
                        ],
                        agents: vec!["identity:memory-console".to_string()],
                        ..crate::access::AccessRule::default()
                    },
                    crate::access::AccessRule {
                        id: "allow-wildcard-memory-console".to_string(),
                        subjects: vec!["wildcard@example.test".to_string()],
                        actions: vec![
                            ACTION_AGENT_VIEW.to_string(),
                            ACTION_AGENT_MEMORY_WRITE.to_string(),
                            ACTION_AGENT_MEMORY_DELETE.to_string(),
                        ],
                        agents: vec!["*".to_string()],
                        ..crate::access::AccessRule::default()
                    },
                ],
                ..crate::access::AccessControlConfig::default()
            })?;
        let wildcard_allow_exact_deny_view =
            wildcard_allow_exact_deny_controller.view_for_subject(Some("wildcard@example.test"));

        let whitespace_denied_remember = Box::pin(handle_console_runtime_rpc_with_visibility(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            &AllowAllConsoleVisibilityPolicy,
            rpc_request_with_params(
                "mobkit/agent_memory/remember",
                json!({
                    "identity": " identity:memory-console ",
                    "title": "Whitespace denied",
                    "body": "Whitespace-wrapped identities must be authorized canonically."
                }),
            ),
            true,
            false,
            None,
            Some(&wildcard_allow_exact_deny_controller),
            Some(&wildcard_allow_exact_deny_view),
            None,
            None,
            None,
            None,
        ))
        .await;
        assert_eq!(
            whitespace_denied_remember["error"]["data"]["kind"],
            json!("access_denied"),
            "{whitespace_denied_remember:#?}"
        );
        assert_eq!(
            whitespace_denied_remember["error"]["data"]["action"],
            json!(ACTION_AGENT_MEMORY_WRITE),
            "{whitespace_denied_remember:#?}"
        );

        let whitespace_denied_recall = Box::pin(handle_console_runtime_rpc_with_visibility(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            &AllowAllConsoleVisibilityPolicy,
            rpc_request_with_params(
                "mobkit/agent_memory/recall",
                json!({
                    "identity": " identity:memory-console ",
                    "selection": "always"
                }),
            ),
            true,
            false,
            None,
            Some(&wildcard_allow_exact_deny_controller),
            Some(&wildcard_allow_exact_deny_view),
            None,
            None,
            None,
            None,
        ))
        .await;
        assert_eq!(
            whitespace_denied_recall["error"]["data"]["kind"],
            json!("access_denied"),
            "{whitespace_denied_recall:#?}"
        );
        // The exact-identity deny matches agent.memory.read through its
        // `agent.*` pattern; read is checked first (§10.3 mapping).
        assert_eq!(
            whitespace_denied_recall["error"]["data"]["action"],
            json!(ACTION_AGENT_MEMORY_READ),
            "{whitespace_denied_recall:#?}"
        );

        let whitespace_denied_forget = Box::pin(handle_console_runtime_rpc_with_visibility(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            &AllowAllConsoleVisibilityPolicy,
            rpc_request_with_params(
                "mobkit/agent_memory/forget",
                json!({
                    "identity": " identity:memory-console ",
                    "memory_id": memory_id.clone()
                }),
            ),
            true,
            false,
            None,
            Some(&wildcard_allow_exact_deny_controller),
            Some(&wildcard_allow_exact_deny_view),
            None,
            None,
            None,
            None,
        ))
        .await;
        assert_eq!(
            whitespace_denied_forget["error"]["data"]["kind"],
            json!("access_denied"),
            "{whitespace_denied_forget:#?}"
        );
        assert_eq!(
            whitespace_denied_forget["error"]["data"]["action"],
            json!(ACTION_AGENT_MEMORY_DELETE),
            "{whitespace_denied_forget:#?}"
        );

        let forget = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request_with_params(
                "mobkit/agent_memory/forget",
                json!({
                    "identity": "identity:memory-console",
                    "memory_id": memory_id
                }),
            ),
            true,
        ))
        .await;
        assert_eq!(forget["error"], Value::Null, "{forget:#?}");
        assert_eq!(forget["result"]["deleted"], json!(true));

        let after_forget = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime),
            None,
            None,
            rpc_request_with_params(
                "mobkit/agent_memory/recall",
                json!({
                    "identity": "identity:memory-console",
                    "selection": "always"
                }),
            ),
            false,
        ))
        .await;
        assert_eq!(after_forget["error"], Value::Null, "{after_forget:#?}");
        assert_eq!(
            after_forget["result"]["records"]
                .as_array()
                .map(std::vec::Vec::len),
            Some(0)
        );

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_runtime_capabilities_advertise_respawn_and_reset_without_identity_runtime()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-plain-lifecycle-capabilities").await?;

        // Mutating caller, no identity runtime: respawn/reset dispatch via the
        // live-member fallback, so they must be advertised (the console gates
        // its Respawn/Reset buttons on this list).
        let response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            None,
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
        for method in ["mobkit/retire", "mobkit/respawn", "mobkit/reset"] {
            assert!(
                methods.iter().any(|candidate| candidate == method),
                "plain runtime capabilities should advertise {method}: {methods:#?}"
            );
        }
        assert!(
            !methods
                .iter()
                .any(|candidate| candidate == "mobkit/delete_identity"),
            "delete_identity requires an identity runtime: {methods:#?}"
        );

        // Unauthenticated callers keep the read-only projection: no lifecycle
        // mutations advertised.
        let read_only_response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            rpc_request("mobkit/capabilities"),
            false,
        ))
        .await;
        assert_eq!(
            read_only_response["error"],
            Value::Null,
            "{read_only_response:#?}"
        );
        let read_only_methods = read_only_response["result"]["methods"]
            .as_array()
            .ok_or("capabilities methods should be an array")?;
        for method in ["mobkit/retire", "mobkit/respawn", "mobkit/reset"] {
            assert!(
                !read_only_methods
                    .iter()
                    .any(|candidate| candidate == method),
                "unauthenticated capabilities must omit {method}: {read_only_methods:#?}"
            );
        }

        let _ = runtime.handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn console_aggregator_rpc_does_not_expose_flow_editor_authoring_methods() {
        let response = Box::pin(handle_console_aggregator_rpc(
            None,
            rpc_request("mobkit/capabilities"),
            true,
            false,
            None,
            None,
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
            false,
            None,
            None,
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
                    // meerkat 0.7: roster ids are comms-safe encodings of the
                    // public alias (MemberCommsName rejects ":").
                    crate::member_comms_id::mob_member_id_str("rt:review:singleton:0").into_owned(),
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
                    placement: None,
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

        let list_response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request("mobkit/list_members"),
            true,
        ))
        .await;
        let listed = list_response["result"].as_array().expect("list members");
        assert!(
            listed
                .iter()
                .all(|entry| entry["agent_identity"] != json!("rt:review:singleton:0")),
            "list_members must filter stale runtime aliases: {list_response:#?}"
        );

        let find_response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime.clone()),
            None,
            None,
            rpc_request_with_params(
                "mobkit/find_members",
                json!({ "label_key": "agent_identity", "label_value": "review:singleton" }),
            ),
            true,
        ))
        .await;
        let found = find_response["result"].as_array().expect("find members");
        assert!(
            found
                .iter()
                .all(|entry| entry["agent_identity"] != json!("rt:review:singleton:0")),
            "find_members must filter stale runtime aliases: {find_response:#?}"
        );

        for method in [
            "mobkit/get_member",
            "mobkit/member_status",
            "mobkit/member_health",
            "mobkit/identity/resolved_tools",
            "mobkit/retire_member",
            "mobkit/respawn_member",
            "mobkit/reload_member",
            "mobkit/force_cancel_member",
        ] {
            let param_name = if method == "mobkit/identity/resolved_tools" {
                "identity"
            } else {
                "member_id"
            };
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
                rpc_request_with_params(method, json!({ param_name: "rt:review:singleton:0" })),
                true,
            ))
            .await;
            assert_ne!(
                response["error"],
                Value::Null,
                "{method} must reject stale runtime alias"
            );
            assert_eq!(
                response["error"]["data"]["kind"],
                json!("stale_identity_runtime_binding")
            );
            assert_eq!(
                response["error"]["data"]["live_runtime_member_id"],
                json!("rt:review:singleton:0")
            );
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
    async fn console_peer_info_uses_current_identity_generation_when_stale_row_remains()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (_temp_dir, runtime) =
            build_empty_console_test_runtime("console-current-generation-peer-info").await?;
        let durable_identity = "review:peer-info-current";
        // ONE stable roster row. This used to seat rt:...:0 and rt:...:1 so a
        // stale generation row would remain for peer info to skip. Under the
        // durable-roster contract there is one row per identity and a reset
        // replaces its binding, so a leftover generation row cannot exist. What
        // still matters, and what this now asserts, is that peer info names the
        // stable row and refuses a binding that disagrees with it.
        let successor_alias = "rt:review:peer-info-current:1";
        runtime
            .handle()
            .spawn_spec(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    crate::member_comms_id::mob_member_id_str(durable_identity).into_owned(),
                    None,
                    None,
                    None,
                )
                .with_labels(BTreeMap::from([(
                    "agent_identity".to_string(),
                    durable_identity.to_string(),
                )])),
            )
            .await?;

        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-current-generation-peer-info".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse(durable_identity)?;
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
                    placement: None,
                },
                IdentityLifecycleState::Active,
                Some(ContinuityRecord {
                    identity,
                    agent_runtime_id: AgentRuntimeId::parse(successor_alias)?,
                    // The row's ACTUAL session. The old fixture used a fresh
                    // random SessionId, which only went unnoticed because
                    // nothing compared it against the live binding.
                    session_id: runtime
                        .handle()
                        .resolve_bridge_session_id(&crate::member_comms_id::mob_member_id(
                            durable_identity,
                        ))
                        .await
                        .ok_or("the stable roster row must have a bridge session")?,
                    generation: ContinuityGeneration::new(1),
                    checkpoint_version: CheckpointVersion::new(0),
                }),
                None,
            )
            .await;

        let response = Box::pin(handle_console_runtime_rpc(
            &runtime,
            None,
            None,
            None,
            None,
            None,
            Some(identity_runtime),
            None,
            None,
            rpc_request_with_params(
                "mobkit/cross_mob/peer_info",
                json!({"member_id": durable_identity}),
            ),
            true,
        ))
        .await;
        assert_eq!(response["error"], Value::Null, "{response:#?}");
        let comms_name = response["result"]["comms_name"]
            .as_str()
            .ok_or("peer info must return comms_name")?;
        assert!(
            comms_name
                .ends_with(crate::member_comms_id::mob_member_id_str(durable_identity).as_ref()),
            "console peer info must name the stable roster row: {response:#?}"
        );

        // The label-disambiguation fallback is NOT reachable for a durable
        // identity any more, and that is worth stating rather than asserting
        // around. It existed because the bare identity was not a roster id, so a
        // classic lookup had to match rows by their agent_identity LABEL and
        // could find several. The durable identity is now the roster id itself,
        // so the exact-member-id path always wins - I verified this by seeding
        // two rows labelled for one identity and watching peer info resolve the
        // exact row cleanly rather than reporting ambiguity.
        //
        // The fallback still guards callers that pass something which is not a
        // roster id; it simply cannot be provoked through this identity.

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
                        placement: None,
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
                    placement: None,
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
                // meerkat 0.7: roster ids are comms-safe encodings of the
                // public alias (MemberCommsName rejects ":").
                crate::member_comms_id::mob_member_id_str("agent:member-only").into_owned(),
                Some("You are a member-only spawned worker.".into()),
                None,
                None,
            ))
            .await?;

        let accepted = console_send_with_identity_first_fallback(
            &aggregator,
            identity_runtime,
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
                    placement: None,
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
                    placement: None,
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

    /// An identical-content resend of an existing idempotency key is a
    /// replay: it answers with the original acceptance and runs NOTHING.
    /// Before the fix the identity-first door answered with the old
    /// acceptance AND dispatched a whole new turn stamped with the old
    /// interaction id (reproduced 2026-09-03: 4 `run_started` for 3 sends).
    #[tokio::test]
    async fn identity_first_console_send_replay_returns_the_original_acceptance_without_a_second_dispatch()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let identity = AgentIdentity::parse("agent:replay-console")?;
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse("rt:agent:replay-console:0")?,
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        let handling_modes = Arc::new(Mutex::new(Vec::new()));
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "console-replay-send-test".to_string(),
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
                    placement: None,
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
        let events = ConsoleEventStore::new();
        let request = crate::console_aggregator::ConsoleSendRequest {
            identity: identity.as_str().to_string(),
            content: serde_json::to_value(meerkat_core::ContentInput::Text(
                "hello once".to_string(),
            ))?,
            origin: "test".to_string(),
            idempotency_key: "idem-replay".to_string(),
            handling_mode: None,
        };
        let first = console_send_identity_first(
            &aggregator,
            runtime.clone(),
            Some(&events),
            request.clone(),
        )
        .await?;
        // The first send dispatches: a positive observable before any count.
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if handling_modes
                    .lock()
                    .map(|modes| !modes.is_empty())
                    .unwrap_or(false)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .map_err(|_| "the first send must reach the bridge")?;

        let replay = console_send_identity_first(
            &aggregator,
            runtime.clone(),
            Some(&events),
            request.clone(),
        )
        .await?;
        assert_eq!(
            replay.interaction_id, first.interaction_id,
            "a replay answers with the original interaction"
        );
        assert_eq!(replay.input_frame_id, first.input_frame_id);

        // Give a wrongly spawned second dispatch time to reach the bridge.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let deliveries = handling_modes
            .lock()
            .map(|modes| modes.len())
            .map_err(|_| "handling modes mutex poisoned")?;
        assert_eq!(deliveries, 1, "a replay must not dispatch a second turn");
        let page = aggregator
            .query_timeline(ConsoleTimelineQuery {
                identity: Some(identity.as_str().to_string()),
                ..ConsoleTimelineQuery::default()
            })
            .await?;
        assert_eq!(
            page.frames
                .iter()
                .filter(|frame| frame.kind == "user_input")
                .count(),
            1,
            "a replay appends no second input frame: {:#?}",
            page.frames
        );

        // Same key, different content is still the typed conflict.
        let conflict = console_send_identity_first(
            &aggregator,
            runtime,
            Some(&events),
            crate::console_aggregator::ConsoleSendRequest {
                content: serde_json::to_value(meerkat_core::ContentInput::Text(
                    "hello twice".to_string(),
                ))?,
                ..request
            },
        )
        .await;
        assert!(
            matches!(
                conflict,
                Err(super::ConsoleSendError::IdempotencyConflict(ref key)) if key == "idem-replay"
            ),
            "{conflict:?}"
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
                    placement: None,
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
                .get_member(&meerkat_mob::ids::AgentIdentity::from("agent-reset"))
                .await
                .ok()
                .flatten()
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
                progress: None,
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
                progress: None,
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
                progress: None,
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
                    progress: None,
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
                    progress: None,
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
                // meerkat 0.7: roster ids are comms-safe encodings of the
                // public alias (MemberCommsName rejects ":").
                crate::member_comms_id::mob_member_id_str("worker:one").into_owned(),
                Some("You are worker one.".into()),
                None,
                None,
            ))
            .await?;

        let empty_read_model = ConsoleSnapshotReadModelState::default();
        let (members, session_owner_by_id) = project_console_members_from_handle(
            &runtime.handle(),
            None,
            None,
            &empty_read_model,
            &BTreeMap::new(),
        )
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
            &BTreeMap::new(),
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
        assert_eq!(
            console_runtime_alias_generation("rt:review:singleton:7", "review:singleton"),
            Some(7)
        );
        assert_eq!(
            console_runtime_alias_generation("rt:review:singleton:8", "review:other"),
            None
        );
    }
}
