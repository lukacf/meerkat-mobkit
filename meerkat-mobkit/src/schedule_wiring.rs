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
//! This is distinct from the static cron oracle (`MobKitBuilder::scheduling`),
//! which drives module-dispatch ticks, not per-agent schedule tools.

use std::path::Path;
use std::sync::Arc;

use meerkat::surface::{
    NoopScheduleMobHost, ScheduleHostHandle, SurfaceScheduleMobHost,
    spawn_runtime_backed_schedule_host_with_mobs,
};
use meerkat::{
    Config, FactoryAgentBuilder, PersistentSessionService, ScheduleService, ScheduleToolDispatcher,
    SessionAgentBuilder, SqliteScheduleStore,
};
use meerkat_core::service::SessionBuildOptions;
use meerkat_mob_mcp::{MobMcpScheduleHost, MobMcpState};
use meerkat_runtime::MeerkatMachine;

/// File name for the durable schedule store, kept beside the runtime DB so a
/// gateway and a library-mode runtime pointed at the same dir share state.
pub const SCHEDULE_STORE_FILE: &str = "schedule.sqlite";

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
    meerkat::surface::set_default_schedule_tools(
        builder,
        Some(Arc::new(ScheduleToolDispatcher::new(service.clone()))),
    );
    Some(service)
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
    owner_id: impl Into<String>,
) -> Option<ScheduleHostHandle> {
    let mob_host: Arc<dyn SurfaceScheduleMobHost> = match mob_state {
        Some(state) => Arc::new(MobMcpScheduleHost::new(state)),
        None => Arc::new(NoopScheduleMobHost::new(
            "scheduled mob targets are not supported: no mob runtime",
        )),
    };
    spawn_runtime_backed_schedule_host_with_mobs(
        service,
        adapter,
        Config::default(),
        schedule_service,
        SessionBuildOptions::default(),
        mob_host,
        owner_id,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use meerkat::AgentFactory;

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
}
