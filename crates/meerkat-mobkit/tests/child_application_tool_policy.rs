//! The host application tool policy for child mobs: mobs a governed member
//! creates with `mob_create`, and the implicit mob its `delegate` helpers run
//! in. Once a consequence-policy registry is installed, meerkat refuses
//! `delegate` with a typed tool error until a child policy is configured; with
//! one configured (a provider binding or an explicit Unmanaged), delegate
//! runs. Same-mob `fork_off` and councils are never governed by it. The spec
//! constructors install the agent mob tools before the host sets the
//! registry, so these also pin that bootstrap forwards the final
//! configuration.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use meerkat_core::service::MobToolAuthorityContext;
use meerkat_core::types::{SessionId, ToolCallView};
use meerkat_core::{
    AgentToolDispatcher, ApplicationToolPolicyBinding, PolicyDigest, PolicyEvaluationProvenance,
    PolicyEvaluationSupervisorConfig, PolicyId, PolicyProviderGeneration, PolicyProviderId,
    PolicyRevision, ToolConsequenceFailure, ToolConsequenceNarrowingPolicy,
    ToolConsequencePolicyRegistry, ToolConsequencePolicySnapshot, ToolConsequenceRequest,
    ToolConsequenceVerdict, ToolDispatchOutcome, ToolError,
};
use meerkat_mob_mcp::MobMcpState;
use meerkat_mobkit::{MobBootstrapOptions, MobBootstrapSpec, MobRuntime};
use serde_json::{Value, json};

/// A host policy that allows every tool.
struct AllowAll;

impl ToolConsequencePolicySnapshot for AllowAll {
    fn provenance(&self) -> PolicyEvaluationProvenance {
        PolicyEvaluationProvenance {
            revision: PolicyRevision(1),
            digest: PolicyDigest::from_canonical_bytes(b"allow-all"),
        }
    }

    fn evaluate(&self, _request: &ToolConsequenceRequest) -> ToolConsequenceVerdict {
        ToolConsequenceVerdict::Allow
    }
}

struct HostProvider(PolicyProviderId);

impl ToolConsequenceNarrowingPolicy for HostProvider {
    fn provider_id(&self) -> &PolicyProviderId {
        &self.0
    }

    fn generation(&self) -> PolicyProviderGeneration {
        PolicyProviderGeneration(1)
    }

    fn snapshot(
        &self,
        _policy_id: &PolicyId,
    ) -> Result<Arc<dyn ToolConsequencePolicySnapshot>, ToolConsequenceFailure> {
        Ok(Arc::new(AllowAll))
    }
}

fn registry() -> Arc<ToolConsequencePolicyRegistry> {
    Arc::new(
        ToolConsequencePolicyRegistry::new(
            vec![Arc::new(HostProvider(
                PolicyProviderId::new("workspace").unwrap(),
            ))],
            PolicyEvaluationSupervisorConfig::default(),
            None,
        )
        .expect("registry"),
    )
}

fn workspace_policy() -> ApplicationToolPolicyBinding {
    ApplicationToolPolicyBinding::Provider {
        provider_id: PolicyProviderId::new("workspace").unwrap(),
        policy_id: PolicyId::new("helper-tools").unwrap(),
    }
}

static NEXT_MOB: AtomicUsize = AtomicUsize::new(0);

fn definition() -> meerkat_mob::MobDefinition {
    meerkat_mob::MobDefinition::from_toml(&format!(
        r#"
[mob]
id = "child-policy-{}-{}"

[profiles.worker]
model = "gpt-5.5"

[profiles.worker.tools]
comms = true
"#,
        std::process::id(),
        NEXT_MOB.fetch_add(1, Ordering::SeqCst)
    ))
    .expect("mob definition")
}

fn options() -> MobBootstrapOptions {
    MobBootstrapOptions {
        allow_ephemeral_sessions: true,
        notify_orchestrator_on_resume: true,
        default_llm_client: Some(Arc::new(meerkat_client::TestClient::default())),
    }
}

async fn boot(
    dir: &tempfile::TempDir,
    configure: impl FnOnce(MobBootstrapSpec) -> MobBootstrapSpec,
) -> MobRuntime {
    // The ephemeral constructor installs the agent mob tools before
    // `configure` runs, as `persistent()` does.
    let spec = MobBootstrapSpec::ephemeral(
        definition(),
        meerkat_mob::MobStorage::in_memory(),
        dir.path().to_path_buf(),
        4,
        None,
    )
    .expect("build the child application tool policy ephemeral spec")
    .with_options(options());
    MobRuntime::bootstrap(configure(spec))
        .await
        .expect("bootstrap the mob runtime")
}

/// Durable sessions and mob storage: `fork_off` needs the session service's
/// durable transcript fork authority.
async fn boot_persistent(
    dir: &tempfile::TempDir,
    configure: impl FnOnce(MobBootstrapSpec) -> MobBootstrapSpec,
) -> MobRuntime {
    let session_store = Arc::new(
        meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.sqlite3"))
            .expect("open session store"),
    );
    let (storage, provenance) = meerkat_mobkit::mob_composition_manifest::persistent_mob_storage(
        dir.path().join("mob.sqlite3"),
    )
    .expect("open persistent mob storage");
    let spec = MobBootstrapSpec::persistent(
        definition(),
        storage,
        dir.path().to_path_buf(),
        4,
        session_store,
    )
    .expect("compose persistent stores")
    .with_mob_storage_provenance(provenance)
    .with_options(options());
    MobRuntime::bootstrap(configure(spec))
        .await
        .expect("bootstrap the mob runtime")
}

/// Create scope only, as a member that may delegate holds.
fn creator_authority() -> MobToolAuthorityContext {
    meerkat_runtime::mob_operator_authority::create_only_mob_operator_authority()
        .expect("generated authority")
}

/// The agent mob tools of `session`, bound to an operation registry the way
/// the agent loop binds every dispatcher.
fn agent_surface(
    state: Arc<MobMcpState>,
    session: SessionId,
    authority: MobToolAuthorityContext,
) -> Arc<dyn AgentToolDispatcher> {
    let surface: Arc<dyn AgentToolDispatcher> =
        Arc::new(meerkat_mob_mcp::AgentMobToolSurface::new(
            state,
            None,
            authority,
            "gpt-5.5".to_string(),
            session.clone(),
            None,
            None,
            None,
        ));
    surface
        .bind_ops_lifecycle(
            Arc::new(meerkat_runtime::ops_lifecycle::RuntimeOpsLifecycleRegistry::new()),
            session,
        )
        .expect("bind ops lifecycle")
        .into_dispatcher()
}

async fn dispatch(
    surface: &Arc<dyn AgentToolDispatcher>,
    name: &'static str,
    args: Value,
) -> Result<ToolDispatchOutcome, ToolError> {
    let raw = serde_json::value::RawValue::from_string(args.to_string()).unwrap();
    tokio::time::timeout(
        std::time::Duration::from_mins(1),
        surface.dispatch(ToolCallView {
            id: "surface-call",
            name,
            args: &raw,
        }),
    )
    .await
    .unwrap_or_else(|_| panic!("{name} returns within the failure bound"))
}

fn agent_mob_tools(runtime: &MobRuntime) -> Arc<MobMcpState> {
    runtime
        .agent_mob_mcp_state()
        .expect("the agent mob tools are installed")
}

/// A member's `delegate` call through the agent mob tools of `runtime`.
async fn delegate(runtime: &MobRuntime) -> Result<ToolDispatchOutcome, ToolError> {
    let surface = agent_surface(
        agent_mob_tools(runtime),
        SessionId::new(),
        creator_authority(),
    );
    dispatch(
        &surface,
        "delegate",
        json!({
            "task": "summarize the project notes",
            "member_id": "helper",
            "result_label": "helper_result",
            "max_text_bytes": 4096,
            "tooling": {
                "mode": "profile",
                "source": {
                    "type": "inline",
                    "model": "gpt-5.5",
                    "tools": { "comms": true }
                }
            }
        }),
    )
    .await
}

fn assert_delegate_completed(outcome: &ToolDispatchOutcome) {
    let result: Value = serde_json::from_str(&outcome.result.text_content()).expect("json result");
    assert_eq!(result["agent_identity"], "helper", "{result}");
    assert_eq!(result["bounded_result"]["status"], "completed", "{result}");
}

#[tokio::test(flavor = "multi_thread")]
async fn delegate_is_refused_until_a_child_policy_is_configured() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = boot(&dir, |spec| {
        spec.with_tool_consequence_policy_registry(registry())
    })
    .await;
    let error = delegate(&runtime)
        .await
        .expect_err("a managed host without a child policy refuses delegate");
    let ToolError::PolicyDenied { denial } = &error else {
        panic!("a typed policy denial, got {error:?}");
    };
    assert_eq!(denial.code, "child_tool_policy_required", "{error:?}");
    // The message says why and names every fix.
    for expected in [
        "this host runs a tool-policy registry and no child application tool policy is configured",
        "`child_application_tool_policy` init parameter",
        "with_child_application_tool_policy",
        r#"{"kind":"unmanaged"}"#,
    ] {
        assert!(
            denial.message.contains(expected),
            "{expected}: {}",
            denial.message
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn delegate_runs_under_a_configured_provider_child_policy() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = boot(&dir, |spec| {
        spec.with_tool_consequence_policy_registry(registry())
            .with_child_application_tool_policy(workspace_policy())
    })
    .await;
    let outcome = delegate(&runtime)
        .await
        .expect("delegate runs with a child policy configured");
    assert_delegate_completed(&outcome);
}

#[tokio::test(flavor = "multi_thread")]
async fn delegate_runs_once_the_host_explicitly_chooses_unmanaged_children() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = boot(&dir, |spec| {
        spec.with_tool_consequence_policy_registry(registry())
            .with_child_application_tool_policy(ApplicationToolPolicyBinding::Unmanaged)
    })
    .await;
    let outcome = delegate(&runtime)
        .await
        .expect("delegate runs with an explicit unmanaged child policy");
    assert_delegate_completed(&outcome);
}

/// A host without application tool policies keeps today's behaviour.
#[tokio::test(flavor = "multi_thread")]
async fn delegate_runs_on_a_host_without_tool_policies() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = boot(&dir, |spec| spec).await;
    let outcome = delegate(&runtime)
        .await
        .expect("delegate runs on an unmanaged host");
    assert_delegate_completed(&outcome);
}

/// Only child mobs are governed: on a managed host without a child policy,
/// a member's same-mob `fork_off` and a council over host members still run.
#[tokio::test(flavor = "multi_thread")]
async fn fork_off_and_councils_are_not_governed_by_the_child_policy() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = boot_persistent(&dir, |spec| {
        spec.with_tool_consequence_policy_registry(registry())
    })
    .await;
    let handle = runtime.handle();
    let mob_id = handle.mob_id().to_string();
    let mut sessions = Vec::new();
    for member in ["alice", "bob"] {
        handle
            .ensure_member(meerkat_mob::SpawnMemberSpec::new(
                meerkat_mob::ProfileName::from("worker"),
                meerkat_mob::AgentIdentity::from(member),
            ))
            .await
            .expect("seat member");
        sessions.push(
            handle
                .resolve_bridge_session_id(&meerkat_mob::AgentIdentity::from(member))
                .await
                .expect("member session"),
        );
    }

    let forker_authority = meerkat_runtime::mob_operator_authority::grant_spawn_profile_in_mob(
        &meerkat_runtime::mob_operator_authority::set_create_authority(&creator_authority(), false)
            .expect("no create scope"),
        &mob_id,
        "worker",
    )
    .expect("spawnable worker profile");
    let forker = agent_surface(
        agent_mob_tools(&runtime),
        sessions[0].clone(),
        forker_authority,
    );
    dispatch(
        &forker,
        "fork_off",
        json!({"member_id": "alice-fork", "task": "check the status board"}),
    )
    .await
    .expect("same-mob fork_off is untouched");

    let convener_authority =
        meerkat_runtime::mob_operator_authority::grant_manage_mob(&creator_authority(), &mob_id)
            .expect("manage scope");
    let convener = agent_surface(
        agent_mob_tools(&runtime),
        sessions[1].clone(),
        convener_authority,
    );
    dispatch(
        &convener,
        "council",
        json!({
            "topic": "Should the heating schedule change?",
            "participants": [
                {"mob_id": mob_id, "member_id": "alice", "role": "analyst"},
                {"mob_id": mob_id, "member_id": "bob", "role": "critic"},
            ],
            "max_rounds": 1,
            "timeout_seconds": 60,
        }),
    )
    .await
    .expect("councils are untouched");
}
