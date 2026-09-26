use super::*;
use meerkat_core::event::AgentEvent;
use meerkat_core::lifecycle::{InputId, RunId};
use meerkat_core::types::{RuntimeAppendOrigin, SessionId, SystemNoticeKind, SystemNoticeMessage};
use std::sync::atomic::AtomicUsize;

const RUNTIME: &str = "notice-runtime";
const IDENTITY: &str = "notice-worker";

fn notice(session: &SessionId, run: u128, input: u128) -> SystemNoticeMessage {
    let mut notice = SystemNoticeMessage::new(SystemNoticeKind::Generic, "Exact boundary notice.");
    notice.runtime_origin = Some(RuntimeAppendOrigin {
        session_id: session.clone(),
        run_id: RunId(uuid::Uuid::from_u128(run)),
        input_id: InputId(uuid::Uuid::from_u128(input)),
        append_ordinal: 0,
    });
    notice
}

fn frame(key: &str, session: &SessionId, kind: &str, payload: Value) -> NewConsoleFrame {
    NewConsoleFrame {
        id: None,
        dedupe_key: key.to_string(),
        timestamp_ms: 1,
        runtime_key: RUNTIME.to_string(),
        identity: IDENTITY.to_string(),
        conversation_id: Some(IDENTITY.to_string()),
        session_id: Some(session.to_string()),
        kind: kind.to_string(),
        status: ConsoleFrameStatus::Completed,
        payload,
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

fn applied(key: &str, session: &SessionId, run: u128, input: u128) -> NewConsoleFrame {
    let mut frame = frame(
        key,
        session,
        "boundary_append_applied",
        json!(AgentEvent::BoundaryAppendApplied {
            run_id: RunId(uuid::Uuid::from_u128(run)),
            input_id: InputId(uuid::Uuid::from_u128(input)),
            content: "Model compatibility projection.".into(),
            append_count: 1,
            notices: vec![notice(session, run, input)],
            transcript_start: Some(2),
        }),
    );
    frame.run_id = Some(uuid::Uuid::from_u128(run).to_string());
    frame
}

fn attempt(run: u128, input: u128) -> (String, String) {
    (
        uuid::Uuid::from_u128(run).to_string(),
        uuid::Uuid::from_u128(input).to_string(),
    )
}

async fn observe(
    aggregator: &MobKitConsoleAggregator,
    session: &SessionId,
) -> ConsoleLogResult<RuntimeNoticeObservation> {
    observe_runtime_notice_attempts(&aggregator.inner, RUNTIME, IDENTITY, &session.to_string())
        .await
}

#[tokio::test]
async fn notice_scan_reaches_exact_attempts_before_and_after_one_thousand_rows()
-> ConsoleLogResult<()> {
    let aggregator = MobKitConsoleAggregator::in_memory();
    let session = SessionId::new();
    let mut expected = BTreeSet::new();
    let mut relevant_frontier = 0;
    for index in 0_u128..2_105 {
        let row = if [0, 999, 1_000, 2_000].contains(&index) {
            expected.insert(attempt(10, 100 + index));
            applied(&format!("applied-{index}"), &session, 10, 100 + index)
        } else {
            frame(
                &format!("noise-{index}"),
                &session,
                "text_delta",
                json!({ "delta": "." }),
            )
        };
        let is_notice = row.kind == "boundary_append_applied";
        let saved = aggregator.store().append_if_absent(row).await?.frame;
        if is_notice {
            relevant_frontier = saved.cursor.seq().ok_or("missing assigned cursor")?;
        }
    }
    let observed = observe(&aggregator, &session).await?;
    assert_eq!(observed.observed_through, 2_105);
    assert_eq!(observed.relevant_frontier, relevant_frontier);
    assert_eq!(observed.attempts, expected);
    assert!(!observed.has_notice_history);
    assert!(observed.previous_snapshot.is_none());
    Ok(())
}

#[tokio::test]
async fn notice_scan_rejects_conflicting_source_scope_and_typed_attempt_identity()
-> ConsoleLogResult<()> {
    let aggregator = MobKitConsoleAggregator::in_memory();
    let session = SessionId::new();
    let other_session = SessionId::new();
    let accepted = aggregator
        .store()
        .append_if_absent(applied("accepted", &session, 10, 20))
        .await?
        .frame;
    for index in 0..12 {
        let mut invalid = applied(&format!("invalid-{index}"), &session, 30, 40 + index);
        match index {
            0 => invalid.runtime_key = "other-runtime".to_string(),
            1 => invalid.session_id = Some(other_session.to_string()),
            2 => invalid.identity = "other-worker".to_string(),
            3 => invalid.source.kind = ConsoleFrameSourceKind::SessionHistory,
            4 => invalid.source.kind = ConsoleFrameSourceKind::Synthetic,
            5 => invalid.run_id = Some(uuid::Uuid::from_u128(31).to_string()),
            6 => invalid.payload["run_id"] = json!(uuid::Uuid::from_u128(31)),
            7 => invalid.payload["input_id"] = json!(uuid::Uuid::from_u128(99)),
            8 => {
                invalid.payload["notices"][0]["runtime_origin"]["session_id"] = json!(other_session)
            }
            9 => {
                invalid.payload["notices"][0]["runtime_origin"]["run_id"] =
                    json!(uuid::Uuid::from_u128(31))
            }
            10 => {
                invalid.payload["notices"][0]["runtime_origin"]["input_id"] =
                    json!(uuid::Uuid::from_u128(99))
            }
            _ => invalid.payload["notices"][0]["runtime_origin"] = Value::Null,
        }
        aggregator.store().append_if_absent(invalid).await?;
    }
    let observed = observe(&aggregator, &session).await?;
    assert_eq!(observed.attempts, BTreeSet::from([attempt(10, 20)]));
    assert_eq!(
        observed.relevant_frontier,
        accepted.cursor.seq().ok_or("missing assigned cursor")?
    );
    assert!(!observed.has_notice_history);
    assert!(observed.previous_snapshot.is_none());
    Ok(())
}

#[tokio::test]
async fn notice_scan_uses_latest_exact_history_snapshot_and_ignores_foreign_history()
-> ConsoleLogResult<()> {
    let aggregator = MobKitConsoleAggregator::in_memory();
    let session = SessionId::new();
    let message = Message::SystemNotice(notice(&session, 10, 20));
    let mut history = frame(
        "history",
        &session,
        "system_notice",
        json!({ "message": message }),
    );
    history.source.kind = ConsoleFrameSourceKind::SessionHistory;
    let history = aggregator.store().append_if_absent(history).await?.frame;
    let first_observed = observe(&aggregator, &session).await?;
    let first = runtime_notice_snapshot_frame(
        RUNTIME,
        IDENTITY,
        &session.to_string(),
        &first_observed,
        &BTreeSet::new(),
        std::slice::from_ref(&message),
    )
    .ok_or("initial snapshot missing")?;
    let first = aggregator.store().append_if_absent(first).await?.frame;
    let observed = observe(&aggregator, &session).await?;
    let removed = runtime_notice_snapshot_frame(
        RUNTIME,
        IDENTITY,
        &session.to_string(),
        &observed,
        &BTreeSet::new(),
        &[],
    )
    .ok_or("empty replacement snapshot missing")?;
    let removed = aggregator.store().append_if_absent(removed).await?.frame;
    for index in 0..3 {
        let mut foreign = frame(
            &format!("foreign-snapshot-{index}"),
            &session,
            "runtime_notice_snapshot",
            first.payload.clone(),
        );
        foreign.source.kind = ConsoleFrameSourceKind::SessionHistory;
        match index {
            0 => foreign.runtime_key = "other-runtime".to_string(),
            1 => foreign.session_id = Some(SessionId::new().to_string()),
            _ => foreign.source.kind = ConsoleFrameSourceKind::ConsoleEvent,
        }
        aggregator.store().append_if_absent(foreign).await?;
    }
    let mut foreign_history = frame(
        "foreign-history",
        &session,
        "system_notice",
        json!({ "message": message }),
    );
    foreign_history.runtime_key = "other-runtime".to_string();
    foreign_history.source.kind = ConsoleFrameSourceKind::SessionHistory;
    aggregator.store().append_if_absent(foreign_history).await?;
    let observed = observe(&aggregator, &session).await?;
    assert!(observed.has_notice_history);
    assert!(observed.attempts.is_empty());
    assert_eq!(
        observed.relevant_frontier,
        history.cursor.seq().ok_or("missing assigned cursor")?
    );
    assert_eq!(
        observed.previous_snapshot.as_ref().map(|row| &row.id),
        Some(&removed.id)
    );
    Ok(())
}

// The rows still live in the real in-memory store. This wrapper injects only a
// read boundary race or a page failure, not a simulated scan result.
struct ObservedStore {
    inner: InMemoryConsoleLogStore,
    append_after_cursor: std::sync::Mutex<Vec<NewConsoleFrame>>,
    query_count: AtomicUsize,
    fail_query: Option<usize>,
}

impl ObservedStore {
    fn new(append_after_cursor: Vec<NewConsoleFrame>, fail_query: Option<usize>) -> Self {
        Self {
            inner: InMemoryConsoleLogStore::new(),
            append_after_cursor: std::sync::Mutex::new(append_after_cursor),
            query_count: AtomicUsize::new(0),
            fail_query,
        }
    }
}

#[async_trait::async_trait]
impl ConsoleLogStore for ObservedStore {
    async fn append_if_absent(&self, row: NewConsoleFrame) -> ConsoleLogResult<AppendOutcome> {
        self.inner.append_if_absent(row).await
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
        let call = self.query_count.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_query == Some(call) {
            return Err(std::io::Error::other("injected later scan page failure").into());
        }
        self.inner.query_frames(query).await
    }

    async fn frame_by_dedupe_key(&self, key: &str) -> ConsoleLogResult<Option<ConsoleFrame>> {
        self.inner.frame_by_dedupe_key(key).await
    }

    async fn latest_cursor(&self) -> ConsoleLogResult<Option<ConsoleCursor>> {
        let observed = self.inner.latest_cursor().await?;
        let rows = std::mem::take(
            &mut *self
                .append_after_cursor
                .lock()
                .map_err(|_| std::io::Error::other("test row lock poisoned"))?,
        );
        for row in rows {
            self.inner.append_if_absent(row).await?;
        }
        Ok(observed)
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
async fn notice_scan_observed_through_excludes_rows_appended_between_cursor_and_scan()
-> ConsoleLogResult<()> {
    let session = SessionId::new();
    let mut late_history = frame(
        "late-history",
        &session,
        "system_notice",
        json!({ "message": Message::SystemNotice(notice(&session, 11, 21)) }),
    );
    late_history.source.kind = ConsoleFrameSourceKind::SessionHistory;
    let mut late_snapshot = frame(
        "late-snapshot",
        &session,
        "runtime_notice_snapshot",
        json!({}),
    );
    late_snapshot.source.kind = ConsoleFrameSourceKind::SessionHistory;
    let store = Arc::new(ObservedStore::new(
        vec![
            applied("late-application", &session, 11, 21),
            late_history,
            late_snapshot,
        ],
        None,
    ));
    let aggregator = MobKitConsoleAggregator::new(store.clone());
    store
        .append_if_absent(applied("observed-application", &session, 10, 20))
        .await?;
    let observed = observe(&aggregator, &session).await?;
    assert_eq!(observed.observed_through, 1);
    assert_eq!(observed.relevant_frontier, 1);
    assert_eq!(observed.attempts, BTreeSet::from([attempt(10, 20)]));
    assert!(!observed.has_notice_history);
    assert!(observed.previous_snapshot.is_none());
    assert_eq!(
        store
            .inner
            .latest_cursor()
            .await?
            .and_then(|cursor| cursor.seq()),
        Some(4)
    );
    let snapshot = runtime_notice_snapshot_frame(
        RUNTIME,
        IDENTITY,
        &session.to_string(),
        &observed,
        &observed.attempts,
        &[],
    )
    .ok_or("settled snapshot missing")?;
    assert_eq!(snapshot.payload["observed_through"], "console:1");
    assert_eq!(
        snapshot.payload["settled_attempts"],
        json!([{
            "run_id": uuid::Uuid::from_u128(10), "input_id": uuid::Uuid::from_u128(20),
        }])
    );
    Ok(())
}

#[tokio::test]
async fn notice_scan_late_page_failure_returns_no_partial_observation() -> ConsoleLogResult<()> {
    let session = SessionId::new();
    let store = Arc::new(ObservedStore::new(Vec::new(), Some(2)));
    let aggregator = MobKitConsoleAggregator::new(store.clone());
    store
        .append_if_absent(applied("old-application", &session, 10, 20))
        .await?;
    for index in 0..1_005 {
        store
            .append_if_absent(frame(
                &format!("noise-{index}"),
                &session,
                "text_delta",
                json!({ "delta": "." }),
            ))
            .await?;
    }
    let result = observe(&aggregator, &session).await;
    assert!(
        result.is_err(),
        "a partial scan must not become a complete observation"
    );
    assert_eq!(store.query_count.load(Ordering::SeqCst), 2);
    let first_page = store
        .inner
        .query_frames(ConsoleTimelineQuery {
            limit: 1_000,
            ..Default::default()
        })
        .await?;
    let last_page = store
        .inner
        .query_frames(ConsoleTimelineQuery {
            after: first_page.frames.last().map(|row| row.cursor.clone()),
            limit: 1_000,
            ..Default::default()
        })
        .await?;
    let rows: Vec<_> = first_page
        .frames
        .into_iter()
        .chain(last_page.frames)
        .collect();
    assert_eq!(rows.len(), 1_006);
    assert!(rows.iter().all(|row| row.kind != "runtime_notice_snapshot"));
    assert!(rows.iter().any(|row| row.dedupe_key == "old-application"));
    Ok(())
}

#[tokio::test]
async fn notice_snapshot_rescan_publishes_changed_origin_offset_and_body_but_coalesces_identical_images()
-> ConsoleLogResult<()> {
    let aggregator = MobKitConsoleAggregator::in_memory();
    let session = SessionId::new();
    let original = notice(&session, 10, 20);
    let mut changed_origin = original.clone();
    changed_origin
        .runtime_origin
        .as_mut()
        .ok_or("test origin missing")?
        .input_id = InputId(uuid::Uuid::from_u128(21));
    let mut changed_body = changed_origin.clone();
    changed_body.body = Some("Canonical text changed without changing identity.".to_string());
    let legacy = Message::SystemNotice(SystemNoticeMessage::new(
        SystemNoticeKind::Generic,
        "Legacy context.",
    ));
    let images = [
        vec![Message::SystemNotice(original)],
        vec![Message::SystemNotice(changed_origin.clone())],
        vec![legacy.clone(), Message::SystemNotice(changed_origin)],
        vec![legacy, Message::SystemNotice(changed_body)],
    ];
    let mut previous_payload = None;
    for (index, messages) in images.iter().enumerate() {
        let observed = observe(&aggregator, &session).await?;
        let snapshot = runtime_notice_snapshot_frame(
            RUNTIME,
            IDENTITY,
            &session.to_string(),
            &observed,
            &BTreeSet::new(),
            messages,
        )
        .ok_or("changed image did not publish")?;
        assert_ne!(
            previous_payload.as_ref(),
            Some(&snapshot.payload["notices"])
        );
        assert_eq!(
            snapshot.payload["notices"][0]["offset"],
            if index < 2 { 0 } else { 1 }
        );
        previous_payload = Some(snapshot.payload["notices"].clone());
        let saved = aggregator.store().append_if_absent(snapshot).await?;
        assert_eq!(saved.disposition, AppendDisposition::Inserted);
        let rescanned = observe(&aggregator, &session).await?;
        assert_eq!(
            rescanned.previous_snapshot.as_ref().map(|row| &row.id),
            Some(&saved.frame.id)
        );
        assert!(
            runtime_notice_snapshot_frame(
                RUNTIME,
                IDENTITY,
                &session.to_string(),
                &rescanned,
                &BTreeSet::new(),
                messages
            )
            .is_none(),
            "unchanged image should coalesce"
        );
    }
    let rows = aggregator
        .store()
        .query_frames(ConsoleTimelineQuery {
            limit: 20,
            ..Default::default()
        })
        .await?;
    assert_eq!(rows.frames.len(), 4);
    Ok(())
}

async fn append_canonical_message(
    aggregator: &MobKitConsoleAggregator,
    session: &SessionId,
    offset: usize,
    message: &Message,
) -> ConsoleLogResult<Vec<ConsoleFrame>> {
    let mut saved = Vec::new();
    for row in frames_from_session_history_message_with_namespace(
        RUNTIME,
        IDENTITY,
        "",
        &session.to_string(),
        offset,
        serde_json::to_value(message)?,
    ) {
        saved.push(aggregator.store().append_if_absent(row).await?.frame);
    }
    Ok(saved)
}

fn assistant_message(text: &str) -> ConsoleLogResult<Message> {
    Ok(serde_json::from_value(json!({
        "role": "block_assistant",
        "blocks": [{ "block_type": "text", "data": { "text": text } }],
        "identity": {
            "interaction_id": uuid::Uuid::from_u128(50),
            "run_id": uuid::Uuid::from_u128(10),
        },
        "stop_reason": "end_turn",
        "created_at": "1970-01-01T00:00:00.100Z",
    }))?)
}

#[tokio::test]
async fn notice_snapshot_shrinking_history_moves_retained_tool_notice_and_answer_together()
-> ConsoleLogResult<()> {
    let aggregator = MobKitConsoleAggregator::in_memory();
    let session = SessionId::new();
    let call: Message = serde_json::from_value(json!({
        "role": "block_assistant",
        "blocks": [{ "block_type": "tool_use", "data": {
            "id": "retained-tool", "name": "lookup_delivery", "args": { "request": "plan" },
        }}],
        "identity": {
            "interaction_id": uuid::Uuid::from_u128(50),
            "run_id": uuid::Uuid::from_u128(10),
        },
        "stop_reason": "tool_use",
        "created_at": "1970-01-01T00:00:00.100Z",
    }))?;
    let result: Message = serde_json::from_value(json!({
        "role": "tool_results",
        "results": [{ "tool_use_id": "retained-tool", "content": "Delivery is ready.", "is_error": false }],
        "created_at": "1970-01-01T00:00:00.200Z",
    }))?;
    let notice = Message::SystemNotice(notice(&session, 10, 20));
    let answer = assistant_message("The delivery plan is ready.")?;
    let tail = [call, result, notice, answer];
    let mut retained = Vec::new();
    for (index, message) in tail.iter().enumerate() {
        retained
            .extend(append_canonical_message(&aggregator, &session, index + 10, message).await?);
    }
    assert_eq!(
        retained.len(),
        4,
        "fixture must exercise actual call/result/notice/answer projections"
    );
    let original_frames = retained.clone();
    let mut current = vec![Message::SystemNotice(SystemNoticeMessage::new(
        SystemNoticeKind::Generic,
        "Compacted context.",
    ))];
    current.extend(tail);
    let observed = observe(&aggregator, &session).await?;
    let snapshot = runtime_notice_snapshot_frame(
        RUNTIME,
        IDENTITY,
        &session.to_string(),
        &observed,
        &BTreeSet::new(),
        &current,
    )
    .ok_or("shrinking image snapshot missing")?;
    let positions = snapshot.payload["history_positions"]
        .as_array()
        .ok_or("positions missing")?;
    assert_eq!(positions.len(), 3);
    let mut current_order = Vec::new();
    for (row, old_offset, new_offset) in [
        (&retained[0], 10, 1),
        (&retained[1], 11, 2),
        (&retained[3], 13, 4),
    ] {
        assert!(
            row.source
                .source_cursor
                .as_deref()
                .is_some_and(|cursor| cursor.starts_with(&format!("{session}:{old_offset}")))
        );
        let position = positions
            .iter()
            .find(|position| position["frame_id"] == row.id)
            .ok_or("retained row lost its exact position mapping")?;
        let cursor = position["source_cursor"]
            .as_str()
            .ok_or("mapped cursor missing")?;
        let expected_suffix = if row.kind == "tool_call_requested" {
            ":tool-call:0"
        } else if row.kind == "tool_execution_completed" {
            ":0"
        } else {
            ""
        };
        assert_eq!(cursor, format!("{session}:{new_offset}{expected_suffix}"));
        current_order.push((new_offset, row.kind.as_str()));
    }
    assert_eq!(snapshot.payload["notices"][0]["offset"], 3);
    assert_eq!(
        snapshot.payload["notices"][0]["message"],
        retained[2].payload["message"]
    );
    current_order.push((3, "system_notice"));
    current_order.sort_unstable();
    assert_eq!(
        current_order,
        vec![
            (1, "tool_call_requested"),
            (2, "tool_execution_completed"),
            (3, "system_notice"),
            (4, "text_complete")
        ]
    );
    let saved_snapshot = aggregator.store().append_if_absent(snapshot).await?.frame;
    let rows = aggregator
        .store()
        .query_frames(ConsoleTimelineQuery {
            limit: 20,
            ..Default::default()
        })
        .await?;
    assert_eq!(
        &rows.frames[..4],
        original_frames.as_slice(),
        "position observations do not rewrite canonical rows"
    );
    let rescanned = observe(&aggregator, &session).await?;
    assert_eq!(
        rescanned.previous_snapshot.as_ref().map(|row| &row.id),
        Some(&saved_snapshot.id)
    );
    assert!(
        runtime_notice_snapshot_frame(
            RUNTIME,
            IDENTITY,
            &session.to_string(),
            &rescanned,
            &BTreeSet::new(),
            &current,
        )
        .is_none(),
        "an unchanged compacted image should coalesce after reconnect rescan"
    );
    Ok(())
}

#[tokio::test]
async fn notice_history_positions_pair_repeated_exact_occurrences_without_borrowing_lineage()
-> ConsoleLogResult<()> {
    let aggregator = MobKitConsoleAggregator::in_memory();
    let session = SessionId::new();
    let message = assistant_message("Repeated authored answer.")?;
    let mut originals = Vec::new();
    // Arrival order deliberately disagrees with canonical order.
    for offset in [30, 10, 20] {
        originals.extend(append_canonical_message(&aggregator, &session, offset, &message).await?);
    }
    assert_eq!(originals.len(), 3);
    for index in 0..4 {
        let mut conflicting = frames_from_session_history_message_with_namespace(
            RUNTIME,
            IDENTITY,
            "",
            &session.to_string(),
            index,
            serde_json::to_value(&message)?,
        )
        .into_iter()
        .next()
        .ok_or("assistant fixture did not project")?;
        conflicting.dedupe_key = format!("conflict-{index}");
        match index {
            0 => conflicting.run_id = Some(uuid::Uuid::from_u128(11).to_string()),
            1 => conflicting.interaction_id = Some(uuid::Uuid::from_u128(51).to_string()),
            2 => conflicting.kind = "user_input".to_string(),
            _ => conflicting.payload["text"] = json!("Different canonical text."),
        }
        aggregator.store().append_if_absent(conflicting).await?;
    }
    let filler = Message::SystemNotice(SystemNoticeMessage::new(
        SystemNoticeKind::Generic,
        "Context.",
    ));
    let current = [filler.clone(), message.clone(), filler, message];
    let observed = observe(&aggregator, &session).await?;
    let positions =
        current_history_positions(RUNTIME, IDENTITY, &session.to_string(), &observed, &current);
    assert_eq!(
        positions,
        vec![
            json!({ "frame_id": originals[1].id, "source_cursor": format!("{session}:1") }),
            json!({ "frame_id": originals[2].id, "source_cursor": format!("{session}:3") }),
        ],
        "only the two exact occurrences can claim current coordinates"
    );
    Ok(())
}

#[test]
fn notice_history_refresh_avoids_forcing_reads_for_stream_bursts_but_keeps_owner_boundaries() {
    let session = SessionId::new();
    let mut forced = 0;
    for index in 0..2_000 {
        let kind = match index % 4 {
            0 => "text_delta",
            1 => "reasoning_delta",
            2 => "tool_input_delta",
            _ => "tool_execution_started",
        };
        let mut streamed = frame(
            &format!("stream-{index}"),
            &session,
            kind,
            json!({ "delta": "streamed content", "source_sequence": index }),
        );
        streamed.source.source_cursor = Some(format!("runtime:{index}"));
        streamed.run_id = Some(uuid::Uuid::from_u128(10).to_string());
        forced += usize::from(console_event_should_refresh_session_history(&streamed));
    }
    assert_eq!(
        forced, 0,
        "the real source session must not force a full history read per delta"
    );
    for kind in [
        "interaction_complete",
        "interaction_failed",
        "message_delivery_failed",
        "run_completed",
        "run_failed",
        "run_cancelled",
        "boundary_append_applied",
        "boundary_appends_discarded",
        "transcript_rewrite_committed",
        "transcript_rewrite_audit_receipt_committed",
    ] {
        assert!(
            console_event_should_refresh_session_history(&frame(kind, &session, kind, json!({}))),
            "{kind} must retain its authoritative refresh trigger"
        );
    }
}

#[tokio::test]
async fn ordinary_notice_reanchors_before_durable_notice_and_answer_after_shrink()
-> ConsoleLogResult<()> {
    let aggregator = MobKitConsoleAggregator::in_memory();
    let session = SessionId::new();
    let ordinary = Message::SystemNotice(SystemNoticeMessage::new(
        SystemNoticeKind::Generic,
        "Keep the operator's constraints in view.",
    ));
    let durable = Message::SystemNotice(notice(&session, 10, 20));
    let answer = assistant_message("Answer following both notices.")?;
    let mut retained = Vec::new();
    for (index, message) in [&ordinary, &durable, &answer].into_iter().enumerate() {
        retained
            .extend(append_canonical_message(&aggregator, &session, index + 10, message).await?);
    }
    assert_eq!(retained.len(), 3);
    let current = [
        Message::SystemNotice(SystemNoticeMessage::new(
            SystemNoticeKind::Generic,
            "Compacted context.",
        )),
        ordinary,
        durable,
        answer,
    ];
    let observed = observe(&aggregator, &session).await?;
    let snapshot = runtime_notice_snapshot_frame(
        RUNTIME,
        IDENTITY,
        &session.to_string(),
        &observed,
        &BTreeSet::new(),
        &current,
    )
    .ok_or("current notice image missing")?;
    assert_eq!(
        snapshot.payload["history_positions"],
        json!([
            { "frame_id": retained[0].id, "source_cursor": format!("{session}:1") },
            { "frame_id": retained[2].id, "source_cursor": format!("{session}:3") },
        ])
    );
    assert_eq!(
        snapshot.payload["notices"]
            .as_array()
            .ok_or("notice rows missing")?
            .len(),
        1
    );
    assert_eq!(snapshot.payload["notices"][0]["offset"], 2);
    assert_eq!(
        snapshot.payload["notices"][0]["message"],
        retained[1].payload["message"]
    );
    assert!(
        retained[0].payload["message"]
            .get("runtime_origin")
            .is_none(),
        "the legacy notice is reanchored without manufacturing a runtime origin"
    );
    Ok(())
}

#[tokio::test]
async fn history_position_occurrences_order_tool_result_subindices_numerically()
-> ConsoleLogResult<()> {
    let aggregator = MobKitConsoleAggregator::in_memory();
    let session = SessionId::new();
    let repeated = json!({ "tool_use_id": "reused-result", "content": "Same exact result.", "is_error": false });
    let old_results: Vec<_> = (0..11).map(|index| {
        if [2, 10].contains(&index) { repeated.clone() }
        else { json!({ "tool_use_id": format!("other-{index}"), "content": "Other result.", "is_error": false }) }
    }).collect();
    let old: Message = serde_json::from_value(json!({
        "role": "tool_results", "results": old_results, "created_at": "1970-01-01T00:00:00.100Z",
    }))?;
    let projected = frames_from_session_history_message_with_namespace(
        RUNTIME,
        IDENTITY,
        "",
        &session.to_string(),
        10,
        serde_json::to_value(old)?,
    );
    assert_eq!(projected.len(), 11);
    let mut selected = Vec::new();
    // A string sort and arrival-order sort would both choose index 10 first.
    for index in [10, 2] {
        selected.push(
            aggregator
                .store()
                .append_if_absent(projected[index].clone())
                .await?
                .frame,
        );
    }
    let current_results: Message = serde_json::from_value(json!({
        "role": "tool_results", "results": [repeated.clone(), repeated],
        "created_at": "1970-01-01T00:00:00.100Z",
    }))?;
    let current = [
        Message::SystemNotice(SystemNoticeMessage::new(
            SystemNoticeKind::Generic,
            "Context.",
        )),
        current_results,
    ];
    let observed = observe(&aggregator, &session).await?;
    let positions =
        current_history_positions(RUNTIME, IDENTITY, &session.to_string(), &observed, &current);
    assert_eq!(
        positions,
        vec![
            json!({ "frame_id": selected[1].id, "source_cursor": format!("{session}:1:0") }),
            json!({ "frame_id": selected[0].id, "source_cursor": format!("{session}:1:1") }),
        ]
    );
    Ok(())
}
