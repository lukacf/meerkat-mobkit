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
        Some(Arc::new(ScopePinnedWorkGraphTools::new(service))),
    );
}

/// Pin every `workgraph_*` tool call to the service's realm + namespace.
///
/// The upstream tool schema exposes `realm_id`/`namespace` as free parameters
/// and models DO fill them with plausible inventions (live-fire: an agent
/// passed `realm_id = "default", namespace = "<mob id>"` — the exact inverse
/// of the service scope), which strands the work outside every operator
/// surface: the RPC snapshot reads the pinned scope, and CAS actions against
/// the invented scope NotFound. In mobkit one runtime is one realm (the mob
/// definition id) and one namespace: mechanism owns scope, agents don't
/// wander realms. Pinning uniformly on reads AND writes keeps the agent's
/// view self-consistent whatever values it invents.
///
/// Both dispatch entry points are overridden: the trait-default
/// `dispatch_with_context` funnels through `dispatch`, which would DROP the
/// `ToolDispatchContext` carrying the attention-projection witness
/// (`WORKGRAPH_ATTENTION_DISPATCH_CONTEXT_KEY`) — silently bypassing every
/// attention-scoped enforcement in the inner surface and permanently denying
/// `workgraph_policy_escalate`/`workgraph_attention_reassign` to
/// legitimately-delegated agents.
struct ScopePinnedWorkGraphTools {
    inner: Arc<WorkGraphToolSurface>,
    realm_id: String,
    namespace: String,
}

impl ScopePinnedWorkGraphTools {
    fn new(service: &WorkGraphService) -> Self {
        Self {
            inner: Arc::new(WorkGraphToolSurface::new(service.clone())),
            realm_id: service.default_realm_id().to_string(),
            namespace: service.default_namespace().as_str().to_string(),
        }
    }

    /// Re-encode a `workgraph_*` call's arguments with the service scope
    /// pinned; `None` means the call is not a workgraph tool and must be
    /// forwarded unchanged.
    fn pin_args(
        &self,
        call: &meerkat_core::types::ToolCallView<'_>,
    ) -> Result<Option<Box<serde_json::value::RawValue>>, meerkat_core::ToolError> {
        if !call.name.starts_with("workgraph_") {
            return Ok(None);
        }
        let mut args: serde_json::Value =
            serde_json::from_str(call.args.get()).map_err(|error| {
                meerkat_core::ToolError::invalid_arguments(
                    call.name,
                    format!("invalid workgraph tool-call arguments JSON: {error}"),
                )
            })?;
        if let Some(object) = args.as_object_mut() {
            object.insert(
                "realm_id".to_string(),
                serde_json::Value::String(self.realm_id.clone()),
            );
            object.insert(
                "namespace".to_string(),
                serde_json::Value::String(self.namespace.clone()),
            );
            // A pinned namespace makes cross-namespace reads meaningless.
            object.remove("all_namespaces");
        }
        serde_json::value::RawValue::from_string(args.to_string())
            .map(Some)
            .map_err(|error| {
                meerkat_core::ToolError::invalid_arguments(
                    call.name,
                    format!("failed to encode pinned workgraph arguments: {error}"),
                )
            })
    }
}

#[async_trait::async_trait]
impl meerkat_core::AgentToolDispatcher for ScopePinnedWorkGraphTools {
    fn tools(&self) -> Arc<[Arc<meerkat_core::types::ToolDef>]> {
        self.inner.tools()
    }

    async fn dispatch(
        &self,
        call: meerkat_core::types::ToolCallView<'_>,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        let Some(pinned) = self.pin_args(&call)? else {
            return self.inner.dispatch(call).await;
        };
        self.inner
            .dispatch(meerkat_core::types::ToolCallView {
                id: call.id,
                name: call.name,
                args: &pinned,
            })
            .await
    }

    async fn dispatch_with_context(
        &self,
        call: meerkat_core::types::ToolCallView<'_>,
        context: &meerkat_core::agent::ToolDispatchContext,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        let Some(pinned) = self.pin_args(&call)? else {
            return self.inner.dispatch_with_context(call, context).await;
        };
        self.inner
            .dispatch_with_context(
                meerkat_core::types::ToolCallView {
                    id: call.id,
                    name: call.name,
                    args: &pinned,
                },
                context,
            )
            .await
    }
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

    /// Live-fire regression (2026-07-08): the upstream tool schema exposes
    /// realm_id/namespace and a real model invented `realm_id = "default",
    /// namespace = "<mob id>"` — the inverse of the service scope — stranding
    /// its items outside every operator surface. The dispatcher must pin both
    /// on every workgraph_* call, reads and writes alike.
    #[tokio::test]
    async fn tool_calls_with_invented_scope_are_pinned_to_the_service_scope() {
        use meerkat_core::AgentToolDispatcher;
        use serde_json::value::RawValue;

        let service = ephemeral_workgraph_service("mob-realm");
        let tools = ScopePinnedWorkGraphTools::new(&service);

        let create_args = RawValue::from_string(
            serde_json::json!({
                "title": "invented scope",
                "realm_id": "default",
                "namespace": "mob-realm"
            })
            .to_string(),
        )
        .expect("args");
        tools
            .dispatch(meerkat_core::types::ToolCallView {
                id: "call-1",
                name: "workgraph_create",
                args: &create_args,
            })
            .await
            .expect("create dispatch");

        // The item must be visible in the SERVICE scope (what the RPC and
        // console read), not the invented one.
        let items = service
            .list(meerkat::WorkItemFilter::default())
            .await
            .expect("list");
        assert_eq!(items.len(), 1, "item must land in the pinned scope");
        assert_eq!(items[0].realm_id, "mob-realm");
        assert_eq!(items[0].namespace.as_str(), "default");

        // Reads with invented scope must see the same world (self-consistent
        // agent view): snapshot with the inverted scope still returns the item.
        let snap_args = RawValue::from_string(
            serde_json::json!({
                "realm_id": "default",
                "namespace": "mob-realm",
                "all_namespaces": false
            })
            .to_string(),
        )
        .expect("snap args");
        let outcome = tools
            .dispatch(meerkat_core::types::ToolCallView {
                id: "call-2",
                name: "workgraph_snapshot",
                args: &snap_args,
            })
            .await
            .expect("snapshot dispatch");
        let rendered = format!("{outcome:?}");
        assert!(
            rendered.contains("invented scope"),
            "pinned snapshot must see the pinned item: {rendered}"
        );
    }

    /// Regression (adversarial finding F1): the trait-default
    /// `dispatch_with_context` funnels through `dispatch` and drops the
    /// dispatch context carrying the attention-projection witness. The wrapper
    /// must forward the context so the inner surface's attention-scoped
    /// enforcement (item-id pinning et al.) still fires on member turns.
    #[tokio::test]
    async fn dispatch_with_context_forwards_the_attention_witness_to_the_surface() {
        use std::collections::BTreeMap;

        use meerkat::{
            AttentionProjectionRequest, GoalAttentionTarget, GoalCreateRequest,
            WORKGRAPH_ATTENTION_DISPATCH_CONTEXT_KEY, WorkOwnerKey,
        };
        use meerkat_core::AgentToolDispatcher;
        use serde_json::value::RawValue;

        let service = ephemeral_workgraph_service("mob-realm");
        let tools = ScopePinnedWorkGraphTools::new(&service);

        let goal = service
            .create_goal(GoalCreateRequest {
                realm_id: None,
                namespace: None,
                title: "attention-scoped goal".to_string(),
                description: None,
                target: GoalAttentionTarget::Owner {
                    owner_key: WorkOwnerKey::principal("operator@example.test").expect("owner key"),
                },
                mode: Default::default(),
                completion_policy: Default::default(),
                delegated_authority: Default::default(),
                projection_policy: Default::default(),
            })
            .await
            .expect("create goal");
        let projection = service
            .attention_projection(AttentionProjectionRequest {
                binding_id: goal.attention.binding_id.clone(),
                realm_id: None,
                namespace: None,
            })
            .await
            .expect("attention projection")
            .projection;
        let decoy = service
            .create(CreateWorkItemRequest {
                title: "outside the attention scope".to_string(),
                ..Default::default()
            })
            .await
            .expect("decoy item");
        let context = meerkat_core::agent::ToolDispatchContext::default().with_turn_metadata(
            BTreeMap::from([(
                WORKGRAPH_ATTENTION_DISPATCH_CONTEXT_KEY.to_string(),
                serde_json::to_value(&projection).expect("projection json"),
            )]),
        );

        // A mutation against a DIFFERENT item must hit the upstream
        // attention-scope rejection — proof the witness reached the surface.
        let foreign_args = RawValue::from_string(
            serde_json::json!({
                "id": decoy.id,
                "expected_revision": decoy.revision,
                "evidence": { "kind": "note", "id": "n-1", "summary": "smuggled" },
            })
            .to_string(),
        )
        .expect("foreign args");
        let error = tools
            .dispatch_with_context(
                meerkat_core::types::ToolCallView {
                    id: "call-scope-1",
                    name: "workgraph_add_evidence",
                    args: &foreign_args,
                },
                &context,
            )
            .await
            .expect_err("attention scope must pin the item id");
        assert!(
            error.to_string().contains("scoped to attention work item"),
            "upstream attention-scope rejection must surface: {error}"
        );

        // The same mutation against the bound item passes the scope checks.
        let scoped_args = RawValue::from_string(
            serde_json::json!({
                "id": goal.item.id,
                "expected_revision": goal.item.revision,
                "evidence": { "kind": "note", "id": "n-2", "summary": "in scope" },
            })
            .to_string(),
        )
        .expect("scoped args");
        tools
            .dispatch_with_context(
                meerkat_core::types::ToolCallView {
                    id: "call-scope-2",
                    name: "workgraph_add_evidence",
                    args: &scoped_args,
                },
                &context,
            )
            .await
            .expect("attention-scoped call on the bound item succeeds");
    }
}
