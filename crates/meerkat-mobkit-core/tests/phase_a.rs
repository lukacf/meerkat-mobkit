use std::sync::Arc;
use std::time::Duration;
use std::{future::Future, pin::Pin};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use meerkat::{build_ephemeral_service, AgentFactory, Config};
use meerkat_client::{LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest, TestClient};
use meerkat_core::StopReason;
use meerkat_mob::{MobDefinition, MobState, MobStorage, SpawnMemberSpec};
use meerkat_mobkit_core::http_sse::interaction_sse_router_with_keep_alive_interval;
use meerkat_mobkit_core::{
    interaction_sse_router, MobBootstrapOptions, MobBootstrapSpec, RealMobRuntime,
};
use serde_json::json;
use serde_json::Value;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower::ServiceExt;

struct RuntimeFixture {
    _temp_dir: TempDir,
    runtime: RealMobRuntime,
}

struct DelayedTestClient {
    delay: Duration,
}

impl DelayedTestClient {
    fn new(delay: Duration) -> Self {
        Self { delay }
    }
}

impl LlmClient for DelayedTestClient {
    fn stream<'a>(
        &'a self,
        _request: &'a LlmRequest,
    ) -> Pin<Box<dyn futures::Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>> {
        let delay = self.delay;
        Box::pin(async_stream::stream! {
            tokio::time::sleep(delay).await;
            yield Ok(LlmEvent::TextDelta {
                delta: "ok".to_string(),
                meta: None,
            });
            tokio::time::sleep(delay).await;
            yield Ok(LlmEvent::Done {
                outcome: LlmDoneOutcome::Success {
                    stop_reason: StopReason::EndTurn,
                },
            });
        })
    }

    fn provider(&self) -> &'static str {
        "delayed-test"
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

fn spawn_spec(profile: &str, member_id: &str) -> SpawnMemberSpec {
    SpawnMemberSpec::from_wire(
        profile.to_string(),
        member_id.to_string(),
        Some(format!("You are {member_id}. Keep responses concise.")),
        None,
        None,
    )
}

async fn build_runtime_fixture() -> RuntimeFixture {
    build_runtime_fixture_with_client(Arc::new(TestClient::default())).await
}

async fn build_runtime_fixture_with_client(
    default_llm_client: Arc<dyn LlmClient>,
) -> RuntimeFixture {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));

    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "phase-a-mob"

[profiles.lead]
model = "gpt-5.2"
external_addressable = true

[profiles.lead.tools]
comms = true

[profiles.worker]
model = "gpt-5.2"
external_addressable = true

[profiles.worker.tools]
comms = true
"#,
    )
    .expect("parse test mob definition");

    let runtime = RealMobRuntime::bootstrap(
        MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service).with_options(
            MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: Some(default_llm_client),
            },
        ),
    )
    .await
    .expect("bootstrap runtime");

    RuntimeFixture {
        _temp_dir: temp_dir,
        runtime,
    }
}

async fn send_interaction_request(
    app: &Router,
    member_id: &str,
    message: &str,
) -> (StatusCode, Value) {
    let request_body = json!({
        "member_id": member_id,
        "message": message
    })
    .to_string();
    let request = Request::builder()
        .method("POST")
        .uri("/interactions/stream")
        .header("content-type", "application/json")
        .body(Body::from(request_body))
        .expect("request");

    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    let parsed = serde_json::from_slice::<Value>(&body).expect("json error payload");
    (status, parsed)
}

async fn start_http_server(
    app: Router,
) -> (std::net::SocketAddr, oneshot::Sender<()>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral listener");
    let address = listener.local_addr().expect("listener address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("serve interactions stream endpoint");
    });
    (address, shutdown_tx, server)
}

async fn post_interactions_stream_raw(
    address: std::net::SocketAddr,
    request_body: &str,
    timeout: Duration,
) -> String {
    let mut stream = TcpStream::connect(address)
        .await
        .expect("connect stream endpoint");
    let request = format!(
        "POST /interactions/stream HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{request_body}",
        request_body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    stream.flush().await.expect("flush request");

    let mut bytes = Vec::new();
    let keep_alive_marker = b": keep-alive";
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let mut chunk = [0_u8; 4096];
        match tokio::time::timeout(remaining, stream.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(read)) => {
                bytes.extend_from_slice(&chunk[..read]);
                if bytes
                    .windows(keep_alive_marker.len())
                    .any(|window| window == keep_alive_marker)
                {
                    break;
                }
            }
            Ok(Err(err)) => panic!("read response failed: {err}"),
            Err(_) => break,
        }
    }

    String::from_utf8(bytes).expect("utf8 response")
}

#[tokio::test]
async fn phase_a_runtime_001_bootstrap_discovery_reconcile_spawn_resume_real_mob_path() {
    let fixture = build_runtime_fixture().await;
    assert_eq!(fixture.runtime.status(), MobState::Running);
    assert!(fixture.runtime.discover().await.is_empty());

    fixture
        .runtime
        .spawn(spawn_spec("lead", "lead-1"))
        .await
        .expect("spawn lead");

    let discovered_after_spawn = fixture.runtime.discover().await;
    assert_eq!(discovered_after_spawn.len(), 1);
    assert_eq!(discovered_after_spawn[0].meerkat_id, "lead-1");
    assert_eq!(discovered_after_spawn[0].profile, "lead");
    assert_eq!(discovered_after_spawn[0].state, "active");

    let reconcile = fixture
        .runtime
        .reconcile(vec![
            spawn_spec("lead", "lead-1"),
            spawn_spec("worker", "worker-1"),
        ])
        .await
        .expect("reconcile");

    assert_eq!(reconcile.desired, vec!["lead-1", "worker-1"]);
    assert_eq!(reconcile.retained, vec!["lead-1"]);
    assert_eq!(reconcile.spawned, vec!["worker-1"]);
    assert_eq!(reconcile.retired, Vec::<String>::new());

    let discovered_after_reconcile = fixture.runtime.discover().await;
    assert_eq!(discovered_after_reconcile.len(), 2);
    assert!(discovered_after_reconcile
        .iter()
        .any(|member| member.meerkat_id == "worker-1"));

    fixture.runtime.stop().await.expect("stop runtime");
    assert_eq!(fixture.runtime.status(), MobState::Stopped);
    fixture.runtime.resume().await.expect("resume runtime");
    assert_eq!(fixture.runtime.status(), MobState::Running);

    fixture
        .runtime
        .handle()
        .retire_all()
        .await
        .expect("retire all");
}

#[tokio::test]
async fn phase_a_runtime_002_reconcile_retires_stale_members_by_default() {
    let fixture = build_runtime_fixture().await;
    fixture
        .runtime
        .spawn(spawn_spec("lead", "lead-1"))
        .await
        .expect("spawn lead");
    fixture
        .runtime
        .spawn(spawn_spec("worker", "worker-1"))
        .await
        .expect("spawn worker");

    let reconcile = fixture
        .runtime
        .reconcile(vec![spawn_spec("lead", "lead-1")])
        .await
        .expect("reconcile");

    assert_eq!(reconcile.desired, vec!["lead-1"]);
    assert_eq!(reconcile.retained, vec!["lead-1"]);
    assert_eq!(reconcile.spawned, Vec::<String>::new());
    assert_eq!(reconcile.retired, vec!["worker-1"]);

    let discovered = fixture.runtime.discover().await;
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].meerkat_id, "lead-1");
    fixture
        .runtime
        .handle()
        .retire_all()
        .await
        .expect("retire all");
}

#[tokio::test]
async fn phase_a_http_001_inject_and_subscribe_sse_framing_streams_events() {
    let fixture = build_runtime_fixture().await;
    fixture
        .runtime
        .spawn(spawn_spec("lead", "lead-1"))
        .await
        .expect("spawn lead");

    let app = interaction_sse_router(fixture.runtime.clone());
    let request_body = json!({
        "member_id": "lead-1",
        "message": "Reply in one sentence."
    })
    .to_string();
    let request = Request::builder()
        .method("POST")
        .uri("/interactions/stream")
        .header("content-type", "application/json")
        .body(Body::from(request_body))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream")));

    let body = tokio::time::timeout(
        Duration::from_secs(20),
        to_bytes(response.into_body(), 2 * 1024 * 1024),
    )
    .await
    .expect("sse stream completed within timeout")
    .expect("response body");
    let body_text = String::from_utf8(body.to_vec()).expect("utf8 body");

    assert!(body_text.contains("event: interaction_started"));
    assert!(body_text.contains("data: {\"interaction_id\":"));
    assert!(body_text.contains("id: "));
    assert!(body_text.matches("event: ").count() >= 2);
    assert!(body_text.contains("data: {\"type\":"));
    assert!(body_text.contains("\n\n"));

    fixture
        .runtime
        .handle()
        .retire_all()
        .await
        .expect("retire all");
}

#[tokio::test]
async fn phase_a_http_002_empty_member_id_or_message_returns_400() {
    let fixture = build_runtime_fixture().await;
    let app = interaction_sse_router(fixture.runtime.clone());

    let (status_member, payload_member) = send_interaction_request(&app, "", "hello").await;
    assert_eq!(status_member, StatusCode::BAD_REQUEST);
    assert_eq!(
        payload_member,
        json!({
            "error": "member_id must not be empty"
        })
    );

    let (status_message, payload_message) = send_interaction_request(&app, "lead-1", "").await;
    assert_eq!(status_message, StatusCode::BAD_REQUEST);
    assert_eq!(
        payload_message,
        json!({
            "error": "message must not be empty"
        })
    );
}

#[tokio::test]
async fn phase_a_http_003_unknown_member_returns_404() {
    let fixture = build_runtime_fixture().await;
    let app = interaction_sse_router(fixture.runtime.clone());

    let (status, payload) = send_interaction_request(&app, "missing-member", "hello").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        payload,
        json!({
            "error": "member_not_found"
        })
    );
}

#[tokio::test]
async fn phase_a_http_004_internal_runtime_error_returns_500_sanitized_payload() {
    let fixture = build_runtime_fixture().await;
    fixture
        .runtime
        .spawn(spawn_spec("lead", "lead-1"))
        .await
        .expect("spawn lead");
    fixture.runtime.stop().await.expect("stop runtime");

    let app = interaction_sse_router(fixture.runtime.clone());
    let (status, payload) = send_interaction_request(&app, "lead-1", "hello").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        payload,
        json!({
            "error": "internal_server_error"
        })
    );
}

#[tokio::test]
async fn phase_a_http_005_keep_alive() {
    let fixture = build_runtime_fixture_with_client(Arc::new(DelayedTestClient::new(
        Duration::from_millis(150),
    )))
    .await;
    fixture
        .runtime
        .spawn(spawn_spec("lead", "lead-1"))
        .await
        .expect("spawn lead");

    let app = interaction_sse_router_with_keep_alive_interval(
        fixture.runtime.clone(),
        Duration::from_millis(10),
    );
    let (address, shutdown_tx, server_handle) = start_http_server(app).await;

    let request_body = json!({
        "member_id": "lead-1",
        "message": "Reply in one sentence."
    })
    .to_string();
    let response =
        post_interactions_stream_raw(address, &request_body, Duration::from_secs(5)).await;

    assert!(
        response.starts_with("HTTP/1.1 200"),
        "expected HTTP 200 response, got: {response}"
    );
    let normalized = response.to_ascii_lowercase();
    assert!(
        normalized.contains("content-type: text/event-stream"),
        "expected SSE content-type header, got: {response}"
    );
    assert!(
        response.contains(": keep-alive"),
        "expected keep-alive marker in SSE stream, got: {response}"
    );

    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(2), server_handle)
        .await
        .expect("server should shut down")
        .expect("server task should complete");

    fixture
        .runtime
        .handle()
        .retire_all()
        .await
        .expect("retire all");
}
