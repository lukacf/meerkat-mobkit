//! OB3 cutover regression (2026-07-29): model and provider are a COHERENT
//! PAIR, never independently masked on resume.
//!
//! Incident shape: a definition edit moved a profile's `model` to another
//! provider's catalog entry with no `provider` key declared. The auto-marked
//! resume override applied the profile MODEL while the durable PROVIDER
//! survived, minting an invalid pair — (claude-fable-5, openai) — that the
//! model registry rejected typed on every resume, degrading the fleet's
//! channels. Required behavior: the pair is derived from the declared model
//! via the meerkat-models catalog and applied atomically, or neither field
//! applies (durable truth wins whole) with the unified resume-divergence
//! line as the tripwire.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use meerkat_client::{LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::types::StopReason;
use meerkat_mob::{MobDefinition, ProfileName};
use meerkat_mobkit::UnifiedRuntimeBuilder;
use meerkat_mobkit::identity_first::contracts::{AgentCustomizer, TopologyProvider};
use meerkat_mobkit::identity_first::orchestrator::{RestoreOutcome, restore_flow};
use meerkat_mobkit::identity_first::{
    AgentAddressability, AgentBuildContext, AgentBuildDraft, AgentIdentity, ContinuityStore,
    CustomizerError, DurabilityPolicy, DurableAgentSpec, IdentityRuntime, IdentityRuntimeConfig,
    LocalContinuityStore, LocalLeaseProvider, ManagedPeerEdge, SessionBridge, TopologyContext,
    TopologyError,
};
use meerkat_mobkit::mob_handle_runtime::SessionCreatedContext;
use tokio::time::sleep;

fn id(name: &str) -> AgentIdentity {
    AgentIdentity::parse(name).unwrap()
}

fn spec(name: &str, profile: &str) -> DurableAgentSpec {
    DurableAgentSpec {
        identity: id(name),
        profile: ProfileName::from(profile),
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
/// complete without a real provider (the injected client serves every
/// provider; the pair under test lives in the build config, not the client).
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

fn definition(mob_id: &str, model_lines: &str) -> MobDefinition {
    let toml = format!(
        "[mob]\nid = \"{mob_id}\"\n\n[profiles.personal]\n{model_lines}\nexternal_addressable = true\nruntime_mode = \"turn_driven\"\n\n[profiles.personal.tools]\ncomms = true\n"
    );
    MobDefinition::from_toml(&toml).expect("parse pair-coherence mob definition")
}

/// Build a fresh runtime + identity runtime against `state_path` with the
/// given definition — one process (re)start against a durable store.
async fn boot(
    state_path: &std::path::Path,
    capture: CaptureClient,
    definition: MobDefinition,
    runtime_instance: &str,
) -> (meerkat_mobkit::UnifiedRuntime, IdentityRuntime) {
    let unified = UnifiedRuntimeBuilder::default()
        .definition(definition)
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
    let store = Arc::new(
        LocalContinuityStore::open(state_path.join("continuity.db")).expect("continuity store"),
    );
    let identity_rt = IdentityRuntime::new(IdentityRuntimeConfig {
        continuity_store: store as Arc<dyn ContinuityStore>,
        lease_provider: Arc::new(LocalLeaseProvider::new()),
        runtime_instance_id: runtime_instance.to_string(),
        has_runtime_store: true,
        durability_policy: DurabilityPolicy::SyncWriteThrough,
        bridge: Some(bridge),
        default_timeout: None,
    });
    (unified, identity_rt)
}

async fn wait_for_request(capture: &CaptureClient, secs: u64, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while capture.count() < 1 {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what} to reach the LLM"
        );
        sleep(Duration::from_millis(100)).await;
    }
}

/// THE incident shape, end to end: boot 1 creates the member on
/// `gpt-5.5` (durable provider: openai). Boot 2's definition moves the
/// profile to `claude-opus-4-8` with NO provider key. The resume must apply
/// the (model, provider) pair atomically — provider derived from the
/// catalog — and succeed onto the same durable session with the transcript
/// intact. Before the pair-coherence fix this resume was REJECTED typed
/// ("model 'claude-opus-4-8' is registered for provider 'anthropic', not
/// provider 'openai'") because only the model was applied.
#[tokio::test(flavor = "multi_thread")]
async fn model_only_definition_edit_resumes_with_catalog_derived_pair() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let state_path = temp.path().join("state");
    let alice = id("personal:alice");
    let roster = vec![spec("personal:alice", "personal")];
    const TOKEN: &str = "MARKER-PAIR-3-OSCAR";

    // --- Boot 1: create on gpt-5.5, deliver turn 1, shut down ---
    let original_session_id;
    {
        let capture = CaptureClient::default();
        let (unified, identity_rt) = boot(
            &state_path,
            capture.clone(),
            definition("pair-coherence", "model = \"gpt-5.5\""),
            "pair-coherence-rt",
        )
        .await;

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
                original_session_id = record.session_id.clone();
            }
            other => panic!("expected Created on first boot, got {other:?}"),
        }

        identity_rt
            .send(
                &alice,
                &meerkat_core::ContentInput::Text(format!("Please note this token: {TOKEN}")),
            )
            .await
            .expect("send turn 1");
        wait_for_request(&capture, 20, "turn 1").await;
        // Let turn 1's assistant response commit before the shutdown flush.
        sleep(Duration::from_millis(500)).await;
        unified.shutdown().await;
    }

    // --- Boot 2: SAME store, definition edited to a model owned by another
    // provider, with no provider key. The pair must move together. ---
    {
        let capture = CaptureClient::default();
        let (unified, identity_rt) = boot(
            &state_path,
            capture.clone(),
            definition("pair-coherence", "model = \"claude-opus-4-8\""),
            "pair-coherence-rt",
        )
        .await;

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
                assert_eq!(
                    record.session_id, original_session_id,
                    "the cross-provider model edit must resume the SAME durable session"
                );
            }
            other => panic!(
                "a model-only definition edit must RESUME with the catalog-derived pair \
                 (the OB3 rejected-resume regression), got: {other:?}"
            ),
        }

        identity_rt
            .send(
                &alice,
                &meerkat_core::ContentInput::Text("What token did I give you earlier?".to_string()),
            )
            .await
            .expect("send turn 2");
        wait_for_request(&capture, 30, "the post-edit turn").await;

        let last_request = capture.last().expect("a post-edit request was captured");
        assert!(
            last_request.contains("\"model\":\"claude-opus-4-8\""),
            "the resumed turn must run on the DECLARED model (the edit was inert): {last_request}"
        );
        assert!(
            last_request.contains(TOKEN),
            "the resumed turn must replay the persisted transcript (token {TOKEN})"
        );

        // Let turn 2 persist the effective identity before reading it back.
        sleep(Duration::from_millis(500)).await;
        unified.shutdown().await;
    }

    // --- The durable pair must have moved ATOMICALLY: model and provider
    // both persisted, coherent with the catalog. ---
    {
        use meerkat::SessionStore as _;
        // Canonical spelling first, legacy spellings beside it (the storage
        // layout's own probe order).
        let db_path = ["sessions.sqlite3", "sessions.db", "sessions.sqlite"]
            .iter()
            .map(|name| state_path.join(name))
            .find(|path| path.exists())
            .expect("a session store file exists in the state dir");
        let store = meerkat_store::SqliteSessionStore::open(db_path)
            .expect("open the session store for the pair audit");
        let session = store
            .load(&original_session_id)
            .await
            .expect("load resumed session")
            .expect("resumed session exists");
        let metadata = session
            .session_metadata()
            .expect("resumed session carries session metadata");
        assert_eq!(
            metadata.model, "claude-opus-4-8",
            "durable metadata must carry the applied model"
        );
        assert_eq!(
            metadata.provider,
            meerkat_core::Provider::Anthropic,
            "durable metadata must carry the model's catalog owner — the pair moved together \
             or not at all"
        );
    }
}

/// Captures the process's tracing output so the test can assert on emitted
/// log lines. Installed as the global default: the runtime logs from tokio
/// worker threads, which a thread-local subscriber would miss.
#[derive(Clone, Default)]
struct LogCapture {
    buffer: Arc<std::sync::Mutex<Vec<u8>>>,
}
impl LogCapture {
    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.buffer.lock().unwrap()).into_owned()
    }
}
impl std::io::Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

const DIVERGENCE_LINE: &str = "resume restored an LLM identity";

/// When NO coherent (model, provider) pair is resolvable from the
/// declaration (a model the catalog does not know, no `provider` key, no
/// `[models.<id>]` entry), NOTHING may be half-applied and durable truth
/// must survive untouched.
///
/// Under meerkat 0.8.11 definition validation this state is refused at BOOT,
/// typed (`UnknownModel`, naming the profile and model) — a definition that
/// cannot resolve a coherent pair never reaches the resume path at all, so
/// the quiet unmasked-divergence state this test originally exercised
/// (durable-wins-whole resume + once-per-boot INFO tripwire) is
/// unrepresentable end to end for inline profiles: every load-valid inline
/// declaration is auto-marked as a coherent masked pair, and every
/// unresolvable one refuses to boot. The loud typed refusal replaced the
/// quiet INFO line as the tripwire. The pure divergence seam
/// (`unmasked_resume_divergence`) stays unit-covered in
/// `identity_first::bridge`.
///
/// This test encodes the surviving contract: the refused boot leaves the
/// durable store untouched (no half-applied pair, no lost transcript), the
/// refusal names the unresolvable declaration, and the next boot on a
/// resolvable definition resumes the SAME durable session with the
/// transcript intact — and no divergence line fires for that coherent,
/// masked resume.
#[tokio::test(flavor = "multi_thread")]
async fn unresolvable_model_edit_refuses_boot_and_durable_truth_survives() {
    let logs = LogCapture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(logs.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("this test owns the process's global tracing subscriber");

    let temp = tempfile::TempDir::new().expect("temp dir");
    let state_path = temp.path().join("state");
    let bob = id("personal:bob");
    let roster = vec![spec("personal:bob", "personal")];
    const TOKEN: &str = "MARKER-PAIR-7-VICTOR";

    // --- Boot 1: create on gpt-5.5, deliver a turn, shut down ---
    let original_session_id;
    {
        let capture = CaptureClient::default();
        let (unified, identity_rt) = boot(
            &state_path,
            capture.clone(),
            definition("pair-divergence", "model = \"gpt-5.5\""),
            "pair-divergence-rt",
        )
        .await;
        let result = restore_flow(
            &identity_rt,
            &roster,
            Some(&EmptyTopology as &dyn TopologyProvider),
            Some(&NoopCustomizer as &dyn AgentCustomizer),
        )
        .await
        .expect("restore_flow (boot 1)");
        match result.outcomes.get(&bob).expect("bob outcome") {
            RestoreOutcome::Created { record, .. } => {
                original_session_id = record.session_id.clone();
            }
            other => panic!("expected Created on first boot, got {other:?}"),
        }
        identity_rt
            .send(
                &bob,
                &meerkat_core::ContentInput::Text(format!("Please note this token: {TOKEN}")),
            )
            .await
            .expect("send turn 1");
        wait_for_request(&capture, 20, "turn 1").await;
        sleep(Duration::from_millis(500)).await;
        unified.shutdown().await;
    }

    // --- Boot 2: the profile now declares a model NOBODY can resolve a
    // provider for. No coherent pair exists, so the runtime must refuse to
    // boot, typed, naming the unresolvable declaration — never half-apply
    // it over the durable identity. ---
    {
        let capture = CaptureClient::default();
        let error = UnifiedRuntimeBuilder::default()
            .definition(definition(
                "pair-divergence",
                "model = \"model-nobody-knows\"",
            ))
            .persistent_state(&state_path)
            .comms(true)
            .default_llm_client(Arc::new(capture))
            .build()
            .await
            .err()
            .expect(
                "a declared model with no resolvable provider must refuse the boot typed, \
                 not half-apply over durable truth",
            );
        let rendered = format!("{error:?}");
        for needle in ["UnknownModel", "model-nobody-knows", "profiles.personal"] {
            assert!(
                rendered.contains(needle),
                "the boot refusal must name the unresolvable declaration \
                 (missing `{needle}`): {rendered}"
            );
        }
    }

    // --- Boot 3: back on a resolvable definition. The refused boot must
    // have left durable truth untouched: the SAME session resumes, the
    // transcript replays, the turn runs on the durable pair, and the
    // coherent masked resume fires no divergence line. ---
    {
        let capture = CaptureClient::default();
        let (unified, identity_rt) = boot(
            &state_path,
            capture.clone(),
            definition("pair-divergence", "model = \"gpt-5.5\""),
            "pair-divergence-rt",
        )
        .await;
        let result = restore_flow(
            &identity_rt,
            &roster,
            Some(&EmptyTopology as &dyn TopologyProvider),
            Some(&NoopCustomizer as &dyn AgentCustomizer),
        )
        .await
        .expect("restore_flow (boot 3)");
        match result.outcomes.get(&bob).expect("bob outcome") {
            RestoreOutcome::Resumed { record, .. } => {
                assert_eq!(
                    record.session_id, original_session_id,
                    "the refused boot must not have touched the durable session binding"
                );
            }
            other => panic!(
                "after a refused boot, a resolvable definition must resume the preserved \
                 durable session, got: {other:?}"
            ),
        }

        identity_rt
            .send(
                &bob,
                &meerkat_core::ContentInput::Text("What token did I give you earlier?".to_string()),
            )
            .await
            .expect("send post-refusal turn");
        wait_for_request(&capture, 30, "the post-refusal turn").await;
        let last_request = capture.last().expect("a post-refusal request was captured");
        assert!(
            last_request.contains("\"model\":\"gpt-5.5\""),
            "the resumed turn must run on the durable pair: {last_request}"
        );
        assert!(
            last_request.contains(TOKEN),
            "the resumed turn must replay the preserved transcript (token {TOKEN})"
        );

        sleep(Duration::from_millis(500)).await;
        unified.shutdown().await;
    }

    assert!(
        !logs.contents().contains(DIVERGENCE_LINE),
        "a coherent masked resume of an untouched durable identity must not fire the \
         divergence tripwire"
    );
}
