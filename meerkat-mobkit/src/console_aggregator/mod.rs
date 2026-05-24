mod state;
mod store;
mod types;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::future::join_all;
use meerkat_core::types::HandlingMode;
use meerkat_core::{ContentInput, Message};
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::runtime::MobMemberListEntry;
use meerkat_mob::{MobError, MobHandle};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{Semaphore, broadcast, oneshot};

use crate::blob_store::BinaryBlobStore;
use crate::console_contracts::SYSTEM_EVENT_IDENTITY;
use crate::mob_handle_runtime::{
    MobRuntime, MobRuntimeError, assert_member_accepts_images, send_message_on_mob_with_mode,
};
use crate::runtime::ConsoleMember;
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
const SESSION_HISTORY_PAGE_LIMIT: usize = 500;
const SESSION_HISTORY_REFRESH_TTL_MS: u64 = 30_000;
const SESSION_HISTORY_GROWING_REFRESH_TTL_MS: u64 = 2_000;
const SESSION_HISTORY_DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);
const EXPLICIT_IDENTITY_BACKFILL_WAIT: Duration = Duration::from_millis(750);
const IDENTITY_FIRST_LIVE_MEMBER_REFRESH_WAIT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentityCollectionMode {
    CachedOnly,
    IncludeLiveMembers,
}

#[derive(Clone)]
pub struct MobKitConsoleAggregator {
    inner: Arc<AggregatorInner>,
}

#[derive(Debug, Clone, Copy)]
pub struct ConsoleAggregatorOptions {
    pub session_history_backfill_enabled: bool,
    pub max_concurrent_session_backfills: usize,
}

impl Default for ConsoleAggregatorOptions {
    fn default() -> Self {
        Self {
            session_history_backfill_enabled: true,
            max_concurrent_session_backfills: 16,
        }
    }
}

struct AggregatorInner {
    store: Arc<dyn ConsoleLogStore>,
    runtimes: RwLock<BTreeMap<String, RuntimeEntry>>,
    event_tx: broadcast::Sender<ConsoleTimelineEvent>,
    active_session_backfills: tokio::sync::Mutex<BTreeSet<String>>,
    opportunistic_session_backfills: tokio::sync::Mutex<BTreeSet<String>>,
    session_backfill_permits: Arc<Semaphore>,
    identity_read_model: ConsoleIdentityReadModel,
    options: ConsoleAggregatorOptions,
}

#[derive(Clone)]
struct ConsoleIdentityReadModel {
    inner: Arc<tokio::sync::RwLock<Vec<ConsoleIdentityRecord>>>,
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
    primed: Arc<AtomicBool>,
}

impl Default for ConsoleIdentityReadModel {
    fn default() -> Self {
        Self {
            inner: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
            primed: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl ConsoleIdentityReadModel {
    async fn snapshot(
        &self,
        inner: Arc<AggregatorInner>,
    ) -> ConsoleLogResult<Vec<ConsoleIdentityRecord>> {
        if !self.primed.load(Ordering::Acquire) {
            self.prime_now(inner).await?;
        }
        Ok(self.inner.read().await.clone())
    }

    async fn current(&self) -> Vec<ConsoleIdentityRecord> {
        self.inner.read().await.clone()
    }

    async fn refresh_now_if_idle(
        &self,
        inner: Arc<AggregatorInner>,
        mode: IdentityCollectionMode,
    ) -> Option<ConsoleLogResult<Vec<ConsoleIdentityRecord>>> {
        let Ok(guard) = self.refresh_lock.clone().try_lock_owned() else {
            return None;
        };
        let result = collect_identity_records(&inner, mode).await;
        drop(guard);
        match result {
            Ok(identities) => {
                self.replace(identities.clone()).await;
                Some(Ok(identities))
            }
            Err(err) => Some(Err(err)),
        }
    }

    async fn prime_now(&self, inner: Arc<AggregatorInner>) -> ConsoleLogResult<()> {
        if self.primed.load(Ordering::Acquire) {
            return Ok(());
        }
        let _guard = self.refresh_lock.clone().lock_owned().await;
        if self.primed.load(Ordering::Acquire) {
            return Ok(());
        }
        let identities =
            collect_identity_records(&inner, IdentityCollectionMode::CachedOnly).await?;
        *self.inner.write().await = identities;
        self.primed.store(true, Ordering::Release);
        Ok(())
    }

    fn refresh_soon(&self, inner: Arc<AggregatorInner>) {
        let Ok(runtime_handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let Ok(guard) = self.refresh_lock.clone().try_lock_owned() else {
            return;
        };
        let read_model = self.clone();
        runtime_handle.spawn(async move {
            let _guard = guard;
            match collect_identity_records(&inner, IdentityCollectionMode::IncludeLiveMembers).await
            {
                Ok(identities) => {
                    *read_model.inner.write().await = identities;
                    read_model.primed.store(true, Ordering::Release);
                }
                Err(err) => {
                    tracing::warn!(error = %err, "console identity read-model refresh failed");
                }
            }
        });
    }

    async fn replace(&self, identities: Vec<ConsoleIdentityRecord>) {
        *self.inner.write().await = identities;
        self.primed.store(true, Ordering::Release);
    }
}

#[derive(Clone)]
struct RuntimeEntry {
    runtime_key: String,
    identity_namespace: String,
    runtime: MobRuntime,
    identity_runtime: Option<Arc<crate::identity_first::IdentityRuntime>>,
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
    fn include_implicit_delegate_members(&self) -> bool {
        true
    }

    fn member_visible(&self, _member: &ConsoleMember) -> bool {
        true
    }

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

#[derive(Debug, Default)]
pub struct HideImplicitDelegateMembersConsoleVisibilityPolicy;

impl ConsoleVisibilityPolicy for HideImplicitDelegateMembersConsoleVisibilityPolicy {
    fn include_implicit_delegate_members(&self) -> bool {
        false
    }

    fn member_visible(&self, member: &ConsoleMember) -> bool {
        !is_implicit_delegate_member(member.role.as_str(), &member.labels)
    }

    fn identity_visible(&self, record: &ConsoleIdentityRecord) -> bool {
        !is_implicit_delegate_member(
            record
                .labels
                .get("role")
                .map(String::as_str)
                .unwrap_or_default(),
            &record.labels,
        )
    }
}

#[derive(Clone)]
pub struct ConsoleRuntimeRegistration {
    pub runtime_key: String,
    pub runtime: Arc<UnifiedRuntime>,
    pub identity_namespace: String,
    pub visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
}

impl MobKitConsoleAggregator {
    pub fn new(store: Arc<dyn ConsoleLogStore>) -> Self {
        Self::new_with_options(store, ConsoleAggregatorOptions::default())
    }

    pub fn new_with_options(
        store: Arc<dyn ConsoleLogStore>,
        mut options: ConsoleAggregatorOptions,
    ) -> Self {
        options.max_concurrent_session_backfills = options.max_concurrent_session_backfills.max(1);
        let (event_tx, _) = broadcast::channel(TIMELINE_CHANNEL_CAP);
        Self {
            inner: Arc::new(AggregatorInner {
                store,
                runtimes: RwLock::new(BTreeMap::new()),
                event_tx,
                active_session_backfills: tokio::sync::Mutex::new(BTreeSet::new()),
                opportunistic_session_backfills: tokio::sync::Mutex::new(BTreeSet::new()),
                session_backfill_permits: Arc::new(Semaphore::new(
                    options.max_concurrent_session_backfills,
                )),
                identity_read_model: ConsoleIdentityReadModel::default(),
                options,
            }),
        }
    }

    pub fn in_memory() -> Self {
        Self::new(Arc::new(InMemoryConsoleLogStore::new()))
    }

    pub fn in_memory_with_options(options: ConsoleAggregatorOptions) -> Self {
        Self::new_with_options(Arc::new(InMemoryConsoleLogStore::new()), options)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConsoleTimelineEvent> {
        self.inner.event_tx.subscribe()
    }

    pub fn store(&self) -> Arc<dyn ConsoleLogStore> {
        self.inner.store.clone()
    }

    pub fn register_runtime(&self, registration: ConsoleRuntimeRegistration) {
        let identity_runtime = registration.runtime.identity_runtime().cloned();
        self.register_runtime_handles_with_policy(
            registration.runtime_key,
            registration.identity_namespace,
            registration.runtime.mob_runtime().clone(),
            identity_runtime,
            registration.runtime.console_events(),
            registration.visibility_policy,
        );
    }

    pub(crate) fn register_runtime_handles_with_policy(
        &self,
        runtime_key: impl Into<String>,
        identity_namespace: impl Into<String>,
        runtime: MobRuntime,
        identity_runtime: Option<Arc<crate::identity_first::IdentityRuntime>>,
        console_events: ConsoleEventStore,
        visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    ) {
        let runtime_key = runtime_key.into();
        let identity_namespace = identity_namespace.into();
        let entry = RuntimeEntry {
            runtime_key: runtime_key.clone(),
            identity_namespace,
            runtime,
            identity_runtime,
            console_events: console_events.clone(),
            visibility_policy,
        };
        if let Ok(mut runtimes) = self.inner.runtimes.write() {
            runtimes.insert(runtime_key.clone(), entry);
        }
        self.inner
            .identity_read_model
            .refresh_soon(self.inner.clone());
        let inner = self.inner.clone();
        let events_for_live = console_events.clone();
        let events_for_live_recovery = console_events.clone();
        let runtime_key_for_live = runtime_key.clone();
        tokio::spawn(async move {
            let mut rx = events_for_live.subscribe();
            loop {
                match rx.recv().await {
                    Ok(envelope) => {
                        let _ =
                            project_console_event(inner.clone(), &runtime_key_for_live, envelope)
                                .await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let _ = recover_lagged_source_events(
                            inner.clone(),
                            &runtime_key_for_live,
                            &events_for_live_recovery,
                        )
                        .await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let inner = self.inner.clone();
        let events_for_replay = console_events;
        let runtime_key_for_replay = runtime_key;
        tokio::spawn(async move {
            let mut ingestion_state = SourceIngestionState::Registered;
            if let Ok((next, _effects)) =
                ingestion_state.apply(SourceIngestionTransition::StartBackfill)
            {
                ingestion_state = next;
            }
            if let Ok(events) = events_for_replay.replay_all(None).await {
                for envelope in events {
                    let _ = project_console_event(inner.clone(), &runtime_key_for_replay, envelope)
                        .await;
                }
            }
            spawn_session_history_backfill(inner.clone(), runtime_key_for_replay.clone());
            spawn_session_history_discovery_loop(inner.clone(), runtime_key_for_replay.clone());
            if let Ok((next, _effects)) =
                ingestion_state.apply(SourceIngestionTransition::BackfillComplete)
            {
                ingestion_state = next;
            }
            let _ = ingestion_state.apply(SourceIngestionTransition::StartLive);
        });
    }

    pub async fn list_identities(&self) -> ConsoleLogResult<Vec<ConsoleIdentityRecord>> {
        if let Some(identities) = self
            .inner
            .identity_read_model
            .refresh_now_if_idle(
                self.inner.clone(),
                IdentityCollectionMode::IncludeLiveMembers,
            )
            .await
        {
            let identities = identities?;
            spawn_identity_backfills_for_records(self.inner.clone(), &identities);
            return Ok(identities);
        }
        self.inner
            .identity_read_model
            .refresh_soon(self.inner.clone());
        let identities = self
            .inner
            .identity_read_model
            .snapshot(self.inner.clone())
            .await?;
        spawn_identity_backfills_for_records(self.inner.clone(), &identities);
        Ok(identities)
    }

    pub(crate) async fn list_identities_fresh(
        &self,
    ) -> ConsoleLogResult<Vec<ConsoleIdentityRecord>> {
        let _guard = self
            .inner
            .identity_read_model
            .refresh_lock
            .clone()
            .lock_owned()
            .await;
        let identities =
            collect_identity_records(&self.inner, IdentityCollectionMode::IncludeLiveMembers)
                .await?;
        self.inner
            .identity_read_model
            .replace(identities.clone())
            .await;
        spawn_identity_backfills_for_records(self.inner.clone(), &identities);
        Ok(identities)
    }

    pub async fn inspect_identity(
        &self,
        identity: &str,
    ) -> ConsoleLogResult<Option<ConsoleIdentityInspection>> {
        let entries = self
            .inner
            .runtimes
            .read()
            .map_err(|_| runtime_registry_lock_error())?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for entry in &entries {
            let Some(identity_runtime) = entry.identity_runtime.clone() else {
                continue;
            };
            let Some(raw_identity) = strip_namespace(identity, &entry.identity_namespace) else {
                continue;
            };
            let Ok(parsed_identity) = crate::identity_first::AgentIdentity::parse(&raw_identity)
            else {
                continue;
            };
            let Ok(status) = identity_runtime.status(&parsed_identity).await else {
                continue;
            };
            let mut record = identity_record_for_status(entry, &status);
            let topology_peers =
                identity_runtime_topology_peers(entry, identity_runtime.as_ref()).await;
            record.topology_peers = topology_peers
                .get(&record.identity)
                .cloned()
                .unwrap_or_default();
            if !entry.visibility_policy.identity_visible(&record) {
                return Ok(None);
            }
            if let Some(session_id) = record.session_id.clone() {
                spawn_session_history_backfill_target(
                    self.inner.clone(),
                    SessionBackfillTarget {
                        entry: entry.clone(),
                        record: record.clone(),
                        session_id,
                    },
                    false,
                );
            }
            return Ok(Some(ConsoleIdentityInspection {
                peers: record.topology_peers.clone(),
                identity: record,
            }));
        }

        if let Some(resolved) = self.resolve_member(identity).await {
            let Some(record) =
                identity_record_for_member(&resolved.entry, &resolved.handle, &resolved.member)
                    .await
            else {
                return Ok(None);
            };
            if !resolved.entry.visibility_policy.identity_visible(&record) {
                return Ok(None);
            }
            if let Some(session_id) = record.session_id.clone() {
                spawn_session_history_backfill_target(
                    self.inner.clone(),
                    SessionBackfillTarget {
                        entry: resolved.entry.clone(),
                        record: record.clone(),
                        session_id,
                    },
                    false,
                );
            }
            let peers = resolved
                .member
                .wired_to
                .iter()
                .map(ToString::to_string)
                .collect();
            return Ok(Some(ConsoleIdentityInspection {
                identity: record,
                peers,
            }));
        }

        Ok(None)
    }

    pub async fn retire_identity(&self, identity: &str) -> ConsoleLogResult<bool> {
        let matches = self.resolve_members(identity).await;
        let mut retired_any = false;
        for resolved in matches {
            let Some(record) =
                identity_record_for_member(&resolved.entry, &resolved.handle, &resolved.member)
                    .await
            else {
                continue;
            };
            if !resolved.entry.visibility_policy.identity_visible(&record) {
                continue;
            }
            resolved
                .handle
                .retire(MeerkatId::from(resolved.runtime_identity.as_str()))
                .await
                .map_err(|err| -> ConsoleLogError {
                    format!("retire failed for {identity}: {err}").into()
                })?;
            retired_any = true;
        }
        Ok(retired_any)
    }

    pub async fn clear_timeline_frames(&self) -> ConsoleLogResult<()> {
        self.inner.store.clear_frames().await
    }

    pub async fn query_timeline(
        &self,
        query: ConsoleTimelineQuery,
    ) -> ConsoleLogResult<ConsoleTimelinePage> {
        let explicit_identity = query.identity.clone();
        let mut page = self.inner.store.query_frames(query.clone()).await?;
        if page.frames.is_empty()
            && let Some(identity) = explicit_identity.clone()
        {
            backfill_identity_for_explicit_query(self.inner.clone(), identity).await;
            page = self.inner.store.query_frames(query).await?;
        }
        let mut visible_frames = Vec::with_capacity(page.frames.len());
        let mut identity_visibility_cache = HashMap::new();
        let identity_records = self.inner.identity_read_model.current().await;
        for frame in page.frames {
            let allow_historical_identity =
                explicit_identity.as_deref() == Some(frame.identity.as_str());
            if frame_is_visible_cached(
                &self.inner,
                &frame,
                allow_historical_identity,
                &mut identity_visibility_cache,
                &identity_records,
            )
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
        let results =
            join_all(runtime_keys.into_iter().map(|runtime_key| {
                backfill_session_history(self.inner.clone(), runtime_key, true)
            }))
            .await;
        for result in results {
            result?;
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
                let identity_records = self.inner.identity_read_model.current().await;
                frame_is_visible(&self.inner, frame, false, &identity_records)
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
        let identity_records = self.inner.identity_read_model.current().await;
        frame_is_visible(
            &self.inner,
            frame,
            allow_historical_identity,
            &identity_records,
        )
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
            .resolve_bridge_session_id_observation(&MeerkatId::from(
                resolved.runtime_identity.as_str(),
            ))
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

        spawn_console_send_dispatch(
            self.inner.clone(),
            resolved,
            content,
            handling_mode,
            dispatching,
            outcome.frame,
            interaction_id,
        );
        Ok(accepted)
    }

    pub async fn reserve_identity_first_interaction(
        &self,
        request: ConsoleSendRequest,
        session_id: Option<&str>,
    ) -> Result<ConsoleInteractionAccepted, ConsoleSendError> {
        validate_send_request(&request)?;
        let _content = content_input_from_value(&request.content)?;
        let runtime_key = self
            .runtime_key_for_identity_first_send(&request.identity)
            .await?;
        let handling_mode_value = request
            .handling_mode
            .as_deref()
            .unwrap_or("queue")
            .to_string();
        let dedupe_key = send_dedupe_key(
            &runtime_key,
            &request.identity,
            &request.origin,
            &request.idempotency_key,
        );
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
        let new_frame = NewConsoleFrame {
            id: None,
            dedupe_key,
            timestamp_ms: current_time_ms(),
            runtime_key,
            identity: request.identity.clone(),
            conversation_id: Some(request.identity.clone()),
            session_id: session_id.map(ToString::to_string),
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
            interaction_id: Some(interaction_id),
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        };
        let outcome = self
            .inner
            .store
            .append_if_absent(new_frame)
            .await
            .map_err(ConsoleSendError::Log)?;
        let _ = self
            .inner
            .event_tx
            .send(ConsoleTimelineEvent::ConsoleFrame {
                frame: outcome.frame.clone(),
            });
        Ok(accepted_from_frame(&outcome.frame))
    }

    async fn runtime_key_for_identity_first_send(
        &self,
        identity: &str,
    ) -> Result<String, ConsoleSendError> {
        let entries = self
            .inner
            .runtimes
            .read()
            .map_err(|_| ConsoleSendError::Log(runtime_registry_lock_error()))?
            .clone();
        if entries.is_empty() {
            return Ok("identity-first".to_string());
        }

        let mut identity_runtime_keys = Vec::new();
        for entry in entries.values() {
            let Some(identity_runtime) = entry.identity_runtime.as_ref() else {
                continue;
            };
            identity_runtime_keys.push(entry.runtime_key.clone());
            let runtime_identity = strip_namespace(identity, &entry.identity_namespace)
                .unwrap_or_else(|| identity.to_string());
            let Ok(parsed_identity) =
                crate::identity_first::AgentIdentity::parse(&runtime_identity)
            else {
                continue;
            };
            if identity_runtime.status(&parsed_identity).await.is_ok() {
                return Ok(entry.runtime_key.clone());
            }
        }

        match identity_runtime_keys.as_slice() {
            [] => Err(ConsoleSendError::UnknownIdentity(identity.to_string())),
            [runtime_key] => Ok(runtime_key.clone()),
            _ => Err(ConsoleSendError::InvalidRequest(format!(
                "identity-first send for '{identity}' did not match exactly one registered runtime"
            ))),
        }
    }

    pub async fn mark_interaction_delivery_failed(
        &self,
        input_frame_id: &str,
    ) -> Result<(), ConsoleSendError> {
        update_frame_status_and_emit(
            &self.inner,
            input_frame_id,
            ConsoleFrameStatus::DeliveryFailed,
        )
        .await
        .map_err(ConsoleSendError::Log)?;
        Ok(())
    }

    pub async fn mark_interaction_delivered(
        &self,
        input_frame_id: &str,
    ) -> Result<(), ConsoleSendError> {
        update_frame_status_and_emit(&self.inner, input_frame_id, ConsoleFrameStatus::Delivered)
            .await
            .map_err(ConsoleSendError::Log)?;
        Ok(())
    }

    pub async fn mark_steer_interaction_delivered(
        &self,
        input_frame_id: &str,
        interaction_id: &str,
    ) -> Result<(), ConsoleSendError> {
        let Some(updated) = update_frame_status_and_emit(
            &self.inner,
            input_frame_id,
            ConsoleFrameStatus::Delivered,
        )
        .await
        .map_err(ConsoleSendError::Log)?
        else {
            return Ok(());
        };
        append_steer_delivery_terminal(&self.inner, &updated, interaction_id)
            .await
            .map_err(ConsoleSendError::Log)?;
        Ok(())
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
        self.resolve_members(identity).await.into_iter().next()
    }

    async fn resolve_members(&self, identity: &str) -> Vec<ResolvedConsoleMember> {
        let entries = self
            .inner
            .runtimes
            .read()
            .ok()
            .map(|entries| entries.clone())
            .unwrap_or_default();
        let mut matches: Vec<(String, ResolvedConsoleMember)> = Vec::new();
        for entry in entries.values() {
            let Some(raw_identity) = strip_namespace(identity, &entry.identity_namespace) else {
                continue;
            };
            let mid = MeerkatId::from(raw_identity.as_str());
            for resolved in member_sources_for_entry(entry)
                .await
                .into_iter()
                .filter(|candidate| {
                    candidate.member.agent_identity == mid
                        || candidate
                            .member
                            .labels
                            .get("agent_identity")
                            .is_some_and(|agent_identity| agent_identity == &raw_identity)
                })
            {
                let session_id = resolved
                    .handle
                    .resolve_bridge_session_id_observation(&resolved.member.agent_identity)
                    .await
                    .map(|sid| sid.to_string())
                    .unwrap_or_default();
                matches.push((session_id, resolved));
            }
        }
        matches.sort_by(|left, right| right.0.cmp(&left.0));
        matches.into_iter().map(|(_, resolved)| resolved).collect()
    }
}

fn dedupe_identity_records(records: Vec<ConsoleIdentityRecord>) -> Vec<ConsoleIdentityRecord> {
    let mut by_identity: BTreeMap<String, ConsoleIdentityRecord> = BTreeMap::new();
    for record in records {
        by_identity
            .entry(record.identity.clone())
            .and_modify(|current| {
                if identity_record_prefer(&record, current) {
                    *current = record.clone();
                }
            })
            .or_insert(record);
    }
    by_identity.into_values().collect()
}

fn identity_record_prefer(
    candidate: &ConsoleIdentityRecord,
    current: &ConsoleIdentityRecord,
) -> bool {
    let candidate_live = candidate.addressable && candidate.health != "retired";
    let current_live = current.addressable && current.health != "retired";
    if candidate_live != current_live {
        return candidate_live;
    }
    candidate.session_id.as_deref().unwrap_or("") > current.session_id.as_deref().unwrap_or("")
}

async fn collect_identity_records(
    inner: &Arc<AggregatorInner>,
    mode: IdentityCollectionMode,
) -> ConsoleLogResult<Vec<ConsoleIdentityRecord>> {
    let entries = inner
        .runtimes
        .read()
        .map_err(|_| runtime_registry_lock_error())?
        .clone();
    let mut identities = Vec::new();
    for entry in entries.values() {
        if let Some(identity_runtime) = entry.identity_runtime.as_ref() {
            let topology_peers =
                identity_runtime_topology_peers(entry, identity_runtime.as_ref()).await;
            for status in identity_runtime.statuses().await {
                let mut record = identity_record_for_status(entry, &status);
                record.topology_peers = topology_peers
                    .get(&record.identity)
                    .cloned()
                    .unwrap_or_default();
                if entry.visibility_policy.identity_visible(&record) {
                    identities.push(record);
                }
            }
            if mode == IdentityCollectionMode::CachedOnly {
                continue;
            }
        }
        let members = if entry.identity_runtime.is_some() {
            match tokio::time::timeout(
                IDENTITY_FIRST_LIVE_MEMBER_REFRESH_WAIT,
                member_sources_for_entry(entry),
            )
            .await
            {
                Ok(members) => members,
                Err(_) => {
                    tracing::warn!(
                        runtime_key = %entry.runtime_key,
                        "identity-first live member refresh timed out; keeping cached identity read model"
                    );
                    Vec::new()
                }
            }
        } else {
            member_sources_for_entry(entry).await
        };
        for resolved in members {
            if let Some(record) =
                identity_record_for_member(entry, &resolved.handle, &resolved.member).await
                && entry.visibility_policy.identity_visible(&record)
            {
                identities.push(record);
            }
        }
    }
    Ok(dedupe_identity_records(identities))
}

async fn identity_runtime_topology_peers(
    entry: &RuntimeEntry,
    identity_runtime: &crate::identity_first::IdentityRuntime,
) -> BTreeMap<String, Vec<String>> {
    let mut peers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in identity_runtime.desired_peer_edges().await {
        let a = apply_namespace(edge.a().as_str(), &entry.identity_namespace);
        let b = apply_namespace(edge.b().as_str(), &entry.identity_namespace);
        peers.entry(a.clone()).or_default().insert(b.clone());
        peers.entry(b).or_default().insert(a);
    }
    peers
        .into_iter()
        .map(|(identity, peers)| (identity, peers.into_iter().collect()))
        .collect()
}

fn spawn_identity_backfills_for_records(
    inner: Arc<AggregatorInner>,
    records: &[ConsoleIdentityRecord],
) {
    if !inner.options.session_history_backfill_enabled {
        return;
    }
    let entries = match inner.runtimes.read() {
        Ok(entries) => entries.clone(),
        Err(_) => return,
    };
    for record in records {
        if record.health == "dormant" || record.health == "uninitialized" {
            continue;
        }
        let Some(entry) = entries.get(&record.runtime_key).cloned() else {
            continue;
        };
        let Some(session_id) = record.session_id.clone() else {
            continue;
        };
        spawn_session_history_backfill_target(
            inner.clone(),
            SessionBackfillTarget {
                entry,
                record: record.clone(),
                session_id,
            },
            false,
        );
    }
}

async fn member_sources_for_entry(entry: &RuntimeEntry) -> Vec<ResolvedConsoleMember> {
    let mut resolved = Vec::new();
    let primary_handle = entry.runtime.handle();
    let primary_mob_id = primary_handle.mob_id().to_string();
    for member in primary_handle.list_members_observation_snapshot().await {
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
    if !entry.visibility_policy.include_implicit_delegate_members() {
        return resolved;
    }
    for (mob_id, handle) in state.mob_handles_snapshot().await {
        if mob_id.as_str() == primary_mob_id {
            continue;
        }
        for member in handle.list_members_observation_snapshot().await {
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
        Err(err) if is_not_externally_addressable(&err) => {
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
                .resolve_bridge_session_id_observation(&mid)
                .await
                .map(|sid| sid.to_string())
                .ok_or_else(|| "member has no bridge session after internal turn".to_string())
        }
        Err(err) => Err(err.to_string()),
    }
}

fn is_not_externally_addressable(err: &MobRuntimeError) -> bool {
    matches!(
        err,
        MobRuntimeError::Mob(MobError::NotExternallyAddressable(_))
    )
}

fn spawn_console_send_dispatch(
    inner: Arc<AggregatorInner>,
    resolved: ResolvedConsoleMember,
    content: ContentInput,
    handling_mode: meerkat_core::types::HandlingMode,
    dispatching: SendState,
    user_frame: ConsoleFrame,
    interaction_id: String,
) {
    tokio::spawn(async move {
        match dispatch_message_to_resolved_member(&resolved, content, handling_mode).await {
            Ok(_) => {
                let _ = dispatching.apply(SendTransition::MarkDelivered);
                if let Err(err) = update_frame_status_and_emit(
                    &inner,
                    &user_frame.id,
                    ConsoleFrameStatus::Delivered,
                )
                .await
                {
                    tracing::warn!(
                        frame_id = %user_frame.id,
                        error = %err,
                        "failed to update console send delivery status"
                    );
                }
                if handling_mode == HandlingMode::Steer
                    && let Err(err) =
                        append_steer_delivery_terminal(&inner, &user_frame, &interaction_id).await
                {
                    tracing::warn!(
                        frame_id = %user_frame.id,
                        interaction_id = %interaction_id,
                        error = %err,
                        "failed to append console steer terminal frame"
                    );
                }
            }
            Err(err) => {
                let _ = dispatching.apply(SendTransition::MarkDeliveryFailed);
                if let Err(update_err) = update_frame_status_and_emit(
                    &inner,
                    &user_frame.id,
                    ConsoleFrameStatus::DeliveryFailed,
                )
                .await
                {
                    tracing::warn!(
                        frame_id = %user_frame.id,
                        error = %update_err,
                        "failed to update console send failure status"
                    );
                }
                let failure_frame = NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("delivery-failed:{}", user_frame.id),
                    timestamp_ms: current_time_ms(),
                    runtime_key: user_frame.runtime_key,
                    identity: user_frame.identity,
                    conversation_id: user_frame.conversation_id,
                    session_id: user_frame.session_id,
                    kind: "message_delivery_failed".to_string(),
                    status: ConsoleFrameStatus::DeliveryFailed,
                    payload: json!({ "reason": err }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::Synthetic,
                        source_cursor: None,
                    },
                    source_event_id: None,
                    interaction_id: Some(interaction_id),
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: Some(user_frame.id.clone()),
                    caused_by_frame_id: Some(user_frame.id),
                };
                if let Err(append_err) = append_and_emit(&inner, failure_frame).await {
                    tracing::warn!(
                        error = %append_err,
                        "failed to append console send failure frame"
                    );
                }
            }
        }
    });
}

async fn append_steer_delivery_terminal(
    inner: &AggregatorInner,
    user_frame: &ConsoleFrame,
    interaction_id: &str,
) -> ConsoleLogResult<AppendOutcome> {
    append_and_emit(
        inner,
        NewConsoleFrame {
            id: None,
            dedupe_key: format!("steer-delivered:{}", user_frame.id),
            timestamp_ms: current_time_ms(),
            runtime_key: user_frame.runtime_key.clone(),
            identity: user_frame.identity.clone(),
            conversation_id: user_frame.conversation_id.clone(),
            session_id: user_frame.session_id.clone(),
            kind: "interaction_complete".to_string(),
            status: ConsoleFrameStatus::Completed,
            payload: json!({
                "reason": "steer_delivered",
                "handling_mode": "steer",
            }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::Synthetic,
                source_cursor: None,
            },
            source_event_id: None,
            interaction_id: Some(interaction_id.to_string()),
            turn_id: None,
            run_id: None,
            parent_frame_id: Some(user_frame.id.clone()),
            caused_by_frame_id: Some(user_frame.id.clone()),
        },
    )
    .await
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
    inner: Arc<AggregatorInner>,
    runtime_key: String,
    force_refresh: bool,
) -> ConsoleLogResult<()> {
    if !inner.options.session_history_backfill_enabled {
        return Ok(());
    }
    let Some(entry) = inner
        .runtimes
        .read()
        .ok()
        .and_then(|entries| entries.get(&runtime_key).cloned())
    else {
        return Ok(());
    };
    let members = member_sources_for_entry(&entry).await;
    let mut targets = Vec::new();
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
        targets.push(SessionBackfillTarget {
            entry: entry.clone(),
            record,
            session_id,
        });
    }
    backfill_session_history_targets(inner, targets, force_refresh).await
}

#[derive(Clone)]
struct SessionBackfillTarget {
    entry: RuntimeEntry,
    record: ConsoleIdentityRecord,
    session_id: String,
}

async fn backfill_session_history_targets(
    inner: Arc<AggregatorInner>,
    targets: Vec<SessionBackfillTarget>,
    force_refresh: bool,
) -> ConsoleLogResult<()> {
    let mut tasks = tokio::task::JoinSet::new();
    for target in targets {
        tasks.spawn(backfill_one_session_history(
            inner.clone(),
            target,
            force_refresh,
        ));
    }
    let mut first_error = None;
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
            Err(err) => {
                if first_error.is_none() {
                    first_error = Some(Box::new(std::io::Error::other(format!(
                        "session backfill task failed: {err}"
                    ))) as ConsoleLogError);
                }
            }
        }
    }
    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(())
    }
}

async fn backfill_one_session_history(
    inner: Arc<AggregatorInner>,
    target: SessionBackfillTarget,
    force_refresh: bool,
) -> ConsoleLogResult<()> {
    let _permit = inner
        .session_backfill_permits
        .clone()
        .acquire_owned()
        .await
        .map_err(|err| -> ConsoleLogError {
            Box::new(std::io::Error::other(format!(
                "session backfill limiter closed: {err}"
            )))
        })?;
    let SessionBackfillTarget {
        entry,
        record,
        session_id,
    } = target;
    let watermark_runtime_key =
        session_history_watermark_runtime_key(&entry.runtime_key, &session_id);
    let watermark = inner
        .store
        .source_watermark(
            &watermark_runtime_key,
            ConsoleFrameSourceKind::SessionHistory,
        )
        .await?;
    let now_ms = current_time_ms();
    let mut offset = watermark
        .as_deref()
        .and_then(|watermark| parse_session_history_watermark(watermark, &session_id))
        .unwrap_or(0);
    if !force_refresh
        && watermark.as_deref().is_some_and(|watermark| {
            session_history_watermark_is_fresh(watermark, &session_id, now_ms)
        })
    {
        return Ok(());
    }
    loop {
        let page = match entry
            .runtime
            .read_session_history(&session_id, offset, Some(SESSION_HISTORY_PAGE_LIMIT))
            .await
        {
            Ok(page) => page,
            Err(err) => {
                append_backfill_gap(
                    &inner,
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
                    &inner,
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
                &inner,
                &entry.runtime_key,
                &record.identity,
                "session history page missing messages".to_string(),
            )
            .await?;
            break;
        };
        if messages.is_empty() {
            if offset > 0 {
                record_session_history_watermark(
                    &inner,
                    &watermark_runtime_key,
                    &session_id,
                    offset,
                )
                .await?;
            }
            break;
        }
        for (idx, message) in messages.iter().enumerate() {
            let absolute_offset = base_offset + idx;
            let frames = frames_from_session_history_message(
                &entry.runtime_key,
                &record.identity,
                &session_id,
                absolute_offset,
                message.clone(),
            );
            for mut frame in frames {
                if history_frame_has_existing_counterpart(&inner, &frame).await? {
                    continue;
                }
                if let Some(redacted) = entry.visibility_policy.redact_payload(&frame) {
                    frame.payload = redacted;
                    frame.status = ConsoleFrameStatus::Redacted;
                }
                append_and_emit(&inner, frame).await?;
            }
        }
        offset = base_offset + messages.len();
        record_session_history_watermark(&inner, &watermark_runtime_key, &session_id, offset)
            .await?;
        let has_more = page_value
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !has_more || messages.len() < SESSION_HISTORY_PAGE_LIMIT {
            break;
        }
    }
    Ok(())
}

async fn record_session_history_watermark(
    inner: &AggregatorInner,
    watermark_runtime_key: &str,
    session_id: &str,
    offset: usize,
) -> ConsoleLogResult<()> {
    inner
        .store
        .record_source_watermark(
            watermark_runtime_key,
            ConsoleFrameSourceKind::SessionHistory,
            &format_session_history_watermark(session_id, offset, current_time_ms()),
        )
        .await
}

fn spawn_session_history_backfill(inner: Arc<AggregatorInner>, runtime_key: String) {
    if !inner.options.session_history_backfill_enabled {
        return;
    }
    tokio::spawn(async move {
        {
            let mut active = inner.active_session_backfills.lock().await;
            if !active.insert(runtime_key.clone()) {
                return;
            }
        }
        let result = backfill_session_history(inner.clone(), runtime_key.clone(), false).await;
        let mut active = inner.active_session_backfills.lock().await;
        active.remove(&runtime_key);
        drop(active);
        if let Err(err) = result {
            tracing::warn!(
                runtime_key = %runtime_key,
                error = %err,
                "console session-history backfill failed"
            );
        }
    });
}

fn spawn_session_history_discovery_loop(inner: Arc<AggregatorInner>, runtime_key: String) {
    if !inner.options.session_history_backfill_enabled {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SESSION_HISTORY_DISCOVERY_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let runtime_still_registered = inner
                .runtimes
                .read()
                .ok()
                .is_some_and(|entries| entries.contains_key(&runtime_key));
            if !runtime_still_registered {
                break;
            }
            spawn_session_history_backfill(inner.clone(), runtime_key.clone());
        }
    });
}

fn spawn_session_history_backfill_target(
    inner: Arc<AggregatorInner>,
    target: SessionBackfillTarget,
    force_refresh: bool,
) {
    if !inner.options.session_history_backfill_enabled {
        return;
    }
    tokio::spawn(async move {
        let active_key = targeted_session_history_active_key(&target, force_refresh);
        let result =
            run_targeted_session_history_backfill(inner.clone(), target, force_refresh).await;
        if let Err(err) = result {
            tracing::warn!(
                active_key = %active_key,
                error = %err,
                "console targeted session-history backfill failed"
            );
        }
    });
}

async fn run_targeted_session_history_backfill(
    inner: Arc<AggregatorInner>,
    target: SessionBackfillTarget,
    force_refresh: bool,
) -> ConsoleLogResult<()> {
    let active_key = targeted_session_history_active_key(&target, force_refresh);
    {
        let mut active = inner.active_session_backfills.lock().await;
        if !active.insert(active_key.clone()) {
            return Ok(());
        }
    }
    let result = backfill_one_session_history(inner.clone(), target, force_refresh).await;
    let mut active = inner.active_session_backfills.lock().await;
    active.remove(&active_key);
    result
}

fn targeted_session_history_active_key(
    target: &SessionBackfillTarget,
    force_refresh: bool,
) -> String {
    let mode = if force_refresh { "force" } else { "refresh" };
    format!(
        "{}:session-history:{}:{mode}",
        target.entry.runtime_key, target.session_id
    )
}

fn spawn_session_history_backfill_for_identity(
    inner: Arc<AggregatorInner>,
    identity: String,
    force_refresh: bool,
) {
    if !inner.options.session_history_backfill_enabled {
        return;
    }
    tokio::spawn(async move {
        let Some(target) = session_backfill_target_for_identity(&inner, &identity).await else {
            return;
        };
        spawn_session_history_backfill_target(inner, target, force_refresh);
    });
}

async fn backfill_identity_for_explicit_query(inner: Arc<AggregatorInner>, identity: String) {
    if !inner.options.session_history_backfill_enabled {
        return;
    }
    let (tx, rx) = oneshot::channel();
    let warning_identity = identity.clone();
    tokio::spawn(async move {
        let result = async {
            let Some(target) = session_backfill_target_for_identity(&inner, &identity).await else {
                return Ok(());
            };
            run_targeted_session_history_backfill(inner, target, true).await
        }
        .await
        .map_err(|err| err.to_string());
        let _ = tx.send(result);
    });
    match tokio::time::timeout(EXPLICIT_IDENTITY_BACKFILL_WAIT, rx).await {
        Ok(Ok(Ok(())) | Err(_)) | Err(_) => {}
        Ok(Ok(Err(err))) => {
            tracing::warn!(
                identity = %warning_identity,
                error = %err,
                "console explicit identity session-history refresh failed"
            );
        }
    }
}

fn spawn_opportunistic_session_history_backfill_for_identity(
    inner: Arc<AggregatorInner>,
    identity: String,
) {
    if !inner.options.session_history_backfill_enabled {
        return;
    }
    tokio::spawn(async move {
        let Some(target) = session_backfill_target_for_identity(&inner, &identity).await else {
            return;
        };
        let active_key = format!(
            "{}:session-history:{}",
            target.entry.runtime_key, target.session_id
        );
        {
            let mut seen = inner.opportunistic_session_backfills.lock().await;
            if !seen.insert(active_key) {
                return;
            }
        }
        spawn_session_history_backfill_target(inner, target, false);
    });
}

async fn session_backfill_target_for_identity(
    inner: &AggregatorInner,
    identity: &str,
) -> Option<SessionBackfillTarget> {
    let entries = inner
        .runtimes
        .read()
        .ok()
        .map(|entries| entries.clone())
        .unwrap_or_default();
    for entry in entries.values() {
        let Some(raw_identity) = strip_namespace(identity, &entry.identity_namespace) else {
            continue;
        };
        let mid = MeerkatId::from(raw_identity.as_str());
        for resolved in member_sources_for_entry(entry)
            .await
            .into_iter()
            .filter(|candidate| candidate.member.agent_identity == mid)
        {
            let Some(record) =
                identity_record_for_member(entry, &resolved.handle, &resolved.member).await
            else {
                continue;
            };
            if !entry.visibility_policy.identity_visible(&record) {
                continue;
            }
            let Some(session_id) = record.session_id.clone() else {
                continue;
            };
            return Some(SessionBackfillTarget {
                entry: entry.clone(),
                record,
                session_id,
            });
        }
    }
    None
}

async fn recover_lagged_source_events(
    inner: Arc<AggregatorInner>,
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
                project_console_event(inner.clone(), runtime_key, envelope).await?;
            }
        }
        Err(err) => {
            append_source_gap(
                &inner,
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
    inner: Arc<AggregatorInner>,
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
    let refresh_identity = if console_event_should_refresh_session_history(&frame) {
        Some(frame.identity.clone())
    } else {
        None
    };
    let opportunistic_refresh_identity = if refresh_identity.is_none()
        && console_event_should_start_session_history_backfill(&frame)
    {
        Some(frame.identity.clone())
    } else {
        None
    };
    append_and_emit(&inner, frame).await?;
    inner
        .store
        .record_source_watermark(
            &entry.runtime_key,
            ConsoleFrameSourceKind::ConsoleEvent,
            &source_cursor,
        )
        .await?;
    if let Some(identity) = refresh_identity {
        spawn_session_history_backfill_for_identity(inner.clone(), identity, true);
    } else if let Some(identity) = opportunistic_refresh_identity {
        spawn_opportunistic_session_history_backfill_for_identity(inner.clone(), identity);
    }
    Ok(())
}

fn console_event_should_refresh_session_history(frame: &NewConsoleFrame) -> bool {
    matches!(
        frame.kind.as_str(),
        "interaction_complete" | "interaction_failed" | "message_delivery_failed"
    ) || frame.session_id.is_some()
}

fn console_event_should_start_session_history_backfill(frame: &NewConsoleFrame) -> bool {
    if frame.identity == SYSTEM_EVENT_IDENTITY {
        return false;
    }
    matches!(
        frame.kind.as_str(),
        "turn_started"
            | "run_started"
            | "reasoning_complete"
            | "tool_call_requested"
            | "tool_call"
            | "tool_execution_started"
            | "text_delta"
            | "system_notice"
    )
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

#[cfg(test)]
fn frame_from_session_history_message(
    runtime_key: &str,
    identity: &str,
    session_id: &str,
    offset: usize,
    message: Value,
) -> Option<NewConsoleFrame> {
    frames_from_session_history_message(runtime_key, identity, session_id, offset, message)
        .into_iter()
        .next()
}

fn frames_from_session_history_message(
    runtime_key: &str,
    identity: &str,
    session_id: &str,
    offset: usize,
    message: Value,
) -> Vec<NewConsoleFrame> {
    let payload_hash = hash_short(&serde_json::to_string(&message).unwrap_or_default());
    let Some(parsed) = serde_json::from_value::<Message>(message.clone()).ok() else {
        return Vec::new();
    };
    if let Message::ToolResults {
        results,
        created_at,
    } = parsed
    {
        return results
            .into_iter()
            .enumerate()
            .map(|(idx, result)| {
                let content = serde_json::to_value(&result.content).unwrap_or(Value::Null);
                let result_text = result.text_content();
                let tool_use_id = result.tool_use_id.clone();
                NewConsoleFrame {
                    id: None,
                    dedupe_key: format!(
                        "session-history:{runtime_key}:{session_id}:{offset}:{idx}:{payload_hash}"
                    ),
                    timestamp_ms: created_at.timestamp_millis().max(0) as u64,
                    runtime_key: runtime_key.to_string(),
                    identity: identity.to_string(),
                    conversation_id: Some(identity.to_string()),
                    session_id: Some(session_id.to_string()),
                    kind: "tool_execution_completed".to_string(),
                    status: ConsoleFrameStatus::Completed,
                    payload: json!({
                        "id": tool_use_id,
                        "tool_call_id": tool_use_id,
                        "result": result_text,
                        "content": content,
                        "is_error": result.is_error,
                        "source_event_type": "session_history",
                        "type": "session_history",
                    }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::SessionHistory,
                        source_cursor: Some(format!("{session_id}:{offset}:{idx}")),
                    },
                    source_event_id: None,
                    interaction_id: None,
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: None,
                    caused_by_frame_id: None,
                }
            })
            .collect();
    }
    let (kind, timestamp_ms, payload) = match &parsed {
        Message::User(user) => {
            if session_history_user_message_is_scaffold(&message) {
                return Vec::new();
            }
            (
                "user_input",
                user.created_at.timestamp_millis().max(0) as u64,
                json!({
                    "content": user.content,
                    "message": message,
                }),
            )
        }
        Message::Assistant(assistant) => (
            "interaction_complete",
            assistant.created_at.timestamp_millis().max(0) as u64,
            json!({
                "result": assistant.content,
                "text": assistant.content,
                "message": message,
                "source_event_type": "session_history",
                "type": "session_history",
            }),
        ),
        Message::BlockAssistant(assistant) => {
            let text = assistant.text_blocks().collect::<Vec<_>>().join("");
            (
                "interaction_complete",
                assistant.created_at.timestamp_millis().max(0) as u64,
                json!({
                    "result": text,
                    "text": text,
                    "message": message,
                    "source_event_type": "session_history",
                    "type": "session_history",
                }),
            )
        }
        Message::SystemNotice(notice) => (
            "system_notice",
            notice.created_at.timestamp_millis().max(0) as u64,
            json!({
                "message": message,
                "kind": notice.kind,
                "render_class": notice.kind.render_class(),
                "body": notice.body,
                "blocks": notice.blocks,
                "source_event_type": "session_history",
                "type": "session_history",
            }),
        ),
        Message::System(_) | Message::ToolResults { .. } => return Vec::new(),
    };
    let mut frames = vec![NewConsoleFrame {
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
    }];
    if let Message::BlockAssistant(assistant) = &parsed {
        frames.extend(spawn_initial_message_frames_from_assistant(
            runtime_key,
            identity,
            session_id,
            offset,
            assistant,
            &payload_hash,
        ));
    }
    frames
}

fn spawn_initial_message_frames_from_assistant(
    runtime_key: &str,
    parent_identity: &str,
    session_id: &str,
    offset: usize,
    assistant: &meerkat_core::types::BlockAssistantMessage,
    payload_hash: &str,
) -> Vec<NewConsoleFrame> {
    let mut frames = Vec::new();
    for (tool_idx, tool) in assistant.tool_calls().enumerate() {
        if tool.name != "mob_spawn_member" && tool.name != "spawn_member" {
            continue;
        }
        let Ok(args) = serde_json::from_str::<Value>(tool.args.get()) else {
            continue;
        };
        for (spawn_idx, (target_identity, initial_message)) in
            spawn_initial_messages_from_tool_args(&args)
                .into_iter()
                .enumerate()
        {
            let message_content = match &initial_message {
                Value::String(text) => json!([
                    {
                        "type": "text",
                        "text": text,
                    }
                ]),
                other => other.clone(),
            };
            let message = match &initial_message {
                Value::String(text) => json!({
                    "role": "user",
                    "content": text,
                    "created_at": assistant.created_at,
                }),
                other => json!({
                    "role": "user",
                    "content": other,
                    "created_at": assistant.created_at,
                }),
            };
            frames.push(NewConsoleFrame {
                id: None,
                dedupe_key: format!(
                    "session-history:{runtime_key}:{session_id}:{offset}:spawn-initial:{tool_idx}:{spawn_idx}:{payload_hash}"
                ),
                timestamp_ms: assistant.created_at.timestamp_millis().max(0) as u64,
                runtime_key: runtime_key.to_string(),
                identity: target_identity.clone(),
                conversation_id: Some(target_identity),
                session_id: None,
                kind: "user_input".to_string(),
                status: ConsoleFrameStatus::Completed,
                payload: json!({
                    "content": message_content,
                    "message": message,
                    "source_event_type": "session_history_spawn_initial_message",
                    "type": "session_history",
                    "tool_call_id": tool.id,
                    "parent_identity": parent_identity,
                    "via_tool": tool.name,
                }),
                source: ConsoleFrameSource {
                    kind: ConsoleFrameSourceKind::SessionHistory,
                    source_cursor: Some(format!(
                        "{session_id}:{offset}:spawn-initial:{tool_idx}:{spawn_idx}"
                    )),
                },
                source_event_id: None,
                interaction_id: None,
                turn_id: None,
                run_id: None,
                parent_frame_id: None,
                caused_by_frame_id: None,
            });
        }
    }
    frames
}

fn spawn_initial_messages_from_tool_args(args: &Value) -> Vec<(String, Value)> {
    let mut messages = Vec::new();
    if let Some(message) = spawn_initial_message_from_record(args) {
        messages.push(message);
    }
    if let Some(specs) = args.get("specs").and_then(Value::as_array) {
        for spec in specs {
            if let Some(message) = spawn_initial_message_from_record(spec) {
                messages.push(message);
            }
        }
    }
    messages
}

fn spawn_initial_message_from_record(record: &Value) -> Option<(String, Value)> {
    let target_identity = record
        .get("member_id")
        .or_else(|| record.get("agent_identity"))
        .and_then(Value::as_str)?
        .trim();
    if target_identity.is_empty() {
        return None;
    }
    let initial_message = record
        .get("initial_message")
        .or_else(|| record.get("task"))?
        .clone();
    Some((target_identity.to_string(), initial_message))
}

fn session_history_user_message_is_scaffold(message: &Value) -> bool {
    message
        .get("content")
        .is_some_and(scaffold_message_content_is_noise)
}

fn scaffold_message_content_is_noise(value: &Value) -> bool {
    match value {
        Value::String(text) => scaffold_message_text_is_noise(text),
        Value::Array(items) => items.iter().any(scaffold_message_content_is_noise),
        Value::Object(map) => ["text", "content", "message"]
            .iter()
            .filter_map(|key| map.get(*key))
            .any(scaffold_message_content_is_noise),
        _ => false,
    }
}

fn scaffold_message_text_is_noise(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("[PEER UPDATE]")
        || trimmed
            .to_ascii_lowercase()
            .starts_with("you have been spawned")
}

fn parse_session_history_watermark(watermark: &str, session_id: &str) -> Option<usize> {
    let rest = watermark.strip_prefix(session_id)?.strip_prefix(':')?;
    rest.split(':').next()?.parse().ok()
}

fn format_session_history_watermark(session_id: &str, offset: usize, checked_at_ms: u64) -> String {
    format!("{session_id}:{offset}:{checked_at_ms}")
}

fn session_history_watermark_is_fresh(watermark: &str, session_id: &str, now_ms: u64) -> bool {
    let Some(checked_at_ms) = watermark
        .rsplit_once(':')
        .and_then(|(_, checked_at_ms)| checked_at_ms.parse::<u64>().ok())
    else {
        return false;
    };
    let offset = parse_session_history_watermark(watermark, session_id).unwrap_or(0);
    let ttl_ms = if offset > 0 {
        SESSION_HISTORY_GROWING_REFRESH_TTL_MS
    } else {
        SESSION_HISTORY_REFRESH_TTL_MS
    };
    now_ms.saturating_sub(checked_at_ms) < ttl_ms
}

async fn history_frame_has_existing_counterpart(
    inner: &AggregatorInner,
    frame: &NewConsoleFrame,
) -> ConsoleLogResult<bool> {
    let fingerprint = transcript_fingerprint(&frame.kind, &frame.payload);
    let Some(fingerprint) = fingerprint else {
        return Ok(false);
    };
    let assistant_terminal = assistant_terminal_fingerprint(&frame.kind, &frame.payload).is_some();
    let mut delta_text_by_turn = BTreeMap::<String, String>::new();
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
        for existing in &page.frames {
            let same_session = existing.session_id == frame.session_id
                || existing.session_id.is_none()
                || frame.session_id.is_none();
            if existing.source.kind == ConsoleFrameSourceKind::SessionHistory || !same_session {
                continue;
            }
            if transcript_fingerprint(&existing.kind, &existing.payload).as_ref()
                == Some(&fingerprint)
            {
                return Ok(true);
            }
            if assistant_terminal
                && let Some(delta) = text_delta_payload_text(&existing.kind, &existing.payload)
            {
                let turn_key = existing
                    .interaction_id
                    .as_deref()
                    .or(existing.turn_id.as_deref())
                    .or(existing.run_id.as_deref())
                    .unwrap_or("session");
                let aggregated = delta_text_by_turn.entry(turn_key.to_string()).or_default();
                aggregated.push_str(delta);
                if normalize_transcript_fingerprint_text(aggregated) == fingerprint {
                    return Ok(true);
                }
            }
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
        "tool_execution_completed" => tool_result_fingerprint(payload),
        "text_delta" => {
            text_delta_payload_text(kind, payload).map(normalize_transcript_fingerprint_text)
        }
        "text_complete" | "interaction_complete" | "run_completed" => {
            assistant_terminal_fingerprint(kind, payload)
        }
        _ => None,
    }
}

fn tool_result_fingerprint(payload: &Value) -> Option<String> {
    let id = payload
        .get("tool_call_id")
        .or_else(|| payload.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let result = payload
        .get("result")
        .or_else(|| payload.get("content"))
        .map(stable_value_fingerprint)?;
    Some(format!("{id}:{result}"))
}

fn assistant_terminal_fingerprint(kind: &str, payload: &Value) -> Option<String> {
    match kind {
        "text_complete" | "interaction_complete" | "run_completed" => payload
            .get("text")
            .or_else(|| payload.get("result"))
            .or_else(|| payload.get("content"))
            .map(stable_value_fingerprint),
        _ => None,
    }
}

fn text_delta_payload_text<'a>(kind: &str, payload: &'a Value) -> Option<&'a str> {
    if kind != "text_delta" {
        return None;
    }
    payload
        .get("delta")
        .or_else(|| payload.get("text"))
        .or_else(|| payload.get("content"))
        .and_then(Value::as_str)
        .or_else(|| payload.as_str())
}

fn stable_value_fingerprint(value: &Value) -> String {
    if let Some(text) = content_value_text(value) {
        return normalize_transcript_fingerprint_text(&text);
    }
    match value {
        Value::String(text) => normalize_transcript_fingerprint_text(text),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn content_value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(content_value_text)
                .collect::<String>();
            (!text.is_empty()).then_some(text)
        }
        Value::Object(map) => ["text", "content", "message", "blocks"]
            .iter()
            .filter_map(|key| map.get(*key))
            .find_map(content_value_text),
        _ => None,
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

#[derive(Debug, Clone, Copy)]
enum CachedIdentityVisibility {
    Visible,
    Hidden,
    Missing,
}

async fn frame_is_visible(
    inner: &AggregatorInner,
    frame: &ConsoleFrame,
    allow_historical_identity: bool,
    identity_records: &[ConsoleIdentityRecord],
) -> ConsoleLogResult<bool> {
    let mut identity_visibility_cache = HashMap::new();
    frame_is_visible_cached(
        inner,
        frame,
        allow_historical_identity,
        &mut identity_visibility_cache,
        identity_records,
    )
    .await
}

async fn frame_is_visible_cached(
    inner: &AggregatorInner,
    frame: &ConsoleFrame,
    allow_historical_identity: bool,
    identity_visibility_cache: &mut HashMap<(String, String), CachedIdentityVisibility>,
    identity_records: &[ConsoleIdentityRecord],
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
        let cache_key = (frame.runtime_key.clone(), frame.identity.clone());
        let identity_visibility =
            if let Some(cached) = identity_visibility_cache.get(&cache_key).copied() {
                cached
            } else {
                let runtime_member_id = strip_namespace(&frame.identity, &entry.identity_namespace)
                    .unwrap_or_else(|| frame.identity.clone());
                let visibility = match identity_records.iter().find(|record| {
                    record.runtime_key == frame.runtime_key
                        && (record.identity == frame.identity
                            || record.runtime_member_id == runtime_member_id)
                }) {
                    Some(record) if entry.visibility_policy.identity_visible(record) => {
                        CachedIdentityVisibility::Visible
                    }
                    Some(_) => CachedIdentityVisibility::Hidden,
                    None => CachedIdentityVisibility::Missing,
                };
                identity_visibility_cache.insert(cache_key, visibility);
                visibility
            };
        match identity_visibility {
            CachedIdentityVisibility::Visible => {}
            CachedIdentityVisibility::Hidden => return Ok(false),
            CachedIdentityVisibility::Missing => {
                return Ok(
                    allow_historical_identity && entry.visibility_policy.frame_visible(frame)
                );
            }
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
    let durable_identity = member
        .labels
        .get("agent_identity")
        .filter(|value| !value.trim().is_empty())
        .map_or(runtime_member_id.as_str(), String::as_str);
    let identity = apply_namespace(durable_identity, &entry.identity_namespace);
    let addressable = member_is_addressable(member);
    let visibility = if member.state == meerkat_mob::MemberState::Retiring {
        ConsoleVisibility::RetiredReadable
    } else if addressable {
        ConsoleVisibility::Addressable
    } else {
        ConsoleVisibility::Hidden
    };
    let session_id = handle
        .resolve_bridge_session_id_observation(&member.agent_identity)
        .await
        .map(|sid| sid.to_string());
    let display_name = member
        .labels
        .get("display_name")
        .cloned()
        .unwrap_or_else(|| runtime_member_id.clone());
    let mut labels = member.labels.clone();
    labels
        .entry("role".to_string())
        .or_insert_with(|| member.role.to_string());
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
        topology_peers: member.wired_to.iter().map(ToString::to_string).collect(),
        labels,
    })
}

fn identity_record_for_status(
    entry: &RuntimeEntry,
    status: &crate::identity_first::IdentityStatus,
) -> ConsoleIdentityRecord {
    let identity = apply_namespace(status.identity.as_str(), &entry.identity_namespace);
    let runtime_member_id = status
        .agent_runtime_id
        .as_ref()
        .map(crate::identity_first::AgentRuntimeId::as_str)
        .unwrap_or_else(|| status.identity.as_str())
        .to_string();
    let addressable = status.addressability
        == crate::identity_first::AgentAddressability::Addressable
        && matches!(
            status.state,
            crate::identity_first::IdentityLifecycleState::Active
                | crate::identity_first::IdentityLifecycleState::Dormant
                | crate::identity_first::IdentityLifecycleState::Uninitialized
        );
    let visibility = match status.state {
        crate::identity_first::IdentityLifecycleState::Retiring => {
            ConsoleVisibility::RetiredReadable
        }
        crate::identity_first::IdentityLifecycleState::Broken
        | crate::identity_first::IdentityLifecycleState::Suspended => {
            ConsoleVisibility::Unreachable
        }
        _ if addressable => ConsoleVisibility::Addressable,
        _ => ConsoleVisibility::Hidden,
    };
    let health = match status.state {
        crate::identity_first::IdentityLifecycleState::Active => "ready",
        crate::identity_first::IdentityLifecycleState::Dormant => "dormant",
        crate::identity_first::IdentityLifecycleState::Uninitialized => "uninitialized",
        crate::identity_first::IdentityLifecycleState::Broken => "broken",
        crate::identity_first::IdentityLifecycleState::Suspended => "suspended",
        crate::identity_first::IdentityLifecycleState::Retiring => "retired",
    }
    .to_string();
    let display_name = status
        .display_name
        .as_ref()
        .map(crate::identity_first::DisplayName::as_str)
        .unwrap_or_else(|| status.identity.as_str())
        .to_string();
    ConsoleIdentityRecord {
        identity,
        display_name,
        runtime_key: entry.runtime_key.clone(),
        runtime_member_id,
        session_id: status.session_id.as_ref().map(ToString::to_string),
        visibility,
        addressable,
        health,
        topology_peers: Vec::new(),
        labels: status.labels.clone(),
    }
}

pub(crate) fn is_implicit_delegate_member(
    role: &str,
    labels: &std::collections::BTreeMap<String, String>,
) -> bool {
    role.eq_ignore_ascii_case("delegate") && labels.contains_key("source_mob_id")
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use std::time::Instant;

    use futures::StreamExt;
    use meerkat::{AgentFactory, Config, build_ephemeral_service};
    use meerkat_client::types::LlmStream;
    use meerkat_client::{LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest, TestClient};
    use meerkat_core::{
        AppendSystemContextRequest, AppendSystemContextResult, CommsRuntime, EventStream,
        RunResult, SessionControlError, SessionError, SessionHistoryPage, SessionHistoryQuery,
        SessionId, SessionQuery, SessionService, SessionServiceCommsExt, SessionServiceControlExt,
        SessionServiceHistoryExt, SessionSummary, SessionView, StartTurnRequest, StopReason,
        StreamError,
    };
    use meerkat_mob::{MobDefinition, MobSessionService, MobStorage, SpawnMemberSpec};
    use serde_json::json;

    use super::*;
    use crate::mob_handle_runtime::MobBootstrapSpec;

    struct CountingConsoleLogStore {
        inner: InMemoryConsoleLogStore,
        source_watermark_calls: AtomicUsize,
        record_watermark_calls: AtomicUsize,
    }

    impl CountingConsoleLogStore {
        fn new() -> Self {
            Self {
                inner: InMemoryConsoleLogStore::new(),
                source_watermark_calls: AtomicUsize::new(0),
                record_watermark_calls: AtomicUsize::new(0),
            }
        }

        fn source_watermark_calls(&self) -> usize {
            self.source_watermark_calls.load(Ordering::SeqCst)
        }
    }

    struct SlowTestClient {
        delay: Duration,
    }

    #[async_trait::async_trait]
    impl LlmClient for SlowTestClient {
        fn project_replay_messages(&self, messages: &[Message]) -> Result<Vec<Message>, LlmError> {
            Ok(messages.to_vec())
        }

        fn stream<'a>(&'a self, _request: &'a LlmRequest) -> LlmStream<'a> {
            let delay = self.delay;
            let delayed_text = futures::stream::once(async move {
                tokio::time::sleep(delay).await;
                Ok(LlmEvent::TextDelta {
                    delta: "slow ok".to_string(),
                    meta: None,
                })
            });
            let done = futures::stream::once(async {
                Ok(LlmEvent::Done {
                    outcome: LlmDoneOutcome::Success {
                        stop_reason: StopReason::EndTurn,
                    },
                })
            });
            Box::pin(delayed_text.chain(done))
        }

        fn provider(&self) -> &'static str {
            "slow-test"
        }

        async fn health_check(&self) -> Result<(), LlmError> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct DelayedHistorySessionService {
        inner: Arc<dyn MobSessionService>,
        delay: Duration,
        read_calls: Arc<AtomicUsize>,
        active_reads: Arc<AtomicUsize>,
        max_active_reads: Arc<AtomicUsize>,
    }

    impl DelayedHistorySessionService {
        fn new(inner: Arc<dyn MobSessionService>, delay: Duration) -> Self {
            Self {
                inner,
                delay,
                read_calls: Arc::new(AtomicUsize::new(0)),
                active_reads: Arc::new(AtomicUsize::new(0)),
                max_active_reads: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn read_calls(&self) -> usize {
            self.read_calls.load(Ordering::SeqCst)
        }

        fn max_active_reads(&self) -> usize {
            self.max_active_reads.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl SessionService for DelayedHistorySessionService {
        async fn create_session(
            &self,
            req: meerkat_core::CreateSessionRequest,
        ) -> Result<RunResult, SessionError> {
            self.inner.create_session(req).await
        }

        async fn start_turn(
            &self,
            id: &SessionId,
            req: StartTurnRequest,
        ) -> Result<RunResult, SessionError> {
            self.inner.start_turn(id, req).await
        }

        async fn interrupt(&self, id: &SessionId) -> Result<(), SessionError> {
            self.inner.interrupt(id).await
        }

        async fn read(&self, id: &SessionId) -> Result<SessionView, SessionError> {
            self.inner.read(id).await
        }

        async fn list(&self, query: SessionQuery) -> Result<Vec<SessionSummary>, SessionError> {
            self.inner.list(query).await
        }

        async fn archive(&self, id: &SessionId) -> Result<(), SessionError> {
            self.inner.archive(id).await
        }

        async fn subscribe_session_events(
            &self,
            id: &SessionId,
        ) -> Result<EventStream, StreamError> {
            SessionService::subscribe_session_events(self.inner.as_ref(), id).await
        }
    }

    #[async_trait::async_trait]
    impl SessionServiceCommsExt for DelayedHistorySessionService {
        async fn comms_runtime(&self, session_id: &SessionId) -> Option<Arc<dyn CommsRuntime>> {
            self.inner.comms_runtime(session_id).await
        }
    }

    #[async_trait::async_trait]
    impl SessionServiceControlExt for DelayedHistorySessionService {
        async fn append_system_context(
            &self,
            id: &SessionId,
            req: AppendSystemContextRequest,
        ) -> Result<AppendSystemContextResult, SessionControlError> {
            self.inner.append_system_context(id, req).await
        }
    }

    #[async_trait::async_trait]
    impl SessionServiceHistoryExt for DelayedHistorySessionService {
        async fn read_history(
            &self,
            id: &SessionId,
            query: SessionHistoryQuery,
        ) -> Result<SessionHistoryPage, SessionError> {
            self.read_calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active_reads.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active_reads.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            let result = self.inner.read_history(id, query).await;
            self.active_reads.fetch_sub(1, Ordering::SeqCst);
            result
        }
    }

    #[async_trait::async_trait]
    impl MobSessionService for DelayedHistorySessionService {
        fn supports_persistent_sessions(&self) -> bool {
            self.inner.supports_persistent_sessions()
        }

        fn runtime_adapter(&self) -> Option<Arc<meerkat_runtime::MeerkatMachine>> {
            self.inner.runtime_adapter()
        }

        async fn session_belongs_to_mob(
            &self,
            session_id: &SessionId,
            mob_id: &meerkat_mob::MobId,
        ) -> bool {
            self.inner.session_belongs_to_mob(session_id, mob_id).await
        }

        async fn cancel_all_checkpointers(&self) {
            self.inner.cancel_all_checkpointers().await;
        }

        async fn rearm_all_checkpointers(&self) {
            self.inner.rearm_all_checkpointers().await;
        }
    }

    #[async_trait::async_trait]
    impl ConsoleLogStore for CountingConsoleLogStore {
        async fn append_if_absent(
            &self,
            frame: NewConsoleFrame,
        ) -> ConsoleLogResult<AppendOutcome> {
            self.inner.append_if_absent(frame).await
        }

        async fn update_frame_status(
            &self,
            frame_id: &str,
            status: ConsoleFrameStatus,
        ) -> ConsoleLogResult<Option<ConsoleFrame>> {
            self.inner.update_frame_status(frame_id, status).await
        }

        async fn query_frames(
            &self,
            query: ConsoleTimelineQuery,
        ) -> ConsoleLogResult<ConsoleTimelinePage> {
            self.inner.query_frames(query).await
        }

        async fn frame_by_dedupe_key(
            &self,
            dedupe_key: &str,
        ) -> ConsoleLogResult<Option<ConsoleFrame>> {
            self.inner.frame_by_dedupe_key(dedupe_key).await
        }

        async fn latest_cursor(&self) -> ConsoleLogResult<Option<ConsoleCursor>> {
            self.inner.latest_cursor().await
        }

        async fn clear_frames(&self) -> ConsoleLogResult<()> {
            self.inner.clear_frames().await
        }

        async fn record_source_watermark(
            &self,
            runtime_key: &str,
            source_kind: ConsoleFrameSourceKind,
            source_cursor: &str,
        ) -> ConsoleLogResult<()> {
            self.record_watermark_calls.fetch_add(1, Ordering::SeqCst);
            self.inner
                .record_source_watermark(runtime_key, source_kind, source_cursor)
                .await
        }

        async fn source_watermark(
            &self,
            runtime_key: &str,
            source_kind: ConsoleFrameSourceKind,
        ) -> ConsoleLogResult<Option<String>> {
            self.source_watermark_calls.fetch_add(1, Ordering::SeqCst);
            self.inner.source_watermark(runtime_key, source_kind).await
        }
    }

    async fn build_single_member_runtime() -> UnifiedRuntime {
        build_single_member_runtime_with_client(Arc::new(TestClient::default())).await
    }

    async fn build_single_member_runtime_with_client(client: Arc<dyn LlmClient>) -> UnifiedRuntime {
        let definition = MobDefinition::from_toml(
            r#"
[mob]
id = "console-aggregator-perf-test"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true

[profiles.worker.tools]
comms = true
"#,
        )
        .expect("definition parses");
        let runtime = UnifiedRuntime::builder()
            .definition(definition)
            .default_llm_client(client)
            .build()
            .await
            .expect("runtime builds");
        runtime
            .spawn(SpawnMemberSpec::from_wire(
                "worker".to_string(),
                "agent-a".to_string(),
                Some("You are agent-a.".into()),
                None,
                None,
            ))
            .await
            .expect("member spawns");
        runtime
    }

    async fn build_empty_runtime(mob_id: &str) -> UnifiedRuntime {
        let definition = MobDefinition::from_toml(&format!(
            r#"
[mob]
id = "{mob_id}"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true

[profiles.worker.tools]
comms = true
"#
        ))
        .expect("definition parses");
        UnifiedRuntime::builder()
            .definition(definition)
            .default_llm_client(Arc::new(TestClient::default()))
            .build()
            .await
            .expect("runtime builds")
    }

    async fn build_stress_runtime(
        member_count: usize,
        history_delay: Duration,
    ) -> (
        tempfile::TempDir,
        Arc<UnifiedRuntime>,
        DelayedHistorySessionService,
    ) {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let session_path = temp_dir.path().join("sessions");
        std::fs::create_dir_all(&session_path).expect("session path");
        let factory = AgentFactory::new(&session_path).comms(true);
        let base_service = Arc::new(build_ephemeral_service(
            factory,
            Config::default(),
            member_count + 8,
        ));
        let delayed_service = DelayedHistorySessionService::new(base_service, history_delay);
        let session_service: Arc<dyn MobSessionService> = Arc::new(delayed_service.clone());
        let definition = MobDefinition::from_toml(
            r#"
[mob]
id = "console-aggregator-stress-test"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true

[profiles.worker.tools]
comms = true
"#,
        )
        .expect("definition parses");
        let spec = MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
            .with_options(crate::mob_handle_runtime::MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: Some(Arc::new(TestClient::default())),
            });
        let runtime = Arc::new(
            UnifiedRuntime::bootstrap(
                spec,
                crate::types::MobKitConfig {
                    modules: Vec::new(),
                    discovery: crate::types::DiscoverySpec {
                        namespace: "stress".to_string(),
                        modules: Vec::new(),
                    },
                    pre_spawn: Vec::new(),
                },
                Duration::from_secs(5),
            )
            .await
            .expect("runtime boots"),
        );
        for idx in 0..member_count {
            runtime
                .spawn(SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    format!("agent-{idx}"),
                    Some(format!("You are agent-{idx}.").into()),
                    None,
                    None,
                ))
                .await
                .expect("member spawns");
        }
        (temp_dir, runtime, delayed_service)
    }

    fn runtime_entry_for_test(runtime_key: &str, runtime: &UnifiedRuntime) -> RuntimeEntry {
        RuntimeEntry {
            runtime_key: runtime_key.to_string(),
            identity_namespace: "test".to_string(),
            runtime: runtime.mob_runtime().clone(),
            identity_runtime: runtime.identity_runtime().cloned(),
            console_events: runtime.console_events(),
            visibility_policy: Arc::new(AllowAllConsoleVisibilityPolicy),
        }
    }

    fn identity_record_for_test(identity: &str) -> ConsoleIdentityRecord {
        ConsoleIdentityRecord {
            identity: identity.to_string(),
            display_name: identity.to_string(),
            runtime_key: "runtime-cache".to_string(),
            runtime_member_id: identity.to_string(),
            session_id: Some(format!("session-{identity}")),
            visibility: ConsoleVisibility::Addressable,
            addressable: true,
            health: "ready".to_string(),
            topology_peers: Vec::new(),
            labels: BTreeMap::new(),
        }
    }

    async fn identity_runtime_for_test(
        identities: &[&str],
    ) -> Result<Arc<crate::identity_first::IdentityRuntime>, Box<dyn std::error::Error + Send + Sync>>
    {
        let runtime = Arc::new(crate::identity_first::IdentityRuntime::new(
            crate::identity_first::IdentityRuntimeConfig {
                continuity_store: Arc::new(
                    crate::identity_first::LocalContinuityStore::in_memory()?
                ),
                lease_provider: Arc::new(crate::identity_first::LocalLeaseProvider::new()),
                runtime_instance_id: "console-aggregator-identity-test".to_string(),
                has_runtime_store: true,
                durability_policy: crate::identity_first::DurabilityPolicy::SyncWriteThrough,
                bridge: None,
                default_timeout: None,
            },
        ));
        for identity in identities {
            let identity = crate::identity_first::AgentIdentity::parse(identity)?;
            let record = crate::identity_first::ContinuityRecord {
                identity: identity.clone(),
                agent_runtime_id: crate::identity_first::AgentRuntimeId::parse(&format!(
                    "rt:{}:0",
                    identity.as_str()
                ))?,
                session_id: SessionId::new(),
                generation: crate::identity_first::ContinuityGeneration::new(0),
                checkpoint_version: crate::identity_first::CheckpointVersion::new(0),
            };
            runtime
                .register(
                    crate::identity_first::DurableAgentSpec {
                        identity: identity.clone(),
                        profile: meerkat_mob::ProfileName::from("worker"),
                        addressability: crate::identity_first::AgentAddressability::Addressable,
                        display_name: None,
                        labels: BTreeMap::new(),
                        context: None,
                        additional_instructions: Vec::new(),
                        initial_message: None,
                        runtime_mode_override: None,
                    },
                    crate::identity_first::IdentityLifecycleState::Active,
                    Some(record),
                    Some(crate::identity_first::LeaseGrant {
                        identity,
                        fencing_token: crate::identity_first::FencingToken::new(1),
                        ttl: Duration::from_mins(5),
                    }),
                )
                .await;
        }
        Ok(runtime)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn identity_record_uses_durable_agent_identity_label_for_identity_first_members()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let runtime = build_empty_runtime("identity-label-test").await;
        let mut labels = BTreeMap::new();
        labels.insert(
            "agent_identity".to_string(),
            "channel:C0SMOKEOB3".to_string(),
        );
        labels.insert("display_name".to_string(), "C0SMOKEOB3".to_string());
        runtime
            .spawn(
                SpawnMemberSpec::from_wire(
                    "worker".to_string(),
                    "rt:channel:C0SMOKEOB3:0".to_string(),
                    Some("You are C0SMOKEOB3.".into()),
                    None,
                    None,
                )
                .with_labels(labels),
            )
            .await
            .expect("member spawns");

        let entry = RuntimeEntry {
            runtime_key: "runtime-a".to_string(),
            identity_namespace: String::new(),
            runtime: runtime.mob_runtime().clone(),
            identity_runtime: runtime.identity_runtime().cloned(),
            console_events: runtime.console_events(),
            visibility_policy: Arc::new(AllowAllConsoleVisibilityPolicy),
        };
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator
            .inner
            .runtimes
            .write()
            .expect("runtime registry")
            .insert("runtime-a".to_string(), entry);

        let records = aggregator.list_identities_fresh().await?;
        let record = records
            .iter()
            .find(|record| record.identity == "channel:C0SMOKEOB3")
            .expect("durable identity is exposed");
        assert_eq!(record.runtime_member_id, "rt:channel:C0SMOKEOB3:0");

        let inspection = aggregator
            .inspect_identity("channel:C0SMOKEOB3")
            .await?
            .expect("durable identity resolves back to runtime member");
        assert_eq!(inspection.identity.identity, "channel:C0SMOKEOB3");
        assert_eq!(
            inspection.identity.runtime_member_id,
            "rt:channel:C0SMOKEOB3:0"
        );

        let _ = runtime.mob_handle().stop().await;
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn identity_first_list_identities_projects_cached_topology_peers()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let runtime = build_empty_runtime("identity-topology-cache-test").await;
        let identity_runtime = identity_runtime_for_test(&["agent:alpha", "agent:beta"]).await?;
        identity_runtime
            .set_desired_peer_edges(vec![crate::identity_first::ManagedPeerEdge::new(
                crate::identity_first::AgentIdentity::parse("agent:alpha")?,
                crate::identity_first::AgentIdentity::parse("agent:beta")?,
            )?])
            .await;

        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator.register_runtime_handles_with_policy(
            "identity-first",
            "",
            runtime.mob_runtime().clone(),
            Some(identity_runtime),
            runtime.console_events(),
            Arc::new(AllowAllConsoleVisibilityPolicy),
        );

        let identities = aggregator.list_identities().await?;
        let alpha = identities
            .iter()
            .find(|record| record.identity == "agent:alpha")
            .ok_or("agent:alpha identity missing")?;
        assert_eq!(alpha.topology_peers, vec!["agent:beta".to_string()]);
        let inspection = aggregator
            .inspect_identity("agent:alpha")
            .await?
            .ok_or("agent:alpha inspection missing")?;
        assert_eq!(inspection.peers, vec!["agent:beta".to_string()]);

        let _ = runtime.mob_handle().stop().await;
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn identity_first_list_identities_includes_member_only_spawned_workers()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let runtime = build_empty_runtime("identity-member-only-test").await;
        let identity_runtime = identity_runtime_for_test(&["agent:alpha"]).await?;
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator.register_runtime_handles_with_policy(
            "identity-first",
            "",
            runtime.mob_runtime().clone(),
            Some(identity_runtime),
            runtime.console_events(),
            Arc::new(AllowAllConsoleVisibilityPolicy),
        );

        runtime
            .spawn(SpawnMemberSpec::from_wire(
                "worker".to_string(),
                "agent:beta".to_string(),
                Some("You are beta.".into()),
                None,
                None,
            ))
            .await?;

        let identities = aggregator.list_identities().await?;
        assert!(
            identities
                .iter()
                .any(|record| record.identity == "agent:beta"),
            "member-only spawned workers should remain visible in identity-first listings: {identities:#?}"
        );

        let _ = runtime.mob_handle().stop().await;
        Ok(())
    }

    #[tokio::test]
    async fn list_identities_serves_hot_cache_while_identity_refresh_is_in_flight() {
        let aggregator = MobKitConsoleAggregator::in_memory();
        let record = identity_record_for_test("agent-cached");
        *aggregator.inner.identity_read_model.inner.write().await = vec![record.clone()];
        aggregator
            .inner
            .identity_read_model
            .primed
            .store(true, Ordering::Release);
        let _guard = aggregator
            .inner
            .identity_read_model
            .refresh_lock
            .clone()
            .lock_owned()
            .await;

        let identities =
            tokio::time::timeout(Duration::from_millis(50), aggregator.list_identities())
                .await
                .expect("hot identity list should not wait for refresh lock")
                .expect("identity list succeeds");

        assert_eq!(identities, vec![record]);
    }

    #[tokio::test]
    async fn list_identities_waits_for_inflight_identity_refresh_on_cold_cache() {
        let aggregator = MobKitConsoleAggregator::in_memory();
        let guard = aggregator
            .inner
            .identity_read_model
            .refresh_lock
            .clone()
            .lock_owned()
            .await;
        let waiter = tokio::spawn({
            let aggregator = aggregator.clone();
            async move { aggregator.list_identities().await }
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            !waiter.is_finished(),
            "cold identity list should wait for the in-flight refresh to finish"
        );

        let record = identity_record_for_test("agent-primed");
        *aggregator.inner.identity_read_model.inner.write().await = vec![record.clone()];
        aggregator
            .inner
            .identity_read_model
            .primed
            .store(true, Ordering::Release);
        drop(guard);

        let identities = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("cold identity list waiter should resume")
            .expect("waiter joins")
            .expect("identity list succeeds");
        assert_eq!(identities, vec![record]);
    }

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
    async fn query_timeline_is_store_local_for_registered_runtimes() {
        let store = Arc::new(CountingConsoleLogStore::new());
        let aggregator = MobKitConsoleAggregator::new(store.clone());
        let runtime = build_single_member_runtime().await;
        aggregator
            .inner
            .runtimes
            .write()
            .expect("runtime registry")
            .insert(
                "runtime-a".to_string(),
                runtime_entry_for_test("runtime-a", &runtime),
            );
        store
            .append_if_absent(NewConsoleFrame {
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
            })
            .await
            .expect("append frame");

        let page = tokio::time::timeout(
            Duration::from_millis(250),
            aggregator.query_timeline(ConsoleTimelineQuery {
                identity: Some("agent-a".to_string()),
                limit: 10,
                ..ConsoleTimelineQuery::default()
            }),
        )
        .await
        .expect("timeline query should not wait for session history")
        .expect("timeline query succeeds");

        assert_eq!(page.frames.len(), 1);
        assert_eq!(
            store.source_watermark_calls(),
            0,
            "query_timeline must not synchronously touch session-history watermarks"
        );
        let _ = runtime.mob_handle().stop().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn console_send_returns_after_acceptance_without_waiting_for_turn_completion() {
        let runtime = build_single_member_runtime_with_client(Arc::new(SlowTestClient {
            delay: Duration::from_secs(2),
        }))
        .await;
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator
            .inner
            .runtimes
            .write()
            .expect("runtime registry")
            .insert(
                "runtime-a".to_string(),
                runtime_entry_for_test("runtime-a", &runtime),
            );

        let start = Instant::now();
        let accepted = tokio::time::timeout(
            Duration::from_millis(300),
            aggregator.send(ConsoleSendRequest {
                identity: "test/agent-a".to_string(),
                content: json!("hello slow agent"),
                origin: "console:test".to_string(),
                idempotency_key: "nonblocking-send".to_string(),
                handling_mode: Some("queue".to_string()),
            }),
        )
        .await
        .expect("console send should return once the input is accepted")
        .expect("send succeeds");

        assert_eq!(accepted.status, ConsoleFrameStatus::Accepted);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "console send should not wait for the delayed LLM turn"
        );

        wait_for_session_history_text(
            &aggregator,
            "test/agent-a",
            "slow ok",
            Duration::from_secs(5),
        )
        .await
        .expect("background dispatch should still complete and project history");
        let _ = runtime.mob_handle().stop().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn discovered_late_member_session_backfills_without_manual_refresh() -> Result<(), String>
    {
        let runtime = Arc::new(build_empty_runtime("console-aggregator-late-member-test").await);
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator.register_runtime(ConsoleRuntimeRegistration {
            runtime_key: "runtime-late".to_string(),
            runtime: runtime.clone(),
            identity_namespace: "late".to_string(),
            visibility_policy: Arc::new(AllowAllConsoleVisibilityPolicy),
        });

        runtime
            .spawn(SpawnMemberSpec::from_wire(
                "worker".to_string(),
                "agent-late".to_string(),
                Some("You are agent-late.".into()),
                None,
                None,
            ))
            .await
            .expect("late member spawns");
        let session_id = send_message_on_mob_with_mode(
            &runtime.mob_handle(),
            "agent-late",
            ContentInput::Text("hello after registration".to_string()),
            meerkat_core::types::HandlingMode::Queue,
        )
        .await
        .expect("direct member send succeeds");
        wait_for_identity_record(
            &aggregator,
            "late/agent-late",
            Some(session_id.as_str()),
            Duration::from_secs(5),
        )
        .await?;

        wait_for_session_history_text(
            &aggregator,
            "late/agent-late",
            "You are agent-late.",
            Duration::from_secs(5),
        )
        .await?;
        let _ = runtime.mob_handle().stop().await;
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn explicit_empty_identity_query_force_refreshes_past_fresh_watermark()
    -> Result<(), String> {
        let runtime = build_single_member_runtime().await;
        let entry = runtime_entry_for_test("runtime-a", &runtime);
        let resolved = member_sources_for_entry(&entry)
            .await
            .into_iter()
            .find(|candidate| candidate.member.agent_identity.as_str() == "agent-a")
            .expect("agent-a member exists");
        let record = identity_record_for_member(&entry, &resolved.handle, &resolved.member)
            .await
            .expect("identity record exists");
        let session_id = record.session_id.expect("agent-a has a session");
        wait_for_runtime_session_history_text(
            &runtime,
            &session_id,
            "You are agent-a.",
            Duration::from_secs(5),
        )
        .await?;

        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator
            .inner
            .runtimes
            .write()
            .expect("runtime registry")
            .insert("runtime-a".to_string(), entry);
        let watermark_key = session_history_watermark_runtime_key("runtime-a", &session_id);
        aggregator
            .store()
            .record_source_watermark(
                &watermark_key,
                ConsoleFrameSourceKind::SessionHistory,
                &format_session_history_watermark(&session_id, 0, current_time_ms()),
            )
            .await
            .expect("record fresh empty watermark");

        let page = tokio::time::timeout(
            Duration::from_secs(2),
            aggregator.query_timeline(ConsoleTimelineQuery {
                identity: Some("test/agent-a".to_string()),
                limit: 20,
                ..ConsoleTimelineQuery::default()
            }),
        )
        .await
        .expect("explicit identity query should not stall")
        .expect("query succeeds");

        assert!(
            page.frames.iter().any(|frame| {
                frame.source.kind == ConsoleFrameSourceKind::SessionHistory
                    && frame.kind == "user_input"
                    && session_history_content_text(frame).as_deref() == Some("You are agent-a.")
            }),
            "explicit identity query should force-refresh stale/fresh empty watermarks; frames: {:#?}",
            page.frames
        );
        let _ = runtime.mob_handle().stop().await;
        Ok(())
    }

    async fn wait_for_session_history_text(
        aggregator: &MobKitConsoleAggregator,
        identity: &str,
        expected: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let mut observed = Vec::new();
        while Instant::now() < deadline {
            let _ = aggregator.list_identities().await;
            let page = aggregator
                .query_timeline(ConsoleTimelineQuery {
                    identity: Some(identity.to_string()),
                    limit: 20,
                    ..ConsoleTimelineQuery::default()
                })
                .await
                .expect("query timeline");
            observed = page.frames;
            if observed.iter().any(|frame| {
                frame.source.kind == ConsoleFrameSourceKind::SessionHistory
                    && (frame.kind == "user_input" || frame.kind == "interaction_complete")
                    && session_history_content_text(frame).as_deref() == Some(expected)
            }) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        Err(format!(
            "session history text {expected:?} was not backfilled; observed frames: {observed:#?}",
        ))
    }

    async fn wait_for_runtime_session_history_text(
        runtime: &UnifiedRuntime,
        session_id: &str,
        expected: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let mut observed = Vec::new();
        while Instant::now() < deadline {
            let page = runtime
                .mob_runtime()
                .read_session_history(session_id, 0, Some(20))
                .await
                .map_err(|err| err.to_string())?;
            observed = page.messages;
            if observed.iter().enumerate().any(|(idx, message)| {
                let Some(message) = serde_json::to_value(message).ok() else {
                    return false;
                };
                frames_from_session_history_message(
                    "runtime-a",
                    "test/agent-a",
                    session_id,
                    idx,
                    message,
                )
                .iter()
                .any(|frame| {
                    frame.kind == "user_input"
                        && session_history_frame_content_text(frame).as_deref() == Some(expected)
                })
            }) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        Err(format!(
            "runtime session history text {expected:?} was not readable before watermark setup; observed messages: {observed:#?}",
        ))
    }

    async fn wait_for_identity_record(
        aggregator: &MobKitConsoleAggregator,
        identity: &str,
        session_id: Option<&str>,
        timeout: Duration,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let mut observed = Vec::new();
        while Instant::now() < deadline {
            observed = aggregator
                .list_identities()
                .await
                .map_err(|err| err.to_string())?;
            if observed.iter().any(|record| {
                record.identity == identity && record.session_id.as_deref() == session_id
            }) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        Err(format!(
            "identity {identity:?} with session {session_id:?} was not projected; observed identities: {observed:#?}",
        ))
    }

    fn session_history_content_text(frame: &ConsoleFrame) -> Option<String> {
        session_history_payload_text(&frame.payload)
    }

    fn session_history_frame_content_text(frame: &NewConsoleFrame) -> Option<String> {
        session_history_payload_text(&frame.payload)
    }

    fn session_history_payload_text(payload: &Value) -> Option<String> {
        if let Some(text) = payload.get("text").and_then(Value::as_str) {
            return Some(text.to_string());
        }
        if let Some(text) = payload.get("result").and_then(Value::as_str) {
            return Some(text.to_string());
        }
        match payload.get("content")? {
            Value::String(text) => Some(text.clone()),
            Value::Array(blocks) => Some(
                blocks
                    .iter()
                    .filter_map(|block| block.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(""),
            ),
            _ => None,
        }
    }

    #[tokio::test]
    async fn query_timeline_handles_large_store_without_backfill_calls() {
        let store = Arc::new(CountingConsoleLogStore::new());
        let aggregator = MobKitConsoleAggregator::new(store.clone());
        for idx in 0..5_000 {
            store
                .append_if_absent(NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("event-{idx}"),
                    timestamp_ms: idx,
                    runtime_key: "runtime-a".to_string(),
                    identity: "agent-a".to_string(),
                    conversation_id: Some("agent-a".to_string()),
                    session_id: Some("session-a".to_string()),
                    kind: "text_delta".to_string(),
                    status: ConsoleFrameStatus::Delivered,
                    payload: json!({ "delta": idx }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::ConsoleEvent,
                        source_cursor: None,
                    },
                    source_event_id: Some(format!("event-{idx}")),
                    interaction_id: None,
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: None,
                    caused_by_frame_id: None,
                })
                .await
                .expect("append frame");
        }

        let page = aggregator
            .query_timeline(ConsoleTimelineQuery {
                identity: Some("agent-a".to_string()),
                limit: 1_000,
                ..ConsoleTimelineQuery::default()
            })
            .await
            .expect("large query");

        assert_eq!(page.frames.len(), 1_000);
        assert_eq!(store.source_watermark_calls(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn explicit_identity_timeline_query_does_not_resolve_roster_per_frame() {
        let store = Arc::new(CountingConsoleLogStore::new());
        let aggregator = MobKitConsoleAggregator::new(store.clone());
        let (_temp, runtime, _delayed_service) =
            build_stress_runtime(64, Duration::from_millis(0)).await;
        aggregator
            .inner
            .runtimes
            .write()
            .expect("runtime registry")
            .insert(
                "runtime-a".to_string(),
                runtime_entry_for_test("runtime-a", runtime.as_ref()),
            );
        for idx in 0..1_000 {
            store
                .append_if_absent(NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("event-{idx}"),
                    timestamp_ms: idx,
                    runtime_key: "runtime-a".to_string(),
                    identity: "test/agent-0".to_string(),
                    conversation_id: Some("test/agent-0".to_string()),
                    session_id: Some("session-a".to_string()),
                    kind: "text_delta".to_string(),
                    status: ConsoleFrameStatus::Delivered,
                    payload: json!({ "delta": idx }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::ConsoleEvent,
                        source_cursor: None,
                    },
                    source_event_id: Some(format!("event-{idx}")),
                    interaction_id: None,
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: None,
                    caused_by_frame_id: None,
                })
                .await
                .expect("append frame");
        }

        let page = tokio::time::timeout(
            Duration::from_secs(2),
            aggregator.query_timeline(ConsoleTimelineQuery {
                identity: Some("test/agent-0".to_string()),
                limit: 1_000,
                ..ConsoleTimelineQuery::default()
            }),
        )
        .await
        .expect("identity timeline query should not rediscover the roster per frame")
        .expect("timeline query succeeds");

        assert_eq!(page.frames.len(), 1_000);
        assert_eq!(store.source_watermark_calls(), 0);
        let _ = runtime.mob_handle().stop().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn refresh_session_history_parallelizes_slow_member_backfills_at_scale() {
        const MEMBER_COUNT: usize = 32;
        let (_temp, runtime, delayed_service) =
            build_stress_runtime(MEMBER_COUNT, Duration::from_millis(40)).await;
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator
            .inner
            .runtimes
            .write()
            .expect("runtime registry")
            .insert(
                "runtime-stress".to_string(),
                runtime_entry_for_test("runtime-stress", &runtime),
            );

        let started = Instant::now();
        aggregator
            .refresh_session_history()
            .await
            .expect("stress refresh");
        let elapsed = started.elapsed();

        assert!(
            delayed_service.read_calls() >= MEMBER_COUNT,
            "expected at least one history read per member, saw {}",
            delayed_service.read_calls()
        );
        assert!(
            delayed_service.max_active_reads() > 1,
            "session history backfill should fan out instead of reading members serially"
        );
        assert!(
            delayed_service.max_active_reads()
                <= ConsoleAggregatorOptions::default().max_concurrent_session_backfills,
            "session history backfill should respect the default concurrency limit"
        );
        assert!(
            elapsed < Duration::from_millis(600),
            "parallel backfill should be far below serial {}ms path, elapsed: {elapsed:?}",
            MEMBER_COUNT * 40
        );
        let _ = runtime.mob_handle().stop().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn session_history_backfill_respects_configured_concurrency_limit() {
        const MEMBER_COUNT: usize = 16;
        let (_temp, runtime, delayed_service) =
            build_stress_runtime(MEMBER_COUNT, Duration::from_millis(30)).await;
        let aggregator =
            MobKitConsoleAggregator::in_memory_with_options(ConsoleAggregatorOptions {
                max_concurrent_session_backfills: 4,
                ..ConsoleAggregatorOptions::default()
            });
        aggregator
            .inner
            .runtimes
            .write()
            .expect("runtime registry")
            .insert(
                "runtime-stress".to_string(),
                runtime_entry_for_test("runtime-stress", &runtime),
            );

        aggregator
            .refresh_session_history()
            .await
            .expect("stress refresh");

        assert!(
            delayed_service.max_active_reads() <= 4,
            "configured concurrency limit should bound session history reads, saw {}",
            delayed_service.max_active_reads()
        );
        let _ = runtime.mob_handle().stop().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn live_event_burst_reaches_store_while_slow_backfill_is_running() {
        const MEMBER_COUNT: usize = 24;
        const LIVE_EVENT_COUNT: usize = 2_048;
        let (_temp, runtime, _delayed_service) =
            build_stress_runtime(MEMBER_COUNT, Duration::from_millis(200)).await;
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator.register_runtime(ConsoleRuntimeRegistration {
            runtime_key: "runtime-burst".to_string(),
            runtime: runtime.clone(),
            identity_namespace: "stress".to_string(),
            visibility_policy: Arc::new(AllowAllConsoleVisibilityPolicy),
        });
        let console_events = runtime.console_events();
        for idx in 0..LIVE_EVENT_COUNT {
            console_events
                .append(
                    "agent-0",
                    Some("burst-turn".to_string()),
                    "text_delta",
                    json!({ "delta": format!("frame-{idx}") }),
                )
                .await;
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observed = 0;
        while Instant::now() < deadline {
            observed = count_console_event_frames(&aggregator, "stress/agent-0").await;
            if observed >= LIVE_EVENT_COUNT {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        assert_eq!(
            observed, LIVE_EVENT_COUNT,
            "live pump should not drop frames while slow background backfill is running"
        );
        let _ = runtime.mob_handle().stop().await;
    }

    async fn count_console_event_frames(
        aggregator: &MobKitConsoleAggregator,
        identity: &str,
    ) -> usize {
        let mut after = None;
        let mut count = 0;
        loop {
            let page = aggregator
                .query_timeline(ConsoleTimelineQuery {
                    identity: Some(identity.to_string()),
                    after,
                    limit: 1_000,
                    ..ConsoleTimelineQuery::default()
                })
                .await
                .expect("query burst timeline");
            if page.frames.is_empty() {
                break;
            }
            count += page
                .frames
                .iter()
                .filter(|frame| frame.source.kind == ConsoleFrameSourceKind::ConsoleEvent)
                .count();
            after = page.next_cursor;
            if after.is_none() {
                break;
            }
        }
        count
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
    async fn steer_delivery_appends_terminal_control_frame() {
        let aggregator = MobKitConsoleAggregator::in_memory();
        let frame = NewConsoleFrame {
            id: None,
            dedupe_key: "send-steer-1".to_string(),
            timestamp_ms: 1,
            runtime_key: "runtime-a".to_string(),
            identity: "agent-a".to_string(),
            conversation_id: Some("agent-a".to_string()),
            session_id: Some("session-1".to_string()),
            kind: "user_input".to_string(),
            status: ConsoleFrameStatus::Delivered,
            payload: json!({
                "content": "operator steer",
                "handling_mode": "steer",
            }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::Send,
                source_cursor: None,
            },
            source_event_id: None,
            interaction_id: Some("interaction-steer-1".to_string()),
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        };
        let inserted = aggregator
            .store()
            .append_if_absent(frame)
            .await
            .expect("append steer input");

        append_steer_delivery_terminal(&aggregator.inner, &inserted.frame, "interaction-steer-1")
            .await
            .expect("append steer terminal");

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
        let terminal = &page.frames[0];
        assert_eq!(terminal.kind, "interaction_complete");
        assert_eq!(terminal.status, ConsoleFrameStatus::Completed);
        assert_eq!(
            terminal.interaction_id.as_deref(),
            Some("interaction-steer-1")
        );
        assert_eq!(
            terminal.parent_frame_id.as_deref(),
            Some(inserted.frame.id.as_str())
        );
        assert_eq!(
            terminal.payload.get("reason").and_then(Value::as_str),
            Some("steer_delivered")
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

    #[tokio::test]
    async fn history_counterpart_scan_matches_content_block_user_prompts() {
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
            payload: json!({
                "content": [{ "type": "text", "text": "hello from operator" }]
            }),
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

    #[tokio::test]
    async fn history_counterpart_scan_matches_live_tool_results() {
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator
            .store()
            .append_if_absent(NewConsoleFrame {
                id: None,
                dedupe_key: "live-tool-result".to_string(),
                timestamp_ms: 2_000,
                runtime_key: "runtime-a".to_string(),
                identity: "agent-a".to_string(),
                conversation_id: Some("agent-a".to_string()),
                session_id: Some("session-a".to_string()),
                kind: "tool_execution_completed".to_string(),
                status: ConsoleFrameStatus::Delivered,
                payload: json!({
                    "id": "call-1",
                    "tool_call_id": "call-1",
                    "result": "{ \"count\": 70 }"
                }),
                source: ConsoleFrameSource {
                    kind: ConsoleFrameSourceKind::ConsoleEvent,
                    source_cursor: None,
                },
                source_event_id: Some("live-tool-result".to_string()),
                interaction_id: None,
                turn_id: None,
                run_id: None,
                parent_frame_id: None,
                caused_by_frame_id: None,
            })
            .await
            .expect("append live tool result");

        let history = NewConsoleFrame {
            id: None,
            dedupe_key: "history-tool-result".to_string(),
            timestamp_ms: 3_000,
            runtime_key: "runtime-a".to_string(),
            identity: "agent-a".to_string(),
            conversation_id: Some("agent-a".to_string()),
            session_id: Some("session-a".to_string()),
            kind: "tool_execution_completed".to_string(),
            status: ConsoleFrameStatus::Completed,
            payload: json!({
                "id": "call-1",
                "tool_call_id": "call-1",
                "result": "{ \"count\": 70 }",
                "content": [{ "type": "text", "text": "{ \"count\": 70 }" }]
            }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::SessionHistory,
                source_cursor: Some("session-a:3:0".to_string()),
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
    async fn history_counterpart_scan_matches_streamed_text_delta_completion() {
        let aggregator = MobKitConsoleAggregator::in_memory();
        for (idx, delta) in ["Ready ", "and standing by."].iter().enumerate() {
            aggregator
                .store()
                .append_if_absent(NewConsoleFrame {
                    id: None,
                    dedupe_key: format!("live-delta-{idx}"),
                    timestamp_ms: 2_000 + idx as u64,
                    runtime_key: "runtime-a".to_string(),
                    identity: "agent-a".to_string(),
                    conversation_id: Some("agent-a".to_string()),
                    session_id: Some("session-a".to_string()),
                    kind: "text_delta".to_string(),
                    status: ConsoleFrameStatus::Delivered,
                    payload: json!({ "delta": delta }),
                    source: ConsoleFrameSource {
                        kind: ConsoleFrameSourceKind::ConsoleEvent,
                        source_cursor: None,
                    },
                    source_event_id: Some(format!("live-delta-{idx}")),
                    interaction_id: Some("turn-a".to_string()),
                    turn_id: None,
                    run_id: None,
                    parent_frame_id: None,
                    caused_by_frame_id: None,
                })
                .await
                .expect("append live delta");
        }

        let history = NewConsoleFrame {
            id: None,
            dedupe_key: "history-assistant-complete".to_string(),
            timestamp_ms: 3_000,
            runtime_key: "runtime-a".to_string(),
            identity: "agent-a".to_string(),
            conversation_id: Some("agent-a".to_string()),
            session_id: Some("session-a".to_string()),
            kind: "interaction_complete".to_string(),
            status: ConsoleFrameStatus::Completed,
            payload: json!({ "result": "Ready and standing by." }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::SessionHistory,
                source_cursor: Some("session-a:3".to_string()),
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
    fn session_history_watermarks_are_cursor_and_ttl_aware() {
        let legacy = "session:with:colon:42";
        let checked = format_session_history_watermark("session:with:colon", 43, 1_000);
        let empty_checked = format_session_history_watermark("session:with:colon", 0, 1_000);

        assert_eq!(
            parse_session_history_watermark(legacy, "session:with:colon"),
            Some(42)
        );
        assert_eq!(
            parse_session_history_watermark(&checked, "session:with:colon"),
            Some(43)
        );
        assert!(session_history_watermark_is_fresh(
            &checked,
            "session:with:colon",
            1_500
        ));
        assert!(!session_history_watermark_is_fresh(
            &checked,
            "session:with:colon",
            1_000 + SESSION_HISTORY_GROWING_REFRESH_TTL_MS + 1
        ));
        assert!(session_history_watermark_is_fresh(
            &empty_checked,
            "session:with:colon",
            1_500
        ));
        assert!(!session_history_watermark_is_fresh(
            &empty_checked,
            "session:with:colon",
            1_000 + SESSION_HISTORY_REFRESH_TTL_MS + 1
        ));
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
                "content": "hi there",
                "stop_reason": "end_turn",
                "usage": { "input_tokens": 1, "output_tokens": 1, "total_tokens": 2 },
                "timestamp_ms": 11
            }),
        )
        .expect("assistant history frame");

        assert_eq!(user.kind, "user_input");
        assert_eq!(user.source.kind, ConsoleFrameSourceKind::SessionHistory);
        assert_eq!(
            user.payload["content"],
            json!([{ "type": "text", "text": "hello" }])
        );
        assert_eq!(assistant.kind, "interaction_complete");
        assert_eq!(assistant.payload["text"], json!("hi there"));
        assert!(
            assistant
                .dedupe_key
                .starts_with("session-history:runtime-a:session-a:1:")
        );
    }

    #[test]
    fn session_history_projection_filters_scaffold_user_messages() {
        let spawn_notice = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            0,
            json!({
                "role": "user",
                "content": "You have been spawned as 'agent-a' (role: worker) in mob 'mob-a'.",
                "timestamp_ms": 10
            }),
        );
        let peer_update = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            1,
            json!({
                "role": "user",
                "content": [{ "type": "text", "text": "[PEER UPDATE] alpha wired to beta" }],
                "timestamp_ms": 11
            }),
        );
        let real_user = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            2,
            json!({
                "role": "user",
                "content": "Please review the incident notes.",
                "timestamp_ms": 12
            }),
        );

        assert!(spawn_notice.is_none());
        assert!(peer_update.is_none());
        assert!(real_user.is_some());
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
                "role": "block_assistant",
                "blocks": [
                    { "block_type": "text", "data": { "text": "hello " } },
                    { "block_type": "text", "data": { "text": "there" } }
                ],
                "stop_reason": "end_turn"
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
                "stop_reason": "end_turn",
                "created_at": "1970-01-01T00:00:00.010Z"
            }),
        )
        .expect("assistant block history frame");

        assert_eq!(frame.kind, "interaction_complete");
        assert_eq!(frame.payload["result"], json!("Ready and standing by."));
    }

    #[test]
    fn session_history_projection_drops_reasoning_blocks_from_result_text() {
        let frame = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            0,
            json!({
                "role": "block_assistant",
                "blocks": [
                    {
                        "block_type": "reasoning",
                        "data": { "text": "**Planning**\nI should not be rendered." }
                    },
                    {
                        "block_type": "text",
                        "data": { "text": "Visible answer." }
                    }
                ],
                "stop_reason": "end_turn",
                "created_at": "1970-01-01T00:00:00.010Z"
            }),
        )
        .expect("assistant block history frame");

        assert_eq!(frame.kind, "interaction_complete");
        assert_eq!(frame.payload["result"], json!("Visible answer."));
        assert_eq!(frame.payload["text"], json!("Visible answer."));
    }

    #[test]
    fn session_history_projection_leaves_reasoning_only_result_empty() {
        let frame = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            0,
            json!({
                "role": "block_assistant",
                "blocks": [
                    {
                        "block_type": "reasoning",
                        "data": { "text": "Private planning text." }
                    },
                    {
                        "block_type": "tool_use",
                        "data": { "id": "toolu-1", "name": "peers", "args": {} }
                    }
                ],
                "stop_reason": "end_turn",
                "created_at": "1970-01-01T00:00:00.010Z"
            }),
        )
        .expect("assistant block history frame");

        assert_eq!(frame.kind, "interaction_complete");
        assert_eq!(frame.payload["result"], json!(""));
        assert_eq!(frame.payload["text"], json!(""));
    }

    #[test]
    fn session_history_projection_preserves_tool_results() {
        let frames = frames_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            5,
            json!({
                "role": "tool_results",
                "results": [
                    {
                        "tool_use_id": "call-peers",
                        "content": "{\"peers\":[{\"peer_id\":\"peer-1\",\"name\":\"mob/worker/peer-1\"}]}",
                        "is_error": false
                    }
                ],
                "created_at": "1970-01-01T00:00:00.050Z"
            }),
        );

        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(frame.kind, "tool_execution_completed");
        assert_eq!(frame.payload["tool_call_id"], json!("call-peers"));
        assert_eq!(
            frame.payload["result"],
            json!("{\"peers\":[{\"peer_id\":\"peer-1\",\"name\":\"mob/worker/peer-1\"}]}")
        );
        assert_eq!(frame.source.kind, ConsoleFrameSourceKind::SessionHistory);
        assert_eq!(frame.timestamp_ms, 50);
    }

    #[test]
    fn session_history_projection_surfaces_spawn_initial_message_on_child_timeline() {
        let frames = frames_from_session_history_message(
            "runtime-a",
            "review:singleton",
            "session-a",
            7,
            json!({
                "role": "block_assistant",
                "blocks": [
                    {
                        "block_type": "tool_use",
                        "data": {
                            "id": "call-spawn",
                            "name": "mob_spawn_member",
                            "args": {
                                "member_id": "review-worker-alpha",
                                "profile": "review-worker",
                                "initial_message": "Review Initiative Alpha and save the result."
                            }
                        }
                    }
                ],
                "stop_reason": "tool_use",
                "created_at": "1970-01-01T00:00:00.070Z"
            }),
        );

        assert_eq!(frames.len(), 2);
        let child = frames
            .iter()
            .find(|frame| frame.identity == "review-worker-alpha")
            .expect("spawned member initial message frame");
        assert_eq!(child.kind, "user_input");
        assert_eq!(child.timestamp_ms, 70);
        assert_eq!(
            child.payload["message"]["content"],
            "Review Initiative Alpha and save the result."
        );
        assert_eq!(child.payload["parent_identity"], "review:singleton");
        assert_eq!(child.payload["via_tool"], "mob_spawn_member");
        assert_eq!(child.source.kind, ConsoleFrameSourceKind::SessionHistory);
    }

    #[test]
    fn session_history_projection_surfaces_specs_spawn_initial_messages() {
        let frames = frames_from_session_history_message(
            "runtime-a",
            "review:singleton",
            "session-a",
            8,
            json!({
                "role": "block_assistant",
                "blocks": [
                    {
                        "block_type": "tool_use",
                        "data": {
                            "id": "call-spawn-many",
                            "name": "mob_spawn_member",
                            "args": {
                                "specs": [
                                    {
                                        "agent_identity": "review-worker-alpha",
                                        "profile": "review-worker",
                                        "initial_message": "Review Initiative Alpha."
                                    },
                                    {
                                        "agent_identity": "review-worker-beta",
                                        "profile": "review-worker",
                                        "initial_message": "Review Initiative Beta."
                                    }
                                ]
                            }
                        }
                    }
                ],
                "stop_reason": "tool_use",
                "created_at": "1970-01-01T00:00:00.080Z"
            }),
        );

        assert_eq!(frames.len(), 3);
        let child_messages = frames
            .iter()
            .filter(|frame| frame.kind == "user_input")
            .map(|frame| {
                (
                    frame.identity.as_str(),
                    frame.payload["message"]["content"]
                        .as_str()
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            child_messages,
            vec![
                ("review-worker-alpha", "Review Initiative Alpha."),
                ("review-worker-beta", "Review Initiative Beta."),
            ]
        );
    }

    #[tokio::test]
    async fn query_timeline_surfaces_spawn_initial_message_on_child_identity_only() {
        let aggregator = MobKitConsoleAggregator::in_memory();
        let frames = frames_from_session_history_message(
            "runtime-a",
            "review:singleton",
            "session-a",
            9,
            json!({
                "role": "block_assistant",
                "blocks": [
                    {
                        "block_type": "tool_use",
                        "data": {
                            "id": "call-spawn-many",
                            "name": "mob_spawn_member",
                            "args": {
                                "specs": [
                                    {
                                        "agent_identity": "review-worker-alpha",
                                        "profile": "review-worker",
                                        "initial_message": "Review Initiative Alpha."
                                    },
                                    {
                                        "agent_identity": "review-worker-beta",
                                        "profile": "review-worker",
                                        "initial_message": "Review Initiative Beta."
                                    }
                                ]
                            }
                        }
                    }
                ],
                "stop_reason": "tool_use",
                "created_at": "1970-01-01T00:00:00.090Z"
            }),
        );
        for frame in frames {
            aggregator
                .store()
                .append_if_absent(frame)
                .await
                .expect("append projected session-history frame");
        }

        let child_page = aggregator
            .query_timeline(ConsoleTimelineQuery {
                identity: Some("review-worker-alpha".to_string()),
                limit: 20,
                ..ConsoleTimelineQuery::default()
            })
            .await
            .expect("query child timeline");
        assert_eq!(child_page.frames.len(), 1);
        assert_eq!(child_page.frames[0].kind, "user_input");
        assert_eq!(
            child_page.frames[0].payload["message"]["content"],
            "Review Initiative Alpha."
        );
        assert_eq!(
            child_page.frames[0].payload["parent_identity"],
            "review:singleton"
        );

        let parent_page = aggregator
            .query_timeline(ConsoleTimelineQuery {
                identity: Some("review:singleton".to_string()),
                limit: 20,
                ..ConsoleTimelineQuery::default()
            })
            .await
            .expect("query parent timeline");
        assert!(
            parent_page
                .frames
                .iter()
                .all(|frame| frame.identity == "review:singleton"),
            "child initial messages must not leak into parent timeline: {:#?}",
            parent_page.frames
        );
    }

    #[test]
    fn session_history_projection_uses_rfc3339_created_at_timestamp() {
        let frame = frame_from_session_history_message(
            "runtime-a",
            "agent-a",
            "session-a",
            0,
            json!({
                "role": "user",
                "content": "hello",
                "created_at": "2026-05-12T05:00:06.564227Z"
            }),
        )
        .expect("user history frame");

        assert_eq!(frame.timestamp_ms, 1_778_562_006_564);
    }
}
