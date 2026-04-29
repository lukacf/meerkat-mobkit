//! Query/subscribe contracts for the structural mob-events surface.
//!
//! Exercises `mob_id`, `run_id`, `step_id`, `event_types`, and `after_seq`
//! cursor pagination via the `mobkit/mob_events/query` RPC, plus the
//! `mobkit/mob_events/subscribe` snapshot frame.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::redundant_clone
)]

use chrono::Utc;
use meerkat_mob::event::{MobEvent, MobEventKind};
use meerkat_mob::ids::{AgentIdentity, AgentRuntimeId, FlowId, MobId, RunId, StepId};
use meerkat_mobkit::unified_runtime::EventQuery;
use meerkat_mobkit::unified_runtime::mob_events::MobEventsStore;

fn flow_started(mob: &str, run: &RunId) -> MobEvent {
    MobEvent {
        cursor: 0,
        timestamp: Utc::now(),
        mob_id: MobId::from(mob),
        kind: MobEventKind::FlowStarted {
            run_id: run.clone(),
            flow_id: FlowId::from("flow-x"),
            params: serde_json::json!({}),
        },
    }
}

fn step_dispatched(mob: &str, run: &RunId, step: &str, agent: &str) -> MobEvent {
    MobEvent {
        cursor: 0,
        timestamp: Utc::now(),
        mob_id: MobId::from(mob),
        kind: MobEventKind::StepDispatched {
            run_id: run.clone(),
            step_id: StepId::from(step),
            target: AgentRuntimeId::initial(AgentIdentity::from(agent)),
        },
    }
}

#[tokio::test]
async fn query_by_mob_id_isolates_per_mob_buffer() {
    let store = MobEventsStore::new();
    let run_a = RunId::new();
    let run_b = RunId::new();
    store
        .project_mob_event(&flow_started("mob-A", &run_a))
        .await;
    store
        .project_mob_event(&flow_started("mob-B", &run_b))
        .await;
    store
        .project_mob_event(&step_dispatched("mob-A", &run_a, "s1", "w1"))
        .await;

    let only_a = store
        .query(&EventQuery {
            mob_id: Some("mob-A".to_string()),
            ..EventQuery::default()
        })
        .await;
    assert_eq!(only_a.len(), 2);
    assert!(only_a.iter().all(|event| event.mob_id == "mob-A"));

    let only_b = store
        .query(&EventQuery {
            mob_id: Some("mob-B".to_string()),
            ..EventQuery::default()
        })
        .await;
    assert_eq!(only_b.len(), 1);
}

#[tokio::test]
async fn query_by_run_id_filters_across_mob_buffer() {
    let store = MobEventsStore::new();
    let run_a = RunId::new();
    let run_b = RunId::new();
    store
        .project_mob_event(&flow_started("mob-A", &run_a))
        .await;
    store
        .project_mob_event(&flow_started("mob-A", &run_b))
        .await;
    store
        .project_mob_event(&step_dispatched("mob-A", &run_a, "s1", "w1"))
        .await;

    let by_run = store
        .query(&EventQuery {
            run_id: Some(run_a.to_string()),
            ..EventQuery::default()
        })
        .await;
    assert_eq!(by_run.len(), 2);
    assert!(
        by_run
            .iter()
            .all(|event| event.run_id.as_deref() == Some(run_a.to_string().as_str()))
    );
}

#[tokio::test]
async fn query_by_step_id_returns_only_matching_step() {
    let store = MobEventsStore::new();
    let run = RunId::new();
    store
        .project_mob_event(&step_dispatched("mob-A", &run, "step-1", "w1"))
        .await;
    store
        .project_mob_event(&step_dispatched("mob-A", &run, "step-2", "w2"))
        .await;

    let by_step = store
        .query(&EventQuery {
            step_id: Some("step-2".to_string()),
            ..EventQuery::default()
        })
        .await;
    assert_eq!(by_step.len(), 1);
    assert_eq!(by_step[0].step_id.as_deref(), Some("step-2"));
    assert_eq!(by_step[0].agent_identity.as_deref(), Some("w2"));
}

#[tokio::test]
async fn query_by_event_types_filters_kind_label() {
    let store = MobEventsStore::new();
    let run = RunId::new();
    store.project_mob_event(&flow_started("mob-A", &run)).await;
    store
        .project_mob_event(&step_dispatched("mob-A", &run, "step-1", "w1"))
        .await;
    store
        .project_mob_event(&MobEvent {
            cursor: 0,
            timestamp: Utc::now(),
            mob_id: MobId::from("mob-A"),
            kind: MobEventKind::FlowCompleted {
                run_id: run.clone(),
                flow_id: FlowId::from("flow-x"),
            },
        })
        .await;

    let started_only = store
        .query(&EventQuery {
            event_types: vec!["flow_started".to_string()],
            ..EventQuery::default()
        })
        .await;
    assert_eq!(started_only.len(), 1);

    let started_or_completed = store
        .query(&EventQuery {
            event_types: vec!["flow_started".to_string(), "flow_completed".to_string()],
            ..EventQuery::default()
        })
        .await;
    assert_eq!(started_or_completed.len(), 2);
}

#[tokio::test]
async fn after_seq_paginates_strictly_newer_events() {
    let store = MobEventsStore::new();
    let run = RunId::new();
    let first = store.project_mob_event(&flow_started("mob-A", &run)).await;
    let second = store
        .project_mob_event(&step_dispatched("mob-A", &run, "step-1", "w1"))
        .await;
    let third = store
        .project_mob_event(&step_dispatched("mob-A", &run, "step-2", "w2"))
        .await;

    // Pagination: after the first cursor, only second + third remain.
    let page = store
        .query(&EventQuery {
            after_seq: Some(first.cursor),
            ..EventQuery::default()
        })
        .await;
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].cursor, second.cursor);
    assert_eq!(page[1].cursor, third.cursor);

    // Cursor at the head: nothing left.
    let nothing = store
        .query(&EventQuery {
            after_seq: Some(third.cursor),
            ..EventQuery::default()
        })
        .await;
    assert!(nothing.is_empty());
}

#[tokio::test]
async fn live_subscribe_receives_new_events() {
    let store = MobEventsStore::new();
    let mut rx = store.subscribe();
    let run = RunId::new();
    store.project_mob_event(&flow_started("mob-A", &run)).await;
    let received = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
        .await
        .expect("recv timed out")
        .expect("recv ok");
    assert_eq!(received.kind, "flow_started");
    assert_eq!(received.mob_id, "mob-A");
}
