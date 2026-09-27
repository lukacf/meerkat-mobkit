use super::*;

const RUNTIME: &str = "assistant-identity-runtime";
const IDENTITY: &str = "worker";
const SESSION_A: &str = "00000000-0000-4000-8000-000000000101";
const SESSION_B: &str = "00000000-0000-4000-8000-000000000102";
const MESSAGE_A: &str = "00000000-0000-4000-8000-000000000201";
const MESSAGE_B: &str = "00000000-0000-4000-8000-000000000202";

fn assistant_row(id: Option<&str>, blocks: Vec<Value>) -> Value {
    let mut row = json!({
        "role": "block_assistant", "blocks": blocks, "stop_reason": "end_turn",
        "created_at": "1970-01-01T00:00:00.100Z",
    });
    if let Some(id) = id {
        row["assistant_message_id"] = json!(id);
    }
    row
}

fn text_block(text: &str) -> Value {
    json!({"block_type": "text", "data": {"text": text}})
}

fn tool_block(id: &str, name: &str, args: Value) -> Value {
    json!({"block_type": "tool_use", "data": {"id": id, "name": name, "args": args}})
}

fn image_block() -> Value {
    json!({"block_type": "image", "data": {
        "image_id": "00000000-0000-4000-8000-000000000301",
        "blob_ref": {"blob_id": "sha256:assistant-image", "media_type": "image/png"},
        "media_type": "image/png", "width": 16, "height": 16,
        "revised_prompt": {"disposition": "not_requested"},
        "meta": {"provider": "not_emitted"},
    }})
}

fn project(session: &str, offset: usize, message: Value) -> Vec<NewConsoleFrame> {
    frames_from_session_history_message(RUNTIME, IDENTITY, session, offset, message)
}

#[test]
fn assistant_identity_history_keeps_exact_text_blocks_and_session_scope() -> ConsoleLogResult<()> {
    let blocks = vec![
        text_block("  First"),
        json!({"block_type": "reasoning", "data": {"text": "private reasoning"}}),
        json!({"block_type":"transcript","data":{"text":"\n","source":"spoken"}}),
        text_block("A\u{030a} and \u{00e5}.  "),
        tool_block(
            "peer-call",
            "send_message",
            json!({"peer_id": "peer", "body": "hello"}),
        ),
    ];
    let row = assistant_row(Some(MESSAGE_A), blocks.clone());
    // Parsing through the actual upstream type is part of the contract test.
    let _: Message = serde_json::from_value(row.clone())?;
    let parent = project(SESSION_A, 3, row.clone());
    let child = project(SESSION_B, 3, row.clone());
    assert_eq!(parent.len(), 2, "text and outgoing peer-call siblings");
    for (frames, session) in [(&parent, SESSION_A), (&child, SESSION_B)] {
        for frame in frames {
            assert_eq!(frame.session_id.as_deref(), Some(session));
            assert_eq!(frame.payload["assistant_message_id"], MESSAGE_A);
            assert_ne!(frame.kind, "interaction_complete");
            assert_ne!(frame.kind, "run_completed");
        }
        let text = frames
            .iter()
            .find(|frame| frame.kind == "text_complete")
            .ok_or("canonical assistant text missing")?;
        assert_eq!(text.payload["text"], "  First\nA\u{030a} and \u{00e5}.  ");
        assert_eq!(text.payload["result"], text.payload["text"]);
        assert_eq!(text.payload["message"], row);
        assert_eq!(text.payload["message"]["blocks"], json!(blocks));
    }
    assert_ne!(
        parent[0].dedupe_key, child[0].dedupe_key,
        "fork-inherited IDs are scoped to the actual session"
    );
    let different_id = project(SESSION_A, 4, assistant_row(Some(MESSAGE_B), blocks));
    assert_ne!(
        parent[0].payload["assistant_message_id"],
        different_id[0].payload["assistant_message_id"]
    );
    assert_ne!(parent[0].dedupe_key, different_id[0].dedupe_key);
    Ok(())
}

#[test]
fn assistant_identity_history_stamps_tool_siblings_without_claiming_child_messages()
-> ConsoleLogResult<()> {
    let mut row = assistant_row(
        Some(MESSAGE_A),
        vec![
            tool_block("lookup", "peers", json!({})),
            tool_block(
                "send",
                "send_message",
                json!({"peer_id": "peer", "body": "hello"}),
            ),
            tool_block(
                "spawn",
                "mob_spawn_member",
                json!({"member_id": "child", "initial_message": "Inspect."}),
            ),
        ],
    );
    row["stop_reason"] = json!("tool_use");
    let frames = project(SESSION_A, 5, row);
    let calls = frames
        .iter()
        .filter(|frame| frame.kind == "tool_call_requested")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 3);
    for frame in calls {
        assert_eq!(frame.payload["assistant_message_id"], MESSAGE_A);
        assert_eq!(frame.session_id.as_deref(), Some(SESSION_A));
        assert_ne!(
            frame.payload["tool_call_id"],
            frame.payload["assistant_message_id"]
        );
    }
    let child = frames
        .iter()
        .find(|frame| frame.kind == "user_input")
        .ok_or("spawn child input missing")?;
    assert_eq!(child.identity, "child");
    assert_eq!(child.session_id, None);
    assert!(
        child.payload.get("assistant_message_id").is_none(),
        "the parent's assistant row is not the child's user message identity"
    );
    assert!(
        frames
            .iter()
            .all(|frame| frame.kind != "interaction_complete" && frame.kind != "run_completed")
    );
    Ok(())
}

#[test]
fn assistant_identity_legacy_null_and_malformed_history_are_not_promoted() -> ConsoleLogResult<()> {
    for id in [None, Some(Value::Null)] {
        let mut row = assistant_row(None, vec![text_block("Legacy text")]);
        if let Some(id) = id {
            row["assistant_message_id"] = id;
        }
        let frames = project(SESSION_A, 0, row);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].payload.get("assistant_message_id").is_none());
    }
    for malformed in [
        json!(""),
        json!("not-an-id"),
        json!(12),
        json!({"id": MESSAGE_A}),
    ] {
        let mut row = assistant_row(None, vec![text_block("Must not become authority")]);
        row["assistant_message_id"] = malformed;
        assert!(serde_json::from_value::<Message>(row.clone()).is_err());
        assert!(project(SESSION_A, 0, row).is_empty());
    }
    for row in [
        json!({"role":"user","content":"Human text","assistant_message_id":MESSAGE_A}),
        json!({"role":"tool_results","results":[{"tool_use_id":"tool-a","content":"Result","is_error":false}],"assistant_message_id":MESSAGE_A}),
    ] {
        let frames = project(SESSION_A, 0, row);
        assert!(!frames.is_empty());
        assert!(
            frames
                .iter()
                .all(|frame| frame.payload.get("assistant_message_id").is_none())
        );
    }
    Ok(())
}

#[test]
fn assistant_identity_rich_only_history_has_a_canonical_nonterminal_carrier() -> ConsoleLogResult<()>
{
    for block in [
        json!({"block_type":"reasoning","data":{"text":"Reasoning only"}}),
        json!({"block_type":"server_tool_content","data":{"id":"server-item","kind":{"kind":"web_search"},"content":{"status":"completed"}}}),
        image_block(),
    ] {
        let row = assistant_row(Some(MESSAGE_A), vec![block]);
        let _: Message = serde_json::from_value(row.clone())?;
        let frames = project(SESSION_A, 0, row.clone());
        let canonical = frames
            .iter()
            .find(|frame| frame.kind == "assistant_message")
            .ok_or("rich-only assistant history lost its canonical row")?;
        assert_eq!(canonical.payload["assistant_message_id"], MESSAGE_A);
        assert_eq!(canonical.payload["message"], row);
        assert_eq!(canonical.session_id.as_deref(), Some(SESSION_A));
        assert!(frames.iter().all(|frame| !matches!(
            frame.kind.as_str(),
            "text_complete" | "interaction_complete" | "run_completed"
        )));
    }
    let empty = project(SESSION_A, 0, assistant_row(Some(MESSAGE_A), Vec::new()));
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0].kind, "assistant_message");
    assert_eq!(empty[0].payload["assistant_message_id"], MESSAGE_A);
    assert!(project(SESSION_A, 0, assistant_row(None, Vec::new())).is_empty());
    Ok(())
}

#[tokio::test]
async fn assistant_identity_survives_policy_storage_reopen_and_nested_update()
-> ConsoleLogResult<()> {
    struct RedactText;
    impl ConsoleVisibilityPolicy for RedactText {
        fn redact_payload(&self, frame: &NewConsoleFrame) -> Option<Value> {
            let mut payload = frame.payload.clone();
            payload["text"] = json!("[redacted]");
            payload["result"] = json!("[redacted]");
            payload.as_object_mut()?.remove("message");
            Some(payload)
        }
    }
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("assistant-history.sqlite");
    let store = Arc::new(SqliteConsoleLogStore::open(&path)?);
    let aggregator = MobKitConsoleAggregator::new(store.clone());
    let frame = project(
        SESSION_A,
        0,
        assistant_row(Some(MESSAGE_A), vec![text_block("Private")]),
    )
    .into_iter()
    .next()
    .ok_or("history frame missing")?;
    let outcome =
        append_and_emit_with_policy(&aggregator.inner, frame, Arc::new(RedactText)).await?;
    assert_eq!(outcome.frame.status, ConsoleFrameStatus::Redacted);
    assert_eq!(outcome.frame.payload["assistant_message_id"], MESSAGE_A);
    assert!(outcome.frame.payload.get("message").is_none());
    let updated = update_frame_status_and_emit(
        &aggregator.inner,
        &outcome.frame.id,
        ConsoleFrameStatus::Completed,
    )
    .await?
    .ok_or("stored frame not updated")?;
    assert_eq!(updated.payload["assistant_message_id"], MESSAGE_A);
    let rows = store.query_frames(ConsoleTimelineQuery::default()).await?;
    let marker = rows
        .frames
        .iter()
        .find(|frame| frame.kind == "frame_updated")
        .ok_or("update marker missing")?;
    assert_eq!(
        marker.payload["frame"]["payload"]["assistant_message_id"],
        MESSAGE_A
    );
    assert_eq!(marker.payload["frame"]["session_id"], SESSION_A);
    assert!(
        serde_json::to_value(&updated)?
            .get("assistant_message_id")
            .is_none(),
        "assistant identity stays payload metadata, not a duplicated frame column"
    );
    drop(aggregator);
    drop(store);
    let reopened = SqliteConsoleLogStore::open(&path)?;
    let persisted = reopened
        .frame_by_dedupe_key(&updated.dedupe_key)
        .await?
        .ok_or("reopened frame missing")?;
    assert_eq!(persisted, updated);
    let marker = reopened
        .frame_by_dedupe_key(&marker.dedupe_key)
        .await?
        .ok_or("reopened marker missing")?;
    assert_eq!(
        marker.payload["frame"]["payload"]["assistant_message_id"],
        MESSAGE_A
    );
    Ok(())
}

#[tokio::test]
async fn assistant_identity_live_events_preserve_carriers_without_inheriting_them()
-> ConsoleLogResult<()> {
    use crate::types::{EventEnvelope, UnifiedEvent};
    let (_temp, runtime, _) = super::tests::build_stress_runtime(0, Duration::ZERO).await;
    let entry = super::tests::runtime_entry_for_test(RUNTIME, &runtime);
    let events = ConsoleEventStore::new();
    events
        .register_runtime_identity("rt:worker:1", IDENTITY)
        .await;
    let carriers = vec![
        json!({"type":"turn_started","turn_number":1}),
        json!({"type":"reasoning_delta","delta":"Reason"}),
        json!({"type":"reasoning_complete","content":"Reason"}),
        json!({"type":"text_delta","delta":"candidate"}),
        json!({"type":"text_complete","content":"candidate"}),
        json!({"type":"server_tool_content","id":"provider-item","kind":{"kind":"web_search"},"content":{"status":"completed"}}),
        json!({"type":"assistant_image_appended","image":image_block()["data"]}),
        json!({"type":"turn_completed","stop_reason":"end_turn"}),
        json!({"type":"retrying","retry":{"failure":{"provider":"fixture","kind":"network_timeout","message":"retry"},"plan":{"attempt":1,"max_retries":2,"computed_delay_ms":1,"selected_delay_ms":1,"rate_limit_floor_applied":false,"budget_capped":false}}}),
        json!({"type":"run_completed","session_id":SESSION_A,"result":"hook-rewritten result","usage":{"input_tokens":1,"output_tokens":1,"cache_creation_tokens":null,"cache_read_tokens":null}}),
    ];
    let mut ordered = carriers
        .into_iter()
        .map(|raw| (true, raw))
        .collect::<Vec<_>>();
    ordered.splice(
        4..4,
        [
            json!({"type":"tool_call_requested","id":"tool-1","name":"peers","args":{}}),
            json!({"type":"tool_execution_started","id":"tool-1","name":"peers"}),
            json!({"type":"text_delta","delta":"id-less summary"}),
        ]
        .into_iter()
        .map(|raw| (false, raw)),
    );
    for (index, (has_identity, mut raw)) in ordered.into_iter().enumerate() {
        if has_identity {
            raw["assistant_message_id"] = json!(MESSAGE_A);
        }
        let typed: meerkat_core::AgentEvent = serde_json::from_value(raw)?;
        let mut payload = crate::mob_handle_runtime::console_agent_event_payload(&typed);
        if has_identity {
            assert_eq!(payload["assistant_message_id"], MESSAGE_A);
        } else {
            assert!(payload.get("assistant_message_id").is_none());
        }
        payload["session_id"] = json!(SESSION_A);
        let kind = meerkat_core::event::agent_event_type(&typed);
        events
            .project_unified_event(&EventEnvelope {
                event_id: format!(
                    "{}-{index}",
                    if has_identity { "carrier" } else { "idless" }
                ),
                source: "agent".into(),
                timestamp_ms: index as u64,
                event: UnifiedEvent::Agent {
                    agent_id: "rt:worker:1".into(),
                    event_type: kind.into(),
                    payload: Some(payload),
                },
            })
            .await;
    }
    let replay = events
        .replay_all(None)
        .await
        .map_err(|error| std::io::Error::other(format!("replay: {error:?}")))?
        .into_iter()
        .filter(|event| event.identity == IDENTITY)
        .collect::<Vec<_>>();
    assert_eq!(replay.len(), 13);
    for envelope in replay {
        let frame = frame_from_console_event(&entry, envelope);
        assert_eq!(frame.session_id.as_deref(), Some(SESSION_A));
        if frame
            .id
            .as_deref()
            .is_some_and(|id| id.starts_with("carrier-"))
        {
            assert_eq!(frame.payload["assistant_message_id"], MESSAGE_A);
            if frame.kind == "interaction_complete" {
                assert_eq!(frame.payload["source_event_type"], "run_completed");
                assert_eq!(frame.payload["result"], "hook-rewritten result");
            } else {
                assert_eq!(
                    frame.status,
                    ConsoleFrameStatus::Delivered,
                    "text/turn completion is not canonical history or durable commit"
                );
            }
        } else {
            assert!(frame.payload.get("assistant_message_id").is_none());
        }
        assert_eq!(frame.source.kind, ConsoleFrameSourceKind::ConsoleEvent);
    }
    runtime.mob_handle().stop().await?;
    Ok(())
}

#[tokio::test]
async fn assistant_identity_whole_payload_redaction_does_not_restore_removed_identity()
-> ConsoleLogResult<()> {
    struct RemovePayload;
    impl ConsoleVisibilityPolicy for RemovePayload {
        fn redact_payload(&self, _: &NewConsoleFrame) -> Option<Value> {
            Some(json!({"redacted":true}))
        }
    }
    let aggregator = MobKitConsoleAggregator::in_memory();
    let frame = project(
        SESSION_A,
        0,
        assistant_row(Some(MESSAGE_A), vec![text_block("Private")]),
    )
    .into_iter()
    .next()
    .ok_or("history frame missing")?;
    let outcome =
        append_and_emit_with_policy(&aggregator.inner, frame, Arc::new(RemovePayload)).await?;
    assert_eq!(outcome.frame.payload, json!({"redacted":true}));
    assert!(outcome.frame.payload.get("assistant_message_id").is_none());
    Ok(())
}

fn history_page(
    session: &str,
    ids: &[&str],
) -> ConsoleLogResult<meerkat_core::service::SessionHistoryPage> {
    let messages = ids
        .iter()
        .map(|id| serde_json::from_value(assistant_row(Some(id), vec![text_block("Same answer")])))
        .collect::<Result<Vec<Message>, _>>()?;
    Ok(meerkat_core::service::SessionHistoryPage::from_messages(
        meerkat_core::types::SessionId::parse(session)?,
        &messages,
        meerkat_core::service::SessionHistoryQuery::default(),
    ))
}

fn observed_live_frame(session: &str, event: &str, id: &str) -> NewConsoleFrame {
    NewConsoleFrame {
        id: None,
        dedupe_key: format!("live:{session}:{event}:{id}"),
        timestamp_ms: 1,
        runtime_key: RUNTIME.into(),
        identity: IDENTITY.into(),
        conversation_id: Some(IDENTITY.into()),
        session_id: Some(session.into()),
        kind: event.into(),
        status: ConsoleFrameStatus::Delivered,
        payload: json!({"assistant_message_id":id,"session_id":session,"delta":"provisional"}),
        source: ConsoleFrameSource {
            member_provenance: None,
            kind: ConsoleFrameSourceKind::ConsoleEvent,
            source_cursor: None,
        },
        source_event_id: None,
        interaction_id: None,
        turn_id: None,
        run_id: None,
        parent_frame_id: None,
        caused_by_frame_id: None,
    }
}

#[tokio::test]
async fn assistant_identity_complete_snapshot_covers_only_prior_events_and_coalesces_after_reopen()
-> ConsoleLogResult<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("snapshot.sqlite");
    let store = Arc::new(SqliteConsoleLogStore::open(&path)?);
    let aggregator = MobKitConsoleAggregator::new(store.clone());
    let covered = store
        .append_if_absent(observed_live_frame(SESSION_A, "text_delta", MESSAGE_A))
        .await?
        .frame;
    let observation =
        observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    let page = complete_current_history_page(SESSION_A, history_page(SESSION_A, &[MESSAGE_A])?)
        .ok_or("complete page rejected")?;
    let snapshot = assistant_history_snapshot_frame(
        RUNTIME,
        IDENTITY,
        SESSION_A,
        &observation,
        &page.messages,
    )
    .ok_or("complete assistant snapshot missing")?;
    assert_eq!(snapshot.kind, "assistant_history_snapshot");
    assert_eq!(snapshot.source.kind, ConsoleFrameSourceKind::SessionHistory);
    assert_eq!(snapshot.session_id.as_deref(), Some(SESSION_A));
    assert_eq!(snapshot.payload["complete"], true);
    assert_eq!(
        snapshot.payload["assistant_message_ids"],
        json!([MESSAGE_A])
    );
    assert_eq!(
        snapshot.payload["observed_through"],
        serde_json::to_value(&covered.cursor)?
    );
    let later = store
        .append_if_absent(observed_live_frame(SESSION_A, "text_delta", MESSAGE_B))
        .await?
        .frame;
    let snapshot = store.append_if_absent(snapshot).await?.frame;
    assert!(later.cursor.seq() > covered.cursor.seq());
    assert_eq!(
        snapshot.payload["observed_through"],
        serde_json::to_value(&covered.cursor)?
    );
    let next =
        observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    let advanced =
        assistant_history_snapshot_frame(RUNTIME, IDENTITY, SESSION_A, &next, &page.messages)
            .ok_or("later live event needs a fresh coverage snapshot")?;
    store.append_if_absent(advanced).await?;
    let stable =
        observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    assert!(
        assistant_history_snapshot_frame(RUNTIME, IDENTITY, SESSION_A, &stable, &page.messages)
            .is_none(),
        "snapshot appends must not trigger endless fresh snapshots"
    );
    assert!(
        stable.canonical_frames.is_empty(),
        "control markers are not canonical message rows"
    );
    drop(aggregator);
    drop(store);
    let reopened = MobKitConsoleAggregator::new(Arc::new(SqliteConsoleLogStore::open(&path)?));
    let stable =
        observe_runtime_notice_attempts(&reopened.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    assert!(
        assistant_history_snapshot_frame(RUNTIME, IDENTITY, SESSION_A, &stable, &page.messages)
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn assistant_identity_complete_snapshot_distinguishes_empty_head_and_restored_ids()
-> ConsoleLogResult<()> {
    let aggregator = MobKitConsoleAggregator::in_memory();
    let empty_observation = RuntimeNoticeObservation::default();
    assert!(
        assistant_history_snapshot_frame(RUNTIME, IDENTITY, SESSION_A, &empty_observation, &[])
            .is_none(),
        "an untouched legacy session needs no assistant identity marker"
    );
    aggregator
        .store()
        .append_if_absent(observed_live_frame(SESSION_A, "text_delta", MESSAGE_A))
        .await?;
    let first = history_page(SESSION_A, &[MESSAGE_A])?;
    let mut last_key = None;
    for messages in [&first.messages[..], &[][..], &first.messages[..]] {
        let observation =
            observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A)
                .await?;
        let snapshot =
            assistant_history_snapshot_frame(RUNTIME, IDENTITY, SESSION_A, &observation, messages)
                .ok_or("changed head must publish a fresh complete snapshot")?;
        assert_ne!(last_key.as_ref(), Some(&snapshot.dedupe_key));
        assert_eq!(
            snapshot.payload["assistant_message_ids"]
                .as_array()
                .map(Vec::len),
            Some(messages.len())
        );
        last_key = Some(snapshot.dedupe_key.clone());
        aggregator.store().append_if_absent(snapshot).await?;
    }
    let mut partial = first.clone();
    partial.has_more = true;
    assert!(complete_current_history_page(SESSION_A, partial).is_none());
    let mut short = first.clone();
    short.message_count += 1;
    assert!(complete_current_history_page(SESSION_A, short).is_none());
    assert!(complete_current_history_page(SESSION_B, first).is_none());
    Ok(())
}

#[tokio::test]
async fn assistant_identity_snapshot_rejects_reset_and_rebuilds_same_cursor_prefix()
-> ConsoleLogResult<()> {
    let store = Arc::new(InMemoryConsoleLogStore::new());
    let aggregator = MobKitConsoleAggregator::new(store.clone());
    let mut live = observed_live_frame(SESSION_A, "text_delta", MESSAGE_A);
    live.id = Some("reused-live-frame-id".into());
    store.append_if_absent(live.clone()).await?;
    let observation =
        observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    let mut snapshot =
        assistant_history_snapshot_frame(RUNTIME, IDENTITY, SESSION_A, &observation, &[])
            .ok_or("snapshot missing")?;
    snapshot.id = Some("reused-snapshot-frame-id".into());
    store.append_if_absent(snapshot.clone()).await?;
    let before =
        observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    store.clear_frames().await?;
    // Reuse the old tail and cursors but replace an earlier canonical row.
    // A tail-only cache check cannot detect this reset.
    let replacement = project(
        SESSION_A,
        0,
        assistant_row(Some(MESSAGE_B), vec![text_block("Replacement")]),
    )
    .into_iter()
    .next()
    .ok_or("replacement missing")?;
    store.append_if_absent(replacement).await?;
    store.append_if_absent(snapshot).await?;
    assert!(!assistant_history_prefix_is_current(&aggregator.inner, IDENTITY, &before).await?);
    let rebuilt =
        observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    assert_eq!(rebuilt.canonical_frames.len(), 1);
    assert_eq!(
        rebuilt.assistant_frontier, 0,
        "old live frontier must not survive reset"
    );
    assert!(rebuilt.has_assistant_history);
    Ok(())
}

#[tokio::test]
async fn assistant_identity_redacted_snapshots_keep_coalescing_without_restoring_payload()
-> ConsoleLogResult<()> {
    struct RedactSnapshot;
    impl ConsoleVisibilityPolicy for RedactSnapshot {
        fn redact_payload(&self, _: &NewConsoleFrame) -> Option<Value> {
            Some(json!({"redacted":true}))
        }
    }
    let aggregator = MobKitConsoleAggregator::in_memory();
    aggregator
        .store()
        .append_if_absent(observed_live_frame(SESSION_A, "text_delta", MESSAGE_A))
        .await?;
    let observation =
        observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    let page = history_page(SESSION_A, &[MESSAGE_A])?;
    let snapshot = assistant_history_snapshot_frame(
        RUNTIME,
        IDENTITY,
        SESSION_A,
        &observation,
        &page.messages,
    )
    .ok_or("snapshot missing")?;
    let saved = append_and_emit_with_policy(&aggregator.inner, snapshot, Arc::new(RedactSnapshot))
        .await?
        .frame;
    assert_eq!(saved.payload, json!({"redacted":true}));
    let observation =
        observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    assert!(
        assistant_history_snapshot_frame(
            RUNTIME,
            IDENTITY,
            SESSION_A,
            &observation,
            &page.messages
        )
        .is_none()
    );
    assert!(observation.canonical_frames.is_empty());
    Ok(())
}

struct NoRevisionStore {
    inner: InMemoryConsoleLogStore,
    scans: std::sync::atomic::AtomicUsize,
    fail_scan: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl ConsoleLogStore for NoRevisionStore {
    async fn append_if_absent(&self, frame: NewConsoleFrame) -> ConsoleLogResult<AppendOutcome> {
        self.inner.append_if_absent(frame).await
    }
    async fn update_frame_status(
        &self,
        id: &str,
        status: ConsoleFrameStatus,
    ) -> ConsoleLogResult<Option<ConsoleFrame>> {
        self.inner.update_frame_status(id, status).await
    }
    async fn query_frames(
        &self,
        query: ConsoleTimelineQuery,
    ) -> ConsoleLogResult<ConsoleTimelinePage> {
        self.scans.fetch_add(1, Ordering::SeqCst);
        if self.fail_scan.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("injected assistant prefix scan failure").into());
        }
        self.inner.query_frames(query).await
    }
    async fn frame_by_dedupe_key(&self, key: &str) -> ConsoleLogResult<Option<ConsoleFrame>> {
        self.inner.frame_by_dedupe_key(key).await
    }
    async fn latest_cursor(&self) -> ConsoleLogResult<Option<ConsoleCursor>> {
        self.inner.latest_cursor().await
    }
    async fn clear_frames(&self) -> ConsoleLogResult<()> {
        self.inner.clear_frames().await
    }
    async fn record_source_watermark(
        &self,
        runtime: &str,
        kind: ConsoleFrameSourceKind,
        cursor: &str,
    ) -> ConsoleLogResult<()> {
        self.inner
            .record_source_watermark(runtime, kind, cursor)
            .await
    }
    async fn source_watermark(
        &self,
        runtime: &str,
        kind: ConsoleFrameSourceKind,
    ) -> ConsoleLogResult<Option<String>> {
        self.inner.source_watermark(runtime, kind).await
    }
}

#[tokio::test]
async fn assistant_identity_custom_store_checks_a_fresh_complete_prefix_and_reports_failure()
-> ConsoleLogResult<()> {
    let store = Arc::new(NoRevisionStore {
        inner: InMemoryConsoleLogStore::new(),
        scans: 0.into(),
        fail_scan: false.into(),
    });
    let aggregator = MobKitConsoleAggregator::new(store.clone());
    store
        .append_if_absent(observed_live_frame(SESSION_A, "text_delta", MESSAGE_A))
        .await?;
    let observation =
        observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    assert!(assistant_history_prefix_is_current(&aggregator.inner, IDENTITY, &observation).await?);
    let scans = store.scans.load(Ordering::SeqCst);
    let next =
        observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, SESSION_A).await?;
    assert!(
        store.scans.load(Ordering::SeqCst) > scans,
        "None revision must not reuse a cached absence proof"
    );
    store.fail_scan.store(true, Ordering::SeqCst);
    assert!(
        assistant_history_prefix_is_current(&aggregator.inner, IDENTITY, &next)
            .await
            .is_err()
    );
    store.fail_scan.store(false, Ordering::SeqCst);
    store.clear_frames().await?;
    store
        .append_if_absent(observed_live_frame(SESSION_A, "text_delta", MESSAGE_B))
        .await?;
    assert!(!assistant_history_prefix_is_current(&aggregator.inner, IDENTITY, &next).await?);
    Ok(())
}

#[tokio::test]
async fn assistant_identity_known_history_is_not_deduplicated_by_identical_text()
-> ConsoleLogResult<()> {
    let aggregator = MobKitConsoleAggregator::in_memory();
    for (offset, id) in [MESSAGE_A, MESSAGE_B].into_iter().enumerate() {
        let frame = project(
            SESSION_A,
            offset,
            assistant_row(Some(id), vec![text_block("Same answer")]),
        )
        .into_iter()
        .next()
        .ok_or("canonical row missing")?;
        assert!(
            !history_frame_has_existing_counterpart(&aggregator.inner, &frame).await?,
            "a different assistant ID must not be swallowed by legacy content matching"
        );
        aggregator.store().append_if_absent(frame).await?;
    }
    assert_eq!(
        aggregator
            .store()
            .query_frames(ConsoleTimelineQuery::default())
            .await?
            .frames
            .len(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn assistant_identity_backfill_publishes_only_complete_current_images_and_restores_rows()
-> ConsoleLogResult<()> {
    let (_temp, runtime, service) = super::tests::build_stress_runtime(1, Duration::ZERO).await;
    let store = Arc::new(InMemoryConsoleLogStore::new());
    let aggregator = MobKitConsoleAggregator::new(store.clone());
    let mut entry = super::tests::runtime_entry_for_test(RUNTIME, &runtime);
    entry.identity_namespace.clear();
    let member = runtime
        .mob_handle()
        .list_members_observation_snapshot()
        .await
        .into_iter()
        .next()
        .ok_or("fixture member missing")?;
    let record = identity_record_for_member(&entry, &runtime.mob_handle(), &member)
        .await
        .ok_or("fixture identity missing")?;
    let session_id = record.session_id.clone().ok_or("fixture session missing")?;
    let mob_id = entry.runtime.handle().mob_id().to_string();
    let provenance = ConsoleFrameMemberProvenance {
        identity: record.clone(),
        member: ConsoleMember {
            agent_identity: record.identity.clone(),
            role: "worker".into(),
            state: "running".into(),
            model_capabilities: Default::default(),
            runtime_mode: None,
            session_id: record.session_id.clone(),
            wired_to: Vec::new(),
            labels: BTreeMap::new(),
            progress: None,
        },
        primary_mob_id: mob_id.clone(),
        source_mob_id: mob_id,
    };
    let target = SessionBackfillTarget {
        assistant_refresh: assistant_history_refresh::AssistantHistoryRefreshReason::Recovery,
        provenance: Some(provenance),
        entry: entry.clone(),
        record,
        session_id: session_id.clone(),
    };
    aggregator
        .inner
        .runtimes
        .write()
        .map_err(|_| std::io::Error::other("runtime fixture lock"))?
        .insert(RUNTIME.into(), entry);
    let script =
        |page| service.script_history([super::tests::ScriptedHistoryRead { page, gate: None }]);
    let first = history_page(&session_id, &[MESSAGE_A])?;
    let mut partial = first.clone();
    partial.has_more = true;
    for page in [
        None,
        Some(partial),
        Some(history_page(SESSION_B, &[MESSAGE_A])?),
    ] {
        script(page);
        backfill_one_session_history(aggregator.inner.clone(), target.clone(), true).await?;
        assert!(
            store
                .query_frames(ConsoleTimelineQuery::default())
                .await?
                .frames
                .iter()
                .all(|frame| frame.kind != "assistant_history_snapshot")
        );
    }
    let reads = service.read_calls();
    let replacement: Message = serde_json::from_value(assistant_row(
        None,
        vec![text_block("Replacement without identity")],
    ))?;
    let rewritten = meerkat_core::service::SessionHistoryPage::from_messages(
        meerkat_core::types::SessionId::parse(&session_id)?,
        &[replacement],
        meerkat_core::service::SessionHistoryQuery::default(),
    );
    let images = [
        first.clone(),
        history_page(&session_id, &[MESSAGE_B])?,
        rewritten,
        history_page(&session_id, &[])?,
        first.clone(),
    ];
    for page in images {
        let expected_messages = serde_json::to_value(&page.messages)?;
        let ids: Vec<_> = page
            .messages
            .iter()
            .filter_map(|message| {
                let Message::BlockAssistant(row) = message else {
                    return None;
                };
                row.assistant_message_id.as_ref()
            })
            .collect();
        let expected_ids = serde_json::to_value(ids)?;
        script(Some(page));
        backfill_one_session_history(aggregator.inner.clone(), target.clone(), true).await?;
        let rows = store
            .query_frames(ConsoleTimelineQuery::default())
            .await?
            .frames;
        let latest = rows
            .iter()
            .rev()
            .find(|frame| frame.kind == "assistant_history_snapshot")
            .ok_or("complete backfill snapshot missing")?;
        assert_eq!(latest.payload["assistant_message_ids"], expected_ids);
        let position_map = rows
            .iter()
            .rev()
            .find(|frame| frame.kind == "runtime_notice_snapshot")
            .ok_or("paired position map missing")?;
        assert_eq!(
            position_map.payload["observed_through"],
            latest.payload["observed_through"]
        );
        assert!(
            position_map.payload.get("history_positions_mode").is_none(),
            "identity projection uses a complete position map"
        );
        let boundary = ConsoleCursor::from(
            position_map.payload["observed_through"]
                .as_str()
                .ok_or("cursor missing")?,
        )
        .seq()
        .ok_or("invalid cursor")?;
        let positions = position_map.payload["history_positions"]
            .as_array()
            .ok_or("positions missing")?;
        let mut visible: Vec<_> = rows
            .iter()
            .filter(|frame| {
                frame.source.kind == ConsoleFrameSourceKind::SessionHistory
                    && frame.payload["message"]["role"] == "block_assistant"
                    && (frame
                        .cursor
                        .seq()
                        .is_some_and(|sequence| sequence > boundary)
                        || positions
                            .iter()
                            .any(|position| position["frame_id"] == frame.id))
            })
            .collect();
        visible.sort_by_key(|frame| frame.source.source_cursor.clone());
        assert_eq!(
            json!(
                visible
                    .iter()
                    .map(|frame| &frame.payload["message"])
                    .collect::<Vec<_>>()
            ),
            expected_messages,
            "complete maps must remove ID rows, show no-ID rewrites, and restore original rows without runtime notices"
        );
    }
    assert_eq!(
        service.read_calls(),
        reads + 5,
        "one owner history read per complete image"
    );
    let before = store.latest_cursor().await?;
    script(Some(first));
    backfill_one_session_history(aggregator.inner.clone(), target, true).await?;
    assert_eq!(
        store.latest_cursor().await?,
        before,
        "unchanged history must not grow snapshots"
    );
    runtime.mob_handle().stop().await?;
    Ok(())
}

#[tokio::test]
async fn assistant_identity_pending_refresh_retries_unchanged_head_through_discovery()
-> ConsoleLogResult<()> {
    let (_temp, runtime, service) = super::tests::build_stress_runtime(1, Duration::ZERO).await;
    let store = Arc::new(InMemoryConsoleLogStore::new());
    let aggregator = MobKitConsoleAggregator::new(store.clone());
    let mut entry = super::tests::runtime_entry_for_test(RUNTIME, &runtime);
    entry.identity_namespace.clear();
    let member = runtime
        .mob_handle()
        .list_members_observation_snapshot()
        .await
        .into_iter()
        .next()
        .ok_or("fixture member missing")?;
    let record = identity_record_for_member(&entry, &runtime.mob_handle(), &member)
        .await
        .ok_or("fixture identity missing")?;
    let session_id = record.session_id.clone().ok_or("fixture session missing")?;
    let machine = meerkat_mob::MobSessionService::runtime_adapter(&service)
        .ok_or("fixture must expose the real runtime machine")?;
    let typed_session_id = meerkat_core::SessionId::parse(&session_id)?;
    assert!(
        machine.contains_session(&typed_session_id).await,
        "fixture session must be registered with its real machine"
    );
    // Spawn acknowledges admission, so await the fresh fixture's initial
    // turn before testing recovery. A terminal run witness also rules out
    // the idle gap before queued input starts; commit status alone cannot.
    let handle = entry.runtime.handle();
    let member_id =
        crate::member_comms_id::roster_member_id_for_supplied_id(&record.runtime_member_id);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut last_observation = String::from("startup observation not completed");
    tokio::time::timeout_at(deadline, async {
        loop {
            let execution =
                meerkat_mob::MobSessionService::execution_snapshot(&service, &typed_session_id)
                    .await?;
            let status = crate::member_status_observation::observe_member_status_until(
                &handle, &member_id, deadline,
            )
            .await?;
            let pending = entry.runtime.session_commit_pending(&session_id).await;
            last_observation = format!(
                "execution={execution:?}, session={:?}, progress={:?}, commit_pending={pending:?}",
                status.current_session_id, status.progress,
            );
            if execution.as_ref().is_some_and(|snapshot| {
                snapshot.turn_terminal && snapshot.terminal_run_id.is_some()
            }) && status.current_session_id.as_ref() == Some(&typed_session_id)
                && status.progress.as_ref().is_some_and(|progress| {
                    progress.run_state == meerkat_mob::MemberRunState::Idle
                        && progress.in_flight_work == 0
                })
                && pending == Some(false)
            {
                return Ok::<(), ConsoleLogError>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| {
        std::io::Error::other(format!(
            "fixture initial turn did not commit before recovery: {last_observation}"
        ))
    })??;
    assert_eq!(
        entry.runtime.session_commit_pending(&session_id).await,
        Some(false),
        "settled retries must observe the real runtime commit authority"
    );
    let mut target = SessionBackfillTarget {
        assistant_refresh: assistant_history_refresh::AssistantHistoryRefreshReason::Recovery,
        provenance: None,
        entry: entry.clone(),
        record: record.clone(),
        session_id: session_id.clone(),
    };
    aggregator
        .inner
        .runtimes
        .write()
        .map_err(|_| std::io::Error::other("runtime fixture lock"))?
        .insert(RUNTIME.into(), entry);
    // Pending and unknown commit observations both map to Pending in the guard
    // unit tests. Exercise that result at the orchestration boundary here, then
    // use the production observer for settlement with either cache state.
    for (index, evict_cache) in [false, true].into_iter().enumerate() {
        let mut live = observed_live_frame(&session_id, "text_delta", MESSAGE_A);
        live.identity = record.identity.clone();
        live.conversation_id = Some(record.identity.clone());
        live.dedupe_key = format!("failed-attempt-{index}");
        store.append_if_absent(live.clone()).await?;
        let before = store
            .query_frames(ConsoleTimelineQuery::default())
            .await?
            .frames
            .iter()
            .filter(|frame| frame.kind == "assistant_history_snapshot")
            .count();
        target.assistant_refresh =
            assistant_history_refresh::AssistantHistoryRefreshReason::Recovery;
        service.script_history([super::tests::ScriptedHistoryRead {
            page: Some(history_page(&session_id, &[])?),
            gate: None,
        }]);
        let mut pending_observations = 0;
        let reads_before_pending = service.read_calls();
        backfill_one_session_history_with_refresh_observer(
            aggregator.inner.clone(),
            target.clone(),
            true,
            |observed_entry, observed_record, observed_session, requested| {
                assert!(requested, "recovery must request a settlement observation");
                assert_eq!(observed_entry.registration_id, target.entry.registration_id);
                assert_eq!(observed_record.identity, record.identity);
                assert_eq!(observed_session, session_id);
                pending_observations += 1;
                Box::pin(std::future::ready(
                    assistant_history_refresh::AssistantHistoryRefreshGate::Pending,
                ))
            },
        )
        .await?;
        assert_eq!(pending_observations, 1);
        assert_eq!(service.read_calls(), reads_before_pending + 1);
        assert_eq!(
            store
                .query_frames(ConsoleTimelineQuery::default())
                .await?
                .frames
                .iter()
                .filter(|frame| frame.kind == "assistant_history_snapshot")
                .count(),
            before
        );
        let key = (
            RUNTIME.to_string(),
            record.identity.clone(),
            session_id.clone(),
        );
        let retry_key = (
            target.entry.registration_id,
            RUNTIME.to_string(),
            record.identity.clone(),
            session_id.clone(),
        );
        assert!(
            aggregator
                .inner
                .assistant_history_retries
                .lock()
                .map_err(|_| std::io::Error::other("fixture retry lock"))?
                .contains(&retry_key)
        );
        {
            let mut cache = aggregator
                .inner
                .notice_observations
                .lock()
                .map_err(|_| std::io::Error::other("fixture cache lock"))?;
            let observation = cache
                .entries
                .get_mut(&key)
                .ok_or("pending retry observation missing")?;
            assert!(!observation.verified_assistant_settled);
            // Exercise the bounded frontier cache path as well as ordinary
            // freshness gating, with identical history and live frontier.
            observation.history_index_complete = false;
            observation.canonical_frames.clear();
            if evict_cache {
                for other in 0..17 {
                    cache.insert(
                        (
                            RUNTIME.to_string(),
                            format!("other-{other}"),
                            format!("other-session-{other}"),
                        ),
                        RuntimeNoticeObservation::default(),
                    );
                }
                assert!(
                    !cache.entries.contains_key(&key),
                    "fixture must evict the pending session projection cache"
                );
            }
        }
        if evict_cache {
            store.clear_frames().await?;
            store.append_if_absent(live).await?;
        }
        target.assistant_refresh =
            assistant_history_refresh::AssistantHistoryRefreshReason::PositiveOnly;
        let watermark_runtime_key = session_history_watermark_runtime_key(RUNTIME, &session_id);
        record_session_history_watermark(&aggregator.inner, &watermark_runtime_key, &session_id, 0)
            .await?;
        let watermark = store
            .source_watermark(
                &watermark_runtime_key,
                ConsoleFrameSourceKind::SessionHistory,
            )
            .await?
            .ok_or("fresh retry watermark missing")?;
        assert!(session_history_watermark_is_fresh(
            &watermark,
            &session_id,
            current_time_ms(),
        ));
        let reads = service.read_calls();
        service.script_history([super::tests::ScriptedHistoryRead {
            page: Some(history_page(&session_id, &[])?),
            gate: None,
        }]);
        backfill_one_session_history(aggregator.inner.clone(), target.clone(), false).await?;
        let rows = store
            .query_frames(ConsoleTimelineQuery::default())
            .await?
            .frames;
        let latest = rows
            .iter()
            .rev()
            .find(|frame| frame.kind == "assistant_history_snapshot")
            .ok_or("settled retry snapshot missing")?;
        assert_eq!(latest.payload["assistant_message_ids"], json!([]));
        let position_map = rows
            .iter()
            .rev()
            .find(|frame| frame.kind == "runtime_notice_snapshot")
            .ok_or("settled retry position map missing")?;
        assert_eq!(
            position_map.payload["observed_through"], latest.payload["observed_through"],
            "retry must publish assistant membership and positions at the same cutoff"
        );
        assert!(position_map.payload.get("history_positions_mode").is_none());
        assert_eq!(position_map.payload["history_positions"], json!([]));
        assert_eq!(
            rows.iter()
                .filter(|frame| frame.kind == "assistant_history_snapshot")
                .count(),
            if evict_cache { 1 } else { before + 1 }
        );
        assert_eq!(
            service.read_calls(),
            reads + 1,
            "pending request must bypass freshness without rereading unchanged owner history"
        );
        assert!(
            !aggregator
                .inner
                .assistant_history_retries
                .lock()
                .map_err(|_| std::io::Error::other("fixture cache lock"))?
                .contains(&retry_key)
        );
    }
    runtime.mob_handle().stop().await?;
    Ok(())
}

#[test]
fn assistant_identity_history_refreshes_after_extraction_compaction_and_truncation() {
    for kind in [
        "compaction_completed",
        "extraction_succeeded",
        "extraction_failed",
        "stream_truncated",
    ] {
        let mut frame = observed_live_frame(SESSION_A, kind, MESSAGE_A);
        frame.payload = json!({"session_id":SESSION_A});
        assert!(
            console_event_should_refresh_session_history(&frame),
            "{kind} must refresh complete history"
        );
    }
}
