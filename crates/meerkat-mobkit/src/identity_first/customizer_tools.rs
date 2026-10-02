//! Customizer tools that follow an identity across meerkat-side rebuilds (#563).
//!
//! The tools a host returns from `AgentCustomizer::customize_build` used to
//! reach a member only as the per-spawn `external_tools` overlay of the one
//! spawn that ran the customizer. meerkat-mob keeps that overlay in memory
//! and never persists it, so a restart restore, an adopted occupant
//! (`MemberAlreadyExists`), a respawn or a delivery-time repair rebuilt the
//! member without them: the host's handlers stayed registered, but the live
//! agent no longer advertised the tools.
//!
//! Instead, every identity member carries ONE stable, dynamic dispatcher
//! ([`IdentityCustomizerTools`]) for its whole life. Each successful
//! `customize_build` publishes its result into that dispatcher, swapping the
//! tool list and the dispatcher that serves it (for gateway hosts, the
//! handler scope) together. meerkat composes per-spawn overlays through a
//! dynamic composite that re-reads `tools()` on every turn, so:
//!
//! - an occupant that is adopted instead of respawned already holds the
//!   identity's dispatcher, and publishing re-attaches the current tools
//!   live, without a respawn;
//! - [`CustomizerToolsSpawnCustomizer`], installed on the mob whenever an
//!   agent customizer exists, attaches the same dispatcher to every
//!   meerkat-side build of a registered identity member (restart restore,
//!   explicit resume, respawn, delivery-time repair). It is synchronous and
//!   makes no host call: it attaches the stable dispatcher rather than
//!   re-running the host.
//!
//! Members that are not roster identities (helpers, forks, flow-provisioned
//! or raw-spawned members under their own ids) are never registered and get
//! no customizer tools: `customize_build` is a contract over roster
//! identities (it takes a `DurableAgentSpec`). A raw spawn that targets a
//! roster identity's member id does get that identity's current tools.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use meerkat_core::types::{ToolCallView, ToolDef};
use meerkat_core::{
    AgentToolDispatcher, ToolCatalogCapabilities, ToolCatalogEntry, ToolDispatchContext,
    ToolDispatchOutcome, ToolError,
};
use meerkat_mob::{
    AgentIdentity as MobAgentIdentity, MobError, SpawnCustomizationContext, SpawnMemberCustomizer,
    SpawnMemberSpec,
};

use super::types::AgentIdentity;

/// One `customize_build` result: the dispatcher serving the published tools
/// (`None` when the build declared none) and the publication generation.
#[derive(Clone)]
struct Published {
    generation: u64,
    dispatcher: Option<Arc<dyn AgentToolDispatcher>>,
}

/// The stable, dynamic customizer-tool dispatcher of one identity member.
///
/// Every read takes one snapshot of the current publication, so the tools a
/// turn sees and the dispatcher that serves a call come from the same
/// `customize_build` result. A call to a tool the current publication does
/// not advertise is refused typed (`ToolError::NotFound`), never routed to a
/// dispatcher (or handler scope) from an earlier publication.
pub struct IdentityCustomizerTools {
    member_id: MobAgentIdentity,
    current: RwLock<Published>,
}

impl IdentityCustomizerTools {
    fn new(member_id: MobAgentIdentity) -> Self {
        Self {
            member_id,
            current: RwLock::new(Published {
                generation: 0,
                dispatcher: None,
            }),
        }
    }

    /// The roster member id this dispatcher belongs to.
    pub fn member_id(&self) -> &MobAgentIdentity {
        &self.member_id
    }

    fn snapshot(&self) -> Published {
        self.current
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Publish one `customize_build` result. `None` clears the tools (the
    /// build declared none). Returns the new publication generation.
    pub fn publish(&self, dispatcher: Option<Arc<dyn AgentToolDispatcher>>) -> u64 {
        let mut current = self
            .current
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        current.generation = current.generation.saturating_add(1);
        current.dispatcher = dispatcher;
        current.generation
    }

    /// How many times a result has been published (0: none yet).
    pub fn generation(&self) -> u64 {
        self.snapshot().generation
    }

    fn owner_for(published: &Published, tool_name: &str) -> Option<Arc<dyn AgentToolDispatcher>> {
        let dispatcher = published.dispatcher.as_ref()?;
        dispatcher
            .tools()
            .iter()
            .any(|tool| tool.name.as_ref() == tool_name)
            .then(|| Arc::clone(dispatcher))
    }

    fn not_advertised(&self, name: &str) -> ToolError {
        tracing::debug!(
            member_id = %self.member_id,
            tool = name,
            "customizer tool call refused: the current customize_build publication does not \
             advertise it"
        );
        ToolError::NotFound {
            name: name.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl AgentToolDispatcher for IdentityCustomizerTools {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        match self.snapshot().dispatcher {
            Some(dispatcher) => dispatcher.tools(),
            None => Arc::from([]),
        }
    }

    fn tool_catalog_capabilities(&self) -> ToolCatalogCapabilities {
        match self.snapshot().dispatcher {
            Some(dispatcher) => dispatcher.tool_catalog_capabilities(),
            // An empty publication is its own exact (empty) registry.
            None => ToolCatalogCapabilities {
                exact_catalog: true,
                may_require_catalog_control_plane: false,
            },
        }
    }

    fn tool_catalog(&self) -> Arc<[ToolCatalogEntry]> {
        match self.snapshot().dispatcher {
            Some(dispatcher) => dispatcher.tool_catalog(),
            None => Arc::from([]),
        }
    }

    fn tool_mutation_class(&self, tool_name: &str) -> meerkat_core::ToolMutationClass {
        match Self::owner_for(&self.snapshot(), tool_name) {
            Some(owner) => owner.tool_mutation_class(tool_name),
            None => meerkat_core::ToolMutationClass::Unknown,
        }
    }

    fn live_bridge_effect_kind(&self, tool_name: &str) -> meerkat_core::LiveBridgeEffectKind {
        match Self::owner_for(&self.snapshot(), tool_name) {
            Some(owner) => owner.live_bridge_effect_kind(tool_name),
            None => meerkat_core::LiveBridgeEffectKind::ExternalIo,
        }
    }

    /// A mutable authority: every publication advances the epoch, including
    /// an identical-metadata replacement (a new handler scope behind the same
    /// tool names), so a binding fingerprinted before it is never mistaken
    /// for the current one.
    fn execution_binding_epoch(&self, tool_name: &str) -> u64 {
        let published = self.snapshot();
        let inner = Self::owner_for(&published, tool_name)
            .map_or(0, |owner| owner.execution_binding_epoch(tool_name));
        (published.generation << 32) | (inner & 0xFFFF_FFFF)
    }

    fn pending_catalog_sources(&self) -> Arc<[String]> {
        match self.snapshot().dispatcher {
            Some(dispatcher) => dispatcher.pending_catalog_sources(),
            None => Arc::from([]),
        }
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        match Self::owner_for(&self.snapshot(), call.name) {
            Some(owner) => owner.dispatch(call).await,
            None => Err(self.not_advertised(call.name)),
        }
    }

    async fn dispatch_with_context(
        &self,
        call: ToolCallView<'_>,
        context: &ToolDispatchContext,
    ) -> Result<ToolDispatchOutcome, ToolError> {
        match Self::owner_for(&self.snapshot(), call.name) {
            Some(owner) => owner.dispatch_with_context(call, context).await,
            None => Err(self.not_advertised(call.name)),
        }
    }

    async fn dispatch_resolved_with_context(
        &self,
        call: ToolCallView<'_>,
        context: &ToolDispatchContext,
        plan: &meerkat_core::ResolvedToolExecutionPlan,
    ) -> Result<ToolDispatchOutcome, ToolError> {
        match Self::owner_for(&self.snapshot(), call.name) {
            Some(owner) => {
                owner
                    .dispatch_resolved_with_context(call, context, plan)
                    .await
            }
            None => Err(self.not_advertised(call.name)),
        }
    }
}

/// The customizer-tool dispatchers of every registered identity member,
/// keyed by roster member id (the deterministic `mob_member_id(identity)`).
#[derive(Default)]
pub struct CustomizerToolRegistry {
    entries: RwLock<BTreeMap<MobAgentIdentity, Arc<IdentityCustomizerTools>>>,
}

impl CustomizerToolRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn member_id(identity: &AgentIdentity) -> MobAgentIdentity {
        crate::member_comms_id::mob_member_id(identity.as_str())
    }

    /// Register `identity` (idempotent) and return its stable dispatcher.
    pub fn ensure(&self, identity: &AgentIdentity) -> Arc<IdentityCustomizerTools> {
        let member_id = Self::member_id(identity);
        if let Some(existing) = self
            .entries
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&member_id)
        {
            return Arc::clone(existing);
        }
        let mut entries = self
            .entries
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(
            entries
                .entry(member_id.clone())
                .or_insert_with(|| Arc::new(IdentityCustomizerTools::new(member_id))),
        )
    }

    /// Register a roster member id directly (idempotent).
    pub fn ensure_member(&self, member_id: &MobAgentIdentity) -> Arc<IdentityCustomizerTools> {
        if let Some(existing) = self.for_member(member_id) {
            return existing;
        }
        let mut entries = self
            .entries
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(
            entries
                .entry(member_id.clone())
                .or_insert_with(|| Arc::new(IdentityCustomizerTools::new(member_id.clone()))),
        )
    }

    /// The registered dispatcher for a roster member id, if any.
    pub fn for_member(&self, member_id: &MobAgentIdentity) -> Option<Arc<IdentityCustomizerTools>> {
        self.entries
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(member_id)
            .cloned()
    }

    /// Publish one `customize_build` result for `identity` (registering it
    /// if needed) and return the identity's stable dispatcher, which is what
    /// every spawn of that identity attaches.
    pub fn publish(
        &self,
        identity: &AgentIdentity,
        dispatcher: Option<Arc<dyn AgentToolDispatcher>>,
    ) -> Arc<IdentityCustomizerTools> {
        let entry = self.ensure(identity);
        entry.publish(dispatcher);
        entry
    }
}

/// Run the host customizer for each roster identity and publish its tools
/// into `registry`, BEFORE meerkat builds any member (#563). Returns the
/// identities whose `customize_build` failed, with the typed reason; they stay
/// registered with nothing published until their materialization publishes.
/// Each failure is logged per identity.
pub async fn prepublish(
    registry: &CustomizerToolRegistry,
    roster: &[super::types::DurableAgentSpec],
    customizer: &dyn super::contracts::AgentCustomizer,
    runtime_services: super::types::AgentRuntimeServices,
    active_peers: &[AgentIdentity],
    managed_edges: &[super::types::ManagedPeerEdge],
) -> BTreeMap<AgentIdentity, super::types::CustomizerToolsPending> {
    let mut pending = BTreeMap::new();
    for spec in roster {
        let build_context = super::types::AgentBuildContext {
            identity: spec.identity.clone(),
            active_peers: active_peers.to_vec(),
            managed_edges: managed_edges.to_vec(),
            runtime_services: runtime_services.clone(),
        };
        let mut draft = super::types::AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: spec.additional_instructions.clone(),
            labels: spec.labels.clone(),
            app_context: spec.context.clone(),
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
            provider_params: None,
            compaction_curator: Default::default(),
        };
        match customizer
            .customize_build(&build_context, spec, &mut draft)
            .await
        {
            Ok(()) => {
                registry.publish(&spec.identity, draft.local_external_tools.dispatcher());
            }
            Err(error) => {
                let reason = format!("pre-activation customize_build failed: {error}");
                tracing::warn!(
                    identity = %spec.identity,
                    %reason,
                    "restored member's customizer tools are not published; it advertises none \
                     until its materialization publishes them"
                );
                registry.ensure(&spec.identity);
                pending.insert(
                    spec.identity.clone(),
                    super::types::CustomizerToolsPending { reason },
                );
            }
        }
    }
    pending
}

fn same_dispatcher(a: &Arc<dyn AgentToolDispatcher>, b: &Arc<IdentityCustomizerTools>) -> bool {
    std::ptr::eq(Arc::as_ptr(a).cast::<()>(), Arc::as_ptr(b).cast::<()>())
}

/// Attaches a registered identity member's customizer-tool dispatcher to
/// every meerkat-side build of it: restart restore and explicit resume
/// (`SpawnSource::Resume`), respawn, delivery-time repair, and raw spawns
/// that target the identity's member id. Members that are not registered
/// identities are left untouched.
pub struct CustomizerToolsSpawnCustomizer {
    registry: Arc<CustomizerToolRegistry>,
}

impl CustomizerToolsSpawnCustomizer {
    pub fn new(registry: Arc<CustomizerToolRegistry>) -> Self {
        Self { registry }
    }
}

impl SpawnMemberCustomizer for CustomizerToolsSpawnCustomizer {
    fn customize_spawn(
        &self,
        _ctx: &SpawnCustomizationContext,
        spec: &mut SpawnMemberSpec,
    ) -> Result<(), MobError> {
        self.apply(spec);
        Ok(())
    }
}

impl CustomizerToolsSpawnCustomizer {
    /// The customizer body, factored off the trait so tests can drive it:
    /// `SpawnCustomizationContext` is `#[non_exhaustive]` and only
    /// meerkat-mob constructs it. The attachment does not depend on the spawn
    /// source: every build of a registered identity member gets it.
    fn apply(&self, spec: &mut SpawnMemberSpec) {
        // A member MobKit built for an identity carries the
        // runtime-authoritative `agent_identity` label (raw member creation
        // may not supply it), and the roster entry keeps it. That marks a
        // restored identity member even when meerkat restores the mob before
        // MobKit has seen the roster, so it is registered here, at its first
        // build, and receives the identity's tools when they are published.
        let entry = match self.registry.for_member(&spec.identity) {
            Some(entry) => entry,
            None if spec
                .labels
                .as_ref()
                .and_then(crate::member_comms_id::durable_identity_label)
                .is_some() =>
            {
                self.registry.ensure_member(&spec.identity)
            }
            None => return,
        };
        spec.external_tools = Some(match spec.external_tools.take() {
            None => entry,
            // MobKit's own identity spawn already attached it.
            Some(existing) if same_dispatcher(&existing, &entry) => existing,
            // A raw spawn of the identity's member id that brought its own
            // overlay: the customizer tools win name collisions, the rest
            // stays reachable.
            Some(existing) => {
                crate::tool_compose::ComposedExternalTools::over(entry, Some(existing))
            }
        });
    }
}

/// Runs several spawn customizers in order. meerkat-mob has a single
/// `SpawnMemberCustomizer` slot; composing keeps a later installer from
/// silently replacing an earlier one.
pub struct ComposedSpawnMemberCustomizer {
    customizers: Vec<Arc<dyn SpawnMemberCustomizer>>,
}

impl ComposedSpawnMemberCustomizer {
    /// `existing` (if any) runs first, then `added`.
    pub fn over(
        existing: Option<Arc<dyn SpawnMemberCustomizer>>,
        added: Arc<dyn SpawnMemberCustomizer>,
    ) -> Arc<dyn SpawnMemberCustomizer> {
        match existing {
            None => added,
            Some(existing) => Arc::new(Self {
                customizers: vec![existing, added],
            }),
        }
    }
}

impl SpawnMemberCustomizer for ComposedSpawnMemberCustomizer {
    fn customize_spawn(
        &self,
        ctx: &SpawnCustomizationContext,
        spec: &mut SpawnMemberSpec,
    ) -> Result<(), MobError> {
        for customizer in &self.customizers {
            customizer.customize_spawn(ctx, spec)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "customizer_tools_tests.rs"]
mod tests;
