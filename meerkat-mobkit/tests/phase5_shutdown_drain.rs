#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::collapsible_if,
    clippy::redundant_clone,
    clippy::needless_raw_string_hashes,
    clippy::single_match,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_pattern_matching,
    clippy::ignored_unit_patterns,
    clippy::clone_on_copy,
    clippy::manual_assert,
    clippy::unwrap_in_result,
    clippy::useless_vec
)]
use std::sync::Arc;
use std::time::Duration;

use meerkat::{AgentFactory, Config, build_ephemeral_service};
use meerkat_client::TestClient;
use meerkat_mob::{MobStorage, Prefab, SpawnMemberSpec};
use meerkat_mobkit::{
    DiscoverySpec, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig, UnifiedRuntime,
};

fn mob_spec(temp_dir: &tempfile::TempDir) -> MobBootstrapSpec {
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(build_ephemeral_service(factory, Config::default(), 16));

    let mut definition = Prefab::CodingSwarm.definition();
    for profile in definition.profiles.values_mut() {
        profile.model = "gpt-5.2".to_string();
    }

    MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service).with_options(
        MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(TestClient::default())),
        },
    )
}

fn empty_module_config(namespace: &str) -> MobKitConfig {
    MobKitConfig {
        modules: vec![],
        discovery: DiscoverySpec {
            namespace: namespace.to_string(),
            modules: vec![],
        },
        pre_spawn: vec![],
    }
}

fn member_spec(profile: &str, member_id: &str) -> SpawnMemberSpec {
    SpawnMemberSpec::from_wire(
        profile.to_string(),
        member_id.to_string(),
        Some(format!("You are {member_id}.")),
        None,
        None,
    )
}

#[tokio::test]
async fn shutdown_drain_completes_immediately_with_no_active_members() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = UnifiedRuntime::bootstrap(
        mob_spec(&temp_dir),
        empty_module_config("drain-no-active"),
        Duration::from_secs(2),
    )
    .await
    .expect("bootstrap");

    let shutdown = runtime.shutdown().await;

    // Drain cycles through the event ingress loop (typically 2 passes even for empty mobs)
    assert!(!shutdown.drain.timed_out);
    assert!(shutdown.drain.drain_duration_ms < 1000);
    assert_eq!(shutdown.module_shutdown.orphan_processes, 0);
    shutdown.mob_stop.expect("mob stop should succeed");
}

#[tokio::test]
#[ignore]
async fn shutdown_drain_report_fields_populated_with_active_members() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = UnifiedRuntime::builder()
        .mob_spec(mob_spec(&temp_dir))
        .module_config(empty_module_config("drain-active"))
        .timeout(Duration::from_secs(2))
        .drain_timeout(Duration::from_millis(200))
        .build()
        .await
        .expect("build");

    runtime
        .spawn(member_spec("lead", "drain-lead-1"))
        .await
        .expect("spawn member");

    let shutdown = runtime.shutdown().await;

    // Drain cycles through the mob event ingress and completes once the channel is quiescent.
    assert!(shutdown.drain.drained_count > 0);
    assert!(!shutdown.drain.timed_out);
    shutdown.mob_stop.expect("mob stop should succeed");
}

#[tokio::test]
async fn shutdown_drain_uses_default_timeout_from_bootstrap() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = UnifiedRuntime::bootstrap(
        mob_spec(&temp_dir),
        empty_module_config("drain-default"),
        Duration::from_secs(2),
    )
    .await
    .expect("bootstrap");

    // With no active members, drain completes immediately regardless of
    // timeout. This test just validates the default path works without panic.
    let shutdown = runtime.shutdown().await;

    assert!(!shutdown.drain.timed_out);
    shutdown.mob_stop.expect("mob stop should succeed");
}

#[tokio::test]
#[ignore]
async fn shutting_down_flag_prevents_new_dispatches_during_drain() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = UnifiedRuntime::builder()
        .mob_spec(mob_spec(&temp_dir))
        .module_config(empty_module_config("drain-flag"))
        .timeout(Duration::from_secs(2))
        .drain_timeout(Duration::from_millis(100))
        .build()
        .await
        .expect("build");

    // Trigger shutdown to set the shutting_down flag
    let _shutdown = runtime.shutdown().await;

    // After shutdown, dispatch_schedule_tick should be rejected
    let result = runtime.dispatch_schedule_tick(&[], 0).await;
    assert!(
        result.is_err(),
        "dispatch_schedule_tick should fail after shutdown"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("shutting down"),
        "error should mention shutting down, got: {err}"
    );
}
