//! Scripted model choices over the real runtime tool dispatcher.
//!
//! This client records requests and emits tool calls. Only the runtime creates
//! tool results, WorkGraph revisions, peer receipts, and image artifacts.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use meerkat_client::types::LlmStream;
use meerkat_client::{LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::{Message, Provider, StopReason, TurnUsage, Usage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Notify;

/// Opt-in ordering control for a real peer turn. It only blocks the scripted
/// model; delivery, tool execution, and console projection stay runtime-owned.
#[derive(Default)]
pub struct ModelBarrier {
    current: Mutex<Option<Arc<BarrierRun>>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BarrierPlan {
    id: String,
    match_text: String,
    source: String,
}

struct BarrierRun {
    plan: BarrierPlan,
    progress: Mutex<BarrierProgress>,
    release: Notify,
}

#[derive(Default)]
struct BarrierProgress {
    entered: bool,
    released: bool,
    requests: usize,
}

impl ModelBarrier {
    pub fn control(&self, command: &Value) -> Result<Value, String> {
        match command.get("action").and_then(Value::as_str) {
            Some("arm") => {
                let plan: BarrierPlan = serde_json::from_value(
                    command.get("plan").cloned().ok_or("missing barrier plan")?,
                )
                .map_err(|error| error.to_string())?;
                if plan.id.is_empty()
                    || plan.id.len() > 48
                    || !plan
                        .id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                    || plan.match_text.is_empty()
                    || plan.source.is_empty()
                {
                    return Err("barrier needs a valid id, nonempty match_text and source".into());
                }
                let mut current = self
                    .current
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if current.is_some() {
                    return Err("this fixture already has an armed barrier".into());
                }
                *current = Some(Arc::new(BarrierRun {
                    plan,
                    progress: Mutex::new(BarrierProgress::default()),
                    release: Notify::new(),
                }));
                Ok(current.as_ref().unwrap().snapshot())
            }
            Some("status") | Some("release") => {
                let current = self
                    .current
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let run = current.as_ref().ok_or("no barrier is armed")?;
                if command.get("id").and_then(Value::as_str) != Some(run.plan.id.as_str()) {
                    return Err("barrier id does not match".into());
                }
                if command["action"] == "release" {
                    run.progress
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .released = true;
                    run.release.notify_waiters();
                }
                Ok(run.snapshot())
            }
            _ => Err("barrier action must be arm, status or release".into()),
        }
    }

    fn matching_request(&self, messages: &[Message]) -> Option<(Arc<BarrierRun>, usize)> {
        let start = messages
            .iter()
            .rposition(|message| matches!(message, Message::User(_)))?;
        let Message::User(user) = &messages[start] else {
            return None;
        };
        let current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let run = current.as_ref()?;
        user.text_content()
            .contains(&run.plan.match_text)
            .then(|| (run.clone(), start))
    }
}

impl BarrierRun {
    fn snapshot(&self) -> Value {
        let progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        json!({
            "id": self.plan.id, "match_text": self.plan.match_text,
            "phase": if progress.released { "released" } else if progress.entered { "entered" } else { "armed" },
            "entered": progress.entered, "released": progress.released, "requests": progress.requests,
        })
    }

    async fn wait_for_release(&self) {
        {
            let mut progress = self
                .progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            progress.entered = true;
            progress.requests += 1;
        }
        loop {
            let released = self.release.notified();
            tokio::pin!(released);
            // Register before reading the flag so release cannot be lost in
            // the gap between checking state and awaiting the notification.
            released.as_mut().enable();
            if self
                .progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .released
            {
                return;
            }
            released.await;
        }
    }
}

fn barrier_turn(plan: &BarrierPlan, messages: &[Message]) -> Result<Turn, String> {
    let id = format!("fixture-{}-peer-ready", plan.id);
    for message in messages.iter().rev() {
        if let Message::ToolResults { results, .. } = message
            && let Some(result) = results.iter().find(|result| result.tool_use_id == id)
        {
            let value: Value =
                serde_json::from_str(&result.text_content()).map_err(|error| error.to_string())?;
            if result.is_error || !value.get("items").is_some_and(Value::is_array) {
                return Err(format!("barrier peer ready tool failed: {value}"));
            }
            return Ok(Turn::Text(plan.source.clone()));
        }
    }
    if messages.iter().any(|message| matches!(message, Message::BlockAssistant(assistant) if assistant.tool_calls().any(|call| call.id == id))) {
        return Err("barrier peer ready tool has no runtime result".into());
    }
    Ok(Turn::Tools(vec![PlannedCall {
        id,
        name: "workgraph_ready",
        args: json!({ "labels": [format!("fixture-{}", plan.id)] }),
    }]))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelPlan {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default = "default_chunk_chars")]
    pub chunk_chars: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<ScenarioPlan>,
}

fn default_chunk_chars() -> usize {
    32
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioKind {
    Workgraph,
    Peer,
    Image,
}

/// A unique run id also supplies the explicit trigger in the user message:
/// `[fixture:RUN_ID]`. Ordinary and incoming peer turns never trigger a script.
/// The host must keep this plan stable until that interaction finishes.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioPlan {
    pub kind: ScenarioKind,
    pub run_id: String,
    #[serde(default)]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub peer_id: Option<String>,
    #[serde(default)]
    pub peer_body: Option<String>,
    #[serde(default)]
    pub image_provider: Option<String>,
    #[serde(default)]
    pub image_prompt: Option<String>,
}

impl ScenarioPlan {
    pub fn trigger(&self) -> String {
        format!("[fixture:{}]", self.run_id)
    }

    fn call_id(&self, step: &str) -> String {
        format!("fixture-{}-{step}", self.run_id)
    }

    fn label(&self) -> String {
        format!("fixture-{}", self.run_id)
    }

    fn validate(&self) -> Result<(), String> {
        if self.run_id.is_empty()
            || self.run_id.len() > 48
            || !self
                .run_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        {
            return Err(
                "run_id must have 1-48 ASCII letters, digits, underscores, or hyphens".into(),
            );
        }
        Ok(())
    }
}

pub struct RecordingClient {
    pub plan: Arc<Mutex<ModelPlan>>,
    pub requests: Arc<Mutex<Vec<Value>>>,
    pub barrier: Arc<ModelBarrier>,
}

impl RecordingClient {
    pub fn new(
        plan: Arc<Mutex<ModelPlan>>,
        requests: Arc<Mutex<Vec<Value>>>,
        barrier: Arc<ModelBarrier>,
    ) -> Self {
        Self {
            plan,
            requests,
            barrier,
        }
    }
}

#[async_trait::async_trait]
impl LlmClient for RecordingClient {
    fn provider(&self) -> Provider {
        Provider::Other
    }

    fn project_replay_messages(&self, messages: &[Message]) -> Result<Vec<Message>, LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(&'a self, request: &'a LlmRequest) -> LlmStream<'a> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(serde_json::to_value(request).unwrap_or(Value::Null));
        let plan = self
            .plan
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let barrier = self.barrier.matching_request(&request.messages);
        let turn = if let Some((run, start)) = &barrier {
            barrier_turn(&run.plan, &request.messages[*start..])
                .unwrap_or_else(|error| Turn::Text(format!("Acceptance scenario stopped: {error}")))
        } else if matches!(request.messages.last(), Some(Message::User(user))
            if user.text_content().contains("You have been spawned as"))
        {
            // The runtime retries an empty successful response. Bootstrap must
            // finish with visible output before the member can accept work.
            Turn::Text("Ready.".into())
        } else if let Some((scenario, active)) = plan.scenario.as_ref().and_then(|scenario| {
            active_messages(scenario, &request.messages).map(|active| (scenario, active))
        }) {
            scenario_turn(scenario, active)
                .unwrap_or_else(|error| Turn::Text(format!("Acceptance scenario stopped: {error}")))
        } else {
            Turn::Text(plan.source.clone())
        };
        let provider = meerkat_models::infer_provider(&request.model).unwrap_or(self.provider());
        let usage = TurnUsage::host_declared(provider, &request.model, Usage::default());
        Box::pin(async_stream::stream! {
            if let Some((barrier, _)) = barrier {
                barrier.wait_for_release().await;
            }
            let stop_reason = match turn {
                Turn::Text(text) => {
                    let chars: Vec<char> = text.chars().collect();
                    for chunk in chars.chunks(plan.chunk_chars.clamp(1, 4096)) {
                        if plan.delay_ms > 0 {
                            tokio::time::sleep(Duration::from_millis(plan.delay_ms.min(1000))).await;
                        }
                        yield Ok(LlmEvent::TextDelta { delta: chunk.iter().collect(), meta: None });
                    }
                    StopReason::EndTurn
                }
                Turn::Tools(calls) => {
                    for call in calls {
                        yield Ok(LlmEvent::ToolCallComplete {
                            id: call.id, name: call.name.into(), args: call.args, meta: None,
                        });
                    }
                    StopReason::ToolUse
                }
            };
            yield Ok(LlmEvent::UsageUpdate { usage });
            yield Ok(LlmEvent::Done { outcome: LlmDoneOutcome::Success { stop_reason }});
        })
    }

    async fn health_check(&self) -> Result<(), LlmError> {
        Ok(())
    }
}

#[derive(Debug)]
enum Turn {
    Text(String),
    Tools(Vec<PlannedCall>),
}

#[derive(Debug)]
struct PlannedCall {
    id: String,
    name: &'static str,
    args: Value,
}

fn active_messages<'a>(scenario: &ScenarioPlan, messages: &'a [Message]) -> Option<&'a [Message]> {
    let start = messages.iter().rposition(|message| {
        matches!(message, Message::User(user) if user.transcript_role.is_conversational())
    })?;
    let Message::User(user) = &messages[start] else {
        return None;
    };
    // Prefix matching prevents a peer quoting a trigger later in its body from
    // accidentally becoming the operator's scripted request.
    user.text_content()
        .trim_start()
        .starts_with(&scenario.trigger())
        .then_some(&messages[start..])
}

struct RecordedTurn<'a> {
    scenario: &'a ScenarioPlan,
    messages: &'a [Message],
}

impl RecordedTurn<'_> {
    fn result(&self, step: &str) -> Result<Option<Value>, String> {
        let id = self.scenario.call_id(step);
        for message in self.messages.iter().rev() {
            if let Message::ToolResults { results, .. } = message {
                if let Some(result) = results.iter().find(|result| result.tool_use_id == id) {
                    let text = result.text_content();
                    if result.is_error {
                        return Err(format!("{step}: {text}"));
                    }
                    let value: Value = serde_json::from_str(&text).map_err(|error| {
                        format!("{step} returned invalid JSON: {error}; {text}")
                    })?;
                    if !value.is_object()
                        || value.get("error").is_some_and(|error| !error.is_null())
                    {
                        return Err(format!("{step} returned an unexpected result: {value}"));
                    }
                    return Ok(Some(value));
                }
            }
        }
        if self.messages.iter().any(|message| {
            matches!(message,
            Message::BlockAssistant(assistant) if assistant.tool_calls().any(|call| call.id == id))
        }) {
            return Err(format!(
                "{step} was already requested without a recorded result; refusing to repeat it"
            ));
        }
        Ok(None)
    }

    fn call(&self, step: &str, name: &'static str, args: Value) -> PlannedCall {
        PlannedCall {
            id: self.scenario.call_id(step),
            name,
            args,
        }
    }

    fn item(&self, step: &str, expected_id: Option<&str>) -> Result<ObservedItem, String> {
        let value = self
            .result(step)?
            .ok_or_else(|| format!("{step} has no recorded result"))?;
        let item = value
            .get("item")
            .ok_or_else(|| format!("{step} has no item: {value}"))?;
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| format!("{step} has no item id: {value}"))?;
        if expected_id.is_some_and(|expected| expected != id) {
            return Err(format!("{step} returned another item: {id}"));
        }
        Ok(ObservedItem {
            id: id.to_owned(),
            revision: item
                .get("revision")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("{step} has no revision: {value}"))?,
            status: item
                .get("status")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{step} has no status: {value}"))?
                .to_owned(),
        })
    }

    fn pending(&self, specs: &[(&str, &'static str, Value)]) -> Result<Option<Turn>, String> {
        let mut calls = Vec::new();
        for (step, name, args) in specs {
            if self.result(step)?.is_none() {
                calls.push(self.call(step, name, args.clone()));
            }
        }
        Ok((!calls.is_empty()).then_some(Turn::Tools(calls)))
    }
}

struct ObservedItem {
    id: String,
    revision: u64,
    status: String,
}

fn required<'a>(value: &'a Option<String>, field: &str) -> Result<&'a str, String> {
    value
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("host must configure {field}"))
}

fn scenario_turn(scenario: &ScenarioPlan, messages: &[Message]) -> Result<Turn, String> {
    scenario.validate()?;
    let recorded = RecordedTurn { scenario, messages };
    match scenario.kind {
        ScenarioKind::Workgraph => workgraph_turn(&recorded),
        ScenarioKind::Peer => peer_turn(&recorded),
        ScenarioKind::Image => image_turn(&recorded),
    }
}

fn workgraph_turn(recorded: &RecordedTurn<'_>) -> Result<Turn, String> {
    let scenario = recorded.scenario;
    let owner_id = required(
        &scenario.owner_id,
        "owner_id (canonical WorkGraph agent owner)",
    )?;
    let create = |title: &str, description: &str| {
        json!({
            "title": title, "description": description, "labels": [scenario.label()],
            "completion_policy": {"kind": "self_attest"},
        })
    };
    if let Some(turn) = recorded.pending(&[
        ("create-review", "workgraph_create", create("Review source change", "Inspect the source change and uploaded release diagram.")),
        ("create-render", "workgraph_create", create("Review release badge", "Inspect the release badge before publishing.")),
        ("create-publish", "workgraph_create", create("Publish release candidate", "Both review prerequisites must complete first. This fixture item does not deploy anything.")),
    ])? { return Ok(turn); }

    let review = recorded.item("create-review", None)?;
    let render = recorded.item("create-render", None)?;
    let publish = recorded.item("create-publish", None)?;
    if review.id == render.id || review.id == publish.id || render.id == publish.id {
        return Err("create returned duplicate item ids".into());
    }
    if let Some(turn) = recorded.pending(&[
        (
            "link-review",
            "workgraph_link",
            json!({"kind": "blocks", "from_id": review.id, "to_id": publish.id}),
        ),
        (
            "link-render",
            "workgraph_link",
            json!({"kind": "blocks", "from_id": render.id, "to_id": publish.id}),
        ),
    ])? {
        return Ok(turn);
    }
    if let Some(turn) = recorded.pending(&[
        (
            "ready-before",
            "workgraph_ready",
            json!({"labels": [scenario.label()]}),
        ),
        ("get-review", "workgraph_get", json!({"id": review.id})),
        ("get-render", "workgraph_get", json!({"id": render.id})),
    ])? {
        return Ok(turn);
    }
    let ready_before = recorded
        .result("ready-before")?
        .ok_or("missing ready-before result")?;
    let ready = item_ids(&ready_before)?;
    if ready.contains(&publish.id.as_str()) {
        return Err("dependent item was ready before its prerequisites completed".into());
    }
    if !ready.contains(&review.id.as_str()) || !ready.contains(&render.id.as_str()) {
        return Err("unclaimed prerequisite items were absent from the ready result".into());
    }
    let current_review = recorded.item("get-review", Some(&review.id))?;
    let current_render = recorded.item("get-render", Some(&render.id))?;
    let claim = |item: &ObservedItem| {
        json!({
            "id": item.id, "expected_revision": item.revision,
            "owner": {"key": {"kind": "agent", "id": owner_id}}, "lease_seconds": 300,
        })
    };
    if let Some(turn) = recorded.pending(&[
        ("claim-review", "workgraph_claim", claim(&current_review)),
        ("claim-render", "workgraph_claim", claim(&current_render)),
    ])? {
        return Ok(turn);
    }
    let claimed_review = recorded.item("claim-review", Some(&review.id))?;
    let claimed_render = recorded.item("claim-render", Some(&render.id))?;
    for item in [&claimed_review, &claimed_render] {
        if item.status != "in_progress" {
            return Err(format!(
                "claim returned status {} for {}",
                item.status, item.id
            ));
        }
    }
    let close = |item: &ObservedItem| {
        json!({
            "id": item.id, "expected_revision": item.revision, "status": "completed",
        })
    };
    if let Some(turn) = recorded.pending(&[
        ("close-review", "workgraph_close", close(&claimed_review)),
        ("close-render", "workgraph_close", close(&claimed_render)),
    ])? {
        return Ok(turn);
    }
    for (step, id) in [("close-review", &review.id), ("close-render", &render.id)] {
        if recorded.item(step, Some(id))?.status != "completed" {
            return Err(format!("{step} did not complete its prerequisite"));
        }
    }
    if let Some(turn) = recorded.pending(&[
        (
            "ready-after",
            "workgraph_ready",
            json!({"labels": [scenario.label()]}),
        ),
        ("get-publish", "workgraph_get", json!({"id": publish.id})),
    ])? {
        return Ok(turn);
    }
    let ready_after = recorded
        .result("ready-after")?
        .ok_or("missing ready-after result")?;
    if !item_ids(&ready_after)?.contains(&publish.id.as_str()) {
        return Err("dependent item did not become ready after its prerequisites completed".into());
    }
    let current_publish = recorded.item("get-publish", Some(&publish.id))?;
    if let Some(turn) =
        recorded.pending(&[("claim-publish", "workgraph_claim", claim(&current_publish))])?
    {
        return Ok(turn);
    }
    let claimed_publish = recorded.item("claim-publish", Some(&publish.id))?;
    if claimed_publish.status != "in_progress" {
        return Err("dependent claim did not enter in_progress".into());
    }
    if let Some(turn) =
        recorded.pending(&[("close-publish", "workgraph_close", close(&claimed_publish))])?
    {
        return Ok(turn);
    }
    if recorded.item("close-publish", Some(&publish.id))?.status != "completed" {
        return Err("dependent item did not complete".into());
    }
    if let Some(turn) = recorded.pending(&[(
        "snapshot",
        "workgraph_snapshot",
        json!({
            "labels": [scenario.label()], "include_terminal": true,
        }),
    )])? {
        return Ok(turn);
    }
    let result = recorded
        .result("snapshot")?
        .ok_or("missing final snapshot")?;
    let snapshot = result
        .get("snapshot")
        .ok_or("final result has no snapshot")?;
    let items = snapshot
        .get("items")
        .and_then(Value::as_array)
        .ok_or("snapshot has no items")?;
    for item in [&review, &render, &publish] {
        if !items
            .iter()
            .any(|observed| observed["id"] == item.id && observed["status"] == "completed")
        {
            return Err(format!(
                "final snapshot does not confirm completed item {}",
                item.id
            ));
        }
    }
    let edges = snapshot
        .get("edges")
        .and_then(Value::as_array)
        .ok_or("snapshot has no edges")?;
    for prerequisite in [&review, &render] {
        if !edges.iter().any(|edge| {
            edge["kind"] == "blocks"
                && edge["from_id"] == prerequisite.id
                && edge["to_id"] == publish.id
        }) {
            return Err(format!(
                "final snapshot does not confirm dependency from {}",
                prerequisite.id
            ));
        }
    }
    Ok(Turn::Text(format!(
        "## WorkGraph scenario complete\n\nThe runtime snapshot confirms two prerequisites and their dependent item are completed. The dependent was absent from the ready set before its prerequisites closed and present afterward.\n\n| Item | Runtime id | Status |\n| --- | --- | --- |\n| Source review | `{}` | completed |\n| Badge review | `{}` | completed |\n| Release candidate | `{}` | completed |\n\nThese are fixture WorkGraph transitions; no deployment was performed.",
        review.id, render.id, publish.id,
    )))
}

fn item_ids(value: &Value) -> Result<Vec<&str>, String> {
    value
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("ready result has no items: {value}"))?
        .iter()
        .map(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("ready item has no id: {item}"))
        })
        .collect()
}

fn peer_turn(recorded: &RecordedTurn<'_>) -> Result<Turn, String> {
    let peer_id = required(
        &recorded.scenario.peer_id,
        "peer_id (host-resolved canonical routing id)",
    )?;
    let body = required(&recorded.scenario.peer_body, "peer_body")?;
    if body.trim_start().starts_with("[fixture:") {
        return Err("peer_body must not start with an operator scenario trigger".into());
    }
    if let Some(turn) = recorded.pending(&[(
        "send-peer",
        "send_message",
        json!({
            "peer_id": peer_id, "body": body, "handling_mode": "queue",
        }),
    )])? {
        return Ok(turn);
    }
    let result = recorded
        .result("send-peer")?
        .ok_or("missing send_message result")?;
    if result["status"] != "sent" || !result.get("receipt").is_some_and(Value::is_object) {
        return Err(format!(
            "send_message did not return a sent receipt: {result}"
        ));
    }
    Ok(Turn::Text(format!(
        "## Peer message submitted\n\nThe runtime returned this send receipt. Recipient processing is verified separately in the recipient conversation.\n\n```json\n{}\n```",
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?,
    )))
}

fn image_turn(recorded: &RecordedTurn<'_>) -> Result<Turn, String> {
    let provider = required(&recorded.scenario.image_provider, "image_provider")?;
    let prompt = required(&recorded.scenario.image_prompt, "image_prompt")?;
    if let Some(turn) = recorded.pending(&[(
        "generate-image",
        "generate_image",
        json!({
            "request": { "intent": "generate", "prompt": prompt, "provider": provider, "count": 1 },
        }),
    )])? {
        return Ok(turn);
    }
    let result = recorded
        .result("generate-image")?
        .ok_or("missing generate_image result")?;
    if result.pointer("/terminal/terminal").and_then(Value::as_str) != Some("generated") {
        return Err(format!(
            "image operation did not generate an image: {result}"
        ));
    }
    let images = result
        .get("images")
        .and_then(Value::as_array)
        .filter(|images| !images.is_empty())
        .ok_or_else(|| format!("generated result has no committed image: {result}"))?;
    for image in images {
        if image.get("image_id").and_then(Value::as_str).is_none()
            || image
                .pointer("/blob_ref/blob_id")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            || image
                .pointer("/blob_ref/media_type")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            return Err(format!(
                "generated result has an incomplete committed image: {image}"
            ));
        }
    }
    // Auto-sized provider results can carry zero dimension metadata. The
    // browser acceptance test resolves the committed blob and decodes its
    // actual pixels; metadata alone cannot establish a valid image.
    Ok(Turn::Text(format!(
        "## Image operation complete\n\nThe runtime reported `generated` and committed {} image artifact(s). The tool's assistant image block carries the preview.\n\n```json\n{}\n```",
        images.len(),
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use meerkat_core::{AssistantBlock, BlockAssistantMessage, ToolResult, UserMessage};

    #[tokio::test]
    async fn barrier_blocks_before_first_tool_until_exact_release() {
        let barrier = Arc::new(ModelBarrier::default());
        let command = json!({"action": "arm", "plan": {
            "id": "overlap-1", "match_text": "peer barrier marker", "source": "Peer complete."
        }});
        assert_eq!(barrier.control(&command).unwrap()["phase"], "armed");
        let client = RecordingClient::new(
            Arc::new(Mutex::new(ModelPlan {
                source: "Ordinary reply".into(),
                delay_ms: 0,
                chunk_chars: 32,
                scenario: None,
            })),
            Arc::new(Mutex::new(Vec::new())),
            barrier.clone(),
        );
        let request = LlmRequest::new(
            "gpt-5.5",
            vec![Message::User(UserMessage::text(
                "Incoming peer barrier marker",
            ))],
        );
        let mut stream = client.stream(&request);
        let first = stream.next();
        tokio::pin!(first);
        assert!(matches!(
            futures::poll!(first.as_mut()),
            std::task::Poll::Pending
        ));
        let status = barrier
            .control(&json!({"action": "status", "id": "overlap-1"}))
            .unwrap();
        assert_eq!(status["phase"], "entered");
        assert_eq!(status["requests"], 1);
        assert!(
            barrier
                .control(&json!({"action": "release", "id": "wrong"}))
                .is_err()
        );
        assert!(matches!(
            futures::poll!(first.as_mut()),
            std::task::Poll::Pending
        ));
        assert_eq!(
            barrier
                .control(&json!({"action": "release", "id": "overlap-1"}))
                .unwrap()["phase"],
            "released"
        );
        assert!(
            matches!(first.await, Some(Ok(LlmEvent::ToolCallComplete { id, name, .. }))
            if id == "fixture-overlap-1-peer-ready" && name == "workgraph_ready")
        );
    }

    #[tokio::test]
    async fn barrier_release_is_not_lost_and_old_peer_message_cannot_capture_new_operator() {
        let barrier = ModelBarrier::default();
        barrier
            .control(&json!({"action": "arm", "plan": {
                "id": "overlap-2", "match_text": "peer barrier marker", "source": "Peer complete."
            }}))
            .unwrap();
        let peer = Message::User(UserMessage::text("Incoming peer barrier marker"));
        let messages = vec![
            peer.clone(),
            Message::User(UserMessage::text("New operator input")),
        ];
        assert!(barrier.matching_request(&messages).is_none());
        let (run, _) = barrier.matching_request(&[peer]).unwrap();
        barrier
            .control(&json!({"action": "release", "id": "overlap-2"}))
            .unwrap();
        assert!(matches!(
            futures::poll!(Box::pin(run.wait_for_release())),
            std::task::Poll::Ready(())
        ));
        let result = Message::tool_results(vec![ToolResult::new(
            "fixture-overlap-2-peer-ready".into(),
            "{\"items\":[]}".into(),
            false,
        )]);
        assert!(
            matches!(barrier_turn(&run.plan, &[result]), Ok(Turn::Text(text)) if text == "Peer complete.")
        );
    }

    #[tokio::test]
    async fn bootstrap_emits_visible_completion_and_usage_before_done() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let client = RecordingClient::new(
            Arc::new(Mutex::new(ModelPlan {
                source: "Scenario source must not run during bootstrap.".into(),
                delay_ms: 0,
                chunk_chars: 256,
                scenario: None,
            })),
            requests.clone(),
            Arc::new(ModelBarrier::default()),
        );
        let request = LlmRequest::new(
            "gpt-5.5",
            vec![Message::User(UserMessage::text(
                "You have been spawned as 'fixture-member' (role: lead) in mob 'fixture'.",
            ))],
        );
        let events: Vec<_> = client.stream(&request).collect().await;
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                Ok(LlmEvent::TextDelta { delta, .. }) => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Ready.");
        assert!(matches!(
            events.get(events.len().saturating_sub(2)),
            Some(Ok(LlmEvent::UsageUpdate { .. }))
        ));
        assert!(matches!(
            events.last(),
            Some(Ok(LlmEvent::Done {
                outcome: LlmDoneOutcome::Success {
                    stop_reason: StopReason::EndTurn
                }
            }))
        ));
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    fn scenario(kind: ScenarioKind) -> ScenarioPlan {
        ScenarioPlan {
            kind,
            run_id: "release-1".into(),
            owner_id: Some("router:main".into()),
            peer_id: Some("canonical-peer-id".into()),
            peer_body: Some("Please review the release candidate.".into()),
            image_provider: Some("openai".into()),
            image_prompt: Some("A small blue shipping crate.".into()),
        }
    }

    fn start(scenario: &ScenarioPlan) -> Vec<Message> {
        vec![Message::User(UserMessage::text(format!(
            "{} Run the acceptance scenario.",
            scenario.trigger()
        )))]
    }

    fn result(scenario: &ScenarioPlan, step: &str, value: Value) -> Message {
        Message::tool_results(vec![ToolResult::new(
            scenario.call_id(step),
            value.to_string(),
            false,
        )])
    }

    fn calls(turn: Turn) -> Vec<PlannedCall> {
        match turn {
            Turn::Tools(calls) => calls,
            _ => panic!("expected a real tool-call turn"),
        }
    }

    fn created(scenario: &ScenarioPlan) -> Vec<Message> {
        let mut messages = start(scenario);
        for (step, id) in [
            ("create-review", "review-id"),
            ("create-render", "render-id"),
            ("create-publish", "publish-id"),
        ] {
            messages.push(result(
                scenario,
                step,
                json!({"item": {
                    "id": id, "revision": 1, "status": "open",
                }}),
            ));
        }
        messages
    }

    fn linked(scenario: &ScenarioPlan) -> Vec<Message> {
        let mut messages = created(scenario);
        messages.push(result(scenario, "link-review", json!({"edge": {}})));
        messages.push(result(scenario, "link-render", json!({"edge": {}})));
        messages.push(result(
            scenario,
            "ready-before",
            json!({"items": [
                {"id": "review-id"}, {"id": "render-id"},
            ]}),
        ));
        messages.push(result(
            scenario,
            "get-review",
            json!({"item": {
                "id": "review-id", "revision": 7, "status": "open",
            }}),
        ));
        messages.push(result(
            scenario,
            "get-render",
            json!({"item": {
                "id": "render-id", "revision": 11, "status": "open",
            }}),
        ));
        messages
    }

    #[test]
    fn independent_creates_and_links_use_observed_ids() {
        let scenario = scenario(ScenarioKind::Workgraph);
        let first = calls(scenario_turn(&scenario, &start(&scenario)).unwrap());
        assert_eq!(first.len(), 3);
        assert!(first.iter().all(|call| call.name == "workgraph_create"));
        let links = calls(scenario_turn(&scenario, &created(&scenario)).unwrap());
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].args["from_id"], "review-id");
        assert_eq!(links[0].args["to_id"], "publish-id");
        assert_eq!(links[1].args["from_id"], "render-id");
    }

    #[test]
    fn mutations_use_current_get_and_claim_revisions() {
        let scenario = scenario(ScenarioKind::Workgraph);
        let mut messages = linked(&scenario);
        let claims = calls(scenario_turn(&scenario, &messages).unwrap());
        assert_eq!(claims[0].args["expected_revision"], 7);
        assert_eq!(claims[1].args["expected_revision"], 11);
        assert_eq!(claims[0].args["owner"]["key"]["id"], "router:main");
        for (step, id, revision) in [
            ("claim-review", "review-id", 8),
            ("claim-render", "render-id", 12),
        ] {
            messages.push(result(
                &scenario,
                step,
                json!({"item": {
                    "id": id, "revision": revision, "status": "in_progress",
                }}),
            ));
        }
        let closes = calls(scenario_turn(&scenario, &messages).unwrap());
        assert_eq!(closes[0].args["expected_revision"], 8);
        assert_eq!(closes[1].args["expected_revision"], 12);
    }

    #[test]
    fn readiness_violation_stops_before_claiming() {
        let scenario = scenario(ScenarioKind::Workgraph);
        let mut messages = linked(&scenario);
        messages.push(result(
            &scenario,
            "ready-before",
            json!({"items": [
                {"id": "review-id"}, {"id": "render-id"}, {"id": "publish-id"},
            ]}),
        ));
        let error = scenario_turn(&scenario, &messages).unwrap_err();
        assert!(error.contains("dependent item was ready before"));
    }

    #[test]
    fn tool_errors_and_missing_results_never_become_success_or_retries() {
        let scenario = scenario(ScenarioKind::Peer);
        let mut messages = start(&scenario);
        messages.push(Message::tool_results(vec![ToolResult::new(
            scenario.call_id("send-peer"),
            "peer_unreachable".into(),
            true,
        )]));
        assert!(
            scenario_turn(&scenario, &messages)
                .unwrap_err()
                .contains("peer_unreachable")
        );
        messages.pop();
        messages.push(Message::BlockAssistant(BlockAssistantMessage::new(
            vec![AssistantBlock::ToolUse {
                id: scenario.call_id("send-peer"),
                name: "send_message".into(),
                args: serde_json::value::to_raw_value(&json!({})).unwrap(),
                meta: None,
            }],
            StopReason::ToolUse,
        )));
        assert!(
            scenario_turn(&scenario, &messages)
                .unwrap_err()
                .contains("without a recorded result")
        );
    }

    #[test]
    fn peer_receipt_is_observed_without_claiming_recipient_processing() {
        let scenario = scenario(ScenarioKind::Peer);
        let mut messages = start(&scenario);
        let send = calls(scenario_turn(&scenario, &messages).unwrap());
        assert_eq!(send[0].name, "send_message");
        assert_eq!(send[0].args["peer_id"], "canonical-peer-id");
        messages.push(result(
            &scenario,
            "send-peer",
            json!({
                "status": "sent", "kind": "message", "receipt": {"id": "receipt-1"},
            }),
        ));
        let Turn::Text(text) = scenario_turn(&scenario, &messages).unwrap() else {
            panic!("expected receipt summary");
        };
        assert!(text.contains("receipt-1"));
        assert!(text.contains("Recipient processing is verified separately"));
    }

    #[test]
    fn fresh_user_boundary_and_peer_turns_do_not_reuse_another_turn() {
        let scenario = scenario(ScenarioKind::Peer);
        let mut messages = start(&scenario);
        messages.push(result(
            &scenario,
            "send-peer",
            json!({
                "status": "sent", "receipt": {"id": "old"},
            }),
        ));
        messages.push(Message::User(UserMessage::text("Ordinary follow-up")));
        assert!(active_messages(&scenario, &messages).is_none());
        messages.push(Message::User(UserMessage::text(scenario.trigger())));
        let active = active_messages(&scenario, &messages).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(calls(scenario_turn(&scenario, active).unwrap()).len(), 1);
        assert!(
            active_messages(
                &scenario,
                &[Message::User(UserMessage::text(
                    "A peer asks to review the release candidate.",
                ))]
            )
            .is_none()
        );
    }

    #[test]
    fn image_completion_requires_generated_terminal_and_committed_artifact() {
        let scenario = scenario(ScenarioKind::Image);
        let mut messages = start(&scenario);
        let call = calls(scenario_turn(&scenario, &messages).unwrap());
        assert_eq!(call[0].name, "generate_image");
        assert_eq!(call[0].args["request"]["intent"], "generate");
        assert_eq!(
            call[0].args["request"]["prompt"],
            "A small blue shipping crate."
        );
        messages.push(result(
            &scenario,
            "generate-image",
            json!({
                "operation_id": "op-1", "terminal": {"terminal": "generated"}, "images": [],
            }),
        ));
        assert!(
            scenario_turn(&scenario, &messages)
                .unwrap_err()
                .contains("committed image")
        );
        messages.pop();
        messages.push(result(
            &scenario,
            "generate-image",
            json!({
                "operation_id": "op-1", "terminal": {"terminal": "denied"},
            }),
        ));
        assert!(
            scenario_turn(&scenario, &messages)
                .unwrap_err()
                .contains("did not generate")
        );
        messages.pop();
        messages.push(result(
            &scenario,
            "generate-image",
            json!({
                "operation_id": "op-1", "terminal": {"terminal": "generated"},
                "images": [{"image_id": "image-1", "width": 1024, "height": 1024,
                    "blob_ref": {"blob_id": "sha256:observed-test-blob", "media_type": "image/png"},
                }],
            }),
        ));
        let Turn::Text(text) = scenario_turn(&scenario, &messages).unwrap() else {
            panic!("expected observed image summary");
        };
        assert!(text.contains("sha256:observed-test-blob"));
        messages.pop();
        messages.push(result(
            &scenario,
            "generate-image",
            json!({
                "operation_id": "op-auto", "terminal": {"terminal": "generated"},
                "images": [{"image_id": "image-auto", "width": 0, "height": 0,
                    "media_type": "image/png",
                    "blob_ref": {"blob_id": "sha256:auto-sized", "media_type": "image/png"},
                }],
            }),
        ));
        assert!(matches!(
            scenario_turn(&scenario, &messages),
            Ok(Turn::Text(_))
        ));
    }

    #[test]
    fn complete_graph_checks_both_readiness_transition_and_final_snapshot() {
        let scenario = scenario(ScenarioKind::Workgraph);
        let mut messages = linked(&scenario);
        for (step, id, revision, status) in [
            ("claim-review", "review-id", 8, "in_progress"),
            ("claim-render", "render-id", 12, "in_progress"),
            ("close-review", "review-id", 9, "completed"),
            ("close-render", "render-id", 13, "completed"),
        ] {
            messages.push(result(
                &scenario,
                step,
                json!({"item": {
                    "id": id, "revision": revision, "status": status,
                }}),
            ));
        }
        messages.push(result(
            &scenario,
            "ready-after",
            json!({"items": [{"id": "publish-id"}]}),
        ));
        messages.push(result(
            &scenario,
            "get-publish",
            json!({"item": {
                "id": "publish-id", "revision": 15, "status": "open",
            }}),
        ));
        let claim = calls(scenario_turn(&scenario, &messages).unwrap());
        assert_eq!(claim[0].args["expected_revision"], 15);
        messages.push(result(
            &scenario,
            "claim-publish",
            json!({"item": {
                "id": "publish-id", "revision": 16, "status": "in_progress",
            }}),
        ));
        let close = calls(scenario_turn(&scenario, &messages).unwrap());
        assert_eq!(close[0].args["expected_revision"], 16);
        messages.push(result(
            &scenario,
            "close-publish",
            json!({"item": {
                "id": "publish-id", "revision": 17, "status": "completed",
            }}),
        ));
        assert_eq!(
            calls(scenario_turn(&scenario, &messages).unwrap())[0].name,
            "workgraph_snapshot"
        );
        messages.push(result(
            &scenario,
            "snapshot",
            json!({"snapshot": {
                "items": [
                    {"id": "review-id", "status": "completed"},
                    {"id": "render-id", "status": "completed"},
                    {"id": "publish-id", "status": "completed"},
                ],
                "edges": [
                    {"kind": "blocks", "from_id": "review-id", "to_id": "publish-id"},
                    {"kind": "blocks", "from_id": "render-id", "to_id": "publish-id"},
                ],
            }}),
        ));
        assert!(matches!(
            scenario_turn(&scenario, &messages).unwrap(),
            Turn::Text(_)
        ));
        messages.push(result(
            &scenario,
            "snapshot",
            json!({"snapshot": {"items": [], "edges": []}}),
        ));
        assert!(
            scenario_turn(&scenario, &messages)
                .unwrap_err()
                .contains("does not confirm completed")
        );
    }
}
