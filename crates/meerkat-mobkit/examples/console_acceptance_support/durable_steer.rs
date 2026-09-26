//! Fixture-only ingress into the public typed durable Steer API.
//! The runtime owns admission, boundary delivery, history, and completion.

use std::sync::Arc;

use axum::{Json, Router, extract::Query, http::StatusCode, routing::post};
use meerkat_core::lifecycle::{
    ConversationAppend, ConversationAppendRole, CoreRenderable, InputId,
};
use meerkat_core::{HandlingMode, SystemNoticeKind, types::SessionId};
use meerkat_mobkit::UnifiedRuntime;
use meerkat_runtime::service_ext::SessionServiceRuntimeExt;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoticeRequest {
    identity: String,
    session_id: SessionId,
    content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputQuery {
    session_id: SessionId,
    input_id: InputId,
}

fn response(result: Result<Value, String>) -> (StatusCode, Json<Value>) {
    match result {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))),
    }
}

fn machine(runtime: &UnifiedRuntime) -> Result<Arc<meerkat_runtime::MeerkatMachine>, String> {
    runtime
        .mob_runtime()
        .session_service()
        .and_then(|service| service.runtime_adapter())
        .ok_or_else(|| "fixture needs the actual session runtime adapter".into())
}

async fn submit(runtime: &UnifiedRuntime, request: NoticeRequest) -> Result<Value, String> {
    if !["router:main", "domain:delivery"].contains(&request.identity.as_str())
        || request.content.trim().is_empty()
    {
        return Err("an existing fixture identity and nonempty notice are required".into());
    }
    let member = meerkat_mobkit::member_comms_id::roster_member_id_for_identity(&request.identity);
    let actual_session = runtime
        .mob_handle()
        .resolve_bridge_session_id(&member)
        .await
        .ok_or("fixture member has no current session")?;
    if actual_session != request.session_id {
        return Err("the requested session is not the current fixture member session".into());
    }
    let mut prompt = meerkat_runtime::PromptInput::new(
        "",
        Some(
            meerkat_core::lifecycle::run_primitive::RuntimeTurnMetadata {
                handling_mode: Some(HandlingMode::Steer),
                ..Default::default()
            },
        ),
    );
    prompt.typed_turn_appends = vec![ConversationAppend {
        runtime_source: None,
        role: ConversationAppendRole::SystemNotice,
        content: CoreRenderable::SystemNotice {
            kind: SystemNoticeKind::Generic,
            body: Some(request.content),
            blocks: Vec::new(),
        },
        identity: None,
    }];
    let input = meerkat_runtime::Input::Prompt(prompt);
    let input_id = input.id().clone();
    let (outcome, _completion) = machine(runtime)?
        .accept_input_with_completion(&actual_session, input)
        .await
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "accepted": outcome.is_accepted(), "input_id": input_id,
        "identity": request.identity, "session_id": actual_session,
    }))
}

async fn inspect(runtime: &UnifiedRuntime, query: InputQuery) -> Result<Value, String> {
    let machine = machine(runtime)?;
    let state = machine
        .input_state(&query.session_id, &query.input_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("runtime input is absent")?;
    let completion = machine
        .input_terminal_completion(&query.session_id, &query.input_id)
        .await
        .map_err(|error| error.to_string())?;
    let snapshot = machine
        .meerkat_machine_archive_snapshot(&query.session_id)
        .await
        .ok_or("runtime snapshot is absent")?;
    Ok(json!({
        "input_id": query.input_id, "session_id": query.session_id,
        "phase": state.seed.phase, "run_id": state.seed.last_run_id,
        "terminal_outcome": state.seed.terminal_outcome, "completion": completion,
        "runtime_phase": snapshot.control.phase,
        "current_run_id": snapshot.control.current_run_id,
        "queue": snapshot.queue, "steer_queue": snapshot.steer_queue,
    }))
}

pub fn router(runtime: Arc<UnifiedRuntime>) -> Router {
    let reader = runtime.clone();
    Router::new().route(
        "/durable-steer",
        post(move |Json(request): Json<NoticeRequest>| {
            let runtime = runtime.clone();
            async move { response(submit(&runtime, request).await) }
        })
        .get(move |Query(query): Query<InputQuery>| {
            let runtime = reader.clone();
            async move { response(inspect(&runtime, query).await) }
        }),
    )
}
