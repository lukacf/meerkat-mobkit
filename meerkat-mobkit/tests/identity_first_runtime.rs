#![allow(
    clippy::all,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    unused_imports,
    redundant_semicolons,
    clippy::redundant_clone
)]
//! Tests for identity-first runtime behavior (Phase 2).
//!
//! Test naming convention: `identity_first_runtime_<feature>_<scenario>`
//! to match the `test(identity_first)` filter.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_mobkit::identity_first::contracts::{
    AgentCustomizer, ContinuityStore, LeaseProvider, TopologyProvider,
};
use meerkat_mobkit::identity_first::orchestrator::{
    ReconcileAction, RestoreOutcome, compute_reconcile_actions, lazy_register_flow, restore_flow,
};
use meerkat_mobkit::identity_first::runtime::IdentityEvent;
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentBuildContext, AgentBuildDraft, AgentIdentity, AgentRuntimeId,
    BridgeError, CheckpointVersion, ContinuityFailure, ContinuityFailureKind, ContinuityGeneration,
    ContinuityRecord, ContinuityResolveState, ContinuityStoreError, CustomizerError,
    DispatchIdempotencyKey, DispatchInput, DispatchOrigin, DurabilityPolicy, DurableAgentSpec,
    FencingToken, IdentityLifecycleState, IdentityRuntime, IdentityRuntimeConfig,
    IdentityRuntimeError, LeaseAcquireResult, LeaseGrant, ManagedPeerEdge, SessionBridge,
    SessionSnapshot, TopologyContext, TopologyError,
};
use meerkat_mobkit::identity_first::{LocalContinuityStore, LocalLeaseProvider};

// ===========================================================================
// Helpers
// ===========================================================================

fn make_identity(name: &str) -> AgentIdentity {
    AgentIdentity::parse(name).unwrap()
}

fn make_spec(name: &str) -> DurableAgentSpec {
    DurableAgentSpec {
        identity: make_identity(name),
        profile: meerkat_mob::ProfileName::from("default"),
        addressability: AgentAddressability::Addressable,
        display_name: None,
        labels: BTreeMap::new(),
        context: None,
        additional_instructions: Vec::new(),
        initial_message: None,
        runtime_mode_override: None,
    }
}

fn make_internal_spec(name: &str) -> DurableAgentSpec {
    DurableAgentSpec {
        addressability: AgentAddressability::InternalOnly,
        ..make_spec(name)
    }
}

fn make_record(name: &str, generation: u64, cpv: u64) -> ContinuityRecord {
    ContinuityRecord {
        identity: make_identity(name),
        agent_runtime_id: AgentRuntimeId::parse(&format!("rt:{name}:{generation}")).unwrap(),
        session_id: meerkat_core::types::SessionId::new(),
        generation: ContinuityGeneration::new(generation),
        checkpoint_version: CheckpointVersion::new(cpv),
    }
}

fn make_grant(name: &str, token: u64) -> LeaseGrant {
    LeaseGrant {
        identity: make_identity(name),
        fencing_token: FencingToken::new(token),
        ttl: Duration::from_mins(5),
    }
}

fn make_runtime(store: Arc<dyn ContinuityStore>, lease: Arc<dyn LeaseProvider>) -> IdentityRuntime {
    IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store,
        lease_provider: lease,
        runtime_instance_id: "test-runtime".to_string(),
        has_runtime_store: false,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: None,
        default_timeout: None,
    })
}

fn make_runtime_with_store(
    store: Arc<dyn ContinuityStore>,
    lease: Arc<dyn LeaseProvider>,
) -> IdentityRuntime {
    IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store,
        lease_provider: lease,
        runtime_instance_id: "test-runtime".to_string(),
        has_runtime_store: true,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: None,
        default_timeout: None,
    })
}

fn make_runtime_with_bridge(
    store: Arc<dyn ContinuityStore>,
    lease: Arc<dyn LeaseProvider>,
    bridge: Arc<dyn SessionBridge>,
) -> Arc<IdentityRuntime> {
    Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store,
        lease_provider: lease,
        runtime_instance_id: "test-runtime".to_string(),
        has_runtime_store: true,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(bridge),
        default_timeout: None,
    }))
}

struct CountingContinuityStore {
    inner: Arc<LocalContinuityStore>,
    resolve_many_calls: AtomicUsize,
    load_snapshot_calls: AtomicUsize,
}

impl CountingContinuityStore {
    fn new() -> Self {
        Self {
            inner: Arc::new(LocalContinuityStore::in_memory().unwrap()),
            resolve_many_calls: AtomicUsize::new(0),
            load_snapshot_calls: AtomicUsize::new(0),
        }
    }

    fn reset_counts(&self) {
        self.resolve_many_calls.store(0, Ordering::SeqCst);
        self.load_snapshot_calls.store(0, Ordering::SeqCst);
    }

    fn resolve_many_calls(&self) -> usize {
        self.resolve_many_calls.load(Ordering::SeqCst)
    }

    fn load_snapshot_calls(&self) -> usize {
        self.load_snapshot_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ContinuityStore for CountingContinuityStore {
    async fn resolve_many(
        &self,
        identities: &[AgentIdentity],
    ) -> Result<BTreeMap<AgentIdentity, ContinuityResolveState>, ContinuityStoreError> {
        self.resolve_many_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.resolve_many(identities).await
    }

    async fn load_session_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
        self.load_snapshot_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.load_session_snapshot(session_id).await
    }

    async fn save_session_snapshot(
        &self,
        identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        generation: ContinuityGeneration,
        version: CheckpointVersion,
        fencing_token: FencingToken,
        snapshot: &SessionSnapshot,
    ) -> Result<(), ContinuityStoreError> {
        self.inner
            .save_session_snapshot(
                identity,
                session_id,
                generation,
                version,
                fencing_token,
                snapshot,
            )
            .await
    }

    async fn upsert_continuity_record(
        &self,
        record: &ContinuityRecord,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        self.inner
            .upsert_continuity_record(record, fencing_token)
            .await
    }

    async fn delete_continuity_record(
        &self,
        identity: &AgentIdentity,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        self.inner
            .delete_continuity_record(identity, fencing_token)
            .await
    }
}

#[derive(Default)]
struct CountingBridge {
    create_calls: AtomicUsize,
    resume_calls: AtomicUsize,
    deliver_calls: AtomicUsize,
    force_resume_fallback: AtomicBool,
    resume_delay: tokio::sync::Mutex<Option<Duration>>,
    fallback_session_id: tokio::sync::Mutex<Option<meerkat_core::types::SessionId>>,
}

impl CountingBridge {
    async fn set_resume_delay(&self, delay: Duration) {
        *self.resume_delay.lock().await = Some(delay);
    }

    async fn set_force_resume_fallback(&self, session_id: meerkat_core::types::SessionId) {
        self.force_resume_fallback.store(true, Ordering::SeqCst);
        *self.fallback_session_id.lock().await = Some(session_id);
    }
}

#[async_trait]
impl SessionBridge for CountingBridge {
    async fn create_session(
        &self,
        _identity: &AgentIdentity,
        _runtime_id: &AgentRuntimeId,
        _spec: &DurableAgentSpec,
        _draft: &AgentBuildDraft,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        self.create_calls.fetch_add(1, Ordering::SeqCst);
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
    ) -> Result<meerkat_mobkit::identity_first::ResumeSessionOutcome, BridgeError> {
        self.resume_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(delay) = *self.resume_delay.lock().await {
            tokio::time::sleep(delay).await;
        }
        if self.force_resume_fallback.load(Ordering::SeqCst) {
            let fallback_session_id = self
                .fallback_session_id
                .lock()
                .await
                .clone()
                .unwrap_or_else(meerkat_core::types::SessionId::new);
            return Ok(
                meerkat_mobkit::identity_first::ResumeSessionOutcome::FreshSpawned {
                    session_id: fallback_session_id,
                    reason:
                        meerkat_mobkit::identity_first::ResumeFallbackReason::RuntimeIdentityIncompatible {
                            detail: "test mismatch".to_string(),
                        },
                },
            );
        }
        Ok(
            meerkat_mobkit::identity_first::ResumeSessionOutcome::Resumed {
                session_id: session_id.clone(),
            },
        )
    }

    async fn deliver(
        &self,
        _runtime_id: &AgentRuntimeId,
        _content: &meerkat_core::ContentInput,
    ) -> Result<meerkat_core::types::SessionId, BridgeError> {
        self.deliver_calls.fetch_add(1, Ordering::SeqCst);
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
}

fn make_content() -> meerkat_core::ContentInput {
    meerkat_core::ContentInput::Text("hello".to_string())
}

fn make_dispatch_input() -> DispatchInput {
    DispatchInput {
        content: make_content(),
        origin: DispatchOrigin::Connector,
        correlation_id: None,
        idempotency_key: None,
    }
}

// ===========================================================================
// Task 2.1 — send(identity, content) with addressability enforcement (REQ-01, REQ-03)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_send_to_addressable_delivers() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let spec = make_spec("triage:main");
    let record = make_record("triage:main", 0, 0);
    let grant = make_grant("triage:main", 1);

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record),
            Some(grant),
        )
        .await;

    let result = runtime
        .send(&make_identity("triage:main"), &make_content())
        .await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), FencingToken::new(1));
}

#[tokio::test]
async fn identity_first_runtime_send_to_internal_only_rejected() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let spec = make_internal_spec("gate:main");
    let record = make_record("gate:main", 0, 0);
    let grant = make_grant("gate:main", 1);

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record),
            Some(grant),
        )
        .await;

    let result = runtime
        .send(&make_identity("gate:main"), &make_content())
        .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        IdentityRuntimeError::NotAddressable(err) => {
            assert_eq!(err.identity, make_identity("gate:main"));
            assert_eq!(err.addressability, AgentAddressability::InternalOnly);
        }
        other => panic!("expected NotAddressable, got: {other:?}"),
    }
}

#[tokio::test]
async fn identity_first_runtime_send_to_unknown_identity_rejected() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let result = runtime
        .send(&make_identity("nonexistent:main"), &make_content())
        .await;
    assert!(matches!(
        result.unwrap_err(),
        IdentityRuntimeError::UnknownIdentity(_)
    ));
}

// ===========================================================================
// Task 2.2 — dispatch(identity, dispatch_input) (REQ-02)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_dispatch_to_addressable_succeeds() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let spec = make_spec("triage:main");
    let record = make_record("triage:main", 0, 0);
    let grant = make_grant("triage:main", 1);

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record),
            Some(grant),
        )
        .await;

    let result = runtime
        .dispatch(&make_identity("triage:main"), &make_dispatch_input())
        .await;
    assert!(result.is_ok());
    let (token, is_durable) = result.unwrap();
    assert_eq!(token, FencingToken::new(1));
    assert!(!is_durable); // no runtime_store
}

#[tokio::test]
async fn identity_first_runtime_dispatch_to_internal_only_succeeds() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let spec = make_internal_spec("gate:main");
    let record = make_record("gate:main", 0, 0);
    let grant = make_grant("gate:main", 1);

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record),
            Some(grant),
        )
        .await;

    let result = runtime
        .dispatch(&make_identity("gate:main"), &make_dispatch_input())
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn identity_first_runtime_dispatch_with_fields_flows_through() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let spec = make_spec("triage:main");
    let record = make_record("triage:main", 0, 0);
    let grant = make_grant("triage:main", 1);

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record),
            Some(grant),
        )
        .await;

    let input = DispatchInput {
        content: make_content(),
        origin: DispatchOrigin::Policy,
        correlation_id: Some(meerkat_mobkit::identity_first::CorrelationId::new("corr-1")),
        idempotency_key: Some(DispatchIdempotencyKey::new("idem-1")),
    };

    let result = runtime
        .dispatch(&make_identity("triage:main"), &input)
        .await;
    assert!(result.is_ok());
}

// ===========================================================================
// Task 2.3 — dispatch() durability contract (REQ-04)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_dispatch_without_runtime_store_is_in_memory() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease); // has_runtime_store = false

    let spec = make_spec("triage:main");
    let record = make_record("triage:main", 0, 0);
    let grant = make_grant("triage:main", 1);

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record),
            Some(grant),
        )
        .await;

    let (_, is_durable) = runtime
        .dispatch(&make_identity("triage:main"), &make_dispatch_input())
        .await
        .unwrap();
    assert!(!is_durable);
}

#[tokio::test]
async fn identity_first_runtime_dispatch_with_runtime_store_is_durable() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime_with_store(store, lease); // has_runtime_store = true

    let spec = make_spec("triage:main");
    let record = make_record("triage:main", 0, 0);
    let grant = make_grant("triage:main", 1);

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record),
            Some(grant),
        )
        .await;

    let (_, is_durable) = runtime
        .dispatch(&make_identity("triage:main"), &make_dispatch_input())
        .await
        .unwrap();
    assert!(is_durable);
}

// ===========================================================================
// Task 2.4 — runtime.agent(identity) (REQ-05)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_agent_returns_for_active() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let spec = make_spec("triage:main");
    let record = make_record("triage:main", 0, 0);
    let grant = make_grant("triage:main", 1);

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record),
            Some(grant),
        )
        .await;

    assert!(runtime.contains(&make_identity("triage:main")).await);
}

#[tokio::test]
async fn identity_first_runtime_agent_errors_for_unknown() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let result = runtime.status(&make_identity("nonexistent:main")).await;
    assert!(matches!(
        result.unwrap_err(),
        IdentityRuntimeError::UnknownIdentity(_)
    ));
}

// ===========================================================================
// Task 2.6 — runtime.status(identity) (REQ-07)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_status_returns_full_identity_status() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let mut spec = make_spec("triage:main");
    spec.labels.insert("env".to_string(), "staging".to_string());
    let record = make_record("triage:main", 1, 5);
    let grant = make_grant("triage:main", 3);

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record.clone()),
            Some(grant),
        )
        .await;

    let status = runtime.status(&make_identity("triage:main")).await.unwrap();
    assert_eq!(status.identity, make_identity("triage:main"));
    assert_eq!(status.state, IdentityLifecycleState::Active);
    assert_eq!(status.agent_runtime_id, Some(record.agent_runtime_id));
    assert_eq!(status.session_id, Some(record.session_id));
    assert_eq!(
        status.profile,
        Some(meerkat_mob::ProfileName::from("default"))
    );
    assert_eq!(status.addressability, AgentAddressability::Addressable);
    assert_eq!(status.labels.get("env"), Some(&"staging".to_string()));
    assert_eq!(status.generation, Some(ContinuityGeneration::new(1)));
    assert_eq!(status.checkpoint_version, Some(CheckpointVersion::new(5)));

    // Lease info
    let lease_info = status.lease.unwrap();
    assert_eq!(lease_info.fencing_token, FencingToken::new(3));
    assert!(lease_info.healthy);

    // Continuity health
    let health = status.continuity_health.unwrap();
    assert!(health.store_reachable);
    assert_eq!(health.durability_policy, DurabilityPolicy::SyncWriteThrough);
    assert_eq!(
        health.last_checkpoint_version,
        Some(CheckpointVersion::new(5))
    );
}

// ===========================================================================
// Task 2.7 — runtime.retire(identity) (REQ-08)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_retire_validates_lease_and_sets_retiring() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let spec = make_spec("triage:main");
    let record = make_record("triage:main", 0, 0);
    let grant = make_grant("triage:main", 1);

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record),
            Some(grant),
        )
        .await;

    let token = runtime.retire(&make_identity("triage:main")).await.unwrap();
    assert_eq!(token, FencingToken::new(1));

    let status = runtime.status(&make_identity("triage:main")).await.unwrap();
    assert_eq!(status.state, IdentityLifecycleState::Retiring);
}

// ===========================================================================
// Task 2.8 — respawn(identity) (REQ-09)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_respawn_preserves_record_and_generation() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    let id = make_identity("triage:main");

    // Pre-seed the continuity record in the store
    let record = make_record("triage:main", 0, 2);
    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();

    // Register with an initial lease
    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(record.clone()),
            Some(make_grant("triage:main", 1)),
        )
        .await;

    // Respawn
    let restored = runtime.respawn(&id).await.unwrap();
    assert_eq!(restored.identity, id);
    assert_eq!(restored.generation, ContinuityGeneration::new(0)); // NOT advanced
    assert_eq!(restored.agent_runtime_id, record.agent_runtime_id); // same runtime ID

    // Should be Active again
    let status = runtime.status(&id).await.unwrap();
    assert_eq!(status.state, IdentityLifecycleState::Active);
}

// ===========================================================================
// Task 2.9 — reset(identity) (REQ-10)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_reset_advances_generation_creates_fresh() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    let id = make_identity("triage:main");

    // Acquire a real lease through the provider so internal state is consistent
    let initial_grants = lease_prov
        .acquire_leases(&[id.clone()], "test-runtime")
        .await
        .unwrap();
    let initial_grant = match initial_grants.get(&id).unwrap() {
        LeaseAcquireResult::Acquired(g) => g.clone(),
        _ => panic!("expected Acquired"),
    };

    // Pre-seed the continuity record with the real fencing token
    let record = make_record("triage:main", 0, 5);
    store
        .upsert_continuity_record(&record, initial_grant.fencing_token)
        .await
        .unwrap();

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(record.clone()),
            Some(initial_grant.clone()),
        )
        .await;

    let new_record = runtime.reset(&id).await.unwrap();
    assert_eq!(new_record.identity, id);
    assert_eq!(new_record.generation, ContinuityGeneration::new(1)); // advanced
    assert_eq!(new_record.checkpoint_version, CheckpointVersion::new(0)); // fresh
    assert_ne!(new_record.session_id, record.session_id); // new session

    // Old-owner late writes should be rejected by stale fencing token.
    // The reset acquired a new lease with a higher token, so the old token is stale.
    let old_write = store
        .save_session_snapshot(
            &id,
            &record.session_id,
            record.generation,
            CheckpointVersion::new(6),
            initial_grant.fencing_token, // old token
            &SessionSnapshot {
                data: b"stale".to_vec(),
            },
        )
        .await;
    assert!(old_write.is_err());
}

// ===========================================================================
// Task 2.10 — delete_identity(identity) (REQ-11)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_delete_identity_removes_from_runtime() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    let id = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);
    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(record),
            Some(make_grant("triage:main", 1)),
        )
        .await;

    runtime.delete_identity(&id).await.unwrap();
    assert!(!runtime.contains(&id).await);

    // Re-bootstrap: same identity treated as Uninitialized
    let resolved = store.resolve_many(&[id.clone()]).await.unwrap();
    assert_eq!(
        resolved.get(&id).unwrap(),
        &ContinuityResolveState::Uninitialized,
        "deleted identity must resolve as Uninitialized"
    );
}

// ===========================================================================
// Lazy materialization — build registers identity metadata, first touch hydrates
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_lazy_register_does_not_load_snapshots_or_spawn_members() {
    let store = Arc::new(CountingContinuityStore::new());
    let lease = Arc::new(LocalLeaseProvider::new());
    let bridge = Arc::new(CountingBridge::default());
    let runtime = make_runtime_with_bridge(store.clone(), lease, bridge.clone());

    let roster = (0..1_000)
        .map(|index| {
            let name = format!("agent:{index}");
            let record = make_record(&name, 0, 1);
            let store = store.clone();
            async move {
                store
                    .upsert_continuity_record(&record, FencingToken::new(1))
                    .await
                    .unwrap();
                make_spec(&name)
            }
        })
        .collect::<Vec<_>>();
    let roster = futures::future::join_all(roster).await;
    store.reset_counts();

    let result = lazy_register_flow(&runtime, &roster, None).await.unwrap();

    assert_eq!(result.outcomes.len(), 1_000);
    assert_eq!(store.resolve_many_calls(), 1);
    assert_eq!(store.load_snapshot_calls(), 0);
    assert_eq!(bridge.create_calls.load(Ordering::SeqCst), 0);
    assert_eq!(bridge.resume_calls.load(Ordering::SeqCst), 0);
    let status = runtime.status(&make_identity("agent:42")).await.unwrap();
    assert_eq!(status.state, IdentityLifecycleState::Dormant);
    assert_eq!(
        status.agent_runtime_id,
        Some(AgentRuntimeId::parse("rt:agent:42:0").unwrap())
    );
}

#[tokio::test]
async fn identity_first_runtime_lazy_first_send_materializes_only_target_once() {
    let store = Arc::new(CountingContinuityStore::new());
    let lease = Arc::new(LocalLeaseProvider::new());
    let bridge = Arc::new(CountingBridge::default());
    let runtime = make_runtime_with_bridge(store.clone(), lease, bridge.clone());

    for name in ["agent:a", "agent:b"] {
        let record = make_record(name, 0, 1);
        store
            .upsert_continuity_record(&record, FencingToken::new(1))
            .await
            .unwrap();
        store
            .save_session_snapshot(
                &make_identity(name),
                &record.session_id,
                record.generation,
                CheckpointVersion::new(2),
                FencingToken::new(1),
                &SessionSnapshot {
                    data: format!("snapshot-{name}").into_bytes(),
                },
            )
            .await
            .unwrap();
    }
    let roster = vec![make_spec("agent:a"), make_spec("agent:b")];
    lazy_register_flow(&runtime, &roster, None).await.unwrap();
    store.reset_counts();

    runtime
        .send(&make_identity("agent:a"), &make_content())
        .await
        .unwrap();

    assert_eq!(store.load_snapshot_calls(), 1);
    assert_eq!(bridge.resume_calls.load(Ordering::SeqCst), 1);
    assert_eq!(bridge.create_calls.load(Ordering::SeqCst), 0);
    assert_eq!(bridge.deliver_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime
            .status(&make_identity("agent:a"))
            .await
            .unwrap()
            .state,
        IdentityLifecycleState::Active
    );
    assert_eq!(
        runtime
            .status(&make_identity("agent:b"))
            .await
            .unwrap()
            .state,
        IdentityLifecycleState::Dormant
    );
}

#[tokio::test]
async fn identity_first_runtime_parallel_sends_to_dormant_identity_coalesce_materialization() {
    let store = Arc::new(CountingContinuityStore::new());
    let lease = Arc::new(LocalLeaseProvider::new());
    let bridge = Arc::new(CountingBridge::default());
    bridge.set_resume_delay(Duration::from_millis(50)).await;
    let runtime = make_runtime_with_bridge(store.clone(), lease, bridge.clone());

    let record = make_record("agent:coalesce", 0, 1);
    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();
    lazy_register_flow(&runtime, &[make_spec("agent:coalesce")], None)
        .await
        .unwrap();
    store.reset_counts();

    let mut tasks = Vec::new();
    for _ in 0..10 {
        let runtime = runtime.clone();
        tasks.push(tokio::spawn(async move {
            runtime
                .send(&make_identity("agent:coalesce"), &make_content())
                .await
                .unwrap();
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }

    assert_eq!(store.load_snapshot_calls(), 1);
    assert_eq!(bridge.resume_calls.load(Ordering::SeqCst), 1);
    assert_eq!(bridge.deliver_calls.load(Ordering::SeqCst), 10);
    assert_eq!(
        runtime
            .status(&make_identity("agent:coalesce"))
            .await
            .unwrap()
            .state,
        IdentityLifecycleState::Active
    );
}

#[tokio::test]
async fn identity_first_runtime_lazy_dispatch_materializes_internal_only_identity() {
    let store = Arc::new(CountingContinuityStore::new());
    let lease = Arc::new(LocalLeaseProvider::new());
    let bridge = Arc::new(CountingBridge::default());
    let runtime = make_runtime_with_bridge(store.clone(), lease, bridge.clone());

    lazy_register_flow(&runtime, &[make_internal_spec("agent:internal")], None)
        .await
        .unwrap();
    runtime
        .dispatch(&make_identity("agent:internal"), &make_dispatch_input())
        .await
        .unwrap();

    assert_eq!(bridge.create_calls.load(Ordering::SeqCst), 1);
    assert_eq!(bridge.deliver_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime
            .status(&make_identity("agent:internal"))
            .await
            .unwrap()
            .state,
        IdentityLifecycleState::Active
    );
}

#[tokio::test]
async fn identity_first_runtime_lazy_resume_incompatible_fallback_is_typed_and_visible() {
    let store = Arc::new(CountingContinuityStore::new());
    let lease = Arc::new(LocalLeaseProvider::new());
    let bridge = Arc::new(CountingBridge::default());
    let fallback_session_id = meerkat_core::types::SessionId::new();
    bridge
        .set_force_resume_fallback(fallback_session_id.clone())
        .await;
    let runtime = make_runtime_with_bridge(store.clone(), lease, bridge.clone());

    let record = make_record("agent:legacy", 0, 1);
    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();
    lazy_register_flow(&runtime, &[make_spec("agent:legacy")], None)
        .await
        .unwrap();
    let mut events = runtime
        .subscribe(&make_identity("agent:legacy"))
        .await
        .unwrap();

    runtime
        .send(&make_identity("agent:legacy"), &make_content())
        .await
        .unwrap();

    let event = events.recv().await.unwrap();
    match event {
        IdentityEvent::ResumeFallback { identity, reason } => {
            assert_eq!(identity, make_identity("agent:legacy"));
            assert!(matches!(
                reason,
                meerkat_mobkit::identity_first::ResumeFallbackReason::RuntimeIdentityIncompatible { .. }
            ));
        }
        other => panic!("expected ResumeFallback, got {other:?}"),
    }
    assert_eq!(
        runtime
            .status(&make_identity("agent:legacy"))
            .await
            .unwrap()
            .session_id,
        Some(fallback_session_id)
    );
}

// ===========================================================================
// Task 2.11 — Full restore flow with sequencing (REQ-12)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_restore_flow_fresh_boot() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    let roster = vec![make_spec("triage:main"), make_spec("worker:main")];

    let result = restore_flow(&runtime, &roster, None, None).await.unwrap();

    assert_eq!(result.outcomes.len(), 2);
    for (id, outcome) in &result.outcomes {
        match outcome {
            RestoreOutcome::Created { record, .. } => {
                assert_eq!(&record.identity, id);
                assert_eq!(record.generation, ContinuityGeneration::new(0));
            }
            other => panic!("expected Created, got: {other:?}"),
        }
    }

    // Both should be registered
    assert!(runtime.contains(&make_identity("triage:main")).await);
    assert!(runtime.contains(&make_identity("worker:main")).await);
}

#[tokio::test]
async fn identity_first_runtime_restore_flow_resumes_ready() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    let id = make_identity("triage:main");
    // Record with checkpoint_version=0 so the snapshot save at version 1 succeeds
    let record = make_record("triage:main", 0, 0);

    // Pre-seed continuity and snapshot
    store
        .upsert_continuity_record(&record, FencingToken::new(0))
        .await
        .unwrap();
    store
        .save_session_snapshot(
            &id,
            &record.session_id,
            record.generation,
            CheckpointVersion::new(1),
            FencingToken::new(0),
            &SessionSnapshot {
                data: b"snapshot data".to_vec(),
            },
        )
        .await
        .unwrap();

    let roster = vec![make_spec("triage:main")];
    let result = restore_flow(&runtime, &roster, None, None).await.unwrap();

    match result.outcomes.get(&id).unwrap() {
        RestoreOutcome::Resumed {
            record: r,
            snapshot,
            ..
        } => {
            assert_eq!(r.identity, id);
            assert_eq!(snapshot.data, b"snapshot data");
        }
        other => panic!("expected Resumed, got: {other:?}"),
    }
}

#[tokio::test]
async fn identity_first_runtime_restore_flow_reconciles_resume_returned_session_id() {
    struct ResumeFallbackBridge {
        actual_session_id: meerkat_core::types::SessionId,
    }

    #[async_trait]
    impl SessionBridge for ResumeFallbackBridge {
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
            _session_id: &meerkat_core::types::SessionId,
            _snapshot: &SessionSnapshot,
        ) -> Result<meerkat_mobkit::identity_first::ResumeSessionOutcome, BridgeError> {
            Ok(
                meerkat_mobkit::identity_first::ResumeSessionOutcome::Resumed {
                    session_id: self.actual_session_id.clone(),
                },
            )
        }

        async fn deliver(
            &self,
            _runtime_id: &AgentRuntimeId,
            _content: &meerkat_core::ContentInput,
        ) -> Result<meerkat_core::types::SessionId, BridgeError> {
            Ok(self.actual_session_id.clone())
        }

        async fn checkpoint_session(
            &self,
            _runtime_id: &AgentRuntimeId,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<SessionSnapshot, BridgeError> {
            Ok(SessionSnapshot {
                data: b"bridge checkpoint".to_vec(),
            })
        }

        async fn retire_member(&self, _runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
            Ok(())
        }
    }

    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let actual_session_id = meerkat_core::types::SessionId::new();
    let runtime = IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store.clone(),
        lease_provider: lease_prov,
        runtime_instance_id: "test-runtime".to_string(),
        has_runtime_store: true,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(Arc::new(ResumeFallbackBridge {
            actual_session_id: actual_session_id.clone(),
        })),
        default_timeout: None,
    });

    let id = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);
    assert_ne!(record.session_id, actual_session_id);
    store
        .upsert_continuity_record(&record, FencingToken::new(0))
        .await
        .unwrap();
    store
        .save_session_snapshot(
            &id,
            &record.session_id,
            record.generation,
            CheckpointVersion::new(1),
            FencingToken::new(0),
            &SessionSnapshot {
                data: b"old snapshot".to_vec(),
            },
        )
        .await
        .unwrap();

    let roster = vec![make_spec("triage:main")];
    let result = restore_flow(&runtime, &roster, None, None).await.unwrap();
    match result.outcomes.get(&id).unwrap() {
        RestoreOutcome::Resumed { record, .. } => {
            assert_eq!(record.session_id, actual_session_id);
            assert_eq!(record.checkpoint_version, CheckpointVersion::new(1));
        }
        other => panic!("expected Resumed, got: {other:?}"),
    }

    let status = runtime.status(&id).await.unwrap();
    assert_eq!(status.session_id, Some(actual_session_id.clone()));
    assert_eq!(status.checkpoint_version, Some(CheckpointVersion::new(1)));

    let next_version = runtime
        .checkpoint(
            &id,
            &SessionSnapshot {
                data: b"new snapshot".to_vec(),
            },
        )
        .await
        .expect("checkpoint should use returned resume session id and current version");
    assert_eq!(next_version, CheckpointVersion::new(2));

    let resolved = store.resolve_many(std::slice::from_ref(&id)).await.unwrap();
    let ContinuityResolveState::Ready { record } = resolved.get(&id).unwrap() else {
        panic!("expected ready record");
    };
    assert_eq!(record.session_id, actual_session_id);
    assert_eq!(record.checkpoint_version, CheckpointVersion::new(2));
}

#[tokio::test]
async fn identity_first_runtime_restore_flow_customizer_receives_peers() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    // Track what the customizer receives
    let seen_contexts = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    struct TrackingCustomizer {
        seen: Arc<tokio::sync::Mutex<Vec<AgentBuildContext>>>,
    }

    #[async_trait]
    impl AgentCustomizer for TrackingCustomizer {
        async fn customize_build(
            &self,
            context: &AgentBuildContext,
            _spec: &DurableAgentSpec,
            _draft: &mut AgentBuildDraft,
        ) -> Result<(), CustomizerError> {
            self.seen.lock().await.push(context.clone());
            Ok(())
        }
    }

    let customizer = TrackingCustomizer {
        seen: seen_contexts.clone(),
    };

    let roster = vec![make_spec("triage:main"), make_spec("worker:main")];
    restore_flow(&runtime, &roster, None, Some(&customizer))
        .await
        .unwrap();

    let contexts = seen_contexts.lock().await;
    assert_eq!(contexts.len(), 2);
    // Each customizer call should see both identities as active_peers (cold boot)
    for ctx in contexts.iter() {
        assert_eq!(ctx.active_peers.len(), 2);
        assert!(ctx.active_peers.contains(&make_identity("triage:main")));
        assert!(ctx.active_peers.contains(&make_identity("worker:main")));
    }
}

// ===========================================================================
// Task 2.12 — Broken continuity fails loudly (REQ-13)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_restore_flow_broken_fails_loudly() {
    // Use a custom store that returns Broken for a specific identity
    struct BrokenStore;

    #[async_trait]
    impl ContinuityStore for BrokenStore {
        async fn resolve_many(
            &self,
            identities: &[AgentIdentity],
        ) -> Result<BTreeMap<AgentIdentity, ContinuityResolveState>, ContinuityStoreError> {
            let mut map = BTreeMap::new();
            for id in identities {
                if id.as_str() == "broken:main" {
                    map.insert(
                        id.clone(),
                        ContinuityResolveState::Broken {
                            failure: ContinuityFailure {
                                identity: id.clone(),
                                kind: ContinuityFailureKind::SnapshotCorrupted,
                                record: None,
                                detail: "corrupted data".to_string(),
                            },
                        },
                    );
                } else {
                    map.insert(id.clone(), ContinuityResolveState::Uninitialized);
                }
            }
            Ok(map)
        }

        async fn load_session_snapshot(
            &self,
            _session_id: &meerkat_core::types::SessionId,
        ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
            Ok(None)
        }

        async fn save_session_snapshot(
            &self,
            _identity: &AgentIdentity,
            _session_id: &meerkat_core::types::SessionId,
            _generation: ContinuityGeneration,
            _version: CheckpointVersion,
            _fencing_token: FencingToken,
            _snapshot: &SessionSnapshot,
        ) -> Result<(), ContinuityStoreError> {
            Ok(())
        }

        async fn upsert_continuity_record(
            &self,
            _record: &ContinuityRecord,
            _fencing_token: FencingToken,
        ) -> Result<(), ContinuityStoreError> {
            Ok(())
        }

        async fn delete_continuity_record(
            &self,
            _identity: &AgentIdentity,
            _fencing_token: FencingToken,
        ) -> Result<(), ContinuityStoreError> {
            Ok(())
        }
    }

    let store = Arc::new(BrokenStore);
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease_prov);

    let roster = vec![make_spec("broken:main")];
    let result = restore_flow(&runtime, &roster, None, None).await.unwrap();

    match result.outcomes.get(&make_identity("broken:main")).unwrap() {
        RestoreOutcome::Broken(failure) => {
            assert_eq!(failure.kind, ContinuityFailureKind::SnapshotCorrupted);
            assert_eq!(failure.detail, "corrupted data");
        }
        other => panic!("expected Broken, got: {other:?}"),
    }
}

// ===========================================================================
// Task 2.13 — Checkpoint cadence (REQ-14)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_checkpoint_saves_snapshot() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    let id = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);

    // Need to seed the record in the store so checkpoint can validate
    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(record.clone()),
            Some(make_grant("triage:main", 1)),
        )
        .await;

    let snapshot = SessionSnapshot {
        data: b"turn-1-data".to_vec(),
    };
    let version = runtime.checkpoint(&id, &snapshot).await.unwrap();
    assert_eq!(version, CheckpointVersion::new(1));

    // Verify snapshot was persisted
    let loaded = store
        .load_session_snapshot(&record.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.data, b"turn-1-data");
}

// ===========================================================================
// Task 2.14 — Checkpoint version ordering (REQ-15)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_checkpoint_version_ordering() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    let id = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);

    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(record.clone()),
            Some(make_grant("triage:main", 1)),
        )
        .await;

    // First checkpoint at version 1 → ok
    let snap1 = SessionSnapshot {
        data: b"v1".to_vec(),
    };
    let v1 = runtime.checkpoint(&id, &snap1).await.unwrap();
    assert_eq!(v1, CheckpointVersion::new(1));

    // Second checkpoint at version 2 → ok
    let snap2 = SessionSnapshot {
        data: b"v2".to_vec(),
    };
    let v2 = runtime.checkpoint(&id, &snap2).await.unwrap();
    assert_eq!(v2, CheckpointVersion::new(2));

    // Direct store attempt at version 2 again → rejected
    let stale = store
        .save_session_snapshot(
            &id,
            &record.session_id,
            record.generation,
            CheckpointVersion::new(2), // same as current
            FencingToken::new(1),
            &SessionSnapshot {
                data: b"stale".to_vec(),
            },
        )
        .await;
    assert!(matches!(
        stale.unwrap_err(),
        ContinuityStoreError::StaleCheckpointVersion { .. }
    ));

    // Direct store attempt at version 1 → also rejected
    let older = store
        .save_session_snapshot(
            &id,
            &record.session_id,
            record.generation,
            CheckpointVersion::new(1),
            FencingToken::new(1),
            &SessionSnapshot {
                data: b"older".to_vec(),
            },
        )
        .await;
    assert!(matches!(
        older.unwrap_err(),
        ContinuityStoreError::StaleCheckpointVersion { .. }
    ));
}

// ===========================================================================
// Task 2.15 — Stale fencing token rejection (REQ-16)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_stale_fencing_token_rejected() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let id = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);

    // Upsert with token 5
    store
        .upsert_continuity_record(&record, FencingToken::new(5))
        .await
        .unwrap();

    // Attempt save with stale token 3 → rejected
    let result = store
        .save_session_snapshot(
            &id,
            &record.session_id,
            record.generation,
            CheckpointVersion::new(1),
            FencingToken::new(3), // stale
            &SessionSnapshot {
                data: b"data".to_vec(),
            },
        )
        .await;
    assert!(matches!(
        result.unwrap_err(),
        ContinuityStoreError::StaleFencingToken { .. }
    ));

    // Token 5 (current) → accepted
    let result = store
        .save_session_snapshot(
            &id,
            &record.session_id,
            record.generation,
            CheckpointVersion::new(1),
            FencingToken::new(5),
            &SessionSnapshot {
                data: b"data".to_vec(),
            },
        )
        .await;
    assert!(result.is_ok());
}

// ===========================================================================
// Task 2.16 — Checkpoint handoff per durability policy (REQ-17)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_checkpoint_policy_sync_write_through() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    let id = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);
    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(record.clone()),
            Some(make_grant("triage:main", 1)),
        )
        .await;

    let snap = SessionSnapshot {
        data: b"sync-data".to_vec(),
    };
    // SyncWriteThrough: checkpoint blocks until durable
    let v = runtime.checkpoint(&id, &snap).await.unwrap();
    assert_eq!(v, CheckpointVersion::new(1));

    // Verify it's durable (can be loaded)
    let loaded = store
        .load_session_snapshot(&record.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.data, b"sync-data");

    // Verify status reports correct policy
    let status = runtime.status(&id).await.unwrap();
    let health = status.continuity_health.unwrap();
    assert_eq!(health.durability_policy, DurabilityPolicy::SyncWriteThrough);
}

#[tokio::test]
async fn identity_first_runtime_checkpoint_policy_async_reported_in_status() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store.clone(),
        lease_provider: lease_prov,
        runtime_instance_id: "test".to_string(),
        has_runtime_store: false,
        durability_policy: DurabilityPolicy::AsyncReplicated,
        bridge: None,
        default_timeout: None,
    });

    let id = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);
    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(record),
            Some(make_grant("triage:main", 1)),
        )
        .await;

    let status = runtime.status(&id).await.unwrap();
    let health = status.continuity_health.unwrap();
    assert_eq!(health.durability_policy, DurabilityPolicy::AsyncReplicated);
}

// ===========================================================================
// Task 2.17 — Local cache promotion rules (REQ-18)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_local_cache_promotion_stale_fails() {
    // If local state is newer than authoritative and CAS fails, it should
    // fail loudly. The ContinuityStore CAS rules enforce this at the store level.
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let id = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);

    // Seed at version 3
    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();
    store
        .save_session_snapshot(
            &id,
            &record.session_id,
            record.generation,
            CheckpointVersion::new(3),
            FencingToken::new(1),
            &SessionSnapshot {
                data: b"v3".to_vec(),
            },
        )
        .await
        .unwrap();

    // Attempt to "promote" local state at version 2 (older) → fails CAS
    let result = store
        .save_session_snapshot(
            &id,
            &record.session_id,
            record.generation,
            CheckpointVersion::new(2), // older than 3
            FencingToken::new(1),
            &SessionSnapshot {
                data: b"local-v2".to_vec(),
            },
        )
        .await;
    assert!(matches!(
        result.unwrap_err(),
        ContinuityStoreError::StaleCheckpointVersion { .. }
    ));
}

// ===========================================================================
// Task 2.18 — INV-01: Lease required before work
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_inv01_send_without_lease_rejected() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    // Register without a lease
    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(make_record("triage:main", 0, 0)),
            None, // no lease
        )
        .await;

    let result = runtime
        .send(&make_identity("triage:main"), &make_content())
        .await;
    assert!(matches!(
        result.unwrap_err(),
        IdentityRuntimeError::NoActiveLease(_)
    ));
}

#[tokio::test]
async fn identity_first_runtime_inv01_dispatch_without_lease_rejected() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(make_record("triage:main", 0, 0)),
            None,
        )
        .await;

    let result = runtime
        .dispatch(&make_identity("triage:main"), &make_dispatch_input())
        .await;
    assert!(matches!(
        result.unwrap_err(),
        IdentityRuntimeError::NoActiveLease(_)
    ));
}

#[tokio::test]
async fn identity_first_runtime_inv01_checkpoint_without_lease_rejected() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(make_record("triage:main", 0, 0)),
            None,
        )
        .await;

    let result = runtime
        .checkpoint(
            &make_identity("triage:main"),
            &SessionSnapshot {
                data: b"data".to_vec(),
            },
        )
        .await;
    assert!(matches!(
        result.unwrap_err(),
        IdentityRuntimeError::NoActiveLease(_)
    ));
}

// ===========================================================================
// Task 2.19 — INV-02: Lease loss blocks new work
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_inv02_lease_loss_blocks_send() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(make_record("triage:main", 0, 0)),
            Some(make_grant("triage:main", 1)),
        )
        .await;

    // Mark lease as lost
    runtime
        .mark_lease_lost(&make_identity("triage:main"))
        .await
        .unwrap();

    let result = runtime
        .send(&make_identity("triage:main"), &make_content())
        .await;
    assert!(matches!(
        result.unwrap_err(),
        IdentityRuntimeError::NoActiveLease(_)
    ));
}

#[tokio::test]
async fn identity_first_runtime_inv02_lease_loss_blocks_dispatch() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(make_record("triage:main", 0, 0)),
            Some(make_grant("triage:main", 1)),
        )
        .await;

    runtime
        .mark_lease_lost(&make_identity("triage:main"))
        .await
        .unwrap();

    let result = runtime
        .dispatch(&make_identity("triage:main"), &make_dispatch_input())
        .await;
    assert!(matches!(
        result.unwrap_err(),
        IdentityRuntimeError::NoActiveLease(_)
    ));
}

// ===========================================================================
// Task 2.20 — INV-05: Reset/delete ownership precondition
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_inv05_reset_fences_old_owner() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    let id = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);
    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(record.clone()),
            Some(make_grant("triage:main", 1)),
        )
        .await;

    // Reset acquires a new lease (fencing old owner) and advances generation
    let new_record = runtime.reset(&id).await.unwrap();
    assert_eq!(new_record.generation, ContinuityGeneration::new(1));

    // The old fencing token (1) should now be stale due to new lease acquisition
    // (the local lease provider issues monotonic tokens, so the new token > 1)
}

#[tokio::test]
async fn identity_first_runtime_inv05_delete_fences_old_owner() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease_prov.clone());

    let id = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);
    store
        .upsert_continuity_record(&record, FencingToken::new(1))
        .await
        .unwrap();

    runtime
        .register(
            make_spec("triage:main"),
            IdentityLifecycleState::Active,
            Some(record),
            Some(make_grant("triage:main", 1)),
        )
        .await;

    // Delete fences old owner via lease re-acquisition
    runtime.delete_identity(&id).await.unwrap();
    assert!(!runtime.contains(&id).await);
}

// ===========================================================================
// Task 2.21 — INV-06: Roster identity uniqueness
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_inv06_duplicate_identities_rejected() {
    let specs = vec![make_spec("triage:main"), make_spec("triage:main")];
    let result = IdentityRuntime::validate_roster_uniqueness(&specs);
    assert!(matches!(
        result.unwrap_err(),
        IdentityRuntimeError::DuplicateIdentity(_)
    ));
}

#[tokio::test]
async fn identity_first_runtime_inv06_unique_identities_accepted() {
    let specs = vec![make_spec("triage:main"), make_spec("worker:main")];
    assert!(IdentityRuntime::validate_roster_uniqueness(&specs).is_ok());
}

#[tokio::test]
async fn identity_first_runtime_inv06_restore_flow_rejects_duplicates() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease_prov);

    let roster = vec![make_spec("triage:main"), make_spec("triage:main")];
    let result = restore_flow(&runtime, &roster, None, None).await;
    assert!(matches!(
        result.unwrap_err(),
        IdentityRuntimeError::DuplicateIdentity(_)
    ));
}

// ===========================================================================
// Task 2.22 — Topology: compute + reconcile (REQ-19, REQ-21)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_topology_compute_edges() {
    struct TestTopology;

    #[async_trait]
    impl TopologyProvider for TestTopology {
        async fn compute_edges(
            &self,
            target_identities: &[AgentIdentity],
            _context: &TopologyContext,
        ) -> Result<Vec<ManagedPeerEdge>, TopologyError> {
            // Create edges between all pairs
            let mut edges = Vec::new();
            for i in 0..target_identities.len() {
                for j in (i + 1)..target_identities.len() {
                    if let Ok(edge) = ManagedPeerEdge::new(
                        target_identities[i].clone(),
                        target_identities[j].clone(),
                    ) {
                        edges.push(edge);
                    }
                }
            }
            Ok(edges)
        }
    }

    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease_prov);

    let roster = vec![make_spec("a:main"), make_spec("b:main")];
    let topology = TestTopology;

    let result = restore_flow(&runtime, &roster, Some(&topology), None)
        .await
        .unwrap();

    assert_eq!(result.managed_edges.len(), 1);
    // The edge should connect a:main and b:main
    let edge = &result.managed_edges[0];
    assert_eq!(edge.a(), &make_identity("a:main"));
    assert_eq!(edge.b(), &make_identity("b:main"));
}

#[tokio::test]
async fn identity_first_runtime_topology_materializes_runtime_peer_wires() {
    #[derive(Default)]
    struct RecordingBridge {
        wires: tokio::sync::Mutex<Vec<(String, String)>>,
        unwires: tokio::sync::Mutex<Vec<(String, String)>>,
        current_wires: tokio::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl SessionBridge for RecordingBridge {
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
        ) -> Result<meerkat_mobkit::identity_first::ResumeSessionOutcome, BridgeError> {
            Ok(
                meerkat_mobkit::identity_first::ResumeSessionOutcome::Resumed {
                    session_id: session_id.clone(),
                },
            )
        }

        async fn deliver(
            &self,
            _runtime_id: &AgentRuntimeId,
            _content: &meerkat_core::ContentInput,
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

        async fn wire_peer(
            &self,
            a: &AgentRuntimeId,
            b: &AgentRuntimeId,
        ) -> Result<(), BridgeError> {
            self.wires
                .lock()
                .await
                .push((a.as_str().to_string(), b.as_str().to_string()));
            Ok(())
        }

        async fn wire_peers_batch(
            &self,
            edges: &[(AgentRuntimeId, AgentRuntimeId)],
        ) -> Result<(), BridgeError> {
            for (a, b) in edges {
                self.wire_peer(a, b).await?;
                self.current_wires
                    .lock()
                    .await
                    .push((a.as_str().to_string(), b.as_str().to_string()));
            }
            Ok(())
        }

        async fn unwire_peer(
            &self,
            a: &AgentRuntimeId,
            b: &AgentRuntimeId,
        ) -> Result<(), BridgeError> {
            self.unwires
                .lock()
                .await
                .push((a.as_str().to_string(), b.as_str().to_string()));
            Ok(())
        }

        async fn current_member_wires(
            &self,
        ) -> Result<Vec<(AgentRuntimeId, AgentRuntimeId)>, BridgeError> {
            Ok(self
                .current_wires
                .lock()
                .await
                .iter()
                .filter_map(|(a, b)| {
                    Some((
                        AgentRuntimeId::parse(a).ok()?,
                        AgentRuntimeId::parse(b).ok()?,
                    ))
                })
                .collect())
        }
    }

    struct StaticTopology(Vec<(&'static str, &'static str)>);

    #[async_trait]
    impl TopologyProvider for StaticTopology {
        async fn compute_edges(
            &self,
            _target_identities: &[AgentIdentity],
            _context: &TopologyContext,
        ) -> Result<Vec<ManagedPeerEdge>, TopologyError> {
            self.0
                .iter()
                .map(|(a, b)| {
                    ManagedPeerEdge::new(make_identity(a), make_identity(b))
                        .map_err(|e| TopologyError::InvalidEdge(format!("{e}")))
                })
                .collect()
        }
    }

    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_provider = Arc::new(LocalLeaseProvider::new());
    let bridge = Arc::new(RecordingBridge::default());
    let runtime = IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store,
        lease_provider,
        runtime_instance_id: "test-runtime".to_string(),
        has_runtime_store: false,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(bridge.clone()),
        default_timeout: None,
    });
    let roster = vec![
        make_spec("a:main"),
        make_spec("b:main"),
        make_spec("c:main"),
    ];

    restore_flow(
        &runtime,
        &roster,
        Some(&StaticTopology(vec![("a:main", "b:main")])),
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        bridge.wires.lock().await.as_slice(),
        &[("rt:a:main:0".to_string(), "rt:b:main:0".to_string())]
    );

    restore_flow(
        &runtime,
        &roster,
        Some(&StaticTopology(vec![("b:main", "c:main")])),
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        bridge.wires.lock().await.as_slice(),
        &[
            ("rt:a:main:0".to_string(), "rt:b:main:0".to_string()),
            ("rt:b:main:0".to_string(), "rt:c:main:0".to_string()),
        ]
    );
    assert_eq!(
        bridge.unwires.lock().await.as_slice(),
        &[("rt:a:main:0".to_string(), "rt:b:main:0".to_string())]
    );
}

#[tokio::test]
async fn identity_first_runtime_topology_claims_persisted_wires_without_rebatching() {
    #[derive(Default)]
    struct RecordingBridge {
        wires: tokio::sync::Mutex<Vec<(String, String)>>,
        unwires: tokio::sync::Mutex<Vec<(String, String)>>,
        current_wires: tokio::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl SessionBridge for RecordingBridge {
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
        ) -> Result<meerkat_mobkit::identity_first::ResumeSessionOutcome, BridgeError> {
            Ok(
                meerkat_mobkit::identity_first::ResumeSessionOutcome::Resumed {
                    session_id: session_id.clone(),
                },
            )
        }

        async fn deliver(
            &self,
            _runtime_id: &AgentRuntimeId,
            _content: &meerkat_core::ContentInput,
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

        async fn wire_peer(
            &self,
            a: &AgentRuntimeId,
            b: &AgentRuntimeId,
        ) -> Result<(), BridgeError> {
            self.wires
                .lock()
                .await
                .push((a.as_str().to_string(), b.as_str().to_string()));
            Ok(())
        }

        async fn unwire_peer(
            &self,
            a: &AgentRuntimeId,
            b: &AgentRuntimeId,
        ) -> Result<(), BridgeError> {
            self.unwires
                .lock()
                .await
                .push((a.as_str().to_string(), b.as_str().to_string()));
            Ok(())
        }

        async fn current_member_wires(
            &self,
        ) -> Result<Vec<(AgentRuntimeId, AgentRuntimeId)>, BridgeError> {
            Ok(self
                .current_wires
                .lock()
                .await
                .iter()
                .filter_map(|(a, b)| {
                    Some((
                        AgentRuntimeId::parse(a).ok()?,
                        AgentRuntimeId::parse(b).ok()?,
                    ))
                })
                .collect())
        }
    }

    struct StaticTopology(Vec<(&'static str, &'static str)>);

    #[async_trait]
    impl TopologyProvider for StaticTopology {
        async fn compute_edges(
            &self,
            _target_identities: &[AgentIdentity],
            _context: &TopologyContext,
        ) -> Result<Vec<ManagedPeerEdge>, TopologyError> {
            self.0
                .iter()
                .map(|(a, b)| {
                    ManagedPeerEdge::new(make_identity(a), make_identity(b))
                        .map_err(|e| TopologyError::InvalidEdge(format!("{e}")))
                })
                .collect()
        }
    }

    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_provider = Arc::new(LocalLeaseProvider::new());
    let bridge = Arc::new(RecordingBridge::default());
    bridge
        .current_wires
        .lock()
        .await
        .push(("rt:a:main:0".to_string(), "rt:b:main:0".to_string()));
    let runtime = IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store,
        lease_provider,
        runtime_instance_id: "test-runtime".to_string(),
        has_runtime_store: false,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(bridge.clone()),
        default_timeout: None,
    });
    let roster = vec![make_spec("a:main"), make_spec("b:main")];

    restore_flow(
        &runtime,
        &roster,
        Some(&StaticTopology(vec![("a:main", "b:main")])),
        None,
    )
    .await
    .unwrap();

    assert!(
        bridge.wires.lock().await.is_empty(),
        "persisted wires should be claimed as managed without re-sending a batch"
    );

    restore_flow(&runtime, &roster, Some(&StaticTopology(Vec::new())), None)
        .await
        .unwrap();

    assert_eq!(
        bridge.unwires.lock().await.as_slice(),
        &[("rt:a:main:0".to_string(), "rt:b:main:0".to_string())]
    );
}

// ===========================================================================
// Task 2.23 — Topology: static wiring preserved (REQ-20)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_topology_static_wiring_not_modified() {
    // Static wiring is managed at the mob level, not by topology provider.
    // The topology provider only returns managed edges — static edges remain untouched.
    // This test verifies that the restore flow only produces managed edges
    // and does not create/modify static edges.
    struct EmptyTopology;

    #[async_trait]
    impl TopologyProvider for EmptyTopology {
        async fn compute_edges(
            &self,
            _target_identities: &[AgentIdentity],
            _context: &TopologyContext,
        ) -> Result<Vec<ManagedPeerEdge>, TopologyError> {
            Ok(Vec::new()) // no managed edges
        }
    }

    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease_prov);

    let roster = vec![make_spec("a:main"), make_spec("b:main")];
    let result = restore_flow(&runtime, &roster, Some(&EmptyTopology), None)
        .await
        .unwrap();

    // No managed edges — static wiring (from mob definition) remains untouched
    assert!(result.managed_edges.is_empty());
}

// ===========================================================================
// Task 2.24 — Topology is not continuity truth (REQ-22)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_topology_recomputed_from_current_state() {
    // Topology is always recomputed, not replayed from stored data.
    // Call restore_flow twice — topology should be freshly computed each time.
    use std::sync::atomic::{AtomicU32, Ordering};

    struct CountingTopology(AtomicU32);

    #[async_trait]
    impl TopologyProvider for CountingTopology {
        async fn compute_edges(
            &self,
            _target: &[AgentIdentity],
            _ctx: &TopologyContext,
        ) -> Result<Vec<ManagedPeerEdge>, TopologyError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        }
    }

    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease_prov = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease_prov);
    let topology = CountingTopology(AtomicU32::new(0));

    let roster = vec![make_spec("a:main")];
    restore_flow(&runtime, &roster, Some(&topology), None)
        .await
        .unwrap();
    restore_flow(&runtime, &roster, Some(&topology), None)
        .await
        .unwrap();

    // Should have been called twice (once per restore_flow)
    assert_eq!(topology.0.load(Ordering::Relaxed), 2);
}

// ===========================================================================
// Task 2.25 — Identity-keyed roster inspection API (REQ-32)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_roster_inspect_returns_all_identities() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let mut spec1 = make_spec("triage:main");
    spec1
        .labels
        .insert("role".to_string(), "triage".to_string());
    let mut spec2 = make_internal_spec("gate:main");
    spec2.labels.insert("role".to_string(), "gate".to_string());

    runtime
        .register(
            spec1.clone(),
            IdentityLifecycleState::Active,
            Some(make_record("triage:main", 0, 2)),
            Some(make_grant("triage:main", 1)),
        )
        .await;
    runtime
        .register(
            spec2.clone(),
            IdentityLifecycleState::Active,
            Some(make_record("gate:main", 0, 0)),
            Some(make_grant("gate:main", 2)),
        )
        .await;

    let roster = runtime.roster_inspect().await;
    assert_eq!(roster.len(), 2);

    let (spec, status) = roster.get(&make_identity("triage:main")).unwrap();
    assert_eq!(spec.labels.get("role"), Some(&"triage".to_string()));
    assert_eq!(status.state, IdentityLifecycleState::Active);
    assert_eq!(status.addressability, AgentAddressability::Addressable);
    assert!(status.lease.is_some());

    let (spec, status) = roster.get(&make_identity("gate:main")).unwrap();
    assert_eq!(spec.labels.get("role"), Some(&"gate".to_string()));
    assert_eq!(status.addressability, AgentAddressability::InternalOnly);
}

// ===========================================================================
// Task 2.26 — Reconciliation with field-level rules (REQ-33)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_reconcile_new_identity_activates() {
    let current = BTreeMap::new();
    let desired = vec![make_spec("triage:main")];
    let actions = compute_reconcile_actions(&desired, &current);

    assert_eq!(actions.len(), 1);
    assert!(
        matches!(&actions[0], ReconcileAction::Activate(s) if s.identity == make_identity("triage:main"))
    );
}

#[tokio::test]
async fn identity_first_runtime_reconcile_removed_identity_retires() {
    let mut current = BTreeMap::new();
    current.insert(make_identity("old:main"), make_spec("old:main"));

    let desired = vec![]; // empty desired roster
    let actions = compute_reconcile_actions(&desired, &current);

    assert_eq!(actions.len(), 1);
    assert!(matches!(&actions[0], ReconcileAction::Retire(id) if id == &make_identity("old:main")));
}

#[tokio::test]
async fn identity_first_runtime_reconcile_labels_change_hot_reloads() {
    let mut old_spec = make_spec("triage:main");
    old_spec
        .labels
        .insert("env".to_string(), "staging".to_string());

    let mut current = BTreeMap::new();
    current.insert(make_identity("triage:main"), old_spec);

    let mut new_spec = make_spec("triage:main");
    new_spec
        .labels
        .insert("env".to_string(), "production".to_string());

    let actions = compute_reconcile_actions(&[new_spec.clone()], &current);
    assert_eq!(actions.len(), 1);
    assert!(
        matches!(&actions[0], ReconcileAction::HotReload { identity, .. }
        if identity == &make_identity("triage:main"))
    );
}

#[tokio::test]
async fn identity_first_runtime_reconcile_addressability_change_hot_reloads() {
    let old_spec = make_spec("triage:main");
    let mut current = BTreeMap::new();
    current.insert(make_identity("triage:main"), old_spec);

    let new_spec = make_internal_spec("triage:main"); // changed to InternalOnly

    let actions = compute_reconcile_actions(&[new_spec], &current);
    assert_eq!(actions.len(), 1);
    assert!(matches!(&actions[0], ReconcileAction::HotReload { .. }));
}

#[tokio::test]
async fn identity_first_runtime_reconcile_profile_change_respawns() {
    let old_spec = make_spec("triage:main");
    let mut current = BTreeMap::new();
    current.insert(make_identity("triage:main"), old_spec);

    let mut new_spec = make_spec("triage:main");
    new_spec.profile = meerkat_mob::ProfileName::from("expert"); // changed profile

    let actions = compute_reconcile_actions(&[new_spec], &current);
    assert_eq!(actions.len(), 1);
    assert!(
        matches!(&actions[0], ReconcileAction::Respawn { identity, .. }
        if identity == &make_identity("triage:main"))
    );
}

#[tokio::test]
async fn identity_first_runtime_reconcile_context_change_hot_reloads() {
    let old_spec = make_spec("triage:main");
    let mut current = BTreeMap::new();
    current.insert(make_identity("triage:main"), old_spec);

    let mut new_spec = make_spec("triage:main");
    new_spec.context = Some(serde_json::json!({"key": "value"}));

    let actions = compute_reconcile_actions(&[new_spec], &current);
    assert_eq!(actions.len(), 1);
    assert!(matches!(&actions[0], ReconcileAction::HotReload { .. }));
}

#[tokio::test]
async fn identity_first_runtime_reconcile_unchanged_no_action() {
    let spec = make_spec("triage:main");
    let mut current = BTreeMap::new();
    current.insert(make_identity("triage:main"), spec.clone());

    let actions = compute_reconcile_actions(&[spec], &current);
    assert!(actions.is_empty());
}

// ===========================================================================
// Task 2.5 — subscribe(identity) (REQ-06)
// ===========================================================================

#[tokio::test]
async fn identity_first_runtime_subscribe_receives_identity_events() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store.clone(), lease.clone());

    let spec = make_spec("triage:main");
    let identity = make_identity("triage:main");
    let record = make_record("triage:main", 0, 0);

    // Acquire a real lease so the provider knows about it
    let grants = lease
        .acquire_leases(std::slice::from_ref(&identity), "test-runtime")
        .await
        .unwrap();
    let grant = match &grants[&identity] {
        LeaseAcquireResult::Acquired(g) => g.clone(),
        _ => panic!("expected acquired lease"),
    };

    runtime
        .register(
            spec,
            IdentityLifecycleState::Active,
            Some(record),
            Some(grant),
        )
        .await;

    // Subscribe
    let mut rx = runtime.subscribe(&identity).await.unwrap();

    // 1. State change event
    runtime
        .set_state(&identity, IdentityLifecycleState::Retiring)
        .await
        .unwrap();

    let event = rx.recv().await.unwrap();
    assert!(matches!(
        event,
        IdentityEvent::StateChanged {
            new_state: IdentityLifecycleState::Retiring,
            ..
        }
    ));

    // 2. Lease loss event
    runtime.mark_lease_lost(&identity).await.unwrap();

    let event = rx.recv().await.unwrap();
    assert!(matches!(event, IdentityEvent::LeaseLost { .. }));

    // 3. Lease update event — re-acquire lease
    let grants = lease
        .acquire_leases(std::slice::from_ref(&identity), "test-runtime")
        .await
        .unwrap();
    let grant = match &grants[&identity] {
        LeaseAcquireResult::Acquired(g) => g.clone(),
        _ => panic!("expected acquired lease"),
    };
    runtime.update_lease(&identity, grant).await.unwrap();

    let event = rx.recv().await.unwrap();
    assert!(matches!(event, IdentityEvent::LeaseUpdated { .. }));

    // 4. Checkpoint event
    let snapshot = SessionSnapshot {
        data: b"test-data".to_vec(),
    };
    runtime.checkpoint(&identity, &snapshot).await.unwrap();

    let event = rx.recv().await.unwrap();
    assert!(matches!(
        event,
        IdentityEvent::CheckpointCompleted { version, .. }
        if version == CheckpointVersion::new(1)
    ));
}

#[tokio::test]
async fn identity_first_runtime_subscribe_unknown_identity_fails() {
    let store = Arc::new(LocalContinuityStore::in_memory().unwrap());
    let lease = Arc::new(LocalLeaseProvider::new());
    let runtime = make_runtime(store, lease);

    let result = runtime.subscribe(&make_identity("nonexistent")).await;
    assert!(matches!(
        result,
        Err(IdentityRuntimeError::UnknownIdentity(_))
    ));
}
