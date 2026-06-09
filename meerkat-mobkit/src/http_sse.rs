//! Server-Sent Events (SSE) streaming endpoints for agent and mob observation.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use futures::StreamExt;
use meerkat_core::AgentEvent;
use meerkat_core::comms::EventStream;
use meerkat_core::event::agent_event_type;
use meerkat_mob::{MobEventRouterHandle, MobHandle};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::access::{ACTION_AGENT_VIEW, ACTION_MOB_OBSERVE, AccessController, AccessView};
use crate::runtime::{RuntimeDecisionState, extract_bearer_token_from_header};
use crate::unified_runtime::EventQuery;
use crate::unified_runtime::mob_events::{MOB_EVENTS_STREAM_PATH, MobEventsStore};

use crate::mob_handle_runtime::MobRuntimeError;
use meerkat_core::comms::SendError;
use meerkat_core::service::SessionError;
use meerkat_mob::MobError;

pub(crate) const DEFAULT_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);
pub(crate) const KEEP_ALIVE_TEXT: &str = "keep-alive";

pub(crate) fn console_agent_event_payload(event: &AgentEvent) -> Value {
    let mut payload = serde_json::to_value(event).unwrap_or_else(|_| json!({}));
    let record = match payload.as_object_mut() {
        Some(record) => record,
        None => return payload,
    };
    let is_tool_event = matches!(
        agent_event_type(event),
        "tool_call_requested"
            | "tool_result_received"
            | "tool_execution_started"
            | "tool_execution_completed"
            | "tool_execution_timed_out"
    );
    if is_tool_event
        && !record.contains_key("tool_call_id")
        && let Some(id) = record.get("id").cloned()
    {
        record.insert("tool_call_id".to_string(), id);
    }
    payload
}

pub fn agent_event_sse(interaction_id: &str, seq: u64, event: &AgentEvent) -> Event {
    let event_name = agent_event_name(event);
    let payload = serde_json::to_string(&console_agent_event_payload(event))
        .unwrap_or_else(|_| "{}".to_string());
    Event::default()
        .id(format!("{interaction_id}:{seq}"))
        .event(event_name)
        .data(payload)
}

fn agent_event_name(event: &AgentEvent) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|value| {
            value
                .as_object()
                .and_then(|object| object.get("type"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "agent_event".to_string())
}

fn http_error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({
            "error": message
        })),
    )
}

fn map_runtime_error(error: MobRuntimeError) -> (StatusCode, Json<Value>) {
    match error {
        MobRuntimeError::InvalidInput(message) => http_error(StatusCode::BAD_REQUEST, message),
        MobRuntimeError::Mob(
            MobError::MemberNotFound(_)
            | MobError::SessionError(SessionError::NotFound { .. })
            | MobError::CommsError(SendError::PeerNotFound(_)),
        ) => http_error(StatusCode::NOT_FOUND, "member_not_found"),
        MobRuntimeError::Mob(MobError::SessionError(SessionError::Unsupported(_))) => {
            http_error(StatusCode::UNPROCESSABLE_ENTITY, "unsupported")
        }
        _ => http_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error"),
    }
}

// ---------------------------------------------------------------------------
// Tier 2: Per-agent persistent SSE  (MK-005)
// ---------------------------------------------------------------------------

pub type AgentEventSubscribeFuture =
    Pin<Box<dyn Future<Output = Result<EventStream, MobRuntimeError>> + Send>>;

pub type AgentEventSubscribeFn = Arc<dyn Fn(String) -> AgentEventSubscribeFuture + Send + Sync>;

#[derive(Clone)]
struct AgentSseState {
    subscribe_fn: AgentEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
}

pub fn agent_events_sse_router(
    subscribe_fn: AgentEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
) -> Router {
    agent_events_sse_router_with_access(subscribe_fn, decisions, None)
}

pub fn agent_events_sse_router_with_access(
    subscribe_fn: AgentEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
) -> Router {
    Router::new()
        .route("/agents/{agent_id}/events", get(agent_events_sse_handler))
        .with_state(AgentSseState {
            subscribe_fn,
            decisions,
            access,
        })
}

async fn agent_events_sse_handler(
    State(state): State<AgentSseState>,
    headers: HeaderMap,
    uri: Uri,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let access_view = sse_access_context(
        state.decisions.as_ref(),
        state.access.as_ref(),
        &headers,
        &uri,
    )
    .map_err(|()| sse_unauthorized("agent events stream requires a valid auth token"))?;
    if access_view
        .as_ref()
        .is_some_and(|view| view.enforced() && !view.allows_agent(ACTION_AGENT_VIEW, &agent_id))
    {
        return Err(sse_access_denied(ACTION_AGENT_VIEW));
    }
    let agent_id = agent_id.trim().to_string();
    if agent_id.is_empty() {
        return Err(http_error(
            StatusCode::BAD_REQUEST,
            "agent_id must not be empty",
        ));
    }

    let event_stream = (state.subscribe_fn)(agent_id.clone())
        .await
        .map_err(map_runtime_error)?;

    let stream = stream! {
        let mut seq = 0_u64;
        tokio::pin!(event_stream);
        while let Some(envelope) = event_stream.next().await {
            let event_name = agent_event_type(&envelope.payload).to_string();
            let payload = serde_json::to_string(&console_agent_event_payload(&envelope.payload))
                .unwrap_or_else(|_| "{}".to_string());
            yield Ok::<Event, Infallible>(
                Event::default()
                    .id(format!("{agent_id}:{seq}"))
                    .event(event_name)
                    .data(payload),
            );
            seq += 1;
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(DEFAULT_KEEP_ALIVE_INTERVAL)
            .text(KEEP_ALIVE_TEXT),
    ))
}

// ---------------------------------------------------------------------------
// Tier 3: Mob-merged SSE  (MK-006)
// ---------------------------------------------------------------------------

pub type MobEventSubscribeFuture = Pin<Box<dyn Future<Output = MobEventRouterHandle> + Send>>;

pub type MobEventSubscribeFn = Arc<dyn Fn() -> MobEventSubscribeFuture + Send + Sync>;

#[derive(Clone)]
struct MobSseState {
    subscribe_fn: MobEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
}

pub fn mob_events_sse_router(
    subscribe_fn: MobEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
) -> Router {
    mob_events_sse_router_with_access(subscribe_fn, decisions, None)
}

pub fn mob_events_sse_router_with_access(
    subscribe_fn: MobEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
) -> Router {
    Router::new()
        .route("/mob/events", get(mob_events_sse_handler))
        .with_state(MobSseState {
            subscribe_fn,
            decisions,
            access,
        })
}

async fn mob_events_sse_handler(
    State(state): State<MobSseState>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let access_view = sse_access_context(
        state.decisions.as_ref(),
        state.access.as_ref(),
        &headers,
        &uri,
    )
    .map_err(|()| sse_unauthorized("mob events stream requires a valid auth token"))?;
    // The merged mob stream carries every agent's events; it requires the
    // whole-mob observation grant rather than per-agent filtering.
    if access_view
        .as_ref()
        .is_some_and(|view| view.enforced() && !view.allows(ACTION_MOB_OBSERVE))
    {
        return Err(sse_access_denied(ACTION_MOB_OBSERVE));
    }
    let mut router_handle = (state.subscribe_fn)().await;

    let stream = stream! {
        let mut seq = 0_u64;
        while let Some(attributed) = router_handle.event_rx.recv().await {
            let event_name = agent_event_type(&attributed.envelope.payload).to_string();
            let source = attributed.source.to_string();
            let data = json!({
                "member_id": &source,
                "source": &source,
                "payload": console_agent_event_payload(&attributed.envelope.payload),
            });
            yield Ok::<Event, Infallible>(
                Event::default()
                    .id(format!("mob:{seq}"))
                    .event(event_name)
                    .data(data.to_string()),
            );
            seq += 1;
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(DEFAULT_KEEP_ALIVE_INTERVAL)
            .text(KEEP_ALIVE_TEXT),
    ))
}

// ---------------------------------------------------------------------------
// Structural mob events: per-client meerkat ledger subscription
// ---------------------------------------------------------------------------

/// Query parameters for `/mobkit/mob_events/stream`. Mirrors
/// [`EventQuery`] for the field filters; cursor pagination is `after_seq`.
#[derive(Debug, Default, Deserialize)]
pub struct MobStructuralStreamQuery {
    #[serde(default)]
    pub after_seq: Option<u64>,
    #[serde(default)]
    pub mob_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub identity: Option<String>,
    #[serde(default)]
    pub member_id: Option<String>,
    /// Comma-separated list of event-kind labels to keep
    /// (e.g. `flow_started,step_completed`). Empty / absent = all.
    #[serde(default)]
    pub event_types: Option<String>,
    #[serde(default)]
    pub since_ms: Option<u64>,
    #[serde(default)]
    pub until_ms: Option<u64>,
}

impl MobStructuralStreamQuery {
    fn into_event_query(self) -> EventQuery {
        EventQuery {
            since_ms: self.since_ms,
            until_ms: self.until_ms,
            member_id: self.member_id,
            identity: self.identity,
            mob_id: self.mob_id,
            run_id: self.run_id,
            step_id: self.step_id,
            event_types: self
                .event_types
                .map(|raw| {
                    raw.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            limit: None,
            after_seq: self.after_seq,
        }
    }
}

#[derive(Clone)]
struct MobStructuralSseState {
    handle: MobHandle,
    store: MobEventsStore,
    /// Optional auth context. When `Some`, requests are gated by the
    /// same `require_app_auth` toggle the console RPC route uses; when
    /// `None`, the route is unauthenticated (in-process or trusted
    /// embedding).
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
}

/// Per-client SSE subscription to the meerkat structural-event ledger.
///
/// Each connection opens its own `MobEventsView::subscribe_after` so
/// catch-up and live tail share the same ordered stream and there is
/// no race window between snapshot and live subscription. Stale
/// cursors are rejected with HTTP 410 Gone before any SSE handshake.
/// Filtering matches the `mobkit/mob_events/query` predicate; the
/// per-client `MobEventsSubscription` is dropped when the client
/// disconnects, which cancels the upstream forwarder automatically.
///
/// When `decisions` is `Some` and `decisions.console.require_app_auth`
/// is on, every request must carry a valid bearer token (Authorization
/// header or `auth_token` query param) — same gate the console RPC
/// route uses. `None` opts out of auth (e.g. trusted local embedding).
pub fn mob_structural_events_sse_router(
    handle: MobHandle,
    store: MobEventsStore,
    decisions: Option<RuntimeDecisionState>,
) -> Router {
    mob_structural_events_sse_router_with_access(handle, store, decisions, None)
}

pub fn mob_structural_events_sse_router_with_access(
    handle: MobHandle,
    store: MobEventsStore,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
) -> Router {
    Router::new()
        .route(
            MOB_EVENTS_STREAM_PATH,
            get(mob_structural_events_sse_handler),
        )
        .with_state(MobStructuralSseState {
            handle,
            store,
            decisions,
            access,
        })
}

/// Shared auth gate for every SSE route in mobkit. When `decisions` is
/// `Some(_)` and `decisions.console.require_app_auth` is on, the
/// request must carry a valid bearer / `auth_token` token; otherwise
/// the route is open. Used by `mob_structural_events_sse_router`,
/// `interaction_stream_router`, and the agent-/mob-event tier 2/3
/// routers.
///
/// On success returns the caller's [`AccessView`] when an
/// [`AccessController`] is wired (anonymous view on open routes), so the
/// SSE handlers can apply per-agent ABAC checks. `Err(())` means 401.
pub(crate) fn sse_access_context(
    decisions: Option<&RuntimeDecisionState>,
    access: Option<&AccessController>,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<Option<AccessView>, ()> {
    let Some(decisions) = decisions else {
        return Ok(access.map(|controller| controller.view_for_subject(None)));
    };
    let bearer_token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(extract_bearer_token_from_header)
        .map(String::from);
    // Parse the query string with `form_urlencoded` so percent-encoded
    // tokens (e.g. base64 padding `=` re-encoded as `%3D`) decode
    // correctly, and so substring-shadowing values like `xauth_token=`
    // don't masquerade as `auth_token=`.
    let query_token = uri.query().and_then(|q| {
        form_urlencoded::parse(q.as_bytes())
            .find(|(key, _)| key == "auth_token")
            .map(|(_, value)| value.into_owned())
    });
    let token = bearer_token.or(query_token);
    if !decisions.console.require_app_auth {
        // Open route: identify callers that volunteered a valid token so
        // per-user ABAC grants apply; everyone else is anonymous.
        let subject = token.as_deref().and_then(|token| {
            crate::runtime::resolve_authorized_console_auth_from_token(decisions, token)
                .map(|auth| auth.email)
        });
        return Ok(access.map(|controller| controller.view_for_subject(subject.as_deref())));
    }
    let token = token.ok_or(())?;
    let auth =
        crate::runtime::resolve_authorized_console_auth_from_token(decisions, &token).ok_or(())?;
    Ok(access.map(|controller| controller.view_for_subject(Some(auth.email.as_str()))))
}

fn sse_unauthorized(reason: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "unauthorized",
            "reason": reason,
        })),
    )
}

fn sse_access_denied(action: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "access_denied",
            "action": action,
        })),
    )
}

async fn mob_structural_events_sse_handler(
    State(state): State<MobStructuralSseState>,
    headers: HeaderMap,
    uri: Uri,
    Query(params): Query<MobStructuralStreamQuery>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<Value>)>
{
    let access_view = sse_access_context(
        state.decisions.as_ref(),
        state.access.as_ref(),
        &headers,
        &uri,
    )
    .map_err(|()| sse_unauthorized("mob_events stream requires a valid auth token"))?;
    // Structural events span the whole mob: require the mob-wide
    // observation grant, mirroring `mobkit/mob_events/query`.
    if access_view
        .as_ref()
        .is_some_and(|view| view.enforced() && !view.allows(ACTION_MOB_OBSERVE))
    {
        return Err(sse_access_denied(ACTION_MOB_OBSERVE));
    }
    let query = params.into_event_query();
    let events_view = state.handle.events();

    let latest = events_view.latest_cursor().await.map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "events_view_unavailable",
                "detail": err.to_string(),
            })),
        )
    })?;

    if let Some(after_seq) = query.after_seq
        && after_seq > latest
    {
        return Err((
            StatusCode::GONE,
            Json(json!({
                "error": "event_query_stale",
                "after_cursor": after_seq,
                "latest_cursor": latest,
            })),
        ));
    }

    let after_cursor = query.after_seq.unwrap_or(latest);
    let mut subscription = events_view
        .subscribe_after(after_cursor)
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "subscribe_failed",
                    "detail": err.to_string(),
                })),
            )
        })?;

    let store = state.store;
    let stream = stream! {
        while let Some(event) = subscription.event_rx.recv().await {
            let envelope = store.project_event_for_query(&event).await;
            if !crate::unified_runtime::mob_events::envelope_matches(&envelope, &query) {
                continue;
            }
            let payload = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
            yield Ok::<Event, Infallible>(
                Event::default()
                    .id(format!("mob-evt-{}", envelope.cursor))
                    .event(envelope.kind.clone())
                    .data(payload),
            );
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(DEFAULT_KEEP_ALIVE_INTERVAL)
            .text(KEEP_ALIVE_TEXT),
    ))
}
