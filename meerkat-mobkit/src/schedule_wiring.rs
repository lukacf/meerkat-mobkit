//! Wire meerkat's per-session schedule tools + firing host into the gateways.
//!
//! A mob profile's `tools.schedule = true` maps (in `meerkat-mob`) to
//! `override_schedule = Enable`, which already resolves on. But the schedule
//! tools only compose if the agent factory's `default_schedule_tools` slot
//! holds a dispatcher — and the mobkit gateways never populated it, so the
//! `meerkat_schedule_*` surface was silently absent even when the capability
//! was declared and allow-listed.
//!
//! [`attach_schedule_tools`] fills that slot (members can author schedules) and
//! [`spawn_schedule_host`] runs the runtime-backed driver (authored schedules
//! actually fire: at due time it materializes a session and runs the prompt as
//! a real agent turn). Both back onto a single durable [`ScheduleService`].
//!
//! [`steward_dream_runnable_host`] + [`ensure_steward_dream_schedule`] register
//! the memory steward's dream as a host-runnable schedule target (§8.5 /
//! upstream ask 7), so the dream fires as a durable, misfire-aware occurrence
//! through the same host instead of a bare in-process interval loop.
//!
//! This is distinct from the static cron oracle (`MobKitBuilder::scheduling`),
//! which drives module-dispatch ticks, not per-agent schedule tools.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use meerkat::surface::{
    NoopScheduleMobHost, ScheduleHostHandle, SurfaceScheduleMobHost, immediate_completed_dispatch,
    immediate_delivery_failure, parse_mob_member_schedule_identity,
    spawn_runtime_backed_schedule_host_with_mobs,
};
use meerkat::{
    Config, CreateScheduleRequest, FactoryAgentBuilder, HostRunnable, HostRunnableError,
    HostRunnableInvocation, HostRunnableName, HostRunnableOutcome, HostRunnableRegistry,
    HostRunnableTargetBinding, IntervalTriggerSpec, MobTargetBinding, Occurrence, OccurrenceFilter,
    OccurrencePhase, PersistentSessionService, Schedule, ScheduleRunnableHost, ScheduleService,
    ScheduleToolDispatcher, ScheduledMobAction, ScheduledSessionAction, SessionAgentBuilder,
    SessionTargetBinding, SqliteScheduleStore, TargetBinding, TriggerSpec, UpdateScheduleRequest,
};
use meerkat_core::service::SessionBuildOptions;
use meerkat_mob::runtime::MobHandle;
use meerkat_mob_mcp::{MobMcpScheduleHost, MobMcpState};
use meerkat_runtime::MeerkatMachine;
use serde_json::{Map, Value};

use crate::identity_first::gateway_bridges::CallbackBridge;
use crate::memory::steward::StewardEngine;

/// File name for the durable schedule store, kept beside the runtime DB so a
/// gateway and a library-mode runtime pointed at the same dir share state.
pub const SCHEDULE_STORE_FILE: &str = "schedule.sqlite";

/// JSON-RPC callback method the gateway sends to the SDK host when a
/// host-runnable schedule target registered via
/// `runtime_options.host_runnables` fires. Params:
/// `{runnable, occurrence: {schedule_id, occurrence_id, due_at, payload?}}`.
pub const SCHEDULE_FIRE_CALLBACK_METHOD: &str = "callback/schedule_fire";

/// Reserved host-runnable name for the memory steward's scheduled dream
/// (§8.5 / upstream ask 7). Stable across boots: the find-or-create schedule
/// keys idempotency on a target that names this runnable.
pub const STEWARD_DREAM_RUNNABLE: &str = "mobkit.memory.steward.dream";

/// Schedule name for the steward dream, used only for operator legibility in
/// schedule listings — idempotency keys on the host-runnable target, not this.
const STEWARD_DREAM_SCHEDULE_NAME: &str = "mobkit-memory-steward-dream";

/// A [`HostRunnable`] that runs one guarded steward dream attempt. Registered
/// with the schedule host so the dream fires as a durable, misfire-aware
/// schedule occurrence instead of a bare in-process interval loop.
struct StewardDreamRunnable {
    engine: Arc<StewardEngine>,
}

#[async_trait]
impl HostRunnable for StewardDreamRunnable {
    async fn run(
        &self,
        _invocation: HostRunnableInvocation,
    ) -> Result<HostRunnableOutcome, HostRunnableError> {
        // `dream_now` is fully self-gating (enabled / min-signals / budget)
        // and never returns an error — a skipped dream is a normal outcome,
        // not an occurrence failure. Always report completion so the
        // occurrence lands terminal-complete.
        self.engine.dream_now().await;
        Ok(HostRunnableOutcome::completed())
    }
}

/// Build a host-runnable registry exposing the steward's dream, when a steward
/// engine is present. Returned as `Arc<dyn ScheduleRunnableHost>` for
/// [`spawn_schedule_host`]'s `runnable_host`; `None` when there is no steward
/// to drive (the schedule host then serves session/mob targets only).
#[must_use]
// The runnable name is a non-empty const and registration into a fresh
// registry cannot duplicate — both expects are structurally infallible.
#[allow(clippy::expect_used)]
pub fn steward_dream_runnable_host(
    steward: Option<Arc<StewardEngine>>,
) -> Option<Arc<dyn ScheduleRunnableHost>> {
    let engine = steward?;
    let mut registry = HostRunnableRegistry::new();
    let name = HostRunnableName::parse(STEWARD_DREAM_RUNNABLE)
        .expect("STEWARD_DREAM_RUNNABLE is a non-empty runnable name");
    registry
        .register(name, Arc::new(StewardDreamRunnable { engine }))
        .expect("first registration into a fresh registry cannot duplicate");
    Some(Arc::new(registry))
}

/// A [`HostRunnable`] whose fire forwards over the SDK callback bridge as
/// [`SCHEDULE_FIRE_CALLBACK_METHOD`], so the app process (Python/TypeScript)
/// runs the occurrence deterministically — no LLM turn is involved.
///
/// A bridge error (handler raised, no handler registered for the name, host
/// gone, callback timeout) is reported as a typed runnable FAILURE so the
/// schedule store records the occurrence attempt as failed rather than
/// silently completing. The app's JSON result is logged and dropped:
/// upstream's `HostRunnableOutcome` deliberately carries no success payload
/// (occurrence completion consumes no detail).
struct CallbackHostRunnable {
    bridge: Arc<dyn CallbackBridge>,
}

#[async_trait]
impl HostRunnable for CallbackHostRunnable {
    async fn run(
        &self,
        invocation: HostRunnableInvocation,
    ) -> Result<HostRunnableOutcome, HostRunnableError> {
        let mut occurrence = Map::new();
        occurrence.insert(
            "schedule_id".to_string(),
            Value::String(invocation.schedule_id.to_string()),
        );
        occurrence.insert(
            "occurrence_id".to_string(),
            Value::String(invocation.occurrence_id.to_string()),
        );
        occurrence.insert(
            "due_at".to_string(),
            Value::String(invocation.trigger_time.to_rfc3339()),
        );
        if let Some(params) = invocation.params.as_deref() {
            match serde_json::from_str::<Value>(params.get()) {
                Ok(payload) => {
                    occurrence.insert("payload".to_string(), payload);
                }
                Err(error) => {
                    // The binding canonicalizes params at every ingress, so a
                    // non-JSON payload here is store corruption — fail the
                    // occurrence rather than firing with silently dropped params.
                    return Err(HostRunnableError::Failed {
                        detail: format!("host-runnable params are not valid JSON: {error}"),
                    });
                }
            }
        }
        let params = serde_json::json!({
            "runnable": invocation.runnable.as_str(),
            "occurrence": Value::Object(occurrence),
        });
        match self
            .bridge
            .call(SCHEDULE_FIRE_CALLBACK_METHOD, params)
            .await
        {
            Ok(result) => {
                tracing::debug!(
                    runnable = %invocation.runnable,
                    occurrence_id = %invocation.occurrence_id,
                    %result,
                    "callback schedule fire completed"
                );
                Ok(HostRunnableOutcome::completed())
            }
            Err(detail) => Err(HostRunnableError::Failed { detail }),
        }
    }
}

/// Compose the gateway's schedule runnable host: the steward dream (when a
/// steward engine is present) plus SDK-registered callback runnables
/// (`runtime_options.host_runnables`), each forwarding its fire over the
/// callback bridge as [`SCHEDULE_FIRE_CALLBACK_METHOD`].
///
/// `Ok(None)` when there is nothing to register (the schedule host then
/// serves session/mob targets only). `Err` on a duplicate runnable name —
/// including a callback runnable colliding with the reserved
/// [`STEWARD_DREAM_RUNNABLE`] name.
// The steward runnable name is a non-empty const, so its parse is infallible.
#[allow(clippy::expect_used)]
pub fn gateway_runnable_host(
    steward: Option<Arc<StewardEngine>>,
    callback_runnables: Option<(Arc<dyn CallbackBridge>, Vec<HostRunnableName>)>,
) -> Result<Option<Arc<dyn ScheduleRunnableHost>>, String> {
    let mut registry = HostRunnableRegistry::new();
    let mut registered = false;
    if let Some(engine) = steward {
        let name = HostRunnableName::parse(STEWARD_DREAM_RUNNABLE)
            .expect("STEWARD_DREAM_RUNNABLE is a non-empty runnable name");
        registry
            .register(name, Arc::new(StewardDreamRunnable { engine }))
            .map_err(|error| error.to_string())?;
        registered = true;
    }
    if let Some((bridge, names)) = callback_runnables {
        for name in names {
            registry
                .register(
                    name,
                    Arc::new(CallbackHostRunnable {
                        bridge: Arc::clone(&bridge),
                    }),
                )
                .map_err(|error| error.to_string())?;
            registered = true;
        }
    }
    Ok(registered.then(|| Arc::new(registry) as Arc<dyn ScheduleRunnableHost>))
}

/// Find-or-create the durable schedule that drives the steward dream runnable
/// at `cadence`. Idempotent across boots against the persistent schedule
/// store: keyed on the reserved host-runnable target, so a restart reuses the
/// existing schedule (and its planning cursor) rather than stacking duplicate
/// dreams. When the configured cadence changed, the existing schedule's
/// interval is updated in place; otherwise it is left untouched.
// The runnable name is a non-empty const, so its parse is infallible.
#[allow(clippy::expect_used)]
pub async fn ensure_steward_dream_schedule(
    service: &ScheduleService,
    cadence: Duration,
    now_utc: chrono::DateTime<chrono::Utc>,
) -> Result<(), String> {
    let every_seconds = cadence.as_secs().max(1);
    let runnable = HostRunnableName::parse(STEWARD_DREAM_RUNNABLE)
        .expect("STEWARD_DREAM_RUNNABLE is a non-empty runnable name");

    let schedules = service
        .list()
        .await
        .map_err(|error| format!("list schedules: {error}"))?;
    let existing = schedules.into_iter().find(|schedule| {
        matches!(
            &schedule.target,
            TargetBinding::HostRunnable(binding) if binding.runnable == runnable
        )
    });

    // First occurrence one interval out (parity with the old loop's initial
    // sleep-then-dream) so a fresh gateway does not dream immediately on boot.
    let start_at_utc = now_utc + chrono::Duration::seconds(every_seconds as i64);
    let trigger = TriggerSpec::Interval(IntervalTriggerSpec {
        start_at_utc,
        every_seconds,
        end_at_utc: None,
    });
    let target = TargetBinding::host_runnable(HostRunnableTargetBinding {
        runnable,
        params: None,
    });

    match existing {
        Some(schedule) => {
            let cadence_unchanged = matches!(
                &schedule.trigger,
                TriggerSpec::Interval(spec) if spec.every_seconds == every_seconds
            );
            if cadence_unchanged {
                return Ok(());
            }
            service
                .update(
                    &schedule.schedule_id,
                    UpdateScheduleRequest {
                        trigger: Some(trigger),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|error| format!("update steward dream schedule cadence: {error}"))?;
            tracing::info!(
                schedule_id = %schedule.schedule_id,
                every_seconds,
                "updated steward dream schedule cadence"
            );
            Ok(())
        }
        None => {
            let created = service
                .create(CreateScheduleRequest {
                    name: Some(STEWARD_DREAM_SCHEDULE_NAME.to_string()),
                    description: Some(
                        "MobKit memory steward dream (host-runnable target)".to_string(),
                    ),
                    trigger,
                    target,
                    misfire_policy: meerkat::MisfirePolicy::default(),
                    overlap_policy: meerkat::OverlapPolicy::default(),
                    missing_target_policy: meerkat::MissingTargetPolicy::default(),
                    labels: std::collections::BTreeMap::new(),
                    planning_horizon_days: None,
                    planning_horizon_occurrences: None,
                })
                .await
                .map_err(|error| format!("create steward dream schedule: {error}"))?;
            tracing::info!(
                schedule_id = %created.schedule_id,
                every_seconds,
                "created steward dream schedule (host-runnable target)"
            );
            Ok(())
        }
    }
}

#[derive(Clone, Default)]
pub struct ScheduleMobTargetRegistry {
    state: Arc<RwLock<Option<Arc<MobMcpState>>>>,
}

impl ScheduleMobTargetRegistry {
    pub fn set_mob_state(&self, state: Option<Arc<MobMcpState>>) {
        if let Ok(mut guard) = self.state.write() {
            *guard = state;
        }
    }

    async fn resolve_bridge_session(&self, session_id: &str) -> Option<(String, String)> {
        let state = self.state.read().ok().and_then(|guard| guard.clone())?;
        let parsed_session_id = meerkat::SessionId::parse(session_id).ok()?;
        let handles = state.mob_handles_snapshot().await.ok()?;
        for (mob_id, handle) in handles {
            let roster = handle.roster().await;
            if let Some(entry) = roster.find_by_bridge_session_id(&parsed_session_id) {
                return Some((mob_id.to_string(), entry.agent_identity.to_string()));
            }
        }
        None
    }
}

pub struct AttachedScheduleTools {
    pub service: ScheduleService,
    pub mob_target_registry: ScheduleMobTargetRegistry,
}

type MobSessionResolver = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Option<(String, String)>> + Send>> + Send + Sync,
>;

struct MobIdentityScheduleToolDispatcher {
    inner: Arc<dyn meerkat_core::AgentToolDispatcher>,
    resolve_mob_session: MobSessionResolver,
}

impl MobIdentityScheduleToolDispatcher {
    fn new(
        inner: Arc<dyn meerkat_core::AgentToolDispatcher>,
        mob_target_registry: ScheduleMobTargetRegistry,
    ) -> Self {
        let resolve_mob_session = Arc::new(move |session_id: String| {
            let mob_target_registry = mob_target_registry.clone();
            Box::pin(async move {
                mob_target_registry
                    .resolve_bridge_session(&session_id)
                    .await
            }) as Pin<Box<dyn Future<Output = Option<(String, String)>> + Send>>
        });
        Self {
            inner,
            resolve_mob_session,
        }
    }

    #[cfg(test)]
    fn new_for_test<F, Fut>(inner: Arc<dyn meerkat_core::AgentToolDispatcher>, resolve: F) -> Self
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<(String, String)>> + Send + 'static,
    {
        let resolve = Arc::new(resolve);
        let resolve_mob_session = Arc::new(move |session_id: String| {
            let resolve = resolve.clone();
            Box::pin(async move { resolve(session_id).await })
                as Pin<Box<dyn Future<Output = Option<(String, String)>> + Send>>
        });
        Self {
            inner,
            resolve_mob_session,
        }
    }

    async fn rewrite_args(&self, call_name: &str, args: Value) -> Value {
        if call_name != "meerkat_schedule_create" && call_name != "meerkat_schedule_update" {
            return args;
        }
        rewrite_resumable_session_target_to_mob_member(args, |session_id| async move {
            (self.resolve_mob_session)(session_id).await
        })
        .await
    }
}

#[async_trait]
impl meerkat_core::AgentToolDispatcher for MobIdentityScheduleToolDispatcher {
    fn tools(&self) -> Arc<[Arc<meerkat_core::types::ToolDef>]> {
        self.inner.tools()
    }

    async fn dispatch(
        &self,
        call: meerkat_core::types::ToolCallView<'_>,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        if call.name != "meerkat_schedule_create" && call.name != "meerkat_schedule_update" {
            return self.inner.dispatch(call).await;
        }

        let args: Value = serde_json::from_str(call.args.get()).map_err(|error| {
            meerkat_core::ToolError::invalid_arguments(
                call.name,
                format!("invalid schedule tool-call arguments JSON: {error}"),
            )
        })?;
        let rewritten = self.rewrite_args(call.name, args).await;
        let rewritten_raw = serde_json::value::RawValue::from_string(rewritten.to_string())
            .map_err(|error| {
                meerkat_core::ToolError::invalid_arguments(
                    call.name,
                    format!("failed to encode rewritten schedule arguments: {error}"),
                )
            })?;
        self.inner
            .dispatch(meerkat_core::types::ToolCallView {
                id: call.id,
                name: call.name,
                args: &rewritten_raw,
            })
            .await
    }
}

async fn rewrite_resumable_session_target_to_mob_member<F, Fut>(
    mut args: Value,
    mut resolve: F,
) -> Value
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Option<(String, String)>>,
{
    let Some(target) = args
        .as_object_mut()
        .and_then(|object| object.get_mut("target"))
        .and_then(Value::as_object_mut)
    else {
        return args;
    };

    let is_resumable_session_target = target
        .get("target_kind")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "session")
        && target
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| value == "resumable_session");
    if !is_resumable_session_target {
        return args;
    }
    let Some(session_id) = target
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return args;
    };
    let Some(action) = target.get("action").and_then(Value::as_object) else {
        return args;
    };
    if action
        .get("type")
        .and_then(Value::as_str)
        .is_none_or(|value| value != "prompt")
    {
        return args;
    }
    if action_value_is_non_empty(action.get("system_prompt"))
        || action_value_is_non_empty(action.get("skill_refs"))
        || action_value_is_non_empty(action.get("additional_instructions"))
    {
        return args;
    }
    let Some(prompt) = action.get("prompt").cloned() else {
        return args;
    };
    let Some((mob_id, member_id)) = resolve(session_id).await else {
        return args;
    };

    let mut mob_action = Map::new();
    mob_action.insert("type".to_string(), Value::String("send".to_string()));
    mob_action.insert("content".to_string(), prompt);
    if let Some(render_metadata) = action.get("render_metadata").cloned() {
        mob_action.insert("render_metadata".to_string(), render_metadata);
    }

    let mut rewritten = Map::new();
    rewritten.insert("target_kind".to_string(), Value::String("mob".to_string()));
    rewritten.insert("type".to_string(), Value::String("member".to_string()));
    rewritten.insert("mob_id".to_string(), Value::String(mob_id));
    rewritten.insert("member_id".to_string(), Value::String(member_id));
    rewritten.insert("action".to_string(), Value::Object(mob_action));
    *target = rewritten;
    args
}

fn action_value_is_non_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Array(values)) => !values.is_empty(),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Object(values)) => !values.is_empty(),
        Some(Value::Bool(_) | Value::Number(_)) => true,
    }
}

async fn rewrite_target_binding_to_mob_member<F, Fut>(
    target: &TargetBinding,
    resolve: &mut F,
) -> Option<TargetBinding>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Option<(String, String)>>,
{
    let TargetBinding::Session(binding) = target else {
        return None;
    };
    let SessionTargetBinding::ResumableSession { session_id, action } = binding.as_ref() else {
        return None;
    };
    let ScheduledSessionAction::Prompt {
        prompt,
        system_prompt,
        render_metadata,
        skill_refs,
        additional_instructions,
    } = action
    else {
        return None;
    };
    if system_prompt.is_some() || !skill_refs.is_empty() || !additional_instructions.is_empty() {
        return None;
    }
    let (mob_id, member_id) = resolve(session_id.to_string()).await?;
    Some(TargetBinding::Mob(Box::new(MobTargetBinding::Member {
        mob_id,
        member_id,
        action: ScheduledMobAction::Send {
            content: prompt.clone(),
            render_metadata: render_metadata.clone(),
        },
    })))
}

async fn repair_resumable_session_targets_with_resolver<F, Fut>(
    service: &ScheduleService,
    mut resolve: F,
) -> Result<usize, String>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Option<(String, String)>>,
{
    let schedules = service
        .list()
        .await
        .map_err(|error| format!("list schedules: {error}"))?;
    let mut repaired = 0usize;
    for schedule in schedules {
        let Some(target) =
            rewrite_target_binding_to_mob_member(&schedule.target, &mut resolve).await
        else {
            continue;
        };
        match service
            .update(
                &schedule.schedule_id,
                UpdateScheduleRequest {
                    target: Some(target),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => repaired += 1,
            Err(error) => {
                tracing::warn!(
                    schedule_id = %schedule.schedule_id,
                    error = %error,
                    "failed to repair legacy resumable_session schedule target; continuing",
                );
            }
        }
    }
    Ok(repaired)
}

/// Best-effort migration for schedules authored before MobKit preserved the
/// identity target. If the old session id still maps to a mob member, rewrite
/// simple resumable-session prompts to durable mob/member delivery before the
/// schedule host starts firing due occurrences.
pub async fn repair_resumable_session_targets_to_mob_members(
    service: &ScheduleService,
    mob_target_registry: &ScheduleMobTargetRegistry,
) -> Result<usize, String> {
    repair_resumable_session_targets_with_resolver(service, |session_id| async move {
        mob_target_registry
            .resolve_bridge_session(&session_id)
            .await
    })
    .await
}

/// Build a durable (Sqlite) [`ScheduleService`] under `state_dir` and attach its
/// tool dispatcher to `builder`'s default schedule-tools slot.
///
/// After this, any member whose profile resolves schedule tools on
/// (`tools.schedule = true` → `override_schedule = Enable`) is built with the
/// `meerkat_schedule_{create,get,list,update,pause,resume,delete,occurrences}`
/// surface. Returns the service so the same instance can back the firing host.
///
/// Call this on the `FactoryAgentBuilder` BEFORE it is consumed into a session
/// service. Returns `None` (with a warning) if the store cannot be opened — the
/// gateway then boots without schedule tools rather than failing closed.
#[must_use]
pub fn attach_schedule_tools(
    builder: &FactoryAgentBuilder,
    state_dir: &Path,
) -> Option<ScheduleService> {
    attach_schedule_tools_with_identity_targets(builder, state_dir).map(|attached| attached.service)
}

#[must_use]
pub fn attach_schedule_tools_with_identity_targets(
    builder: &FactoryAgentBuilder,
    state_dir: &Path,
) -> Option<AttachedScheduleTools> {
    let path = state_dir.join(SCHEDULE_STORE_FILE);
    let store = match SqliteScheduleStore::open(&path) {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "failed to open schedule store; schedule tools disabled for this gateway",
            );
            return None;
        }
    };
    let service = ScheduleService::new(Arc::new(store));
    let mob_target_registry = ScheduleMobTargetRegistry::default();
    let dispatcher = MobIdentityScheduleToolDispatcher::new(
        Arc::new(ScheduleToolDispatcher::new(service.clone())),
        mob_target_registry.clone(),
    );
    meerkat::surface::set_default_schedule_tools(builder, Some(Arc::new(dispatcher)));
    Some(AttachedScheduleTools {
        service,
        mob_target_registry,
    })
}

/// Spawn the runtime-backed schedule host so authored schedules fire: at due
/// time the driver materializes a session and runs the scheduled prompt as a
/// real agent turn. Session targets are served by meerkat's runtime-backed
/// host; mob targets by `meerkat-mob-mcp`'s host when a mob runtime is present.
///
/// Requires a persistent session service + the runtime adapter (the firing
/// driver is not available for ephemeral sessions). The returned handle MUST be
/// kept alive for the gateway's lifetime — dropping it shuts the host down.
/// Returns `None` if the store kind cannot host.
///
/// Generic over the session builder `B` so both gateways can drive firing: the
/// console gateway with `FactoryAgentBuilder`, and the SDK gateway with its
/// `StdioCallbackAgentBuilder` (so scheduled sessions are materialized through
/// the SDK build callback and keep their identity-scoped tools).
/// Mob host wrapper: mob-member schedule deliveries are INTERNAL addressing.
///
/// A schedule targeting a member of this mob was authored inside the mob (the
/// agent tool rewrite or delivery-time identity recovery produced it), so its
/// delivery is mob coordination — `WorkOrigin::Internal` — not external
/// ingress. The stock meerkat-mob-mcp host routes `member_send` through the
/// EXTERNAL work door, which rejects members whose profile is
/// `external_addressable = false` ("mob member is not externally
/// addressable") — HomeCore's domain agents are internal-only by design, and
/// flipping them externally addressable to receive their own schedules would
/// be the wrong fix. This wrapper delivers member-addressed prompts through
/// the same internal work lane the identity bridge uses
/// (`submit_work_with_mode` + `WorkOrigin::Internal`); everything else
/// (flows, helpers, probes) delegates to the wrapped host.
struct InternalDeliveryScheduleMobHost {
    inner: Arc<dyn SurfaceScheduleMobHost>,
    handle: MobHandle,
    identity_runtime: Option<Arc<crate::identity_first::IdentityRuntime>>,
}

impl InternalDeliveryScheduleMobHost {
    async fn deliver_internal_member_prompt(
        &self,
        occurrence: &meerkat::Occurrence,
        mob_id: &str,
        member_id: &str,
        content: &meerkat_core::ContentInput,
    ) -> Option<meerkat::DeliveryDispatch> {
        if self.handle.definition().id.as_str() != mob_id {
            return None;
        }
        // Binding member ids arrive in ROSTER space (the authoring rewrite
        // stores `entry.agent_identity`, and meerkat's delivery-time identity
        // recovery stamps the same roster id from the session's mob-member
        // binding) or occasionally in alias space. Roster ids of
        // identity-first members are already `mk--`-encoded, and the codec
        // deliberately RE-encodes marker-prefixed input — encoding the raw
        // binding id double-encodes those and misses the roster (the 0.7.28
        // HomeCore field failure: the miss fell through to the external
        // door). Decode-then-encode canonicalizes both spaces to the roster
        // key; it is the identity on plain member names.
        let member_alias = crate::member_comms_id::runtime_alias_str(member_id).into_owned();
        if let Some(identity_runtime) = self.identity_runtime.as_ref()
            && let Some(identity) = identity_runtime
                .identity_for_member_mutation(&member_alias)
                .await
        {
            let input = crate::identity_first::DispatchInput {
                content: content.clone(),
                origin: crate::identity_first::DispatchOrigin::Scheduler,
                correlation_id: None,
                idempotency_key: None,
            };
            return Some(
                match identity_runtime
                    .dispatch_member_alias_with_session_tracked(&identity, &member_alias, &input)
                    .await
                {
                    Ok(Some(_)) => {
                        immediate_completed_dispatch(occurrence, Some(member_id.to_string()))
                    }
                    Ok(None) => immediate_delivery_failure(
                        occurrence,
                        "identity schedule delivery has no bound session bridge".to_string(),
                        meerkat::DeliveryFailureReason::RuntimeRejected,
                        Some(member_id.to_string()),
                        None,
                    ),
                    Err(error) => immediate_delivery_failure(
                        occurrence,
                        format!("identity schedule delivery failed: {error}"),
                        meerkat::DeliveryFailureReason::RuntimeRejected,
                        Some(member_id.to_string()),
                        None,
                    ),
                },
            );
        }
        if crate::member_comms_id::is_reserved_generated_alias(&member_alias) {
            return Some(immediate_delivery_failure(
                occurrence,
                format!(
                    "generated member alias requires current identity authority: {member_alias}"
                ),
                meerkat::DeliveryFailureReason::RuntimeRejected,
                Some(member_id.to_string()),
                None,
            ));
        }
        let member = crate::member_comms_id::mob_member_id(&member_alias);
        let entry = match self.handle.get_member(&member).await {
            Ok(Some(entry)) => entry,
            Ok(None) => return None,
            Err(error) => {
                return Some(immediate_delivery_failure(
                    occurrence,
                    format!("member lookup failed: {error}"),
                    meerkat::DeliveryFailureReason::RuntimeRejected,
                    None,
                    None,
                ));
            }
        };
        let spec = meerkat_mob::WorkSpec::new(content.clone(), meerkat_mob::WorkOrigin::Internal);
        match self
            .handle
            .submit_work_with_mode(
                entry.agent_runtime_id.clone(),
                entry.fence_token,
                meerkat_mob::WorkRef::new(),
                spec,
                meerkat_core::types::HandlingMode::Queue,
            )
            .await
        {
            Ok(_receipt) => Some(immediate_completed_dispatch(
                occurrence,
                Some(member_id.to_string()),
            )),
            Err(error) => Some(immediate_delivery_failure(
                occurrence,
                format!("internal schedule delivery failed: {error}"),
                meerkat::DeliveryFailureReason::RuntimeRejected,
                Some(member_id.to_string()),
                None,
            )),
        }
    }
}

#[async_trait]
impl SurfaceScheduleMobHost for InternalDeliveryScheduleMobHost {
    async fn probe_mob_target(
        &self,
        binding: &MobTargetBinding,
    ) -> Result<meerkat::TargetProbeOutcome, meerkat::ScheduleDomainError> {
        self.inner.probe_mob_target(binding).await
    }

    async fn deliver_mob_target(
        &self,
        occurrence: &meerkat::Occurrence,
        binding: &MobTargetBinding,
    ) -> Result<meerkat::DeliveryDispatch, meerkat::ScheduleDomainError> {
        if let MobTargetBinding::Member {
            mob_id,
            member_id,
            action: ScheduledMobAction::Send { content, .. },
        } = binding
            && let Some(dispatch) = self
                .deliver_internal_member_prompt(occurrence, mob_id, member_id, content)
                .await
        {
            return Ok(dispatch);
        }
        self.inner.deliver_mob_target(occurrence, binding).await
    }

    async fn probe_identity_target(
        &self,
        binding: &meerkat::IdentityTargetBinding,
    ) -> Result<Option<meerkat::TargetProbeOutcome>, meerkat::ScheduleDomainError> {
        self.inner.probe_identity_target(binding).await
    }

    async fn deliver_identity_target(
        &self,
        occurrence: &meerkat::Occurrence,
        binding: &meerkat::IdentityTargetBinding,
    ) -> Result<Option<meerkat::DeliveryDispatch>, meerkat::ScheduleDomainError> {
        if let Some(identity) = parse_mob_member_schedule_identity(binding.identity())
            && let meerkat::ScheduledSessionAction::Prompt {
                prompt,
                system_prompt: None,
                skill_refs,
                additional_instructions,
                ..
            } = binding.action()
            && skill_refs.is_empty()
            && additional_instructions.is_empty()
            && let Some(dispatch) = self
                .deliver_internal_member_prompt(
                    occurrence,
                    &identity.mob_id,
                    &identity.member,
                    prompt,
                )
                .await
        {
            return Ok(Some(dispatch));
        }
        self.inner
            .deliver_identity_target(occurrence, binding)
            .await
    }
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn spawn_schedule_host<B: SessionAgentBuilder + 'static>(
    service: Arc<PersistentSessionService<B>>,
    adapter: Arc<MeerkatMachine>,
    schedule_service: ScheduleService,
    mob_state: Option<Arc<MobMcpState>>,
    mob_handle: MobHandle,
    runnable_host: Option<Arc<dyn meerkat::ScheduleRunnableHost>>,
    workgraph_service: Option<meerkat::WorkGraphService>,
    owner_id: impl Into<String>,
) -> Option<ScheduleHostHandle> {
    spawn_schedule_host_with_identity_runtime(
        service,
        adapter,
        schedule_service,
        mob_state,
        mob_handle,
        None,
        runnable_host,
        workgraph_service,
        owner_id,
    )
}

/// Identity-aware schedule host. Generated member aliases are dispatched
/// through the durable runtime instead of the raw member plane.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn spawn_schedule_host_with_identity_runtime<B: SessionAgentBuilder + 'static>(
    service: Arc<PersistentSessionService<B>>,
    adapter: Arc<MeerkatMachine>,
    schedule_service: ScheduleService,
    mob_state: Option<Arc<MobMcpState>>,
    mob_handle: MobHandle,
    identity_runtime: Option<Arc<crate::identity_first::IdentityRuntime>>,
    runnable_host: Option<Arc<dyn meerkat::ScheduleRunnableHost>>,
    workgraph_service: Option<meerkat::WorkGraphService>,
    owner_id: impl Into<String>,
) -> Option<ScheduleHostHandle> {
    let inner_mob_host: Arc<dyn SurfaceScheduleMobHost> = match mob_state {
        Some(state) => Arc::new(MobMcpScheduleHost::new(state)),
        None => Arc::new(NoopScheduleMobHost::new(
            "scheduled mob targets are not supported: no mob runtime",
        )),
    };
    // Member-addressed deliveries take the INTERNAL work lane (schedule
    // delivery to a mob member is mob coordination, not external ingress) —
    // internal-only members receive their own schedules.
    let mob_host: Arc<dyn SurfaceScheduleMobHost> = Arc::new(InternalDeliveryScheduleMobHost {
        inner: inner_mob_host,
        handle: mob_handle,
        identity_runtime,
    });
    // meerkat 0.7.13 (upstream ask 10): thread a host-runnable registry so
    // host-registered runnables (e.g. the memory steward's dream) can be
    // driven as schedule occurrences. `None` keeps mob/session targets only.
    // meerkat 0.7.23: the runtime-backed session host injects the WorkGraph
    // attention projection at apply time when given a WorkGraphService, so
    // scheduled turns on goal-bound sessions carry the scoped tool overlay.
    spawn_runtime_backed_schedule_host_with_mobs(
        service,
        adapter,
        Config::default(),
        schedule_service,
        SessionBuildOptions::default(),
        mob_host,
        runnable_host,
        workgraph_service,
        owner_id,
    )
}

/// Cadence and sensitivity for [`spawn_schedule_claim_watchdog`].
#[derive(Debug, Clone, Copy)]
pub struct ScheduleClaimWatchdogConfig {
    /// Delay between probes.
    pub poll_interval: Duration,
    /// A pending occurrence due longer ago than this counts as stalled.
    pub overdue_threshold: Duration,
    /// While unhealthy with an unchanged report, re-log every Nth poll.
    pub heartbeat_polls: u32,
}

impl Default for ScheduleClaimWatchdogConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_mins(1),
            overdue_threshold: Duration::from_mins(2),
            heartbeat_polls: 10,
        }
    }
}

/// Verdict from one firing-pipeline probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleFiringProbe {
    Healthy,
    /// The pipeline is not delivering and here is why, as precisely as this
    /// side of the wire can name it.
    Stalled {
        report: String,
    },
}

fn triage_poisoned_rows(
    store_path: &Path,
    table: &str,
    id_column: &str,
    json_column: &str,
    parse: impl Fn(&[u8]) -> Result<(), String>,
) -> Vec<String> {
    let conn = match rusqlite::Connection::open_with_flags(
        store_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(conn) => conn,
        Err(err) => return vec![format!("(row triage unavailable: {err})")],
    };
    let sql = format!("SELECT {id_column}, {json_column} FROM {table}");
    let mut stmt = match conn.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(err) => return vec![format!("(row triage query failed: {err})")],
    };
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    });
    let mut poisoned = Vec::new();
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            let (id, bytes) = row;
            if let Err(err) = parse(&bytes) {
                poisoned.push(format!("{table}.{id_column}={id}: {err}"));
                if poisoned.len() >= 5 {
                    poisoned.push(format!("(further poisoned {table} rows elided)"));
                    break;
                }
            }
        }
    }
    poisoned
}

/// Probe whether the schedule firing pipeline can actually deliver, and if
/// not, name the reason — down to the poisoned row where possible.
///
/// Exists because meerkat's schedule host discards every driver tick error
/// (`let _ = driver.tick_once()`), and a tick aborts wholesale on the FIRST
/// poisoned row anywhere in the store: `service.list()` fails on any
/// unrecoverable schedule row (e.g. a Deleted tombstone rejected by
/// `deleted_has_no_planning_cursor`), and the claim scan deserializes and
/// classifies EVERY occurrence row before leasing anything. One stale row →
/// zero claims, forever, with nothing in any log — HomeCore 0.7.24 sat with
/// 31 pending occurrences and `lease_expires_at_ms=NULL` across the board.
pub async fn probe_schedule_firing_pipeline(
    schedule_service: &ScheduleService,
    store_path: &Path,
    overdue_threshold: Duration,
) -> ScheduleFiringProbe {
    // 1. The tick preflight: one poisoned schedule row fails the whole list.
    if let Err(err) = schedule_service.list().await {
        let mut report = format!(
            "schedule list is failing, so every firing-driver tick aborts before claiming (nothing will fire): {err}"
        );
        let poisoned = triage_poisoned_rows(
            store_path,
            "schedule_schedules",
            "schedule_id",
            "schedule_json",
            |bytes| {
                serde_json::from_slice::<Schedule>(bytes)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
        );
        if !poisoned.is_empty() {
            report.push_str(&format!("; poisoned rows: {}", poisoned.join("; ")));
        }
        return ScheduleFiringProbe::Stalled { report };
    }

    // 2. The claim scan's poison surface: it deserializes every occurrence.
    let store = schedule_service.store();
    let now = match store.get_store_time_utc().await {
        Ok(now) => now,
        Err(err) => {
            return ScheduleFiringProbe::Stalled {
                report: format!("schedule store clock read failed: {err}"),
            };
        }
    };
    let overdue_before = now
        - chrono::Duration::from_std(overdue_threshold)
            .unwrap_or_else(|_| chrono::Duration::seconds(120));
    let pending_overdue = match store
        .list_occurrences(OccurrenceFilter {
            phase: Some(OccurrencePhase::Pending),
            include_terminal: false,
            due_before_utc: Some(overdue_before),
            ..OccurrenceFilter::default()
        })
        .await
    {
        Ok(occurrences) => occurrences,
        Err(err) => {
            let mut report = format!(
                "occurrence scan is failing, so the firing driver cannot claim anything (nothing will fire): {err}"
            );
            let poisoned = triage_poisoned_rows(
                store_path,
                "schedule_occurrences",
                "occurrence_id",
                "occurrence_json",
                |bytes| {
                    serde_json::from_slice::<Occurrence>(bytes)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            );
            if !poisoned.is_empty() {
                report.push_str(&format!("; poisoned rows: {}", poisoned.join("; ")));
            }
            return ScheduleFiringProbe::Stalled { report };
        }
    };

    if pending_overdue.is_empty() {
        return ScheduleFiringProbe::Healthy;
    }

    // 3. Reads are healthy but due work is not being claimed. Classify each
    // overdue occurrence the way the claim scan would — a classify error on
    // ANY row (even one belonging to another schedule) aborts the whole
    // claim transaction upstream.
    let mut classify_errors = Vec::new();
    for occurrence in &pending_overdue {
        if let Err(err) = occurrence.classify_due_action(now) {
            classify_errors.push(format!(
                "occurrence {} (due {}): {err}",
                occurrence.occurrence_id, occurrence.due_at_utc
            ));
            if classify_errors.len() >= 5 {
                break;
            }
        }
    }
    let mut report = format!(
        "{} pending occurrence(s) overdue by more than {}s and never claimed (oldest due {}); the firing driver's claim loop is failing silently",
        pending_overdue.len(),
        overdue_threshold.as_secs(),
        pending_overdue
            .first()
            .map(|o| o.due_at_utc.to_rfc3339())
            .unwrap_or_default(),
    );
    if classify_errors.is_empty() {
        report.push_str(
            "; every overdue occurrence classifies cleanly, so the abort is in the claim/lease transaction or a row outside the overdue set",
        );
    } else {
        report.push_str(&format!(
            "; rows failing due-classification: {}",
            classify_errors.join("; ")
        ));
    }
    ScheduleFiringProbe::Stalled { report }
}

/// Watchdog for the silent-stall failure mode above: probes the firing
/// pipeline and logs LOUDLY when due work is not being claimed, naming the
/// poisoned row when one is identifiable. Purely read-only — it never claims
/// or transitions occurrences, so it cannot race the real driver.
///
/// Logs an ERROR when the stall report first appears or changes, a WARN
/// heartbeat every `heartbeat_polls` while it persists, and an INFO when the
/// pipeline recovers.
pub fn spawn_schedule_claim_watchdog(
    schedule_service: ScheduleService,
    store_path: std::path::PathBuf,
    config: ScheduleClaimWatchdogConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_report: Option<String> = None;
        let mut unhealthy_polls: u32 = 0;
        loop {
            tokio::time::sleep(config.poll_interval).await;
            match probe_schedule_firing_pipeline(
                &schedule_service,
                &store_path,
                config.overdue_threshold,
            )
            .await
            {
                ScheduleFiringProbe::Healthy => {
                    if last_report.take().is_some() {
                        tracing::info!("schedule firing pipeline recovered");
                    }
                    unhealthy_polls = 0;
                }
                ScheduleFiringProbe::Stalled { report } => {
                    unhealthy_polls += 1;
                    if last_report.as_deref() != Some(report.as_str()) {
                        tracing::error!(%report, "schedule firing pipeline is stalled");
                    } else if config.heartbeat_polls > 0
                        && unhealthy_polls.is_multiple_of(config.heartbeat_polls)
                    {
                        tracing::warn!(%report, "schedule firing pipeline is still stalled");
                    }
                    last_report = Some(report);
                }
            }
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use meerkat::{
        AgentFactory, MemoryScheduleStore, MobTargetBinding,
        ScheduleToolDispatcher as InnerScheduleToolDispatcher, ScheduledMobAction, TargetBinding,
    };
    use meerkat_core::AgentToolDispatcher;
    use meerkat_core::types::{SessionId, ToolCallView};
    use meerkat_schedule::CurrentSessionScheduleToolDispatcher;
    use serde_json::json;
    use serde_json::value::RawValue;

    #[test]
    fn attach_schedule_tools_populates_the_builder_slot_and_opens_the_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let factory = AgentFactory::new(dir.path());
        let builder = FactoryAgentBuilder::new(factory, Config::default());

        // The default schedule-tools slot starts empty — this is exactly why
        // `profile.tools.schedule = true` was inert before this wiring.
        assert!(
            builder.default_schedule_tools.read().unwrap().is_none(),
            "slot should start empty"
        );

        let service = attach_schedule_tools(&builder, dir.path());

        assert!(service.is_some(), "a durable schedule service is created");
        assert!(
            builder.default_schedule_tools.read().unwrap().is_some(),
            "the dispatcher is installed so override_schedule=Enable members compose schedule tools",
        );
        assert!(
            dir.path().join(SCHEDULE_STORE_FILE).exists(),
            "the durable store file is created",
        );
    }

    #[tokio::test]
    async fn current_session_resumable_prompt_rewrites_to_identity_mob_member_target() {
        let args = json!({
            "name": "daily-digest",
            "trigger": {
                "type": "calendar",
                "timezone": "Europe/Stockholm",
                "hour": { "kind": "values", "values": [7] },
                "minute": { "kind": "values", "values": [0] }
            },
            "target": {
                "target_kind": "session",
                "type": "resumable_session",
                "session_id": "019ee0a7-a594-7670-b530-97e7c9e263b7",
                "action": {
                    "type": "prompt",
                    "prompt": "Send the morning digest.",
                    "render_metadata": {
                        "source": "daily-digest"
                    }
                }
            }
        });

        let rewritten =
            rewrite_resumable_session_target_to_mob_member(args, |session_id| async move {
                assert_eq!(session_id, "019ee0a7-a594-7670-b530-97e7c9e263b7");
                Some(("homecore".to_string(), "domain:security".to_string()))
            })
            .await;

        assert_eq!(
            rewritten["target"],
            json!({
                "target_kind": "mob",
                "type": "member",
                "mob_id": "homecore",
                "member_id": "domain:security",
                "action": {
                    "type": "send",
                    "content": "Send the morning digest.",
                    "render_metadata": {
                        "source": "daily-digest"
                    }
                }
            })
        );
    }

    #[tokio::test]
    async fn resumable_session_target_stays_session_when_not_mob_owned() {
        let args = json!({
            "target": {
                "target_kind": "session",
                "type": "resumable_session",
                "session_id": "019ee0a7-a594-7670-b530-97e7c9e263b7",
                "action": {
                    "type": "prompt",
                    "prompt": "Keep this pinned."
                }
            }
        });

        let rewritten =
            rewrite_resumable_session_target_to_mob_member(args.clone(), |_session_id| async {
                None
            })
            .await;

        assert_eq!(rewritten, args);
    }

    #[tokio::test]
    async fn resumable_session_target_with_session_only_prompt_options_is_not_rewritten() {
        let args = json!({
            "target": {
                "target_kind": "session",
                "type": "resumable_session",
                "session_id": "019ee0a7-a594-7670-b530-97e7c9e263b7",
                "action": {
                    "type": "prompt",
                    "prompt": "Run with custom context.",
                    "system_prompt": "Use a one-off persona."
                }
            }
        });

        let rewritten = rewrite_resumable_session_target_to_mob_member(args.clone(), |_| async {
            Some(("homecore".to_string(), "domain:security".to_string()))
        })
        .await;

        assert_eq!(rewritten, args);
    }

    #[tokio::test]
    async fn empty_session_prompt_options_still_rewrite_to_identity_mob_member_target() {
        let args = json!({
            "target": {
                "target_kind": "session",
                "type": "resumable_session",
                "session_id": "019ee0a7-a594-7670-b530-97e7c9e263b7",
                "action": {
                    "type": "prompt",
                    "prompt": "Run with explicit defaults.",
                    "system_prompt": null,
                    "skill_refs": [],
                    "additional_instructions": []
                }
            }
        });

        let rewritten = rewrite_resumable_session_target_to_mob_member(args, |_| async {
            Some(("homecore".to_string(), "domain:security".to_string()))
        })
        .await;

        assert_eq!(
            rewritten["target"],
            json!({
                "target_kind": "mob",
                "type": "member",
                "mob_id": "homecore",
                "member_id": "domain:security",
                "action": {
                    "type": "send",
                    "content": "Run with explicit defaults."
                }
            })
        );
    }

    #[tokio::test]
    async fn current_session_dispatch_chain_creates_and_updates_identity_mob_targets() {
        let service = ScheduleService::new(Arc::new(MemoryScheduleStore::default()));
        let schedule_dispatcher = Arc::new(InnerScheduleToolDispatcher::new(service.clone()));
        let mob_dispatcher = Arc::new(MobIdentityScheduleToolDispatcher::new_for_test(
            schedule_dispatcher,
            |session_id| async move {
                assert_eq!(session_id, "019ee0a7-a594-7670-b530-97e7c9e263b7");
                Some(("homecore".to_string(), "domain:security".to_string()))
            },
        ));
        let session_id =
            SessionId::parse("019ee0a7-a594-7670-b530-97e7c9e263b7").expect("session id");
        let dispatcher = CurrentSessionScheduleToolDispatcher::new(mob_dispatcher, session_id);

        let create_args = RawValue::from_string(
            json!({
                "name": "daily-digest",
                "trigger": {
                    "type": "interval",
                    "start_at_utc": "2026-07-01T05:00:00Z",
                    "every_seconds": 86400
                },
                "target": {
                    "target_kind": "session",
                    "type": "current_session",
                    "action": {
                        "type": "prompt",
                        "prompt": "Send the morning digest.",
                        "render_metadata": {
                            "class": "external_event",
                            "salience": "important"
                        }
                    }
                },
                "planning_horizon_occurrences": 1
            })
            .to_string(),
        )
        .expect("create args");
        let create_call = ToolCallView {
            id: "create-digest",
            name: "meerkat_schedule_create",
            args: &create_args,
        };
        dispatcher
            .dispatch(create_call)
            .await
            .expect("create schedule through dispatch chain");

        let schedules = service.list().await.expect("list schedules");
        assert_eq!(schedules.len(), 1);
        let schedule_id = schedules[0].schedule_id.to_string();
        assert_mob_member_target(
            &schedules[0].target,
            "homecore",
            "domain:security",
            "Send the morning digest.",
            "external_event",
        );

        let update_args = RawValue::from_string(
            json!({
                "schedule_id": schedule_id,
                "target": {
                    "target_kind": "session",
                    "type": "current_session",
                    "action": {
                        "type": "prompt",
                        "prompt": "Send the updated digest.",
                        "render_metadata": {
                            "class": "peer_request",
                            "salience": "urgent"
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect("update args");
        let update_call = ToolCallView {
            id: "update-digest",
            name: "meerkat_schedule_update",
            args: &update_args,
        };
        dispatcher
            .dispatch(update_call)
            .await
            .expect("update schedule through dispatch chain");

        let updated = service.list().await.expect("list updated schedules");
        assert_eq!(updated.len(), 1);
        assert_mob_member_target(
            &updated[0].target,
            "homecore",
            "domain:security",
            "Send the updated digest.",
            "peer_request",
        );
    }

    fn assert_mob_member_target(
        target: &TargetBinding,
        expected_mob_id: &str,
        expected_member_id: &str,
        expected_content: &str,
        expected_class: &str,
    ) {
        assert!(
            matches!(target, TargetBinding::Mob(_)),
            "expected mob target, got {target:?}"
        );
        let TargetBinding::Mob(binding) = target else {
            return;
        };
        assert!(
            matches!(binding.as_ref(), MobTargetBinding::Member { .. }),
            "expected member mob target, got {binding:?}"
        );
        let MobTargetBinding::Member {
            mob_id,
            member_id,
            action,
        } = binding.as_ref()
        else {
            return;
        };
        assert_eq!(mob_id, expected_mob_id);
        assert_eq!(member_id, expected_member_id);
        let ScheduledMobAction::Send {
            content,
            render_metadata,
        } = action;
        assert_eq!(content.text_content(), expected_content);
        let metadata = render_metadata.as_ref().expect("render metadata");
        let metadata = serde_json::to_value(metadata).expect("metadata value");
        assert_eq!(metadata["class"], expected_class);
    }

    #[test]
    fn steward_dream_runnable_host_is_none_without_a_steward() {
        assert!(steward_dream_runnable_host(None).is_none());
    }

    /// Records bridge calls and answers each with a scripted result, standing
    /// in for the SDK host on the far side of the stdio callback bridge.
    struct ScriptedCallbackBridge {
        calls: std::sync::Mutex<Vec<(String, Value)>>,
        result: Result<Value, String>,
    }

    impl ScriptedCallbackBridge {
        fn new(result: Result<Value, String>) -> Arc<Self> {
            Arc::new(Self {
                calls: std::sync::Mutex::new(Vec::new()),
                result,
            })
        }
    }

    #[async_trait]
    impl CallbackBridge for ScriptedCallbackBridge {
        async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
            self.calls
                .lock()
                .expect("calls lock")
                .push((method.to_string(), params));
            self.result.clone()
        }
    }

    fn runnable_name(value: &str) -> HostRunnableName {
        HostRunnableName::parse(value).expect("valid runnable name")
    }

    fn callback_invocation(runnable: &str, params_json: Option<&str>) -> HostRunnableInvocation {
        HostRunnableInvocation {
            occurrence_id: meerkat::OccurrenceId::new(),
            schedule_id: meerkat::ScheduleId::new(),
            runnable: runnable_name(runnable),
            trigger_time: chrono::Utc::now(),
            params: params_json
                .map(|text| RawValue::from_string(text.to_string()).expect("valid raw params")),
        }
    }

    #[tokio::test]
    async fn callback_runnable_fires_over_the_bridge_with_the_occurrence_shape() {
        let bridge = ScriptedCallbackBridge::new(Ok(json!({"digest": "sent"})));
        let host = gateway_runnable_host(
            None,
            Some((
                Arc::clone(&bridge) as Arc<dyn CallbackBridge>,
                vec![runnable_name("digest")],
            )),
        )
        .expect("compose runnable host")
        .expect("callback runnables registered");

        assert_eq!(
            host.probe_runnable(&runnable_name("digest")),
            meerkat::RunnableProbe::Registered
        );
        assert_eq!(
            host.probe_runnable(&runnable_name("unknown")),
            meerkat::RunnableProbe::Unknown
        );

        let invocation = callback_invocation("digest", Some(r#"{"depth":3}"#));
        let outcome = host
            .run_occurrence(invocation.clone())
            .await
            .expect("bridge success completes the occurrence");
        assert_eq!(outcome, HostRunnableOutcome::completed());

        let calls = bridge.calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 1);
        let (method, params) = &calls[0];
        assert_eq!(method, SCHEDULE_FIRE_CALLBACK_METHOD);
        assert_eq!(params["runnable"], json!("digest"));
        let occurrence = params["occurrence"].as_object().expect("occurrence object");
        assert_eq!(
            occurrence["schedule_id"],
            json!(invocation.schedule_id.to_string())
        );
        assert_eq!(
            occurrence["occurrence_id"],
            json!(invocation.occurrence_id.to_string())
        );
        assert_eq!(
            occurrence["due_at"],
            json!(invocation.trigger_time.to_rfc3339())
        );
        assert_eq!(occurrence["payload"], json!({"depth": 3}));
    }

    #[tokio::test]
    async fn callback_runnable_omits_payload_when_the_binding_has_no_params() {
        let bridge = ScriptedCallbackBridge::new(Ok(Value::Null));
        let host = gateway_runnable_host(
            None,
            Some((
                Arc::clone(&bridge) as Arc<dyn CallbackBridge>,
                vec![runnable_name("digest")],
            )),
        )
        .expect("compose runnable host")
        .expect("callback runnables registered");

        host.run_occurrence(callback_invocation("digest", None))
            .await
            .expect("fire without params");
        let calls = bridge.calls.lock().expect("calls lock");
        assert!(
            calls[0].1["occurrence"].get("payload").is_none(),
            "no payload key without binding params: {}",
            calls[0].1
        );
    }

    #[tokio::test]
    async fn callback_runnable_maps_bridge_error_to_runnable_failure() {
        let bridge = ScriptedCallbackBridge::new(Err("callback error: handler raised".to_string()));
        let host = gateway_runnable_host(
            None,
            Some((
                Arc::clone(&bridge) as Arc<dyn CallbackBridge>,
                vec![runnable_name("digest")],
            )),
        )
        .expect("compose runnable host")
        .expect("callback runnables registered");

        let error = host
            .run_occurrence(callback_invocation("digest", None))
            .await
            .expect_err("bridge failure must fail the occurrence");
        assert!(
            matches!(&error, HostRunnableError::Failed { detail } if detail.contains("handler raised")),
            "expected typed Failed with the bridge detail, got {error:?}"
        );
    }

    #[test]
    fn gateway_runnable_host_is_none_with_nothing_to_register() {
        assert!(
            gateway_runnable_host(None, None)
                .expect("empty composition is not an error")
                .is_none()
        );
    }

    #[test]
    fn gateway_runnable_host_rejects_duplicate_callback_names() {
        let bridge = ScriptedCallbackBridge::new(Ok(Value::Null));
        let error = match gateway_runnable_host(
            None,
            Some((
                bridge as Arc<dyn CallbackBridge>,
                vec![runnable_name("digest"), runnable_name("digest")],
            )),
        ) {
            Err(error) => error,
            Ok(_) => panic!("duplicate names must be rejected"),
        };
        assert!(error.contains("digest"), "{error}");
    }

    /// The agent-facing schedule tools and any generic RPC pass-through
    /// deserialize targets through `TargetBinding`'s serde — pin that the
    /// host-runnable wire form is accepted and creation lands in the store.
    #[tokio::test]
    async fn schedule_creation_accepts_the_host_runnable_target_wire_form() {
        let target: TargetBinding = serde_json::from_value(json!({
            "target_kind": "host_runnable",
            "runnable": "digest",
            "params": {"depth": 3}
        }))
        .expect("host_runnable target wire form deserializes");
        assert!(
            matches!(&target, TargetBinding::HostRunnable(binding) if binding.runnable == runnable_name("digest")),
            "expected host-runnable binding, got {target:?}"
        );

        let service = ScheduleService::new(Arc::new(MemoryScheduleStore::new()));
        let created = service
            .create(CreateScheduleRequest {
                name: Some("sdk-digest".to_string()),
                description: None,
                trigger: TriggerSpec::Interval(IntervalTriggerSpec {
                    start_at_utc: chrono::Utc::now() + chrono::Duration::seconds(60),
                    every_seconds: 3600,
                    end_at_utc: None,
                }),
                target,
                misfire_policy: meerkat::MisfirePolicy::default(),
                overlap_policy: meerkat::OverlapPolicy::default(),
                missing_target_policy: meerkat::MissingTargetPolicy::default(),
                labels: std::collections::BTreeMap::new(),
                planning_horizon_days: None,
                planning_horizon_occurrences: None,
            })
            .await
            .expect("create schedule with host_runnable target");
        assert!(
            matches!(&created.target, TargetBinding::HostRunnable(_)),
            "persisted schedule keeps the host-runnable target"
        );
    }

    fn dream_target_count(schedules: &[meerkat::Schedule]) -> usize {
        let runnable = HostRunnableName::parse(STEWARD_DREAM_RUNNABLE).expect("name");
        schedules
            .iter()
            .filter(|schedule| {
                matches!(
                    &schedule.target,
                    TargetBinding::HostRunnable(binding) if binding.runnable == runnable
                )
            })
            .count()
    }

    fn dream_interval_seconds(schedule: &meerkat::Schedule) -> u64 {
        match &schedule.trigger {
            TriggerSpec::Interval(spec) => spec.every_seconds,
            other => panic!("expected interval trigger, got {other:?}"),
        }
    }

    /// Author an interval schedule (host-runnable target: no session machinery
    /// needed) against a REAL sqlite store and plan its horizon, mirroring what
    /// the firing driver's tick preflight sees.
    async fn seed_sqlite_schedule(
        dir: &std::path::Path,
        start_at_utc: chrono::DateTime<chrono::Utc>,
    ) -> (ScheduleService, std::path::PathBuf) {
        let store_path = dir.join(SCHEDULE_STORE_FILE);
        let store = SqliteScheduleStore::open(&store_path).expect("open sqlite schedule store");
        let service = ScheduleService::new(Arc::new(store));
        let created = service
            .create(CreateScheduleRequest {
                name: Some("watchdog-fixture".to_string()),
                description: None,
                trigger: TriggerSpec::Interval(IntervalTriggerSpec {
                    start_at_utc,
                    every_seconds: 3600,
                    end_at_utc: None,
                }),
                target: TargetBinding::host_runnable(HostRunnableTargetBinding {
                    runnable: HostRunnableName::parse("watchdog.fixture").expect("runnable name"),
                    params: None,
                }),
                misfire_policy: meerkat::MisfirePolicy::default(),
                overlap_policy: meerkat::OverlapPolicy::default(),
                missing_target_policy: meerkat::MissingTargetPolicy::default(),
                labels: std::collections::BTreeMap::new(),
                planning_horizon_days: None,
                planning_horizon_occurrences: None,
            })
            .await
            .expect("create schedule");
        service
            .refill_horizon(&created.schedule_id)
            .await
            .expect("plan occurrences");
        (service, store_path)
    }

    fn corrupt_first_row(store_path: &std::path::Path, table: &str, json_column: &str) {
        let conn = rusqlite::Connection::open(store_path).expect("open store for corruption");
        let changed = conn
            .execute(
                &format!(
                    "UPDATE {table} SET {json_column} = X'7B22706F69736F6E22' \
                     WHERE rowid = (SELECT MIN(rowid) FROM {table})"
                ),
                [],
            )
            .expect("corrupt row");
        assert_eq!(changed, 1, "one {table} row corrupted");
    }

    /// meerkat 0.7.19 carry guard (asks 16-19): a poisoned occurrence row no
    /// longer starves the whole claim — the sqlite claim scan skips it as a
    /// typed row fault and claims healthy due neighbors. This is the exact
    /// HomeCore shape: one stale row, everything else due.
    #[tokio::test]
    async fn schedule_claim_tolerates_poisoned_neighbor_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Schedule A: healthy, due in the past via the aged-occurrence
        // rewrite used by the watchdog tests. Schedule B: poisoned row.
        let (service, store_path) =
            seed_sqlite_schedule(dir.path(), chrono::Utc::now() + chrono::Duration::hours(1)).await;
        // Age schedule A's first occurrence JUST past due (inside any misfire
        // window, so it classifies as claimable rather than misfired).
        let overdue = chrono::Utc::now() - chrono::Duration::seconds(5);
        {
            let conn = rusqlite::Connection::open(&store_path).expect("open store");
            let (rowid, bytes): (i64, Vec<u8>) = conn
                .query_row(
                    "SELECT rowid, occurrence_json FROM schedule_occurrences \
                     ORDER BY rowid LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("read first occurrence");
            let mut json: serde_json::Value =
                serde_json::from_slice(&bytes).expect("occurrence json");
            let old_due_ms = json["machine_state"]["due_at_utc_ms"]
                .as_i64()
                .expect("machine due ms");
            let shift = old_due_ms - overdue.timestamp_millis();
            json["due_at_utc"] = serde_json::Value::String(overdue.to_rfc3339());
            json["machine_state"]["due_at_utc_ms"] =
                serde_json::Value::from(overdue.timestamp_millis());
            if let Some(deadline) = json["machine_state"]["misfire_deadline_utc_ms"].as_i64() {
                json["machine_state"]["misfire_deadline_utc_ms"] =
                    serde_json::Value::from(deadline - shift);
            }
            let updated = serde_json::to_vec(&json).expect("serialize occurrence");
            conn.execute(
                "UPDATE schedule_occurrences SET occurrence_json = ?1, due_at_ms = ?2 \
                 WHERE rowid = ?3",
                rusqlite::params![updated, overdue.timestamp_millis(), rowid],
            )
            .expect("age occurrence");
            // Poison a NEIGHBOR row: insert a corrupt occurrence for the same
            // schedule so the claim scan meets it.
            conn.execute(
                "INSERT INTO schedule_occurrences \
                 (occurrence_id, schedule_id, schedule_revision, occurrence_ordinal, \
                  phase, due_at_ms, occurrence_json) \
                 SELECT 'poisoned-row', schedule_id, schedule_revision, \
                        occurrence_ordinal + 999, phase, ?1, X'7B22706F69736F6E22' \
                 FROM schedule_occurrences WHERE rowid = ?2",
                rusqlite::params![overdue.timestamp_millis() - 1000, rowid],
            )
            .expect("insert poisoned row");
        }

        let claimed = service
            .store()
            .claim_due_occurrences(meerkat::ClaimDueRequest {
                owner_id: "carry-test".to_string(),
                limit: 8,
                lease_duration: chrono::Duration::seconds(60),
            })
            .await
            .expect("claim must tolerate the poisoned neighbor (meerkat >= 0.7.19)");
        assert_eq!(
            claimed.claimed.len(),
            1,
            "the healthy due occurrence must be claimed despite the poisoned neighbor"
        );
    }

    #[tokio::test]
    async fn identity_dispatch_without_session_bridge_is_terminal_runtime_rejected() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let factory = AgentFactory::new(temp_dir.path()).comms(true);
        let session_service = Arc::new(meerkat::build_ephemeral_service(
            factory,
            Config::default(),
            4,
        ));
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "schedule-none"

[profiles.general]
model = "gpt-5.5"
external_addressable = true
"#,
        )
        .expect("schedule test mob definition");
        let mob_spec = crate::mob_handle_runtime::MobBootstrapSpec::new(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            session_service,
        )
        .with_options(crate::mob_handle_runtime::MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(meerkat_client::TestClient::default())),
        });
        let runtime = crate::UnifiedRuntime::bootstrap(
            mob_spec,
            crate::MobKitConfig {
                modules: vec![],
                discovery: crate::DiscoverySpec {
                    namespace: "schedule-none".to_string(),
                    modules: vec![],
                },
                pre_spawn: vec![],
            },
            Duration::from_secs(2),
        )
        .await
        .expect("bootstrap schedule test runtime");

        let identity =
            crate::identity_first::AgentIdentity::parse("domain:scheduled").expect("identity");
        let alias = "rt:domain:scheduled:0";
        let lease_provider = Arc::new(crate::identity_first::LocalLeaseProvider::new());
        let lease_results = crate::identity_first::LeaseProvider::acquire_leases(
            lease_provider.as_ref(),
            std::slice::from_ref(&identity),
            "schedule-none",
        )
        .await
        .expect("acquire identity lease");
        let lease = match lease_results.get(&identity) {
            Some(crate::identity_first::LeaseAcquireResult::Acquired(lease)) => lease.clone(),
            other => panic!("expected acquired identity lease, got {other:?}"),
        };
        let record = crate::identity_first::ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: crate::identity_first::AgentRuntimeId::parse(alias)
                .expect("runtime alias"),
            session_id: SessionId::new(),
            generation: crate::identity_first::ContinuityGeneration::new(0),
            checkpoint_version: crate::identity_first::CheckpointVersion::new(0),
        };
        let identity_runtime = Arc::new(crate::identity_first::IdentityRuntime::new(
            crate::identity_first::IdentityRuntimeConfig {
                continuity_store: Arc::new(
                    crate::identity_first::LocalContinuityStore::in_memory()
                        .expect("identity store"),
                ),
                lease_provider,
                runtime_instance_id: "schedule-none".to_string(),
                has_runtime_store: true,
                durability_policy: crate::identity_first::DurabilityPolicy::SyncWriteThrough,
                bridge: None,
                default_timeout: None,
            },
        ));
        identity_runtime
            .register(
                crate::identity_first::DurableAgentSpec {
                    identity,
                    profile: meerkat_mob::ProfileName::from("general"),
                    addressability: crate::identity_first::AgentAddressability::Addressable,
                    display_name: None,
                    labels: BTreeMap::new(),
                    context: None,
                    additional_instructions: Vec::new(),
                    initial_message: None,
                    runtime_mode_override: None,
                    backend: None,
                    binding: None,
                },
                crate::identity_first::IdentityLifecycleState::Active,
                Some(record),
                Some(lease),
            )
            .await;

        let schedule = Schedule::new(CreateScheduleRequest {
            name: Some("identity-no-bridge".to_string()),
            description: None,
            trigger: TriggerSpec::Interval(IntervalTriggerSpec {
                start_at_utc: chrono::Utc::now(),
                every_seconds: 60,
                end_at_utc: None,
            }),
            target: TargetBinding::session(SessionTargetBinding::ExactSession {
                session_id: SessionId::new(),
                action: ScheduledSessionAction::Prompt {
                    prompt: meerkat_core::ContentInput::Text("deliver".to_string()),
                    system_prompt: None,
                    render_metadata: None,
                    skill_refs: Vec::new(),
                    additional_instructions: Vec::new(),
                },
            }),
            misfire_policy: meerkat::MisfirePolicy::default(),
            overlap_policy: meerkat::OverlapPolicy::default(),
            missing_target_policy: meerkat::MissingTargetPolicy::default(),
            labels: BTreeMap::new(),
            planning_horizon_days: None,
            planning_horizon_occurrences: None,
        })
        .expect("schedule");
        let occurrence = Occurrence::planned_from_schedule(
            &schedule,
            meerkat::OccurrenceOrdinal(0),
            chrono::Utc::now(),
        )
        .expect("occurrence");
        let host = InternalDeliveryScheduleMobHost {
            inner: Arc::new(NoopScheduleMobHost::new("unused fallback")),
            handle: runtime.mob_handle(),
            identity_runtime: Some(identity_runtime),
        };

        let dispatch = host
            .deliver_internal_member_prompt(
                &occurrence,
                "schedule-none",
                alias,
                &meerkat_core::ContentInput::Text("deliver".to_string()),
            )
            .await
            .expect("identity dispatch must be handled terminally");
        let terminal = dispatch.completion.await.expect("delivery terminal");
        assert_eq!(terminal.phase, OccurrencePhase::DeliveryFailed);
        assert_eq!(
            terminal.delivery_failure_reason,
            Some(meerkat::DeliveryFailureReason::RuntimeRejected)
        );
        assert!(
            terminal
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("no bound session bridge"))
        );

        runtime.shutdown().await;
    }

    /// HomeCore 0.7.26 "last link" e2e: an agent-authored one-shot must
    /// DELIVER through the real schedule host — the full rpc_gateway chain
    /// (attach_schedule_tools_with_identity_targets → agent tool dispatch →
    /// planning → claim → delivery). `register_mob_state` toggles whether
    /// the mob-target registry can resolve the authoring session at create
    /// time (the field failure fired on the unresolved path).
    async fn one_shot_delivery_e2e(register_mob_state: bool) -> (String, String) {
        one_shot_delivery_e2e_with_addressability(register_mob_state, true).await
    }

    async fn one_shot_delivery_e2e_with_addressability(
        register_mob_state: bool,
        external_addressable: bool,
    ) -> (String, String) {
        one_shot_delivery_e2e_with_member(register_mob_state, external_addressable, "digest-owner")
            .await
    }

    async fn one_shot_delivery_e2e_with_member(
        register_mob_state: bool,
        external_addressable: bool,
        member_identity: &str,
    ) -> (String, String) {
        use meerkat_core::AgentToolDispatcher;
        use serde_json::value::RawValue;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let state = temp_dir.path().join("state");
        std::fs::create_dir_all(&state).expect("state dir");
        let session_store: Arc<dyn meerkat::SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(state.join("sessions.db"))
                .expect("session store"),
        );
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new(state.join("runtime.sqlite"))
                .expect("runtime store"),
        );
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(meerkat_store::MemoryBlobStore::new());
        let factory = meerkat::AgentFactory::new(&state).comms(true);
        let mut inner_builder = FactoryAgentBuilder::new(factory, Config::default());
        inner_builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
            session_store.clone(),
        )));
        inner_builder.default_blob_store = Some(blob_store.clone());
        let attached = attach_schedule_tools_with_identity_targets(&inner_builder, &state)
            .expect("schedule tools attach");
        let inner_builder_mob_tools_slot = Arc::clone(&inner_builder.default_mob_tools);
        let adapter = Arc::new(meerkat_runtime::MeerkatMachine::persistent(
            Arc::clone(&runtime_store),
            Arc::clone(&blob_store),
        ));
        let concrete = Arc::new(PersistentSessionService::new(
            inner_builder,
            16,
            session_store,
            runtime_store,
            blob_store,
        ));
        let definition = meerkat_mob::MobDefinition::from_toml(&format!(
            r#"
[mob]
id = "delivery-e2e"

[profiles.general]
model = "gpt-5.5"
external_addressable = {external_addressable}

[profiles.general.tools]
comms = true
schedule = true
"#
        ))
        .expect("definition");
        let agent_mob_tools_slot = Arc::clone(&inner_builder_mob_tools_slot);
        let mob_spec = crate::mob_handle_runtime::MobBootstrapSpec::new(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            concrete.clone(),
        )
        .with_session_runtime_adapter(adapter.clone())
        .with_agent_mob_tools(agent_mob_tools_slot)
        .with_options(crate::mob_handle_runtime::MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(meerkat_client::TestClient::default())),
        });
        let runtime = crate::UnifiedRuntime::bootstrap(
            mob_spec,
            crate::MobKitConfig {
                modules: vec![],
                discovery: crate::DiscoverySpec {
                    namespace: "delivery-e2e".to_string(),
                    modules: vec![],
                },
                pre_spawn: vec![],
            },
            std::time::Duration::from_secs(2),
        )
        .await
        .expect("bootstrap");

        let handle = runtime.mob_handle();
        handle
            .ensure_member(meerkat_mob::SpawnMemberSpec::new(
                meerkat_mob::ProfileName::from("general"),
                meerkat_mob::ids::AgentIdentity::from(member_identity),
            ))
            .await
            .expect("ensure schedule author member");
        let owner_session = handle
            .resolve_bridge_session_id_observation(&meerkat_mob::ids::AgentIdentity::from(
                member_identity,
            ))
            .await
            .expect("owner session id");

        // A generated runtime alias is not merely a funny-looking raw member
        // id: give the schedule host the same durable authority the gateway
        // carries in production. This keeps the encoded-roster regression
        // realistic while ensuring an authority-less host remains fail-closed.
        let public_member_alias =
            crate::member_comms_id::runtime_alias_str(member_identity).into_owned();
        let identity_runtime =
            if crate::member_comms_id::is_reserved_generated_alias(&public_member_alias) {
                let identity =
                    crate::identity_first::IdentityRuntime::identity_for_generated_member_alias(
                        &public_member_alias,
                    )
                    .expect("generated alias identity");
                let continuity_store = Arc::new(
                    crate::identity_first::LocalContinuityStore::in_memory()
                        .expect("identity continuity store"),
                );
                let lease_provider = Arc::new(crate::identity_first::LocalLeaseProvider::new());
                let lease_results = crate::identity_first::LeaseProvider::acquire_leases(
                    lease_provider.as_ref(),
                    std::slice::from_ref(&identity),
                    "delivery-e2e",
                )
                .await
                .expect("identity lease");
                let lease = match lease_results.get(&identity) {
                    Some(crate::identity_first::LeaseAcquireResult::Acquired(grant)) => {
                        grant.clone()
                    }
                    other => panic!("expected acquired identity lease, got {other:?}"),
                };
                let record = crate::identity_first::ContinuityRecord {
                    identity: identity.clone(),
                    agent_runtime_id: crate::identity_first::AgentRuntimeId::parse(
                        &public_member_alias,
                    )
                    .expect("runtime alias"),
                    session_id: owner_session.clone(),
                    generation: crate::identity_first::ContinuityGeneration::new(0),
                    checkpoint_version: crate::identity_first::CheckpointVersion::new(0),
                };
                crate::identity_first::ContinuityStore::upsert_continuity_record(
                    continuity_store.as_ref(),
                    &record,
                    lease.fencing_token,
                )
                .await
                .expect("persist identity continuity");
                let identity_runtime = Arc::new(crate::identity_first::IdentityRuntime::new(
                    crate::identity_first::IdentityRuntimeConfig {
                        continuity_store,
                        lease_provider,
                        runtime_instance_id: "delivery-e2e".to_string(),
                        has_runtime_store: true,
                        durability_policy:
                            crate::identity_first::DurabilityPolicy::SyncWriteThrough,
                        bridge: Some(Arc::new(crate::identity_first::MobSessionBridge::new(
                            handle.clone(),
                        ))),
                        default_timeout: None,
                    },
                ));
                identity_runtime
                    .register(
                        crate::identity_first::DurableAgentSpec {
                            identity,
                            profile: meerkat_mob::ProfileName::from("general"),
                            addressability: crate::identity_first::AgentAddressability::Addressable,
                            display_name: None,
                            labels: std::collections::BTreeMap::new(),
                            context: None,
                            additional_instructions: Vec::new(),
                            initial_message: None,
                            runtime_mode_override: None,
                            backend: None,
                            binding: None,
                        },
                        crate::identity_first::IdentityLifecycleState::Active,
                        Some(record),
                        Some(lease),
                    )
                    .await;
                Some(identity_runtime)
            } else {
                None
            };

        let mob_state = runtime.mob_runtime().agent_mob_mcp_state();
        assert!(
            mob_state.is_some(),
            "with_agent_mob_tools must install the mob authority"
        );
        if register_mob_state {
            attached
                .mob_target_registry
                .set_mob_state(mob_state.clone());
        }

        // Author AS THE AGENT: through the installed dispatcher chain, bound
        // to the member's session, current-session target, due just-past.
        let mob_dispatcher = MobIdentityScheduleToolDispatcher::new(
            Arc::new(ScheduleToolDispatcher::new(attached.service.clone())),
            attached.mob_target_registry.clone(),
        );
        let dispatcher = meerkat_schedule::CurrentSessionScheduleToolDispatcher::new(
            Arc::new(mob_dispatcher),
            owner_session.clone(),
        );
        let due = chrono::Utc::now() + chrono::Duration::seconds(2);
        let create_args = RawValue::from_string(
            serde_json::json!({
                "name": "delivery-e2e-oneshot",
                "trigger": { "type": "once", "due_at_utc": due.to_rfc3339() },
                "target": {
                    "target_kind": "session",
                    "type": "current_session",
                    "action": {
                        "type": "prompt",
                        "prompt": "Fire the e2e one-shot.",
                        "render_metadata": { "class": "external_event", "salience": "important", "source": "delivery-e2e" }
                    }
                }
            })
            .to_string(),
        )
        .expect("create args");
        dispatcher
            .dispatch(meerkat_core::types::ToolCallView {
                id: "create-e2e",
                name: "meerkat_schedule_create",
                args: &create_args,
            })
            .await
            .expect("author one-shot through the agent dispatcher");
        let authored_target = attached
            .service
            .list()
            .await
            .expect("list")
            .first()
            .map(|s| s.target.clone());
        if register_mob_state {
            assert!(
                matches!(
                    &authored_target,
                    Some(TargetBinding::Mob(binding))
                        if matches!(binding.as_ref(), MobTargetBinding::Member { .. })
                ),
                "with the mob authority registered, agent-authored current-session \
                 schedules must rewrite to mob-member targets: {authored_target:?}"
            );
        }

        // Ensure mob state is registered for DELIVERY either way (rpc_gateway
        // always sets it before spawning the host).
        attached
            .mob_target_registry
            .set_mob_state(mob_state.clone());
        // A workgraph service on the host exercises upstream arg-8
        // pass-through on every delivery e2e (overlay injection is inert
        // without attention bindings).
        let _host = spawn_schedule_host_with_identity_runtime(
            concrete,
            adapter,
            attached.service.clone(),
            mob_state,
            handle.clone(),
            identity_runtime,
            None,
            Some(crate::workgraph_wiring::ephemeral_workgraph_service(
                "delivery-e2e",
            )),
            "delivery-e2e",
        )
        .expect("spawn schedule host");

        // Poll receipts until one lands (or timeout) — receipts are keyed by
        // occurrence, so read raw rows from the store file.
        let receipts_path = state.join(SCHEDULE_STORE_FILE);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let row: Option<Vec<u8>> = {
                let conn = rusqlite::Connection::open(&receipts_path).expect("open receipts store");
                conn.query_row(
                    "SELECT receipt_json FROM schedule_receipts ORDER BY rowid DESC LIMIT 1",
                    [],
                    |r| r.get::<_, Vec<u8>>(0),
                )
                .ok()
            };
            if let Some(receipt_bytes) = row {
                let receipt_json = String::from_utf8_lossy(&receipt_bytes).to_string();
                let receipt: serde_json::Value =
                    serde_json::from_str(&receipt_json).unwrap_or_default();
                let stage = receipt["stage"].as_str().unwrap_or_default().to_string();
                // Only a TERMINAL delivery verdict counts — planner
                // bookkeeping (superseded) and in-flight stages
                // (dispatch_started/accepted) keep the poll waiting.
                if matches!(
                    stage.as_str(),
                    "completed" | "delivery_failed" | "misfired" | "skipped"
                ) {
                    runtime.shutdown().await;
                    return (stage, receipt_json);
                }
            }
            if std::time::Instant::now() >= deadline {
                let conn = rusqlite::Connection::open(&receipts_path).expect("open store for dump");
                let mut stmt = conn
                    .prepare(
                        "SELECT phase, due_at_ms, lease_expires_at_ms FROM schedule_occurrences",
                    )
                    .expect("prep");
                let rows: Vec<(String, i64, Option<i64>)> = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                    .expect("q")
                    .filter_map(Result::ok)
                    .collect();
                panic!("no delivery receipt within 20s; occurrences: {rows:?}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }

    /// Registry-resolved authoring (rpc_gateway steady state): the one-shot
    /// rewrites to a mob-member target at authoring and DELIVERS.
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_authored_one_shot_delivers_to_mob_member() {
        let (stage, detail) = one_shot_delivery_e2e(true).await;
        assert_eq!(
            stage, "completed",
            "one-shot must deliver to the member: {detail}"
        );
    }

    /// HomeCore field case: domain agents are internal_only by design (only
    /// person identities are externally addressable). A schedule firing back
    /// into ITS OWN AUTHOR's session is internal addressing — the external
    /// addressability posture must not block self-delivery.
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_authored_one_shot_delivers_to_internal_only_author() {
        let (stage, detail) = one_shot_delivery_e2e_with_addressability(true, false).await;
        assert_eq!(
            stage, "completed",
            "self-delivery to an internal_only author must succeed: {detail}"
        );
    }

    /// HomeCore 0.7.28 field case: identity-first bridge members' ROSTER ids
    /// are the comms-ENCODED runtime id (`mk--rt_cdomain_chome_c0` for
    /// `rt:domain:home:0` — bridge.rs member_id_for_spawn_spec), and the
    /// authoring rewrite stores that roster id in the binding. The internal
    /// lane must resolve it WITHOUT re-encoding (the codec re-encodes
    /// marker-prefixed input by design); before the canonicalization fix the
    /// lookup missed and delivery fell through to the external door:
    /// "mob member is not externally addressable: mk--rt_cdomain_chome_c0".
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_authored_one_shot_delivers_to_internal_only_identity_bridge_member() {
        let roster_id = crate::member_comms_id::mob_member_id_str("rt:domain:home:0").into_owned();
        assert!(
            roster_id.starts_with("mk--"),
            "repro precondition: the roster id must be marker-encoded"
        );
        let (stage, detail) = one_shot_delivery_e2e_with_member(true, false, &roster_id).await;
        assert_eq!(
            stage, "completed",
            "self-delivery to an internal_only identity-bridge member (roster-space \
             binding id) must take the internal lane: {detail}"
        );
    }

    /// Unresolved-at-authoring shape (the HomeCore 0.7.26 field path): the
    /// target stays a resumable session, and DELIVERY-TIME recovery resolves
    /// the mob-member identity through the (now installed) mob authority.
    /// Before the with_agent_mob_tools fix this failed with
    /// "scheduled identity targets are not supported by this session host".
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_authored_one_shot_delivers_via_identity_recovery() {
        let (stage, detail) = one_shot_delivery_e2e(false).await;
        assert_eq!(
            stage, "completed",
            "delivery-time identity recovery must deliver: {detail}"
        );
    }

    /// Ask-22 upgrade-carry guard: a just-past ONE-SHOT must converge —
    /// bounded occurrences over repeated refill+claim rounds.
    ///
    /// Root cause (found by Luka, fix targeted at meerkat 0.7.20):
    /// sub-millisecond precision loss in the planning-cursor round-trip —
    /// the cursor is machine-owned at ms precision (truncate_ms(due)) while
    /// `next_due_after(Once, cursor)` compares at ns precision, so
    /// `due > cursor` stays true forever and the planner re-yields the same
    /// due each tick (one spawn per tick ≈ the field's ~1/sec, unbounded).
    /// Reproduces with PURE meerkat service APIs (refill_horizon +
    /// claim_due_occurrences) — no mobkit code in the loop; the claim
    /// watchdog is read-only and ticks at 60s, exonerated twice over.
    /// Fixed in meerkat 0.7.20: trigger yields/compares ms-truncated dues,
    /// and the ScheduleLifecycleMachine owns a planning-monotonicity
    /// invariant so future representation bugs converge as refill faults.
    #[tokio::test]
    async fn one_shot_misfire_must_not_regenerate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store_path = dir.path().join(SCHEDULE_STORE_FILE);
        let store = SqliteScheduleStore::open(&store_path).expect("open store");
        let service = ScheduleService::new(Arc::new(store));
        // One-shot due WELL past (beyond any catch-up window → misfires).
        let created = service
            .create(CreateScheduleRequest {
                name: Some("oneshot-runaway-probe".to_string()),
                description: None,
                trigger: TriggerSpec::Once {
                    due_at_utc: chrono::Utc::now() - chrono::Duration::seconds(30),
                },
                target: TargetBinding::host_runnable(HostRunnableTargetBinding {
                    runnable: HostRunnableName::parse("runaway.probe").expect("name"),
                    params: None,
                }),
                misfire_policy: meerkat::MisfirePolicy::default(),
                overlap_policy: meerkat::OverlapPolicy::default(),
                missing_target_policy: meerkat::MissingTargetPolicy::default(),
                labels: std::collections::BTreeMap::new(),
                planning_horizon_days: None,
                planning_horizon_occurrences: None,
            })
            .await
            .expect("create one-shot");

        let count_occurrences = |path: std::path::PathBuf| -> i64 {
            let conn = rusqlite::Connection::open(path).expect("open");
            conn.query_row("SELECT COUNT(*) FROM schedule_occurrences", [], |r| {
                r.get(0)
            })
            .expect("count")
        };

        // DEBUG: dump schedule + occurrence bookkeeping after create.
        {
            let conn = rusqlite::Connection::open(&store_path).expect("open");
            let (rev, cursor): (i64, Option<i64>) = conn
                .query_row(
                    "SELECT revision, planning_cursor_at_ms FROM schedule_schedules LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .expect("schedule row");
            eprintln!("after create: schedule revision={rev} planning_cursor_ms={cursor:?}");
            let mut stmt = conn
                .prepare(
                    "SELECT occurrence_ordinal, schedule_revision, phase FROM schedule_occurrences",
                )
                .expect("prep");
            let rows: Vec<(i64, i64, String)> = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .expect("q")
                .filter_map(Result::ok)
                .collect();
            eprintln!("occurrences after create: {rows:?}");
        }
        // Simulate driver ticks: refill + claim, several rounds.
        for round in 0..6 {
            let _ = service.refill_horizon(&created.schedule_id).await;
            let _ = service
                .store()
                .claim_due_occurrences(meerkat::ClaimDueRequest {
                    owner_id: "runaway-probe".to_string(),
                    limit: 8,
                    lease_duration: chrono::Duration::seconds(60),
                })
                .await;
            {
                let conn = rusqlite::Connection::open(&store_path).expect("open");
                let (rev, cursor): (i64, Option<i64>) = conn
                    .query_row(
                        "SELECT revision, planning_cursor_at_ms FROM schedule_schedules LIMIT 1",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .expect("schedule row");
                eprintln!(
                    "round {round}: occurrences = {} schedule_rev={rev} cursor_ms={cursor:?}",
                    count_occurrences(store_path.clone())
                );
            }
        }
        let total = count_occurrences(store_path.clone());
        assert!(
            total <= 1,
            "a one-shot must never regenerate after misfire; got {total} occurrences"
        );
    }

    /// Healthy pipeline: future work only, nothing overdue → the watchdog has
    /// nothing to say.
    #[tokio::test]
    async fn schedule_claim_watchdog_probe_is_healthy_on_future_work() {
        let dir = tempfile::tempdir().expect("tempdir");
        let start = chrono::Utc::now() + chrono::Duration::hours(1);
        let (service, store_path) = seed_sqlite_schedule(dir.path(), start).await;

        let probe =
            probe_schedule_firing_pipeline(&service, &store_path, Duration::from_secs(0)).await;
        assert_eq!(probe, ScheduleFiringProbe::Healthy);
    }

    /// HomeCore Observation A: due occurrences sit pending with no lease and
    /// the driver says nothing. The probe must call that out loudly.
    #[tokio::test]
    async fn schedule_claim_watchdog_probe_flags_overdue_unclaimed_occurrences() {
        let dir = tempfile::tempdir().expect("tempdir");
        let start = chrono::Utc::now() + chrono::Duration::hours(1);
        let (service, store_path) = seed_sqlite_schedule(dir.path(), start).await;

        // Age the first planned occurrence 10 minutes into the past — both the
        // projection's due_at_utc and the machine state's due_at_utc_ms (they
        // are recovery-checked against each other), plus the ordering column.
        let overdue = chrono::Utc::now() - chrono::Duration::minutes(10);
        {
            let conn = rusqlite::Connection::open(&store_path).expect("open store");
            let (rowid, bytes): (i64, Vec<u8>) = conn
                .query_row(
                    "SELECT rowid, occurrence_json FROM schedule_occurrences \
                     ORDER BY rowid LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("read first occurrence");
            let mut json: serde_json::Value =
                serde_json::from_slice(&bytes).expect("occurrence json");
            let old_due_ms = json["machine_state"]["due_at_utc_ms"]
                .as_i64()
                .expect("machine due ms");
            let shift = old_due_ms - overdue.timestamp_millis();
            json["due_at_utc"] = serde_json::Value::String(overdue.to_rfc3339());
            json["machine_state"]["due_at_utc_ms"] =
                serde_json::Value::from(overdue.timestamp_millis());
            if let Some(deadline) = json["machine_state"]["misfire_deadline_utc_ms"].as_i64() {
                json["machine_state"]["misfire_deadline_utc_ms"] =
                    serde_json::Value::from(deadline - shift);
            }
            let updated = serde_json::to_vec(&json).expect("serialize occurrence");
            conn.execute(
                "UPDATE schedule_occurrences SET occurrence_json = ?1, due_at_ms = ?2 \
                 WHERE rowid = ?3",
                rusqlite::params![updated, overdue.timestamp_millis(), rowid],
            )
            .expect("age occurrence");
        }

        let probe =
            probe_schedule_firing_pipeline(&service, &store_path, Duration::from_mins(1)).await;
        let ScheduleFiringProbe::Stalled { report } = probe else {
            panic!("an overdue unclaimed occurrence must probe as Stalled");
        };
        assert!(
            report.contains("never claimed"),
            "report must state the claim stall: {report}"
        );
    }

    /// HomeCore Observation B: one poisoned schedule row (e.g. a Deleted
    /// tombstone the recovery invariant rejects) fails the whole list, which
    /// aborts every driver tick before claiming. The probe must surface the
    /// list failure and name the poisoned row.
    #[tokio::test]
    async fn schedule_claim_watchdog_probe_names_poisoned_schedule_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let start = chrono::Utc::now() + chrono::Duration::hours(1);
        let (service, store_path) = seed_sqlite_schedule(dir.path(), start).await;
        corrupt_first_row(&store_path, "schedule_schedules", "schedule_json");

        let probe =
            probe_schedule_firing_pipeline(&service, &store_path, Duration::from_mins(1)).await;
        let ScheduleFiringProbe::Stalled { report } = probe else {
            panic!("a poisoned schedule row must probe as Stalled");
        };
        assert!(
            report.contains("schedule list is failing"),
            "report must name the failing surface: {report}"
        );
        assert!(
            report.contains("schedule_schedules.schedule_id="),
            "report must name the poisoned row: {report}"
        );
    }

    /// The claim scan deserializes EVERY occurrence row before leasing
    /// anything, so one poisoned occurrence silently starves all schedules.
    /// The probe must surface the scan failure and name the row.
    #[tokio::test]
    async fn schedule_claim_watchdog_probe_names_poisoned_occurrence_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let start = chrono::Utc::now() + chrono::Duration::hours(1);
        let (service, store_path) = seed_sqlite_schedule(dir.path(), start).await;
        corrupt_first_row(&store_path, "schedule_occurrences", "occurrence_json");

        let probe =
            probe_schedule_firing_pipeline(&service, &store_path, Duration::from_mins(1)).await;
        let ScheduleFiringProbe::Stalled { report } = probe else {
            panic!("a poisoned occurrence row must probe as Stalled");
        };
        assert!(
            report.contains("occurrence scan is failing"),
            "report must name the failing surface: {report}"
        );
        assert!(
            report.contains("schedule_occurrences.occurrence_id="),
            "report must name the poisoned row: {report}"
        );
    }

    #[tokio::test]
    async fn ensure_steward_dream_schedule_is_idempotent_and_updates_cadence() {
        let service = ScheduleService::new(Arc::new(MemoryScheduleStore::default()));
        let now: chrono::DateTime<chrono::Utc> = "2026-07-01T00:00:00Z".parse().expect("fixed now");

        // First call creates exactly one host-runnable schedule.
        ensure_steward_dream_schedule(&service, Duration::from_hours(6), now)
            .await
            .expect("create dream schedule");
        let schedules = service.list().await.expect("list");
        assert_eq!(
            dream_target_count(&schedules),
            1,
            "one dream schedule created"
        );
        assert_eq!(dream_interval_seconds(&schedules[0]), 6 * 3600);

        // Re-running with the same cadence is a no-op — no duplicate stacking
        // across boots.
        ensure_steward_dream_schedule(&service, Duration::from_hours(6), now)
            .await
            .expect("idempotent re-ensure");
        let schedules = service.list().await.expect("list");
        assert_eq!(
            dream_target_count(&schedules),
            1,
            "still one dream schedule"
        );

        // A changed cadence updates the existing schedule in place, not a new
        // one.
        ensure_steward_dream_schedule(&service, Duration::from_mins(30), now)
            .await
            .expect("update cadence");
        let schedules = service.list().await.expect("list");
        assert_eq!(
            dream_target_count(&schedules),
            1,
            "cadence change reuses schedule"
        );
        assert_eq!(dream_interval_seconds(&schedules[0]), 30 * 60);
    }

    #[tokio::test]
    async fn repair_rewrites_persisted_resumable_session_prompt_targets() {
        let service = ScheduleService::new(Arc::new(MemoryScheduleStore::default()));
        let request = serde_json::from_value(json!({
            "name": "legacy-digest",
            "trigger": {
                "type": "interval",
                "start_at_utc": "2026-07-01T05:00:00Z",
                "every_seconds": 86400
            },
            "target": {
                "target_kind": "session",
                "type": "resumable_session",
                "session_id": "019ee0a7-a594-7670-b530-97e7c9e263b7",
                "action": {
                    "type": "prompt",
                    "prompt": "Send the morning digest.",
                    "render_metadata": {
                        "class": "external_event",
                        "salience": "important"
                    }
                }
            },
            "planning_horizon_occurrences": 1
        }))
        .expect("create request");
        service
            .create(request)
            .await
            .expect("create legacy schedule");

        let repaired =
            repair_resumable_session_targets_with_resolver(&service, |session_id| async move {
                assert_eq!(session_id, "019ee0a7-a594-7670-b530-97e7c9e263b7");
                Some(("homecore".to_string(), "domain:security".to_string()))
            })
            .await
            .expect("repair");

        assert_eq!(repaired, 1);
        let schedules = service.list().await.expect("list schedules");
        assert_eq!(schedules.len(), 1);
        assert_mob_member_target(
            &schedules[0].target,
            "homecore",
            "domain:security",
            "Send the morning digest.",
            "external_event",
        );
    }
}
