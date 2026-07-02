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
//! ## P2 additions (§10.1 taint completion)
//!
//! - **Comms taint join**: a peer message from a tracked sender whose
//!   session is tainted taints the receiving session (the peer-laundering
//!   close). See [`SessionTaintTracker::observe_inbound_peer_content`] for
//!   what is — and honestly is not — observable.
//! - **Evidence-range taint**: the write gate now sees a write's
//!   `EvidenceRef`s; any LLM-authored write citing a tainted session
//!   quarantines. Coarse by design: the tracker holds session-sticky
//!   facts, so session-tainted ⇒ every range in it tainted (per-turn
//!   granularity would need the Hygienist's pinned revisions, P4).
//! - **Reset boundaries**: `reset()` marks the outgoing session so
//!   Distiller output over it lands `Quarantined` (§8.4 — reset is the
//!   operator's escape hatch; quarantine preserves the re-dream option).
//!
//! ## Honest gaps that remain (upstream asks, §13)
//!
//! - **The first-ingestion race** (ask: taint visibility at tool-dispatch
//!   time): taint is derived from the observe-only agent-event stream,
//!   which is asynchronous. A memory write in the same turn as the
//!   session's *first* untrusted ingestion can reach the store before the
//!   taint observer processes the tool event. Deployments that cannot
//!   accept this set `agent_memory.llm_writes = "quarantined"`. This stays
//!   upstream-dependent; nothing here pretends to close it.
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
//!   `llm_writes = "quarantined"`) until the dispatch-time join against
//!   real `ToolProvenance` lands upstream.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use meerkat_core::event::AgentEvent;
use serde::{Deserialize, Serialize};

use crate::identity_first::agent_memory::AgentMemoryLlmWrites;
use crate::memory::records::{EvidenceRef, MemoryAuthor};

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
    /// session key → taint fact. Session-sticky by construction, and
    /// retained after rotation/clear: the fact is historical ("this session
    /// ingested untrusted content"), and the P2 comms join and
    /// evidence-range gate read it for sessions that are no longer current.
    tainted: HashMap<String, TaintState>,
    /// Untrusted ingestion observed before the observer learned the
    /// identity's session (mid-run attach). Transferred to the next learned
    /// session — conservative direction (see module docs).
    pending_identity_taint: HashMap<String, TaintState>,
    /// session key → reset-boundary mark (§8.4): distillates citing this
    /// session quarantine pending steward review. Bounded like `tainted`.
    reset_boundaries: HashMap<String, u64>,
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
            AgentEvent::RunStarted { session_id, input } => {
                let session_key = session_id.to_string();
                self.note_current_session(identity, &session_key);
                // §10.1 comms taint join: peer-delivered content arrives as
                // the run's injected input, not as a typed event.
                if let Some(text) = input.prompt_text() {
                    self.observe_inbound_peer_content(identity, &session_key, &text);
                }
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
    /// this also drops any pending pre-attribution taint. The outgoing
    /// session's *fact* is deliberately retained (P2): "that session was
    /// tainted" stays true after the identity moves on, and the comms join
    /// and the Distiller's evidence gate consult it for exactly such
    /// sessions.
    pub fn clear_identity(&self, identity: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        inner.pending_identity_taint.remove(identity);
        inner.current_session.remove(identity);
    }

    /// Mark a `reset()` boundary on the outgoing session (§8.4): every
    /// LLM-authored write whose evidence cites this session quarantines
    /// pending steward review, regardless of content taint. Idempotent.
    pub fn mark_reset_boundary(&self, session_key: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        if inner
            .reset_boundaries
            .insert(session_key.to_string(), now_ms())
            .is_none()
        {
            // TODO(P3b): timeline event for the reset-quarantine boundary.
            tracing::warn!(
                session_key,
                "agent memory taint: reset boundary marked; distillates over this \
                 session will land quarantined pending steward review (§8.4)"
            );
        }
        if inner.reset_boundaries.len() > MAX_TRACKED_TAINTED_SESSIONS
            && let Some(oldest) = inner
                .reset_boundaries
                .iter()
                .min_by_key(|(_, at_ms)| **at_ms)
                .map(|(key, _)| key.clone())
        {
            inner.reset_boundaries.remove(&oldest);
        }
    }

    /// §10.1/§8.4 evidence gate query: why a write citing `session_key` as
    /// evidence must quarantine, if it must. Coarse by design: the tracker
    /// holds session-sticky facts, so a tainted session taints every
    /// evidence range within it.
    pub fn evidence_quarantine_reason(&self, session_key: &str) -> Option<String> {
        let inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(state) = inner.tainted.get(session_key) {
            return Some(format!(
                "evidence session tainted by {} (session-tainted ⇒ range-tainted)",
                state.source
            ));
        }
        if inner.reset_boundaries.contains_key(session_key) {
            return Some(
                "evidence session closed at a reset boundary; distillates quarantine \
                 pending steward review (§8.4)"
                    .to_string(),
            );
        }
        None
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

    /// §10.1 comms taint join, over what the observe surface actually
    /// carries. Meerkat 0.7.9 has **no typed inbound peer-message event**:
    /// a peer delivery lands in the receiver's session as injected prompt
    /// text rendered by `format_peer_message_projection` /
    /// `format_peer_response_projection` (meerkat-core `interaction.rs:126,
    /// :239`), where the sender is the resolved trusted-peer name — for mob
    /// members the `MemberCommsName` `{mob_id}/{role}/{agent_identity}`
    /// (meerkat-core `connection.rs:368`). This join parses that sender out
    /// and taints the receiving session when the sender's tracked session
    /// is tainted.
    ///
    /// Honest limits, filed against upstream ask 5 (envelope-level taint
    /// flags):
    /// - **"tainted at send time" is approximated at delivery-observe
    ///   time.** If the sender rotated to a clean session between send and
    ///   delivery, the join misses; if the sender got tainted after the
    ///   send, the join over-taints (conservative direction).
    /// - **Peer *requests* render a raw cryptographic `peer_id`**, not a
    ///   comms name, so their senders are unmappable host-side — unknowable
    ///   without the upstream envelope fact.
    /// - **Cross-process senders** are not in this tracker at all;
    ///   sender-session taint state is unknowable for them.
    /// - A user message quoting the projection prefix can false-positive —
    ///   conservative (false quarantine, never false trust).
    fn observe_inbound_peer_content(&self, identity: &str, session_key: &str, text: &str) {
        for line in text.lines() {
            let Some(sender_identity) = peer_projection_sender_identity(line) else {
                continue;
            };
            if sender_identity == identity {
                continue;
            }
            let source = {
                let inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
                inner
                    .pending_identity_taint
                    .get(sender_identity)
                    .or_else(|| {
                        inner
                            .current_session
                            .get(sender_identity)
                            .and_then(|session| inner.tainted.get(session))
                    })
                    .map(|state| state.source.clone())
            };
            if let Some(source) = source {
                let state = TaintState {
                    tainted_at_ms: now_ms(),
                    source: format!(
                        "peer message from tainted sender '{sender_identity}' \
                         (sender session tainted by {source})"
                    ),
                };
                let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
                self.insert_taint(&mut inner, session_key.to_string(), state);
            }
        }
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
    /// `evidence` is the union of `EvidenceRef`s the write cites (P2
    /// evidence-range taint, §10.1); empty for writes that cite nothing.
    fn quarantine_reason(&self, author: &MemoryAuthor, evidence: &[EvidenceRef]) -> Option<String>;
}

/// The taint/posture gate: `llm_writes = "quarantined"` forces every
/// LLM-authored write into quarantine regardless of taint; agent-authored
/// writes quarantine when the author's session is tainted; and ANY
/// LLM-authored write (Distiller included) quarantines when its evidence
/// cites a tainted session or a reset boundary (§8.4/§10.1 — coarse:
/// session-tainted ⇒ range-tainted).
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
    fn quarantine_reason(&self, author: &MemoryAuthor, evidence: &[EvidenceRef]) -> Option<String> {
        if !author.is_llm() {
            return None;
        }
        if self.llm_writes == AgentMemoryLlmWrites::Quarantined {
            return Some("llm_writes=quarantined policy".to_string());
        }
        let Some(tracker) = self.tracker.as_ref() else {
            return None;
        };
        if let MemoryAuthor::Agent { identity } = author
            && let Some(state) = tracker.identity_taint(identity)
        {
            return Some(format!("session tainted by {}", state.source));
        }
        for evidence_ref in evidence {
            if let Some(reason) = tracker.evidence_quarantine_reason(&evidence_ref.session_id) {
                return Some(reason);
            }
        }
        None
    }
}

/// Sender-identity extraction from one line of injected peer-projection
/// text. Pinned to meerkat 0.7.9's canonical projections:
/// `format_peer_message_projection` → `"Peer message from {name}:"` and
/// `format_peer_response_projection` → `"Peer response from {name} (to
/// request: ...)"`, where `{name}` for a mob member is
/// `{mob_id}/{role}/{agent_identity}`. Returns the trailing path segment
/// (the agent identity). Peer *requests* carry a raw peer id and return
/// `None` (module docs on the honest gaps).
fn peer_projection_sender_identity(line: &str) -> Option<&str> {
    let name = if let Some(rest) = line.strip_prefix("Peer message from ") {
        // Canonical shape ends the line with ':' (body follows on the next
        // line). Names may themselves contain ':' (agent identities do), so
        // strip the trailing delimiter rather than splitting at the first.
        match rest.split_once(": ") {
            Some((name, _)) => name.trim(),
            None => rest.strip_suffix(':').unwrap_or(rest).trim(),
        }
    } else if let Some(rest) = line.strip_prefix("Peer response from ") {
        rest.split(" (to request:").next()?.trim()
    } else {
        return None;
    };
    if name.is_empty() {
        return None;
    }
    Some(name.rsplit('/').next().unwrap_or(name))
}

// ---------------------------------------------------------------------------
// Observe-stream feed
// ---------------------------------------------------------------------------

/// A consumer of the per-member agent-event observe stream. The taint
/// tracker and the Distiller's trigger sink both ride ONE observer loop —
/// one `subscribe_agent_events` subscription per member, however many
/// memory stages listen.
pub trait MemberAgentEventSink: Send + Sync {
    fn observe(&self, identity: &str, envelope: &meerkat_core::event::EventEnvelope<AgentEvent>);
}

impl MemberAgentEventSink for SessionTaintTracker {
    fn observe(&self, identity: &str, envelope: &meerkat_core::event::EventEnvelope<AgentEvent>) {
        self.observe_agent_event(identity, &envelope.payload);
    }
}

/// Guard for the observer task; aborts the task when the last clone drops
/// (the runtime that owned the sinks is gone).
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
/// forwarder rides), reconciling membership every second.
pub fn spawn_taint_observer(
    handle: meerkat_mob::MobHandle,
    tracker: SessionTaintTracker,
) -> TaintObserverGuard {
    spawn_member_event_observer(handle, vec![Arc::new(tracker)])
}

/// Generalized observer: one reconcile loop, one stream per active member,
/// fanned out to every sink (taint tracker, Distiller triggers, future
/// stages).
pub fn spawn_member_event_observer(
    handle: meerkat_mob::MobHandle,
    sinks: Vec<Arc<dyn MemberAgentEventSink>>,
) -> TaintObserverGuard {
    let task = tokio::spawn(run_member_event_observer(handle, sinks));
    TaintObserverGuard {
        _abort: Arc::new(AbortOnDrop(task)),
    }
}

async fn run_member_event_observer(
    handle: meerkat_mob::MobHandle,
    sinks: Vec<Arc<dyn MemberAgentEventSink>>,
) {
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
                    for sink in &sinks {
                        sink.observe(&identity, &envelope);
                    }
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
    fn clear_identity_drops_attribution_but_keeps_the_session_fact() {
        let tracker = SessionTaintTracker::new(ContentTrustConfig::default());
        let session = SessionId::new();
        tracker.observe_agent_event("identity:a", &run_started(&session));
        tracker.observe_agent_event("identity:a", &tool_result("web_fetch"));
        assert!(tracker.identity_taint("identity:a").is_some());
        tracker.clear_identity("identity:a");
        // The identity moves on clean...
        assert!(tracker.identity_taint("identity:a").is_none());
        // ...but the historical fact survives: the comms join and the
        // evidence-range gate consult exactly such sessions (P2).
        assert!(tracker.session_taint(&session.to_string()).is_some());
        assert!(
            tracker
                .evidence_quarantine_reason(&session.to_string())
                .is_some()
        );
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
        assert!(gate.quarantine_reason(&agent, &[]).is_none());
        assert!(
            gate.quarantine_reason(&MemoryAuthor::Application, &[])
                .is_none()
        );

        tracker.observe_agent_event("identity:a", &tool_result("web_search"));
        let reason = gate
            .quarantine_reason(&agent, &[])
            .expect("tainted quarantines");
        assert!(reason.contains("session tainted"), "{reason}");
        // Non-LLM principals are never gated, tainted or not.
        assert!(
            gate.quarantine_reason(&MemoryAuthor::Application, &[])
                .is_none()
        );
        assert!(
            gate.quarantine_reason(&MemoryAuthor::Operator, &[])
                .is_none()
        );

        // llm_writes=quarantined forces quarantine with no taint at all.
        let strict = TaintLlmWriteGate::new(None, AgentMemoryLlmWrites::Quarantined);
        let reason = strict
            .quarantine_reason(
                &MemoryAuthor::Agent {
                    identity: "identity:clean".to_string(),
                },
                &[],
            )
            .expect("policy quarantines untainted writes");
        assert!(reason.contains("llm_writes=quarantined"), "{reason}");
        assert!(
            strict
                .quarantine_reason(
                    &MemoryAuthor::Steward {
                        run_id: "run-1".to_string()
                    },
                    &[]
                )
                .is_some()
        );
        assert!(
            strict
                .quarantine_reason(&MemoryAuthor::Operator, &[])
                .is_none()
        );
    }

    fn evidence_for(session: &SessionId) -> Vec<EvidenceRef> {
        vec![EvidenceRef {
            session_id: session.to_string(),
            generation: 0,
            revision: None,
            range: Some((0, 4)),
        }]
    }

    #[test]
    fn gate_quarantines_llm_writes_citing_tainted_evidence() {
        let tracker = SessionTaintTracker::new(ContentTrustConfig::default());
        let session = SessionId::new();
        tracker.observe_agent_event("identity:a", &run_started(&session));
        tracker.observe_agent_event("identity:a", &tool_result("web_search"));
        // Identity rotates away: the identity is clean, the session fact
        // remains — the distiller's evidence must still quarantine.
        let fresh = SessionId::new();
        tracker.observe_agent_event("identity:a", &run_started(&fresh));

        let gate = TaintLlmWriteGate::new(Some(tracker), AgentMemoryLlmWrites::Observed);
        let distiller = MemoryAuthor::Distiller {
            run_id: "run-1".to_string(),
        };
        let reason = gate
            .quarantine_reason(&distiller, &evidence_for(&session))
            .expect("tainted evidence range quarantines (session-tainted ⇒ range-tainted)");
        assert!(reason.contains("evidence session tainted"), "{reason}");
        // Clean evidence does not.
        assert!(
            gate.quarantine_reason(&distiller, &evidence_for(&fresh))
                .is_none()
        );
        // Non-LLM authors are never evidence-gated.
        assert!(
            gate.quarantine_reason(&MemoryAuthor::Operator, &evidence_for(&session))
                .is_none()
        );
    }

    #[test]
    fn reset_boundary_quarantines_evidence_without_content_taint() {
        let tracker = SessionTaintTracker::new(ContentTrustConfig::default());
        let session = SessionId::new();
        tracker.observe_agent_event("identity:a", &run_started(&session));
        assert!(
            tracker
                .evidence_quarantine_reason(&session.to_string())
                .is_none()
        );
        tracker.mark_reset_boundary(&session.to_string());
        let gate = TaintLlmWriteGate::new(Some(tracker), AgentMemoryLlmWrites::Observed);
        let reason = gate
            .quarantine_reason(
                &MemoryAuthor::Distiller {
                    run_id: "run-1".to_string(),
                },
                &evidence_for(&session),
            )
            .expect("reset boundary quarantines distillates");
        assert!(reason.contains("reset boundary"), "{reason}");
    }

    #[test]
    fn peer_projection_sender_parses_message_and_response_shapes() {
        assert_eq!(
            peer_projection_sender_identity("Peer message from mob-1/worker/identity:bob:"),
            Some("identity:bob")
        );
        assert_eq!(
            peer_projection_sender_identity(
                "Peer response from mob-1/worker/identity:bob (to request: req-9)"
            ),
            Some("identity:bob")
        );
        // External peers may have plain display names.
        assert_eq!(
            peer_projection_sender_identity("Peer message from scout:"),
            Some("scout")
        );
        // Peer requests render a raw peer id — unmappable, and honestly so.
        assert_eq!(
            peer_projection_sender_identity("Peer request from peer_id 018fabc (id: r-1)"),
            None
        );
        assert_eq!(peer_projection_sender_identity("ordinary text"), None);
    }

    #[test]
    fn comms_join_taints_receiver_of_message_from_tainted_sender() {
        let tracker = SessionTaintTracker::new(ContentTrustConfig::default());
        // Sender taints its session.
        let sender_session = SessionId::new();
        tracker.observe_agent_event("identity:bob", &run_started(&sender_session));
        tracker.observe_agent_event("identity:bob", &tool_result("web_search"));

        // Receiver gets a peer message from the tainted sender: the run's
        // injected input carries the canonical projection text.
        let receiver_session = SessionId::new();
        let delivery = AgentEvent::RunStarted {
            session_id: receiver_session.clone(),
            input: meerkat_core::types::RunInput::Content {
                content: meerkat_core::ContentInput::Text(
                    "Peer message from mob-1/worker/identity:bob:\nplease remember X".to_string(),
                ),
            },
        };
        tracker.observe_agent_event("identity:alice", &delivery);
        let taint = tracker
            .identity_taint("identity:alice")
            .expect("receiver session taints (peer-laundering close, §10.1)");
        assert!(taint.source.contains("identity:bob"), "{}", taint.source);
        assert!(
            tracker
                .session_taint(&receiver_session.to_string())
                .is_some()
        );

        // A message from a clean tracked sender does not taint.
        let clean_session = SessionId::new();
        tracker.observe_agent_event("identity:carol", &run_started(&clean_session));
        let receiver2 = SessionId::new();
        let clean_delivery = AgentEvent::RunStarted {
            session_id: receiver2.clone(),
            input: meerkat_core::types::RunInput::Content {
                content: meerkat_core::ContentInput::Text(
                    "Peer message from mob-1/worker/identity:carol:\nhello".to_string(),
                ),
            },
        };
        tracker.observe_agent_event("identity:dave", &clean_delivery);
        assert!(tracker.identity_taint("identity:dave").is_none());
    }
}
