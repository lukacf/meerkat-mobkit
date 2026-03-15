//! HTTP interaction streaming — observe-only SSE endpoint.
//!
//! The published console contract is:
//!   1. Client sends the message via `mobkit/send_message` (RPC).
//!   2. Client opens `POST /interactions/stream` with `{ member_id }` to
//!      observe the resulting agent event stream.
//!
//! This endpoint does NOT send a message; it only subscribes to agent events
//! for the given member and streams them until a terminal event arrives.

use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::post;
use axum::{Json, Router};
use futures::StreamExt;
use meerkat_core::{AgentEvent, event::agent_event_type};
use meerkat_mob::MeerkatId;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::http_sse::{DEFAULT_KEEP_ALIVE_INTERVAL, KEEP_ALIVE_TEXT};
use crate::mob_handle_runtime::MobRuntimeError;

/// Observe-only request: only `member_id` is required.
#[derive(Debug, Deserialize)]
struct InteractionStreamRequest {
    member_id: String,
}

#[derive(Clone)]
struct InteractionState {
    runtime: crate::mob_handle_runtime::RealMobRuntime,
}

pub fn interaction_stream_router(runtime: crate::mob_handle_runtime::RealMobRuntime) -> Router {
    Router::new()
        .route("/interactions/stream", post(interaction_stream_handler))
        .with_state(InteractionState { runtime })
}

fn http_error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({
            "error": message
        })),
    )
}

fn map_runtime_error(error: &MobRuntimeError) -> (StatusCode, Json<Value>) {
    match error {
        MobRuntimeError::InvalidInput(message) => http_error(StatusCode::BAD_REQUEST, message),
        MobRuntimeError::Mob(_) => {
            let text = error.to_string();
            if text.contains("not found") {
                http_error(StatusCode::NOT_FOUND, "member_not_found")
            } else if text.contains("not externally addressable") {
                http_error(StatusCode::FORBIDDEN, "not_externally_addressable")
            } else if text.contains("unsupported") {
                http_error(StatusCode::UNPROCESSABLE_ENTITY, "unsupported")
            } else if text.contains("busy") {
                http_error(StatusCode::CONFLICT, "member_busy")
            } else {
                http_error(StatusCode::INTERNAL_SERVER_ERROR, "interaction_failed")
            }
        }
    }
}

async fn interaction_stream_handler(
    State(state): State<InteractionState>,
    Json(request): Json<InteractionStreamRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let member_id = request.member_id.trim().to_string();

    if member_id.is_empty() {
        return Err(http_error(
            StatusCode::BAD_REQUEST,
            "member_id must not be empty",
        ));
    }

    let mut event_stream = state
        .runtime
        .handle()
        .subscribe_agent_events(&MeerkatId::from(member_id.clone()))
        .await
        .map_err(|error| map_runtime_error(&MobRuntimeError::Mob(error)))?;

    let stream = stream! {
        yield Ok::<Event, Infallible>(
            Event::default()
                .event("subscribed")
                .data(json!({
                    "type": "subscribed",
                    "member_id": member_id,
                }).to_string())
        );

        let mut seq = 0_u64;
        loop {
            let next = tokio::time::timeout(Duration::from_secs(300), event_stream.next()).await;
            let Some(envelope) = next.unwrap_or_default() else {
                break;
            };

            let event_name = agent_event_type(&envelope.payload).to_string();
            let payload = serde_json::to_string(&envelope.payload)
                .unwrap_or_else(|_| "{}".to_string());
            let terminal = matches!(
                envelope.payload,
                AgentEvent::RunCompleted { .. }
                    | AgentEvent::RunFailed { .. }
                    | AgentEvent::InteractionComplete { .. }
                    | AgentEvent::InteractionFailed { .. }
            );

            yield Ok::<Event, Infallible>(
                Event::default()
                    .id(format!("{member_id}:{seq}"))
                    .event(event_name)
                    .data(payload),
            );
            seq += 1;

            if terminal {
                break;
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(DEFAULT_KEEP_ALIVE_INTERVAL)
            .text(KEEP_ALIVE_TEXT),
    ))
}
