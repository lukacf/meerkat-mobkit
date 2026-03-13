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
use meerkat_mob::MobHandle;
use meerkat_mob::{MobDefinition, MobSessionService, MobState, MobStorage, SpawnMemberSpec};
use meerkat_mobkit_core::{
    DiscoverySpec, ErrorEvent, ErrorHook, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig,
    PostReconcileHook, PostSpawnHook, UnifiedRuntime, UnifiedRuntimeReconcileReport,
};
use tokio::sync::Mutex;

fn spawn_spec(profile: &str, member_id: &str) -> SpawnMemberSpec {
    SpawnMemberSpec::from_wire(
        profile.to_string(),
        member_id.to_string(),
        Some(format!("You are {member_id}. Keep responses concise.")),
        None,
        None,
    )
}

fn build_session_service(temp_dir: &tempfile::TempDir) -> Arc<dyn MobSessionService> {
    let session_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");
    let factory = AgentFactory::new(&session_path).comms(true);
    Arc::new(build_ephemeral_service(factory, Config::default(), 16))
}

fn build_mob_spec(temp_dir: &tempfile::TempDir) -> MobBootstrapSpec {
    let definition = MobDefinition::from_toml(
        r#"
[mob]
id = "hooks-test-mob"

[profiles.worker]
model = "gpt-5.2"
external_addressable = true

[profiles.worker.tools]
comms = true
"#,
    )
    .expect("parse mob definition");

    MobBootstrapSpec::new(
        definition,
        MobStorage::in_memory(),
        build_session_service(temp_dir),
    )
    .with_options(MobBootstrapOptions {
        allow_ephemeral_sessions: true,
        notify_orchestrator_on_resume: true,
        default_llm_client: Some(Arc::new(TestClient::default())),
    })
}

fn empty_module_config() -> MobKitConfig {
    MobKitConfig {
        modules: vec![],
        discovery: DiscoverySpec {
            namespace: "hooks-test".to_string(),
            modules: vec![],
        },
        pre_spawn: vec![],
    }
}

#[tokio::test]
async fn post_spawn_hook_receives_spawned_member_id() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let spawned_ids: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let captured = spawned_ids.clone();

    let hook: PostSpawnHook = Arc::new(move |ids| {
        let captured = captured.clone();
        Box::pin(async move {
            captured.lock().await.push(ids);
        })
    });

    let runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(empty_module_config())
        .timeout(Duration::from_secs(2))
        .post_spawn_hook(hook)
        .build()
        .await
        .expect("build unified runtime");

    assert_eq!(runtime.status(), MobState::Running);

    runtime
        .spawn(spawn_spec("worker", "hook-worker-1"))
        .await
        .expect("spawn member");

    let captured = spawned_ids.lock().await;
    assert_eq!(captured.len(), 1, "post-spawn hook should be called once");
    assert_eq!(
        captured[0],
        vec!["hook-worker-1".to_string()],
        "post-spawn hook should receive the spawned member id"
    );

    runtime.shutdown().await;
}

#[tokio::test]
async fn post_reconcile_hook_receives_reconcile_report() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let reports: Arc<Mutex<Vec<UnifiedRuntimeReconcileReport>>> = Arc::new(Mutex::new(Vec::new()));
    let captured = reports.clone();

    let hook: PostReconcileHook = Arc::new(move |report| {
        let captured = captured.clone();
        Box::pin(async move {
            captured.lock().await.push(report);
        })
    });

    let runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(empty_module_config())
        .timeout(Duration::from_secs(2))
        .post_reconcile_hook(hook)
        .build()
        .await
        .expect("build unified runtime");

    let desired = vec![
        spawn_spec("worker", "reconcile-a"),
        spawn_spec("worker", "reconcile-b"),
    ];
    let report = runtime
        .reconcile(desired)
        .await
        .expect("reconcile should succeed");

    let captured = reports.lock().await;
    assert_eq!(
        captured.len(),
        1,
        "post-reconcile hook should be called once"
    );
    assert_eq!(
        captured[0], report,
        "post-reconcile hook should receive the same report returned by reconcile"
    );
    assert_eq!(captured[0].mob.spawned.len(), 2);
    assert!(captured[0].mob.spawned.contains(&"reconcile-a".to_string()));
    assert!(captured[0].mob.spawned.contains(&"reconcile-b".to_string()));

    runtime.shutdown().await;
}

#[tokio::test]
async fn mob_handle_returns_working_handle() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(empty_module_config())
        .timeout(Duration::from_secs(2))
        .build()
        .await
        .expect("build unified runtime");

    let handle: MobHandle = runtime.mob_handle();

    // The handle should report running state
    assert_eq!(handle.status(), MobState::Running);

    // Spawn via the handle directly
    handle
        .spawn_spec(spawn_spec("worker", "handle-worker-1"))
        .await
        .expect("spawn via mob_handle");

    // Verify the member is visible through the handle
    let members = handle.list_members().await;
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].meerkat_id.to_string(), "handle-worker-1");

    runtime.shutdown().await;
}

#[tokio::test]
async fn no_hook_still_works() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(empty_module_config())
        .timeout(Duration::from_secs(2))
        .build()
        .await
        .expect("build unified runtime");

    // spawn without hooks set should work fine
    runtime
        .spawn(spawn_spec("worker", "no-hook-worker"))
        .await
        .expect("spawn without hook");

    // reconcile without hooks set should work fine
    let report = runtime
        .reconcile(vec![
            spawn_spec("worker", "no-hook-worker"),
            spawn_spec("worker", "no-hook-worker-2"),
        ])
        .await
        .expect("reconcile without hook");

    assert_eq!(report.mob.spawned.len(), 1);
    assert!(report.mob.spawned.contains(&"no-hook-worker-2".to_string()));

    runtime.shutdown().await;
}

#[tokio::test]
async fn error_hook_fires_on_spawn_failure() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let errors: Arc<Mutex<Vec<ErrorEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let captured = errors.clone();

    let hook: ErrorHook = Arc::new(move |event| {
        let captured = captured.clone();
        Box::pin(async move {
            captured.lock().await.push(event);
        })
    });

    let runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(empty_module_config())
        .timeout(Duration::from_secs(2))
        .on_error(hook)
        .build()
        .await
        .expect("build unified runtime");

    // Spawn with a non-existent profile to trigger a failure
    let result = runtime
        .spawn(SpawnMemberSpec::from_wire(
            "nonexistent-profile".to_string(),
            "error-hook-agent".to_string(),
            None,
            None,
            None,
        ))
        .await;
    assert!(result.is_err(), "spawn should fail for nonexistent profile");

    // Yield to let the fire-and-forget error hook task complete
    tokio::task::yield_now().await;

    let captured = errors.lock().await;
    assert_eq!(captured.len(), 1, "error hook should fire once");
    match &captured[0] {
        ErrorEvent::SpawnFailure {
            member_id,
            profile,
            error,
        } => {
            assert_eq!(member_id, "error-hook-agent");
            assert_eq!(profile, "nonexistent-profile");
            assert!(!error.is_empty(), "error message should not be empty");
        }
        other => panic!("expected SpawnFailure, got {other:?}"),
    }

    runtime.shutdown().await;
}

#[tokio::test]
async fn list_members_returns_roster() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(empty_module_config())
        .timeout(Duration::from_secs(2))
        .build()
        .await
        .expect("build unified runtime");

    runtime
        .spawn(spawn_spec("worker", "roster-a"))
        .await
        .expect("spawn roster-a");
    runtime
        .spawn(spawn_spec("worker", "roster-b"))
        .await
        .expect("spawn roster-b");

    let members = runtime.list_members().await;
    assert_eq!(members.len(), 2, "should list both members");
    let ids: Vec<&str> = members.iter().map(|m| m.meerkat_id.as_str()).collect();
    assert!(ids.contains(&"roster-a"), "should contain roster-a");
    assert!(ids.contains(&"roster-b"), "should contain roster-b");

    for m in &members {
        assert_eq!(m.profile, "worker");
        assert_eq!(m.state, "active");
    }

    runtime.shutdown().await;
}

#[tokio::test]
async fn get_member_returns_snapshot_or_none() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(empty_module_config())
        .timeout(Duration::from_secs(2))
        .build()
        .await
        .expect("build unified runtime");

    runtime
        .spawn(spawn_spec("worker", "get-member-1"))
        .await
        .expect("spawn get-member-1");

    let found = runtime.get_member("get-member-1").await;
    assert!(found.is_some(), "should find spawned member");
    let snapshot = found.unwrap();
    assert_eq!(snapshot.meerkat_id, "get-member-1");
    assert_eq!(snapshot.profile, "worker");
    assert_eq!(snapshot.state, "active");

    let not_found = runtime.get_member("nonexistent").await;
    assert!(not_found.is_none(), "should return None for unknown member");

    runtime.shutdown().await;
}

#[tokio::test]
async fn retire_member_transitions_state() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(empty_module_config())
        .timeout(Duration::from_secs(2))
        .build()
        .await
        .expect("build unified runtime");

    runtime
        .spawn(spawn_spec("worker", "retire-me"))
        .await
        .expect("spawn retire-me");

    runtime
        .retire_member("retire-me")
        .await
        .expect("retire should succeed");

    // After retire, the member either shows as "retiring" (if it has active
    // work to drain) or is already gone (idle member disposes immediately).
    let members = runtime.list_members().await;
    let retired = members.iter().find(|m| m.meerkat_id == "retire-me");
    match retired {
        Some(m) => assert_eq!(
            m.state, "retiring",
            "if still visible, state should be retiring"
        ),
        None => {} // idle member was immediately disposed — acceptable
    }

    runtime.shutdown().await;
}

#[tokio::test]
async fn respawn_member_replaces_member() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let runtime = UnifiedRuntime::builder()
        .mob_spec(build_mob_spec(&temp_dir))
        .module_config(empty_module_config())
        .timeout(Duration::from_secs(2))
        .build()
        .await
        .expect("build unified runtime");

    runtime
        .spawn(spawn_spec("worker", "respawn-me"))
        .await
        .expect("spawn respawn-me");

    runtime
        .respawn_member("respawn-me")
        .await
        .expect("respawn should succeed");

    // After respawn, the member should still exist in the roster
    let found = runtime.get_member("respawn-me").await;
    assert!(found.is_some(), "member should still exist after respawn");

    runtime.shutdown().await;
}
