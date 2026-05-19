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
use meerkat_mobkit::identity_first::contracts::{ContinuityStore, LeaseProvider};
use meerkat_mobkit::identity_first::{
    AgentIdentity, CheckpointVersion, ContinuityGeneration, ContinuityRecord,
    ContinuityResolveState, ContinuityStoreError, FencingToken, LeaseAcquireResult, LeaseError,
    LeaseGrant, LeaseRenewResult, SessionSnapshot,
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
        .scratch_dir(tmp.path())
        .blob_store(Arc::new(meerkat_store::FsBlobStore::new(
            tmp.path().join("blobs"),
        )));
    assert_build_err_not_contains(builder, "mutually exclusive", "conflicting").await;
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
