//! Ingestion contract for the structural mob-events surface.
//!
//! Asserts every `MobEventKind` variant survives projection into
//! `MobStructuralEventEnvelope` with structural fields intact.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::redundant_clone
)]

use chrono::Utc;
use meerkat_core::comms::{PeerId, PeerName, TrustedPeerDescriptor};
use meerkat_core::service::OpaquePrincipalToken;
use meerkat_mob::MobDefinition;
use meerkat_mob::event::{MemberSpawnedEvent, MobEvent, MobEventKind};
use meerkat_mob::ids::{
    AgentIdentity, AgentRuntimeId, FenceToken, FlowId, Generation, MobId, ProfileName, RunId,
    StepId, TaskId,
};
use meerkat_mob::tasks::TaskStatus;
use meerkat_mobkit::unified_runtime::mob_events::MobEventsStore;

fn project(kind: MobEventKind) -> meerkat_mobkit::MobStructuralEventEnvelope {
    let store = MobEventsStore::new();
    let event = MobEvent {
        cursor: 0,
        timestamp: Utc::now(),
        mob_id: MobId::from("test-mob"),
        kind,
    };
    futures::executor::block_on(store.project_mob_event(&event))
}

fn sample_definition() -> MobDefinition {
    MobDefinition::from_toml(
        r#"
[mob]
id = "test-mob"

[profiles.worker]
model = "test-model"
external_addressable = false
"#,
    )
    .expect("parse mob definition")
}

fn run() -> RunId {
    RunId::new()
}

#[test]
fn all_mob_event_kinds_project_with_kind_label() {
    let identity = AgentIdentity::from("worker-1");
    let runtime_id = AgentRuntimeId::initial(identity.clone());
    let run_id = run();
    let step_id = StepId::from("step-a");
    let flow_id = FlowId::from("flow-a");
    let task_id = TaskId::from("task-1");
    let peer_descriptor = TrustedPeerDescriptor::test_only_unsigned_typed(
        "remote/worker/agent-x",
        PeerId::new(),
        "inproc://remote",
    )
    .expect("descriptor");

    // 25 cases: assert each projects with the expected snake_case kind
    // label and that structural fields survive when present.

    let m = project(MobEventKind::MobCreated {
        definition: Box::new(sample_definition()),
    });
    assert_eq!(m.kind, "mob_created");
    assert_eq!(m.run_id, None);

    assert_eq!(project(MobEventKind::MobCompleted).kind, "mob_completed");
    assert_eq!(project(MobEventKind::MobReset).kind, "mob_reset");

    let m = project(MobEventKind::MemberSpawned(MemberSpawnedEvent::new(
        identity.clone(),
        Generation::INITIAL,
        FenceToken::new(1),
        runtime_id.clone(),
        ProfileName::from("worker"),
    )));
    assert_eq!(m.kind, "member_spawned");
    assert_eq!(m.agent_identity.as_deref(), Some("worker-1"));

    let m = project(MobEventKind::MemberRetired {
        agent_identity: identity.clone(),
        generation: Generation::new(1),
        role: ProfileName::from("worker"),
    });
    assert_eq!(m.kind, "member_retired");
    assert_eq!(m.agent_identity.as_deref(), Some("worker-1"));

    let m = project(MobEventKind::MemberReset {
        agent_identity: identity.clone(),
        previous_generation: Generation::new(0),
        new_generation: Generation::new(1),
        fence_token: FenceToken::new(2),
        agent_runtime_id: AgentRuntimeId::new(identity.clone(), Generation::new(1)),
    });
    assert_eq!(m.kind, "member_reset");
    assert_eq!(m.agent_identity.as_deref(), Some("worker-1"));

    // `MobMemberKickoffSnapshot` is `#[non_exhaustive]` and contains a
    // `SystemTime`, so we can't construct it directly from the test
    // crate. Build the variant via JSON to exercise the projection.
    // Bincode's SystemTime serialization is `[secs, nanos]`; JSON
    // expects the same shape. (See `meerkat_mob::roster`.)
    let kickoff_kind: MobEventKind = serde_json::from_value(serde_json::json!({
        "type": "member_kickoff_updated",
        "member": "worker-1",
        "kickoff": {
            "phase": "pending",
            "updated_at": { "secs_since_epoch": 0, "nanos_since_epoch": 0 },
        },
    }))
    .expect("kickoff variant deserializes");
    let m = project(kickoff_kind);
    assert_eq!(m.kind, "member_kickoff_updated");
    assert_eq!(m.agent_identity.as_deref(), Some("worker-1"));

    let m = project(MobEventKind::MembersWired {
        a: AgentIdentity::from("a-1"),
        b: AgentIdentity::from("b-2"),
    });
    assert_eq!(m.kind, "members_wired");
    let m = project(MobEventKind::MembersUnwired {
        a: AgentIdentity::from("a-1"),
        b: AgentIdentity::from("b-2"),
    });
    assert_eq!(m.kind, "members_unwired");

    let m = project(MobEventKind::ExternalPeerWired {
        local: identity.clone(),
        spec: peer_descriptor.clone(),
    });
    assert_eq!(m.kind, "external_peer_wired");
    assert_eq!(m.agent_identity.as_deref(), Some("worker-1"));

    let m = project(MobEventKind::ExternalPeerUnwired {
        local: identity.clone(),
        peer_name: PeerName::new("remote/worker/agent-x").unwrap(),
    });
    assert_eq!(m.kind, "external_peer_unwired");

    let m = project(MobEventKind::TaskCreated {
        task_id: task_id.clone(),
        subject: "subject".to_string(),
        description: "desc".to_string(),
        blocked_by: vec![],
    });
    assert_eq!(m.kind, "task_created");

    let m = project(MobEventKind::TaskUpdated {
        task_id: task_id.clone(),
        status: TaskStatus::InProgress,
        owner: Some(identity.clone()),
    });
    assert_eq!(m.kind, "task_updated");
    assert_eq!(m.agent_identity.as_deref(), Some("worker-1"));

    let m = project(MobEventKind::FlowStarted {
        run_id: run_id.clone(),
        flow_id: flow_id.clone(),
        params: serde_json::json!({}),
    });
    assert_eq!(m.kind, "flow_started");
    assert_eq!(m.run_id.as_deref(), Some(run_id.to_string().as_str()));

    let m = project(MobEventKind::FlowCompleted {
        run_id: run_id.clone(),
        flow_id: flow_id.clone(),
    });
    assert_eq!(m.kind, "flow_completed");
    assert_eq!(m.run_id.as_deref(), Some(run_id.to_string().as_str()));

    let m = project(MobEventKind::FlowFailed {
        run_id: run_id.clone(),
        flow_id: flow_id.clone(),
        reason: "boom".to_string(),
    });
    assert_eq!(m.kind, "flow_failed");

    let m = project(MobEventKind::FlowCanceled {
        run_id: run_id.clone(),
        flow_id: flow_id.clone(),
    });
    assert_eq!(m.kind, "flow_canceled");

    let m = project(MobEventKind::StepDispatched {
        run_id: run_id.clone(),
        step_id: step_id.clone(),
        target: runtime_id.clone(),
    });
    assert_eq!(m.kind, "step_dispatched");
    assert_eq!(m.step_id.as_deref(), Some("step-a"));
    assert_eq!(m.agent_identity.as_deref(), Some("worker-1"));

    let m = project(MobEventKind::StepTargetCompleted {
        run_id: run_id.clone(),
        step_id: step_id.clone(),
        target: runtime_id.clone(),
    });
    assert_eq!(m.kind, "step_target_completed");

    let m = project(MobEventKind::StepTargetFailed {
        run_id: run_id.clone(),
        step_id: step_id.clone(),
        target: runtime_id.clone(),
        reason: "fail".to_string(),
    });
    assert_eq!(m.kind, "step_target_failed");

    let m = project(MobEventKind::StepCompleted {
        run_id: run_id.clone(),
        step_id: step_id.clone(),
    });
    assert_eq!(m.kind, "step_completed");

    let m = project(MobEventKind::StepFailed {
        run_id: run_id.clone(),
        step_id: step_id.clone(),
        reason: "timeout".to_string(),
    });
    assert_eq!(m.kind, "step_failed");

    let m = project(MobEventKind::StepSkipped {
        run_id: run_id.clone(),
        step_id: step_id.clone(),
        reason: "branch lost".to_string(),
    });
    assert_eq!(m.kind, "step_skipped");

    let m = project(MobEventKind::TopologyViolation {
        from_role: ProfileName::from("lead"),
        to_role: ProfileName::from("worker"),
    });
    assert_eq!(m.kind, "topology_violation");

    let m = project(MobEventKind::SupervisorEscalation {
        run_id: run_id.clone(),
        step_id: step_id.clone(),
        escalated_to: identity.clone(),
    });
    assert_eq!(m.kind, "supervisor_escalation");
    assert_eq!(m.agent_identity.as_deref(), Some("worker-1"));

    let m = project(MobEventKind::OperatorActionRecorded {
        tool_name: "mob_create".to_string(),
        principal_token: OpaquePrincipalToken::new("opaque"),
        caller_provenance: None,
        audit_invocation_id: None,
    });
    assert_eq!(m.kind, "operator_action_recorded");
}

#[test]
fn cursor_assigned_monotonically_and_data_preserves_kind_payload() {
    let store = MobEventsStore::new();
    let kind = MobEventKind::FlowStarted {
        run_id: run(),
        flow_id: FlowId::from("flow-a"),
        params: serde_json::json!({"k": "v"}),
    };
    let event = MobEvent {
        cursor: 0,
        timestamp: Utc::now(),
        mob_id: MobId::from("mob-A"),
        kind,
    };
    let first = futures::executor::block_on(store.project_mob_event(&event));
    let second = futures::executor::block_on(store.project_mob_event(&MobEvent {
        cursor: 0,
        timestamp: Utc::now(),
        mob_id: MobId::from("mob-A"),
        kind: MobEventKind::MobCompleted,
    }));
    assert!(second.cursor > first.cursor);
    assert_eq!(first.data["type"], "flow_started");
    assert_eq!(first.data["params"], serde_json::json!({"k": "v"}));
    assert_eq!(second.data["type"], "mob_completed");
}
