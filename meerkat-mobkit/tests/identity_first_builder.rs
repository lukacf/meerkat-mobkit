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
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_client::{LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::StopReason;
use meerkat_mobkit::identity_first::contracts::{
    ContinuityStore, LeaseProvider, RosterProvider, TopologyProvider,
};
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentIdentity, CheckpointVersion, ContinuityGeneration, ContinuityRecord,
    ContinuityResolveState, ContinuityStoreError, DurableAgentSpec, FencingToken,
    IdentityLifecycleState, LeaseAcquireResult, LeaseError, LeaseGrant, LeaseRenewResult,
    LocalContinuityStore, ManagedPeerEdge, RosterContext, RosterError, SessionSnapshot,
    TopologyContext, TopologyError,
};
use meerkat_mobkit::unified_runtime::{IdentityBootstrapMode, UnifiedRuntimeBuilder};
use meerkat_mobkit::{
    AllowAllConsoleVisibilityPolicy, ConsoleRuntimeRegistration, ConsoleVisibility,
    JsonRpcResponse, MobKitConsoleAggregator, handle_unified_rpc_json,
};
use serde_json::json;

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

struct CountingReadyContinuityStore {
    records: BTreeMap<AgentIdentity, ContinuityRecord>,
    load_snapshot_calls: AtomicUsize,
    upsert_calls: AtomicUsize,
}

impl CountingReadyContinuityStore {
    fn new(records: BTreeMap<AgentIdentity, ContinuityRecord>) -> Self {
        Self {
            records,
            load_snapshot_calls: AtomicUsize::new(0),
            upsert_calls: AtomicUsize::new(0),
        }
    }

    fn load_snapshot_calls(&self) -> usize {
        self.load_snapshot_calls.load(Ordering::SeqCst)
    }

    fn upsert_calls(&self) -> usize {
        self.upsert_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ContinuityStore for CountingReadyContinuityStore {
    async fn resolve_many(
        &self,
        identities: &[AgentIdentity],
    ) -> Result<BTreeMap<AgentIdentity, ContinuityResolveState>, ContinuityStoreError> {
        Ok(identities
            .iter()
            .map(|id| {
                let state = self
                    .records
                    .get(id)
                    .cloned()
                    .map(|record| ContinuityResolveState::Ready { record })
                    .unwrap_or(ContinuityResolveState::Uninitialized);
                (id.clone(), state)
            })
            .collect())
    }

    async fn load_session_snapshot(
        &self,
        _sid: &meerkat_core::types::SessionId,
    ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
        self.load_snapshot_calls.fetch_add(1, Ordering::SeqCst);
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
        self.upsert_calls.fetch_add(1, Ordering::SeqCst);
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

struct SlowTurnClient {
    delay: Duration,
}

impl LlmClient for SlowTurnClient {
    fn stream<'a>(
        &'a self,
        _request: &'a LlmRequest,
    ) -> Pin<Box<dyn futures::Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>> {
        let delay = self.delay;
        Box::pin(async_stream::stream! {
            tokio::time::sleep(delay).await;
            yield Ok(LlmEvent::TextDelta {
                delta: "slow ok".to_string(),
                meta: None,
            });
            yield Ok(LlmEvent::Done {
                outcome: LlmDoneOutcome::Success {
                    stop_reason: StopReason::EndTurn,
                },
            });
        })
    }

    // meerkat 0.7: LlmClient::provider returns the typed Provider.
    fn provider(&self) -> meerkat_core::Provider {
        meerkat_core::Provider::OpenAI
    }

    fn health_check<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn Future<Output = Result<(), LlmError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Ok(()) })
    }
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

fn review_flow_definition() -> meerkat_mob::MobDefinition {
    meerkat_mob::MobDefinition::from_toml(
        r#"
[mob]
id = "identity-builder-flow-test"

[profiles.default]
model = "gpt-5.5"
runtime_mode = "turn_driven"

[profiles.default.tools]
comms = true

[flows.review_cycle]
description = "OB3-shaped review flow"

[flows.review_cycle.steps.review]
role = "default"
message = "run review cycle"
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
        backend: None,
        binding: None,
    }
}

fn continuity_record(identity: &str) -> ContinuityRecord {
    ContinuityRecord {
        identity: AgentIdentity::parse(identity).unwrap(),
        agent_runtime_id: meerkat_mobkit::identity_first::AgentRuntimeId::parse(&format!(
            "rt:{identity}:0"
        ))
        .unwrap(),
        session_id: meerkat_core::types::SessionId::new(),
        generation: ContinuityGeneration::new(0),
        checkpoint_version: CheckpointVersion::new(1),
    }
}

/// Helper: assert builder.build() returns Err and the error message contains the given substring.
async fn assert_build_err_contains(builder: UnifiedRuntimeBuilder, expected: &str) {
    match Box::pin(builder.build()).await {
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
    match Box::pin(builder.build()).await {
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
    Box::pin(assert_build_err_contains(builder, "mutually exclusive")).await;
}

#[tokio::test]
async fn identity_first_builder_persistent_state_conflicts_with_lease_provider() {
    let builder = UnifiedRuntimeBuilder::default()
        .persistent_state("/tmp/test-state")
        .lease_provider(Arc::new(StubLeaseProvider));
    Box::pin(assert_build_err_contains(builder, "mutually exclusive")).await;
}

#[tokio::test]
async fn identity_first_builder_persistent_state_conflicts_with_scratch_dir() {
    let builder = UnifiedRuntimeBuilder::default()
        .persistent_state("/tmp/test-state")
        .scratch_dir("/tmp/test-scratch");
    Box::pin(assert_build_err_contains(builder, "mutually exclusive")).await;
}

// ===========================================================================
// Task 1.13: Builder external path requires all three (REQ-24)
// ===========================================================================

#[tokio::test]
async fn identity_first_builder_external_path_missing_lease_and_scratch() {
    let builder = UnifiedRuntimeBuilder::default().continuity_store(Arc::new(StubContinuityStore));
    Box::pin(assert_build_err_contains(builder, "lease_provider")).await;
}

#[tokio::test]
async fn identity_first_builder_external_path_missing_continuity_store() {
    let builder = UnifiedRuntimeBuilder::default()
        .lease_provider(Arc::new(StubLeaseProvider))
        .scratch_dir("/tmp/test-scratch");
    Box::pin(assert_build_err_contains(builder, "continuity_store")).await;
}

#[tokio::test]
async fn identity_first_builder_external_path_missing_scratch_dir() {
    let builder = UnifiedRuntimeBuilder::default()
        .continuity_store(Arc::new(StubContinuityStore))
        .lease_provider(Arc::new(StubLeaseProvider));
    Box::pin(assert_build_err_contains(builder, "scratch_dir")).await;
}

#[tokio::test]
async fn identity_first_builder_identity_first_optional_setters_require_core_providers() {
    let builder = UnifiedRuntimeBuilder::default()
        .topology_provider(Arc::new(StubTopologyProvider {
            edges: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }))
        .identity_runtime_instance_id("builder-test");
    Box::pin(assert_build_err_contains(builder, "roster_provider")).await;
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
    Box::pin(assert_build_err_not_contains(
        builder,
        "mutually exclusive",
        "conflicting",
    ))
    .await;
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
    Box::pin(assert_build_err_not_contains(
        builder,
        "mutually exclusive",
        "conflicting",
    ))
    .await;
}

#[tokio::test]
async fn identity_first_builder_bootstraps_and_exposes_identity_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let roster = Arc::new(StubRosterProvider::new(vec![durable_spec("agent:alpha")]));

    let runtime = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .continuity_store(Arc::new(StubContinuityStore))
            .lease_provider(Arc::new(StubLeaseProvider))
            .roster_provider(roster)
            .scratch_dir(tmp.path())
            .identity_runtime_instance_id("builder-test")
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build(),
    )
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
async fn identity_first_send_does_not_park_mob_actor_until_turn_completion() {
    let tmp = tempfile::tempdir().unwrap();
    let roster = Arc::new(StubRosterProvider::new(vec![durable_spec("agent:alpha")]));

    let runtime = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .continuity_store(Arc::new(StubContinuityStore))
            .lease_provider(Arc::new(StubLeaseProvider))
            .roster_provider(roster)
            .scratch_dir(tmp.path())
            .identity_runtime_instance_id("builder-slow-send-test")
            .default_llm_client(Arc::new(SlowTurnClient {
                delay: Duration::from_secs(2),
            }))
            .build(),
    )
    .await
    .expect("builder should bootstrap identity-first runtime");

    let identity_runtime = runtime
        .identity_runtime()
        .expect("identity runtime should be exposed")
        .clone();
    tokio::time::timeout(
        Duration::from_millis(250),
        identity_runtime.send(
            &AgentIdentity::parse("agent:alpha").unwrap(),
            &meerkat_core::ContentInput::Text("start slow turn".to_string()),
        ),
    )
    .await
    .expect("identity send should ack at ingress, not turn completion")
    .expect("identity send should be accepted");

    Box::pin(tokio::time::timeout(
        Duration::from_millis(250),
        runtime
            .mob_handle()
            .spawn_spec(meerkat_mob::SpawnMemberSpec::from_wire(
                "default".to_string(),
                // meerkat 0.7: MemberCommsName is fail-closed; raw mob member
                // ids must be identifier-safe (no ":").
                "agent-beta".to_string(),
                Some("You are beta.".into()),
                None,
                None,
            )),
    ))
    .await
    .expect("mob actor should still process spawn while alpha turn runs")
    .expect("spawn should succeed");

    let _ = runtime.mob_handle().stop().await;
}

#[tokio::test]
async fn identity_first_builder_lazy_materialize_registers_large_ready_roster_without_hydration() {
    let tmp = tempfile::tempdir().unwrap();
    let specs = (0..1_000)
        .map(|index| durable_spec(&format!("agent:{index}")))
        .collect::<Vec<_>>();
    let records = specs
        .iter()
        .map(|spec| {
            (
                spec.identity.clone(),
                continuity_record(spec.identity.as_str()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let continuity_store = Arc::new(CountingReadyContinuityStore::new(records));

    let runtime = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .continuity_store(continuity_store.clone())
            .lease_provider(Arc::new(StubLeaseProvider))
            .roster_provider(Arc::new(StubRosterProvider::new(specs)))
            .scratch_dir(tmp.path())
            .identity_runtime_instance_id("builder-lazy-test")
            .identity_bootstrap_mode(IdentityBootstrapMode::LazyMaterialize)
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build(),
    )
    .await
    .expect("lazy builder should bootstrap identity metadata");

    assert_eq!(
        continuity_store.load_snapshot_calls(),
        0,
        "lazy build must not hydrate session snapshots"
    );
    assert_eq!(
        continuity_store.upsert_calls(),
        0,
        "lazy build must not rewrite continuity records"
    );
    assert!(
        runtime
            .mob_handle()
            .list_members_including_retiring()
            .await
            .is_empty(),
        "lazy build must not spawn/resume mob members"
    );
    let identity_runtime = runtime
        .identity_runtime()
        .expect("identity runtime should be exposed");
    let status = identity_runtime
        .status(&AgentIdentity::parse("agent:42").unwrap())
        .await
        .expect("dormant identity should be inspectable");
    assert_eq!(status.state, IdentityLifecycleState::Dormant);
    assert_eq!(status.profile.unwrap().as_str(), "default");
}

#[tokio::test]
async fn identity_first_builder_lazy_console_lists_and_inspects_dormant_identities() {
    let tmp = tempfile::tempdir().unwrap();
    let specs = vec![durable_spec("agent:alpha"), durable_spec("agent:beta")];
    let alpha_record = continuity_record("agent:alpha");
    let expected_alpha_session_id = alpha_record.session_id.to_string();
    let beta_record = continuity_record("agent:beta");
    let records = BTreeMap::from([
        (AgentIdentity::parse("agent:alpha").unwrap(), alpha_record),
        (AgentIdentity::parse("agent:beta").unwrap(), beta_record),
    ]);
    let continuity_store = Arc::new(CountingReadyContinuityStore::new(records));

    let runtime = Arc::new(
        Box::pin(
            UnifiedRuntimeBuilder::default()
                .definition(test_definition())
                .continuity_store(continuity_store.clone())
                .lease_provider(Arc::new(StubLeaseProvider))
                .roster_provider(Arc::new(StubRosterProvider::new(specs)))
                .scratch_dir(tmp.path())
                .identity_runtime_instance_id("builder-lazy-console-test")
                .identity_bootstrap_mode(IdentityBootstrapMode::LazyMaterialize)
                .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
                .build(),
        )
        .await
        .expect("lazy builder should bootstrap identity metadata"),
    );
    let aggregator = MobKitConsoleAggregator::in_memory();
    aggregator.register_runtime(ConsoleRuntimeRegistration {
        runtime_key: "runtime-a".to_string(),
        runtime: runtime.clone(),
        identity_namespace: "prod".to_string(),
        visibility_policy: Arc::new(AllowAllConsoleVisibilityPolicy),
    });

    let records = aggregator
        .list_identities()
        .await
        .expect("console identity list should work");
    let alpha = records
        .iter()
        .find(|record| record.identity == "prod/agent:alpha")
        .expect("dormant alpha should be visible");
    assert_eq!(alpha.health, "dormant");
    assert_eq!(alpha.visibility, ConsoleVisibility::Addressable);
    assert_eq!(alpha.session_id, Some(expected_alpha_session_id));
    assert!(
        runtime
            .mob_handle()
            .list_members_including_retiring()
            .await
            .is_empty(),
        "console listing must not materialize dormant identities"
    );
    assert_eq!(
        continuity_store.load_snapshot_calls(),
        0,
        "console listing must not load session snapshots"
    );

    let inspection = Box::pin(aggregator.inspect_identity("prod/agent:alpha"))
        .await
        .expect("console inspect should not fail")
        .expect("dormant alpha should be inspectable");
    assert_eq!(inspection.identity.health, "dormant");
    assert!(inspection.peers.is_empty());
    assert!(
        runtime
            .mob_handle()
            .list_members_including_retiring()
            .await
            .is_empty(),
        "console inspection must not materialize dormant identities"
    );
}

#[tokio::test]
async fn identity_first_builder_rejects_zero_background_warm_concurrency() {
    let tmp = tempfile::tempdir().unwrap();
    let builder = UnifiedRuntimeBuilder::default()
        .definition(test_definition())
        .continuity_store(Arc::new(StubContinuityStore))
        .lease_provider(Arc::new(StubLeaseProvider))
        .roster_provider(Arc::new(StubRosterProvider::new(vec![durable_spec(
            "agent:alpha",
        )])))
        .scratch_dir(tmp.path())
        .identity_bootstrap_mode(IdentityBootstrapMode::LazyWithBackgroundWarm { concurrency: 0 })
        .default_llm_client(Arc::new(meerkat_client::TestClient::default()));

    Box::pin(assert_build_err_contains(builder, "concurrency")).await;
}

#[tokio::test]
async fn identity_first_builder_lazy_topology_refresh_stays_metadata_only() {
    let tmp = tempfile::tempdir().unwrap();
    let specs = vec![durable_spec("agent:a"), durable_spec("agent:b")];
    let records = specs
        .iter()
        .map(|spec| {
            (
                spec.identity.clone(),
                continuity_record(spec.identity.as_str()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let continuity_store = Arc::new(CountingReadyContinuityStore::new(records));
    let topology = Arc::new(StubTopologyProvider {
        edges: Arc::new(tokio::sync::Mutex::new(vec![
            ManagedPeerEdge::new(
                AgentIdentity::parse("agent:a").unwrap(),
                AgentIdentity::parse("agent:b").unwrap(),
            )
            .unwrap(),
        ])),
    });

    let runtime = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .continuity_store(continuity_store.clone())
            .lease_provider(Arc::new(StubLeaseProvider))
            .roster_provider(Arc::new(StubRosterProvider::new(specs)))
            .topology_provider(topology)
            .scratch_dir(tmp.path())
            .identity_runtime_instance_id("builder-lazy-refresh-test")
            .identity_bootstrap_mode(IdentityBootstrapMode::LazyMaterialize)
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build(),
    )
    .await
    .expect("lazy builder should bootstrap");

    runtime
        .refresh_desired_topology()
        .await
        .expect("lazy refresh should succeed");

    assert_eq!(
        continuity_store.load_snapshot_calls(),
        0,
        "lazy topology refresh must not hydrate snapshots"
    );
    assert!(
        runtime
            .mob_handle()
            .list_members_including_retiring()
            .await
            .is_empty(),
        "lazy topology refresh must not spawn members"
    );
    let status = runtime
        .identity_runtime()
        .unwrap()
        .status(&AgentIdentity::parse("agent:a").unwrap())
        .await
        .unwrap();
    assert_eq!(status.state, IdentityLifecycleState::Dormant);
}

#[tokio::test]
async fn identity_first_builder_lazy_run_flow_materializes_ob3_shaped_roster_before_flow_start() {
    let tmp = tempfile::tempdir().unwrap();
    let specs = vec![
        durable_spec("review:singleton"),
        durable_spec("initiative:alpha"),
        durable_spec("initiative:beta"),
    ];
    let topology = Arc::new(StubTopologyProvider {
        edges: Arc::new(tokio::sync::Mutex::new(vec![
            ManagedPeerEdge::new(
                AgentIdentity::parse("review:singleton").unwrap(),
                AgentIdentity::parse("initiative:alpha").unwrap(),
            )
            .unwrap(),
            ManagedPeerEdge::new(
                AgentIdentity::parse("review:singleton").unwrap(),
                AgentIdentity::parse("initiative:beta").unwrap(),
            )
            .unwrap(),
        ])),
    });

    let runtime = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(review_flow_definition())
            .continuity_store(Arc::new(StubContinuityStore))
            .lease_provider(Arc::new(StubLeaseProvider))
            .roster_provider(Arc::new(StubRosterProvider::new(specs)))
            .topology_provider(topology)
            .scratch_dir(tmp.path())
            .identity_runtime_instance_id("builder-lazy-flow-test")
            .identity_bootstrap_mode(IdentityBootstrapMode::LazyMaterialize)
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build(),
    )
    .await
    .expect("lazy builder should bootstrap");

    assert!(
        runtime
            .mob_handle()
            .list_members_including_retiring()
            .await
            .is_empty(),
        "lazy build must still start without concrete members"
    );

    let response: JsonRpcResponse = serde_json::from_str(
        &handle_unified_rpc_json(
            &runtime,
            &json!({
                "jsonrpc": "2.0",
                "id": "review-flow",
                "method": "mobkit/run_flow",
                "params": {
                    "flow_id": "review_cycle",
                    "params": { "source": "ob3" }
                }
            })
            .to_string(),
            Duration::from_secs(2),
            None,
            None,
        )
        .await,
    )
    .expect("json-rpc response");

    assert!(
        response.error.is_none(),
        "run_flow should hydrate lazy identities before concrete flow execution: {:?}",
        response.error
    );
    assert!(
        response
            .result
            .as_ref()
            .and_then(|result| result.get("run_id"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|run_id| !run_id.is_empty()),
        "run_flow should return a concrete run id"
    );

    let members = runtime.mob_handle().list_members_including_retiring().await;
    // meerkat 0.7: roster member ids are comms-safe encodings of the public
    // runtime-id aliases (MemberCommsName rejects ":"); the wire projection
    // (member_entry_to_json) decodes back to the alias space asserted below.
    let member_ids = members
        .iter()
        .map(|member| {
            meerkat_mobkit::mob_handle_runtime::member_entry_to_json(member)["agent_identity"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        member_ids.len(),
        3,
        "flow entrypoint must materialize the full review roster, got {member_ids:?}"
    );
    for expected in [
        "rt:review:singleton:0",
        "rt:initiative:alpha:0",
        "rt:initiative:beta:0",
    ] {
        assert!(
            member_ids.iter().any(|member_id| member_id == expected),
            "expected materialized member {expected}, got {member_ids:?}"
        );
    }

    let identity_runtime = runtime.identity_runtime().unwrap();
    for identity in ["review:singleton", "initiative:alpha", "initiative:beta"] {
        assert_eq!(
            identity_runtime
                .status(&AgentIdentity::parse(identity).unwrap())
                .await
                .unwrap()
                .state,
            IdentityLifecycleState::Active
        );
    }
}

#[tokio::test]
async fn identity_first_builder_runtime_checkpoint_follows_initial_session_save_version() {
    let tmp = tempfile::tempdir().unwrap();
    let roster = Arc::new(StubRosterProvider::new(vec![durable_spec("agent:alpha")]));
    let continuity_store = Arc::new(LocalContinuityStore::in_memory().unwrap());

    let runtime = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .continuity_store(continuity_store)
            .lease_provider(Arc::new(StubLeaseProvider))
            .roster_provider(roster)
            .scratch_dir(tmp.path())
            .identity_runtime_instance_id("builder-checkpoint-test")
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build(),
    )
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
        Box::pin(
            UnifiedRuntimeBuilder::default()
                .definition(test_definition())
                .continuity_store(continuity_store.clone())
                .lease_provider(Arc::new(StubLeaseProvider))
                .roster_provider(roster)
                .scratch_dir(tmp.path())
                .identity_runtime_instance_id("builder-resume-seed")
                .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
                .build(),
        )
        .await
        .expect("seed runtime should create continuity");
    }

    let tmp = tempfile::tempdir().unwrap();
    let roster = Arc::new(StubRosterProvider::new(vec![durable_spec("agent:alpha")]));
    let runtime = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .continuity_store(continuity_store)
            .lease_provider(Arc::new(StubLeaseProvider))
            .roster_provider(roster)
            .scratch_dir(tmp.path())
            .identity_runtime_instance_id("builder-resume-test")
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build(),
    )
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

    let runtime = Box::pin(
        UnifiedRuntimeBuilder::default()
            .definition(test_definition())
            .continuity_store(Arc::new(StubContinuityStore))
            .lease_provider(Arc::new(StubLeaseProvider))
            .roster_provider(roster)
            .topology_provider(Arc::new(StubTopologyProvider {
                edges: edges.clone(),
            }))
            .scratch_dir(tmp.path())
            .default_llm_client(Arc::new(meerkat_client::TestClient::default()))
            .build(),
    )
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
