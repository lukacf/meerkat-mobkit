#![allow(
    clippy::all,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    unused_imports,
    redundant_semicolons,
    clippy::redundant_clone
)]
//! Tests for identity-first builder validation (Phase 1, Tasks 1.12–1.15).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use meerkat_mobkit::identity_first::contracts::{
    ContinuityStore, LeaseProvider, RosterProvider, TopologyProvider,
};
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentIdentity, CheckpointVersion, ContinuityGeneration, ContinuityRecord,
    ContinuityResolveState, ContinuityStoreError, DurableAgentSpec, FencingToken,
    LeaseAcquireResult, LeaseError, LeaseGrant, LeaseRenewResult, LocalContinuityStore,
    ManagedPeerEdge, RosterContext, RosterError, SessionSnapshot, TopologyContext, TopologyError,
};
use meerkat_mobkit::unified_runtime::UnifiedRuntimeBuilder;

// ---------------------------------------------------------------------------
// Minimal mock implementations for builder testing
// ---------------------------------------------------------------------------

struct StubContinuityStore;

#[async_trait]
impl ContinuityStore for StubContinuityStore {
    async fn resolve_many(
        &self,
        identities: &[AgentIdentity],
    ) -> Result<BTreeMap<AgentIdentity, ContinuityResolveState>, ContinuityStoreError> {
        Ok(identities
            .iter()
            .map(|id| (id.clone(), ContinuityResolveState::Uninitialized))
            .collect())
    }
    async fn load_session_snapshot(
        &self,
        _sid: &meerkat_core::types::SessionId,
    ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
        Ok(None)
    }
    async fn save_session_snapshot(
        &self,
        _identity: &AgentIdentity,
        _sid: &meerkat_core::types::SessionId,
        _gen: ContinuityGeneration,
        _ver: CheckpointVersion,
        _ft: FencingToken,
        _snap: &SessionSnapshot,
    ) -> Result<(), ContinuityStoreError> {
        Ok(())
    }
    async fn upsert_continuity_record(
        &self,
        _record: &ContinuityRecord,
        _ft: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        Ok(())
    }
    async fn delete_continuity_record(
        &self,
        _identity: &AgentIdentity,
        _ft: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        Ok(())
    }
}

struct StubLeaseProvider;

#[async_trait]
impl LeaseProvider for StubLeaseProvider {
    async fn acquire_leases(
        &self,
        identities: &[AgentIdentity],
        _instance: &str,
    ) -> Result<BTreeMap<AgentIdentity, LeaseAcquireResult>, LeaseError> {
        Ok(identities
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    LeaseAcquireResult::Acquired(LeaseGrant {
                        identity: id.clone(),
                        fencing_token: FencingToken::new(1),
                        ttl: Duration::from_secs(30),
                    }),
                )
            })
            .collect())
    }
    async fn renew_leases(
        &self,
        _grants: &[LeaseGrant],
    ) -> Result<BTreeMap<AgentIdentity, LeaseRenewResult>, LeaseError> {
        Ok(BTreeMap::new())
    }
    async fn release_leases(&self, _grants: &[LeaseGrant]) -> Result<(), LeaseError> {
        Ok(())
    }
}

#[derive(Clone)]
struct StubRosterProvider {
    specs: Arc<tokio::sync::Mutex<Vec<DurableAgentSpec>>>,
}

impl StubRosterProvider {
    fn new(specs: Vec<DurableAgentSpec>) -> Self {
        Self {
            specs: Arc::new(tokio::sync::Mutex::new(specs)),
        }
    }
}

#[async_trait]
impl RosterProvider for StubRosterProvider {
    async fn roster(&self, _context: &RosterContext) -> Result<Vec<DurableAgentSpec>, RosterError> {
        Ok(self.specs.lock().await.clone())
    }
}

struct StubTopologyProvider {
    edges: Arc<tokio::sync::Mutex<Vec<ManagedPeerEdge>>>,
}

#[async_trait]
impl TopologyProvider for StubTopologyProvider {
    async fn compute_edges(
        &self,
        _target_identities: &[AgentIdentity],
        _context: &TopologyContext,
    ) -> Result<Vec<ManagedPeerEdge>, TopologyError> {
        Ok(self.edges.lock().await.clone())
    }
}

fn test_definition() -> meerkat_mob::MobDefinition {
    meerkat_mob::MobDefinition::from_toml(
        r#"
[mob]
id = "identity-builder-test"

[profiles.default]
model = "gpt-5.5"
runtime_mode = "turn_driven"

[profiles.default.tools]
comms = true
"#,
    )
    .unwrap()
}

fn durable_spec(identity: &str) -> DurableAgentSpec {
    DurableAgentSpec {
        identity: AgentIdentity::parse(identity).unwrap(),
        profile: meerkat_mob::ProfileName::from("default"),
        addressability: AgentAddressability::Addressable,
        display_name: None,
        labels: BTreeMap::new(),
        context: None,
        additional_instructions: Vec::new(),
        initial_message: None,
        runtime_mode_override: Some(meerkat_mob::MobRuntimeMode::TurnDriven),
    }
}

/// Helper: assert builder.build() returns Err and the error message contains the given substring.
async fn assert_build_err_contains(builder: UnifiedRuntimeBuilder, expected: &str) {
    match builder.build().await {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains(expected),
                "expected error containing {expected:?}, got: {msg}"
            );
        }
        Ok(_) => panic!("expected builder error containing {expected:?}, but build succeeded"),
    }
}

/// Helper: assert builder.build() returns Err and the error message does NOT contain either substring.
async fn assert_build_err_not_contains(builder: UnifiedRuntimeBuilder, not_a: &str, not_b: &str) {
    match builder.build().await {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                !msg.contains(not_a) && !msg.contains(not_b),
                "expected error NOT containing {not_a:?} or {not_b:?}, got: {msg}"
            );
        }
        Ok(_) => {
            // If it somehow succeeded, that's fine for this assertion
            // (the point was that it shouldn't fail with conflicting config)
        }
    }
}

// ===========================================================================
// Task 1.12: Builder persistent_state mutual exclusivity (REQ-23)
// ===========================================================================

#[tokio::test]
async fn identity_first_builder_persistent_state_conflicts_with_continuity_store() {
    let builder = UnifiedRuntimeBuilder::default()
        .persistent_state("/tmp/test-state")
        .continuity_store(Arc::new(StubContinuityStore));
    assert_build_err_contains(builder, "mutually exclusive").await;
}

#[tokio::test]
async fn identity_first_builder_persistent_state_conflicts_with_lease_provider() {
    let builder = UnifiedRuntimeBuilder::default()
        .persistent_state("/tmp/test-state")
        .lease_provider(Arc::new(StubLeaseProvider));
    assert_build_err_contains(builder, "mutually exclusive").await;
}

#[tokio::test]
async fn identity_first_builder_persistent_state_conflicts_with_scratch_dir() {
    let builder = UnifiedRuntimeBuilder::default()
        .persistent_state("/tmp/test-state")
        .scratch_dir("/tmp/test-scratch");
    assert_build_err_contains(builder, "mutually exclusive").await;
}

// ===========================================================================
// Task 1.13: Builder external path requires all three (REQ-24)
// ===========================================================================

#[tokio::test]
async fn identity_first_builder_external_path_missing_lease_and_scratch() {
    let builder = UnifiedRuntimeBuilder::default().continuity_store(Arc::new(StubContinuityStore));
    assert_build_err_contains(builder, "lease_provider").await;
}

#[tokio::test]
async fn identity_first_builder_external_path_missing_continuity_store() {
    let builder = UnifiedRuntimeBuilder::default()
        .lease_provider(Arc::new(StubLeaseProvider))
        .scratch_dir("/tmp/test-scratch");
    assert_build_err_contains(builder, "continuity_store").await;
}

#[tokio::test]
async fn identity_first_builder_external_path_missing_scratch_dir() {
    let builder = UnifiedRuntimeBuilder::default()
        .continuity_store(Arc::new(StubContinuityStore))
        .lease_provider(Arc::new(StubLeaseProvider));
    assert_build_err_contains(builder, "scratch_dir").await;
}

#[tokio::test]
async fn identity_first_builder_identity_first_optional_setters_require_core_providers() {
    let builder = UnifiedRuntimeBuilder::default()
        .topology_provider(Arc::new(StubTopologyProvider {
            edges: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }))
        .identity_runtime_instance_id("builder-test");
    assert_build_err_contains(builder, "roster_provider").await;
}

#[tokio::test]
async fn identity_first_builder_blob_store_accepted_with_persistent_state() {
    // blob_store should be accepted alongside persistent_state without
    // triggering a conflict error. The build will fail later (missing definition).
    let tmp = tempfile::tempdir().unwrap();
    let builder = UnifiedRuntimeBuilder::default()
        .persistent_state(tmp.path())
        .blob_store(Arc::new(meerkat_store::FsBlobStore::new(
            tmp.path().join("blobs"),
        )));
    assert_build_err_not_contains(builder, "mutually exclusive", "conflicting").await;
}

#[tokio::test]
async fn identity_first_builder_blob_store_accepted_with_external_path() {
    let tmp = tempfile::tempdir().unwrap();
    let builder = UnifiedRuntimeBuilder::default()
        .continuity_store(Arc::new(StubContinuityStore))
        .lease_provider(Arc::new(StubLeaseProvider))
        .roster_provider(Arc::new(StubRosterProvider::new(vec![])))
        .scratch_dir(tmp.path())
        .blob_store(Arc::new(meerkat_store::FsBlobStore::new(
            tmp.path().join("blobs"),
        )));
    assert_build_err_not_contains(builder, "mutually exclusive", "conflicting").await;
}

#[tokio::test]
async fn identity_first_builder_bootstraps_and_exposes_identity_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let roster = Arc::new(StubRosterProvider::new(vec![durable_spec("agent:alpha")]));

    let runtime = UnifiedRuntimeBuilder::default()
        .definition(test_definition())
        .continuity_store(Arc::new(StubContinuityStore))
        .lease_provider(Arc::new(StubLeaseProvider))
        .roster_provider(roster)
        .scratch_dir(tmp.path())
        .identity_runtime_instance_id("builder-test")
        .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
        .build()
        .await
        .expect("builder should bootstrap identity-first runtime");

    let identity_runtime = runtime
        .identity_runtime()
        .expect("identity runtime should be exposed");
    let status = identity_runtime
        .status(&AgentIdentity::parse("agent:alpha").unwrap())
        .await
        .expect("identity should be active");
    assert_eq!(status.profile.unwrap().as_str(), "default");
    assert_eq!(
        status.runtime_mode,
        Some(meerkat_mob::MobRuntimeMode::TurnDriven)
    );
}

#[tokio::test]
async fn identity_first_builder_runtime_checkpoint_follows_initial_session_save_version() {
    let tmp = tempfile::tempdir().unwrap();
    let roster = Arc::new(StubRosterProvider::new(vec![durable_spec("agent:alpha")]));
    let continuity_store = Arc::new(LocalContinuityStore::in_memory().unwrap());

    let runtime = UnifiedRuntimeBuilder::default()
        .definition(test_definition())
        .continuity_store(continuity_store)
        .lease_provider(Arc::new(StubLeaseProvider))
        .roster_provider(roster)
        .scratch_dir(tmp.path())
        .identity_runtime_instance_id("builder-checkpoint-test")
        .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
        .build()
        .await
        .expect("builder should bootstrap identity-first runtime");

    let identity = AgentIdentity::parse("agent:alpha").unwrap();
    let identity_runtime = runtime
        .identity_runtime()
        .expect("identity runtime should be exposed");
    let status = identity_runtime
        .status(&identity)
        .await
        .expect("identity should be active");
    assert!(
        status
            .checkpoint_version
            .is_some_and(|version| version.get() >= 1),
        "initial session save should advance the live checkpoint version"
    );

    let next_version = identity_runtime
        .checkpoint(
            &identity,
            &SessionSnapshot {
                data: b"builder checkpoint".to_vec(),
            },
        )
        .await
        .expect("checkpoint after bootstrap should not be stale");
    assert!(next_version.get() >= 2);
}

#[tokio::test]
async fn identity_first_builder_resume_checkpoint_follows_registered_session_save_version() {
    let continuity_store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let identity = AgentIdentity::parse("agent:alpha").unwrap();

    {
        let tmp = tempfile::tempdir().unwrap();
        let roster = Arc::new(StubRosterProvider::new(vec![durable_spec("agent:alpha")]));
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .continuity_store(continuity_store.clone())
            .lease_provider(Arc::new(StubLeaseProvider))
            .roster_provider(roster)
            .scratch_dir(tmp.path())
            .identity_runtime_instance_id("builder-resume-seed")
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build()
            .await
            .expect("seed runtime should create continuity");
    }

    let tmp = tempfile::tempdir().unwrap();
    let roster = Arc::new(StubRosterProvider::new(vec![durable_spec("agent:alpha")]));
    let runtime = UnifiedRuntimeBuilder::default()
        .definition(test_definition())
        .continuity_store(continuity_store)
        .lease_provider(Arc::new(StubLeaseProvider))
        .roster_provider(roster)
        .scratch_dir(tmp.path())
        .identity_runtime_instance_id("builder-resume-test")
        .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
        .build()
        .await
        .expect("builder should resume identity-first runtime");

    let identity_runtime = runtime
        .identity_runtime()
        .expect("identity runtime should be exposed");
    let status = identity_runtime
        .status(&identity)
        .await
        .expect("identity should be active after resume");
    let restored_version = status
        .checkpoint_version
        .expect("resume should expose a checkpoint version")
        .get();
    assert!(
        restored_version >= 1,
        "resume should inherit any registered session save version"
    );

    let next_version = identity_runtime
        .checkpoint(
            &identity,
            &SessionSnapshot {
                data: b"resume checkpoint".to_vec(),
            },
        )
        .await
        .expect("checkpoint after resume should not be stale");
    assert!(next_version.get() > restored_version);
}

#[tokio::test]
async fn identity_first_builder_refreshes_desired_topology_from_providers() {
    let tmp = tempfile::tempdir().unwrap();
    let roster = Arc::new(StubRosterProvider::new(vec![
        durable_spec("agent:alpha"),
        durable_spec("agent:beta"),
    ]));
    let edges = Arc::new(tokio::sync::Mutex::new(vec![
        ManagedPeerEdge::new(
            AgentIdentity::parse("agent:alpha").unwrap(),
            AgentIdentity::parse("agent:beta").unwrap(),
        )
        .unwrap(),
    ]));

    let runtime = UnifiedRuntimeBuilder::default()
        .definition(test_definition())
        .continuity_store(Arc::new(StubContinuityStore))
        .lease_provider(Arc::new(StubLeaseProvider))
        .roster_provider(roster)
        .topology_provider(Arc::new(StubTopologyProvider {
            edges: edges.clone(),
        }))
        .scratch_dir(tmp.path())
        .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
        .build()
        .await
        .expect("builder should bootstrap identity-first runtime");

    edges.lock().await.clear();
    let result = runtime
        .refresh_desired_topology()
        .await
        .expect("refresh should succeed")
        .expect("identity-first context should be present");
    assert!(result.managed_edges.is_empty());
}

// ===========================================================================
// Task 1.15: Legacy surface preservation (REQ-31)
// ===========================================================================

#[test]
fn identity_first_builder_legacy_types_still_compile() {
    // AgentDiscoverySpec is still usable
    let _spec = meerkat_mobkit::types::AgentDiscoverySpec {
        profile: "default".to_string(),
        meerkat_id: "agent-1".to_string(),
        labels: None,
        context: None,
        additional_instructions: vec![],
        resume_session_id: None,
    };

    // DiscoverySpec is still usable
    let _disc = meerkat_mobkit::types::DiscoverySpec {
        namespace: "test".to_string(),
        modules: vec![],
    };
}

#[test]
fn identity_first_builder_legacy_adapter_still_works() {
    let spec = meerkat_mobkit::types::AgentDiscoverySpec {
        profile: "default".to_string(),
        meerkat_id: "triage:main".to_string(),
        labels: None,
        context: None,
        additional_instructions: vec![],
        resume_session_id: Some("old-session-123".to_string()),
    };
    let durable = meerkat_mobkit::identity_first::agent_discovery_to_durable(&spec).unwrap();
    assert_eq!(durable.identity.as_str(), "triage:main");
}
