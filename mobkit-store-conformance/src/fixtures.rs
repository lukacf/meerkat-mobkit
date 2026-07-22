//! Deterministic fixtures shared by the chapters.
//!
//! Session fabrication is delegated to the upstream harness's fixtures
//! (`meerkat_store_conformance::fixtures`) so legacy session documents here
//! are byte-compatible with the shapes Meerkat's own conformance suite pins:
//! current-envelope documents whose metadata lacks the typed checkpoint
//! stamp — exactly what 0.7.x fleets persisted.

use meerkat_core::Session;
use meerkat_mobkit::console_aggregator::{
    ConsoleFrameSource, ConsoleFrameSourceKind, ConsoleFrameStatus, NewConsoleFrame,
};
use meerkat_mobkit::identity_first::SessionSnapshot;
use meerkat_mobkit::memory::{MemoryKind, NewMemoryRecord};
use meerkat_mobkit::types::UnifiedEvent;
use meerkat_mobkit::unified_runtime::PersistedEvent;
use meerkat_store_conformance::ConformanceFailure;
pub use meerkat_store_conformance::fixtures::{legacy_session_blob, push_text, session_with_texts};

/// Serialize a session into the canonical continuity-snapshot payload: the
/// Meerkat `Session` JSON document (what `ContinuitySessionStoreAdapter` and
/// `MobSessionBridge::checkpoint_session` both write).
pub fn session_snapshot(session: &Session) -> Result<SessionSnapshot, ConformanceFailure> {
    let data = serde_json::to_vec(session).map_err(|error| {
        ConformanceFailure::new("fixtures", "session_snapshot", error.to_string())
    })?;
    Ok(SessionSnapshot { data })
}

/// A deterministic console frame keyed by `dedupe_key`.
pub fn console_frame(dedupe_key: &str, identity: &str, timestamp_ms: u64) -> NewConsoleFrame {
    NewConsoleFrame {
        id: None,
        dedupe_key: dedupe_key.to_string(),
        timestamp_ms,
        runtime_key: "conformance-runtime".to_string(),
        identity: identity.to_string(),
        conversation_id: None,
        session_id: None,
        kind: "conformance_probe".to_string(),
        status: ConsoleFrameStatus::Completed,
        payload: serde_json::json!({ "marker": dedupe_key }),
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
    }
}

/// A deterministic persisted operational event with a caller-chosen id and
/// ingestion sequence number.
pub fn persisted_event(id: &str, seq: u64) -> PersistedEvent {
    PersistedEvent {
        id: id.to_string(),
        seq,
        timestamp_ms: 1_000 + seq,
        member_id: None,
        event: UnifiedEvent::Agent {
            agent_id: "conformance-agent".to_string(),
            event_type: "conformance_probe".to_string(),
            payload: Some(serde_json::json!({ "id": id })),
        },
    }
}

/// A deterministic new memory record for staged/authored write chapters.
pub fn new_memory_record(title: &str, body: &str) -> NewMemoryRecord {
    NewMemoryRecord {
        kind: MemoryKind::Fact,
        title: title.to_string(),
        description: String::new(),
        body: body.to_string(),
        tags: vec!["conformance".to_string()],
        evidence: Vec::new(),
        verification: None,
    }
}
