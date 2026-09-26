#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meerkat_client::{LlmClient, LlmError, LlmEvent, LlmRequest};
use meerkat_core::types::{HandlingMode, TranscriptUserRole};
use meerkat_core::{ContentInput, Message, Provider, SessionId};
use meerkat_mob::{MobDefinition, MobDeliveryIdentity, WorkOrigin, WorkSpec};
use serde_json::json;

use crate::console_aggregator::{
    AllowAllConsoleVisibilityPolicy, ConsoleFrameStatus, ConsoleInteractionAccepted,
    ConsoleSendRequest, ConsoleTimelineQuery, MobKitConsoleAggregator,
};
use crate::identity_first::agent_memory::{
    AgentMemoryConfig, AgentMemoryError, AgentMemoryPerTurnInjection, AgentMemoryProvider,
    AgentMemoryRecallRequest, AgentMemoryRecord, AgentMemoryRuntimeInjector,
};
use crate::identity_first::bridge::{HostHumanInputError, MobSessionBridge};
use crate::identity_first::orchestrator::restore_flow;
use crate::identity_first::*;
use crate::unified_runtime::{ConsoleEventStore, UnifiedRuntime, UnifiedRuntimeBuilder};

const WAIT: Duration = Duration::from_secs(30);
const HUMAN: &str = "CURRENT_VALUE is cobalt; remember my exact choice.";

#[derive(Clone)]
struct RecordingClient {
    requests: Arc<Mutex<Vec<LlmRequest>>>,
    gate: tokio::sync::watch::Sender<bool>,
}

impl RecordingClient {
    fn new() -> Self {
        Self {
            requests: Arc::default(),
            gate: tokio::sync::watch::channel(true).0,
        }
    }
}

#[async_trait::async_trait]
impl LlmClient for RecordingClient {
    fn project_replay_messages(&self, messages: &[Message]) -> Result<Vec<Message>, LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(&'a self, request: &'a LlmRequest) -> meerkat_client::types::LlmStream<'a> {
        self.requests.lock().unwrap().push(request.clone());
        let mut gate = self.gate.subscribe();
        Box::pin(async_stream::try_stream! {
            while !*gate.borrow_and_update() {
                gate.changed().await.unwrap();
            }
            yield LlmEvent::TextDelta { delta: "Your choice is cobalt.".to_string(), meta: None };
            yield LlmEvent::UsageUpdate {
                usage: meerkat_core::TurnUsage::host_declared(
                    Provider::OpenAI, &request.model, meerkat_core::Usage::default(),
                ),
            };
            yield LlmEvent::Done {
                outcome: meerkat_client::LlmDoneOutcome::Success {
                    stop_reason: meerkat_core::StopReason::EndTurn,
                },
            };
        })
    }

    fn provider(&self) -> Provider {
        Provider::OpenAI
    }
    async fn health_check(&self) -> Result<(), LlmError> {
        Ok(())
    }
}

struct Harness {
    unified: UnifiedRuntime,
    identity_runtime: Arc<IdentityRuntime>,
    bridge: Arc<MobSessionBridge>,
    aggregator: MobKitConsoleAggregator,
    identity: AgentIdentity,
    runtime_id: AgentRuntimeId,
    session_id: SessionId,
    client: RecordingClient,
    _state: tempfile::TempDir,
}

impl Harness {
    async fn new(budget: Duration) -> Self {
        Self::with_kickoff(budget, false).await
    }

    async fn with_kickoff(budget: Duration, kickoff: bool) -> Self {
        let state = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let client = RecordingClient::new();
        let unified = UnifiedRuntimeBuilder::default()
            .definition(
                MobDefinition::from_toml(&format!(
                    r#"
[mob]
id = "console-human-{}"
[profiles.human]
model = "gpt-5.5"
external_addressable = true
runtime_mode = "autonomous_host"
[profiles.human.tools]
comms = true
"#,
                    uuid::Uuid::new_v4()
                ))
                .unwrap(),
            )
            .persistent_state(state.path())
            .comms(true)
            .default_llm_client(Arc::new(client.clone()))
            .build()
            .await
            .unwrap();
        let bridge = Arc::new(
            MobSessionBridge::with_session_service(
                unified.mob_handle(),
                unified.mob_runtime().session_service().cloned().unwrap(),
            )
            .with_actor_admission_budget(budget),
        );
        let identity_runtime = Arc::new(
            IdentityRuntime::new(IdentityRuntimeConfig {
                continuity_store: Arc::new(LocalContinuityStore::in_memory().unwrap()),
                lease_provider: Arc::new(LocalLeaseProvider::new()),
                runtime_instance_id: "human-test".to_string(),
                has_runtime_store: true,
                durability_policy: DurabilityPolicy::SyncWriteThrough,
                bridge: Some(bridge.clone()),
                default_timeout: None,
            })
            .with_runtime_services(AgentRuntimeServices::new(unified.mob_handle())),
        );
        let identity = AgentIdentity::parse("human").unwrap();
        let spec = DurableAgentSpec {
            identity: identity.clone(),
            profile: "human".into(),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: BTreeMap::new(),
            context: None,
            additional_instructions: Vec::new(),
            initial_message: kickoff.then(|| "fixture kickoff".into()),
            runtime_mode_override: Some(meerkat_mob::MobRuntimeMode::AutonomousHost),
            backend: None,
            binding: None,
            placement: None,
        };
        restore_flow(&identity_runtime, &[spec], None, None)
            .await
            .unwrap();
        let status = identity_runtime.status(&identity).await.unwrap();
        if kickoff {
            tokio::time::timeout(WAIT, async {
                loop {
                    let snapshot = unified
                        .mob_handle()
                        .member_status(&crate::member_comms_id::mob_member_id(identity.as_str()))
                        .await
                        .unwrap();
                    if snapshot.kickoff.as_ref().is_some_and(|kickoff| {
                        kickoff.phase == meerkat_mob::MobMemberKickoffPhase::Started
                    }) {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        let aggregator = MobKitConsoleAggregator::in_memory();
        aggregator.register_runtime_handles_with_policy(
            "human",
            "",
            unified.mob_runtime().clone(),
            Some(identity_runtime.clone()),
            ConsoleEventStore::new(),
            Arc::new(AllowAllConsoleVisibilityPolicy),
        );
        Self {
            unified,
            identity_runtime,
            bridge,
            aggregator,
            identity,
            runtime_id: status.agent_runtime_id.unwrap(),
            session_id: status.session_id.unwrap(),
            client,
            _state: state,
        }
    }

    fn request(&self, key: &str, content: &str) -> ConsoleSendRequest {
        ConsoleSendRequest {
            identity: self.identity.to_string(),
            content: json!(content),
            origin: "console".to_string(),
            idempotency_key: key.to_string(),
            handling_mode: None,
            origin_kind: None,
        }
    }

    async fn send(
        &self,
        request: ConsoleSendRequest,
    ) -> Result<ConsoleInteractionAccepted, crate::console_aggregator::ConsoleSendError> {
        super::console_send_identity_first(
            &self.aggregator,
            self.identity_runtime.clone(),
            None,
            request,
        )
        .await
    }

    async fn wait_status(
        &self,
        accepted: &ConsoleInteractionAccepted,
        expected: ConsoleFrameStatus,
    ) {
        tokio::time::timeout(WAIT, async {
            loop {
                let page = self
                    .aggregator
                    .query_timeline(ConsoleTimelineQuery {
                        identity: Some(accepted.identity.clone()),
                        ..Default::default()
                    })
                    .await
                    .unwrap();
                if page
                    .frames
                    .iter()
                    .any(|frame| frame.id == accepted.input_frame_id && frame.status == expected)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("console dispatch status");
    }

    async fn finish_human(&self, accepted: &ConsoleInteractionAccepted, content: &str) {
        self.finish_human_with_mode(accepted, content, HandlingMode::Queue)
            .await;
    }

    async fn finish_human_with_mode(
        &self,
        accepted: &ConsoleInteractionAccepted,
        content: &str,
        mode: HandlingMode,
    ) {
        self.finish_human_with_context(accepted, content, mode, Vec::new())
            .await;
    }

    async fn finish_human_with_context(
        &self,
        accepted: &ConsoleInteractionAccepted,
        content: &str,
        mode: HandlingMode,
        injected_context: Vec<ContentInput>,
    ) {
        self.wait_status(accepted, ConsoleFrameStatus::Delivered)
            .await;
        let handle = self.unified.mob_handle();
        let member = handle
            .get_member(&crate::member_comms_id::mob_member_id(&accepted.identity))
            .await
            .unwrap()
            .unwrap();
        let interaction = accepted.interaction_id.parse::<uuid::Uuid>().unwrap();
        let spec = WorkSpec::new(content, WorkOrigin::Internal)
            .with_injected_context(injected_context)
            .with_interaction_id(meerkat_core::interaction::InteractionId(interaction));
        let turn = handle
            .start_host_human_input_bounded(
                member.agent_runtime_id,
                member.fence_token,
                spec,
                mode,
                MobDeliveryIdentity::new(&accepted.input_frame_id, interaction.to_string())
                    .unwrap(),
                std::time::Instant::now() + WAIT,
            )
            .await
            .unwrap();
        tokio::time::timeout(WAIT, turn.wait())
            .await
            .unwrap()
            .unwrap();
    }

    async fn history(&self) -> Vec<Message> {
        self.unified
            .mob_runtime()
            .read_session_history(&self.session_id.to_string(), 0, None)
            .await
            .unwrap()
            .messages
    }

    async fn stop(self) {
        // Production teardown: a member whose runtime is still mid-kickoff
        // (`Runtime not ready: attached`, e.g. the reset successor before
        // its first turn) refuses a raw `MobHandle::stop`. The unified
        // runtime waits that readiness window out and degrades it typed
        // instead of failing teardown; every other refusal still fails.
        match self.unified.stop_mob_for_teardown().await {
            crate::unified_runtime::MobStopOutcome::Stopped
            | crate::unified_runtime::MobStopOutcome::ProceededWithoutInterrupt { .. } => {}
            crate::unified_runtime::MobStopOutcome::Failed(error) => {
                panic!("mob teardown failed: {error}");
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn console_human_is_canonical_once_and_generic_identity_work_stays_external_event() {
    let h = Harness::new(WAIT).await;
    let request = h.request("canonical", HUMAN);
    let accepted = h.send(request.clone()).await.unwrap();
    h.finish_human(&accepted, HUMAN).await;
    let history = h.history().await;
    let interaction =
        meerkat_core::interaction::InteractionId(accepted.interaction_id.parse().unwrap());
    let users: Vec<_> = history
        .iter()
        .filter_map(|message| match message {
            Message::User(user) if user.identity.interaction_id == Some(interaction) => Some(user),
            _ => None,
        })
        .collect();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].text_content(), HUMAN);
    assert_eq!(users[0].transcript_role, TranscriptUserRole::Conversational);
    assert!(history.iter().any(|message| matches!(message,
        Message::BlockAssistant(answer) if answer.identity.interaction_id == Some(interaction))));

    let calls = h.client.requests.lock().unwrap().len();
    let replay = h.send(request.clone()).await.unwrap();
    assert_eq!(replay.input_frame_id, accepted.input_frame_id);
    assert_eq!(h.client.requests.lock().unwrap().len(), calls);
    let mut changed = request;
    changed.content = json!("changed user intent");
    assert!(matches!(
        h.send(changed).await,
        Err(crate::console_aggregator::ConsoleSendError::IdempotencyConflict(_))
    ));

    let page = h
        .aggregator
        .query_timeline(ConsoleTimelineQuery {
            identity: Some(h.identity.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        page.frames
            .iter()
            .filter(|frame| frame.kind == "user_input"
                && frame.interaction_id.as_deref() == Some(&accepted.interaction_id))
            .count(),
        1,
        "optimistic/native history must join by the exact interaction"
    );

    h.identity_runtime
        .send(
            &h.identity,
            &ContentInput::Text("GENERIC_SYSTEM_WORK".to_string()),
        )
        .await
        .unwrap();
    let generic = tokio::time::timeout(WAIT, async {
        loop {
            let messages = h.history().await;
            if messages.iter().any(|message| {
                matches!(message, Message::SystemNotice(_))
                    && serde_json::to_string(message)
                        .unwrap()
                        .contains("GENERIC_SYSTEM_WORK")
            }) {
                break messages;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(generic.iter().any(|message| {
        matches!(message, Message::SystemNotice(_))
            && serde_json::to_string(message)
                .unwrap()
                .contains("GENERIC_SYSTEM_WORK")
    }));
    assert!(!generic.iter().any(|message|
        matches!(message, Message::User(user) if user.text_content() == "GENERIC_SYSTEM_WORK")));
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn console_human_recall_stays_canonical_context_not_refreshed_or_paged_human_input() {
    const MEMORY: &str = "Recalled reference: the cobalt sample is stored in locker seven.";
    struct RecallProvider;
    #[async_trait::async_trait]
    impl AgentMemoryProvider for RecallProvider {
        async fn recall(
            &self,
            _request: AgentMemoryRecallRequest,
        ) -> Result<Vec<AgentMemoryRecord>, AgentMemoryError> {
            Ok(vec![AgentMemoryRecord {
                memory_id: "cobalt-location".to_string(),
                title: "Cobalt reference".to_string(),
                body: MEMORY.to_string(),
                tags: Vec::new(),
                created_at_ms: 1,
                updated_at_ms: 1,
            }])
        }
    }
    let h = Harness::new(WAIT).await;
    h.identity_runtime
        .set_agent_memory(Some(AgentMemoryRuntimeInjector::new(
            Arc::new(RecallProvider),
            AgentMemoryConfig {
                per_turn_injection: AgentMemoryPerTurnInjection::Budgeted,
                ..Default::default()
            },
        )))
        .await;
    let accepted = h.send(h.request("recall-projection", HUMAN)).await.unwrap();
    let interaction =
        meerkat_core::interaction::InteractionId(accepted.interaction_id.parse().unwrap());
    // Read the exact prepared slot from the real session, not another recall:
    // the completion-bearing replay must match the originally admitted input.
    let injected = tokio::time::timeout(WAIT, async {
        loop {
            let injected: Vec<_> = h
                .history()
                .await
                .into_iter()
                .filter_map(|message| match message {
                    Message::User(user)
                        if user.identity.interaction_id == Some(interaction)
                            && user.transcript_role == TranscriptUserRole::InjectedContext =>
                    {
                        Some(ContentInput::Text(user.text_content()))
                    }
                    _ => None,
                })
                .collect();
            if !injected.is_empty() {
                break injected;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("configured recall must reach the canonical injected-context slot");
    assert!(serde_json::to_string(&injected).unwrap().contains(MEMORY));
    h.finish_human_with_context(&accepted, HUMAN, HandlingMode::Queue, injected)
        .await;

    for _ in 0..2 {
        h.aggregator.refresh_session_history().await.unwrap();
        let full = h
            .aggregator
            .query_timeline(ConsoleTimelineQuery {
                identity: Some(h.identity.to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            full.frames
                .iter()
                .filter(|frame| frame.kind == "user_input")
                .count(),
            1,
            "ambient memory must not become a second human input; frames: {:#?}",
            full.frames
        );
        assert!(
            !serde_json::to_string(&full.frames)
                .unwrap()
                .contains(MEMORY)
        );
    }
    let mut cursor = accepted.cursor.clone();
    let mut reached_end = false;
    for _ in 0..20 {
        let page = h
            .aggregator
            .query_timeline(ConsoleTimelineQuery {
                identity: Some(h.identity.to_string()),
                after: Some(cursor.clone()),
                limit: 1,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            !page.frames.iter().any(|frame| frame.kind == "user_input"),
            "paging past the reserved human input must not surface ambient memory as another"
        );
        assert!(
            !serde_json::to_string(&page.frames)
                .unwrap()
                .contains(MEMORY)
        );
        let Some(last) = page.frames.last() else {
            reached_end = true;
            break;
        };
        cursor = last.cursor.clone();
    }
    assert!(
        reached_end,
        "bounded pagination must exhaust the transcript"
    );

    // A fresh observer has no optimistic reservation to hide behind.
    let history_only = MobKitConsoleAggregator::in_memory();
    history_only.register_runtime_handles_with_policy(
        "human",
        "",
        h.unified.mob_runtime().clone(),
        Some(h.identity_runtime.clone()),
        ConsoleEventStore::new(),
        Arc::new(AllowAllConsoleVisibilityPolicy),
    );
    history_only.refresh_session_history().await.unwrap();
    let page = history_only
        .query_timeline(ConsoleTimelineQuery {
            identity: Some(h.identity.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        page.frames
            .iter()
            .filter(|frame| frame.kind == "user_input")
            .count(),
        1
    );
    assert!(
        !serde_json::to_string(&page.frames)
            .unwrap()
            .contains(MEMORY)
    );

    let canonical = h.history().await;
    assert!(canonical.iter().any(|message| matches!(message,
        Message::User(user) if user.transcript_role == TranscriptUserRole::InjectedContext
            && user.identity.interaction_id == Some(interaction)
            && user.text_content().contains(MEMORY))));
    assert_eq!(canonical.iter().filter(|message| matches!(message,
        Message::User(user) if user.transcript_role == TranscriptUserRole::Conversational
            && user.identity.interaction_id == Some(interaction) && user.text_content() == HUMAN
    )).count(), 1);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn console_human_timeout_replay_preserves_failed_receipt_without_repreparing() {
    let h = Harness::new(Duration::ZERO).await;
    let request = h.request("unknown", HUMAN);
    let accepted = h.send(request.clone()).await.unwrap();
    h.wait_status(&accepted, ConsoleFrameStatus::DeliveryFailed)
        .await;
    let replay = h.send(request).await.unwrap_err();
    let data = super::console_send_error_data(&replay).unwrap();
    assert_eq!(data["kind"], "host_human_input_replay_unavailable");
    assert_eq!(data["input_frame_id"], accepted.input_frame_id);
    assert_eq!(data["execution_fate"], "unknown");
    assert_eq!(data["retry_submitted"], false);
    assert!(h.client.requests.lock().unwrap().is_empty());
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn console_human_lost_admission_observation_does_not_reexecute_committed_work() {
    let h = Harness::new(WAIT).await;
    let request = h.request("lost-response", HUMAN);
    let accepted = h.send(request.clone()).await.unwrap();
    h.finish_human(&accepted, HUMAN).await;
    let calls = h.client.requests.lock().unwrap().len();
    // The runtime really committed. Only the console's observation is lost;
    // mark its existing receipt failed as the timeout path does.
    h.aggregator
        .mark_interaction_delivery_failed(&accepted.input_frame_id)
        .await
        .unwrap();
    let replay = h.send(request).await.unwrap_err();
    assert_eq!(
        super::console_send_error_data(&replay).unwrap()["kind"],
        "host_human_input_replay_unavailable",
    );
    assert_eq!(h.client.requests.lock().unwrap().len(), calls);
    assert_eq!(
        h.history()
            .await
            .iter()
            .filter(
                |message| matches!(message, Message::User(user) if user.text_content() == HUMAN)
            )
            .count(),
        1
    );
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn console_human_bridge_slots_strict_replay_and_stale_session_refusal() {
    let h = Harness::new(WAIT).await;
    let interaction = uuid::Uuid::new_v4();
    let mut delivery = BridgeDelivery::new(HUMAN.into(), HandlingMode::Queue);
    delivery.interaction_id = Some(interaction.to_string());
    delivery.delivery_identity =
        Some(MobDeliveryIdentity::new("slots", interaction.to_string()).unwrap());
    delivery.system_prompt = Some("PER_TURN_SYSTEM".to_string());
    delivery.injected_context = vec!["AMBIENT_MEMORY".into()];
    h.bridge
        .deliver_host_human_input(&h.runtime_id, &h.session_id, delivery.clone())
        .await
        .unwrap();
    let member = h
        .unified
        .mob_handle()
        .get_member(&crate::member_comms_id::mob_member_id(h.identity.as_str()))
        .await
        .unwrap()
        .unwrap();
    let spec = WorkSpec::new(HUMAN, WorkOrigin::Internal)
        .with_system_prompt("PER_TURN_SYSTEM")
        .with_injected_context(vec!["AMBIENT_MEMORY".into()])
        .with_interaction_id(meerkat_core::interaction::InteractionId(interaction));
    let turn = h
        .unified
        .mob_handle()
        .start_host_human_input_bounded(
            member.agent_runtime_id,
            member.fence_token,
            spec,
            HandlingMode::Queue,
            delivery.delivery_identity.clone().unwrap(),
            std::time::Instant::now() + WAIT,
        )
        .await
        .unwrap();
    tokio::time::timeout(WAIT, turn.wait())
        .await
        .unwrap()
        .unwrap();
    let history = h.history().await;
    assert!(history.iter().any(|message| matches!(message,
        Message::User(user) if user.transcript_role == TranscriptUserRole::InjectedContext
            && user.text_content() == "AMBIENT_MEMORY")));
    assert!(history.iter().any(|message| matches!(message,
        Message::System(system) if system.content == "PER_TURN_SYSTEM")));
    let calls_after_turn = h.client.requests.lock().unwrap().len();
    h.bridge
        .deliver_host_human_input(&h.runtime_id, &h.session_id, delivery.clone())
        .await
        .unwrap();
    delivery.injected_context = vec!["CHANGED_MEMORY".into()];
    assert!(matches!(
        h.bridge.deliver_host_human_input(&h.runtime_id, &h.session_id, delivery.clone()).await,
        Err(BridgeError::HostHumanInput(HostHumanInputError::Mob(error)))
            if matches!(*error, meerkat_mob::MobError::WorkInputIdempotencyConflict { .. })
    ));
    assert!(matches!(
        h.bridge
            .deliver_host_human_input(&h.runtime_id, &SessionId::new(), delivery.clone())
            .await,
        Err(BridgeError::HostHumanInput(
            HostHumanInputError::BindingChanged
        ))
    ));
    delivery.handling_mode = HandlingMode::Steer;
    let interaction = uuid::Uuid::new_v4();
    delivery.interaction_id = Some(interaction.to_string());
    delivery.delivery_identity =
        Some(MobDeliveryIdentity::new("steer-slots", interaction.to_string()).unwrap());
    let refusal = h
        .bridge
        .deliver_host_human_input(&h.runtime_id, &h.session_id, delivery.clone())
        .await;
    assert!(
        matches!(
                &refusal,
                Err(BridgeError::HostHumanInput(HostHumanInputError::Mob(error)))
                    if matches!(error.as_ref(), meerkat_mob::MobError::InjectedContextUndeliverable { .. })
        ),
        "{refusal:?}"
    );
    delivery.injected_context.clear();
    let refusal = h
        .bridge
        .deliver_host_human_input(&h.runtime_id, &h.session_id, delivery)
        .await;
    assert!(
        matches!(
        &refusal,
        Err(BridgeError::HostHumanInput(HostHumanInputError::Mob(error)))
                if matches!(error.as_ref(), meerkat_mob::MobError::UnsupportedForMode { .. })
        ),
        "{refusal:?}"
    );
    assert_eq!(h.client.requests.lock().unwrap().len(), calls_after_turn);
    assert_eq!(
        h.history()
            .await
            .iter()
            .filter(
                |message| matches!(message, Message::User(user) if user.text_content() == HUMAN)
            )
            .count(),
        1
    );
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn console_human_member_only_worker_uses_the_same_canonical_user_seam() {
    let h = Harness::new(WAIT).await;
    h.unified
        .mob_handle()
        .spawn_spec(meerkat_mob::SpawnMemberSpec::from_wire(
            "human".to_string(),
            "worker".to_string(),
            None,
            None,
            None,
        ))
        .await
        .unwrap();
    let request = ConsoleSendRequest {
        identity: "worker".to_string(),
        ..h.request("worker", HUMAN)
    };
    let accepted = h.aggregator.send(request.clone()).await.unwrap();
    h.finish_human(&accepted, HUMAN).await;
    let messages = h
        .unified
        .mob_runtime()
        .read_session_history(accepted.session_id.as_deref().unwrap(), 0, None)
        .await
        .unwrap()
        .messages;
    assert_eq!(
        messages
            .iter()
            .filter(
                |message| matches!(message, Message::User(user) if user.text_content() == HUMAN)
            )
            .count(),
        1
    );
    let replay = h.aggregator.send(request).await.unwrap();
    assert_eq!(replay.input_frame_id, accepted.input_frame_id);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn console_human_active_steer_stays_steer_and_commits_once() {
    let h = Harness::with_kickoff(WAIT, true).await;
    let calls_before = h.client.requests.lock().unwrap().len();
    h.client.gate.send_replace(false);
    let active = h
        .send(h.request("active", "active queue work"))
        .await
        .unwrap();
    tokio::time::timeout(WAIT, async {
        while h.client.requests.lock().unwrap().len() <= calls_before {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let request = ConsoleSendRequest {
        handling_mode: Some("steer".to_string()),
        origin_kind: None,
        ..h.request("steer", HUMAN)
    };
    let steer = tokio::time::timeout(WAIT, h.send(request.clone()))
        .await
        .unwrap()
        .unwrap();
    assert!(
        !*h.client.gate.borrow(),
        "admission cannot wait for the active LLM"
    );
    h.client.gate.send_replace(true);
    h.finish_human(&active, "active queue work").await;
    h.finish_human_with_mode(&steer, HUMAN, HandlingMode::Steer)
        .await;
    assert_eq!(
        h.history()
            .await
            .iter()
            .filter(
                |message| matches!(message, Message::User(user) if user.text_content() == HUMAN)
            )
            .count(),
        1
    );
    assert_eq!(
        h.send(request).await.unwrap().input_frame_id,
        steer.input_frame_id
    );
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn console_human_reset_refuses_old_prepared_session_and_old_worker_fence() {
    let h = Harness::new(WAIT).await;
    let request = h.request("before-reset", HUMAN);
    let accepted = h.send(request.clone()).await.unwrap();
    h.finish_human(&accepted, HUMAN).await;
    let old_member = h
        .unified
        .mob_handle()
        .list_members()
        .await
        .into_iter()
        .find(|member| member.agent_identity.as_str() == h.identity.as_str())
        .unwrap();
    let successor = h.identity_runtime.reset(&h.identity).await.unwrap();
    assert_ne!(successor.session_id, h.session_id);
    let replay = h.send(request).await.unwrap();
    assert_eq!(
        replay.session_id.as_deref(),
        Some(h.session_id.to_string().as_str())
    );
    let mut delivery = BridgeDelivery::new(HUMAN.into(), HandlingMode::Queue);
    delivery.interaction_id = Some(accepted.interaction_id.clone());
    delivery.delivery_identity =
        Some(MobDeliveryIdentity::new(&accepted.input_frame_id, &accepted.interaction_id).unwrap());
    assert!(matches!(
        h.bridge
            .deliver_host_human_input(&h.runtime_id, &h.session_id, delivery)
            .await,
        Err(BridgeError::HostHumanInput(
            HostHumanInputError::BindingChanged
        ))
    ));
    assert!(
        crate::mob_handle_runtime::send_console_human_on_mob(
            &h.unified.mob_handle(),
            &old_member,
            HUMAN.into(),
            HandlingMode::Queue,
            &accepted,
        )
        .await
        .is_err()
    );
    let replacement_history = h
        .unified
        .mob_runtime()
        .read_session_history(&successor.session_id.to_string(), 0, None)
        .await
        .unwrap();
    assert!(
        !replacement_history
            .messages
            .iter()
            .any(|message| matches!(message, Message::User(user) if user.text_content() == HUMAN))
    );
    h.stop().await;
}
