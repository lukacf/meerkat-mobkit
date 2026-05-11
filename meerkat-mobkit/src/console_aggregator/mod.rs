mod state;
mod store;
mod types;

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use meerkat_core::ContentInput;
use meerkat_mob::MobHandle;
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::runtime::MobMemberListEntry;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::blob_store::BinaryBlobStore;
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
    console_events: ConsoleEventStore,
    visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
}

#[derive(Clone)]
struct ResolvedConsoleMember {
    entry: RuntimeEntry,
    handle: MobHandle,
    member: MobMemberListEntry,
    runtime_identity: String,
}

pub trait ConsoleVisibilityPolicy: Send + Sync {
    fn identity_visible(&self, _record: &ConsoleIdentityRecord) -> bool {
        true
    }

    fn frame_visible(&self, _frame: &ConsoleFrame) -> bool {
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
            console_events: console_events.clone(),
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
            let mut rx = console_events.subscribe();
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
            let _ = backfill_session_history(&inner, &runtime_key_for_task).await;
            if let Ok((next, _effects)) =
                ingestion_state.apply(SourceIngestionTransition::BackfillComplete)
            {
                ingestion_state = next;
            }
            let _ = ingestion_state.apply(SourceIngestionTransition::StartLive);

            loop {
                match rx.recv().await {
                    Ok(envelope) => {
                        let _ =
                            project_console_event(&inner, &runtime_key_for_task, envelope).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let _ = recover_lagged_source_events(
                            &inner,
                            &runtime_key_for_task,
                            &events_for_backfill,
                        )
                        .await;
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
            for resolved in member_sources_for_entry(entry).await {
                if let Some(record) =
                    identity_record_for_member(entry, &resolved.handle, &resolved.member).await
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
        let Some(resolved) = self.resolve_member(identity).await else {
            return Ok(None);
        };
        let Some(record) =
            identity_record_for_member(&resolved.entry, &resolved.handle, &resolved.member).await
        else {
            return Ok(None);
        };
        if !resolved.entry.visibility_policy.identity_visible(&record) {
            return Ok(None);
        }
        let peers = resolved
            .member
            .wired_to
            .iter()
            .map(ToString::to_string)
            .collect();
        Ok(Some(ConsoleIdentityInspection {
            identity: record,
            peers,
        }))
    }

    pub async fn query_timeline(
        &self,
        query: ConsoleTimelineQuery,
    ) -> ConsoleLogResult<ConsoleTimelinePage> {
        let explicit_identity = query.identity.clone();
        self.refresh_session_history().await?;
        let mut page = self.inner.store.query_frames(query).await?;
        let mut visible_frames = Vec::with_capacity(page.frames.len());
        for frame in page.frames {
            let allow_historical_identity =
                explicit_identity.as_deref() == Some(frame.identity.as_str());
            if frame_is_visible(&self.inner, &frame, allow_historical_identity)
                .await
                .unwrap_or(false)
            {
                visible_frames.push(frame);
            }
        }
        page.frames = visible_frames;
        Ok(page)
    }

    pub async fn refresh_session_history(&self) -> ConsoleLogResult<()> {
        let runtime_keys = self
            .inner
            .runtimes
            .read()
            .map_err(|_| runtime_registry_lock_error())?
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for runtime_key in runtime_keys {
            backfill_session_history(&self.inner, &runtime_key).await?;
        }
        Ok(())
    }

    pub async fn latest_cursor(&self) -> ConsoleLogResult<Option<ConsoleCursor>> {
        self.inner.store.latest_cursor().await
    }

    pub async fn timeline_event_visible(&self, event: &ConsoleTimelineEvent) -> bool {
        match event {
            ConsoleTimelineEvent::ConsoleFrame { frame }
            | ConsoleTimelineEvent::FrameUpdated { frame } => {
                frame_is_visible(&self.inner, frame, false)
                    .await
                    .unwrap_or(false)
            }
            ConsoleTimelineEvent::SnapshotStarted { .. }
            | ConsoleTimelineEvent::SnapshotComplete { .. }
            | ConsoleTimelineEvent::ReplayUnavailable { .. } => true,
        }
    }

    pub async fn timeline_frame_visible_for_query(
        &self,
        frame: &ConsoleFrame,
        identity: Option<&str>,
    ) -> bool {
        let allow_historical_identity = identity == Some(frame.identity.as_str());
        frame_is_visible(&self.inner, frame, allow_historical_identity)
            .await
            .unwrap_or(false)
    }

    pub async fn send(
        &self,
        request: ConsoleSendRequest,
    ) -> Result<ConsoleInteractionAccepted, ConsoleSendError> {
        validate_send_request(&request)?;
        let Some(resolved) = self.resolve_member(&request.identity).await else {
            return Err(ConsoleSendError::UnknownIdentity(request.identity));
        };
        let Some(record) =
            identity_record_for_member(&resolved.entry, &resolved.handle, &resolved.member).await
        else {
            return Err(ConsoleSendError::UnknownIdentity(request.identity));
        };
        if !resolved.entry.visibility_policy.identity_visible(&record) {
            return Err(ConsoleSendError::UnknownIdentity(request.identity));
        }
        if !member_is_addressable(&resolved.member) {
            return Err(ConsoleSendError::NotAddressable(request.identity));
        }
        if resolved.member.state == meerkat_mob::MemberState::Retiring {
            return Err(ConsoleSendError::Retired(request.identity));
        }

        let content = content_input_from_value(&request.content)?;
        let handling_mode = parse_handling_mode(request.handling_mode.as_deref())?;
        assert_member_accepts_images(
            &resolved.handle,
            resolved.entry.runtime.session_service(),
            &resolved.runtime_identity,
            &content,
        )
        .await
        .map_err(|err| ConsoleSendError::InvalidContent(err.to_string()))?;

        let dedupe_key = send_dedupe_key(
            &resolved.entry.runtime_key,
            &request.identity,
            &request.origin,
            &request.idempotency_key,
        );
        let handling_mode_value = request
            .handling_mode
            .as_deref()
            .unwrap_or("queue")
            .to_string();
        let request_fingerprint =
            send_request_fingerprint(&request.origin, &request.content, &handling_mode_value);
        if let Some(existing) = self
            .inner
            .store
            .frame_by_dedupe_key(&dedupe_key)
            .await
            .map_err(ConsoleSendError::Log)?
        {
            let same_request = existing.source.source_cursor.as_deref()
                == Some(request_fingerprint.as_str())
                || existing.source.source_cursor.is_none()
                    && existing.payload.get("origin").and_then(Value::as_str)
                        == Some(request.origin.as_str())
                    && existing.payload.get("content") == Some(&request.content)
                    && existing
                        .payload
                        .get("handling_mode")
                        .and_then(Value::as_str)
                        == Some(handling_mode_value.as_str());
            if !same_request {
                return Err(ConsoleSendError::IdempotencyConflict(
                    request.idempotency_key,
                ));
            }
            return Ok(accepted_from_frame(&existing));
        }

        let interaction_id = format!("console-interaction-{}", hash_short(&dedupe_key));
        resolved
            .entry
            .console_events
            .reserve_interaction_value(
                &resolved.runtime_identity,
                Some(resolved.runtime_identity.as_str()),
                &interaction_id,
                &request.origin,
                request.content.clone(),
            )
            .await
            .map_err(ConsoleSendError::State)?;
        let session_id = resolved
            .handle
            .resolve_bridge_session_id(&MeerkatId::from(resolved.runtime_identity.as_str()))
            .await
            .map(|sid| sid.to_string());
        let mut new_frame = NewConsoleFrame {
            id: None,
            dedupe_key,
            timestamp_ms: current_time_ms(),
            runtime_key: resolved.entry.runtime_key.clone(),
            identity: request.identity.clone(),
            conversation_id: Some(request.identity.clone()),
            session_id: session_id.clone(),
            kind: "user_input".to_string(),
            status: ConsoleFrameStatus::Accepted,
            payload: json!({
                "content": request.content,
                "origin": request.origin,
                "idempotency_key": request.idempotency_key,
                "handling_mode": handling_mode_value,
            }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::Send,
                source_cursor: Some(request_fingerprint),
            },
            source_event_id: None,
            interaction_id: Some(interaction_id.clone()),
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        };
        if let Some(redacted) = resolved.entry.visibility_policy.redact_payload(&new_frame) {
            new_frame.payload = redacted;
            new_frame.status = ConsoleFrameStatus::Redacted;
        }

        let _ = SendState::Requested
            .apply(SendTransition::PersistAccepted)
            .map_err(ConsoleSendError::State)?;
        let outcome = self
            .inner
            .store
            .append_if_absent(new_frame)
            .await
            .map_err(ConsoleSendError::Log)?;
        if outcome.disposition == AppendDisposition::Existing {
            return Ok(accepted_from_frame(&outcome.frame));
        }
        let _ = self
            .inner
            .event_tx
            .send(ConsoleTimelineEvent::ConsoleFrame {
                frame: outcome.frame.clone(),
            });
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

        match dispatch_message_to_resolved_member(&resolved, content, handling_mode).await {
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
                    runtime_key: resolved.entry.runtime_key,
                    identity: request.identity,
                    conversation_id: outcome.frame.conversation_id,
                    session_id: outcome.frame.session_id,
                    kind: "message_delivery_failed".to_string(),
                    status: ConsoleFrameStatus::DeliveryFailed,
                    payload: json!({ "reason": err.clone() }),
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
                Ok(accepted)
            }
        }
    }

    pub async fn binary_blob_store_for_identity(
        &self,
        identity: &str,
    ) -> Result<Option<Arc<dyn BinaryBlobStore>>, ConsoleSendError> {
        if identity.trim().is_empty() {
            return Err(ConsoleSendError::InvalidRequest(
                "identity must be non-empty".to_string(),
            ));
        }
        let Some(resolved) = self.resolve_member(identity).await else {
            return Err(ConsoleSendError::UnknownIdentity(identity.to_string()));
        };
        let Some(record) =
            identity_record_for_member(&resolved.entry, &resolved.handle, &resolved.member).await
        else {
            return Err(ConsoleSendError::UnknownIdentity(identity.to_string()));
        };
        if !resolved.entry.visibility_policy.identity_visible(&record) {
            return Err(ConsoleSendError::UnknownIdentity(identity.to_string()));
        }
        if !member_is_addressable(&resolved.member) {
            return Err(ConsoleSendError::NotAddressable(identity.to_string()));
        }
        if resolved.member.state == meerkat_mob::MemberState::Retiring {
            return Err(ConsoleSendError::Retired(identity.to_string()));
        }
        Ok(resolved.entry.runtime.binary_blob_store())
    }

    pub fn binary_blob_stores(&self) -> Vec<Arc<dyn BinaryBlobStore>> {
        self.inner
            .runtimes
            .read()
            .map(|entries| {
                entries
                    .values()
                    .filter_map(|entry| entry.runtime.binary_blob_store())
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn resolve_member(&self, identity: &str) -> Option<ResolvedConsoleMember> {
        let entries = self.inner.runtimes.read().ok()?.clone();
        for entry in entries.values() {
            let raw_identity = strip_namespace(identity, &entry.identity_namespace)?;
            let mid = MeerkatId::from(raw_identity.as_str());
            if let Some(mut resolved) = member_sources_for_entry(entry)
                .await
                .into_iter()
                .find(|candidate| candidate.member.agent_identity == mid)
            {
                resolved.runtime_identity = raw_identity;
                return Some(resolved);
            }
        }
        None
    }
}

async fn member_sources_for_entry(entry: &RuntimeEntry) -> Vec<ResolvedConsoleMember> {
    let mut resolved = Vec::new();
    let primary_handle = entry.runtime.handle();
    let primary_mob_id = primary_handle.mob_id().to_string();
    for member in primary_handle.list_members_including_retiring().await {
        resolved.push(ResolvedConsoleMember {
            entry: entry.clone(),
            handle: primary_handle.clone(),
            runtime_identity: member.agent_identity.to_string(),
            member,
        });
    }

    let Some(state) = entry.runtime.agent_mob_mcp_state() else {
        return resolved;
    };
    for (mob_id, _state) in state.mob_list().await {
        if mob_id.as_str() == primary_mob_id {
            continue;
        }
        let Ok(handle) = state.handle_for(&mob_id).await else {
            continue;
        };
        for member in handle.list_members_including_retiring().await {
            resolved.push(ResolvedConsoleMember {
                entry: entry.clone(),
                handle: handle.clone(),
                runtime_identity: member.agent_identity.to_string(),
                member,
            });
        }
    }
    resolved
}

async fn dispatch_message_to_resolved_member(
    resolved: &ResolvedConsoleMember,
    content: ContentInput,
    handling_mode: meerkat_core::types::HandlingMode,
) -> Result<String, String> {
    let mid = MeerkatId::from(resolved.runtime_identity.as_str());
    match send_message_on_mob_with_mode(
        &resolved.handle,
        &resolved.runtime_identity,
        content.clone(),
        handling_mode,
    )
    .await
    {
        Ok(session_id) => Ok(session_id),
        Err(err) if err.to_string().contains("not externally addressable") => {
            let member = resolved
                .handle
                .member(&mid)
                .await
                .map_err(|err| err.to_string())?;
            let _receipt = member
                .internal_turn(content)
                .await
                .map_err(|err| err.to_string())?;
            resolved
                .handle
                .resolve_bridge_session_id(&mid)
                .await
                .map(|sid| sid.to_string())
                .ok_or_else(|| "member has no bridge session after internal turn".to_string())
        }
        Err(err) => Err(err.to_string()),
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

async fn backfill_session_history(
    inner: &AggregatorInner,
    runtime_key: &str,
) -> ConsoleLogResult<()> {
    const SESSION_HISTORY_PAGE_LIMIT: usize = 500;
    let Some(entry) = inner
        .runtimes
        .read()
        .ok()
        .and_then(|entries| entries.get(runtime_key).cloned())
    else {
        return Ok(());
    };
    let members = member_sources_for_entry(&entry).await;
    for resolved in members {
        let member = resolved.member;
        let Some(record) = identity_record_for_member(&entry, &resolved.handle, &member).await
        else {
            continue;
        };
        if !entry.visibility_policy.identity_visible(&record) {
            continue;
        }
        let Some(session_id) = record.session_id.clone() else {
            continue;
        };
        let watermark_runtime_key =
            session_history_watermark_runtime_key(&entry.runtime_key, &session_id);
        let mut offset = inner
            .store
            .source_watermark(
                &watermark_runtime_key,
                ConsoleFrameSourceKind::SessionHistory,
            )
            .await?
            .and_then(|watermark| parse_session_history_watermark(&watermark, &session_id))
            .unwrap_or(0);
        loop {
            let page = match entry
                .runtime
                .read_session_history(&session_id, offset, Some(SESSION_HISTORY_PAGE_LIMIT))
                .await
            {
                Ok(page) => page,
                Err(err) => {
                    append_backfill_gap(
                        inner,
                        &entry.runtime_key,
                        &record.identity,
                        err.to_string(),
                    )
                    .await?;
                    break;
                }
            };
            let page_value = match serde_json::to_value(page) {
                Ok(value) => value,
                Err(err) => {
                    append_backfill_gap(
                        inner,
                        &entry.runtime_key,
                        &record.identity,
                        err.to_string(),
                    )
                    .await?;
                    break;
                }
            };
            let base_offset = page_value
                .get("offset")
                .and_then(Value::as_u64)
                .unwrap_or(offset as u64) as usize;
            let Some(messages) = page_value.get("messages").and_then(Value::as_array) else {
                append_backfill_gap(
                    inner,
                    &entry.runtime_key,
                    &record.identity,
                    "session history page missing messages".to_string(),
                )
                .await?;
                break;
            };
            if messages.is_empty() {
                break;
            }
            for (idx, message) in messages.iter().enumerate() {
                let absolute_offset = base_offset + idx;
                let Some(mut frame) = frame_from_session_history_message(
                    &entry.runtime_key,
                    &record.identity,
                    &session_id,
                    absolute_offset,
                    message.clone(),
                ) else {
                    continue;
                };
                if history_frame_has_existing_counterpart(inner, &frame).await? {
                    continue;
                }
                if let Some(redacted) = entry.visibility_policy.redact_payload(&frame) {
                    frame.payload = redacted;
                    frame.status = ConsoleFrameStatus::Redacted;
                }
                append_and_emit(inner, frame).await?;
            }
            offset = base_offset + messages.len();
            inner
                .store
                .record_source_watermark(
                    &watermark_runtime_key,
                    ConsoleFrameSourceKind::SessionHistory,
                    &format!("{session_id}:{offset}"),
                )
                .await?;
            let has_more = page_value
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !has_more || messages.len() < SESSION_HISTORY_PAGE_LIMIT {
                break;
            }
        }
    }
    Ok(())
}

async fn recover_lagged_source_events(
    inner: &AggregatorInner,
    runtime_key: &str,
    console_events: &ConsoleEventStore,
) -> ConsoleLogResult<()> {
    let watermark = inner
        .store
        .source_watermark(runtime_key, ConsoleFrameSourceKind::ConsoleEvent)
        .await?;
    match console_events.replay_all(watermark.as_deref()).await {
        Ok(events) => {
            for envelope in events {
                project_console_event(inner, runtime_key, envelope).await?;
            }
        }
        Err(err) => {
            append_source_gap(
                inner,
                runtime_key,
                format!(
                    "{}:{}:{}",
                    err.error, err.stream, err.requested_last_event_id
                ),
            )
            .await?;
        }
    }
    Ok(())
}

async fn append_source_gap(
    inner: &AggregatorInner,
    runtime_key: &str,
    reason: String,
) -> ConsoleLogResult<()> {
    append_and_emit(
        inner,
        NewConsoleFrame {
            id: None,
            dedupe_key: format!("source-gap:{runtime_key}:{}", current_time_ms()),
            timestamp_ms: current_time_ms(),
            runtime_key: runtime_key.to_string(),
            identity: "__console__".to_string(),
            conversation_id: None,
            session_id: None,
            kind: "replay_unavailable".to_string(),
            status: ConsoleFrameStatus::DeliveryFailed,
            payload: json!({
                "reason": reason,
                "source_kind": "console_event",
            }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::Synthetic,
                source_cursor: None,
            },
            source_event_id: None,
            interaction_id: None,
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        },
    )
    .await?;
    let _ = inner
        .event_tx
        .send(ConsoleTimelineEvent::ReplayUnavailable {
            requested_cursor: format!("source-gap:{runtime_key}"),
            latest_cursor: inner.store.latest_cursor().await.ok().flatten(),
        });
    Ok(())
}

async fn append_backfill_gap(
    inner: &AggregatorInner,
    runtime_key: &str,
    identity: &str,
    reason: String,
) -> ConsoleLogResult<()> {
    append_and_emit(
        inner,
        NewConsoleFrame {
            id: None,
            dedupe_key: format!(
                "session-backfill-gap:{runtime_key}:{identity}:{}",
                current_time_ms()
            ),
            timestamp_ms: current_time_ms(),
            runtime_key: runtime_key.to_string(),
            identity: identity.to_string(),
            conversation_id: Some(identity.to_string()),
            session_id: None,
            kind: "replay_unavailable".to_string(),
            status: ConsoleFrameStatus::DeliveryFailed,
            payload: json!({
                "reason": reason,
                "source_kind": "session_history",
            }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::Synthetic,
                source_cursor: None,
            },
            source_event_id: None,
            interaction_id: None,
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        },
    )
    .await?;
    Ok(())
}

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

fn frame_from_session_history_message(
    runtime_key: &str,
    identity: &str,
    session_id: &str,
    offset: usize,
    message: Value,
) -> Option<NewConsoleFrame> {
    let payload_hash = hash_short(&serde_json::to_string(&message).unwrap_or_default());
    let role = message
        .get("role")
        .or_else(|| message.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("message");
    let kind = if role.contains("user") {
        "user_input"
    } else if role.contains("assistant") {
        "interaction_complete"
    } else {
        return None;
    };
    let timestamp_ms = history_timestamp_ms(&message).unwrap_or_else(current_time_ms);
    let payload = if kind == "interaction_complete" {
        let text = extract_history_text(&message);
        json!({
            "result": text,
            "text": text,
            "message": message,
            "source_event_type": "session_history",
            "type": "session_history",
        })
    } else if kind == "user_input" {
        json!({
            "content": extract_history_content(&message),
            "message": message,
        })
    } else {
        json!({ "message": message })
    };
    Some(NewConsoleFrame {
        id: None,
        dedupe_key: format!("session-history:{runtime_key}:{session_id}:{offset}:{payload_hash}"),
        timestamp_ms,
        runtime_key: runtime_key.to_string(),
        identity: identity.to_string(),
        conversation_id: Some(identity.to_string()),
        session_id: Some(session_id.to_string()),
        kind: kind.to_string(),
        status: ConsoleFrameStatus::Completed,
        payload,
        source: ConsoleFrameSource {
            kind: ConsoleFrameSourceKind::SessionHistory,
            source_cursor: Some(format!("{session_id}:{offset}")),
        },
        source_event_id: None,
        interaction_id: None,
        turn_id: None,
        run_id: None,
        parent_frame_id: None,
        caused_by_frame_id: None,
    })
}

fn history_timestamp_ms(message: &Value) -> Option<u64> {
    message
        .get("timestamp_ms")
        .or_else(|| message.get("created_at_ms"))
        .and_then(Value::as_u64)
}

fn extract_history_content(message: &Value) -> Value {
    message
        .get("content")
        .or_else(|| message.get("blocks"))
        .cloned()
        .unwrap_or_else(|| Value::String(extract_history_text(message)))
}

fn extract_history_text(message: &Value) -> String {
    if let Some(text) = message.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(content) = message.get("content") {
        if let Some(text) = content.as_str() {
            return text.to_string();
        }
        if let Some(blocks) = content.as_array() {
            return blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .or_else(|| block.get("content"))
                        .and_then(Value::as_str)
                })
                .collect::<Vec<_>>()
                .join("");
        }
    }
    if let Some(blocks) = message.get("blocks").and_then(Value::as_array) {
        return blocks
            .iter()
            .filter_map(|block| {
                block
                    .get("text")
                    .or_else(|| block.get("content"))
                    .or_else(|| block.get("data").and_then(|data| data.get("text")))
                    .or_else(|| block.get("data").and_then(|data| data.get("content")))
                    .and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join("");
    }
    String::new()
}

fn parse_session_history_watermark(watermark: &str, session_id: &str) -> Option<usize> {
    let (watermark_session_id, offset) = watermark.rsplit_once(':')?;
    if watermark_session_id != session_id {
        return None;
    }
    offset.parse().ok()
}

async fn history_frame_has_existing_counterpart(
    inner: &AggregatorInner,
    frame: &NewConsoleFrame,
) -> ConsoleLogResult<bool> {
    let fingerprint = transcript_fingerprint(&frame.kind, &frame.payload);
    if fingerprint.is_none() {
        return Ok(false);
    }
    let mut after = None;
    loop {
        let page = inner
            .store
            .query_frames(ConsoleTimelineQuery {
                identity: Some(frame.identity.clone()),
                conversation_id: frame.conversation_id.clone(),
                after,
                limit: 1_000,
            })
            .await?;
        if page.frames.iter().any(|existing| {
            let same_session = existing.session_id == frame.session_id
                || existing.session_id.is_none()
                || frame.session_id.is_none();
            existing.source.kind != ConsoleFrameSourceKind::SessionHistory
                && same_session
                && transcript_fingerprint(&existing.kind, &existing.payload) == fingerprint
        }) {
            return Ok(true);
        }
        if page.frames.is_empty() || page.next_cursor.is_none() {
            return Ok(false);
        }
        after = page.next_cursor;
    }
}

fn session_history_watermark_runtime_key(runtime_key: &str, session_id: &str) -> String {
    format!("{runtime_key}:session-history:{session_id}")
}

fn transcript_fingerprint(kind: &str, payload: &Value) -> Option<String> {
    match kind {
        "user_input" | "interaction_started" => payload
            .get("content")
            .map(stable_value_fingerprint)
            .or_else(|| payload.get("message").map(stable_value_fingerprint)),
        "text_complete" | "interaction_complete" | "run_completed" => payload
            .get("text")
            .or_else(|| payload.get("result"))
            .or_else(|| payload.get("content"))
            .map(stable_value_fingerprint),
        _ => None,
    }
}

fn stable_value_fingerprint(value: &Value) -> String {
    match value {
        Value::String(text) => normalize_transcript_fingerprint_text(text),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn normalize_transcript_fingerprint_text(text: &str) -> String {
    let trimmed = text.trim();
    trimmed
        .strip_prefix("[EVENT via rpc] ")
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

async fn frame_is_visible(
    inner: &AggregatorInner,
    frame: &ConsoleFrame,
    allow_historical_identity: bool,
) -> ConsoleLogResult<bool> {
    let entry = {
        let entries = inner
            .runtimes
            .read()
            .map_err(|_| runtime_registry_lock_error())?;
        if entries.is_empty() {
            return Ok(true);
        }
        let Some(entry) = entries.get(&frame.runtime_key) else {
            return Ok(false);
        };
        entry.clone()
    };
    if frame.identity != "__console__" {
        let runtime_member_id = strip_namespace(&frame.identity, &entry.identity_namespace)
            .unwrap_or_else(|| frame.identity.clone());
        let runtime_member = MeerkatId::from(runtime_member_id.as_str());
        let Some(resolved) = member_sources_for_entry(&entry)
            .await
            .into_iter()
            .find(|member| member.member.agent_identity == runtime_member)
        else {
            return Ok(allow_historical_identity && entry.visibility_policy.frame_visible(frame));
        };
        let Some(record) =
            identity_record_for_member(&entry, &resolved.handle, &resolved.member).await
        else {
            return Ok(false);
        };
        if !entry.visibility_policy.identity_visible(&record) {
            return Ok(false);
        }
    }
    Ok(entry.visibility_policy.frame_visible(frame))
}

async fn identity_record_for_member(
    entry: &RuntimeEntry,
    handle: &MobHandle,
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
    let session_id = handle
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

fn send_request_fingerprint(origin: &str, content: &Value, handling_mode: &str) -> String {
    let content_json = serde_json::to_string(content).unwrap_or_default();
    hash_short(&format!("{origin}\n{handling_mode}\n{content_json}"))
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

    #[tokio::test]
    async fn history_counterpart_scan_is_not_capped_to_one_page() {
        let aggregator = MobKitConsoleAggregator::in_memory();
        for idx in 0..1_005 {
            aggregator
                .store()
                .append_if_absent(NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("filler-{idx}"),
                    timestamp_ms: idx,
                    runtime_key: "runtime-a".to_string(),
                    identity: "agent-a".to_string(),
                    conversation_id: Some("agent-a".to_string()),
                    session_id: Some("session-a".to_string()),
                    kind: "text_delta".to_string(),
                    status: ConsoleFrameStatus::Completed,
                    payload: json!({ "delta": idx }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::ConsoleEvent,
                        source_cursor: None,
                    },
                    source_event_id: Some(format!("filler-{idx}")),
                    interaction_id: None,
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: None,
                    caused_by_frame_id: None,
                })
                .await
                .expect("append filler");
        }
        aggregator
            .store()
            .append_if_absent(NewConsoleFrame {
                id: None,
                dedupe_key: "live-user-input".to_string(),
                timestamp_ms: 2_000,
                runtime_key: "runtime-a".to_string(),
                identity: "agent-a".to_string(),
                conversation_id: Some("agent-a".to_string()),
                session_id: Some("session-a".to_string()),
                kind: "user_input".to_string(),
                status: ConsoleFrameStatus::Delivered,
                payload: json!({ "content": "already here" }),
                source: ConsoleFrameSource {
                    kind: ConsoleFrameSourceKind::ConsoleEvent,
                    source_cursor: None,
                },
                source_event_id: Some("live-user-input".to_string()),
                interaction_id: None,
                turn_id: None,
                run_id: None,
                parent_frame_id: None,
                caused_by_frame_id: None,
            })
            .await
            .expect("append live input");

        let history = NewConsoleFrame {
            id: None,
            dedupe_key: "history-user-input".to_string(),
            timestamp_ms: 3_000,
            runtime_key: "runtime-a".to_string(),
            identity: "agent-a".to_string(),
            conversation_id: Some("agent-a".to_string()),
            session_id: Some("session-a".to_string()),
            kind: "user_input".to_string(),
            status: ConsoleFrameStatus::Completed,
            payload: json!({ "content": "already here" }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::SessionHistory,
                source_cursor: Some("session-a:1006".to_string()),
            },
            source_event_id: None,
            interaction_id: None,
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        };

        assert!(
            history_frame_has_existing_counterpart(&aggregator.inner, &history)
                .await
                .expect("counterpart scan")
        );
    }

    #[tokio::test]
    async fn history_counterpart_scan_matches_rpc_wrapped_user_prompts() {
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator
            .store()
            .append_if_absent(NewConsoleFrame {
                id: None,
                dedupe_key: "live-user-input".to_string(),
                timestamp_ms: 2_000,
                runtime_key: "runtime-a".to_string(),
                identity: "agent-a".to_string(),
                conversation_id: Some("agent-a".to_string()),
                session_id: Some("session-a".to_string()),
                kind: "user_input".to_string(),
                status: ConsoleFrameStatus::Delivered,
                payload: json!({ "content": "hello from operator" }),
                source: ConsoleFrameSource {
                    kind: ConsoleFrameSourceKind::ConsoleEvent,
                    source_cursor: None,
                },
                source_event_id: Some("live-user-input".to_string()),
                interaction_id: None,
                turn_id: None,
                run_id: None,
                parent_frame_id: None,
                caused_by_frame_id: None,
            })
            .await
            .expect("append live input");

        let history = NewConsoleFrame {
            id: None,
            dedupe_key: "history-user-input".to_string(),
            timestamp_ms: 3_000,
            runtime_key: "runtime-a".to_string(),
            identity: "agent-a".to_string(),
            conversation_id: Some("agent-a".to_string()),
            session_id: Some("session-a".to_string()),
            kind: "user_input".to_string(),
            status: ConsoleFrameStatus::Completed,
            payload: json!({ "content": "[EVENT via rpc] hello from operator" }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::SessionHistory,
                source_cursor: Some("session-a:2".to_string()),
            },
            source_event_id: None,
            interaction_id: None,
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        };

        assert!(
            history_frame_has_existing_counterpart(&aggregator.inner, &history)
                .await
                .expect("counterpart scan")
        );
    }

    #[test]
    fn session_history_watermark_key_is_session_scoped() {
        assert_ne!(
            session_history_watermark_runtime_key("runtime-a", "session-1"),
            session_history_watermark_runtime_key("runtime-a", "session-2")
        );
    }

    #[test]
    fn session_history_messages_project_to_renderable_frames() {
        let user = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            0,
            json!({
                "role": "user",
                "content": "hello",
                "timestamp_ms": 10
            }),
        )
        .expect("user history frame");
        let assistant = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            1,
            json!({
                "role": "assistant",
                "content": [{ "type": "text", "text": "hi there" }],
                "timestamp_ms": 11
            }),
        )
        .expect("assistant history frame");

        assert_eq!(user.kind, "user_input");
        assert_eq!(user.source.kind, ConsoleFrameSourceKind::SessionHistory);
        assert_eq!(user.payload["content"], json!("hello"));
        assert_eq!(assistant.kind, "interaction_complete");
        assert_eq!(assistant.payload["text"], json!("hi there"));
        assert!(
            assistant
                .dedupe_key
                .starts_with("session-history:runtime-a:session-a:1:")
        );
    }

    #[test]
    fn session_history_projection_skips_non_transcript_messages() {
        let skipped = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            0,
            json!({
                "content": "internal system prompt"
            }),
        );
        assert!(skipped.is_none());
    }

    #[test]
    fn session_history_projection_extracts_assistant_blocks() {
        let frame = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            0,
            json!({
                "role": "assistant",
                "blocks": [
                    { "type": "text", "text": "hello " },
                    { "type": "text", "text": "there" }
                ]
            }),
        )
        .expect("assistant block history frame");
        assert_eq!(frame.payload["text"], json!("hello there"));
    }

    #[test]
    fn session_history_projection_extracts_nested_text_block_data() {
        let frame = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            0,
            json!({
                "role": "block_assistant",
                "blocks": [
                    {
                        "block_type": "text",
                        "data": { "text": "Ready and standing by." }
                    }
                ],
                "timestamp_ms": 10
            }),
        )
        .expect("assistant block history frame");

        assert_eq!(frame.kind, "interaction_complete");
        assert_eq!(frame.payload["result"], json!("Ready and standing by."));
    }
}
