//! HTTP server and route assembly for the unified runtime.

use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use futures::StreamExt;
use meerkat_core::comms::EventStream;

use crate::console_aggregator::{
    ConsoleVisibilityPolicy, HideImplicitDelegateMembersConsoleVisibilityPolicy,
};
use crate::http_console::{
    console_frontend_router, console_json_router_with_runtime_events_and_policy,
};
use crate::http_flow_editor::protected_flow_editor_router_with_runtime_catalog;
use crate::http_sse::{
    agent_events_sse_router_with_access_and_priming, mob_events_sse_router_with_access_and_priming,
    mob_structural_events_sse_router_with_access_and_priming,
};
use crate::runtime::RuntimeDecisionState;
use tower::limit::ConcurrencyLimitLayer;

use super::UnifiedRuntime;

async fn healthz_response(
    headers: HeaderMap,
    job_health_projection: Arc<std::sync::RwLock<Option<serde_json::Value>>>,
) -> axum::response::Response {
    let wants_json = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|item| item.trim().starts_with("application/json"))
        });
    if !wants_json {
        return "ok".into_response();
    }
    let projection = job_health_projection
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let public_projection = projection.map_or_else(
        || {
            serde_json::json!({
                "status": "ok",
                "detached_jobs": null
            })
        },
        |projection| {
            serde_json::json!({
                "status": projection
                    .get("status")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!("ok")),
                "detached_jobs": projection
                    .get("detached_jobs")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null)
            })
        },
    );
    Json(public_projection).into_response()
}

/// Default cap for the stock reference app router.
///
/// SSE routes keep HTTP requests open for the lifetime of the subscription, so
/// a demo-scale cap such as 20 can make the console look frozen while streams
/// occupy all slots. Hosts that wrap MobKit in their own axum service may still
/// choose a different outer limit, but the reference router itself defaults to
/// a ceiling high enough for real console usage.
pub const DEFAULT_REFERENCE_APP_MAX_CONCURRENT_REQUESTS: usize = 1024;

async fn resolve_agent_event_member_id(
    handle: &meerkat_mob::MobHandle,
    agent_id: &str,
) -> meerkat_mob::ids::AgentIdentity {
    let direct = crate::member_comms_id::mob_member_id(agent_id);
    if handle.get_member(&direct).await.ok().flatten().is_some() {
        return direct;
    }
    handle
        .list_members_including_retiring()
        .await
        .into_iter()
        .find(|entry| {
            crate::member_comms_id::durable_identity_label(&entry.labels)
                .is_some_and(|identity| identity == agent_id)
        })
        .map(|entry| entry.agent_identity)
        .unwrap_or(direct)
}

async fn identity_event_alias_is_current(
    identity_runtime: &crate::identity_first::IdentityRuntime,
    identity: &crate::identity_first::AgentIdentity,
    expected_alias: &str,
) -> bool {
    identity_runtime.status(identity).await.is_ok_and(|status| {
        status.state == crate::identity_first::IdentityLifecycleState::Active
            && status
                .agent_runtime_id
                .as_ref()
                .is_some_and(|runtime_id| runtime_id.as_str() == expected_alias)
    })
}

/// Keep a per-agent event subscription bound to the exact identity generation
/// that was current when the HTTP request was admitted. A reset replaces that
/// generation while the lower-plane event stream can remain alive long enough
/// to emit late output; forwarding it through the durable/current alias would
/// make a stale owner appear authoritative.
fn generation_authoritative_agent_event_stream(
    mut event_stream: EventStream,
    mut identity_events: tokio::sync::broadcast::Receiver<
        crate::identity_first::runtime::IdentityEvent,
    >,
    identity_runtime: Arc<crate::identity_first::IdentityRuntime>,
    identity: crate::identity_first::AgentIdentity,
    expected_alias: String,
) -> EventStream {
    Box::pin(async_stream::stream! {
        // Reset currently updates the continuity entry without guaranteeing a
        // state-change notification on the pre-reset identity channel. Poll
        // the cheap in-memory status snapshot as a liveness backstop so an
        // otherwise-idle SSE connection still closes promptly.
        let mut authority_check = tokio::time::interval(Duration::from_millis(250));
        authority_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                // Prefer lifecycle invalidation when both it and a stale
                // lower-plane event are ready in the same scheduler turn.
                biased;
                lifecycle = identity_events.recv() => {
                    match lifecycle {
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            if !identity_event_alias_is_current(
                                identity_runtime.as_ref(),
                                &identity,
                                &expected_alias,
                            )
                            .await
                            {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = authority_check.tick() => {
                    if !identity_event_alias_is_current(
                        identity_runtime.as_ref(),
                        &identity,
                        &expected_alias,
                    )
                    .await
                    {
                        break;
                    }
                }
                event = event_stream.next() => {
                    let Some(event) = event else {
                        break;
                    };
                    // Reset marks the identity non-Active before publishing
                    // the replacement generation. Recheck on every payload so
                    // output racing that transition is dropped immediately,
                    // even before the eventual Active lifecycle event arrives.
                    if !identity_event_alias_is_current(
                        identity_runtime.as_ref(),
                        &identity,
                        &expected_alias,
                    )
                    .await
                    {
                        break;
                    }
                    yield event;
                }
            }
        }
    })
}

impl UnifiedRuntime {
    pub fn build_console_json_router(&self, decisions: RuntimeDecisionState) -> Router {
        self.build_console_json_router_with_policy(
            decisions,
            Arc::new(HideImplicitDelegateMembersConsoleVisibilityPolicy),
        )
    }

    pub fn build_console_json_router_with_policy(
        &self,
        decisions: RuntimeDecisionState,
        visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    ) -> Router {
        console_json_router_with_runtime_events_and_policy(
            decisions,
            self.mob_runtime.clone(),
            Some(self.module_runtime_handle()),
            self.contact_directory.clone(),
            self.event_log_store(),
            self.gateway_peer_keys().cloned(),
            Some(self.console_events()),
            Some(self.console_log_store()),
            Some(self.mob_events_store()),
            Some(Arc::clone(self.metadata_table())),
            self.identity_runtime().cloned(),
            visibility_policy,
            self.access_controller().cloned(),
            self.memory_panel_store(),
            self.console_operator_resolver(),
            self.console_identity_roster(),
            self.workgraph_service(),
            Some(self.topology_runtime_handle()),
            Some(Arc::clone(&self.job_health_projection)),
        )
    }

    pub fn build_console_frontend_router(&self) -> Router {
        console_frontend_router()
    }

    pub fn build_reference_app_router(&self, decisions: RuntimeDecisionState) -> Router {
        self.build_reference_app_router_with_console_visibility_policy(
            decisions,
            Arc::new(HideImplicitDelegateMembersConsoleVisibilityPolicy),
        )
    }

    pub fn build_reference_app_router_with_console_visibility_policy(
        &self,
        decisions: RuntimeDecisionState,
        visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    ) -> Router {
        let agent_runtime = self.mob_runtime.clone();
        let agent_identity_runtime = self.identity_runtime().cloned();
        let mob_runtime = self.mob_runtime.clone();
        // Every SSE route shares the same `RuntimeDecisionState` the
        // console RPC route uses: when `require_app_auth` is on, requests
        // must carry a valid bearer / auth_token. Pre-fix, only the
        // structural-events route gated; tier-2 (`/agents/{id}/events`)
        // and tier-3 (`/mob/events`) shipped unauthenticated.
        let flow_editor_decisions = decisions.clone();
        let flow_editor_runtime_catalog = self.mobpack_runtime_catalog_state_snapshot();
        let sse_decisions_a = decisions.clone();
        let sse_decisions_b = decisions.clone();
        let sse_decisions_c = decisions.clone();
        let access = self.access_controller().cloned();
        let agent_sse_visibility_policy = visibility_policy.clone();
        let mob_sse_visibility_policy = visibility_policy.clone();
        let structural_sse_visibility_policy = visibility_policy.clone();
        let job_health_projection = Arc::clone(&self.job_health_projection);
        Router::new()
            .route(
                "/healthz",
                get(move |headers: HeaderMap| {
                    let job_health_projection = Arc::clone(&job_health_projection);
                    healthz_response(headers, job_health_projection)
                }),
            )
            .merge(self.build_console_frontend_router())
            .merge(protected_flow_editor_router_with_runtime_catalog(
                flow_editor_decisions,
                flow_editor_runtime_catalog,
                access.clone(),
            ))
            .merge(self.build_console_json_router_with_policy(decisions, visibility_policy))
            .merge(agent_events_sse_router_with_access_and_priming(
                Arc::new(move |agent_id| {
                    let runtime = agent_runtime.clone();
                    let identity_runtime = agent_identity_runtime.clone();
                    Box::pin(async move {
                        let agent_id =
                            crate::member_comms_id::runtime_alias_str(&agent_id).into_owned();
                        let handle = runtime.handle();
                        let target = if let Some(identity_runtime) = identity_runtime.as_ref() {
                            identity_runtime
                                .member_alias_lifecycle_target(&agent_id)
                                .await
                                .map_err(|error| {
                                    crate::MobRuntimeError::InvalidConfig(error.to_string())
                                })?
                        } else {
                            None
                        };
                        if let Some(target) = target {
                            let Some(authority_runtime) = identity_runtime.clone() else {
                                return Err(crate::MobRuntimeError::InvalidConfig(
                                    "identity event target resolved without identity runtime"
                                        .to_string(),
                                ));
                            };
                            let authority_identity = authority_runtime
                                .identity_for_member_mutation(&agent_id)
                                .await
                                .ok_or_else(|| {
                                    crate::MobRuntimeError::InvalidConfig(format!(
                                        "identity event target lost authority before subscription: {agent_id}"
                                    ))
                                })?;
                            let operation_agent_id = agent_id.clone();
                            return crate::identity_first::IdentityRuntime::run_member_alias_targets_operation_tracked(
                                vec![target],
                                move || async move {
                                    let identity_events = authority_runtime
                                        .subscribe(&authority_identity)
                                        .await
                                        .map_err(|error| error.to_string())?;
                                    let member_id = resolve_agent_event_member_id(
                                        &handle,
                                        &operation_agent_id,
                                    )
                                    .await;
                                    let expected_alias = crate::member_comms_id::runtime_alias_str(
                                        member_id.as_str(),
                                    )
                                    .into_owned();
                                    let event_stream = handle
                                        .subscribe_agent_events(&member_id)
                                        .await
                                        .map_err(|error| error.to_string())?;
                                    Ok(generation_authoritative_agent_event_stream(
                                        event_stream,
                                        identity_events,
                                        authority_runtime,
                                        authority_identity,
                                        expected_alias,
                                    ))
                                },
                            )
                            .await
                            .map_err(|error| {
                                crate::MobRuntimeError::InvalidConfig(error.to_string())
                            });
                        }
                        let member_id = resolve_agent_event_member_id(&handle, &agent_id).await;
                        handle.subscribe_agent_events(&member_id).await.map_err(Into::into)
                    })
                }),
                Some(sse_decisions_a),
                access.clone(),
                Some(self.mob_runtime.clone()),
                agent_sse_visibility_policy,
            ))
            .merge(mob_events_sse_router_with_access_and_priming(
                Arc::new(move || {
                    let mob_runtime = mob_runtime.clone();
                    Box::pin(async move { mob_runtime.handle().subscribe_mob_events().await })
                }),
                Some(sse_decisions_b),
                access.clone(),
                Some(self.mob_runtime.clone()),
                mob_sse_visibility_policy,
            ))
            .merge(mob_structural_events_sse_router_with_access_and_priming(
                self.mob_runtime.handle(),
                self.mob_events_store(),
                Some(sse_decisions_c),
                access,
                Some(self.mob_runtime.clone()),
                structural_sse_visibility_policy,
            ))
            .layer(ConcurrencyLimitLayer::new(
                DEFAULT_REFERENCE_APP_MAX_CONCURRENT_REQUESTS,
            ))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    use axum::http::{HeaderMap, header};
    use futures::StreamExt;
    use meerkat_client::TestClient;
    use meerkat_core::comms::EventStream;
    use meerkat_mob::{MobDefinition, MobRuntimeMode, ProfileName};

    use super::{
        DEFAULT_REFERENCE_APP_MAX_CONCURRENT_REQUESTS, generation_authoritative_agent_event_stream,
        healthz_response,
    };
    use crate::identity_first::{
        AgentAddressability, AgentIdentity, DurableAgentSpec, LocalContinuityStore,
        LocalLeaseProvider, MutableRosterProvider,
    };
    use crate::unified_runtime::UnifiedRuntimeBuilder;

    #[test]
    fn reference_router_default_concurrency_allows_sse_fanout() {
        assert_eq!(DEFAULT_REFERENCE_APP_MAX_CONCURRENT_REQUESTS, 1024);
    }

    #[tokio::test]
    async fn healthz_preserves_text_probe_and_negotiates_structured_jobs() {
        let projection = Arc::new(std::sync::RwLock::new(Some(serde_json::json!({
            "status": "ok",
            "detached_jobs": {
                "queued": 1,
                "running": 2,
                "awaiting_members": 1,
                "stale_leases": 0,
                "needs_attention": 0,
                "delivery_backlog": 0
            }
        }))));
        let text = healthz_response(HeaderMap::new(), Arc::clone(&projection)).await;
        assert_eq!(text.status(), axum::http::StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(text.into_body(), usize::MAX)
                .await
                .expect("text body"),
            "ok"
        );

        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, "application/json".parse().expect("accept"));
        let json = healthz_response(headers, projection).await;
        let body = axum::body::to_bytes(json.into_body(), usize::MAX)
            .await
            .expect("json body");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(body["detached_jobs"]["running"], 2);
    }

    #[tokio::test]
    async fn durable_agent_event_stream_terminates_when_reset_replaces_generation() {
        let definition = MobDefinition::from_toml(
            r#"
[mob]
id = "generation-authoritative-agent-events"

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "turn_driven"

[profiles.worker.tools]
comms = true
"#,
        )
        .expect("definition parses");
        let identity = AgentIdentity::parse("lead").expect("identity parses");
        let roster = Arc::new(MutableRosterProvider::new(vec![DurableAgentSpec {
            identity: identity.clone(),
            profile: ProfileName::from("worker"),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: BTreeMap::new(),
            context: None,
            additional_instructions: Vec::new(),
            initial_message: None,
            runtime_mode_override: Some(MobRuntimeMode::TurnDriven),
            backend: None,
            binding: None,
        }]));
        let scratch = tempfile::tempdir().expect("scratch dir");
        let runtime = UnifiedRuntimeBuilder::default()
            .definition(definition)
            .continuity_store(Arc::new(
                LocalContinuityStore::in_memory().expect("continuity store"),
            ))
            .lease_provider(Arc::new(LocalLeaseProvider::new()))
            .roster_provider(roster)
            .scratch_dir(scratch.path())
            .identity_runtime_instance_id("generation-authoritative-agent-events")
            .default_llm_client(Arc::new(TestClient::default()))
            .build()
            .await
            .expect("identity-first runtime builds");

        let identity_runtime = runtime
            .identity_runtime()
            .expect("identity runtime installed")
            .clone();
        let expected_alias = identity_runtime
            .status(&identity)
            .await
            .expect("identity active")
            .agent_runtime_id
            .expect("active runtime alias")
            .as_str()
            .to_string();
        let identity_events = identity_runtime
            .subscribe(&identity)
            .await
            .expect("identity event subscription");
        // A pending lower-plane stream proves termination comes from the
        // identity generation gate, not from old-member channel teardown.
        let lower_plane: EventStream = Box::pin(futures::stream::pending());
        let mut guarded = generation_authoritative_agent_event_stream(
            lower_plane,
            identity_events,
            identity_runtime.clone(),
            identity.clone(),
            expected_alias,
        );

        let replacement = identity_runtime
            .reset(&identity)
            .await
            .expect("identity reset succeeds");
        assert_eq!(replacement.generation.get(), 1);
        let next = tokio::time::timeout(Duration::from_secs(5), guarded.next())
            .await
            .expect("generation gate terminates promptly");
        assert!(
            next.is_none(),
            "stream opened on the prior generation must terminate after reset"
        );

        let _ = runtime.mob_handle().stop().await;
    }
}
