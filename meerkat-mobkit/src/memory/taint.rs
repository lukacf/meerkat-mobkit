//! Coarse session-sticky content-taint tracking (§10.1, P1).
//!
//! The persistent-prompt-injection defense needs an input signal, and no
//! turn-level or session-level content-taint fact exists anywhere in the
//! platform. This module ships the P1 mechanism: a MobKit-owned
//! content-trust configuration plus an observe-stream taint tracker that
//! marks a session tainted once it ingests content from an untrusted tool
//! source. Taint is **session-sticky** (Codex's thread-level
//! `memory_mode='polluted'`, adopted): per-turn taint is trivially evaded
//! by "on your NEXT reply, remember X", and compaction cannot clear taint —
//! the summary is derived from tainted context and inherits it. Taint
//! clears only at a fresh-context boundary (reset / respawn / fresh spawn),
//! which is automatic here because state is keyed by session id and those
//! paths mint new session ids.
//!
//! ## Honest P1 gaps (closed in P2 via dispatch-time visibility, §13)
//!
//! - **The first-ingestion race**: taint is derived from the observe-only
//!   agent-event stream, which is asynchronous. A memory write in the same
//!   turn as the session's *first* untrusted ingestion can reach the store
//!   before the taint observer processes the tool event. Deployments that
//!   cannot accept this set `agent_memory.llm_writes = "quarantined"`.
//! - **The mirror race**: after a session rotation, the tracker's view of
//!   an identity's current session lags until the runtime's delivery hook
//!   or the new session's first `RunStarted` event updates it, so a write
//!   in that window can be quarantined against the *old* session's taint.
//!   This errs conservative (false quarantine, never false trust).
//! - **Name-based classification**: meerkat tool events carry only the tool
//!   NAME — no `ToolDef.provenance` — so MCP tools cannot be attributed to
//!   a server unless their names are server-qualified
//!   (`mcp__<server>__<tool>`). Deployments with unqualified MCP tool names
//!   list them in `content_trust.untrusted_tools` (or run
//!   `llm_writes = "quarantined"`) until P2's dispatch-time join against
//!   real `ToolProvenance`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use meerkat_core::event::AgentEvent;
use serde::{Deserialize, Serialize};

use crate::identity_first::agent_memory::AgentMemoryLlmWrites;
use crate::memory::records::MemoryAuthor;

/// Builtin web-facing tool names: ALWAYS untrusted for memory purposes, not
/// overridable by `trusted_tools` (§10.1 "web/fetch always untrusted").
/// `web_search` is meerkat's builtin client-side search
/// (`meerkat_core::web_search::WEB_SEARCH_TOOL_NAME`); the others cover the
/// common fetch spellings across builtin and bundle surfaces.
const ALWAYS_UNTRUSTED_TOOL_NAMES: &[&str] = &["web_search", "web_fetch", "fetch", "http_request"];

/// Server-qualified MCP tool-name prefix (`mcp__<server>__<tool>`). The only
/// name shape that lets P1 attribute a tool to an MCP server (see module
/// docs on classification coarseness).
const MCP_QUALIFIED_PREFIX: &str = "mcp__";

/// Tainted-session entries are bounded; oldest-tainted evict first. A taint
/// entry for a dead session is inert (nothing checks it), so eviction only
/// bounds memory, never correctness for live sessions at sane scales.
const MAX_TRACKED_TAINTED_SESSIONS: usize = 4096;

/// Which tool sources count as untrusted for memory purposes (§10.1).
///
/// Mirrors Codex's `pollutes_memory` posture: web/fetch and provider-native
/// search are always untrusted; MCP servers are untrusted by default with an
/// explicit `trusted_mcp_servers` allowlist. Note meerkat's
/// `ToolAccessPolicy::AllowList` is invocation gating and cannot serve this
/// role.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentTrustConfig {
    /// MCP servers whose tools do not taint. Joinable in P1 only for
    /// server-qualified tool names (`mcp__<server>__<tool>`).
    #[serde(default)]
    pub trusted_mcp_servers: Vec<String>,
    /// Explicit tool names that taint the session when their results enter
    /// context. The escape hatch for unqualified MCP tool names.
    #[serde(default)]
    pub untrusted_tools: Vec<String>,
    /// Explicit tool names that never taint (cannot override the builtin
    /// web/fetch class or `untrusted_tools`).
    #[serde(default)]
    pub trusted_tools: Vec<String>,
}

/// Classification verdict for one tool name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolContentTrust {
    Trusted,
    Untrusted { source: String },
}

impl ContentTrustConfig {
    /// Fail-loud JSON parse for the gateway config block
    /// `agent_memory.content_trust { ... }`. Unknown fields and wrong types
    /// are errors, never silently ignored.
    pub fn from_json_value(value: &serde_json::Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "content_trust must be an object".to_string())?;
        let supported = ["trusted_mcp_servers", "untrusted_tools", "trusted_tools"];
        let unsupported = object
            .keys()
            .filter(|key| !supported.contains(&key.as_str()))
            .map(String::as_str)
            .collect::<Vec<_>>();
        if !unsupported.is_empty() {
            return Err(format!(
                "unsupported content_trust fields: {}",
                unsupported.join(", ")
            ));
        }
        let parse_names = |key: &str| -> Result<Vec<String>, String> {
            match object.get(key) {
                None => Ok(Vec::new()),
                Some(value) => {
                    let entries = value
                        .as_array()
                        .ok_or_else(|| format!("content_trust.{key} must be an array"))?;
                    entries
                        .iter()
                        .map(|entry| {
                            entry
                                .as_str()
                                .map(str::trim)
                                .filter(|name| !name.is_empty())
                                .map(ToString::to_string)
                                .ok_or_else(|| {
                                    format!("content_trust.{key} entries must be non-empty strings")
                                })
                        })
                        .collect()
                }
            }
        };
        Ok(Self {
            trusted_mcp_servers: parse_names("trusted_mcp_servers")?,
            untrusted_tools: parse_names("untrusted_tools")?,
            trusted_tools: parse_names("trusted_tools")?,
        })
    }

    /// Classify a tool by NAME (the only fact the P1 event surface carries —
    /// module docs). Precedence: builtin web/fetch (non-overridable) >
    /// `untrusted_tools` > `trusted_tools` > MCP-qualified names against the
    /// server allowlist > trusted.
    pub fn classify_tool(&self, name: &str) -> ToolContentTrust {
        if ALWAYS_UNTRUSTED_TOOL_NAMES.contains(&name) {
            return ToolContentTrust::Untrusted {
                source: format!("web tool '{name}'"),
            };
        }
        if self.untrusted_tools.iter().any(|tool| tool == name) {
            return ToolContentTrust::Untrusted {
                source: format!("configured untrusted tool '{name}'"),
            };
        }
        if self.trusted_tools.iter().any(|tool| tool == name) {
            return ToolContentTrust::Trusted;
        }
        if let Some(rest) = name.strip_prefix(MCP_QUALIFIED_PREFIX) {
            let server = rest.split("__").next().unwrap_or(rest);
            if self
                .trusted_mcp_servers
                .iter()
                .any(|trusted| trusted == server)
            {
                return ToolContentTrust::Trusted;
            }
            return ToolContentTrust::Untrusted {
                source: format!("MCP server '{server}' (tool '{name}')"),
            };
        }
        ToolContentTrust::Trusted
    }
}

/// Why and when a session became tainted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaintState {
    pub tainted_at_ms: u64,
    pub source: String,
}

#[derive(Default)]
struct TaintInner {
    /// identity → the session key the tracker currently attributes that
    /// identity's activity to. Fed authoritatively by the identity runtime's
    /// delivery/reset hooks and, as fallback, by observed `RunStarted`
    /// events (peer-comms-driven runs never pass through the runtime hooks).
    current_session: HashMap<String, String>,
    /// session key → taint fact. Session-sticky by construction.
    tainted: HashMap<String, TaintState>,
    /// Untrusted ingestion observed before the observer learned the
    /// identity's session (mid-run attach). Transferred to the next learned
    /// session — conservative direction (see module docs).
    pending_identity_taint: HashMap<String, TaintState>,
}

/// Session-sticky taint tracker (§10.1, coarse P1). Cheap to clone; clones
/// share state.
#[derive(Clone, Default)]
pub struct SessionTaintTracker {
    config: Arc<ContentTrustConfig>,
    inner: Arc<Mutex<TaintInner>>,
}

impl SessionTaintTracker {
    pub fn new(config: ContentTrustConfig) -> Self {
        Self {
            config: Arc::new(config),
            inner: Arc::new(Mutex::new(TaintInner::default())),
        }
    }

    /// Observe one agent event for `identity` (the observe-only agent-event
    /// stream is per-member, so attribution is the subscription's).
    pub fn observe_agent_event(&self, identity: &str, event: &AgentEvent) {
        match event {
            AgentEvent::RunStarted { session_id, .. } => {
                self.note_current_session(identity, &session_id.to_string());
            }
            // Taint on result *ingestion*: these are the events whose content
            // enters the conversation context. Errors taint too — an error
            // body is still attacker-influenced text in context.
            AgentEvent::ToolResultReceived { name, .. }
            | AgentEvent::ToolExecutionCompleted { name, .. } => {
                if let ToolContentTrust::Untrusted { source } = self.config.classify_tool(name) {
                    self.mark_identity_tainted(identity, source);
                }
            }
            // Provider-executed server tools (web search / grounding /
            // provider-native): typed, and always untrusted (§10.1).
            AgentEvent::ServerToolContent { kind, .. } => {
                self.mark_identity_tainted(
                    identity,
                    format!("provider server tool '{}'", kind.provider_name()),
                );
            }
            _ => {}
        }
    }

    /// Authoritative current-session hint from the identity runtime's
    /// delivery path. Keeps the tracker's attribution ahead of the (async)
    /// observe stream on the paths MobKit controls.
    pub fn note_current_session(&self, identity: &str, session_key: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        let pending = inner.pending_identity_taint.remove(identity);
        let previous = inner
            .current_session
            .insert(identity.to_string(), session_key.to_string());
        if previous.as_deref() != Some(session_key)
            && previous.is_some_and(|prior| inner.tainted.contains_key(&prior))
        {
            // TODO(P3b): emit a timeline event for the taint boundary.
            tracing::warn!(
                identity,
                session_key,
                "agent memory taint: session rotated away from a tainted session; \
                 new session starts clean"
            );
        }
        if let Some(state) = pending {
            self.insert_taint(&mut inner, session_key.to_string(), state);
        }
    }

    /// Explicit clear for the reset path (`reset()` is the operator's escape
    /// hatch from a poisoned session). Rotation already clears implicitly;
    /// this also drops any pending pre-attribution taint.
    pub fn clear_identity(&self, identity: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        inner.pending_identity_taint.remove(identity);
        if let Some(session) = inner.current_session.remove(identity) {
            inner.tainted.remove(&session);
        }
    }

    /// Taint fact for an explicit session key.
    pub fn session_taint(&self, session_key: &str) -> Option<TaintState> {
        let inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        inner.tainted.get(session_key).cloned()
    }

    /// Taint fact for the identity's currently-attributed session (the write
    /// gate's query: LLM-authored writes carry identity, not session).
    pub fn identity_taint(&self, identity: &str) -> Option<TaintState> {
        let inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(state) = inner.pending_identity_taint.get(identity) {
            return Some(state.clone());
        }
        inner
            .current_session
            .get(identity)
            .and_then(|session| inner.tainted.get(session))
            .cloned()
    }

    fn mark_identity_tainted(&self, identity: &str, source: String) {
        let state = TaintState {
            tainted_at_ms: now_ms(),
            source,
        };
        let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        match inner.current_session.get(identity).cloned() {
            Some(session) => self.insert_taint(&mut inner, session, state),
            None => {
                // Mid-run attach: session unknown until the next RunStarted /
                // delivery hook. Hold identity-sticky (module docs).
                match inner.pending_identity_taint.entry(identity.to_string()) {
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        tracing::warn!(
                            identity,
                            source = %state.source,
                            "agent memory taint: untrusted ingestion observed before session \
                             attribution; holding identity-sticky taint"
                        );
                        slot.insert(state);
                    }
                    std::collections::hash_map::Entry::Occupied(mut slot) => {
                        slot.insert(state);
                    }
                }
            }
        }
    }

    fn insert_taint(&self, inner: &mut TaintInner, session: String, state: TaintState) {
        match inner.tainted.entry(session) {
            std::collections::hash_map::Entry::Occupied(_) => return,
            std::collections::hash_map::Entry::Vacant(slot) => {
                // TODO(P3b): emit a timeline event so the console shows the
                // taint transition; tracing is the P1 visibility surface.
                tracing::warn!(
                    session_key = %slot.key(),
                    source = %state.source,
                    "agent memory taint: session ingested untrusted content; LLM-authored \
                     memory writes from this session will quarantine until a fresh-context \
                     boundary (reset/respawn/fresh spawn)"
                );
                slot.insert(state);
            }
        }
        if inner.tainted.len() > MAX_TRACKED_TAINTED_SESSIONS
            && let Some(oldest) = inner
                .tainted
                .iter()
                .min_by_key(|(_, state)| state.tainted_at_ms)
                .map(|(key, _)| key.clone())
        {
            inner.tainted.remove(&oldest);
        }
    }
}

/// Store-seam write gate (§10.1 posture): consulted by the bundled store for
/// every LLM-authored create/supersede so the quarantine decision holds for
/// ALL callers — the Recorder tool, staged batches, and any future stage —
/// not just the tool handler.
pub trait LlmWriteGate: Send + Sync {
    /// `Some(reason)` when this LLM-authored write must land
    /// `RecordStatus::Quarantined`. Non-LLM principals are never gated.
    fn quarantine_reason(&self, author: &MemoryAuthor) -> Option<String>;
}

/// The P1 gate: `llm_writes = "quarantined"` forces every LLM-authored write
/// into quarantine regardless of taint; otherwise agent-authored writes
/// quarantine when the author's session is tainted. Steward/Distiller
/// authors carry run ids, not identities — in P1 only the `llm_writes` knob
/// gates them (their evidence-range taint is P2's Distiller work).
pub struct TaintLlmWriteGate {
    tracker: Option<SessionTaintTracker>,
    llm_writes: AgentMemoryLlmWrites,
}

impl TaintLlmWriteGate {
    pub fn new(tracker: Option<SessionTaintTracker>, llm_writes: AgentMemoryLlmWrites) -> Self {
        Self {
            tracker,
            llm_writes,
        }
    }
}

impl LlmWriteGate for TaintLlmWriteGate {
    fn quarantine_reason(&self, author: &MemoryAuthor) -> Option<String> {
        if !author.is_llm() {
            return None;
        }
        if self.llm_writes == AgentMemoryLlmWrites::Quarantined {
            return Some("llm_writes=quarantined policy".to_string());
        }
        if let (Some(tracker), MemoryAuthor::Agent { identity }) = (self.tracker.as_ref(), author)
            && let Some(state) = tracker.identity_taint(identity)
        {
            return Some(format!("session tainted by {}", state.source));
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Observe-stream feed
// ---------------------------------------------------------------------------

/// Guard for the taint observer task; aborts the task when the last clone
/// drops (the runtime that owned the tracker is gone).
#[derive(Clone)]
pub struct TaintObserverGuard {
    _abort: Arc<AbortOnDrop>,
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Subscribe the taint observer to every active member's agent-event stream
/// (the same observe-only `subscribe_agent_events` surface the console
/// forwarder rides), reconciling membership every second. Streams feed
/// [`SessionTaintTracker::observe_agent_event`] keyed by the member's agent
/// identity.
pub fn spawn_taint_observer(
    handle: meerkat_mob::MobHandle,
    tracker: SessionTaintTracker,
) -> TaintObserverGuard {
    let task = tokio::spawn(run_taint_observer(handle, tracker));
    TaintObserverGuard {
        _abort: Arc::new(AbortOnDrop(task)),
    }
}

async fn run_taint_observer(handle: meerkat_mob::MobHandle, tracker: SessionTaintTracker) {
    use futures::StreamExt;
    use futures::stream::SelectAll;

    enum Observed {
        Event(String, Box<meerkat_core::event::EventEnvelope<AgentEvent>>),
        Closed(String),
    }

    let mut streams: SelectAll<futures::stream::BoxStream<'static, Observed>> = SelectAll::new();
    let mut subscribed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut warned: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut reconcile = tokio::time::interval(std::time::Duration::from_secs(1));
    reconcile.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            Some(observed) = streams.next() => match observed {
                Observed::Event(identity, envelope) => {
                    tracker.observe_agent_event(&identity, &envelope.payload);
                }
                Observed::Closed(identity) => {
                    subscribed.remove(&identity);
                }
            },
            _ = reconcile.tick() => {
                for entry in handle.list_members_including_retiring().await {
                    // Only Active members have a live runtime delta stream;
                    // subscribing others fails every tick (the console
                    // forwarder learned this the hard way).
                    if entry.status != meerkat_mob::MobMemberStatus::Active {
                        continue;
                    }
                    let identity = entry.agent_identity.to_string();
                    if subscribed.contains(&identity) {
                        continue;
                    }
                    match handle.subscribe_agent_events(&entry.agent_identity).await {
                        Ok(stream) => {
                            warned.remove(&identity);
                            subscribed.insert(identity.clone());
                            let close_key = identity.clone();
                            streams.push(
                                stream
                                    .map(move |envelope| {
                                        Observed::Event(identity.clone(), Box::new(envelope))
                                    })
                                    .chain(futures::stream::once(async move {
                                        Observed::Closed(close_key)
                                    }))
                                    .boxed(),
                            );
                        }
                        Err(error) => {
                            // Usually a short-lived spawn race; retried next
                            // tick. Warn once per identity, then debug.
                            if warned.insert(identity.clone()) {
                                tracing::warn!(
                                    identity = %identity,
                                    error = %error,
                                    "agent memory taint observer: failed to subscribe; will retry"
                                );
                            } else {
                                tracing::debug!(
                                    identity = %identity,
                                    error = %error,
                                    "agent memory taint observer: subscribe still failing"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use meerkat_core::types::{ContentBlock, ServerToolKind, SessionId};
    use serde_json::json;

    fn run_started(session: &SessionId) -> AgentEvent {
        AgentEvent::RunStarted {
            session_id: session.clone(),
            input: meerkat_core::types::RunInput::Content {
                content: meerkat_core::ContentInput::Text("hi".to_string()),
            },
        }
    }

    fn tool_result(name: &str) -> AgentEvent {
        AgentEvent::ToolResultReceived {
            id: "tool-1".to_string(),
            name: name.to_string(),
            content: vec![ContentBlock::Text {
                text: "ok".to_string(),
            }],
            is_error: false,
        }
    }

    #[test]
    fn content_trust_parse_rejects_unknown_fields_and_bad_types() {
        let err = ContentTrustConfig::from_json_value(&json!({"servers": []}))
            .expect_err("unknown field must fail loud");
        assert!(err.contains("unsupported content_trust fields"), "{err}");
        let err = ContentTrustConfig::from_json_value(&json!({"trusted_mcp_servers": "kg"}))
            .expect_err("non-array must fail loud");
        assert!(err.contains("must be an array"), "{err}");
        let err = ContentTrustConfig::from_json_value(&json!({"untrusted_tools": [1]}))
            .expect_err("non-string entry must fail loud");
        assert!(err.contains("non-empty strings"), "{err}");
        let err =
            ContentTrustConfig::from_json_value(&json!([])).expect_err("non-object must fail loud");
        assert!(err.contains("must be an object"), "{err}");
    }

    #[test]
    fn content_trust_parse_accepts_full_block() {
        let config = ContentTrustConfig::from_json_value(&json!({
            "trusted_mcp_servers": ["knowledge_graph"],
            "untrusted_tools": ["scrape_page"],
            "trusted_tools": ["mcp__scanner__lint"],
        }))
        .expect("valid block parses");
        assert_eq!(config.trusted_mcp_servers, vec!["knowledge_graph"]);
        assert_eq!(config.untrusted_tools, vec!["scrape_page"]);
        assert_eq!(config.trusted_tools, vec!["mcp__scanner__lint"]);
    }

    #[test]
    fn classification_precedence_holds() {
        let config = ContentTrustConfig {
            trusted_mcp_servers: vec!["kg".to_string()],
            untrusted_tools: vec!["scrape_page".to_string()],
            // Web builtins are never overridable.
            trusted_tools: vec!["web_search".to_string(), "mcp__evil__probe".to_string()],
        };
        assert!(matches!(
            config.classify_tool("web_search"),
            ToolContentTrust::Untrusted { .. }
        ));
        assert!(matches!(
            config.classify_tool("scrape_page"),
            ToolContentTrust::Untrusted { .. }
        ));
        // Explicit per-tool trust overrides server-level distrust.
        assert_eq!(
            config.classify_tool("mcp__evil__probe"),
            ToolContentTrust::Trusted
        );
        // MCP untrusted by default; allowlisted server trusted.
        assert!(matches!(
            config.classify_tool("mcp__other__search"),
            ToolContentTrust::Untrusted { .. }
        ));
        assert_eq!(
            config.classify_tool("mcp__kg__query"),
            ToolContentTrust::Trusted
        );
        // Unknown plain names are trusted in P1 (documented coarseness).
        assert_eq!(config.classify_tool("shell"), ToolContentTrust::Trusted);
    }

    #[test]
    fn tracker_taints_on_untrusted_tool_and_clears_on_rotation() {
        let tracker = SessionTaintTracker::new(ContentTrustConfig::default());
        let session = SessionId::new();
        tracker.observe_agent_event("identity:a", &run_started(&session));
        assert!(tracker.identity_taint("identity:a").is_none());

        tracker.observe_agent_event("identity:a", &tool_result("shell"));
        assert!(tracker.identity_taint("identity:a").is_none());

        tracker.observe_agent_event("identity:a", &tool_result("web_search"));
        let taint = tracker
            .identity_taint("identity:a")
            .expect("web tool result taints the session");
        assert!(taint.source.contains("web_search"), "{}", taint.source);
        assert!(tracker.session_taint(&session.to_string()).is_some());

        // Session-sticky: a later benign event does not clear.
        tracker.observe_agent_event("identity:a", &tool_result("shell"));
        assert!(tracker.identity_taint("identity:a").is_some());

        // Rotation (reset/respawn/fresh spawn mint a new session id) clears.
        let fresh = SessionId::new();
        tracker.observe_agent_event("identity:a", &run_started(&fresh));
        assert!(tracker.identity_taint("identity:a").is_none());
        // The old session's fact remains recorded (P2 comms joins read it).
        assert!(tracker.session_taint(&session.to_string()).is_some());
    }

    #[test]
    fn tracker_taints_on_server_tool_content() {
        let tracker = SessionTaintTracker::new(ContentTrustConfig::default());
        let session = SessionId::new();
        tracker.note_current_session("identity:a", &session.to_string());
        tracker.observe_agent_event(
            "identity:a",
            &AgentEvent::ServerToolContent {
                id: None,
                kind: ServerToolKind::WebSearch,
                content: json!({"results": []}),
            },
        );
        let taint = tracker.identity_taint("identity:a").expect("taints");
        assert!(taint.source.contains("web_search"), "{}", taint.source);
    }

    #[test]
    fn pre_attribution_taint_holds_identity_sticky_then_transfers() {
        let tracker = SessionTaintTracker::new(ContentTrustConfig::default());
        // Tool event before any RunStarted (mid-run attach).
        tracker.observe_agent_event("identity:a", &tool_result("fetch"));
        assert!(tracker.identity_taint("identity:a").is_some());

        // The next attributed session inherits the pending taint.
        let session = SessionId::new();
        tracker.observe_agent_event("identity:a", &run_started(&session));
        assert!(tracker.session_taint(&session.to_string()).is_some());
        assert!(tracker.identity_taint("identity:a").is_some());
    }

    #[test]
    fn clear_identity_drops_taint_explicitly() {
        let tracker = SessionTaintTracker::new(ContentTrustConfig::default());
        let session = SessionId::new();
        tracker.observe_agent_event("identity:a", &run_started(&session));
        tracker.observe_agent_event("identity:a", &tool_result("web_fetch"));
        assert!(tracker.identity_taint("identity:a").is_some());
        tracker.clear_identity("identity:a");
        assert!(tracker.identity_taint("identity:a").is_none());
        assert!(tracker.session_taint(&session.to_string()).is_none());
    }

    #[test]
    fn gate_quarantines_tainted_agents_and_quarantined_policy() {
        let tracker = SessionTaintTracker::new(ContentTrustConfig::default());
        let session = SessionId::new();
        tracker.observe_agent_event("identity:a", &run_started(&session));

        let gate = TaintLlmWriteGate::new(Some(tracker.clone()), AgentMemoryLlmWrites::Observed);
        let agent = MemoryAuthor::Agent {
            identity: "identity:a".to_string(),
        };
        assert!(gate.quarantine_reason(&agent).is_none());
        assert!(gate.quarantine_reason(&MemoryAuthor::Application).is_none());

        tracker.observe_agent_event("identity:a", &tool_result("web_search"));
        let reason = gate.quarantine_reason(&agent).expect("tainted quarantines");
        assert!(reason.contains("session tainted"), "{reason}");
        // Non-LLM principals are never gated, tainted or not.
        assert!(gate.quarantine_reason(&MemoryAuthor::Application).is_none());
        assert!(gate.quarantine_reason(&MemoryAuthor::Operator).is_none());

        // llm_writes=quarantined forces quarantine with no taint at all.
        let strict = TaintLlmWriteGate::new(None, AgentMemoryLlmWrites::Quarantined);
        let reason = strict
            .quarantine_reason(&MemoryAuthor::Agent {
                identity: "identity:clean".to_string(),
            })
            .expect("policy quarantines untainted writes");
        assert!(reason.contains("llm_writes=quarantined"), "{reason}");
        assert!(
            strict
                .quarantine_reason(&MemoryAuthor::Steward {
                    run_id: "run-1".to_string()
                })
                .is_some()
        );
        assert!(strict.quarantine_reason(&MemoryAuthor::Operator).is_none());
    }
}
