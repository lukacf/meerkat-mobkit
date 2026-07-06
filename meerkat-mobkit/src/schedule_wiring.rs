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
    NoopScheduleMobHost, ScheduleHostHandle, SurfaceScheduleMobHost,
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
use meerkat_mob_mcp::{MobMcpScheduleHost, MobMcpState};
use meerkat_runtime::MeerkatMachine;
use serde_json::{Map, Value};

use crate::memory::steward::StewardEngine;

/// File name for the durable schedule store, kept beside the runtime DB so a
/// gateway and a library-mode runtime pointed at the same dir share state.
pub const SCHEDULE_STORE_FILE: &str = "schedule.sqlite";

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
#[must_use]
pub fn spawn_schedule_host<B: SessionAgentBuilder + 'static>(
    service: Arc<PersistentSessionService<B>>,
    adapter: Arc<MeerkatMachine>,
    schedule_service: ScheduleService,
    mob_state: Option<Arc<MobMcpState>>,
    runnable_host: Option<Arc<dyn meerkat::ScheduleRunnableHost>>,
    owner_id: impl Into<String>,
) -> Option<ScheduleHostHandle> {
    let mob_host: Arc<dyn SurfaceScheduleMobHost> = match mob_state {
        Some(state) => Arc::new(MobMcpScheduleHost::new(state)),
        None => Arc::new(NoopScheduleMobHost::new(
            "scheduled mob targets are not supported: no mob runtime",
        )),
    };
    // meerkat 0.7.13 (upstream ask 10): thread a host-runnable registry so
    // host-registered runnables (e.g. the memory steward's dream) can be
    // driven as schedule occurrences. `None` keeps mob/session targets only.
    spawn_runtime_backed_schedule_host_with_mobs(
        service,
        adapter,
        Config::default(),
        schedule_service,
        SessionBuildOptions::default(),
        mob_host,
        runnable_host,
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
