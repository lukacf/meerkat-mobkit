use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConsoleCursor(String);

impl ConsoleCursor {
    pub(crate) fn from_seq(seq: u64) -> Self {
        Self(format!("console:{seq}"))
    }

    pub(crate) fn seq(&self) -> Option<u64> {
        self.0.strip_prefix("console:")?.parse::<u64>().ok()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ConsoleCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ConsoleCursor {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ConsoleCursor {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleFrameStatus {
    Accepted,
    Dispatching,
    Delivered,
    DeliveryFailed,
    Completed,
    Redacted,
}

impl ConsoleFrameStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Dispatching => "dispatching",
            Self::Delivered => "delivered",
            Self::DeliveryFailed => "delivery_failed",
            Self::Completed => "completed",
            Self::Redacted => "redacted",
        }
    }

    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "accepted" => Self::Accepted,
            "dispatching" => Self::Dispatching,
            "delivered" => Self::Delivered,
            "delivery_failed" => Self::DeliveryFailed,
            "redacted" => Self::Redacted,
            _ => Self::Completed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleFrameSourceKind {
    ConsoleEvent,
    SessionHistory,
    Send,
    Synthetic,
}

impl ConsoleFrameSourceKind {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::ConsoleEvent => "console_event",
            Self::SessionHistory => "session_history",
            Self::Send => "send",
            Self::Synthetic => "synthetic",
        }
    }

    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "console_event" => Self::ConsoleEvent,
            "session_history" => Self::SessionHistory,
            "send" => Self::Send,
            _ => Self::Synthetic,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleFrameSource {
    pub kind: ConsoleFrameSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleFrame {
    pub id: String,
    pub cursor: ConsoleCursor,
    pub dedupe_key: String,
    pub timestamp_ms: u64,
    pub runtime_key: String,
    pub identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub kind: String,
    pub status: ConsoleFrameStatus,
    pub frame_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<u64>,
    pub payload: Value,
    pub source: ConsoleFrameSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_frame_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caused_by_frame_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewConsoleFrame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub dedupe_key: String,
    pub timestamp_ms: u64,
    pub runtime_key: String,
    pub identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub kind: String,
    pub status: ConsoleFrameStatus,
    pub payload: Value,
    pub source: ConsoleFrameSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_frame_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caused_by_frame_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppendDisposition {
    Inserted,
    Existing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppendOutcome {
    pub disposition: AppendDisposition,
    pub frame: ConsoleFrame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleTimelineQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<ConsoleCursor>,
    /// Same wire/programmatic default alignment as
    /// [`ConsoleTimelineWindowQuery::limit`]: a bare `#[serde(default)]`
    /// deserialized 0 for omitted limits while [`Default`] said 200.
    #[serde(default = "default_timeline_window_limit")]
    pub limit: usize,
}

impl Default for ConsoleTimelineQuery {
    fn default() -> Self {
        Self {
            identity: None,
            conversation_id: None,
            after: None,
            limit: 200,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleTimelineWindowQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<ConsoleCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<ConsoleCursor>,
    #[serde(default)]
    pub mode: ConsoleTimelineMode,
    /// Maximum frames returned. Serde default matches [`Default`] (200):
    /// `#[serde(default)]` alone yielded 0, which the visibility scan clamps
    /// to 1 — so every wire caller that omitted `limit` got exactly ONE frame
    /// while programmatic construction got 200. Only the reference front-end
    /// (which always sends limit=400) never noticed.
    #[serde(default = "default_timeline_window_limit")]
    pub limit: usize,
}

fn default_timeline_window_limit() -> usize {
    200
}

impl Default for ConsoleTimelineWindowQuery {
    fn default() -> Self {
        Self {
            identity: None,
            conversation_id: None,
            after: None,
            before: None,
            mode: ConsoleTimelineMode::Since,
            limit: 200,
        }
    }
}

impl From<ConsoleTimelineQuery> for ConsoleTimelineWindowQuery {
    fn from(query: ConsoleTimelineQuery) -> Self {
        Self {
            identity: query.identity,
            conversation_id: query.conversation_id,
            after: query.after,
            before: None,
            mode: ConsoleTimelineMode::Since,
            limit: query.limit,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleTimelineMode {
    #[default]
    Since,
    Recent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleTimelinePage {
    pub frames: Vec<ConsoleFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<ConsoleCursor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleTimelineWindowPage {
    pub frames: Vec<ConsoleFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<ConsoleCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_cursor: Option<ConsoleCursor>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub exhausted: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleVisibility {
    Addressable,
    Hidden,
    RetiredReadable,
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleIdentityRecord {
    pub identity: String,
    pub display_name: String,
    pub runtime_key: String,
    pub runtime_member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub visibility: ConsoleVisibility,
    pub addressable: bool,
    pub health: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topology_peers: Vec<String>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleIdentityInspection {
    pub identity: ConsoleIdentityRecord,
    #[serde(default)]
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleSendRequest {
    pub identity: String,
    pub content: Value,
    pub origin: String,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handling_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleInteractionAccepted {
    pub interaction_id: String,
    pub identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub input_frame_id: String,
    pub cursor: ConsoleCursor,
    pub status: ConsoleFrameStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConsoleTimelineEvent {
    SnapshotStarted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<ConsoleCursor>,
    },
    ConsoleFrame {
        frame: ConsoleFrame,
    },
    FrameUpdated {
        frame: ConsoleFrame,
    },
    SnapshotComplete {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<ConsoleCursor>,
    },
    ReplayUnavailable {
        requested_cursor: String,
        latest_cursor: Option<ConsoleCursor>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleReplayUnavailable {
    pub error: String,
    pub requested_cursor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_cursor: Option<ConsoleCursor>,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod timeline_window_query_defaults {
    use super::*;

    /// An omitted `limit` on the wire must match the programmatic default.
    /// Regression: `#[serde(default)]` alone deserialized 0, the scan clamped
    /// it to 1, and every SDK/test caller that omitted `limit` silently got a
    /// single frame (the reference front-end always sends 400, hiding it).
    #[test]
    fn omitted_limit_deserializes_to_the_struct_default() {
        let q: ConsoleTimelineWindowQuery = serde_json::from_str("{}").expect("empty query");
        assert_eq!(q.limit, ConsoleTimelineWindowQuery::default().limit);
        assert_eq!(q.limit, 200);
    }

    #[test]
    fn omitted_limit_on_the_plain_query_matches_its_default_too() {
        let q: ConsoleTimelineQuery = serde_json::from_str("{}").expect("empty query");
        assert_eq!(q.limit, ConsoleTimelineQuery::default().limit);
    }

    #[test]
    fn explicit_limit_is_preserved() {
        let q: ConsoleTimelineWindowQuery =
            serde_json::from_str(r#"{"limit": 7}"#).expect("explicit limit");
        assert_eq!(q.limit, 7);
    }
}
