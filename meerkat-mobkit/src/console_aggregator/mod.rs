mod state;
mod store;
mod types;

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use meerkat_core::ContentInput;
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::runtime::MobMemberListEntry;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::mob_handle_runtime::{
    MobRuntime, assert_member_accepts_images, send_message_on_mob_with_mode,
};
use crate::unified_runtime::{ConsoleEventStore, UnifiedRuntime};

pub use state::{
    ReplaySubscriptionEffect, ReplaySubscriptionState, ReplaySubscriptionTransition, SendEffect,
    SendState, SendTransition, SourceIngestionEffect, SourceIngestionState,
    SourceIngestionTransition,
};
pub use store::{
    ConsoleLogError, ConsoleLogResult, ConsoleLogStore, InMemoryConsoleLogStore,
    SqliteConsoleLogStore,
};
pub use types::{
    AppendDisposition, AppendOutcome, ConsoleCursor, ConsoleFrame, ConsoleFrameSource,
    ConsoleFrameSourceKind, ConsoleFrameStatus, ConsoleIdentityInspection, ConsoleIdentityRecord,
    ConsoleInteractionAccepted, ConsoleReplayUnavailable, ConsoleSendRequest, ConsoleTimelineEvent,
    ConsoleTimelinePage, ConsoleTimelineQuery, ConsoleVisibility, NewConsoleFrame,
};

const TIMELINE_CHANNEL_CAP: usize = 1024;

#[derive(Clone)]
pub struct MobKitConsoleAggregator {
    inner: Arc<AggregatorInner>,
}

struct AggregatorInner {
    store: Arc<dyn ConsoleLogStore>,
    runtimes: RwLock<BTreeMap<String, RuntimeEntry>>,
    event_tx: broadcast::Sender<ConsoleTimelineEvent>,
}

#[derive(Clone)]
struct RuntimeEntry {
    runtime_key: String,
    identity_namespace: String,
    runtime: MobRuntime,
    visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
}

pub trait ConsoleVisibilityPolicy: Send + Sync {
    fn identity_visible(&self, _record: &ConsoleIdentityRecord) -> bool {
        true
    }

    fn redact_payload(&self, _frame: &NewConsoleFrame) -> Option<Value> {
        None
    }
}

#[derive(Debug, Default)]
pub struct AllowAllConsoleVisibilityPolicy;

impl ConsoleVisibilityPolicy for AllowAllConsoleVisibilityPolicy {}

#[derive(Clone)]
pub struct ConsoleRuntimeRegistration {
    pub runtime_key: String,
    pub runtime: Arc<UnifiedRuntime>,
    pub identity_namespace: String,
    pub visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
}

impl MobKitConsoleAggregator {
    pub fn new(store: Arc<dyn ConsoleLogStore>) -> Self {
        let (event_tx, _) = broadcast::channel(TIMELINE_CHANNEL_CAP);
        Self {
            inner: Arc::new(AggregatorInner {
                store,
                runtimes: RwLock::new(BTreeMap::new()),
                event_tx,
            }),
        }
    }

    pub fn in_memory() -> Self {
        Self::new(Arc::new(InMemoryConsoleLogStore::new()))
    }

    pub(crate) fn single_runtime(
        runtime_key: impl Into<String>,
        runtime: MobRuntime,
        console_events: ConsoleEventStore,
    ) -> Self {
        let aggregator = Self::in_memory();
        aggregator.register_runtime_handles(runtime_key, "", runtime, console_events);
        aggregator
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConsoleTimelineEvent> {
        self.inner.event_tx.subscribe()
    }

    pub fn store(&self) -> Arc<dyn ConsoleLogStore> {
        self.inner.store.clone()
    }

    pub fn register_runtime(&self, registration: ConsoleRuntimeRegistration) {
        self.register_runtime_handles_with_policy(
            registration.runtime_key,
            registration.identity_namespace,
            registration.runtime.mob_runtime().clone(),
            registration.runtime.console_events(),
            registration.visibility_policy,
        );
    }

    pub(crate) fn register_runtime_handles(
        &self,
        runtime_key: impl Into<String>,
        identity_namespace: impl Into<String>,
        runtime: MobRuntime,
        console_events: ConsoleEventStore,
    ) {
        self.register_runtime_handles_with_policy(
            runtime_key,
            identity_namespace,
            runtime,
            console_events,
            Arc::new(AllowAllConsoleVisibilityPolicy),
        );
    }

    pub(crate) fn register_runtime_handles_with_policy(
        &self,
        runtime_key: impl Into<String>,
        identity_namespace: impl Into<String>,
        runtime: MobRuntime,
        console_events: ConsoleEventStore,
        visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    ) {
        let runtime_key = runtime_key.into();
        let identity_namespace = identity_namespace.into();
        let entry = RuntimeEntry {
            runtime_key: runtime_key.clone(),
            identity_namespace,
            runtime,
            visibility_policy,
        };
        if let Ok(mut runtimes) = self.inner.runtimes.write() {
            runtimes.insert(runtime_key.clone(), entry);
        }
        let inner = self.inner.clone();
        let events_for_backfill = console_events.clone();
        let runtime_key_for_task = runtime_key;
        tokio::spawn(async move {
            let mut ingestion_state = SourceIngestionState::Registered;
            if let Ok((next, _effects)) =
                ingestion_state.apply(SourceIngestionTransition::StartBackfill)
            {
                ingestion_state = next;
            }
            if let Ok(events) = events_for_backfill.replay_all(None).await {
                for envelope in events {
                    let _ = project_console_event(&inner, &runtime_key_for_task, envelope).await;
                }
            }
            if let Ok((next, _effects)) =
                ingestion_state.apply(SourceIngestionTransition::BackfillComplete)
            {
                ingestion_state = next;
            }
            let _ = ingestion_state.apply(SourceIngestionTransition::StartLive);

            let mut rx = console_events.subscribe();
            loop {
                match rx.recv().await {
                    Ok(envelope) => {
                        let _ =
                            project_console_event(&inner, &runtime_key_for_task, envelope).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    pub async fn list_identities(&self) -> ConsoleLogResult<Vec<ConsoleIdentityRecord>> {
        let entries = self
            .inner
            .runtimes
            .read()
            .map_err(|_| runtime_registry_lock_error())?
            .clone();
        let mut identities = Vec::new();
        for entry in entries.values() {
            let members = entry
                .runtime
                .handle()
                .list_members_including_retiring()
                .await;
            for member in members {
                if let Some(record) = identity_record_for_member(entry, &member).await
                    && entry.visibility_policy.identity_visible(&record)
                {
                    identities.push(record);
                }
            }
        }
        identities.sort_by(|left, right| left.identity.cmp(&right.identity));
        Ok(identities)
    }

    pub async fn inspect_identity(
        &self,
        identity: &str,
    ) -> ConsoleLogResult<Option<ConsoleIdentityInspection>> {
        let Some((entry, member, _raw_identity)) = self.resolve_member(identity).await else {
            return Ok(None);
        };
        let Some(record) = identity_record_for_member(&entry, &member).await else {
            return Ok(None);
        };
        if !entry.visibility_policy.identity_visible(&record) {
            return Ok(None);
        }
        let peers = member.wired_to.iter().map(ToString::to_string).collect();
        Ok(Some(ConsoleIdentityInspection {
            identity: record,
            peers,
        }))
    }

    pub async fn query_timeline(
        &self,
        query: ConsoleTimelineQuery,
    ) -> ConsoleLogResult<ConsoleTimelinePage> {
        self.inner.store.query_frames(query).await
    }

    pub async fn latest_cursor(&self) -> ConsoleLogResult<Option<ConsoleCursor>> {
        self.inner.store.latest_cursor().await
    }

    pub async fn send(
        &self,
        request: ConsoleSendRequest,
    ) -> Result<ConsoleInteractionAccepted, ConsoleSendError> {
        validate_send_request(&request)?;
        let Some((entry, member, runtime_identity)) = self.resolve_member(&request.identity).await
        else {
            return Err(ConsoleSendError::UnknownIdentity(request.identity));
        };
        let Some(record) = identity_record_for_member(&entry, &member).await else {
            return Err(ConsoleSendError::UnknownIdentity(request.identity));
        };
        if !entry.visibility_policy.identity_visible(&record) {
            return Err(ConsoleSendError::UnknownIdentity(request.identity));
        }
        if !member_is_addressable(&member) {
            return Err(ConsoleSendError::NotAddressable(request.identity));
        }
        if member.state == meerkat_mob::MemberState::Retiring {
            return Err(ConsoleSendError::Retired(request.identity));
        }

        let content = content_input_from_value(&request.content)?;
        assert_member_accepts_images(
            &entry.runtime.handle(),
            entry.runtime.session_service(),
            &runtime_identity,
            &content,
        )
        .await
        .map_err(|err| ConsoleSendError::InvalidContent(err.to_string()))?;

        let dedupe_key = send_dedupe_key(
            &entry.runtime_key,
            &request.identity,
            &request.origin,
            &request.idempotency_key,
        );
        if let Some(existing) = self
            .inner
            .store
            .frame_by_dedupe_key(&dedupe_key)
            .await
            .map_err(ConsoleSendError::Log)?
        {
            let same_origin = existing.payload.get("origin").and_then(Value::as_str)
                == Some(request.origin.as_str());
            let same_content = existing.payload.get("content") == Some(&request.content);
            if !same_origin || !same_content {
                return Err(ConsoleSendError::IdempotencyConflict(
                    request.idempotency_key,
                ));
            }
            return Ok(accepted_from_frame(&existing));
        }

        let interaction_id = format!("console-interaction-{}", hash_short(&dedupe_key));
        let session_id = entry
            .runtime
            .handle()
            .resolve_bridge_session_id(&MeerkatId::from(runtime_identity.as_str()))
            .await
            .map(|sid| sid.to_string());
        let new_frame = NewConsoleFrame {
            id: None,
            dedupe_key,
            timestamp_ms: current_time_ms(),
            runtime_key: entry.runtime_key.clone(),
            identity: request.identity.clone(),
            conversation_id: Some(request.identity.clone()),
            session_id: session_id.clone(),
            kind: "user_input".to_string(),
            status: ConsoleFrameStatus::Accepted,
            payload: json!({
                "content": request.content,
                "origin": request.origin,
                "idempotency_key": request.idempotency_key,
            }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::Send,
                source_cursor: None,
            },
            source_event_id: None,
            interaction_id: Some(interaction_id.clone()),
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        };

        let _ = SendState::Requested
            .apply(SendTransition::PersistAccepted)
            .map_err(ConsoleSendError::State)?;
        let outcome = self
            .inner
            .store
            .append_if_absent(new_frame)
            .await
            .map_err(ConsoleSendError::Log)?;
        if outcome.disposition == AppendDisposition::Inserted {
            let _ = self
                .inner
                .event_tx
                .send(ConsoleTimelineEvent::ConsoleFrame {
                    frame: outcome.frame.clone(),
                });
        }
        let accepted = accepted_from_frame(&outcome.frame);

        let (dispatching, _effects) = SendState::AcceptedPersisted
            .apply(SendTransition::StartDispatch)
            .map_err(ConsoleSendError::State)?;
        update_frame_status_and_emit(
            &self.inner,
            &outcome.frame.id,
            ConsoleFrameStatus::Dispatching,
        )
        .await
        .map_err(ConsoleSendError::Log)?;

        let handling_mode = parse_handling_mode(request.handling_mode.as_deref())?;
        match send_message_on_mob_with_mode(
            &entry.runtime.handle(),
            &runtime_identity,
            content,
            handling_mode,
        )
        .await
        {
            Ok(delivered_session_id) => {
                let _ = dispatching
                    .apply(SendTransition::MarkDelivered)
                    .map_err(ConsoleSendError::State)?;
                update_frame_status_and_emit(
                    &self.inner,
                    &outcome.frame.id,
                    ConsoleFrameStatus::Delivered,
                )
                .await
                .map_err(ConsoleSendError::Log)?;
                if accepted.session_id.is_none() && !delivered_session_id.is_empty() {
                    return Ok(ConsoleInteractionAccepted {
                        session_id: Some(delivered_session_id),
                        ..accepted
                    });
                }
                Ok(accepted)
            }
            Err(err) => {
                let _ = dispatching
                    .apply(SendTransition::MarkDeliveryFailed)
                    .map_err(ConsoleSendError::State)?;
                update_frame_status_and_emit(
                    &self.inner,
                    &outcome.frame.id,
                    ConsoleFrameStatus::DeliveryFailed,
                )
                .await
                .map_err(ConsoleSendError::Log)?;
                let failure_frame = NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("delivery-failed:{}", outcome.frame.id),
                    timestamp_ms: current_time_ms(),
                    runtime_key: entry.runtime_key,
                    identity: request.identity,
                    conversation_id: outcome.frame.conversation_id,
                    session_id: outcome.frame.session_id,
                    kind: "message_delivery_failed".to_string(),
                    status: ConsoleFrameStatus::DeliveryFailed,
                    payload: json!({ "reason": err.to_string() }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::Synthetic,
                        source_cursor: None,
                    },
                    source_event_id: None,
                    interaction_id: Some(interaction_id),
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: Some(outcome.frame.id.clone()),
                    caused_by_frame_id: Some(outcome.frame.id),
                };
                let _ = append_and_emit(&self.inner, failure_frame).await;
                Err(ConsoleSendError::Dispatch(err.to_string()))
            }
        }
    }

    async fn resolve_member(
        &self,
        identity: &str,
    ) -> Option<(RuntimeEntry, MobMemberListEntry, String)> {
        let entries = self.inner.runtimes.read().ok()?.clone();
        for entry in entries.values() {
            let raw_identity = strip_namespace(identity, &entry.identity_namespace)?;
            let mid = MeerkatId::from(raw_identity.as_str());
            let members = entry
                .runtime
                .handle()
                .list_members_including_retiring()
                .await;
            if let Some(member) = members
                .into_iter()
                .find(|candidate| candidate.agent_identity == mid)
            {
                return Some((entry.clone(), member, raw_identity));
            }
        }
        None
    }
}

#[derive(Debug)]
pub enum ConsoleSendError {
    UnknownIdentity(String),
    NotAddressable(String),
    Retired(String),
    InvalidContent(String),
    InvalidHandlingMode(String),
    InvalidRequest(String),
    IdempotencyConflict(String),
    State(&'static str),
    Dispatch(String),
    Log(ConsoleLogError),
}

impl std::fmt::Display for ConsoleSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownIdentity(identity) => write!(f, "unknown identity: {identity}"),
            Self::NotAddressable(identity) => write!(f, "not addressable: {identity}"),
            Self::Retired(identity) => write!(f, "identity retired: {identity}"),
            Self::InvalidContent(message) => write!(f, "invalid content: {message}"),
            Self::InvalidHandlingMode(mode) => write!(f, "invalid handling mode: {mode}"),
            Self::InvalidRequest(message) => write!(f, "invalid request: {message}"),
            Self::IdempotencyConflict(key) => write!(f, "idempotency key conflict: {key}"),
            Self::State(message) => write!(f, "console send state error: {message}"),
            Self::Dispatch(message) => write!(f, "dispatch failed: {message}"),
            Self::Log(err) => write!(f, "console log error: {err}"),
        }
    }
}

impl std::error::Error for ConsoleSendError {}

async fn project_console_event(
    inner: &AggregatorInner,
    runtime_key: &str,
    envelope: crate::console_contracts::ConsoleIdentityEventEnvelope,
) -> ConsoleLogResult<()> {
    let Some(entry) = inner
        .runtimes
        .read()
        .ok()
        .and_then(|entries| entries.get(runtime_key).cloned())
    else {
        return Ok(());
    };
    let mut frame = frame_from_console_event(&entry, envelope);
    if let Some(redacted) = entry.visibility_policy.redact_payload(&frame) {
        frame.payload = redacted;
        frame.status = ConsoleFrameStatus::Redacted;
    }
    let source_cursor = frame
        .source_event_id
        .clone()
        .unwrap_or_else(|| frame.dedupe_key.clone());
    append_and_emit(inner, frame).await?;
    inner
        .store
        .record_source_watermark(
            &entry.runtime_key,
            ConsoleFrameSourceKind::ConsoleEvent,
            &source_cursor,
        )
        .await
}

async fn append_and_emit(
    inner: &AggregatorInner,
    frame: NewConsoleFrame,
) -> ConsoleLogResult<AppendOutcome> {
    let outcome = inner.store.append_if_absent(frame).await?;
    if outcome.disposition == AppendDisposition::Inserted {
        let _ = inner.event_tx.send(ConsoleTimelineEvent::ConsoleFrame {
            frame: outcome.frame.clone(),
        });
    }
    Ok(outcome)
}

async fn update_frame_status_and_emit(
    inner: &AggregatorInner,
    frame_id: &str,
    status: ConsoleFrameStatus,
) -> ConsoleLogResult<Option<ConsoleFrame>> {
    let Some(updated) = inner.store.update_frame_status(frame_id, status).await? else {
        return Ok(None);
    };
    let update_marker = NewConsoleFrame {
        id: None,
        dedupe_key: format!("frame-update:{}:{}", updated.id, updated.frame_version),
        timestamp_ms: updated.updated_at_ms.unwrap_or_else(current_time_ms),
        runtime_key: updated.runtime_key.clone(),
        identity: updated.identity.clone(),
        conversation_id: updated.conversation_id.clone(),
        session_id: updated.session_id.clone(),
        kind: "frame_updated".to_string(),
        status: updated.status,
        payload: json!({ "frame": updated.clone() }),
        source: ConsoleFrameSource {
            kind: ConsoleFrameSourceKind::Synthetic,
            source_cursor: None,
        },
        source_event_id: None,
        interaction_id: updated.interaction_id.clone(),
        turn_id: updated.turn_id.clone(),
        run_id: updated.run_id.clone(),
        parent_frame_id: Some(updated.id.clone()),
        caused_by_frame_id: Some(updated.id.clone()),
    };
    let outcome = inner.store.append_if_absent(update_marker).await?;
    if outcome.disposition == AppendDisposition::Inserted {
        let _ = inner.event_tx.send(ConsoleTimelineEvent::ConsoleFrame {
            frame: outcome.frame,
        });
    }
    Ok(Some(updated))
}

fn frame_from_console_event(
    entry: &RuntimeEntry,
    envelope: crate::console_contracts::ConsoleIdentityEventEnvelope,
) -> NewConsoleFrame {
    let turn_id = envelope
        .data
        .get("turn_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let run_id = envelope
        .data
        .get("run_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let status = match envelope.event_type.as_str() {
        "interaction_started" => ConsoleFrameStatus::Accepted,
        "interaction_failed" | "run_failed" => ConsoleFrameStatus::DeliveryFailed,
        "interaction_complete" | "run_completed" => ConsoleFrameStatus::Completed,
        _ => ConsoleFrameStatus::Delivered,
    };
    let identity = apply_namespace(&envelope.identity, &entry.identity_namespace);
    NewConsoleFrame {
        id: Some(envelope.event_id.clone()),
        dedupe_key: format!("console-event:{}:{}", entry.runtime_key, envelope.event_id),
        timestamp_ms: envelope.timestamp_ms,
        runtime_key: entry.runtime_key.clone(),
        identity: identity.clone(),
        conversation_id: Some(identity),
        session_id: envelope
            .data
            .get("session_id")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        kind: envelope.event_type,
        status,
        payload: envelope.data,
        source: ConsoleFrameSource {
            kind: ConsoleFrameSourceKind::ConsoleEvent,
            source_cursor: None,
        },
        source_event_id: Some(envelope.event_id),
        interaction_id: envelope.interaction_id,
        turn_id,
        run_id,
        parent_frame_id: None,
        caused_by_frame_id: None,
    }
}

async fn identity_record_for_member(
    entry: &RuntimeEntry,
    member: &MobMemberListEntry,
) -> Option<ConsoleIdentityRecord> {
    let runtime_member_id = member.agent_identity.to_string();
    let identity = apply_namespace(&runtime_member_id, &entry.identity_namespace);
    let addressable = member_is_addressable(member);
    let visibility = if member.state == meerkat_mob::MemberState::Retiring {
        ConsoleVisibility::RetiredReadable
    } else if addressable {
        ConsoleVisibility::Addressable
    } else {
        ConsoleVisibility::Hidden
    };
    let session_id = entry
        .runtime
        .handle()
        .resolve_bridge_session_id(&member.agent_identity)
        .await
        .map(|sid| sid.to_string());
    let display_name = member
        .labels
        .get("display_name")
        .cloned()
        .unwrap_or_else(|| runtime_member_id.clone());
    Some(ConsoleIdentityRecord {
        identity,
        display_name,
        runtime_key: entry.runtime_key.clone(),
        runtime_member_id,
        session_id,
        visibility,
        addressable,
        health: if addressable {
            "ready"
        } else {
            "hidden_by_policy"
        }
        .to_string(),
        labels: member.labels.clone(),
    })
}

fn member_is_addressable(member: &MobMemberListEntry) -> bool {
    member
        .labels
        .get("addressable")
        .map(|value| !value.eq_ignore_ascii_case("false"))
        .unwrap_or(true)
}

fn apply_namespace(identity: &str, namespace: &str) -> String {
    let namespace = namespace.trim().trim_matches('/');
    if namespace.is_empty() {
        identity.to_string()
    } else {
        format!("{namespace}/{identity}")
    }
}

fn strip_namespace(identity: &str, namespace: &str) -> Option<String> {
    let namespace = namespace.trim().trim_matches('/');
    if namespace.is_empty() {
        return Some(identity.to_string());
    }
    identity
        .strip_prefix(namespace)
        .and_then(|rest| rest.strip_prefix('/'))
        .map(ToString::to_string)
}

fn validate_send_request(request: &ConsoleSendRequest) -> Result<(), ConsoleSendError> {
    if request.identity.trim().is_empty() {
        return Err(ConsoleSendError::InvalidRequest(
            "identity must be non-empty".to_string(),
        ));
    }
    if request.origin.trim().is_empty() {
        return Err(ConsoleSendError::InvalidRequest(
            "origin must be non-empty".to_string(),
        ));
    }
    if request.idempotency_key.trim().is_empty() {
        return Err(ConsoleSendError::InvalidRequest(
            "idempotency_key must be non-empty".to_string(),
        ));
    }
    Ok(())
}

fn content_input_from_value(value: &Value) -> Result<ContentInput, ConsoleSendError> {
    let content: ContentInput = serde_json::from_value(value.clone())
        .map_err(|err| ConsoleSendError::InvalidContent(err.to_string()))?;
    match &content {
        ContentInput::Text(text) if text.trim().is_empty() => Err(
            ConsoleSendError::InvalidContent("content must be non-empty".to_string()),
        ),
        ContentInput::Blocks(blocks) if blocks.is_empty() => Err(ConsoleSendError::InvalidContent(
            "content blocks must be non-empty".to_string(),
        )),
        _ => Ok(content),
    }
}

fn parse_handling_mode(
    value: Option<&str>,
) -> Result<meerkat_core::types::HandlingMode, ConsoleSendError> {
    match value.unwrap_or("queue") {
        "queue" => Ok(meerkat_core::types::HandlingMode::Queue),
        "steer" => Ok(meerkat_core::types::HandlingMode::Steer),
        other => Err(ConsoleSendError::InvalidHandlingMode(other.to_string())),
    }
}

fn accepted_from_frame(frame: &ConsoleFrame) -> ConsoleInteractionAccepted {
    ConsoleInteractionAccepted {
        interaction_id: frame
            .interaction_id
            .clone()
            .unwrap_or_else(|| format!("console-interaction-{}", hash_short(&frame.dedupe_key))),
        identity: frame.identity.clone(),
        conversation_id: frame.conversation_id.clone(),
        session_id: frame.session_id.clone(),
        input_frame_id: frame.id.clone(),
        cursor: frame.cursor.clone(),
        status: frame.status,
    }
}

fn send_dedupe_key(
    runtime_key: &str,
    identity: &str,
    origin: &str,
    idempotency_key: &str,
) -> String {
    format!("send:{runtime_key}:{identity}:{origin}:{idempotency_key}")
}

fn hash_short(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    to_hex(&digest[..8])
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn current_time_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    }
}

fn runtime_registry_lock_error() -> ConsoleLogError {
    Box::new(std::io::Error::other(
        "console runtime registry lock poisoned",
    ))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn query_timeline_reads_from_aggregate_store() {
        let aggregator = MobKitConsoleAggregator::in_memory();
        let frame = NewConsoleFrame {
            id: None,
            dedupe_key: "event-1".to_string(),
            timestamp_ms: 1,
            runtime_key: "runtime-a".to_string(),
            identity: "agent-a".to_string(),
            conversation_id: Some("agent-a".to_string()),
            session_id: None,
            kind: "text_delta".to_string(),
            status: ConsoleFrameStatus::Delivered,
            payload: json!({ "delta": "hello" }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::ConsoleEvent,
                source_cursor: None,
            },
            source_event_id: Some("event-1".to_string()),
            interaction_id: None,
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        };
        aggregator
            .store()
            .append_if_absent(frame)
            .await
            .expect("append frame");

        let page = aggregator
            .query_timeline(ConsoleTimelineQuery {
                identity: Some("agent-a".to_string()),
                limit: 10,
                ..ConsoleTimelineQuery::default()
            })
            .await
            .expect("query timeline");
        assert_eq!(page.frames.len(), 1);
        assert_eq!(page.frames[0].kind, "text_delta");
    }

    #[tokio::test]
    async fn status_updates_get_replayable_aggregate_cursors() {
        let aggregator = MobKitConsoleAggregator::in_memory();
        let frame = NewConsoleFrame {
            id: None,
            dedupe_key: "send-1".to_string(),
            timestamp_ms: 1,
            runtime_key: "runtime-a".to_string(),
            identity: "agent-a".to_string(),
            conversation_id: Some("agent-a".to_string()),
            session_id: Some("session-1".to_string()),
            kind: "user_input".to_string(),
            status: ConsoleFrameStatus::Accepted,
            payload: json!({ "content": "hello" }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::Send,
                source_cursor: None,
            },
            source_event_id: None,
            interaction_id: Some("interaction-1".to_string()),
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        };
        let inserted = aggregator
            .store()
            .append_if_absent(frame)
            .await
            .expect("append frame");

        update_frame_status_and_emit(
            &aggregator.inner,
            &inserted.frame.id,
            ConsoleFrameStatus::Delivered,
        )
        .await
        .expect("update status");

        let page = aggregator
            .query_timeline(ConsoleTimelineQuery {
                identity: Some("agent-a".to_string()),
                after: Some(inserted.frame.cursor.clone()),
                limit: 10,
                ..ConsoleTimelineQuery::default()
            })
            .await
            .expect("query timeline");
        assert_eq!(page.frames.len(), 1);
        assert_eq!(page.frames[0].kind, "frame_updated");
        assert_eq!(page.frames[0].parent_frame_id, Some(inserted.frame.id));
        assert_eq!(
            page.frames[0]
                .payload
                .get("frame")
                .and_then(|frame| frame.get("status"))
                .and_then(Value::as_str),
            Some("delivered")
        );
    }
}
