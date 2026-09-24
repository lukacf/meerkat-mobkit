//! Console composition over Meerkat's shared live host and generated custody.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use meerkat::experimental_gpt_live::{
    ExperimentalGptLiveOpenAuthority, ExperimentalLiveOpenAuthorityProvider as _,
    PublicGptLiveOpenAuthorityConfig, PublicGptLivePlaybackPolicy,
};
use meerkat::session_runtime::live_summary::{
    LiveContextBootstrapMode, LiveContextSummarizer, LiveContextSummaryError,
    LiveContextSummaryPolicy, LiveContextSummarySnapshot,
};
use meerkat_core::{Config, SessionId, SessionLlmIdentity};
use meerkat_session::{PersistentSessionService, SessionAgentBuilder};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::auth::{ConsoleLiveBindingAuthority, ConsoleLiveGrant};
use super::summary_window::{SummaryCache, SummaryKey, recent_window};
use super::{
    ConsoleVoiceController, ConsoleVoiceHost, ConsoleVoiceSession, VoiceContextPreparation,
    VoiceError,
};
use crate::access::{ACTION_AGENT_SEND, ACTION_AGENT_VIEW, AccessController};
use crate::live_contracts::{ExperimentalLiveChannelStatus, PendingLiveChannelHandle};
use crate::live_wiring::{
    AuthenticatedHttpLiveAuthority, LiveCapabilityProvider, LiveOperation, LiveRpcHandler,
    LiveRpcResponseDeliveryCustody, LiveSurfaceAuthority, capture_live_rpc_response_delivery,
};
use crate::public_live_config::{ConsoleVoiceSummaryConfig, PublicLiveRegistration};
use crate::unified_runtime::UnifiedRuntime;

const PROFILE: &str = "openai.gpt-live-1.client-context.v1";

/// Admission ceiling handed to Meerkat's summary policy. The policy refuses a
/// snapshot above this size instead of windowing it; the configured
/// `max_input_bytes` window is cut below, inside the summariser.
const SUMMARY_CAPTURE_CEILING_BYTES: usize = 4 * 1024 * 1024;
const SUMMARY_TIMEOUT: Duration = Duration::from_mins(1);

struct FactorySummarizer {
    factory: meerkat::AgentFactory,
    config: Config,
    machine: Arc<meerkat_runtime::MeerkatMachine>,
    summary: ConsoleVoiceSummaryConfig,
    cache: SummaryCache,
}

#[async_trait]
impl LiveContextSummarizer for FactorySummarizer {
    async fn summarize(
        &self,
        snapshot: LiveContextSummarySnapshot<'_>,
    ) -> Result<String, LiveContextSummaryError> {
        let started = std::time::Instant::now();
        let mut identity = snapshot.llm_identity().clone();
        if let Some(model) = &self.summary.model {
            identity.model.clone_from(model);
        }
        // Newest messages that fit the configured window; a size bound only.
        // A newest message larger than the window is summarised whole (the
        // bound degrades, it does not fail); Meerkat's capture ceiling has
        // already refused a truly oversized snapshot.
        let window = recent_window(snapshot.messages(), self.summary.max_input_bytes);
        let key = SummaryKey::new(
            snapshot.session_id(),
            snapshot.canonical_message_cursor(),
            &identity.model,
            window,
        )
        .map_err(|_| LiveContextSummaryError::Producer("summary input encoding".to_string()))?;
        if let Some(text) = self.cache.get(&key) {
            tracing::info!(
                model = %identity.model,
                elapsed_ms = started.elapsed().as_millis(),
                output_bytes = text.len(),
                window_messages = window.len(),
                total_messages = snapshot.messages().len(),
                cache = "hit",
                "console voice context summary finished"
            );
            return Ok(text);
        }
        let client = self
            .factory
            .build_llm_client_for_identity_with_auth_lease(
                &self.config,
                &identity,
                Some(self.machine.generated_auth_lease_handle()),
            )
            .await
            .map_err(|_| {
                LiveContextSummaryError::Producer("summary client is unavailable".to_string())
            })?;
        let result = super::summary::summarize_context(
            client.as_ref(),
            &identity.model,
            window,
            snapshot.max_output_bytes(),
        )
        .await;
        tracing::info!(
            model = %identity.model,
            elapsed_ms = started.elapsed().as_millis(),
            output_bytes = ?result.as_ref().ok().map(String::len),
            window_messages = window.len(),
            total_messages = snapshot.messages().len(),
            cache = "miss",
            error = ?result.as_ref().err(),
            "console voice context summary finished"
        );
        let text = result.map_err(|error| {
            LiveContextSummaryError::Producer(format!("summary rejected: {error:?}"))
        })?;
        self.cache.insert(key, text.clone());
        Ok(text)
    }
}

struct SharedHost {
    handle: meerkat_mob::MobHandle,
    identity_runtime: Option<Arc<crate::identity_first::IdentityRuntime>>,
    access: AccessController,
    binding: Arc<ConsoleLiveBindingAuthority>,
    authority: Arc<ExperimentalGptLiveOpenAuthority>,
    handler: LiveRpcHandler,
    principal: String,
    selection: meerkat_contracts::WireLiveExecutionIdentityOverrideV1,
}

struct Host(Arc<SharedHost>);

struct RejectPlaybackPublication;

#[async_trait]
impl meerkat::experimental_gpt_live::ExperimentalLivePublicObservationPublisher
    for RejectPlaybackPublication
{
    async fn publish(
        &self,
        _observation: meerkat::experimental_gpt_live::ExperimentalLivePublicObservation,
    ) -> Result<(), meerkat::experimental_gpt_live::ExperimentalLivePublicObservationDeliveryError>
    {
        // Console's unmeasured policy must never request a played-output
        // publication. Reject a policy mismatch rather than attest delivery.
        Err(meerkat::experimental_gpt_live::ExperimentalLivePublicObservationDeliveryError::Rejected)
    }
}

impl ConsoleVoiceController {
    /// Compose one console-only public Live registration. A gateway must not
    /// simultaneously install a different live context-mirror owner.
    /// Console voice as the gateway's only live door: composes its own live
    /// context over `service`/`machine` and needs no arbitration.
    pub fn with_live_host<B: SessionAgentBuilder + 'static>(
        runtime: &Arc<UnifiedRuntime>,
        service: Arc<PersistentSessionService<B>>,
        machine: Arc<meerkat_runtime::MeerkatMachine>,
        factory: meerkat::AgentFactory,
        config: Config,
        registration: PublicLiveRegistration,
    ) -> Result<Self, String> {
        let ctx = Arc::new(crate::live_wiring::attach_live(
            Arc::clone(&service),
            Arc::clone(&machine),
            &factory,
            config,
            String::new(),
            None,
        ));
        Self::compose_live_host(runtime, ctx, service, machine, factory, registration, None)
    }

    /// Console voice beside the external live channel: both doors open
    /// member channels through `ctx` (one adapter host, one provider
    /// registration) and take turns on its voice-path arbiter, newest
    /// engagement first.
    pub fn with_shared_live_context<B: SessionAgentBuilder + 'static>(
        runtime: &Arc<UnifiedRuntime>,
        ctx: Arc<crate::live_wiring::GatewayLiveContext>,
        service: Arc<PersistentSessionService<B>>,
        machine: Arc<meerkat_runtime::MeerkatMachine>,
        factory: meerkat::AgentFactory,
        registration: PublicLiveRegistration,
    ) -> Result<Self, String> {
        Self::compose_live_host(runtime, ctx, service, machine, factory, registration, None)
    }

    pub(crate) fn compose_live_host<B: SessionAgentBuilder + 'static>(
        runtime: &Arc<UnifiedRuntime>,
        ctx: Arc<crate::live_wiring::GatewayLiveContext>,
        service: Arc<PersistentSessionService<B>>,
        machine: Arc<meerkat_runtime::MeerkatMachine>,
        factory: meerkat::AgentFactory,
        registration: PublicLiveRegistration,
        summary_override: Option<(LiveContextSummaryPolicy, String)>,
    ) -> Result<Self, String> {
        let summary_config = registration.summary.clone();
        let access = runtime
            .access_controller()
            .cloned()
            .unwrap_or_else(AccessController::disabled);
        let binding = Arc::new(ConsoleLiveBindingAuthority::new(
            runtime.mob_handle(),
            Arc::clone(&machine),
            access.clone(),
            registration.binding.clone(),
        ));
        let arbiter = Arc::clone(&ctx.arbiter);
        let transport =
            Arc::new(meerkat::experimental_gpt_live::ExperimentalGptLiveWebrtcTransport::new());
        let authority =
            ExperimentalGptLiveOpenAuthority::new_public(PublicGptLiveOpenAuthorityConfig {
                agent_factory: factory.clone(),
                config_source: ctx.experimental_live_config_source(),
                binding_authority: binding.clone(),
                execution_identity: SessionLlmIdentity {
                    provider: meerkat_core::Provider::OpenAI,
                    model: meerkat::GPT_LIVE_PUBLIC_MODEL.to_string(),
                    self_hosted_server_id: None,
                    provider_params: None,
                    auth_binding: Some(registration.binding.clone()),
                },
                realm: registration.realm.clone(),
                transport: transport.clone(),
                voice: registration.voice,
                session_instructions: registration.session_instructions,
                // Per-member identity, peers, tools and skills, resolved at
                // open for the target session; see `capabilities`.
                session_instructions_preface: Some(Arc::new(
                    super::capabilities::MemberPreface::new(runtime.mob_handle()),
                )),
            })
            .and_then(|authority| {
                authority.with_public_playback_policy(
                    PublicGptLivePlaybackPolicy::ProviderManagedUnmeasured,
                )
            })
            .map_err(|error| error.to_string())?;
        // Unit fixtures must never negotiate with the real provider.
        #[cfg(test)]
        let authority = authority.with_test_base_url(
            summary_override
                .as_ref()
                .map_or("http://127.0.0.1:9", |(_, url)| url.as_str()),
        );
        let authority = Arc::new(authority);
        let summary = match summary_override {
            Some((policy, _)) => policy,
            None => LiveContextSummaryPolicy::new(
                Arc::new(FactorySummarizer {
                    factory: factory.clone(),
                    config: ctx.config_source.config().clone(),
                    machine: Arc::clone(&machine),
                    summary: summary_config.clone(),
                    cache: SummaryCache::default(),
                }),
                SUMMARY_CAPTURE_CEILING_BYTES,
                summary_config.max_output_bytes,
                SUMMARY_TIMEOUT,
            )
            .map_err(|error| error.to_string())?,
        }
        .with_bootstrap_mode(LiveContextBootstrapMode::Concurrent);
        let capability = LiveCapabilityProvider::public(
            Arc::new(factory),
            registration.realm,
            authority.clone(),
            transport,
            runtime
                .mob_runtime()
                .agent_mob_mcp_state()
                .ok_or("console voice requires the Mob MCP owner")?,
            Arc::new(RejectPlaybackPublication),
        );
        let handler = crate::live_wiring::live_rpc_handler_with_console_policy(
            ctx, service, machine, capability, summary,
        );
        let selection = serde_json::from_value(json!({ "version": "v1", "profile_id": PROFILE }))
            .map_err(|error| format!("invalid public live profile: {error}"))?;
        Ok(Self {
            host: Some(Arc::new(Host(Arc::new(SharedHost {
                handle: runtime.mob_handle(),
                identity_runtime: runtime.identity_runtime().cloned(),
                access,
                binding,
                authority,
                handler,
                principal: registration.principal,
                selection,
            })))),
            ..Self::default()
        }
        .with_arbiter(arbiter))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        Base64BlobStoreAdapter, BinaryBlobStore, DiscoverySpec, MobBootstrapOptions,
        MobBootstrapSpec, MobKitConfig, ObjectStoreBlobStore,
    };
    use std::sync::atomic::AtomicUsize;
    use tower::ServiceExt as _;

    // These real hosts share upstream process-wide projection budgets.
    static SHARED_HOST_TEST_LOCK: Mutex<()> = Mutex::const_new(());

    #[derive(Default)]
    pub(crate) struct ProviderCapture {
        creates: AtomicUsize,
        body: StdMutex<Option<Value>>,
        disconnect: tokio::sync::Notify,
        disconnected: tokio::sync::Notify,
        /// The concurrent bootstrap summary. Meerkat seeds a summary that is
        /// ready before the provider open as a developer `session.input` item
        /// and delivers a late one on the thinking lane only after the user's
        /// first turn on the channel, never as spoken commentary.
        /// Wire fragments (500 chars each) concatenated in arrival order.
        summary_append: StdMutex<Option<String>>,
        /// Summary fragments received so far (thinking lane).
        summary_received: AtomicUsize,
        /// Fragments acknowledged so far; acks are held until the test
        /// releases them, then every received fragment is acknowledged.
        summary_acknowledged: AtomicUsize,
        release_summary_append: tokio::sync::Notify,
        /// Every thinking-lane append, the late summary included.
        thinking_received: AtomicUsize,
        /// Instructions-lane appends. Meerkat 0.8.41 sends none for the
        /// summary: a ready one is seeded at create, a late one rides the
        /// thinking lane. Acknowledged immediately.
        instructions_received: AtomicUsize,
        /// The test asks the fixture to make the user speak: one
        /// `session.input_transcript.delta`, the provider event Meerkat
        /// treats as the user's first turn on the channel.
        speak: tokio::sync::Notify,
    }

    // Only the external provider is simulated. HTTP, WebSocket sideband,
    // shared Meerkat custody and activation are exercised through real owners.
    pub(crate) struct ProviderFixture {
        pub(crate) url: String,
        pub(crate) capture: Arc<ProviderCapture>,
        server: tokio::task::JoinHandle<()>,
    }

    impl Drop for ProviderFixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    impl ProviderFixture {
        pub(crate) async fn start() -> Self {
            use axum::extract::State;
            use axum::extract::ws::{Message as SocketMessage, WebSocketUpgrade};
            use axum::response::IntoResponse as _;
            use axum::routing::{get, post};
            async fn create(
                State(capture): State<Arc<ProviderCapture>>,
                axum::Json(body): axum::Json<Value>,
            ) -> impl axum::response::IntoResponse {
                capture.creates.fetch_add(1, Ordering::SeqCst);
                *capture.body.lock().expect("capture") = Some(body);
                (
                    axum::http::StatusCode::CREATED,
                    axum::Json(json!({
                        "session":{"id":"console_voice_fixture"},
                        "transport":{"type":"webrtc","sdp":"v=0\r\nCONSOLE_FIXTURE_ANSWER"}
                    })),
                )
            }
            async fn attach(
                State(capture): State<Arc<ProviderCapture>>,
                upgrade: WebSocketUpgrade,
            ) -> axum::response::Response {
                upgrade.on_upgrade(move |mut socket| async move {
                    let session = json!({"id":"console_voice_fixture","model":"gpt-live-1","status":"active","expires_at":12345.5});
                    socket.send(SocketMessage::Text(json!({
                        "type":"session.started","event_id":"started","session":session
                    }).to_string().into())).await.expect("session started");
                    let mut held_summary_acks: Vec<Value> = Vec::new();
                    let mut summary_released = false;
                    loop {
                        let message = tokio::select! {
                            () = capture.disconnect.notified() => {
                                drop(socket);
                                capture.disconnected.notify_one();
                                return;
                            }
                            () = capture.release_summary_append.notified(), if !summary_released => {
                                summary_released = true;
                                for client_event_id in held_summary_acks.drain(..) {
                                    socket.send(SocketMessage::Text(json!({
                                        "type":"session.thinking.appended","event_id":"thinking-ack",
                                        "client_event_id":client_event_id,"start_ms":0.0,"end_ms":0.0
                                    }).to_string().into())).await.expect("thinking acknowledgement");
                                    capture.summary_acknowledged.fetch_add(1, Ordering::SeqCst);
                                }
                                continue;
                            }
                            () = capture.speak.notified() => {
                                socket.send(SocketMessage::Text(json!({
                                    "type":"session.input_transcript.delta","event_id":"first-user-delta",
                                    "delta":"hello","start_ms":0.0,"end_ms":600.0
                                }).to_string().into())).await.expect("user transcript delta");
                                continue;
                            }
                            message = socket.recv() => message,
                        };
                        let Some(Ok(message)) = message else { break; };
                        let SocketMessage::Text(text) = message else { continue; };
                        let event: Value = serde_json::from_str(&text).expect("provider command");
                        match event["type"].as_str() {
                            Some("session.instructions.append") => {
                                capture.instructions_received.fetch_add(1, Ordering::SeqCst);
                                socket.send(SocketMessage::Text(json!({
                                    "type":"session.instructions.appended","event_id":"instructions-ack",
                                    "client_event_id":event["event_id"],"start_ms":0.0,"end_ms":0.0
                                }).to_string().into())).await.expect("instructions acknowledgement");
                            }
                            Some("session.thinking.append") => {
                                // The late summary arrives on the thinking
                                // lane as pipelined fragments; the ack of
                                // each is held until the test releases the
                                // lane so the "delivering" stage stays
                                // observable.
                                let fragment = event["content"].as_str().unwrap_or_default().to_string();
                                capture
                                    .summary_append
                                    .lock()
                                    .expect("summary capture")
                                    .get_or_insert_with(String::new)
                                    .push_str(&fragment);
                                capture.summary_received.fetch_add(1, Ordering::SeqCst);
                                capture.thinking_received.fetch_add(1, Ordering::SeqCst);
                                if summary_released {
                                    socket.send(SocketMessage::Text(json!({
                                        "type":"session.thinking.appended","event_id":"thinking-ack",
                                        "client_event_id":event["event_id"],"start_ms":0.0,"end_ms":0.0
                                    }).to_string().into())).await.expect("thinking acknowledgement");
                                    capture.summary_acknowledged.fetch_add(1, Ordering::SeqCst);
                                } else {
                                    held_summary_acks.push(event["event_id"].clone());
                                }
                            }
                            Some("session.commentary.append") => {
                                socket.send(SocketMessage::Text(json!({
                                    "type":"session.commentary.appended","event_id":"append-ack",
                                    "client_event_id":event["event_id"],"start_ms":0.0,"end_ms":0.0
                                }).to_string().into())).await.expect("append acknowledgement");
                            }
                            Some("session.close") => {
                                socket.send(SocketMessage::Text(json!({
                                    "type":"session.closed","event_id":"closed","session":session,
                                    "reason":"close_requested","usage":{"seconds":0.1}
                                }).to_string().into())).await.expect("close acknowledgement");
                                break;
                            }
                            _ => {}
                        }
                    }
                }).into_response()
            }
            let capture = Arc::new(ProviderCapture::default());
            let app = axum::Router::new()
                .route("/health", get(|| async { "ok" }))
                .route("/v1/live/sessions", post(create))
                .route("/v1/live/sessions/{session_id}/attach", get(attach))
                .with_state(capture.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("fixture listener");
            let address = listener.local_addr().expect("fixture address");
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.expect("fixture server");
            });
            let health = reqwest::get(format!("http://{address}/health"))
                .await
                .expect("fixture health");
            assert_eq!(health.status(), reqwest::StatusCode::OK);
            Self {
                url: format!("http://{address}/v1/"),
                capture,
                server,
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum SummaryScenario {
        Success,
        Failure,
        CancelWhileGenerating,
    }

    pub(crate) struct Summary {
        pub(crate) calls: Arc<AtomicUsize>,
        pub(crate) release: Arc<tokio::sync::Notify>,
        pub(crate) cancelled: Arc<std::sync::atomic::AtomicBool>,
        pub(crate) scenario: SummaryScenario,
    }

    struct SummaryCancellation {
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        finished: bool,
    }

    impl Drop for SummaryCancellation {
        fn drop(&mut self) {
            if !self.finished {
                self.cancelled.store(true, Ordering::SeqCst);
            }
        }
    }

    #[async_trait]
    impl LiveContextSummarizer for Summary {
        async fn summarize(
            &self,
            snapshot: LiveContextSummarySnapshot<'_>,
        ) -> Result<String, LiveContextSummaryError> {
            assert_eq!(snapshot.llm_identity().model, "gpt-5.5");
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut cancellation = SummaryCancellation {
                cancelled: self.cancelled.clone(),
                finished: false,
            };
            self.release.notified().await;
            cancellation.finished = true;
            if self.scenario == SummaryScenario::Failure {
                Err(LiveContextSummaryError::Producer(
                    "private fixture producer detail".to_string(),
                ))
            } else {
                Ok("The background agent is configured for the test conversation.".to_string())
            }
        }
    }

    pub(crate) fn config() -> Config {
        let mut config = Config::default();
        let mut realm = meerkat_core::RealmConfigSection::default();
        realm.backend.insert(
            "api".to_string(),
            meerkat_core::BackendProfileConfig {
                provider: "openai".to_string(),
                backend_kind: "openai_api".to_string(),
                base_url: None,
                options: Value::Null,
                server: None,
            },
        );
        realm.auth.insert(
            "key".to_string(),
            meerkat_core::AuthProfileConfig {
                provider: "openai".to_string(),
                auth_method: "api_key".to_string(),
                source: meerkat_core::CredentialSourceSpec::InlineSecret {
                    secret: "sk-voice-test-only".to_string(),
                },
                constraints: Default::default(),
                metadata_defaults: Default::default(),
            },
        );
        realm.binding.insert(
            "openai".to_string(),
            meerkat_core::ProviderBindingConfig {
                backend_profile: "api".to_string(),
                auth_profile: "key".to_string(),
                credential_account: None,
                default_model: None,
                policy: Default::default(),
                provider_default: true,
            },
        );
        config.realm.insert("voice".to_string(), realm);
        config
    }

    pub(crate) async fn rpc(app: &axum::Router, token: &str, method: &str, params: Value) -> Value {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/console/rpc")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(axum::body::Body::from(
                        json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("body"),
        )
        .expect("response JSON")
    }

    #[tokio::test]
    async fn console_voice_actual_shared_host_opens_owned_pending_channel_without_mutating_background()
     {
        exercise_shared_host(false, SummaryScenario::Success).await;
    }

    #[tokio::test]
    async fn console_voice_actual_shared_host_closes_after_provider_disconnect() {
        exercise_shared_host(true, SummaryScenario::Success).await;
    }

    #[tokio::test]
    async fn console_voice_concurrent_summary_failure_is_typed_and_does_not_gate_activation() {
        exercise_shared_host(false, SummaryScenario::Failure).await;
    }

    #[tokio::test]
    async fn console_voice_close_cancels_exact_pending_summary_and_fences_status() {
        exercise_shared_host(false, SummaryScenario::CancelWhileGenerating).await;
    }

    async fn wait_context(
        app: &axum::Router,
        token: &str,
        pending: &Value,
        expected: Value,
    ) -> Value {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let status = rpc(app, token, super::super::VOICE_CONTEXT_STATUS_METHOD, json!({
                    "identity":"agent-a","request_id":"voice-request","channel_id":pending["channel_id"],
                })).await;
                assert!(status["error"].is_null(), "{status}");
                if status["result"]["context_preparation"] == expected { break status; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("actual context preparation transition")
    }

    /// Everything a real-host test needs: a fixture provider, SQLite stores,
    /// the persistent service, the machine, the factory and a bootstrapped
    /// runtime with member `agent-a` spawned.
    struct RealRuntime {
        _directory: tempfile::TempDir,
        provider: ProviderFixture,
        runtime: Arc<UnifiedRuntime>,
        service: Arc<PersistentSessionService<meerkat::FactoryAgentBuilder>>,
        machine: Arc<meerkat_runtime::MeerkatMachine>,
        factory: meerkat::AgentFactory,
        config: Config,
    }
    impl RealRuntime {
        async fn start(suffix: &str) -> Self {
            // Several tests share a logical suffix and `cargo test` runs them
            // on parallel threads in one process. The mob id becomes the comms
            // participant name and the discovery namespace is shared, so two
            // fixtures with the same suffix collided on
            // `ParticipantNameOccupied` and saw each other's history rows.
            // Derive both from a process-unique nonce.
            static FIXTURE_NONCE: std::sync::atomic::AtomicUsize =
                std::sync::atomic::AtomicUsize::new(0);
            let nonce = FIXTURE_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let unique = format!("{suffix}-{}-{nonce}", std::process::id());
            let provider = ProviderFixture::start().await;
            std::fs::create_dir_all(".rct").expect("test state parent");
            let directory = tempfile::Builder::new()
                .prefix("console-voice-")
                .tempdir_in(".rct")
                .expect("test state");
            let store: Arc<dyn meerkat::SessionStore> = Arc::new(
                meerkat_store::SqliteSessionStore::open(directory.path().join("sessions.sqlite"))
                    .expect("session store"),
            );
            let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
                meerkat_runtime::store::SqliteRuntimeStore::new(
                    directory.path().join("runtime.sqlite"),
                )
                .expect("runtime store"),
            );
            let binary: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
            let blobs: Arc<dyn meerkat_core::BlobStore> =
                Arc::new(Base64BlobStoreAdapter::new(binary.clone()));
            let machine = Arc::new(meerkat_runtime::MeerkatMachine::persistent(
                runtime_store.clone(),
                blobs.clone(),
            ));
            let factory = meerkat::AgentFactory::new(directory.path())
                .session_store(store.clone())
                .runtime_root(directory.path())
                .project_root(directory.path())
                .builtins(false)
                .mob(true)
                .comms(true);
            let config = config();
            let client = Arc::new(meerkat_client::TestClient::for_provider(
                meerkat_core::Provider::OpenAI,
            ));
            let mut builder = meerkat::FactoryAgentBuilder::new(factory.clone(), config.clone());
            builder.default_llm_client = Some(client.clone());
            builder.default_blob_store = Some(blobs.clone());
            let mob_tools = Arc::clone(&builder.default_mob_tools);
            let service = Arc::new(PersistentSessionService::new(
                builder,
                16,
                store,
                runtime_store,
                blobs,
            ));
            let definition = meerkat_mob::MobDefinition::from_toml(&format!(
                r#"
    [mob]
    id = "console-voice-{unique}"
    [profiles.agent]
    model = "gpt-5.5"
    external_addressable = true
    skills = ["voice-notes"]
    [profiles.agent.tools]
    comms = true
    [skills.voice-notes]
    source = "inline"
    content = "Keep spoken answers short."
    "#,
            ))
            .expect("definition");
            let mut spec = MobBootstrapSpec::new(
                definition,
                meerkat_mob::MobStorage::in_memory(),
                service.clone(),
            )
            .with_session_runtime_adapter(machine.clone())
            .with_options(MobBootstrapOptions {
                allow_ephemeral_sessions: false,
                notify_orchestrator_on_resume: true,
                default_llm_client: Some(client),
            });
            spec = spec.with_agent_mob_tools(mob_tools);
            spec.runtime_adapter = Some(machine.clone());
            spec.binary_blob_store = Some(binary);
            let runtime = Arc::new(
                UnifiedRuntime::bootstrap(
                    spec,
                    MobKitConfig {
                        modules: vec![],
                        pre_spawn: vec![],
                        discovery: DiscoverySpec {
                            namespace: format!("voice-test-{unique}"),
                            modules: vec![],
                        },
                    },
                    Duration::from_secs(2),
                )
                .await
                .expect("runtime"),
            );
            runtime
                .spawn(meerkat_mob::SpawnMemberSpec::from_wire(
                    "agent".to_string(),
                    "agent-a".to_string(),
                    None,
                    None,
                    None,
                ))
                .await
                .expect("member");
            // The spawn returns once the member is durable, but its kickoff
            // turn ("You have been spawned as ...", answered by the test
            // client) commits asynchronously. Tests snapshot the source
            // transcript and assert that voice setup leaves it unchanged, so
            // a kickoff row landing between their `before` and `after` reads
            // is a false rewrite. Wait for the kickoff exchange to be durable
            // before handing the fixture out; on a two-core runner the gap
            // was wide enough to trip the assertion.
            let session = runtime
                .mob_handle()
                .resolve_bridge_session_id(&meerkat_mob::AgentIdentity::from("agent-a"))
                .await
                .expect("source session for kickoff settle");
            crate::test_wait::poll_until(
                "the spawn kickoff exchange of agent-a is durable",
                crate::test_wait::STRUCTURAL_BACKSTOP,
                async || {
                    let snapshot = service
                        .export_realtime_refresh_session_snapshot(&session)
                        .await
                        .expect("kickoff settle snapshot");
                    let messages = snapshot.messages();
                    let kickoff_seen = messages.iter().any(|message| match message {
                        meerkat_core::types::Message::User(user) => user.content.iter().any(|block| {
                            matches!(block, meerkat_core::types::ContentBlock::Text { text } if text.starts_with("You have been spawned as"))
                        }),
                        _ => false,
                    });
                    kickoff_seen && matches!(messages.last(), Some(meerkat_core::types::Message::BlockAssistant(_)))
                },
            )
            .await;
            Self {
                _directory: directory,
                provider,
                runtime,
                service,
                machine,
                factory,
                config,
            }
        }
    }
    /// Console voice and the external `mobkit/live/*` door share one live
    /// context and take turns on its voice-path arbiter, newest engagement
    /// first, each loser closed through its own sequence with a typed reason.
    #[tokio::test]
    async fn console_voice_and_external_live_take_turns_on_the_shared_voice_path() {
        let _guard = SHARED_HOST_TEST_LOCK.lock().await;
        let contract: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/console_voice_v1.json"))
                .expect("shared voice contract");
        let RealRuntime {
            _directory,
            provider,
            runtime,
            service,
            machine,
            factory,
            config,
        } = RealRuntime::start("shared-owner").await;
        runtime
            .spawn(meerkat_mob::SpawnMemberSpec::from_wire(
                "agent".to_string(),
                "agent-b".to_string(),
                None,
                None,
                None,
            ))
            .await
            .expect("external member");
        let session_b = runtime
            .mob_handle()
            .resolve_bridge_session_id(&meerkat_mob::AgentIdentity::from("agent-b"))
            .await
            .expect("external member session");

        // ONE live context for both doors, exactly as the gateway composes it.
        // The external door's provider open goes through the context's
        // realtime session factory, which in production resolves a real
        // credential; unit fixtures must never negotiate with the provider,
        // so the shared context carries the scripted factory instead (the
        // console door negotiates through the fixture provider above).
        let realtime =
            Arc::new(meerkat::test_fixtures::realtime::ScriptedRealtimeSessionFactory::new());
        let mut shared = crate::live_wiring::attach_live(
            Arc::clone(&service),
            Arc::clone(&machine),
            &factory,
            config,
            "ws://127.0.0.1/shared-owner".to_string(),
            None,
        );
        shared.session_factory = Arc::clone(&realtime)
            as Arc<dyn meerkat_client::realtime_session::RealtimeSessionFactory>;
        let ctx = Arc::new(shared);
        let notifications: Arc<StdMutex<Vec<(String, Value)>>> = Arc::default();
        let sink = Arc::clone(&notifications);
        ctx.arbiter.set_notifier(Arc::new(move |method, params| {
            sink.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((method.to_string(), params));
        }));
        let release_summary = Arc::new(tokio::sync::Notify::new());
        release_summary.notify_one();
        let policy = LiveContextSummaryPolicy::new(
            Arc::new(Summary {
                calls: Arc::new(AtomicUsize::new(0)),
                release: Arc::clone(&release_summary),
                cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                scenario: SummaryScenario::Success,
            }),
            4 * 1024 * 1024,
            4096,
            Duration::from_secs(30),
        )
        .expect("summary policy");
        let registration = PublicLiveRegistration::parse(&json!({
            "principal":"voice@example.com","realm":"voice",
            "auth_binding":{"realm":"voice","binding":"openai"},"voice":"marin"
        }))
        .expect("registration");
        let controller = ConsoleVoiceController::compose_live_host(
            &runtime,
            Arc::clone(&ctx),
            service.clone(),
            Arc::clone(&machine),
            factory.clone(),
            registration,
            Some((policy, provider.url.clone())),
        )
        .expect("console host");
        let external = crate::live_wiring::live_rpc_handler_with_capabilities(
            Arc::clone(&ctx),
            service.clone(),
            Arc::clone(&machine),
            crate::live_wiring::LiveCapabilityProvider::disabled(),
        );
        let external_open = |rpc_id: &str| {
            external.dispatch(
                crate::live_wiring::LiveSurfaceAuthority::host_trusted_stdio(),
                Some(session_b.clone()),
                Some("agent-b".to_string()),
                "mobkit/live/open".to_string(),
                // The member's text model is not realtime-capable; reachyd
                // selects the realtime lane per open exactly like this.
                json!({"identity":"agent-b", "model":"gpt-realtime-2"}),
                json!(rpc_id),
            )
        };
        let external_call = |method: &str, params: Value, rpc_id: &str| {
            external.dispatch(
                crate::live_wiring::LiveSurfaceAuthority::host_trusted_stdio(),
                Some(session_b.clone()),
                Some("agent-b".to_string()),
                method.to_string(),
                params,
                json!(rpc_id),
            )
        };
        let decisions = crate::console_auth_config::parse_console_auth_config(&json!({
            "shared_secret":"console-test-signing", "email_allowlist":["voice@example.com"]
        }))
        .expect("auth");
        let token = jsonwebtoken::encode(&jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256), &json!({
                "iss":"http://127.0.0.1/mobkit-gateway","aud":"persistent-gateway",
                "sub":"voice@example.com","email":"voice@example.com","exp":chrono::Utc::now().timestamp()+300
            }), &jsonwebtoken::EncodingKey::from_secret(b"console-test-signing")).expect("token");
        let app = runtime
            .build_reference_app_router(decisions)
            .layer(axum::Extension(controller.clone()));

        // Phase 1: the console holds the path. Its channel is bound to the
        // engagement, so the external door knows exactly what it preempts.
        let opened = rpc(
            &app,
            &token,
            "mobkit/console/voice/open",
            json!({"identity":"agent-a","request_id":"voice-first"}),
        )
        .await;
        assert!(opened["error"].is_null(), "{opened}");
        let console_channel = opened["result"]["channel_id"]
            .as_str()
            .expect("console channel")
            .to_string();
        assert_eq!(
            ctx.arbiter.holder().await,
            Some(crate::live_wiring::LiveOwner::ConsoleVoice {
                principal: "voice@example.com".to_string(),
                identity: "agent-a".to_string(),
                channel_id: Some(console_channel.clone()),
            })
        );

        // Phase 2: an external open preempts the console call. The open
        // result names what it superseded; the console learns why through
        // its own replacement poll; readiness names the new holder; the
        // stdio notification fires at once.
        let external_first = external_open("external-first").await;
        assert!(external_first.error.is_none(), "{external_first:?}");
        let external_result = external_first.result.clone().expect("external open result");
        let external_channel = external_result["channel_id"]
            .as_str()
            .expect("external channel")
            .to_string();
        assert_eq!(
            external_result["superseded"],
            json!({"owner":"console_voice","identity":"agent-a","channel_id":console_channel}),
        );
        assert_eq!(
            realtime.open_count(),
            1,
            "the external door opened through the scripted realtime factory"
        );
        let replacement = rpc(
            &app,
            &token,
            "mobkit/console/voice/replacement",
            json!({"identity":"agent-a","request_id":"voice-first"}),
        )
        .await;
        assert_eq!(
            replacement["error"], contract["superseded_error"],
            "{replacement}"
        );
        let readiness = rpc(
            &app,
            &token,
            "mobkit/console/voice/readiness",
            json!({"identity":"agent-a"}),
        )
        .await;
        let mut expected_readiness = contract["readiness_external_live_active"].clone();
        expected_readiness["holder"]["channel_id"] = json!(external_channel);
        assert_eq!(readiness["result"], expected_readiness, "{readiness}");
        {
            let notified = notifications
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert_eq!(notified.len(), 1, "{notified:?}");
            assert_eq!(notified[0].0, "mobkit/live/superseded");
            assert_eq!(notified[0].1["owner"], "console_voice");
            assert_eq!(notified[0].1["identity"], "agent-a");
            assert_eq!(notified[0].1["channel_id"], console_channel);
            assert_eq!(notified[0].1["reason"], "superseded_by_external_live");
            assert_eq!(notified[0].1["superseded_by"]["owner"], "external_live");
        }
        // The console's own close of the superseded call is idempotent.
        let closed = rpc(
            &app,
            &token,
            "mobkit/console/voice/close",
            json!({"identity":"agent-a","request_id":"voice-first"}),
        )
        .await;
        assert_eq!(closed["result"], json!({"phase":"closed"}), "{closed}");
        assert_eq!(
            ctx.arbiter.holder().await.map(|owner| owner.kind()),
            Some("external_live"),
            "a superseded console close never evicts the external owner"
        );

        // Phase 3: a console open takes the path back. The external channel is
        // closed through the external door's own sequence and its status
        // reports the typed reason; the notification names the console.
        let reopened = rpc(
            &app,
            &token,
            "mobkit/console/voice/open",
            json!({"identity":"agent-a","request_id":"voice-second"}),
        )
        .await;
        assert!(reopened["error"].is_null(), "{reopened}");
        let second_console_channel = reopened["result"]["channel_id"]
            .as_str()
            .expect("second console channel")
            .to_string();
        let status = external_call(
            "mobkit/live/status",
            json!({"identity":"agent-b","channel_id":external_channel}),
            "external-status",
        )
        .await;
        assert!(status.error.is_none(), "{status:?}");
        let status = status.result.expect("status result");
        assert_eq!(status["status"], json!({"status":"closed"}), "{status}");
        assert_eq!(
            status["close_reason"], "superseded_by_console_voice",
            "{status}"
        );
        {
            let notified = notifications
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert_eq!(notified.len(), 2, "{notified:?}");
            assert_eq!(notified[1].1["owner"], "external_live");
            assert_eq!(notified[1].1["identity"], "agent-b");
            assert_eq!(notified[1].1["channel_id"], external_channel);
            assert_eq!(notified[1].1["reason"], "superseded_by_console_voice");
            assert_eq!(notified[1].1["superseded_by"]["owner"], "console_voice");
        }
        let readiness = rpc(
            &app,
            &token,
            "mobkit/console/voice/readiness",
            json!({"identity":"agent-a"}),
        )
        .await;
        assert_eq!(
            readiness["result"], contract["readiness_available"],
            "{readiness}"
        );

        // Phase 4: the external door wins again, then closes on its own. An
        // ordinary close frees the path and carries no close reason.
        let external_second = external_open("external-second").await;
        assert!(external_second.error.is_none(), "{external_second:?}");
        let external_result = external_second.result.expect("second external open");
        assert_eq!(
            external_result["superseded"],
            json!({"owner":"console_voice","identity":"agent-a","channel_id":second_console_channel}),
        );
        let second_external_channel = external_result["channel_id"]
            .as_str()
            .expect("second external channel")
            .to_string();
        let replacement = rpc(
            &app,
            &token,
            "mobkit/console/voice/replacement",
            json!({"identity":"agent-a","request_id":"voice-second"}),
        )
        .await;
        assert_eq!(
            replacement["error"], contract["superseded_error"],
            "{replacement}"
        );
        let closed = external_call(
            "mobkit/live/close",
            json!({"identity":"agent-b","channel_id":second_external_channel}),
            "external-close",
        )
        .await;
        assert!(closed.error.is_none(), "{closed:?}");
        let closed = closed.result.expect("close result");
        assert_eq!(closed["status"], "closed", "{closed}");
        assert!(closed.get("close_reason").is_none(), "{closed}");
        assert!(ctx.arbiter.holder().await.is_none());
        let readiness = rpc(
            &app,
            &token,
            "mobkit/console/voice/readiness",
            json!({"identity":"agent-a"}),
        )
        .await;
        assert_eq!(
            readiness["result"], contract["readiness_available"],
            "{readiness}"
        );

        // Phase 5: the external channel ends inside meerkat-live (its socket
        // dropped) without passing through the door. The stale holder must
        // not lock the console out: readiness is available and the console
        // opens without preempting anything.
        let external_third = external_open("external-third").await;
        assert!(external_third.error.is_none(), "{external_third:?}");
        let third_external_channel =
            external_third.result.expect("third external open")["channel_id"]
                .as_str()
                .expect("third external channel")
                .to_string();
        assert_eq!(
            ctx.arbiter.holder().await.map(|owner| owner.kind()),
            Some("external_live")
        );
        let direct_host = crate::live_wiring::member_live_host_for_test(&ctx, &service, &machine);
        direct_host
            .close_live_channel(
                None,
                &meerkat_core::LiveChannelId::new(&third_external_channel),
            )
            .await
            .expect("meerkat-side close of the external channel");
        let readiness = rpc(
            &app,
            &token,
            "mobkit/console/voice/readiness",
            json!({"identity":"agent-a"}),
        )
        .await;
        assert_eq!(
            readiness["result"], contract["readiness_available"],
            "a dead external channel must not hold the path: {readiness}"
        );
        let notifications_before = notifications
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        let third_console = rpc(
            &app,
            &token,
            "mobkit/console/voice/open",
            json!({"identity":"agent-a","request_id":"voice-third"}),
        )
        .await;
        assert!(third_console["error"].is_null(), "{third_console}");
        assert_eq!(
            notifications
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
            notifications_before,
            "opening over a dead holder supersedes nobody"
        );
        assert_eq!(
            ctx.arbiter.holder().await.map(|owner| owner.kind()),
            Some("console_voice")
        );
        assert_eq!(
            realtime.open_count(),
            3,
            "every external open in this test went through the scripted factory"
        );
        controller.shutdown().await.expect("voice shutdown");
        runtime.shutdown().await;
    }
    async fn exercise_shared_host(disconnect_provider: bool, scenario: SummaryScenario) {
        let _guard = SHARED_HOST_TEST_LOCK.lock().await;
        let contract: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/console_voice_v1.json"))
                .expect("shared voice contract");
        let RealRuntime {
            _directory,
            provider,
            runtime,
            service,
            machine,
            factory,
            config,
        } = RealRuntime::start(if disconnect_provider {
            "disconnect"
        } else {
            "normal"
        })
        .await;
        // A second member gives the capabilities preface a peer to name.
        runtime
            .spawn(meerkat_mob::SpawnMemberSpec::from_wire(
                "agent".to_string(),
                "agent-b".to_string(),
                None,
                None,
                None,
            ))
            .await
            .expect("peer member");
        let session = runtime
            .mob_handle()
            .resolve_bridge_session_id(&meerkat_mob::AgentIdentity::from("agent-a"))
            .await
            .expect("source session");
        let before = service
            .export_realtime_refresh_session_snapshot(&session)
            .await
            .expect("before");
        let calls = Arc::new(AtomicUsize::new(0));
        let release_summary = Arc::new(tokio::sync::Notify::new());
        let summary_cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let policy = LiveContextSummaryPolicy::new(
            Arc::new(Summary {
                calls: calls.clone(),
                release: release_summary.clone(),
                cancelled: summary_cancelled.clone(),
                scenario,
            }),
            4 * 1024 * 1024,
            4096,
            Duration::from_secs(30),
        )
        .expect("summary policy");
        assert_eq!(
            policy.bootstrap_mode(),
            LiveContextBootstrapMode::BeforeOpen,
            "generic upstream policy remains unchanged; only console composition opts in"
        );
        let registration = PublicLiveRegistration::parse(&json!({
            "principal":"voice@example.com","realm":"voice",
            "auth_binding":{"realm":"voice","binding":"openai"},"voice":"marin"
        }))
        .expect("registration");
        let ctx = Arc::new(crate::live_wiring::attach_live(
            Arc::clone(&service),
            Arc::clone(&machine),
            &factory,
            config,
            String::new(),
            None,
        ));
        let controller = ConsoleVoiceController::compose_live_host(
            &runtime,
            ctx,
            service.clone(),
            machine,
            factory,
            registration,
            Some((policy, provider.url.clone())),
        )
        .expect("console host");
        assert!(matches!(
            controller.ready("other@example.com", "agent-a").await,
            Err(VoiceError::Unauthorized)
        ));
        assert!(
            controller
                .ready("voice@example.com", "agent-a")
                .await
                .expect("readiness")
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "readiness must not invoke summary production"
        );
        // Open no longer re-probes readiness; the host's own target
        // resolution must still refuse what the probe refused, with the
        // same typed answers the console maps today.
        let open_request = |request_id: &str, identity: &str| super::super::VoiceRequest {
            identity: identity.to_string(),
            request_id: request_id.to_string(),
        };
        assert_eq!(
            controller
                .open("other@example.com", open_request("voice-denied", "agent-a"))
                .await
                .err(),
            Some(VoiceError::Unauthorized),
            "a foreign principal is refused by the open path itself"
        );
        assert_eq!(
            controller
                .open(
                    "voice@example.com",
                    open_request("voice-missing", "agent-missing")
                )
                .await
                .err(),
            Some(VoiceError::Unavailable),
            "an unknown member is unavailable on the open path itself"
        );
        assert!(
            !controller
                .ready("voice@example.com", "agent-missing")
                .await
                .expect("missing member readiness"),
            "readiness and open agree on an unknown member"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "refused opens never reach summary production"
        );
        let decisions = crate::console_auth_config::parse_console_auth_config(&json!({
            "shared_secret":"console-test-signing", "email_allowlist":["voice@example.com"]
        }))
        .expect("auth");
        let token = jsonwebtoken::encode(&jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256), &json!({
                "iss":"http://127.0.0.1/mobkit-gateway","aud":"persistent-gateway",
                "sub":"voice@example.com","email":"voice@example.com","exp":chrono::Utc::now().timestamp()+300
            }), &jsonwebtoken::EncodingKey::from_secret(b"console-test-signing")).expect("token");
        let mut read_only_decisions = decisions.clone();
        read_only_decisions.console.read_only = true;
        let read_only_app = runtime
            .build_reference_app_router(read_only_decisions)
            .layer(axum::Extension(controller.clone()));
        let app = runtime
            .build_reference_app_router(decisions)
            .layer(axum::Extension(controller.clone()));
        let readiness = rpc(
            &app,
            &token,
            "mobkit/console/voice/readiness",
            json!({"identity":"agent-a"}),
        )
        .await;
        assert_eq!(readiness["result"], contract["readiness_available"]);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "HTTP readiness must not summarize"
        );
        let opened = rpc(
            &app,
            &token,
            "mobkit/console/voice/open",
            json!({"identity":"agent-a","request_id":"voice-request"}),
        )
        .await;
        assert!(opened["error"].is_null(), "{opened}");
        let pending = &opened["result"];
        assert_eq!(pending["execution_mode"], "client_context");
        assert_eq!(pending["target_identity"], "agent-a");
        assert_eq!(pending["capabilities"]["text_in"], false);
        assert_eq!(pending["capabilities"]["text_out"], false);
        let context = wait_context(
            &app,
            &token,
            pending,
            json!({"phase":"preparing","stage":"generating"}),
        )
        .await;
        assert_eq!(
            context["result"],
            json!({
                "identity":"agent-a","request_id":"voice-request","channel_id":pending["channel_id"],
                "context_preparation":{"phase":"preparing","stage":"generating"},
            })
        );
        let read_only_context = rpc(&read_only_app, &token, super::super::VOICE_CONTEXT_STATUS_METHOD,
            json!({"identity":"agent-a","request_id":"voice-request","channel_id":pending["channel_id"]})).await;
        assert_eq!(
            read_only_context["result"], context["result"],
            "read-only console permits owned status reads"
        );
        for invalid in [
            json!({"identity":"agent-a","request_id":"voice-request"}),
            json!({"identity":"agent-a","request_id":"voice-request","channel_id":pending["channel_id"],"pending_receipt":pending["pending_receipt"]}),
        ] {
            let response = rpc(
                &app,
                &token,
                super::super::VOICE_CONTEXT_STATUS_METHOD,
                invalid,
            )
            .await;
            assert_eq!(response["error"]["code"], -32602);
        }
        for mismatch in [
            json!({"identity":"agent-a","request_id":"other-request","channel_id":pending["channel_id"]}),
            json!({"identity":"other-agent","request_id":"voice-request","channel_id":pending["channel_id"]}),
            json!({"identity":"agent-a","request_id":"voice-request","channel_id":"other-channel"}),
        ] {
            let response = rpc(
                &app,
                &token,
                super::super::VOICE_CONTEXT_STATUS_METHOD,
                mismatch,
            )
            .await;
            assert_eq!(response["error"]["data"]["kind"], "voice_request_conflict");
        }
        assert_eq!(
            controller
                .context_status(
                    "other@example.com",
                    super::super::VoiceContextStatusRequest {
                        identity: "agent-a".to_string(),
                        request_id: "voice-request".to_string(),
                        channel_id: pending["channel_id"].as_str().expect("channel").to_string(),
                    }
                )
                .await,
            Err(VoiceError::RequestConflict)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!opened.to_string().contains("sk-voice-test-only"));
        let registered = rpc(&app, &token, "mobkit/live/playback_owner/register", json!({
                "identity":"agent-a","channel_id":pending["channel_id"],"pending_receipt":pending["pending_receipt"]
            })).await;
        assert!(
            registered["result"]["readiness_receipt"].as_str().is_some(),
            "{registered}"
        );
        let attached_readiness = rpc(
            &app,
            &token,
            "mobkit/console/voice/readiness",
            json!({"identity":"agent-a"}),
        )
        .await;
        assert_eq!(
            attached_readiness["result"], contract["readiness_available"],
            "an existing live attachment must not fail the exact auth probe"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "attached readiness does not summarize or reopen"
        );
        assert_eq!(
            provider.capture.creates.load(Ordering::SeqCst),
            0,
            "readiness, summary, and pending open do not create a provider session"
        );
        let answer = rpc(
            &app,
            &token,
            "live/webrtc/answer",
            json!({
                "identity":"agent-a","channel_id":pending["channel_id"],
                "pending_receipt":pending["pending_receipt"],
                "readiness_receipt":registered["result"]["readiness_receipt"],
                "token":pending["transport"]["token"],"offer_sdp":"v=0\r\nCONSOLE_FIXTURE_OFFER"
            }),
        )
        .await;
        assert_eq!(
            answer["result"]["answer_sdp"], "v=0\r\nCONSOLE_FIXTURE_ANSWER",
            "{answer}"
        );
        let before_ack = rpc(&app, &token, "mobkit/live/status", json!({
            "identity":"agent-a","channel_id":pending["channel_id"],"pending_receipt":pending["pending_receipt"]
        })).await;
        assert_eq!(
            before_ack["result"]["phase"], "pending",
            "SDP response alone cannot activate"
        );
        let delivered = rpc(
            &app,
            &token,
            "mobkit/console/voice/answer_received",
            json!({
                "identity":"agent-a","request_id":"voice-request","channel_id":pending["channel_id"]
            }),
        )
        .await;
        assert_eq!(delivered["result"], json!({"accepted":true}), "{delivered}");
        let active = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = rpc(&app, &token, "mobkit/live/status", json!({
                    "identity":"agent-a","channel_id":pending["channel_id"],"pending_receipt":pending["pending_receipt"]
                })).await;
                if status["result"]["phase"] == "active" { break status; }
                assert!(status["error"].is_null(), "{status}");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("generated activation");
        assert!(
            active["result"]["handle"]["activation_receipt"]
                .as_str()
                .is_some()
        );
        wait_context(
            &app,
            &token,
            pending,
            json!({"phase":"preparing","stage":"generating"}),
        )
        .await;
        assert_eq!(
            provider.capture.summary_received.load(Ordering::SeqCst),
            0,
            "Active can be reached before summary generation or summary delivery"
        );
        if scenario != SummaryScenario::CancelWhileGenerating {
            release_summary.notify_one();
            if scenario == SummaryScenario::Failure {
                let failed = wait_context(
                    &app,
                    &token,
                    pending,
                    json!({"phase":"failed","reason":"generation"}),
                )
                .await;
                assert!(
                    !failed
                        .to_string()
                        .contains("private fixture producer detail")
                );
                assert_eq!(provider.capture.summary_received.load(Ordering::SeqCst), 0);
            } else {
                // Meerkat 0.8.41 never appends a late summary into silence:
                // generation may finish, but the status stays at generating
                // and no lane carries anything until the user's first turn.
                tokio::time::sleep(Duration::from_millis(300)).await;
                wait_context(
                    &app,
                    &token,
                    pending,
                    json!({"phase":"preparing","stage":"generating"}),
                )
                .await;
                assert_eq!(
                    provider.capture.summary_received.load(Ordering::SeqCst),
                    0,
                    "a late summary must not be appended before the user speaks"
                );
                assert_eq!(
                    provider
                        .capture
                        .instructions_received
                        .load(Ordering::SeqCst),
                    0,
                    "nothing rides the instructions lane before the user speaks"
                );
                provider.capture.speak.notify_one();
                wait_context(
                    &app,
                    &token,
                    pending,
                    json!({"phase":"preparing","stage":"delivering"}),
                )
                .await;
                tokio::time::timeout(Duration::from_secs(5), async {
                    while provider.capture.summary_received.load(Ordering::SeqCst) == 0 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("actual summary append on the thinking lane after the first user turn");
                assert_eq!(
                    provider.capture.summary_acknowledged.load(Ordering::SeqCst),
                    0
                );
                wait_context(
                    &app,
                    &token,
                    pending,
                    json!({"phase":"preparing","stage":"delivering"}),
                )
                .await;
                let summary_append = provider
                    .capture
                    .summary_append
                    .lock()
                    .expect("summary append")
                    .clone()
                    .expect("append");
                assert!(
                    summary_append
                        .contains(meerkat::experimental_gpt_live::LIVE_LATE_SUMMARY_PREFIX),
                    "the late summary must open with the context-data prefix: {summary_append}"
                );
                assert!(
                    summary_append
                        .contains("The background agent is configured for the test conversation."),
                    "the thinking-lane append must carry the generated summary: {summary_append}"
                );
                provider.capture.release_summary_append.notify_one();
                wait_context(
                    &app,
                    &token,
                    pending,
                    json!({"phase":"provider_acknowledged"}),
                )
                .await;
                let fragments = provider.capture.summary_received.load(Ordering::SeqCst);
                assert!(
                    fragments >= 1,
                    "the summary must have arrived in at least one fragment"
                );
                assert_eq!(
                    provider
                        .capture
                        .instructions_received
                        .load(Ordering::SeqCst),
                    0,
                    "the late summary travels on the thinking lane; the instructions lane stays quiet"
                );
                assert_eq!(
                    provider.capture.summary_acknowledged.load(Ordering::SeqCst),
                    fragments,
                    "every summary fragment must be acknowledged exactly once"
                );
            }
        }
        let active_readiness = rpc(
            &app,
            &token,
            "mobkit/console/voice/readiness",
            json!({"identity":"agent-a"}),
        )
        .await;
        assert_eq!(
            active_readiness["result"], contract["readiness_available"],
            "{active_readiness}"
        );
        assert_eq!(
            provider.capture.creates.load(Ordering::SeqCst),
            1,
            "active probe must not reopen"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "active probe must not summarize"
        );
        let body = provider
            .capture
            .body
            .lock()
            .expect("create body")
            .clone()
            .expect("provider created");
        // Meerkat 0.8.41 opens as a continuing conversation: no user-role
        // startup input, the pending-context notice rides the instructions.
        assert!(body["session"].get("input").is_none(), "{body}");
        assert!(
            !body
                .to_string()
                .contains("The background agent is configured for the test conversation."),
            "concurrent context must not be required by the provider-create request"
        );
        assert!(body["session"]["tools"].is_null());
        // The capabilities preface opens the session instructions at create
        // time, on its own carrier, whether or not the summary later
        // succeeds: identity and role, the peer, the tool, and the skill.
        assert!(
            body["session"]["instructions"].is_string(),
            "create body must carry session instructions: {body}"
        );
        let instructions = body["session"]["instructions"]
            .as_str()
            .expect("string checked above");
        assert!(
            instructions.starts_with("You speak for agent-a (agent) in mob console-voice-"),
            "the preface must open the session instructions: {instructions}"
        );
        for needle in [
            "You are connected to: agent-b (agent).",
            "available through the executor: comms.",
            "Skills: voice-notes.",
            "answer from this list",
        ] {
            assert!(
                instructions.contains(needle),
                "the preface must carry {needle:?}: {instructions}"
            );
        }
        assert!(
            instructions.contains("do not greet or introduce yourself"),
            "the default continuing-conversation guidance must survive the preface: {instructions}"
        );
        if disconnect_provider {
            provider.capture.disconnect.notify_one();
            provider.capture.disconnected.notified().await;
        }
        let closed = tokio::time::timeout(
            Duration::from_secs(5),
            rpc(
                &app,
                &token,
                "mobkit/console/voice/close",
                json!({"identity":"agent-a","request_id":"voice-request"}),
            ),
        )
        .await
        .expect("exact close must complete within the browser teardown deadline");
        assert_eq!(closed["result"], json!({"phase":"closed"}), "{closed}");
        if scenario == SummaryScenario::CancelWhileGenerating {
            tokio::time::timeout(Duration::from_secs(5), async {
                while !summary_cancelled.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("close cancels the actual summarizer future");
            assert_eq!(provider.capture.summary_received.load(Ordering::SeqCst), 0);
        }
        let closed_context = rpc(&app, &token, super::super::VOICE_CONTEXT_STATUS_METHOD, json!({
            "identity":"agent-a","request_id":"voice-request","channel_id":pending["channel_id"],
        })).await;
        assert_eq!(closed_context["error"]["data"]["kind"], "voice_closed");
        let closed_replacement = rpc(
            &app,
            &token,
            "mobkit/console/voice/replacement",
            json!({"identity":"agent-a","request_id":"voice-request"}),
        )
        .await;
        assert_eq!(closed_replacement["error"]["data"]["kind"], "voice_closed");
        let after = service
            .export_realtime_refresh_session_snapshot(&session)
            .await
            .expect("after");
        // Voice setup must not rewrite source history. The one utterance the
        // fixture spoke may have become a canonical user row at the end of
        // the source; every pre-existing row stays byte-identical.
        assert!(
            after.messages().len() >= before.messages().len(),
            "voice setup must not drop source history"
        );
        assert_eq!(
            &after.messages()[..before.messages().len()],
            before.messages(),
            "voice setup must not rewrite source history"
        );
        for appended in &after.messages()[before.messages().len()..] {
            let meerkat_core::Message::User(user) = appended else {
                panic!("only the spoken user utterance may be appended by voice: {appended:?}");
            };
            assert!(
                user.content.iter().any(|block| {
                    matches!(block, meerkat_core::ContentBlock::Text { text } if text.contains("hello"))
                }),
                "an appended row must be the fixture's spoken utterance: {user:?}"
            );
        }
        assert_eq!(
            service
                .live_session_llm_identity(&session)
                .await
                .expect("identity")
                .model,
            "gpt-5.5"
        );
        // agent-a plus the agent-b peer spawned above: voice open adds no member.
        assert_eq!(runtime.mob_handle().list_members().await.len(), 2);
        let reopened = rpc(
            &app,
            &token,
            "mobkit/console/voice/open",
            json!({"identity":"agent-a","request_id":"voice-reopened"}),
        )
        .await;
        assert!(reopened["error"].is_null(), "{reopened}");
        assert_ne!(reopened["result"]["channel_id"], pending["channel_id"]);
        let stale_context = rpc(&app, &token, super::super::VOICE_CONTEXT_STATUS_METHOD, json!({
            "identity":"agent-a","request_id":"voice-reopened","channel_id":pending["channel_id"],
        })).await;
        assert_eq!(
            stale_context["error"]["data"]["kind"],
            "voice_request_conflict"
        );
        let reclosed = rpc(
            &app,
            &token,
            "mobkit/console/voice/close",
            json!({"identity":"agent-a","request_id":"voice-reopened"}),
        )
        .await;
        assert_eq!(reclosed["result"], json!({"phase":"closed"}), "{reclosed}");
        controller.shutdown().await.expect("voice shutdown");
        runtime.shutdown().await;
    }
}
impl SharedHost {
    async fn target(
        &self,
        principal: &str,
        identity: &str,
    ) -> Result<Arc<ConsoleLiveGrant>, VoiceError> {
        if principal != self.principal {
            return Err(VoiceError::Unauthorized);
        }
        let access = self.access.view_for_subject(Some(principal));
        if !access.allows_agent(ACTION_AGENT_VIEW, identity)
            || !access.allows_agent(ACTION_AGENT_SEND, identity)
        {
            return Err(VoiceError::Unauthorized);
        }
        // Every unavailable answer below names its cause in the log; the wire
        // answer stays the plain typed `Unavailable`.
        let unavailable = |cause: &str| {
            tracing::warn!(identity, cause, "console voice target unavailable");
            VoiceError::Unavailable
        };
        let authoritative = if let Some(runtime) = &self.identity_runtime {
            runtime
                .member_alias_lifecycle_target(identity)
                .await
                .map_err(|error| unavailable(&format!("identity lifecycle lookup: {error:?}")))?
                .is_some()
        } else {
            false
        };
        let session = crate::rpc::resolve_live_target(
            &self.handle,
            self.identity_runtime.as_ref(),
            authoritative,
            &json!({"identity": identity}),
        )
        .await
        .map_err(|error| unavailable(&format!("live target resolution: {error:?}")))?
        .ok_or_else(|| unavailable("no live-capable session for identity"))?;
        let mut owner = None;
        for member in self.handle.list_members().await {
            if self
                .handle
                .resolve_bridge_session_id(&member.agent_identity)
                .await
                .as_ref()
                == Some(&session)
            {
                if owner.is_some() {
                    return Err(unavailable("more than one member owns the target session"));
                }
                owner = Some(member.agent_identity);
            }
        }
        let owner = owner.ok_or_else(|| unavailable("no mob member owns the target session"))?;
        self.binding
            .register(principal, owner, session)
            .map_err(|error| match error {
                VoiceError::Busy => {
                    tracing::warn!(
                        identity,
                        "console voice target grant is held by another principal or identity"
                    );
                    VoiceError::Busy
                }
                other => other,
            })
    }

    fn guard(
        &self,
        grant: Arc<ConsoleLiveGrant>,
        identity: String,
    ) -> Result<LiveSurfaceAuthority, VoiceError> {
        let principal = meerkat_core::PrincipalRef::new(
            meerkat_core::PrincipalKind::Human,
            grant.principal.clone(),
        )
        .map_err(|_| VoiceError::Unauthorized)?;
        Ok(LiveSurfaceAuthority::authenticated_http(Arc::new(
            HttpAuthority {
                grant,
                identity,
                principal,
                access: self.access.clone(),
            },
        )))
    }

    async fn open_owned(
        self: &Arc<Self>,
        principal: &str,
        identity: &str,
    ) -> Result<Arc<dyn ConsoleVoiceSession>, VoiceError> {
        let target_started = std::time::Instant::now();
        // The readiness probe reported a grant held by another principal as
        // plain unavailability; the open path keeps that answer rather than
        // the same-request "teardown pending" retry hint.
        let grant = self
            .target(principal, identity)
            .await
            .map_err(|error| match error {
                VoiceError::Busy => VoiceError::Unavailable,
                other => other,
            })?;
        let target_ms = elapsed_ms(target_started);
        let surface = self.guard(Arc::clone(&grant), identity.to_string())?;
        let live_open_started = std::time::Instant::now();
        let (response, delivery) = capture_live_rpc_response_delivery(self.handler.dispatch(
            surface, Some(grant.session.clone()), Some(identity.to_string()),
            "mobkit/live/open".to_string(),
            json!({"identity": identity, "transport": "webrtc", "execution_identity": {"version":"v1", "profile_id":PROFILE}}),
            json!("console-voice-open"),
        )).await;
        tracing::info!(
            target: "meerkat_mobkit::console_voice::timing",
            identity,
            target_ms,
            live_open_ms = elapsed_ms(live_open_started),
            ok = response.error.is_none(),
            "console voice shared host open"
        );
        let pending = match response
            .result
            .and_then(|value| serde_json::from_value::<PendingLiveChannelHandle>(value).ok())
        {
            Some(pending) if response.error.is_none() => pending,
            _ => {
                if let Some(delivery) = delivery {
                    let _ = delivery.rejected().await;
                }
                tracing::warn!(error = ?response.error, "console live open refused by shared host");
                // Meerkat's open admission refuses an unavailable target,
                // credential, binding, or member with the capability code;
                // that is the same answer the readiness probe gave.
                let unavailable = response
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == crate::rpc::CAPABILITY_UNAVAILABLE_CODE);
                return Err(if unavailable {
                    VoiceError::Unavailable
                } else {
                    VoiceError::HostFailed
                });
            }
        };
        Ok(Arc::new(Session {
            shared: Arc::clone(self),
            grant,
            identity: identity.to_string(),
            current: StdMutex::new(pending.clone()),
            channels: StdMutex::new(vec![pending]),
            deliveries: Mutex::new(Deliveries {
                open: delivery,
                ..Deliveries::default()
            }),
            polls: StdMutex::new(HashMap::new()),
        }))
    }
}

fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[async_trait]
impl ConsoleVoiceHost for Host {
    async fn ready(&self, principal: &str, identity: &str) -> Result<bool, VoiceError> {
        let target_started = std::time::Instant::now();
        let grant = match self.0.target(principal, identity).await {
            Ok(grant) => grant,
            Err(VoiceError::Unavailable | VoiceError::Busy) => return Ok(false),
            Err(error) => {
                tracing::warn!(
                    identity,
                    ?error,
                    "console voice readiness refused by target authorization"
                );
                return Err(error);
            }
        };
        let target_ms = elapsed_ms(target_started);
        // Shared admission resolves the actual selected configured credential,
        // but does not open/register a provider channel at this preparation seam.
        let probe_started = std::time::Instant::now();
        let probe = self
            .0
            .authority
            .probe_execution_readiness(&grant.session, &self.0.selection)
            .await;
        match &probe {
            Ok(()) => tracing::debug!(
                target: "meerkat_mobkit::console_voice::timing",
                identity,
                target_ms,
                probe_ms = elapsed_ms(probe_started),
                "console voice readiness probe"
            ),
            Err(error) => tracing::warn!(
                target: "meerkat_mobkit::console_voice::timing",
                identity,
                target_ms,
                probe_ms = elapsed_ms(probe_started),
                cause = %error,
                "console voice readiness probe failed: execution credential not ready"
            ),
        }
        probe.map(|()| true).map_err(|_| VoiceError::Unavailable)
    }

    async fn open(
        &self,
        principal: &str,
        identity: &str,
    ) -> Result<Arc<dyn ConsoleVoiceSession>, VoiceError> {
        let tracked = if let Some(runtime) = &self.0.identity_runtime {
            runtime
                .member_alias_lifecycle_target(identity)
                .await
                .map_err(|_| VoiceError::Unavailable)?
        } else {
            None
        };
        if let Some(target) = tracked {
            let shared = Arc::clone(&self.0);
            let principal = principal.to_string();
            let identity = identity.to_string();
            crate::identity_first::IdentityRuntime::run_member_alias_targets_operation_tracked(
                vec![target],
                move || async move {
                    shared
                        .open_owned(&principal, &identity)
                        .await
                        .map_err(|error| format!("{error:?}"))
                },
            )
            .await
            .map_err(|_| VoiceError::HostFailed)
        } else {
            self.0.open_owned(principal, identity).await
        }
    }
}

struct HttpAuthority {
    grant: Arc<ConsoleLiveGrant>,
    identity: String,
    principal: meerkat_core::PrincipalRef,
    access: AccessController,
}

#[async_trait]
impl AuthenticatedHttpLiveAuthority for HttpAuthority {
    fn principal(&self) -> &meerkat_core::PrincipalRef {
        &self.principal
    }

    async fn authorize(
        &self,
        operation: LiveOperation,
        session: Option<&SessionId>,
        params: &Value,
        _machine: &meerkat_runtime::MeerkatMachine,
    ) -> Result<(), crate::rpc::JsonRpcError> {
        if session != Some(&self.grant.session)
            || params.get("identity").and_then(Value::as_str) != Some(self.identity.as_str())
        {
            return Err(VoiceError::Unauthorized.rpc_error());
        }
        if !matches!(
            operation,
            LiveOperation::Close | LiveOperation::Status | LiveOperation::PlaybackOwnerRevoke
        ) {
            let access = self.access.view_for_subject(Some(&self.grant.principal));
            if self.grant.revoked.load(Ordering::SeqCst)
                || !access.allows_agent(ACTION_AGENT_VIEW, &self.identity)
                || !access.allows_agent(ACTION_AGENT_SEND, &self.identity)
            {
                return Err(VoiceError::Unauthorized.rpc_error());
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Deliveries {
    open: Option<LiveRpcResponseDeliveryCustody>,
    answers: HashMap<String, LiveRpcResponseDeliveryCustody>,
    received: HashSet<String>,
}

/// Activation polls observed for one pending channel.
struct PollTrace {
    count: u32,
    first: std::time::Instant,
}

struct Session {
    shared: Arc<SharedHost>,
    grant: Arc<ConsoleLiveGrant>,
    identity: String,
    current: StdMutex<PendingLiveChannelHandle>,
    channels: StdMutex<Vec<PendingLiveChannelHandle>>,
    deliveries: Mutex<Deliveries>,
    /// Status polls per channel until the channel is first seen active.
    polls: StdMutex<HashMap<String, PollTrace>>,
}

impl Session {
    /// Count activation status polls per channel and report how many the
    /// caller needed, and how long it waited, once the channel is active.
    fn trace_status_poll(&self, channel: &str, result: &Value) {
        let phase = result.get("phase").and_then(Value::as_str).unwrap_or("");
        let mut polls = self
            .polls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let trace = polls
            .entry(channel.to_string())
            .or_insert_with(|| PollTrace {
                count: 0,
                first: std::time::Instant::now(),
            });
        trace.count += 1;
        if phase == "active" || phase == "closed" || phase == "revoked" {
            let trace = polls.remove(channel).unwrap_or(PollTrace {
                count: 0,
                first: std::time::Instant::now(),
            });
            tracing::info!(
                target: "meerkat_mobkit::console_voice::timing",
                identity = %self.identity,
                channel_id = channel,
                phase,
                polls = trace.count,
                waited_ms = elapsed_ms(trace.first),
                "console voice status polls settled"
            );
        }
    }

    async fn call(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(Value, Option<LiveRpcResponseDeliveryCustody>), VoiceError> {
        let channel_id = params
            .get("channel_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let surface = self
            .shared
            .guard(Arc::clone(&self.grant), self.identity.clone())?;
        let (response, delivery) =
            capture_live_rpc_response_delivery(self.shared.handler.dispatch(
                surface,
                Some(self.grant.session.clone()),
                Some(self.identity.clone()),
                method.to_string(),
                params,
                json!("console-voice-control"),
            ))
            .await;
        if response.error.is_some() {
            if let Some(delivery) = delivery {
                let _ = delivery.rejected().await;
            }
            tracing::warn!(
                method,
                identity = %self.identity,
                channel_id,
                error = ?response.error,
                "console live control refused by shared host"
            );
            return Err(VoiceError::HostFailed);
        }
        Ok((response.result.ok_or(VoiceError::HostFailed)?, delivery))
    }

    async fn retain_replacement(&self) -> Result<Option<Value>, VoiceError> {
        let Some(replacement) = self
            .shared
            .handler
            .pending_replacement(&self.grant.session)
            .await
        else {
            return Ok(None);
        };
        let reason = match &replacement {
            meerkat::surface::ExperimentalLiveReplacementRequired::CanonicalContext { .. } => {
                "canonical_context"
            }
            meerkat::surface::ExperimentalLiveReplacementRequired::DelegationResult { .. } => {
                "delegation_result"
            }
        };
        let pending = PendingLiveChannelHandle::new(
            self.identity.clone(),
            self.pending().execution_mode,
            replacement.pending_receipt(),
            replacement.open().clone(),
        );
        let (status, _) = self
            .call(
                "mobkit/live/status",
                json!({
                    "identity": self.identity,
                    "channel_id": pending.channel_id,
                    "pending_receipt": pending.pending_receipt,
                }),
            )
            .await?;
        let status: ExperimentalLiveChannelStatus =
            serde_json::from_value(status).map_err(|_| VoiceError::HostFailed)?;
        if matches!(
            status,
            ExperimentalLiveChannelStatus::Closed | ExperimentalLiveChannelStatus::Revoked
        ) {
            tracing::debug!(
                channel_id = %pending.channel_id,
                "ignoring terminal console voice replacement"
            );
            return Ok(None);
        }
        let mut channels = self
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !channels
            .iter()
            .any(|channel| channel.channel_id == pending.channel_id)
        {
            channels.push(pending.clone());
        }
        *self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = pending.clone();
        Ok(Some(
            json!({"required":true, "reason":reason, "replacement":pending,
                "canonical_seed_cursor":replacement.canonical_seed_cursor()}),
        ))
    }
}

#[async_trait]
impl ConsoleVoiceSession for Session {
    fn pending(&self) -> PendingLiveChannelHandle {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    async fn context_preparation(
        &self,
        channel: &str,
    ) -> Result<VoiceContextPreparation, VoiceError> {
        let pending = self.pending();
        if pending.channel_id != channel {
            return Err(VoiceError::RequestConflict);
        }
        if self.grant.revoked.load(Ordering::SeqCst) {
            return Err(VoiceError::Closed);
        }
        let access = self
            .shared
            .access
            .view_for_subject(Some(&self.grant.principal));
        if !access.allows_agent(ACTION_AGENT_VIEW, &self.identity) {
            return Err(VoiceError::Unauthorized);
        }
        // Durable-source availability is validated once, at open admission
        // (`prepare_open`). Re-validating on every context status poll loaded
        // the full persisted session body on the mob actor once a second per
        // preparing voice call and queued every other mob command behind it.
        // The custody read below is the authority for whether this channel is
        // still preparing, active, or closed.
        let read = async {
            let custody = self
                .shared
                .handler
                .read_console_channel_custody(
                    &self.grant.session,
                    &meerkat_core::LiveChannelId::new(channel),
                    &pending.pending_receipt,
                )
                .await
                .map_err(|_| VoiceError::ContextReadFailed)?;
            if matches!(
                custody.phase(),
                meerkat::surface::ExperimentalLiveChannelPhaseStatus::Closed
                    | meerkat::surface::ExperimentalLiveChannelPhaseStatus::Revoked
            ) {
                return Err(VoiceError::Closed);
            }
            if self.pending().channel_id != channel {
                return Err(VoiceError::RequestConflict);
            }
            Ok(VoiceContextPreparation::from(custody.context_preparation()))
        };
        tokio::time::timeout(Duration::from_secs(5), read)
            .await
            .map_err(|_| VoiceError::ContextReadFailed)?
    }

    async fn dispatch(&self, method: &str, params: Value) -> Result<Value, VoiceError> {
        let channel = params
            .get("channel_id")
            .and_then(Value::as_str)
            .ok_or(VoiceError::InvalidRequest)?
            .to_string();
        if method == "mobkit/live/playback_owner/register" {
            let pending = self.pending();
            if params.get("pending_receipt").and_then(Value::as_str)
                != Some(pending.pending_receipt.as_str())
            {
                return Err(VoiceError::InvalidRequest);
            }
            if let Some(delivery) = self.deliveries.lock().await.open.take() {
                delivery
                    .delivered()
                    .await
                    .map_err(|_| VoiceError::HostFailed)?;
            }
        }
        // Only pending-receipt polls are activation polls; the active call's
        // periodic status checks carry the activation receipt instead.
        let activation_poll =
            method == "mobkit/live/status" && params.get("pending_receipt").is_some();
        let (result, delivery) = self.call(method, params).await?;
        if activation_poll {
            self.trace_status_poll(&channel, &result);
        }
        if let Some(delivery) = delivery {
            if method != "live/webrtc/answer" {
                let _ = delivery.rejected().await;
                return Err(VoiceError::HostFailed);
            }
            self.deliveries
                .lock()
                .await
                .answers
                .insert(channel, delivery);
        }
        Ok(result)
    }

    async fn answer_received(&self, channel: &str) -> Result<(), VoiceError> {
        let mut deliveries = self.deliveries.lock().await;
        if deliveries.received.contains(channel) {
            return Ok(());
        }
        let delivery = deliveries
            .answers
            .remove(channel)
            .ok_or(VoiceError::InvalidRequest)?;
        delivery
            .delivered()
            .await
            .map_err(|_| VoiceError::HostFailed)?;
        deliveries.received.insert(channel.to_string());
        Ok(())
    }

    async fn replacement_required(&self) -> Result<Value, VoiceError> {
        Ok(self
            .retain_replacement()
            .await?
            .unwrap_or_else(|| json!({"required":false})))
    }

    async fn close(&self) -> Result<(), VoiceError> {
        self.grant.revoked.store(true, Ordering::SeqCst);
        let mut deliveries = self.deliveries.lock().await;
        if let Some(delivery) = deliveries.open.take() {
            delivery
                .rejected()
                .await
                .map_err(|_| VoiceError::HostFailed)?;
        }
        for (_, delivery) in std::mem::take(&mut deliveries.answers) {
            delivery
                .rejected()
                .await
                .map_err(|_| VoiceError::HostFailed)?;
        }
        drop(deliveries);
        self.retain_replacement().await?;
        let channels = self
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        // Recovery may have superseded the caller's last active handle.
        // Close the newly discovered replacement before older fenced channels.
        for channel in channels.into_iter().rev() {
            let params = json!({"identity":self.identity, "channel_id":channel.channel_id,
                "pending_receipt":channel.pending_receipt});
            let (status, _) = self.call("mobkit/live/status", params.clone()).await?;
            let status: ExperimentalLiveChannelStatus =
                serde_json::from_value(status).map_err(|_| VoiceError::HostFailed)?;
            match status {
                // Receipt close also cancels recovery prepared after transport loss.
                ExperimentalLiveChannelStatus::Closed
                | ExperimentalLiveChannelStatus::Revoked
                | ExperimentalLiveChannelStatus::Pending => {
                    self.call("mobkit/live/close", params).await?;
                }
                ExperimentalLiveChannelStatus::Active { handle } => {
                    self.call("mobkit/live/close", json!({"identity":self.identity,
                        "channel_id":handle.channel_id,"activation_receipt":handle.activation_receipt})).await?;
                }
            }
        }
        Ok(())
    }
}
