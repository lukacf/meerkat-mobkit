use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use meerkat_core::comms::EventStream;
use meerkat_core::event::agent_event_type;
use meerkat_core::AgentEvent;
use meerkat_mob::MobEventRouterHandle;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::mob_handle_runtime::{MobRuntimeError, RealInteractionSubscription, RealMobRuntime};
use meerkat_core::comms::SendError;
use meerkat_core::service::SessionError;
use meerkat_mob::MobError;

const DEFAULT_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);
const KEEP_ALIVE_TEXT: &str = "keep-alive";

#[derive(Clone)]
struct InteractionSseState {
    inject_and_subscribe: InteractionSseInjectFn,
    keep_alive_interval: Duration,
}

pub(crate) type InteractionSseInjectFuture =
    Pin<Box<dyn Future<Output = Result<RealInteractionSubscription, MobRuntimeError>> + Send>>;
pub(crate) type InteractionSseInjectFn =
    Arc<dyn Fn(String, String) -> InteractionSseInjectFuture + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InjectSseRequest {
    pub member_id: String,
    pub message: String,
}

pub fn interaction_sse_router(runtime: RealMobRuntime) -> Router {
    interaction_sse_router_with_injector(runtime_inject_fn(runtime))
}

pub fn interaction_sse_router_with_keep_alive_interval(
    runtime: RealMobRuntime,
    keep_alive_interval: Duration,
) -> Router {
    interaction_sse_router_with_injector_and_keep_alive_interval(
        runtime_inject_fn(runtime),
        keep_alive_interval,
    )
}

pub(crate) fn interaction_sse_router_with_injector(
    inject_and_subscribe: InteractionSseInjectFn,
) -> Router {
    interaction_sse_router_with_injector_and_keep_alive_interval(
        inject_and_subscribe,
        DEFAULT_KEEP_ALIVE_INTERVAL,
    )
}

pub(crate) fn interaction_sse_router_with_injector_and_keep_alive_interval(
    inject_and_subscribe: InteractionSseInjectFn,
    keep_alive_interval: Duration,
) -> Router {
    Router::new()
        .route(
            "/interactions/stream",
            post(interaction_sse_handler_with_state),
        )
        .with_state(InteractionSseState {
            inject_and_subscribe,
            keep_alive_interval,
        })
}

pub async fn interaction_sse_handler(
    State(runtime): State<RealMobRuntime>,
    Json(request): Json<InjectSseRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    interaction_sse_response(
        runtime_inject_fn(runtime),
        request,
        DEFAULT_KEEP_ALIVE_INTERVAL,
    )
    .await
}

async fn interaction_sse_handler_with_state(
    State(state): State<InteractionSseState>,
    Json(request): Json<InjectSseRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    interaction_sse_response(
        state.inject_and_subscribe.clone(),
        request,
        state.keep_alive_interval,
    )
    .await
}

async fn interaction_sse_response(
    inject_and_subscribe: InteractionSseInjectFn,
    request: InjectSseRequest,
    keep_alive_interval: Duration,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let member_id = request.member_id.trim().to_string();
    if member_id.is_empty() {
        return Err(http_error(
            StatusCode::BAD_REQUEST,
            "member_id must not be empty",
        ));
    }
    if request.message.trim().is_empty() {
        return Err(http_error(
            StatusCode::BAD_REQUEST,
            "message must not be empty",
        ));
    }

    let mut subscription = (inject_and_subscribe)(member_id.clone(), request.message)
        .await
        .map_err(map_runtime_error)?;

    let interaction_id = subscription.interaction_id.clone();
    let stream_member_id = member_id.clone();
    let stream = stream! {
        let mut seq = 0_u64;
        let start_data = json!({
            "interaction_id": interaction_id,
            "member_id": stream_member_id,
        });
        yield Ok::<Event, Infallible>(
            Event::default()
                .id(format!("{interaction_id}:{seq}"))
                .event("interaction_started")
                .data(start_data.to_string()),
        );
        seq += 1;

        while let Some(event) = subscription.events.recv().await {
            yield Ok::<Event, Infallible>(agent_event_sse(&interaction_id, seq, &event));
            seq += 1;
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(keep_alive_interval)
            .text(KEEP_ALIVE_TEXT),
    ))
}

pub fn agent_event_sse(interaction_id: &str, seq: u64, event: &AgentEvent) -> Event {
    let event_name = agent_event_name(event);
    let payload = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
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
        MobRuntimeError::Mob(MobError::MeerkatNotFound(_))
        | MobRuntimeError::Mob(MobError::SessionError(SessionError::NotFound { .. }))
        | MobRuntimeError::Mob(MobError::CommsError(SendError::PeerNotFound(_))) => {
            http_error(StatusCode::NOT_FOUND, "member_not_found")
        }
        _ => http_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error"),
    }
}

fn runtime_inject_fn(runtime: RealMobRuntime) -> InteractionSseInjectFn {
    Arc::new(move |member_id: String, message: String| {
        let runtime = runtime.clone();
        Box::pin(async move { runtime.inject_and_subscribe(&member_id, message).await })
    })
}

// ---------------------------------------------------------------------------
// Tier 2: Per-agent persistent SSE  (MK-005)
// ---------------------------------------------------------------------------

pub type AgentEventSubscribeFuture =
    Pin<Box<dyn Future<Output = Result<EventStream, MobRuntimeError>> + Send>>;

pub type AgentEventSubscribeFn =
    Arc<dyn Fn(String) -> AgentEventSubscribeFuture + Send + Sync>;

#[derive(Clone)]
struct AgentSseState {
    subscribe_fn: AgentEventSubscribeFn,
}

pub fn agent_events_sse_router(subscribe_fn: AgentEventSubscribeFn) -> Router {
    Router::new()
        .route("/agents/:agent_id/events", get(agent_events_sse_handler))
        .with_state(AgentSseState { subscribe_fn })
}

async fn agent_events_sse_handler(
    State(state): State<AgentSseState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let agent_id = agent_id.trim().to_string();
    if agent_id.is_empty() {
        return Err(http_error(StatusCode::BAD_REQUEST, "agent_id must not be empty"));
    }

    let event_stream = (state.subscribe_fn)(agent_id.clone())
        .await
        .map_err(map_runtime_error)?;

    let stream = stream! {
        let mut seq = 0_u64;
        tokio::pin!(event_stream);
        while let Some(envelope) = event_stream.next().await {
            let event_name = agent_event_type(&envelope.payload).to_string();
            let payload = serde_json::to_string(&envelope.payload)
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

pub type MobEventSubscribeFuture =
    Pin<Box<dyn Future<Output = MobEventRouterHandle> + Send>>;

pub type MobEventSubscribeFn = Arc<dyn Fn() -> MobEventSubscribeFuture + Send + Sync>;

#[derive(Clone)]
struct MobSseState {
    subscribe_fn: MobEventSubscribeFn,
}

pub fn mob_events_sse_router(subscribe_fn: MobEventSubscribeFn) -> Router {
    Router::new()
        .route("/mob/events", get(mob_events_sse_handler))
        .with_state(MobSseState { subscribe_fn })
}

async fn mob_events_sse_handler(State(state): State<MobSseState>) -> impl IntoResponse {
    let mut router_handle = (state.subscribe_fn)().await;

    let stream = stream! {
        let mut seq = 0_u64;
        while let Some(attributed) = router_handle.event_rx.recv().await {
            let event_name = agent_event_type(&attributed.envelope.payload).to_string();
            let data = json!({
                "agent_id": attributed.source.to_string(),
                "event": attributed.envelope.payload,
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

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(DEFAULT_KEEP_ALIVE_INTERVAL)
            .text(KEEP_ALIVE_TEXT),
    )
}
