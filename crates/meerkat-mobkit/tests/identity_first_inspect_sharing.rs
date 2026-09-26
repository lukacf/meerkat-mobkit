#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Member-status load (K1): `IdentityRuntime::inspect`, behind
//! `mobkit/inspect_identity` and `wait_for_output`, shares its bridge read.
//!
//! The bridge's inspection is a full meerkat member-status read, and a mob
//! admits one such read at a time (before meerkat 0.8.45 the rest were
//! refused as `observation_lane_saturated`). Concurrent inspections of one
//! member incarnation now join one read in flight. A settled read is never
//! reused, and a caller never joins a read that started before a completion
//! it already counts: `mobkit/inspect_identity` returns the completion
//! cursor next to the output, and SDK completion waits take the output as
//! soon as the cursor moves.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_mobkit::identity_first::contracts::{ContinuityStore, LeaseProvider};
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentBuildDraft, AgentIdentity, AgentRuntimeId, BridgeDelivery,
    BridgeError, CheckpointVersion, CompletionCursor, ContinuityGeneration, ContinuityRecord,
    DurabilityPolicy, DurableAgentSpec, FencingToken, IdentityLifecycleState, IdentityRuntime,
    IdentityRuntimeConfig, LeaseGrant, LocalContinuityStore, LocalLeaseProvider, MemberInspection,
    ResumeSessionOutcome, SessionBridge, SessionSnapshot,
};

/// Stands in for the bridge's member-status read: counts reads, takes
/// `delay` per read (the way a busy member's status read does), and reports
/// the output that was committed when the read started.
struct CountingInspectBridge {
    reads: AtomicUsize,
    delay: Duration,
    fail_next: AtomicBool,
    committed_output: std::sync::Mutex<Option<String>>,
}

impl CountingInspectBridge {
    fn new(delay: Duration) -> Self {
        Self {
            reads: AtomicUsize::new(0),
            delay,
            fail_next: AtomicBool::new(false),
            committed_output: std::sync::Mutex::new(None),
        }
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    fn commit_output(&self, output: &str) {
        *self.committed_output.lock().unwrap() = Some(output.to_string());
    }
}

#[async_trait]
impl SessionBridge for CountingInspectBridge {
    async fn create_session(
        &self,
        _identity: &AgentIdentity,
        _runtime_id: &AgentRuntimeId,
        _spec: &DurableAgentSpec,
        _draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        Ok(session_id.clone())
    }

    async fn resume_session(
        &self,
        _identity: &AgentIdentity,
        _runtime_id: &AgentRuntimeId,
        _spec: &DurableAgentSpec,
        _draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
        _snapshot: &SessionSnapshot,
    ) -> Result<ResumeSessionOutcome, BridgeError> {
        Ok(ResumeSessionOutcome::Resumed {
            session_id: session_id.clone(),
        })
    }

    async fn deliver_admitted(
        &self,
        _runtime_id: &AgentRuntimeId,
        _delivery: BridgeDelivery,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        Ok(meerkat_core::types::SessionId::new())
    }

    async fn checkpoint_session(
        &self,
        _runtime_id: &AgentRuntimeId,
        _session_id: &meerkat_core::types::SessionId,
    ) -> Result<SessionSnapshot, BridgeError> {
        Ok(SessionSnapshot { data: Vec::new() })
    }

    async fn retire_member(&self, _runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
        Ok(())
    }

    async fn inspect_member(
        &self,
        runtime_id: &AgentRuntimeId,
    ) -> Result<MemberInspection, BridgeError> {
        let read = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        let committed = self.committed_output.lock().unwrap().clone();
        tokio::time::sleep(self.delay).await;
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(BridgeError::Mob(
                "mob lifecycle operation admission is still pending at \
                 observation_lane_saturated: member_status_observation"
                    .to_string(),
            ));
        }
        Ok(MemberInspection {
            output_preview: committed.or_else(|| Some(format!("{runtime_id} read {read}"))),
            is_final: false,
            peer_reachable_count: 0,
        })
    }
}

fn make_runtime(bridge: Arc<CountingInspectBridge>) -> Arc<IdentityRuntime> {
    let store: Arc<dyn ContinuityStore> = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease: Arc<dyn LeaseProvider> = Arc::new(LocalLeaseProvider::new());
    Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store,
        lease_provider: lease,
        runtime_instance_id: "inspect-sharing-test".to_string(),
        has_runtime_store: true,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(bridge),
        default_timeout: None,
    }))
}

async fn register_active(runtime: &IdentityRuntime, name: &str) -> AgentIdentity {
    let identity = AgentIdentity::parse(name).unwrap();
    runtime
        .register(
            DurableAgentSpec {
                identity: identity.clone(),
                profile: meerkat_mob::ProfileName::from("default"),
                addressability: AgentAddressability::Addressable,
                display_name: None,
                labels: BTreeMap::new(),
                context: None,
                additional_instructions: Vec::new(),
                initial_message: None,
                runtime_mode_override: None,
                backend: None,
                binding: None,
                placement: None,
            },
            IdentityLifecycleState::Active,
            Some(ContinuityRecord {
                identity: identity.clone(),
                agent_runtime_id: AgentRuntimeId::parse(&format!("rt:{name}")).unwrap(),
                session_id: meerkat_core::types::SessionId::new(),
                generation: ContinuityGeneration::new(0),
                checkpoint_version: CheckpointVersion::new(0),
            }),
            Some(LeaseGrant {
                identity: identity.clone(),
                fencing_token: FencingToken::new(1),
                ttl: Duration::from_mins(5),
            }),
        )
        .await;
    identity
}

/// Two concurrent `inspect_identity` calls for one identity make a single
/// member-status read, and both get its result.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_inspections_of_one_identity_share_one_read() {
    let bridge = Arc::new(CountingInspectBridge::new(Duration::from_millis(300)));
    let runtime = make_runtime(Arc::clone(&bridge));
    let calendar = register_active(&runtime, "calendar").await;

    let (first, second) = tokio::join!(runtime.inspect(&calendar), runtime.inspect(&calendar));
    let (first, second) = (first.expect("first"), second.expect("second"));
    assert_eq!(bridge.reads(), 1, "one member-status read for both callers");
    assert_eq!(first.output_preview, second.output_preview);
}

/// A settled inspection is never reused: the next caller reads afresh.
#[tokio::test]
async fn a_settled_inspection_is_never_reused() {
    let bridge = Arc::new(CountingInspectBridge::new(Duration::ZERO));
    let runtime = make_runtime(Arc::clone(&bridge));
    let calendar = register_active(&runtime, "calendar").await;

    runtime.inspect(&calendar).await.expect("first read");
    runtime.inspect(&calendar).await.expect("second read");
    assert_eq!(bridge.reads(), 2);
}

/// What an SDK completion wait does against `mobkit/inspect_identity`:
/// read the cursor, then the inspection, and return the output as soon as
/// the cursor has moved past `baseline`.
async fn sdk_wait_for_completion(
    runtime: &IdentityRuntime,
    identity: &AgentIdentity,
    baseline: CompletionCursor,
) -> Option<String> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let cursor = runtime.completion_cursor(identity).await;
            let inspection = runtime.inspect(identity).await.expect("inspect");
            if cursor > baseline {
                return inspection.output_preview;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the turn completes")
}

/// Review of #451 (P1): an inspection of turn A must not go out next to
/// the cursor of a later turn B. With the old one-second reuse, an
/// inspection primed with A was returned after B completed, and an SDK
/// completion wait returned A's text for turn B.
#[tokio::test]
async fn an_inspection_after_a_completion_reports_the_new_output() {
    let bridge = Arc::new(CountingInspectBridge::new(Duration::ZERO));
    let runtime = make_runtime(Arc::clone(&bridge));
    let calendar = register_active(&runtime, "calendar").await;

    bridge.commit_output("A");
    let baseline = runtime.completion_cursor(&calendar).await;
    let primed = runtime.inspect(&calendar).await.expect("prime with A");
    assert_eq!(primed.output_preview.as_deref(), Some("A"));

    // Turn B completes well within the old reuse window.
    bridge.commit_output("B");
    let completed = runtime.record_turn_completed(&calendar).await;
    assert!(completed > baseline);

    assert_eq!(
        sdk_wait_for_completion(&runtime, &calendar, baseline)
            .await
            .as_deref(),
        Some("B")
    );
    let inspection = runtime.inspect(&calendar).await.expect("inspect");
    assert_eq!(inspection.output_preview.as_deref(), Some("B"));
    assert_eq!(runtime.completion_cursor(&calendar).await, completed);
}

/// A read still in flight when a turn completes may report the previous
/// output; a caller that already counts the new turn starts its own read
/// instead of joining it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_read_in_flight_across_a_completion_is_not_joined_by_a_later_caller() {
    let bridge = Arc::new(CountingInspectBridge::new(Duration::from_millis(300)));
    let runtime = make_runtime(Arc::clone(&bridge));
    let calendar = register_active(&runtime, "calendar").await;

    bridge.commit_output("A");
    let before = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let calendar = calendar.clone();
        async move { runtime.inspect(&calendar).await }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    bridge.commit_output("B");
    runtime.record_turn_completed(&calendar).await;

    let after = runtime.inspect(&calendar).await.expect("after the turn");
    assert_eq!(after.output_preview.as_deref(), Some("B"));
    let before = before.await.expect("join").expect("before the turn");
    assert_eq!(before.output_preview.as_deref(), Some("A"));
    assert_eq!(bridge.reads(), 2);
}

/// A failed read is shared only with the callers that waited on it; the
/// next caller reads again.
#[tokio::test]
async fn a_failed_inspection_is_not_reused() {
    let bridge = Arc::new(CountingInspectBridge::new(Duration::ZERO));
    let runtime = make_runtime(Arc::clone(&bridge));
    let calendar = register_active(&runtime, "calendar").await;

    bridge.fail_next.store(true, Ordering::SeqCst);
    assert!(runtime.inspect(&calendar).await.is_err());
    runtime.inspect(&calendar).await.expect("read again");
    assert_eq!(bridge.reads(), 2);
}

/// Different members are never shared.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inspections_of_different_identities_are_read_separately() {
    let bridge = Arc::new(CountingInspectBridge::new(Duration::from_millis(100)));
    let runtime = make_runtime(Arc::clone(&bridge));
    let calendar = register_active(&runtime, "calendar").await;
    let mail = register_active(&runtime, "mail").await;

    let (calendar_view, mail_view) =
        tokio::join!(runtime.inspect(&calendar), runtime.inspect(&mail));
    assert_eq!(bridge.reads(), 2);
    assert_ne!(
        calendar_view.expect("calendar").output_preview,
        mail_view.expect("mail").output_preview
    );
}
