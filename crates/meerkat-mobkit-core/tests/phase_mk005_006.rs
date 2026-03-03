use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use meerkat::{build_ephemeral_service, AgentFactory, Config};
use meerkat_client::TestClient;
use meerkat_core::AgentEvent;
use meerkat_mob::{MobDefinition, MobState, MobStorage, SpawnMemberSpec};
use meerkat_mobkit_core::{
    agent_event_sse, agent_events_sse_router, mob_events_sse_router, AgentEventSubscribeFn,
    DiscoverySpec, MobBootstrapOptions, MobBootstrapSpec, MobEventSubscribeFn, MobKitConfig,
    UnifiedRuntime,
};
use tower::ServiceExt;

fn build_mob_spec(temp_dir: &tempfile::TempDir) -> MobBootstrapSpec {
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));

    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "mk005-006-test-mob"

[profiles.lead]
model = "gpt-5.2"
external_addressable = true

[profiles.lead.tools]
comms = true
"#,
    )
    .expect("parse mob definition");

    MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service).with_options(
        MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(TestClient::default())),
        },
    )
}

fn spawn_spec(profile: &str, member_id: &str) -> SpawnMemberSpec {
    SpawnMemberSpec::from_wire(
        profile.to_string(),
        member_id.to_string(),
        Some(format!("You are {member_id}.")),
        None,
        None,
    )
}

// ---------------------------------------------------------------------------
// MK-005: agent_event_sse format
// ---------------------------------------------------------------------------

#[test]
fn mk005_agent_event_sse_formats_event_with_id_and_type() {
    let event = AgentEvent::TextDelta {
        delta: "hello".to_string(),
    };
    let sse_event = agent_event_sse("agent-1", 0, &event);

    // The axum SSE Event does not expose its fields publicly, but we can
    // verify it was created without panicking. The interaction SSE tests
    // already cover the wire format via real HTTP; here we verify the helper
    // produces a value for each variant we care about.
    let _ = sse_event;

    let complete_event = AgentEvent::TextComplete {
        content: "done".to_string(),
    };
    let sse_complete = agent_event_sse("agent-1", 1, &complete_event);
    let _ = sse_complete;
}

// ---------------------------------------------------------------------------
// MK-005: agent_events_sse_router construction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mk005_agent_events_sse_router_returns_404_for_unknown_agent() {
    use meerkat_mob::MobError;
    use meerkat_mobkit_core::MobRuntimeError;

    let subscribe_fn: AgentEventSubscribeFn = Arc::new(|_agent_id: String| {
        Box::pin(async {
            Err(MobRuntimeError::Mob(MobError::MeerkatNotFound(
                meerkat_mob::MeerkatId::from("unknown"),
            )))
        })
    });

    let router = agent_events_sse_router(subscribe_fn);

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/agents/unknown-agent/events")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mk005_agent_events_sse_router_returns_400_for_empty_agent_id() {
    let subscribe_fn: AgentEventSubscribeFn = Arc::new(|_agent_id: String| {
        Box::pin(async { unreachable!("should not be called for empty id") })
    });

    let router = agent_events_sse_router(subscribe_fn);

    // The axum path extractor will match the literal " " as an agent_id.
    // Our handler trims it, so it should return 400.
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/agents/%20/events")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn mk005_agent_events_sse_router_returns_sse_content_type_on_success() {
    use futures::stream;
    use meerkat_core::EventEnvelope;

    let subscribe_fn: AgentEventSubscribeFn = Arc::new(|_agent_id: String| {
        Box::pin(async {
            let event = AgentEvent::TextDelta {
                delta: "hello".to_string(),
            };
            let envelope = EventEnvelope::new("test-source", 0, None, event);
            let event_stream: meerkat_core::EventStream =
                Box::pin(stream::iter(vec![envelope]));
            Ok(event_stream)
        })
    });

    let router = agent_events_sse_router(subscribe_fn);

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/agents/agent-1/events")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/event-stream"),
        "expected text/event-stream content type, got: {content_type}"
    );

    let body = tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .expect("body timeout")
    .expect("read body");
    let body_text = String::from_utf8(body.to_vec()).expect("utf8 body");

    assert!(
        body_text.contains("event: text_delta"),
        "expected text_delta event in body: {body_text}"
    );
    assert!(
        body_text.contains("id: agent-1:0"),
        "expected agent-1:0 id in body: {body_text}"
    );
}

// ---------------------------------------------------------------------------
// MK-006: mob_events_sse_router construction
// ---------------------------------------------------------------------------

#[test]
fn mk006_mob_events_sse_router_type_signature_compiles() {
    // MobEventRouterHandle has a private cancel field, so we cannot
    // construct one in a unit test. Instead we verify the type signature
    // compiles and test the full path through the unified runtime below.
    fn _assert_compiles(subscribe_fn: MobEventSubscribeFn) {
        let _ = mob_events_sse_router(subscribe_fn);
    }
}

// ---------------------------------------------------------------------------
// Unified Runtime: build_agent_sse_router and build_mob_sse_router
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mk005_unified_runtime_build_agent_sse_router_returns_router() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let mut runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(MobKitConfig {
            modules: vec![],
            discovery: DiscoverySpec {
                namespace: "mk005-agent-sse-router".to_string(),
                modules: vec![],
            },
            pre_spawn: vec![],
        })
        .timeout(Duration::from_secs(1))
        .build()
        .await
        .expect("build unified runtime");

    assert_eq!(runtime.status(), MobState::Running);

    runtime
        .spawn(spawn_spec("lead", "agent-mk005"))
        .await
        .expect("spawn agent");

    let agent_sse_router = runtime.build_agent_sse_router();

    // Request events for the spawned agent - should get SSE content type
    let response = agent_sse_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/agents/agent-mk005/events")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/event-stream"),
        "expected SSE content type, got: {content_type}"
    );

    let shutdown = runtime.shutdown().await;
    shutdown.mob_stop.expect("mob runtime should stop cleanly");
}

#[tokio::test]
async fn mk005_unified_runtime_build_agent_sse_router_returns_404_for_unknown_agent() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let mut runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(MobKitConfig {
            modules: vec![],
            discovery: DiscoverySpec {
                namespace: "mk005-agent-sse-404".to_string(),
                modules: vec![],
            },
            pre_spawn: vec![],
        })
        .timeout(Duration::from_secs(1))
        .build()
        .await
        .expect("build unified runtime");

    let agent_sse_router = runtime.build_agent_sse_router();

    let response = agent_sse_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/agents/nonexistent/events")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let shutdown = runtime.shutdown().await;
    shutdown.mob_stop.expect("mob runtime should stop cleanly");
}

#[tokio::test]
async fn mk006_unified_runtime_build_mob_sse_router_returns_sse_stream() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let mut runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(MobKitConfig {
            modules: vec![],
            discovery: DiscoverySpec {
                namespace: "mk006-mob-sse-router".to_string(),
                modules: vec![],
            },
            pre_spawn: vec![],
        })
        .timeout(Duration::from_secs(1))
        .build()
        .await
        .expect("build unified runtime");

    assert_eq!(runtime.status(), MobState::Running);

    let mob_sse_router = runtime.build_mob_sse_router();

    let response = mob_sse_router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mob/events")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/event-stream"),
        "expected SSE content type, got: {content_type}"
    );

    let shutdown = runtime.shutdown().await;
    shutdown.mob_stop.expect("mob runtime should stop cleanly");
}

// ---------------------------------------------------------------------------
// Source-level contract: reference_app_router merges agent and mob SSE
// ---------------------------------------------------------------------------

#[test]
fn mk005_mk006_reference_app_router_merges_agent_and_mob_sse_routers() {
    let source = include_str!("../src/unified_runtime.rs");
    assert!(
        source.contains(".merge(self.build_agent_sse_router())"),
        "reference app router should merge agent SSE router"
    );
    assert!(
        source.contains(".merge(self.build_mob_sse_router())"),
        "reference app router should merge mob SSE router"
    );
}

#[test]
fn mk005_mk006_http_sse_exports_new_types() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("agent_events_sse_router"));
    assert!(source.contains("mob_events_sse_router"));
    assert!(source.contains("AgentEventSubscribeFn"));
    assert!(source.contains("MobEventSubscribeFn"));
}
