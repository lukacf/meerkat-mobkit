#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::redundant_clone,
    clippy::needless_raw_string_hashes,
    clippy::ignored_unit_patterns,
    clippy::useless_vec
)]
//! TDD tests for the new UnifiedRuntimeBuilder convenience API.
//!
//! Each test is written before the corresponding implementation code exists.
//! They cover: definition loading, persistent/ephemeral paths, session hooks,
//! capability flags, defaults, and backward-compat escape hatch.

use std::sync::Arc;

use async_trait::async_trait;
use meerkat_client::TestClient;
use meerkat_core::service::{CreateSessionRequest, SessionError};
use meerkat_mob::{MobDefinition, MobState, MobStorage};
use meerkat_mobkit::{
    DiscoverySpec, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig, SessionHook, UnifiedRuntime,
};

const MINIMAL_MOB_TOML: &str = r#"
[mob]
id = "builder-test-mob"

[profiles.worker]
model = "test-model"
"#;

// ---------------------------------------------------------------------------
// Helper: build a definition from the inline TOML
// ---------------------------------------------------------------------------
fn test_definition() -> MobDefinition {
    MobDefinition::from_toml(MINIMAL_MOB_TOML).expect("parse test mob definition")
}

// ---------------------------------------------------------------------------
// 1. Builder ephemeral (no persistent_state → auto temp dir)
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore]
async fn test_builder_ephemeral() {
    let runtime = UnifiedRuntime::builder()
        .definition(test_definition())
        .default_llm_client(Arc::new(TestClient::default()))
        .build()
        .await
        .expect("ephemeral build");

    assert_eq!(
        runtime.mob_handle().status().await.unwrap(),
        MobState::Running
    );
    runtime.mob_handle().stop().await.expect("stop");
}

// ---------------------------------------------------------------------------
// 2. Builder persistent (SQLite session store + redb mob storage)
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore]
async fn test_builder_persistent_default() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let state_path = tmp.path().join("state");

    let runtime = UnifiedRuntime::builder()
        .definition(test_definition())
        .persistent_state(&state_path)
        .default_llm_client(Arc::new(TestClient::default()))
        .build()
        .await
        .expect("persistent build");

    assert_eq!(
        runtime.mob_handle().status().await.unwrap(),
        MobState::Running
    );

    // Verify persistent artifacts were created.
    assert!(
        state_path.join("sessions.db").exists(),
        "SQLite session store must be created"
    );

    runtime.mob_handle().stop().await.expect("stop");
}

// ---------------------------------------------------------------------------
// 3. Builder TOML definition from path
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore]
async fn test_builder_toml_definition() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let toml_path = tmp.path().join("mob.toml");
    std::fs::write(&toml_path, MINIMAL_MOB_TOML).expect("write toml");

    let runtime = UnifiedRuntime::builder()
        .definition_path(&toml_path)
        .default_llm_client(Arc::new(TestClient::default()))
        .build()
        .await
        .expect("toml definition build");

    assert_eq!(
        runtime.mob_handle().status().await.unwrap(),
        MobState::Running
    );
    runtime.mob_handle().stop().await.expect("stop");
}

// ---------------------------------------------------------------------------
// 4. Capability flags — shell(false) propagates
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore]
async fn test_builder_capability_flags() {
    // This test verifies that the builder accepts capability flag methods
    // and that the runtime bootstraps successfully with modified flags.
    let runtime = UnifiedRuntime::builder()
        .definition(test_definition())
        .default_llm_client(Arc::new(TestClient::default()))
        .shell(false)
        .build()
        .await
        .expect("build with shell disabled");

    assert_eq!(
        runtime.mob_handle().status().await.unwrap(),
        MobState::Running
    );
    runtime.mob_handle().stop().await.expect("stop");
}

// ---------------------------------------------------------------------------
// 5. Session hook — before_create mutates request
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore]
async fn test_builder_session_hook_before_create() {
    struct LabelInjector;
    #[async_trait]
    impl SessionHook for LabelInjector {
        async fn before_create(&self, req: &mut CreateSessionRequest) -> Result<(), SessionError> {
            let labels = req.labels.get_or_insert_with(Default::default);
            labels.insert("injected".to_string(), "true".to_string());
            Ok(())
        }
    }

    let runtime = UnifiedRuntime::builder()
        .definition(test_definition())
        .default_llm_client(Arc::new(TestClient::default()))
        .session_hook(Arc::new(LabelInjector))
        .build()
        .await
        .expect("build with session hook");

    assert_eq!(
        runtime.mob_handle().status().await.unwrap(),
        MobState::Running
    );
    runtime.mob_handle().stop().await.expect("stop");
}

// ---------------------------------------------------------------------------
// 6. Backward-compat — .mob_spec() escape hatch still works
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore]
async fn test_builder_mob_spec_escape_hatch() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let session_path = tmp.path().join("sessions");
    std::fs::create_dir_all(&session_path).expect("session path");

    let factory = meerkat::AgentFactory::new(&session_path).comms(true);
    let session_service = Arc::new(meerkat::build_ephemeral_service(
        factory,
        meerkat::Config::default(),
        16,
    ));

    let spec = MobBootstrapSpec::new(test_definition(), MobStorage::in_memory(), session_service)
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(TestClient::default())),
        });

    let runtime = UnifiedRuntime::builder()
        .mob_spec(spec)
        .module_config(MobKitConfig {
            modules: Vec::new(),
            discovery: DiscoverySpec {
                namespace: String::new(),
                modules: Vec::new(),
            },
            pre_spawn: Vec::new(),
        })
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .await
        .expect("escape hatch build");

    assert_eq!(
        runtime.mob_handle().status().await.unwrap(),
        MobState::Running
    );
    runtime.mob_handle().stop().await.expect("stop");
}

// ---------------------------------------------------------------------------
// 7. Builder defaults — module_config and timeout are defaulted
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore]
async fn test_builder_defaults() {
    // When using the new .definition() path, module_config and timeout
    // must be defaulted (no longer required fields).
    let runtime = UnifiedRuntime::builder()
        .definition(test_definition())
        .default_llm_client(Arc::new(TestClient::default()))
        .build()
        .await
        .expect("build with defaults");

    assert_eq!(
        runtime.mob_handle().status().await.unwrap(),
        MobState::Running
    );
    runtime.mob_handle().stop().await.expect("stop");
}

// ---------------------------------------------------------------------------
// 8. Builder persistent with custom session store
// ---------------------------------------------------------------------------
#[tokio::test]
#[ignore]
async fn test_builder_persistent_custom_store() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let state_path = tmp.path().join("state");
    std::fs::create_dir_all(&state_path).expect("create state dir");

    // Open a custom SQLite store at a non-default path to prove the builder
    // uses it instead of creating its own.
    let custom_db_path = state_path.join("custom_sessions.db");
    let custom_store: std::sync::Arc<dyn meerkat::SessionStore> = std::sync::Arc::new(
        meerkat_store::SqliteSessionStore::open(&custom_db_path).expect("open custom store"),
    );

    let runtime = UnifiedRuntime::builder()
        .definition(test_definition())
        .persistent_state(&state_path)
        .session_store(custom_store)
        .default_llm_client(Arc::new(TestClient::default()))
        .build()
        .await
        .expect("persistent build with custom store");

    assert_eq!(
        runtime.mob_handle().status().await.unwrap(),
        MobState::Running
    );

    // The custom store path should exist (we created it).
    assert!(
        custom_db_path.exists(),
        "custom session store db must exist"
    );
    // The default sessions.db should NOT have been created by the builder.
    assert!(
        !state_path.join("sessions.db").exists(),
        "builder must use custom store, not create default SQLite"
    );

    runtime.mob_handle().stop().await.expect("stop");
}
