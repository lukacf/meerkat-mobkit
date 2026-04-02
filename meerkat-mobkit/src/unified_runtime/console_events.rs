use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast};

use crate::console_contracts::{ConsoleIdentityEventEnvelope, ReplayUnavailableError};
use crate::types::{EventEnvelope, UnifiedEvent};
use crate::unified_runtime::EventQuery;

const IDENTITY_REPLAY_CAP: usize = 1024;
const ALL_EVENTS_REPLAY_CAP: usize = 4096;
const EVENT_CHANNEL_CAP: usize = 512;

#[derive(Clone)]
pub(crate) struct ConsoleEventStore {
    next_event_seq: Arc<AtomicU64>,
    state: Arc<RwLock<ConsoleEventReplayState>>,
    event_tx: broadcast::Sender<ConsoleIdentityEventEnvelope>,
}

struct ConsoleEventReplayState {
    all_events: VecDeque<ConsoleIdentityEventEnvelope>,
    by_identity: BTreeMap<String, VecDeque<ConsoleIdentityEventEnvelope>>,
    all_stream_checkpoint: Option<String>,
    identity_stream_checkpoints: BTreeMap<String, String>,
    pending_by_identity: BTreeMap<String, VecDeque<PendingInteraction>>,
    runtime_to_identity: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct PendingInteraction {
    interaction_id: String,
    origin: String,
    content: String,
}

impl ConsoleEventStore {
    pub(crate) fn new() -> Self {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAP);
        let bootstrap = ConsoleIdentityEventEnvelope {
            event_id: "console-evt-1".to_string(),
            interaction_id: None,
            identity: "identity:system".to_string(),
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
                all_stream_checkpoint: None,
                identity_stream_checkpoints: BTreeMap::new(),
                pending_by_identity: BTreeMap::new(),
                runtime_to_identity: BTreeMap::new(),
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

    pub(crate) async fn replay_identity(
        &self,
        identity: &str,
        last_event_id: Option<&str>,
    ) -> Result<Vec<ConsoleIdentityEventEnvelope>, ReplayUnavailableError> {
        let state = self.state.read().await;
        if last_event_id.is_some_and(|value| {
            state
                .identity_stream_checkpoints
                .get(identity)
                .map(|checkpoint| checkpoint == value)
                .unwrap_or(false)
        }) {
            return Ok(state
                .by_identity
                .get(identity)
                .cloned()
                .unwrap_or_else(VecDeque::new)
                .into_iter()
                .collect());
        }
        let events = state
            .by_identity
            .get(identity)
            .cloned()
            .unwrap_or_else(VecDeque::new);
        replay_slice(
            events,
            last_event_id,
            "identity",
            state.all_events.back().map(|event| event.event_id.clone()),
        )
    }

    pub(crate) async fn replay_all(
        &self,
        last_event_id: Option<&str>,
    ) -> Result<Vec<ConsoleIdentityEventEnvelope>, ReplayUnavailableError> {
        let state = self.state.read().await;
        if last_event_id.is_some_and(|value| {
            state
                .all_stream_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint == value)
                .unwrap_or(false)
        }) {
            return Ok(state.all_events.iter().cloned().collect());
        }
        replay_slice(
            state.all_events.clone(),
            last_event_id,
            "all_events",
            state.all_events.back().map(|event| event.event_id.clone()),
        )
    }

    pub(crate) async fn note_identity_stream_checkpoint(&self, identity: &str, checkpoint: String) {
        let mut state = self.state.write().await;
        state
            .identity_stream_checkpoints
            .insert(identity.to_string(), checkpoint);
    }

    pub(crate) async fn note_all_stream_checkpoint(&self, checkpoint: String) {
        let mut state = self.state.write().await;
        state.all_stream_checkpoint = Some(checkpoint);
    }

    pub(crate) async fn query(&self, query: &EventQuery) -> Vec<ConsoleIdentityEventEnvelope> {
        let state = self.state.read().await;
        let mut events = match query.identity.as_deref() {
            Some(identity) => state
                .by_identity
                .get(identity)
                .cloned()
                .unwrap_or_else(VecDeque::new)
                .into_iter()
                .collect::<Vec<_>>(),
            None => state.all_events.iter().cloned().collect::<Vec<_>>(),
        };
        if let Some(since_ms) = query.since_ms {
            events.retain(|event| event.timestamp_ms >= since_ms);
        }
        if let Some(until_ms) = query.until_ms {
            events.retain(|event| event.timestamp_ms < until_ms);
        }
        if !query.event_types.is_empty() {
            events.retain(|event| query.event_types.iter().any(|ty| ty == &event.event_type));
        }
        if let Some(limit) = query.limit
            && events.len() > limit
        {
            let start = events.len().saturating_sub(limit);
            events = events.split_off(start);
        }
        events
    }

    pub(crate) async fn record_interaction(
        &self,
        identity: &str,
        runtime_member_id: Option<&str>,
        interaction_id: &str,
        origin: &str,
        content: &str,
    ) {
        {
            let mut state = self.state.write().await;
            state
                .pending_by_identity
                .entry(identity.to_string())
                .or_default()
                .push_back(PendingInteraction {
                    interaction_id: interaction_id.to_string(),
                    origin: origin.to_string(),
                    content: content.to_string(),
                });
            if let Some(runtime_member_id) =
                runtime_member_id.filter(|value| !value.trim().is_empty())
            {
                state
                    .runtime_to_identity
                    .insert(runtime_member_id.to_string(), identity.to_string());
            }
        }
        self.append(
            identity,
            Some(interaction_id.to_string()),
            "interaction_started",
            json!({
                "status": "accepted",
                "origin": origin,
                "content": content,
            }),
        )
        .await;
    }

    pub(crate) async fn record_lifecycle(&self, identity: &str, event_type: &str, data: Value) {
        let failed = {
            let mut state = self.state.write().await;
            let pending = state
                .pending_by_identity
                .remove(identity)
                .unwrap_or_default();
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

    pub(crate) async fn fail_interaction(
        &self,
        identity: &str,
        interaction_id: &str,
        reason: &str,
        data: Value,
    ) {
        {
            let mut state = self.state.write().await;
            if let Some(queue) = state.pending_by_identity.get_mut(identity) {
                queue.retain(|pending| pending.interaction_id != interaction_id);
                if queue.is_empty() {
                    state.pending_by_identity.remove(identity);
                }
            }
        }
        self.append(
            identity,
            Some(interaction_id.to_string()),
            "interaction_failed",
            json!({
                "reason": reason,
                "detail": data,
            }),
        )
        .await;
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

        let (identity, interaction_id) = {
            let mut state = self.state.write().await;
            let identity = state
                .runtime_to_identity
                .get(agent_id)
                .cloned()
                .or_else(|| derive_identity_from_runtime_id(agent_id));
            let Some(identity) = identity else {
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

        let Some(interaction_id) = interaction_id else {
            return;
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
            interaction_id: Some(interaction_id.clone()),
            identity: identity.clone(),
            event_type: projected_type.to_string(),
            timestamp_ms: event.timestamp_ms,
            data: projected_data,
        })
        .await;

        if matches!(event_type.as_str(), "run_completed" | "run_failed") {
            let mut state = self.state.write().await;
            if let Some(queue) = state.pending_by_identity.get_mut(&identity)
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
    let mut replay = events.into_iter().collect::<Vec<_>>();
    let Some(last_event_id) = last_event_id.filter(|value| !value.trim().is_empty()) else {
        return Ok(replay);
    };
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
    Ok(replay.split_off(start_idx))
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn derive_identity_from_runtime_id(runtime_id: &str) -> Option<String> {
    let (identity, generation_suffix) = runtime_id.rsplit_once(":gen")?;
    if generation_suffix.is_empty() || !generation_suffix.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(identity.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EventEnvelope;

    #[tokio::test]
    async fn replay_keeps_identity_windows_and_rejects_unknown_checkpoints() {
        let store = ConsoleEventStore::new();
        let first = store
            .append(
                "identity:luka",
                Some("turn-1".to_string()),
                "interaction_started",
                json!({}),
            )
            .await;
        let second = store
            .append(
                "identity:luka",
                Some("turn-1".to_string()),
                "interaction_complete",
                json!({}),
            )
            .await;

        let replay = store
            .replay_identity("identity:luka", Some(&first.event_id))
            .await
            .expect("known checkpoint");
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].event_id, first.event_id);
        assert_eq!(replay[1].event_id, second.event_id);

        let err = store
            .replay_identity("identity:luka", Some("evt-too-old"))
            .await
            .expect_err("unknown checkpoint");
        assert_eq!(err.error, "replay_unavailable");
        assert_eq!(err.stream, "identity");
        assert_eq!(err.latest_event_id, second.event_id);
    }

    #[tokio::test]
    async fn reconnecting_from_subscribed_checkpoint_replays_current_window() {
        let store = ConsoleEventStore::new();
        store
            .note_identity_stream_checkpoint(
                "identity:luka",
                "console-stream-identity-1".to_string(),
            )
            .await;
        let event = store
            .append(
                "identity:luka",
                Some("turn-1".to_string()),
                "interaction_started",
                json!({ "origin": "console:panel-1" }),
            )
            .await;

        let replay = store
            .replay_identity("identity:luka", Some("console-stream-identity-1"))
            .await
            .expect("subscribed checkpoint should replay retained events");
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].event_id, event.event_id);
    }

    #[tokio::test]
    async fn projected_agent_events_follow_pending_interaction_until_terminal() {
        let store = ConsoleEventStore::new();
        store
            .record_interaction(
                "identity:luka",
                Some("identity:luka:gen0"),
                "turn-1",
                "console:panel-1",
                "hello",
            )
            .await;
        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-agent-1".to_string(),
                source: "agent".to_string(),
                timestamp_ms: 10,
                event: UnifiedEvent::Agent {
                    agent_id: "identity:luka:gen0".to_string(),
                    event_type: "text_delta".to_string(),
                    payload: Some(json!({ "delta": "hi" })),
                },
            })
            .await;
        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-agent-2".to_string(),
                source: "agent".to_string(),
                timestamp_ms: 11,
                event: UnifiedEvent::Agent {
                    agent_id: "identity:luka:gen0".to_string(),
                    event_type: "run_completed".to_string(),
                    payload: Some(json!({ "text": "done" })),
                },
            })
            .await;

        let events = store
            .query(&EventQuery {
                identity: Some("identity:luka".to_string()),
                ..EventQuery::default()
            })
            .await;
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, "interaction_started");
        assert_eq!(events[0].interaction_id.as_deref(), Some("turn-1"));
        assert_eq!(events[1].event_type, "text_delta");
        assert_eq!(events[1].interaction_id.as_deref(), Some("turn-1"));
        assert_eq!(events[2].event_type, "interaction_complete");
        assert_eq!(events[2].event_id, "evt-agent-2");
        assert_eq!(events[2].data["source_event_type"], json!("run_completed"));
    }

    #[tokio::test]
    async fn lifecycle_mutation_fails_pending_interactions() {
        let store = ConsoleEventStore::new();
        store
            .record_interaction(
                "identity:luka",
                Some("identity:luka:gen0"),
                "turn-1",
                "console:panel-1",
                "hello",
            )
            .await;
        store
            .record_lifecycle(
                "identity:luka",
                "identity_retired",
                json!({ "reason": "operator_request" }),
            )
            .await;

        let events = store
            .query(&EventQuery {
                identity: Some("identity:luka".to_string()),
                ..EventQuery::default()
            })
            .await;
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, "interaction_started");
        assert_eq!(events[1].event_type, "interaction_failed");
        assert_eq!(events[1].data["lifecycle_event"], json!("identity_retired"));
        assert_eq!(events[2].event_type, "identity_retired");
    }
}
