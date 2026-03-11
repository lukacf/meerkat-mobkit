//! Server-Sent Events (SSE) streaming endpoints for agent and mob observation.

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
use axum::routing::get;
use axum::{Json, Router};
use futures::StreamExt;
use meerkat_core::comms::EventStream;
use meerkat_core::event::agent_event_type;
use meerkat_core::AgentEvent;
use meerkat_mob::MobEventRouterHandle;
use serde_json::{json, Value};

use crate::mob_handle_runtime::MobRuntimeError;
use meerkat_core::comms::SendError;
use meerkat_core::service::SessionError;
use meerkat_mob::MobError;

pub(crate) const DEFAULT_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);
const KEEP_ALIVE_TEXT: &str = "keep-alive";

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
