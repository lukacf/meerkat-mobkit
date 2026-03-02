use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use meerkat_core::AgentEvent;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::mob_handle_runtime::{MobRuntimeError, RealMobRuntime};
use meerkat_core::comms::SendError;
use meerkat_core::service::SessionError;
use meerkat_mob::MobError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InjectSseRequest {
    pub member_id: String,
    pub message: String,
}

pub fn interaction_sse_router(runtime: RealMobRuntime) -> Router {
    Router::new()
        .route("/interactions/stream", post(interaction_sse_handler))
        .with_state(runtime)
}

pub async fn interaction_sse_handler(
    State(runtime): State<RealMobRuntime>,
    Json(request): Json<InjectSseRequest>,
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

    let mut subscription = runtime
        .inject_and_subscribe(&member_id, request.message)
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
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
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
