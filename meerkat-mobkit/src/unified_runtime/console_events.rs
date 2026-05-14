use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast};

use crate::console_contracts::{
    ALL_EVENTS_STREAM_NAME, ConsoleIdentityEventEnvelope, ReplayUnavailableError,
    SYSTEM_EVENT_IDENTITY,
};
use crate::types::{EventEnvelope, UnifiedEvent};

const IDENTITY_REPLAY_CAP: usize = 1024;
const ALL_EVENTS_REPLAY_CAP: usize = 4096;
const EVENT_CHANNEL_CAP: usize = 512;
const PENDING_INTERACTION_CAP: usize = 256;

#[derive(Clone)]
pub(crate) struct ConsoleEventStore {
    next_event_seq: Arc<AtomicU64>,
    state: Arc<RwLock<ConsoleEventReplayState>>,
    event_tx: broadcast::Sender<ConsoleIdentityEventEnvelope>,
}

struct ConsoleEventReplayState {
    all_events: VecDeque<ConsoleIdentityEventEnvelope>,
    by_identity: BTreeMap<String, VecDeque<ConsoleIdentityEventEnvelope>>,
    pending_by_identity: BTreeMap<String, VecDeque<PendingInteraction>>,
    runtime_to_identity: BTreeMap<String, String>,
    response_phase_by_identity: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Clone)]
struct PendingInteraction {
    interaction_id: String,
    origin: String,
    content: Value,
}

impl ConsoleEventStore {
    pub(crate) fn new() -> Self {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAP);
        let bootstrap = ConsoleIdentityEventEnvelope {
            event_id: "console-evt-1".to_string(),
            interaction_id: None,
            identity: SYSTEM_EVENT_IDENTITY.to_string(),
            event_type: "runtime_bootstrapped".to_string(),
            timestamp_ms: current_time_ms(),
            data: json!({
                "source": "unified_runtime",
            }),
        };
        let mut by_identity = BTreeMap::new();
        by_identity.insert(
            bootstrap.identity.clone(),
            VecDeque::from([bootstrap.clone()]),
        );
        Self {
            next_event_seq: Arc::new(AtomicU64::new(2)),
            state: Arc::new(RwLock::new(ConsoleEventReplayState {
                all_events: VecDeque::from([bootstrap]),
                by_identity,
                pending_by_identity: BTreeMap::new(),
                runtime_to_identity: BTreeMap::new(),
                response_phase_by_identity: BTreeMap::new(),
            })),
            event_tx,
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<ConsoleIdentityEventEnvelope> {
        self.event_tx.subscribe()
    }

    pub(crate) async fn append(
        &self,
        identity: impl Into<String>,
        interaction_id: Option<String>,
        event_type: impl Into<String>,
        data: Value,
    ) -> ConsoleIdentityEventEnvelope {
        let identity = identity.into();
        let envelope = ConsoleIdentityEventEnvelope {
            event_id: format!(
                "console-evt-{}",
                self.next_event_seq.fetch_add(1, Ordering::Relaxed)
            ),
            interaction_id,
            identity,
            event_type: event_type.into(),
            timestamp_ms: current_time_ms(),
            data,
        };
        self.append_envelope(envelope.clone()).await;
        envelope
    }

    pub(crate) async fn append_envelope(&self, envelope: ConsoleIdentityEventEnvelope) {
        {
            let mut state = self.state.write().await;
            state.all_events.push_back(envelope.clone());
            trim_deque(&mut state.all_events, ALL_EVENTS_REPLAY_CAP);

            let replay = state
                .by_identity
                .entry(envelope.identity.clone())
                .or_insert_with(VecDeque::new);
            replay.push_back(envelope.clone());
            trim_deque(replay, IDENTITY_REPLAY_CAP);
        }
        let _ = self.event_tx.send(envelope);
    }

    pub(crate) async fn register_runtime_identity(
        &self,
        runtime_member_id: impl Into<String>,
        identity: impl Into<String>,
    ) {
        let runtime_member_id = runtime_member_id.into();
        let identity = identity.into();
        if runtime_member_id.trim().is_empty() || identity.trim().is_empty() {
            return;
        }
        let mut state = self.state.write().await;
        state
            .runtime_to_identity
            .insert(runtime_member_id, identity);
    }

    pub(crate) async fn replay_all(
        &self,
        last_event_id: Option<&str>,
    ) -> Result<Vec<ConsoleIdentityEventEnvelope>, ReplayUnavailableError> {
        let state = self.state.read().await;
        replay_slice(
            state.all_events.clone(),
            last_event_id,
            ALL_EVENTS_STREAM_NAME,
            state.all_events.back().map(|event| event.event_id.clone()),
        )
    }

    pub(crate) async fn reserve_interaction_value(
        &self,
        identity: &str,
        runtime_member_id: Option<&str>,
        interaction_id: &str,
        origin: &str,
        content: Value,
    ) -> Result<(), &'static str> {
        // If projection later fails to resolve an identity (e.g. runtime id
        // format changes), stale pending entries can accumulate. Rather than
        // reject new interactions once the per-identity cap is hit — which
        // would deadlock legitimate traffic behind orphans — evict the oldest
        // entry and surface an `interaction_failed` event so the client
        // stops waiting.
        let evicted = {
            let mut state = self.state.write().await;
            let queue = state
                .pending_by_identity
                .entry(identity.to_string())
                .or_default();
            let evicted = if queue.len() >= PENDING_INTERACTION_CAP {
                queue.pop_front()
            } else {
                None
            };
            queue.push_back(PendingInteraction {
                interaction_id: interaction_id.to_string(),
                origin: origin.to_string(),
                content,
            });
            if let Some(runtime_member_id) =
                runtime_member_id.filter(|value| !value.trim().is_empty())
            {
                state
                    .runtime_to_identity
                    .insert(runtime_member_id.to_string(), identity.to_string());
            }
            state
                .response_phase_by_identity
                .insert(identity.to_string(), Some("waiting".to_string()));
            evicted
        };
        if let Some(evicted) = evicted {
            tracing::warn!(
                identity = %identity,
                interaction_id = %evicted.interaction_id,
                "evicting stalled pending interaction: per-identity queue at cap"
            );
            self.append(
                identity,
                Some(evicted.interaction_id),
                "interaction_failed",
                json!({
                    "reason": "queue_overflow",
                    "origin": evicted.origin,
                    "content": evicted.content,
                }),
            )
            .await;
        }
        Ok(())
    }

    pub(crate) async fn record_lifecycle(&self, identity: &str, event_type: &str, data: Value) {
        let failed = {
            let mut state = self.state.write().await;
            let pending = state
                .pending_by_identity
                .remove(identity)
                .unwrap_or_default();
            state
                .response_phase_by_identity
                .insert(identity.to_string(), None);
            pending.into_iter().collect::<Vec<_>>()
        };
        for pending in failed {
            self.append(
                identity,
                Some(pending.interaction_id),
                "interaction_failed",
                json!({
                    "reason": "lifecycle_mutation",
                    "origin": pending.origin,
                    "content": pending.content,
                    "lifecycle_event": event_type,
                }),
            )
            .await;
        }
        self.append(identity, None, event_type, data).await;
    }

    pub(crate) async fn project_unified_event(&self, event: &EventEnvelope<UnifiedEvent>) {
        let UnifiedEvent::Agent {
            agent_id,
            event_type,
            payload,
        } = &event.event
        else {
            return;
        };
        if is_empty_web_search_annotations_event(event_type, payload.as_ref()) {
            return;
        }

        let (identity, interaction_id) = {
            let mut state = self.state.write().await;
            let identity = state
                .runtime_to_identity
                .get(agent_id)
                .cloned()
                .or_else(|| derive_identity_from_runtime_id(agent_id));
            let Some(identity) = identity else {
                tracing::debug!(
                    agent_id = %agent_id,
                    event_type = %event_type,
                    "dropping agent event: runtime id did not resolve to a registered identity"
                );
                return;
            };
            state
                .runtime_to_identity
                .entry(agent_id.clone())
                .or_insert_with(|| identity.clone());
            let interaction_id = state
                .pending_by_identity
                .get(&identity)
                .and_then(|queue| queue.front())
                .map(|pending| pending.interaction_id.clone());
            (identity, interaction_id)
        };

        let projected_type = match event_type.as_str() {
            "run_completed" => "interaction_complete",
            "run_failed" => "interaction_failed",
            other => other,
        };
        let mut projected_data = payload.clone().unwrap_or_else(|| json!({}));
        if let Some(object) = projected_data.as_object_mut() {
            object
                .entry("source_event_type".to_string())
                .or_insert_with(|| Value::String(event_type.clone()));
        }

        self.append_envelope(ConsoleIdentityEventEnvelope {
            event_id: event.event_id.clone(),
            interaction_id: interaction_id.clone(),
            identity: identity.clone(),
            event_type: projected_type.to_string(),
            timestamp_ms: event.timestamp_ms,
            data: projected_data.clone(),
        })
        .await;

        if let Some(image_result) = parse_generate_image_tool_result(&projected_data) {
            for (idx, image) in image_result.images.iter().enumerate() {
                self.append_envelope(ConsoleIdentityEventEnvelope {
                    event_id: format!("{}#assistant_image:{idx}", event.event_id),
                    interaction_id: interaction_id.clone(),
                    identity: identity.clone(),
                    event_type: "assistant_image".to_string(),
                    timestamp_ms: event.timestamp_ms,
                    data: json!({
                        "source_event_type": event_type,
                        "tool_call_id": projected_data.get("id").cloned().unwrap_or(Value::Null),
                        "image_id": image.image_id.0.to_string(),
                        "blob_id": image.blob_ref.blob_id,
                        "media_type": image.media_type.as_str(),
                        "width": image.width,
                        "height": image.height,
                        "revised_prompt": image_result.revised_prompt.clone(),
                    }),
                })
                .await;
            }
        }

        {
            let mut state = self.state.write().await;
            match event_type.as_str() {
                "tool_call_requested" | "tool_call" | "tool_result_received" => {
                    state
                        .response_phase_by_identity
                        .insert(identity.clone(), Some("tool-executing".to_string()));
                }
                "text_delta" => {
                    state
                        .response_phase_by_identity
                        .insert(identity.clone(), Some("generating".to_string()));
                }
                "run_completed" | "run_failed" => {
                    state
                        .response_phase_by_identity
                        .insert(identity.clone(), None);
                }
                _ => {}
            }
        }

        if matches!(event_type.as_str(), "run_completed" | "run_failed") {
            let mut state = self.state.write().await;
            if let Some(interaction_id) = interaction_id.as_deref()
                && let Some(queue) = state.pending_by_identity.get_mut(&identity)
                && queue
                    .front()
                    .is_some_and(|pending| pending.interaction_id == interaction_id)
            {
                queue.pop_front();
                if queue.is_empty() {
                    state.pending_by_identity.remove(&identity);
                }
            }
        }
    }

    pub(crate) async fn response_phase_for_identity(&self, identity: &str) -> Option<String> {
        self.state
            .read()
            .await
            .response_phase_by_identity
            .get(identity)
            .cloned()
            .flatten()
    }
}

fn parse_generate_image_tool_result(
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

pub(crate) fn is_empty_web_search_annotations_event(
    event_type: &str,
    payload: Option<&Value>,
) -> bool {
    if event_type != "server_tool_content" {
        return false;
    }
    let Some(payload) = payload else {
        return false;
    };
    payload.get("name").and_then(Value::as_str) == Some("web_search_annotations")
        && payload
            .get("content")
            .and_then(|content| content.get("type"))
            .and_then(Value::as_str)
            == Some("message_annotations")
        && payload
            .get("content")
            .and_then(|content| content.get("annotations"))
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
}

fn trim_deque(deque: &mut VecDeque<ConsoleIdentityEventEnvelope>, cap: usize) {
    while deque.len() > cap {
        deque.pop_front();
    }
}

fn replay_slice(
    events: VecDeque<ConsoleIdentityEventEnvelope>,
    last_event_id: Option<&str>,
    stream: &str,
    latest_event_id: Option<String>,
) -> Result<Vec<ConsoleIdentityEventEnvelope>, ReplayUnavailableError> {
    let Some(last_event_id) = last_event_id.filter(|value| !value.trim().is_empty()) else {
        // Fresh connection (no Last-Event-ID): return all buffered events
        // so new subscribers see the conversation so far.
        return Ok(events.into_iter().collect());
    };
    let mut replay = events.into_iter().collect::<Vec<_>>();
    let Some(start_idx) = replay
        .iter()
        .position(|event| event.event_id == last_event_id)
    else {
        return Err(ReplayUnavailableError {
            error: "replay_unavailable".to_string(),
            stream: stream.to_string(),
            requested_last_event_id: last_event_id.to_string(),
            latest_event_id: latest_event_id.unwrap_or_default(),
        });
    };
    // Inclusive replay: include the checkpoint event so clients can
    // verify continuity and deduplicate by event_id.
    Ok(replay.split_off(start_idx))
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

/// Strip a runtime-generation suffix from a runtime id, returning the
/// durable identity. Real agent events from meerkat-mob 0.6 use
/// `{identity}:{N}` (`AgentRuntimeId`'s Display form).
///
/// Identities themselves often contain colons (e.g. `personal:alice@x.com`),
/// so we only strip the LAST colon-delimited segment and only when that
/// segment parses as a generation suffix. If the format changes, this
/// returns `None` and the caller must fall back to explicit
/// runtime-to-identity registration instead of guessing.
fn derive_identity_from_runtime_id(runtime_id: &str) -> Option<String> {
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replay_all_retains_latest_4096_events() {
        let store = ConsoleEventStore::new();
        for idx in 0..(ALL_EVENTS_REPLAY_CAP + 8) {
            store
                .append(
                    "identity:luka",
                    Some("turn-1".to_string()),
                    "text_delta",
                    json!({ "idx": idx }),
                )
                .await;
        }

        let replay = store
            .replay_all(None)
            .await
            .expect("all-events replay should succeed");
        assert_eq!(replay.len(), ALL_EVENTS_REPLAY_CAP);
        assert_eq!(
            replay.first().and_then(|event| event.data["idx"].as_u64()),
            Some(8)
        );
    }
}
