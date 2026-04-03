#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::collapsible_if,
    clippy::redundant_clone,
    clippy::needless_raw_string_hashes,
    clippy::single_match,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_pattern_matching,
    clippy::ignored_unit_patterns,
    clippy::clone_on_copy,
    clippy::manual_assert,
    clippy::unwrap_in_result,
    clippy::useless_vec
)]
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meerkat::{AgentFactory, Config, build_ephemeral_service};
use meerkat_client::TestClient;
use meerkat_mob::{MobStorage, Prefab};
use meerkat_mobkit::unified_runtime::EventLogError;
use meerkat_mobkit::{
    EventLogConfig, EventLogStore, EventQuery, JsonRpcResponse, MobBootstrapOptions,
    MobBootstrapSpec, MobKitConfig, PersistedEvent, UnifiedEvent, UnifiedRuntime,
    handle_unified_rpc_json,
};
use serde_json::json;

#[derive(Clone)]
struct RecordingStore {
    queries: Arc<Mutex<Vec<EventQuery>>>,
    events: Arc<Vec<PersistedEvent>>,
}

impl EventLogStore for RecordingStore {
    fn append_batch(
        &self,
        _events: Vec<PersistedEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<(), EventLogError>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn query(
        &self,
        query: EventQuery,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PersistedEvent>, EventLogError>> + Send + '_>> {
        let queries = self.queries.clone();
        let events = self.events.clone();
        Box::pin(async move {
            queries.lock().expect("queries").push(query.clone());
            let selected_identity = query.identity.as_deref();
            let selected_member_id = query.member_id.as_deref();
            let filtered = events
                .iter()
                .filter(|event| {
                    let UnifiedEvent::Agent { agent_id, .. } = &event.event else {
                        return selected_identity.is_none() && selected_member_id.is_none();
                    };
                    selected_identity
                        .map(|identity| identity == agent_id)
                        .unwrap_or(true)
                        && selected_member_id
                            .map(|member_id| event.member_id.as_deref() == Some(member_id))
                            .unwrap_or(true)
                })
                .cloned()
                .collect::<Vec<_>>();
            Ok(filtered)
        })
    }
}

async fn build_runtime_with_event_log(
    store: RecordingStore,
) -> (
    tempfile::TempDir,
    UnifiedRuntime,
    Arc<Mutex<Vec<EventQuery>>>,
) {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));

    let mut definition = Prefab::CodingSwarm.definition();
    for profile in definition.profiles.values_mut() {
        profile.model = "gpt-5.2".to_string();
    }

    let query_log = store.queries.clone();
    let runtime = UnifiedRuntime::builder()
        .mob_spec(
            MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service)
                .with_options(MobBootstrapOptions {
                    allow_ephemeral_sessions: true,
                    notify_orchestrator_on_resume: true,
                    default_llm_client: Some(Arc::new(TestClient::default())),
                }),
        )
        .module_config(MobKitConfig {
            modules: vec![],
            discovery: meerkat_mobkit::DiscoverySpec {
                namespace: "phase0-query-events".to_string(),
                modules: vec![],
            },
            pre_spawn: vec![],
        })
        .event_log(EventLogConfig {
            store: Box::new(store),
            ..Default::default()
        })
        .timeout(Duration::from_secs(2))
        .build()
        .await
        .expect("build runtime");

    (temp_dir, runtime, query_log)
}

fn parse_json_rpc(response: &str) -> JsonRpcResponse {
    serde_json::from_str(response).expect("json-rpc response")
}

#[tokio::test]
async fn phase0_contract_006_query_events_forwards_identity_filter_and_payloads() {
    let store = RecordingStore {
        queries: Arc::new(Mutex::new(Vec::new())),
        events: Arc::new(vec![
            PersistedEvent {
                id: "evt-agent-1".to_string(),
                seq: 1,
                timestamp_ms: 10,
                member_id: Some("member-1".to_string()),
                event: UnifiedEvent::Agent {
                    agent_id: "identity:luka".to_string(),
                    event_type: "text_delta".to_string(),
                    payload: Some(json!({
                        "type": "text_delta",
                        "delta": "hello"
                    })),
                },
            },
            PersistedEvent {
                id: "evt-agent-2".to_string(),
                seq: 2,
                timestamp_ms: 11,
                member_id: Some("member-2".to_string()),
                event: UnifiedEvent::Agent {
                    agent_id: "identity:other".to_string(),
                    event_type: "tool_execution_completed".to_string(),
                    payload: Some(json!({
                        "type": "tool_execution_completed",
                        "tool_call_id": "tool-2",
                        "result": "skip"
                    })),
                },
            },
        ]),
    };
    let (_temp_dir, runtime, query_log) = build_runtime_with_event_log(store).await;

    let response = parse_json_rpc(
        &handle_unified_rpc_json(
            &runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "query-1",
                "method": "mobkit/query_events",
                "params": {
                    "identity": "identity:luka",
                    "limit": 10
                }
            })
            .to_string(),
            Duration::from_secs(1),
            None,
            None,
        )
        .await,
    );

    assert!(
        response.error.is_none(),
        "unexpected rpc error: {:?}",
        response.error
    );
    let result = response.result.expect("query result");
    let events = result.as_array().expect("events array");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["id"], json!("evt-agent-1"));
    assert_eq!(events[0]["event"]["kind"], json!("agent"));
    assert_eq!(events[0]["event"]["agent_id"], json!("identity:luka"));
    assert_eq!(events[0]["event"]["payload"]["type"], json!("text_delta"));
    assert_eq!(events[0]["event"]["payload"]["delta"], json!("hello"));

    {
        let seen_queries = query_log.lock().expect("query log");
        assert_eq!(seen_queries.len(), 1);
        assert_eq!(seen_queries[0].identity.as_deref(), Some("identity:luka"));
        assert_eq!(seen_queries[0].limit, Some(10));
    }

    let shutdown = runtime.shutdown().await;
    assert!(shutdown.mob_stop.is_ok());
}
