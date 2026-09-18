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
use crate::public_live_config::PublicLiveRegistration;
use crate::unified_runtime::UnifiedRuntime;

const PROFILE: &str = "openai.gpt-live-1.client-context.v1";

struct FactorySummarizer {
    factory: meerkat::AgentFactory,
    config: Config,
    machine: Arc<meerkat_runtime::MeerkatMachine>,
}

#[async_trait]
impl LiveContextSummarizer for FactorySummarizer {
    async fn summarize(
        &self,
        snapshot: LiveContextSummarySnapshot<'_>,
    ) -> Result<String, LiveContextSummaryError> {
        let started = std::time::Instant::now();
        let client = self
            .factory
            .build_llm_client_for_identity_with_auth_lease(
                &self.config,
                snapshot.llm_identity(),
                Some(self.machine.generated_auth_lease_handle()),
            )
            .await
            .map_err(|_| {
                LiveContextSummaryError::Producer("summary client is unavailable".to_string())
            })?;
        let result = super::summary::summarize_context(
            client.as_ref(),
            &snapshot.llm_identity().model,
            snapshot.messages(),
            snapshot.max_output_bytes(),
        )
        .await;
        tracing::info!(
            model = %snapshot.llm_identity().model,
            elapsed_ms = started.elapsed().as_millis(),
            output_bytes = ?result.as_ref().ok().map(String::len),
            error = ?result.as_ref().err(),
            "console voice context summary finished"
        );
        result.map_err(|error| {
            LiveContextSummaryError::Producer(format!("summary rejected: {error:?}"))
        })
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
    pub fn with_live_host<B: SessionAgentBuilder + 'static>(
        runtime: &Arc<UnifiedRuntime>,
        service: Arc<PersistentSessionService<B>>,
        machine: Arc<meerkat_runtime::MeerkatMachine>,
        factory: meerkat::AgentFactory,
        config: Config,
        registration: PublicLiveRegistration,
    ) -> Result<Self, String> {
        Self::compose_live_host(
            runtime,
            service,
            machine,
            factory,
            config,
            registration,
            None,
        )
    }

    fn compose_live_host<B: SessionAgentBuilder + 'static>(
        runtime: &Arc<UnifiedRuntime>,
        service: Arc<PersistentSessionService<B>>,
        machine: Arc<meerkat_runtime::MeerkatMachine>,
        factory: meerkat::AgentFactory,
        config: Config,
        registration: PublicLiveRegistration,
        summary_override: Option<(LiveContextSummaryPolicy, String)>,
    ) -> Result<Self, String> {
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
        let ctx = Arc::new(crate::live_wiring::attach_live(
            Arc::clone(&service),
            Arc::clone(&machine),
            &factory,
            config.clone(),
            String::new(),
            None,
        ));
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
                    config,
                    machine: Arc::clone(&machine),
                }),
                4 * 1024 * 1024,
                16 * 1024,
                Duration::from_mins(1),
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
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
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
    struct ProviderCapture {
        creates: AtomicUsize,
        body: StdMutex<Option<Value>>,
        disconnect: tokio::sync::Notify,
        disconnected: tokio::sync::Notify,
        /// The concurrent bootstrap summary. Meerkat 0.8.40 delivers it on
        /// the provider's instructions lane (recalled knowledge), never as
        /// spoken commentary and no longer as quiet thinking.
        /// Wire fragments (500 chars each) concatenated in arrival order.
        summary_append: StdMutex<Option<String>>,
        /// Fragments received so far.
        summary_received: AtomicUsize,
        /// Fragments acknowledged so far; acks are held until the test
        /// releases them, then every received fragment is acknowledged.
        summary_acknowledged: AtomicUsize,
        release_summary_append: tokio::sync::Notify,
        /// Quiet causal-tail reassertions still travel on the thinking lane;
        /// the fixture acknowledges them immediately and only counts them.
        thinking_received: AtomicUsize,
    }

    // Only the external provider is simulated. HTTP, WebSocket sideband,
    // shared Meerkat custody and activation are exercised through real owners.
    struct ProviderFixture {
        url: String,
        capture: Arc<ProviderCapture>,
        server: tokio::task::JoinHandle<()>,
    }

    impl Drop for ProviderFixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    impl ProviderFixture {
        async fn start() -> Self {
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
                                        "type":"session.instructions.appended","event_id":"instructions-ack",
                                        "client_event_id":client_event_id,"start_ms":0.0,"end_ms":0.0
                                    }).to_string().into())).await.expect("instructions acknowledgement");
                                    capture.summary_acknowledged.fetch_add(1, Ordering::SeqCst);
                                }
                                continue;
                            }
                            message = socket.recv() => message,
                        };
                        let Some(Ok(message)) = message else { break; };
                        let SocketMessage::Text(text) = message else { continue; };
                        let event: Value = serde_json::from_str(&text).expect("provider command");
                        match event["type"].as_str() {
                            Some("session.instructions.append") => {
                                // The bootstrap summary arrives as pipelined
                                // 500-char fragments; the ack of each is held
                                // until the test releases the lane so the
                                // "delivering" stage stays observable.
                                let fragment = event["content"].as_str().unwrap_or_default().to_string();
                                capture
                                    .summary_append
                                    .lock()
                                    .expect("summary capture")
                                    .get_or_insert_with(String::new)
                                    .push_str(&fragment);
                                capture.summary_received.fetch_add(1, Ordering::SeqCst);
                                if summary_released {
                                    socket.send(SocketMessage::Text(json!({
                                        "type":"session.instructions.appended","event_id":"instructions-ack",
                                        "client_event_id":event["event_id"],"start_ms":0.0,"end_ms":0.0
                                    }).to_string().into())).await.expect("instructions acknowledgement");
                                    capture.summary_acknowledged.fetch_add(1, Ordering::SeqCst);
                                } else {
                                    held_summary_acks.push(event["event_id"].clone());
                                }
                            }
                            Some("session.thinking.append") => {
                                capture.thinking_received.fetch_add(1, Ordering::SeqCst);
                                socket.send(SocketMessage::Text(json!({
                                    "type":"session.thinking.appended","event_id":"thinking-ack",
                                    "client_event_id":event["event_id"],"start_ms":0.0,"end_ms":0.0
                                }).to_string().into())).await.expect("thinking acknowledgement");
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
    enum SummaryScenario {
        Success,
        Failure,
        CancelWhileGenerating,
    }

    struct Summary {
        calls: Arc<AtomicUsize>,
        release: Arc<tokio::sync::Notify>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
        scenario: SummaryScenario,
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

    fn config() -> Config {
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

    async fn rpc(app: &axum::Router, token: &str, method: &str, params: Value) -> Value {
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

    async fn exercise_shared_host(disconnect_provider: bool, scenario: SummaryScenario) {
        let _guard = SHARED_HOST_TEST_LOCK.lock().await;
        let contract: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/console_voice_v1.json"))
                .expect("shared voice contract");
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
    id = "console-voice-{suffix}"
    [profiles.agent]
    model = "gpt-5.5"
    external_addressable = true
    [profiles.agent.tools]
    comms = true
    "#,
            suffix = if disconnect_provider {
                "disconnect"
            } else {
                "normal"
            },
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
                        namespace: "voice-test".to_string(),
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
        let controller = ConsoleVoiceController::compose_live_host(
            &runtime,
            service.clone(),
            machine,
            factory,
            config,
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
                .expect("actual summary append on the instructions lane");
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
                        .contains("The background agent is configured for the test conversation."),
                    "the instructions-lane append must carry the generated summary: {summary_append}"
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
                    provider.capture.thinking_received.load(Ordering::SeqCst),
                    0,
                    "the summary travels on the instructions lane; without native speech no \
                     causal-tail reassertion should reach the thinking lane"
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
        assert!(body["session"]["input"].is_array(), "{body}");
        assert!(
            !body
                .to_string()
                .contains("The background agent is configured for the test conversation."),
            "concurrent context must not be required by the provider-create request"
        );
        assert!(body["session"]["tools"].is_null());
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
        assert_eq!(
            before.messages(),
            after.messages(),
            "voice setup must not rewrite source history"
        );
        assert_eq!(
            service
                .live_session_llm_identity(&session)
                .await
                .expect("identity")
                .model,
            "gpt-5.5"
        );
        assert_eq!(runtime.mob_handle().list_members().await.len(), 1);
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
        let authoritative = if let Some(runtime) = &self.identity_runtime {
            runtime
                .member_alias_lifecycle_target(identity)
                .await
                .map_err(|_| VoiceError::Unavailable)?
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
        .map_err(|_| VoiceError::Unavailable)?
        .ok_or(VoiceError::Unavailable)?;
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
                    return Err(VoiceError::Unavailable);
                }
                owner = Some(member.agent_identity);
            }
        }
        self.binding
            .register(principal, owner.ok_or(VoiceError::Unavailable)?, session)
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
        let grant = self.target(principal, identity).await?;
        let surface = self.guard(Arc::clone(&grant), identity.to_string())?;
        let (response, delivery) = capture_live_rpc_response_delivery(self.handler.dispatch(
            surface, Some(grant.session.clone()), Some(identity.to_string()),
            "mobkit/live/open".to_string(),
            json!({"identity": identity, "transport": "webrtc", "execution_identity": {"version":"v1", "profile_id":PROFILE}}),
            json!("console-voice-open"),
        )).await;
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
                return Err(VoiceError::HostFailed);
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
        }))
    }
}

#[async_trait]
impl ConsoleVoiceHost for Host {
    async fn ready(&self, principal: &str, identity: &str) -> Result<bool, VoiceError> {
        let grant = match self.0.target(principal, identity).await {
            Ok(grant) => grant,
            Err(VoiceError::Unavailable | VoiceError::Busy) => return Ok(false),
            Err(error) => return Err(error),
        };
        // Shared admission resolves the actual selected configured credential,
        // but does not open/register a provider channel at this preparation seam.
        self.0
            .authority
            .probe_execution_readiness(&grant.session, &self.0.selection)
            .await
            .map(|()| true)
            .map_err(|_| VoiceError::Unavailable)
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

struct Session {
    shared: Arc<SharedHost>,
    grant: Arc<ConsoleLiveGrant>,
    identity: String,
    current: StdMutex<PendingLiveChannelHandle>,
    channels: StdMutex<Vec<PendingLiveChannelHandle>>,
    deliveries: Mutex<Deliveries>,
}

impl Session {
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
        let read = async {
            use meerkat::experimental_gpt_live::ExperimentalLiveSessionBindingAuthority as _;
            self.shared
                .binding
                .validate_live_durable_source_availability(&self.grant.session)
                .await
                .map_err(|_| VoiceError::ContextReadFailed)?;
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
        let (result, delivery) = self.call(method, params).await?;
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
