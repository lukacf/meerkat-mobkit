//! Identity-first runtime: delivery, status, lifecycle, and ownership enforcement.
//!
//! This module implements the behavioral core of identity-first continuity:
//! - Delivery: `send()` and `dispatch()` with addressability and lease enforcement
//! - Status: `status()` returning `IdentityStatus`
//! - Lifecycle: `retire()`, `respawn()`, `reset()`, `delete_identity()`
//! - Ownership: lease tracking, fencing, and invariant enforcement

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};
use meerkat_core::types::HandlingMode;
use tokio::sync::{Mutex, RwLock, broadcast};

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

const MANAGED_PEER_RECONCILE_CONCURRENCY: usize = 64;

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
    runtime_services: AgentRuntimeServices,
    managed_peer_edges: RwLock<BTreeSet<(AgentIdentity, AgentIdentity)>>,
    managed_peer_reconcile_lock: Mutex<()>,
    desired_peer_edges: RwLock<Vec<ManagedPeerEdge>>,
    materialization_locks: RwLock<BTreeMap<AgentIdentity, Arc<Mutex<()>>>>,
    customizer: RwLock<Option<Arc<dyn AgentCustomizer>>>,
    default_timeout: Duration,
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
            runtime_services: AgentRuntimeServices::empty(),
            managed_peer_edges: RwLock::new(BTreeSet::new()),
            managed_peer_reconcile_lock: Mutex::new(()),
            desired_peer_edges: RwLock::new(Vec::new()),
            materialization_locks: RwLock::new(BTreeMap::new()),
            customizer: RwLock::new(None),
            default_timeout: config.default_timeout.unwrap_or(Duration::from_secs(90)),
        }
    }

    pub fn with_runtime_services(mut self, runtime_services: AgentRuntimeServices) -> Self {
        self.runtime_services = runtime_services;
        self
    }

    pub(crate) fn runtime_services(&self) -> AgentRuntimeServices {
        self.runtime_services.clone()
    }

    pub async fn set_agent_customizer(&self, customizer: Option<Arc<dyn AgentCustomizer>>) {
        *self.customizer.write().await = customizer;
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
        let retained_logical_edges: Vec<(AgentIdentity, AgentIdentity)> = desired
            .iter()
            .filter(|edge| !managed_snapshot.contains(*edge))
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
            .filter(|edge| !managed_snapshot.contains(*edge))
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
            if !active_runtimes.contains_key(&a) || !active_runtimes.contains_key(&b) {
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
        self.entries.write().await.insert(identity.clone(), entry);

        // Create event channel for this identity
        let (tx, _) = broadcast::channel(IDENTITY_EVENT_CHANNEL_CAPACITY);
        self.event_channels.write().await.insert(identity, tx);
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
        let lock = self.materialization_lock_for(identity).await;
        let _guard = lock.lock().await;

        let (spec, continuity, state) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            if entry.state == IdentityLifecycleState::Active {
                return entry.continuity.clone().ok_or_else(|| {
                    IdentityRuntimeError::Internal(format!(
                        "active identity {identity} has no continuity record"
                    ))
                });
            }
            (entry.spec.clone(), entry.continuity.clone(), entry.state)
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
            _ => return Err(IdentityRuntimeError::NoActiveLease(identity.clone())),
        };

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
        if let Some(customizer) = self.customizer.read().await.clone() {
            customizer
                .customize_build(&build_context, &spec, &mut draft)
                .await
                .map_err(|err| IdentityRuntimeError::Internal(format!("customizer: {err}")))?;
        }

        let mut record = if let Some(mut record) = continuity {
            let snapshot = self
                .continuity_store
                .load_session_snapshot(&record.session_id)
                .await
                .map_err(IdentityRuntimeError::Store)?;

            if let Some(bridge) = self.bridge.as_ref() {
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
                            "bridge register_session_runtime_state: {err}"
                        ))
                    })?;
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
                    .await
                    .map_err(|err| {
                        IdentityRuntimeError::Internal(format!("bridge resume_session: {err}"))
                    })?;
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
                record.session_id = outcome.session_id().clone();
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
            self.continuity_store
                .upsert_continuity_record(&record, grant.fencing_token)
                .await?;
            if let Some(bridge) = self.bridge.as_ref() {
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
                            "bridge register_session_runtime_state: {err}"
                        ))
                    })?;
                record.session_id = bridge
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
                    })?;
            }
            record
        };

        self.continuity_store
            .upsert_continuity_record(&record, grant.fencing_token)
            .await?;
        if let Some(bridge) = self.bridge.as_ref() {
            let effective_checkpoint_version = bridge
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
                        "bridge register actual session runtime state: {err}"
                    ))
                })?;
            record.checkpoint_version = effective_checkpoint_version;
        }

        {
            let mut entries = self.entries.write().await;
            let entry = entries
                .get_mut(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;
            entry.continuity = Some(record.clone());
            entry.lease = Some(LeaseEntry {
                fencing_token: grant.fencing_token,
                ttl: grant.ttl,
                acquired_at: Instant::now(),
            });
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
        if !desired_edges.is_empty() {
            self.reconcile_managed_peer_edges(&desired_edges).await?;
        }
        Ok(record)
    }

    /// Materialize all identities currently registered with the runtime.
    ///
    /// Lazy bootstrap keeps `build()` metadata-only, but flow execution still
    /// enters the mob runtime through concrete member IDs. This helper gives
    /// flow entrypoints a single choke point to hydrate the concrete mob graph
    /// before calling `run_flow`.
    pub async fn materialize_all(&self) -> Result<Vec<ContinuityRecord>, IdentityRuntimeError> {
        let identities = self.registered_identities().await;
        let results = stream::iter(
            identities
                .into_iter()
                .map(|identity| async move { self.materialize(&identity).await }),
        )
        .buffer_unordered(MANAGED_PEER_RECONCILE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

        let mut records = Vec::with_capacity(results.len());
        for result in results {
            records.push(result?);
        }

        let desired_edges = self.desired_peer_edges.read().await.clone();
        if !desired_edges.is_empty() {
            self.reconcile_managed_peer_edges(&desired_edges).await?;
        }

        Ok(records)
    }

    /// Ensure an active identity's desired peer neighborhood exists in the
    /// concrete mob graph before ordinary communication starts.
    pub async fn materialize_reachable_peers(
        &self,
        identity: &AgentIdentity,
    ) -> Result<Vec<ContinuityRecord>, IdentityRuntimeError> {
        let peers = self.reachable_peer_identities(identity).await;
        let results = stream::iter(
            peers
                .into_iter()
                .map(|peer| async move { self.materialize(&peer).await }),
        )
        .buffer_unordered(MANAGED_PEER_RECONCILE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

        let mut records = Vec::with_capacity(results.len());
        for result in results {
            records.push(result?);
        }

        let desired_edges = self.desired_peer_edges.read().await.clone();
        if !desired_edges.is_empty() {
            self.reconcile_managed_peer_edges(&desired_edges).await?;
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

        let (token, runtime_id) = {
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

            // INV-01 / INV-02: require active lease
            let token = Self::check_lease(entry)?;

            let runtime_id = entry
                .continuity
                .as_ref()
                .map(|c| c.agent_runtime_id.clone());

            (token, runtime_id)
        };

        // Deliver through the session bridge when available.
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id) {
            bridge
                .deliver_with_mode(rid, content, handling_mode)
                .await
                .map_err(|e| IdentityRuntimeError::Internal(format!("bridge deliver: {e}")))?;
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

        let (token, is_durable, runtime_id) = {
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

            // INV-01 / INV-02: require active lease
            let token = Self::check_lease(entry)?;

            // REQ-04: durability depends on runtime_store
            let is_durable = entry.has_runtime_store;

            let runtime_id = entry
                .continuity
                .as_ref()
                .map(|c| c.agent_runtime_id.clone());

            (token, is_durable, runtime_id)
        };

        // Deliver through the session bridge when available.
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id) {
            bridge
                .deliver(rid, &input.content)
                .await
                .map_err(|e| IdentityRuntimeError::Internal(format!("bridge dispatch: {e}")))?;
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
        let (token, runtime_id) = {
            let mut entries = self.entries.write().await;
            let entry = entries
                .get_mut(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;

            let token = Self::check_lease(entry)?;
            entry.state = IdentityLifecycleState::Retiring;

            let runtime_id = entry
                .continuity
                .as_ref()
                .map(|c| c.agent_runtime_id.clone());

            (token, runtime_id)
        };

        // Retire the mob member through the session bridge when available.
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id) {
            bridge
                .retire_member(rid)
                .await
                .map_err(|e| IdentityRuntimeError::Internal(format!("bridge retire: {e}")))?;
        }

        Ok(token)
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
        // Fence the old owner by re-acquiring the lease
        let acquire_result = self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
            .map_err(IdentityRuntimeError::Lease)?;

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };

        // Resolve current continuity state
        let resolved = self
            .continuity_store
            .resolve_many(std::slice::from_ref(identity))
            .await
            .map_err(IdentityRuntimeError::Store)?;

        let record = match resolved.get(identity) {
            Some(super::types::ContinuityResolveState::Ready { record }) => record.clone(),
            Some(super::types::ContinuityResolveState::Broken { failure }) => {
                return Err(IdentityRuntimeError::Internal(format!(
                    "broken continuity for {identity}: {}",
                    failure.detail
                )));
            }
            Some(super::types::ContinuityResolveState::Uninitialized) => {
                return Err(IdentityRuntimeError::Internal(format!(
                    "cannot respawn uninitialized identity {identity}"
                )));
            }
            None => {
                return Err(IdentityRuntimeError::Store(
                    ContinuityStoreError::NotFound {
                        identity: identity.clone(),
                    },
                ));
            }
        };

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
        // INV-05: fence the old owner first
        let acquire_result = self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
            .map_err(IdentityRuntimeError::Lease)?;

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };

        // Resolve to get current generation
        let resolved = self
            .continuity_store
            .resolve_many(std::slice::from_ref(identity))
            .await
            .map_err(IdentityRuntimeError::Store)?;

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
        let new_runtime_id = AgentRuntimeId::parse(&format!("{}:gen{}", identity, new_gen.get()))
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

        // Persist the new record (fencing token from new lease protects against old writes)
        self.continuity_store
            .upsert_continuity_record(&new_record, grant.fencing_token)
            .await?;

        // Bridge: retire old mob member and create fresh session for the new identity.
        if let Some(bridge) = &self.bridge {
            // Retire old member (best-effort — may already be gone)
            let old_runtime_id = {
                let entries = self.entries.read().await;
                entries
                    .get(identity)
                    .and_then(|e| e.continuity.as_ref().map(|c| c.agent_runtime_id.clone()))
            };
            if let Some(old_id) = old_runtime_id {
                let _ = bridge.retire_member(&old_id).await;
            }

            // Get the spec from the runtime entry
            let spec = {
                let entries = self.entries.read().await;
                entries.get(identity).map(|e| e.spec.clone())
            };
            if let Some(spec) = spec {
                let draft = super::types::AgentBuildDraft {
                    model: None,
                    system_prompt: None,
                    additional_instructions: spec.additional_instructions.clone(),
                    labels: spec.labels.clone(),
                    app_context: spec.context.clone(),
                    external_tools: Vec::new(),
                    local_external_tools: Default::default(),
                };
                bridge
                    .register_session_runtime_state(
                        &new_record.session_id,
                        identity,
                        new_record.generation,
                        new_record.checkpoint_version,
                        grant.fencing_token,
                    )
                    .await
                    .map_err(|e| {
                        IdentityRuntimeError::Internal(format!(
                            "bridge register_session_runtime_state after reset: {e}"
                        ))
                    })?;
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
                    })?;
                // Update the record with the actual session ID
                let initial_session_id = new_record.session_id.clone();
                let mut new_record = new_record;
                new_record.session_id = session_id;

                // Persist the updated record
                if new_record.session_id != initial_session_id {
                    self.continuity_store
                        .upsert_continuity_record(&new_record, grant.fencing_token)
                        .await?;
                }
                let effective_checkpoint_version = bridge
                    .register_session_runtime_state(
                        &new_record.session_id,
                        identity,
                        new_record.generation,
                        new_record.checkpoint_version,
                        grant.fencing_token,
                    )
                    .await
                    .map_err(|e| {
                        IdentityRuntimeError::Internal(format!(
                            "bridge register actual session runtime state after reset: {e}"
                        ))
                    })?;
                new_record.checkpoint_version = effective_checkpoint_version;

                // Update runtime state
                let mut entries = self.entries.write().await;
                if let Some(entry) = entries.get_mut(identity) {
                    entry.continuity = Some(new_record.clone());
                    entry.lease = Some(LeaseEntry {
                        fencing_token: grant.fencing_token,
                        ttl: grant.ttl,
                        acquired_at: Instant::now(),
                    });
                    entry.state = IdentityLifecycleState::Active;
                    entry.checkpoint_version = new_record.checkpoint_version;
                }
                return Ok(new_record);
            }
        }

        // No bridge — update runtime state only (validation mode)
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(identity) {
            entry.continuity = Some(new_record.clone());
            entry.lease = Some(LeaseEntry {
                fencing_token: grant.fencing_token,
                ttl: grant.ttl,
                acquired_at: Instant::now(),
            });
            entry.state = IdentityLifecycleState::Active;
            entry.checkpoint_version = CheckpointVersion::new(0);
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
        // INV-05: fence the old owner first
        let acquire_result = self
            .lease_provider
            .acquire_leases(std::slice::from_ref(identity), &self.runtime_instance_id)
            .await
            .map_err(IdentityRuntimeError::Lease)?;

        let grant = match acquire_result.get(identity) {
            Some(super::types::LeaseAcquireResult::Acquired(g)) => g.clone(),
            _ => {
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        };

        // Retire the mob member through the session bridge before removing
        // the continuity record. This ensures the mob actor is cleaned up.
        let runtime_id = {
            let entries = self.entries.read().await;
            entries
                .get(identity)
                .and_then(|e| e.continuity.as_ref().map(|c| c.agent_runtime_id.clone()))
        };
        if let (Some(bridge), Some(rid)) = (&self.bridge, &runtime_id) {
            // Best-effort: if the member is already gone, ignore the error.
            let _ = bridge.retire_member(rid).await;
        }

        // Remove authoritative continuity record from the store
        self.continuity_store
            .delete_continuity_record(identity, grant.fencing_token)
            .await?;

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
        let (record, token, new_version) = {
            let entries = self.entries.read().await;
            let entry = entries
                .get(identity)
                .ok_or_else(|| IdentityRuntimeError::UnknownIdentity(identity.clone()))?;

            // INV-01: require active lease
            let token = Self::check_lease(entry)?;

            let record = entry
                .continuity
                .as_ref()
                .ok_or_else(|| {
                    IdentityRuntimeError::Internal(format!("no continuity record for {identity}"))
                })?
                .clone();

            let new_version = CheckpointVersion::new(entry.checkpoint_version.get() + 1);
            (record, token, new_version)
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
    local_unified
        .wire_cross_mob(local_rt.as_str(), remote_rt.as_str(), remote_mob_id)
        .await
        .map_err(|e| IdentityRuntimeError::Internal(format!("wire_cross_mob: {e}")))
}
