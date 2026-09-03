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
const EVENT_CHANNEL_BASE_CAP: usize = 4096;
const EVENT_CHANNEL_CAP_PER_MEMBER: usize = 128;
const EVENT_CHANNEL_DEFAULT_MEMBER_BUDGET: usize = 256;
const EVENT_CHANNEL_MAX_CAP: usize = 65_536;
const PENDING_INTERACTION_CAP: usize = 256;

pub(crate) fn event_channel_capacity_for_members(member_count: usize) -> usize {
    EVENT_CHANNEL_BASE_CAP
        .saturating_add(member_count.saturating_mul(EVENT_CHANNEL_CAP_PER_MEMBER))
        .clamp(EVENT_CHANNEL_BASE_CAP, EVENT_CHANNEL_MAX_CAP)
}

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
    active_interaction_by_identity: BTreeMap<String, String>,
    runtime_to_identity: BTreeMap<String, String>,
    response_phase_by_identity: BTreeMap<String, Option<String>>,
    /// Console identity metadata registered by spawn paths (spawn labels,
    /// `spawned_by` lineage, `via_tool`). Merged into identity projections
    /// and access-control attribute priming; member/roster labels win on
    /// conflict.
    labels_by_identity: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone)]
struct PendingInteraction {
    interaction_id: String,
    origin: String,
    content: Value,
}

impl ConsoleEventReplayState {
    fn resolve_identity_for_runtime_event(&self, runtime_event_id: &str) -> Option<String> {
        if let Some(identity) = self.runtime_to_identity.get(runtime_event_id) {
            return Some(identity.clone());
        }

        self.runtime_to_identity
            .iter()
            .filter_map(|(runtime_member_id, identity)| {
                runtime_event_id
                    .strip_prefix(runtime_member_id)
                    .filter(|suffix| suffix.starts_with(':'))
                    .map(|_| (runtime_member_id.len(), identity.clone()))
            })
            .max_by_key(|(prefix_len, _)| *prefix_len)
            .map(|(_, identity)| identity)
    }
}

impl ConsoleEventStore {
    pub(crate) fn new() -> Self {
        Self::with_member_capacity(EVENT_CHANNEL_DEFAULT_MEMBER_BUDGET)
    }

    pub(crate) fn with_member_capacity(member_count: usize) -> Self {
        let (event_tx, _) = broadcast::channel(event_channel_capacity_for_members(member_count));
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
                active_interaction_by_identity: BTreeMap::new(),
                runtime_to_identity: BTreeMap::new(),
                response_phase_by_identity: BTreeMap::new(),
                labels_by_identity: BTreeMap::new(),
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

    pub(crate) async fn append_envelope(&self, envelope: ConsoleIdentityEventEnvelope) -> bool {
        {
            let mut state = self.state.write().await;
            if state
                .all_events
                .iter()
                .any(|existing| existing.event_id == envelope.event_id)
            {
                return false;
            }
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
        true
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

    /// Register a runtime-id → identity mapping only when no mapping exists
    /// for that key yet. Used by roster-refresh paths (reconcile) whose
    /// alias self-mapping is a fallback: it must never clobber the durable
    /// console identity that spawn/reserve paths registered for the same
    /// runtime member id.
    pub(crate) async fn register_runtime_identity_fallback(
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
            .entry(runtime_member_id)
            .or_insert(identity);
    }

    /// Register console identity metadata (spawn labels, lineage). Merges
    /// per key; the latest registration wins so a respawn can update labels.
    pub(crate) async fn register_identity_labels(
        &self,
        identity: impl Into<String>,
        labels: BTreeMap<String, String>,
    ) {
        let identity = identity.into();
        if identity.trim().is_empty() || labels.is_empty() {
            return;
        }
        let mut state = self.state.write().await;
        state
            .labels_by_identity
            .entry(identity)
            .or_default()
            .extend(labels);
    }

    /// Remove one registered console metadata key for an identity, e.g. a
    /// stale `spawned_by` after a respawn whose spawner is unknown.
    pub(crate) async fn unregister_identity_label(&self, identity: &str, key: &str) {
        let mut state = self.state.write().await;
        if let Some(labels) = state.labels_by_identity.get_mut(identity) {
            labels.remove(key);
            if labels.is_empty() {
                state.labels_by_identity.remove(identity);
            }
        }
    }

    /// Registered console metadata for one identity, if any.
    pub(crate) async fn identity_labels(&self, identity: &str) -> Option<BTreeMap<String, String>> {
        self.state
            .read()
            .await
            .labels_by_identity
            .get(identity)
            .cloned()
    }

    /// Snapshot of all registered console identity metadata.
    pub(crate) async fn identity_labels_snapshot(
        &self,
    ) -> BTreeMap<String, BTreeMap<String, String>> {
        self.state.read().await.labels_by_identity.clone()
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

    /// Terminalize one exact pending interaction without classifying the
    /// failure itself as an identity lifecycle mutation.
    pub(crate) async fn record_interaction_failure(
        &self,
        identity: &str,
        interaction_id: &str,
        data: Value,
    ) {
        {
            let mut state = self.state.write().await;
            close_console_interaction(&mut state, identity, interaction_id);
        }
        self.append(
            identity,
            Some(interaction_id.to_string()),
            "interaction_failed",
            data,
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
        if is_empty_web_search_annotations_event(event_type, payload.as_ref()) {
            return;
        }

        let mut projected_data = payload.clone().unwrap_or_else(|| json!({}));
        if let Some(object) = projected_data.as_object_mut() {
            object
                .entry("source_event_type".to_string())
                .or_insert_with(|| Value::String(event_type.clone()));
        }

        let (identity, interaction_id, superseded_pending) = {
            let mut state = self.state.write().await;
            let registered_identity = state.resolve_identity_for_runtime_event(agent_id);
            let identity = registered_identity
                .clone()
                .or_else(|| derive_identity_from_runtime_id(agent_id));
            let Some(identity) = identity else {
                tracing::warn!(
                    agent_id = %agent_id,
                    event_type = %event_type,
                    "dropping agent event: runtime id did not resolve to a registered identity"
                );
                return;
            };
            // Cache only registration-backed resolutions. A heuristic
            // derive guess must stay repairable: caching it would make the
            // exact-match lookup permanently shadow a later
            // `register_runtime_identity` (spawn/reserve/reconcile), so a
            // single early event would strand every later interaction.
            if registered_identity.is_some() {
                state
                    .runtime_to_identity
                    .entry(agent_id.clone())
                    .or_insert_with(|| identity.clone());
            }
            let (interaction_id, superseded_pending) = match event_type.as_str() {
                "run_started" => {
                    select_interaction_for_run_started(&mut state, &identity, &projected_data)
                }
                "interaction_complete" | "interaction_failed" | "interaction_callback_pending" => (
                    select_interaction_for_directed_terminal(&state, &identity, &projected_data),
                    Vec::new(),
                ),
                _ => (
                    state
                        .active_interaction_by_identity
                        .get(&identity)
                        .cloned()
                        .or_else(|| {
                            state
                                .pending_by_identity
                                .get(&identity)
                                .and_then(|queue| queue.front())
                                .map(|pending| pending.interaction_id.clone())
                        }),
                    Vec::new(),
                ),
            };
            (identity, interaction_id, superseded_pending)
        };
        for pending in superseded_pending {
            self.append(
                identity.clone(),
                Some(pending.interaction_id),
                "interaction_failed",
                json!({
                    "reason": "superseded_by_later_run",
                    "origin": pending.origin,
                    "content": pending.content,
                }),
            )
            .await;
        }

        let projected_type = match event_type.as_str() {
            "run_completed" => "interaction_complete",
            "run_failed" => "interaction_failed",
            other => other,
        };

        let inserted = self
            .append_envelope(ConsoleIdentityEventEnvelope {
                event_id: event.event_id.clone(),
                interaction_id: interaction_id.clone(),
                identity: identity.clone(),
                event_type: projected_type.to_string(),
                timestamp_ms: event.timestamp_ms,
                data: projected_data.clone(),
            })
            .await;
        if !inserted {
            return;
        }

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

        let terminal_turn_completed = is_terminal_turn_completed_event(event_type, &projected_data);
        {
            let mut state = self.state.write().await;
            match event_type.as_str() {
                "tool_call_requested" | "tool_call" | "tool_result_received" => {
                    state
                        .response_phase_by_identity
                        .insert(identity.clone(), Some("tool-executing".to_string()));
                }
                "text_delta" | "reasoning_delta" => {
                    state
                        .response_phase_by_identity
                        .insert(identity.clone(), Some("generating".to_string()));
                }
                "run_completed" | "run_failed" => {
                    state.active_interaction_by_identity.remove(&identity);
                    state
                        .response_phase_by_identity
                        .insert(identity.clone(), None);
                }
                "interaction_complete" | "interaction_failed" | "interaction_callback_pending" => {
                    if let Some(interaction_id) = interaction_id.as_deref() {
                        close_console_interaction(&mut state, &identity, interaction_id);
                    }
                }
                "turn_completed" if terminal_turn_completed => {
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
                && let Some(position) = queue
                    .iter()
                    .position(|pending| pending.interaction_id == interaction_id)
            {
                queue.remove(position);
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

fn select_interaction_for_run_started(
    state: &mut ConsoleEventReplayState,
    identity: &str,
    payload: &Value,
) -> (Option<String>, Vec<PendingInteraction>) {
    let Some(queue) = state.pending_by_identity.get_mut(identity) else {
        return (None, Vec::new());
    };

    let matched_position = queue
        .iter()
        .position(|pending| pending_matches_run_started(pending, payload))
        .unwrap_or(0);

    let mut superseded = Vec::new();
    for _ in 0..matched_position {
        if let Some(pending) = queue.pop_front() {
            superseded.push(pending);
        }
    }

    let interaction_id = queue.front().map(|pending| pending.interaction_id.clone());
    if let Some(interaction_id) = &interaction_id {
        state
            .active_interaction_by_identity
            .insert(identity.to_string(), interaction_id.clone());
    }
    (interaction_id, superseded)
}

/// Runtime-minted interaction terminals (`interaction_complete`,
/// `interaction_failed`, `interaction_callback_pending`) name the interaction
/// they close in `payload.interaction_id`. They belong to a console
/// interaction only when that id is one the console reserved for this
/// identity; peer, flow-step and schedule inputs mint their own ids, and their
/// terminals must not close whichever console send happens to be pending.
fn select_interaction_for_directed_terminal(
    state: &ConsoleEventReplayState,
    identity: &str,
    payload: &Value,
) -> Option<String> {
    let directed = payload.get("interaction_id").and_then(Value::as_str)?;
    let reserved = state
        .active_interaction_by_identity
        .get(identity)
        .is_some_and(|active| active == directed)
        || state
            .pending_by_identity
            .get(identity)
            .is_some_and(|queue| {
                queue
                    .iter()
                    .any(|pending| pending.interaction_id == directed)
            });
    reserved.then(|| directed.to_string())
}

/// Drop one console interaction from the identity's pending queue and, when
/// it is the active one, from the active slot: the identity is idle once the
/// interaction it was serving reached a terminal.
fn close_console_interaction(
    state: &mut ConsoleEventReplayState,
    identity: &str,
    interaction_id: &str,
) {
    if let Some(queue) = state.pending_by_identity.get_mut(identity) {
        if let Some(position) = queue
            .iter()
            .position(|pending| pending.interaction_id == interaction_id)
        {
            queue.remove(position);
        }
        if queue.is_empty() {
            state.pending_by_identity.remove(identity);
        }
    }
    if state
        .active_interaction_by_identity
        .get(identity)
        .is_some_and(|active| active == interaction_id)
    {
        state.active_interaction_by_identity.remove(identity);
    }
    state
        .response_phase_by_identity
        .insert(identity.to_string(), None);
}

/// meerkat's `RunStarted` carries `input: RunInput`, serialized as
/// `{"kind": "content", "content": <ContentInput>}` (a string or a block
/// array) or `{"kind": "pending_tool_results"}`, which has no prompt at all.
/// There is no `prompt` field; reading one never matched, so every
/// `run_started` bound to the queue front regardless of which send it
/// actually started.
fn pending_matches_run_started(pending: &PendingInteraction, payload: &Value) -> bool {
    let Some(prompt) = run_started_prompt_text(payload) else {
        return false;
    };
    content_value_matches_text(&pending.content, &prompt)
}

fn run_started_prompt_text(payload: &Value) -> Option<String> {
    match payload.get("input")?.get("content")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let combined = blocks
                .iter()
                .filter_map(text_from_content_block)
                .collect::<String>();
            (!combined.is_empty()).then_some(combined)
        }
        _ => None,
    }
}

fn content_value_matches_text(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(text) => text == expected,
        Value::Array(items) => {
            let combined = items
                .iter()
                .filter_map(text_from_content_block)
                .collect::<String>();
            !combined.is_empty() && combined == expected
        }
        Value::Object(map) => {
            map.get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| text == expected)
                || map
                    .get("content")
                    .is_some_and(|content| content_value_matches_text(content, expected))
                || map
                    .get("blocks")
                    .is_some_and(|blocks| content_value_matches_text(blocks, expected))
        }
        _ => false,
    }
}

fn text_from_content_block(value: &Value) -> Option<&str> {
    value
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| value.get("content").and_then(Value::as_str))
}

fn is_terminal_turn_completed_event(event_type: &str, payload: &Value) -> bool {
    if event_type != "turn_completed" {
        return false;
    }
    let stop_reason = payload
        .get("stop_reason")
        .or_else(|| payload.get("stopReason"))
        .and_then(Value::as_str);
    !matches!(stop_reason, Some("tool_use"))
}

fn parse_generate_image_tool_result(
    payload: &Value,
) -> Option<meerkat_core::image_generation::ImageGenerationToolResult> {
    if payload.get("name").and_then(Value::as_str) != Some("generate_image") {
        return None;
    }
    if let Some(result_value) = payload.get("result") {
        return if let Some(result_text) = result_value.as_str() {
            serde_json::from_str(result_text).ok()
        } else {
            serde_json::from_value(result_value.clone()).ok()
        };
    }
    // meerkat 0.7 removed the flat `result` key from ToolExecutionCompleted;
    // mobkit's ingest edges re-derive it, but fall back to the typed
    // `content` blocks so raw 0.7-shaped payloads still project image frames.
    let text = payload
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(text_from_content_block)
        .collect::<String>();
    serde_json::from_str(&text).ok()
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

    #[tokio::test]
    async fn terminal_turn_completed_clears_phase_without_stealing_run_correlation() {
        let store = ConsoleEventStore::new();
        store
            .register_runtime_identity("rt:worker:1", "worker")
            .await;
        store
            .reserve_interaction_value(
                "worker",
                Some("rt:worker:1"),
                "turn-1",
                "console",
                json!({}),
            )
            .await
            .expect("reserve first interaction");
        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-1".to_string(),
                source: "test".to_string(),
                timestamp_ms: 1,
                event: UnifiedEvent::Agent {
                    agent_id: "rt:worker:1".to_string(),
                    event_type: "text_delta".to_string(),
                    payload: Some(json!({ "delta": "working" })),
                },
            })
            .await;
        assert_eq!(
            store.response_phase_for_identity("worker").await.as_deref(),
            Some("generating")
        );

        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-2".to_string(),
                source: "test".to_string(),
                timestamp_ms: 2,
                event: UnifiedEvent::Agent {
                    agent_id: "rt:worker:1".to_string(),
                    event_type: "turn_completed".to_string(),
                    payload: Some(json!({ "stop_reason": "max_tokens" })),
                },
            })
            .await;
        assert_eq!(store.response_phase_for_identity("worker").await, None);

        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-3".to_string(),
                source: "test".to_string(),
                timestamp_ms: 3,
                event: UnifiedEvent::Agent {
                    agent_id: "rt:worker:1".to_string(),
                    event_type: "run_completed".to_string(),
                    payload: Some(json!({ "result": "done" })),
                },
            })
            .await;

        let replay = store
            .replay_all(None)
            .await
            .expect("all-events replay should succeed");
        let run_completed = replay
            .iter()
            .find(|event| event.event_id == "evt-3")
            .expect("run completion should be replayed");
        assert_eq!(run_completed.interaction_id.as_deref(), Some("turn-1"));
    }

    #[tokio::test]
    async fn duplicate_unified_event_ids_are_projected_once() {
        let store = ConsoleEventStore::new();
        store
            .register_runtime_identity("rt:worker:1", "worker")
            .await;
        store
            .reserve_interaction_value(
                "worker",
                Some("rt:worker:1"),
                "turn-1",
                "console",
                json!({}),
            )
            .await
            .expect("reserve interaction");

        let event = EventEnvelope {
            event_id: "evt-duplicate".to_string(),
            source: "test".to_string(),
            timestamp_ms: 1,
            event: UnifiedEvent::Agent {
                agent_id: "rt:worker:1".to_string(),
                event_type: "run_completed".to_string(),
                payload: Some(json!({ "result": "done" })),
            },
        };
        store.project_unified_event(&event).await;
        store.project_unified_event(&event).await;

        let replay = store
            .replay_all(None)
            .await
            .expect("all-events replay should succeed");
        let projected = replay
            .iter()
            .filter(|event| event.event_id == "evt-duplicate")
            .collect::<Vec<_>>();
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].interaction_id.as_deref(), Some("turn-1"));
    }

    #[tokio::test]
    async fn exact_delivery_failure_preserves_error_and_other_pending_interactions() {
        let store = ConsoleEventStore::new();
        for interaction_id in ["turn-1", "turn-2"] {
            store
                .reserve_interaction_value(
                    "worker",
                    Some("rt:worker:1"),
                    interaction_id,
                    "console",
                    json!({ "interaction": interaction_id }),
                )
                .await
                .expect("reserve interaction");
        }

        store
            .record_interaction_failure(
                "worker",
                "turn-1",
                json!({ "error": "typed delivery failure" }),
            )
            .await;

        let replay = store
            .replay_all(None)
            .await
            .expect("all-events replay should succeed");
        let failure = replay
            .iter()
            .find(|event| event.interaction_id.as_deref() == Some("turn-1"))
            .expect("failed interaction should be projected");
        assert_eq!(failure.event_type, "interaction_failed");
        assert_eq!(failure.data["error"], "typed delivery failure");
        assert!(
            store
                .state
                .read()
                .await
                .pending_by_identity
                .get("worker")
                .is_some_and(|pending| pending
                    .iter()
                    .any(|entry| entry.interaction_id == "turn-2")),
            "an unrelated queued interaction must remain pending"
        );
    }

    #[tokio::test]
    async fn run_started_matches_pending_prompt_and_fails_superseded_input() {
        let store = ConsoleEventStore::new();
        store
            .register_runtime_identity("rt:worker:1", "worker")
            .await;
        store
            .reserve_interaction_value(
                "worker",
                Some("rt:worker:1"),
                "stale-turn",
                "console",
                json!("debug probe that was superseded"),
            )
            .await
            .expect("reserve stale interaction");
        store
            .reserve_interaction_value(
                "worker",
                Some("rt:worker:1"),
                "real-turn",
                "console",
                json!("reply with this exact token"),
            )
            .await
            .expect("reserve real interaction");

        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-run-started".to_string(),
                source: "test".to_string(),
                timestamp_ms: 1,
                event: UnifiedEvent::Agent {
                    agent_id: "rt:worker:1".to_string(),
                    event_type: "run_started".to_string(),
                    payload: Some(json!({
                        "session_id": "session-1",
                        "input": { "kind": "content", "content": "reply with this exact token" }
                    })),
                },
            })
            .await;
        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-run-completed".to_string(),
                source: "test".to_string(),
                timestamp_ms: 2,
                event: UnifiedEvent::Agent {
                    agent_id: "rt:worker:1".to_string(),
                    event_type: "run_completed".to_string(),
                    payload: Some(json!({ "result": "reply with this exact token" })),
                },
            })
            .await;

        let replay = store
            .replay_all(None)
            .await
            .expect("all-events replay should succeed");
        let stale_failed = replay
            .iter()
            .find(|event| event.interaction_id.as_deref() == Some("stale-turn"))
            .expect("stale interaction should be failed");
        assert_eq!(stale_failed.event_type, "interaction_failed");
        assert_eq!(stale_failed.data["reason"], "superseded_by_later_run");

        let run_started = replay
            .iter()
            .find(|event| event.event_id == "evt-run-started")
            .expect("run_started should be replayed");
        assert_eq!(run_started.interaction_id.as_deref(), Some("real-turn"));
        let run_completed = replay
            .iter()
            .find(|event| event.event_id == "evt-run-completed")
            .expect("run completion should be replayed");
        assert_eq!(run_completed.interaction_id.as_deref(), Some("real-turn"));
    }

    #[tokio::test]
    async fn runtime_event_child_id_uses_registered_identity_alias() {
        let store = ConsoleEventStore::new();
        store
            .register_runtime_identity("rt:review:singleton:0", "review:singleton")
            .await;
        store
            .reserve_interaction_value(
                "review:singleton",
                Some("rt:review:singleton:0"),
                "turn-1",
                "console",
                json!({}),
            )
            .await
            .expect("reserve interaction");

        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-1".to_string(),
                source: "test".to_string(),
                timestamp_ms: 1,
                event: UnifiedEvent::Agent {
                    agent_id: "rt:review:singleton:0:0".to_string(),
                    event_type: "text_delta".to_string(),
                    payload: Some(json!({ "delta": "ok" })),
                },
            })
            .await;

        let replay = store
            .replay_all(None)
            .await
            .expect("all-events replay should succeed");
        let projected = replay
            .iter()
            .find(|event| event.event_id == "evt-1")
            .expect("child runtime event should project");
        assert_eq!(projected.identity, "review:singleton");
        assert_eq!(projected.interaction_id.as_deref(), Some("turn-1"));
        assert_eq!(
            store
                .response_phase_for_identity("review:singleton")
                .await
                .as_deref(),
            Some("generating")
        );
    }

    #[tokio::test]
    async fn tool_use_turn_completed_keeps_phase_and_pending_interaction() {
        let store = ConsoleEventStore::new();
        store
            .register_runtime_identity("rt:worker:1", "worker")
            .await;
        store
            .reserve_interaction_value(
                "worker",
                Some("rt:worker:1"),
                "turn-1",
                "console",
                json!({}),
            )
            .await
            .expect("reserve interaction");
        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-1".to_string(),
                source: "test".to_string(),
                timestamp_ms: 1,
                event: UnifiedEvent::Agent {
                    agent_id: "rt:worker:1".to_string(),
                    event_type: "tool_call".to_string(),
                    payload: Some(json!({ "name": "inspect" })),
                },
            })
            .await;
        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-2".to_string(),
                source: "test".to_string(),
                timestamp_ms: 2,
                event: UnifiedEvent::Agent {
                    agent_id: "rt:worker:1".to_string(),
                    event_type: "turn_completed".to_string(),
                    payload: Some(json!({ "stop_reason": "tool_use" })),
                },
            })
            .await;
        assert_eq!(
            store.response_phase_for_identity("worker").await.as_deref(),
            Some("tool-executing")
        );

        store
            .project_unified_event(&EventEnvelope {
                event_id: "evt-3".to_string(),
                source: "test".to_string(),
                timestamp_ms: 3,
                event: UnifiedEvent::Agent {
                    agent_id: "rt:worker:1".to_string(),
                    event_type: "text_delta".to_string(),
                    payload: Some(json!({ "delta": "after tool" })),
                },
            })
            .await;
        let replay = store
            .replay_all(None)
            .await
            .expect("all-events replay should succeed");
        let after_tool_delta = replay
            .iter()
            .find(|event| event.event_id == "evt-3")
            .expect("after-tool delta should be replayed");
        assert_eq!(after_tool_delta.interaction_id.as_deref(), Some("turn-1"));
    }

    fn agent_event(
        event_id: &str,
        agent_id: &str,
        event_type: &str,
    ) -> EventEnvelope<UnifiedEvent> {
        agent_event_with_payload(event_id, agent_id, event_type, json!({}))
    }

    fn agent_event_with_payload(
        event_id: &str,
        agent_id: &str,
        event_type: &str,
        payload: Value,
    ) -> EventEnvelope<UnifiedEvent> {
        EventEnvelope {
            event_id: event_id.to_string(),
            source: "test".to_string(),
            timestamp_ms: 1,
            event: UnifiedEvent::Agent {
                agent_id: agent_id.to_string(),
                event_type: event_type.to_string(),
                payload: Some(payload),
            },
        }
    }

    /// Regression: an agent event that arrives before any registration must
    /// not poison the runtime-id cache. The derive-based guess used to be
    /// cached via `or_insert`, so the exact-match lookup permanently shadowed
    /// the later `register_runtime_identity` and reserved interactions never
    /// completed.
    #[tokio::test]
    async fn early_event_does_not_poison_cache_against_later_registration() {
        let store = ConsoleEventStore::new();

        // Event lands before any spawn/reserve/reconcile registration.
        store
            .project_unified_event(&agent_event(
                "evt-early",
                "rt:review:singleton:0:1",
                "text_delta",
            ))
            .await;

        // Identity-bridge send path: reserve registers the runtime member id
        // against the durable identity.
        store
            .register_runtime_identity("rt:review:singleton:0", "review:singleton")
            .await;
        store
            .reserve_interaction_value(
                "review:singleton",
                Some("rt:review:singleton:0"),
                "turn-1",
                "mobkit/send_message",
                json!({}),
            )
            .await
            .expect("reserve interaction");

        store
            .project_unified_event(&agent_event(
                "evt-complete",
                "rt:review:singleton:0:1",
                "run_completed",
            ))
            .await;

        let replay = store
            .replay_all(None)
            .await
            .expect("all-events replay should succeed");
        let completion = replay
            .iter()
            .find(|event| event.event_id == "evt-complete")
            .expect("completion event should project");
        assert_eq!(
            completion.identity, "review:singleton",
            "completion must project under the registered durable identity"
        );
        assert_eq!(completion.event_type, "interaction_complete");
        assert_eq!(completion.interaction_id.as_deref(), Some("turn-1"));
    }

    /// Regression: the reconcile roster refresh registers the alias
    /// self-mapping as a fallback only — it must never clobber the durable
    /// identity that an identity-bridge reserve registered for the same
    /// runtime member id.
    #[tokio::test]
    async fn fallback_registration_never_clobbers_durable_identity() {
        let store = ConsoleEventStore::new();
        store
            .register_runtime_identity("rt:review:singleton:0", "review:singleton")
            .await;
        // Reconcile tick after the reserve.
        store
            .register_runtime_identity_fallback("rt:review:singleton:0", "rt:review:singleton:0")
            .await;
        store
            .reserve_interaction_value(
                "review:singleton",
                Some("rt:review:singleton:0"),
                "turn-1",
                "mobkit/send_message",
                json!({}),
            )
            .await
            .expect("reserve interaction");

        store
            .project_unified_event(&agent_event(
                "evt-complete",
                "rt:review:singleton:0:1",
                "run_completed",
            ))
            .await;

        let replay = store
            .replay_all(None)
            .await
            .expect("all-events replay should succeed");
        let completion = replay
            .iter()
            .find(|event| event.event_id == "evt-complete")
            .expect("completion event should project");
        assert_eq!(completion.identity, "review:singleton");
        assert_eq!(completion.interaction_id.as_deref(), Some("turn-1"));
    }

    #[test]
    fn event_channel_capacity_scales_with_member_count() {
        assert_eq!(event_channel_capacity_for_members(0), 4096);
        assert!(
            event_channel_capacity_for_members(136) >= 136 * 128,
            "large mobs need broadcast headroom for startup bursts"
        );
        assert_eq!(event_channel_capacity_for_members(10_000), 65_536);
    }

    /// meerkat's `RunStarted` carries `input: RunInput`, never a `prompt`.
    #[test]
    fn run_started_matcher_reads_the_typed_run_input() {
        let pending = PendingInteraction {
            interaction_id: "turn-1".to_string(),
            origin: "console".to_string(),
            content: json!("reply with this exact token"),
        };
        assert!(pending_matches_run_started(
            &pending,
            &json!({
                "session_id": "s-1",
                "input": { "kind": "content", "content": "reply with this exact token" }
            })
        ));
        assert!(pending_matches_run_started(
            &pending,
            &json!({
                "session_id": "s-1",
                "input": {
                    "kind": "content",
                    "content": [ { "type": "text", "text": "reply with this exact token" } ]
                }
            })
        ));
        assert!(!pending_matches_run_started(
            &pending,
            &json!({
                "session_id": "s-1",
                "input": { "kind": "content", "content": "something else" }
            })
        ));
        assert!(!pending_matches_run_started(
            &pending,
            &json!({ "session_id": "s-1", "input": { "kind": "pending_tool_results" } })
        ));
    }

    /// A runtime-minted terminal for an interaction the console never
    /// reserved (peer, flow-step and schedule inputs mint their own ids) must
    /// not close the console's pending send; one that names the reserved id
    /// is attributed to it and closes it.
    #[tokio::test]
    async fn directed_terminals_attribute_only_to_the_interaction_they_name() {
        let store = ConsoleEventStore::new();
        store
            .register_runtime_identity("rt:worker:1", "worker")
            .await;
        store
            .reserve_interaction_value(
                "worker",
                Some("rt:worker:1"),
                "console-turn",
                "console",
                json!("hello"),
            )
            .await
            .expect("reserve console interaction");

        store
            .project_unified_event(&agent_event_with_payload(
                "evt-foreign-complete",
                "rt:worker:1",
                "interaction_complete",
                json!({ "interaction_id": "peer-directed-turn", "result": "" }),
            ))
            .await;
        store
            .project_unified_event(&agent_event_with_payload(
                "evt-delta-while-pending",
                "rt:worker:1",
                "text_delta",
                json!({ "delta": "hi" }),
            ))
            .await;
        store
            .project_unified_event(&agent_event_with_payload(
                "evt-own-complete",
                "rt:worker:1",
                "interaction_complete",
                json!({ "interaction_id": "console-turn", "result": "done" }),
            ))
            .await;
        assert_eq!(
            store.response_phase_for_identity("worker").await,
            None,
            "the identity is idle once its own terminal projected"
        );
        store
            .project_unified_event(&agent_event_with_payload(
                "evt-delta-after-close",
                "rt:worker:1",
                "text_delta",
                json!({ "delta": "later" }),
            ))
            .await;

        let replay = store
            .replay_all(None)
            .await
            .expect("all-events replay should succeed");
        let by_id = |event_id: &str| {
            replay
                .iter()
                .find(|event| event.event_id == event_id)
                .expect(event_id)
        };
        let foreign = by_id("evt-foreign-complete");
        assert_eq!(foreign.event_type, "interaction_complete");
        assert_eq!(
            foreign.interaction_id, None,
            "a terminal naming another interaction must project unattributed"
        );
        assert_eq!(
            by_id("evt-delta-while-pending").interaction_id.as_deref(),
            Some("console-turn"),
            "the console send is still pending after a foreign terminal"
        );
        assert_eq!(
            by_id("evt-own-complete").interaction_id.as_deref(),
            Some("console-turn"),
            "a terminal naming the reserved interaction is attributed to it"
        );
        assert_eq!(
            by_id("evt-delta-after-close").interaction_id,
            None,
            "the console interaction is closed once its own terminal projected"
        );
    }
}
