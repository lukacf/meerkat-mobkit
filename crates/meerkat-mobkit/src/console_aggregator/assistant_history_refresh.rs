//! Runtime-owned permission to reconcile absent assistant occurrences.

use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use meerkat_mob::MemberRunState;
use serde_json::Value;

use super::{ConsoleIdentityRecord, RuntimeEntry};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum AssistantHistoryRefreshReason {
    #[default]
    PositiveOnly,
    Recovery,
    SessionBoundary {
        session_id: String,
    },
}

impl AssistantHistoryRefreshReason {
    pub(super) fn permits_session(&self, session_id: &str) -> bool {
        match self {
            Self::PositiveOnly => false,
            Self::Recovery => !session_id.is_empty(),
            Self::SessionBoundary {
                session_id: expected,
            } => expected == session_id,
        }
    }

    pub(super) fn from_event(kind: &str, session_id: Option<&str>, payload: &Value) -> Self {
        let Some(session_id) = session_id.filter(|value| !value.is_empty()) else {
            return Self::PositiveOnly;
        };
        let run_completed = kind == "run_completed"
            || (kind == "interaction_complete"
                && (payload.get("source_event_type").and_then(Value::as_str)
                    == Some("run_completed")
                    || payload.get("type").and_then(Value::as_str) == Some("run_completed")));
        let final_completion = run_completed
            && matches!(
                payload.get("extraction_required"),
                None | Some(Value::Bool(false))
            );
        if final_completion
            || matches!(
                kind,
                "run_failed"
                    | "interaction_failed"
                    | "stream_truncated"
                    | "extraction_succeeded"
                    | "extraction_failed"
                    | "transcript_rewrite_committed"
                    | "transcript_rewrite_audit_receipt_committed"
                    | "compaction_completed"
            )
        {
            Self::SessionBoundary {
                session_id: session_id.to_string(),
            }
        } else {
            Self::PositiveOnly
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AssistantHistoryRefreshGate {
    PositiveOnly,
    Pending,
    Settled,
}

/// Coalesce a recovery behind an active runtime-wide poll. Ordinary polling
/// cannot erase an explicit recovery or turn a burst into one job per trigger.
pub(super) fn admit_runtime_refresh(
    active: &mut BTreeMap<String, Option<AssistantHistoryRefreshReason>>,
    runtime_key: &str,
    reason: AssistantHistoryRefreshReason,
) -> bool {
    match active.entry(runtime_key.to_string()) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(None);
            true
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            if reason == AssistantHistoryRefreshReason::Recovery {
                entry.insert(Some(reason));
            }
            false
        }
    }
}

/// Call after capturing the console prefix and before reading current history.
/// Even a terminal trigger checks current runtime progress: another run may
/// have started before its queued backfill acquired the projection lock.
pub(super) async fn observe(
    entry: &RuntimeEntry,
    record: &ConsoleIdentityRecord,
    session_id: &str,
    requested: bool,
) -> AssistantHistoryRefreshGate {
    let member =
        crate::member_comms_id::roster_member_id_for_supplied_id(&record.runtime_member_id);
    let handle = entry.runtime.handle();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    observe_with(
        requested,
        session_id,
        || async {
            let status = crate::member_status_observation::observe_member_status_until(
                &handle, &member, deadline,
            )
            .await
            .ok()?;
            Some((
                status.current_session_id?.to_string(),
                status.progress?.run_state,
            ))
        },
        || async {
            tokio::time::timeout_at(
                deadline,
                entry.runtime.live_transcript_awaits_commit(session_id),
            )
            .await
            .ok()
            .flatten()
        },
    )
    .await
}

async fn observe_with<StatusRead, StatusFuture, CommitRead, CommitFuture>(
    requested: bool,
    session_id: &str,
    read_status: StatusRead,
    read_commit: CommitRead,
) -> AssistantHistoryRefreshGate
where
    StatusRead: FnOnce() -> StatusFuture,
    StatusFuture: Future<Output = Option<(String, MemberRunState)>>,
    CommitRead: FnOnce() -> CommitFuture,
    CommitFuture: Future<Output = Option<bool>>,
{
    if !requested {
        return AssistantHistoryRefreshGate::PositiveOnly;
    }
    let Some((current_session, MemberRunState::Idle)) = read_status().await else {
        return AssistantHistoryRefreshGate::Pending;
    };
    if current_session != session_id || read_commit().await != Some(false) {
        return AssistantHistoryRefreshGate::Pending;
    }
    AssistantHistoryRefreshGate::Settled
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::future::ready;

    use super::*;

    #[test]
    fn eligibility_requires_an_explicit_same_session_boundary() {
        let recovery = AssistantHistoryRefreshReason::Recovery;
        let boundary = AssistantHistoryRefreshReason::SessionBoundary {
            session_id: "session-a".into(),
        };
        assert!(recovery.permits_session("session-a"));
        assert!(boundary.permits_session("session-a"));
        assert!(!boundary.permits_session("session-b"));
        assert!(!AssistantHistoryRefreshReason::PositiveOnly.permits_session("session-a"));
    }

    #[test]
    fn runtime_recovery_is_queued_once_when_a_poll_already_owns_the_worker() {
        let mut active = std::collections::BTreeMap::new();
        assert!(admit_runtime_refresh(
            &mut active,
            "runtime",
            AssistantHistoryRefreshReason::PositiveOnly
        ));
        assert!(!admit_runtime_refresh(
            &mut active,
            "runtime",
            AssistantHistoryRefreshReason::Recovery
        ));
        for _ in 0..32 {
            assert!(!admit_runtime_refresh(
                &mut active,
                "runtime",
                AssistantHistoryRefreshReason::PositiveOnly
            ));
        }
        assert_eq!(active.len(), 1);
        assert_eq!(
            active.get_mut("runtime").and_then(Option::take),
            Some(AssistantHistoryRefreshReason::Recovery)
        );
        assert_eq!(active.get_mut("runtime").and_then(Option::take), None);
    }

    #[test]
    fn completion_requires_final_extraction_and_cannot_be_inferred_from_text_events() {
        let reason = |kind, payload| {
            AssistantHistoryRefreshReason::from_event(kind, Some("session-a"), &payload)
        };
        assert!(reason("interaction_complete", serde_json::json!({"source_event_type":"run_completed", "extraction_required":false})).permits_session("session-a"));
        assert!(
            !reason(
                "interaction_complete",
                serde_json::json!({"source_event_type":"run_completed", "extraction_required":true})
            )
            .permits_session("session-a")
        );
        assert!(!reason("interaction_complete", serde_json::json!({"source_event_type":"run_completed", "extraction_required":"false"})).permits_session("session-a"));
        for kind in [
            "extraction_succeeded",
            "extraction_failed",
            "run_failed",
            "stream_truncated",
            "transcript_rewrite_committed",
            "compaction_completed",
        ] {
            assert!(
                reason(kind, serde_json::json!({})).permits_session("session-a"),
                "{kind}"
            );
        }
        for kind in [
            "text_complete",
            "turn_completed",
            "turn_started",
            "text_delta",
        ] {
            assert!(
                !reason(kind, serde_json::json!({})).permits_session("session-a"),
                "{kind}"
            );
        }
        assert!(
            !AssistantHistoryRefreshReason::from_event("run_failed", None, &serde_json::json!({}))
                .permits_session("session-a")
        );
    }

    #[tokio::test]
    async fn opportunistic_reads_do_not_probe_or_authorize_absence() {
        let calls = RefCell::new(Vec::new());
        let gate = observe_with(
            false,
            "session-a",
            || {
                calls.borrow_mut().push("status");
                ready(Some(("session-a".into(), MemberRunState::Idle)))
            },
            || {
                calls.borrow_mut().push("commit");
                ready(Some(false))
            },
        )
        .await;
        assert_eq!(gate, AssistantHistoryRefreshGate::PositiveOnly);
        assert!(calls.borrow().is_empty());
    }

    #[tokio::test]
    async fn active_successor_unknown_or_replaced_session_never_authorizes_absence() {
        for status in [
            None,
            Some(("session-a".into(), MemberRunState::RunOpen)),
            Some(("session-a".into(), MemberRunState::Unknown)),
            Some(("session-b".into(), MemberRunState::Idle)),
        ] {
            let calls = RefCell::new(Vec::new());
            let gate = observe_with(
                true,
                "session-a",
                || {
                    calls.borrow_mut().push("status");
                    ready(status)
                },
                || {
                    calls.borrow_mut().push("commit");
                    ready(Some(false))
                },
            )
            .await;
            assert_eq!(gate, AssistantHistoryRefreshGate::Pending);
            assert_eq!(*calls.borrow(), ["status"]);
        }
    }

    #[tokio::test]
    async fn idle_status_precedes_commit_check_and_pending_observations_remain_retryable() {
        for pending in [Some(true), None, Some(false)] {
            let calls = RefCell::new(Vec::new());
            let gate = observe_with(
                true,
                "session-a",
                || {
                    calls.borrow_mut().push("status");
                    ready(Some(("session-a".into(), MemberRunState::Idle)))
                },
                || {
                    calls.borrow_mut().push("commit");
                    ready(pending)
                },
            )
            .await;
            assert_eq!(*calls.borrow(), ["status", "commit"]);
            assert_eq!(
                gate,
                if pending == Some(false) {
                    AssistantHistoryRefreshGate::Settled
                } else {
                    AssistantHistoryRefreshGate::Pending
                }
            );
        }
    }
}
