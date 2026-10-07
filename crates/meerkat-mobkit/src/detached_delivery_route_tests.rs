//! Every MobKit composition that installs agent mob tools gives `fork_off`
//! and `council` the DETACHED route.
//!
//! Detached completion needs the session service's canonical runtime and a
//! bound continuation owner. Without that composition, `fork_off` and
//! `council` fall back to blocking for the child's whole run.
//!
//! `MobMcpState::new` takes its adapter from the session service it is given
//! (`MobSessionService::acquire_runtime_adapter`), captured when MobKit
//! installs the agent mob tools. Constructor checks pin the detached route
//! and shared runtime. The actual-tool fixtures then prove that MobRuntime's
//! delivery owner applies a member-addressed completion once, even with no
//! callback jobs from which an older host could discover that member.

#![allow(clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use meerkat::{Config, FactoryAgentBuilder};
use meerkat_session::PersistentSessionService;

use crate::mob_handle_runtime::{CapabilityFlags, MobBootstrapSpec, MobRuntimeDelivery};

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
    let jobs: Arc<dyn meerkat::DetachedJobStore> = Arc::new(meerkat::MemoryDetachedJobStore::new());
    builder.default_detached_job_store = Some(Arc::clone(&jobs));
    let delivery = MobRuntimeDelivery::new(Arc::clone(&runtime_store), jobs);
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
    .with_runtime_delivery(delivery)
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
    let jobs: Arc<dyn meerkat::DetachedJobStore> = Arc::new(meerkat::MemoryDetachedJobStore::new());
    builder.default_detached_job_store = Some(Arc::clone(&jobs));
    let delivery = MobRuntimeDelivery::new(Arc::clone(&runtime_store), jobs);
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
    .with_runtime_delivery(delivery)
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

/// Exercise the host composition through real native tools and sessions. Only
/// provider responses are scripted; no test code submits or drains completion
/// inputs. The callback job store stays empty, so enumerating callback origins
/// cannot discover these member-addressed continuation rows.
mod completion_delivery {
    use std::sync::Mutex;
    use std::time::Duration;

    use futures::StreamExt as _;
    use meerkat_client::{LlmClient, LlmError, LlmEvent, LlmRequest, LlmStream};
    use meerkat_core::event::BackgroundJobTerminalStatus;
    use meerkat_core::types::{SystemNoticeBlock, SystemNoticeKind, ToolCallView};
    use meerkat_core::{AgentToolDispatcher, Message, SessionId};
    use meerkat_mob::{AgentIdentity, MobSessionService, SpawnMemberSpec};
    use serde_json::{Value, json};

    use super::*;
    use crate::mob_handle_runtime::{MobBootstrapOptions, MobRuntime};

    const WAIT: Duration = Duration::from_secs(60);
    const CHILD_TASK: &str = "MOBKIT-HELD-FORK-TASK";
    const CHILD_REPLY: &str = "MOBKIT-FORK-RESULT";
    const COUNCIL_SUMMARY: &str = "MOBKIT-COUNCIL-SUMMARY";
    const FOLLOW_UP: &str = "MOBKIT-OWNER-FOLLOW-UP";

    struct TurnGate {
        open: tokio::sync::watch::Sender<bool>,
        entered: tokio::sync::watch::Sender<usize>,
    }

    impl TurnGate {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                open: tokio::sync::watch::Sender::new(false),
                entered: tokio::sync::watch::Sender::new(0),
            })
        }

        async fn wait(&self) {
            let mut open = self.open.subscribe();
            self.entered.send_modify(|count| *count += 1);
            while !*open.borrow_and_update() {
                open.changed().await.expect("gate remains owned");
            }
        }

        async fn wait_entered(&self, count: usize) {
            let mut entered = self.entered.subscribe();
            tokio::time::timeout(WAIT, entered.wait_for(|seen| *seen >= count))
                .await
                .expect("child provider turn reaches the gate")
                .expect("gate remains owned");
        }

        fn release(&self) {
            self.open.send_replace(true);
        }
    }

    struct ReleaseOnDrop(Arc<TurnGate>);

    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    #[derive(Default)]
    struct RequestLog {
        messages: Mutex<Vec<Vec<Message>>>,
        recorded: tokio::sync::watch::Sender<usize>,
    }

    impl RequestLog {
        fn record(&self, request: &LlmRequest) {
            self.messages
                .lock()
                .expect("request log")
                .push(request.messages.clone());
            self.recorded.send_modify(|count| *count += 1);
        }

        fn requests_with_completion(&self, job_id: &str) -> usize {
            self.messages
                .lock()
                .expect("request log")
                .iter()
                .filter(|messages| !completion_records(messages, job_id).is_empty())
                .count()
        }

        fn assert_follow_up_retains_completion(&self, job_id: &str) {
            let messages = self.messages.lock().expect("request log");
            let follow_ups = messages
                .iter()
                .filter(|messages| {
                    messages
                        .iter()
                        .rev()
                        .find_map(|message| match message {
                            Message::User(user) => Some(user.text_content()),
                            _ => None,
                        })
                        .as_deref()
                        == Some(FOLLOW_UP)
                })
                .collect::<Vec<_>>();
            assert_eq!(follow_ups.len(), 1, "one explicit owner follow-up request");
            assert_eq!(
                completion_records(follow_ups[0], job_id).len(),
                1,
                "the follow-up itself carries the one persisted completion"
            );
        }

        async fn wait_for_completion_request(&self, job_id: &str) {
            let mut recorded = self.recorded.subscribe();
            tokio::time::timeout(
                WAIT,
                recorded.wait_for(|_| self.requests_with_completion(job_id) > 0),
            )
            .await
            .expect("completion wakes its owner")
            .expect("request log remains owned");
        }
    }

    struct CompletionClient {
        gate: Arc<TurnGate>,
        log: Arc<RequestLog>,
    }

    #[async_trait::async_trait]
    impl LlmClient for CompletionClient {
        fn project_replay_messages(&self, messages: &[Message]) -> Result<Vec<Message>, LlmError> {
            Ok(messages.to_vec())
        }

        fn stream<'a>(&'a self, request: &'a LlmRequest) -> LlmStream<'a> {
            self.log.record(request);
            let users = request
                .messages
                .iter()
                .filter_map(|message| match message {
                    Message::User(user) => Some(user.text_content()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let all_users = users.join("\n");
            let role = all_users
                .rsplit_once("You are '")
                .and_then(|(_, rest)| rest.split_once('\''))
                .map(|(role, _)| role);
            let (held, text) = if all_users.contains("bounded plain-text summary") {
                (false, COUNCIL_SUMMARY.to_string())
            } else if users.last().is_some_and(|text| text.contains(CHILD_TASK)) {
                (true, CHILD_REPLY.to_string())
            } else if let Some(role) = role {
                (true, format!("position from {role}"))
            } else {
                (false, "owner acknowledged".to_string())
            };
            let gate = Arc::clone(&self.gate);
            let model = request.model.clone();
            Box::pin(
                futures::stream::once(async move {
                    if held {
                        gate.wait().await;
                    }
                    vec![
                        LlmEvent::TextDelta {
                            delta: text,
                            meta: None,
                        },
                        LlmEvent::UsageUpdate {
                            usage: meerkat_core::TurnUsage::host_declared(
                                meerkat_core::Provider::OpenAI,
                                &model,
                                meerkat_core::Usage::default(),
                            ),
                        },
                        LlmEvent::Done {
                            outcome: meerkat_client::LlmDoneOutcome::Success {
                                stop_reason: meerkat_core::StopReason::EndTurn,
                            },
                        },
                    ]
                })
                .flat_map(|events| futures::stream::iter(events.into_iter().map(Ok))),
            )
        }

        fn provider(&self) -> meerkat_core::Provider {
            meerkat_core::Provider::OpenAI
        }

        async fn health_check(&self) -> Result<(), LlmError> {
            Ok(())
        }
    }

    struct CompletionFixture {
        runtime: MobRuntime,
        service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
        runtime_store: Arc<dyn meerkat_runtime::RuntimeStore>,
        jobs: Arc<dyn meerkat::DetachedJobStore>,
        delivery: MobRuntimeDelivery,
        log: Arc<RequestLog>,
        _temp: tempfile::TempDir,
    }

    impl CompletionFixture {
        async fn start(gate: Arc<TurnGate>, members: &[&str]) -> Self {
            let temp = tempfile::tempdir().expect("completion fixture directory");
            let session_store: Arc<dyn meerkat::SessionStore> =
                Arc::new(meerkat::MemoryStore::new());
            let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
                Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
            let blob_store: Arc<dyn meerkat_core::BlobStore> =
                Arc::new(meerkat_store::MemoryBlobStore::new());
            let jobs: Arc<dyn meerkat::DetachedJobStore> =
                Arc::new(meerkat::MemoryDetachedJobStore::new());
            let delivery = MobRuntimeDelivery::new(Arc::clone(&runtime_store), Arc::clone(&jobs));
            let log = Arc::new(RequestLog::default());
            let client: Arc<dyn LlmClient> = Arc::new(CompletionClient {
                gate,
                log: Arc::clone(&log),
            });
            let factory = meerkat::AgentFactory::new(temp.path()).comms(true);
            let mut builder = FactoryAgentBuilder::new(factory, Config::default());
            builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
                Arc::clone(&session_store),
            )));
            builder.default_blob_store = Some(Arc::clone(&blob_store));
            builder.default_detached_job_store = Some(Arc::clone(&jobs));
            builder.default_llm_client = Some(Arc::clone(&client));
            let slot = Arc::clone(&builder.default_mob_tools);
            let adapter = Arc::new(
                meerkat_runtime::MeerkatMachine::persistent(
                    Arc::clone(&runtime_store),
                    Arc::clone(&blob_store),
                )
                .expect("completion runtime machine"),
            );
            let service = Arc::new(PersistentSessionService::new(
                builder,
                16,
                session_store,
                Arc::clone(&runtime_store),
                blob_store,
            ));
            let spec = MobBootstrapSpec::new(
                definition(&format!("delivery-{}", uuid::Uuid::new_v4().simple())),
                meerkat_mob::MobStorage::in_memory(),
                service.clone(),
            )
            .with_session_runtime_adapter(adapter)
            .expect("acquire completion fixture owner")
            .with_runtime_delivery(delivery.clone())
            .with_agent_mob_tools(slot)
            .expect("install actual agent mob tools")
            .with_options(MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: false,
                default_llm_client: Some(client),
            });
            let runtime = tokio::time::timeout(WAIT, Box::pin(MobRuntime::bootstrap(spec)))
                .await
                .expect("bounded completion fixture bootstrap")
                .expect("bootstrap completion fixture");
            for member in members {
                let mut spawn = SpawnMemberSpec::new("general".into(), (*member).into());
                spawn.runtime_mode = Some(meerkat_mob::MobRuntimeMode::TurnDriven);
                tokio::time::timeout(WAIT, runtime.handle().spawn_spec(spawn))
                    .await
                    .expect("bounded source member spawn")
                    .expect("seat source member");
            }
            assert!(
                delivery.is_running(),
                "MobRuntime retains its native delivery owner"
            );
            let fixture = Self {
                runtime,
                service,
                runtime_store,
                jobs,
                delivery,
                log,
                _temp: temp,
            };
            fixture.assert_no_callback_jobs().await;
            fixture
        }

        async fn session(&self, member: &str) -> SessionId {
            tokio::time::timeout(
                WAIT,
                self.runtime
                    .handle()
                    .resolve_bridge_session_id(&AgentIdentity::from(member)),
            )
            .await
            .expect("bounded member session lookup")
            .expect("source member session")
        }

        async fn messages(&self, session: &SessionId) -> Vec<Message> {
            tokio::time::timeout(WAIT, self.service.load_persisted_session(session))
                .await
                .expect("bounded owner transcript read")
                .expect("load owner transcript")
                .expect("owner transcript exists")
                .messages()
                .to_vec()
        }

        async fn assert_no_callback_jobs(&self) {
            assert!(
                self.jobs
                    .list_all(1)
                    .await
                    .expect("read callback jobs")
                    .is_empty(),
                "fork and council completion must not depend on callback-origin enumeration"
            );
        }

        async fn surface(&self, member: &str, council: bool) -> Arc<dyn AgentToolDispatcher> {
            let handle = self.runtime.handle();
            let authority =
                meerkat_runtime::mob_operator_authority::create_only_mob_operator_authority()
                    .expect("native fixture authority");
            let authority = if council {
                meerkat_runtime::mob_operator_authority::grant_manage_mob(
                    &authority,
                    handle.mob_id().as_str(),
                )
                .expect("council may use the source mob")
            } else {
                let authority = meerkat_runtime::mob_operator_authority::set_create_authority(
                    &authority, false,
                )
                .expect("fork has no create scope");
                meerkat_runtime::mob_operator_authority::grant_spawn_profile_in_mob(
                    &authority,
                    handle.mob_id().as_str(),
                    "general",
                )
                .expect("fork may spawn only its source profile")
            };
            let session = self.session(member).await;
            let surface: Arc<dyn AgentToolDispatcher> =
                Arc::new(meerkat_mob_mcp::AgentMobToolSurface::new(
                    self.runtime.agent_mob_mcp_state().expect("final MCP state"),
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
                .expect("bind native agent tool lifecycle")
                .into_dispatcher()
        }

        async fn member_address(&self, member: &str) -> meerkat_runtime::LogicalRuntimeId {
            let handle = self.runtime.handle();
            let identity = AgentIdentity::from(member);
            let generation = handle
                .roster()
                .await
                .get_by_identity(&identity)
                .expect("seated completion owner")
                .generation
                .get();
            meerkat::member_delivery_address(handle.mob_id().as_str(), member, generation)
                .expect("native member address")
        }

        async fn assert_no_completion(&self, member: &str, job_id: &str) {
            let address = self.member_address(member).await;
            let session = self.session(member).await;
            assert!(completion_records(&self.messages(&session).await, job_id).is_empty());
            assert!(
                self.runtime_store
                    .list_runtime_delivery_records(&address, 0, 1)
                    .await
                    .expect("read held completion's member inbox")
                    .is_empty(),
                "a held child has not submitted any completion"
            );
            self.assert_no_callback_jobs().await;
        }

        async fn assert_delivered(&self, member: &str, job_id: &str, expected: &str) -> Value {
            let handle = self.runtime.handle();
            let identity = AgentIdentity::from(member);
            let address = self.member_address(member).await;
            let session = self.session(member).await;
            let inbox = self.delivery.inbox();
            tokio::time::timeout(WAIT, async {
                loop {
                    let rows = self
                        .runtime_store
                        .list_runtime_delivery_records(&address, 0, 2)
                        .await
                        .expect("read native member inbox");
                    assert!(rows.len() <= 1, "one continuation row for one completion");
                    if let Some(row) = rows.first() {
                        let delivery_id = meerkat_runtime::RuntimeDeliveryId::new(row.delivery_id())
                            .expect("native delivery id");
                        let status = inbox
                            .delivery_status(&address, &delivery_id)
                            .await
                            .expect("native delivery status");
                        if matches!(status, meerkat_runtime::RuntimeDeliveryStatus::Applied { .. }) {
                            let admission = inbox
                                .continuation_admission(&address, &delivery_id)
                                .await
                                .expect("native admission receipt");
                            assert!(
                                matches!(
                                    admission,
                                    Some(meerkat_runtime::ContinuationAdmission::Applied { session_id, .. })
                                        if session_id == session
                                ),
                                "the member inbox was applied to the exact owner session"
                            );
                            break;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await
            .expect("native owner applies the member-addressed continuation");
            self.log.wait_for_completion_request(job_id).await;
            assert_eq!(
                self.log.requests_with_completion(job_id),
                1,
                "one completion wake"
            );

            let bounded = meerkat_mob::BoundedResultSpec::new("delivery-follow-up", 1024)
                .expect("bounded follow-up");
            let work = tokio::time::timeout(
                WAIT,
                handle.start_work_for_identity_bounded(
                    identity,
                    meerkat_mob::WorkSpec::new(FOLLOW_UP.into(), meerkat_mob::WorkOrigin::Internal),
                    meerkat_core::HandlingMode::Queue,
                    bounded.clone(),
                ),
            )
            .await
            .expect("bounded follow-up admission")
            .expect("queue owner follow-up");
            let result = tokio::time::timeout(WAIT, work.wait_bounded(bounded))
                .await
                .expect("bounded owner follow-up")
                .expect("owner follow-up succeeds");
            assert_eq!(result.result().result().text(), "owner acknowledged");
            self.log.assert_follow_up_retains_completion(job_id);
            assert_eq!(
                self.log.requests_with_completion(job_id),
                2,
                "one wake and one explicit follow-up carry the same completion"
            );
            let records = completion_records(&self.messages(&session).await, job_id);
            assert_eq!(
                records.len(),
                1,
                "one persisted completion after the follow-up"
            );
            assert_eq!(records[0].0, BackgroundJobTerminalStatus::Completed);
            assert!(
                records[0].1.contains(expected),
                "completion retains the child result"
            );
            self.assert_no_callback_jobs().await;
            serde_json::from_str(&records[0].1).expect("typed completion detail")
        }

        async fn stop(self) {
            let report = tokio::time::timeout(WAIT, self.runtime.handle().destroy())
                .await
                .expect("bounded fixture destruction")
                .expect("destroy fixture mob");
            assert!(report.errors.is_empty());
            assert!(report.orphaned_remote_members.is_empty());
            assert!(!report.remote_cleanup_deadline_exceeded);
            assert!(report.metadata_scrubbed && report.events_cleared && report.namespace_cleaned);
        }
    }

    fn completion_records(
        messages: &[Message],
        job_id: &str,
    ) -> Vec<(BackgroundJobTerminalStatus, String)> {
        messages
            .iter()
            .filter_map(|message| match message {
                Message::SystemNotice(notice) if notice.kind == SystemNoticeKind::BackgroundJob => {
                    notice.blocks.iter().find_map(|block| match block {
                        SystemNoticeBlock::BackgroundJob {
                            job_id: id,
                            status,
                            detail,
                            persisted: true,
                            ..
                        } if id == job_id => Some((*status, detail.clone().unwrap_or_default())),
                        _ => None,
                    })
                }
                _ => None,
            })
            .collect()
    }

    async fn start_detached(
        surface: &Arc<dyn AgentToolDispatcher>,
        tool: &str,
        args: Value,
    ) -> Value {
        let raw =
            serde_json::value::RawValue::from_string(args.to_string()).expect("tool arguments");
        let outcome = tokio::time::timeout(
            WAIT,
            surface.dispatch(ToolCallView {
                id: "detached-completion-fixture",
                name: tool,
                args: &raw,
            }),
        )
        .await
        .expect("tool returns before its held child completes")
        .expect("native agent tool starts");
        assert!(
            !outcome.result.is_error,
            "{}",
            outcome.result.text_content()
        );
        let started: Value =
            serde_json::from_str(&outcome.result.text_content()).expect("tool result");
        assert_eq!(started["status"], "running", "{started}");
        assert!(started["job_id"].as_str().is_some_and(|id| !id.is_empty()));
        assert!(started.get("blocked_because").is_none(), "{started}");
        started
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fork_completion_reaches_member_with_no_callback_jobs() {
        let gate = TurnGate::new();
        let _release = ReleaseOnDrop(Arc::clone(&gate));
        let fixture = CompletionFixture::start(Arc::clone(&gate), &["owner"]).await;
        let surface = fixture.surface("owner", false).await;
        let started = start_detached(
            &surface,
            "fork_off",
            json!({
                "member_id": "child", "task": CHILD_TASK, "max_run_secs": 120,
            }),
        )
        .await;
        let job_id = started["job_id"].as_str().expect("fork job id");
        gate.wait_entered(1).await;
        fixture.assert_no_completion("owner", job_id).await;
        assert_eq!(
            fixture
                .runtime
                .handle()
                .get_member(&AgentIdentity::from("child"))
                .await
                .expect("fork child")
                .expect("child remains seated")
                .spawned_by,
            Some(AgentIdentity::from("owner"))
        );
        gate.release();
        fixture.assert_delivered("owner", job_id, CHILD_REPLY).await;
        fixture.stop().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn council_completion_reaches_member_with_no_callback_jobs() {
        let gate = TurnGate::new();
        let _release = ReleaseOnDrop(Arc::clone(&gate));
        let fixture =
            CompletionFixture::start(Arc::clone(&gate), &["convener", "alice", "bob"]).await;
        let surface = fixture.surface("convener", true).await;
        let mob_id = fixture.runtime.handle().mob_id().to_string();
        let started = start_detached(
            &surface,
            "council",
            json!({
                "topic": "Should we ship the delivery repair?",
                "participants": [
                    {"mob_id": mob_id, "member_id": "alice", "role": "analyst"},
                    {"mob_id": mob_id, "member_id": "bob", "role": "critic"},
                ],
                "max_rounds": 1, "timeout_seconds": 120,
            }),
        )
        .await;
        assert!(started["council_id"].as_str().is_some());
        let job_id = started["job_id"].as_str().expect("council job id");
        gate.wait_entered(1).await;
        fixture.assert_no_completion("convener", job_id).await;
        gate.release();
        let outcome = fixture
            .assert_delivered("convener", job_id, COUNCIL_SUMMARY)
            .await;
        assert_eq!(
            outcome["result"]["exit_reason"]["reason"], "completed",
            "{outcome}"
        );
        let participants = outcome["result"]["participants"]
            .as_array()
            .expect("council participants");
        assert_eq!(participants.len(), 2);
        assert!(
            participants
                .iter()
                .all(|participant| participant["seated"] == true)
        );
        assert!(
            outcome["result"]["exchanges"]
                .as_array()
                .is_some_and(|exchanges| exchanges.len() >= 2)
        );
        fixture.stop().await;
    }
}
