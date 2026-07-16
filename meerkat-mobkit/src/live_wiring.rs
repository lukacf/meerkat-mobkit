//! Live (realtime) member sessions through the mobkit gateways.
//!
//! Port of meerkat-rpc's `SessionServiceProjectionSink`
//! (`meerkat-rpc/src/live_projection_sink.rs`), `RuntimeLiveToolDispatcher`
//! (`router.rs`), the `live/*` handlers (`handlers/live.rs`, websocket arms
//! only) and the per-open factory composition (`live_wiring.rs`) onto
//! mobkit's `PersistentSessionService<B>` + `MeerkatMachine` pair. The
//! reference composition lives in the meerkat-rpc BINARY crate and is not
//! re-exported, so mobkit carries this glue itself (design:
//! `docs/design/live-sessions.md`).
//!
//! Translation map vs the reference:
//! - `SessionRuntime::runtime_adapter()` → the gateway's `MeerkatMachine`
//!   (the same one wired for schedules and image generation — one machine
//!   per process, machine-owned live lifecycle authority).
//! - `SessionRuntime::<session seam>` → the same-named inherent method on
//!   `PersistentSessionService<B>` (`append_realtime_transcript_event`,
//!   `append_external_user_content`, `append_external_assistant_output`,
//!   `dispatch_external_tool_call`); `record_live_terminal_error` /
//!   `record_live_output_audio_degraded` route through the `SessionService`
//!   trait impl the persistent service overrides.
//! - `interrupt_live_with_machine_authority` →
//!   `interrupt_with_machine_authority(id, machine.session_control_authority())`.
//! - `in_flight_realtime_assistant_response_ids` →
//!   `load_authoritative_session` + the `Session` accessor.
//! - `live_open_config_for_session` → rebuilt here from the published
//!   `PersistentSessionService` snapshot seams + the facade's pub
//!   `realtime_projection_*` free functions. The reference additionally
//!   recovers archived/staged sessions before projecting
//!   (`recover_live_session_for_realtime_open`); mobkit member sessions are
//!   held live by the bridge, so an archived target surfaces as a typed
//!   not-found instead of being silently resumed.
//! - `ensure_live_peer_ingress` has no mobkit equivalent: member comms
//!   ingress is wired at member build time by the mob runtime, not lazily
//!   per live open.
//!
//! Everything else is kept verbatim: the pending-turn buffer keyed on
//! `(SessionId, response_id)` (R6), the display-text vs spoken-transcript
//! lane split (T6/CC2/CC3), fail-closed delta identity (#199), the
//! `LiveProjectionError::from_session_error` classification, and the
//! machine-authority-first shape of every handler (no public result or
//! error class is minted surface-side).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use async_trait::async_trait;
use meerkat::AgentFactory;
use meerkat::session_runtime::LiveOpenPrecheckError;
use meerkat::session_runtime::live_orchestration::{
    precheck_identity, realtime_projection_messages, realtime_projection_root_system_message,
    realtime_projection_runtime_system_context,
};
use meerkat::session_runtime::realtime_credentials::RealtimeCurrentConfigSource;
use meerkat_client::realtime_session::{RealtimeSessionFactory, RealtimeSessionOpenConfig};
use meerkat_contracts::{
    LiveChannelParams, LiveCloseResult, LiveCommitInputParams, LiveCommitInputResult,
    LiveInterruptResult, LiveOpenResult, LiveOpenTransport, LiveRefreshResult, LiveSendInputParams,
    LiveSendInputResult, LiveStatusResult, LiveTruncateParams, LiveTruncateResult,
    RealtimeCapabilities, RealtimeTurningMode, WireLiveAdapterStatus, WireLiveDegradationReason,
};
use meerkat_core::live_adapter::{
    LiveAdapterCommand, LiveAdapterErrorCode, LiveAudioConfig, LiveChannelCapabilities,
    LiveContinuityMode, LiveProjectionSnapshot, LiveTransportBootstrap,
};
use meerkat_core::service::SessionService as _;
use meerkat_core::session::SystemContextSource;
use meerkat_core::types::{AssistantBlock, ContentInput, Message, SessionId, StopReason, Usage};
use meerkat_core::{
    Config, ConfigError, CoreRenderable, PendingSystemContextAppend, RealtimeTranscriptEvent,
};
use meerkat_live::{
    LiveAdapterHost, LiveAdapterHostError, LiveChannelCloseFeedback, LiveChannelCloseObservation,
    LiveChannelId, LiveChannelStatusFeedback, LiveChannelStatusObservation, LiveProjectionError,
    LiveProjectionSink, LiveTokenString, LiveToolDispatcher, LiveTranscriptIdentity,
    LiveTranscriptIdentityError, LiveWsState, LiveWsTokenAdmission,
    LiveWsTokenAdmissionPublicErrorClass, LiveWsTokenAdmissionRejection, LiveWsTokenAuthority,
    LiveWsTokenIssue, live_input_chunk_from_wire,
};
use meerkat_runtime::MeerkatMachine;
use meerkat_session::{PersistentSessionService, SessionAgentBuilder};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::rpc::{JSONRPC_VERSION, JsonRpcError, JsonRpcResponse};

/// JSON-RPC error code for live methods invoked on a gateway with no live
/// context (ephemeral mode, or live disabled at init). The CALLER owns this
/// arm — `handle_live_method` is only reachable with a live context — but
/// the code and the `data.kind` discriminator live here so every surface
/// answers identically.
pub const LIVE_UNAVAILABLE_CODE: i64 = -32050;

/// `data.kind` discriminator carried by [`live_unavailable_response`].
pub const LIVE_UNAVAILABLE_KIND: &str = "live_unavailable";

const INVALID_PARAMS_CODE: i64 = -32602;
const METHOD_NOT_FOUND_CODE: i64 = -32601;
const INTERNAL_ERROR_CODE: i64 = -32000;

/// One buffered assistant **display-text** fragment awaiting an
/// authoritative `signal_turn_completed` flush.
///
/// CC2/CC3 (upstream Round-4 reconciliation, kept verbatim): the production
/// assistant-transcript commit path is canonical via the realtime-staging
/// pipeline (`append_realtime_transcript_event` → session staging →
/// materializer flush at `AssistantTurnCompleted`). The buffered-final path
/// retained here covers **only** the display-text lane: spoken transcript
/// commits through staging (buffering it here would commit twice), and
/// display text keeps this lane because no typed `AssistantTextFinal`
/// observation exists upstream yet.
#[derive(Debug, Clone)]
enum PendingAssistantContent {
    /// Display lane — flushes as `AssistantBlock::Text`.
    Text(String),
}

#[derive(Debug, Default)]
struct PendingTurn {
    /// Display-text fragments in arrival order. Consecutive fragments
    /// coalesce into a single `AssistantBlock::Text` at flush time.
    blocks: Vec<PendingAssistantContent>,
}

/// Per-(session, response_id) buffer of assistant finals awaiting an
/// authoritative `TurnCompleted`.
///
/// R6: keyed by `(SessionId, Option<String>)` where the second element is
/// the provider's `response_id`. Keyed only on `SessionId`, an interrupted,
/// stale, or overlapping `response.done` carrying the next turn's
/// `stop_reason`/`usage` would flush the wrong buffered transcript. Orphan
/// completions (no `response_id` from the provider) bucket under the same
/// `None` slot, preserving legacy behaviour for adapters without response
/// identity. Extracted from the sink so the keying contract is unit-testable
/// without a `PersistentSessionService`.
#[derive(Default)]
struct PendingTurnLedger {
    /// The lock is held only for short, sync sections — never across
    /// `.await`.
    slots: StdMutex<HashMap<(SessionId, Option<String>), PendingTurn>>,
}

impl PendingTurnLedger {
    fn buffer(
        &self,
        session_id: &SessionId,
        response_id: Option<&str>,
        content: PendingAssistantContent,
    ) {
        let Ok(mut slots) = self.slots.lock() else {
            // Lock poisoning would mean a previous panic in this struct; the
            // only operations holding the guard are short sync reads/writes.
            // Falling through silently preserves the prior buffer rather
            // than breaking the in-flight turn.
            return;
        };
        slots
            .entry((session_id.clone(), response_id.map(ToString::to_string)))
            .or_default()
            .blocks
            .push(content);
    }

    /// Drain the matching slot at turn-completed time.
    ///
    /// R6: callers must pass the same `response_id` they buffered under so
    /// the matching slot drains; mismatched ids leave the buffer alone — a
    /// stale/overlapping `response.done` cannot flush a different turn's
    /// transcript.
    fn drain(&self, session_id: &SessionId, response_id: Option<&str>) -> PendingTurn {
        let Ok(mut slots) = self.slots.lock() else {
            return PendingTurn::default();
        };
        slots
            .remove(&(session_id.clone(), response_id.map(ToString::to_string)))
            .unwrap_or_default()
    }

    /// Drain every slot for `session_id` regardless of `response_id`.
    ///
    /// Used on terminal error: when the channel is torn down every buffered
    /// final becomes non-canonical, so per-response keying does not need to
    /// be honoured for cleanup.
    fn drain_all(&self, session_id: &SessionId) {
        let Ok(mut slots) = self.slots.lock() else {
            return;
        };
        slots.retain(|(sid, _resp), _| sid != session_id);
    }
}

/// Bridges [`LiveProjectionSink`] callbacks (plus the transport feedback and
/// WS-token-authority seams) into the gateway's persistent session service
/// and machine. Port of meerkat-rpc's `SessionServiceProjectionSink`.
pub struct GatewayLiveProjectionSink<B: SessionAgentBuilder + 'static> {
    service: Arc<PersistentSessionService<B>>,
    machine: Arc<MeerkatMachine>,
    pending_turns: PendingTurnLedger,
}

impl<B: SessionAgentBuilder + 'static> GatewayLiveProjectionSink<B> {
    pub fn new(service: Arc<PersistentSessionService<B>>, machine: Arc<MeerkatMachine>) -> Self {
        Self {
            service,
            machine,
            pending_turns: PendingTurnLedger::default(),
        }
    }
}

/// Build the realtime-transcript event the sink stages on the
/// **display-text** lane. Extracted so unit tests can assert the exact event
/// shape without a service.
///
/// #199: the required delta identity triple (`response_id` / `delta_id` /
/// `item_id`) resolves through the canonical fail-closed accessor
/// [`LiveTranscriptIdentity::require_delta_identity`]; a delta missing any
/// required id yields a typed [`LiveTranscriptIdentityError`] so the
/// projection rejects the malformed delta instead of emitting empty-string
/// identity truth.
fn build_assistant_text_delta_event(
    delta: &str,
    identity: LiveTranscriptIdentity<'_>,
) -> Result<RealtimeTranscriptEvent, LiveTranscriptIdentityError> {
    let resolved = identity.require_delta_identity()?;
    Ok(RealtimeTranscriptEvent::AssistantTextDelta {
        response_id: resolved.response_id.to_string(),
        delta_id: resolved.delta_id.to_string(),
        item_id: resolved.item_id.to_string(),
        previous_item_id: resolved.previous_item_id.map(ToString::to_string),
        content_index: resolved.content_index.unwrap_or(0),
        delta: delta.to_string(),
    })
}

/// Build the realtime-transcript event the sink stages on the
/// **spoken-transcript** lane (T9/T10: the dedicated
/// `AssistantTranscriptDelta` variant, so the materializer flushes
/// `AssistantBlock::Transcript { source: Spoken }` rather than `Text`).
fn build_assistant_transcript_delta_event(
    delta: &str,
    identity: LiveTranscriptIdentity<'_>,
) -> Result<RealtimeTranscriptEvent, LiveTranscriptIdentityError> {
    let resolved = identity.require_delta_identity()?;
    Ok(RealtimeTranscriptEvent::AssistantTranscriptDelta {
        response_id: resolved.response_id.to_string(),
        delta_id: resolved.delta_id.to_string(),
        item_id: resolved.item_id.to_string(),
        previous_item_id: resolved.previous_item_id.map(ToString::to_string),
        content_index: resolved.content_index.unwrap_or(0),
        delta: delta.to_string(),
    })
}

/// A malformed delta (missing required identity) is a projection rejection,
/// not an internal fault: it lands in [`LiveProjectionError::Rejected`],
/// mirroring the `SessionError::Unsupported` classification.
fn identity_error_to_projection(err: LiveTranscriptIdentityError) -> LiveProjectionError {
    LiveProjectionError::Rejected(err.to_string())
}

/// Collapse arrival-order display-text fragments into a single
/// `AssistantBlock::Text` (or empty list if no fragments were buffered).
fn collapse_pending_blocks(buffered: Vec<PendingAssistantContent>) -> Vec<AssistantBlock> {
    if buffered.is_empty() {
        return Vec::new();
    }
    let mut acc = String::new();
    for PendingAssistantContent::Text(fragment) in buffered {
        acc.push_str(&fragment);
    }
    vec![AssistantBlock::Text {
        text: acc,
        meta: None,
    }]
}

/// Classify a [`meerkat_core::SessionError`] through the single canonical
/// classification owner [`LiveProjectionError::from_session_error`] so every
/// variant lands in a distinct typed `LiveProjectionError` — no variant is
/// collapsed into a prose-only `Internal(to_string())` at this surface.
fn session_error_to_projection(
    err: meerkat_core::SessionError,
    id: &SessionId,
) -> LiveProjectionError {
    LiveProjectionError::from_session_error(id, err)
}

#[async_trait]
impl<B: SessionAgentBuilder + 'static> LiveChannelCloseFeedback for GatewayLiveProjectionSink<B> {
    async fn record_live_channel_closed(
        &self,
        channel_id: &LiveChannelId,
        observation: &LiveChannelCloseObservation,
    ) -> Result<meerkat_live::LiveChannelCloseCommitAuthority, String> {
        let session_id = self
            .machine
            .live_session_for_active_channel(channel_id)
            .await
            .ok_or_else(|| {
                format!("generated live active-channel authority absent for channel {channel_id}")
            })?;
        self.machine
            .resolve_live_close_result(&session_id, observation)
            .await
            .map_err(|err| err.to_string())?
            .into_channel_close_commit_authority()
            .ok_or_else(|| {
                format!(
                    "generated live close authority omitted host commit handoff for channel {channel_id}"
                )
            })
    }
}

#[async_trait]
impl<B: SessionAgentBuilder + 'static> LiveChannelStatusFeedback for GatewayLiveProjectionSink<B> {
    async fn record_live_channel_status(
        &self,
        channel_id: &LiveChannelId,
        observation: &LiveChannelStatusObservation,
    ) -> Result<meerkat_live::LiveChannelStatusCommitAuthority, String> {
        if observation.channel_id() != channel_id.as_str() {
            return Err(format!(
                "generated live status observation channel mismatch: observed {}, requested {}",
                observation.channel_id(),
                channel_id
            ));
        }
        let session_id = self
            .machine
            .live_session_for_status_channel(channel_id)
            .await
            .ok_or_else(|| {
                format!("generated live status-channel authority absent for channel {channel_id}")
            })?;
        self.machine
            .resolve_live_channel_status_result(&session_id, observation)
            .await
            .map_err(|err| err.to_string())?
            .into_channel_status_commit_authority()
            .ok_or_else(|| {
                format!(
                    "generated live status authority omitted host commit handoff for channel {channel_id}"
                )
            })
    }
}

#[async_trait]
impl<B: SessionAgentBuilder + 'static> LiveWsTokenAuthority for GatewayLiveProjectionSink<B> {
    async fn record_live_ws_token_issued(
        &self,
        session_id: &SessionId,
        channel_id: &LiveChannelId,
        token: &LiveTokenString,
        issued_at_ms: u64,
        ttl_ms: u64,
    ) -> Result<LiveWsTokenIssue, String> {
        let authority = self
            .machine
            .record_live_websocket_token_issued(
                session_id,
                channel_id,
                token.as_str(),
                issued_at_ms,
                ttl_ms,
            )
            .await
            .map_err(|err| err.to_string())?;
        let token = LiveTokenString::new(authority.token).map_err(|err| err.to_string())?;
        Ok(LiveWsTokenIssue {
            token,
            expires_at_ms: authority.expires_at_ms,
            sequence: authority.sequence,
        })
    }

    async fn resolve_live_ws_token_admission(
        &self,
        channel_id: &LiveChannelId,
        token: &str,
        observed_at_ms: u64,
    ) -> Result<LiveWsTokenAdmission, String> {
        // Admission resolves against the token's OWNING session when the
        // machine knows the token; otherwise against the channel's bound
        // session; otherwise through the unbound rejection path. All three
        // arms end in generated machine authority — the transport never
        // decides token facts.
        let token_owner = self.machine.live_session_for_websocket_token(token).await;
        let authority = match token_owner {
            Some(session_id) => {
                self.machine
                    .resolve_live_websocket_token_admission(
                        &session_id,
                        channel_id,
                        token,
                        observed_at_ms,
                    )
                    .await
            }
            None => match self
                .machine
                .live_session_for_active_channel(channel_id)
                .await
            {
                Some(session_id) => {
                    self.machine
                        .resolve_live_websocket_token_admission(
                            &session_id,
                            channel_id,
                            token,
                            observed_at_ms,
                        )
                        .await
                }
                None => {
                    self.machine
                        .resolve_unbound_live_websocket_token_admission(
                            channel_id,
                            token,
                            observed_at_ms,
                        )
                        .await
                }
            },
        }
        .map_err(|err| err.to_string())?;

        Ok(LiveWsTokenAdmission {
            channel_id: channel_id.clone(),
            admitted: authority.admitted,
            rejection: authority
                .rejection
                .map(live_ws_token_admission_rejection_from_machine),
            public_error_class: authority
                .public_error_class
                .map(live_ws_token_public_error_class_from_machine),
            sequence: authority.sequence,
        })
    }
}

fn live_ws_token_admission_rejection_from_machine(
    rejection: meerkat_runtime::meerkat_machine::dsl::LiveWebsocketTokenAdmissionRejection,
) -> LiveWsTokenAdmissionRejection {
    use meerkat_runtime::meerkat_machine::dsl::LiveWebsocketTokenAdmissionRejection as Dsl;
    match rejection {
        Dsl::TokenNotFound => LiveWsTokenAdmissionRejection::TokenNotFound,
        Dsl::TokenExpired => LiveWsTokenAdmissionRejection::TokenExpired,
        Dsl::TokenChannelMismatch => LiveWsTokenAdmissionRejection::TokenChannelMismatch,
        Dsl::TokenAlreadyConsumed => LiveWsTokenAdmissionRejection::TokenAlreadyConsumed,
        Dsl::ChannelNotBound => LiveWsTokenAdmissionRejection::ChannelNotBound,
    }
}

fn live_ws_token_public_error_class_from_machine(
    public_error_class: meerkat_runtime::meerkat_machine::dsl::LiveWebsocketTokenAdmissionPublicErrorClass,
) -> LiveWsTokenAdmissionPublicErrorClass {
    use meerkat_runtime::meerkat_machine::dsl::LiveWebsocketTokenAdmissionPublicErrorClass as Dsl;
    match public_error_class {
        Dsl::InvalidToken => LiveWsTokenAdmissionPublicErrorClass::InvalidToken,
    }
}

#[async_trait]
impl<B: SessionAgentBuilder + 'static> LiveProjectionSink for GatewayLiveProjectionSink<B> {
    async fn append_user_transcript(
        &self,
        session_id: &SessionId,
        text: &str,
        identity: LiveTranscriptIdentity<'_>,
    ) -> Result<(), LiveProjectionError> {
        // P2#1: with a stable `provider_item_id`, route through the typed
        // realtime transcript seam so its idempotent ordering / dedup owns
        // the canonical commit (duplicate provider finals collapse by
        // item_id + content_index instead of producing duplicate user turns).
        if let Some(item_id) = identity.provider_item_id {
            let event = RealtimeTranscriptEvent::UserTranscriptFinal {
                item_id: item_id.to_string(),
                previous_item_id: identity.previous_item_id.map(ToString::to_string),
                content_index: identity.content_index.unwrap_or(0),
                text: text.to_string(),
            };
            return self
                .service
                .append_realtime_transcript_event(session_id, event)
                .await
                .map(|_outcome| ())
                .map_err(|err| session_error_to_projection(err, session_id));
        }

        // Legacy fallback: providers without a stable item id cannot be
        // deduplicated by the realtime layer, so commit directly into
        // canonical history.
        self.service
            .append_external_user_content(session_id, ContentInput::Text(text.to_string()))
            .await
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn append_assistant_text_delta(
        &self,
        session_id: &SessionId,
        delta: &str,
        identity: LiveTranscriptIdentity<'_>,
    ) -> Result<(), LiveProjectionError> {
        // A3/A11/T6: display-text delta lane, staged through the typed
        // realtime seam with the full identity tuple.
        let event = build_assistant_text_delta_event(delta, identity)
            .map_err(identity_error_to_projection)?;
        self.service
            .append_realtime_transcript_event(session_id, event)
            .await
            .map(|_outcome| ())
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn append_assistant_transcript_delta(
        &self,
        session_id: &SessionId,
        delta: &str,
        identity: LiveTranscriptIdentity<'_>,
    ) -> Result<(), LiveProjectionError> {
        // T6/T9/T10: spoken-transcript delta lane; the staging site tags the
        // owning item `TranscriptLane::Spoken` so the materializer flushes
        // `AssistantBlock::Transcript { source: Spoken }`.
        let event = build_assistant_transcript_delta_event(delta, identity)
            .map_err(identity_error_to_projection)?;
        self.service
            .append_realtime_transcript_event(session_id, event)
            .await
            .map(|_outcome| ())
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn append_assistant_text_final(
        &self,
        session_id: &SessionId,
        text: &str,
        _identity: LiveTranscriptIdentity<'_>,
        _stop_reason: StopReason,
        _usage: Usage,
        response_id: Option<&str>,
    ) -> Result<(), LiveProjectionError> {
        // P1#1 + T6: buffer display-text finals on the per-(session,
        // response_id) slot; `signal_turn_completed` drains and flushes with
        // the authoritative stop_reason/usage (the values stamped on this
        // event are provider best-effort sentinels). Display text is
        // preserved across barge-in (T7).
        self.pending_turns.buffer(
            session_id,
            response_id,
            PendingAssistantContent::Text(text.to_string()),
        );
        Ok(())
    }

    async fn append_assistant_transcript_final(
        &self,
        session_id: &SessionId,
        text: &str,
        identity: LiveTranscriptIdentity<'_>,
        _stop_reason: StopReason,
        _usage: Usage,
        response_id: Option<&str>,
    ) -> Result<(), LiveProjectionError> {
        // R5-7: forward the authoritative final transcript text into the
        // realtime-staging pipeline. The materializer respects barge-in
        // discards, creates the staged item for final-only providers,
        // promotes the lane to Spoken, and REPLACES the staged segment with
        // this authoritative text (repairing dropped deltas).
        // `stop_reason`/`usage` are ignored here — the canonical values
        // arrive atomically with `TurnCompleted`. The identity-carried
        // `response_id` takes precedence over the arg form; empty ids are
        // rejected downstream by the materializer as a typed Rejected.
        let response_id = identity
            .response_id
            .map(ToString::to_string)
            .or_else(|| response_id.map(ToString::to_string))
            .unwrap_or_default();
        let event = RealtimeTranscriptEvent::AssistantTranscriptFinalText {
            response_id,
            item_id: identity
                .provider_item_id
                .map(ToString::to_string)
                .unwrap_or_default(),
            content_index: identity.content_index.unwrap_or(0),
            text: text.to_string(),
        };
        self.service
            .append_realtime_transcript_event(session_id, event)
            .await
            .map(|_outcome| ())
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn truncate_assistant_transcript(
        &self,
        session_id: &SessionId,
        provider_item_id: Option<&str>,
        _previous_item_id: Option<&str>,
        content_index: Option<u32>,
        response_id: Option<&str>,
        text: Option<&str>,
    ) -> Result<(), LiveProjectionError> {
        // P1#3: never fabricate empty identity — the realtime layer rejects
        // empty response_ids, which made the pre-fix projection inert.
        // Surface a typed Rejected when the provider genuinely omitted the
        // id (a provider-fact gap, not a silently-dropped event).
        let Some(response_id) = response_id else {
            return Err(LiveProjectionError::Rejected(
                "AssistantTranscriptTruncated missing response_id from adapter".to_string(),
            ));
        };
        let Some(item_id) = provider_item_id else {
            return Err(LiveProjectionError::Rejected(
                "AssistantTranscriptTruncated missing provider_item_id from adapter".to_string(),
            ));
        };
        let event = RealtimeTranscriptEvent::AssistantTranscriptTruncated {
            response_id: response_id.to_string(),
            item_id: item_id.to_string(),
            content_index: content_index.unwrap_or(0),
            text: text.unwrap_or_default().to_string(),
        };
        self.service
            .append_realtime_transcript_event(session_id, event)
            .await
            .map(|_outcome| ())
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn signal_turn_interrupt(
        &self,
        session_id: &SessionId,
        response_id: Option<&str>,
    ) -> Result<(), LiveProjectionError> {
        // A6/CC4: barge-in coordinates ACROSS the two layers holding
        // in-flight transcript state — (1) the realtime-staging pipeline,
        // where `AssistantTurnInterrupted` must be synthesized for every
        // in-flight response so staged deltas do not survive into the next
        // turn's materializer sweep, and (2) the buffered display-text path,
        // which is preserved (the user is not "speaking over" written
        // output). G4: the observation-carried `response_id` is
        // authoritative even before any delta staged; the fallback discovers
        // every staged in-flight response id, and the two sources union.
        let mut response_ids: Vec<String> = Vec::new();
        if let Some(rid) = response_id.filter(|s| !s.is_empty()) {
            response_ids.push(rid.to_string());
        }
        match self.service.load_authoritative_session(session_id).await {
            Ok(Some(session)) => {
                for id in session.in_flight_realtime_assistant_response_ids() {
                    if !response_ids.contains(&id) {
                        response_ids.push(id);
                    }
                }
            }
            Ok(None) => {}
            Err(
                meerkat_core::SessionError::NotFound { .. }
                | meerkat_core::SessionError::Unsupported(_),
            ) => {}
            Err(err) => return Err(session_error_to_projection(err, session_id)),
        }
        for rid in response_ids {
            let event = RealtimeTranscriptEvent::AssistantTurnInterrupted { response_id: rid };
            match self
                .service
                .append_realtime_transcript_event(session_id, event)
                .await
            {
                Ok(_) => {}
                Err(
                    meerkat_core::SessionError::NotFound { .. }
                    | meerkat_core::SessionError::Unsupported(_),
                ) => {}
                Err(err) => return Err(session_error_to_projection(err, session_id)),
            }
        }

        // Project through the same machine-authority interrupt path the
        // user-facing `mobkit/live/interrupt` RPC uses. Tolerate
        // `NotRunning` (typical when no turn is in flight) so a late
        // provider-side interrupt does not poison the channel.
        match self
            .service
            .interrupt_with_machine_authority(session_id, self.machine.session_control_authority())
            .await
        {
            Ok(()) => Ok(()),
            Err(meerkat_core::SessionError::NotRunning { .. }) => Ok(()),
            Err(err) => Err(session_error_to_projection(err, session_id)),
        }
    }

    async fn signal_turn_completed(
        &self,
        session_id: &SessionId,
        stop_reason: StopReason,
        usage: Usage,
        response_id: Option<&str>,
    ) -> Result<(), LiveProjectionError> {
        // CC2: synthesize `AssistantTurnCompleted` BEFORE draining the
        // buffered display-text path — the staging materializer commits any
        // staged spoken-transcript items for `response_id` here (the
        // provider emits `TurnCompleted` directly, never a staged
        // `AssistantTurnCompleted`, so without this synthesis staged deltas
        // leak forever). Orphan completions (no `response_id`) skip the
        // synthesis (staging rejects empty ids) but still flush the buffer.
        let mut realtime_materialized = false;
        if let Some(rid) = response_id.filter(|s| !s.is_empty()) {
            let event = RealtimeTranscriptEvent::AssistantTurnCompleted {
                response_id: rid.to_string(),
                stop_reason,
                usage: usage.clone(),
            };
            match self
                .service
                .append_realtime_transcript_event(session_id, event)
                .await
            {
                Ok(outcome) => {
                    // If the materializer fired it has already recorded the
                    // authoritative usage for this turn; the drain below must
                    // then forward `Usage::default()` to stay single-counted.
                    realtime_materialized = !outcome.is_inert();
                }
                Err(
                    meerkat_core::SessionError::NotFound { .. }
                    | meerkat_core::SessionError::Unsupported(_),
                ) => {}
                Err(err) => return Err(session_error_to_projection(err, session_id)),
            }
        }

        // R6: drain only the matching (session, response_id) display-text
        // slot. If realtime materialized and nothing was buffered, skip the
        // empty append entirely — no synthetic empty assistant block and no
        // double-recorded usage.
        let pending = self.pending_turns.drain(session_id, response_id);
        let blocks = collapse_pending_blocks(pending.blocks);
        if realtime_materialized && blocks.is_empty() {
            return Ok(());
        }
        let usage_for_drain = if realtime_materialized {
            Usage::default()
        } else {
            usage
        };
        self.service
            .append_external_assistant_output(session_id, blocks, stop_reason, usage_for_drain)
            .await
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn signal_terminal_error(
        &self,
        session_id: &SessionId,
        code: LiveAdapterErrorCode,
        message: &str,
    ) -> Result<(), LiveProjectionError> {
        // R6: drop ALL buffered finals across every response_id slot —
        // terminal error invalidates every in-flight response.
        self.pending_turns.drain_all(session_id);
        tracing::warn!(
            target: "meerkat_mobkit::live_wiring",
            session_id = %session_id,
            ?code,
            message,
            "live adapter terminal error",
        );
        // Route the typed terminal cause onto the session's owned event
        // stream via the canonical `SessionService::record_live_terminal_error`
        // seam — observable to session subscribers, not only tracing.
        self.service
            .record_live_terminal_error(session_id, code)
            .await
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn signal_output_audio_degraded(
        &self,
        session_id: &SessionId,
        dropped: u64,
    ) -> Result<(), LiveProjectionError> {
        // K16: transport delivery degradation is a typed session-observable
        // fact, never a transport-local counter or a tracing-only warning.
        self.service
            .record_live_output_audio_degraded(session_id, dropped)
            .await
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn append_realtime_transcript(
        &self,
        session_id: &SessionId,
        event: &RealtimeTranscriptEvent,
    ) -> Result<meerkat_core::RealtimeTranscriptApplyOutcome, LiveProjectionError> {
        // P1#2: forward provider-emitted realtime transcript events into the
        // same idempotent ordering / staging path used by deltas +
        // truncation; without this override `ItemObserved` /
        // `AssistantTurnCompleted` etc. would be silently dropped. 0.7.27:
        // the apply outcome flows back so the host synthesizes the redacted
        // image receipt only AFTER durable reducer application.
        self.service
            .append_realtime_transcript_event(session_id, event.clone())
            .await
            .map_err(|err| session_error_to_projection(err, session_id))
    }
}

/// Routes live tool calls into the member session's NORMAL external-tool
/// dispatch (callback bridge, composed recorder tools, gating — unchanged).
/// Port of meerkat-rpc's `RuntimeLiveToolDispatcher`.
pub struct GatewayLiveToolDispatcher<B: SessionAgentBuilder + 'static> {
    service: Arc<PersistentSessionService<B>>,
}

impl<B: SessionAgentBuilder + 'static> GatewayLiveToolDispatcher<B> {
    pub fn new(service: Arc<PersistentSessionService<B>>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl<B: SessionAgentBuilder + 'static> LiveToolDispatcher for GatewayLiveToolDispatcher<B> {
    async fn dispatch_live_tool_call(
        &self,
        session_id: &SessionId,
        call: meerkat_core::ToolCall,
    ) -> Result<meerkat_core::ops::ToolDispatchOutcome, meerkat_live::LiveToolDispatchError> {
        self.service
            .dispatch_external_tool_call(session_id, call)
            .await
            .map_err(|err| meerkat_live::LiveToolDispatchError::from_session_error(session_id, err))
    }
}

/// [`RealtimeCurrentConfigSource`] serving the gateway's effective `Config`.
///
/// mobkit gateways have no durable config store; per-open realtime
/// credential resolution rides the session identity's auth binding or the
/// provider default (env `OPENAI_API_KEY`) against this fixed config —
/// matching text-model behaviour. Embedders with real config stores can swap
/// in `StoreBackedRealtimeConfigSource` later.
pub struct EnvRealtimeConfigSource {
    config: Config,
}

impl EnvRealtimeConfigSource {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

#[async_trait]
impl RealtimeCurrentConfigSource for EnvRealtimeConfigSource {
    async fn current_config(&self) -> Result<Config, ConfigError> {
        Ok(self.config.clone())
    }
}

/// Everything a gateway needs to serve live channels: the adapter host, the
/// WS transport state (mount via `meerkat_live::live_ws_router(ws_state)` on
/// the gateway's HTTP app), the per-open realtime session factory, and the
/// externally-reachable base URL the WS bootstrap embeds.
#[derive(Clone)]
pub struct GatewayLiveContext {
    pub host: Arc<LiveAdapterHost>,
    pub ws_state: Arc<LiveWsState>,
    pub session_factory: Arc<dyn RealtimeSessionFactory>,
    pub ws_base_url: String,
    /// Gateway-wide seed-projection clamp (`runtime_options.live.seed_max_chars`),
    /// overridable per open. `None` = no clamp. Stopgap for upstream ask 30
    /// (docs/design/upstream-asks.md): the provider caps live instructions at
    /// 65,536 tokens and long member transcripts overflow the projected seed.
    pub seed_max_chars: Option<usize>,
}

/// Compose the live stack for a persistent-mode gateway.
///
/// One `GatewayLiveProjectionSink` instance serves all four injected trait
/// seams (projection sink, close feedback, status feedback, WS token
/// authority), exactly like the upstream wiring. The tool dispatcher rides
/// the host builder with the upstream default tool timeout
/// (`meerkat_live::DEFAULT_LIVE_TOOL_TIMEOUT`). Credentials resolve PER OPEN
/// via the facade's `PerOpenCredentialRealtimeSessionFactory` over
/// [`EnvRealtimeConfigSource`].
pub fn attach_live<B: SessionAgentBuilder + 'static>(
    service: Arc<PersistentSessionService<B>>,
    machine: Arc<MeerkatMachine>,
    factory: &AgentFactory,
    config: Config,
    ws_base_url: String,
    seed_max_chars: Option<usize>,
) -> GatewayLiveContext {
    let sink = Arc::new(GatewayLiveProjectionSink::new(
        Arc::clone(&service),
        machine,
    ));
    let dispatcher: Arc<dyn LiveToolDispatcher> = Arc::new(GatewayLiveToolDispatcher::new(service));
    let host = Arc::new(
        LiveAdapterHost::new(Arc::clone(&sink) as Arc<dyn LiveProjectionSink>)
            .with_live_tool_dispatcher(dispatcher),
    );
    let ws_state = Arc::new(LiveWsState::new(
        Arc::clone(&host),
        Arc::clone(&sink) as Arc<dyn LiveChannelCloseFeedback>,
        Arc::clone(&sink) as Arc<dyn LiveChannelStatusFeedback>,
        sink as Arc<dyn LiveWsTokenAuthority>,
    ));
    let session_factory = factory
        .build_openai_realtime_session_factory(Arc::new(EnvRealtimeConfigSource::new(config)));
    GatewayLiveContext {
        host,
        ws_state,
        session_factory,
        ws_base_url,
        seed_max_chars,
    }
}

// ---------------------------------------------------------------------------
// mobkit/live/* handlers — port of meerkat-rpc/src/handlers/live.rs
// (websocket arms; webrtc deliberately not ported, per design §"What we
// deliberately do NOT do in v1").
// ---------------------------------------------------------------------------

/// `instructions` on `mobkit/live/open` accepts a single string or an array
/// of strings — both spell the same per-open overlay.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LiveOpenInstructions {
    One(String),
    Many(Vec<String>),
}

impl LiveOpenInstructions {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(text) => vec![text],
            Self::Many(items) => items,
        }
    }
}

/// The gateway-side params for `mobkit/live/open`. The member target
/// (`identity` / `member_id` / `session_id`) is resolved by the CALLER
/// before `handle_live_method`; unknown fields are ignored here so the
/// target spellings pass through untouched.
#[derive(Debug, Deserialize)]
struct GatewayLiveOpenParams {
    #[serde(default)]
    turning_mode: Option<RealtimeTurningMode>,
    #[serde(default)]
    transport: Option<LiveOpenTransport>,
    /// v1 realtime-model override (design §6): members whose text model is
    /// not realtime-capable open the channel against this model instead.
    #[serde(default)]
    model: Option<String>,
    /// Per-open ephemeral instruction overlay. Rides the runtime
    /// system-context lane of the open projection, so it reaches the
    /// provider's instructions channel without ever touching the member's
    /// durable transcript or prompt truth. Dropped on `live/refresh` (the
    /// refresh path re-projects from the durable session) and on reopen.
    #[serde(default)]
    instructions: Option<LiveOpenInstructions>,
    /// Per-open override of the gateway-wide seed clamp
    /// (`runtime_options.live.seed_max_chars`).
    #[serde(default)]
    seed_max_chars: Option<usize>,
}

fn live_success(rpc_id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: rpc_id,
        result: Some(result),
        error: None,
    }
}

fn live_error(rpc_id: Value, code: i64, message: impl Into<String>) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: rpc_id,
        result: None,
        error: Some(JsonRpcError::new(code, message)),
    }
}

/// The CALLER answers this when the gateway has no [`GatewayLiveContext`]
/// (ephemeral mode, or `runtime_options.live` off). Kept here so every
/// surface emits the identical `-32050` / `data.kind = "live_unavailable"`
/// shape.
/// Type-erased live RPC entry point. The gateway — which knows the concrete
/// session-builder type `B` — captures its `GatewayLiveContext`, service, and
/// machine into this closure so the shared (non-generic) RPC dispatch in
/// `crate::rpc` can serve `mobkit/live/*` without a type parameter.
pub type LiveRpcHandler = Arc<
    dyn Fn(
            Option<meerkat_core::types::SessionId>,
            String,
            Value,
            Value,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = JsonRpcResponse> + Send>>
        + Send
        + Sync,
>;

/// Erase `handle_live_method` over the gateway's concrete types.
pub fn live_rpc_handler<B: SessionAgentBuilder + 'static>(
    ctx: Arc<GatewayLiveContext>,
    service: Arc<PersistentSessionService<B>>,
    machine: Arc<MeerkatMachine>,
) -> LiveRpcHandler {
    Arc::new(move |resolved_session, method, params, rpc_id| {
        let ctx = Arc::clone(&ctx);
        let service = Arc::clone(&service);
        let machine = Arc::clone(&machine);
        Box::pin(async move {
            handle_live_method(
                &ctx,
                &service,
                &machine,
                resolved_session,
                &method,
                &params,
                rpc_id,
            )
            .await
        })
    })
}

#[must_use]
pub fn live_unavailable_response(rpc_id: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: rpc_id,
        result: None,
        error: Some(
            JsonRpcError::new(
                LIVE_UNAVAILABLE_CODE,
                "live sessions are not available on this gateway",
            )
            .with_data(serde_json::json!({ "kind": LIVE_UNAVAILABLE_KIND })),
        ),
    }
}

fn parse_live_params<T: DeserializeOwned>(
    params: &Value,
    rpc_id: &Value,
) -> Result<T, Box<JsonRpcResponse>> {
    serde_json::from_value(params.clone()).map_err(|err| {
        Box::new(live_error(
            rpc_id.clone(),
            INVALID_PARAMS_CODE,
            format!("invalid params: {err}"),
        ))
    })
}

/// Dispatch a `mobkit/live/*` method against a live-enabled gateway.
///
/// `resolved_session` is the member target already canonicalized by the
/// caller (identity → member alias → raw session id — the same
/// canonicalization class as `/agents/{id}/events`); only `mobkit/live/open`
/// consumes it, every other method addresses a `channel_id`. `service` and
/// `machine` are passed alongside the (deliberately non-generic)
/// [`GatewayLiveContext`] because open/refresh must re-project the member
/// session and every handler resolves results through machine authority.
pub async fn handle_live_method<B: SessionAgentBuilder + 'static>(
    ctx: &GatewayLiveContext,
    service: &Arc<PersistentSessionService<B>>,
    machine: &Arc<MeerkatMachine>,
    resolved_session: Option<SessionId>,
    method: &str,
    params: &Value,
    rpc_id: Value,
) -> JsonRpcResponse {
    match method {
        "mobkit/live/open" => {
            let Some(session_id) = resolved_session else {
                return live_error(
                    rpc_id,
                    INVALID_PARAMS_CODE,
                    "live/open requires a resolvable member target \
                     (identity, member_id, or session_id)",
                );
            };
            handle_live_open(ctx, service, machine, &session_id, params, rpc_id).await
        }
        "mobkit/live/status" => handle_live_status(ctx, machine, params, rpc_id).await,
        "mobkit/live/close" => handle_live_close(ctx, machine, params, rpc_id).await,
        "mobkit/live/refresh" => handle_live_refresh(ctx, service, machine, params, rpc_id).await,
        "mobkit/live/send_input" => handle_live_send_input(ctx, machine, params, rpc_id).await,
        "mobkit/live/commit_input" => handle_live_commit_input(ctx, machine, params, rpc_id).await,
        "mobkit/live/interrupt" => handle_live_interrupt(ctx, machine, params, rpc_id).await,
        "mobkit/live/truncate" => handle_live_truncate(ctx, machine, params, rpc_id).await,
        other => live_error(
            rpc_id,
            METHOD_NOT_FOUND_CODE,
            format!("unknown live method {other}"),
        ),
    }
}

/// Project the member session into a [`RealtimeSessionOpenConfig`].
///
/// mobkit mirror of the facade `LiveOrchestrator::live_open_config_for_session`
/// (which is pinned to `FactoryAgentBuilder` + the staged-session registry,
/// so a generic-`B` gateway cannot borrow it): the same three published
/// service seams feed the same three pub projection free functions. No
/// staged/archived recovery — mobkit member sessions are held live by the
/// bridge, and an archived target surfaces as `NotFound`.
async fn live_open_config_for_session<B: SessionAgentBuilder + 'static>(
    service: &PersistentSessionService<B>,
    session_id: &SessionId,
    turning_mode: RealtimeTurningMode,
    seed_max_chars: Option<usize>,
) -> Result<RealtimeSessionOpenConfig, meerkat_core::SessionError> {
    let (session, canonical_user_image_decoded_bytes) = service
        .export_realtime_open_session_snapshot_with_image_usage(session_id)
        .await?;
    let llm_identity = service.live_session_llm_identity(session_id).await?;
    let visible_tools = service.live_visible_tool_defs(session_id).await?;
    // 0.7.27: exact-retry + live-rewrite guards ride the open config — the
    // user-content identity lane and the transcript rewrite generation come
    // from the exported snapshot, mirroring the facade orchestrator.
    let transcript_rewrite_generation = session.transcript_rewrite_generation().map_err(|err| {
        meerkat_core::SessionError::Agent(meerkat_core::error::AgentError::InternalError(
            err.to_string(),
        ))
    })?;
    // 0.7.28: seed bounding is upstream-owned (`live/open.seed_max_chars`,
    // upstream ask 30 SHIPPED) — the windowed projection preserves the
    // enabled root context, an affordable compaction summary, and the
    // identity/tombstone/rewrite-generation/canonical-image sidecars, and
    // reports degraded continuity explicitly. This replaced mobkit's
    // oldest-first clamp stopgap.
    let seed_projection = match seed_max_chars {
        Some(max_chars) => {
            let window =
                meerkat::session_runtime::live_orchestration::LiveSeedWindow::new(max_chars)
                    .map_err(|err| {
                        meerkat_core::SessionError::Agent(
                            meerkat_core::error::AgentError::InternalError(err.to_string()),
                        )
                    })?;
            let projection =
                meerkat::session_runtime::live_orchestration::realtime_projection_messages_with_window(
                    &session, window,
                )
                .map_err(|err| {
                    meerkat_core::SessionError::Agent(
                        meerkat_core::error::AgentError::InternalError(err.to_string()),
                    )
                })?;
            if !matches!(
                projection.status,
                meerkat::session_runtime::live_orchestration::LiveSeedProjectionStatus::Complete
            ) {
                tracing::info!(
                    target: "meerkat_mobkit::live_wiring",
                    %session_id,
                    max_chars,
                    status = ?projection.status,
                    "live open seed windowed (upstream seed_max_chars)"
                );
            }
            projection.messages
        }
        None => realtime_projection_messages(&session)?,
    };
    Ok(
        RealtimeSessionOpenConfig::new(turning_mode, llm_identity, visible_tools, seed_projection)
            .with_user_content_identities(session.realtime_user_content_identities())
            .with_user_content_tombstones(session.realtime_user_content_tombstones())
            .with_canonical_user_image_decoded_bytes(canonical_user_image_decoded_bytes)
            .with_transcript_rewrite_generation(transcript_rewrite_generation)
            .with_runtime_system_context(realtime_projection_runtime_system_context(&session)?)
            .with_system_prompt(match realtime_projection_root_system_message(&session)? {
                Some(Message::System(system)) => Some(system.content),
                // The projector only ever yields `Message::System` (or `None`);
                // any other shape means no root system prompt to project.
                _ => None,
            }),
    )
}

/// Provenance marker on the runtime system-context append carrying the
/// per-open instruction overlay from `mobkit/live/open` `instructions`.
const LIVE_OPEN_INSTRUCTIONS_SOURCE: &str = "mobkit/live/open#instructions";

/// Fold the per-open instruction overlay into the open config's runtime
/// system-context lane.
///
/// Lane choice: `runtime_system_context` (NOT `with_system_prompt`). The
/// typed `system_prompt` field is the projection of the member's durable
/// prompt truth (R10: single owner of live prompt truth), so baking a
/// per-open overlay into it would misreport the member's prompt to the
/// provider and to every snapshot consumer. The runtime system-context lane
/// is exactly the typed carrier for runtime-authored ephemeral context:
/// adapters fold it into the provider session as authoritative instructions,
/// and because this append exists only on this open's projection it never
/// persists into the durable transcript.
fn apply_live_open_instruction_overlay(
    open_config: &mut RealtimeSessionOpenConfig,
    instructions: Vec<String>,
) {
    let joined = instructions
        .iter()
        .map(|instruction| instruction.trim())
        .filter(|instruction| !instruction.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if joined.is_empty() {
        return;
    }
    open_config
        .runtime_system_context
        .push(PendingSystemContextAppend {
            content: CoreRenderable::text(joined),
            source: Some(LIVE_OPEN_INSTRUCTIONS_SOURCE.to_string()),
            idempotency_key: None,
            source_kind: SystemContextSource::RuntimeSteer,
            peer_response_terminal: None,
            accepted_at: std::time::SystemTime::now(),
        });
}

/// #176: project the factory's typed realtime audio policy into the typed
/// [`LiveAudioConfig`] the snapshot carries. `None` when the factory does
/// not advertise both directions of audio, so the caller fails closed
/// rather than inventing a sample rate.
fn live_audio_config_from_capabilities(
    capabilities: &RealtimeCapabilities,
) -> Option<LiveAudioConfig> {
    let input = capabilities.audio_input_format.as_ref()?;
    let output = capabilities.audio_output_format.as_ref()?;
    Some(LiveAudioConfig {
        input_sample_rate_hz: input.sample_rate_hz,
        input_channels: u16::from(input.channels),
        output_sample_rate_hz: output.sample_rate_hz,
        output_channels: u16::from(output.channels),
    })
}

/// #176: derive the WS `&format=` query token from the typed audio policy.
/// The WS transport stamps inbound binary-chunk sample rates from this
/// token, and the only layout it accepts today is 16-bit signed LE PCM,
/// 24 kHz, mono. Fail closed (`None`) when the resolved policy does not map
/// — a mismatch would silently stamp the wrong rate onto every frame.
fn live_ws_audio_format_param(audio: &LiveAudioConfig) -> Option<&'static str> {
    const PCM_24K_MONO_RATE_HZ: u32 = 24_000;
    const PCM_24K_MONO_CHANNELS: u16 = 1;
    if audio.input_sample_rate_hz == PCM_24K_MONO_RATE_HZ
        && audio.input_channels == PCM_24K_MONO_CHANNELS
    {
        Some("pcm_24k_mono")
    } else {
        None
    }
}

/// A8: build a `LiveProjectionSnapshot` from the resolved open config.
/// `snapshot_version = 0` is the open-time placeholder; the refresh path
/// overwrites it via `host.next_snapshot_version(channel_id)` (R8).
fn build_live_projection_snapshot(
    session_id: &SessionId,
    open_config: &RealtimeSessionOpenConfig,
    audio_config: Option<LiveAudioConfig>,
) -> LiveProjectionSnapshot {
    LiveProjectionSnapshot {
        session_id: session_id.clone(),
        snapshot_version: 0,
        seed_messages: open_config.seed_messages.clone(),
        visible_tools: open_config.visible_tools.clone(),
        user_content_identities: open_config.user_content_identities.clone(),
        user_content_tombstones: open_config.user_content_tombstones.clone(),
        transcript_rewrite_generation: open_config.transcript_rewrite_generation,
        canonical_user_image_decoded_bytes: open_config.canonical_user_image_decoded_bytes,
        // R10: the typed `system_prompt` field is the single owner of live
        // prompt truth — never re-derived from `seed_messages[0]` (the
        // history projector drops `Message::System`, so seed inference
        // silently wipes the prompt on the refresh path).
        system_prompt: open_config.system_prompt.clone(),
        model_id: open_config.llm_identity.model.clone(),
        provider_id: open_config.llm_identity.provider,
        audio_config,
        // R3: typed runtime system-context rides the snapshot so adapters
        // fold it into the provider session as authoritative instructions.
        runtime_system_context: open_config.runtime_system_context.clone(),
    }
}

/// A8: `Fresh` on empty seed history, `TranscriptOnly` once seeded.
/// Provider-native resume is not wired by any shipped provider.
fn continuity_from_snapshot(snapshot: &LiveProjectionSnapshot) -> LiveContinuityMode {
    if snapshot.seed_messages.is_empty() {
        LiveContinuityMode::Fresh
    } else {
        LiveContinuityMode::TranscriptOnly
    }
}

async fn abandon_live_open_admission(
    machine: &Arc<MeerkatMachine>,
    session_id: &SessionId,
    channel_id: &LiveChannelId,
) {
    if let Err(err) = machine
        .abandon_live_open_admission(session_id, channel_id)
        .await
    {
        tracing::warn!(
            target: "meerkat_mobkit::live_wiring",
            ?channel_id,
            ?session_id,
            ?err,
            "generated live-open admission abandonment failed"
        );
    }
}

/// #355: open-failure cleanup is fail-closed, not best-effort. Attempt the
/// generated graceful close first (retains a queryable closed-channel
/// status); if the close authority rejects, omits the host handoff, or the
/// host commit fails, fall through to `abandon_live_open_admission` so a
/// failed open never leaves an orphaned machine-owned channel binding.
async fn close_live_channel_after_open_failure(
    host: &LiveAdapterHost,
    machine: &Arc<MeerkatMachine>,
    session_id: &SessionId,
    channel_id: &LiveChannelId,
) {
    match host.reserve_channel_close_observation(channel_id).await {
        Ok(observation) => {
            let committed = commit_live_close_for_open_failure(
                host,
                machine,
                session_id,
                channel_id,
                &observation,
            )
            .await;
            if !committed {
                abandon_live_open_admission(machine, session_id, channel_id).await;
            }
        }
        Err(LiveAdapterHostError::ChannelNotFound(_)) => {
            abandon_live_open_admission(machine, session_id, channel_id).await;
        }
        Err(err) => {
            tracing::warn!(
                target: "meerkat_mobkit::live_wiring",
                ?channel_id,
                ?session_id,
                ?err,
                "failed to close live channel after open failure; evicting admission"
            );
            abandon_live_open_admission(machine, session_id, channel_id).await;
        }
    }
}

/// Returns `true` only when the host commit succeeded (the machine-owned
/// channel binding is cleared); `false` after logging the typed cause.
async fn commit_live_close_for_open_failure(
    host: &LiveAdapterHost,
    machine: &Arc<MeerkatMachine>,
    session_id: &SessionId,
    channel_id: &LiveChannelId,
    observation: &LiveChannelCloseObservation,
) -> bool {
    let authority = match machine
        .resolve_live_close_result(session_id, observation)
        .await
    {
        Ok(authority) => authority,
        Err(err) => {
            tracing::warn!(
                target: "meerkat_mobkit::live_wiring",
                ?channel_id,
                ?session_id,
                ?err,
                "generated live-close authority rejected open-failure cleanup; evicting admission"
            );
            return false;
        }
    };
    let Some(close_commit_authority) = authority.channel_close_commit_authority() else {
        tracing::warn!(
            target: "meerkat_mobkit::live_wiring",
            ?channel_id,
            ?session_id,
            "generated live-close result omitted host commit authority; evicting admission"
        );
        return false;
    };
    if let Err(err) = host
        .commit_channel_close_observation(observation, close_commit_authority)
        .await
    {
        tracing::warn!(
            target: "meerkat_mobkit::live_wiring",
            ?channel_id,
            ?session_id,
            ?err,
            "host live-close commit failed after generated open-failure cleanup; evicting admission"
        );
        return false;
    }
    true
}

#[allow(clippy::too_many_lines)]
async fn handle_live_open<B: SessionAgentBuilder + 'static>(
    ctx: &GatewayLiveContext,
    service: &Arc<PersistentSessionService<B>>,
    machine: &Arc<MeerkatMachine>,
    session_id: &SessionId,
    params: &Value,
    rpc_id: Value,
) -> JsonRpcResponse {
    let parsed: GatewayLiveOpenParams = match parse_live_params(params, &rpc_id) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    // The mobkit gateway mounts the WS transport only; a webrtc request is
    // a caller error, not a silent websocket fallback.
    match parsed.transport {
        None | Some(LiveOpenTransport::Websocket) => {}
        Some(_) => {
            return live_error(
                rpc_id,
                INVALID_PARAMS_CODE,
                "only the websocket live transport is supported by this gateway",
            );
        }
    }

    // R3-1: honor the caller's optional `turning_mode`; default
    // `ProviderManaged`. Text-only callers that drive `commit_input` must
    // pass `ExplicitCommit` (the OpenAI realtime API rejects
    // `input_audio_buffer.commit` outside explicit-commit sessions).
    let turning_mode = parsed
        .turning_mode
        .unwrap_or(RealtimeTurningMode::ProviderManaged);
    // The per-open seed_max_chars override beats the gateway-wide
    // `runtime_options.live.seed_max_chars`; windowing is upstream-owned
    // (0.7.28, ask 30 SHIPPED).
    let seed_max_chars = parsed.seed_max_chars.or(ctx.seed_max_chars);
    let mut open_config =
        match live_open_config_for_session(service, session_id, turning_mode, seed_max_chars).await
        {
            Ok(config) => config,
            Err(meerkat_core::SessionError::NotFound { .. }) => {
                return live_error(
                    rpc_id,
                    INVALID_PARAMS_CODE,
                    format!("session {session_id} not found"),
                );
            }
            Err(err) => {
                return live_error(
                    rpc_id,
                    INTERNAL_ERROR_CODE,
                    format!("failed to build session config: {err}"),
                );
            }
        };
    // Design §6: the member session's model decides; an explicit `model`
    // override swaps the realtime model for this channel without touching
    // the member's text identity. Applied before precheck + admission so
    // both gate — and the machine binds — the identity actually opened.
    if let Some(model) = parsed.model {
        open_config.llm_identity.model = model;
    }
    // Per-open ephemeral instruction overlay — runtime system-context lane
    // (see apply_live_open_instruction_overlay for the lane rationale).
    if let Some(instructions) = parsed.instructions {
        apply_live_open_instruction_overlay(&mut open_config, instructions.into_vec());
    }

    // B19 precheck runs BEFORE any channel infra is minted (the reference
    // prechecks after `open_channel_with_authority` and then unwinds; with
    // the identity already resolved there is nothing to unwind here).
    if let Err(precheck_err) = precheck_identity(&open_config.llm_identity) {
        let (code, message) = match &precheck_err {
            LiveOpenPrecheckError::ModelNotRealtime { .. } => {
                (INVALID_PARAMS_CODE, precheck_err.to_string())
            }
            _ => (INTERNAL_ERROR_CODE, precheck_err.to_string()),
        };
        return live_error(rpc_id, code, message);
    }
    // B18: the factory that mints the adapter owns provider support.
    if !ctx
        .session_factory
        .supports_provider(open_config.llm_identity.provider)
    {
        return live_error(
            rpc_id,
            INTERNAL_ERROR_CODE,
            format!(
                "provider {} has no live adapter wired in this build",
                open_config.llm_identity.provider.as_str()
            ),
        );
    }

    // One machine-owned lifecycle boundary spans generated admission and all
    // provider/transport materialization. Retaining the lease prevents a
    // concurrent retire from proving the live registry empty before this open
    // installs its channel.
    let _live_lifecycle_lease = match machine.acquire_live_open_lifecycle_lease(session_id).await {
        Ok(lease) => lease,
        Err(err) => {
            return live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!("live open lifecycle authority unavailable: {err}"),
            );
        }
    };

    let live_open_identity = open_config.llm_identity.clone();
    let candidate_channel_id = LiveChannelId::random_uuid();
    let open_authority = match machine
        .resolve_live_open_admission(session_id, &candidate_channel_id, &live_open_identity)
        .await
    {
        Ok(authority) => authority,
        Err(err) => {
            return live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!("live open authority rejected admission: {err}"),
            );
        }
    };
    if !open_authority.admitted() {
        use meerkat_runtime::meerkat_machine::dsl::LiveOpenAdmissionRejection;
        return match open_authority.rejection() {
            Some(LiveOpenAdmissionRejection::AlreadyBound) => live_error(
                rpc_id,
                INVALID_PARAMS_CODE,
                format!("session {session_id} already has an active live channel"),
            ),
            Some(LiveOpenAdmissionRejection::ChannelAlreadyBound) => live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!("generated duplicate live channel id {candidate_channel_id}"),
            ),
            Some(LiveOpenAdmissionRejection::LifecycleClosed) => live_error(
                rpc_id,
                INVALID_PARAMS_CODE,
                "session lifecycle is closed to live channel admission",
            ),
            None => live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                "live open authority rejected admission without a reason",
            ),
        };
    }

    let Some(channel_open_authority) = open_authority.channel_open_authority() else {
        abandon_live_open_admission(machine, session_id, &candidate_channel_id).await;
        return live_error(
            rpc_id,
            INTERNAL_ERROR_CODE,
            "live open admission was accepted without a generated host handoff",
        );
    };

    let channel_id = match ctx
        .host
        .open_channel_with_authority(channel_open_authority)
        .await
    {
        Ok(ch) => ch,
        Err(LiveAdapterHostError::SessionAlreadyBound(sid)) => {
            abandon_live_open_admission(machine, session_id, &candidate_channel_id).await;
            return live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!(
                    "live host transport cache still has active channel for session {sid} \
                     after generated admission"
                ),
            );
        }
        Err(err) => {
            abandon_live_open_admission(machine, session_id, &candidate_channel_id).await;
            return live_error(rpc_id, INTERNAL_ERROR_CODE, err.to_string());
        }
    };

    // #176: resolve the typed audio policy from the factory's capabilities
    // (the provider/model audio-format owner) — the WS `&format=` token and
    // the snapshot `audio_config` both read this single typed value.
    let resolved_audio_config =
        live_audio_config_from_capabilities(&ctx.session_factory.capabilities());
    // E25: open the provider-native adapter directly. The factory already
    // consumed seed_messages + runtime_system_context, so no
    // `LiveAdapterCommand::Open` is dispatched afterwards (R2: re-seeding
    // would compound the provider transcript).
    let capabilities: LiveChannelCapabilities;
    let continuity: LiveContinuityMode;
    match ctx.session_factory.open_live_adapter(&open_config).await {
        Ok(adapter) => {
            // P2#3: the adapter's real capability set, queried before the
            // host takes canonical ownership of the Arc.
            capabilities = adapter.capabilities();
            if let Err(err) = ctx.host.attach_adapter(&channel_id, adapter).await {
                close_live_channel_after_open_failure(&ctx.host, machine, session_id, &channel_id)
                    .await;
                return live_error(
                    rpc_id,
                    INTERNAL_ERROR_CODE,
                    format!("failed to attach adapter: {err}"),
                );
            }
            let snapshot = build_live_projection_snapshot(
                session_id,
                &open_config,
                resolved_audio_config.clone(),
            );
            continuity = continuity_from_snapshot(&snapshot);
        }
        Err(err) => {
            close_live_channel_after_open_failure(&ctx.host, machine, session_id, &channel_id)
                .await;
            return live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!("failed to open provider session: {err}"),
            );
        }
    }

    let token = match ctx
        .ws_state
        .mint_token(session_id, channel_id.clone())
        .await
    {
        Ok(token) => token,
        Err(err) => {
            close_live_channel_after_open_failure(&ctx.host, machine, session_id, &channel_id)
                .await;
            return live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!("live WebSocket token authority rejected issue: {err}"),
            );
        }
    };
    let token_str = token.to_string();
    let Some(audio_config) = resolved_audio_config.as_ref() else {
        close_live_channel_after_open_failure(&ctx.host, machine, session_id, &channel_id).await;
        return live_error(
            rpc_id,
            INTERNAL_ERROR_CODE,
            "live websocket transport requires a resolved audio policy; no realtime \
             factory audio format was available",
        );
    };
    let Some(format_param) = live_ws_audio_format_param(audio_config) else {
        close_live_channel_after_open_failure(&ctx.host, machine, session_id, &channel_id).await;
        return live_error(
            rpc_id,
            INTERNAL_ERROR_CODE,
            format!(
                "resolved live audio policy (input {}Hz/{}ch) has no websocket binary \
                 format the transport can negotiate",
                audio_config.input_sample_rate_hz, audio_config.input_channels,
            ),
        );
    };
    // G38: the bearer token is pinned to the channel via the `channel`
    // query param so a leaked token cannot replay against another channel.
    let transport = LiveTransportBootstrap::Websocket {
        url: format!(
            "{base_url}{path}?token={token_str}&channel={channel_id}&format={format_param}",
            base_url = ctx.ws_base_url,
            path = meerkat_live::LIVE_WS_PATH,
        ),
        token: token_str,
    };

    // CC5/CC6/G8: project core typed shapes into the wire mirrors at the
    // boundary so SDK codegen sees typed unions, byte-compatible payloads.
    let result = LiveOpenResult {
        channel_id: channel_id.to_string(),
        transport: transport.into(),
        capabilities: capabilities.into(),
        continuity: continuity.into(),
    };
    match serde_json::to_value(result) {
        Ok(value) => live_success(rpc_id, value),
        Err(err) => live_error(
            rpc_id,
            INTERNAL_ERROR_CODE,
            format!("failed to serialize LiveOpenResult: {err}"),
        ),
    }
}

fn live_refresh_result_from_machine_authority(
    authority: &meerkat_runtime::meerkat_machine::LiveRefreshResultAuthority,
) -> LiveRefreshResult {
    // Exhaustive: a future generated status variant forces a compile error
    // here, so the wire projection can never silently misreport.
    match authority.status {
        meerkat_runtime::meerkat_machine::dsl::LiveRefreshPublicStatus::Queued => {
            LiveRefreshResult::queued()
        }
    }
}

fn live_close_result_from_machine_authority(
    authority: &meerkat_runtime::meerkat_machine::LiveCloseResultAuthority,
) -> LiveCloseResult {
    match authority.status {
        meerkat_runtime::meerkat_machine::dsl::LiveClosePublicStatus::Closed => {
            LiveCloseResult::closed()
        }
    }
}

/// #234: typed result shapes for command acceptances, projected only from
/// generated machine authority. Serde failures surface as `String` so the
/// caller maps them onto the RPC error channel.
fn live_command_result_from_machine_authority(
    authority: &meerkat_runtime::meerkat_machine::LiveCommandResultAuthority,
    expected: meerkat_runtime::meerkat_machine::dsl::LiveCommandPublicKind,
) -> Result<Value, String> {
    use meerkat_runtime::meerkat_machine::dsl::LiveCommandPublicKind;

    if authority.command != expected {
        return Err(format!(
            "LiveCommandResultResolved emitted command {:?} for expected {:?}",
            authority.command, expected
        ));
    }

    match expected {
        LiveCommandPublicKind::SendInput => serde_json::to_value(LiveSendInputResult::sent())
            .map_err(|err| format!("failed to serialize LiveSendInputResult: {err}")),
        LiveCommandPublicKind::CommitInput => {
            serde_json::to_value(LiveCommitInputResult::committed())
                .map_err(|err| format!("failed to serialize LiveCommitInputResult: {err}"))
        }
        LiveCommandPublicKind::Interrupt => {
            serde_json::to_value(LiveInterruptResult::interrupted())
                .map_err(|err| format!("failed to serialize LiveInterruptResult: {err}"))
        }
        LiveCommandPublicKind::TruncateAssistantOutput => {
            serde_json::to_value(LiveTruncateResult::truncated())
                .map_err(|err| format!("failed to serialize LiveTruncateResult: {err}"))
        }
    }
}

fn live_status_result_from_machine_authority(
    channel_id: String,
    authority: &meerkat_runtime::meerkat_machine::LiveChannelStatusAuthority,
) -> Result<LiveStatusResult, String> {
    Ok(LiveStatusResult {
        channel_id,
        status: wire_live_status_from_machine_authority(authority)?,
    })
}

fn wire_live_status_from_machine_authority(
    authority: &meerkat_runtime::meerkat_machine::LiveChannelStatusAuthority,
) -> Result<WireLiveAdapterStatus, String> {
    use meerkat_runtime::meerkat_machine::dsl::LiveChannelPublicStatus;

    match authority.status {
        LiveChannelPublicStatus::Idle => Ok(WireLiveAdapterStatus::Idle),
        LiveChannelPublicStatus::Opening => Ok(WireLiveAdapterStatus::Opening),
        LiveChannelPublicStatus::Ready => Ok(WireLiveAdapterStatus::Ready),
        LiveChannelPublicStatus::Closing => Ok(WireLiveAdapterStatus::Closing),
        LiveChannelPublicStatus::Closed => Ok(WireLiveAdapterStatus::Closed),
        LiveChannelPublicStatus::Degraded => {
            let reason = authority.degradation_reason.ok_or_else(|| {
                "LiveChannelStatusResolved emitted degraded status without reason".to_string()
            })?;
            Ok(WireLiveAdapterStatus::Degraded {
                reason: wire_live_degradation_reason_from_machine_authority(
                    reason,
                    authority.degradation_detail.as_deref(),
                ),
            })
        }
    }
}

fn wire_live_degradation_reason_from_machine_authority(
    reason: meerkat_runtime::meerkat_machine::dsl::LiveChannelDegradationReason,
    detail: Option<&str>,
) -> WireLiveDegradationReason {
    use meerkat_runtime::meerkat_machine::dsl::LiveChannelDegradationReason;

    match reason {
        LiveChannelDegradationReason::RateLimited => WireLiveDegradationReason::RateLimited,
        LiveChannelDegradationReason::ProviderThrottled => {
            WireLiveDegradationReason::ProviderThrottled
        }
        LiveChannelDegradationReason::NetworkUnstable => WireLiveDegradationReason::NetworkUnstable,
        LiveChannelDegradationReason::Other => WireLiveDegradationReason::Other {
            detail: detail.unwrap_or_default().to_string(),
        },
        LiveChannelDegradationReason::Unknown => WireLiveDegradationReason::Unknown {
            debug: detail
                .unwrap_or("unknown live channel degradation")
                .to_string(),
        },
    }
}

fn live_command_rejection_response_from_machine_authority(
    rpc_id: Value,
    authority: &meerkat_runtime::meerkat_machine::LiveCommandRejectionAuthority,
    expected: meerkat_runtime::meerkat_machine::dsl::LiveCommandPublicKind,
    channel_id: &LiveChannelId,
    host_error: &LiveAdapterHostError,
) -> JsonRpcResponse {
    use meerkat_runtime::meerkat_machine::dsl::{
        LiveCommandRejectionPublicErrorClass, LiveCommandRejectionReason,
    };

    if authority.command != expected {
        return live_error(
            rpc_id,
            INTERNAL_ERROR_CODE,
            format!(
                "LiveCommandRejectionResolved emitted command {:?} for expected {:?}",
                authority.command, expected
            ),
        );
    }

    let code = match authority.public_error_class {
        LiveCommandRejectionPublicErrorClass::InvalidParams => INVALID_PARAMS_CODE,
        LiveCommandRejectionPublicErrorClass::InternalError => INTERNAL_ERROR_CODE,
    };
    let message = match authority.rejection {
        LiveCommandRejectionReason::ChannelNotFound => format!("channel {channel_id} not found"),
        LiveCommandRejectionReason::NoAdapter => {
            format!("channel {channel_id} has no adapter attached")
        }
        LiveCommandRejectionReason::ChannelNotReady
        | LiveCommandRejectionReason::UnsupportedCommand
        | LiveCommandRejectionReason::AdapterError
        | LiveCommandRejectionReason::InternalHostError => host_error.to_string(),
    };
    live_error(rpc_id, code, message)
}

async fn live_command_error_response(
    rpc_id: Value,
    machine: &Arc<MeerkatMachine>,
    session_id: &SessionId,
    channel_id: &LiveChannelId,
    command: meerkat_runtime::meerkat_machine::dsl::LiveCommandPublicKind,
    host_error: &LiveAdapterHostError,
) -> JsonRpcResponse {
    let authority = match machine
        .resolve_live_command_rejection_result(session_id, channel_id, command, host_error)
        .await
    {
        Ok(authority) => authority,
        Err(error) => {
            return live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!("live command rejection authority rejected result: {error}"),
            );
        }
    };
    live_command_rejection_response_from_machine_authority(
        rpc_id, &authority, command, channel_id, host_error,
    )
}

async fn live_unbound_command_error_response(
    rpc_id: Value,
    machine: &Arc<MeerkatMachine>,
    channel_id: &LiveChannelId,
    command: meerkat_runtime::meerkat_machine::dsl::LiveCommandPublicKind,
) -> JsonRpcResponse {
    let authority = match machine
        .resolve_unbound_live_command_rejection_result(channel_id, command)
        .await
    {
        Ok(authority) => authority,
        Err(error) => {
            return live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!("unbound live command rejection authority rejected result: {error}"),
            );
        }
    };
    let host_error = LiveAdapterHostError::ChannelNotFound(channel_id.clone());
    live_command_rejection_response_from_machine_authority(
        rpc_id,
        &authority,
        command,
        channel_id,
        &host_error,
    )
}

fn live_channel_request_rejection_response_from_machine_authority(
    rpc_id: Value,
    authority: &meerkat_runtime::meerkat_machine::LiveChannelRequestRejectionAuthority,
    expected: meerkat_runtime::meerkat_machine::dsl::LiveChannelRequestPublicKind,
    channel_id: &LiveChannelId,
    detail: Option<String>,
) -> JsonRpcResponse {
    use meerkat_runtime::meerkat_machine::dsl::{
        LiveChannelRequestRejectionPublicErrorClass, LiveChannelRequestRejectionReason,
    };

    if authority.request != expected {
        return live_error(
            rpc_id,
            INTERNAL_ERROR_CODE,
            format!(
                "LiveChannelRequestRejectionResolved emitted request {:?} for expected {:?}",
                authority.request, expected
            ),
        );
    }

    let code = match authority.public_error_class {
        LiveChannelRequestRejectionPublicErrorClass::InvalidParams => INVALID_PARAMS_CODE,
        LiveChannelRequestRejectionPublicErrorClass::InternalError => INTERNAL_ERROR_CODE,
    };
    let message = match authority.rejection {
        LiveChannelRequestRejectionReason::ChannelNotFound => {
            format!("channel {channel_id} not found")
        }
        LiveChannelRequestRejectionReason::NoAdapter => {
            format!("channel {channel_id} has no adapter attached")
        }
        LiveChannelRequestRejectionReason::InvalidToken
        | LiveChannelRequestRejectionReason::InvalidPayload
        | LiveChannelRequestRejectionReason::WebrtcAnswerError
        | LiveChannelRequestRejectionReason::InternalHostError => detail.unwrap_or_else(|| {
            format!(
                "live channel request {:?} rejected for channel {}",
                authority.request, channel_id
            )
        }),
    };
    live_error(rpc_id, code, message)
}

async fn live_channel_request_error_response(
    rpc_id: Value,
    machine: &Arc<MeerkatMachine>,
    session_id: &SessionId,
    channel_id: &LiveChannelId,
    request: meerkat_runtime::meerkat_machine::dsl::LiveChannelRequestPublicKind,
    host_error: &LiveAdapterHostError,
) -> JsonRpcResponse {
    let authority = match machine
        .resolve_live_channel_request_rejection_result(session_id, channel_id, request, host_error)
        .await
    {
        Ok(authority) => authority,
        Err(error) => {
            return live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!("live channel request rejection authority rejected result: {error}"),
            );
        }
    };
    live_channel_request_rejection_response_from_machine_authority(
        rpc_id,
        &authority,
        request,
        channel_id,
        Some(host_error.to_string()),
    )
}

async fn live_unbound_channel_request_error_response(
    rpc_id: Value,
    machine: &Arc<MeerkatMachine>,
    channel_id: &LiveChannelId,
    request: meerkat_runtime::meerkat_machine::dsl::LiveChannelRequestPublicKind,
) -> JsonRpcResponse {
    let authority = match machine
        .resolve_unbound_live_channel_request_rejection_result(channel_id, request)
        .await
    {
        Ok(authority) => authority,
        Err(error) => {
            return live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!(
                    "unbound live channel request rejection authority rejected result: {error}"
                ),
            );
        }
    };
    let host_error = LiveAdapterHostError::ChannelNotFound(channel_id.clone());
    live_channel_request_rejection_response_from_machine_authority(
        rpc_id,
        &authority,
        request,
        channel_id,
        Some(host_error.to_string()),
    )
}

async fn handle_live_status(
    ctx: &GatewayLiveContext,
    machine: &Arc<MeerkatMachine>,
    params: &Value,
    rpc_id: Value,
) -> JsonRpcResponse {
    let parsed: LiveChannelParams = match parse_live_params(params, &rpc_id) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    let channel_id = LiveChannelId::new(&parsed.channel_id);

    let request_kind = meerkat_runtime::meerkat_machine::dsl::LiveChannelRequestPublicKind::Status;
    let Some(session_id) = machine.live_session_for_status_channel(&channel_id).await else {
        return live_unbound_channel_request_error_response(
            rpc_id,
            machine,
            &channel_id,
            request_kind,
        )
        .await;
    };

    match ctx.host.channel_status_observation(&channel_id).await {
        Ok(observation) => {
            let authority = match machine
                .resolve_live_channel_status_result(&session_id, &observation)
                .await
            {
                Ok(authority) => authority,
                Err(error) => {
                    return live_error(
                        rpc_id,
                        INTERNAL_ERROR_CODE,
                        format!("live status authority rejected result: {error}"),
                    );
                }
            };
            let result =
                match live_status_result_from_machine_authority(parsed.channel_id, &authority) {
                    Ok(result) => result,
                    Err(error) => return live_error(rpc_id, INTERNAL_ERROR_CODE, error),
                };
            match serde_json::to_value(result) {
                Ok(value) => live_success(rpc_id, value),
                Err(err) => live_error(
                    rpc_id,
                    INTERNAL_ERROR_CODE,
                    format!("failed to serialize LiveStatusResult: {err}"),
                ),
            }
        }
        Err(err) => {
            live_channel_request_error_response(
                rpc_id,
                machine,
                &session_id,
                &channel_id,
                request_kind,
                &err,
            )
            .await
        }
    }
}

async fn handle_live_close(
    ctx: &GatewayLiveContext,
    machine: &Arc<MeerkatMachine>,
    params: &Value,
    rpc_id: Value,
) -> JsonRpcResponse {
    let parsed: LiveChannelParams = match parse_live_params(params, &rpc_id) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    let channel_id = LiveChannelId::new(&parsed.channel_id);

    let request_kind = meerkat_runtime::meerkat_machine::dsl::LiveChannelRequestPublicKind::Close;
    let Some(session_id) = machine.live_session_for_active_channel(&channel_id).await else {
        return live_unbound_channel_request_error_response(
            rpc_id,
            machine,
            &channel_id,
            request_kind,
        )
        .await;
    };

    match ctx
        .host
        .reserve_channel_close_observation(&channel_id)
        .await
    {
        Ok(observation) => {
            let authority = match machine
                .resolve_live_close_result(&session_id, &observation)
                .await
            {
                Ok(authority) => authority,
                Err(error) => {
                    return live_error(
                        rpc_id,
                        INTERNAL_ERROR_CODE,
                        format!("live close authority rejected result: {error}"),
                    );
                }
            };
            let Some(close_commit_authority) = authority.channel_close_commit_authority() else {
                return live_error(
                    rpc_id,
                    INTERNAL_ERROR_CODE,
                    "live close authority omitted host commit handoff",
                );
            };
            if let Err(error) = ctx
                .host
                .commit_channel_close_observation(&observation, close_commit_authority)
                .await
            {
                return live_error(
                    rpc_id,
                    INTERNAL_ERROR_CODE,
                    format!("live close host commit failed after generated authority: {error}"),
                );
            }
            let result = live_close_result_from_machine_authority(&authority);
            match serde_json::to_value(result) {
                Ok(body) => live_success(rpc_id, body),
                Err(error) => live_error(
                    rpc_id,
                    INTERNAL_ERROR_CODE,
                    format!("live close authority projection failed: {error}"),
                ),
            }
        }
        Err(err) => {
            live_channel_request_error_response(
                rpc_id,
                machine,
                &session_id,
                &channel_id,
                request_kind,
                &err,
            )
            .await
        }
    }
}

/// P1#5: enqueue a mutable live-config update against the active channel.
/// Config-only by design — history is `live/open`'s seed step (re-seeding on
/// refresh compounds the provider transcript). Identity swaps require close
/// and reopen. R7: the honest reply is `status: queued`; the adapter pump
/// applies asynchronously and failures surface on the realtime stream.
async fn handle_live_refresh<B: SessionAgentBuilder + 'static>(
    ctx: &GatewayLiveContext,
    service: &Arc<PersistentSessionService<B>>,
    machine: &Arc<MeerkatMachine>,
    params: &Value,
    rpc_id: Value,
) -> JsonRpcResponse {
    let parsed: LiveChannelParams = match parse_live_params(params, &rpc_id) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    let channel_id = LiveChannelId::new(&parsed.channel_id);

    let request_kind = meerkat_runtime::meerkat_machine::dsl::LiveChannelRequestPublicKind::Refresh;
    let Some(session_id) = machine.live_session_for_active_channel(&channel_id).await else {
        return live_unbound_channel_request_error_response(
            rpc_id,
            machine,
            &channel_id,
            request_kind,
        )
        .await;
    };

    // Refresh re-projects from the durable session; the gateway-wide seed
    // window applies (there is no per-refresh override on the wire).
    let open_config = match live_open_config_for_session(
        service,
        &session_id,
        RealtimeTurningMode::ProviderManaged,
        ctx.seed_max_chars,
    )
    .await
    {
        Ok(config) => config,
        Err(err) => {
            return live_error(
                rpc_id,
                INTERNAL_ERROR_CODE,
                format!("failed to build session config: {err}"),
            );
        }
    };
    // R8: stamp the host's monotonic per-channel version so adapters gating
    // on `snapshot_version` for stale-refresh detection see strictly
    // increasing generations. #176: refresh has no factory audio policy in
    // scope; the format negotiated at open time stays in force.
    let mut snapshot = build_live_projection_snapshot(&session_id, &open_config, None);
    match ctx.host.next_snapshot_version(&channel_id).await {
        Ok(v) => snapshot.snapshot_version = v,
        Err(err) => {
            return live_channel_request_error_response(
                rpc_id,
                machine,
                &session_id,
                &channel_id,
                request_kind,
                &err,
            )
            .await;
        }
    }

    match ctx.host.enqueue_refresh(&channel_id, snapshot).await {
        Ok(acceptance) => {
            let authority = match machine
                .resolve_live_refresh_queued_result(&session_id, &acceptance)
                .await
            {
                Ok(authority) => authority,
                Err(error) => {
                    return live_error(
                        rpc_id,
                        INTERNAL_ERROR_CODE,
                        format!("live refresh queued authority rejected result: {error}"),
                    );
                }
            };
            let result = live_refresh_result_from_machine_authority(&authority);
            match serde_json::to_value(result) {
                Ok(body) => live_success(rpc_id, body),
                Err(error) => live_error(
                    rpc_id,
                    INTERNAL_ERROR_CODE,
                    format!("live refresh queued authority projection failed: {error}"),
                ),
            }
        }
        Err(err) => {
            live_channel_request_error_response(
                rpc_id,
                machine,
                &session_id,
                &channel_id,
                request_kind,
                &err,
            )
            .await
        }
    }
}

async fn handle_live_send_input(
    ctx: &GatewayLiveContext,
    machine: &Arc<MeerkatMachine>,
    params: &Value,
    rpc_id: Value,
) -> JsonRpcResponse {
    let parsed: LiveSendInputParams = match parse_live_params(params, &rpc_id) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    let channel_id = LiveChannelId::new(&parsed.channel_id);
    let chunk = match live_input_chunk_from_wire(parsed.chunk) {
        Ok(chunk) => chunk,
        Err(err) => {
            return live_error(rpc_id, INVALID_PARAMS_CODE, err.to_string());
        }
    };

    let command_kind = meerkat_runtime::meerkat_machine::dsl::LiveCommandPublicKind::SendInput;
    let Some(session_id) = machine.live_session_for_active_channel(&channel_id).await else {
        return live_unbound_command_error_response(rpc_id, machine, &channel_id, command_kind)
            .await;
    };

    match ctx.host.send_input_observed(&channel_id, chunk).await {
        Ok(acceptance) => {
            let authority = match machine
                .resolve_live_command_result(&session_id, &acceptance)
                .await
            {
                Ok(authority) => authority,
                Err(error) => {
                    return live_error(
                        rpc_id,
                        INTERNAL_ERROR_CODE,
                        format!("live send_input authority rejected result: {error}"),
                    );
                }
            };
            match live_command_result_from_machine_authority(&authority, command_kind) {
                Ok(value) => live_success(rpc_id, value),
                Err(error) => live_error(rpc_id, INTERNAL_ERROR_CODE, error),
            }
        }
        Err(err) => {
            live_command_error_response(
                rpc_id,
                machine,
                &session_id,
                &channel_id,
                command_kind,
                &err,
            )
            .await
        }
    }
}

/// I50/G9: flush buffered uncommitted input; the optional
/// `response_modality` requests a text-only response on an audio-first
/// channel without flipping the channel-wide modality.
async fn handle_live_commit_input(
    ctx: &GatewayLiveContext,
    machine: &Arc<MeerkatMachine>,
    params: &Value,
    rpc_id: Value,
) -> JsonRpcResponse {
    let parsed: LiveCommitInputParams = match parse_live_params(params, &rpc_id) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    let channel_id = LiveChannelId::new(&parsed.channel_id);
    // R5-3: the wire mirror is `TryFrom`-only; an unrecognized future
    // modality rejects loudly instead of silently coercing.
    let response_modality = match parsed.response_modality.map(TryInto::try_into) {
        Some(Ok(modality)) => Some(modality),
        Some(Err(err)) => {
            return live_error(
                rpc_id,
                INVALID_PARAMS_CODE,
                format!("invalid response_modality: {err}"),
            );
        }
        None => None,
    };

    let command_kind = meerkat_runtime::meerkat_machine::dsl::LiveCommandPublicKind::CommitInput;
    let Some(session_id) = machine.live_session_for_active_channel(&channel_id).await else {
        return live_unbound_command_error_response(rpc_id, machine, &channel_id, command_kind)
            .await;
    };

    match ctx
        .host
        .send_command_observed(
            &channel_id,
            LiveAdapterCommand::CommitInput { response_modality },
        )
        .await
    {
        Ok(acceptance) => {
            let authority = match machine
                .resolve_live_command_result(&session_id, &acceptance)
                .await
            {
                Ok(authority) => authority,
                Err(error) => {
                    return live_error(
                        rpc_id,
                        INTERNAL_ERROR_CODE,
                        format!("live commit_input authority rejected result: {error}"),
                    );
                }
            };
            match live_command_result_from_machine_authority(&authority, command_kind) {
                Ok(value) => live_success(rpc_id, value),
                Err(error) => live_error(rpc_id, INTERNAL_ERROR_CODE, error),
            }
        }
        Err(err) => {
            live_command_error_response(
                rpc_id,
                machine,
                &session_id,
                &channel_id,
                command_kind,
                &err,
            )
            .await
        }
    }
}

/// A7: explicit barge-in surface — without it callers can only rely on
/// provider-native VAD.
async fn handle_live_interrupt(
    ctx: &GatewayLiveContext,
    machine: &Arc<MeerkatMachine>,
    params: &Value,
    rpc_id: Value,
) -> JsonRpcResponse {
    let parsed: LiveChannelParams = match parse_live_params(params, &rpc_id) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    let channel_id = LiveChannelId::new(&parsed.channel_id);
    let command_kind = meerkat_runtime::meerkat_machine::dsl::LiveCommandPublicKind::Interrupt;
    let Some(session_id) = machine.live_session_for_active_channel(&channel_id).await else {
        return live_unbound_command_error_response(rpc_id, machine, &channel_id, command_kind)
            .await;
    };

    match ctx
        .host
        .send_command_observed(&channel_id, LiveAdapterCommand::Interrupt)
        .await
    {
        Ok(acceptance) => {
            let authority = match machine
                .resolve_live_command_result(&session_id, &acceptance)
                .await
            {
                Ok(authority) => authority,
                Err(error) => {
                    return live_error(
                        rpc_id,
                        INTERNAL_ERROR_CODE,
                        format!("live interrupt authority rejected result: {error}"),
                    );
                }
            };
            match live_command_result_from_machine_authority(&authority, command_kind) {
                Ok(value) => live_success(rpc_id, value),
                Err(error) => live_error(rpc_id, INTERNAL_ERROR_CODE, error),
            }
        }
        Err(err) => {
            live_command_error_response(
                rpc_id,
                machine,
                &session_id,
                &channel_id,
                command_kind,
                &err,
            )
            .await
        }
    }
}

/// A7: `mobkit/live/truncate` — truncate an assistant item at the given
/// playback cursor. Port of the reference `handle_live_truncate`; maps to
/// `LiveAdapterCommand::TruncateAssistantOutput` (no webrtc output-audio
/// discard arm — the mobkit gateway mounts the WS transport only).
async fn handle_live_truncate(
    ctx: &GatewayLiveContext,
    machine: &Arc<MeerkatMachine>,
    params: &Value,
    rpc_id: Value,
) -> JsonRpcResponse {
    let parsed: LiveTruncateParams = match parse_live_params(params, &rpc_id) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    // Validate item_id is non-empty. content_index is `u32` and
    // audio_played_ms is `u64`, so the type system already rejects negatives
    // at deserialization (`>= 0` is satisfied by construction).
    if parsed.item_id.is_empty() {
        return live_error(rpc_id, INVALID_PARAMS_CODE, "item_id must be non-empty");
    }

    let channel_id = LiveChannelId::new(&parsed.channel_id);
    let command_kind =
        meerkat_runtime::meerkat_machine::dsl::LiveCommandPublicKind::TruncateAssistantOutput;
    let Some(session_id) = machine.live_session_for_active_channel(&channel_id).await else {
        return live_unbound_command_error_response(rpc_id, machine, &channel_id, command_kind)
            .await;
    };
    let command = LiveAdapterCommand::TruncateAssistantOutput {
        item_id: parsed.item_id.clone(),
        content_index: parsed.content_index,
        audio_played_ms: parsed.audio_played_ms,
    };

    match ctx.host.send_command_observed(&channel_id, command).await {
        Ok(acceptance) => {
            let authority = match machine
                .resolve_live_command_result(&session_id, &acceptance)
                .await
            {
                Ok(authority) => authority,
                Err(error) => {
                    return live_error(
                        rpc_id,
                        INTERNAL_ERROR_CODE,
                        format!("live truncate authority rejected result: {error}"),
                    );
                }
            };
            match live_command_result_from_machine_authority(&authority, command_kind) {
                Ok(value) => live_success(rpc_id, value),
                Err(error) => live_error(rpc_id, INTERNAL_ERROR_CODE, error),
            }
        }
        Err(err) => {
            live_command_error_response(
                rpc_id,
                machine,
                &session_id,
                &channel_id,
                command_kind,
                &err,
            )
            .await
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn test_session_id() -> SessionId {
        SessionId::parse("00000000-0000-0000-0000-000000000001").unwrap()
    }

    fn other_session_id() -> SessionId {
        SessionId::parse("00000000-0000-0000-0000-000000000002").unwrap()
    }

    fn test_open_config(seed_messages: Vec<Message>) -> RealtimeSessionOpenConfig {
        RealtimeSessionOpenConfig::new(
            RealtimeTurningMode::ProviderManaged,
            meerkat_core::SessionLlmIdentity {
                model: "gpt-realtime-2".to_string(),
                provider: meerkat_core::Provider::OpenAI,
                self_hosted_server_id: None,
                provider_params: None,
                auth_binding: None,
            },
            Vec::new(),
            seed_messages,
        )
        .with_system_prompt(Some("You are the member.".to_string()))
    }

    fn user_message(text: &str) -> Message {
        Message::User(meerkat_core::types::UserMessage::text(text))
    }

    fn system_message(text: &str) -> Message {
        Message::System(meerkat_core::types::SystemMessage::new(text))
    }

    /// Fix 2 lane assertion: the per-open overlay lands ONLY on the runtime
    /// system-context lane — the typed system prompt (durable prompt truth)
    /// and the projected seed history stay byte-identical.
    #[test]
    fn instruction_overlay_rides_the_runtime_system_context_lane() {
        let seed = vec![system_message("You are the member."), user_message("hello")];
        let mut config = test_open_config(seed.clone());

        apply_live_open_instruction_overlay(
            &mut config,
            vec![
                "Speak Swedish.".to_string(),
                "   ".to_string(),
                "Keep replies short.".to_string(),
            ],
        );

        assert_eq!(config.runtime_system_context.len(), 1);
        let append = &config.runtime_system_context[0];
        assert_eq!(
            append.content.render_text(),
            "Speak Swedish.\n\nKeep replies short."
        );
        assert_eq!(
            append.source.as_deref(),
            Some(LIVE_OPEN_INSTRUCTIONS_SOURCE)
        );
        assert!(
            append.source_kind.is_runtime_steer(),
            "overlay is a transient runtime steer"
        );
        assert_eq!(
            config.system_prompt.as_deref(),
            Some("You are the member."),
            "the typed system prompt is not the overlay lane"
        );
        assert_eq!(config.seed_messages, seed, "seed history untouched");
    }

    #[test]
    fn empty_or_whitespace_instructions_append_nothing() {
        let mut config = test_open_config(Vec::new());
        apply_live_open_instruction_overlay(&mut config, Vec::new());
        apply_live_open_instruction_overlay(&mut config, vec!["  ".to_string(), String::new()]);
        assert!(config.runtime_system_context.is_empty());
    }

    /// #301 port: the surface mapper routes through the canonical typed
    /// owner so distinct `SessionError` variants land in distinct typed
    /// `LiveProjectionError` variants — no collapse into
    /// `Internal(to_string())`.
    #[test]
    fn session_error_maps_to_distinct_typed_projection_variants() {
        let id = SessionId::new();

        assert!(matches!(
            session_error_to_projection(
                meerkat_core::SessionError::NotFound { id: id.clone() },
                &id,
            ),
            LiveProjectionError::SessionNotFound(_)
        ));
        assert!(matches!(
            session_error_to_projection(
                meerkat_core::SessionError::Unsupported("nope".to_string()),
                &id,
            ),
            LiveProjectionError::Rejected(_)
        ));
        assert!(matches!(
            session_error_to_projection(meerkat_core::SessionError::Busy { id: id.clone() }, &id),
            LiveProjectionError::SessionBusy(_)
        ));
        assert!(matches!(
            session_error_to_projection(
                meerkat_core::SessionError::NotRunning { id: id.clone() },
                &id,
            ),
            LiveProjectionError::SessionNotRunning(_)
        ));
        assert!(matches!(
            session_error_to_projection(meerkat_core::SessionError::PersistenceDisabled, &id),
            LiveProjectionError::CapabilityDisabled { .. }
        ));
    }

    /// T10 port: the display-text lane builds the `AssistantTextDelta`
    /// variant with the full identity tuple.
    #[test]
    fn assistant_text_delta_helper_builds_text_delta_event() {
        let identity = LiveTranscriptIdentity {
            provider_item_id: Some("item_text"),
            previous_item_id: Some("item_prev"),
            content_index: Some(2),
            response_id: Some("resp_text"),
            delta_id: Some("delta_text"),
        };
        let event = build_assistant_text_delta_event("display fragment", identity)
            .expect("complete identity must build a typed delta event");
        match event {
            RealtimeTranscriptEvent::AssistantTextDelta {
                response_id,
                delta_id,
                item_id,
                previous_item_id,
                content_index,
                delta,
            } => {
                assert_eq!(response_id, "resp_text");
                assert_eq!(delta_id, "delta_text");
                assert_eq!(item_id, "item_text");
                assert_eq!(previous_item_id.as_deref(), Some("item_prev"));
                assert_eq!(content_index, 2);
                assert_eq!(delta, "display fragment");
            }
            other => panic!("display-text delta path must build AssistantTextDelta, got {other:?}"),
        }
    }

    /// T10 port: the spoken-transcript lane builds the dedicated
    /// `AssistantTranscriptDelta` variant so the materializer flushes
    /// `AssistantBlock::Transcript` rather than `Text`.
    #[test]
    fn assistant_transcript_delta_helper_builds_transcript_delta_event() {
        let identity = LiveTranscriptIdentity {
            provider_item_id: Some("item_tx"),
            previous_item_id: Some("item_prev"),
            content_index: Some(0),
            response_id: Some("resp_tx"),
            delta_id: Some("delta_tx"),
        };
        let event = build_assistant_transcript_delta_event("spoken fragment", identity)
            .expect("complete identity must build a typed transcript delta event");
        match event {
            RealtimeTranscriptEvent::AssistantTranscriptDelta {
                response_id,
                delta_id,
                item_id,
                previous_item_id,
                content_index,
                delta,
            } => {
                assert_eq!(response_id, "resp_tx");
                assert_eq!(delta_id, "delta_tx");
                assert_eq!(item_id, "item_tx");
                assert_eq!(previous_item_id.as_deref(), Some("item_prev"));
                assert_eq!(content_index, 0);
                assert_eq!(delta, "spoken fragment");
            }
            other => panic!(
                "spoken-transcript delta path must build AssistantTranscriptDelta, got {other:?}"
            ),
        }
    }

    /// #199 port: a delta missing a required identity id fails closed with
    /// a typed error rather than emitting empty-string identity.
    #[test]
    fn missing_delta_identity_fails_closed_typed() {
        let missing_response = LiveTranscriptIdentity {
            provider_item_id: Some("item"),
            previous_item_id: None,
            content_index: Some(0),
            response_id: None,
            delta_id: Some("delta"),
        };
        assert_eq!(
            build_assistant_text_delta_event("fragment", missing_response),
            Err(LiveTranscriptIdentityError::MissingResponseId)
        );

        let missing_delta = LiveTranscriptIdentity {
            provider_item_id: Some("item"),
            previous_item_id: None,
            content_index: Some(0),
            response_id: Some("resp"),
            delta_id: None,
        };
        assert_eq!(
            build_assistant_transcript_delta_event("fragment", missing_delta),
            Err(LiveTranscriptIdentityError::MissingDeltaId)
        );

        let missing_item = LiveTranscriptIdentity {
            provider_item_id: None,
            previous_item_id: None,
            content_index: Some(0),
            response_id: Some("resp"),
            delta_id: Some("delta"),
        };
        assert_eq!(
            build_assistant_text_delta_event("fragment", missing_item),
            Err(LiveTranscriptIdentityError::MissingItemId)
        );
    }

    /// R6 port: two finals from different provider responses must NOT pool
    /// into the same buffer slot; each completion drains only its own slot,
    /// and a mismatched drain leaves the buffer untouched.
    #[test]
    fn r6_pending_turn_ledger_keys_on_response_id() {
        let ledger = PendingTurnLedger::default();
        let session_id = test_session_id();

        ledger.buffer(
            &session_id,
            Some("resp_a"),
            PendingAssistantContent::Text("from resp_a".to_string()),
        );
        ledger.buffer(
            &session_id,
            Some("resp_b"),
            PendingAssistantContent::Text("from resp_b".to_string()),
        );

        // A completion carrying a response id no slot was buffered under
        // must not flush another turn's transcript.
        assert!(
            ledger
                .drain(&session_id, Some("resp_stale"))
                .blocks
                .is_empty()
        );

        let drained_a = collapse_pending_blocks(ledger.drain(&session_id, Some("resp_a")).blocks);
        assert_eq!(drained_a.len(), 1);
        match &drained_a[0] {
            AssistantBlock::Text { text, .. } => assert_eq!(text, "from resp_a"),
            other => panic!("expected text block, got {other:?}"),
        }

        // resp_b's buffer survived resp_a's completion.
        let drained_b = collapse_pending_blocks(ledger.drain(&session_id, Some("resp_b")).blocks);
        assert_eq!(drained_b.len(), 1);
        match &drained_b[0] {
            AssistantBlock::Text { text, .. } => assert_eq!(text, "from resp_b"),
            other => panic!("expected text block, got {other:?}"),
        }

        // Both slots consumed exactly once.
        assert!(ledger.drain(&session_id, Some("resp_a")).blocks.is_empty());
        assert!(ledger.drain(&session_id, Some("resp_b")).blocks.is_empty());
    }

    /// Terminal-error port: `drain_all` clears every slot for the session
    /// (including the `None` orphan slot) while other sessions' buffers
    /// survive.
    #[test]
    fn terminal_error_drains_all_response_slots_for_session_only() {
        let ledger = PendingTurnLedger::default();
        let session_id = test_session_id();
        let other = other_session_id();

        ledger.buffer(
            &session_id,
            Some("resp_a"),
            PendingAssistantContent::Text("a".to_string()),
        );
        ledger.buffer(
            &session_id,
            None,
            PendingAssistantContent::Text("orphan".to_string()),
        );
        ledger.buffer(
            &other,
            Some("resp_x"),
            PendingAssistantContent::Text("x".to_string()),
        );

        ledger.drain_all(&session_id);

        assert!(ledger.drain(&session_id, Some("resp_a")).blocks.is_empty());
        assert!(ledger.drain(&session_id, None).blocks.is_empty());
        let survived = ledger.drain(&other, Some("resp_x"));
        assert_eq!(survived.blocks.len(), 1);
    }

    /// P1#1/T6 port: consecutive same-lane fragments coalesce into ONE
    /// `AssistantBlock::Text` in arrival order; an empty buffer collapses
    /// to no blocks (the orphan-completion shape).
    #[test]
    fn collapse_pending_blocks_coalesces_fragments_in_arrival_order() {
        let blocks = collapse_pending_blocks(vec![
            PendingAssistantContent::Text("part one ".to_string()),
            PendingAssistantContent::Text("part two".to_string()),
        ]);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            AssistantBlock::Text { text, .. } => assert_eq!(text, "part one part two"),
            other => panic!("expected Text block, got {other:?}"),
        }

        assert!(collapse_pending_blocks(Vec::new()).is_empty());
    }

    /// Token-admission mapping port: every generated rejection variant maps
    /// onto its transport twin, and the public error class collapses to the
    /// transport's `InvalidToken`.
    #[test]
    fn ws_token_admission_mapping_covers_all_machine_variants() {
        use meerkat_runtime::meerkat_machine::dsl::{
            LiveWebsocketTokenAdmissionPublicErrorClass as DslClass,
            LiveWebsocketTokenAdmissionRejection as Dsl,
        };

        let cases = [
            (
                Dsl::TokenNotFound,
                LiveWsTokenAdmissionRejection::TokenNotFound,
            ),
            (
                Dsl::TokenExpired,
                LiveWsTokenAdmissionRejection::TokenExpired,
            ),
            (
                Dsl::TokenChannelMismatch,
                LiveWsTokenAdmissionRejection::TokenChannelMismatch,
            ),
            (
                Dsl::TokenAlreadyConsumed,
                LiveWsTokenAdmissionRejection::TokenAlreadyConsumed,
            ),
            (
                Dsl::ChannelNotBound,
                LiveWsTokenAdmissionRejection::ChannelNotBound,
            ),
        ];
        for (machine, transport) in cases {
            assert_eq!(
                live_ws_token_admission_rejection_from_machine(machine),
                transport
            );
        }

        assert_eq!(
            live_ws_token_public_error_class_from_machine(DslClass::InvalidToken),
            LiveWsTokenAdmissionPublicErrorClass::InvalidToken
        );
    }

    /// `EnvRealtimeConfigSource` serves exactly the config it was given —
    /// per-open resolution then rides the session identity's auth binding
    /// against that config, matching text-model behaviour.
    #[tokio::test]
    async fn env_realtime_config_source_returns_given_config() {
        let mut config = Config::default();
        config.realm.insert(
            "live-wiring-test".to_string(),
            meerkat_core::RealmConfigSection::default(),
        );

        let source = EnvRealtimeConfigSource::new(config);
        let served = source.current_config().await.expect("static config source");
        assert!(served.realm.contains_key("live-wiring-test"));

        // Stable across opens: the source never consults the environment or
        // mutates between calls.
        let served_again = source.current_config().await.expect("static config source");
        assert!(served_again.realm.contains_key("live-wiring-test"));
    }
}
