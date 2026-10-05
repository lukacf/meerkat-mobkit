//! Native delegate -> fork_off acceptance through the registered probe bundle.
//! Delegate retirement cascades to its fork. MobKit currently gives implicit
//! child mobs in-memory journals, so this test proves the live access route
//! and durable document/ACL storage without claiming a persistent child mob.

use super::*;
use meerkat_core::{Message, SessionId};
use meerkat_mob::{MemberCreationProvenance, MobHandle, MobId};
use tokio::sync::{Semaphore, mpsc};

const OWNER: &str = "CROSS_MOB_OWNER";
const HELPER: &str = "CROSS_MOB_HELPER";
const FORK: &str = "CROSS_MOB_FORK";
const REOPEN_OWNER: &str = "CROSS_MOB_REOPEN_OWNER";
const ROOT_OWNER: &str = "PERSISTENT_ROOT_OWNER";
const ROOT_FORK: &str = "PERSISTENT_ROOT_FORK";
const ROOT_BARRIER: &str = "PERSISTENT_ROOT_BARRIER";
const ROOT_REVOKED: &str = "PERSISTENT_ROOT_REVOKED";
const ROOT_REOPEN_DENIED: &str = "PERSISTENT_ROOT_REOPEN_DENIED";
const ROOT_REOPEN_EDITOR: &str = "PERSISTENT_ROOT_REOPEN_EDITOR";
const ROOT_REOPEN_REVOKED: &str = "PERSISTENT_ROOT_REOPEN_REVOKED";

#[derive(Default)]
struct Evidence {
    document: Option<Document>,
    actors: BTreeMap<String, (SessionId, Principal)>,
}

pub(super) struct Scenario {
    evidence: tokio::sync::Mutex<Evidence>,
    events: mpsc::UnboundedSender<String>,
    grant: Semaphore,
    revoke: Semaphore,
    finish: Semaphore,
}

impl Scenario {
    fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<String>) {
        let (events, receiver) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                evidence: Default::default(),
                events,
                grant: Semaphore::new(0),
                revoke: Semaphore::new(0),
                finish: Semaphore::new(0),
            }),
            receiver,
        )
    }

    pub(super) async fn dispatch(
        &self,
        bundle: &ToolBundleContext,
        caller: &HostAccessContext,
        call: ToolCallView<'_>,
        execution: &ToolDispatchContext,
    ) -> std::result::Result<ToolDispatchOutcome, ToolError> {
        let args: serde_json::Value = call.parse_args().unwrap();
        let action = args["action"].as_str().unwrap();
        let actor = args["actor"].as_str().unwrap();
        let session = execution.origin_session_id().unwrap().clone();
        {
            let mut evidence = self.evidence.lock().await;
            let actual = (session, caller.principal().clone());
            if let Some(previous) = evidence.actors.get(actor) {
                assert_eq!(previous, &actual, "native actor identity must stay exact");
            } else {
                evidence.actors.insert(actor.to_owned(), actual);
            }
        }
        if action == "create" {
            assert_eq!(actor, "owner");
            let receipt = bundle
                .documents
                .mutate(
                    caller,
                    &request("cross-mob-create"),
                    Mutation::Create(NewDocument {
                        content: content(),
                        owner: None,
                        access: DocumentAccess::default(),
                    }),
                )
                .await
                .unwrap()
                .receipt;
            let document = bundle
                .documents
                .get(caller, &receipt.document_id)
                .await
                .unwrap();
            assert_eq!(document.owner, Owner::Agent(caller.principal().clone()));
            self.evidence.lock().await.document = Some(document);
        } else {
            let document = self.evidence.lock().await.document.clone().unwrap();
            if action == "owner-persisted" {
                let visible = bundle.documents.get(caller, &document.id).await.unwrap();
                assert_eq!(visible.id, document.id);
                assert_eq!(visible.revision, document.revision);
                assert_eq!(visible.owner, Owner::Agent(caller.principal().clone()));
                assert_eq!(visible.content.payload, content().payload);
                assert!(visible.access.grants.is_empty());
            } else if action.starts_with("editor") {
                let visible = bundle.documents.get(caller, &document.id).await.unwrap();
                assert_eq!(visible.id, document.id);
                assert_eq!(visible.content.payload, content().payload);
                assert_eq!(visible.owner, document.owner);
                bundle
                    .documents
                    .mutate(
                        caller,
                        &request(&format!("{actor}-{action}-edit")),
                        Mutation::Replace {
                            id: document.id.clone(),
                            expected_revision: visible.revision,
                            content: content(),
                        },
                    )
                    .await
                    .unwrap();
                let updated = bundle.documents.get(caller, &document.id).await.unwrap();
                assert!(matches!(
                    bundle
                        .documents
                        .mutate(
                            caller,
                            &request(&format!("{actor}-{action}-admin")),
                            Mutation::SetAccess {
                                id: document.id,
                                expected_revision: updated.revision.clone(),
                                access: DocumentAccess::default(),
                            }
                        )
                        .await,
                    Err(Error::NotFound)
                ));
                self.evidence.lock().await.document = Some(updated);
            } else if action.starts_with("reader") {
                let visible = bundle.documents.get(caller, &document.id).await.unwrap();
                assert_eq!(visible.id, document.id);
                assert_eq!(visible.content.payload, content().payload);
                assert_eq!(visible.owner, document.owner);
                // Reader grants never confer either data writes or ACL control.
                for (suffix, mutation) in [
                    (
                        "edit",
                        Mutation::Replace {
                            id: document.id.clone(),
                            expected_revision: visible.revision.clone(),
                            content: content(),
                        },
                    ),
                    (
                        "admin",
                        Mutation::SetAccess {
                            id: document.id.clone(),
                            expected_revision: visible.revision,
                            access: DocumentAccess::default(),
                        },
                    ),
                ] {
                    assert!(matches!(
                        bundle
                            .documents
                            .mutate(
                                caller,
                                &request(&format!("{actor}-{action}-{suffix}")),
                                mutation
                            )
                            .await,
                        Err(Error::NotFound)
                    ));
                }
            } else {
                assert!(matches!(
                    bundle.documents.get(caller, &document.id).await,
                    Err(Error::NotFound)
                ));
            }
        }
        self.events.send(format!("{actor}:{action}")).unwrap();
        let gate = match action {
            "private" => Some(&self.grant),
            "reader-hold" => Some(&self.revoke),
            "revoked-hold" => Some(&self.finish),
            _ => None,
        };
        if let Some(gate) = gate {
            tokio::time::timeout(Duration::from_secs(60), gate.acquire())
                .await
                .expect("test did not release the native probe barrier")
                .unwrap()
                .forget();
        }
        Ok(ToolResult::new(call.id.to_owned(), "{}".into(), false).into())
    }

    async fn set_shared(
        &self,
        bundle: &ToolBundleContext,
        owner_session: &SessionId,
        id: &str,
        shared: bool,
    ) {
        let helper = self.evidence.lock().await.actors["helper"].1.clone();
        let grants = if shared {
            vec![Grant {
                audience: Audience::Agent {
                    principal: helper,
                    reach: Reach::Forks,
                },
                role: Role::Reader,
            }]
        } else {
            vec![]
        };
        self.set_access(
            bundle,
            owner_session,
            id,
            DocumentAccess {
                grants,
                ..Default::default()
            },
        )
        .await;
    }

    async fn set_access(
        &self,
        bundle: &ToolBundleContext,
        owner_session: &SessionId,
        id: &str,
        access: DocumentAccess,
    ) {
        // The host refreshes the executing owner's exact native authority.
        // No fixture-created HostAccessContext or lineage enters the provider.
        let owner = caller(bundle, owner_session).await;
        let document = self.evidence.lock().await.document.clone().unwrap();
        let current = bundle.documents.get(&owner, &document.id).await.unwrap();
        bundle
            .documents
            .mutate(
                &owner,
                &request(id),
                Mutation::SetAccess {
                    id: document.id.clone(),
                    expected_revision: current.revision,
                    access,
                },
            )
            .await
            .unwrap();
        let updated = bundle.documents.get(&owner, &document.id).await.unwrap();
        self.evidence.lock().await.document = Some(updated);
    }
}

#[derive(Default)]
struct CrossMobClient {
    calls: Mutex<BTreeMap<&'static str, usize>>,
}

impl CrossMobClient {
    fn after_restart() -> Self {
        // Replayed job-completion notifications may revive an owner. They
        // must not repeat this test's already-completed creation script.
        Self {
            calls: Mutex::new(BTreeMap::from([(OWNER, 10), (ROOT_OWNER, 10)])),
        }
    }
}

#[async_trait::async_trait]
impl LlmClient for CrossMobClient {
    fn project_replay_messages(
        &self,
        messages: &[Message],
    ) -> std::result::Result<Vec<Message>, LlmError> {
        Ok(messages.to_vec())
    }

    fn provider(&self) -> meerkat_core::Provider {
        meerkat_core::Provider::OpenAI
    }

    async fn health_check(&self) -> std::result::Result<(), LlmError> {
        Ok(())
    }

    fn stream<'a>(
        &'a self,
        request: &'a LlmRequest,
    ) -> Pin<Box<dyn futures::Stream<Item = std::result::Result<LlmEvent, LlmError>> + Send + 'a>>
    {
        let role = request
            .messages
            .iter()
            .rev()
            .filter_map(|message| match message {
                Message::User(user) => Some(user.text_content()),
                _ => None,
            })
            .find_map(|text| {
                [
                    REOPEN_OWNER,
                    ROOT_REOPEN_DENIED,
                    ROOT_REOPEN_EDITOR,
                    ROOT_REOPEN_REVOKED,
                    ROOT_REVOKED,
                    ROOT_BARRIER,
                    ROOT_FORK,
                    ROOT_OWNER,
                    FORK,
                    HELPER,
                    OWNER,
                ]
                .into_iter()
                .find(|role| text.contains(role))
            })
            .expect("scripted native member task marker");
        let index = {
            let mut calls = self.calls.lock().unwrap();
            let index = calls.entry(role).or_default();
            let current = *index;
            *index += 1;
            current
        };
        for message in &request.messages {
            if let Message::ToolResults { results, .. } = message {
                for result in results {
                    assert!(
                        !result.is_error,
                        "native tool failed: {}",
                        result.text_content()
                    );
                }
            }
        }
        let probe = |actor: &str, action: &str| {
            Some((
                if action == "create" || action.starts_with("editor") {
                    "botus_apply"
                } else {
                    "botus_read"
                },
                serde_json::json!({"actor":actor,"action":action,
                    "principal":"forged-admin","lineage":["forged-root"]}),
            ))
        };
        let call = match (role, index) {
            (OWNER, 0) => probe("owner", "create"),
            (OWNER, 1) => Some((
                "delegate",
                serde_json::json!({"member_id":"cross-mob-helper","task":HELPER,
                    "result_label":"cross-mob-helper-result","max_text_bytes":1024,
                    "tooling":{"mode":"inherit_parent"}}),
            )),
            (HELPER, 0) => probe("helper", "private"),
            (HELPER, 1) => probe("helper", "reader-before-fork"),
            (HELPER, 2) => Some((
                "fork_off",
                serde_json::json!({"member_id":"cross-mob-fork","task":FORK,
                    "result_label":"cross-mob-fork-result","max_text_bytes":1024,
                    "idle_retire_secs":3600}),
            )),
            (HELPER, 3) => probe("helper", "reader-hold"),
            (HELPER, 4) => probe("helper", "revoked-hold"),
            (FORK, 0) => probe("fork", "reader-hold"),
            (FORK, 1) => probe("fork", "revoked-hold"),
            (REOPEN_OWNER, 0) => probe("owner", "owner-persisted"),
            (ROOT_OWNER, 0) => probe("owner", "create"),
            (ROOT_OWNER, 1) => Some((
                "fork_off",
                serde_json::json!({
                    "member_id":"root-persistent-fork", "task":ROOT_FORK,
                    "idle_retire_secs":3600, "max_text_bytes":1024,
                }),
            )),
            (ROOT_FORK, 0) => probe("fork", "editor-initial"),
            (ROOT_REVOKED, 0) => probe("fork", "root-revoked"),
            (ROOT_REOPEN_DENIED, 0) => probe("fork", "root-reopen-denied"),
            (ROOT_REOPEN_EDITOR, 0) => probe("fork", "editor-reopen"),
            (ROOT_REOPEN_REVOKED, 0) => probe("fork", "root-reopen-revoked"),
            _ => None,
        };
        let mut events = Vec::new();
        if let Some((name, args)) = &call {
            assert!(
                request.tools.iter().any(|tool| tool.name == *name),
                "native inherited surface must offer {name}"
            );
            events.push(Ok(LlmEvent::ToolCallComplete {
                id: format!("{role}-{index}"),
                name: (*name).into(),
                args: args.clone(),
                meta: None,
            }));
        } else {
            events.push(Ok(LlmEvent::TextDelta {
                delta: "done".into(),
                meta: None,
            }));
        }
        events.push(Ok(LlmEvent::UsageUpdate {
            usage: meerkat_core::TurnUsage::host_declared(
                self.provider(),
                &request.model,
                meerkat_core::Usage::default(),
            ),
        }));
        events.push(Ok(LlmEvent::Done {
            outcome: LlmDoneOutcome::Success {
                stop_reason: if call.is_some() {
                    StopReason::ToolUse
                } else {
                    StopReason::EndTurn
                },
            },
        }));
        Box::pin(futures::stream::iter(events))
    }
}

async fn expect_events(receiver: &mut mpsc::UnboundedReceiver<String>, expected: &[&str]) {
    let mut expected: std::collections::BTreeSet<String> =
        expected.iter().map(|event| (*event).to_owned()).collect();
    while !expected.is_empty() {
        let event = tokio::time::timeout(Duration::from_secs(60), receiver.recv())
            .await
            .unwrap_or_else(|_| panic!("waiting for native probe events: {expected:?}"))
            .expect("native probe channel closed");
        assert!(
            expected.remove(&event),
            "unexpected native probe event {event}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn extension_native_delegate_cross_mob_fork_sharing_revocation_and_document_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (scenario, mut events) = Scenario::new();
    let (runtime, context, _) = harness_with_client(
        dir.path(),
        None,
        Arc::new(CrossMobClient::default()),
        Some(scenario.clone()),
    )
    .await;
    let root = runtime.spawn(worker("cross-mob-owner")).await.unwrap();
    let owner_session = member_session(&runtime, &root.agent_identity).await;
    let owner_turn = runtime
        .start_member_turn(
            "cross-mob-owner",
            ContentInput::Text(OWNER.into()),
            HandlingMode::Queue,
            meerkat_mob::MemberTurnOptions::new(),
            None,
        )
        .await
        .unwrap();
    expect_events(&mut events, &["owner:create", "helper:private"]).await;
    let (helper_session, helper_principal) =
        scenario.evidence.lock().await.actors["helper"].clone();
    let Principal::Worker {
        mob_id: child_mob,
        member_id,
        ..
    } = &helper_principal
    else {
        panic!("native delegate must resolve its own Worker principal");
    };
    assert_ne!(child_mob, runtime.mob_handle().mob_id().as_str());
    assert_eq!(member_id, "cross-mob-helper");
    assert_ne!(helper_session, owner_session);
    let child_mob = MobId::from(child_mob.as_str());
    let state = runtime.mob_runtime().agent_mob_mcp_state().unwrap();
    let child = state.handle_for(&child_mob).await.unwrap();
    let helper_creation = child
        .member_creation_for_session(&helper_session)
        .await
        .unwrap()
        .unwrap();
    let MemberCreationProvenance::Spawn { source } = &helper_creation.creation.provenance else {
        panic!("delegate must record a native Spawn edge");
    };
    assert_eq!(source.session_id, owner_session);
    assert_eq!(
        source.member_binding.mob_id,
        runtime.mob_handle().mob_id().as_str()
    );

    scenario
        .set_shared(&context, &owner_session, "cross-mob-share", true)
        .await;
    scenario.grant.add_permits(1);
    expect_events(
        &mut events,
        &[
            "helper:reader-before-fork",
            "helper:reader-hold",
            "fork:reader-hold",
        ],
    )
    .await;
    let (fork_session, fork_principal) = scenario.evidence.lock().await.actors["fork"].clone();
    assert_ne!(fork_principal, helper_principal);
    let fork_creation = child
        .member_creation_for_session(&fork_session)
        .await
        .unwrap()
        .unwrap();
    let MemberCreationProvenance::Fork { source_creation_id } = &fork_creation.creation.provenance
    else {
        panic!("fork_off must record a native Fork edge");
    };
    assert_eq!(
        Some(*source_creation_id),
        helper_creation.creation.creation_id
    );
    assert_eq!(
        fork_creation
            .fork_source
            .as_ref()
            .unwrap()
            .source_session_id,
        helper_session
    );
    scenario
        .set_shared(&context, &owner_session, "cross-mob-revoke", false)
        .await;
    scenario.revoke.add_permits(2);
    expect_events(&mut events, &["helper:revoked-hold", "fork:revoked-hold"]).await;
    scenario.finish.add_permits(2);
    tokio::time::timeout(Duration::from_secs(60), owner_turn.turn.wait())
        .await
        .unwrap()
        .unwrap();
    // Delegate always retires its helper and cascades that retirement to
    // fork_off descendants. The fixture must not override that lifetime.
    assert!(
        child
            .get_member(&AgentIdentity::from("cross-mob-helper"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        child
            .get_member(&AgentIdentity::from("cross-mob-fork"))
            .await
            .unwrap()
            .is_none()
    );
    let sessions = state.session_service();
    assert!(
        sessions
            .load_persisted_session_metadata(&helper_session)
            .await
            .unwrap()
            .is_none()
    );
    let retained = sessions
        .load_retained_session_metadata(&helper_session)
        .await
        .unwrap()
        .unwrap()
        .session_metadata
        .unwrap();
    assert_eq!(
        retained.mob_member_binding.as_ref(),
        Some(&helper_creation.member_binding)
    );
    // Stop through the native child handle before releasing the old host.
    state.mob_stop(&child_mob).await.unwrap();
    runtime.shutdown().await;
    drop(sessions);
    drop(child);
    drop(state);
    drop(context);
    drop(runtime);

    let (reopened, context, _) = harness_with_client(
        dir.path(),
        None,
        Arc::new(CrossMobClient::after_restart()),
        Some(scenario.clone()),
    )
    .await;
    let state = reopened.mob_runtime().agent_mob_mcp_state().unwrap();
    // This is the current native capability boundary, not a reconstructed
    // registry or a promise that ephemeral helper mobs survive restart.
    assert!(matches!(
        state.handle_for(&child_mob).await,
        Err(meerkat_mob::MobError::MobNotFound(_))
    ));
    let retained_after = state
        .session_service()
        .load_retained_session_metadata(&helper_session)
        .await
        .unwrap()
        .unwrap()
        .session_metadata
        .unwrap();
    assert_eq!(
        retained_after.mob_member_binding,
        retained.mob_member_binding
    );
    assert_eq!(
        retained_after.tooling.tool_access_policy,
        retained.tooling.tool_access_policy
    );
    let owner_turn = reopened
        .start_member_turn(
            "cross-mob-owner",
            ContentInput::Text(REOPEN_OWNER.into()),
            HandlingMode::Queue,
            meerkat_mob::MemberTurnOptions::new(),
            None,
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(60), owner_turn.turn.wait())
        .await
        .unwrap()
        .unwrap();
    expect_events(&mut events, &["owner:owner-persisted"]).await;
    drop(context);
    reopened.shutdown().await;
}

async fn run_root_fork_turn(handle: &MobHandle, task: &str) {
    let member = handle
        .member(&AgentIdentity::from("root-persistent-fork"))
        .await
        .unwrap();
    let turn = member
        .start_turn(
            ContentInput::Text(task.into()),
            HandlingMode::Queue,
            meerkat_mob::MemberTurnOptions::new(),
            None,
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(60), turn.wait())
        .await
        .unwrap()
        .unwrap();
}

/// A fork of the persistent root member remains seated after its work ends.
/// This is separate from delegate: the durable owner is not retired here.
#[tokio::test(flavor = "multi_thread")]
async fn extension_native_root_fork_off_document_access_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (scenario, mut events) = Scenario::new();
    let (runtime, context, _) = harness_with_client(
        dir.path(),
        None,
        Arc::new(CrossMobClient::default()),
        Some(scenario.clone()),
    )
    .await;
    let root = runtime.spawn(worker("persistent-owner")).await.unwrap();
    let owner_session = member_session(&runtime, &root.agent_identity).await;
    let owner_turn = runtime
        .start_member_turn(
            "persistent-owner",
            ContentInput::Text(ROOT_OWNER.into()),
            HandlingMode::Queue,
            meerkat_mob::MemberTurnOptions::new(),
            None,
        )
        .await
        .unwrap();
    expect_events(&mut events, &["owner:create", "fork:editor-initial"]).await;
    tokio::time::timeout(Duration::from_secs(60), owner_turn.turn.wait())
        .await
        .unwrap()
        .unwrap();
    // A queued tracked turn is a native completion barrier for the initial
    // fork job, whose public fork_off result can return before it finishes.
    run_root_fork_turn(&runtime.mob_handle(), ROOT_BARRIER).await;
    let (fork_session, fork_principal) = scenario.evidence.lock().await.actors["fork"].clone();
    let owner_principal = scenario.evidence.lock().await.actors["owner"].1.clone();
    assert_ne!(fork_principal, owner_principal);
    let original = runtime
        .mob_handle()
        .member_creation_for_session(&fork_session)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        original.creation.provenance,
        MemberCreationProvenance::Fork { .. }
    ));
    assert_eq!(
        original.fork_source.as_ref().unwrap().source_session_id,
        owner_session
    );
    assert_eq!(
        original.member_binding.mob_id,
        runtime.mob_handle().mob_id().as_str()
    );

    let private = || DocumentAccess {
        owner_reach: Reach::SelfOnly,
        ..Default::default()
    };
    scenario
        .set_access(&context, &owner_session, "root-fork-revoke", private())
        .await;
    run_root_fork_turn(&runtime.mob_handle(), ROOT_REVOKED).await;
    expect_events(&mut events, &["fork:root-revoked"]).await;
    runtime.shutdown().await;
    drop(context);
    drop(runtime);

    let (reopened, context, _) = harness_with_client(
        dir.path(),
        None,
        Arc::new(CrossMobClient::after_restart()),
        Some(scenario.clone()),
    )
    .await;
    let restored = reopened
        .mob_handle()
        .member_creation_for_session(&fork_session)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored, original);
    assert_eq!(
        member_session(&reopened, &AgentIdentity::from("root-persistent-fork")).await,
        fork_session
    );
    // The revoked ACL survives the new host and freshly built dispatcher.
    run_root_fork_turn(&reopened.mob_handle(), ROOT_REOPEN_DENIED).await;
    expect_events(&mut events, &["fork:root-reopen-denied"]).await;
    scenario
        .set_access(
            &context,
            &owner_session,
            "root-fork-restore-reach",
            DocumentAccess::default(),
        )
        .await;
    // Restoring default Forks reach grants data editing, never ACL control.
    run_root_fork_turn(&reopened.mob_handle(), ROOT_REOPEN_EDITOR).await;
    expect_events(&mut events, &["fork:editor-reopen"]).await;
    scenario
        .set_access(
            &context,
            &owner_session,
            "root-fork-revoke-again",
            private(),
        )
        .await;
    run_root_fork_turn(&reopened.mob_handle(), ROOT_REOPEN_REVOKED).await;
    expect_events(&mut events, &["fork:root-reopen-revoked"]).await;
    reopened.shutdown().await;
}
