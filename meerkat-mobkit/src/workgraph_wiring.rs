//! Wire meerkat's WorkGraph (goals, work items, attention bindings and
//! apply-time attention overlays) into the gateways.
//!
//! A mob profile's `tools.workgraph = true` maps (in `meerkat-mob`) to
//! `override_workgraph = Enable`, but a build with the category enabled and
//! no dispatcher in the agent factory's `default_workgraph_tools` slot is a
//! hard build error on non-wasm surfaces. [`attach_workgraph_tools`] fills
//! that slot from a durable SQLite store; [`attach_workgraph_tools_ephemeral`]
//! is the memory-store variant for non-persistent launches. Both return the
//! realm-scoped [`WorkGraphService`] so the same instance can back the
//! `mobkit/workgraph/*` RPC surface, mob-runtime attention overlays
//! (`MobBuilder::with_workgraph_service` via `MobBootstrapSpec`), and the
//! schedule host's apply-time overlay injection.
//!
//! The realm id is the mob definition id — mirroring the schedule host's
//! `owner_id` choice. Meerkat hosts refuse to invent a realm, so callers must
//! always pass a real one.

use std::path::Path;
use std::sync::Arc;

use meerkat::{
    FactoryAgentBuilder, MemoryWorkGraphStore, SqliteWorkGraphStore, WorkGraphService,
    WorkGraphStore, WorkGraphToolSurface, WorkNamespace,
};

/// File name for the durable workgraph store, kept beside the runtime DB so a
/// gateway and a library-mode runtime pointed at the same dir share state.
/// Matches meerkat's `PersistenceBundle` convention.
pub const WORKGRAPH_STORE_FILE: &str = "workgraph.sqlite3";

/// Build a realm-scoped [`WorkGraphService`] over a durable SQLite store
/// under `state_dir`. Returns `None` (with a warning) if the store cannot be
/// opened — the gateway then boots without workgraph rather than failing
/// closed, matching the schedule-tools posture.
#[must_use]
pub fn open_workgraph_service(state_dir: &Path, realm_id: &str) -> Option<WorkGraphService> {
    let path = state_dir.join(WORKGRAPH_STORE_FILE);
    let store = match SqliteWorkGraphStore::open(&path) {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "failed to open workgraph store; workgraph disabled for this gateway",
            );
            return None;
        }
    };
    Some(scoped_workgraph_service(Arc::new(store), realm_id))
}

/// Build a realm-scoped [`WorkGraphService`] over an in-memory store, for
/// non-persistent launches. Tools stay profile-gated, so nothing changes for
/// members that do not opt in.
#[must_use]
pub fn ephemeral_workgraph_service(realm_id: &str) -> WorkGraphService {
    scoped_workgraph_service(Arc::new(MemoryWorkGraphStore::new()), realm_id)
}

fn scoped_workgraph_service(store: Arc<dyn WorkGraphStore>, realm_id: &str) -> WorkGraphService {
    WorkGraphService::with_scope(store, realm_id, WorkNamespace::default())
}

/// Fill `builder`'s default workgraph-tools slot with the full
/// [`WorkGraphToolSurface`] over `service`. After this, any member whose
/// profile resolves workgraph tools on (`tools.workgraph = true`) is built
/// with the `workgraph_*` surface. Call on the `FactoryAgentBuilder` BEFORE
/// it is consumed into a session service (the slot is interior-mutable, but
/// attaching up front matches the schedule-tools wiring).
pub fn install_workgraph_tools(builder: &FactoryAgentBuilder, service: &WorkGraphService) {
    meerkat::surface::set_default_workgraph_tools(
        builder,
        Some(Arc::new(WorkGraphToolSurface::new(service.clone()))),
    );
}

/// Open the durable store under `state_dir` and attach its tool surface to
/// `builder`. Returns the service, or `None` (boot-without) on open failure.
#[must_use]
pub fn attach_workgraph_tools(
    builder: &FactoryAgentBuilder,
    state_dir: &Path,
    realm_id: &str,
) -> Option<WorkGraphService> {
    let service = open_workgraph_service(state_dir, realm_id)?;
    install_workgraph_tools(builder, &service);
    Some(service)
}

/// Memory-store variant of [`attach_workgraph_tools`] for ephemeral launches.
#[must_use]
pub fn attach_workgraph_tools_ephemeral(
    builder: &FactoryAgentBuilder,
    realm_id: &str,
) -> WorkGraphService {
    let service = ephemeral_workgraph_service(realm_id);
    install_workgraph_tools(builder, &service);
    service
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use meerkat::{AgentFactory, Config, CreateWorkItemRequest, WorkGraphStoreKind};

    fn test_builder(dir: &Path) -> FactoryAgentBuilder {
        FactoryAgentBuilder::new(AgentFactory::new(dir), Config::default())
    }

    fn slot_is_filled(builder: &FactoryAgentBuilder) -> bool {
        builder
            .default_workgraph_tools
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    #[tokio::test]
    async fn attach_workgraph_tools_populates_the_builder_slot_and_opens_the_store() {
        let dir = tempfile::tempdir().expect("temp dir");
        let builder = test_builder(dir.path());
        assert!(!slot_is_filled(&builder));

        let service = attach_workgraph_tools(&builder, dir.path(), "wiring-realm")
            .expect("sqlite workgraph store should open in a writable dir");

        assert!(
            slot_is_filled(&builder),
            "attach must fill the default_workgraph_tools slot"
        );
        assert_eq!(service.default_realm_id(), "wiring-realm");
        assert_eq!(service.store().kind(), WorkGraphStoreKind::Sqlite);
        assert!(
            dir.path().join(WORKGRAPH_STORE_FILE).exists(),
            "store file must be created beside the runtime DB"
        );
    }

    #[tokio::test]
    async fn sqlite_store_survives_reopen() {
        let dir = tempfile::tempdir().expect("temp dir");
        let service =
            open_workgraph_service(dir.path(), "reopen-realm").expect("open workgraph store");
        let item = service
            .create(CreateWorkItemRequest {
                title: "persisted item".to_string(),
                ..Default::default()
            })
            .await
            .expect("create item");

        let reopened =
            open_workgraph_service(dir.path(), "reopen-realm").expect("reopen workgraph store");
        let loaded = reopened
            .get(None, None, item.id.clone())
            .await
            .expect("item must survive reopen");
        assert_eq!(loaded.title, "persisted item");
        assert_eq!(loaded.realm_id, "reopen-realm");
    }

    #[tokio::test]
    async fn open_failure_boots_without_workgraph() {
        let dir = tempfile::tempdir().expect("temp dir");
        // A directory occupying the store path makes SQLite open fail.
        std::fs::create_dir_all(dir.path().join(WORKGRAPH_STORE_FILE)).expect("blocker dir");
        assert!(
            open_workgraph_service(dir.path(), "broken-realm").is_none(),
            "open failure must degrade to None, not panic"
        );
    }

    #[tokio::test]
    async fn ephemeral_variant_fills_slot_with_memory_store() {
        let dir = tempfile::tempdir().expect("temp dir");
        let builder = test_builder(dir.path());

        let service = attach_workgraph_tools_ephemeral(&builder, "ephemeral-realm");

        assert!(slot_is_filled(&builder));
        assert_eq!(service.default_realm_id(), "ephemeral-realm");
        assert_eq!(service.store().kind(), WorkGraphStoreKind::Memory);
        assert!(
            !dir.path().join(WORKGRAPH_STORE_FILE).exists(),
            "ephemeral variant must not create a store file"
        );
    }
}
