use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use meerkat::AgentToolDispatcher;
use meerkat_client::LlmClient;
use meerkat_core::error::ToolError;
use meerkat_core::service::{CreateSessionRequest, SessionBuildOptions};
use meerkat_core::{ToolCallView, ToolDef, ToolDispatchOutcome, ToolResult};
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::{MobDefinition, ProfileName, SpawnMemberSpec};
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct IncidentSmokeExpectations {
    #[serde(default)]
    pub watched_identities: Vec<String>,
    #[serde(default)]
    pub degraded_identities: Vec<String>,
    #[serde(default)]
    pub critical_identities: Vec<String>,
    #[serde(default)]
    pub prompts: IncidentSmokePrompts,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct IncidentSmokePrompts {
    #[serde(default)]
    pub tool_sweep: String,
    #[serde(default)]
    pub merchant_status: String,
    #[serde(default)]
    pub alpha_follow_up: String,
    #[serde(default)]
    pub bravo_follow_up: String,
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
    build_runtime_bundle_with_client(&scenario, None).await
}

pub async fn build_runtime_bundle_with_default_client(
    path: &Path,
    default_llm_client: Arc<dyn LlmClient>,
) -> Result<IncidentRuntimeBundle> {
    let scenario = load_scenario(path)?;
    build_runtime_bundle_with_client(&scenario, Some(default_llm_client)).await
}

async fn build_runtime_bundle_with_client(
    scenario: &IncidentScenario,
    default_llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<IncidentRuntimeBundle> {
    let definition = incident_definition()?;
    let mut builder = UnifiedRuntime::builder()
        .definition(definition)
        .image_generation(true)
        .session_hook(Arc::new(IncidentSessionHook))
        .module_config(example_module_config(scenario)?)
        .edge_discovery(ScenarioEdgeDiscovery::new(scenario.links.clone()))
        .timeout(Duration::from_mins(1));
    if let Some(client) = default_llm_client {
        builder = builder.default_llm_client(client);
    }
    let runtime = builder.build().await.context("build incident runtime")?;

    seed_runtime(&runtime, scenario).await?;

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
        release_metadata_json: include_str!("../../assets/release-targets.json").to_string(),
    })
    .map_err(|err| anyhow!("failed to build incident console decisions: {err:?}"))?;

    Ok(IncidentRuntimeBundle {
        scenario: scenario.clone(),
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

pub fn incident_model() -> String {
    std::env::var("RKAT_INCIDENT_MODEL").unwrap_or_else(|_| "gpt-5.5".to_string())
}

fn incident_definition() -> Result<MobDefinition> {
    let model = incident_model();
    MobDefinition::from_toml(&format!(
        r#"
[mob]
id = "incident-command-center"

[skills.comms_protocol]
source = "inline"
content = """
## Incident Comms Protocol

- Ignore mob.peer_added and mob.peer_retired lifecycle chatter. Do not reply to those notices.
- Use the `peers` tool before your first substantive peer message in a turn so you know who is reachable.
- Use `peer_request` when you need another teammate to answer a question or return facts to you.
- Use `peer_response` with `in_reply_to` when answering a peer request.
- When sending `peer_response`, put the actual answer in the `result` payload as plain fields like `summary`, `status_line`, or `facts`; do not send an empty result object.
- Use `peer_message` only for one-way updates or FYIs that do not require a reply.
- When responding to a teammate, send the answer back over comms. Do not keep the answer local-only.
- Keep peer messages short and factual: 1-3 sentences or tight bullets.
- If you learn a new material fact, notify scribe unless you are scribe.
"""

[skills.commander_role]
source = "inline"
content = """
You are the incident commander for the fictional CardinalPay payments outage.

TEAMMATES YOU SHOULD USE:
- payments-sre: live service health, mitigations, rollback safety
- api-investigator: root cause, blast radius, rollback reasoning
- merchant-comms: external and status-page wording
- merchant-success: VIP/customer-friendly phrasing
- scribe: timeline and established facts

HOW TO OPERATE:
- For any operator request about current status, customer impact, rollback, or publication readiness, use `peers` first and then send concise `peer_request` questions to at least two relevant teammates before you finalize your answer.
- Default coordination path for a status sweep: payments-sre + merchant-comms + scribe.
- Ask api-investigator whenever root cause or rollback confidence is part of the question.
- After a meaningful exchange, send a short factual note to scribe.
- When a teammate replies with `peer_response`, read the actual answer from the response payload fields such as `result.summary`, `result.status_line`, or `result.facts`. Do not treat a completed response as empty if those fields are present.
- Your final operator answer should be concise, operationally useful, and mention which teammates you consulted.
- Do not pretend a peer confirmed something if they have not replied.
"""

[skills.payments_sre_role]
source = "inline"
content = """
You are Payments SRE for the fictional CardinalPay incident.

JOB:
- Own the live technical posture of the payments-api and mitigation safety.
- When commander or api-investigator asks for status, run inspect_service for payments-api before you answer unless you just did so.
- Reply with terse facts: current health, likely risk, and the next safe action.
- If the request expects a reply, answer with `peer_response` and put the actual sentence in `result.summary` or `result.status_line`.
- If you uncover a material fact, send a short `peer_message` note to scribe.
- If you need root-cause help and expect a direct answer, ask api-investigator via `send` using `kind: "peer_request"` and put the question in the body.
"""

[skills.api_investigator_role]
source = "inline"
content = """
You are the API investigator for the fictional CardinalPay incident.

JOB:
- Focus on root cause, blast radius, and rollback reasoning.
- If commander asks for root cause or rollback confidence and you need fresh technical confirmation, consult payments-sre via `send` using `kind: "peer_request"`.
- If the requester expects a reply, answer with `peer_response` and put the actual answer in `result.summary`; otherwise send concise findings back to commander via `peer_message`.
- Send a short timeline fact to scribe.
- Keep your replies analytical and specific; do not draft customer copy.
"""

[skills.merchant_comms_role]
source = "inline"
content = """
You are Merchant Comms for the fictional CardinalPay incident.

JOB:
- Draft status-page and merchant-facing wording.
- When commander asks for an external update, coordinate with merchant-success for customer wording via `peer_request` and send approval-gate a concise publication request via `peer_message`.
- If commander explicitly requested a reply, use `peer_response` and put the wording summary in `result.summary`; otherwise report back to commander with the latest draft and approval state.
- Send the approved or pending wording summary to scribe.
"""

[skills.merchant_success_role]
source = "inline"
content = """
You are Merchant Success for the fictional CardinalPay incident.

JOB:
- Translate incident facts into concise customer/VIP messaging.
- When asked for merchant wording and context is stale, ask merchant-comms or incident-commander first via `peer_request`.
- Keep updates calm, concrete, and short.
- If replying to a peer request, place the actual sentence in `result.summary`.
- Send important customer-impact facts to scribe.
"""

[skills.scribe_role]
source = "inline"
content = """
You are the incident scribe for the fictional CardinalPay incident.

JOB:
- Maintain the running timeline of confirmed facts.
- When a peer sends a `peer_request` asking for a summary or facts, you must answer with `peer_response`, include `in_reply_to`, and put the actual answer text in `result.summary`.
- When a peer sends a one-way `peer_message` update, acknowledge it locally and update your timeline, but do not assume they are waiting on a reply unless they asked.
- If you cannot identify the sender from the comms notice, say that explicitly; otherwise always send the reply.
- When commander asks what is currently established, answer with the tightest fact pattern you have.
- Do not invent operational actions; you summarize and confirm.
"""

[skills.approval_gate_role]
source = "inline"
content = """
You are the internal approval gate for the fictional CardinalPay incident.

JOB:
- Review publication requests from merchant-comms.
- Reply only to the requesting peer with a `peer_message` containing approve, reject, or escalate plus a short reason.
- Never act like an operator-facing assistant.
"""

[skills.health_monitor_role]
source = "inline"
content = """
You are the internal health monitor for the fictional CardinalPay incident.

JOB:
- Provide terse machine-style health facts when peers ask.
- Run inspect_service if a peer asks about live service condition.
- If the state is severe, notify incident-commander and scribe via `peer_message`.
- Never act like an operator-facing assistant.
"""

[profiles.commander]
model = "{model}"
external_addressable = true
runtime_mode = "autonomous_host"
skills = ["comms_protocol", "commander_role"]
peer_description = "Incident commander coordinating the CardinalPay outage response"

[profiles.commander.tools]
builtins = true
comms = true
image_generation = true

[profiles.payments_sre]
model = "{model}"
external_addressable = true
runtime_mode = "autonomous_host"
skills = ["comms_protocol", "payments_sre_role"]
peer_description = "Payments SRE handling technical status and mitigation safety"

[profiles.payments_sre.tools]
comms = true

[profiles.api_investigator]
model = "{model}"
external_addressable = true
runtime_mode = "autonomous_host"
skills = ["comms_protocol", "api_investigator_role"]
peer_description = "API investigator focused on root cause and rollback confidence"

[profiles.api_investigator.tools]
comms = true

[profiles.merchant_comms]
model = "{model}"
external_addressable = true
runtime_mode = "autonomous_host"
skills = ["comms_protocol", "merchant_comms_role"]
peer_description = "Merchant communications lead for status-page and publication wording"

[profiles.merchant_comms.tools]
comms = true

[profiles.merchant_success]
model = "{model}"
external_addressable = true
runtime_mode = "autonomous_host"
skills = ["comms_protocol", "merchant_success_role"]
peer_description = "Merchant success lead for VIP/customer updates"

[profiles.merchant_success.tools]
comms = true

[profiles.scribe]
model = "{model}"
external_addressable = true
runtime_mode = "autonomous_host"
skills = ["comms_protocol", "scribe_role"]
peer_description = "Incident scribe maintaining the confirmed timeline"

[profiles.scribe.tools]
comms = true

[profiles.approval_gate]
model = "{model}"
external_addressable = false
runtime_mode = "autonomous_host"
skills = ["comms_protocol", "approval_gate_role"]
peer_description = "Internal approval gate for publication requests"

[profiles.approval_gate.tools]
comms = true

[profiles.health_monitor]
model = "{model}"
external_addressable = false
runtime_mode = "autonomous_host"
skills = ["comms_protocol", "health_monitor_role"]
peer_description = "Internal health monitor for the incident"

[profiles.health_monitor.tools]
comms = true
"#,
    ))
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
        active_members: Vec<crate::unified_runtime::edge_types::EdgeMemberView>,
    ) -> Pin<Box<dyn futures::Future<Output = Vec<DesiredPeerEdge>> + Send + '_>> {
        let desired = Arc::clone(&self.desired);
        Box::pin(async move {
            let active = active_members
                .into_iter()
                .map(|member| member.agent_identity)
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
                "You may still collaborate with peers over the comms tools when they ask for approval, health, or internal control-plane help.".to_string(),
            ]);
        } else {
            build.additional_instructions = Some(vec![
                "You are part of a synthetic incident command center for a fictional payments outage. Stay within that scenario and never claim real-world access.".to_string(),
                "Be concise and operator-focused. Use one short paragraph unless the operator explicitly asks for more.".to_string(),
                "Use the stock comms tools for real collaboration: call peers first when you need teammate context, then send concise requests or updates.".to_string(),
                "When the operator asks for a status sweep, you must run both available tools before answering: inspect_service with service=payments-api and analyze_customer_impact with cohort=enterprise-merchants.".to_string(),
                "When the operator asks a short follow-up, answer directly from current context, but if the question depends on another specialist's knowledge you should consult that peer instead of guessing.".to_string(),
            ]);
        }
        Ok(())
    }

    async fn after_create(
        &self,
        session_id: &meerkat_core::types::SessionId,
        ctx: &SessionCreatedContext,
    ) {
        if std::env::var_os("MOBKIT_TRACE_INCIDENT_STARTUP").is_some() {
            tracing::warn!(
                session_id = %session_id,
                model = %ctx.model,
                labels = ?ctx.labels,
                "incident session created"
            );
        }
    }
}

#[derive(Clone)]
struct IncidentToolDispatcher;

#[async_trait]
impl AgentToolDispatcher for IncidentToolDispatcher {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        vec![
            Arc::new(ToolDef {
                name: "inspect_service".into(),
                description: "Inspect the current health and saturation of a named service"
                    .to_string(),
                input_schema: meerkat_tools::schema_for::<InspectServiceArgs>(),
                provenance: None,
            }),
            Arc::new(ToolDef {
                name: "analyze_customer_impact".into(),
                description: "Estimate customer-facing impact for a named merchant cohort"
                    .to_string(),
                input_schema: meerkat_tools::schema_for::<AnalyzeImpactArgs>(),
                provenance: None,
            }),
        ]
        .into()
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        if std::env::var_os("MOBKIT_TRACE_INCIDENT_STARTUP").is_some() {
            tracing::warn!(
                tool = %call.name,
                tool_call_id = %call.id,
                "incident tool dispatch"
            );
        }
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use futures::Stream;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::mob_handle_runtime::take_runtime_turn_traces;
    use meerkat_client::{LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
    use meerkat_core::StopReason;
    use meerkat_core::comms::{CommsCommand, SendReceipt};

    #[derive(Default)]
    struct CountingClient {
        stream_calls: AtomicUsize,
    }

    impl CountingClient {
        fn calls(&self) -> usize {
            self.stream_calls.load(Ordering::SeqCst)
        }
    }

    #[derive(Default)]
    struct RecordingClient {
        stream_calls: AtomicUsize,
        prompts: Mutex<Vec<String>>,
    }

    impl RecordingClient {
        fn calls(&self) -> usize {
            self.stream_calls.load(Ordering::SeqCst)
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().expect("recording client mutex").clone()
        }
    }

    impl LlmClient for RecordingClient {
        fn stream<'a>(
            &'a self,
            request: &'a LlmRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>> {
            self.stream_calls.fetch_add(1, Ordering::SeqCst);
            let rendered = request
                .messages
                .iter()
                .map(|message| format!("{message:?}"))
                .collect::<Vec<_>>()
                .join("\n---\n");
            self.prompts
                .lock()
                .expect("recording client mutex")
                .push(rendered);
            Box::pin(async_stream::stream! {
                yield Ok(LlmEvent::TextDelta {
                    delta: "ok".to_string(),
                    meta: None,
                });
                yield Ok(LlmEvent::Done {
                    outcome: LlmDoneOutcome::Success {
                        stop_reason: StopReason::EndTurn,
                    },
                });
            })
        }

        fn provider(&self) -> &'static str {
            "incident-recording-test"
        }

        fn health_check<'life0, 'async_trait>(
            &'life0 self,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<(), LlmError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(()) })
        }
    }

    impl LlmClient for CountingClient {
        fn stream<'a>(
            &'a self,
            _request: &'a LlmRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>> {
            self.stream_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async_stream::stream! {
                yield Ok(LlmEvent::TextDelta {
                    delta: "ok".to_string(),
                    meta: None,
                });
                yield Ok(LlmEvent::Done {
                    outcome: LlmDoneOutcome::Success {
                        stop_reason: StopReason::EndTurn,
                    },
                });
            })
        }

        fn provider(&self) -> &'static str {
            "incident-counting-test"
        }

        fn health_check<'life0, 'async_trait>(
            &'life0 self,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<(), LlmError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn incident_definition_includes_role_specific_profiles_and_skills() {
        let definition = incident_definition().expect("incident definition should parse");
        assert!(definition.skills.contains_key("comms_protocol"));
        assert!(definition.skills.contains_key("commander_role"));
        assert!(definition.skills.contains_key("merchant_comms_role"));
        assert!(
            definition
                .profiles
                .contains_key(&ProfileName::from("commander"))
        );
        assert!(
            definition
                .profiles
                .contains_key(&ProfileName::from("payments_sre"))
        );
        assert!(
            definition
                .profiles
                .contains_key(&ProfileName::from("merchant_comms"))
        );
        assert!(
            definition
                .profiles
                .contains_key(&ProfileName::from("approval_gate"))
        );
        let commander = definition
            .profiles
            .get(&ProfileName::from("commander"))
            .expect("commander profile present")
            .as_inline()
            .expect("commander is an inline profile");
        assert_eq!(
            commander.runtime_mode.to_string(),
            "autonomous_host",
            "incident commander must be a long-running autonomous host so peer replies are drained while idle",
        );
    }

    #[test]
    fn incident_scenario_uses_role_specific_profiles() {
        let scenario =
            load_scenario(&scenario_path().expect("scenario path")).expect("scenario loads");
        assert!(
            scenario
                .identities
                .iter()
                .any(|identity| identity.profile == "commander")
        );
        assert!(
            scenario
                .identities
                .iter()
                .any(|identity| identity.profile == "payments_sre")
        );
        assert!(
            scenario
                .identities
                .iter()
                .any(|identity| identity.profile == "merchant_comms")
        );
        assert!(
            scenario
                .identities
                .iter()
                .any(|identity| identity.profile == "approval_gate")
        );
    }

    #[tokio::test]
    #[ignore = "diagnostic: measure whether incident startup keeps creating new LLM turns while idle"]
    async fn incident_idle_does_not_keep_starting_new_llm_turns() {
        let scenario =
            load_scenario(&scenario_path().expect("scenario path")).expect("scenario loads");
        let client = Arc::new(CountingClient::default());
        let bundle = build_runtime_bundle_with_client(&scenario, Some(client.clone()))
            .await
            .expect("incident runtime bundle");

        tokio::time::sleep(Duration::from_millis(250)).await;
        let after_250ms = client.calls();
        tokio::time::sleep(Duration::from_millis(750)).await;
        let after_1s = client.calls();

        eprintln!(
            "incident idle llm call counts: after_250ms={}, after_1s={}",
            after_250ms, after_1s
        );

        bundle
            .runtime
            .mob_handle()
            .stop()
            .await
            .map_err(crate::mob_handle_runtime::MobRuntimeError::from)
            .expect("stop runtime");

        assert!(
            after_1s <= after_250ms,
            "idle incident runtime kept starting new LLM turns after startup: {} -> {}",
            after_250ms,
            after_1s
        );
    }

    #[tokio::test]
    #[ignore = "diagnostic: compare startup and idle turn counts with and without peer wiring"]
    async fn incident_idle_turn_counts_with_vs_without_links() {
        async fn measure_calls(scenario: IncidentScenario) -> (usize, usize, usize) {
            let client = Arc::new(CountingClient::default());
            let bundle = build_runtime_bundle_with_client(&scenario, Some(client.clone()))
                .await
                .expect("incident runtime bundle");

            tokio::time::sleep(Duration::from_millis(250)).await;
            let after_250ms = client.calls();
            tokio::time::sleep(Duration::from_millis(750)).await;
            let after_1s = client.calls();
            tokio::time::sleep(Duration::from_secs(4)).await;
            let after_5s = client.calls();

            bundle
                .runtime
                .mob_handle()
                .stop()
                .await
                .map_err(crate::mob_handle_runtime::MobRuntimeError::from)
                .expect("stop runtime");
            (after_250ms, after_1s, after_5s)
        }

        let scenario =
            load_scenario(&scenario_path().expect("scenario path")).expect("scenario loads");
        let linked = measure_calls(scenario.clone()).await;

        let mut unlinked_scenario = scenario;
        unlinked_scenario.links.clear();
        let unlinked = measure_calls(unlinked_scenario).await;

        eprintln!(
            "incident idle llm call counts with links: 250ms={}, 1s={}, 5s={}; without links: 250ms={}, 1s={}, 5s={}",
            linked.0, linked.1, linked.2, unlinked.0, unlinked.1, unlinked.2
        );

        assert!(
            linked.2 <= linked.1,
            "linked incident runtime kept starting new LLM turns after 1s: {} -> {}",
            linked.1,
            linked.2
        );
        assert!(
            unlinked.2 <= unlinked.1,
            "unlinked incident runtime kept starting new LLM turns after 1s: {} -> {}",
            unlinked.1,
            unlinked.2
        );
    }

    #[tokio::test]
    #[ignore = "diagnostic: capture which prompts drive unsolicited startup turns"]
    async fn incident_startup_turn_origins() {
        let scenario =
            load_scenario(&scenario_path().expect("scenario path")).expect("scenario loads");
        let client = Arc::new(RecordingClient::default());
        let _ = take_runtime_turn_traces();
        let bundle = build_runtime_bundle_with_client(&scenario, Some(client.clone()))
            .await
            .expect("incident runtime bundle");

        tokio::time::sleep(Duration::from_secs(1)).await;

        let calls = client.calls();
        let prompts = client.prompts();
        let traces = take_runtime_turn_traces();
        let runtime_handle = bundle.runtime.mob_handle();
        let entries = runtime_handle.list_members_including_retiring().await;
        let mut identities_by_session: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        for entry in entries {
            if let Some(session_id) = runtime_handle
                .resolve_bridge_session_id(&entry.agent_identity)
                .await
            {
                identities_by_session
                    .insert(session_id.to_string(), entry.agent_identity.to_string());
            }
        }

        eprintln!("incident startup turn count: {}", calls);
        eprintln!("incident startup runtime apply traces: {}", traces.len());
        for trace in &traces {
            let identity = identities_by_session
                .get(&trace.session_id)
                .cloned()
                .unwrap_or_else(|| "<unknown>".to_string());
            eprintln!(
                "trace session={} identity={} boundary={} contributing_inputs={} outcome={}",
                trace.session_id,
                identity,
                trace.boundary,
                trace.contributing_input_count,
                trace.outcome
            );
        }
        for (index, prompt) in prompts.iter().enumerate() {
            let head = prompt.lines().take(12).collect::<Vec<_>>().join("\n");
            eprintln!("--- startup prompt {} ---\n{}\n", index + 1, head);
        }

        bundle
            .runtime
            .mob_handle()
            .stop()
            .await
            .map_err(crate::mob_handle_runtime::MobRuntimeError::from)
            .expect("stop runtime");

        assert_eq!(
            calls,
            prompts.len(),
            "recorded prompt count should match stream call count"
        );
    }

    #[tokio::test]
    #[ignore = "timing-sensitive: commander may still be processing when assertion fires"]
    async fn incident_terminal_peer_response_advances_commander_session() {
        let scenario =
            load_scenario(&scenario_path().expect("scenario path")).expect("scenario loads");
        let client = Arc::new(CountingClient::default());
        let bundle = build_runtime_bundle_with_client(&scenario, Some(client.clone()))
            .await
            .expect("incident runtime bundle");

        tokio::time::sleep(Duration::from_millis(300)).await;

        let commander_session_id = bundle
            .runtime
            .mob_handle()
            .resolve_bridge_session_id(&meerkat_mob::ids::MeerkatId::from("incident-commander"))
            .await
            .expect("commander session")
            .to_string();
        let scribe_session_id = bundle
            .runtime
            .mob_handle()
            .resolve_bridge_session_id(&meerkat_mob::ids::MeerkatId::from("scribe"))
            .await
            .expect("scribe session")
            .to_string();

        let commander_state_before = bundle
            .runtime
            .mob_runtime()
            .runtime_state_for_session(&commander_session_id)
            .await
            .expect("commander runtime state before")
            .expect("commander runtime state present");
        let commander_active_inputs_before = bundle
            .runtime
            .mob_runtime()
            .active_input_ids_for_session(&commander_session_id)
            .await
            .expect("commander active inputs before")
            .expect("commander active inputs present");

        let commander_history_before = bundle
            .runtime
            .mob_runtime()
            .read_session_history(&commander_session_id, 0, Some(200))
            .await
            .expect("commander history before");
        let commander_count_before = commander_history_before.messages.len();

        let commander_comms = bundle
            .runtime
            .mob_runtime()
            .comms_runtime_for_session(&commander_session_id)
            .await
            .expect("commander comms runtime")
            .expect("commander comms runtime present");
        let scribe_comms = bundle
            .runtime
            .mob_runtime()
            .comms_runtime_for_session(&scribe_session_id)
            .await
            .expect("scribe comms runtime")
            .expect("scribe comms runtime present");

        let scribe_peer_route = commander_comms
            .peers()
            .await
            .into_iter()
            .find(|entry| entry.name.as_str().contains("/scribe/"))
            .map(|entry| {
                meerkat_core::comms::PeerRoute::with_display_name(entry.peer_id, entry.name)
            })
            .expect("scribe peer visible to commander");
        let commander_peer_route = scribe_comms
            .peers()
            .await
            .into_iter()
            .find(|entry| entry.name.as_str().contains("/commander/"))
            .map(|entry| {
                meerkat_core::comms::PeerRoute::with_display_name(entry.peer_id, entry.name)
            })
            .expect("commander peer visible to scribe");

        let request_receipt = commander_comms
            .send(CommsCommand::PeerRequest {
                to: scribe_peer_route,
                intent: "request_summary".to_string(),
                params: json!({ "body": "Summarize the incident." }),
                handling_mode: meerkat_core::types::HandlingMode::Queue,
                stream: meerkat_core::comms::InputStreamMode::None,
            })
            .await
            .expect("send request to scribe");
        let request_id = match request_receipt {
            SendReceipt::PeerRequestSent { interaction_id, .. } => interaction_id,
            other => panic!("expected peer request receipt, got {other:?}"),
        };

        let response_receipt = scribe_comms
            .send(CommsCommand::PeerResponse {
                to: commander_peer_route,
                in_reply_to: request_id,
                status: meerkat_core::ResponseStatus::Completed,
                result: json!({
                    "summary": "Scribe reply from test harness",
                }),
                handling_mode: Some(meerkat_core::types::HandlingMode::Queue),
            })
            .await
            .expect("send response to commander");
        match response_receipt {
            SendReceipt::PeerResponseSent { in_reply_to, .. } => {
                assert_eq!(in_reply_to, request_id);
            }
            other => panic!("expected peer response receipt, got {other:?}"),
        }

        tokio::time::sleep(Duration::from_millis(750)).await;

        let lingering_commander_inbox = commander_comms.drain_inbox_interactions().await;

        let commander_session_id_after = bundle
            .runtime
            .mob_handle()
            .resolve_bridge_session_id(&meerkat_mob::ids::MeerkatId::from("incident-commander"))
            .await
            .expect("commander session after")
            .to_string();
        let commander_state_after = bundle
            .runtime
            .mob_runtime()
            .runtime_state_for_session(&commander_session_id_after)
            .await
            .expect("commander runtime state after")
            .expect("commander runtime state present");
        let commander_active_inputs_after = bundle
            .runtime
            .mob_runtime()
            .active_input_ids_for_session(&commander_session_id_after)
            .await
            .expect("commander active inputs after")
            .expect("commander active inputs present");
        let commander_history_after = bundle
            .runtime
            .mob_runtime()
            .read_session_history(&commander_session_id_after, 0, Some(200))
            .await
            .expect("commander history after");
        let commander_count_after = commander_history_after.messages.len();
        let history_dump = commander_history_after
            .messages
            .iter()
            .map(|message| format!("{message:?}"))
            .collect::<Vec<_>>()
            .join("\n---\n");

        bundle
            .runtime
            .mob_handle()
            .stop()
            .await
            .map_err(crate::mob_handle_runtime::MobRuntimeError::from)
            .expect("stop runtime");

        assert!(
            commander_count_after > commander_count_before,
            "commander history did not advance after peer response; session_before={commander_session_id} session_after={commander_session_id_after} state_before={commander_state_before:?} state_after={commander_state_after:?} active_before={commander_active_inputs_before:?} active_after={commander_active_inputs_after:?} lingering_inbox={lingering_commander_inbox:?}\n{history_dump}"
        );
        assert!(
            history_dump.contains("Scribe reply from test harness") || client.calls() > 7,
            "commander did not appear to process the peer response; session_before={commander_session_id} session_after={commander_session_id_after} state_before={commander_state_before:?} state_after={commander_state_after:?} active_before={commander_active_inputs_before:?} active_after={commander_active_inputs_after:?} lingering_inbox={lingering_commander_inbox:?}\n{history_dump}"
        );
    }
}
