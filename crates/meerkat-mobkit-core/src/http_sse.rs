use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_stream::stream;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use meerkat_core::AgentEvent;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::mob_handle_runtime::{MobRuntimeError, RealInteractionSubscription, RealMobRuntime};
use meerkat_core::comms::SendError;
use meerkat_core::service::SessionError;
use meerkat_mob::MobError;

const DEFAULT_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);
const KEEP_ALIVE_TEXT: &str = "keep-alive";
const DEFAULT_RING_BUFFER_CAPACITY: usize = 2000;

// ---------------------------------------------------------------------------
// SSE Ring Buffer
// ---------------------------------------------------------------------------

/// A single SSE event stored for replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseStoredEvent {
    pub id: String,
    pub event_type: String,
    pub data: String,
}

/// A fixed-capacity ring buffer that stores recent SSE events so that
/// reconnecting clients can receive events they missed.
///
/// When the buffer is at capacity, the oldest event is evicted on push.
pub struct SseRingBuffer {
    events: VecDeque<SseStoredEvent>,
    capacity: usize,
}

impl SseRingBuffer {
    /// Create a new ring buffer with the given maximum capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push an event into the buffer, evicting the oldest if at capacity.
    pub fn push(&mut self, event: SseStoredEvent) {
        if self.capacity == 0 {
            return;
        }
        if self.events.len() == self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// Return all events stored after the event with the given ID.
    ///
    /// Returns `Some(events)` when the ID is found (the vec may be empty
    /// when the ID is the most recent event).  Returns `None` when the
    /// ID is not in the buffer (replay gap).
    pub fn replay_after(&self, last_event_id: &str) -> Option<Vec<&SseStoredEvent>> {
        let idx = self
            .events
            .iter()
            .position(|e| e.id == last_event_id)?;

        Some(self.events.iter().skip(idx + 1).collect())
    }

    /// Check whether the buffer contains an event with the given ID.
    pub fn contains(&self, event_id: &str) -> bool {
        self.events.iter().any(|e| e.id == event_id)
    }

    /// The number of events currently stored.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Thread-safe wrapper around [`SseRingBuffer`].
///
/// Uses `std::sync::Mutex` because all operations are fast, in-memory
/// data-structure manipulations with no async work while the lock is held.
#[derive(Clone)]
pub struct SharedSseRingBuffer {
    inner: Arc<Mutex<SseRingBuffer>>,
}

impl SharedSseRingBuffer {
    /// Create a shared ring buffer with the default capacity (2000 events).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_RING_BUFFER_CAPACITY)
    }

    /// Create a shared ring buffer with a custom capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SseRingBuffer::new(capacity))),
        }
    }

    /// Push an event into the buffer.
    pub fn push(&self, event: SseStoredEvent) {
        let mut buf = self.inner.lock().expect("ring buffer lock poisoned");
        buf.push(event);
    }

    /// Replay events after the given `last_event_id`.
    ///
    /// Returns `Ok(events)` when the ID is found (events may be empty if the
    /// ID was the most recent).  Returns `Err(())` when the ID is not found,
    /// indicating a replay gap.
    pub fn replay_after(&self, last_event_id: &str) -> Result<Vec<SseStoredEvent>, ()> {
        let buf = self.inner.lock().expect("ring buffer lock poisoned");
        buf.replay_after(last_event_id)
            .map(|events| events.into_iter().cloned().collect())
            .ok_or(())
    }

    /// The number of events currently stored.
    pub fn len(&self) -> usize {
        let buf = self.inner.lock().expect("ring buffer lock poisoned");
        buf.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for SharedSseRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// State & types
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct InteractionSseState {
    inject_and_subscribe: InteractionSseInjectFn,
    keep_alive_interval: Duration,
    ring_buffer: Option<SharedSseRingBuffer>,
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

pub fn interaction_sse_router_with_ring_buffer(
    runtime: RealMobRuntime,
    ring_buffer: SharedSseRingBuffer,
) -> Router {
    build_interaction_sse_router(
        runtime_inject_fn(runtime),
        DEFAULT_KEEP_ALIVE_INTERVAL,
        Some(ring_buffer),
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
    build_interaction_sse_router(inject_and_subscribe, keep_alive_interval, None)
}

fn build_interaction_sse_router(
    inject_and_subscribe: InteractionSseInjectFn,
    keep_alive_interval: Duration,
    ring_buffer: Option<SharedSseRingBuffer>,
) -> Router {
    Router::new()
        .route(
            "/interactions/stream",
            post(interaction_sse_handler_with_state),
        )
        .with_state(InteractionSseState {
            inject_and_subscribe,
            keep_alive_interval,
            ring_buffer,
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
        None,
        None,
    )
    .await
}

async fn interaction_sse_handler_with_state(
    State(state): State<InteractionSseState>,
    headers: HeaderMap,
    Json(request): Json<InjectSseRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let last_event_id = headers
        .get("Last-Event-ID")
        .or_else(|| headers.get("last-event-id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    interaction_sse_response(
        state.inject_and_subscribe.clone(),
        request,
        state.keep_alive_interval,
        state.ring_buffer.clone(),
        last_event_id,
    )
    .await
}

async fn interaction_sse_response(
    inject_and_subscribe: InteractionSseInjectFn,
    request: InjectSseRequest,
    keep_alive_interval: Duration,
    ring_buffer: Option<SharedSseRingBuffer>,
    last_event_id: Option<String>,
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

    // Determine replay events before starting the live stream.
    let replay = match (&ring_buffer, &last_event_id) {
        (Some(buf), Some(id)) if !id.is_empty() => Some(buf.replay_after(id)),
        _ => None,
    };

    // SAFETY: When Last-Event-ID is present, this is a browser reconnect.
    // Reject with 409 to prevent re-injection (duplicate side effects).
    // Clients should use the replay-only endpoint for reconnection.
    if last_event_id.is_some() {
        return Err(http_error(
            StatusCode::CONFLICT,
            "reconnect with Last-Event-ID on POST would re-inject; use GET /interactions/replay instead",
        ));
    }

    let mut subscription = (inject_and_subscribe)(member_id.clone(), request.message)
        .await
        .map_err(map_runtime_error)?;

    let interaction_id = subscription.interaction_id.clone();
    let stream_member_id = member_id.clone();
    let stream = stream! {
        // --- Replay phase ---
        if let Some(replay_result) = replay {
            match replay_result {
                Ok(events) => {
                    for stored in &events {
                        yield Ok::<Event, Infallible>(
                            Event::default()
                                .id(&*stored.id)
                                .event(&*stored.event_type)
                                .data(&*stored.data),
                        );
                    }
                }
                Err(()) => {
                    // The Last-Event-ID was not found in the buffer -- gap.
                    let gap_data = json!({
                        "last_event_id": last_event_id,
                        "reason": "buffer_overflow",
                    });
                    yield Ok::<Event, Infallible>(
                        Event::default()
                            .event("replay_gap")
                            .data(gap_data.to_string()),
                    );
                }
            }
        }

        // --- Live phase ---
        let mut seq = 0_u64;
        let start_data = json!({
            "interaction_id": interaction_id,
            "member_id": stream_member_id,
        });

        let start_id = format!("{interaction_id}:{seq}");
        let start_event_type = "interaction_started".to_string();
        let start_data_str = start_data.to_string();

        if let Some(ref buf) = ring_buffer {
            buf.push(SseStoredEvent {
                id: start_id.clone(),
                event_type: start_event_type.clone(),
                data: start_data_str.clone(),
            });
        }

        yield Ok::<Event, Infallible>(
            Event::default()
                .id(start_id)
                .event(start_event_type)
                .data(start_data_str),
        );
        seq += 1;

        while let Some(event) = subscription.events.recv().await {
            let id = format!("{interaction_id}:{seq}");
            let event_name = agent_event_name(&event);
            let payload = serde_json::to_string(&event)
                .unwrap_or_else(|_| "{}".to_string());

            if let Some(ref buf) = ring_buffer {
                buf.push(SseStoredEvent {
                    id: id.clone(),
                    event_type: event_name.clone(),
                    data: payload.clone(),
                });
            }

            yield Ok::<Event, Infallible>(
                Event::default()
                    .id(id)
                    .event(event_name)
                    .data(payload),
            );
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
