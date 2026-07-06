//! Identity-first runtime: delivery, status, lifecycle, and ownership enforcement.
//!
//! This module implements the behavioral core of identity-first continuity:
//! - Delivery: `send()` and `dispatch()` with addressability and lease enforcement
//! - Status: `status()` returning `IdentityStatus`
//! - Lifecycle: `retire()`, `respawn()`, `reset()`, `delete_identity()`
//! - Ownership: lease tracking, fencing, and invariant enforcement

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock as StdRwLock};
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};
use meerkat_core::types::{HandlingMode, SessionId};
use tokio::sync::{Mutex, Notify, RwLock, broadcast};
use tokio::task::JoinHandle;

use super::agent_memory::{
    AgentMemoryError, AgentMemoryForgetResult, AgentMemoryRecallRequest, AgentMemoryRecord,
    AgentMemoryRuntimeInjector, NewAgentMemory,
};
use super::bridge::SessionBridge;
use super::contracts::{
    AgentCustomizer, ContinuityStore, LeaseProvider, RosterProvider, TopologyProvider,
};
use super::types::{
    AgentAddressability, AgentBuildContext, AgentIdentity, AgentRuntimeId, AgentRuntimeServices,
    CheckpointVersion, ContinuityGeneration, ContinuityHealth, ContinuityRecord,
    ContinuityStoreError, DispatchInput, DurabilityPolicy, DurableAgentSpec, FencingToken,
    IdentityLifecycleState, IdentityStatus, LeaseGrant, LeaseInfo, ManagedPeerEdge, NotAddressable,
    RosterContext, SessionSnapshot,
};
use crate::memory::records::{
    ManifestTier, MemoryId, MemoryKind, MemoryScope, NewMemoryRecord, RecordMeta, UsageEvent,
};

const MANAGED_PEER_RECONCILE_CONCURRENCY: usize = 64;
const MATERIALIZATION_FAILURE_BACKOFF: Duration = Duration::from_secs(30);
fn durable_spec_uses_external_binding(spec: &DurableAgentSpec) -> bool {
    matches!(spec.backend, Some(meerkat_mob::MobBackendKind::External))
        || matches!(
            spec.binding.as_ref(),
            Some(meerkat_contracts::WireRuntimeBinding::External { .. })
        )
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors from identity-first runtime operations.
#[derive(Debug)]
pub enum IdentityRuntimeError {
    /// Target identity is not registered/active.
    UnknownIdentity(AgentIdentity),
    /// send() rejected: target is InternalOnly.
    NotAddressable(NotAddressable),
    /// Operation rejected: no active lease for this identity.
    NoActiveLease(AgentIdentity),
    /// Fail-closed single-embodiment guard: the identity's durable lease is
    /// held by another live runtime instance — a second live embodiment is
    /// refused, loudly and with the holder named. This is a POLICY point,
    /// not a structural assumption: multi-bind / forked-session semantics
    /// (future work) relax exactly this arm.
    AlreadyEmbodied {
        identity: AgentIdentity,
        holder: String,
    },
    /// Operation rejected: lease was lost.
    LeaseLost(AgentIdentity),
    /// Operation rejected: identity is not in a state that permits this operation.
    InvalidState {
        identity: AgentIdentity,
        state: IdentityLifecycleState,
        operation: &'static str,
    },
    /// Continuity store error.
    Store(ContinuityStoreError),
    /// Lease provider error.
    Lease(super::types::LeaseError),
    /// Duplicate identities in roster.
    DuplicateIdentity(AgentIdentity),
    /// Stale fencing token on checkpoint.
    StaleFencingToken {
        identity: AgentIdentity,
        presented: FencingToken,
        current: FencingToken,
    },
    /// Stale checkpoint version.
    StaleCheckpointVersion {
        identity: AgentIdentity,
        presented: CheckpointVersion,
        current: CheckpointVersion,
    },
    /// Generic I/O or internal error.
    Internal(String),
}

impl std::fmt::Display for IdentityRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownIdentity(id) => write!(f, "unknown identity: {id}"),
            Self::NotAddressable(err) => write!(f, "{err}"),
            Self::NoActiveLease(id) => write!(f, "no active lease for {id}"),
            Self::AlreadyEmbodied { identity, holder } => write!(
                f,
                "identity {identity} is already embodied by runtime instance '{holder}' \
                 (single-embodiment guard: refusing a second live bind)"
            ),
            Self::LeaseLost(id) => write!(f, "lease lost for {id}"),
            Self::InvalidState {
                identity,
                state,
                operation,
            } => write!(
                f,
                "cannot {operation} identity {identity} in state {state:?}"
            ),
            Self::Store(err) => write!(f, "continuity store: {err}"),
            Self::Lease(err) => write!(f, "lease provider: {err}"),
            Self::DuplicateIdentity(id) => write!(f, "duplicate identity in roster: {id}"),
            Self::StaleFencingToken {
                identity,
                presented,
                current,
            } => write!(
                f,
                "stale fencing token for {identity}: presented {presented}, current {current}"
            ),
            Self::StaleCheckpointVersion {
                identity,
                presented,
                current,
            } => write!(
                f,
                "stale checkpoint version for {identity}: presented {presented}, current {current}"
            ),
            Self::Internal(msg) => write!(f, "internal: {msg}"),
        }
    }
}

impl std::error::Error for IdentityRuntimeError {}

impl From<ContinuityStoreError> for IdentityRuntimeError {
    fn from(err: ContinuityStoreError) -> Self {
        match err {
            ContinuityStoreError::StaleFencingToken {
                identity,
                presented,
                current,
            } => Self::StaleFencingToken {
                identity,
                presented,
                current,
            },
            ContinuityStoreError::StaleCheckpointVersion {
                identity,
                presented,
                current,
            } => Self::StaleCheckpointVersion {
                identity,
                presented,
                current,
            },
            other => Self::Store(other),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-identity runtime state
// ---------------------------------------------------------------------------

/// Tracks the live state for a single identity within the runtime.
#[derive(Debug, Clone)]
pub(crate) struct IdentityEntry {
    pub spec: DurableAgentSpec,
    pub state: IdentityLifecycleState,
    pub continuity: Option<ContinuityRecord>,
    pub lease: Option<LeaseEntry>,
    pub checkpoint_version: CheckpointVersion,
    /// Whether a durable runtime_store is available (affects dispatch ack semantics).
    pub has_runtime_store: bool,
}

/// Tracks a held lease for an identity.
#[derive(Debug, Clone)]
pub(crate) struct LeaseEntry {
    pub fencing_token: FencingToken,
    pub ttl: Duration,
    pub acquired_at: Instant,
}

impl LeaseEntry {
    pub fn is_expired(&self) -> bool {
        self.acquired_at.elapsed() > self.ttl
    }

    pub fn ttl_remaining(&self) -> Duration {
        self.ttl.saturating_sub(self.acquired_at.elapsed())
    }

    pub fn is_healthy(&self) -> bool {
        // Healthy if more than 20% TTL remains
        let remaining = self.ttl_remaining();
        remaining > self.ttl / 5
    }
}

// ---------------------------------------------------------------------------
// Identity-scoped events
// ---------------------------------------------------------------------------

/// Events emitted for a specific identity, used by `subscribe()`.
#[derive(Debug, Clone)]
pub enum IdentityEvent {
    /// Lifecycle state changed.
    StateChanged {
        identity: AgentIdentity,
        new_state: IdentityLifecycleState,
    },
    /// Lease acquired or renewed.
    LeaseUpdated {
        identity: AgentIdentity,
        fencing_token: FencingToken,
    },
    /// Lease lost.
    LeaseLost { identity: AgentIdentity },
    /// Checkpoint completed.
    CheckpointCompleted {
        identity: AgentIdentity,
        version: CheckpointVersion,
    },
    /// Resume could not reuse a persisted runtime binding and materialization
    /// fresh-spawned a member instead.
    ResumeFallback {
        identity: AgentIdentity,
        reason: super::bridge::ResumeFallbackReason,
    },
}

/// Per-identity event channel capacity.
const IDENTITY_EVENT_CHANNEL_CAPACITY: usize = 64;
const DEFAULT_LEASE_RENEWAL_MAX_POLL_INTERVAL: Duration = Duration::from_mins(1);
const DEFAULT_LEASE_RENEWAL_MIN_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Base delay for the lease-renewal failure backoff. The renewal tick's
/// normal cadence is TTL-derived (down to a 10ms floor), so a lease provider
/// that errors persistently would otherwise retry — and warn — at that floor
/// rate. On failure we back off from this base, doubling toward the max poll
/// interval, so a backend outage can't spin the renewal task.
const LEASE_RENEWAL_FAILURE_BACKOFF_BASE: Duration = Duration::from_secs(1);

fn lease_renewal_failure_backoff(
    consecutive_failures: u32,
    max_poll_interval: Duration,
) -> Duration {
    LEASE_RENEWAL_FAILURE_BACKOFF_BASE
        .saturating_mul(1u32 << consecutive_failures.min(6))
        .min(max_poll_interval)
}

// ---------------------------------------------------------------------------
// IdentityRuntime
// ---------------------------------------------------------------------------

/// Configuration for the identity-first runtime.
pub struct IdentityRuntimeConfig {
    pub continuity_store: Arc<dyn ContinuityStore>,
    pub lease_provider: Arc<dyn LeaseProvider>,
    pub runtime_instance_id: String,
    pub has_runtime_store: bool,
    pub durability_policy: DurabilityPolicy,
    /// Optional session bridge for real session delivery. When `None`,
    /// delivery operations validate invariants but do not forward to
    /// the Meerkat session pipeline (useful for tests).
    pub bridge: Option<Arc<dyn SessionBridge>>,
    /// Default timeout for wait_for_output / wait_for_output_containing.
    /// Defaults to 90 seconds if not set.
    pub default_timeout: Option<Duration>,
}

#[derive(Clone)]
pub struct IdentityFirstRuntimeContext {
    pub runtime: Arc<IdentityRuntime>,
    pub roster_provider: Arc<dyn RosterProvider>,
    pub topology_provider: Option<Arc<dyn TopologyProvider>>,
    pub customizer: Option<Arc<dyn AgentCustomizer>>,
    mob_definition: Option<meerkat_mob::MobDefinition>,
    lazy_materialization: bool,
}

impl IdentityFirstRuntimeContext {
    pub fn new(
        runtime: Arc<IdentityRuntime>,
        roster_provider: Arc<dyn RosterProvider>,
        topology_provider: Option<Arc<dyn TopologyProvider>>,
        customizer: Option<Arc<dyn AgentCustomizer>>,
        mob_definition: Option<meerkat_mob::MobDefinition>,
    ) -> Self {
        Self::new_with_lazy_materialization(
            runtime,
            roster_provider,
            topology_provider,
            customizer,
            mob_definition,
            false,
        )
    }

    pub fn new_with_lazy_materialization(
        runtime: Arc<IdentityRuntime>,
        roster_provider: Arc<dyn RosterProvider>,
        topology_provider: Option<Arc<dyn TopologyProvider>>,
        customizer: Option<Arc<dyn AgentCustomizer>>,
        mob_definition: Option<meerkat_mob::MobDefinition>,
        lazy_materialization: bool,
    ) -> Self {
        runtime.set_reset_roster_provider_context(
            Some(roster_provider.clone()),
            mob_definition.clone(),
        );
        Self {
            runtime,
            roster_provider,
            topology_provider,
            customizer,
            mob_definition,
            lazy_materialization,
        }
    }

    pub async fn refresh_desired_topology(
        &self,
    ) -> Result<super::orchestrator::RestoreFlowResult, IdentityRuntimeError> {
        let roster = self
            .roster_provider
            .roster(&RosterContext {
                mob_definition: self.mob_definition.clone(),
                previous_identities: Vec::new(),
            })
            .await
            .map_err(|err| IdentityRuntimeError::Internal(format!("roster provider: {err}")))?;

        if self.lazy_materialization {
            super::orchestrator::lazy_register_flow(
                &self.runtime,
                &roster,
                self.topology_provider.as_deref(),
            )
            .await
        } else {
            super::orchestrator::restore_flow(
                &self.runtime,
                &roster,
                self.topology_provider.as_deref(),
                self.customizer.as_deref(),
            )
            .await
        }
    }

    /// Background repair for Broken identities. A rejected resume degrades the
    /// identity to Broken while preserving the durable session; the documented
    /// contract is "the next reconcile retries the resume" — but without this
    /// task, nothing runs that reconcile: delivery refuses Broken identities
    /// (REQ-13 fails loudly), `materialize` refuses the Broken state, and the
    /// only retries were a manual `mobkit/reconcile_identity` RPC or a process
    /// restart. HomeCore 0.7.23 sat with 14 preserved-but-parked identities
    /// because of exactly that gap.
    ///
    /// The loop sleeps, and only when at least one identity is Broken re-runs
    /// [`Self::refresh_desired_topology`] — the same idempotent flow the
    /// reconcile RPC runs (eager: `restore_flow` retries the resume; lazy:
    /// `lazy_register_flow` re-registers a store-Ready identity as Dormant so
    /// on-demand materialization retries). Backoff doubles while identities
    /// stay Broken — persistent causes (e.g. an upstream store regression)
    /// produce bounded log noise, and transient causes (disk full, lock
    /// contention) heal without a restart.
    pub fn spawn_broken_identity_repair_task(
        self: Arc<Self>,
        policy: ContinuityRepairPolicy,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut backoff = policy.initial_backoff;
            loop {
                tokio::time::sleep(backoff).await;
                let broken = self.runtime.broken_identities().await;
                if broken.is_empty() {
                    backoff = policy.initial_backoff;
                    continue;
                }
                tracing::info!(
                    broken = broken.len(),
                    "continuity repair: retrying restore for Broken identities"
                );
                if let Err(err) = self.refresh_desired_topology().await {
                    tracing::warn!(
                        error = %err,
                        "continuity repair reconcile failed; backing off"
                    );
                    backoff = (backoff * 2).min(policy.max_backoff);
                    continue;
                }
                let still_broken = self.runtime.broken_identities().await;
                let healed = broken
                    .iter()
                    .filter(|id| !still_broken.contains(id))
                    .count();
                if healed > 0 {
                    tracing::info!(
                        healed,
                        still_broken = still_broken.len(),
                        "continuity repair healed identities"
                    );
                }
                backoff = if still_broken.is_empty() {
                    policy.initial_backoff
                } else {
                    (backoff * 2).min(policy.max_backoff)
                };
            }
        })
    }
}

/// Retry cadence for [`IdentityFirstRuntimeContext::spawn_broken_identity_repair_task`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContinuityRepairPolicy {
    /// Delay before the first check and after every fully-healed pass.
    pub initial_backoff: Duration,
    /// Ceiling for the doubling backoff while identities stay Broken.
    pub max_backoff: Duration,
}

impl Default for ContinuityRepairPolicy {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_secs(30),
            max_backoff: Duration::from_mins(10),
        }
    }
}

/// The identity-first runtime tracks active identities and enforces delivery,
/// ownership, and lifecycle invariants.
pub struct IdentityRuntime {
    entries: RwLock<BTreeMap<AgentIdentity, IdentityEntry>>,
    event_channels: RwLock<BTreeMap<AgentIdentity, broadcast::Sender<IdentityEvent>>>,
    continuity_store: Arc<dyn ContinuityStore>,
    lease_provider: Arc<dyn LeaseProvider>,
    runtime_instance_id: String,
    has_runtime_store: bool,
    durability_policy: DurabilityPolicy,
    bridge: Option<Arc<dyn SessionBridge>>,
    reset_roster_source: StdRwLock<Option<ResetRosterSource>>,
    runtime_services: AgentRuntimeServices,
    managed_peer_edges: RwLock<BTreeSet<(AgentIdentity, AgentIdentity)>>,
    managed_peer_reconcile_lock: Mutex<()>,
    desired_peer_edges: RwLock<Vec<ManagedPeerEdge>>,
    materialization_locks: RwLock<BTreeMap<AgentIdentity, Arc<Mutex<()>>>>,
    best_effort_materialization_locks: RwLock<BTreeMap<AgentIdentity, Arc<Mutex<()>>>>,
    lifecycle_locks: RwLock<BTreeMap<AgentIdentity, Arc<Mutex<()>>>>,
    customizer: RwLock<Option<Arc<dyn AgentCustomizer>>>,
    agent_memory: RwLock<Option<AgentMemoryRuntimeInjector>>,
    lease_renewal_notify: Notify,
    default_timeout: Duration,
    materialization_failure_backoff: RwLock<BTreeMap<AgentIdentity, MaterializationFailureBackoff>>,
    error_hook: StdRwLock<Option<crate::unified_runtime::ErrorHook>>,
}

#[derive(Clone)]
struct ResetRosterSource {
    provider: Arc<dyn RosterProvider>,
    mob_definition: Option<meerkat_mob::MobDefinition>,
}

#[derive(Debug, Clone)]
struct MaterializationFailureBackoff {
    suppress_until: Instant,
    error: String,
}

impl IdentityRuntime {
    /// Create a new identity runtime with the given configuration.
    pub fn new(config: IdentityRuntimeConfig) -> Self {
        Self {
            entries: RwLock::new(BTreeMap::new()),
            event_channels: RwLock::new(BTreeMap::new()),
            continuity_store: config.continuity_store,
            lease_provider: config.lease_provider,
            runtime_instance_id: config.runtime_instance_id,
            has_runtime_store: config.has_runtime_store,
            durability_policy: config.durability_policy,
            bridge: config.bridge,
            reset_roster_source: StdRwLock::new(None),
            runtime_services: AgentRuntimeServices::empty(),
            managed_peer_edges: RwLock::new(BTreeSet::new()),
            managed_peer_reconcile_lock: Mutex::new(()),
            desired_peer_edges: RwLock::new(Vec::new()),
            materialization_locks: RwLock::new(BTreeMap::new()),
            best_effort_materialization_locks: RwLock::new(BTreeMap::new()),
            lifecycle_locks: RwLock::new(BTreeMap::new()),
            customizer: RwLock::new(None),
            agent_memory: RwLock::new(None),
            lease_renewal_notify: Notify::new(),
            default_timeout: config.default_timeout.unwrap_or(Duration::from_secs(90)),
            materialization_failure_backoff: RwLock::new(BTreeMap::new()),
            error_hook: StdRwLock::new(None),
        }
    }

    pub fn with_runtime_services(mut self, runtime_services: AgentRuntimeServices) -> Self {
        self.runtime_services = runtime_services;
        self
    }

    pub fn with_reset_roster_provider(self, provider: Arc<dyn RosterProvider>) -> Self {
        self.set_reset_roster_provider(Some(provider));
        self
    }

    pub fn with_reset_roster_provider_context(
        self,
        provider: Arc<dyn RosterProvider>,
        mob_definition: Option<meerkat_mob::MobDefinition>,
    ) -> Self {
        self.set_reset_roster_provider_context(Some(provider), mob_definition);
        self
    }

    pub(crate) fn runtime_services(&self) -> AgentRuntimeServices {
        self.runtime_services.clone()
    }

    pub async fn set_agent_customizer(&self, customizer: Option<Arc<dyn AgentCustomizer>>) {
        *self.customizer.write().await = customizer;
    }

    pub async fn set_agent_memory(&self, injector: Option<AgentMemoryRuntimeInjector>) {
        *self.agent_memory.write().await = injector;
    }

    pub async fn agent_memory_supports_recall(&self) -> bool {
        self.agent_memory.read().await.is_some()
    }

    pub async fn agent_memory_supports_remember(&self) -> bool {
        self.agent_memory
            .read()
            .await
            .as_ref()
            .is_some_and(|injector| injector.provider().supports_remember())
    }

    pub async fn agent_memory_supports_forget(&self) -> bool {
        self.agent_memory
            .read()
            .await
            .as_ref()
            .is_some_and(|injector| injector.provider().supports_forget())
    }

    pub async fn agent_memory_supports_update(&self) -> bool {
        self.agent_memory
            .read()
            .await
            .as_ref()
            .is_some_and(|injector| injector.provider().supports_supersede())
    }

    pub async fn agent_memory_supports_manifest(&self) -> bool {
        self.agent_memory
            .read()
            .await
            .as_ref()
            .is_some_and(|injector| injector.provider().supports_manifest())
    }

    pub async fn remember_agent_memory(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        memory: NewAgentMemory,
    ) -> Result<AgentMemoryRecord, AgentMemoryError> {
        self.status(identity)
            .await
            .map_err(|err| AgentMemoryError::InvalidConfig(err.to_string()))?;
        let provider = self
            .agent_memory
            .read()
            .await
            .as_ref()
            .map(AgentMemoryRuntimeInjector::provider)
            .ok_or_else(|| {
                AgentMemoryError::InvalidConfig("agent memory is not configured".to_string())
            })?;
        provider.remember(realm, identity, memory).await
    }

    pub async fn forget_agent_memory(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        memory_id: &str,
    ) -> Result<AgentMemoryForgetResult, AgentMemoryError> {
        self.status(identity)
            .await
            .map_err(|err| AgentMemoryError::InvalidConfig(err.to_string()))?;
        let provider = self
            .agent_memory
            .read()
            .await
            .as_ref()
            .map(AgentMemoryRuntimeInjector::provider)
            .ok_or_else(|| {
                AgentMemoryError::InvalidConfig("agent memory is not configured".to_string())
            })?;
        provider.forget(realm, identity, memory_id).await
    }

    /// Supersede `memory_id` within its lineage (the D4 fix): the new
    /// title/body/tags become the active record; the prior stays
    /// retrievable with provenance.
    pub async fn update_agent_memory(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        memory_id: &str,
        memory: NewAgentMemory,
    ) -> Result<MemoryId, AgentMemoryError> {
        self.status(identity)
            .await
            .map_err(|err| AgentMemoryError::InvalidConfig(err.to_string()))?;
        let provider = self
            .agent_memory
            .read()
            .await
            .as_ref()
            .map(AgentMemoryRuntimeInjector::provider)
            .ok_or_else(|| {
                AgentMemoryError::InvalidConfig("agent memory is not configured".to_string())
            })?;
        let scope = MemoryScope::Identity {
            realm: realm.to_string(),
            identity: identity.as_str().to_string(),
        };
        let record = NewMemoryRecord {
            kind: MemoryKind::Fact,
            title: memory.title,
            description: String::new(),
            body: memory.body,
            tags: memory.tags,
            evidence: Vec::new(),
            verification: None,
        };
        provider.supersede(&scope, memory_id, record).await
    }

    /// Tiered metadata manifest for the identity's own scope (§8.3).
    pub async fn manifest_agent_memory(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        tier: ManifestTier,
    ) -> Result<Vec<RecordMeta>, AgentMemoryError> {
        self.status(identity)
            .await
            .map_err(|err| AgentMemoryError::InvalidConfig(err.to_string()))?;
        let provider = self
            .agent_memory
            .read()
            .await
            .as_ref()
            .map(AgentMemoryRuntimeInjector::provider)
            .ok_or_else(|| {
                AgentMemoryError::InvalidConfig("agent memory is not configured".to_string())
            })?;
        let scope = MemoryScope::Identity {
            realm: realm.to_string(),
            identity: identity.as_str().to_string(),
        };
        provider.manifest(&[scope], tier).await
    }

    pub async fn recall_agent_memory(
        &self,
        request: AgentMemoryRecallRequest,
    ) -> Result<Vec<AgentMemoryRecord>, AgentMemoryError> {
        self.status(&request.identity)
            .await
            .map_err(|err| AgentMemoryError::InvalidConfig(err.to_string()))?;
        let provider = self
            .agent_memory
            .read()
            .await
            .as_ref()
            .map(AgentMemoryRuntimeInjector::provider)
            .ok_or_else(|| {
                AgentMemoryError::InvalidConfig("agent memory is not configured".to_string())
            })?;
        let records = provider.recall(request).await?;
        // §9.2: explicit recall reads mark usage mechanically. Telemetry
        // never fails the read — providers without usage support
        // (markdown) return Unsupported, which is downgraded here.
        if !records.is_empty() {
            let ids: Vec<MemoryId> = records
                .iter()
                .map(|record| record.memory_id.clone())
                .collect();
            if let Err(err) = provider.mark_usage(&ids, UsageEvent::ExplicitRecall).await {
                tracing::debug!(error = %err, "agent memory explicit-recall usage marking skipped");
            }
        }
        Ok(records)
    }

    /// Attach the roster provider reset should consult for current specs.
    pub fn set_reset_roster_provider(&self, provider: Option<Arc<dyn RosterProvider>>) {
        self.set_reset_roster_provider_context(provider, None);
    }

    /// Attach the roster provider and context reset should consult for current specs.
    pub fn set_reset_roster_provider_context(
        &self,
        provider: Option<Arc<dyn RosterProvider>>,
        mob_definition: Option<meerkat_mob::MobDefinition>,
    ) {
        let source = provider.map(|provider| ResetRosterSource {
            provider,
            mob_definition,
        });
        match self.reset_roster_source.write() {
            Ok(mut stored_source) => *stored_source = source,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "identity runtime reset roster source lock poisoned; dropping provider update"
                );
            }
        }
    }

    async fn adopt_current_roster_spec_for_reset(&self, identity: &AgentIdentity) {
        let source = match self.reset_roster_source.read() {
            Ok(stored_source) => stored_source.clone(),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "reset: roster source lock poisoned; rebuilding on stored spec"
                );
                None
            }
        };
        if let Some(source) = source {
            self.adopt_roster_spec_with_context(
                &source.provider,
                identity,
                source.mob_definition.clone(),
            )
            .await;
        }
    }

    /// Attach a best-effort operational error hook used for alerting.
    pub fn set_error_hook(&self, hook: Option<crate::unified_runtime::ErrorHook>) {
        match self.error_hook.write() {
            Ok(mut stored_hook) => *stored_hook = hook,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "identity runtime error hook lock poisoned; dropping hook update"
                );
            }
        }
    }

    /// Spawn a background supervisor that renews active identity leases before
    /// they reach their TTL deadline.
    pub fn spawn_lease_renewal_task(self: Arc<Self>) -> JoinHandle<()> {
        self.spawn_lease_renewal_task_with_poll_interval(DEFAULT_LEASE_RENEWAL_MAX_POLL_INTERVAL)
    }

    /// Spawn a lease renewal supervisor with a caller-provided maximum poll
    /// interval. Embedders can use this for shorter external lease TTLs; tests
    /// use it to exercise renewal without waiting on wall-clock TTLs.
    pub fn spawn_lease_renewal_task_with_poll_interval(
        self: Arc<Self>,
        max_poll_interval: Duration,
    ) -> JoinHandle<()> {
        let max_poll_interval = max_poll_interval.max(DEFAULT_LEASE_RENEWAL_MIN_POLL_INTERVAL);
        tokio::spawn(async move {
            let mut consecutive_failures: u32 = 0;
            loop {
                let base = self.lease_renewal_sleep_interval(max_poll_interval).await;
                // While the provider is failing, hold off at least the backoff
                // delay so a persistent outage retries at a bounded rate
                // instead of the TTL-derived floor (down to 10ms).
                let sleep = if consecutive_failures > 0 {
                    base.max(lease_renewal_failure_backoff(
                        consecutive_failures,
                        max_poll_interval,
                    ))
                } else {
                    base
                };
                tokio::select! {
                    () = tokio::time::sleep(sleep) => {}
                    () = self.lease_renewal_notify.notified() => {}
                }
                match self.renew_due_leases_once().await {
                    Ok(_) => consecutive_failures = 0,
                    Err(err) => {
                        // Warn once, then debounce to debug so a backend outage
                        // can't flood the log at the renewal cadence.
                        if consecutive_failures == 0 {
                            tracing::warn!(
                                error = %err,
                                "identity-first proactive lease renewal tick failed; backing off"
                            );
                        } else {
                            tracing::debug!(
                                error = %err,
                                consecutive_failures,
                                "identity-first lease renewal still failing; backing off"
                            );
                        }
                        consecutive_failures = consecutive_failures.saturating_add(1);
                    }
                }
            }
        })
    }

    async fn lease_renewal_sleep_interval(&self, max_poll_interval: Duration) -> Duration {
        let entries = self.entries.read().await;
        entries
            .values()
            .filter(|entry| entry.state == IdentityLifecycleState::Active)
            .filter_map(|entry| entry.lease.as_ref())
            .map(|lease| (lease.ttl / 10).max(DEFAULT_LEASE_RENEWAL_MIN_POLL_INTERVAL))
            .min()
            .unwrap_or(max_poll_interval)
            .min(max_poll_interval)
    }

    /// Renew every active lease that has entered the runtime's renewal window.
    pub async fn renew_due_leases_once(&self) -> Result<usize, IdentityRuntimeError> {
        let due = {
            let entries = self.entries.read().await;
            entries
                .iter()
                .filter(|(_, entry)| entry.state == IdentityLifecycleState::Active)
                .filter_map(|(identity, entry)| {
                    entry
                        .lease
                        .as_ref()
                        .filter(|lease| !lease.is_healthy())
                        .map(|_| identity.clone())
                })
                .collect::<Vec<_>>()
        };

        let mut renewed = 0;
        let mut first_error = None;
        for identity in due {
            let lifecycle_lock = self.lifecycle_lock_for(&identity).await;
            let _lifecycle_guard = lifecycle_lock.lock().await;
            match self.ensure_active_lease(&identity).await {
                Ok(_) => renewed += 1,
                Err(err) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }
        if let Some(err) = first_error {
            Err(err)
        } else {
            Ok(renewed)
        }
    }

    async fn release_uninstalled_materialize_lease(&self, grant: &LeaseGrant) -> Option<String> {
        self.lease_provider
            .release_leases(std::slice::from_ref(grant))
            .await
            .err()
            .map(|err| err.to_string())
    }

    pub async fn set_desired_peer_edges(&self, edges: Vec<ManagedPeerEdge>) {
        *self.desired_peer_edges.write().await = edges;
    }

    pub async fn desired_peer_edges(&self) -> Vec<ManagedPeerEdge> {
        self.desired_peer_edges.read().await.clone()
    }

    async fn registered_identities(&self) -> Vec<AgentIdentity> {
        self.entries.read().await.keys().cloned().collect()
    }

    async fn reachable_peer_identities(&self, identity: &AgentIdentity) -> Vec<AgentIdentity> {
        self.desired_peer_edges
            .read()
            .await
            .iter()
            .filter_map(|edge| {
                if edge.a() == identity {
                    Some(edge.b().clone())
                } else if edge.b() == identity {
                    Some(edge.a().clone())
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    #[must_use]
    pub fn has_session_bridge(&self) -> bool {
        self.bridge.is_some()
    }

    /// Apply identity-first managed topology to the concrete mob graph.
    ///
    /// Topology providers return stable logical identities. The mob comms graph
    /// is keyed by active runtime member IDs, so this resolves each endpoint
    /// through continuity records before calling the same-mob bridge wire APIs.
    pub async fn reconcile_managed_peer_edges(
        &self,
        desired_edges: &[ManagedPeerEdge],
    ) -> Result<(), IdentityRuntimeError> {
        let _guard = self.managed_peer_reconcile_lock.lock().await;
        let Some(bridge) = self.bridge.clone() else {
            return Ok(());
        };

        let active_runtimes: BTreeMap<AgentIdentity, AgentRuntimeId> = {
            let entries = self.entries.read().await;
            entries
                .iter()
                .filter_map(|(identity, entry)| {
                    if entry.state != IdentityLifecycleState::Active {
                        return None;
                    }
                    entry
                        .continuity
                        .as_ref()
                        .map(|record| (identity.clone(), record.agent_runtime_id.clone()))
                })
                .collect()
        };
        let runtime_identities: BTreeMap<AgentRuntimeId, AgentIdentity> = active_runtimes
            .iter()
            .map(|(identity, runtime_id)| (runtime_id.clone(), identity.clone()))
            .collect();
        let current_logical_edges: Option<BTreeSet<(AgentIdentity, AgentIdentity)>> =
            match bridge.current_member_wires().await {
                Ok(current_runtime_edges) => Some(
                    current_runtime_edges
                        .iter()
                        .filter_map(|(runtime_a, runtime_b)| {
                            let a = runtime_identities.get(runtime_a)?;
                            let b = runtime_identities.get(runtime_b)?;
                            if a <= b {
                                Some((a.clone(), b.clone()))
                            } else {
                                Some((b.clone(), a.clone()))
                            }
                        })
                        .collect(),
                ),
                Err(err) => {
                    tracing::debug!(
                        error = %err,
                        "identity-first topology reconcile could not inspect current member wires"
                    );
                    None
                }
            };

        let desired: BTreeSet<(AgentIdentity, AgentIdentity)> = desired_edges
            .iter()
            .map(|edge| (edge.a().clone(), edge.b().clone()))
            .collect();

        let managed_snapshot = self.managed_peer_edges.read().await.clone();
        let edge_is_managed_and_live = |edge: &(AgentIdentity, AgentIdentity)| {
            // Managed-but-missing live edges are retried deliberately so tolerant topology restores self-heal.
            managed_snapshot.contains(edge)
                && current_logical_edges
                    .as_ref()
                    .is_none_or(|edges| edges.contains(edge))
        };
        let retained_logical_edges: Vec<(AgentIdentity, AgentIdentity)> = desired
            .iter()
            .filter(|edge| !edge_is_managed_and_live(edge))
            .filter(|edge| {
                current_logical_edges
                    .as_ref()
                    .is_some_and(|edges| edges.contains(*edge))
            })
            .filter(|(a, b)| active_runtimes.contains_key(a) && active_runtimes.contains_key(b))
            .cloned()
            .collect();
        let to_wire: Vec<(AgentIdentity, AgentIdentity, AgentRuntimeId, AgentRuntimeId)> = desired
            .iter()
            .filter(|edge| !edge_is_managed_and_live(edge))
            .filter(|edge| {
                current_logical_edges
                    .as_ref()
                    .is_none_or(|edges| !edges.contains(*edge))
            })
            .filter_map(|(a, b)| {
                let runtime_a = active_runtimes.get(a)?;
                let runtime_b = active_runtimes.get(b)?;
                Some((a.clone(), b.clone(), runtime_a.clone(), runtime_b.clone()))
            })
            .collect();

        let stale: Vec<(AgentIdentity, AgentIdentity)> = managed_snapshot
            .iter()
            .filter(|edge| !desired.contains(*edge))
            .cloned()
            .collect();
        let to_unwire: Vec<(AgentIdentity, AgentIdentity, AgentRuntimeId, AgentRuntimeId)> = stale
            .iter()
            .filter_map(|(a, b)| {
                let runtime_a = active_runtimes.get(a)?;
                let runtime_b = active_runtimes.get(b)?;
                if current_logical_edges
                    .as_ref()
                    .is_some_and(|edges| !edges.contains(&(a.clone(), b.clone())))
                {
                    return None;
                }
                Some((a.clone(), b.clone(), runtime_a.clone(), runtime_b.clone()))
            })
            .collect();

        let wire_logical_edges = to_wire
            .iter()
            .map(|(a, b, _, _)| (a.clone(), b.clone()))
            .collect::<Vec<_>>();
        let wire_runtime_edges = to_wire
            .iter()
            .map(|(_, _, runtime_a, runtime_b)| (runtime_a.clone(), runtime_b.clone()))
            .collect::<Vec<_>>();
        if !wire_runtime_edges.is_empty() {
            bridge
                .wire_peers_batch(&wire_runtime_edges)
                .await
                .map_err(|e| {
                    IdentityRuntimeError::Internal(format!("bridge wire_peers_batch: {e}"))
                })?;
        }

        let unwire_results =
            stream::iter(to_unwire.into_iter().map(|(a, b, runtime_a, runtime_b)| {
                let bridge = bridge.clone();
                async move {
                    let result = bridge
                        .unwire_peer(&runtime_a, &runtime_b)
                        .await
                        .map_err(|e| format!("{e}"));
                    (a, b, result)
                }
            }))
            .buffer_unordered(MANAGED_PEER_RECONCILE_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

        let mut managed = self.managed_peer_edges.write().await;
        for (a, b) in retained_logical_edges {
            managed.insert((a, b));
        }
        for (a, b) in wire_logical_edges {
            managed.insert((a, b));
        }

        for (a, b) in stale {
            let key = (a.clone(), b.clone());
            if !active_runtimes.contains_key(&a)
                || !active_runtimes.contains_key(&b)
                || current_logical_edges
                    .as_ref()
                    .is_some_and(|edges| !edges.contains(&key))
            {
                managed.remove(&key);
            }
        }
        for (a, b, result) in unwire_results {
            result
                .map_err(|e| IdentityRuntimeError::Internal(format!("bridge unwire_peer: {e}")))?;
            managed.remove(&(a, b));
        }

        Ok(())
    }

    /// Emit an event for the given identity. Best-effort — no error if no subscribers.
    async fn emit_event(&self, identity: &AgentIdentity, event: IdentityEvent) {
        let channels = self.event_channels.read().await;
        if let Some(tx) = channels.get(identity) {
            let _ = tx.send(event);
        }
    }

    fn emit_error(&self, event: crate::unified_runtime::types::ErrorEvent) {
        let hook = match self.error_hook.read() {
            Ok(stored_hook) => stored_hook.clone(),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "identity runtime error hook lock poisoned; dropping error event"
                );
                None
            }
        };
        if let Some(hook) = hook {
            tokio::spawn(async move {
                let () = hook(event).await;
            });
        }
    }

    async fn materialization_backoff_error(&self, identity: &AgentIdentity) -> Option<String> {
        let backoffs = self.materialization_failure_backoff.read().await;
        let backoff = backoffs.get(identity)?;
        if Instant::now() < backoff.suppress_until {
            Some(backoff.error.clone())
        } else {
            None
        }
    }

    async fn clear_materialization_backoff(&self, identity: &AgentIdentity) {
        self.materialization_failure_backoff
            .write()
            .await
            .remove(identity);
    }

    async fn record_best_effort_materialization_failure(
        &self,
        identity: &AgentIdentity,
        initiator: Option<&AgentIdentity>,
        operation: &'static str,
        err: &IdentityRuntimeError,
    ) {
        let error = err.to_string();
        let suppress_until = Instant::now() + MATERIALIZATION_FAILURE_BACKOFF;
        self.materialization_failure_backoff.write().await.insert(
            identity.clone(),
            MaterializationFailureBackoff {
                suppress_until,
                error: error.clone(),
            },
        );
        self.emit_error(
            crate::unified_runtime::types::ErrorEvent::IdentityMaterializationFailure {
                identity: identity.to_string(),
                initiator: initiator.map(ToString::to_string),
                operation: operation.to_string(),
                error,
            },
        );
    }

    // -----------------------------------------------------------------------
    // Registration / activation
    // -----------------------------------------------------------------------

    /// Register an identity entry in the runtime (called during restore flow).
    pub async fn register(
        &self,
        spec: DurableAgentSpec,
        state: IdentityLifecycleState,
        continuity: Option<ContinuityRecord>,
        lease: Option<LeaseGrant>,
    ) {
        let identity = spec.identity.clone();
        let cpv = continuity
            .as_ref()
            .map(|r| r.checkpoint_version)
            .unwrap_or(CheckpointVersion::new(0));
        let lease_entry = lease.map(|g| LeaseEntry {
            fencing_token: g.fencing_token,
            ttl: g.ttl,
            acquired_at: Instant::now(),
        });
        let entry = IdentityEntry {
            spec,
            state,
            continuity,
            lease: lease_entry,
            checkpoint_version: cpv,
            has_runtime_store: self.has_runtime_store,
        };
        let has_active_lease =
            entry.state == IdentityLifecycleState::Active && entry.lease.is_some();
        self.entries.write().await.insert(identity.clone(), entry);

        // Create event channel for this identity
        let (tx, _) = broadcast::channel(IDENTITY_EVENT_CHANNEL_CAPACITY);
        self.event_channels
            .write()
            .await
            .insert(identity.clone(), tx);
        if has_active_lease {
            self.lease_renewal_notify.notify_one();
        }
    }

    async fn materialization_lock_for(&self, identity: &AgentIdentity) -> Arc<Mutex<()>> {
        if let Some(lock) = self.materialization_locks.read().await.get(identity) {
            return lock.clone();
        }
        let mut locks = self.materialization_locks.write().await;
        locks
            .entry(identity.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn best_effort_materialization_lock_for(
        &self,
        identity: &AgentIdentity,
    ) -> Arc<Mutex<()>> {
        if let Some(lock) = self
            .best_effort_materialization_locks
            .read()
            .await
            .get(identity)
        {
            return lock.clone();
        }
        let mut locks = self.best_effort_materialization_locks.write().await;
        locks
            .entry(identity.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn lifecycle_lock_for(&self, identity: &AgentIdentity) -> Arc<Mutex<()>> {
        if let Some(lock) = self.lifecycle_locks.read().await.get(identity) {
            return lock.clone();
        }
        let mut locks = self.lifecycle_locks.write().await;
        locks
            .entry(identity.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Materialize a dormant identity into a concrete mob member/session.
    ///
    /// This is the lazy counterpart to eager `restore_flow`: it performs the
    /// expensive bridge create/resume and snapshot load only when an identity is
    /// actually touched. Parallel calls for one identity coalesce on a
    /// per-identity lock and re-check state after acquiring it.
    pub async fn materialize(
        &self,
        identity: &AgentIdentity,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        let lock = self.materialization_lock_for(identity).await;
        let _guard = lock.lock().await;

        let (spec, continuity, state) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            if entry.state == IdentityLifecycleState::Active {
                let continuity = entry.continuity.clone().ok_or_else(|| {
                    IdentityRuntimeError::Internal(format!(
                        "active identity {identity} has no continuity record"
                    ))
                })?;
                drop(entries);
                self.clear_materialization_backoff(identity).await;
                return Ok(continuity);
            }
            (entry.spec.clone(), entry.continuity.clone(), entry.state)
        };
        let original_continuity = continuity.clone();
        let continuity = if durable_spec_uses_external_binding(&spec) {
            None
        } else {
            continuity
        };

        match state {
            IdentityLifecycleState::Dormant | IdentityLifecycleState::Uninitialized => {}
            IdentityLifecycleState::Broken
            | IdentityLifecycleState::Retiring
            | IdentityLifecycleState::Suspended => {
                return Err(IdentityRuntimeError::InvalidState {
                    identity: identity.clone(),
                    state,
                    operation: "materialize",
                });
            }
            IdentityLifecycleState::Active => unreachable!("active handled above"),
        }

        let lease_results = self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
            .map_err(IdentityRuntimeError::Lease)?;
        let grant = match lease_results.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(grant)) => grant.clone(),
            Some(super::types::LeaseAcquireResult::AlreadyHeld { holder, .. }) => {
                tracing::error!(
                    %identity,
                    holder = %holder,
                    "single-embodiment guard: refusing to materialize an identity whose \
                     durable lease is held by another live runtime instance"
                );
                return Err(IdentityRuntimeError::AlreadyEmbodied {
                    identity: identity.clone(),
                    holder: holder.clone(),
                });
            }
            None => return Err(IdentityRuntimeError::NoActiveLease(identity.clone())),
        };
        if let Some(record) = continuity.as_ref()
            && let Err(err) = self
                .continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
        {
            let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
            return Err(IdentityRuntimeError::Internal(format!(
                "continuity upsert before materialize: {err}{}",
                cleanup_error
                    .as_ref()
                    .map(|e| format!("; lease cleanup failed: {e}"))
                    .unwrap_or_default(),
            )));
        }

        let active_peers = self.entries.read().await.keys().cloned().collect();
        let managed_edges = self.desired_peer_edges.read().await.clone();
        let build_context = AgentBuildContext {
            identity: identity.clone(),
            active_peers,
            managed_edges,
            runtime_services: self.runtime_services(),
        };
        let mut draft = super::types::AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: spec.additional_instructions.clone(),
            labels: spec.labels.clone(),
            app_context: spec.context.clone(),
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
        };
        if let Some(customizer) = self.customizer.read().await.clone()
            && let Err(err) = customizer
                .customize_build(&build_context, &spec, &mut draft)
                .await
        {
            let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
            return Err(IdentityRuntimeError::Internal(format!(
                "customizer: {err}{}",
                cleanup_error
                    .as_ref()
                    .map(|e| format!("; lease cleanup failed: {e}"))
                    .unwrap_or_default(),
            )));
        }

        let mut abandoned_session_registrations: Vec<SessionId> = Vec::new();
        let mut record = if let Some(mut record) = continuity {
            let snapshot = match self
                .continuity_store
                .load_session_snapshot(&record.session_id)
                .await
            {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
                    return Err(IdentityRuntimeError::Internal(format!(
                        "load session snapshot before materialize: {err}{}",
                        cleanup_error
                            .as_ref()
                            .map(|e| format!("; lease cleanup failed: {e}"))
                            .unwrap_or_default(),
                    )));
                }
            };

            if let Some(bridge) = self.bridge.as_ref() {
                if let Err(err) = bridge
                    .register_session_runtime_state(
                        &record.session_id,
                        identity,
                        record.generation,
                        record.checkpoint_version,
                        grant.fencing_token,
                    )
                    .await
                {
                    let unregister_error = Self::unregister_bridge_session_runtime_states(
                        bridge.as_ref(),
                        std::slice::from_ref(&record.session_id),
                    )
                    .await;
                    let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
                    return Err(IdentityRuntimeError::Internal(format!(
                        "bridge register_session_runtime_state: {err}{}{}",
                        unregister_error
                            .as_ref()
                            .map(|e| format!("; unregister session failed: {e}"))
                            .unwrap_or_default(),
                        cleanup_error
                            .as_ref()
                            .map(|e| format!("; lease cleanup failed: {e}"))
                            .unwrap_or_default(),
                    )));
                }
                let registered_session_id = record.session_id.clone();
                let snapshot = snapshot.unwrap_or(SessionSnapshot { data: Vec::new() });
                let outcome = bridge
                    .resume_session(
                        identity,
                        &record.agent_runtime_id,
                        &spec,
                        &draft,
                        &record.session_id,
                        &snapshot,
                    )
                    .await;
                let outcome = match outcome {
                    Ok(outcome) => outcome,
                    Err(err) => {
                        // A rejected resume must NEVER abandon the durable
                        // session (the transcript is the only copy). Keep the
                        // identity → session binding intact — no retire, no
                        // continuity rebind — mark the identity Broken with
                        // the error attached so this send fails loudly, and
                        // let the next reconcile retry the resume. Unregister
                        // only the session-runtime-state bookkeeping (it is
                        // re-registered on retry).
                        tracing::error!(
                            %identity,
                            session_id = %registered_session_id,
                            error = %err,
                            "materialize resume rejected; marking identity Broken and preserving \
                             the durable session for reconcile retry"
                        );
                        let unregister_error = Self::unregister_bridge_session_runtime_states(
                            bridge.as_ref(),
                            std::slice::from_ref(&registered_session_id),
                        )
                        .await;
                        let lease_cleanup_error =
                            self.release_uninstalled_materialize_lease(&grant).await;
                        {
                            let mut entries = self.entries.write().await;
                            if let Some(entry) = entries.get_mut(identity) {
                                entry.state = IdentityLifecycleState::Broken;
                                entry.lease = None;
                            }
                        }
                        self.emit_event(
                            identity,
                            IdentityEvent::StateChanged {
                                identity: identity.clone(),
                                new_state: IdentityLifecycleState::Broken,
                            },
                        )
                        .await;
                        let detail = format!(
                            "bridge resume_session rejected (identity degraded, durable session \
                             preserved for reconcile retry): {err}{}{}",
                            unregister_error
                                .as_ref()
                                .map(|e| format!("; unregister session failed: {e}"))
                                .unwrap_or_default(),
                            lease_cleanup_error
                                .as_ref()
                                .map(|e| format!("; lease cleanup failed: {e}"))
                                .unwrap_or_default(),
                        );
                        return Err(IdentityRuntimeError::Internal(detail));
                    }
                };
                if let Some(reason) = outcome.fallback_reason().cloned() {
                    tracing::warn!(
                        %identity,
                        reason = ?reason,
                        "lazy identity materialization fresh-spawned after typed resume fallback"
                    );
                    self.emit_event(
                        identity,
                        IdentityEvent::ResumeFallback {
                            identity: identity.clone(),
                            reason,
                        },
                    )
                    .await;
                }
                let effective_session_id = outcome.session_id().clone();
                if effective_session_id != registered_session_id {
                    // §8.4 trigger (b): the resume fallback abandoned the
                    // registered session — harvest it detached (materialize
                    // is a hot path; the session store read stays valid).
                    if let Some(injector) = self.agent_memory.read().await.as_ref() {
                        let abandoned_key = registered_session_id.to_string();
                        injector.note_session_generation(
                            identity,
                            &abandoned_key,
                            record.generation.get(),
                        );
                        injector.spawn_rotation_distillation(
                            identity,
                            &abandoned_key,
                            crate::memory::distiller::DistillCause::ResumeFallback,
                        );
                    }
                    abandoned_session_registrations.push(registered_session_id);
                }
                record.session_id = effective_session_id;
            }
            record
        } else {
            let new_runtime_id =
                AgentRuntimeId::parse(&format!("rt:{identity}:0")).map_err(|err| {
                    IdentityRuntimeError::Internal(format!("failed to mint runtime id: {err}"))
                })?;
            let mut record = ContinuityRecord {
                identity: identity.clone(),
                agent_runtime_id: new_runtime_id,
                session_id: meerkat_core::types::SessionId::new(),
                generation: ContinuityGeneration::new(0),
                checkpoint_version: CheckpointVersion::new(0),
            };
            if let Err(err) = self
                .continuity_store
                .upsert_continuity_record(&record, grant.fencing_token)
                .await
            {
                let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "continuity upsert before materialize create: {err}{}",
                    cleanup_error
                        .as_ref()
                        .map(|e| format!("; lease cleanup failed: {e}"))
                        .unwrap_or_default(),
                )));
            }
            if let Some(bridge) = self.bridge.as_ref() {
                let provisional_session_id = record.session_id.clone();
                if let Err(err) = bridge
                    .register_session_runtime_state(
                        &record.session_id,
                        identity,
                        record.generation,
                        record.checkpoint_version,
                        grant.fencing_token,
                    )
                    .await
                    .map_err(|err| {
                        IdentityRuntimeError::Internal(format!(
                            "bridge register_session_runtime_state: {err}"
                        ))
                    })
                {
                    let unregister_error = Self::unregister_bridge_session_runtime_states(
                        bridge.as_ref(),
                        std::slice::from_ref(&provisional_session_id),
                    )
                    .await;
                    let delete_error = self
                        .continuity_store
                        .delete_continuity_record(identity, grant.fencing_token)
                        .await
                        .err();
                    let cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
                    if let Some(delete_error) = delete_error {
                        return Err(IdentityRuntimeError::Internal(format!(
                            "{err}{}; tentative continuity cleanup failed: {delete_error}{}",
                            unregister_error
                                .as_ref()
                                .map(|e| format!("; unregister session failed: {e}"))
                                .unwrap_or_default(),
                            cleanup_error
                                .as_ref()
                                .map(|e| format!("; lease cleanup failed: {e}"))
                                .unwrap_or_default(),
                        )));
                    }
                    if let Some(cleanup_error) = cleanup_error {
                        return Err(IdentityRuntimeError::Internal(format!(
                            "{err}{}; lease cleanup failed: {cleanup_error}",
                            unregister_error
                                .as_ref()
                                .map(|e| format!("; unregister session failed: {e}"))
                                .unwrap_or_default(),
                        )));
                    }
                    if let Some(unregister_error) = unregister_error {
                        return Err(IdentityRuntimeError::Internal(format!(
                            "{err}; unregister session failed: {unregister_error}"
                        )));
                    }
                    return Err(err);
                }
                let created_session_id = bridge
                    .create_session(
                        identity,
                        &record.agent_runtime_id,
                        &spec,
                        &draft,
                        &record.session_id,
                    )
                    .await
                    .map_err(|err| {
                        IdentityRuntimeError::Internal(format!("bridge create_session: {err}"))
                    });
                match created_session_id {
                    Ok(session_id) => {
                        if session_id != provisional_session_id {
                            abandoned_session_registrations.push(provisional_session_id);
                        }
                        record.session_id = session_id;
                    }
                    Err(err) => {
                        let unregister_error = Self::unregister_bridge_session_runtime_states(
                            bridge.as_ref(),
                            std::slice::from_ref(&provisional_session_id),
                        )
                        .await;
                        let cleanup_error =
                            bridge.retire_member(&record.agent_runtime_id).await.err();
                        let delete_error = self
                            .continuity_store
                            .delete_continuity_record(identity, grant.fencing_token)
                            .await
                            .err();
                        let lease_cleanup_error =
                            self.release_uninstalled_materialize_lease(&grant).await;
                        if unregister_error.is_some()
                            || cleanup_error.is_some()
                            || delete_error.is_some()
                            || lease_cleanup_error.is_some()
                        {
                            return Err(IdentityRuntimeError::Internal(format!(
                                "{err}{}{}{}{}",
                                unregister_error
                                    .as_ref()
                                    .map(|e| format!("; unregister session failed: {e}"))
                                    .unwrap_or_default(),
                                cleanup_error
                                    .as_ref()
                                    .map(|e| format!("; cleanup retire failed: {e}"))
                                    .unwrap_or_default(),
                                delete_error
                                    .as_ref()
                                    .map(|e| format!("; tentative continuity cleanup failed: {e}"))
                                    .unwrap_or_default(),
                                lease_cleanup_error
                                    .as_ref()
                                    .map(|e| format!("; lease cleanup failed: {e}"))
                                    .unwrap_or_default(),
                            )));
                        }
                        return Err(err);
                    }
                }
            }
            record
        };

        if let Err(err) = self
            .continuity_store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await
        {
            let unregister_error = if let Some(bridge) = self.bridge.as_ref() {
                let mut sessions_to_unregister = abandoned_session_registrations.clone();
                sessions_to_unregister.push(record.session_id.clone());
                Self::unregister_bridge_session_runtime_states(
                    bridge.as_ref(),
                    &sessions_to_unregister,
                )
                .await
            } else {
                None
            };
            let cleanup_error = if let Some(bridge) = self.bridge.as_ref() {
                bridge.retire_member(&record.agent_runtime_id).await.err()
            } else {
                None
            };
            let restore_error = self
                .restore_continuity_after_materialize_failure(
                    identity,
                    original_continuity.as_ref(),
                    &grant,
                )
                .await;
            let lease_cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
            if unregister_error.is_some()
                || cleanup_error.is_some()
                || restore_error.is_some()
                || lease_cleanup_error.is_some()
            {
                return Err(IdentityRuntimeError::Internal(format!(
                    "continuity upsert after materialize: {err}{}{}{}{}",
                    unregister_error
                        .as_ref()
                        .map(|e| format!("; unregister session failed: {e}"))
                        .unwrap_or_default(),
                    cleanup_error
                        .as_ref()
                        .map(|e| format!("; cleanup retire failed: {e}"))
                        .unwrap_or_default(),
                    restore_error
                        .as_ref()
                        .map(|e| format!("; continuity rollback failed: {e}"))
                        .unwrap_or_default(),
                    lease_cleanup_error
                        .as_ref()
                        .map(|e| format!("; lease cleanup failed: {e}"))
                        .unwrap_or_default(),
                )));
            }
            return Err(IdentityRuntimeError::Internal(format!(
                "continuity upsert after materialize: {err}"
            )));
        }
        if let Some(bridge) = self.bridge.as_ref() {
            let register_result = bridge
                .register_session_runtime_state(
                    &record.session_id,
                    identity,
                    record.generation,
                    record.checkpoint_version,
                    grant.fencing_token,
                )
                .await;
            let effective_checkpoint_version = match register_result {
                Ok(version) => version,
                Err(err) => {
                    let mut sessions_to_unregister = abandoned_session_registrations.clone();
                    sessions_to_unregister.push(record.session_id.clone());
                    let unregister_error = Self::unregister_bridge_session_runtime_states(
                        bridge.as_ref(),
                        &sessions_to_unregister,
                    )
                    .await;
                    let cleanup_error = bridge.retire_member(&record.agent_runtime_id).await.err();
                    let restore_error = self
                        .restore_continuity_after_materialize_failure(
                            identity,
                            original_continuity.as_ref(),
                            &grant,
                        )
                        .await;
                    let lease_cleanup_error =
                        self.release_uninstalled_materialize_lease(&grant).await;
                    return Err(IdentityRuntimeError::Internal(format!(
                        "bridge register actual session runtime state: {err}{}{}{}{}",
                        unregister_error
                            .as_ref()
                            .map(|e| format!("; unregister session failed: {e}"))
                            .unwrap_or_default(),
                        cleanup_error
                            .as_ref()
                            .map(|e| format!("; cleanup retire failed: {e}"))
                            .unwrap_or_default(),
                        restore_error
                            .as_ref()
                            .map(|e| format!("; continuity rollback failed: {e}"))
                            .unwrap_or_default(),
                        lease_cleanup_error
                            .as_ref()
                            .map(|e| format!("; lease cleanup failed: {e}"))
                            .unwrap_or_default(),
                    )));
                }
            };
            record.checkpoint_version = effective_checkpoint_version;
            if let Some(err) = Self::unregister_bridge_session_runtime_states(
                bridge.as_ref(),
                &abandoned_session_registrations,
            )
            .await
            {
                let actual_unregister_error = Self::unregister_bridge_session_runtime_states(
                    bridge.as_ref(),
                    std::slice::from_ref(&record.session_id),
                )
                .await;
                let unregister_error = actual_unregister_error
                    .map(|actual_err| format!("{err}; actual session: {actual_err}"))
                    .unwrap_or(err);
                let cleanup_error = bridge.retire_member(&record.agent_runtime_id).await.err();
                let restore_error = self
                    .restore_continuity_after_materialize_failure(
                        identity,
                        original_continuity.as_ref(),
                        &grant,
                    )
                    .await;
                let lease_cleanup_error = self.release_uninstalled_materialize_lease(&grant).await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "bridge unregister abandoned session runtime state: {unregister_error}{}{}{}",
                    cleanup_error
                        .as_ref()
                        .map(|e| format!("; cleanup retire failed: {e}"))
                        .unwrap_or_default(),
                    restore_error
                        .as_ref()
                        .map(|e| format!("; continuity rollback failed: {e}"))
                        .unwrap_or_default(),
                    lease_cleanup_error
                        .as_ref()
                        .map(|e| format!("; lease cleanup failed: {e}"))
                        .unwrap_or_default(),
                )));
            }
        }

        {
            let mut entries = self.entries.write().await;
            let entry = entries
                .get_mut(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            entry.continuity = Some(record.clone());
            entry.lease = Some(Self::lease_entry_from_grant(&grant));
            entry.state = IdentityLifecycleState::Active;
            entry.checkpoint_version = record.checkpoint_version;
        }
        self.emit_event(
            identity,
            IdentityEvent::StateChanged {
                identity: identity.clone(),
                new_state: IdentityLifecycleState::Active,
            },
        )
        .await;
        let desired_edges = self.desired_peer_edges.read().await.clone();
        if !desired_edges.is_empty()
            && let Err(err) = self.reconcile_managed_peer_edges(&desired_edges).await
        {
            tracing::warn!(
                identity = %identity,
                error = %err,
                "identity materialized with topology reconcile warning"
            );
        }
        self.clear_materialization_backoff(identity).await;
        Ok(record)
    }

    async fn best_effort_materialize_identity(
        &self,
        identity: AgentIdentity,
        initiator: Option<&AgentIdentity>,
        operation: &'static str,
    ) -> Option<ContinuityRecord> {
        let attempt_lock = self.best_effort_materialization_lock_for(&identity).await;
        let _attempt_guard = attempt_lock.lock().await;

        if let Some(error) = self.materialization_backoff_error(&identity).await {
            tracing::debug!(
                identity = %identity,
                initiator = initiator.map(ToString::to_string).as_deref(),
                error = %error,
                "identity best-effort materialization skipped due to materialization backoff"
            );
            return None;
        }

        match self.materialize(&identity).await {
            Ok(record) => {
                self.clear_materialization_backoff(&identity).await;
                Some(record)
            }
            Err(err) => {
                tracing::warn!(
                    identity = %identity,
                    initiator = initiator.map(ToString::to_string).as_deref(),
                    error = %err,
                    "identity best-effort materialization skipped identity after materialization failure"
                );
                self.record_best_effort_materialization_failure(
                    &identity, initiator, operation, &err,
                )
                .await;
                None
            }
        }
    }

    async fn materialize_all_records(
        &self,
    ) -> Vec<(
        AgentIdentity,
        Result<ContinuityRecord, IdentityRuntimeError>,
    )> {
        let identities = self.registered_identities().await;
        stream::iter(identities.into_iter().map(|identity| async move {
            let result = self.materialize(&identity).await;
            (identity, result)
        }))
        .buffer_unordered(MANAGED_PEER_RECONCILE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
    }

    /// Materialize all identities currently registered with the runtime.
    ///
    /// Fleet hydration is best-effort: one member that cannot build is skipped
    /// and surfaced through logs/error hooks rather than aborting unrelated
    /// members.
    pub async fn materialize_all(&self) -> Result<Vec<ContinuityRecord>, IdentityRuntimeError> {
        let identities = self.registered_identities().await;
        let records = stream::iter(identities.into_iter().map(|identity| async move {
            self.best_effort_materialize_identity(identity, None, "materialize_all")
                .await
        }))
        .buffer_unordered(MANAGED_PEER_RECONCILE_CONCURRENCY)
        .filter_map(async move |record| record)
        .collect::<Vec<_>>()
        .await;

        let desired_edges = self.desired_peer_edges.read().await.clone();
        if !desired_edges.is_empty()
            && let Err(err) = self.reconcile_managed_peer_edges(&desired_edges).await
        {
            tracing::warn!(
                error = %err,
                "identity materialize_all completed with topology reconcile warning"
            );
        }

        Ok(records)
    }

    /// Materialize all identities and fail if any registered identity cannot
    /// hydrate. Flow admission uses this strict variant so a run is not accepted
    /// with a partially materialized identity-first fleet.
    pub async fn materialize_all_required(
        &self,
    ) -> Result<Vec<ContinuityRecord>, IdentityRuntimeError> {
        let results = self.materialize_all_records().await;
        let mut records = Vec::with_capacity(results.len());
        let mut failures = Vec::new();

        for (identity, result) in results {
            match result {
                Ok(record) => records.push(record),
                Err(err) => failures.push(format!("{identity}: {err}")),
            }
        }

        if !failures.is_empty() {
            return Err(IdentityRuntimeError::Internal(format!(
                "identity-first required materialization failed for {} identities: {}",
                failures.len(),
                failures.join("; ")
            )));
        }

        let desired_edges = self.desired_peer_edges.read().await.clone();
        if !desired_edges.is_empty() {
            self.reconcile_managed_peer_edges(&desired_edges).await?;
        }

        Ok(records)
    }

    pub(crate) async fn best_effort_background_warm_identity(&self, identity: AgentIdentity) {
        self.best_effort_materialize_identity(identity, None, "background_warm")
            .await;
    }

    /// Ensure an active identity's desired peer neighborhood exists in the
    /// concrete mob graph before ordinary communication starts.
    pub async fn materialize_reachable_peers(
        &self,
        identity: &AgentIdentity,
    ) -> Result<Vec<ContinuityRecord>, IdentityRuntimeError> {
        let peers = self.reachable_peer_identities(identity).await;
        let records = stream::iter(peers.into_iter().map(|peer| async move {
            self.best_effort_materialize_identity(
                peer,
                Some(identity),
                "materialize_reachable_peers",
            )
            .await
        }))
        .buffer_unordered(MANAGED_PEER_RECONCILE_CONCURRENCY)
        .filter_map(async move |record| record)
        .collect::<Vec<_>>()
        .await;

        let desired_edges = self.desired_peer_edges.read().await.clone();
        if !desired_edges.is_empty()
            && let Err(err) = self.reconcile_managed_peer_edges(&desired_edges).await
        {
            tracing::warn!(
                identity = %identity,
                error = %err,
                "identity peer materialization completed with topology reconcile warning"
            );
        }

        Ok(records)
    }

    // -----------------------------------------------------------------------
    // Subscribe — REQ-06
    // -----------------------------------------------------------------------

    /// Subscribe to identity-scoped events.
    ///
    /// Returns a broadcast receiver that yields `IdentityEvent` items for
    /// state changes, lease updates, lease loss, and checkpoint completions.
    pub async fn subscribe(
        &self,
        identity: &AgentIdentity,
    ) -> Result<broadcast::Receiver<IdentityEvent>, IdentityRuntimeError> {
        let channels = self.event_channels.read().await;
        let tx = channels
            .get(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        Ok(tx.subscribe())
    }

    /// Update the spec for an existing identity (used during reconciliation).
    pub async fn update_spec(&self, spec: DurableAgentSpec) -> Result<(), IdentityRuntimeError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(&spec.identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(spec.identity.clone()))?;
        entry.spec = spec;
        Ok(())
    }

    /// Adopt the roster's CURRENT spec for `identity` into the in-memory entry,
    /// so a subsequent [`reset`](Self::reset) rebuilds the regenerated session
    /// on the current profile instead of carrying the stored one forward.
    ///
    /// Best-effort: logs and leaves the stored spec in place if the roster
    /// can't be resolved or no longer lists the identity (reset's primary job —
    /// the destructive continuity reset — must not fail because the roster
    /// provider hiccuped). The runtime stays roster-agnostic; the provider is
    /// supplied by the caller (the reset RPC handler), which owns it.
    pub async fn adopt_roster_spec(
        &self,
        roster_provider: &Arc<dyn RosterProvider>,
        identity: &AgentIdentity,
    ) {
        self.adopt_roster_spec_with_context(roster_provider, identity, None)
            .await;
    }

    async fn adopt_roster_spec_with_context(
        &self,
        roster_provider: &Arc<dyn RosterProvider>,
        identity: &AgentIdentity,
        mob_definition: Option<meerkat_mob::MobDefinition>,
    ) {
        match roster_provider
            .roster(&RosterContext {
                mob_definition,
                previous_identities: Vec::new(),
            })
            .await
        {
            Ok(specs) => {
                if let Some(spec) = specs.into_iter().find(|s| &s.identity == identity)
                    && let Err(err) = self.update_spec(spec).await
                {
                    tracing::warn!(
                        identity = %identity,
                        error = %err,
                        "reset: failed to adopt current roster spec; rebuilding on stored spec",
                    );
                }
            }
            Err(err) => {
                tracing::warn!(
                    identity = %identity,
                    error = %err,
                    "reset: roster provider failed; rebuilding on stored spec",
                );
            }
        }
    }

    /// Update the lease for an identity.
    pub async fn update_lease(
        &self,
        identity: &AgentIdentity,
        grant: LeaseGrant,
    ) -> Result<(), IdentityRuntimeError> {
        let fencing_token = grant.fencing_token;
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        entry.lease = Some(LeaseEntry {
            fencing_token,
            ttl: grant.ttl,
            acquired_at: Instant::now(),
        });
        drop(entries);
        self.lease_renewal_notify.notify_one();
        self.emit_event(
            identity,
            IdentityEvent::LeaseUpdated {
                identity: identity.clone(),
                fencing_token,
            },
        )
        .await;
        Ok(())
    }

    /// Mark a lease as lost for an identity (INV-02).
    pub async fn mark_lease_lost(
        &self,
        identity: &AgentIdentity,
    ) -> Result<(), IdentityRuntimeError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        entry.lease = None;
        drop(entries);
        self.emit_event(
            identity,
            IdentityEvent::LeaseLost {
                identity: identity.clone(),
            },
        )
        .await;
        Ok(())
    }

    /// Remove an identity from the runtime.
    #[allow(dead_code)]
    pub(crate) async fn remove(&self, identity: &AgentIdentity) -> Option<IdentityEntry> {
        self.event_channels.write().await.remove(identity);
        self.entries.write().await.remove(identity)
    }

    /// Set the lifecycle state for an identity.
    pub async fn set_state(
        &self,
        identity: &AgentIdentity,
        state: IdentityLifecycleState,
    ) -> Result<(), IdentityRuntimeError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        entry.state = state;
        drop(entries);
        if state == IdentityLifecycleState::Active {
            self.lease_renewal_notify.notify_one();
        }
        self.emit_event(
            identity,
            IdentityEvent::StateChanged {
                identity: identity.clone(),
                new_state: state,
            },
        )
        .await;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Lease checking (INV-01, INV-02)
    // -----------------------------------------------------------------------

    /// Check that the identity has an active, non-expired lease.
    /// Returns the fencing token if valid.
    fn check_lease(entry: &IdentityEntry) -> Result<FencingToken, IdentityRuntimeError> {
        match &entry.lease {
            Some(lease) if !lease.is_expired() => Ok(lease.fencing_token),
            Some(_) => Err(IdentityRuntimeError::LeaseLost(entry.spec.identity.clone())),
            None => Err(IdentityRuntimeError::NoActiveLease(
                entry.spec.identity.clone(),
            )),
        }
    }

    fn lease_entry_from_grant(grant: &LeaseGrant) -> LeaseEntry {
        LeaseEntry {
            fencing_token: grant.fencing_token,
            ttl: grant.ttl,
            acquired_at: Instant::now(),
        }
    }

    async fn ensure_active_lease(
        &self,
        identity: &AgentIdentity,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        let (grant, continuity) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            let lease = match &entry.lease {
                Some(lease) if lease.is_healthy() => return Ok(lease.fencing_token),
                Some(lease) => lease,
                None => return Err(IdentityRuntimeError::NoActiveLease(identity.clone())),
            };
            (
                LeaseGrant {
                    identity: identity.clone(),
                    fencing_token: lease.fencing_token,
                    ttl: lease.ttl,
                },
                entry.continuity.clone(),
            )
        };

        let renewed = self
            .lease_provider
            .renew_leases(std::slice::from_ref(&grant))
            .await
            .map_err(IdentityRuntimeError::Lease)?;
        let renewed_grant = match renewed.get(identity) {
            Some(super::types::LeaseRenewResult::Renewed(grant)) => grant.clone(),
            Some(super::types::LeaseRenewResult::Lost { .. }) | None => {
                self.mark_lease_lost(identity).await?;
                return Err(IdentityRuntimeError::LeaseLost(identity.clone()));
            }
        };

        if let Some(record) = continuity.as_ref() {
            self.continuity_store
                .upsert_continuity_record(record, renewed_grant.fencing_token)
                .await
                .map_err(IdentityRuntimeError::Store)?;
            if let Some(bridge) = self.bridge.as_ref() {
                bridge
                    .register_session_runtime_state(
                        &record.session_id,
                        identity,
                        record.generation,
                        record.checkpoint_version,
                        renewed_grant.fencing_token,
                    )
                    .await
                    .map_err(|err| {
                        IdentityRuntimeError::Internal(format!(
                            "bridge refresh session runtime state after lease renewal: {err}"
                        ))
                    })?;
            }
        }

        let fencing_token = renewed_grant.fencing_token;
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        match entry.lease.as_ref() {
            Some(current) if current.fencing_token == grant.fencing_token => {
                entry.lease = Some(Self::lease_entry_from_grant(&renewed_grant));
            }
            Some(current) => return Ok(current.fencing_token),
            None => return Err(IdentityRuntimeError::NoActiveLease(identity.clone())),
        }
        drop(entries);
        self.lease_renewal_notify.notify_one();

        self.emit_event(
            identity,
            IdentityEvent::LeaseUpdated {
                identity: identity.clone(),
                fencing_token,
            },
        )
        .await;
        Ok(fencing_token)
    }

    async fn mark_lifecycle_in_progress(
        &self,
        identity: &AgentIdentity,
        state: IdentityLifecycleState,
    ) -> Result<IdentityEntry, IdentityRuntimeError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        let snapshot = entry.clone();
        entry.state = state;
        entry.lease = None;
        Ok(snapshot)
    }

    async fn restore_entry(&self, identity: &AgentIdentity, entry: IdentityEntry) {
        self.entries.write().await.insert(identity.clone(), entry);
    }

    async fn restore_entry_with_grant(
        &self,
        identity: &AgentIdentity,
        mut entry: IdentityEntry,
        grant: &LeaseGrant,
    ) {
        let restore_live_lease = entry.state == IdentityLifecycleState::Active;
        if let Some(record) = entry.continuity.as_ref() {
            if let Err(err) = self
                .continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
            {
                tracing::warn!(
                    %identity,
                    error = %err,
                    "failed to advance restored continuity fencing token after lifecycle failure"
                );
                entry.state = IdentityLifecycleState::Broken;
            } else if let Some(bridge) = self.bridge.as_ref()
                && let Err(err) = bridge
                    .register_session_runtime_state(
                        &record.session_id,
                        identity,
                        record.generation,
                        record.checkpoint_version,
                        grant.fencing_token,
                    )
                    .await
            {
                tracing::warn!(
                    %identity,
                    error = %err,
                    "failed to refresh restored session runtime state after lifecycle failure"
                );
                entry.state = IdentityLifecycleState::Broken;
            }
            entry.lease = restore_live_lease.then(|| Self::lease_entry_from_grant(grant));
        }
        self.restore_entry(identity, entry).await;
        if restore_live_lease {
            self.lease_renewal_notify.notify_one();
        }
    }

    pub(crate) async fn refresh_active_restore_grant(
        &self,
        identity: &AgentIdentity,
        grant: &LeaseGrant,
    ) -> Result<(), IdentityRuntimeError> {
        let record = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            if entry.state != IdentityLifecycleState::Active {
                return Err(IdentityRuntimeError::InvalidState {
                    identity: identity.clone(),
                    state: entry.state,
                    operation: "refresh_active_restore_grant",
                });
            }
            entry.continuity.clone()
        };

        if let Some(record) = record.as_ref()
            && let Err(err) = self
                .continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
        {
            let mut entries = self.entries.write().await;
            if let Some(entry) = entries.get_mut(identity) {
                entry.state = IdentityLifecycleState::Broken;
                entry.lease = None;
            }
            drop(entries);
            self.emit_event(
                identity,
                IdentityEvent::StateChanged {
                    identity: identity.clone(),
                    new_state: IdentityLifecycleState::Broken,
                },
            )
            .await;
            return Err(IdentityRuntimeError::Store(err));
        }

        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        if entry.state != IdentityLifecycleState::Active {
            return Err(IdentityRuntimeError::InvalidState {
                identity: identity.clone(),
                state: entry.state,
                operation: "refresh_active_restore_grant",
            });
        }
        entry.lease = Some(Self::lease_entry_from_grant(grant));
        drop(entries);
        self.emit_event(
            identity,
            IdentityEvent::LeaseUpdated {
                identity: identity.clone(),
                fencing_token: grant.fencing_token,
            },
        )
        .await;
        Ok(())
    }

    async fn restore_broken_entry_with_fenced_store(
        &self,
        identity: &AgentIdentity,
        mut entry: IdentityEntry,
        grant: &LeaseGrant,
    ) {
        entry.state = IdentityLifecycleState::Broken;
        entry.lease = None;
        if let Some(record) = entry.continuity.as_ref()
            && let Err(err) = self
                .continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
        {
            tracing::warn!(
                %identity,
                error = %err,
                "failed to preserve fenced continuity record for broken identity"
            );
        }
        self.restore_entry(identity, entry).await;
    }

    async fn mark_rebind_failure_broken(
        &self,
        identity: &AgentIdentity,
        mut entry: IdentityEntry,
        grant: &LeaseGrant,
        rebound_record: &ContinuityRecord,
    ) {
        entry.state = IdentityLifecycleState::Broken;
        entry.lease = None;
        entry.checkpoint_version = rebound_record.checkpoint_version;
        entry.continuity = Some(rebound_record.clone());
        if let Err(err) = self
            .continuity_store
            .upsert_continuity_record(rebound_record, grant.fencing_token)
            .await
        {
            tracing::warn!(
                %identity,
                session_id = %rebound_record.session_id,
                error = %err,
                "failed to preserve rebound continuity after live respawn rebind failure"
            );
        }
        self.restore_entry(identity, entry).await;
    }

    async fn restore_entry_after_reset_bridge_failure(
        &self,
        identity: &AgentIdentity,
        entry: IdentityEntry,
        grant: &LeaseGrant,
        force_broken: bool,
    ) -> Option<ContinuityStoreError> {
        let delete_error = if entry.continuity.is_none() {
            self.continuity_store
                .delete_continuity_record(identity, grant.fencing_token)
                .await
                .err()
        } else {
            None
        };
        if force_broken || delete_error.is_some() {
            self.restore_broken_entry_with_fenced_store(identity, entry, grant)
                .await;
        } else {
            self.restore_entry_with_grant(identity, entry, grant).await;
        }
        delete_error
    }

    async fn restore_continuity_after_materialize_failure(
        &self,
        identity: &AgentIdentity,
        previous: Option<&ContinuityRecord>,
        grant: &LeaseGrant,
    ) -> Option<ContinuityStoreError> {
        match previous {
            Some(record) => self
                .continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
                .err(),
            None => self
                .continuity_store
                .delete_continuity_record(identity, grant.fencing_token)
                .await
                .err(),
        }
    }

    async fn unregister_bridge_session_runtime_states(
        bridge: &dyn SessionBridge,
        session_ids: &[SessionId],
    ) -> Option<String> {
        let mut errors = Vec::new();
        let mut seen = BTreeSet::new();
        for session_id in session_ids {
            if !seen.insert(session_id.to_string()) {
                continue;
            }
            if let Err(err) = bridge.unregister_session_runtime_state(session_id).await {
                errors.push(format!("{session_id}: {err}"));
            }
        }
        (!errors.is_empty()).then(|| errors.join("; "))
    }

    async fn advance_existing_continuity_fence(
        &self,
        identity: &AgentIdentity,
        entry: &IdentityEntry,
        grant: &LeaseGrant,
    ) -> Result<(), IdentityRuntimeError> {
        if let Some(record) = entry.continuity.as_ref() {
            self.continuity_store
                .upsert_continuity_record(record, grant.fencing_token)
                .await
                .map_err(IdentityRuntimeError::Store)?;
        }
        let _ = identity;
        Ok(())
    }

    async fn refresh_existing_session_runtime_state(
        &self,
        identity: &AgentIdentity,
        record: &ContinuityRecord,
        grant: &LeaseGrant,
    ) -> Result<CheckpointVersion, IdentityRuntimeError> {
        let Some(bridge) = self.bridge.as_ref() else {
            return Ok(record.checkpoint_version);
        };
        bridge
            .register_session_runtime_state(
                &record.session_id,
                identity,
                record.generation,
                record.checkpoint_version,
                grant.fencing_token,
            )
            .await
            .map_err(|err| {
                IdentityRuntimeError::Internal(format!(
                    "bridge refresh session runtime state: {err}"
                ))
            })
    }

    // -----------------------------------------------------------------------
    // Delivery: send() — REQ-01, REQ-03
    // -----------------------------------------------------------------------

    /// Send conversational content to an addressable identity.
    ///
    /// Enforces:
    /// - Identity must be registered and active
    /// - Identity must be Addressable (REQ-03)
    /// - Lease must be held (INV-01)
    /// - Lease must not be lost (INV-02)
    ///
    /// Returns the fencing token for the delivery (caller uses it for checkpoint).
    pub async fn send(
        &self,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send_with_mode(identity, content, HandlingMode::Queue)
            .await
    }

    /// Send conversational content using an explicit turn handling mode.
    ///
    /// This is the identity-first counterpart to the mob member send path used
    /// by the console. Ordinary API callers can keep using [`Self::send`],
    /// which preserves queue semantics.
    pub async fn send_with_mode(
        &self,
        identity: &AgentIdentity,
        content: &meerkat_core::ContentInput,
        handling_mode: HandlingMode,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        let should_materialize = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;

            // REQ-03: reject send to InternalOnly
            if entry.spec.addressability == AgentAddressability::InternalOnly {
                return Err(IdentityRuntimeError::NotAddressable(NotAddressable {
                    identity: identity.clone(),
                    addressability: entry.spec.addressability,
                }));
            }
            entry.state == IdentityLifecycleState::Dormant
                || entry.state == IdentityLifecycleState::Uninitialized
        };
        if should_materialize {
            self.materialize(identity).await?;
        }
        // Live steers are latency-sensitive operator input for an already
        // active turn. Ordinary sends may hydrate the reachable topology first,
        // but a steer must reach the current session boundary before the tool
        // turn resumes; background/full-fleet materialization owns the peers.
        if handling_mode != HandlingMode::Steer {
            self.materialize_reachable_peers(identity).await?;
        }

        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            if entry.state != IdentityLifecycleState::Active {
                return Err(IdentityRuntimeError::InvalidState {
                    identity: identity.clone(),
                    state: entry.state,
                    operation: "send",
                });
            }
        }

        let mut token = self.ensure_active_lease(identity).await?;
        let (runtime_id, memory_session_key, memory_generation) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            (
                entry
                    .continuity
                    .as_ref()
                    .map(|c| c.agent_runtime_id.clone()),
                // Scopes the injector's cross-turn dedup + cumulative budget.
                entry.continuity.as_ref().map(|c| c.session_id.to_string()),
                entry.continuity.as_ref().map(|c| c.generation.get()),
            )
        };
        // Steer is latency-sensitive live operator input: it bypasses both
        // memory injection and inbound defanging by design. Every other send
        // is defanged first (§9.1 anti-spoofing — even with injection off,
        // forged memory envelopes are an inbound threat) and only then
        // considered for ambient injection.
        // Ask 1: the user content and the ambient memory recall travel as
        // SEPARATE bodies — `content_to_deliver` is the (defanged) user
        // message, `injected_context` is the recall assembled as its own
        // typed injected-context body. They are never fused into one text.
        let (content_to_deliver, injected_context) = if handling_mode == HandlingMode::Steer {
            (content.clone(), Vec::new())
        } else {
            match self.agent_memory.read().await.clone() {
                Some(injector) => {
                    // §10.1 taint hook: authoritative session attribution
                    // ahead of the async observe stream — the run this send
                    // triggers belongs to this session. The generation bind
                    // feeds the Distiller's EvidenceRefs (§8.4).
                    if let Some(session_key) = memory_session_key.as_deref() {
                        injector.note_current_session(identity, session_key);
                        if let Some(generation) = memory_generation {
                            injector.note_session_generation(identity, session_key, generation);
                        }
                    }
                    let defanged = injector.defang_inbound(identity, content);
                    let injected_context = injector
                        .inject_for_turn(identity, memory_session_key.as_deref(), &defanged)
                        .await
                        .map_err(|err| {
                            IdentityRuntimeError::Internal(format!("agent memory recall: {err}"))
                        })?;
                    (defanged, injected_context)
                }
                None => (content.clone(), Vec::new()),
            }
        };

        // Deliver through the session bridge when available.
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id) {
            let delivered_session_id = bridge
                .deliver_with_mode_and_context(
                    rid,
                    &content_to_deliver,
                    &injected_context,
                    handling_mode,
                )
                .await
                .map_err(|e| IdentityRuntimeError::Internal(format!("bridge deliver: {e}")))?;
            if let Some(rebound_token) = self
                .reconcile_delivered_session_locked(identity, delivered_session_id)
                .await?
            {
                token = rebound_token;
            }
        }

        Ok(token)
    }

    // -----------------------------------------------------------------------
    // Delivery: dispatch() — REQ-02
    // -----------------------------------------------------------------------

    /// Dispatch internal content to any identity (Addressable or InternalOnly).
    ///
    /// Enforces:
    /// - Identity must be registered and active
    /// - Lease must be held (INV-01)
    /// - Lease must not be lost (INV-02)
    ///
    /// Returns (fencing_token, is_durable) where is_durable indicates whether
    /// the dispatch is backed by a runtime_store (REQ-04).
    pub async fn dispatch(
        &self,
        identity: &AgentIdentity,
        input: &DispatchInput,
    ) -> Result<(FencingToken, bool), IdentityRuntimeError> {
        let should_materialize = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            entry.state == IdentityLifecycleState::Dormant
                || entry.state == IdentityLifecycleState::Uninitialized
        };
        if should_materialize {
            self.materialize(identity).await?;
        }
        self.materialize_reachable_peers(identity).await?;

        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            if entry.state != IdentityLifecycleState::Active {
                return Err(IdentityRuntimeError::InvalidState {
                    identity: identity.clone(),
                    state: entry.state,
                    operation: "dispatch",
                });
            }
        }

        let mut token = self.ensure_active_lease(identity).await?;
        let (is_durable, runtime_id) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            // REQ-04: durability depends on runtime_store
            let is_durable = entry.has_runtime_store;

            let runtime_id = entry
                .continuity
                .as_ref()
                .map(|c| c.agent_runtime_id.clone());

            (is_durable, runtime_id)
        };

        // Deliver through the session bridge when available.
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id) {
            let delivered_session_id = bridge
                .deliver(rid, &input.content)
                .await
                .map_err(|e| IdentityRuntimeError::Internal(format!("bridge dispatch: {e}")))?;
            if let Some(rebound_token) = self
                .reconcile_delivered_session_locked(identity, delivered_session_id)
                .await?
            {
                token = rebound_token;
            }
        }

        Ok((token, is_durable))
    }

    // -----------------------------------------------------------------------
    // Status: status() — REQ-07
    // -----------------------------------------------------------------------

    /// Return the full identity status for the given identity.
    pub async fn status(
        &self,
        identity: &AgentIdentity,
    ) -> Result<IdentityStatus, IdentityRuntimeError> {
        let entries = self.entries.read().await;
        let entry = entries
            .get(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;

        let lease_info = entry.lease.as_ref().map(|l| LeaseInfo {
            fencing_token: l.fencing_token,
            ttl_remaining: l.ttl_remaining(),
            healthy: l.is_healthy(),
        });

        let continuity_health = Some(ContinuityHealth {
            store_reachable: true, // tracked per-store in production
            durability_policy: self.durability_policy.clone(),
            last_checkpoint_version: if entry.checkpoint_version.get() > 0 {
                Some(entry.checkpoint_version)
            } else {
                None
            },
        });

        Ok(IdentityStatus {
            identity: identity.clone(),
            state: entry.state,
            agent_runtime_id: entry
                .continuity
                .as_ref()
                .map(|c| c.agent_runtime_id.clone()),
            session_id: entry.continuity.as_ref().map(|c| c.session_id.clone()),
            profile: Some(entry.spec.profile.clone()),
            runtime_mode: entry.spec.runtime_mode_override,
            addressability: entry.spec.addressability,
            display_name: entry.spec.display_name.clone(),
            labels: entry.spec.labels.clone(),
            generation: entry.continuity.as_ref().map(|c| c.generation),
            checkpoint_version: if entry.checkpoint_version.get() > 0 {
                Some(entry.checkpoint_version)
            } else {
                None
            },
            lease: lease_info,
            continuity_health,
        })
    }

    /// Return statuses for every registered identity without materializing
    /// dormant members.
    pub async fn statuses(&self) -> Vec<IdentityStatus> {
        let identities = self
            .entries
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut statuses = Vec::with_capacity(identities.len());
        for identity in identities {
            if let Ok(status) = self.status(&identity).await {
                statuses.push(status);
            }
        }
        statuses
    }

    // -----------------------------------------------------------------------
    // Lifecycle: retire() — REQ-08
    // -----------------------------------------------------------------------

    /// Retire an identity. Validates lease ownership and retires the mob member.
    pub async fn retire(
        &self,
        identity: &AgentIdentity,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        self.ensure_active_lease(identity).await?;
        let registered_entry = self
            .mark_lifecycle_in_progress(identity, IdentityLifecycleState::Retiring)
            .await?;
        let _previous_token = match Self::check_lease(&registered_entry) {
            Ok(token) => token,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(err);
            }
        };
        let runtime_id = registered_entry
            .continuity
            .as_ref()
            .map(|c| c.agent_runtime_id.clone());
        let session_id = registered_entry
            .continuity
            .as_ref()
            .map(|c| c.session_id.clone());

        let acquire_result = match self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::Lease(err));
            }
        };

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };
        if let Err(err) = self
            .advance_existing_continuity_fence(identity, &registered_entry, &grant)
            .await
        {
            let mut broken_entry = registered_entry;
            broken_entry.state = IdentityLifecycleState::Broken;
            broken_entry.lease = None;
            self.restore_entry(identity, broken_entry).await;
            return Err(err);
        }

        // §8.4 trigger (b): distill the outgoing session's tail BEFORE the
        // member retires. Best-effort and bounded — retirement proceeds at
        // the distiller's pre-rotation timeout.
        if let Some(injector) = self.agent_memory.read().await.clone() {
            if let Some(session_id) = session_id.as_ref() {
                injector
                    .distill_before_rotation(
                        identity,
                        &session_id.to_string(),
                        crate::memory::distiller::DistillCause::Retire,
                    )
                    .await;
            }
            // §8.5 exit interview: queue the retired identity's store for
            // the next dream's harvest sub-phase.
            injector
                .note_identity_retired(
                    identity,
                    session_id
                        .as_ref()
                        .map(std::string::ToString::to_string)
                        .as_deref(),
                    "retire",
                )
                .await;
        }

        // Retire the mob member through the session bridge when available.
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id)
            && let Err(err) = bridge.retire_member(rid).await
        {
            self.restore_entry_with_grant(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Internal(format!(
                "bridge retire: {err}"
            )));
        }
        if let (Some(bridge), Some(session_id)) = (&self.bridge, &session_id)
            && let Err(err) = bridge.unregister_session_runtime_state(session_id).await
        {
            let mut broken_entry = registered_entry;
            broken_entry.state = IdentityLifecycleState::Broken;
            broken_entry.lease = None;
            self.restore_entry(identity, broken_entry).await;
            return Err(IdentityRuntimeError::Internal(format!(
                "bridge unregister retired session: {err}"
            )));
        }

        Ok(grant.fencing_token)
    }

    // -----------------------------------------------------------------------
    // Lifecycle: respawn() — REQ-09
    // -----------------------------------------------------------------------

    /// Respawn: non-destructive recovery.
    ///
    /// 1. Fence the current owner
    /// 2. Attempt final checkpoint
    /// 3. Reactivate from authoritative continuity with same record + runtime ID
    /// 4. ContinuityGeneration does NOT advance
    pub async fn respawn(
        &self,
        identity: &AgentIdentity,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        let registered_entry = self
            .mark_lifecycle_in_progress(identity, IdentityLifecycleState::Suspended)
            .await?;

        // Fence the old owner by re-acquiring the lease
        let acquire_result = match self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::Lease(err));
            }
        };

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };

        if let Err(err) = self
            .advance_existing_continuity_fence(identity, &registered_entry, &grant)
            .await
        {
            let mut broken_entry = registered_entry;
            broken_entry.state = IdentityLifecycleState::Broken;
            broken_entry.lease = None;
            self.restore_entry(identity, broken_entry).await;
            return Err(err);
        }

        // Resolve current continuity state
        let resolved = match self
            .continuity_store
            .resolve_many(std::slice::from_ref(identity))
            .await
        {
            Ok(resolved) => resolved,
            Err(err) => {
                self.restore_entry_with_grant(identity, registered_entry.clone(), &grant)
                    .await;
                return Err(IdentityRuntimeError::Store(err));
            }
        };

        let record = match resolved.get(identity) {
            Some(super::types::ContinuityResolveState::Ready { record }) => record.clone(),
            Some(super::types::ContinuityResolveState::Broken { failure }) => {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "broken continuity for {identity}: {}",
                    failure.detail
                )));
            }
            Some(super::types::ContinuityResolveState::Uninitialized) => {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "cannot respawn uninitialized identity {identity}"
                )));
            }
            None => {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::Store(
                    ContinuityStoreError::NotFound {
                        identity: identity.clone(),
                    },
                ));
            }
        };

        // §8.4 trigger (b): respawn is a recovery boundary — harvest the
        // session's window before the runtime refreshes (the SessionId does
        // not rotate here; the cursor stays valid). Bounded; respawn
        // proceeds at the pre-rotation timeout.
        if let Some(injector) = self.agent_memory.read().await.clone() {
            let session_key = record.session_id.to_string();
            injector.note_session_generation(identity, &session_key, record.generation.get());
            injector
                .distill_before_rotation(
                    identity,
                    &session_key,
                    crate::memory::distiller::DistillCause::Respawn,
                )
                .await;
        }

        let effective_checkpoint_version = match self
            .refresh_existing_session_runtime_state(identity, &record, &grant)
            .await
        {
            Ok(version) => version,
            Err(err) => {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(err);
            }
        };
        let mut record = record;
        record.checkpoint_version = effective_checkpoint_version;

        // Update runtime state: same record, new lease, back to Active
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(identity) {
            entry.continuity = Some(record.clone());
            entry.lease = Some(LeaseEntry {
                fencing_token: grant.fencing_token,
                ttl: grant.ttl,
                acquired_at: Instant::now(),
            });
            entry.state = IdentityLifecycleState::Active;
            entry.checkpoint_version = record.checkpoint_version;
        }

        Ok(record)
    }

    /// Rebind continuity to the concrete session created by a lower-level
    /// member respawn. This keeps identity-first status aligned when a control
    /// surface refreshes the mob member outside the identity runtime bridge.
    pub async fn rebind_session_after_live_respawn(
        &self,
        identity: &AgentIdentity,
        session_id: SessionId,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        self.rebind_session_after_live_respawn_locked(identity, session_id)
            .await
    }

    async fn reconcile_delivered_session_locked(
        &self,
        identity: &AgentIdentity,
        delivered_session_id: SessionId,
    ) -> Result<Option<FencingToken>, IdentityRuntimeError> {
        let current_session_id = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            entry
                .continuity
                .as_ref()
                .map(|record| record.session_id.clone())
        };

        let Some(current_session_id) = current_session_id else {
            return Ok(None);
        };
        if current_session_id == delivered_session_id {
            return Ok(None);
        }

        tracing::warn!(
            %identity,
            old_session_id = %current_session_id,
            new_session_id = %delivered_session_id,
            "identity bridge delivery returned a rotated session; rebinding continuity"
        );
        self.rebind_session_after_live_respawn_locked(identity, delivered_session_id)
            .await?;

        let entries = self.entries.read().await;
        let entry = entries
            .get(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        Ok(entry.lease.as_ref().map(|lease| lease.fencing_token))
    }

    async fn rebind_session_after_live_respawn_locked(
        &self,
        identity: &AgentIdentity,
        session_id: SessionId,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let registered_entry = self
            .mark_lifecycle_in_progress(identity, IdentityLifecycleState::Suspended)
            .await?;

        let acquire_result = match self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::Lease(err));
            }
        };

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };

        let mut record = match registered_entry.continuity.as_ref() {
            Some(record) => record.clone(),
            None => {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::UnknownIdentity(identity.clone()));
            }
        };
        if let Err(err) = self
            .advance_existing_continuity_fence(identity, &registered_entry, &grant)
            .await
        {
            if let Some(bridge) = self.bridge.as_ref()
                && let Err(unregister_err) =
                    bridge.unregister_session_runtime_state(&session_id).await
            {
                tracing::warn!(
                    %identity,
                    session_id = %session_id,
                    error = %unregister_err,
                    "failed to unregister rebound session after continuity fence failure"
                );
            }
            let mut broken_entry = registered_entry;
            broken_entry.state = IdentityLifecycleState::Broken;
            broken_entry.lease = None;
            self.restore_entry(identity, broken_entry).await;
            return Err(err);
        }
        let previous_session_id = record.session_id.clone();
        record.session_id = session_id;

        if let Err(err) = self
            .continuity_store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await
        {
            if let Some(bridge) = self.bridge.as_ref()
                && let Err(unregister_err) = bridge
                    .unregister_session_runtime_state(&record.session_id)
                    .await
            {
                tracing::warn!(
                    %identity,
                    session_id = %record.session_id,
                    error = %unregister_err,
                    "failed to unregister rebound session after continuity upsert failure"
                );
            }
            self.mark_rebind_failure_broken(identity, registered_entry, &grant, &record)
                .await;
            return Err(IdentityRuntimeError::Store(err));
        }

        if let Some(bridge) = self.bridge.as_ref() {
            match bridge
                .register_session_runtime_state(
                    &record.session_id,
                    identity,
                    record.generation,
                    record.checkpoint_version,
                    grant.fencing_token,
                )
                .await
            {
                Ok(version) => record.checkpoint_version = version,
                Err(err) => {
                    if let Err(unregister_err) = bridge
                        .unregister_session_runtime_state(&record.session_id)
                        .await
                    {
                        tracing::warn!(
                            %identity,
                            session_id = %record.session_id,
                            error = %unregister_err,
                            "failed to unregister rebound session after bridge register failure"
                        );
                    }
                    self.mark_rebind_failure_broken(identity, registered_entry, &grant, &record)
                        .await;
                    return Err(IdentityRuntimeError::Internal(format!(
                        "bridge rebind respawned session runtime state: {err}"
                    )));
                }
            }
            if previous_session_id != record.session_id
                && let Err(err) = bridge
                    .unregister_session_runtime_state(&previous_session_id)
                    .await
            {
                tracing::warn!(
                    %identity,
                    session_id = %previous_session_id,
                    error = %err,
                    "failed to unregister previous session after live respawn rebind"
                );
            }
        }

        if let Err(err) = self
            .continuity_store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await
        {
            if let Some(bridge) = self.bridge.as_ref()
                && let Err(unregister_err) = bridge
                    .unregister_session_runtime_state(&record.session_id)
                    .await
            {
                tracing::warn!(
                    %identity,
                    session_id = %record.session_id,
                    error = %unregister_err,
                    "failed to unregister rebound session after final continuity upsert failure"
                );
            }
            self.mark_rebind_failure_broken(identity, registered_entry, &grant, &record)
                .await;
            return Err(IdentityRuntimeError::Store(err));
        }

        self.register(
            registered_entry.spec,
            IdentityLifecycleState::Active,
            Some(record.clone()),
            Some(grant),
        )
        .await;
        Ok(record)
    }

    // -----------------------------------------------------------------------
    // Lifecycle: reset() — REQ-10
    // -----------------------------------------------------------------------

    /// Reset: destructive continuity reset.
    ///
    /// 1. Fence old owner
    /// 2. Advance ContinuityGeneration
    /// 3. Create fresh continuity under the same AgentIdentity
    /// 4. Old-owner late writes rejected by stale fencing token
    pub async fn reset(
        &self,
        identity: &AgentIdentity,
    ) -> Result<ContinuityRecord, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        // Re-profile before snapshotting the lifecycle entry so the rebuilt
        // session uses the current roster spec, not the old checkpoint spec.
        self.adopt_current_roster_spec_for_reset(identity).await;
        let registered_entry = self
            .mark_lifecycle_in_progress(identity, IdentityLifecycleState::Suspended)
            .await?;

        // INV-05: fence the old owner first
        let acquire_result = match self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::Lease(err));
            }
        };

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };
        if let Err(err) = self
            .advance_existing_continuity_fence(identity, &registered_entry, &grant)
            .await
        {
            let mut broken_entry = registered_entry;
            broken_entry.state = IdentityLifecycleState::Broken;
            broken_entry.lease = None;
            self.restore_entry(identity, broken_entry).await;
            return Err(err);
        }

        // Resolve to get current generation
        let resolved = match self
            .continuity_store
            .resolve_many(std::slice::from_ref(identity))
            .await
        {
            Ok(resolved) => resolved,
            Err(err) => {
                self.restore_entry_with_grant(identity, registered_entry.clone(), &grant)
                    .await;
                return Err(IdentityRuntimeError::Store(err));
            }
        };

        let current_gen = match resolved.get(identity) {
            Some(super::types::ContinuityResolveState::Ready { record }) => record.generation,
            Some(super::types::ContinuityResolveState::Uninitialized) => {
                ContinuityGeneration::new(0)
            }
            _ => ContinuityGeneration::new(0),
        };

        // Advance generation
        let new_gen = ContinuityGeneration::new(current_gen.get() + 1);
        let new_session_id = meerkat_core::types::SessionId::new();
        let new_runtime_id = AgentRuntimeId::parse(&format!("rt:{identity}:{}", new_gen.get()))
            .map_err(|e| {
                IdentityRuntimeError::Internal(format!("failed to mint runtime id: {e}"))
            })?;

        let new_record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: new_runtime_id,
            session_id: new_session_id,
            generation: new_gen,
            checkpoint_version: CheckpointVersion::new(0),
        };
        let spec = registered_entry.spec.clone();
        let mut draft = super::types::AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: spec.additional_instructions.clone(),
            labels: spec.labels.clone(),
            app_context: spec.context.clone(),
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
        };
        if self.bridge.is_some() {
            let active_peers = self.entries.read().await.keys().cloned().collect();
            let managed_edges = self.desired_peer_edges.read().await.clone();
            let build_context = AgentBuildContext {
                identity: identity.clone(),
                active_peers,
                managed_edges,
                runtime_services: self.runtime_services(),
            };
            if let Some(customizer) = self.customizer.read().await.clone()
                && let Err(err) = customizer
                    .customize_build(&build_context, &spec, &mut draft)
                    .await
            {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::Internal(format!(
                    "customizer after reset: {err}"
                )));
            }
        }

        // Bridge: retire old mob member and create fresh session for the new identity.
        if let Some(bridge) = &self.bridge {
            if let Err(err) = self
                .continuity_store
                .upsert_continuity_record(&new_record, grant.fencing_token)
                .await
            {
                self.restore_entry_with_grant(identity, registered_entry, &grant)
                    .await;
                return Err(IdentityRuntimeError::Store(err));
            }

            let old_runtime_id = registered_entry
                .continuity
                .as_ref()
                .map(|c| c.agent_runtime_id.clone());
            let old_session_id = registered_entry
                .continuity
                .as_ref()
                .map(|c| c.session_id.clone());

            let session_id = bridge
                .create_session(
                    identity,
                    &new_record.agent_runtime_id,
                    &spec,
                    &draft,
                    &new_record.session_id,
                )
                .await
                .map_err(|e| {
                    IdentityRuntimeError::Internal(format!(
                        "bridge create_session after reset: {e}"
                    ))
                });
            let session_id = match session_id {
                Ok(session_id) => session_id,
                Err(err) => {
                    let cleanup_error = bridge
                        .retire_member(&new_record.agent_runtime_id)
                        .await
                        .err();
                    let delete_error = self
                        .restore_entry_after_reset_bridge_failure(
                            identity,
                            registered_entry.clone(),
                            &grant,
                            cleanup_error.is_some(),
                        )
                        .await;
                    if cleanup_error.is_some() || delete_error.is_some() {
                        return Err(IdentityRuntimeError::Internal(format!(
                            "{err}{}{}",
                            cleanup_error
                                .as_ref()
                                .map(|e| format!("; cleanup retire failed: {e}"))
                                .unwrap_or_default(),
                            delete_error
                                .as_ref()
                                .map(|e| format!("; tentative continuity cleanup failed: {e}"))
                                .unwrap_or_default()
                        )));
                    }
                    return Err(err);
                }
            };
            // Update the record with the actual session ID
            let mut new_record = new_record;
            new_record.session_id = session_id;
            tracing::debug!(
                identity = %identity,
                runtime_id = %new_record.agent_runtime_id,
                session_id = %new_record.session_id,
                "reset bridge create_session completed",
            );

            if let Err(err) = self
                .continuity_store
                .upsert_continuity_record(&new_record, grant.fencing_token)
                .await
            {
                let unregister_error = Self::unregister_bridge_session_runtime_states(
                    bridge.as_ref(),
                    std::slice::from_ref(&new_record.session_id),
                )
                .await;
                let cleanup_error = bridge
                    .retire_member(&new_record.agent_runtime_id)
                    .await
                    .err();
                let delete_error = self
                    .restore_entry_after_reset_bridge_failure(
                        identity,
                        registered_entry.clone(),
                        &grant,
                        unregister_error.is_some() || cleanup_error.is_some(),
                    )
                    .await;
                if unregister_error.is_some() || cleanup_error.is_some() || delete_error.is_some() {
                    return Err(IdentityRuntimeError::Internal(format!(
                        "continuity upsert actual session after reset: {err}{}{}{}",
                        unregister_error
                            .as_ref()
                            .map(|e| format!("; unregister session failed: {e}"))
                            .unwrap_or_default(),
                        cleanup_error
                            .as_ref()
                            .map(|e| format!("; cleanup retire failed: {e}"))
                            .unwrap_or_default(),
                        delete_error
                            .as_ref()
                            .map(|e| format!("; tentative continuity cleanup failed: {e}"))
                            .unwrap_or_default(),
                    )));
                }
                return Err(IdentityRuntimeError::Store(err));
            }

            let register_result = bridge
                .register_session_runtime_state(
                    &new_record.session_id,
                    identity,
                    new_record.generation,
                    new_record.checkpoint_version,
                    grant.fencing_token,
                )
                .await;
            let effective_checkpoint_version = match register_result {
                Ok(version) => version,
                Err(err) => {
                    let unregister_error = Self::unregister_bridge_session_runtime_states(
                        bridge.as_ref(),
                        std::slice::from_ref(&new_record.session_id),
                    )
                    .await;
                    let cleanup_error = bridge
                        .retire_member(&new_record.agent_runtime_id)
                        .await
                        .err();
                    let mut detail =
                        format!("bridge register actual session runtime state after reset: {err}");
                    if let Some(unregister_error) = unregister_error.as_ref() {
                        detail
                            .push_str(&format!("; unregister session failed: {unregister_error}"));
                    }
                    if let Some(cleanup_error) = cleanup_error.as_ref() {
                        detail.push_str(&format!("; cleanup retire failed: {cleanup_error}"));
                    }
                    let delete_error = self
                        .restore_entry_after_reset_bridge_failure(
                            identity,
                            registered_entry.clone(),
                            &grant,
                            unregister_error.is_some() || cleanup_error.is_some(),
                        )
                        .await;
                    if let Some(delete_error) = delete_error {
                        return Err(IdentityRuntimeError::Internal(format!(
                            "{detail}; tentative continuity cleanup failed: {delete_error}"
                        )));
                    }
                    return Err(IdentityRuntimeError::Internal(detail));
                }
            };
            new_record.checkpoint_version = effective_checkpoint_version;
            tracing::debug!(
                identity = %identity,
                runtime_id = %new_record.agent_runtime_id,
                session_id = %new_record.session_id,
                checkpoint_version = new_record.checkpoint_version.get(),
                "reset bridge session runtime state registered",
            );

            let cleanup_old_runtime_id = old_runtime_id
                .as_ref()
                .filter(|old_id| *old_id != &new_record.agent_runtime_id)
                .cloned();
            let cleanup_old_session_id = old_session_id
                .as_ref()
                .filter(|old_session_id| *old_session_id != &new_record.session_id)
                .cloned();
            self.spawn_old_bridge_cleanup_after_reset(
                bridge.clone(),
                cleanup_old_runtime_id,
                cleanup_old_session_id,
            );
            tracing::debug!(
                identity = %identity,
                runtime_id = %new_record.agent_runtime_id,
                session_id = %new_record.session_id,
                "reset old bridge cleanup scheduled",
            );

            if let Err(err) = self
                .continuity_store
                .upsert_continuity_record(&new_record, grant.fencing_token)
                .await
            {
                tracing::warn!(
                    identity = %identity,
                    runtime_id = %new_record.agent_runtime_id,
                    session_id = %new_record.session_id,
                    error = %err,
                    "reset final continuity upsert failed after bridge materialization; rolling back new generation",
                );
                let unregister_error = bridge
                    .unregister_session_runtime_state(&new_record.session_id)
                    .await
                    .err();
                let cleanup_error = bridge
                    .retire_member(&new_record.agent_runtime_id)
                    .await
                    .err();
                let rollback_error = self
                    .restore_continuity_after_materialize_failure(
                        identity,
                        registered_entry.continuity.as_ref(),
                        &grant,
                    )
                    .await;
                let mut entries = self.entries.write().await;
                if let Some(entry) = entries.get_mut(identity) {
                    entry.state = IdentityLifecycleState::Broken;
                }
                if unregister_error.is_some() || cleanup_error.is_some() || rollback_error.is_some()
                {
                    return Err(IdentityRuntimeError::Internal(format!(
                        "continuity upsert after reset: {err}{}{}{}",
                        unregister_error
                            .as_ref()
                            .map(|e| format!("; unregister session failed: {e}"))
                            .unwrap_or_default(),
                        cleanup_error
                            .as_ref()
                            .map(|e| format!("; cleanup retire failed: {e}"))
                            .unwrap_or_default(),
                        rollback_error
                            .as_ref()
                            .map(|e| format!("; continuity rollback failed: {e}"))
                            .unwrap_or_default()
                    )));
                }
                return Err(IdentityRuntimeError::Store(err));
            }

            // Update runtime state
            let mut entries = self.entries.write().await;
            let Some(entry) = entries.get_mut(identity) else {
                tracing::warn!(
                    identity = %identity,
                    runtime_id = %new_record.agent_runtime_id,
                    session_id = %new_record.session_id,
                    "reset entry disappeared after bridge materialization; rolling back new generation",
                );
                let _ = bridge
                    .unregister_session_runtime_state(&new_record.session_id)
                    .await;
                let _ = bridge.retire_member(&new_record.agent_runtime_id).await;
                if registered_entry.continuity.is_none() {
                    let _ = self
                        .continuity_store
                        .delete_continuity_record(identity, grant.fencing_token)
                        .await;
                }
                return Err(IdentityRuntimeError::UnknownIdentity(identity.clone()));
            };
            entry.continuity = Some(new_record.clone());
            entry.lease = Some(Self::lease_entry_from_grant(&grant));
            entry.state = IdentityLifecycleState::Active;
            entry.checkpoint_version = new_record.checkpoint_version;
            tracing::debug!(
                identity = %identity,
                runtime_id = %new_record.agent_runtime_id,
                session_id = %new_record.session_id,
                "reset completed",
            );
            drop(entries);
            // §10.1: reset is the deliberate clean-slate boundary — clear
            // session taint explicitly (rotation clears implicitly; this
            // also drops pending pre-attribution taint). §8.4: distill the
            // outgoing session DETACHED (never on the reset critical path;
            // the session store outlives the member, so the read stays
            // valid after teardown) with the reset boundary marked first so
            // every distillate lands Quarantined pending steward review.
            if let Some(injector) = self.agent_memory.read().await.as_ref() {
                injector.clear_taint_for_identity(identity);
                injector.note_session_generation(
                    identity,
                    &new_record.session_id.to_string(),
                    new_record.generation.get(),
                );
                if let Some(old_continuity) = registered_entry.continuity.as_ref() {
                    let old_session_key = old_continuity.session_id.to_string();
                    injector.note_reset_boundary(&old_session_key);
                    injector.note_session_generation(
                        identity,
                        &old_session_key,
                        old_continuity.generation.get(),
                    );
                    injector.spawn_rotation_distillation(
                        identity,
                        &old_session_key,
                        crate::memory::distiller::DistillCause::Reset,
                    );
                }
            }
            return Ok(new_record);
        }

        // Persist the new record (fencing token from new lease protects against old writes)
        if let Err(err) = self
            .continuity_store
            .upsert_continuity_record(&new_record, grant.fencing_token)
            .await
        {
            self.restore_entry_with_grant(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Store(err));
        }

        // No bridge — update runtime state only (validation mode)
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        entry.continuity = Some(new_record.clone());
        entry.lease = Some(Self::lease_entry_from_grant(&grant));
        entry.state = IdentityLifecycleState::Active;
        entry.checkpoint_version = CheckpointVersion::new(0);
        drop(entries);
        if let Some(injector) = self.agent_memory.read().await.as_ref() {
            injector.clear_taint_for_identity(identity);
            // No-bridge (validation) reset: same §8.4 boundary semantics.
            if let Some(old_continuity) = registered_entry.continuity.as_ref() {
                let old_session_key = old_continuity.session_id.to_string();
                injector.note_reset_boundary(&old_session_key);
                injector.spawn_rotation_distillation(
                    identity,
                    &old_session_key,
                    crate::memory::distiller::DistillCause::Reset,
                );
            }
        }

        Ok(new_record)
    }

    // -----------------------------------------------------------------------
    // Lifecycle: delete_identity() — REQ-11
    // -----------------------------------------------------------------------

    /// Delete an identity: removes continuity record.
    ///
    /// 1. Fence old owner
    /// 2. Remove ContinuityRecord
    /// 3. Future bootstrap treats identity as Uninitialized
    pub async fn delete_identity(
        &self,
        identity: &AgentIdentity,
    ) -> Result<(), IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        let registered_entry = self
            .mark_lifecycle_in_progress(identity, IdentityLifecycleState::Retiring)
            .await?;
        let runtime_id = registered_entry
            .continuity
            .as_ref()
            .map(|c| c.agent_runtime_id.clone());
        let session_id = registered_entry
            .continuity
            .as_ref()
            .map(|c| c.session_id.clone());

        // INV-05: fence the old owner first
        let acquire_result = match self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::Lease(err));
            }
        };

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                self.restore_entry(identity, registered_entry).await;
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };
        if let Err(err) = self
            .advance_existing_continuity_fence(identity, &registered_entry, &grant)
            .await
        {
            let mut broken_entry = registered_entry;
            broken_entry.state = IdentityLifecycleState::Broken;
            broken_entry.lease = None;
            self.restore_entry(identity, broken_entry).await;
            return Err(err);
        }

        // §8.4 trigger (b): delete is the identity's LAST boundary — harvest
        // the outgoing session before teardown (its exit-interview analog,
        // §8.5, is the steward's; the distillate is what it will read).
        // Bounded; deletion proceeds at the pre-rotation timeout.
        if let Some(injector) = self.agent_memory.read().await.clone() {
            if let Some(session_id) = session_id.as_ref() {
                let session_key = session_id.to_string();
                injector
                    .distill_before_rotation(
                        identity,
                        &session_key,
                        crate::memory::distiller::DistillCause::Delete,
                    )
                    .await;
                // Ask 2 GC: delete permanently abandons this session id, and
                // its knowledge has just been distilled into the identity-keyed
                // MobKit store (which the exit-interview harvest below reads —
                // a DIFFERENT store from the meerkat session-memory scope). So
                // reclaiming the orphaned meerkat scope here frees dead
                // re-embed weight without starving any downstream read.
                injector
                    .drop_orphaned_session_scope(
                        &session_key,
                        crate::memory::distiller::DistillCause::Delete,
                    )
                    .await;
            }
            // §8.5 exit interview (delete is the identity's LAST boundary).
            injector
                .note_identity_retired(
                    identity,
                    session_id
                        .as_ref()
                        .map(std::string::ToString::to_string)
                        .as_deref(),
                    "delete",
                )
                .await;
        }

        // Retire the mob member through the session bridge before removing
        // the continuity record. This ensures the mob actor is cleaned up.
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id)
            && let Err(err) = bridge.retire_member(rid).await
        {
            self.restore_entry_with_grant(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Internal(format!(
                "bridge retire before delete: {err}"
            )));
        }

        if let (Some(bridge), Some(session_id)) = (&self.bridge, &session_id)
            && let Some(err) = Self::unregister_bridge_session_runtime_states(
                bridge.as_ref(),
                std::slice::from_ref(session_id),
            )
            .await
        {
            self.restore_broken_entry_with_fenced_store(identity, registered_entry, &grant)
                .await;
            return Err(IdentityRuntimeError::Internal(format!(
                "bridge unregister session before delete: {err}"
            )));
        }

        // Remove authoritative continuity record from the store
        if let Err(err) = self
            .continuity_store
            .delete_continuity_record(identity, grant.fencing_token)
            .await
        {
            let mut entries = self.entries.write().await;
            if let Some(entry) = entries.get_mut(identity) {
                entry.state = IdentityLifecycleState::Broken;
            }
            return Err(IdentityRuntimeError::Store(err));
        }

        // Remove from runtime tracking
        self.event_channels.write().await.remove(identity);
        self.entries.write().await.remove(identity);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Checkpoint — REQ-14, REQ-15, REQ-16, REQ-17
    // -----------------------------------------------------------------------

    /// Save a checkpoint snapshot. Enforces version ordering and fencing.
    pub async fn checkpoint(
        &self,
        identity: &AgentIdentity,
        snapshot: &SessionSnapshot,
    ) -> Result<CheckpointVersion, IdentityRuntimeError> {
        let lifecycle_lock = self.lifecycle_lock_for(identity).await;
        let _lifecycle_guard = lifecycle_lock.lock().await;
        {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            if entry.state != IdentityLifecycleState::Active {
                return Err(IdentityRuntimeError::InvalidState {
                    identity: identity.clone(),
                    state: entry.state,
                    operation: "checkpoint",
                });
            }
        }

        let token = self.ensure_active_lease(identity).await?;
        let (record, new_version) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            let record = entry
                .continuity
                .as_ref()
                .ok_or_else(|| {
                    IdentityRuntimeError::Internal(format!("no continuity record for {identity}"))
                })?
                .clone();

            let new_version = CheckpointVersion::new(entry.checkpoint_version.get() + 1);
            (record, new_version)
        };

        // REQ-15 + REQ-16: store enforces version ordering and fencing
        self.continuity_store
            .save_session_snapshot(
                identity,
                &record.session_id,
                record.generation,
                new_version,
                token,
                snapshot,
            )
            .await?;

        // Update local checkpoint version
        {
            let mut entries = self.entries.write().await;
            if let Some(entry) = entries.get_mut(identity) {
                entry.checkpoint_version = new_version;
            }
        }

        self.emit_event(
            identity,
            IdentityEvent::CheckpointCompleted {
                identity: identity.clone(),
                version: new_version,
            },
        )
        .await;

        Ok(new_version)
    }

    // -----------------------------------------------------------------------
    // Roster inspection — REQ-32
    // -----------------------------------------------------------------------

    /// Return all active identities with their specs and status.
    pub async fn roster_inspect(
        &self,
    ) -> BTreeMap<AgentIdentity, (DurableAgentSpec, IdentityStatus)> {
        let entries = self.entries.read().await;
        let mut result = BTreeMap::new();
        for (identity, entry) in entries.iter() {
            let lease_info = entry.lease.as_ref().map(|l| LeaseInfo {
                fencing_token: l.fencing_token,
                ttl_remaining: l.ttl_remaining(),
                healthy: l.is_healthy(),
            });
            let continuity_health = Some(ContinuityHealth {
                store_reachable: true,
                durability_policy: self.durability_policy.clone(),
                last_checkpoint_version: if entry.checkpoint_version.get() > 0 {
                    Some(entry.checkpoint_version)
                } else {
                    None
                },
            });
            let status = IdentityStatus {
                identity: identity.clone(),
                state: entry.state,
                agent_runtime_id: entry
                    .continuity
                    .as_ref()
                    .map(|c| c.agent_runtime_id.clone()),
                session_id: entry.continuity.as_ref().map(|c| c.session_id.clone()),
                profile: Some(entry.spec.profile.clone()),
                runtime_mode: entry.spec.runtime_mode_override,
                addressability: entry.spec.addressability,
                display_name: entry.spec.display_name.clone(),
                labels: entry.spec.labels.clone(),
                generation: entry.continuity.as_ref().map(|c| c.generation),
                checkpoint_version: if entry.checkpoint_version.get() > 0 {
                    Some(entry.checkpoint_version)
                } else {
                    None
                },
                lease: lease_info,
                continuity_health,
            };
            result.insert(identity.clone(), (entry.spec.clone(), status));
        }
        result
    }

    // -----------------------------------------------------------------------
    // Roster uniqueness validation — INV-06
    // -----------------------------------------------------------------------

    /// Validate that a roster contains no duplicate identities.
    pub fn validate_roster_uniqueness(
        specs: &[DurableAgentSpec],
    ) -> Result<(), IdentityRuntimeError> {
        let mut seen = std::collections::BTreeSet::new();
        for spec in specs {
            if !seen.insert(&spec.identity) {
                return Err(IdentityRuntimeError::DuplicateIdentity(
                    spec.identity.clone(),
                ));
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Accessors for internal state
    // -----------------------------------------------------------------------

    /// Get the current entries (read-only snapshot).
    #[allow(dead_code)]
    pub(crate) async fn entries(&self) -> BTreeMap<AgentIdentity, IdentityEntry> {
        self.entries.read().await.clone()
    }

    /// Check if an identity is registered.
    pub async fn contains(&self, identity: &AgentIdentity) -> bool {
        self.entries.read().await.contains_key(identity)
    }

    /// Check if an identity is registered AND in Active state.
    pub async fn is_active(&self, identity: &AgentIdentity) -> bool {
        self.entries
            .read()
            .await
            .get(identity)
            .is_some_and(|e| e.state == IdentityLifecycleState::Active)
    }

    /// Resolve a member alias to the durable identity that OWNS it, if any.
    ///
    /// Accepts both a generated runtime alias (`rt:<identity>:<generation>`)
    /// and a plain identity string; returns the identity only when it is
    /// actually registered in this runtime. Member-scoped RPCs use this to
    /// route lifecycle mutations of identity-owned members through the
    /// identity authority — a classic `handle.retire()`/`respawn()` on such
    /// a member would mutate it behind the IdentityRuntime's back (stale
    /// continuity binding, generation drift), which is the doctrine's
    /// "mob plane must not mangle durables" rule.
    pub async fn owned_identity_for_member_alias(&self, alias: &str) -> Option<AgentIdentity> {
        let identity = alias
            .strip_prefix("rt:")
            .and_then(|rest| rest.rsplit_once(':'))
            .filter(|(identity, generation)| {
                !identity.is_empty()
                    && !generation.is_empty()
                    && generation.chars().all(|ch| ch.is_ascii_digit())
            })
            .and_then(|(identity, _)| AgentIdentity::parse(identity).ok())
            .or_else(|| AgentIdentity::parse(alias).ok())?;
        self.contains(&identity).await.then_some(identity)
    }

    /// Identities currently in the Broken lifecycle state.
    pub async fn broken_identities(&self) -> Vec<AgentIdentity> {
        self.entries
            .read()
            .await
            .iter()
            .filter(|(_, entry)| entry.state == IdentityLifecycleState::Broken)
            .map(|(identity, _)| identity.clone())
            .collect()
    }

    /// Get the continuity store reference.
    pub fn continuity_store(&self) -> &Arc<dyn ContinuityStore> {
        &self.continuity_store
    }

    /// Get the lease provider reference.
    pub fn lease_provider(&self) -> &Arc<dyn LeaseProvider> {
        &self.lease_provider
    }

    /// Get the runtime instance ID.
    pub fn runtime_instance_id(&self) -> &str {
        &self.runtime_instance_id
    }

    /// Get the durability policy.
    pub fn durability_policy(&self) -> &DurabilityPolicy {
        &self.durability_policy
    }

    /// Get whether a runtime store is configured.
    pub fn has_runtime_store(&self) -> bool {
        self.has_runtime_store
    }

    /// Get the session bridge reference, if configured.
    pub fn bridge(&self) -> Option<&Arc<dyn SessionBridge>> {
        self.bridge.as_ref()
    }

    // -----------------------------------------------------------------------
    // Convenience methods
    // -----------------------------------------------------------------------

    /// Send plain text to an addressable identity.
    pub async fn send_text(
        &self,
        identity: &AgentIdentity,
        text: impl Into<String>,
    ) -> Result<FencingToken, IdentityRuntimeError> {
        self.send(identity, &meerkat_core::ContentInput::Text(text.into()))
            .await
    }

    /// Dispatch plain text with system origin.
    pub async fn dispatch_text(
        &self,
        identity: &AgentIdentity,
        text: impl Into<String>,
    ) -> Result<(FencingToken, bool), IdentityRuntimeError> {
        self.dispatch(identity, &DispatchInput::system(text)).await
    }

    /// Execute the restore flow for the given roster.
    pub async fn restore_flow(
        &self,
        roster: &[DurableAgentSpec],
        topology_provider: Option<&dyn super::contracts::TopologyProvider>,
        customizer: Option<&dyn super::contracts::AgentCustomizer>,
    ) -> Result<super::orchestrator::RestoreFlowResult, IdentityRuntimeError> {
        super::orchestrator::restore_flow(self, roster, topology_provider, customizer).await
    }

    /// Resolve the AgentRuntimeId for a registered identity.
    pub async fn runtime_id_for(
        &self,
        identity: &AgentIdentity,
    ) -> Result<AgentRuntimeId, IdentityRuntimeError> {
        let entries = self.entries.read().await;
        let entry = entries
            .get(identity)
            .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
        entry
            .continuity
            .as_ref()
            .map(|c| c.agent_runtime_id.clone())
            .ok_or_else(|| {
                IdentityRuntimeError::Internal(format!("no continuity record for {identity}"))
            })
    }

    /// Inspect the current execution state of an identity via the bridge.
    pub async fn inspect(
        &self,
        identity: &AgentIdentity,
    ) -> Result<super::bridge::MemberInspection, IdentityRuntimeError> {
        let runtime_id = self.runtime_id_for(identity).await?;
        let bridge = self
            .bridge
            .as_ref()
            .ok_or_else(|| IdentityRuntimeError::Internal("no bridge configured".to_string()))?;
        bridge
            .inspect_member(&runtime_id)
            .await
            .map_err(|e| IdentityRuntimeError::Internal(format!("inspect: {e}")))
    }

    /// The configured default timeout for wait operations.
    pub fn default_timeout(&self) -> Duration {
        self.default_timeout
    }

    fn spawn_old_bridge_cleanup_after_reset(
        &self,
        bridge: Arc<dyn SessionBridge>,
        old_runtime_id: Option<AgentRuntimeId>,
        old_session_id: Option<SessionId>,
    ) {
        if old_runtime_id.is_none() && old_session_id.is_none() {
            return;
        }
        let runtime_instance_id = self.runtime_instance_id.clone();
        let timeout = self.default_timeout;
        tokio::spawn(async move {
            if let Some(old_runtime_id) = old_runtime_id {
                tracing::debug!(
                    runtime_instance_id = %runtime_instance_id,
                    runtime_id = %old_runtime_id,
                    "skipping old bridge member retire after reset; reset commits the new generation and only clears stale session projection",
                );
            }

            if let Some(old_session_id) = old_session_id {
                match tokio::time::timeout(
                    timeout,
                    bridge.unregister_session_runtime_state(&old_session_id),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        tracing::warn!(
                            runtime_instance_id = %runtime_instance_id,
                            session_id = %old_session_id,
                            error = %err,
                            "failed to unregister old bridge session after reset; continuing with new generation",
                        );
                    }
                    Err(_) => {
                        tracing::warn!(
                            runtime_instance_id = %runtime_instance_id,
                            session_id = %old_session_id,
                            timeout_ms = timeout.as_millis(),
                            "timed out unregistering old bridge session after reset; continuing with new generation",
                        );
                    }
                }
            }
        });
    }

    /// Poll until the identity produces an output_preview, or timeout.
    pub async fn wait_for_output(
        &self,
        identity: &AgentIdentity,
        timeout: Duration,
    ) -> Result<String, IdentityRuntimeError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(inspection) = self.inspect(identity).await
                && let Some(preview) = inspection.output_preview
            {
                return Ok(preview);
            }
            if Instant::now() >= deadline {
                return Err(IdentityRuntimeError::Internal(format!(
                    "timed out waiting for output from {identity}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Poll until output_preview contains the given substring, or timeout.
    pub async fn wait_for_output_containing(
        &self,
        identity: &AgentIdentity,
        needle: &str,
        timeout: Duration,
    ) -> Result<String, IdentityRuntimeError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(inspection) = self.inspect(identity).await
                && let Some(ref preview) = inspection.output_preview
                && preview.contains(needle)
            {
                return Ok(preview.clone());
            }
            if Instant::now() >= deadline {
                return Err(IdentityRuntimeError::Internal(format!(
                    "timed out waiting for output containing '{needle}' from {identity}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

/// Wire two identities across mobs, resolving runtime IDs from both IdentityRuntimes.
///
/// This is a convenience function for same-process cross-mob scenarios where both
/// IdentityRuntimes are available. It resolves the AgentRuntimeId for each identity
/// and delegates to `UnifiedRuntime::wire_cross_mob()`.
pub async fn wire_cross_mob_by_identity(
    local_irt: &IdentityRuntime,
    local_identity: &AgentIdentity,
    remote_irt: &IdentityRuntime,
    remote_identity: &AgentIdentity,
    local_unified: &crate::UnifiedRuntime,
    remote_mob_id: &str,
) -> Result<(), IdentityRuntimeError> {
    let local_rt = local_irt.runtime_id_for(local_identity).await?;
    let remote_rt = remote_irt.runtime_id_for(remote_identity).await?;
    Box::pin(local_unified.wire_cross_mob(local_rt.as_str(), remote_rt.as_str(), remote_mob_id))
        .await
        .map_err(|e| IdentityRuntimeError::Internal(format!("wire_cross_mob: {e}")))
}

#[cfg(test)]
mod reset_reprofile_tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::{Mutex as AsyncMutex, RwLock as AsyncRwLock};

    use super::super::bridge::{BridgeError, MemberInspection, ResumeSessionOutcome};
    use super::super::contracts::RosterProvider;
    use super::super::local_lease::LocalLeaseProvider;
    use super::super::local_store::LocalContinuityStore;
    use super::super::types::{AgentBuildDraft, RosterError, SessionSnapshot};

    struct MutableRoster {
        specs: AsyncRwLock<Vec<DurableAgentSpec>>,
    }

    impl MutableRoster {
        fn new(specs: Vec<DurableAgentSpec>) -> Self {
            Self {
                specs: AsyncRwLock::new(specs),
            }
        }

        async fn set(&self, specs: Vec<DurableAgentSpec>) {
            *self.specs.write().await = specs;
        }
    }

    #[async_trait::async_trait]
    impl RosterProvider for MutableRoster {
        async fn roster(
            &self,
            _context: &RosterContext,
        ) -> Result<Vec<DurableAgentSpec>, RosterError> {
            Ok(self.specs.read().await.clone())
        }
    }

    #[derive(Default)]
    struct RecordingBridge {
        create_profiles: AsyncMutex<Vec<String>>,
        retired_runtime_ids: AsyncMutex<Vec<String>>,
        hanging_retire_runtime_ids: AsyncMutex<BTreeSet<String>>,
        failing_unregister_session_ids: AsyncMutex<BTreeSet<String>>,
    }

    impl RecordingBridge {
        async fn create_profiles(&self) -> Vec<String> {
            self.create_profiles.lock().await.clone()
        }

        async fn retired_runtime_ids(&self) -> Vec<String> {
            self.retired_runtime_ids.lock().await.clone()
        }

        async fn hang_retire_for(&self, runtime_id: &AgentRuntimeId) {
            self.hanging_retire_runtime_ids
                .lock()
                .await
                .insert(runtime_id.to_string());
        }

        async fn fail_unregister_for(&self, session_id: &SessionId) {
            self.failing_unregister_session_ids
                .lock()
                .await
                .insert(session_id.to_string());
        }
    }

    #[async_trait::async_trait]
    impl SessionBridge for RecordingBridge {
        async fn create_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            session_id: &SessionId,
        ) -> Result<SessionId, BridgeError> {
            self.create_profiles
                .lock()
                .await
                .push(spec.profile.to_string());
            Ok(session_id.clone())
        }

        async fn resume_session(
            &self,
            _identity: &AgentIdentity,
            _runtime_id: &AgentRuntimeId,
            _spec: &DurableAgentSpec,
            _draft: &AgentBuildDraft,
            _session_id: &SessionId,
            _snapshot: &SessionSnapshot,
        ) -> Result<ResumeSessionOutcome, BridgeError> {
            Err(BridgeError::Mob(
                "resume not used in reset test".to_string(),
            ))
        }

        async fn deliver(
            &self,
            _runtime_id: &AgentRuntimeId,
            _content: &meerkat_core::ContentInput,
        ) -> Result<SessionId, BridgeError> {
            Err(BridgeError::Mob(
                "deliver not used in reset test".to_string(),
            ))
        }

        async fn checkpoint_session(
            &self,
            _runtime_id: &AgentRuntimeId,
            _session_id: &SessionId,
        ) -> Result<SessionSnapshot, BridgeError> {
            Err(BridgeError::Mob(
                "checkpoint not used in reset test".to_string(),
            ))
        }

        async fn retire_member(&self, runtime_id: &AgentRuntimeId) -> Result<(), BridgeError> {
            self.retired_runtime_ids
                .lock()
                .await
                .push(runtime_id.to_string());
            if self
                .hanging_retire_runtime_ids
                .lock()
                .await
                .contains(runtime_id.as_str())
            {
                futures::future::pending::<()>().await;
            }
            Ok(())
        }

        async fn inspect_member(
            &self,
            _runtime_id: &AgentRuntimeId,
        ) -> Result<MemberInspection, BridgeError> {
            Err(BridgeError::Mob(
                "inspect not used in reset test".to_string(),
            ))
        }

        async fn unregister_session_runtime_state(
            &self,
            session_id: &SessionId,
        ) -> Result<(), BridgeError> {
            if self
                .failing_unregister_session_ids
                .lock()
                .await
                .contains(&session_id.to_string())
            {
                return Err(BridgeError::Mob("old session still draining".to_string()));
            }
            Ok(())
        }
    }

    fn durable_spec(identity: AgentIdentity, profile: &str) -> DurableAgentSpec {
        DurableAgentSpec {
            identity,
            profile: meerkat_mob::ProfileName::from(profile),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: BTreeMap::new(),
            context: None,
            additional_instructions: Vec::new(),
            initial_message: None,
            runtime_mode_override: None,
            backend: None,
            binding: None,
        }
    }

    #[tokio::test]
    async fn reset_reprofiles_session_from_runtime_configured_roster_provider()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:security")?;
        let roster = Arc::new(MutableRoster::new(vec![durable_spec(
            identity.clone(),
            "domain",
        )]));
        let bridge = Arc::new(RecordingBridge::default());
        let runtime = Arc::new(
            IdentityRuntime::new(IdentityRuntimeConfig {
                continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
                lease_provider: Arc::new(LocalLeaseProvider::new()),
                runtime_instance_id: "reset-reprofile-test".to_string(),
                has_runtime_store: true,
                durability_policy: DurabilityPolicy::SyncWriteThrough,
                bridge: Some(bridge.clone()),
                default_timeout: None,
            })
            .with_reset_roster_provider(roster.clone()),
        );

        super::super::orchestrator::restore_flow(
            &runtime,
            &roster
                .roster(&RosterContext {
                    mob_definition: None,
                    previous_identities: Vec::new(),
                })
                .await?,
            None,
            None,
        )
        .await?;
        roster
            .set(vec![durable_spec(identity.clone(), "security")])
            .await;

        let record = runtime.reset(&identity).await?;

        assert_eq!(record.generation.get(), 1);
        assert_eq!(
            bridge.create_profiles().await,
            vec!["domain".to_string(), "security".to_string()]
        );
        let status = runtime.status(&identity).await?;
        assert_eq!(
            status.profile.map(|profile| profile.to_string()).as_deref(),
            Some("security")
        );
        Ok(())
    }

    #[tokio::test]
    async fn reset_reprofiles_session_from_identity_first_context_roster_provider()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:security")?;
        let roster = Arc::new(MutableRoster::new(vec![durable_spec(
            identity.clone(),
            "domain",
        )]));
        let bridge = Arc::new(RecordingBridge::default());
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "reset-reprofile-context-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(bridge.clone()),
            default_timeout: None,
        }));
        let context =
            IdentityFirstRuntimeContext::new(runtime.clone(), roster.clone(), None, None, None);

        context.refresh_desired_topology().await?;
        roster
            .set(vec![durable_spec(identity.clone(), "security")])
            .await;

        let record = runtime.reset(&identity).await?;

        assert_eq!(record.generation.get(), 1);
        assert_eq!(
            bridge.create_profiles().await,
            vec!["domain".to_string(), "security".to_string()]
        );
        let status = runtime.status(&identity).await?;
        assert_eq!(
            status.profile.map(|profile| profile.to_string()).as_deref(),
            Some("security")
        );
        Ok(())
    }

    #[tokio::test]
    async fn reset_does_not_retire_old_generation_during_cleanup()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:security")?;
        let roster = Arc::new(MutableRoster::new(vec![durable_spec(
            identity.clone(),
            "domain",
        )]));
        let bridge = Arc::new(RecordingBridge::default());
        let runtime = Arc::new(
            IdentityRuntime::new(IdentityRuntimeConfig {
                continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
                lease_provider: Arc::new(LocalLeaseProvider::new()),
                runtime_instance_id: "reset-skips-old-retire-test".to_string(),
                has_runtime_store: true,
                durability_policy: DurabilityPolicy::SyncWriteThrough,
                bridge: Some(bridge.clone()),
                default_timeout: Some(Duration::from_millis(50)),
            })
            .with_reset_roster_provider(roster.clone()),
        );

        super::super::orchestrator::restore_flow(
            &runtime,
            &roster
                .roster(&RosterContext {
                    mob_definition: None,
                    previous_identities: Vec::new(),
                })
                .await?,
            None,
            None,
        )
        .await?;

        let old_runtime_id = AgentRuntimeId::parse("rt:domain:security:0")?;
        bridge.hang_retire_for(&old_runtime_id).await;
        roster
            .set(vec![durable_spec(identity.clone(), "security")])
            .await;

        let record = tokio::time::timeout(Duration::from_secs(1), runtime.reset(&identity))
            .await
            .map_err(|_| "reset timed out waiting for old generation retirement")??;

        assert_eq!(record.generation.get(), 1);
        assert_eq!(
            bridge.create_profiles().await,
            vec!["domain".to_string(), "security".to_string()]
        );
        assert!(
            !bridge
                .retired_runtime_ids()
                .await
                .contains(&old_runtime_id.to_string()),
            "reset cleanup must not call the cancellation-unsafe mob-member retire path"
        );
        let status = runtime.status(&identity).await?;
        assert_eq!(
            status.profile.map(|profile| profile.to_string()).as_deref(),
            Some("security")
        );
        Ok(())
    }

    #[tokio::test]
    async fn reset_returns_when_old_session_unregister_fails_after_new_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = AgentIdentity::parse("domain:security")?;
        let roster = Arc::new(MutableRoster::new(vec![durable_spec(
            identity.clone(),
            "domain",
        )]));
        let bridge = Arc::new(RecordingBridge::default());
        let runtime = Arc::new(
            IdentityRuntime::new(IdentityRuntimeConfig {
                continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
                lease_provider: Arc::new(LocalLeaseProvider::new()),
                runtime_instance_id: "reset-unregister-failure-test".to_string(),
                has_runtime_store: true,
                durability_policy: DurabilityPolicy::SyncWriteThrough,
                bridge: Some(bridge.clone()),
                default_timeout: Some(Duration::from_millis(50)),
            })
            .with_reset_roster_provider(roster.clone()),
        );

        super::super::orchestrator::restore_flow(
            &runtime,
            &roster
                .roster(&RosterContext {
                    mob_definition: None,
                    previous_identities: Vec::new(),
                })
                .await?,
            None,
            None,
        )
        .await?;

        let old_status = runtime.status(&identity).await?;
        let Some(old_session_id) = old_status.session_id else {
            return Err("initial session id missing".into());
        };
        bridge.fail_unregister_for(&old_session_id).await;
        roster
            .set(vec![durable_spec(identity.clone(), "security")])
            .await;

        let record = tokio::time::timeout(Duration::from_secs(1), runtime.reset(&identity))
            .await
            .map_err(|_| "reset timed out waiting for old session unregister cleanup")??;

        assert_eq!(record.generation.get(), 1);
        assert_eq!(
            bridge.create_profiles().await,
            vec!["domain".to_string(), "security".to_string()]
        );
        let status = runtime.status(&identity).await?;
        assert_eq!(
            status.profile.map(|profile| profile.to_string()).as_deref(),
            Some("security")
        );
        Ok(())
    }
}

#[cfg(test)]
mod lease_renewal_backoff_tests {
    use super::*;

    /// Regression: a lease provider that errors persistently must not spin the
    /// renewal task at the TTL-derived floor (down to 10ms). The failure
    /// backoff grows from a 1s base and caps at the max poll interval.
    #[test]
    fn lease_renewal_failure_backoff_grows_and_caps() {
        let max = Duration::from_mins(1);
        assert_eq!(
            lease_renewal_failure_backoff(0, max),
            LEASE_RENEWAL_FAILURE_BACKOFF_BASE
        );
        assert_eq!(
            lease_renewal_failure_backoff(1, max),
            LEASE_RENEWAL_FAILURE_BACKOFF_BASE * 2
        );
        assert_eq!(lease_renewal_failure_backoff(6, max), max);
        // Saturates at the cap for arbitrarily many failures (no shift overflow).
        assert_eq!(lease_renewal_failure_backoff(99, max), max);
        assert!(lease_renewal_failure_backoff(2, max) > lease_renewal_failure_backoff(1, max));
    }
}
