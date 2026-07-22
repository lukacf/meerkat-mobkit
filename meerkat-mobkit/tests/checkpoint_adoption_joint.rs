//! H3 joint acceptance case (storage-unification plan): the
//! **independently-adopted byte-divergent pair**.
//!
//! On surfaces where the continuity store is NOT the meerkat session
//! authority (this builder's `persistent_state` arm; `mobkit_gateway`'s
//! identity-first mode), a continuity snapshot and the meerkat store row for
//! the SAME session can both be legacy (pre-typed) and byte-divergent — the
//! projection legitimately extends the snapshot by trailing turns. The two
//! copies are then adopted **independently**: H3 stamps the continuity copy
//! with the observed cursor from the continuity record; meerkat's resolver
//! adopts its own store row through its machine-owned migration at INITIAL
//! cursors (meerkat #909/#910). The acceptance gate is that a subsequent
//! resume through the bridge reaches a working verified authority — the
//! transcript carries — rather than an ambiguous-checkpoint terminal state.
//!
//! What this test does NOT cover (reported honestly): the shape where
//! meerkat's resolver sees BOTH adopted copies at once (runtime snapshot vs
//! adapter-loaded projection on an adapter-installed surface). That
//! arbitration is meerkat's machine-owned read-source verdict, proven in the
//! meerkat crates; this harness proves the two adoptions land on the same
//! deterministic lineage (`SessionLineageId::for_session`), which is what
//! keeps that future arbitration a within-lineage revision comparison
//! instead of a fail-closed `DifferentLineage`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use meerkat::SessionStore;
use meerkat_client::{LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::types::StopReason;
use meerkat_mob::{MobDefinition, ProfileName};
use meerkat_mobkit::UnifiedRuntimeBuilder;
use meerkat_mobkit::identity_first::contracts::{
    AgentCustomizer, ContinuityStore, TopologyProvider,
};
use meerkat_mobkit::identity_first::orchestrator::{RestoreOutcome, restore_flow};
use meerkat_mobkit::identity_first::{
    AdoptionMode, AgentAddressability, AgentBuildContext, AgentBuildDraft, AgentIdentity,
    CheckpointVersion, ContinuityRecord, ContinuityResolveState, CustomizerError, DurabilityPolicy,
    DurableAgentSpec, FencingToken, IdentityRuntime, IdentityRuntimeConfig, LocalContinuityStore,
    LocalLeaseProvider, ManagedPeerEdge, SessionBridge, SessionSnapshot, TopologyContext,
    TopologyError, adopt_continuity_snapshots,
};
use meerkat_mobkit::mob_handle_runtime::SessionCreatedContext;
use tokio::time::sleep;

fn id(name: &str) -> AgentIdentity {
    AgentIdentity::parse(name).unwrap()
}

fn spec(name: &str) -> DurableAgentSpec {
    DurableAgentSpec {
        identity: id(name),
        profile: ProfileName::from("personal"),
        addressability: AgentAddressability::Addressable,
        display_name: None,
        labels: BTreeMap::new(),
        context: None,
        additional_instructions: Vec::new(),
        initial_message: None,
        runtime_mode_override: None,
        backend: None,
        binding: None,
    }
}

const MOB_TOML: &str = r#"
[mob]
id = "checkpoint-adoption-joint"

[profiles.personal]
model = "gpt-5.5"
external_addressable = true
runtime_mode = "turn_driven"

[profiles.personal.tools]
comms = true
"#;

fn definition() -> MobDefinition {
    MobDefinition::from_toml(MOB_TOML).expect("parse mob definition")
}

struct EmptyTopology;
#[async_trait]
impl TopologyProvider for EmptyTopology {
    async fn compute_edges(
        &self,
        _target_identities: &[AgentIdentity],
        _context: &TopologyContext,
    ) -> Result<Vec<ManagedPeerEdge>, TopologyError> {
        Ok(vec![])
    }
}

struct NoopCustomizer;
#[async_trait]
impl AgentCustomizer for NoopCustomizer {
    async fn customize_build(
        &self,
        _context: &AgentBuildContext,
        _spec: &DurableAgentSpec,
        _draft: &mut AgentBuildDraft,
    ) -> Result<(), CustomizerError> {
        Ok(())
    }
    async fn after_create(
        &self,
        _identity: &AgentIdentity,
        _session_id: &meerkat_core::types::SessionId,
        _context: &SessionCreatedContext,
    ) -> Result<(), CustomizerError> {
        Ok(())
    }
}

/// Records the serialized LLM request each turn and answers "ok" so turns
/// complete without a real provider (cold-restart harness pattern).
#[derive(Clone, Default)]
struct CaptureClient {
    requests: Arc<std::sync::Mutex<Vec<String>>>,
}
impl CaptureClient {
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
    fn last(&self) -> Option<String> {
        self.requests.lock().unwrap().last().cloned()
    }
}
impl meerkat_client::LlmClient for CaptureClient {
    fn project_replay_messages(
        &self,
        messages: &[meerkat_core::Message],
    ) -> Result<Vec<meerkat_core::Message>, LlmError> {
        Ok(messages.to_vec())
    }
    fn stream<'a>(
        &'a self,
        request: &'a LlmRequest,
    ) -> Pin<Box<dyn futures::Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>> {
        self.requests
            .lock()
            .unwrap()
            .push(serde_json::to_string(request).unwrap_or_default());
        Box::pin(async_stream::stream! {
            yield Ok(LlmEvent::TextDelta { delta: "ok".to_string(), meta: None });
            yield Ok(LlmEvent::Done {
                outcome: LlmDoneOutcome::Success { stop_reason: StopReason::EndTurn },
            });
        })
    }
    fn provider(&self) -> meerkat::Provider {
        meerkat::Provider::OpenAI
    }
    fn health_check<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), LlmError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Ok(()) })
    }
}

/// Boot a runtime + identity runtime against `state_path`, mirroring the
/// production wiring (fencing floor seeded from the persisted high-water).
async fn boot(
    state_path: &Path,
    continuity_db: &Path,
    capture: CaptureClient,
) -> (meerkat_mobkit::UnifiedRuntime, IdentityRuntime) {
    let unified = UnifiedRuntimeBuilder::default()
        .definition(definition())
        .persistent_state(state_path)
        .comms(true)
        .default_llm_client(Arc::new(capture))
        .build()
        .await
        .expect("build UnifiedRuntime");
    let bridge: Arc<dyn SessionBridge> = unified
        .session_bridge()
        .expect("session_bridge should exist")
        .clone();
    let (store, fencing_floor) =
        LocalContinuityStore::open_with_fencing_floor(continuity_db.to_path_buf())
            .await
            .expect("continuity store");
    let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: Arc::new(store) as Arc<dyn ContinuityStore>,
        lease_provider: Arc::new(LocalLeaseProvider::with_floor(fencing_floor)),
        runtime_instance_id: "checkpoint-adoption-joint".to_string(),
        has_runtime_store: true,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(bridge),
        default_timeout: None,
    });
    (unified, identity_rt)
}

async fn deliver_and_wait(
    identity_rt: &IdentityRuntime,
    alice: &AgentIdentity,
    capture: &CaptureClient,
    text: &str,
) {
    identity_rt
        .send(alice, &meerkat_core::ContentInput::Text(text.to_string()))
        .await
        .expect("send turn");
    let deadline = Instant::now() + Duration::from_secs(30);
    while capture.count() < 1 {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the turn to reach the LLM"
        );
        sleep(Duration::from_millis(100)).await;
    }
    // Let the run-boundary commit land before shutdown.
    sleep(Duration::from_millis(500)).await;
}

/// Strip the typed checkpoint stamp from a session document, producing the
/// exact pre-typed (0.7.x) durable shape: same content, `LegacyUnverified`.
fn strip_stamp(session: &meerkat_core::Session) -> meerkat_core::Session {
    let mut value = serde_json::to_value(session).expect("serialize session");
    let removed = value
        .get_mut("metadata")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|metadata| metadata.remove(meerkat_core::SESSION_CHECKPOINT_STAMP_KEY));
    assert!(
        removed.is_some(),
        "fixture session must have carried a typed stamp to strip"
    );
    let stripped: meerkat_core::Session =
        serde_json::from_value(value).expect("deserialize stripped session");
    assert!(
        matches!(
            stripped.try_checkpoint_state().expect("state"),
            meerkat_core::SessionCheckpointState::LegacyUnverified { .. }
        ),
        "stripped session must decode as legacy-unverified"
    );
    stripped
}

fn verified_stamp(session: &meerkat_core::Session) -> meerkat_core::SessionCheckpointStamp {
    match session.try_checkpoint_state().expect("checkpoint state") {
        meerkat_core::SessionCheckpointState::Verified(stamp) => stamp,
        other => panic!("expected a verified document, got {other:?}"),
    }
}

const TOKEN_A: &str = "MARKER-JOINT-ALPHA-11";
const TOKEN_B: &str = "MARKER-JOINT-BRAVO-22";

#[tokio::test(flavor = "multi_thread")]
async fn independently_adopted_byte_divergent_pair_converges_on_resume() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let state_path = temp.path().join("state");
    let continuity_db = state_path.join("continuity.sqlite3");
    let session_db = state_path.join("sessions.sqlite3");
    let runtime_db = state_path.join("runtime.sqlite");
    let alice = id("personal:alice");
    let roster = vec![spec("personal:alice")];

    // --- Boot 1: create the member, deliver turn 1 (token A), shut down ---
    let session_id;
    {
        let capture = CaptureClient::default();
        let (unified, identity_rt) = boot(&state_path, &continuity_db, capture.clone()).await;
        let result = restore_flow(
            &identity_rt,
            &roster,
            Some(&EmptyTopology as &dyn TopologyProvider),
            Some(&NoopCustomizer as &dyn AgentCustomizer),
        )
        .await
        .expect("restore_flow (boot 1)");
        match result.outcomes.get(&alice).expect("alice outcome") {
            RestoreOutcome::Created { record, .. } => {
                session_id = record.session_id.clone();
            }
            other => panic!("expected Created on first boot, got {other:?}"),
        }
        deliver_and_wait(
            &identity_rt,
            &alice,
            &capture,
            &format!("Please note this token: {TOKEN_A}"),
        )
        .await;
        unified.shutdown().await;
    }

    // The turn-1 transcript is the SNAPSHOT copy of the divergent pair.
    let turn1_session = {
        let store = meerkat_store::SqliteSessionStore::open(&session_db).expect("open session db");
        store
            .load(&session_id)
            .await
            .expect("load after boot 1")
            .expect("session row present")
    };

    // --- Boot 2: resume, deliver turn 2 (token B), shut down ---
    {
        let capture = CaptureClient::default();
        let (unified, identity_rt) = boot(&state_path, &continuity_db, capture.clone()).await;
        let result = restore_flow(
            &identity_rt,
            &roster,
            Some(&EmptyTopology as &dyn TopologyProvider),
            Some(&NoopCustomizer as &dyn AgentCustomizer),
        )
        .await
        .expect("restore_flow (boot 2)");
        match result.outcomes.get(&alice).expect("alice outcome") {
            RestoreOutcome::Resumed { record, .. } => {
                assert_eq!(record.session_id, session_id);
            }
            other => panic!("expected Resumed on boot 2, got {other:?}"),
        }
        deliver_and_wait(
            &identity_rt,
            &alice,
            &capture,
            &format!("Also note this token: {TOKEN_B}"),
        )
        .await;
        unified.shutdown().await;
    }

    // --- Manufacture the legacy byte-divergent pair -----------------------
    // Projection (meerkat store row): the FULL turn-2 transcript, stamp
    // stripped — the pre-typed durable shape a 0.7.x fleet carried.
    // Snapshot (continuity store): the turn-1 prefix, stamp stripped.
    let projection_legacy;
    let snapshot_generation;
    let snapshot_revision;
    {
        let store = meerkat_store::SqliteSessionStore::open(&session_db).expect("open session db");
        let turn2_session = store
            .load(&session_id)
            .await
            .expect("load after boot 2")
            .expect("session row present");
        assert!(
            turn2_session.messages().len() > turn1_session.messages().len(),
            "turn 2 must have extended the transcript"
        );
        projection_legacy = strip_stamp(&turn2_session);
        // The projection verb bypasses the append-only guard: this is a
        // deliberate fixture rewrite back into the pre-typed durable shape.
        store
            .save_authoritative_projection(&projection_legacy)
            .await
            .expect("rewrite store row as legacy");

        let snapshot_legacy = strip_stamp(&turn1_session);
        let snapshot_bytes = serde_json::to_vec(&snapshot_legacy).expect("serialize snapshot");
        assert_ne!(
            snapshot_bytes,
            serde_json::to_vec(&projection_legacy).expect("serialize projection"),
            "the pair must be byte-divergent"
        );

        let continuity = LocalContinuityStore::open(&continuity_db).expect("continuity store");
        let resolved = continuity
            .resolve_many(std::slice::from_ref(&alice))
            .await
            .expect("resolve alice");
        let record: ContinuityRecord = match resolved.get(&alice).expect("alice state") {
            ContinuityResolveState::Ready { record } => record.clone(),
            other => panic!("expected Ready continuity state, got {other:?}"),
        };
        assert_eq!(record.session_id, session_id);
        let fence = continuity.max_fencing_token().expect("fencing high-water");
        snapshot_generation = record.generation;
        snapshot_revision = CheckpointVersion::new(record.checkpoint_version.get() + 1);
        continuity
            .save_session_snapshot(
                &alice,
                &session_id,
                record.generation,
                snapshot_revision,
                FencingToken::new(fence),
                &SessionSnapshot {
                    data: snapshot_bytes,
                },
            )
            .await
            .expect("seed legacy continuity snapshot");
    }
    // Clear the typed runtime snapshot written by boots 1-2 so meerkat's
    // resolver sees the pre-typed fleet shape (store row only). 0.7.x-era
    // runtime state is not reconstructible through public API; store-only is
    // the sanctioned fixture approximation and exercises meerkat's
    // MigrateStoreProjection disposition.
    for suffix in ["", "-wal", "-shm"] {
        let path = runtime_db.with_file_name(format!("runtime.sqlite{suffix}"));
        let _ = std::fs::remove_file(path);
    }

    // --- H3 batch: adopt the continuity copy with the OBSERVED cursor ----
    let report = adopt_continuity_snapshots(&continuity_db, AdoptionMode::Apply)
        .await
        .expect("H3 apply");
    assert_eq!(report.adopted, 1, "the seeded legacy snapshot must adopt");
    assert!(report.is_clean(), "refusals: {:?}", report.refused);

    // --- Boot 3: meerkat independently adopts ITS copy; resume must work --
    {
        let capture = CaptureClient::default();
        let (unified, identity_rt) = boot(&state_path, &continuity_db, capture.clone()).await;
        let result = restore_flow(
            &identity_rt,
            &roster,
            Some(&EmptyTopology as &dyn TopologyProvider),
            Some(&NoopCustomizer as &dyn AgentCustomizer),
        )
        .await
        .expect("restore_flow (boot 3)");
        match result.outcomes.get(&alice).expect("alice outcome") {
            RestoreOutcome::Resumed { record, .. } => {
                assert_eq!(
                    record.session_id, session_id,
                    "resume must stay bound to the durable session"
                );
            }
            other => {
                panic!("resume over the independently-adopted pair must succeed, got {other:?}")
            }
        }
        deliver_and_wait(
            &identity_rt,
            &alice,
            &capture,
            "What tokens did I give you earlier?",
        )
        .await;
        let last = capture.last().expect("turn 3 request");
        assert!(
            last.contains(TOKEN_A) && last.contains(TOKEN_B),
            "the post-adoption resume must carry the full transcript \
             (working verified authority, not an ambiguous-checkpoint terminal state)"
        );
        unified.shutdown().await;
    }

    // --- Two valid stamps over different bytes, one lineage ---------------
    let continuity_stamp = {
        let continuity = LocalContinuityStore::open(&continuity_db).expect("continuity store");
        let snap = continuity
            .load_session_snapshot(&session_id)
            .await
            .expect("load continuity snapshot")
            .expect("continuity snapshot present");
        let session: meerkat_core::Session =
            serde_json::from_slice(&snap.data).expect("decode continuity snapshot");
        verified_stamp(&session)
    };
    // H3's stamp binds the observed continuity cursor, never INITIAL blindly.
    assert_eq!(
        continuity_stamp.generation(),
        meerkat_core::SessionGeneration::new(snapshot_generation.get())
    );
    assert_eq!(
        continuity_stamp.checkpoint_revision(),
        meerkat_core::SessionCheckpointRevision::new(snapshot_revision.get())
    );

    let store_stamp = {
        let store = meerkat_store::SqliteSessionStore::open(&session_db).expect("open session db");
        let session = store
            .load(&session_id)
            .await
            .expect("load after boot 3")
            .expect("session row present");
        verified_stamp(&session)
    };
    // Meerkat's own migration rooted at INITIAL generation (generation-0
    // fleet), independent of H3's stamp.
    assert_eq!(
        store_stamp.generation(),
        meerkat_core::SessionGeneration::INITIAL
    );
    // Both adoptions land on the SAME deterministic lineage: any later
    // arbitration between the copies is a within-lineage revision comparison,
    // never a fail-closed DifferentLineage.
    assert_eq!(continuity_stamp.lineage_id(), store_stamp.lineage_id());
    assert_ne!(
        continuity_stamp.digest(),
        store_stamp.digest(),
        "the pair stays byte-divergent — two valid stamps over different bytes"
    );
}
