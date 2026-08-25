//! Persistent mob storage: durable mob state must survive a literal drop and
//! rebootstrap, and a resume must never silently boot a stale composition.
//!
//! Before this lane existed, the persistent launch paths composed
//! `MobStorage::in_memory()`, so every restart presented as a healthy boot
//! with the right member count and no durable mob state at all.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use meerkat::{AgentFactory, Config, build_ephemeral_service};
use meerkat_client::TestClient;
use meerkat_mob::{MobDefinition, MobStorage, SpawnMemberSpec};
use meerkat_mobkit::mob_composition_manifest::{
    MobCompositionProvenanceError, MobStorageProvenance, persistent_mob_storage,
};
use meerkat_mobkit::mob_handle_runtime::MobRuntimeError;
use meerkat_mobkit::{MobBootstrapOptions, MobBootstrapSpec, MobRuntime};

fn definition_with(profiles: &str) -> MobDefinition {
    MobDefinition::from_toml(profiles).expect("parse mob definition")
}

/// The comms participant registry is process-global and the supervisor name is
/// derived from the mob id, so every test needs its own id or parallel tests
/// collide with `ParticipantNameOccupied` on a name that has nothing to do
/// with what they are testing.
fn base_definition_for(mob_id: &str) -> MobDefinition {
    definition_with(&format!(
        r#"
[mob]
id = "{mob_id}"

[profiles.lead]
model = "gpt-5.5"
external_addressable = true

[profiles.lead.tools]
comms = true
"#
    ))
}

/// Same mob, one extra profile: a real composition change an operator could
/// make by editing their config between restarts.
fn diverged_definition_for(mob_id: &str) -> MobDefinition {
    definition_with(&format!(
        r#"
[mob]
id = "{mob_id}"

[profiles.lead]
model = "gpt-5.5"
external_addressable = true

[profiles.lead.tools]
comms = true

[profiles.worker]
model = "gpt-5.5"
external_addressable = true

[profiles.worker.tools]
comms = true
"#
    ))
}

fn options() -> MobBootstrapOptions {
    MobBootstrapOptions {
        allow_ephemeral_sessions: true,
        notify_orchestrator_on_resume: true,
        default_llm_client: Some(Arc::new(TestClient::default())),
    }
}

async fn session_service(session_root: &Path) -> Arc<dyn meerkat_mob::MobSessionService> {
    std::fs::create_dir_all(session_root).expect("session root");
    let factory = AgentFactory::new(session_root).comms(true);
    Arc::new(build_ephemeral_service(factory, Config::default(), 16))
}

/// Boot against a persistent mob path, declaring provenance the way the stock
/// launch paths do.
async fn boot(
    mob_path: &Path,
    session_root: &Path,
    definition: MobDefinition,
) -> Result<MobRuntime, MobRuntimeError> {
    let (storage, provenance) =
        persistent_mob_storage(mob_path.to_path_buf()).expect("open persistent mob storage");
    MobRuntime::bootstrap(
        MobBootstrapSpec::new(definition, storage, session_service(session_root).await)
            .with_mob_storage_provenance(provenance)
            .with_options(options()),
    )
    .await
}

/// The event log written by a live mob must still be on disk after the
/// runtime is shut down and dropped, and a fresh bootstrap against the same
/// path must resume it rather than create a second mob.
///
/// The observables are positive on purpose. Asserting that the second
/// bootstrap merely returns `Ok` would pass just as well against the defect
/// being fixed here, because creating a brand new empty mob also succeeds.
/// What cannot happen under the defect is a non-empty event log at the same
/// path after the process-side state is gone.
#[tokio::test]
async fn durable_mob_state_survives_drop_and_rebootstrap() {
    const MOB_ID: &str = "persistence-survives";
    let temp = tempfile::tempdir().expect("temp dir");
    let mob_path = temp.path().join("mob.sqlite3");
    let session_root = temp.path().join("sessions");

    let first = boot(&mob_path, &session_root, base_definition_for(MOB_ID))
        .await
        .expect("first bootstrap");
    let handle = first.handle();
    Box::pin(handle.spawn_spec(SpawnMemberSpec::from_wire(
        "lead".to_string(),
        "lead-1".to_string(),
        Some("You are lead-1.".into()),
        None,
        None,
    )))
    .await
    .expect("spawn lead-1");
    let before: Vec<String> = handle
        .list_members_including_retiring()
        .await
        .into_iter()
        .map(|member| member.agent_identity.as_str().to_string())
        .collect();
    assert!(
        before.contains(&"lead-1".to_string()),
        "the member must exist before the drop, else the test proves nothing: {before:?}"
    );

    // Retire before shutdown to release the member's comms endpoint. The comms
    // participant registry is process-global and `shutdown` alone does not
    // release member endpoints, so an in-process rebootstrap of the same mob
    // otherwise fails with a durable-generation-binding disagreement. A real
    // restart is a new process and never sees this.
    let identity = handle
        .list_members_including_retiring()
        .await
        .into_iter()
        .find(|member| member.agent_identity.as_str() == "lead-1")
        .map(|member| member.agent_identity)
        .expect("lead-1 identity");
    handle.retire(identity).await.expect("retire lead-1");
    handle.shutdown().await.expect("shutdown first runtime");
    drop(handle);
    drop(first);

    // The mob's own events are on disk, written by a runtime that no longer
    // exists. Under the defect this path held an in-memory storage and there
    // would be nothing here at all.
    let reopened = MobStorage::persistent(&mob_path).expect("reopen persistent storage");
    assert!(
        !reopened
            .is_event_log_empty()
            .await
            .expect("read event log emptiness"),
        "the mob event log did not survive the drop: persistent storage is not persisting"
    );
    drop(reopened);

    // And a fresh bootstrap against that same non-empty path resumes it. The
    // companion test proves this took the resume branch rather than creating a
    // second mob: a changed definition at this point is refused, which is only
    // reachable through the resume path.
    let second = boot(&mob_path, &session_root, base_definition_for(MOB_ID))
        .await
        .expect("second bootstrap must resume the existing storage");
    drop(second);
}

/// A changed definition must refuse loudly, naming the diverged field, rather
/// than boot the composition recorded at first create.
#[tokio::test]
async fn changed_definition_refuses_instead_of_booting_stale() {
    const MOB_ID: &str = "persistence-diverged";
    let temp = tempfile::tempdir().expect("temp dir");
    let mob_path = temp.path().join("mob.sqlite3");
    let session_root = temp.path().join("sessions");

    let first = boot(&mob_path, &session_root, base_definition_for(MOB_ID))
        .await
        .expect("first bootstrap");
    drop(first);

    let result = boot(&mob_path, &session_root, diverged_definition_for(MOB_ID)).await;
    match result {
        Err(MobRuntimeError::CompositionProvenance(MobCompositionProvenanceError::Divergent {
            fields,
            ..
        })) => {
            // The FIELD PATH, not the table it lives in. An operator who is
            // about to declare through this refusal has to be able to see what
            // moved, and "profiles diverged" on one changed pin is the message
            // that gets declared through unread.
            assert!(
                fields.iter().any(|field| field.starts_with("profiles.")),
                "the refusal must name the diverged field PATH so the operator can act: \
                 {fields:?}"
            );
            assert!(
                !fields.iter().any(|field| field == "profiles"),
                "reporting the bare table instead of the path is the regression this asserts \
                 against: {fields:?}"
            );
        }
        Err(other) => panic!("expected a composition divergence refusal, got: {other}"),
        Ok(_) => panic!(
            "a changed definition booted successfully: the resume silently used the \
             composition recorded at first create"
        ),
    }
}

/// Durable storage supplied through the PUBLIC bootstrap API with a non-empty
/// event log and no provenance declaration must fail closed, not resume
/// unverified.
///
/// This is the external-embedder case. `MobBootstrapSpec::new` accepts
/// arbitrary `MobStorage`, so the default provenance cannot be an ephemeral
/// claim: that would assert a fact the constructor cannot know and let a
/// caller's durable database resume with no composition verification. Scanning
/// MobKit's own callers does not close this - the crate is a library.
#[tokio::test]
async fn nonempty_storage_without_provenance_fails_closed() {
    const MOB_ID: &str = "persistence-unproven";
    let temp = tempfile::tempdir().expect("temp dir");
    let mob_path = temp.path().join("mob.sqlite3");
    let session_root = temp.path().join("sessions");

    let first = boot(&mob_path, &session_root, base_definition_for(MOB_ID))
        .await
        .expect("first bootstrap");
    drop(first);

    // Same path, opened directly, provenance never declared.
    let undeclared = MobStorage::persistent(&mob_path).expect("reopen persistent storage");
    let result = MobRuntime::bootstrap(
        MobBootstrapSpec::new(
            base_definition_for(MOB_ID),
            undeclared,
            session_service(&session_root).await,
        )
        .with_options(options()),
    )
    .await;
    match result {
        Err(MobRuntimeError::CompositionProvenance(
            MobCompositionProvenanceError::UnprovenStorage,
        )) => {}
        Err(other) => panic!("expected an unproven-storage refusal, got: {other}"),
        Ok(_) => panic!("undeclared persistent storage resumed with no verification"),
    }
}

/// An ephemeral launch keeps working unchanged: in-memory storage is empty at
/// bootstrap, so it takes the create path and needs no provenance.
#[tokio::test]
async fn declared_ephemeral_storage_still_boots() {
    const MOB_ID: &str = "persistence-ephemeral";
    let temp = tempfile::tempdir().expect("temp dir");
    let session_root = temp.path().join("sessions");
    let runtime = MobRuntime::bootstrap(
        MobBootstrapSpec::new(
            base_definition_for(MOB_ID),
            MobStorage::in_memory(),
            session_service(&session_root).await,
        )
        .with_mob_storage_provenance(MobStorageProvenance::declared_ephemeral())
        .with_options(options()),
    )
    .await
    .expect("ephemeral bootstrap must still work");
    assert!(
        runtime
            .handle()
            .list_members_including_retiring()
            .await
            .is_empty()
    );
}

/// A pre-seeded in-memory storage may resume when the caller declares what it
/// is. The refusal above targets silence, not in-process reuse: a storage built
/// in this same process has no composition outliving it to disagree with.
#[tokio::test]
async fn declared_ephemeral_storage_may_resume_in_process() {
    const MOB_ID: &str = "persistence-inproc-resume";
    let temp = tempfile::tempdir().expect("temp dir");
    let session_root = temp.path().join("sessions");

    let storage = MobStorage::in_memory();
    let first = MobRuntime::bootstrap(
        MobBootstrapSpec::new(
            base_definition_for(MOB_ID),
            storage.clone(),
            session_service(&session_root).await,
        )
        .with_declared_ephemeral_mob_storage()
        .with_options(options()),
    )
    .await
    .expect("first ephemeral bootstrap");
    first
        .handle()
        .shutdown()
        .await
        .expect("shutdown first runtime");
    drop(first);

    assert!(
        !storage
            .is_event_log_empty()
            .await
            .expect("read event log emptiness"),
        "the in-memory log must be non-empty for this test to exercise the resume path"
    );

    // Same storage, now non-empty, declared for what it is: permitted.
    let second = MobRuntime::bootstrap(
        MobBootstrapSpec::new(
            base_definition_for(MOB_ID),
            storage,
            session_service(&session_root).await,
        )
        .with_declared_ephemeral_mob_storage()
        .with_options(options()),
    )
    .await
    .expect("a declared in-process storage must be allowed to resume");
    drop(second);
}
