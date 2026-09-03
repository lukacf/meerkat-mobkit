//! Shared composition-root scaffolding for the two gateway binaries
//! (simplification item 10, "unify gateway composition roots").
//!
//! # Charter, and how it differs from [`crate::gateway_wiring`]
//!
//! [`crate::gateway_wiring`] owns ONE thing: opening the identity substrate
//! (continuity store + lease provider) so both gateways get the same durable
//! identity semantics. That module is deliberately narrow.
//!
//! This module owns the COMPOSITION ROOT itself: the process-level boot
//! scaffolding a gateway binary performs, in order, around the runtime it
//! builds. Five bands live here:
//!
//! 1. The boot preamble - things done before any runtime exists (tokio
//!    runtime, tracing subscriber, argv parsing).
//! 2. Explicit compatibility-profile declarations. The two binaries retain
//!    their existing wire/config shapes, but those differences can no longer
//!    be mistaken for independent composition authority.
//! 3. A type-state runtime root - prepare, bootstrap, profile enrichment,
//!    activate - which is the only gateway path into `UnifiedRuntime`.
//! 4. Post-bootstrap surface attachment that both binaries perform with
//!    identical observable behaviour (the schedule firing host).
//! 5. Loopback HTTP admission and ordered cooperative shutdown. HTTP stops
//!    first, profile-owned drains quiesce second, runtime authority cleanup
//!    completes third, and registry/process cleanup is last.
//!
//! The rule for adding to this module: a seam belongs here when BOTH
//! binaries must do it and the observable behaviour (log strings, ordering,
//! error posture) is already identical. Substrate construction belongs in
//! `gateway_wiring`; if a seam here starts opening stores, it has forked the
//! charter and should move.
//!
//! # Compatibility differences intentionally retained
//!
//! This is the structural history the item asks for. Every claim below was
//! read off the source in this tree, and is cited BY SYMBOL, never by line
//! number: both binaries are under active edit, and a line-numbered survey
//! decays into a survey that sends its reader to the wrong place while still
//! looking authoritative.
//!
//! The former roots are `run` in `src/bin/mobkit_gateway.rs` and
//! `run_persistent_inner` in `src/bin/rpc_gateway.rs`. They now translate
//! their compatibility inputs into [`GatewayRuntimeBootstrapPlan`] and use
//! [`GatewayComposition`] for runtime and lifecycle ownership.
//!
//! - **Init params.** `mobkit_gateway` deserializes a typed `InitParams`
//!   struct straight off the TOP LEVEL of `params` (`parse_init_request`).
//!   `rpc_gateway` reads exactly one ad-hoc top-level key
//!   (`params.get("mob_config")`) and routes everything else through
//!   `parse_gateway_runtime_options`, which reads the NESTED
//!   `params.runtime_options` object. These are not merely two different
//!   types: `console_read_only` and `compaction` exist in BOTH vocabularies
//!   at DIFFERENT nesting depths, so converging them is wire-visible to both
//!   SDKs. They also disagree on strictness - `InitParams` derives a plain
//!   `Deserialize` and silently ignores unknown keys, while
//!   `parse_gateway_runtime_options` refuses them ("unsupported
//!   runtime_options fields"). Largest single remaining piece.
//!
//! - **Bootstrap inputs.** The console profile keeps empty events, default
//!   runtime options and in-memory metadata. The stdio profile keeps its
//!   configured options and persistent metadata. Both flow through the same
//!   exact low-level bootstrap call.
//!
//! - **Console log store.** `rpc_gateway` installs `SqliteConsoleLogStore`
//!   for persistent launches (`set_console_log_store`). `mobkit_gateway`
//!   never calls `set_console_log_store`; its console timeline is in-memory
//!   by contract, declared through `StorageSlotSummary::declared_ephemeral`
//!   for both the `console` and the `metadata` slot.
//!
//! - **Identity mechanism.** Two different attachment paths, not two
//!   configurations of one. `mobkit_gateway` builds an
//!   `IdentityFirstRuntimeContext` and calls
//!   `attach_identity_first_context`, default ON
//!   (`params.identity_first.unwrap_or(true)`). `rpc_gateway` builds a
//!   `crate::rpc::IdentityFirstContext` and threads it through the stdio
//!   dispatcher rather than attaching it to the runtime; it is gated on the
//!   SDK's `has_roster_provider` flag.
//!
//! - **Console identity seam.** Each binary installs a DIFFERENT one, and
//!   neither installs the other's: `mobkit_gateway` calls
//!   `runtime.set_console_identity_roster`, `rpc_gateway` calls
//!   `runtime.set_console_operator_resolver`.
//!
//! - **Memory / steward / gating / job surfaces.** Present only in
//!   `rpc_gateway`: `GatewayMobPurposeSource`, `GatewayMemoryGatingBridge`,
//!   `GatewayMemoryConflictBridge`, the `StewardEngine` slot,
//!   `register_gating_resolution_observer`, `set_memory_panel_store`,
//!   `set_error_hook` and `set_job_health_projection`. `mobkit_gateway`
//!   wires none of them, so the agent-memory panel, the promotion gate and
//!   the job-health projection are structurally absent from the
//!   console-only gateway.
//!
//! - **Runnable hosts.** `rpc_gateway` composes a `gateway_runnable_host`
//!   from the steward dream plus SDK-declared
//!   `runtime_options.host_runnables`, and warns when they are configured
//!   with no host to run them. `mobkit_gateway` passes `None` and has no
//!   host-runnable vocabulary at all. This is the one divergence
//!   [`spawn_gateway_schedule_host`] below makes explicit rather than
//!   implicit: it is now a named field, not a hard-coded argument.
//!
//! - **Callback plane.** The stdio compatibility adapter owns `StdioCallbackBridge`,
//!   `DetachedCallbackJobRuntime`, `CallbackToolDispatcher` and
//!   `StdioCallbackAgentBuilder` - ~2,600 lines from the first of those to
//!   the start of `run_persistent`. `mobkit_gateway` has no callback plane;
//!   it builds agents directly through `FactoryAgentBuilder`. These cannot
//!   converge without the init-param convergence above, because the callback
//!   plane only exists when an SDK host is on the other end of stdin.
//!
//! - **Registry / resume.** `mobkit_gateway` maintains a cross-process
//!   runtime registry keyed by a config fingerprint and can answer
//!   `launch_state: "resumed"` without booting a runtime at all.
//!   `rpc_gateway` has no such concept.
//!
//! Profile-specific enrichment remains explicit between bootstrap and
//! activation. This is the checked seam list, not a closed census:
//! `set_contact_directory`,
//! `set_access_controller`, `set_gateway_peer_keys`,
//! `start_control_listener`, `with_session_write_epochs`,
//! `with_session_runtime_adapter`, `with_workgraph_service`,
//! `with_workgraph_admission_slot`, `with_workgraph_admission_sidecar`,
//! `with_runtime_archived_terminal_authority`,
//! `with_committed_boundary_recoverer`, `with_runtime_services`,
//! `with_image_generation_machine`, and the direct `spec.runtime_adapter` /
//! `spec.resolved_storage` / `spec.binary_blob_store` /
//! `spec.committed_boundary_recoverer` assignments.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use meerkat::surface::ScheduleHostHandle;
use meerkat::{
    PersistentSessionService, ScheduleRunnableHost, ScheduleService, SessionAgentBuilder,
    WorkGraphService,
};
use meerkat_mob_mcp::MobMcpState;
use meerkat_runtime::MeerkatMachine;

use crate::mob_handle_runtime::MobBootstrapSpec;
use crate::runtime::cross_mob_control::ControlListenAddr;
use crate::runtime::{InMemoryMetadataStore, PersistentMetadataStore, RuntimeOptions};
use crate::schedule_wiring::{
    ScheduleClaimWatchdogConfig, ScheduleFiringHostBinding, ScheduleMobTargetRegistry,
};
use crate::types::{EventEnvelope, MobKitConfig, UnifiedEvent};
use crate::unified_runtime::{
    UnifiedRuntime, UnifiedRuntimeBootstrapError, UnifiedRuntimeShutdownReport,
};

// ---------------------------------------------------------------------------
// Compatibility profiles
// ---------------------------------------------------------------------------

/// The two shipped binaries are compatibility profiles over one composition
/// root, not independent runtime assemblers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayCompatibilityProfile {
    /// `mobkit_gateway`: console/admin HTTP, top-level init params, registry
    /// resume and maintenance verbs.
    ConsoleHttp,
    /// `rpc_gateway --persistent`: SDK stdin JSON-RPC, callbacks and the
    /// private bounded shutdown handshake.
    StdioRpc,
}

/// Wire-visible differences which must remain explicit while the runtime
/// composition and lifecycle are shared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayCompatibilityContract {
    pub top_level_permissive_init: bool,
    pub registry_resume: bool,
    pub post_init_stdin_rpc: bool,
    pub callback_plane: bool,
    pub single_shot: bool,
    pub maintenance_verbs: bool,
}

impl GatewayCompatibilityProfile {
    pub const fn contract(self) -> GatewayCompatibilityContract {
        match self {
            Self::ConsoleHttp => GatewayCompatibilityContract {
                top_level_permissive_init: true,
                registry_resume: true,
                post_init_stdin_rpc: false,
                callback_plane: false,
                single_shot: false,
                maintenance_verbs: true,
            },
            Self::StdioRpc => GatewayCompatibilityContract {
                top_level_permissive_init: false,
                registry_resume: false,
                post_init_stdin_rpc: true,
                callback_plane: true,
                single_shot: true,
                maintenance_verbs: false,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Typed runtime composition stages
// ---------------------------------------------------------------------------

/// Exact low-level inputs to `UnifiedRuntime::bootstrap_with_options`.
///
/// The console profile uses [`Self::console_http`], which reproduces
/// `UnifiedRuntime::bootstrap` exactly. The stdio profile supplies its
/// existing events, options and metadata authority. Keeping both behind this
/// one call prevents a third bootstrap path without changing either wire
/// schema.
pub struct GatewayRuntimeBootstrapPlan {
    mob_spec: MobBootstrapSpec,
    module_config: MobKitConfig,
    module_agent_events: Vec<EventEnvelope<UnifiedEvent>>,
    timeout: Duration,
    runtime_options: RuntimeOptions,
    persistent_metadata: Arc<dyn PersistentMetadataStore>,
}

impl GatewayRuntimeBootstrapPlan {
    pub fn console_http(
        mob_spec: MobBootstrapSpec,
        module_config: MobKitConfig,
        timeout: Duration,
    ) -> Self {
        Self {
            mob_spec,
            module_config,
            module_agent_events: Vec::new(),
            timeout,
            runtime_options: RuntimeOptions::default(),
            persistent_metadata: Arc::new(InMemoryMetadataStore::new()),
        }
    }

    pub fn stdio_rpc(
        mob_spec: MobBootstrapSpec,
        module_config: MobKitConfig,
        module_agent_events: Vec<EventEnvelope<UnifiedEvent>>,
        timeout: Duration,
        runtime_options: RuntimeOptions,
        persistent_metadata: Arc<dyn PersistentMetadataStore>,
    ) -> Self {
        Self {
            mob_spec,
            module_config,
            module_agent_events,
            timeout,
            runtime_options,
            persistent_metadata,
        }
    }
}

pub struct PreparedGateway {
    plan: GatewayRuntimeBootstrapPlan,
}

pub struct BootstrappedGateway {
    runtime: UnifiedRuntime,
}

pub struct ActiveGateway {
    runtime: Arc<UnifiedRuntime>,
}

/// Type-state composition root. Profile-specific config parsing and optional
/// seams happen around this root, but runtime bootstrap and process lifecycle
/// have one owner.
pub struct GatewayComposition<State> {
    profile: GatewayCompatibilityProfile,
    state: State,
}

impl GatewayComposition<PreparedGateway> {
    pub fn prepare(
        profile: GatewayCompatibilityProfile,
        plan: GatewayRuntimeBootstrapPlan,
    ) -> Self {
        Self {
            profile,
            state: PreparedGateway { plan },
        }
    }

    pub async fn bootstrap(
        self,
    ) -> Result<GatewayComposition<BootstrappedGateway>, UnifiedRuntimeBootstrapError> {
        let plan = self.state.plan;
        let runtime = UnifiedRuntime::bootstrap_with_options(
            plan.mob_spec,
            plan.module_config,
            plan.module_agent_events,
            plan.timeout,
            plan.runtime_options,
            plan.persistent_metadata,
        )
        .await?;
        Ok(GatewayComposition {
            profile: self.profile,
            state: BootstrappedGateway { runtime },
        })
    }
}

impl GatewayComposition<BootstrappedGateway> {
    pub fn runtime(&self) -> &UnifiedRuntime {
        &self.state.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut UnifiedRuntime {
        &mut self.state.runtime
    }

    pub fn activate(self) -> GatewayComposition<ActiveGateway> {
        GatewayComposition {
            profile: self.profile,
            state: ActiveGateway {
                runtime: Arc::new(self.state.runtime),
            },
        }
    }
}

impl GatewayComposition<ActiveGateway> {
    pub fn profile(&self) -> GatewayCompatibilityProfile {
        self.profile
    }

    pub fn runtime(&self) -> &Arc<UnifiedRuntime> {
        &self.state.runtime
    }
}

// ---------------------------------------------------------------------------
// Shared HTTP admission and ordered shutdown
// ---------------------------------------------------------------------------

pub const GATEWAY_HTTP_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
/// Covers every bounded phase inside `UnifiedRuntime::shutdown`, including
/// `RETIRED_SUPERVISOR_JOIN_BUDGET`. Asserted component-by-component in
/// `advertised_shutdown_horizon_covers_every_bounded_gateway_phase`.
pub const GATEWAY_RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(312);

pub struct GatewayHttpBinding {
    listener: tokio::net::TcpListener,
    port: u16,
}

impl GatewayHttpBinding {
    pub async fn bind_loopback() -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn http_base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn serve(self, app: Router) -> GatewayHttpServer {
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            axum::serve(self.listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.changed().await;
                })
                .await
        });
        GatewayHttpServer {
            shutdown_tx,
            task: Some(task),
        }
    }
}

pub struct GatewayHttpServer {
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}

#[derive(Debug)]
pub enum GatewayHttpDrainOutcome {
    Completed(std::io::Result<()>),
    TimedOut,
    JoinFailed(String),
}

impl GatewayHttpServer {
    /// Wait for an unexpected server exit. Cancellation-safe: when used in a
    /// `select!` and another branch wins, the owned task remains available to
    /// the ordered shutdown path.
    pub async fn wait(&mut self) -> GatewayHttpDrainOutcome {
        let Some(task) = self.task.as_mut() else {
            return GatewayHttpDrainOutcome::Completed(Ok(()));
        };
        let outcome = match task.await {
            Ok(result) => GatewayHttpDrainOutcome::Completed(result),
            Err(error) => GatewayHttpDrainOutcome::JoinFailed(error.to_string()),
        };
        self.task = None;
        outcome
    }
}

pub struct GatewayShutdownOutcome<Cleanup> {
    pub http: GatewayHttpDrainOutcome,
    pub runtime: Option<UnifiedRuntimeShutdownReport>,
    pub cleanup: Cleanup,
}

async fn ordered_shutdown_tail<
    BeforeRuntime,
    BeforeFuture,
    RuntimeFuture,
    RuntimeOutput,
    Cleanup,
    CleanupFuture,
    CleanupOutput,
>(
    before_runtime: BeforeRuntime,
    runtime_shutdown: RuntimeFuture,
    cleanup: Cleanup,
) -> (RuntimeOutput, CleanupOutput)
where
    BeforeRuntime: FnOnce() -> BeforeFuture,
    BeforeFuture: std::future::Future<Output = ()>,
    RuntimeFuture: std::future::Future<Output = RuntimeOutput>,
    Cleanup: FnOnce() -> CleanupFuture,
    CleanupFuture: std::future::Future<Output = CleanupOutput>,
{
    before_runtime().await;
    let runtime = runtime_shutdown.await;
    let cleanup = cleanup().await;
    (runtime, cleanup)
}

impl GatewayComposition<ActiveGateway> {
    /// One authoritative shutdown order for both gateway profiles:
    ///
    /// 1. stop HTTP admission and bound its outer-handler drain;
    /// 2. run the profile's pre-runtime hook (for example event-drain task
    ///    cancellation);
    /// 3. cooperatively await the runtime's authority cleanup within the
    ///    established SDK horizon;
    /// 4. only then run profile cleanup (for example registry removal).
    pub async fn shutdown<BeforeRuntime, BeforeFuture, Cleanup, CleanupFuture, CleanupOutput>(
        &self,
        mut server: GatewayHttpServer,
        before_runtime: BeforeRuntime,
        cleanup: Cleanup,
    ) -> GatewayShutdownOutcome<CleanupOutput>
    where
        BeforeRuntime: FnOnce() -> BeforeFuture,
        BeforeFuture: std::future::Future<Output = ()>,
        Cleanup: FnOnce() -> CleanupFuture,
        CleanupFuture: std::future::Future<Output = CleanupOutput>,
    {
        let _ = server.shutdown_tx.send(true);
        let http = if let Some(mut task) = server.task.take() {
            match tokio::time::timeout(GATEWAY_HTTP_DRAIN_TIMEOUT, &mut task).await {
                Ok(Ok(result)) => GatewayHttpDrainOutcome::Completed(result),
                Ok(Err(error)) => GatewayHttpDrainOutcome::JoinFailed(error.to_string()),
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    GatewayHttpDrainOutcome::TimedOut
                }
            }
        } else {
            GatewayHttpDrainOutcome::Completed(Ok(()))
        };

        let runtime_shutdown = async {
            match tokio::time::timeout(
                GATEWAY_RUNTIME_SHUTDOWN_TIMEOUT,
                self.state.runtime.shutdown(),
            )
            .await
            {
                Ok(report) => {
                    if !report.cleanup_completed() {
                        tracing::warn!(
                            drain_timed_out = report.drain.timed_out,
                            mob_stop = ?report.mob_stop,
                            identity_authority_release = ?report.identity_authority_release,
                            orphan_processes = report.module_shutdown.orphan_processes,
                            "gateway runtime shutdown completed without cleanup attestation"
                        );
                    }
                    Some(report)
                }
                Err(_) => {
                    tracing::warn!(
                        timeout_ms = GATEWAY_RUNTIME_SHUTDOWN_TIMEOUT.as_millis(),
                        "gateway runtime shutdown exceeded its bounded horizon"
                    );
                    None
                }
            }
        };
        let (runtime, cleanup) =
            ordered_shutdown_tail(before_runtime, runtime_shutdown, cleanup).await;
        GatewayShutdownOutcome {
            http,
            runtime,
            cleanup,
        }
    }
}

// ---------------------------------------------------------------------------
// Boot preamble
// ---------------------------------------------------------------------------

/// Worker stack size for gateway tokio runtimes.
///
/// Meerkat's generated machine-authority apply path needs deep worker stacks;
/// this mirrors meerkat-rpc's explicit 16 MiB tokio worker sizing. Both
/// gateway binaries had this constant inline and identical.
const GATEWAY_WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

/// Build the multi-threaded tokio runtime both gateway binaries run on.
///
/// The error is returned rather than handled: the two binaries report a
/// build failure differently on purpose - `mobkit_gateway` emits a JSON-RPC
/// init error on stdout (its parent is an SDK reading the handshake),
/// `rpc_gateway` writes to stderr. Only the builder itself is shared.
pub fn gateway_tokio_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(GATEWAY_WORKER_STACK_BYTES)
        .build()
}

/// Default tracing filter for a gateway binary when `RUST_LOG` is unset:
/// this crate's own targets at INFO, the binary's own target at INFO,
/// dependencies at WARN.
///
/// The binary target is a parameter and NOT a shared constant. Collapsing
/// the two filters into one string would silence whichever binary lost its
/// target, and the operationally significant boot phases (the one-time
/// head-canonical conversion, continuity repair, storage-verb migration
/// progress) report at INFO. A 2026-07 production deploy was aborted because
/// a supervisor read a silent-but-working migration as a hang.
pub fn default_tracing_filter(binary_target: &str) -> String {
    format!("warn,meerkat_mobkit=info,{binary_target}=info")
}

/// Install the gateway tracing subscriber on stderr.
///
/// Stderr, never stdout: stdout carries the init JSON handshake and the
/// storage verbs' report output. `RUST_LOG` overrides the default filter.
pub fn init_gateway_tracing(binary_target: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(default_tracing_filter(binary_target))
            }),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
}

/// Extract and validate the optional `--control-listen <addr>` flag.
///
/// `args` is argv WITHOUT the program name at both call sites, arrived at
/// two different ways: `mobkit_gateway` collects `std::env::args().skip(1)`,
/// `rpc_gateway` collects the whole argv and passes `&args[1..]`. The scan is
/// by exact flag match, so the distinction would not matter anyway, but it is
/// stated because it did not hold when this doc was first written.
pub fn parse_control_listen_arg(args: &[String]) -> Result<Option<ControlListenAddr>, String> {
    let Some(position) = args.iter().position(|arg| arg == "--control-listen") else {
        return Ok(None);
    };
    let Some(value) = args.get(position + 1) else {
        return Err(
            "--control-listen requires an address (tcp://host:port or uds:///path)".to_string(),
        );
    };
    ControlListenAddr::parse(value)
        .map(Some)
        .map_err(|error| format!("--control-listen: {error}"))
}

// ---------------------------------------------------------------------------
// Host configuration ingress
// ---------------------------------------------------------------------------

/// A top-level `mob.toml` table that belongs to meerkat's HOST config
/// (`config.toml`), not to the mob definition.
///
/// `MobDefinition::from_toml` deserializes without `deny_unknown_fields`, so a
/// `[self_hosted]` or `[realm]` table written into `mob.toml` was dropped in
/// silence, and the first self-hosted member then died at build time with
/// "self-hosted model '..' is not registered in config", two layers away from
/// the file that caused it. Both gateways refuse the table at init instead
/// and name the option that carries the host config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostConfigTableInMobToml {
    /// `[self_hosted]`: serving endpoints and model aliases.
    SelfHosted,
    /// `[realm]`: backend, auth and binding profiles.
    Realm,
}

impl HostConfigTableInMobToml {
    /// Every table this check refuses, in the order it reports them.
    pub const ALL: [Self; 2] = [Self::SelfHosted, Self::Realm];

    /// The table name as written in TOML.
    pub fn table(self) -> &'static str {
        match self {
            Self::SelfHosted => "self_hosted",
            Self::Realm => "realm",
        }
    }
}

impl std::fmt::Display for HostConfigTableInMobToml {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mob.toml declares a top-level [{table}] table, which is meerkat host configuration \
             that the mob definition parser ignores; move it into the meerkat config.toml and \
             point the gateway at that file: runtime_options.meerkat_config_path on rpc_gateway, \
             or the meerkat_config_path init param (default <workspace>/.rkat/config.toml) on \
             mobkit_gateway",
            table = self.table()
        )
    }
}

impl std::error::Error for HostConfigTableInMobToml {}

/// Refuse `[self_hosted]` and `[realm]` at the top level of a `mob.toml`
/// document.
///
/// Runs on the RAW text, because by the time a typed `MobDefinition` exists
/// the tables are already gone. Text that is not valid TOML is accepted here:
/// `MobDefinition::from_toml` owns that verdict and reports it with its own
/// message, and both gateways run it first.
pub fn refuse_host_config_tables_in_mob_toml(
    mob_toml: &str,
) -> Result<(), HostConfigTableInMobToml> {
    let Ok(toml::Value::Table(document)) = toml::from_str::<toml::Value>(mob_toml) else {
        return Ok(());
    };
    match HostConfigTableInMobToml::ALL
        .into_iter()
        .find(|table| document.contains_key(table.table()))
    {
        Some(table) => Err(table),
        None => Ok(()),
    }
}

/// Why a meerkat host `config.toml` could not become a gateway's agent
/// config. Both binaries refuse init on it: an operator who pointed the
/// gateway at a file expects that file in force, and booting from
/// `Config::default()` instead would resurface as an unexplained
/// "self-hosted model is not registered in config" at the first member build.
#[derive(Debug)]
pub enum GatewayHostConfigError {
    /// The file could not be read.
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The file is not a valid meerkat config document.
    Parse {
        path: PathBuf,
        source: meerkat::ConfigError,
    },
}

impl std::fmt::Display for GatewayHostConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    f,
                    "failed to read meerkat config {}: {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(f, "meerkat config {} is invalid: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for GatewayHostConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

/// Load a meerkat host `config.toml` as the base config for every agent a
/// gateway builds.
///
/// One explicit file, merged over `Config::default()` with meerkat's own
/// file-merge semantics (`Config::merge_toml_str`, the step `Config::load`
/// applies to a discovered `.rkat/config.toml`). No directory walk and no
/// home-directory fallback: the gateway is handed a path, so what it loads is
/// exactly what the operator named. This is what carries `[self_hosted]`,
/// `[realm]` and `[models]` to `FactoryAgentBuilder`; the caller layers the
/// gateway's own overlays (comms transport, compaction policy) on top.
pub fn load_gateway_host_config(path: &Path) -> Result<meerkat::Config, GatewayHostConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| GatewayHostConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut config = meerkat::Config::default();
    config
        .merge_toml_str(&text)
        .map_err(|source| GatewayHostConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(config)
}

// ---------------------------------------------------------------------------
// Schedule firing host
// ---------------------------------------------------------------------------

/// Everything [`spawn_gateway_schedule_host`] consumes.
///
/// A struct rather than a long argument list because the two binaries built
/// their tuples in DIFFERENT field orders: `rpc_gateway` puts the
/// `MeerkatMachine` adapter fourth, right after the session service, while
/// `mobkit_gateway`'s `ScheduleHostInputs` alias has no adapter at all and a
/// later `.map` APPENDS it last. That is exactly the kind of divergence that
/// silently swaps two same-typed arguments.
pub struct GatewayScheduleHostInputs<B: SessionAgentBuilder + 'static> {
    pub schedule_service: ScheduleService,
    /// The CONCRETE persistent service: the runtime-backed firing host needs
    /// the typed form, not the erased `dyn MobSessionService` the bootstrap
    /// spec consumes.
    pub session_service: Arc<PersistentSessionService<B>>,
    pub runtime_adapter: Arc<MeerkatMachine>,
    pub schedule_store_path: PathBuf,
    pub firing_host_binding: ScheduleFiringHostBinding,
    /// Host-runnable targets. `rpc_gateway` composes the steward dream plus
    /// SDK-declared runnables here; `mobkit_gateway` has none.
    pub runnable_host: Option<Arc<dyn ScheduleRunnableHost>>,
    pub workgraph_service: Option<WorkGraphService>,
    pub owner_id: String,
}

/// Point the mob-target registry at the live mob state and repair persisted
/// resumable-session schedules to identity mob targets.
///
/// Split out from [`spawn_gateway_schedule_host`] deliberately:
/// `rpc_gateway` performs steward-dream registration BETWEEN this step and
/// the host spawn, and folding both into one call would have reordered a
/// durable-store boot sequence to no benefit. Returns the mob state so the
/// caller can hand it to the host spawn.
pub async fn adopt_schedule_mob_targets(
    runtime: &UnifiedRuntime,
    schedule_service: &ScheduleService,
    mob_target_registry: &ScheduleMobTargetRegistry,
) -> Option<Arc<MobMcpState>> {
    let mob_state = runtime.mob_runtime().agent_mob_mcp_state();
    mob_target_registry.set_mob_state(mob_state.clone());
    match crate::schedule_wiring::repair_resumable_session_targets_to_mob_members(
        schedule_service,
        mob_target_registry,
    )
    .await
    {
        Ok(repaired) if repaired > 0 => {
            tracing::info!(
                repaired,
                "repaired persisted resumable-session schedules to identity mob targets"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "failed to repair persisted resumable-session schedules to identity mob targets",
            );
        }
    }
    mob_state
}

/// Spawn the resident claim watchdog and the runtime-backed schedule firing
/// host, bind the firing-intent write gate if the host came up, and report
/// one observed verdict about the pipeline.
///
/// Both gateways ran byte-identical versions of this block, including every
/// log string. The only behavioural difference was the runnable host, which
/// is now the named [`GatewayScheduleHostInputs::runnable_host`] field.
///
/// Ordering is load-bearing and preserved exactly: the watchdog contract is
/// logged BEFORE the watchdog is spawned, `firing_host_binding.bind()`
/// happens only AFTER the host is confirmed up (binding earlier would admit
/// firing-intent writes into a store nothing local drains), and the probe
/// runs only on the bound path.
pub async fn spawn_gateway_schedule_host<B: SessionAgentBuilder + 'static>(
    runtime: &UnifiedRuntime,
    mob_state: Option<Arc<MobMcpState>>,
    inputs: GatewayScheduleHostInputs<B>,
) -> (Option<ScheduleHostHandle>, tokio::task::JoinHandle<()>) {
    let GatewayScheduleHostInputs {
        schedule_service,
        session_service,
        runtime_adapter,
        schedule_store_path,
        firing_host_binding,
        runnable_host,
        workgraph_service,
        owner_id,
    } = inputs;
    // The firing driver discards its own tick errors upstream, so the claim
    // watchdog is the only thing that turns "everything stays pending
    // forever" into a row-level diagnosis.
    //
    // Stated as a RESIDENT LIVENESS CONTRACT rather than a promise about a
    // future log line: the handle returned below is held for the process
    // lifetime, and these are the numbers it holds the pipeline to. A reader
    // who greps the boot log now learns the cadence and the overdue
    // threshold instead of learning that a watchdog exists.
    let watchdog_config = ScheduleClaimWatchdogConfig::default();
    tracing::info!(
        poll_interval_secs = watchdog_config.poll_interval.as_secs(),
        overdue_threshold_secs = watchdog_config.overdue_threshold.as_secs(),
        heartbeat_polls = watchdog_config.heartbeat_polls,
        "schedule claim watchdog resident: probes the firing pipeline on this cadence, ERROR on \
         a new or changed stall report, WARN heartbeat while it persists, INFO on recovery"
    );
    let watchdog = crate::schedule_wiring::spawn_schedule_claim_watchdog(
        schedule_service.clone(),
        schedule_store_path.clone(),
        watchdog_config,
    );
    let schedule_service_for_probe = schedule_service.clone();
    let schedule_host = crate::schedule_wiring::spawn_schedule_host_with_identity_runtime(
        session_service,
        runtime_adapter,
        schedule_service,
        mob_state,
        runtime.mob_handle(),
        runtime.identity_runtime().cloned(),
        runnable_host,
        workgraph_service,
        owner_id,
    );
    if schedule_host.is_some() {
        // The firing host now drains the store: firing-intent schedule writes
        // (create/update/resume) are admissible from here on (Bug C stopgap -
        // the gate refused them until this point).
        firing_host_binding.bind();
        // One probe NOW, with the host up, so the boot log carries an
        // observed verdict about the firing pipeline instead of a claim about
        // a watchdog that has not ticked yet. Deliberately WARN, not ERROR,
        // for a stall found here: a gateway that was down across a due time
        // legitimately restarts with a backlog the host has not drained yet.
        // The resident watchdog above is what escalates - if the same report
        // is still there on its cadence, it is a real stall and it says so at
        // ERROR.
        match crate::schedule_wiring::probe_schedule_firing_pipeline(
            &schedule_service_for_probe,
            &schedule_store_path,
            watchdog_config.overdue_threshold,
        )
        .await
        {
            crate::schedule_wiring::ScheduleFiringProbe::Healthy => {
                tracing::info!("schedule firing pipeline healthy at boot");
            }
            crate::schedule_wiring::ScheduleFiringProbe::Stalled { report } => {
                tracing::warn!(
                    %report,
                    poll_interval_secs = watchdog_config.poll_interval.as_secs(),
                    "schedule firing pipeline is not delivering at boot; the resident claim \
                     watchdog re-probes on its cadence and escalates to ERROR if this persists \
                     (a restart backlog clears on the first host ticks)"
                );
            }
        }
    } else {
        // Present-state verdict, not a prediction: the host is NOT running,
        // and the write gate is therefore still closed. Both halves are
        // observed facts at this point in boot.
        tracing::warn!(
            "schedule host did not spawn over the attached schedule store: no firing driver is \
             running in this gateway, and the firing-intent write gate is consequently still \
             closed, so create/update/resume are being refused rather than accepted durably"
        );
        // The old tail of that sentence ("into a store nothing drains")
        // generalized a process-local fact to the whole store. meerkat 0.8.22
        // lets the store answer that question itself: the executor lease is
        // singular per realm store, so the holder (if any) is named rather
        // than guessed at. It is one instantaneous read, not a liveness
        // verdict - a peer mid-restart reads vacant, and a crashed
        // predecessor still reads held until its lease expires, so each arm
        // below says what it actually proves. Read only: the observation
        // carries no bearer token and cannot take firing authority away from
        // whoever holds it.
        match crate::schedule_wiring::observe_schedule_firing_authority(&schedule_service_for_probe)
            .await
        {
            // Bound as `holder_id`, not `owner_id`, for readability only -
            // shadowing the moved-from `owner_id` input would compile fine,
            // but this arm reports ANOTHER process's identity, and neither
            // original call site had a name that could be confused with it
            // in scope here (they used `schedule_owner_id` / `runtime_id`).
            // The log FIELD name is unchanged, so the envelope is identical.
            crate::schedule_wiring::ScheduleFiringAuthority::Held {
                owner_id: holder_id,
                fencing_token,
                expires_in_secs,
            } => tracing::warn!(
                executor_owner_id = %holder_id,
                fencing_token,
                lease_expires_in_secs = expires_in_secs,
                "the schedule store itself reports SOME process holding the realm's singular \
                 firing authority, so durable schedules may still be drained elsewhere; note the \
                 holder can also be this deployment's own crashed predecessor, whose lease stays \
                 live until it expires, so check the owner id and expiry above before concluding \
                 anything is actually draining. This gateway's firing-intent write gate stays \
                 closed either way: it gates on a LOCAL host"
            ),
            crate::schedule_wiring::ScheduleFiringAuthority::Vacant => tracing::warn!(
                "the schedule store itself reports its firing authority vacant AT THIS INSTANT: \
                 no process holds the executor lease. A peer gateway mid-restart is vacant only \
                 until its first tick, so this is proof of an unattended store only if it \
                 persists - the resident claim watchdog is what escalates once durable work \
                 actually goes unclaimed"
            ),
            crate::schedule_wiring::ScheduleFiringAuthority::Unobservable { detail } => {
                tracing::warn!(
                    %detail,
                    "the schedule store cannot report firing authority, so whether any other \
                     process drains this store is unknown from here"
                );
            }
        }
    }
    (schedule_host, watchdog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_profiles_pin_the_two_existing_command_surfaces() {
        assert_eq!(
            GatewayCompatibilityProfile::ConsoleHttp.contract(),
            GatewayCompatibilityContract {
                top_level_permissive_init: true,
                registry_resume: true,
                post_init_stdin_rpc: false,
                callback_plane: false,
                single_shot: false,
                maintenance_verbs: true,
            }
        );
        assert_eq!(
            GatewayCompatibilityProfile::StdioRpc.contract(),
            GatewayCompatibilityContract {
                top_level_permissive_init: false,
                registry_resume: false,
                post_init_stdin_rpc: true,
                callback_plane: true,
                single_shot: true,
                maintenance_verbs: false,
            }
        );
    }

    const HOST_CONFIG_FIXTURE: &str = r#"
[self_hosted]
default_model = "gemma-4-31b"

[self_hosted.servers.local]
transport = "openai_compatible"
base_url = "http://127.0.0.1:11434"
api_style = "chat_completions"

[self_hosted.models.gemma-4-31b]
server = "local"
remote_model = "gemma4:31b"

[models.house-model]
provider = "openai"

[realm.global]
default_binding = "local"
"#;

    /// The host config's `[self_hosted]`, `[models]` and `[realm]` tables must
    /// arrive in the loaded `Config`: nothing else in either gateway can
    /// supply them (`TomlDefinition` drops the first and last, and the
    /// registry refuses `[models.<id>] provider = "self_hosted"`).
    #[test]
    fn host_config_carries_self_hosted_models_and_realm_tables()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.toml");
        std::fs::write(&path, HOST_CONFIG_FIXTURE)?;

        let config = load_gateway_host_config(&path)?;

        assert_eq!(
            config
                .self_hosted
                .models
                .get("gemma-4-31b")
                .map(|model| model.server.as_str()),
            Some("local")
        );
        assert!(config.self_hosted.servers.contains_key("local"));
        assert_eq!(
            config.self_hosted.default_model.as_deref(),
            Some("gemma-4-31b")
        );
        assert_eq!(
            config
                .models
                .custom
                .get("house-model")
                .map(|model| model.provider == meerkat_core::Provider::OpenAI),
            Some(true)
        );
        assert!(config.realm.contains_key("global"));
        Ok(())
    }

    /// A named file that cannot be read or parsed is a typed refusal that
    /// names the path, never a silent `Config::default()`.
    #[test]
    fn host_config_refuses_missing_and_malformed_files_by_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let missing = dir.path().join("absent.toml");
        let error = load_gateway_host_config(&missing).err();
        assert!(
            matches!(&error, Some(GatewayHostConfigError::Read { path, .. }) if path == &missing),
            "{error:?}"
        );
        assert!(
            error
                .map(|error| error.to_string())
                .unwrap_or_default()
                .contains("absent.toml")
        );

        let malformed = dir.path().join("broken.toml");
        std::fs::write(&malformed, "[self_hosted\n")?;
        let error = load_gateway_host_config(&malformed).err();
        assert!(
            matches!(&error, Some(GatewayHostConfigError::Parse { path, .. }) if path == &malformed),
            "{error:?}"
        );
        Ok(())
    }

    /// `TomlDefinition` has no `deny_unknown_fields`, so the two host-config
    /// tables vanish from a parsed definition without a diagnostic. The raw
    /// check must name the table and the option that carries the host config,
    /// and must leave every table that DOES belong in mob.toml alone.
    #[test]
    fn mob_toml_host_config_tables_are_refused_by_name() {
        let self_hosted = "[mob]\nid = \"m\"\n\n[self_hosted.servers.local]\n\
                           base_url = \"http://127.0.0.1:11434\"\n";
        assert_eq!(
            refuse_host_config_tables_in_mob_toml(self_hosted),
            Err(HostConfigTableInMobToml::SelfHosted)
        );
        let realm = "[mob]\nid = \"m\"\n\n[realm.global]\ndefault_binding = \"local\"\n";
        assert_eq!(
            refuse_host_config_tables_in_mob_toml(realm),
            Err(HostConfigTableInMobToml::Realm)
        );
        for table in HostConfigTableInMobToml::ALL {
            let message = table.to_string();
            assert!(
                message.contains(&format!("[{}]", table.table())),
                "{message}"
            );
            assert!(message.contains("meerkat_config_path"), "{message}");
        }
        let ordinary = "[mob]\nid = \"m\"\n\n[profiles.w]\nmodel = \"house-model\"\n\n\
                        [models.house-model]\nprovider = \"openai\"\n";
        assert_eq!(refuse_host_config_tables_in_mob_toml(ordinary), Ok(()));
        // Invalid TOML is the definition parser's verdict, not this check's.
        assert_eq!(refuse_host_config_tables_in_mob_toml("[mob\n"), Ok(()));
    }

    #[tokio::test]
    async fn shutdown_tail_orders_profile_quiesce_runtime_cleanup_then_registry_cleanup() {
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let before_observed = Arc::clone(&observed);
        let runtime_observed = Arc::clone(&observed);
        let cleanup_observed = Arc::clone(&observed);

        let (runtime, cleanup) = ordered_shutdown_tail(
            move || async move {
                before_observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push("profile_quiesce");
            },
            async move {
                runtime_observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push("runtime_shutdown");
                "runtime_report"
            },
            move || async move {
                cleanup_observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push("registry_cleanup");
                "cleanup_report"
            },
        )
        .await;

        assert_eq!(runtime, "runtime_report");
        assert_eq!(cleanup, "cleanup_report");
        assert_eq!(
            *observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            ["profile_quiesce", "runtime_shutdown", "registry_cleanup"]
        );
    }

    /// Golden envelope for the shared filter builder: the exact strings the
    /// two binaries carried as their own constants before this module
    /// existed. This is a byte-level pin, not a behavioural one - each
    /// binary keeps its own `EnvFilter`-level test proving its target
    /// actually passes at INFO while dependencies stay at WARN.
    #[test]
    fn default_tracing_filter_reproduces_both_binary_constants() {
        assert_eq!(
            default_tracing_filter("rpc_gateway"),
            "warn,meerkat_mobkit=info,rpc_gateway=info"
        );
        assert_eq!(
            default_tracing_filter("mobkit_gateway"),
            "warn,meerkat_mobkit=info,mobkit_gateway=info"
        );
    }

    /// The filter must keep the crate's own target at INFO for every binary:
    /// dropping `meerkat_mobkit=info` is the regression that made a working
    /// migration look like a hang, and it would not be visible in a test
    /// that only checked the binary's own target.
    #[test]
    fn default_tracing_filter_always_keeps_the_crate_at_info() {
        for target in ["rpc_gateway", "mobkit_gateway", "some_future_gateway"] {
            let filter = default_tracing_filter(target);
            assert!(
                filter.contains("meerkat_mobkit=info"),
                "filter for {target} dropped the crate's own INFO target: {filter}"
            );
            assert!(
                filter.starts_with("warn,"),
                "filter for {target} lost the dependency WARN default: {filter}"
            );
        }
    }

    #[test]
    fn control_listen_arg_absent_is_none() -> Result<(), String> {
        let args = vec!["--persistent".to_string()];
        assert!(parse_control_listen_arg(&args)?.is_none());
        Ok(())
    }

    #[test]
    fn control_listen_arg_parses_tcp() -> Result<(), String> {
        let args = vec![
            "--persistent".to_string(),
            "--control-listen".to_string(),
            "tcp://127.0.0.1:0".to_string(),
        ];
        assert!(parse_control_listen_arg(&args)?.is_some());
        Ok(())
    }

    /// A trailing `--control-listen` is a launch error, not a silently
    /// ignored flag: both binaries exit 2 on this.
    #[test]
    fn control_listen_arg_without_value_is_an_error() {
        let args = vec!["--control-listen".to_string()];
        let error = parse_control_listen_arg(&args).err().unwrap_or_default();
        assert!(
            error.contains("requires an address"),
            "unexpected error text: {error}"
        );
    }

    /// `inproc` has no listener; the refusal must name the flag so the
    /// operator can tell which argument was rejected.
    #[test]
    fn control_listen_arg_rejects_inproc_and_names_the_flag() {
        let args = vec!["--control-listen".to_string(), "inproc".to_string()];
        let error = parse_control_listen_arg(&args).err().unwrap_or_default();
        assert!(
            error.starts_with("--control-listen: "),
            "refusal must name the flag: {error}"
        );
    }
}
