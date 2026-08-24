//! Operator verbs for oversized/wedged member transcripts.
//!
//! Two verbs, one incident class (HomeCore parent-1: a member transcript
//! grows past what a turn can carry and the only prior remedy was row
//! surgery):
//!
//! - `mobkit/compact_member` — the routine verb. Arms a temporary
//!   auto-compaction floor for the identity (bridge-applied via the typed
//!   `override_profile.auto_compact_threshold` carrier), rebuilds the member
//!   onto its SAME durable session (retire to roster absence, then
//!   re-materialize — identity respawn only re-fences authority and never
//!   rebuilds the agent) so a fresh build picks the floor up, drives one
//!   queued maintenance turn so the forced compaction fires at that turn's
//!   pre-LLM boundary, then disarms the floor and rebuilds again so the live
//!   build returns to the original profile value. Leaving the floor armed
//!   would compact every turn — the meerkat override is deliberately
//!   non-durable across recovery, so the profile is what re-supplies it on
//!   every build.
//!
//! - `mobkit/bound_member_transcript` — the surgical escape hatch. Commits
//!   one audited keep-last-N rewrite through meerkat's
//!   `SessionServiceTranscriptEditExt` on the CONCRETE persistent session
//!   service (the erased `dyn MobSessionService` cannot reach the extension
//!   trait — no trait upcasting), with a pair-safe cut point that never
//!   separates an assistant tool-use message from its adjacent tool_results.
//!   The service refuses live/running sessions with `SessionError::Busy`
//!   (`TranscriptEditRunningBehavior` has only `Reject`); that surfaces as
//!   this verb's typed refusal — quiesce the member first (retire/park), then
//!   retry.
//!
//! Both verbs resolve their target through the same identity-control gate the
//! neighboring destructive verbs (`mobkit/respawn`, `mobkit/reset`,
//! `mobkit/retire`) use, so identities the gateway does not own are refused
//! at resolution.

use super::*;
use crate::identity_first::AgentIdentity;
use meerkat_core::SessionError;
use meerkat_core::types::{Message, SystemNoticeKind, SystemNoticeMessage};
use std::num::NonZeroU64;

/// Typed refusal: the target session is live/running and transcript surgery
/// only supports `Reject` while work is active. Distinct from the SDKs'
/// reserved `-32004` capability code and the identity-plane `-32001..-32005`
/// band.
pub const OPERATOR_SESSION_BUSY_CODE: i64 = -32015;

/// The verb exists on this gateway build but its wiring is absent (no
/// compaction-floor registry / no concrete transcript-edit service threaded
/// into the RPC context).
pub const OPERATOR_VERB_UNAVAILABLE_CODE: i64 = -32016;

/// Default temporary floor: small enough that any oversized transcript is
/// past it, large enough to be a legal non-zero threshold.
const DEFAULT_COMPACT_FLOOR_TOKENS: u64 = 1024;

/// Default budget for the forced-compaction maintenance turn.
const DEFAULT_COMPACT_TIMEOUT_MS: u64 = 60_000;

/// Bound for the post-timeout transcript read that decides whether the forced
/// compaction had already landed. It reads the same session whose turn just
/// missed its budget, so the evidence clause is worth a few seconds and never
/// worth a second unbounded wait.
const COMPACT_TIMEOUT_EVIDENCE_BUDGET: Duration = Duration::from_secs(5);

/// Default keep-last-N for `bound_member_transcript`.
const DEFAULT_BOUND_KEEP_LAST: usize = 50;

fn rpc_error(response_id: Value, code: i64, message: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message,
            data: None,
        }),
    }
}

fn rpc_result(response_id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: response_id,
        result: Some(result),
        error: None,
    }
}

fn optional_u64_param(params: &Value, field: &str) -> Result<Option<u64>, String> {
    match params.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => match value.as_u64() {
            Some(parsed) if parsed > 0 => Ok(Some(parsed)),
            _ => Err(format!("{field} must be a positive integer")),
        },
    }
}

/// The largest prefix length that can be cut while keeping at least
/// `keep_last` trailing messages without orphaning a tool pair.
///
/// The transcript shape rule (meerkat-core `validate_transcript_tool_result_shape`)
/// makes tool pairs ADJACENT: every `ToolResults` is immediately preceded by
/// the assistant tool-use message carrying exactly its tool_use_id set, and
/// vice versa. A cut point is therefore pair-safe iff the first KEPT message
/// is not a `ToolResults`; walking the cut back one message re-includes the
/// paired assistant message. In a shape-valid transcript one step suffices
/// (two adjacent `ToolResults` are impossible); the loop is defensive.
pub(crate) fn pair_safe_cut_index(messages: &[Message], keep_last: usize) -> usize {
    let mut cut = messages.len().saturating_sub(keep_last);
    while cut > 0 && matches!(messages.get(cut), Some(Message::ToolResults { .. })) {
        cut -= 1;
    }
    cut
}

struct ResolvedOperatorTarget {
    identity: AgentIdentity,
    /// Alias-pinned lifecycle precondition, mirroring `mobkit/respawn`:
    /// present only when the caller addressed the member by its reserved
    /// generated runtime alias.
    expected_alias: Option<String>,
}

/// Shared target resolution + authorization for both operator verbs: the
/// exact identity-control gate the destructive identity verbs use, including
/// the stale-live-alias refusal.
async fn resolve_operator_target(
    runtime: &UnifiedRuntime,
    identity_rt: &crate::identity_first::IdentityRuntime,
    params: &Value,
    response_id: &Value,
) -> Result<ResolvedOperatorTarget, Box<JsonRpcResponse>> {
    let identity_str = params
        .get("identity")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let target = match resolve_rpc_identity_control_target(runtime, identity_rt, identity_str).await
    {
        Ok(target) => target,
        Err(e) => {
            return Err(Box::new(rpc_error(
                response_id.clone(),
                -32602,
                format!("invalid identity: {e}"),
            )));
        }
    };
    if let Some(response) =
        rpc_stale_live_alias_error_response(identity_rt, &target, response_id.clone()).await
    {
        return Err(Box::new(response));
    }
    let expected_alias = crate::member_comms_id::is_reserved_generated_alias(identity_str)
        .then(|| identity_str.to_string());
    Ok(ResolvedOperatorTarget {
        identity: target.identity,
        expected_alias,
    })
}

/// Transcript facts read through the concrete session service: message count
/// plus the head revision and the most recent rewrite commit reason.
async fn read_transcript_facts(
    service: &Arc<dyn crate::memory::hygienist::TranscriptEditSessionService>,
    session_id: &meerkat_core::types::SessionId,
) -> Result<(usize, Option<String>, Option<String>), SessionError> {
    let page = service
        .read_history(
            session_id,
            meerkat_core::service::SessionHistoryQuery {
                offset: 0,
                limit: None,
            },
        )
        .await?;
    let (head_revision, last_reason) = match service
        .list_transcript_revisions(
            session_id,
            meerkat_core::service::SessionTranscriptRevisionListQuery {
                limit: None,
                offset: None,
            },
        )
        .await
    {
        Ok(list) => (
            Some(list.head_revision),
            list.entries.last().map(|entry| entry.reason.clone()),
        ),
        Err(SessionError::Unsupported(_)) => (None, None),
        Err(err) => return Err(err),
    };
    Ok((page.messages.len(), head_revision, last_reason))
}

/// `mobkit/compact_member`: force one compaction on a member's next turn via
/// a temporary profile-level threshold floor, then restore the original
/// threshold.
pub(super) async fn handle_compact_member(
    runtime: &UnifiedRuntime,
    ctx: &IdentityFirstContext,
    params: &Value,
    response_id: Value,
) -> JsonRpcResponse {
    let identity_rt = &ctx.runtime;
    let Some(floors) = ctx.compaction_floors.as_ref() else {
        return rpc_error(
            response_id,
            OPERATOR_VERB_UNAVAILABLE_CODE,
            "compact_member is not wired on this gateway: the identity bridge's compaction-floor \
             registry was not threaded into the RPC context"
                .to_string(),
        );
    };
    let floor_tokens = match optional_u64_param(params, "floor_tokens") {
        Ok(value) => value.unwrap_or(DEFAULT_COMPACT_FLOOR_TOKENS),
        Err(message) => return rpc_error(response_id, -32602, message),
    };
    let Some(floor) = NonZeroU64::new(floor_tokens) else {
        return rpc_error(
            response_id,
            -32602,
            "floor_tokens must be greater than 0".to_string(),
        );
    };
    let timeout_ms = match optional_u64_param(params, "timeout_ms") {
        Ok(value) => value.unwrap_or(DEFAULT_COMPACT_TIMEOUT_MS),
        Err(message) => return rpc_error(response_id, -32602, message),
    };
    let target = match resolve_operator_target(runtime, identity_rt, params, &response_id).await {
        Ok(target) => target,
        Err(response) => return *response,
    };
    let identity = target.identity;

    // The identity's CURRENT registered spec: the rebuild re-materializes on
    // exactly this spec, so nothing but the armed floor changes in the build.
    let Some(spec) = identity_rt
        .roster_inspect()
        .await
        .remove(&identity)
        .map(|(spec, _)| spec)
    else {
        // Mirror the destructive identity verbs: an identity this gateway
        // does not own is the typed identity-plane refusal.
        return identity_error_response(
            response_id,
            &crate::identity_first::IdentityRuntimeError::UnknownIdentity(identity),
        );
    };

    // Arm the floor, then rebuild the member so a FRESH agent build lowers it
    // into `SessionBuildOptions::auto_compact_threshold_override`. Identity
    // respawn only re-fences authority (the live agent keeps its old build),
    // so the rebuild is the real quiesce-and-rematerialize cycle: retire the
    // member to roster absence (durable session preserved), then restore_flow
    // resumes the SAME session through the bridge, whose spawn-spec build
    // applies the armed floor. Every exit path below disarms the registry;
    // error paths additionally attempt the restore rebuild so a failed verb
    // does not leave a live floored build.
    floors.set(&identity, floor);
    let session_id = match rebuild_member_for_fresh_build(
        ctx,
        &identity,
        target.expected_alias.as_deref(),
        spec.clone(),
    )
    .await
    {
        Ok(session_id) => session_id,
        Err(detail) => {
            floors.clear(&identity);
            return rpc_error(
                response_id,
                -32000,
                format!("compact_member could not rebuild the member with the floor: {detail}"),
            );
        }
    };

    let before = match ctx.transcript_edit_service.as_ref() {
        Some(service) => match read_transcript_facts(service, &session_id).await {
            Ok(facts) => Some(facts),
            Err(err) => {
                floors.clear(&identity);
                let (_, restore) = restore_after_floor(ctx, &identity, spec.clone()).await;
                return rpc_error(
                    response_id,
                    -32000,
                    format!(
                        "compact_member aborted reading the transcript before compaction: {err}\
                         {restore}"
                    ),
                );
            }
        },
        None => None,
    };

    // One queued maintenance turn: the forced compaction fires at this turn's
    // pre-LLM boundary. The prompt lands in the post-compaction transcript.
    let nudge = meerkat_core::ContentInput::Text(
        "[mobkit-gateway operator verb compact_member] Maintenance turn: transcript compaction \
         was forced for this turn. Reply with a brief acknowledgement only."
            .to_string(),
    );
    let admission = match identity_rt
        .send_admission_tracked(
            &identity,
            None,
            &nudge,
            meerkat_core::types::HandlingMode::Queue,
            None,
        )
        .await
    {
        Ok(admission) => admission,
        Err(err) => {
            floors.clear(&identity);
            let (_, restore) = restore_after_floor(ctx, &identity, spec.clone()).await;
            return rpc_error(
                response_id,
                -32000,
                format!("compact_member maintenance turn was not admitted: {err}{restore}"),
            );
        }
    };
    if let Err(err) = identity_rt
        .wait_for_completion(
            &identity,
            admission.completion_baseline,
            Duration::from_millis(timeout_ms),
        )
        .await
    {
        floors.clear(&identity);
        let (rolled_back, restore) = restore_after_floor(ctx, &identity, spec.clone()).await;
        // Honest timeout semantics: the wait gave up, not the turn. The
        // rollback rebuild that just ran retires the member, and mob
        // retirement quiesces the session's active runtime turn before
        // retiring it (`cancel_active_runtime_turn_before_retire`, under the
        // retirement deadline), so a landed rollback IS the interrupt - no
        // second cancel path is introduced here. A failed rollback leaves the
        // turn running on the floored build, and the caller must be told so
        // rather than left to infer that the timeout stopped anything.
        let turn_fate = if rolled_back {
            "; the in-flight maintenance turn was quiesced by the rollback rebuild \
             (mob retirement cancels the active runtime turn before retiring)"
        } else {
            "; the maintenance turn may still be running on the floored build, so this member \
             can stay briefly unresponsive and later reads on it may queue"
        };
        // The forced compaction fires at the turn's PRE-LLM boundary, so it
        // can already be durable when the wait expires. This evidence read
        // touches the same session that just failed to answer in time, so it
        // gets its own short bound: unproven evidence is dropped from the
        // message, never traded for a second hang.
        let compaction_evidence = match (before.as_ref(), ctx.transcript_edit_service.as_ref()) {
            (Some(before), Some(service)) => {
                match tokio::time::timeout(
                    COMPACT_TIMEOUT_EVIDENCE_BUDGET,
                    read_transcript_facts(service, &session_id),
                )
                .await
                {
                    Ok(Ok(after)) if &after != before => {
                        "; the forced compaction rewrite IS durably applied \
                         (transcript facts changed before the timeout)"
                    }
                    Ok(Ok(_)) => {
                        "; the forced compaction rewrite had not durably applied at \
                                  timeout"
                    }
                    Ok(Err(_)) | Err(_) => "",
                }
            }
            _ => "",
        };
        return rpc_error(
            response_id,
            -32000,
            format!(
                "compact_member maintenance turn did not complete within {timeout_ms}ms: \
                 {err}{turn_fate}{compaction_evidence}{restore}"
            ),
        );
    }

    // Disarm and rebuild at the original profile threshold. The compaction
    // rewrite is already durable; this rebuild only swaps the live build.
    floors.clear(&identity);
    if let Err(detail) = rebuild_member_for_fresh_build(ctx, &identity, None, spec).await {
        return rpc_error(
            response_id,
            -32000,
            format!(
                "compact_member forced the compaction but the restore rebuild failed: {detail}; \
                 the member keeps the temporary floor ({floor} tokens) until its next rebuild"
            ),
        );
    }

    let after = match ctx.transcript_edit_service.as_ref() {
        Some(service) => match read_transcript_facts(service, &session_id).await {
            Ok(facts) => Some(facts),
            Err(err) => {
                return rpc_error(
                    response_id,
                    -32000,
                    format!(
                        "compact_member completed but reading the post-compaction transcript \
                         failed: {err}"
                    ),
                );
            }
        },
        None => None,
    };

    let messages_before = before.as_ref().map(|(count, _, _)| *count);
    let messages_after = after.as_ref().map(|(count, _, _)| *count);
    let compaction_applied = match (messages_before, messages_after) {
        (Some(before), Some(after)) => Some(after < before),
        _ => None,
    };
    rpc_result(
        response_id,
        serde_json::json!({
            "identity": identity.as_str(),
            "session_id": session_id.to_string(),
            "floor_tokens": floor.get(),
            "messages_before": messages_before,
            "messages_after": messages_after,
            "compaction_applied": compaction_applied,
            "head_revision": after.as_ref().and_then(|(_, head, _)| head.clone()),
            "last_rewrite_reason": after.as_ref().and_then(|(_, _, reason)| reason.clone()),
        }),
    )
}

/// Tear the identity's live member down and re-materialize it onto the SAME
/// durable session, forcing a fresh agent build.
///
/// This is the quiesce-and-rebuild cycle `compact_member` rides: identity
/// respawn (`respawn_identity_in_place_tracked`) only re-fences authority and
/// leaves the live agent's build untouched, so a build-time input like the
/// armed compaction floor never reaches it. Retire tears the member down to
/// roster absence with the durable session preserved; `restore_flow` then
/// re-registers the identity from its current spec and resumes the SAME
/// session through the bridge's spawn-spec build (where the floor applies).
///
/// A Dormant/never-materialized identity skips the retire and goes straight
/// to materialization. Deliberately NOT `restore_flow`: that is fleet-scoped
/// (it overwrites the runtime's desired peer edges from the roster it is
/// handed), while this verb must touch exactly one identity. Fails typed when
/// the durable continuity binding needed for a same-session resume is
/// incomplete - materializing a fresh session would abandon the transcript
/// this verb exists to compact.
async fn rebuild_member_for_fresh_build(
    ctx: &IdentityFirstContext,
    identity: &AgentIdentity,
    expected_alias: Option<&str>,
    spec: crate::identity_first::DurableAgentSpec,
) -> Result<meerkat_core::types::SessionId, String> {
    use crate::identity_first::IdentityLifecycleState;
    let identity_rt = &ctx.runtime;
    let state = identity_rt
        .status(identity)
        .await
        .map_err(|err| format!("status before rebuild: {err}"))?
        .state;
    if state == IdentityLifecycleState::Active {
        match expected_alias {
            Some(alias) => identity_rt
                .retire_member_alias_tracked(identity, alias)
                .await
                .map(|_| ()),
            None => identity_rt.retire_tracked(identity).await.map(|_| ()),
        }
        .map_err(|err| format!("quiesce retire: {err}"))?;
    }
    // Reproject the entry as Dormant with the DURABLE continuity binding so
    // `materialize` (which refuses Retiring) resumes the SAME durable
    // session. The store record is read after the retire because retire runs
    // a final checkpoint - the stored row carries the freshest checkpoint
    // version.
    let resolved = identity_rt
        .continuity_store()
        .resolve_many(std::slice::from_ref(identity))
        .await
        .map_err(|err| format!("continuity resolve after quiesce: {err}"))?;
    let record = match resolved.get(identity) {
        Some(crate::identity_first::ContinuityResolveState::Ready { record }) => record.clone(),
        Some(crate::identity_first::ContinuityResolveState::Broken { failure }) => {
            return Err(format!(
                "identity {} has broken continuity ({}); cannot rebuild onto the same durable \
                 session",
                identity.as_str(),
                failure.detail
            ));
        }
        Some(crate::identity_first::ContinuityResolveState::Uninitialized) | None => {
            return Err(format!(
                "identity {} has no durable continuity record; cannot rebuild onto the same \
                 durable session",
                identity.as_str()
            ));
        }
    };
    identity_rt
        .register(spec, IdentityLifecycleState::Dormant, Some(record), None)
        .await;
    identity_rt
        .materialize_tracked(identity)
        .await
        .map(|record| record.session_id)
        .map_err(|err| format!("re-materialization: {err}"))
}

/// Best-effort restore rebuild for `compact_member` error paths, after the
/// floor registry entry was cleared.
///
/// Returns whether the rollback rebuild actually landed, plus a suffix for the
/// error message stating what the member build is left with. The landed flag
/// is load-bearing on the timeout path: the rebuild retires the member, and
/// mob retirement quiesces the session's active runtime turn before retiring,
/// so a landed rollback is also what stops an in-flight maintenance turn.
async fn restore_after_floor(
    ctx: &IdentityFirstContext,
    identity: &AgentIdentity,
    spec: crate::identity_first::DurableAgentSpec,
) -> (bool, String) {
    match rebuild_member_for_fresh_build(ctx, identity, None, spec).await {
        Ok(_) => (
            true,
            "; the temporary floor was rolled back (member rebuilt at its original threshold)"
                .to_string(),
        ),
        Err(detail) => (
            false,
            format!(
                "; rollback rebuild also failed ({detail}) - the member keeps the temporary \
                 floor until its next rebuild"
            ),
        ),
    }
}

/// `mobkit/bound_member_transcript`: one audited keep-last-N transcript
/// rewrite on a quiesced member session.
pub(super) async fn handle_bound_member_transcript(
    runtime: &UnifiedRuntime,
    ctx: &IdentityFirstContext,
    params: &Value,
    response_id: Value,
) -> JsonRpcResponse {
    let identity_rt = &ctx.runtime;
    let Some(service) = ctx.transcript_edit_service.as_ref() else {
        return rpc_error(
            response_id,
            OPERATOR_VERB_UNAVAILABLE_CODE,
            "bound_member_transcript is not wired on this gateway: the concrete persistent \
             session service was not threaded into the RPC context (the erased MobSessionService \
             cannot reach SessionServiceTranscriptEditExt)"
                .to_string(),
        );
    };
    let keep_last = match optional_u64_param(params, "keep_last") {
        Ok(value) => value.map_or(DEFAULT_BOUND_KEEP_LAST, |parsed| parsed as usize),
        Err(message) => return rpc_error(response_id, -32602, message),
    };
    let note = match params.get("note") {
        None | Some(Value::Null) => None,
        Some(Value::String(note)) => Some(note.clone()),
        Some(_) => {
            return rpc_error(
                response_id,
                -32602,
                "note must be a string when provided".to_string(),
            );
        }
    };
    let target = match resolve_operator_target(runtime, identity_rt, params, &response_id).await {
        Ok(target) => target,
        Err(response) => return *response,
    };
    let identity = target.identity;

    let status = match identity_rt.status(&identity).await {
        Ok(status) => status,
        Err(err) => return identity_error_response(response_id, &err),
    };
    let Some(session_id) = status.session_id else {
        return rpc_error(
            response_id,
            -32000,
            format!(
                "identity {} has no current session to bound",
                identity.as_str()
            ),
        );
    };

    let messages = match service
        .read_history(
            &session_id,
            meerkat_core::service::SessionHistoryQuery {
                offset: 0,
                limit: None,
            },
        )
        .await
    {
        Ok(page) => page.messages,
        Err(err) => {
            return rpc_error(
                response_id,
                -32000,
                format!("bound_member_transcript failed to read the transcript: {err}"),
            );
        }
    };
    // Fresh head for the compare-and-swap: the rewrite is rejected if the
    // head advances between this read and the commit.
    let expected_parent_revision = match service
        .list_transcript_revisions(
            &session_id,
            meerkat_core::service::SessionTranscriptRevisionListQuery {
                limit: Some(0),
                offset: None,
            },
        )
        .await
    {
        Ok(list) => Some(list.head_revision),
        Err(SessionError::Unsupported(_)) => None,
        Err(err) => {
            return rpc_error(
                response_id,
                -32000,
                format!("bound_member_transcript failed to read the head revision: {err}"),
            );
        }
    };

    let cut = pair_safe_cut_index(&messages, keep_last);
    if cut == 0 {
        return rpc_result(
            response_id,
            serde_json::json!({
                "identity": identity.as_str(),
                "session_id": session_id.to_string(),
                "bounded": false,
                "removed": 0,
                "message_count": messages.len(),
            }),
        );
    }

    let marker = Message::SystemNotice(SystemNoticeMessage::new(
        SystemNoticeKind::Generic,
        format!(
            "[operator] transcript bounded: {cut} earlier message(s) were removed by \
             bound_member_transcript; the conversation continues from the {} most recent \
             message(s).",
            messages.len() - cut
        ),
    ));
    let mut reason = meerkat_core::TranscriptRewriteReason::new("operator_bound_transcript");
    reason.note = Some(note.unwrap_or_else(|| {
        format!("mobkit-gateway operator verb bound_member_transcript keep_last={keep_last}")
    }));
    let request = meerkat_core::service::SessionTranscriptRewriteRequest {
        selection: meerkat_core::TranscriptRewriteSelection::MessageRange { start: 0, end: cut },
        replacement: vec![marker],
        reason,
        actor: Some("mobkit-gateway operator verb".to_string()),
        expected_parent_revision,
        running_behavior: meerkat_core::TranscriptEditRunningBehavior::default(),
    };
    match service
        .rewrite_session_transcript(&session_id, request)
        .await
    {
        Ok(result) => rpc_result(
            response_id,
            serde_json::json!({
                "identity": identity.as_str(),
                "session_id": result.session_id.to_string(),
                "bounded": true,
                "removed": cut,
                "kept": messages.len() - cut,
                "message_count": result.message_count,
                "parent_revision": result.parent_revision,
                "revision": result.revision,
            }),
        ),
        Err(SessionError::Busy { id }) => rpc_error(
            response_id,
            OPERATOR_SESSION_BUSY_CODE,
            format!(
                "session {id} is live/running; transcript surgery only supports Reject while \
                 work is active - quiesce the member first (retire or park it), then retry"
            ),
        ),
        Err(err) => rpc_error(
            response_id,
            -32000,
            format!("bound_member_transcript rewrite failed: {err}"),
        ),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use meerkat_core::types::{AssistantBlock, BlockAssistantMessage, ToolResult, UserMessage};

    fn user(text: &str) -> Message {
        Message::User(UserMessage::text(text))
    }

    fn tool_use_pair(id: &str) -> (Message, Message) {
        let assistant = Message::BlockAssistant(BlockAssistantMessage::new(
            vec![AssistantBlock::ToolUse {
                id: id.to_string(),
                name: "probe".to_string(),
                args: serde_json::value::RawValue::from_string("{}".to_string()).expect("raw args"),
                meta: None,
            }],
            meerkat_core::types::StopReason::ToolUse,
        ));
        let results = Message::tool_results(vec![ToolResult {
            tool_use_id: id.to_string(),
            content: vec![],
            is_error: false,
        }]);
        (assistant, results)
    }

    #[test]
    fn cut_keeps_whole_transcript_when_keep_last_covers_it() {
        let messages = vec![user("a"), user("b")];
        assert_eq!(pair_safe_cut_index(&messages, 2), 0);
        assert_eq!(pair_safe_cut_index(&messages, 10), 0);
    }

    #[test]
    fn cut_lands_on_plain_message_boundary() {
        let messages = vec![user("a"), user("b"), user("c"), user("d")];
        assert_eq!(pair_safe_cut_index(&messages, 2), 2);
    }

    /// The naive cut would land ON the tool_results (index 2), orphaning it
    /// from its assistant tool-use message at index 1. The pair-safe cut must
    /// walk back to include the whole pair.
    #[test]
    fn cut_never_orphans_tool_results_from_their_assistant_message() {
        let (assistant, results) = tool_use_pair("call-1");
        let messages = vec![user("a"), assistant, results, user("tail")];
        assert_eq!(pair_safe_cut_index(&messages, 2), 1);
    }

    #[test]
    fn cut_after_tool_pair_is_untouched() {
        let (assistant, results) = tool_use_pair("call-1");
        let messages = vec![user("a"), assistant, results, user("tail")];
        // keep_last = 1 cuts [0, 3): the pair is entirely inside the cut.
        assert_eq!(pair_safe_cut_index(&messages, 1), 3);
    }

    #[test]
    fn cut_walks_back_to_zero_when_transcript_leads_with_pairs() {
        let (assistant, results) = tool_use_pair("call-1");
        let messages = vec![assistant, results, user("tail")];
        // Naive cut = 1 lands on the results; walking back reaches 0.
        assert_eq!(pair_safe_cut_index(&messages, 2), 0);
    }

    use crate::identity_first::{
        AgentAddressability, ContinuityGeneration, ContinuityRecord, ContinuityStore,
        DurabilityPolicy, DurableAgentSpec, IdentityLifecycleState, IdentityRuntime,
        IdentityRuntimeConfig, LeaseAcquireResult, LeaseProvider, LocalContinuityStore,
        LocalLeaseProvider, MobSessionBridge, RosterContext, RosterError, RosterProvider,
    };
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn worker_spec(identity: &AgentIdentity) -> DurableAgentSpec {
        DurableAgentSpec {
            identity: identity.clone(),
            profile: meerkat_mob::ProfileName::from("worker"),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: std::collections::BTreeMap::new(),
            context: None,
            additional_instructions: Vec::new(),
            initial_message: None,
            runtime_mode_override: None,
            backend: None,
            binding: None,
            placement: None,
        }
    }

    struct EmptyRoster;

    #[async_trait]
    impl RosterProvider for EmptyRoster {
        async fn roster(
            &self,
            _context: &RosterContext,
        ) -> Result<Vec<DurableAgentSpec>, RosterError> {
            Ok(Vec::new())
        }
    }

    /// Scripted LLM double reporting REAL input-token usage (zero usage
    /// silently disarms the compaction input trigger) with an optional gate
    /// that holds a turn in flight so the Busy refusal is deterministic.
    struct GatedUsageLlmClient {
        input_tokens: u64,
        gate_armed: Arc<AtomicBool>,
        in_call: Arc<AtomicBool>,
        release: Arc<tokio::sync::Notify>,
    }

    impl meerkat_client::LlmClient for GatedUsageLlmClient {
        fn project_replay_messages(
            &self,
            messages: &[meerkat_core::Message],
        ) -> Result<Vec<meerkat_core::Message>, meerkat_client::LlmError> {
            Ok(messages.to_vec())
        }
        fn stream<'a>(
            &'a self,
            request: &'a meerkat_client::LlmRequest,
        ) -> std::pin::Pin<
            Box<
                dyn futures::Stream<
                        Item = Result<meerkat_client::LlmEvent, meerkat_client::LlmError>,
                    > + Send
                    + 'a,
            >,
        > {
            use futures::StreamExt;
            let gate_armed = self.gate_armed.load(Ordering::SeqCst);
            let in_call = Arc::clone(&self.in_call);
            let release = Arc::clone(&self.release);
            let input_tokens = self.input_tokens;
            Box::pin(
                futures::stream::once(async move {
                    in_call.store(true, Ordering::SeqCst);
                    if gate_armed {
                        release.notified().await;
                    }
                    in_call.store(false, Ordering::SeqCst);
                    let [usage, done] =
                        crate::mob_handle_runtime::test_llm_usage::usage_then_done_with(
                            request,
                            meerkat_core::Provider::OpenAI,
                            meerkat_core::types::Usage {
                                input_tokens,
                                ..Default::default()
                            },
                            meerkat_core::types::StopReason::EndTurn,
                        );
                    futures::stream::iter(vec![
                        Ok(meerkat_client::LlmEvent::TextDelta {
                            delta: "ack".to_string(),
                            meta: None,
                        }),
                        Ok(usage),
                        Ok(done),
                    ])
                })
                .flatten(),
            )
        }
        fn provider(&self) -> meerkat_core::Provider {
            meerkat_core::Provider::OpenAI
        }
        fn health_check<'life0, 'async_trait>(
            &'life0 self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<(), meerkat_client::LlmError>>
                    + Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(()) })
        }
    }

    struct OperatorVerbHarness {
        _temp_dir: tempfile::TempDir,
        runtime: crate::UnifiedRuntime,
        concrete: Arc<meerkat_session::PersistentSessionService<meerkat::FactoryAgentBuilder>>,
        identity_runtime: Arc<IdentityRuntime>,
        floors: Arc<crate::identity_first::CompactionFloorRegistry>,
        identity: AgentIdentity,
        member_alias: String,
        gate_armed: Arc<AtomicBool>,
        in_call: Arc<AtomicBool>,
        release: Arc<tokio::sync::Notify>,
    }

    impl OperatorVerbHarness {
        fn identity_ctx(&self) -> IdentityFirstContext {
            IdentityFirstContext {
                runtime: Arc::clone(&self.identity_runtime),
                roster_provider: Arc::new(EmptyRoster),
                topology_provider: None,
                customizer: None,
                agent_memory_provider: None,
                mob_definition: Some(self.runtime.mob_handle().definition().clone()),
                transcript_edit_service: Some(Arc::clone(&self.concrete) as _),
                compaction_floors: Some(Arc::clone(&self.floors)),
            }
        }

        /// Drive one member turn to completion through the identity runtime.
        async fn run_turn(&self, text: String) {
            let admission = self
                .identity_runtime
                .send_admission_tracked(
                    &self.identity,
                    None,
                    &meerkat_core::ContentInput::Text(text),
                    meerkat_core::types::HandlingMode::Queue,
                    None,
                )
                .await
                .expect("seed turn admitted");
            self.identity_runtime
                .wait_for_completion(
                    &self.identity,
                    admission.completion_baseline,
                    Duration::from_secs(30),
                )
                .await
                .expect("seed turn completed");
        }

        async fn transcript_facts(&self) -> (usize, Option<String>) {
            let service: Arc<dyn crate::memory::hygienist::TranscriptEditSessionService> =
                Arc::clone(&self.concrete) as _;
            let session_id = self
                .identity_runtime
                .status(&self.identity)
                .await
                .expect("identity status")
                .session_id
                .expect("identity session");
            let (count, _head, last_reason) = read_transcript_facts(&service, &session_id)
                .await
                .expect("transcript facts");
            (count, last_reason)
        }

        /// `transcript_facts` count, read off a transcript that has stopped
        /// moving. Use this for any count a later assertion is COMPARED
        /// AGAINST: a baseline snapshotted mid-write drifts upward on its own,
        /// and then a "did it grow" check passes without the growth it names.
        async fn settled_transcript_count(&self) -> usize {
            let service: Arc<dyn crate::memory::hygienist::TranscriptEditSessionService> =
                Arc::clone(&self.concrete) as _;
            let session_id = self
                .identity_runtime
                .status(&self.identity)
                .await
                .expect("identity status")
                .session_id
                .expect("identity session");
            settled_transcript_len(&service, &session_id).await
        }
    }

    /// Message count on the DURABLE session surface - the one the operator
    /// verbs read, which is not the same clock as the identity's completion
    /// cursor.
    async fn transcript_len(
        service: &Arc<dyn crate::memory::hygienist::TranscriptEditSessionService>,
        session_id: &meerkat_core::types::SessionId,
    ) -> usize {
        service
            .read_history(
                session_id,
                meerkat_core::service::SessionHistoryQuery {
                    offset: 0,
                    limit: None,
                },
            )
            .await
            .expect("read durable transcript")
            .messages
            .len()
    }

    /// [`transcript_len`], but only once the durable transcript has stopped
    /// moving.
    ///
    /// A turn's completion cursor advances before the turn's rows are all
    /// durable, so a length read immediately after `run_turn` is a RACING
    /// READ. Any index computed from it is stale the moment a trailing row
    /// lands, which does not happen on an idle box and does happen under
    /// full-suite contention. Requiring the length to repeat across
    /// consecutive observations is what makes it usable as an index.
    ///
    /// The ceiling is a backstop, not a measurement: the transcript settles in
    /// milliseconds when anything is working.
    async fn settled_transcript_len(
        service: &Arc<dyn crate::memory::hygienist::TranscriptEditSessionService>,
        session_id: &meerkat_core::types::SessionId,
    ) -> usize {
        const REQUIRED_STABLE_OBSERVATIONS: usize = 3;
        let deadline = std::time::Instant::now() + Duration::from_mins(1);
        let mut settled = transcript_len(service, session_id).await;
        let mut stable = 1;
        while stable < REQUIRED_STABLE_OBSERVATIONS {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let observed = transcript_len(service, session_id).await;
            if observed == settled {
                stable += 1;
            } else {
                settled = observed;
                stable = 1;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "durable transcript never settled: still growing (last observed {observed} \
                 messages), so no index derived from it can be trusted"
            );
        }
        settled
    }

    /// Full production wiring in one process: a concrete
    /// `PersistentSessionService` backing the mob, an identity-first member
    /// bridged by the production `MobSessionBridge`, and the bridge's own
    /// compaction-floor registry shared into the RPC context - exactly the
    /// rpc_gateway composition, minus the stdio callback bridge.
    async fn operator_verb_harness(member: &str, mob_id: &str) -> OperatorVerbHarness {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let state = temp_dir.path().join("state");
        std::fs::create_dir_all(&state).expect("state dir");
        let session_store: Arc<dyn meerkat::SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(state.join("sessions.db"))
                .expect("session store"),
        );
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new(state.join("runtime.sqlite"))
                .expect("runtime store"),
        );
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(meerkat_store::MemoryBlobStore::new());
        let factory = meerkat::AgentFactory::new(&state).comms(true);
        let mut inner_builder =
            meerkat::FactoryAgentBuilder::new(factory, meerkat::Config::default());
        inner_builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
            session_store.clone(),
        )));
        inner_builder.default_blob_store = Some(blob_store.clone());
        let adapter = Arc::new(meerkat_runtime::MeerkatMachine::persistent(
            Arc::clone(&runtime_store),
            Arc::clone(&blob_store),
        ));
        let concrete = Arc::new(meerkat_session::PersistentSessionService::new(
            inner_builder,
            16,
            session_store.clone(),
            runtime_store,
            blob_store,
        ));

        let gate_armed = Arc::new(AtomicBool::new(false));
        let in_call = Arc::new(AtomicBool::new(false));
        let release = Arc::new(tokio::sync::Notify::new());
        let definition = meerkat_mob::MobDefinition::from_toml(&format!(
            r#"
[mob]
id = "{mob_id}"

[profiles.worker]
model = "gpt-5.5"

[profiles.worker.tools]
comms = true
"#
        ))
        .expect("mob definition");
        let mob_spec = crate::mob_handle_runtime::MobBootstrapSpec::new(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            concrete.clone(),
        )
        .with_session_runtime_adapter(adapter)
        .with_options(crate::mob_handle_runtime::MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(GatedUsageLlmClient {
                input_tokens: 5_000,
                gate_armed: Arc::clone(&gate_armed),
                in_call: Arc::clone(&in_call),
                release: Arc::clone(&release),
            })),
        });
        let mut runtime = crate::UnifiedRuntime::bootstrap(
            mob_spec,
            crate::MobKitConfig {
                modules: vec![],
                discovery: crate::DiscoverySpec {
                    namespace: mob_id.to_string(),
                    modules: vec![],
                },
                pre_spawn: vec![],
            },
            Duration::from_secs(5),
        )
        .await
        .expect("bootstrap unified runtime");
        let handle = runtime.mob_handle();

        // Identity-first member: the roster id is the mk--encoded generated
        // runtime alias, the shape HomeCore fleets address. The
        // `agent_identity` label is what the identity health monitor uses to
        // map this member's RunCompleted events onto the durable identity's
        // completion cursor.
        // `member` is the DURABLE identity now, not a generated `rt:{id}:{gen}`
        // alias: the roster identity is the durable identity's comms-safe
        // encoding, and AgentRuntimeId is incarnation detail. This used to
        // receive an alias and derive the durable identity back out of it.
        let roster_id = crate::member_comms_id::mob_member_id_str(member).into_owned();
        let roster_identity = meerkat_mob::ids::AgentIdentity::from(roster_id.clone());
        let durable_identity = crate::identity_first::AgentIdentity::parse(member)
            .expect("member argument must be a durable identity");
        let mut member_labels = std::collections::BTreeMap::new();
        member_labels.insert(
            "agent_identity".to_string(),
            durable_identity.as_str().to_string(),
        );
        handle
            .ensure_member(
                meerkat_mob::SpawnMemberSpec::new(
                    meerkat_mob::ProfileName::from("worker"),
                    roster_identity.clone(),
                )
                .with_labels(member_labels),
            )
            .await
            .expect("spawn identity-first member");
        handle
            .wait_for_members_kickoff_complete(
                std::slice::from_ref(&roster_identity),
                Some(Duration::from_secs(5)),
            )
            .await
            .expect("member kickoff settled");
        let member_session = handle
            .resolve_bridge_session_id_observation(&roster_identity)
            .await
            .expect("member session id");

        // Durable identity authority over that member, bridged by the
        // production MobSessionBridge (the gateway wiring).
        let public_member_alias =
            crate::member_comms_id::runtime_alias_str(&roster_id).into_owned();
        let identity = durable_identity;
        // The roster identity and the runtime BINDING are different things now.
        // The roster row is the durable identity's encoding; the binding is a
        // generated `rt:{identity}:{generation}` incarnation. This harness needs
        // both, and used to conflate them because they were the same string.
        let runtime_alias = format!("rt:{}:0", identity.as_str());
        let continuity_store =
            Arc::new(LocalContinuityStore::in_memory().expect("continuity store"));
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let lease_results = LeaseProvider::acquire_leases(
            lease_provider.as_ref(),
            std::slice::from_ref(&identity),
            "operator-verb-test",
        )
        .await
        .expect("acquire identity lease");
        let lease = match lease_results.get(&identity) {
            Some(LeaseAcquireResult::Acquired(lease)) => lease.clone(),
            other => panic!("expected acquired identity lease, got {other:?}"),
        };
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: crate::identity_first::AgentRuntimeId::parse(&runtime_alias)
                .expect("runtime alias"),
            session_id: member_session,
            generation: ContinuityGeneration::new(0),
            checkpoint_version: crate::identity_first::CheckpointVersion::new(0),
        };
        ContinuityStore::upsert_continuity_record(
            continuity_store.as_ref(),
            &record,
            lease.fencing_token,
        )
        .await
        .expect("persist identity continuity");
        let bridge = MobSessionBridge::with_session_service(handle.clone(), concrete.clone());
        let floors = bridge.compaction_floors();
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store,
            lease_provider,
            runtime_instance_id: "operator-verb-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(Arc::new(bridge)),
            default_timeout: None,
        }));
        identity_runtime
            .register(
                worker_spec(&identity),
                IdentityLifecycleState::Active,
                Some(record),
                Some(lease),
            )
            .await;

        // Install the identity authority on the unified runtime so its
        // identity health monitor drives the completion cursor from member
        // RunCompleted events (`wait_for_completion` moves on nothing else).
        runtime.attach_identity_first_context(Arc::new(
            crate::identity_first::IdentityFirstRuntimeContext::new(
                Arc::clone(&identity_runtime),
                Arc::new(EmptyRoster),
                None,
                None,
                Some(handle.definition().clone()),
            ),
        ));

        OperatorVerbHarness {
            _temp_dir: temp_dir,
            runtime,
            concrete,
            identity_runtime,
            floors,
            identity,
            member_alias: public_member_alias,
            gate_armed,
            in_call,
            release,
        }
    }

    async fn rpc(harness: &OperatorVerbHarness, method: &str, params: Value) -> Value {
        let ctx = harness.identity_ctx();
        let raw = handle_unified_rpc_json(
            &harness.runtime,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params,
            })
            .to_string(),
            Duration::from_mins(1),
            None,
            Some(&ctx),
        )
        .await;
        serde_json::from_str(&raw).expect("json-rpc response")
    }

    /// End-to-end `mobkit/compact_member`: seed a member transcript past the
    /// floor, invoke the verb, and require (a) a compaction-semantic rewrite
    /// landed (messages_after < messages_before), (b) the floor registry is
    /// disarmed afterwards, and (c) the restored build does NOT keep
    /// compacting (the compact-every-turn hazard the restore step exists to
    /// prevent).
    ///
    /// What this proves with the scripted keyless LLM double: the full arm ->
    /// respawn -> forced pre-LLM compaction -> disarm -> restore-respawn loop
    /// over the production bridge/service wiring, with the double reporting
    /// real input-token usage (5k > the 256-token floor) so the provider
    /// input trigger fires exactly as it would in production. It does NOT
    /// prove real-model summary quality.
    #[tokio::test(flavor = "multi_thread")]
    async fn compact_member_forces_one_compaction_and_restores_the_profile() {
        let harness = operator_verb_harness("worker:main", "operator-compact-verb").await;

        // Seed past both the floor and the recent-turn budget (4 turns) so
        // compaction has an older region to summarize away.
        let fat = "seeded transcript ballast ".repeat(160);
        for turn in 0..8 {
            harness.run_turn(format!("turn {turn}: {fat}")).await;
        }
        // Settled: this is a LOWER BOUND on a durable surface that lags the
        // completion cursor, so an unsettled read fails at `>= 16` for a
        // transcript that is merely still landing rather than one that is short.
        let messages_before = harness.settled_transcript_count().await;
        assert!(
            messages_before >= 16,
            "seed must materialize a fat transcript, got {messages_before}"
        );

        let response = rpc(
            &harness,
            "mobkit/compact_member",
            serde_json::json!({
                "identity": harness.member_alias,
                "floor_tokens": 256,
                "timeout_ms": 30_000,
            }),
        )
        .await;
        assert!(
            response["error"].is_null(),
            "compact_member must succeed: {response:#?}"
        );
        let result = &response["result"];
        assert_eq!(
            result["compaction_applied"],
            Value::Bool(true),
            "{result:#?}"
        );
        let reported_before = result["messages_before"].as_u64().expect("messages_before");
        let reported_after = result["messages_after"].as_u64().expect("messages_after");
        assert!(
            reported_after < reported_before,
            "compaction must shrink the transcript: {result:#?}"
        );
        assert!(
            result["last_rewrite_reason"]
                .as_str()
                .is_some_and(|reason| reason.to_lowercase().contains("compact")),
            "the landed rewrite must be compaction-semantic: {result:#?}"
        );
        assert!(
            harness.floors.get(&harness.identity).is_none(),
            "the floor registry must be disarmed after the verb"
        );

        // Restore evidence: the post-verb build must be back on the original
        // profile threshold. The recent turns alone (~5k reported input
        // tokens) are far past the 256-token floor, so a still-armed floor
        // would compact again on this very turn.
        // Both reads are on the DURABLE session surface, and `run_turn` returns
        // on the identity's COMPLETION CURSOR, which leads it (same clock split
        // documented on `transcript_len` and already fixed once in the sibling
        // timeout test). Sampling the surface once, at cursor timing, made this
        // read the append before it landed and report `3 -> 3` - a lagging
        // write and a wrongly re-compacting build are indistinguishable in a
        // single sample. Baseline is settled so it cannot drift upward on its
        // own and satisfy the comparison without an append; the growth itself
        // is then polled under the structural backstop, so a build that really
        // does re-compact never satisfies it and fails naming what never
        // happened.
        let count_after_verb = harness.settled_transcript_count().await;
        harness.run_turn("post-verb probe turn".to_string()).await;
        crate::test_wait::poll_until(
            &format!(
                "the restored build appended to the durable transcript instead of re-compacting \
                 (still {count_after_verb} messages)"
            ),
            crate::test_wait::STRUCTURAL_BACKSTOP,
            async || harness.transcript_facts().await.0 > count_after_verb,
        )
        .await;

        // Authorization shape: an identity the gateway does not own is a
        // typed refusal, not a fall-through.
        let response = rpc(
            &harness,
            "mobkit/compact_member",
            serde_json::json!({ "identity": "nobody:here" }),
        )
        .await;
        assert_eq!(
            response["error"]["code"],
            serde_json::json!(-32001),
            "an unowned identity must surface the typed unknown-identity refusal: {response:#?}"
        );

        let _ = harness.runtime.mob_handle().stop().await;
    }

    /// Honest timeout semantics: when the maintenance-turn wait gives up, the
    /// error must state what actually happened to the turn - the rollback
    /// rebuild's retire quiesces it, or, when the rollback did not land, it
    /// may still be running on the floored build - instead of implying the
    /// timeout stopped anything. The member must also come back usable: the
    /// rollback rebuild restores the original profile threshold.
    ///
    /// Only the invariants that hold on BOTH branches are asserted. Which
    /// branch runs depends on wall-clock progress the test does not control,
    /// and the optional compaction-evidence clause is dropped whenever its
    /// own bounded read cannot answer.
    #[tokio::test(flavor = "multi_thread")]
    async fn compact_member_timeout_names_the_turn_fate_and_restores_the_member() {
        let harness = operator_verb_harness("worker:main", "operator-compact-timeout").await;
        let fat = "seeded transcript ballast ".repeat(160);
        for turn in 0..4 {
            harness.run_turn(format!("turn {turn}: {fat}")).await;
        }

        // A 1ms wait cannot outlast a real bridge respawn + turn: the wait
        // times out while the verb's machinery is still working.
        let response = rpc(
            &harness,
            "mobkit/compact_member",
            serde_json::json!({
                "identity": harness.member_alias,
                "floor_tokens": 256,
                "timeout_ms": 1,
            }),
        )
        .await;
        let message = response["error"]["message"]
            .as_str()
            .unwrap_or_else(|| panic!("timeout must surface a typed error: {response:#?}"));
        assert!(
            message.contains("did not complete within 1ms"),
            "the error must name the exhausted deadline: {message}"
        );
        assert!(
            message.contains("quiesced by the rollback rebuild")
                || message.contains("may still be running on the floored build"),
            "the error must state the in-flight turn's actual fate: {message}"
        );
        assert!(
            !message.contains("did not complete: "),
            "the pre-fix message shape (bare wait error, no turn fate) must be gone: {message}"
        );
        assert!(
            harness.floors.get(&harness.identity).is_none(),
            "the floor registry must be disarmed after a timed-out verb"
        );

        // The member must be usable after the rollback: a probe turn appends.
        //
        // Read off the clock the assertion actually reads. `run_turn` returns on
        // the identity's COMPLETION CURSOR; this reads the DURABLE session
        // surface, which lags it. Sampling once at cursor timing cannot tell a
        // write that has not landed from a member that never accepted the turn -
        // both render as no growth, which is how this failed CI at `3 -> 3`.
        // Third instance of this split in this file: the seeded-length read and
        // the sibling forced-compaction probe were both fixed the same way.
        let count_before_probe = harness.settled_transcript_count().await;
        harness
            .run_turn("post-timeout probe turn".to_string())
            .await;
        crate::test_wait::poll_until(
            &format!(
                "the rolled-back member accepted a turn and appended to the durable transcript \
                 (still {count_before_probe} messages)"
            ),
            crate::test_wait::STRUCTURAL_BACKSTOP,
            async || harness.transcript_facts().await.0 > count_before_probe,
        )
        .await;

        let _ = harness.runtime.mob_handle().stop().await;
    }

    /// `mobkit/bound_member_transcript` on an idle member session whose tool
    /// pair straddles the naive cut point: the commit must succeed with the
    /// pair kept whole, and the resulting transcript must start with the
    /// operator marker followed by the intact pair.
    #[tokio::test(flavor = "multi_thread")]
    async fn bound_member_transcript_commits_a_pair_safe_cut() {
        let harness = operator_verb_harness("worker:main", "operator-bound-verb").await;
        let service: Arc<dyn crate::memory::hygienist::TranscriptEditSessionService> =
            Arc::clone(&harness.concrete) as _;

        // Commit one ordinary turn so the session is materialized/idle, then
        // seed the tool-pair fixture through the SAME audited edit surface
        // the verb uses (append at the end; the whole-transcript shape
        // validation admits the adjacent pair). The turn just finished, so a
        // still-draining runtime admission can answer Busy briefly; that is
        // the documented posture, retried here rather than raced.
        harness
            .run_turn("seed one committed turn".to_string())
            .await;
        let session_id = harness
            .identity_runtime
            .status(&harness.identity)
            .await
            .expect("identity status")
            .session_id
            .expect("identity session");
        // Every index below is derived from this length, so it must be read
        // off a SETTLED transcript. `run_turn` waits on the identity's
        // completion cursor, which advances when the turn completes - not when
        // the turn's rows are durable in the session store, which is the
        // surface both this rewrite and the verb read. Snapshotting on the
        // cursor's timing is a racing read: under full-suite contention two
        // further rows landed between the snapshot and the rewrite and every
        // derived index was wrong (`removed` 7 against an expected 4, 1-of-5
        // full-suite runs). Quiesce on the durable surface instead.
        let seeded_len = settled_transcript_len(&service, &session_id).await;
        let (assistant, results) = tool_use_pair("call-straddle");
        let fixture = vec![user("older"), assistant, results, user("tail")];
        let fixture_len = fixture.len();
        // The turn just finished, so a still-draining runtime admission can
        // answer Busy briefly; that is the documented posture, retried here
        // rather than raced. Kept tight deliberately: the refusal is TRUE when
        // it happens, so a longer wait would only delay a correct answer and
        // hide the mechanism behind it.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let request = meerkat_core::service::SessionTranscriptRewriteRequest {
                selection: meerkat_core::TranscriptRewriteSelection::MessageRange {
                    start: seeded_len,
                    end: seeded_len,
                },
                replacement: fixture.clone(),
                reason: meerkat_core::TranscriptRewriteReason::new("test_seed"),
                actor: Some("operator-verb-test".to_string()),
                expected_parent_revision: None,
                running_behavior: meerkat_core::TranscriptEditRunningBehavior::default(),
            };
            match service
                .rewrite_session_transcript(&session_id, request)
                .await
            {
                Ok(_) => break,
                Err(SessionError::Busy { .. }) if std::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(err) => panic!("fixture seed rewrite failed: {err}"),
            }
        }

        // Transcript is now [..seeded_len ordinary rows, older, assistant,
        // results, tail] with the results row at index seeded_len + 2.
        // keep_last = 2 naively cuts at len - 2 = seeded_len + 2, which IS
        // the tool_results row; the pair-safe cut walks back one to keep the
        // pair whole (removed = seeded_len + 1, kept = 3).
        //
        // Pin that premise before spending it on `removed` below. If anything
        // else reached the transcript, this fails naming the race rather than
        // surfacing later as unexplained index arithmetic.
        let staged_len = transcript_len(&service, &session_id).await;
        assert_eq!(
            staged_len,
            seeded_len + fixture_len,
            "the seeded fixture must be the whole of the transcript growth: expected the \
             {seeded_len} settled rows plus the {fixture_len} fixture rows, saw {staged_len}. A \
             different count means rows landed alongside the fixture and every index below is \
             derived from a stale premise."
        );
        let response = rpc(
            &harness,
            "mobkit/bound_member_transcript",
            serde_json::json!({
                "identity": harness.member_alias,
                "keep_last": 2,
            }),
        )
        .await;
        assert!(
            response["error"].is_null(),
            "bound_member_transcript must succeed on an idle session: {response:#?}"
        );
        let result = &response["result"];
        assert_eq!(result["bounded"], Value::Bool(true), "{result:#?}");
        assert_eq!(
            result["removed"],
            serde_json::json!(seeded_len + 1),
            "the cut must walk back off the tool_results row: {result:#?}"
        );
        assert!(result["revision"].as_str().is_some(), "{result:#?}");

        let page = service
            .read_history(
                &session_id,
                meerkat_core::service::SessionHistoryQuery {
                    offset: 0,
                    limit: None,
                },
            )
            .await
            .expect("read bounded transcript");
        assert_eq!(page.messages.len(), 4, "marker + intact pair + tail");
        assert!(
            matches!(page.messages[0], Message::SystemNotice(_)),
            "bounded transcript must lead with the operator marker"
        );
        assert!(
            matches!(page.messages[1], Message::BlockAssistant(_))
                && matches!(page.messages[2], Message::ToolResults { .. }),
            "the straddled tool pair must survive whole"
        );

        let _ = harness.runtime.mob_handle().stop().await;
    }

    /// `mobkit/bound_member_transcript` while the member is mid-turn: the
    /// service's `SessionError::Busy` surfaces as the verb's typed refusal.
    #[tokio::test(flavor = "multi_thread")]
    async fn bound_member_transcript_refuses_running_sessions_typed() {
        let harness = operator_verb_harness("worker:main", "operator-bound-busy").await;

        // Commit one ordinary turn first so the transcript has messages past
        // keep_last = 1 (a zero-length transcript would no-op before ever
        // reaching the rewrite's Busy check).
        harness
            .run_turn("seed one committed turn".to_string())
            .await;

        // Arm the gate, then hold one member turn in flight inside the LLM
        // call so the session's active runtime admission is observably held.
        harness.gate_armed.store(true, Ordering::SeqCst);
        let admission = harness
            .identity_runtime
            .send_admission_tracked(
                &harness.identity,
                None,
                &meerkat_core::ContentInput::Text("held turn".to_string()),
                meerkat_core::types::HandlingMode::Queue,
                None,
            )
            .await
            .expect("held turn admitted");
        // Backstop only: the admitted turn either reaches the gated LLM call or
        // it never will. Generous so full-suite CPU starvation cannot reach it.
        let deadline = std::time::Instant::now() + Duration::from_mins(1);
        while !harness.in_call.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < deadline,
                "the held turn never reached the LLM call: the gate was armed and the turn was \
                 admitted, but `in_call` was never set"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let response = rpc(
            &harness,
            "mobkit/bound_member_transcript",
            serde_json::json!({
                "identity": harness.member_alias,
                "keep_last": 1,
            }),
        )
        .await;
        assert_eq!(
            response["error"]["code"],
            serde_json::json!(OPERATOR_SESSION_BUSY_CODE),
            "a running session must surface the typed Busy refusal: {response:#?}"
        );
        assert!(
            response["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("quiesce")),
            "the refusal must document quiesce-first: {response:#?}"
        );

        // Release the held turn so shutdown is clean.
        harness.gate_armed.store(false, Ordering::SeqCst);
        harness.release.notify_waiters();
        harness
            .identity_runtime
            .wait_for_completion(
                &harness.identity,
                admission.completion_baseline,
                Duration::from_secs(30),
            )
            .await
            .expect("held turn completed after release");

        let _ = harness.runtime.mob_handle().stop().await;
    }

    #[test]
    fn optional_u64_param_rejects_zero_and_non_integers() {
        let params = serde_json::json!({ "floor_tokens": 0 });
        assert!(optional_u64_param(&params, "floor_tokens").is_err());
        let params = serde_json::json!({ "floor_tokens": "many" });
        assert!(optional_u64_param(&params, "floor_tokens").is_err());
        let params = serde_json::json!({});
        assert_eq!(optional_u64_param(&params, "floor_tokens").unwrap(), None);
        let params = serde_json::json!({ "floor_tokens": 2048 });
        assert_eq!(
            optional_u64_param(&params, "floor_tokens").unwrap(),
            Some(2048)
        );
    }
}
