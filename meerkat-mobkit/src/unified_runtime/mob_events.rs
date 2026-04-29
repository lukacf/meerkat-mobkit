//! In-memory store for structural mob events projected from
//! `meerkat_mob::AttributedEvent` / `MobEventKind`.
//!
//! Mirrors the deque-backed replay shape of [`super::console_events`]: a
//! per-mob ring buffer + global ring buffer, a broadcast channel for
//! live SSE subscribers, and a `query()` filtered by [`EventQuery`].
//!
//! Unlike `ConsoleEventStore`, the structural surface preserves `mob_id`,
//! `run_id`, `step_id`, and `agent_identity` from the originating
//! `MobEventKind` variant so downstream consumers can reconstruct flow
//! topology without going back to the lossy `UnifiedEvent::Agent`
//! projection.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use meerkat_mob::event::{AttributedEvent, MobEvent, MobEventKind};
use serde_json::Value;
use tokio::sync::{RwLock, broadcast};

use crate::runtime::{MetadataScope, RuntimeMetadataTable};
use crate::types::MobStructuralEventEnvelope;
use crate::unified_runtime::EventQuery;

/// Maximum events retained in the global replay buffer.
pub(crate) const MOB_EVENTS_REPLAY_CAP: usize = 4096;
/// Maximum events retained per mob_id replay buffer.
const MOB_EVENTS_PER_MOB_CAP: usize = 1024;
/// Capacity of the broadcast channel used by SSE subscribers.
const MOB_EVENTS_CHANNEL_CAP: usize = 512;

/// Deque-backed replay store for structural mob events.
///
/// Public so integration tests can construct one directly. Internal
/// callers should obtain the runtime's store via
/// [`crate::unified_runtime::UnifiedRuntime::query_mob_events`] /
/// [`crate::unified_runtime::UnifiedRuntime::subscribe_mob_events`].
#[derive(Clone)]
pub struct MobEventsStore {
    next_cursor: Arc<AtomicU64>,
    state: Arc<RwLock<MobEventsState>>,
    event_tx: broadcast::Sender<MobStructuralEventEnvelope>,
    metadata_table: Option<Arc<RuntimeMetadataTable>>,
}

impl Default for MobEventsStore {
    fn default() -> Self {
        Self::new()
    }
}

struct MobEventsState {
    all_events: VecDeque<MobStructuralEventEnvelope>,
    by_mob: BTreeMap<String, VecDeque<MobStructuralEventEnvelope>>,
}

impl MobEventsStore {
    /// Create an empty in-memory store with no label provider attached.
    /// Events projected through this store carry empty `mob_labels` /
    /// `run_labels`. Use [`Self::with_metadata_table`] to wire in the
    /// runtime's `RuntimeMetadataTable` so structural events are
    /// label-enriched at projection time.
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(MOB_EVENTS_CHANNEL_CAP);
        Self {
            next_cursor: Arc::new(AtomicU64::new(1)),
            state: Arc::new(RwLock::new(MobEventsState {
                all_events: VecDeque::new(),
                by_mob: BTreeMap::new(),
            })),
            event_tx,
            metadata_table: None,
        }
    }

    /// Wire a label provider into the store. After this, every projected
    /// structural envelope is enriched with the matching `mob_labels` and
    /// (when the event has a `run_id`) `run_labels` snapshotted at
    /// projection time. Returns the same store with the table attached so
    /// callers can chain.
    #[must_use]
    pub fn with_metadata_table(mut self, table: Arc<RuntimeMetadataTable>) -> Self {
        self.metadata_table = Some(table);
        self
    }

    /// Subscribe to live structural mob events.
    pub fn subscribe(&self) -> broadcast::Receiver<MobStructuralEventEnvelope> {
        self.event_tx.subscribe()
    }

    /// Project an [`AttributedEvent`] into a structural envelope. The
    /// `AttributedEvent` carries an agent-level `EventEnvelope`; we use it
    /// only for the agent identity and timestamp fallback. The bulk of the
    /// structural fields come from the corresponding [`MobEvent`] (see
    /// [`Self::project_mob_event`]).
    ///
    /// Returns `None` because attributed agent events on their own do not
    /// have structural mob fields — only the `MobEvent` stream does. Kept
    /// here so callers wiring both streams have a symmetric API.
    pub async fn project_attributed_event(
        &self,
        _event: &AttributedEvent,
    ) -> Option<MobStructuralEventEnvelope> {
        // Attributed agent events do not carry structural mob fields. The
        // structural projection is driven by `project_mob_event` below.
        None
    }

    /// Project a [`MobEvent`] into a structural envelope and record it.
    /// When a `RuntimeMetadataTable` is attached (see
    /// [`Self::with_metadata_table`]), the envelope's `mob_labels` and
    /// `run_labels` are populated with snapshots taken at projection time.
    pub async fn project_mob_event(&self, event: &MobEvent) -> MobStructuralEventEnvelope {
        let cursor = self.next_cursor.fetch_add(1, Ordering::Relaxed);
        let mob_id = event.mob_id.as_str().to_string();
        let timestamp_ms = event.timestamp.timestamp_millis().max(0) as u64;
        let kind = event_kind_label(&event.kind).to_string();
        let (run_id, step_id, agent_identity) = extract_structural_fields(&event.kind);
        let data = serde_json::to_value(&event.kind).unwrap_or(Value::Null);
        let (mob_labels, run_labels) = self.lookup_labels(&mob_id, run_id.as_deref()).await;
        let envelope = MobStructuralEventEnvelope {
            event_id: format!("mob-evt-{cursor}"),
            cursor,
            mob_id,
            timestamp_ms,
            kind,
            run_id,
            step_id,
            agent_identity,
            mob_labels,
            run_labels,
            data,
        };
        self.append_envelope(envelope.clone()).await;
        envelope
    }

    async fn lookup_labels(
        &self,
        mob_id: &str,
        run_id: Option<&str>,
    ) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let Some(table) = &self.metadata_table else {
            return (BTreeMap::new(), BTreeMap::new());
        };
        let mob_labels = table
            .get_labels(&MetadataScope::Mob(mob_id.to_string()))
            .await;
        let run_labels = match run_id {
            Some(run_id) => {
                table
                    .get_labels(&MetadataScope::Run(mob_id.to_string(), run_id.to_string()))
                    .await
            }
            None => BTreeMap::new(),
        };
        (mob_labels, run_labels)
    }

    async fn append_envelope(&self, envelope: MobStructuralEventEnvelope) {
        {
            let mut state = self.state.write().await;
            state.all_events.push_back(envelope.clone());
            trim_deque(&mut state.all_events, MOB_EVENTS_REPLAY_CAP);
            let per_mob = state
                .by_mob
                .entry(envelope.mob_id.clone())
                .or_insert_with(VecDeque::new);
            per_mob.push_back(envelope.clone());
            trim_deque(per_mob, MOB_EVENTS_PER_MOB_CAP);
        }
        let _ = self.event_tx.send(envelope);
    }

    /// Filter retained events by the given [`EventQuery`]. Returns events
    /// in cursor-ascending order. Honors `mob_id`, `run_id`, `step_id`,
    /// `event_types`, `since_ms`, `until_ms`, `after_seq` (exclusive),
    /// `limit`. `member_id`/`identity` map to `agent_identity`.
    pub async fn query(&self, query: &EventQuery) -> Vec<MobStructuralEventEnvelope> {
        let state = self.state.read().await;
        let source: Vec<MobStructuralEventEnvelope> = match query.mob_id.as_deref() {
            Some(mob_id) => state
                .by_mob
                .get(mob_id)
                .cloned()
                .unwrap_or_else(VecDeque::new)
                .into_iter()
                .collect(),
            None => state.all_events.iter().cloned().collect(),
        };
        let mut events = source;
        if let Some(after) = query.after_seq {
            events.retain(|event| event.cursor > after);
        }
        if let Some(since) = query.since_ms {
            events.retain(|event| event.timestamp_ms >= since);
        }
        if let Some(until) = query.until_ms {
            events.retain(|event| event.timestamp_ms < until);
        }
        if let Some(run_id) = query.run_id.as_deref() {
            events.retain(|event| event.run_id.as_deref() == Some(run_id));
        }
        if let Some(step_id) = query.step_id.as_deref() {
            events.retain(|event| event.step_id.as_deref() == Some(step_id));
        }
        let identity_filter = query.identity.as_deref().or(query.member_id.as_deref());
        if let Some(identity) = identity_filter {
            events.retain(|event| event.agent_identity.as_deref() == Some(identity));
        }
        if !query.event_types.is_empty() {
            events.retain(|event| query.event_types.iter().any(|ty| ty == &event.kind));
        }
        if let Some(limit) = query.limit
            && events.len() > limit
        {
            // Tail-window: keep the most recent `limit` events to match
            // ConsoleEventStore::query semantics.
            let start = events.len().saturating_sub(limit);
            events = events.split_off(start);
        }
        events
    }

    /// Replay events for SSE catchup, optionally resuming after a
    /// last-seen `event_id`. If the checkpoint is unknown, fall back to
    /// the full retained window.
    #[allow(dead_code)]
    pub(crate) async fn replay_all(
        &self,
        last_event_id: Option<&str>,
    ) -> Vec<MobStructuralEventEnvelope> {
        let state = self.state.read().await;
        replay_slice(state.all_events.iter().cloned().collect(), last_event_id)
    }
}

fn trim_deque(deque: &mut VecDeque<MobStructuralEventEnvelope>, cap: usize) {
    while deque.len() > cap {
        deque.pop_front();
    }
}

fn replay_slice(
    events: Vec<MobStructuralEventEnvelope>,
    last_event_id: Option<&str>,
) -> Vec<MobStructuralEventEnvelope> {
    let Some(last_event_id) = last_event_id.filter(|value| !value.trim().is_empty()) else {
        return events;
    };
    if let Some(idx) = events
        .iter()
        .position(|event| event.event_id == last_event_id)
    {
        // Inclusive replay so clients can dedup by event_id.
        return events[idx..].to_vec();
    }
    events
}

/// Snake-case label for a `MobEventKind` matching the `serde(tag="type",
/// rename_all="snake_case")` wire form.
fn event_kind_label(kind: &MobEventKind) -> &'static str {
    match kind {
        MobEventKind::MobCreated { .. } => "mob_created",
        MobEventKind::MobCompleted => "mob_completed",
        MobEventKind::MobReset => "mob_reset",
        MobEventKind::MemberSpawned(_) => "member_spawned",
        MobEventKind::MemberRetired { .. } => "member_retired",
        MobEventKind::MemberReset { .. } => "member_reset",
        MobEventKind::MemberKickoffUpdated { .. } => "member_kickoff_updated",
        MobEventKind::MembersWired { .. } => "members_wired",
        MobEventKind::MembersUnwired { .. } => "members_unwired",
        MobEventKind::ExternalPeerWired { .. } => "external_peer_wired",
        MobEventKind::ExternalPeerUnwired { .. } => "external_peer_unwired",
        MobEventKind::TaskCreated { .. } => "task_created",
        MobEventKind::TaskUpdated { .. } => "task_updated",
        MobEventKind::FlowStarted { .. } => "flow_started",
        MobEventKind::FlowCompleted { .. } => "flow_completed",
        MobEventKind::FlowFailed { .. } => "flow_failed",
        MobEventKind::FlowCanceled { .. } => "flow_canceled",
        MobEventKind::StepDispatched { .. } => "step_dispatched",
        MobEventKind::StepTargetCompleted { .. } => "step_target_completed",
        MobEventKind::StepTargetFailed { .. } => "step_target_failed",
        MobEventKind::StepCompleted { .. } => "step_completed",
        MobEventKind::StepFailed { .. } => "step_failed",
        MobEventKind::StepSkipped { .. } => "step_skipped",
        MobEventKind::TopologyViolation { .. } => "topology_violation",
        MobEventKind::SupervisorEscalation { .. } => "supervisor_escalation",
        MobEventKind::OperatorActionRecorded { .. } => "operator_action_recorded",
    }
}

/// Pull `(run_id, step_id, agent_identity)` out of variants that carry
/// them. Variants without a given field return `None` for that slot.
fn extract_structural_fields(
    kind: &MobEventKind,
) -> (Option<String>, Option<String>, Option<String>) {
    match kind {
        MobEventKind::FlowStarted { run_id, .. }
        | MobEventKind::FlowCompleted { run_id, .. }
        | MobEventKind::FlowFailed { run_id, .. }
        | MobEventKind::FlowCanceled { run_id, .. } => (Some(run_id.to_string()), None, None),
        MobEventKind::StepDispatched {
            run_id,
            step_id,
            target,
        }
        | MobEventKind::StepTargetCompleted {
            run_id,
            step_id,
            target,
        } => (
            Some(run_id.to_string()),
            Some(step_id.as_str().to_string()),
            Some(target.identity.as_str().to_string()),
        ),
        MobEventKind::StepTargetFailed {
            run_id,
            step_id,
            target,
            ..
        } => (
            Some(run_id.to_string()),
            Some(step_id.as_str().to_string()),
            Some(target.identity.as_str().to_string()),
        ),
        MobEventKind::StepCompleted { run_id, step_id }
        | MobEventKind::StepFailed {
            run_id, step_id, ..
        }
        | MobEventKind::StepSkipped {
            run_id, step_id, ..
        } => (
            Some(run_id.to_string()),
            Some(step_id.as_str().to_string()),
            None,
        ),
        MobEventKind::SupervisorEscalation {
            run_id,
            step_id,
            escalated_to,
        } => (
            Some(run_id.to_string()),
            Some(step_id.as_str().to_string()),
            Some(escalated_to.as_str().to_string()),
        ),
        MobEventKind::MemberSpawned(event) => {
            (None, None, Some(event.agent_identity.as_str().to_string()))
        }
        MobEventKind::MemberRetired { agent_identity, .. }
        | MobEventKind::MemberReset { agent_identity, .. } => {
            (None, None, Some(agent_identity.as_str().to_string()))
        }
        MobEventKind::MemberKickoffUpdated { member, .. } => {
            (None, None, Some(member.as_str().to_string()))
        }
        MobEventKind::ExternalPeerWired { local, .. }
        | MobEventKind::ExternalPeerUnwired { local, .. } => {
            (None, None, Some(local.as_str().to_string()))
        }
        MobEventKind::TaskUpdated { owner, .. } => {
            (None, None, owner.as_ref().map(|v| v.as_str().to_string()))
        }
        // Variants without flow / step / identity context.
        MobEventKind::MobCreated { .. }
        | MobEventKind::MobCompleted
        | MobEventKind::MobReset
        | MobEventKind::MembersWired { .. }
        | MobEventKind::MembersUnwired { .. }
        | MobEventKind::TaskCreated { .. }
        | MobEventKind::TopologyViolation { .. }
        | MobEventKind::OperatorActionRecorded { .. } => (None, None, None),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use chrono::Utc;
    use meerkat_mob::event::MemberSpawnedEvent;
    use meerkat_mob::ids::{
        AgentIdentity, AgentRuntimeId, FenceToken, FlowId, Generation, MobId, ProfileName, RunId,
        StepId,
    };

    fn mob_event(kind: MobEventKind) -> MobEvent {
        MobEvent {
            cursor: 0,
            timestamp: Utc::now(),
            mob_id: MobId::from("test-mob"),
            kind,
        }
    }

    #[tokio::test]
    async fn projects_flow_started_with_run_id() {
        let store = MobEventsStore::new();
        let run_id = RunId::new();
        let envelope = store
            .project_mob_event(&mob_event(MobEventKind::FlowStarted {
                run_id: run_id.clone(),
                flow_id: FlowId::from("flow-a"),
                params: serde_json::json!({}),
            }))
            .await;
        assert_eq!(envelope.kind, "flow_started");
        assert_eq!(
            envelope.run_id.as_deref(),
            Some(run_id.to_string().as_str())
        );
        assert_eq!(envelope.step_id, None);
        assert_eq!(envelope.mob_id, "test-mob");
    }

    #[tokio::test]
    async fn projects_step_dispatched_with_run_step_target() {
        let store = MobEventsStore::new();
        let identity = AgentIdentity::from("worker-1");
        let run_id = RunId::new();
        let envelope = store
            .project_mob_event(&mob_event(MobEventKind::StepDispatched {
                run_id: run_id.clone(),
                step_id: StepId::from("step-a"),
                target: AgentRuntimeId::initial(identity),
            }))
            .await;
        assert_eq!(envelope.kind, "step_dispatched");
        assert_eq!(
            envelope.run_id.as_deref(),
            Some(run_id.to_string().as_str())
        );
        assert_eq!(envelope.step_id.as_deref(), Some("step-a"));
        assert_eq!(envelope.agent_identity.as_deref(), Some("worker-1"));
    }

    #[tokio::test]
    async fn projects_member_spawned_with_identity() {
        let store = MobEventsStore::new();
        let identity = AgentIdentity::from("researcher");
        let envelope = store
            .project_mob_event(&mob_event(MobEventKind::MemberSpawned(
                MemberSpawnedEvent::new(
                    identity.clone(),
                    Generation::INITIAL,
                    FenceToken::new(1),
                    AgentRuntimeId::initial(identity),
                    ProfileName::from("worker"),
                ),
            )))
            .await;
        assert_eq!(envelope.kind, "member_spawned");
        assert_eq!(envelope.agent_identity.as_deref(), Some("researcher"));
    }

    #[tokio::test]
    async fn query_filters_by_run_id_and_after_seq() {
        let store = MobEventsStore::new();
        let run_a = RunId::new();
        let run_b = RunId::new();
        let first = store
            .project_mob_event(&mob_event(MobEventKind::FlowStarted {
                run_id: run_a.clone(),
                flow_id: FlowId::from("flow-a"),
                params: serde_json::json!({}),
            }))
            .await;
        let _second = store
            .project_mob_event(&mob_event(MobEventKind::FlowStarted {
                run_id: run_b,
                flow_id: FlowId::from("flow-b"),
                params: serde_json::json!({}),
            }))
            .await;
        let third = store
            .project_mob_event(&mob_event(MobEventKind::StepDispatched {
                run_id: run_a.clone(),
                step_id: StepId::from("step-1"),
                target: AgentRuntimeId::initial(AgentIdentity::from("worker")),
            }))
            .await;

        let filtered = store
            .query(&EventQuery {
                run_id: Some(run_a.to_string()),
                ..EventQuery::default()
            })
            .await;
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].cursor, first.cursor);
        assert_eq!(filtered[1].cursor, third.cursor);

        let after = store
            .query(&EventQuery {
                run_id: Some(run_a.to_string()),
                after_seq: Some(first.cursor),
                ..EventQuery::default()
            })
            .await;
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].cursor, third.cursor);
    }

    #[tokio::test]
    async fn query_filters_by_mob_id_and_event_types() {
        let store = MobEventsStore::new();
        let r1 = RunId::new();
        let r2 = RunId::new();
        let _ = store
            .project_mob_event(&MobEvent {
                cursor: 0,
                timestamp: Utc::now(),
                mob_id: MobId::from("mob-A"),
                kind: MobEventKind::FlowStarted {
                    run_id: r1.clone(),
                    flow_id: FlowId::from("f1"),
                    params: serde_json::json!({}),
                },
            })
            .await;
        let _ = store
            .project_mob_event(&MobEvent {
                cursor: 0,
                timestamp: Utc::now(),
                mob_id: MobId::from("mob-A"),
                kind: MobEventKind::FlowCompleted {
                    run_id: r1,
                    flow_id: FlowId::from("f1"),
                },
            })
            .await;
        let _ = store
            .project_mob_event(&MobEvent {
                cursor: 0,
                timestamp: Utc::now(),
                mob_id: MobId::from("mob-B"),
                kind: MobEventKind::FlowStarted {
                    run_id: r2,
                    flow_id: FlowId::from("f2"),
                    params: serde_json::json!({}),
                },
            })
            .await;

        let mob_a = store
            .query(&EventQuery {
                mob_id: Some("mob-A".to_string()),
                ..EventQuery::default()
            })
            .await;
        assert_eq!(mob_a.len(), 2);

        let only_started = store
            .query(&EventQuery {
                event_types: vec!["flow_started".to_string()],
                ..EventQuery::default()
            })
            .await;
        assert_eq!(only_started.len(), 2);
        assert!(only_started.iter().all(|e| e.kind == "flow_started"));
    }
}
