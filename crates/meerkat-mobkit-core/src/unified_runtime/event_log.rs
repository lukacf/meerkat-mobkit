//! Persistent operational event log with buffered ingestion and pluggable storage.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::types::{EventEnvelope, UnifiedEvent};

/// Boxed error type returned by [`EventLogStore`] methods.
pub type EventLogError = Box<dyn std::error::Error + Send>;

/// Optional event filter predicate.
type EventFilter = Box<dyn Fn(&UnifiedEvent) -> bool + Send + Sync>;

// ---------------------------------------------------------------------------
// Persisted event model
// ---------------------------------------------------------------------------

/// A persisted operational event with monotonic ordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedEvent {
    /// Unique event ID (from the original event envelope).
    pub id: String,
    /// Monotonic sequence number assigned at ingestion time.
    /// Deterministic ordering within and across batches.
    pub seq: u64,
    /// Millisecond timestamp from the original event.
    pub timestamp_ms: u64,
    /// Member/agent ID. `None` for module events.
    pub member_id: Option<String>,
    /// The full event payload.
    pub event: UnifiedEvent,
}

// ---------------------------------------------------------------------------
// Query model
// ---------------------------------------------------------------------------

/// Query parameters for historical event retrieval.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventQuery {
    /// Only events after this timestamp (inclusive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<u64>,
    /// Only events before this timestamp (exclusive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_ms: Option<u64>,
    /// Filter to events from a specific member/agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    /// Filter to specific event types (e.g. "run_completed", "run_failed").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_types: Vec<String>,
    /// Maximum number of events to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Resume after this sequence number (for pagination).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<u64>,
}

// ---------------------------------------------------------------------------
// Storage trait
// ---------------------------------------------------------------------------

/// Trait for persisting and querying operational events.
///
/// MobKit defines the contract; apps provide the implementation for their
/// storage backend (BigQuery, Postgres, SQLite, in-memory, etc.).
///
/// Same pattern as `Discovery`, `EdgeDiscovery`, `SessionAgentBuilder`.
pub trait EventLogStore: Send + Sync {
    /// Persist a batch of events. Called periodically by the ingestion engine.
    ///
    /// Must be idempotent — duplicate events (same `id`) should be ignored.
    /// Failures are logged via the error hook but never block agent execution.
    fn append_batch(
        &self,
        events: Vec<PersistedEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<(), EventLogError>> + Send + '_>>;

    /// Query historical events matching the given criteria.
    fn query(
        &self,
        query: EventQuery,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PersistedEvent>, EventLogError>> + Send + '_>>;
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the event log ingestion engine.
pub struct EventLogConfig {
    /// App-provided storage backend.
    pub store: Box<dyn EventLogStore>,
    /// Optional filter — return `true` to persist, `false` to skip.
    /// If `None`, all events are persisted.
    pub filter: Option<EventFilter>,
    /// Number of events to buffer before flushing to storage.
    /// Default: 64.
    pub batch_size: usize,
    /// Maximum time between flushes, even if batch is not full.
    /// Default: 1 second.
    pub flush_interval: Duration,
}

impl Default for EventLogConfig {
    fn default() -> Self {
        Self {
            store: Box::new(NullEventLogStore),
            filter: None,
            batch_size: 64,
            flush_interval: Duration::from_secs(1),
        }
    }
}

/// No-op store for when event logging is not configured.
struct NullEventLogStore;

impl EventLogStore for NullEventLogStore {
    fn append_batch(
        &self,
        _events: Vec<PersistedEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<(), EventLogError>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn query(
        &self,
        _query: EventQuery,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PersistedEvent>, EventLogError>> + Send + '_>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

// ---------------------------------------------------------------------------
// Ingestion engine
// ---------------------------------------------------------------------------

/// Shared handle to the event log, held by UnifiedRuntime.
pub(crate) struct EventLogHandle {
    store: Arc<dyn EventLogStore>,
    /// Sender for the ingestion buffer. Events are sent here and flushed
    /// in batches by a background task.
    ingress_tx: mpsc::Sender<EventEnvelope<UnifiedEvent>>,
}

impl EventLogHandle {
    /// Query the underlying store.
    pub async fn query(&self, query: EventQuery) -> Result<Vec<PersistedEvent>, EventLogError> {
        self.store.query(query).await
    }

    /// Ingest an event into the log (non-blocking, buffered).
    pub fn ingest(&self, event: EventEnvelope<UnifiedEvent>) {
        // Non-blocking: drop if the buffer is full (backpressure protection)
        let _ = self.ingress_tx.try_send(event);
    }
}

/// Start the event log ingestion engine. Returns a handle for the runtime
/// and spawns a background flush task.
pub(crate) fn start_event_log(
    config: EventLogConfig,
    error_hook: Option<super::ErrorHook>,
) -> EventLogHandle {
    let store: Arc<dyn EventLogStore> = Arc::from(config.store);
    let seq = Arc::new(AtomicU64::new(1));
    // Buffer capacity: 4x batch size to absorb bursts
    let (ingress_tx, ingress_rx) = mpsc::channel(config.batch_size * 4);

    let handle = EventLogHandle {
        store: store.clone(),
        ingress_tx,
    };

    tokio::spawn(run_flush_loop(
        ingress_rx,
        store,
        seq,
        config.filter,
        config.batch_size,
        config.flush_interval,
        error_hook,
    ));

    handle
}

async fn run_flush_loop(
    mut rx: mpsc::Receiver<EventEnvelope<UnifiedEvent>>,
    store: Arc<dyn EventLogStore>,
    seq: Arc<AtomicU64>,
    filter: Option<EventFilter>,
    batch_size: usize,
    flush_interval: Duration,
    error_hook: Option<super::ErrorHook>,
) {
    let mut batch: Vec<PersistedEvent> = Vec::with_capacity(batch_size);
    let mut interval = tokio::time::interval(flush_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            maybe_event = rx.recv() => {
                match maybe_event {
                    Some(envelope) => {
                        if let Some(ref f) = filter {
                            if !f(&envelope.event) {
                                continue;
                            }
                        }
                        let persisted = to_persisted(&seq, &envelope);
                        batch.push(persisted);
                        if batch.len() >= batch_size {
                            flush_batch(&store, &mut batch, &error_hook).await;
                        }
                    }
                    None => {
                        // Channel closed — flush remaining and exit
                        if !batch.is_empty() {
                            flush_batch(&store, &mut batch, &error_hook).await;
                        }
                        break;
                    }
                }
            }
            _ = interval.tick() => {
                if !batch.is_empty() {
                    flush_batch(&store, &mut batch, &error_hook).await;
                }
            }
        }
    }
}

fn to_persisted(seq: &AtomicU64, envelope: &EventEnvelope<UnifiedEvent>) -> PersistedEvent {
    let member_id = match &envelope.event {
        UnifiedEvent::Agent { agent_id, .. } => Some(agent_id.clone()),
        UnifiedEvent::Module(_) => None,
    };
    PersistedEvent {
        id: envelope.event_id.clone(),
        seq: seq.fetch_add(1, Ordering::Relaxed),
        timestamp_ms: envelope.timestamp_ms,
        member_id,
        event: envelope.event.clone(),
    }
}

async fn flush_batch(
    store: &Arc<dyn EventLogStore>,
    batch: &mut Vec<PersistedEvent>,
    error_hook: &Option<super::ErrorHook>,
) {
    let events = std::mem::take(batch);
    if let Err(err) = store.append_batch(events).await {
        // Fire-and-forget error reporting via the error hook
        if let Some(hook) = error_hook {
            let hook = hook.clone();
            let msg = format!("event log flush failed: {err}");
            tokio::spawn(async move {
                let _ = hook(super::types::ErrorEvent::EventLogFlushFailure { error: msg }).await;
            });
        }
    }
}
