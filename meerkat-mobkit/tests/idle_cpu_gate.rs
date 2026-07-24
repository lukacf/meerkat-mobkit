//! Idle-CPU regression gate (HomeCore activation blocker, 2026-07).
//!
//! A fully idle mobkit gateway burned ~0.3 CPU cores PER durable member
//! forever: the mob actor's identity reconcile re-read and re-verified each
//! member's full persisted session document once per scan interval even when
//! nothing had changed (sha256 canonical-digest + serde of the whole
//! transcript, reached through `mob_handle_runtime` →
//! `PersistentSessionService::resolve_runtime_snapshot_read_source`). A
//! 17-member fleet consumed ~5 cores on a 4-core host.
//!
//! This gate boots a real persistent gateway with multiple durable members,
//! waits for convergence, idles, and asserts the PROCESS CPU-TIME delta over
//! the idle window stays far below the historical burn. It measures CPU time
//! via `getrusage`, never wall clock. This test must stay alone in its own
//! integration binary so no sibling test's CPU pollutes the measurement.
//!
//! The threshold is deliberately generous to CI noise (10% of one core over
//! the window); the historical defect consumed ~30% of a core per member and
//! scales with member count, so a hot-loop recurrence trips this
//! immediately. CAVEAT: the size-proportional REVERIFICATION class (per-pass
//! canonical-JSON hashing of large documents) needs a large transcript to
//! reproduce — small fixtures alone will not trip it. The meerkat
//! e2e-smoke-turbo-s idle gate carries the large-document fixture; this
//! gate covers the loop cadence itself.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use meerkat_client::{LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::types::StopReason;
use meerkat_mob::{MobDefinition, ProfileName};
use meerkat_mobkit::UnifiedRuntimeBuilder;
use meerkat_mobkit::identity_first::orchestrator::{RestoreOutcome, restore_flow};
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentIdentity, ContinuityStore, DurabilityPolicy, DurableAgentSpec,
    IdentityRuntime, IdentityRuntimeConfig, LocalContinuityStore, LocalLeaseProvider,
    SessionBridge,
};
use tokio::time::sleep;

const MOB_TOML: &str = r#"
[mob]
id = "idle-cpu-gate"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true
runtime_mode = "turn_driven"

[profiles.worker.tools]
comms = true
"#;

const MEMBER_COUNT: usize = 3;
const IDLE_WINDOW: Duration = Duration::from_secs(30);
/// 10% of one core over the idle window. The pre-fix defect consumed
/// ~0.3 core-seconds per second PER member (0.9 cores for this fleet),
/// i.e. ~27 CPU-seconds over this window.
const MAX_IDLE_CPU: Duration = Duration::from_secs(3);

fn identity(name: &str) -> AgentIdentity {
    AgentIdentity::parse(name).unwrap()
}

fn spec(name: &str) -> DurableAgentSpec {
    DurableAgentSpec {
        identity: identity(name),
        profile: ProfileName::from("worker"),
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

/// Answers "ok" to every turn and counts requests, so turns complete without
/// a live provider.
#[derive(Clone, Default)]
struct CaptureClient {
    requests: Arc<std::sync::Mutex<usize>>,
}

impl CaptureClient {
    fn count(&self) -> usize {
        *self.requests.lock().unwrap()
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
        _request: &'a LlmRequest,
    ) -> Pin<Box<dyn futures::Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>> {
        *self.requests.lock().unwrap() += 1;
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

/// Total (user + system) CPU time this process has consumed since start.
fn process_cpu_time() -> Duration {
    cpu_time::ProcessTime::try_now()
        .expect("read process CPU time")
        .as_duration()
}

#[tokio::test(flavor = "multi_thread")]
async fn converged_idle_gateway_consumes_near_zero_cpu() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let state_path = temp.path().join("state");
    let capture = CaptureClient::default();

    let unified = UnifiedRuntimeBuilder::default()
        .definition(MobDefinition::from_toml(MOB_TOML).expect("parse idle-cpu mob definition"))
        .persistent_state(&state_path)
        .comms(true)
        .default_llm_client(Arc::new(capture.clone()))
        .build()
        .await
        .expect("build UnifiedRuntime");
    let bridge: Arc<dyn SessionBridge> = unified
        .session_bridge()
        .expect("session_bridge should exist")
        .clone();
    let store = Arc::new(
        LocalContinuityStore::open(state_path.join("continuity.db")).expect("continuity store"),
    );
    let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store as Arc<dyn ContinuityStore>,
        lease_provider: Arc::new(LocalLeaseProvider::new()),
        runtime_instance_id: "idle-cpu-gate".to_string(),
        has_runtime_store: true,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(bridge),
        default_timeout: None,
    });

    let roster: Vec<DurableAgentSpec> = (0..MEMBER_COUNT)
        .map(|index| spec(&format!("worker:member-{index}")))
        .collect();
    let result = restore_flow(&identity_rt, &roster, None, None)
        .await
        .expect("restore_flow");
    for member in &roster {
        match result.outcomes.get(&member.identity) {
            Some(RestoreOutcome::Created { .. } | RestoreOutcome::Resumed { .. }) => {}
            other => panic!(
                "expected materialized member {}, got {other:?}",
                member.identity
            ),
        }
    }

    // Give every member a small persisted transcript, then let the turns and
    // their durable commits fully settle before the measured window.
    for member in &roster {
        identity_rt
            .send(
                &member.identity,
                &meerkat_core::ContentInput::Text(format!(
                    "fixture transcript for {}",
                    member.identity
                )),
            )
            .await
            .expect("seed turn");
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while capture.count() < MEMBER_COUNT {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for seed turns to reach the LLM"
        );
        sleep(Duration::from_millis(100)).await;
    }
    sleep(Duration::from_secs(3)).await;

    // The measured contract: a converged, idle gateway must consume ~zero
    // CPU regardless of member count or transcript size.
    let cpu_before = process_cpu_time();
    sleep(IDLE_WINDOW).await;
    let idle_cpu = process_cpu_time().saturating_sub(cpu_before);
    assert!(
        idle_cpu <= MAX_IDLE_CPU,
        "idle gateway burned {idle_cpu:?} CPU over {IDLE_WINDOW:?} \
         (limit {MAX_IDLE_CPU:?}); the converged fleet must be event-driven, \
         not busy re-verifying unchanged session documents"
    );

    // Cheap-idle must not mean dead: a member still serves a turn afterwards.
    let turns_before = capture.count();
    identity_rt
        .send(
            &roster[0].identity,
            &meerkat_core::ContentInput::Text("post-idle liveness probe".to_string()),
        )
        .await
        .expect("post-idle turn");
    let deadline = Instant::now() + Duration::from_secs(30);
    while capture.count() <= turns_before {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the post-idle turn to reach the LLM"
        );
        sleep(Duration::from_millis(100)).await;
    }

    unified.shutdown().await;
}
