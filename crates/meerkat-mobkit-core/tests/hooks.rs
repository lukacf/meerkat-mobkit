use std::sync::Arc;
use std::time::Duration;

use meerkat::{build_ephemeral_service, AgentFactory, Config};
use meerkat_client::TestClient;
use meerkat_mob::{MobDefinition, MobSessionService, MobState, MobStorage, SpawnMemberSpec};
use meerkat_mob::MobHandle;
use meerkat_mobkit_core::{
    DiscoverySpec, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig,
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

    let mut runtime = UnifiedRuntime::builder()
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
    let reports: Arc<Mutex<Vec<UnifiedRuntimeReconcileReport>>> =
        Arc::new(Mutex::new(Vec::new()));
    let captured = reports.clone();

    let hook: PostReconcileHook = Arc::new(move |report| {
        let captured = captured.clone();
        Box::pin(async move {
            captured.lock().await.push(report);
        })
    });

    let mut runtime = UnifiedRuntime::builder()
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
    let mut runtime = UnifiedRuntime::builder()
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
    let mut runtime = UnifiedRuntime::builder()
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
