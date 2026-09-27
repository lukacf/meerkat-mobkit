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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    observe_with(
        requested,
        session_id,
        || async {
            tokio::time::timeout_at(deadline, async {
                let expected_session = meerkat_core::SessionId::parse(session_id).ok()?;
                let primary = entry.runtime.handle();
                let (handle, member) = if primary.resolve_bridge_session_id(&member).await
                    == Some(expected_session.clone())
                {
                    (primary, member)
                } else {
                    // Alias and source labels cannot distinguish identical
                    // member names in different mobs. Recover the handle from
                    // the existing member resolver and its runtime binding.
                    let mut owner = None;
                    for resolved in
                        Box::pin(super::member_sources_for_entry_including_hidden(entry)).await
                    {
                        if resolved.member.agent_identity != member
                            && resolved.runtime_identity != record.runtime_member_id
                        {
                            continue;
                        }
                        if resolved
                            .handle
                            .resolve_bridge_session_id(&resolved.member.agent_identity)
                            .await
                            != Some(expected_session.clone())
                        {
                            continue;
                        }
                        if owner.is_some() {
                            return None;
                        }
                        owner = Some((resolved.handle, resolved.member.agent_identity));
                    }
                    owner?
                };
                // The binding only routes the observation. Fresh typed status
                // must still prove that this exact session is idle.
                let status = crate::member_status_observation::observe_member_status_until(
                    &handle, &member, deadline,
                )
                .await
                .ok()?;
                Some((
                    status.current_session_id?.to_string(),
                    status.progress?.run_state,
                ))
            })
            .await
            .ok()
            .flatten()
        },
        || async {
            tokio::time::timeout_at(deadline, entry.runtime.session_commit_pending(session_id))
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
    #[allow(clippy::expect_used)]
    async fn secondary_mob_refresh_uses_the_owner_of_the_exact_session() {
        let definition = meerkat_mob::MobDefinition::from_toml(&format!(
            r#"
[mob]
id = "refresh-primary-{}"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true

[profiles.worker.tools]
comms = true
mob = true
"#,
            uuid::Uuid::new_v4()
        ))
        .expect("primary definition parses");
        // The convenience builder composes the runtime-backed session service
        // and installs agent mob tools over that same authority.
        let runtime = crate::unified_runtime::UnifiedRuntime::builder()
            .definition(definition)
            .default_llm_client(std::sync::Arc::new(
                meerkat_client::TestClient::for_provider(meerkat_core::Provider::OpenAI),
            ))
            .build()
            .await
            .expect("runtime with agent mob control builds");
        runtime
            .spawn(meerkat_mob::SpawnMemberSpec::from_wire(
                "worker".into(),
                "agent-0".into(),
                Some("Reply that the primary is ready.".into()),
                None,
                None,
            ))
            .await
            .expect("primary member spawns");
        let entry = super::super::tests::runtime_entry_for_test("refresh-owner", &runtime);
        let primary = entry.runtime.handle();
        let state = entry
            .runtime
            .agent_mob_mcp_state()
            .expect("runtime exposes shared mob control");
        let service = entry
            .runtime
            .session_service()
            .expect("runtime exposes its session service");
        assert!(
            std::sync::Arc::ptr_eq(
                &service.runtime_adapter().expect("parent runtime machine"),
                &state
                    .session_service()
                    .runtime_adapter()
                    .expect("child runtime machine"),
            ),
            "agent-created mobs must share the parent's real runtime authority"
        );
        let definition = meerkat_mob::MobDefinition::from_toml(&format!(
            r#"
[mob]
id = "refresh-child-{}"

[profiles.worker]
realm_profile = "worker"
"#,
            uuid::Uuid::new_v4()
        ))
        .expect("child definition parses");
        let mob_id = Box::pin(state.mob_create_definition(definition))
            .await
            .expect("child shares the runtime session service");
        let member = crate::member_comms_id::roster_member_id_for_identity("agent-0");
        Box::pin(state.mob_spawn_spec(
            &mob_id,
            meerkat_mob::SpawnMemberSpec::from_wire(
                "worker".into(),
                "agent-0".into(),
                Some("Reply that the child is ready.".into()),
                None,
                None,
            ),
        ))
        .await
        .expect("child member spawns with the primary member's alias");
        let child = state
            .handle_for(&mob_id)
            .await
            .expect("child handle exists");
        let row = child
            .list_members_observation_snapshot()
            .await
            .into_iter()
            .find(|row| row.agent_identity == member)
            .expect("child member is in its owner's roster");
        let mut record = super::super::identity_record_for_member(&entry, &child, &row)
            .await
            .expect("child identity projects");
        let session_id = record.session_id.clone().expect("child session exists");
        let typed_session = meerkat_core::SessionId::parse(&session_id)
            .expect("child session is a typed runtime session");
        let primary_session = primary
            .resolve_bridge_session_id_observation(&member)
            .await
            .expect("primary member with the same alias has its own session");
        assert_ne!(
            primary_session, typed_session,
            "the same alias in the primary mob must not select the child session"
        );
        // Source labels are display metadata, not authority to select a mob.
        record
            .labels
            .insert("source_mob_id".into(), primary.mob_id().to_string());

        // Spawn only acknowledges admission. Require a terminal run witness
        // and the real shared commit authority before asserting settlement.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        tokio::time::timeout_at(deadline, async {
            loop {
                let execution = service
                    .execution_snapshot(&typed_session)
                    .await
                    .expect("shared service observes the child session");
                let status = crate::member_status_observation::observe_member_status_until(
                    &child, &member, deadline,
                )
                .await
                .expect("child status is observable");
                if execution.as_ref().is_some_and(|snapshot| {
                    snapshot.turn_terminal && snapshot.terminal_run_id.is_some()
                }) && status.current_session_id.as_ref() == Some(&typed_session)
                    && status.progress.as_ref().is_some_and(|progress| {
                        progress.run_state == MemberRunState::Idle && progress.in_flight_work == 0
                    })
                    && entry.runtime.session_commit_pending(&session_id).await == Some(false)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("child's initial run commits before refreshing its history");

        assert_eq!(
            observe(&entry, &record, &session_id, true).await,
            AssistantHistoryRefreshGate::Settled,
            "a settled secondary member must authorize its own history refresh"
        );
        state
            .mob_destroy(&mob_id)
            .await
            .expect("child is destroyed");
        assert_eq!(
            observe(&entry, &record, &session_id, true).await,
            AssistantHistoryRefreshGate::Pending,
            "the surviving primary alias cannot authorize the removed child's session"
        );
        primary.stop().await.expect("primary fixture stops");
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
