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
use meerkat_core::{AgentToolDispatcher, ToolDispatchOutcome, ToolError};

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
    pub fn over(
        primary: Arc<dyn AgentToolDispatcher>,
        fallback: Option<Arc<dyn AgentToolDispatcher>>,
    ) -> Arc<dyn AgentToolDispatcher> {
        match fallback {
            None => primary,
            Some(fallback) => Arc::new(Self { primary, fallback }),
        }
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
        dispatched: AtomicUsize,
        contexted: AtomicUsize,
    }

    impl Probe {
        fn new(names: Vec<&'static str>) -> Arc<Self> {
            Arc::new(Self {
                names,
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
    async fn no_fallback_is_the_identity() {
        let primary = Probe::new(vec!["only"]);
        let composed = ComposedExternalTools::over(Arc::clone(&primary) as _, None);
        assert!(Arc::ptr_eq(
            &(Arc::clone(&primary) as Arc<dyn AgentToolDispatcher>),
            &composed
        ));
    }
}
