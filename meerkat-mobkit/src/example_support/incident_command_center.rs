use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_stream::stream;
use async_trait::async_trait;
use meerkat::AgentToolDispatcher;
use meerkat_client::types::LlmStream;
use meerkat_client::{LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::error::ToolError;
use meerkat_core::service::{CreateSessionRequest, SessionBuildOptions};
use meerkat_core::{
    ContentBlock, Message, StopReason, ToolCallView, ToolDef, ToolDispatchOutcome, ToolResult,
};
use meerkat_mob::{MeerkatId, MobDefinition, ProfileName, SpawnMemberSpec};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::sleep;

use crate::decisions::{AuthPolicy, BigQueryNaming, ConsolePolicy, RuntimeOpsPolicy};
use crate::mob_handle_runtime::{SessionCreatedContext, SessionHook};
use crate::runtime::{
    DeliverySendRequest, GatingDecideRequest, GatingDecision, GatingEvaluateRequest,
    GatingPendingEntry, GatingRiskTier, RuntimeRoute,
};
use crate::unified_runtime::{DesiredPeerEdge, EdgeDiscovery, UnifiedRuntime};
use crate::{
    DiscoverySpec, MobKitConfig, ModuleConfig, PreSpawnData, RestartPolicy, RuntimeDecisionInputs,
    RuntimeDecisionState, TrustedOidcRuntimeConfig, build_runtime_decision_state,
};

const BOUNDARY_ENV_KEY: &str = "MOBKIT_MODULE_BOUNDARY";
const BOUNDARY_ENV_VALUE_MCP: &str = "mcp";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IncidentScenario {
    pub scenario_id: String,
    pub namespace: String,
    pub listen_addr: String,
    pub approver_id: String,
    #[serde(default)]
    pub identities: Vec<IncidentIdentity>,
    #[serde(default)]
    pub links: Vec<IncidentLink>,
    #[serde(default)]
    pub routes: Vec<IncidentRouteSeed>,
    #[serde(default)]
    pub deliveries: Vec<IncidentDeliverySeed>,
    #[serde(default)]
    pub gating: Vec<IncidentGatingSeed>,
    #[serde(default)]
    pub scripted_turns: Vec<IncidentScriptedTurn>,
    #[serde(default)]
    pub smoke: IncidentSmokeExpectations,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IncidentIdentity {
    pub identity: String,
    pub profile: String,
    pub display_name: String,
    pub group: String,
    #[serde(default = "default_addressable")]
    pub addressable: bool,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IncidentLink {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IncidentRouteSeed {
    pub route_key: String,
    pub recipient: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub sink: String,
    #[serde(default = "default_delivery_target_module")]
    pub target_module: String,
    #[serde(default)]
    pub retry_max: Option<u32>,
    #[serde(default)]
    pub backoff_ms: Option<u64>,
    #[serde(default)]
    pub rate_limit_per_minute: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IncidentDeliverySeed {
    pub recipient: String,
    pub channel: String,
    pub payload: Value,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IncidentGatingSeed {
    pub action: String,
    pub actor_id: String,
    pub risk_tier: String,
    pub requested_approver: String,
    pub approval_recipient: String,
    pub approval_channel: String,
    #[serde(default)]
    pub approval_timeout_ms: Option<u64>,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub entity: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IncidentScriptedTurn {
    pub prompt_contains: String,
    #[serde(default)]
    pub tool_calls: Vec<IncidentScriptedToolCall>,
    #[serde(default)]
    pub final_text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IncidentScriptedToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct IncidentSmokeExpectations {
    #[serde(default)]
    pub watched_identities: Vec<String>,
    #[serde(default)]
    pub degraded_identities: Vec<String>,
    #[serde(default)]
    pub critical_identities: Vec<String>,
}

#[derive(Debug, Clone, JsonSchema, Deserialize)]
struct InspectServiceArgs {
    service: String,
}

#[derive(Debug, Clone, JsonSchema, Deserialize)]
struct AnalyzeImpactArgs {
    cohort: String,
}

#[derive(Clone)]
pub struct IncidentRuntimeBundle {
    pub scenario: IncidentScenario,
    pub runtime: Arc<UnifiedRuntime>,
    pub decisions: RuntimeDecisionState,
}

pub fn scenario_path() -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("workspace root")?
        .join("examples")
        .join("001-incident-command-center-pack")
        .join("scenario.yaml"))
}

pub fn load_scenario(path: &Path) -> Result<IncidentScenario> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read scenario at {}", path.display()))?;
    let scenario = serde_yaml::from_str::<IncidentScenario>(&raw)
        .with_context(|| format!("failed to parse scenario at {}", path.display()))?;
    Ok(scenario)
}

pub async fn build_runtime_bundle(path: &Path) -> Result<IncidentRuntimeBundle> {
    let scenario = load_scenario(path)?;
    let definition = incident_definition()?;
    let runtime = UnifiedRuntime::builder()
        .definition(definition)
        .default_llm_client(Arc::new(IncidentScriptedLlmClient::new(
            scenario.scripted_turns.clone(),
        )))
        .session_hook(Arc::new(IncidentSessionHook))
        .module_config(example_module_config(&scenario)?)
        .edge_discovery(ScenarioEdgeDiscovery::new(scenario.links.clone()))
        .timeout(Duration::from_secs(5))
        .build()
        .await
        .context("build incident runtime")?;

    seed_runtime(&runtime, &scenario).await?;

    let decisions = build_runtime_decision_state(RuntimeDecisionInputs {
        bigquery: BigQueryNaming {
            dataset: format!("{}_dataset", scenario.namespace.replace('-', "_")),
            table: "console_events".to_string(),
        },
        trusted_mobkit_toml: trusted_modules_toml(),
        auth: AuthPolicy::default(),
        trusted_oidc: trusted_oidc(),
        console: ConsolePolicy {
            require_app_auth: false,
        },
        ops: RuntimeOpsPolicy::default(),
        release_metadata_json: include_str!("../../../docs/rct/release-targets.json").to_string(),
    })
    .map_err(|err| anyhow!("failed to build incident console decisions: {err:?}"))?;

    Ok(IncidentRuntimeBundle {
        scenario,
        runtime: Arc::new(runtime),
        decisions,
    })
}

pub async fn seed_runtime(runtime: &UnifiedRuntime, scenario: &IncidentScenario) -> Result<()> {
    runtime
        .reconcile(
            scenario
                .identities
                .iter()
                .map(identity_to_spawn_spec)
                .collect::<Vec<_>>(),
        )
        .await
        .context("reconcile incident roster")?;

    for route in &scenario.routes {
        runtime
            .add_runtime_route(RuntimeRoute {
                route_key: route.route_key.clone(),
                recipient: route.recipient.clone(),
                channel: route.channel.clone(),
                sink: route.sink.clone(),
                target_module: route.target_module.clone(),
                retry_max: route.retry_max,
                backoff_ms: route.backoff_ms,
                rate_limit_per_minute: route.rate_limit_per_minute,
            })
            .await
            .with_context(|| format!("add route {}", route.route_key))?;
    }

    for delivery in &scenario.deliveries {
        let resolution = runtime
            .resolve_routing(crate::runtime::RoutingResolveRequest {
                recipient: delivery.recipient.clone(),
                channel: Some(delivery.channel.clone()),
                retry_max: None,
                backoff_ms: None,
                rate_limit_per_minute: None,
            })
            .await
            .with_context(|| format!("resolve delivery recipient {}", delivery.recipient))?;
        runtime
            .send_delivery(DeliverySendRequest {
                resolution,
                payload: delivery.payload.clone(),
                idempotency_key: Some(delivery.idempotency_key.clone()),
            })
            .await
            .with_context(|| format!("seed delivery {}", delivery.idempotency_key))?;
    }

    for gating in &scenario.gating {
        runtime
            .evaluate_gating_action(GatingEvaluateRequest {
                action: gating.action.clone(),
                actor_id: gating.actor_id.clone(),
                risk_tier: parse_risk_tier(&gating.risk_tier)?,
                rationale: gating.rationale.clone(),
                requested_approver: Some(gating.requested_approver.clone()),
                approval_recipient: Some(gating.approval_recipient.clone()),
                approval_channel: Some(gating.approval_channel.clone()),
                approval_timeout_ms: gating.approval_timeout_ms,
                entity: gating.entity.clone(),
                topic: gating.topic.clone(),
            })
            .await;
    }

    Ok(())
}

pub async fn seed_escalation_chain(
    runtime: &UnifiedRuntime,
    pending_id: &str,
    approver_id: &str,
) -> Result<GatingPendingEntry> {
    let escalation = runtime
        .decide_gating_action(GatingDecideRequest {
            pending_id: pending_id.to_string(),
            approver_id: approver_id.to_string(),
            decision: GatingDecision::Escalate,
            reason: Some("incident_command_center_smoke".to_string()),
        })
        .await
        .context("escalate gating entry")?;
    let next_pending_id = escalation
        .next_pending_id
        .clone()
        .ok_or_else(|| anyhow!("expected escalation to create successor pending entry"))?;
    let pending = runtime.list_gating_pending().await;
    pending
        .into_iter()
        .find(|entry| entry.pending_id == next_pending_id)
        .ok_or_else(|| anyhow!("successor pending entry {next_pending_id} not found"))
}

fn default_addressable() -> bool {
    true
}

fn default_delivery_target_module() -> String {
    "delivery".to_string()
}

fn incident_definition() -> Result<MobDefinition> {
    MobDefinition::from_toml(
        r#"
[mob]
id = "incident-command-center"

[profiles.ops]
model = "gpt-5.2"
external_addressable = true

[profiles.ops.tools]
comms = true

[profiles.internal]
model = "gpt-5.2"
external_addressable = false

[profiles.internal.tools]
comms = true
"#,
    )
    .map_err(|error| anyhow!("incident command center definition must parse: {error}"))
}

fn identity_to_spawn_spec(identity: &IncidentIdentity) -> SpawnMemberSpec {
    let mut labels = identity.labels.clone();
    labels.insert("display_name".to_string(), identity.display_name.clone());
    labels.insert("group".to_string(), identity.group.clone());
    labels.insert(
        "addressable".to_string(),
        if identity.addressable {
            "true"
        } else {
            "false"
        }
        .to_string(),
    );
    SpawnMemberSpec::new(
        ProfileName::from(identity.profile.as_str()),
        MeerkatId::from(identity.identity.as_str()),
    )
    .with_labels(labels)
}

fn example_module_config(scenario: &IncidentScenario) -> Result<MobKitConfig> {
    let fixture_binary = fixture_binary_path()?;
    Ok(MobKitConfig {
        modules: vec![
            fixture_module("router", &fixture_binary),
            fixture_module("delivery", &fixture_binary),
        ],
        discovery: DiscoverySpec {
            namespace: scenario.namespace.clone(),
            modules: vec!["router".to_string(), "delivery".to_string()],
        },
        pre_spawn: vec![
            PreSpawnData {
                module_id: "router".to_string(),
                env: mcp_env(&[]),
            },
            PreSpawnData {
                module_id: "delivery".to_string(),
                env: mcp_env(&[]),
            },
        ],
    })
}

fn fixture_binary_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_mcp_fixture") {
        return Ok(PathBuf::from(path));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().context("workspace root")?;
    let binary_path = workspace_root
        .join("target")
        .join("debug")
        .join("mcp_fixture");
    if binary_path.exists() {
        return Ok(binary_path);
    }

    let status = Command::new("cargo")
        .args(["build", "-p", "meerkat-mobkit", "--bin", "mcp_fixture"])
        .current_dir(workspace_root)
        .status()
        .context("build mcp_fixture")?;
    if !status.success() {
        bail!("building mcp_fixture must succeed");
    }
    Ok(binary_path)
}

fn fixture_module(id: &str, fixture_binary: &Path) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: fixture_binary.display().to_string(),
        args: vec!["--module".to_string(), id.to_string()],
        restart_policy: RestartPolicy::Never,
    }
}

fn mcp_env(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = vec![(
        BOUNDARY_ENV_KEY.to_string(),
        BOUNDARY_ENV_VALUE_MCP.to_string(),
    )];
    env.extend(
        extra
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string())),
    );
    env
}

fn parse_risk_tier(raw: &str) -> Result<GatingRiskTier> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "r0" => Ok(GatingRiskTier::R0),
        "r1" => Ok(GatingRiskTier::R1),
        "r2" => Ok(GatingRiskTier::R2),
        "r3" => Ok(GatingRiskTier::R3),
        other => bail!("unknown gating risk tier: {other}"),
    }
}

fn trusted_modules_toml() -> String {
    r#"
[[modules]]
id = "router"
command = "mcp_fixture"
args = ["--module", "router"]
restart_policy = "never"

[[modules]]
id = "delivery"
command = "mcp_fixture"
args = ["--module", "delivery"]
restart_policy = "never"
"#
    .to_string()
}

fn trusted_oidc() -> TrustedOidcRuntimeConfig {
    TrustedOidcRuntimeConfig {
        discovery_json:
            r#"{"issuer":"https://trusted.mobkit.local","jwks_uri":"https://trusted.mobkit.local/.well-known/jwks.json"}"#
                .to_string(),
        jwks_json: r#"{"keys":[{"kid":"kid-current","kty":"oct","alg":"HS256","k":"cGhhc2U3LXRydXN0ZWQtY3VycmVudC1zZWNyZXQ"}]}"#
            .to_string(),
        audience: "meerkat-console".to_string(),
    }
}

#[derive(Clone)]
struct ScenarioEdgeDiscovery {
    desired: Arc<Vec<IncidentLink>>,
}

impl ScenarioEdgeDiscovery {
    fn new(desired: Vec<IncidentLink>) -> Self {
        Self {
            desired: Arc::new(desired),
        }
    }
}

impl EdgeDiscovery for ScenarioEdgeDiscovery {
    fn discover_edges(
        &self,
        active_members: Vec<crate::mob_handle_runtime::MobMemberSnapshot>,
    ) -> Pin<Box<dyn futures::Future<Output = Vec<DesiredPeerEdge>> + Send + '_>> {
        let desired = Arc::clone(&self.desired);
        Box::pin(async move {
            let active = active_members
                .into_iter()
                .map(|member| member.meerkat_id)
                .collect::<BTreeSet<_>>();
            desired
                .iter()
                .filter(|edge| active.contains(&edge.from) && active.contains(&edge.to))
                .filter_map(|edge| DesiredPeerEdge::new(edge.from.clone(), edge.to.clone()).ok())
                .collect()
        })
    }
}

#[derive(Clone)]
struct IncidentSessionHook;

#[async_trait]
impl SessionHook for IncidentSessionHook {
    async fn before_create(
        &self,
        req: &mut CreateSessionRequest,
    ) -> Result<(), meerkat_core::SessionError> {
        let labels = req.labels.clone().unwrap_or_default();
        let build = req.build.get_or_insert_with(SessionBuildOptions::default);
        build.external_tools = Some(Arc::new(IncidentToolDispatcher));
        if labels
            .get("addressable")
            .is_some_and(|value| value.eq_ignore_ascii_case("false"))
        {
            build.additional_instructions = Some(vec![
                "You are an internal-only control-plane identity. Refuse conversational requests and explain that the operator should use console controls instead.".to_string(),
            ]);
        }
        Ok(())
    }

    async fn after_create(
        &self,
        _session_id: &meerkat_core::types::SessionId,
        _ctx: &SessionCreatedContext,
    ) {
    }
}

#[derive(Clone)]
struct IncidentToolDispatcher;

#[async_trait]
impl AgentToolDispatcher for IncidentToolDispatcher {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        vec![
            Arc::new(ToolDef {
                name: "inspect_service".to_string(),
                description: "Inspect the current health and saturation of a named service"
                    .to_string(),
                input_schema: meerkat_tools::schema_for::<InspectServiceArgs>(),
            }),
            Arc::new(ToolDef {
                name: "analyze_customer_impact".to_string(),
                description: "Estimate customer-facing impact for a named merchant cohort"
                    .to_string(),
                input_schema: meerkat_tools::schema_for::<AnalyzeImpactArgs>(),
            }),
        ]
        .into()
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        match call.name {
            "inspect_service" => {
                let args: InspectServiceArgs = call
                    .parse_args()
                    .map_err(|error| ToolError::invalid_arguments(call.name, error.to_string()))?;
                sleep(Duration::from_millis(220)).await;
                Ok(ToolResult::new(
                    call.id.to_string(),
                    json!({
                        "service": args.service,
                        "state": "degraded",
                        "error_budget_remaining_percent": 12,
                        "current_error_rate_percent": 38,
                    })
                    .to_string(),
                    false,
                )
                .into())
            }
            "analyze_customer_impact" => {
                let args: AnalyzeImpactArgs = call
                    .parse_args()
                    .map_err(|error| ToolError::invalid_arguments(call.name, error.to_string()))?;
                sleep(Duration::from_millis(80)).await;
                Ok(ToolResult::new(
                    call.id.to_string(),
                    json!({
                        "cohort": args.cohort,
                        "affected_merchants": 124,
                        "payment_failures_last_5m": 893,
                        "recommended_banner": "Investigating elevated payment failures",
                    })
                    .to_string(),
                    false,
                )
                .into())
            }
            _ => Err(ToolError::not_found(call.name)),
        }
    }
}

#[derive(Clone)]
struct IncidentScriptedLlmClient {
    scripted_turns: Arc<Vec<IncidentScriptedTurn>>,
}

impl IncidentScriptedLlmClient {
    fn new(scripted_turns: Vec<IncidentScriptedTurn>) -> Self {
        Self {
            scripted_turns: Arc::new(scripted_turns),
        }
    }

    fn response_events(&self, request: &LlmRequest) -> Vec<Result<LlmEvent, LlmError>> {
        let raw_messages = serde_json::to_string(&request.messages).unwrap_or_default();
        let latest_prompt = latest_user_text(request);
        if has_tool_results(request, &raw_messages) {
            let final_text = if let Some(scripted_final_text) = self
                .scripted_turns
                .iter()
                .find(|turn| {
                    !turn.final_text.is_empty()
                        && latest_prompt
                            .as_deref()
                            .is_some_and(|prompt| prompt.contains(&turn.prompt_contains))
                })
                .map(|turn| turn.final_text.clone())
            {
                scripted_final_text
            } else if latest_prompt
                .as_deref()
                .is_some_and(|prompt| prompt.contains("parallel status sweep"))
            {
                "Payments SRE confirms a cross-region API timeout burst, and merchant impact is concentrated in high-volume accounts. Publish the short status banner now and keep incident-commander focused on mitigation.".to_string()
            } else if latest_prompt
                .as_deref()
                .is_some_and(|prompt| prompt.contains("panel alpha follow-up"))
            {
                "Alpha panel confirms rollback guardrails are in place for eu-west-1.".to_string()
            } else if latest_prompt
                .as_deref()
                .is_some_and(|prompt| prompt.contains("panel bravo follow-up"))
            {
                "Bravo panel confirms customer impact is shrinking but major merchants still need updates.".to_string()
            } else if latest_prompt
                .as_deref()
                .is_some_and(|prompt| prompt.contains("draft merchant update"))
            {
                "Draft merchant update prepared. Request approval before sending externally."
                    .to_string()
            } else {
                "Incident note captured and ready for operator review.".to_string()
            };
            return vec![
                Ok(LlmEvent::TextDelta {
                    delta: final_text,
                    meta: None,
                }),
                Ok(LlmEvent::Done {
                    outcome: LlmDoneOutcome::Success {
                        stop_reason: StopReason::EndTurn,
                    },
                }),
            ];
        }

        if let Some(scripted) = self
            .scripted_turns
            .iter()
            .find(|turn| raw_messages.contains(&turn.prompt_contains))
        {
            if !scripted.tool_calls.is_empty() {
                let mut events = scripted
                    .tool_calls
                    .iter()
                    .map(|tool| {
                        Ok(LlmEvent::ToolCallComplete {
                            id: tool.id.clone(),
                            name: tool.name.clone(),
                            args: tool.args.clone(),
                            meta: None,
                        })
                    })
                    .collect::<Vec<_>>();
                events.push(Ok(LlmEvent::Done {
                    outcome: LlmDoneOutcome::Success {
                        stop_reason: StopReason::ToolUse,
                    },
                }));
                return events;
            }

            let final_text = if scripted.final_text.is_empty() {
                "Incident update acknowledged.".to_string()
            } else {
                scripted.final_text.clone()
            };
            return vec![
                Ok(LlmEvent::TextDelta {
                    delta: final_text,
                    meta: None,
                }),
                Ok(LlmEvent::Done {
                    outcome: LlmDoneOutcome::Success {
                        stop_reason: StopReason::EndTurn,
                    },
                }),
            ];
        }

        vec![
            Ok(LlmEvent::TextDelta {
                delta: "Incident command center standing by.".to_string(),
                meta: None,
            }),
            Ok(LlmEvent::Done {
                outcome: LlmDoneOutcome::Success {
                    stop_reason: StopReason::EndTurn,
                },
            }),
        ]
    }
}

fn has_tool_results(request: &LlmRequest, raw_messages: &str) -> bool {
    request
        .messages
        .iter()
        .any(|message| matches!(message, Message::ToolResults { .. }))
        || raw_messages.contains("error_budget_remaining_percent")
        || raw_messages.contains("recommended_banner")
        || raw_messages.contains("\"tool_results\"")
}

fn latest_user_text(request: &LlmRequest) -> Option<String> {
    request
        .messages
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::User(user) => {
                let text = user
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (!text.trim().is_empty()).then_some(text)
            }
            _ => None,
        })
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl LlmClient for IncidentScriptedLlmClient {
    fn stream<'a>(&'a self, request: &'a LlmRequest) -> LlmStream<'a> {
        let events = self.response_events(request);
        Box::pin(stream! {
            let mut emitted_text_delta = false;
            for event in events {
                if emitted_text_delta {
                    sleep(Duration::from_millis(120)).await;
                    emitted_text_delta = false;
                }
                if matches!(event, Ok(LlmEvent::TextDelta { .. })) {
                    emitted_text_delta = true;
                }
                yield event;
            }
        })
    }

    fn provider(&self) -> &'static str {
        "incident-scripted"
    }

    async fn health_check(&self) -> Result<(), LlmError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meerkat_core::{ContentBlock, UserMessage};

    #[test]
    fn scripted_llm_ends_turn_after_tool_results() {
        let client = IncidentScriptedLlmClient::new(vec![IncidentScriptedTurn {
            prompt_contains: "parallel status sweep".to_string(),
            tool_calls: vec![IncidentScriptedToolCall {
                id: "tool-inspect-service".to_string(),
                name: "inspect_service".to_string(),
                args: json!({ "service": "payments-api" }),
            }],
            final_text: String::new(),
        }]);
        let request = LlmRequest {
            model: "gpt-5.2".to_string(),
            messages: vec![
                Message::User(UserMessage {
                    content: vec![ContentBlock::Text {
                        text: "parallel status sweep".to_string(),
                    }],
                }),
                Message::ToolResults {
                    results: vec![ToolResult::new(
                        "tool-inspect-service".to_string(),
                        "{\"service\":\"payments-api\",\"state\":\"degraded\"}".to_string(),
                        false,
                    )],
                },
            ],
            tools: Vec::new(),
            max_tokens: 512,
            temperature: None,
            stop_sequences: None,
            provider_params: None,
        };

        let events = client.response_events(&request);
        assert!(events.iter().any(|event| {
            matches!(
                event,
                Ok(LlmEvent::Done {
                    outcome: LlmDoneOutcome::Success {
                        stop_reason: StopReason::EndTurn,
                    },
                })
            )
        }));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Ok(LlmEvent::ToolCallComplete { .. })))
        );
    }

    #[test]
    fn scripted_llm_prefers_latest_follow_up_prompt_over_earlier_tool_turn_history() {
        let client = IncidentScriptedLlmClient::new(vec![IncidentScriptedTurn {
            prompt_contains: "panel alpha follow-up".to_string(),
            tool_calls: vec![],
            final_text: "Alpha panel confirms rollback guardrails are in place for eu-west-1."
                .to_string(),
        }]);
        let request = LlmRequest {
            model: "gpt-5.2".to_string(),
            messages: vec![
                Message::User(UserMessage {
                    content: vec![ContentBlock::Text {
                        text: "parallel status sweep".to_string(),
                    }],
                }),
                Message::ToolResults {
                    results: vec![ToolResult::new(
                        "tool-inspect-service".to_string(),
                        "{\"service\":\"payments-api\",\"state\":\"degraded\"}".to_string(),
                        false,
                    )],
                },
                Message::User(UserMessage {
                    content: vec![ContentBlock::Text {
                        text: "panel alpha follow-up".to_string(),
                    }],
                }),
            ],
            tools: Vec::new(),
            max_tokens: 512,
            temperature: None,
            stop_sequences: None,
            provider_params: None,
        };

        let events = client.response_events(&request);
        assert!(events.iter().any(|event| {
            matches!(
                event,
                Ok(LlmEvent::TextDelta { delta, .. })
                    if delta.contains("Alpha panel confirms rollback guardrails are in place for eu-west-1.")
            )
        }));
        assert!(!events.iter().any(|event| {
            matches!(
                event,
                Ok(LlmEvent::TextDelta { delta, .. })
                    if delta.contains("Payments SRE confirms a cross-region API timeout burst")
            )
        }));
    }
}
