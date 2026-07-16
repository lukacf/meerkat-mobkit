//! Server-Sent Events (SSE) streaming endpoints for agent and mob observation.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use futures::StreamExt;
use meerkat_core::AgentEvent;
use meerkat_core::comms::EventStream;
use meerkat_core::event::agent_event_type;
use meerkat_mob::{MobEventRouterHandle, MobHandle};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::access::{ACTION_AGENT_VIEW, ACTION_MOB_OBSERVE, AccessController, AccessView};
use crate::console_aggregator::{
    AllowAllConsoleVisibilityPolicy, ConsoleCursor, ConsoleFrame, ConsoleFrameSource,
    ConsoleFrameSourceKind, ConsoleFrameStatus, ConsoleVisibilityPolicy, NewConsoleFrame,
};
use crate::runtime::{RuntimeDecisionState, extract_bearer_token_from_header};
use crate::unified_runtime::EventQuery;
use crate::unified_runtime::mob_events::{MOB_EVENTS_STREAM_PATH, MobEventsStore};

use crate::mob_handle_runtime::{MobRuntime, MobRuntimeError};
use meerkat_core::comms::SendError;
use meerkat_core::service::SessionError;
use meerkat_mob::MobError;

pub(crate) const DEFAULT_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);
pub(crate) const KEEP_ALIVE_TEXT: &str = "keep-alive";

pub(crate) use crate::mob_handle_runtime::console_agent_event_payload;

pub fn agent_event_sse(interaction_id: &str, seq: u64, event: &AgentEvent) -> Event {
    let event_name = agent_event_name(event);
    let payload = serde_json::to_string(&console_agent_event_payload(event))
        .unwrap_or_else(|_| "{}".to_string());
    Event::default()
        .id(format!("{interaction_id}:{seq}"))
        .event(event_name)
        .data(payload)
}

fn agent_event_name(event: &AgentEvent) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|value| {
            value
                .as_object()
                .and_then(|object| object.get("type"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "agent_event".to_string())
}

fn http_error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({
            "error": message
        })),
    )
}

fn map_runtime_error(error: MobRuntimeError) -> (StatusCode, Json<Value>) {
    match error {
        MobRuntimeError::InvalidInput(message) => http_error(StatusCode::BAD_REQUEST, message),
        MobRuntimeError::Mob(
            MobError::MemberNotFound(_)
            | MobError::SessionError(SessionError::NotFound { .. })
            | MobError::CommsError(SendError::PeerNotFound(_)),
        ) => http_error(StatusCode::NOT_FOUND, "member_not_found"),
        MobRuntimeError::Mob(MobError::SessionError(SessionError::Unsupported(_))) => {
            http_error(StatusCode::UNPROCESSABLE_ENTITY, "unsupported")
        }
        _ => http_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error"),
    }
}

/// Apply the console's frame visibility/redaction contract to an SSE payload.
/// SSE remains a distinct transport, but it must not become a side door around
/// the policy already enforced by the console timeline.
fn project_sse_payload(
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    identity: &str,
    kind: &str,
    timestamp_ms: u64,
    payload: Value,
) -> Option<Value> {
    let mut projected = NewConsoleFrame {
        id: None,
        dedupe_key: format!("sse:{identity}:{kind}:{timestamp_ms}"),
        timestamp_ms,
        runtime_key: "sse".to_string(),
        identity: identity.to_string(),
        conversation_id: Some(identity.to_string()),
        session_id: None,
        kind: kind.to_string(),
        status: ConsoleFrameStatus::Completed,
        payload,
        source: ConsoleFrameSource {
            kind: ConsoleFrameSourceKind::Synthetic,
            source_cursor: None,
        },
        source_event_id: None,
        interaction_id: None,
        turn_id: None,
        run_id: None,
        parent_frame_id: None,
        caused_by_frame_id: None,
    };
    if let Some(redacted) = visibility_policy.redact_payload(&projected) {
        projected.payload = redacted;
        projected.status = ConsoleFrameStatus::Redacted;
    }
    let frame = ConsoleFrame {
        id: projected.dedupe_key.clone(),
        cursor: ConsoleCursor::from(projected.dedupe_key.as_str()),
        dedupe_key: projected.dedupe_key.clone(),
        timestamp_ms: projected.timestamp_ms,
        runtime_key: projected.runtime_key.clone(),
        identity: projected.identity.clone(),
        conversation_id: projected.conversation_id.clone(),
        session_id: projected.session_id.clone(),
        kind: projected.kind.clone(),
        status: projected.status,
        frame_version: 1,
        updated_at_ms: None,
        payload: projected.payload.clone(),
        source: projected.source.clone(),
        source_event_id: projected.source_event_id.clone(),
        interaction_id: projected.interaction_id.clone(),
        turn_id: projected.turn_id.clone(),
        run_id: projected.run_id.clone(),
        parent_frame_id: projected.parent_frame_id.clone(),
        caused_by_frame_id: projected.caused_by_frame_id.clone(),
    };
    visibility_policy
        .frame_visible(&frame)
        .then_some(projected.payload)
}

fn mob_event_authorization_alias(runtime_id: &meerkat_mob::ids::AgentRuntimeId) -> String {
    crate::member_comms_id::runtime_alias_str(runtime_id.identity.as_str()).into_owned()
}

// ---------------------------------------------------------------------------
// Tier 2: Per-agent persistent SSE  (MK-005)
// ---------------------------------------------------------------------------

pub type AgentEventSubscribeFuture =
    Pin<Box<dyn Future<Output = Result<EventStream, MobRuntimeError>> + Send>>;

pub type AgentEventSubscribeFn = Arc<dyn Fn(String) -> AgentEventSubscribeFuture + Send + Sync>;

#[derive(Clone)]
struct AgentSseState {
    subscribe_fn: AgentEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
    visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    /// Live mob runtime used to prime the access attribute cache at
    /// connection time (roster plus spawn-registered console metadata), so
    /// label/role/lineage rules resolve without a prior
    /// `/console/experience` call. `None` keeps the route behaviour unchanged.
    prime_runtime: Option<MobRuntime>,
}

pub fn agent_events_sse_router(
    subscribe_fn: AgentEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
) -> Router {
    agent_events_sse_router_with_access(subscribe_fn, decisions, None)
}

pub fn agent_events_sse_router_with_access(
    subscribe_fn: AgentEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
) -> Router {
    agent_events_sse_router_with_access_and_priming(
        subscribe_fn,
        decisions,
        access,
        None,
        Arc::new(AllowAllConsoleVisibilityPolicy),
    )
}

pub(crate) fn agent_events_sse_router_with_access_and_priming(
    subscribe_fn: AgentEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
    prime_runtime: Option<MobRuntime>,
    visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
) -> Router {
    Router::new()
        .route("/agents/{agent_id}/events", get(agent_events_sse_handler))
        .with_state(AgentSseState {
            subscribe_fn,
            decisions,
            access,
            visibility_policy,
            prime_runtime,
        })
}

async fn agent_events_sse_handler(
    State(state): State<AgentSseState>,
    headers: HeaderMap,
    uri: Uri,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let access_view = sse_access_context(
        state.decisions.as_ref(),
        state.access.as_ref(),
        &headers,
        &uri,
    )
    .map_err(|()| sse_unauthorized("agent events stream requires a valid auth token"))?;
    let agent_id = agent_id.trim().to_string();
    if agent_id.is_empty() {
        return Err(http_error(
            StatusCode::BAD_REQUEST,
            "agent_id must not be empty",
        ));
    }
    if let Err(message) =
        crate::member_comms_id::validate_public_member_alias("agent_id", &agent_id)
    {
        return Err(http_error(StatusCode::BAD_REQUEST, &message));
    }
    let policy_identity = if let Some(runtime) = state.prime_runtime.as_ref()
        && let Some((identity, visible)) = crate::http_console::sse_member_identity_visibility(
            &runtime.handle(),
            state.visibility_policy.as_ref(),
            &agent_id,
        )
        .await
    {
        if !visible {
            return Err(http_error(StatusCode::NOT_FOUND, "member_not_found"));
        }
        identity
    } else {
        agent_id.clone()
    };
    prime_sse_access_cache(state.prime_runtime.as_ref(), state.access.as_ref()).await;
    if access_view
        .as_ref()
        .is_some_and(|view| view.enforced() && !view.allows_agent(ACTION_AGENT_VIEW, &agent_id))
    {
        return Err(sse_access_denied(ACTION_AGENT_VIEW));
    }

    let event_stream = (state.subscribe_fn)(agent_id.clone())
        .await
        .map_err(map_runtime_error)?;
    let visibility_policy = state.visibility_policy;

    let stream = stream! {
        let mut seq = 0_u64;
        tokio::pin!(event_stream);
        while let Some(envelope) = event_stream.next().await {
            let event_name = agent_event_type(&envelope.payload).to_string();
            let Some(payload) = project_sse_payload(
                visibility_policy.as_ref(),
                &policy_identity,
                &event_name,
                envelope.timestamp_ms,
                console_agent_event_payload(&envelope.payload),
            ) else {
                continue;
            };
            let payload = serde_json::to_string(&payload)
                .unwrap_or_else(|_| "{}".to_string());
            yield Ok::<Event, Infallible>(
                Event::default()
                    .id(format!("{agent_id}:{seq}"))
                    .event(event_name)
                    .data(payload),
            );
            seq += 1;
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(DEFAULT_KEEP_ALIVE_INTERVAL)
            .text(KEEP_ALIVE_TEXT),
    ))
}

// ---------------------------------------------------------------------------
// Tier 3: Mob-merged SSE  (MK-006)
// ---------------------------------------------------------------------------

/// Meerkat 0.7: mob event-router subscription is fallible (machine command
/// faults surface as `MobError` instead of panicking inside the router).
pub type MobEventSubscribeFuture =
    Pin<Box<dyn Future<Output = Result<MobEventRouterHandle, meerkat_mob::MobError>> + Send>>;

pub type MobEventSubscribeFn = Arc<dyn Fn() -> MobEventSubscribeFuture + Send + Sync>;

#[derive(Clone)]
struct MobSseState {
    subscribe_fn: MobEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
    visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
    /// See [`AgentSseState::prime_runtime`].
    prime_runtime: Option<MobRuntime>,
}

pub fn mob_events_sse_router(
    subscribe_fn: MobEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
) -> Router {
    mob_events_sse_router_with_access(subscribe_fn, decisions, None)
}

pub fn mob_events_sse_router_with_access(
    subscribe_fn: MobEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
) -> Router {
    mob_events_sse_router_with_access_and_priming(
        subscribe_fn,
        decisions,
        access,
        None,
        Arc::new(AllowAllConsoleVisibilityPolicy),
    )
}

pub(crate) fn mob_events_sse_router_with_access_and_priming(
    subscribe_fn: MobEventSubscribeFn,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
    prime_runtime: Option<MobRuntime>,
    visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
) -> Router {
    Router::new()
        .route("/mob/events", get(mob_events_sse_handler))
        .with_state(MobSseState {
            subscribe_fn,
            decisions,
            access,
            visibility_policy,
            prime_runtime,
        })
}

/// Refresh the access attribute cache from the live roster and the spawn
/// registry before an SSE stream applies any per-agent filter, so
/// label/role/lineage rules resolve without depending on a prior
/// `/console/experience` call.
async fn prime_sse_access_cache(
    prime_runtime: Option<&MobRuntime>,
    access: Option<&AccessController>,
) {
    if let (Some(runtime), Some(controller)) = (
        prime_runtime,
        access.filter(|controller| controller.enabled()),
    ) {
        crate::http_console::prime_access_cache_from_runtime(runtime, controller).await;
    }
}

/// An enforced SSE view may authorize only identities whose current resource
/// attributes are known. This intentionally composes `knows_agent` with the
/// normal decision: label/role-scoped denies cannot match a cold identity, so
/// calling `can_view_agent` alone would turn missing attributes into an allow.
fn sse_access_allows_known_agent(view: Option<&AccessView>, identity: &str) -> bool {
    view.is_none_or(|view| view.knows_agent(identity) && view.can_view_agent(identity))
}

/// Apply the shared authorization and console-visibility projection for one
/// structural event envelope. Callers must prime the access cache first; an
/// enforced view then fails closed for identities whose attributes remain
/// unknown. Attributed historical events also fail closed when no live member
/// projection exists to evaluate the configured console policy.
pub(crate) async fn project_structural_envelope_for_console(
    handle: &MobHandle,
    visibility_policy: &dyn ConsoleVisibilityPolicy,
    access_view: Option<&AccessView>,
    mut envelope: crate::MobStructuralEventEnvelope,
) -> Option<crate::MobStructuralEventEnvelope> {
    let access_view = access_view.filter(|view| view.enforced());
    let policy_identity = if let Some(identity) = envelope.agent_identity.as_deref() {
        if !sse_access_allows_known_agent(access_view, identity) {
            return None;
        }
        let (console_identity, visible) = crate::http_console::sse_member_identity_visibility(
            handle,
            visibility_policy,
            identity,
        )
        .await?;
        if !visible {
            return None;
        }
        console_identity
    } else {
        crate::console_contracts::SYSTEM_EVENT_IDENTITY.to_string()
    };
    envelope.data = project_sse_payload(
        visibility_policy,
        &policy_identity,
        &envelope.kind,
        envelope.timestamp_ms,
        envelope.data,
    )?;
    Some(envelope)
}

async fn mob_events_sse_handler(
    State(state): State<MobSseState>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let access_view = sse_access_context(
        state.decisions.as_ref(),
        state.access.as_ref(),
        &headers,
        &uri,
    )
    .map_err(|()| sse_unauthorized("mob events stream requires a valid auth token"))?;
    prime_sse_access_cache(state.prime_runtime.as_ref(), state.access.as_ref()).await;
    // `mob.observe` gates access to the merged stream surface. The events
    // flowing through it carry the same rich per-agent payload as
    // `/agents/{id}/events`, so each one is still filtered by `agent.view`
    // on its source — otherwise a `mob.observe` grant would silently defeat
    // a per-agent view denial. "Observe everything" is expressed by also
    // granting `agent.view` on `*`.
    if access_view
        .as_ref()
        .is_some_and(|view| view.enforced() && !view.allows(ACTION_MOB_OBSERVE))
    {
        return Err(sse_access_denied(ACTION_MOB_OBSERVE));
    }
    let stream_view = access_view.filter(AccessView::enforced);
    // Captured so the long-lived stream can re-prime the shared attribute cache
    // for members spawned AFTER the one-time subscribe prime (the view reads
    // attributes through the controller's shared cache).
    let reprime_runtime = state.prime_runtime.clone();
    let reprime_access = state.access.clone();
    let visibility_runtime = state.prime_runtime.clone();
    let visibility_policy = state.visibility_policy;
    let mut router_handle = (state.subscribe_fn)().await.map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("mob event subscription failed: {err}")})),
        )
    })?;

    let stream = stream! {
        let mut seq = 0_u64;
        // Roster role/labels are immutable for one concrete member generation.
        // Re-prime on the first event and whenever the same public alias moves
        // to a new generated runtime source, so a respawn cannot inherit stale
        // authorization attributes from the previous embodiment.
        let mut authorized_source_by_alias = std::collections::HashMap::<String, String>::new();
        while let Some(attributed) = router_handle.event_rx.recv().await {
            // Decode the comms-safe roster member id back to the public
            // alias space: SDK `EventStream` consumers filter by alias, and
            // fail-closed per-agent ABAC view rules are written against
            // aliases — an encoded id would silently drop both.
            let source = crate::member_comms_id::runtime_event_alias(&attributed.source);
            // ABAC speaks the member/durable alias, not meerkat's upstream
            // `{member}:{generation}` runtime id. The latter is still kept in
            // the public event payload for stream ordering/debugging, but
            // policy evaluation must not append a second generation to an
            // identity-first runtime alias.
            let authorization_alias = mob_event_authorization_alias(&attributed.source);
            let concrete_source = attributed.source.to_string();
            let source_changed = authorized_source_by_alias
                .insert(authorization_alias.clone(), concrete_source.clone())
                .as_deref()
                != Some(concrete_source.as_str());
            if stream_view.is_some() && source_changed {
                prime_sse_access_cache(reprime_runtime.as_ref(), reprime_access.as_ref()).await;
            }
            // Historical/live-tail events can outlive the operational roster
            // entry that supplied their role and labels. A broad allow plus a
            // label/role-scoped deny fails open when those attributes are
            // absent, so an identity that is still unknown after the single
            // re-prime is not safe to disclose.
            if !sse_access_allows_known_agent(stream_view.as_ref(), &authorization_alias) {
                continue;
            }
            let policy_identity = if let Some(runtime) = visibility_runtime.as_ref() {
                let Some((identity, visible)) =
                    crate::http_console::sse_member_identity_visibility(
                        &runtime.handle(),
                        visibility_policy.as_ref(),
                        &authorization_alias,
                    )
                    .await
                else {
                    // Visibility policies are roster projections. Once the
                    // attributed member is gone we no longer have the record
                    // needed to prove it visible, so replay must fail closed.
                    continue;
                };
                if !visible {
                    continue;
                }
                identity
            } else {
                authorization_alias.clone()
            };
            let event_name = agent_event_type(&attributed.envelope.payload).to_string();
            let Some(payload) = project_sse_payload(
                visibility_policy.as_ref(),
                &policy_identity,
                &event_name,
                attributed.envelope.timestamp_ms,
                console_agent_event_payload(&attributed.envelope.payload),
            ) else {
                continue;
            };
            let data = json!({
                "member_id": &source,
                "source": &source,
                "payload": payload,
            });
            yield Ok::<Event, Infallible>(
                Event::default()
                    .id(format!("mob:{seq}"))
                    .event(event_name)
                    .data(data.to_string()),
            );
            seq += 1;
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(DEFAULT_KEEP_ALIVE_INTERVAL)
            .text(KEEP_ALIVE_TEXT),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{AccessControlConfig, AccessEffect, AccessRule, AgentResourceAttributes};
    use std::collections::BTreeMap;

    struct RedactAndFilterPolicy;

    impl ConsoleVisibilityPolicy for RedactAndFilterPolicy {
        fn frame_visible(&self, frame: &ConsoleFrame) -> bool {
            frame.kind != "hidden"
        }

        fn redact_payload(&self, _frame: &NewConsoleFrame) -> Option<Value> {
            Some(json!({"redacted": true}))
        }
    }

    #[test]
    fn sse_projection_applies_redaction_and_frame_filtering() {
        let policy = RedactAndFilterPolicy;
        assert_eq!(
            project_sse_payload(&policy, "agent", "visible", 1, json!({"secret": true})),
            Some(json!({"redacted": true}))
        );
        assert_eq!(
            project_sse_payload(&policy, "agent", "hidden", 1, json!({"secret": true})),
            None
        );
    }

    #[test]
    fn enforced_sse_access_rejects_unknown_agent_before_label_deny_can_fail_open()
    -> Result<(), crate::access::AccessConfigError> {
        let controller = AccessController::new(AccessControlConfig {
            enabled: true,
            admins: vec!["root@example.test".to_string()],
            rules: vec![
                AccessRule {
                    id: "view-all".to_string(),
                    actions: vec![ACTION_AGENT_VIEW.to_string()],
                    agents: vec!["*".to_string()],
                    ..AccessRule::default()
                },
                AccessRule {
                    id: "deny-secret".to_string(),
                    effect: AccessEffect::Deny,
                    actions: vec![ACTION_AGENT_VIEW.to_string()],
                    match_labels: BTreeMap::from([("org".to_string(), "secret".to_string())]),
                    ..AccessRule::default()
                },
            ],
            ..AccessControlConfig::default()
        })?;
        let view = controller.view_for_subject(None);

        // The broad allow wins if the cold cache is evaluated directly. Both
        // merged and structural SSE call the helper after their one re-prime,
        // so a completed member that remains unknown is instead denied.
        assert!(view.can_view_agent("historical-secret"));
        assert!(!sse_access_allows_known_agent(
            Some(&view),
            "historical-secret"
        ));

        controller.record_agent_attributes(AgentResourceAttributes {
            identity: "historical-secret".to_string(),
            agent_id: Some("historical-secret".to_string()),
            role: Some("lead".to_string()),
            labels: BTreeMap::from([("org".to_string(), "secret".to_string())]),
        });
        assert!(!sse_access_allows_known_agent(
            Some(&view),
            "historical-secret"
        ));
        Ok(())
    }

    #[test]
    fn mob_event_authorization_uses_decoded_alias_without_upstream_generation() {
        let identity = crate::member_comms_id::mob_member_id("rt:identity:secret:0");
        let runtime_id =
            meerkat_mob::ids::AgentRuntimeId::new(identity, meerkat_mob::ids::Generation::new(7));

        assert_eq!(
            mob_event_authorization_alias(&runtime_id),
            "rt:identity:secret:0"
        );
        assert_eq!(
            crate::member_comms_id::runtime_event_alias(&runtime_id),
            "rt:identity:secret:0:7"
        );
    }
}

// ---------------------------------------------------------------------------
// Structural mob events: per-client meerkat ledger subscription
// ---------------------------------------------------------------------------

/// Query parameters for `/mobkit/mob_events/stream`. Mirrors
/// [`EventQuery`] for the field filters; cursor pagination is `after_seq`.
#[derive(Debug, Default, Deserialize)]
pub struct MobStructuralStreamQuery {
    #[serde(default)]
    pub after_seq: Option<u64>,
    #[serde(default)]
    pub mob_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub identity: Option<String>,
    #[serde(default)]
    pub member_id: Option<String>,
    /// Comma-separated list of event-kind labels to keep
    /// (e.g. `flow_started,step_completed`). Empty / absent = all.
    #[serde(default)]
    pub event_types: Option<String>,
    #[serde(default)]
    pub since_ms: Option<u64>,
    #[serde(default)]
    pub until_ms: Option<u64>,
}

impl MobStructuralStreamQuery {
    fn into_event_query(self) -> EventQuery {
        EventQuery {
            since_ms: self.since_ms,
            until_ms: self.until_ms,
            member_id: self.member_id,
            identity: self.identity,
            mob_id: self.mob_id,
            run_id: self.run_id,
            step_id: self.step_id,
            event_types: self
                .event_types
                .map(|raw| {
                    raw.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            limit: None,
            after_seq: self.after_seq,
        }
    }
}

#[derive(Clone)]
struct MobStructuralSseState {
    handle: MobHandle,
    store: MobEventsStore,
    /// See [`AgentSseState::prime_runtime`]. `None` falls back to a
    /// roster-only prime from `handle`.
    prime_runtime: Option<MobRuntime>,
    /// Optional auth context. When `Some`, requests are gated by the
    /// same `require_app_auth` toggle the console RPC route uses; when
    /// `None`, the route is unauthenticated (in-process or trusted
    /// embedding).
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
    visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
}

/// Per-client SSE subscription to the meerkat structural-event ledger.
///
/// Each connection opens its own `MobEventsView::subscribe_after` so
/// catch-up and live tail share the same ordered stream and there is
/// no race window between snapshot and live subscription. Stale
/// cursors are rejected with HTTP 410 Gone before any SSE handshake.
/// Filtering matches the `mobkit/mob_events/query` predicate; the
/// per-client `MobEventsSubscription` is dropped when the client
/// disconnects, which cancels the upstream forwarder automatically.
///
/// When `decisions` is `Some` and `decisions.console.require_app_auth`
/// is on, every request must carry a valid bearer token (Authorization
/// header or `auth_token` query param) — same gate the console RPC
/// route uses. `None` opts out of auth (e.g. trusted local embedding).
pub fn mob_structural_events_sse_router(
    handle: MobHandle,
    store: MobEventsStore,
    decisions: Option<RuntimeDecisionState>,
) -> Router {
    mob_structural_events_sse_router_with_access(handle, store, decisions, None)
}

pub fn mob_structural_events_sse_router_with_access(
    handle: MobHandle,
    store: MobEventsStore,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
) -> Router {
    mob_structural_events_sse_router_with_access_and_priming(
        handle,
        store,
        decisions,
        access,
        None,
        Arc::new(AllowAllConsoleVisibilityPolicy),
    )
}

pub(crate) fn mob_structural_events_sse_router_with_access_and_priming(
    handle: MobHandle,
    store: MobEventsStore,
    decisions: Option<RuntimeDecisionState>,
    access: Option<AccessController>,
    prime_runtime: Option<MobRuntime>,
    visibility_policy: Arc<dyn ConsoleVisibilityPolicy>,
) -> Router {
    Router::new()
        .route(
            MOB_EVENTS_STREAM_PATH,
            get(mob_structural_events_sse_handler),
        )
        .with_state(MobStructuralSseState {
            handle,
            store,
            prime_runtime,
            decisions,
            access,
            visibility_policy,
        })
}

/// Shared auth gate for every SSE route in mobkit. When `decisions` is
/// `Some(_)` and `decisions.console.require_app_auth` is on, the
/// request must carry a valid bearer / `auth_token` token; otherwise
/// the route is open. Used by `mob_structural_events_sse_router`,
/// `interaction_stream_router`, and the agent-/mob-event tier 2/3
/// routers.
///
/// On success returns the caller's [`AccessView`] when an
/// [`AccessController`] is wired (anonymous view on open routes), so the
/// SSE handlers can apply per-agent ABAC checks. `Err(())` means 401.
pub(crate) fn sse_access_context(
    decisions: Option<&RuntimeDecisionState>,
    access: Option<&AccessController>,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<Option<AccessView>, ()> {
    let Some(decisions) = decisions else {
        return Ok(access.map(|controller| controller.view_for_subject(None)));
    };
    let bearer_token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(extract_bearer_token_from_header)
        .map(String::from);
    // Parse the query string with `form_urlencoded` so percent-encoded
    // tokens (e.g. base64 padding `=` re-encoded as `%3D`) decode
    // correctly, and so substring-shadowing values like `xauth_token=`
    // don't masquerade as `auth_token=`.
    let query_token = uri.query().and_then(|q| {
        form_urlencoded::parse(q.as_bytes())
            .find(|(key, _)| key == "auth_token")
            .map(|(_, value)| value.into_owned())
    });
    let token = bearer_token.or(query_token);
    if !decisions.console.require_app_auth {
        // Open route: identify callers that volunteered a valid token so
        // per-user ABAC grants apply; everyone else is anonymous.
        let subject = token.as_deref().and_then(|token| {
            crate::runtime::resolve_authorized_console_auth_from_token(decisions, token)
                .map(|auth| auth.email)
        });
        return Ok(access.map(|controller| controller.view_for_subject(subject.as_deref())));
    }
    let token = token.ok_or(())?;
    let auth =
        crate::runtime::resolve_authorized_console_auth_from_token(decisions, &token).ok_or(())?;
    Ok(access.map(|controller| controller.view_for_subject(Some(auth.email.as_str()))))
}

fn sse_unauthorized(reason: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "unauthorized",
            "reason": reason,
        })),
    )
}

fn sse_access_denied(action: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "access_denied",
            "action": action,
        })),
    )
}

async fn mob_structural_events_sse_handler(
    State(state): State<MobStructuralSseState>,
    headers: HeaderMap,
    uri: Uri,
    Query(params): Query<MobStructuralStreamQuery>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<Value>)>
{
    let access_view = sse_access_context(
        state.decisions.as_ref(),
        state.access.as_ref(),
        &headers,
        &uri,
    )
    .map_err(|()| sse_unauthorized("mob_events stream requires a valid auth token"))?;
    if state.prime_runtime.is_some() {
        prime_sse_access_cache(state.prime_runtime.as_ref(), state.access.as_ref()).await;
    } else if let Some(controller) = state
        .access
        .as_ref()
        .filter(|controller| controller.enabled())
    {
        crate::http_console::prime_access_cache_from_handle(&state.handle, controller).await;
    }
    // Structural events span the whole mob: require the mob-wide
    // observation grant, mirroring `mobkit/mob_events/query`. Envelopes
    // attributed to a specific agent are additionally filtered by
    // `agent.view` on that agent (below), so `mob.observe` cannot surface
    // the lifecycle of an agent the caller is denied. Mob-level envelopes
    // with no agent attribution flow under `mob.observe` alone.
    if access_view
        .as_ref()
        .is_some_and(|view| view.enforced() && !view.allows(ACTION_MOB_OBSERVE))
    {
        return Err(sse_access_denied(ACTION_MOB_OBSERVE));
    }
    let stream_view = access_view.filter(AccessView::enforced);
    let query = params.into_event_query();
    let events_view = state.handle.events();

    let latest = events_view.latest_cursor().await.map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "events_view_unavailable",
                "detail": err.to_string(),
            })),
        )
    })?;

    if let Some(after_seq) = query.after_seq
        && after_seq > latest
    {
        return Err((
            StatusCode::GONE,
            Json(json!({
                "error": "event_query_stale",
                "after_cursor": after_seq,
                "latest_cursor": latest,
            })),
        ));
    }

    let after_cursor = query.after_seq.unwrap_or(latest);
    let mut subscription = events_view
        .subscribe_after(after_cursor)
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "subscribe_failed",
                    "detail": err.to_string(),
                })),
            )
        })?;

    let store = state.store;
    // Captured so the long-lived stream can re-prime the shared attribute cache
    // for members spawned AFTER the one-time subscribe prime.
    let reprime_runtime = state.prime_runtime.clone();
    let reprime_handle = state.handle.clone();
    let reprime_access = state.access.clone();
    let visibility_handle = state.handle.clone();
    let visibility_policy = state.visibility_policy;
    let stream = stream! {
        while let Some(event) = subscription.event_rx.recv().await {
            let envelope = store.project_event_for_query(&event).await;
            if !crate::unified_runtime::mob_events::envelope_matches(&envelope, &query) {
                continue;
            }
            // Agent-attributed structural events are gated by `agent.view`
            // on their agent; mob-level events (no attribution) pass on the
            // `mob.observe` grant alone.
            if envelope.agent_identity.is_some() {
                // Structural envelopes carry the durable alias but not the
                // concrete member generation. Refresh the live projection for
                // every attributed event before deciding; the subsequent
                // visibility lookup fails closed if the alias disappeared.
                if stream_view.is_some() {
                    if reprime_runtime.is_some() {
                        prime_sse_access_cache(reprime_runtime.as_ref(), reprime_access.as_ref())
                            .await;
                    } else if let Some(controller) = reprime_access
                        .as_ref()
                        .filter(|controller| controller.enabled())
                    {
                        crate::http_console::prime_access_cache_from_handle(
                            &reprime_handle,
                            controller,
                        )
                        .await;
                    }
                }
            }
            let Some(envelope) = project_structural_envelope_for_console(
                &visibility_handle,
                visibility_policy.as_ref(),
                stream_view.as_ref(),
                envelope,
            )
            .await else {
                continue;
            };
            let payload = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
            yield Ok::<Event, Infallible>(
                Event::default()
                    .id(format!("mob-evt-{}", envelope.cursor))
                    .event(envelope.kind.clone())
                    .data(payload),
            );
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(DEFAULT_KEEP_ALIVE_INTERVAL)
            .text(KEEP_ALIVE_TEXT),
    ))
}
