//! Every MobKit composition that installs agent mob tools gives `fork_off`
//! and `council` the DETACHED route.
//!
//! meerkat #1190 defaults detached completion delivery to available only when
//! the `MobMcpState` has a runtime adapter; without one, `fork_off` and
//! `council` block for the child's whole run and report `no_runtime_adapter`.
//! A MobKit host must never land there: HomeCore's calendar `fork_off` would
//! hold its turn for the whole child run.
//!
//! `MobMcpState::new` takes its adapter from the session service it is given
//! (`MobSessionService::acquire_runtime_adapter`), captured when MobKit
//! installs the agent mob tools. Each test pins two facts per composition:
//! the route is not blocked, and it uses the machine hosting the sessions (the
//! one `MobRuntime` acquires from the session service with the spec's explicit
//! runtime adapter), so a detached completion is admitted by the runtime
//! that owns the owner's session.

#![allow(clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use meerkat::{Config, FactoryAgentBuilder};
use meerkat_session::PersistentSessionService;

use crate::mob_handle_runtime::{CapabilityFlags, MobBootstrapSpec};

fn definition(mob_id: &str) -> meerkat_mob::MobDefinition {
    meerkat_mob::MobDefinition::from_toml(&format!(
        r#"
[mob]
id = "{mob_id}"

[profiles.general]
model = "gpt-5.5"

[profiles.general.tools]
comms = true
"#
    ))
    .expect("mob definition")
}

/// The route is detached and runs on the machine the runtime uses.
fn assert_detached_route_on_runtime_machine(spec: &MobBootstrapSpec, composition: &str) {
    let state = spec
        .agent_mob_mcp_state
        .as_ref()
        .unwrap_or_else(|| panic!("{composition}: agent mob tools must be installed"));
    assert_eq!(
        state.detached_delivery_blocked_because(),
        None,
        "{composition}: fork_off/council must deliver detached, not block"
    );
    let route = state
        .session_service()
        .acquire_runtime_adapter(None)
        .unwrap_or_else(|error| {
            panic!("{composition}: acquire the detached route runtime: {error}")
        })
        .unwrap_or_else(|| panic!("{composition}: the session service exposes no runtime"));
    // The canonical machine `MobRuntime` hands to `MobBuilder`, acquired
    // from the session service with the spec's explicit adapter.
    let runtime = spec
        .session_service
        .acquire_runtime_adapter(spec.runtime_adapter.clone())
        .unwrap_or_else(|error| panic!("{composition}: acquire the runtime machine: {error}"))
        .unwrap_or_else(|| panic!("{composition}: the runtime has no machine"));
    assert!(
        Arc::ptr_eq(&route, &runtime),
        "{composition}: detached delivery must use the machine that hosts the sessions"
    );
}

/// `rpc_gateway --persistent` (HomeCore) and `mobkit_gateway` with console
/// voice: a concrete persistent session service, an explicit persistent
/// machine wired with `with_session_runtime_adapter`, then
/// `with_agent_mob_tools`, in that order.
#[tokio::test]
async fn gateway_persistent_composition_delivers_detached() {
    let temp = tempfile::tempdir().expect("temp dir");
    let state = temp.path().join("state");
    std::fs::create_dir_all(&state).expect("state dir");
    let session_store: Arc<dyn meerkat::SessionStore> = Arc::new(
        meerkat_store::SqliteSessionStore::open(state.join("session-store.sqlite3"))
            .expect("session store"),
    );
    let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
        meerkat_runtime::store::SqliteRuntimeStore::new(state.join("runtime-store.sqlite3"))
            .expect("runtime store"),
    );
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(meerkat_store::MemoryBlobStore::new());
    let factory = meerkat::AgentFactory::new(&state).comms(true);
    let mut builder = FactoryAgentBuilder::new(factory, Config::default());
    builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
        session_store.clone(),
    )));
    builder.default_blob_store = Some(blob_store.clone());
    let mob_tools_slot = Arc::clone(&builder.default_mob_tools);
    let adapter = Arc::new(
        meerkat_runtime::MeerkatMachine::persistent(
            Arc::clone(&runtime_store),
            Arc::clone(&blob_store),
        )
        .expect("acquire the detached delivery fixture runtime machine"),
    );
    let service = Arc::new(PersistentSessionService::new(
        builder,
        16,
        session_store,
        runtime_store,
        blob_store,
    ));
    let mut spec = MobBootstrapSpec::new(
        definition("route-gateway-persistent"),
        meerkat_mob::MobStorage::in_memory(),
        service,
    )
    .with_session_runtime_adapter(adapter.clone())
    .expect("acquire the fixture session runtime owner")
    .with_agent_mob_tools(mob_tools_slot)
    .expect("install the detached delivery fixture's agent mob tools");
    spec.runtime_adapter = Some(adapter);
    assert_detached_route_on_runtime_machine(&spec, "gateway persistent composition");
}

/// `rpc_gateway`'s and `mobkit_gateway`'s process-local modes: a persistent
/// session service over in-memory stores with an explicit machine.
#[tokio::test]
async fn gateway_ephemeral_session_composition_delivers_detached() {
    let temp = tempfile::tempdir().expect("temp dir");
    let session_store: Arc<dyn meerkat::SessionStore> = Arc::new(meerkat::MemoryStore::new());
    let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
        Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(meerkat_store::MemoryBlobStore::new());
    let factory = meerkat::AgentFactory::new(temp.path()).comms(true);
    let mut builder = FactoryAgentBuilder::new(factory, Config::default());
    builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
        session_store.clone(),
    )));
    builder.default_blob_store = Some(blob_store.clone());
    let mob_tools_slot = Arc::clone(&builder.default_mob_tools);
    let adapter = Arc::new(
        meerkat_runtime::MeerkatMachine::persistent(
            Arc::clone(&runtime_store),
            Arc::clone(&blob_store),
        )
        .expect("acquire the detached delivery fixture runtime machine"),
    );
    let service = Arc::new(PersistentSessionService::new(
        builder,
        16,
        session_store,
        runtime_store,
        blob_store,
    ));
    let mut spec = MobBootstrapSpec::new(
        definition("route-gateway-ephemeral"),
        meerkat_mob::MobStorage::in_memory(),
        service,
    )
    .with_session_runtime_adapter(adapter.clone())
    .expect("acquire the fixture session runtime owner")
    .with_agent_mob_tools(mob_tools_slot)
    .expect("install the detached delivery fixture's agent mob tools");
    spec.runtime_adapter = Some(adapter);
    assert_detached_route_on_runtime_machine(&spec, "gateway ephemeral-session composition");
}

/// `MobBootstrapSpec::persistent`, the persistent `UnifiedRuntimeBuilder`
/// path (identity-first library hosts).
#[tokio::test]
async fn library_persistent_constructor_delivers_detached() {
    let temp = tempfile::tempdir().expect("temp dir");
    let session_store: Arc<dyn meerkat::SessionStore> = Arc::new(
        meerkat_store::SqliteSessionStore::open(temp.path().join("session-store.sqlite3"))
            .expect("session store"),
    );
    let spec = MobBootstrapSpec::persistent(
        definition("route-library-persistent"),
        meerkat_mob::MobStorage::in_memory(),
        temp.path().join("state"),
        16,
        session_store,
    )
    .expect("persistent spec");
    assert_detached_route_on_runtime_machine(&spec, "library persistent constructor");
}

/// The runtime-backed ephemeral constructor, the ephemeral
/// `UnifiedRuntimeBuilder` path.
#[tokio::test]
async fn library_runtime_backed_ephemeral_constructor_delivers_detached() {
    let temp = tempfile::tempdir().expect("temp dir");
    let spec = MobBootstrapSpec::ephemeral_runtime_backed_inner(
        definition("route-library-runtime-backed"),
        meerkat_mob::MobStorage::in_memory(),
        temp.path().to_path_buf(),
        16,
        None,
        "test session store",
        None,
        None,
        None,
        None,
        CapabilityFlags::default(),
        None,
        None,
    )
    .expect("build the runtime-backed ephemeral detached delivery spec");
    assert_detached_route_on_runtime_machine(&spec, "library runtime-backed ephemeral constructor");
}

/// `MobBootstrapSpec::ephemeral`, the plain ephemeral library constructor.
#[tokio::test]
async fn library_ephemeral_constructor_delivers_detached() {
    let temp = tempfile::tempdir().expect("temp dir");
    let spec = MobBootstrapSpec::ephemeral(
        definition("route-library-ephemeral"),
        meerkat_mob::MobStorage::in_memory(),
        temp.path().to_path_buf(),
        16,
        None,
    )
    .expect("build the ephemeral detached delivery spec");
    assert_detached_route_on_runtime_machine(&spec, "library ephemeral constructor");
}
