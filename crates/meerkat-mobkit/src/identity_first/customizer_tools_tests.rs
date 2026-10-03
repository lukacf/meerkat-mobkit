#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use meerkat_core::types::{ToolCallView, ToolDef};
use meerkat_core::{AgentToolDispatcher, ToolDispatchOutcome, ToolError};
use meerkat_mob::SpawnMemberSpec;
use serde_json::json;

use super::{CustomizerToolRegistry, CustomizerToolsSpawnCustomizer};
use crate::identity_first::types::AgentIdentity;

/// A fixed-list dispatcher standing in for one `customize_build` result: its
/// `scope` plays the gateway handler scope the calls are routed to.
struct Scope {
    scope: &'static str,
    names: Vec<&'static str>,
    dispatched: AtomicUsize,
}

impl Scope {
    fn new(scope: &'static str, names: Vec<&'static str>) -> Arc<Self> {
        Arc::new(Self {
            scope,
            names,
            dispatched: AtomicUsize::new(0),
        })
    }
}

#[async_trait::async_trait]
impl AgentToolDispatcher for Scope {
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
        Ok(meerkat_core::ToolResult {
            tool_use_id: call.id.to_string(),
            content: vec![meerkat_core::types::ContentBlock::Text {
                text: format!("{}:{}", self.scope, call.name),
            }],
            is_error: false,
        }
        .into())
    }
}

fn call<'a>(name: &'a str, args: &'a serde_json::value::RawValue) -> ToolCallView<'a> {
    ToolCallView {
        id: "call-1",
        name,
        args,
    }
}

fn names(dispatcher: &dyn AgentToolDispatcher) -> Vec<String> {
    dispatcher
        .tools()
        .iter()
        .map(|tool| tool.name.to_string())
        .collect()
}

fn served_by(outcome: &ToolDispatchOutcome) -> String {
    match outcome.result.content.first() {
        Some(meerkat_core::types::ContentBlock::Text { text }) => text.clone(),
        other => panic!("unexpected content {other:?}"),
    }
}

fn identity(value: &str) -> AgentIdentity {
    AgentIdentity::parse(value).expect("identity")
}

#[tokio::test]
async fn a_publication_swaps_the_tool_list_and_the_serving_scope_together() {
    let registry = CustomizerToolRegistry::new();
    let id = identity("domain:school");
    let first = Scope::new("customize-1", vec!["lookup", "notify"]);
    let entry = registry.publish(&id, Some(first.clone()));
    assert_eq!(names(entry.as_ref()), ["lookup", "notify"]);
    let args = serde_json::value::RawValue::from_string("{}".to_string()).unwrap();
    let outcome = entry.dispatch(call("lookup", &args)).await.unwrap();
    assert_eq!(served_by(&outcome), "customize-1:lookup");
    let first_epoch = entry.execution_binding_epoch("lookup");

    // A second customize_build (a reset or reconcile) replaces both at once.
    let second = Scope::new("customize-2", vec!["lookup", "schedule"]);
    let same = registry.publish(&id, Some(second.clone()));
    assert!(
        Arc::ptr_eq(&entry, &same),
        "one stable dispatcher per identity"
    );
    assert_eq!(names(entry.as_ref()), ["lookup", "schedule"]);
    let outcome = entry.dispatch(call("lookup", &args)).await.unwrap();
    assert_eq!(
        served_by(&outcome),
        "customize-2:lookup",
        "never the stale scope"
    );
    assert!(
        entry.execution_binding_epoch("lookup") > first_epoch,
        "an identical-name replacement still advances the binding epoch"
    );
    // A tool only the old set advertised is refused typed, not routed.
    let refused = entry.dispatch(call("notify", &args)).await.unwrap_err();
    assert!(matches!(refused, ToolError::NotFound { ref name } if name == "notify"));
    assert_eq!(first.dispatched.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_empty_publication_advertises_nothing_and_refuses_calls() {
    let registry = CustomizerToolRegistry::new();
    let id = identity("domain:calendar");
    let entry = registry.publish(&id, Some(Scope::new("customize-1", vec!["lookup"])));
    registry.publish(&id, None);
    assert!(entry.tools().is_empty());
    assert!(entry.tool_catalog().is_empty());
    assert!(entry.tool_catalog_capabilities().exact_catalog);
    let args = serde_json::value::RawValue::from_string("{}".to_string()).unwrap();
    assert!(matches!(
        entry.dispatch(call("lookup", &args)).await,
        Err(ToolError::NotFound { .. })
    ));
}

#[test]
fn the_registry_is_keyed_by_roster_member_id_and_ensure_is_idempotent() {
    let registry = CustomizerToolRegistry::new();
    let id = identity("identity:luka");
    let entry = registry.ensure(&id);
    assert!(Arc::ptr_eq(&entry, &registry.ensure(&id)));
    let member_id = crate::member_comms_id::mob_member_id(id.as_str());
    assert_eq!(entry.member_id(), &member_id);
    assert!(Arc::ptr_eq(
        &entry,
        &registry.for_member(&member_id).unwrap()
    ));
    assert_eq!(entry.generation(), 0, "registered before any publication");
    assert!(
        registry
            .for_member(&crate::member_comms_id::mob_member_id("helper-7"))
            .is_none()
    );
}

#[tokio::test]
async fn a_rebuild_of_a_registered_member_reattaches_its_dispatcher() {
    let registry = CustomizerToolRegistry::new();
    let id = identity("domain:school");
    let member_id = crate::member_comms_id::mob_member_id(id.as_str());
    // Registered before the mob is lifted, with nothing published yet: the
    // restart restore attaches the empty dispatcher...
    registry.ensure(&id);
    let customizer = CustomizerToolsSpawnCustomizer::new(registry.clone());
    // The attachment does not depend on the spawn source (restore, respawn,
    // raw spawn): every build of a registered member gets the dispatcher.
    for _build in 0..3 {
        let mut spec = SpawnMemberSpec::new("worker", member_id.clone());
        customizer.apply(&mut spec);
        let attached = spec.external_tools.expect("dispatcher attached");
        assert!(attached.tools().is_empty());
        // ...and the live member it built sees the tools the moment the
        // identity's materialization publishes them, without a respawn.
        registry.publish(&id, Some(Scope::new("customize-1", vec!["lookup"])));
        assert_eq!(names(attached.as_ref()), ["lookup"]);
        registry.publish(&id, None);
    }
}

#[test]
fn unregistered_members_are_left_untouched() {
    let registry = CustomizerToolRegistry::new();
    registry.ensure(&identity("domain:school"));
    let customizer = CustomizerToolsSpawnCustomizer::new(registry);
    // Helpers, forks and flow-provisioned members spawn under their own ids.
    for member in ["helper-1", "fork-of-school-1", "flow-target-1"] {
        let mut spec = SpawnMemberSpec::new("worker", member);
        customizer.apply(&mut spec);
        assert!(spec.external_tools.is_none());
    }
}

#[test]
fn mobkits_own_attachment_is_not_wrapped_twice_and_a_foreign_overlay_is_composed() {
    let registry = CustomizerToolRegistry::new();
    let id = identity("domain:school");
    let member_id = crate::member_comms_id::mob_member_id(id.as_str());
    let entry = registry.publish(&id, Some(Scope::new("customize-1", vec!["lookup"])));
    let customizer = CustomizerToolsSpawnCustomizer::new(registry);

    let mut own = SpawnMemberSpec::new("worker", member_id.clone());
    own.external_tools = Some(entry.clone());
    customizer.apply(&mut own);
    let attached = own.external_tools.unwrap();
    assert!(std::ptr::eq(
        Arc::as_ptr(&attached).cast::<()>(),
        Arc::as_ptr(&entry).cast::<()>()
    ));

    // A raw spawn of the roster member id that brought its own overlay.
    let mut raw = SpawnMemberSpec::new("worker", member_id);
    raw.external_tools = Some(Scope::new("raw", vec!["raw_tool"]));
    customizer.apply(&mut raw);
    let mut composed = names(raw.external_tools.unwrap().as_ref());
    composed.sort();
    assert_eq!(composed, ["lookup", "raw_tool"]);
}

#[test]
fn a_restored_identity_member_is_registered_from_its_identity_label() {
    // meerkat can restore a persisted Running mob while MobKit is still
    // building it, before any roster callback: the restored spec carries the
    // runtime-authoritative `agent_identity` label MobKit set at spawn.
    let registry = CustomizerToolRegistry::new();
    let id = identity("domain:school");
    let member_id = crate::member_comms_id::mob_member_id(id.as_str());
    let customizer = CustomizerToolsSpawnCustomizer::new(registry.clone());
    let mut restored = SpawnMemberSpec::new("worker", member_id.clone());
    restored.labels = Some(
        [("agent_identity".to_string(), id.as_str().to_string())]
            .into_iter()
            .collect(),
    );
    customizer.apply(&mut restored);
    let attached = restored.external_tools.expect("registered at first build");
    // The later materialization publishes into the same dispatcher.
    registry.publish(&id, Some(Scope::new("customize-1", vec!["lookup"])));
    assert_eq!(names(attached.as_ref()), ["lookup"]);
    assert!(registry.for_member(&member_id).is_some());
}
