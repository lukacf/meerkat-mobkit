//! Composition for external-tool dispatchers.
//!
//! `build.external_tools` is a single slot, and assigning it wholesale is a
//! recurring defect class: a later installer silently discards whatever an
//! earlier one put there (the agent-memory recorder's `memory` tool has been
//! clobbered this way twice — by an example SessionHook, fixed in f8d11e57,
//! and by the rpc_gateway callback build path, HomeCore "Bug D"). Installers
//! MUST compose over the existing slot value instead of assigning.
//!
//! [`ComposedExternalTools`] is the canonical way to do that: the `primary`
//! dispatcher (the tools being installed) wins name collisions; anything it
//! does not advertise falls through to the `fallback` (whatever was already
//! in the slot). BOTH dispatch entry points forward — the
//! `ToolDispatchContext` carries per-turn authority witnesses (workgraph
//! attention projections), and relying on the trait's default
//! `dispatch_with_context` silently drops it, which was a verified
//! attention-scope bypass in an earlier wrapper.

use std::sync::Arc;

use meerkat_core::types::{ToolCallView, ToolDef};
use meerkat_core::{
    AgentToolDispatcher, ToolCatalogCapabilities, ToolCatalogEntry, ToolDispatchOutcome, ToolError,
};

/// Two external-tool dispatchers behind one slot: `primary` wins name
/// collisions, unknown calls fall through to `fallback`.
pub struct ComposedExternalTools {
    primary: Arc<dyn AgentToolDispatcher>,
    fallback: Arc<dyn AgentToolDispatcher>,
}

impl ComposedExternalTools {
    /// Compose `primary` over an optional pre-existing dispatcher. With no
    /// fallback this is the identity — callers can use it unconditionally on
    /// the slot they are about to fill.
    ///
    /// Name collisions are legal (primary wins) but never silent: a
    /// pre-installed tool that stops being dispatchable is the same defect
    /// class as the clobber this type exists to prevent, just scoped to one
    /// name — and it is exactly how a host tool named `memory` disables the
    /// agent-memory recorder while every layer still reports the tool
    /// present.
    pub fn over(
        primary: Arc<dyn AgentToolDispatcher>,
        fallback: Option<Arc<dyn AgentToolDispatcher>>,
    ) -> Arc<dyn AgentToolDispatcher> {
        match fallback {
            None => primary,
            Some(fallback) => {
                let composed = Self { primary, fallback };
                let shadowed = composed.shadowed_fallback_names();
                if !shadowed.is_empty() {
                    tracing::warn!(
                        shadowed = %shadowed.join(", "),
                        "external-tool composition shadows pre-installed tools: the \
                         installing dispatcher wins these names and the pre-installed \
                         implementations become unreachable. If 'memory' is listed, the \
                         agent-memory recorder is disabled for this member — rename the \
                         host tool or disable agent_memory.recorder_tool"
                    );
                }
                Arc::new(composed)
            }
        }
    }

    /// Fallback tool names the primary also advertises — present in the
    /// merged catalog, but their pre-installed implementations are
    /// unreachable through this composition.
    fn shadowed_fallback_names(&self) -> Vec<String> {
        let primary = self.primary.tools();
        self.fallback
            .tools()
            .iter()
            .filter(|tool| primary.iter().any(|existing| existing.name == tool.name))
            .map(|tool| tool.name.to_string())
            .collect()
    }

    fn primary_advertises(&self, name: &str) -> bool {
        self.primary
            .tools()
            .iter()
            .any(|tool| tool.name.as_ref() == name)
    }
}

#[async_trait::async_trait]
impl AgentToolDispatcher for ComposedExternalTools {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        let primary = self.primary.tools();
        let mut merged: Vec<Arc<ToolDef>> = primary.iter().cloned().collect();
        for tool in self.fallback.tools().iter() {
            if !primary.iter().any(|existing| existing.name == tool.name) {
                merged.push(Arc::clone(tool));
            }
        }
        merged.into()
    }

    /// The merged catalog is exact only when both halves are: the composition
    /// adds no names of its own, so exactness is the conjunction. Leaving this
    /// on the trait default made every member whose build composed host tools
    /// over a pre-installed dispatcher read as `NonExactCatalog` downstream.
    fn tool_catalog_capabilities(&self) -> ToolCatalogCapabilities {
        let primary = self.primary.tool_catalog_capabilities();
        let fallback = self.fallback.tool_catalog_capabilities();
        ToolCatalogCapabilities {
            exact_catalog: primary.exact_catalog && fallback.exact_catalog,
            may_require_catalog_control_plane: primary.may_require_catalog_control_plane
                || fallback.may_require_catalog_control_plane,
        }
    }

    /// Primary entries verbatim, then every fallback entry whose name the
    /// primary does not advertise - the same precedence `tools()` applies.
    fn tool_catalog(&self) -> Arc<[ToolCatalogEntry]> {
        let mut merged: Vec<ToolCatalogEntry> = self.primary.tool_catalog().to_vec();
        for entry in self.fallback.tool_catalog().iter() {
            if !self.primary_advertises(&entry.tool.name) {
                merged.push(entry.clone());
            }
        }
        merged.into()
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        if self.primary_advertises(call.name) {
            self.primary.dispatch(call).await
        } else {
            self.fallback.dispatch(call).await
        }
    }

    async fn dispatch_with_context(
        &self,
        call: ToolCallView<'_>,
        context: &meerkat_core::ToolDispatchContext,
    ) -> Result<ToolDispatchOutcome, ToolError> {
        if self.primary_advertises(call.name) {
            self.primary.dispatch_with_context(call, context).await
        } else {
            self.fallback.dispatch_with_context(call, context).await
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Probe {
        names: Vec<&'static str>,
        exact: bool,
        dispatched: AtomicUsize,
        contexted: AtomicUsize,
    }

    impl Probe {
        fn new(names: Vec<&'static str>) -> Arc<Self> {
            Self::with_exactness(names, true)
        }

        fn with_exactness(names: Vec<&'static str>, exact: bool) -> Arc<Self> {
            Arc::new(Self {
                names,
                exact,
                dispatched: AtomicUsize::new(0),
                contexted: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait::async_trait]
    impl AgentToolDispatcher for Probe {
        fn tools(&self) -> Arc<[Arc<ToolDef>]> {
            self.names
                .iter()
                .map(|name| {
                    Arc::new(ToolDef {
                        name: (*name).into(),
                        description: String::new(),
                        input_schema: json!({"type": "object"}),
                        provenance: None,
                    })
                })
                .collect::<Vec<_>>()
                .into()
        }

        fn tool_catalog_capabilities(&self) -> ToolCatalogCapabilities {
            ToolCatalogCapabilities {
                exact_catalog: self.exact,
                may_require_catalog_control_plane: false,
            }
        }

        async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
            self.dispatched.fetch_add(1, Ordering::SeqCst);
            Err(ToolError::not_found(call.name))
        }

        async fn dispatch_with_context(
            &self,
            call: ToolCallView<'_>,
            _context: &meerkat_core::ToolDispatchContext,
        ) -> Result<ToolDispatchOutcome, ToolError> {
            self.contexted.fetch_add(1, Ordering::SeqCst);
            Err(ToolError::not_found(call.name))
        }
    }

    fn call<'a>(name: &'a str, args: &'a serde_json::value::RawValue) -> ToolCallView<'a> {
        ToolCallView {
            id: "call-1",
            name,
            args,
        }
    }

    #[tokio::test]
    async fn tools_merge_with_primary_winning_name_collisions() {
        let primary = Probe::new(vec!["shared", "python_tool"]);
        let fallback = Probe::new(vec!["shared", "memory"]);
        let composed = ComposedExternalTools::over(primary, Some(fallback));
        let names: Vec<String> = composed
            .tools()
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();
        assert_eq!(names, vec!["shared", "python_tool", "memory"]);
    }

    #[tokio::test]
    async fn catalog_exactness_is_the_conjunction_and_the_catalog_follows_precedence() {
        let composed = ComposedExternalTools::over(
            Probe::new(vec!["shared", "python_tool"]),
            Some(Probe::new(vec!["shared", "memory"])),
        );
        assert!(composed.tool_catalog_capabilities().exact_catalog);
        let names: Vec<String> = composed
            .tool_catalog()
            .iter()
            .map(|entry| entry.tool.name.to_string())
            .collect();
        assert_eq!(names, vec!["shared", "python_tool", "memory"]);

        for (primary_exact, fallback_exact) in [(false, true), (true, false), (false, false)] {
            let composed = ComposedExternalTools::over(
                Probe::with_exactness(vec!["python_tool"], primary_exact),
                Some(Probe::with_exactness(vec!["memory"], fallback_exact)),
            );
            assert!(
                !composed.tool_catalog_capabilities().exact_catalog,
                "one non-exact half must make the composition non-exact"
            );
        }
    }

    #[tokio::test]
    async fn dispatch_routes_by_advertised_name_on_both_entry_points() {
        let primary = Probe::new(vec!["python_tool"]);
        let fallback = Probe::new(vec!["memory"]);
        let composed = ComposedExternalTools::over(
            Arc::clone(&primary) as _,
            Some(Arc::clone(&fallback) as _),
        );
        let args = serde_json::value::RawValue::from_string("{}".to_string()).expect("raw");

        let _ = composed.dispatch(call("python_tool", &args)).await;
        assert_eq!(primary.dispatched.load(Ordering::SeqCst), 1);
        assert_eq!(fallback.dispatched.load(Ordering::SeqCst), 0);

        let _ = composed.dispatch(call("memory", &args)).await;
        assert_eq!(fallback.dispatched.load(Ordering::SeqCst), 1);

        // The context entry point must forward AS the context entry point —
        // the trait default would drop the ToolDispatchContext (verified
        // witness-loss class).
        let _ = composed
            .dispatch_with_context(
                call("memory", &args),
                &meerkat_core::ToolDispatchContext::default(),
            )
            .await;
        assert_eq!(fallback.contexted.load(Ordering::SeqCst), 1);
        assert_eq!(
            fallback.dispatched.load(Ordering::SeqCst),
            1,
            "context calls must not degrade to plain dispatch"
        );
    }

    #[tokio::test]
    async fn shadowed_fallback_names_name_exactly_the_unreachable_tools() {
        // "shared" collides (primary wins, fallback copy unreachable);
        // "memory" survives untouched. The composition-time warn is driven by
        // this list, so pinning it pins the observability contract: a host
        // tool shadowing the agent-memory recorder is loud, never silent.
        let composed = ComposedExternalTools {
            primary: Probe::new(vec!["shared", "python_tool"]),
            fallback: Probe::new(vec!["shared", "memory"]),
        };
        assert_eq!(composed.shadowed_fallback_names(), vec!["shared"]);

        let recorder_shadowed = ComposedExternalTools {
            primary: Probe::new(vec!["memory"]),
            fallback: Probe::new(vec!["memory"]),
        };
        assert_eq!(recorder_shadowed.shadowed_fallback_names(), vec!["memory"]);

        let disjoint = ComposedExternalTools {
            primary: Probe::new(vec!["python_tool"]),
            fallback: Probe::new(vec!["memory"]),
        };
        assert!(disjoint.shadowed_fallback_names().is_empty());
    }

    #[tokio::test]
    async fn no_fallback_is_the_identity() {
        let primary = Probe::new(vec!["only"]);
        let composed = ComposedExternalTools::over(Arc::clone(&primary) as _, None);
        assert!(Arc::ptr_eq(
            &(Arc::clone(&primary) as Arc<dyn AgentToolDispatcher>),
            &composed
        ));
    }
}
