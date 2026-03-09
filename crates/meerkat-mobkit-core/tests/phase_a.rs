use std::sync::Arc;

use meerkat::{build_ephemeral_service, AgentFactory, Config};
use meerkat_client::TestClient;
use meerkat_mob::{MobDefinition, MobState, MobStorage, SpawnMemberSpec};
use meerkat_mobkit_core::{
    MobBootstrapOptions, MobBootstrapSpec, RealMobRuntime,
};
use tempfile::TempDir;

struct RuntimeFixture {
    _temp_dir: TempDir,
    runtime: RealMobRuntime,
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
                default_llm_client: Some(Arc::new(TestClient::default())),
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

