#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Member-status load (K1): `IdentityRuntime::inspect`, behind
//! `mobkit/inspect_identity` and `wait_for_output`, shares its bridge read.
//!
//! The bridge's inspection is a full meerkat member-status read, and a mob
//! admits one such read at a time (the rest are refused as
//! `observation_lane_saturated`). A once-a-second `inspect_identity` poller
//! kept that lane busy. Concurrent inspections of one member incarnation now
//! share one read, and a successful result is reused for about a second.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_mobkit::identity_first::contracts::{ContinuityStore, LeaseProvider};
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentBuildDraft, AgentIdentity, AgentRuntimeId, BridgeDelivery,
    BridgeError, CheckpointVersion, ContinuityGeneration, ContinuityRecord, DurabilityPolicy,
    DurableAgentSpec, FencingToken, IdentityLifecycleState, IdentityRuntime, IdentityRuntimeConfig,
    LeaseGrant, LocalContinuityStore, LocalLeaseProvider, MemberInspection, ResumeSessionOutcome,
    SessionBridge, SessionSnapshot,
};

/// Stands in for the bridge's member-status read: counts reads and takes
/// `delay` per read, the way a busy member's status read does.
struct CountingInspectBridge {
    reads: AtomicUsize,
    delay: Duration,
    fail_next: AtomicBool,
}

impl CountingInspectBridge {
    fn new(delay: Duration) -> Self {
        Self {
            reads: AtomicUsize::new(0),
            delay,
            fail_next: AtomicBool::new(false),
        }
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
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
        tokio::time::sleep(self.delay).await;
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(BridgeError::Mob(
                "mob lifecycle operation admission is still pending at \
                 observation_lane_saturated: member_status_observation"
                    .to_string(),
            ));
        }
        Ok(MemberInspection {
            output_preview: Some(format!("{runtime_id} read {read}")),
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

/// A successful inspection is reused for about a second, then read again.
#[tokio::test]
async fn a_successful_inspection_is_reused_briefly_then_read_again() {
    let bridge = Arc::new(CountingInspectBridge::new(Duration::ZERO));
    let runtime = make_runtime(Arc::clone(&bridge));
    let calendar = register_active(&runtime, "calendar").await;

    runtime.inspect(&calendar).await.expect("first read");
    runtime.inspect(&calendar).await.expect("reused");
    assert_eq!(bridge.reads(), 1);
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    runtime.inspect(&calendar).await.expect("fresh read");
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
